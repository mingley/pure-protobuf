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

#[test]
fn duplicate_cell_ids_fail_before_any_ratios_are_printed() {
    let base = report(Some(100.0));
    let current = report(Some(1.0));
    for side in ["baseline", "current"] {
        let original = if side == "baseline" { &base } else { &current };
        let mut duplicate: serde_json::Value = serde_json::from_str(original).unwrap();
        let second = duplicate["cells"][0].clone();
        duplicate["cells"].as_array_mut().unwrap().push(second);
        let duplicate = duplicate.to_string();
        let (old, new) = if side == "baseline" {
            (&duplicate, &current)
        } else {
            (&base, &duplicate)
        };
        let output = compare_json(old, new, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{side}: {stdout}");
        assert!(
            stdout.contains(&format!("INCOMPARABLE duplicate {side} cell ID")),
            "{stdout}"
        );
        assert!(
            !stdout.contains("improved") && !stdout.contains("REGRESSED"),
            "{stdout}"
        );
    }
}

#[test]
fn empty_or_disjoint_reports_fail_without_claiming_a_comparison() {
    let valid = report(Some(1.0));
    let mut empty: serde_json::Value = serde_json::from_str(&valid).unwrap();
    empty["cells"] = json!([]);
    let empty = empty.to_string();
    let mut disjoint: serde_json::Value = serde_json::from_str(&valid).unwrap();
    disjoint["cells"][0]["id"] = json!("lb.ring_hash.pick");
    let disjoint = disjoint.to_string();
    for (old, new) in [
        (&empty, &empty),
        (&empty, &valid),
        (&valid, &empty),
        (&valid, &disjoint),
        (&disjoint, &valid),
    ] {
        let output = compare_json(old, new, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{stdout}");
        assert!(stdout.contains("INCOMPARABLE: no shared cells"), "{stdout}");
        assert!(
            !stdout.contains("improved") && !stdout.contains("REGRESSED"),
            "{stdout}"
        );
    }
}

#[test]
fn empty_cell_identity_fields_are_rejected() {
    let valid = report(Some(1.0));
    for field in ["id", "kind", "codec"] {
        for value in ["", "   "] {
            let mut invalid: serde_json::Value = serde_json::from_str(&valid).unwrap();
            invalid["cells"][0][field] = json!(value);
            let invalid = invalid.to_string();
            for (old, new) in [
                (&invalid, &invalid),
                (&invalid, &valid),
                (&valid, &invalid),
            ] {
                let output = compare_json(old, new, None);
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert_eq!(output.status.code(), Some(1), "{field}: {stdout}");
                assert!(
                    stdout.contains(&format!("invalid baseline {field}"))
                        || stdout.contains(&format!("invalid current {field}")),
                    "{stdout}"
                );
                assert!(
                    !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                    "{stdout}"
                );
            }
        }
    }
}

#[test]
fn zero_sample_counts_fail_even_when_both_reports_match() {
    let valid = report(Some(1.0));
    for field in ["iters", "repeats"] {
        let mut invalid: serde_json::Value = serde_json::from_str(&valid).unwrap();
        invalid["cells"][0][field] = json!(0);
        let invalid = invalid.to_string();
        for (old, new) in [
            (&invalid, &invalid),
            (&invalid, &valid),
            (&valid, &invalid),
        ] {
            let output = compare_json(old, new, None);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(1), "{field}: {stdout}");
            assert!(stdout.contains("INCOMPARABLE invalid"), "{stdout}");
            assert!(stdout.contains("sample counts"), "{stdout}");
            assert!(
                !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                "{stdout}"
            );
        }
    }
}

#[test]
fn changed_cell_metadata_fails_without_comparing_values() {
    let base = report(Some(100.0));
    for (field, different) in [
        ("kind", json!("rpc")),
        ("codec", json!("other")),
        ("iters", json!(123)),
        ("repeats", json!(2)),
    ] {
        let mut changed: serde_json::Value = serde_json::from_str(&report(Some(1.0))).unwrap();
        changed["cells"][0][field] = different;
        let changed = changed.to_string();
        for (old, current) in [(&base, &changed), (&changed, &base)] {
            let output = compare_json(old, current, None);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(1), "{field}: {stdout}");
            assert!(
                stdout.contains(&format!("INCOMPARABLE {field}")),
                "{stdout}"
            );
            assert!(
                !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                "{stdout}"
            );
        }
    }
}

#[test]
fn local_comparison_does_not_require_matching_hosts() {
    let base = report(Some(1.0));
    let mut other_host: serde_json::Value = serde_json::from_str(&base).unwrap();
    other_host["host"]["cpu"] = json!("other CPU");
    let output = compare_json(&base, &other_host.to_string(), None);
    assert!(output.status.success());
}

