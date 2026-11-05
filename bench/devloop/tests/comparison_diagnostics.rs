//! Exercise the public comparison command, including its exit status.
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "devloop-comparison-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create independent comparison fixture");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
enum FixtureMetric {
    Measured { value: f64, unit: &'static str },
    NotRun { reason: &'static str },
}

#[derive(Serialize)]
struct FixtureHost {
    os: &'static str,
    arch: &'static str,
    cpu: &'static str,
    rustc: &'static str,
    perf: bool,
    strace: bool,
    valgrind: bool,
}

#[derive(Serialize)]
struct FixtureCell {
    id: &'static str,
    kind: &'static str,
    codec: &'static str,
    iters: u64,
    repeats: u32,
    instructions: FixtureMetric,
    allocs: FixtureMetric,
    alloc_bytes: FixtureMetric,
    syscalls: FixtureMetric,
    locks: FixtureMetric,
    wall_ns: FixtureMetric,
    wall_cv: Option<f64>,
}

#[derive(Serialize)]
struct FixtureReport {
    schema: &'static str,
    devloop_commit: &'static str,
    host: FixtureHost,
    cells: Vec<FixtureCell>,
}

fn report(locks: Option<f64>) -> String {
    let missing = FixtureMetric::NotRun { reason: "control" };
    let locks = match locks {
        Some(value) => FixtureMetric::Measured {
            value,
            unit: "futex-family syscalls per op (includes wakes)",
        },
        None => missing.clone(),
    };
    serde_json::to_string(&FixtureReport {
        schema: "devloop/1",
        devloop_commit: "control",
        host: FixtureHost {
            os: "control",
            arch: "control",
            cpu: "control",
            rustc: "control",
            perf: false,
            strace: false,
            valgrind: false,
        },
        cells: vec![FixtureCell {
            id: "lb.pick_first.pick",
            kind: "lb",
            codec: "pbrs-grpc",
            iters: 1_000_000,
            repeats: 3,
            instructions: missing.clone(),
            allocs: missing.clone(),
            alloc_bytes: missing.clone(),
            syscalls: missing.clone(),
            locks,
            wall_ns: missing,
            wall_cv: None,
        }],
    })
    .unwrap()
}

fn compare(old: Option<f64>, new: Option<f64>, ceiling: Option<f64>) -> Output {
    compare_json(&report(old), &report(new), ceiling)
}

#[expect(
    clippy::disallowed_methods,
    reason = "synchronous CLI fixture setup runs without an async executor"
)]
fn compare_json(base_json: &str, current_json: &str, ceiling: Option<f64>) -> Output {
    let temp = TempDir::new();
    let base_path = temp.0.join("base.json");
    let current_path = temp.0.join("current.json");
    std::fs::write(&base_path, base_json).unwrap();
    std::fs::write(&current_path, current_json).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_devloop"));
    command
        .arg("compare")
        .arg("--baseline")
        .arg(base_path)
        .arg("--current")
        .arg(current_path);
    if let Some(limit) = ceiling {
        let path = temp.0.join("budget.json");
        std::fs::write(
            &path,
            json!({"schema": "devloop-budget/1",
                   "cells": {"lb.pick_first.pick": {"locks": limit}}})
            .to_string(),
        )
        .unwrap();
        command.arg("--budget").arg(path);
    }
    command.output().expect("run devloop comparison")
}

