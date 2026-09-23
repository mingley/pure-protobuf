use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn count(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric {field}: {value}"))
}

#[test]
fn separate_process_mixed_load_preserves_outcomes_and_rejects_qualification() {
    let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/fairness.json");
    let path = std::env::temp_dir().join(format!(
        "pbrs-rt07-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos()
    ));
    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    let output = Command::new(binary)
        .arg("fairness")
        .arg("--scenario")
        .arg(&scenario)
        .arg("--smoke")
        .arg("--output")
        .arg(&path)
        .output()
        .expect("start fairness diagnostic");
    assert!(
        output.status.success(),
        "fairness diagnostic failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    #[allow(
        clippy::disallowed_methods,
        reason = "this synchronous test reads the completed child process's report"
    )]
    let artifact = std::fs::read_to_string(&path).expect("report artifact");
    std::fs::remove_file(&path).expect("clean up only this report artifact");
    let report: Value = serde_json::from_str(&artifact).expect("valid report artifact");
    let stdout: Value = serde_json::from_slice(&output.stdout).expect("same report on stdout");
    assert_eq!(report, stdout);
    assert_eq!(report["scenario_id"], "bulk_vs_small_streams");
    assert_eq!(report["mode"], "diagnostic_smoke");
    assert_eq!(report["peer_combination"], "native/native");
    assert_eq!(report["qualification"]["qualified"], false);
    assert!(
        !report["qualification"]["blockers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(count(&report, "small_calls_while_bulk_in_flight") > 0);

    let classes = &report["per_workload_class"];
    for name in ["bulk_streams", "competing_small_rpcs"] {
        let class = &classes[name];
        let offered = count(class, "offered_calls");
        let dispatched = count(class, "dispatched_calls");
        let successes = count(class, "successful_calls");
        let failures = count(class, "failed_calls");
        let overflows = count(&class["rejected_calls_by_reason"], "QUEUE_OVERFLOW");
        let errors: u64 = class["status_errors"]
            .as_object()
            .expect("typed error breakdown")
            .values()
            .map(|n| n.as_u64().expect("status count"))
            .sum();
        assert!(
            offered > 0 && successes > 0,
            "{name} did not exercise the service"
        );
        assert_eq!(
            offered,
            dispatched + overflows,
            "{name} dropped offered calls"
        );
        assert_eq!(
            dispatched,
            successes + failures,
            "{name} lost dispatched calls"
        );
        assert_eq!(errors, failures + overflows, "{name} omitted status errors");
        assert_eq!(
            class["latency_samples_nanos"].as_array().unwrap().len() as u64,
            dispatched,
            "{name} omitted latency samples"
        );
        assert!(
            class["queue_delay_nanos"].is_null(),
            "full queue delay is unsupported"
        );
        assert!(
            class["observed_client_pool_wait_nanos"]["samples"]
                .as_u64()
                .is_some_and(|count| count > 0),
            "{name} did not sample real pool waits"
        );
    }

    let small = &classes["competing_small_rpcs"];
    assert_eq!(small["arrival"], "poisson");
    assert_eq!(
        small["scheduling_lag_samples_nanos"]
            .as_array()
            .unwrap()
            .len() as u64,
        count(small, "offered_calls")
    );
    assert!(small["scheduling_lag_p99_nanos"].is_number());
    let overflow = count(&small["rejected_calls_by_reason"], "QUEUE_OVERFLOW");
    assert_eq!(
        small["p99_latency_nanos"].is_null(),
        overflow > 0 || count(small, "failed_calls") > count(small, "timed_out_calls")
    );
    let bulk = &classes["bulk_streams"];
    assert_eq!(bulk["arrival"], "closed_concurrent");
    assert!(bulk["scheduling_lag_p99_nanos"].is_null());
    assert_eq!(
        bulk["p99_latency_nanos"].is_null(),
        count(bulk, "failed_calls") > count(bulk, "timed_out_calls")
    );
    assert_eq!(
        count(bulk, "successful_stream_messages"),
        count(bulk, "successful_calls") * 64
    );

    let endpoints = &report["per_endpoint"];
    assert_ne!(
        count(endpoints, "client_pid"),
        count(endpoints, "server_pid")
    );
    let budget = count(&report["case"]["limits"], "byte_budget_bytes");
    for role in ["client", "server"] {
        let start = count(endpoints, &format!("{role}_start_rss_bytes"));
        assert!(start > 0, "{role} baseline RSS is not measured");
        assert!(
            count(endpoints, &format!("{role}_peak_rss_bytes")) >= start,
            "{role} peak RSS contradicts baseline"
        );
        assert!(
            endpoints[format!("{role}_cpu_seconds")]
                .as_f64()
                .is_some_and(|cpu| cpu >= 0.0),
            "{role} CPU is unavailable"
        );
        assert_eq!(
            count(
                endpoints,
                &format!("{role}_byte_budget_allocated_bytes_post_drain")
            ),
            0,
            "{role} budget did not drain"
        );
        let observed = count(endpoints, &format!("{role}_sampled_byte_budget_peak_bytes"));
        assert!(
            observed > 0 && observed <= budget,
            "{role} byte sample is not credible"
        );
        assert!(endpoints[format!("{role}_byte_budget_allocated_bytes_peak")].is_null());
        assert!(endpoints[format!("{role}_active_permits_peak")].is_null());
        assert!(endpoints[format!("{role}_active_permits_post_drain")].is_null());
    }
    assert_eq!(endpoints["post_drain_probe_success"], true);
    assert!(
        report["unsupported_metrics"]["per_endpoint.*_active_permits_peak"]
            .as_str()
            .is_some_and(|reason| reason.contains("no public gauge"))
    );
    assert!(
        report["unsupported_metrics"]["per_endpoint.*_byte_budget_allocated_bytes_peak"]
            .as_str()
            .is_some_and(|reason| reason.contains("lower bounds"))
    );

    let strict = Command::new(binary)
        .arg("fairness")
        .arg("--scenario")
        .arg(&scenario)
        .arg("--smoke")
        .arg("--require-qualified")
        .output()
        .expect("start strict fairness diagnostic");
    assert!(
        !strict.status.success(),
        "missing qualification evidence must fail closed"
    );
    let strict_report: Value =
        serde_json::from_slice(&strict.stdout).expect("strict run still reports data");
    assert_eq!(strict_report["qualification"]["qualified"], false);
    assert!(String::from_utf8_lossy(&strict.stderr).contains("RT-07 qualification unavailable"));
}