fn measured_report(metric: &str, value: f64, unit: &str, method: Option<&str>) -> String {
    let mut result: serde_json::Value = serde_json::from_str(&report(None)).unwrap();
    result["cells"][0][metric] =
        json!({"status": "measured", "data": {"value": value, "unit": unit}});
    if let Some(method) = method {
        result["cells"][0]["instruction_method"] = json!(method);
    }
    result.to_string()
}

#[test]
fn bridge_fingerprints_are_checked_without_the_bridge_feature() {
    fn bridge_report(fingerprint: Option<&str>) -> String {
        let mut result: serde_json::Value = serde_json::from_str(&report(Some(1.0))).unwrap();
        result["cells"][0]["id"] =
            json!("codec.adoption.bridge.prost_to_pbrs.query.d3.v0.read_all");
        if let Some(fingerprint) = fingerprint {
            result["cells"][0]["input_wire_fingerprint"] = json!(fingerprint);
        }
        result.to_string()
    }

    for (first, second) in [
        (None, None),
        (Some(""), Some("")),
        (Some(" "), Some(" ")),
        (Some("\t\n"), Some("source")),
        (Some("source"), Some("\t\n")),
        (Some("source"), None),
        (None, Some("source")),
        (Some("source"), Some("different")),
    ] {
        let output = compare_json(&bridge_report(first), &bridge_report(second), None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{stdout}");
        assert!(
            stdout.contains("INCOMPARABLE") && stdout.contains("input fingerprint"),
            "{stdout}"
        );
        assert!(
            !stdout.contains("improved") && !stdout.contains("REGRESSED"),
            "{stdout}"
        );
    }
    let mut null: serde_json::Value = serde_json::from_str(&bridge_report(None)).unwrap();
    null["cells"][0]["input_wire_fingerprint"] = serde_json::Value::Null;
    assert!(
        !compare_json(&null.to_string(), &bridge_report(None), None)
            .status
            .success()
    );
    let valid = bridge_report(Some("source"));
    assert!(compare_json(&valid, &valid, None).status.success());
}

#[test]
fn ordinary_adoption_rejects_present_blank_fingerprints_but_keeps_legacy_pairs() {
    fn adoption_report(id: &str, fingerprint: Option<&str>) -> String {
        let mut result: serde_json::Value = serde_json::from_str(&report(Some(1.0))).unwrap();
        result["cells"][0]["id"] = json!(id);
        if let Some(fingerprint) = fingerprint {
            result["cells"][0]["input_wire_fingerprint"] = json!(fingerprint);
        }
        result.to_string()
    }

    for id in [
        "codec.adoption.pbrs.maps.n8.read_all",
        "rpc.adoption.native_pbrs.maps.n8.unary",
        "rpc.tonic_transport.native_native.maps.n8.unary",
    ] {
        let legacy = adoption_report(id, None);
        let valid = adoption_report(id, Some("wire"));
        assert!(compare_json(&legacy, &legacy, None).status.success());
        assert!(compare_json(&valid, &valid, None).status.success());
        for fingerprint in ["", " ", "\t\n"] {
            let blank = adoption_report(id, Some(fingerprint));
            for (old, new) in [
                (&blank, &blank),
                (&blank, &valid),
                (&valid, &blank),
                (&blank, &legacy),
                (&legacy, &blank),
            ] {
                let output = compare_json(old, new, None);
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert_eq!(output.status.code(), Some(1), "{id}: {stdout}");
                assert!(
                    stdout.contains("INCOMPARABLE blank")
                        && stdout.contains("input fingerprint"),
                    "{stdout}"
                );
                assert!(
                    !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                    "{stdout}"
                );
            }
        }
    }
    // A blank fingerprint is invalid even on a newly added or removed cell.
    let common = report(Some(1.0));
    let mut with_extra: serde_json::Value = serde_json::from_str(&common).unwrap();
    let extra: serde_json::Value = serde_json::from_str(&adoption_report(
        "codec.adoption.pbrs.maps.n8.read_all",
        Some(" "),
    ))
    .unwrap();
    with_extra["cells"]
        .as_array_mut()
        .unwrap()
        .push(extra["cells"][0].clone());
    let with_extra = with_extra.to_string();
    for (old, new) in [(&with_extra, &common), (&common, &with_extra)] {
        let output = compare_json(old, new, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{stdout}");
        assert!(stdout.contains("input fingerprint"), "{stdout}");
    }
}

#[test]
fn negative_measured_values_fail_even_against_not_run() {
    let missing = report(None);
    for metric in [
        "instructions",
        "allocs",
        "alloc_bytes",
        "syscalls",
        "locks",
        "wall_ns",
    ] {
        let invalid = measured_report(metric, -1.0, "test", Some("differential_perf_2n_minus_n"));
        for (old, new, side) in [(&invalid, &missing, "baseline"), (&missing, &invalid, "current")]
        {
            let output = compare_json(old, new, None);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(1), "{metric}: {stdout}");
            assert!(
                stdout.contains(&format!("{metric}: INCOMPARABLE invalid {side} value")),
                "{stdout}"
            );
            assert!(
                !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                "{stdout}"
            );
        }
    }
}

#[test]
fn zero_baselines_report_new_cost_without_division() {
    for metric in ["instructions", "allocs", "alloc_bytes", "syscalls", "locks"] {
        let zero = measured_report(metric, 0.0, "test", Some("differential_perf_2n_minus_n"));
        let positive = measured_report(metric, 1.0, "test", Some("differential_perf_2n_minus_n"));
        let output = compare_json(&zero, &positive, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{metric}: {stdout}");
        assert!(
            stdout.contains(&format!("{metric}: REGRESSED")),
            "{stdout}"
        );
        assert!(stdout.contains("(zero baseline)"), "{stdout}");
        assert!(!stdout.contains("improved"), "{stdout}");
        assert!(compare_json(&zero, &zero, None).status.success());
        let output = compare_json(&positive, &zero, None);
        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(&format!("{metric}: improved"))
        );
    }
    let zero = measured_report("wall_ns", 0.0, "ns per op (secondary)", None);
    let positive = measured_report("wall_ns", 1.0, "ns per op (secondary)", None);
    let output = compare_json(&zero, &positive, None);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("REGRESSED"));
}