fn displayed_pair(output: &Output, marker: &str, separator: &str) -> (f64, f64) {
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let line = stdout
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| {
            panic!(
                "missing {marker:?}; stdout={stdout:?}, stderr={:?}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    let tail = line.split_once(marker).unwrap().1;
    let (left, right) = tail.split_once(separator).unwrap();
    (
        left.parse().unwrap(),
        right.split_whitespace().next().unwrap().parse().unwrap(),
    )
}

#[test]
fn budget_reports_round_trip_and_keeps_threshold_behavior() {
    for (value, limit) in [
        (0.000_001, 0.0),
        (f64::from_bits(1), 0.0),
        (1.0 + f64::EPSILON, 1.0),
    ] {
        let output = compare(Some(value), Some(value), Some(limit));
        assert_eq!(
            output.status.code(),
            Some(1),
            "stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let (actual_value, actual_limit) = displayed_pair(&output, "BUDGET EXCEEDED ", " > ");
        assert_eq!(actual_value.to_bits(), value.to_bits());
        assert_eq!(actual_limit.to_bits(), limit.to_bits());
        assert!(actual_value > actual_limit);
    }
    for (value, limit) in [
        (Some(0.000_001), 0.000_001),
        (Some(0.000_001), 0.000_002),
        (None, 0.0),
    ] {
        let output = compare(value, value, Some(limit));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("BUDGET EXCEEDED"));
    }
}

#[test]
fn relative_small_rates_are_reported_without_rounding_to_zero() {
    let old = 1.0e-8;
    for (new, marker, succeeds) in [(1.04e-8, "REGRESSED ", false), (0.96e-8, "improved ", true)] {
        let output = compare(Some(old), Some(new), None);
        assert_eq!(output.status.success(), succeeds);
        let (actual_old, actual_new) = displayed_pair(&output, marker, " -> ");
        assert_eq!(actual_old.to_bits(), old.to_bits());
        assert_eq!(actual_new.to_bits(), new.to_bits());
    }
}

#[test]
fn valid_metric_objects_can_put_data_before_status() {
    for value in [0.0, 0.000_001, f64::from_bits(1), 1.0 + f64::EPSILON] {
        let original = report(Some(value));
        let data_first: serde_json::Value = serde_json::from_str(&original).unwrap();
        let data_first = data_first.to_string();
        assert!(data_first.contains("\"data\":{\"unit\":"));
        for (base, current) in [(&original, &data_first), (&data_first, &original)] {
            let output = compare_json(base, current, Some(value));
            assert!(
                output.status.success(),
                "value={value:e}; stdout={}, stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

fn report_with_raw_lock(metric: &str) -> String {
    let report = report(Some(1234.5));
    let original = "{\"status\":\"measured\",\"data\":{\"value\":1234.5,\"unit\":\"futex-family syscalls per op (includes wakes)\"}}";
    assert_eq!(report.matches(original).count(), 1);
    report.replace(original, metric)
}

#[test]
fn invalid_metric_objects_remain_rejected() {
    for metric in [
        r#"{}"#,
        r#"{"status":"measured"}"#,
        r#"{"data":{"value":1,"unit":"calls"}}"#,
        r#"{"status":"measured","data":null}"#,
        r#"{"status":"measured","data":{"unit":"calls"}}"#,
        r#"{"status":"measured","data":{"value":1}}"#,
        r#"{"status":"measured","data":{"value":"1","unit":"calls"}}"#,
        r#"{"status":"measured","data":{"value":null,"unit":"calls"}}"#,
        r#"{"status":"measured","data":{"value":1,"value":2,"unit":"calls"}}"#,
        r#"{"status":"measured","data":{"value":1,"unit":"calls","unit":"other"}}"#,
        r#"{"data":{"value":1,"unit":"calls","value":2},"status":"measured"}"#,
        r#"{"status":"measured","status":"measured","data":{"value":1,"unit":"calls"}}"#,
        r#"{"status":"measured","data":{"value":1,"unit":"calls"},"data":{"value":2,"unit":"calls"}}"#,
        r#"{"status":"not_run","data":{"reason":12}}"#,
        r#"{"status":"not_run","data":{"reason":"a","reason":"b"}}"#,
        r#"{"status":"unknown","data":{"reason":"x"}}"#,
    ] {
        let output = compare_json(&report_with_raw_lock(metric), &report(None), None);
        assert!(!output.status.success(), "accepted invalid metric {metric}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("parse baseline"));
    }
}

#[test]
fn unknown_metric_fields_remain_readable() {
    for metric in [
        r#"{"status":"measured","data":{"value":1.25,"unit":"calls","reason":42,"reason":false}}"#,
        r#"{"status":"not_run","data":{"reason":"x","value":"other","value":false}}"#,
        r#"{"unused":null,"status":"measured","data":{"value":1,"unit":"calls"}}"#,
    ] {
        let report = report_with_raw_lock(metric);
        let output = compare_json(&report, &report, None);
        assert!(
            output.status.success(),
            "rejected extension {metric}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
