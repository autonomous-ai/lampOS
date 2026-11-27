use lamp_sensor_score::digest;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sensor-v2-cli-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}
fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-sensor-score"))
        .args(args)
        .output()
        .unwrap()
}
fn examples(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let manifest = dir.join("manifest.json");
    let run = dir.join("run.json");
    fs::write(
        &manifest,
        include_bytes!("../example-head-v2-manifest.json"),
    )
    .unwrap();
    fs::write(&run, include_bytes!("../example-head-v2-run.json")).unwrap();
    (manifest, run)
}
fn comparison() -> Value {
    let identity = json!({"build_sha":"a".repeat(40),"producer_binary_sha256":"b".repeat(64),
        "source_patch_sha256":null,"config_sha256":"c".repeat(64),"model_manifest_sha256":"d".repeat(64)});
    json!({"version":2,"rubric":"sensor-team-2026-11-27-v2","purpose":"baseline_characterization",
        "baseline":identity,"candidate":identity,"unchanged_config_sha256":"e".repeat(64),
        "kit_sha256":"f".repeat(64),"kit_reviewed":false,"collection_manifest_sha256":"1".repeat(64),
        "cases_sha256":digest(b"[]"),"scorer_revision_sha":"2".repeat(40),
        "scorer_binary_sha256":digest(&fs::read(env!("CARGO_BIN_EXE_lamp-sensor-score")).unwrap()),
        "protocol_frozen_unix_s":1000,"labels_adjudicated_before_predictions":false,"holdout_disjoint":false,
        "accepted_sensors":[],"added_sensor":null,"target_scenario":null,"prerequisite":null,
        "added_component_model_sha256":null,"added_component_config_sha256":null,"cases":[],"runs":[]})
}

#[test]
fn published_example_is_development_only_and_outputs_are_private_retained() {
    let dir = directory();
    let (m, r) = examples(&dir);
    assert!(invoke(&["validate", m.to_str().unwrap()]).status.success());
    let out = dir.join("scored");
    let result = invoke(&[
        "score",
        "head",
        m.to_str().unwrap(),
        r.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_slice(&fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["qualified"], false);
    assert_eq!(report["counts"]["total"], 0);
    assert_eq!(
        report["raw_by_split"]["development"]["counts"]["correct"],
        1
    );
    assert_eq!(report["raw_by_split"]["development"]["counts"]["total"], 1);
    assert_eq!(report["rows"][0]["latency_us"], Value::Null);
    assert!(
        fs::read_to_string(out.join("trials.tsv"))
            .unwrap()
            .contains("split\tdata_origin\tapplicable")
    );
    let sums = fs::read_to_string(out.join("SHA256SUMS")).unwrap();
    for name in ["report.json", "trials.tsv", "README.md"] {
        assert!(sums.contains(&format!(
            "{}  {name}",
            digest(&fs::read(out.join(name)).unwrap())
        )));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(out.join(name)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    assert_eq!(
        invoke(&[
            "score",
            "head",
            m.to_str().unwrap(),
            r.to_str().unwrap(),
            out.to_str().unwrap()
        ])
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn cd_reports_missing_evidence_without_promoting() {
    let dir = directory();
    let c = dir.join("comparison.json");
    let out = dir.join("out");
    fs::write(&c, serde_json::to_vec(&comparison()).unwrap()).unwrap();
    let result = invoke(&[
        "compare-whole",
        c.to_str().unwrap(),
        "-",
        "-",
        "-",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_slice(&fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["purpose"], "baseline_characterization");
}

#[test]
fn forged_qualified_prerequisite_cannot_replace_real_recomputation() {
    let dir = directory();
    let (m, r) = examples(&dir);
    let forged = json!({"version":2,"rubric_id":"sensor-team-2026-11-27-v2","sensor":"head",
        "qualified":true,"evidence":"lamp_live","checks":[{"passed":true}],
        "scorer_binary_sha256":digest(&fs::read(env!("CARGO_BIN_EXE_lamp-sensor-score")).unwrap()),
        "model_sha256":"d".repeat(64),"config_sha256":"e".repeat(64)});
    let report = dir.join("forged.json");
    let bytes = serde_json::to_vec(&forged).unwrap();
    fs::write(&report, &bytes).unwrap();
    let mut c = comparison();
    c["purpose"] = "one_sensor_addition".into();
    c["added_sensor"] = "head".into();
    c["target_scenario"] = "alone".into();
    c["added_component_model_sha256"] = "d".repeat(64).into();
    c["added_component_config_sha256"] = "e".repeat(64).into();
    c["prerequisite"] = json!({"sensor":"head","report_sha256":digest(&bytes),"rubric":"sensor-team-2026-11-27-v2",
        "qualified":true,"holdout_on_lamp":true,"model_sha256":"d".repeat(64),"config_sha256":"e".repeat(64)});
    let path = dir.join("comparison.json");
    fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
    let out = dir.join("out");
    let result = invoke(&[
        "compare-whole",
        path.to_str().unwrap(),
        m.to_str().unwrap(),
        r.to_str().unwrap(),
        report.to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        result.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("reproduce exactly"));
    assert!(!out.exists());
}

#[test]
fn declared_whole_scorer_binary_must_be_the_running_executable() {
    let dir = directory();
    let path = dir.join("comparison.json");
    let mut c = comparison();
    c["scorer_binary_sha256"] = "0".repeat(64).into();
    fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
    let out = dir.join("out");
    let result = invoke(&[
        "compare-whole",
        path.to_str().unwrap(),
        "-",
        "-",
        "-",
        out.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(!out.exists());
}