#[test]
fn incompatible_measured_units_fail_without_reporting_a_win_or_loss() {
    for metric in [
        "instructions",
        "allocs",
        "alloc_bytes",
        "syscalls",
        "locks",
        "wall_ns",
    ] {
        let old = measured_report(
            metric,
            100.0,
            "old unit",
            Some("differential_perf_2n_minus_n"),
        );
        let new = measured_report(
            metric,
            1.0,
            "new unit",
            Some("differential_perf_2n_minus_n"),
        );
        for (base, current) in [(&old, &new), (&new, &old)] {
            let output = compare_json(base, current, None);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(1), "{metric}: {stdout}");
            assert!(
                stdout.contains(&format!("{metric}: INCOMPARABLE units")),
                "{stdout}"
            );
            assert!(
                !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                "{stdout}"
            );
        }
    }
    let blank = measured_report("locks", 1.0, "", None);
    let output = compare_json(&blank, &blank, None);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("locks: INCOMPARABLE units"));
}

#[test]
fn instruction_methods_must_match_when_both_values_are_measured() {
    let perf = measured_report(
        "instructions",
        100.0,
        "retired per op",
        Some("differential_perf_2n_minus_n"),
    );
    let callgrind = measured_report(
        "instructions",
        1.0,
        "retired per op",
        Some("differential_callgrind_2n_minus_n"),
    );
    let legacy = measured_report("instructions", 1.0, "retired per op", None);
    for other in [&callgrind, &legacy] {
        for (base, current) in [(&perf, other), (other, &perf)] {
            let output = compare_json(base, current, None);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(output.status.code(), Some(1), "{stdout}");
            assert!(
                stdout.contains("instructions: INCOMPARABLE instruction methods"),
                "{stdout}"
            );
            assert!(
                !stdout.contains("improved") && !stdout.contains("REGRESSED"),
                "{stdout}"
            );
        }
    }
    for invalid in ["", "not_run"] {
        let report = measured_report("instructions", 1.0, "retired per op", Some(invalid));
        let output = compare_json(&report, &report, None);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("INCOMPARABLE instruction methods")
        );
    }
    assert!(compare_json(&perf, &perf, None).status.success());
    // A missing method retains the existing whole-process legacy default.
    assert!(compare_json(&legacy, &legacy, None).status.success());
}

#[test]
fn not_run_still_skips_and_wall_values_are_not_thresholded() {
    let missing = report(None);
    let measured = measured_report("instructions", 1.0, "retired per op", Some("not_run"));
    for (base, current) in [(&missing, &measured), (&measured, &missing)] {
        let output = compare_json(base, current, None);
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("INCOMPARABLE"));
    }
    let old = measured_report("wall_ns", 100.0, "ns per op (secondary)", None);
    let new = measured_report("wall_ns", 1.0, "ns per op (secondary)", None);
    let output = compare_json(&old, &new, None);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("improved") && !stdout.contains("REGRESSED"),
        "{stdout}"
    );
}
