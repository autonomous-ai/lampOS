#[path = "support/openat2.rs"]
mod openat2;
#[path = "support/records_control.rs"]
mod records_control;

use lamp_records::{
    Disk, RootReport, RunReport,
    config::Config,
    json_file, plan,
    report::{Reporter, diagnostic},
    run::{self, RUN_BUDGET},
    store::Store,
};
use std::{
    collections::BTreeSet,
    fs::{self, File, FileTimes, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        // Short enough for the real bus's Unix socket path on both platforms.
        let tmp = if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        };
        let path = Path::new(tmp).join(format!(
            "lr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&path);
        Self(path)
    }
    fn config(&self) -> Config {
        let mut config = Config::customer_default();
        config.enabled = true;
        config.roots[0].path = self.0.join("sessions");
        config.bus_root = self.0.join("bus");
        config.roots[0].unit = "lamp-records-fixture-voice.service".into();
        private_dir(&config.roots[0].path);
        config
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn private_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn session(root: &Path, n: u32, bytes: usize) -> String {
    let id = format!("{n:032x}");
    let dir = root.join(&id);
    private_dir(&dir);
    private_dir(&dir.join("metadata"));
    for name in [
        "events-0.jsonl",
        "events-1.jsonl",
        "events-2.jsonl",
        "events-3.jsonl",
        "health.json",
    ] {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(dir.join("metadata").join(name))
            .unwrap();
        file.write_all(&vec![b'x'; bytes]).unwrap();
        file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100)))
            .unwrap();
    }
    for path in [&dir, &dir.join("metadata")] {
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100)))
            .unwrap();
    }
    id
}
fn entries(root: &Path) -> Vec<(PathBuf, u64, u64, i64, i64, u32)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        found.push((
            path.clone(),
            metadata.ino(),
            metadata.size(),
            metadata.mtime(),
            metadata.ctime(),
            metadata.mode(),
        ));
        if metadata.is_dir() {
            found.extend(entries(&path));
        }
    }
    found.sort();
    found
}

#[test]
fn public_tree_pipeline_dry_then_apply_keeps_live_memory_and_outside_link() {
    if openat2::store_cannot_run_here() {
        return;
    }
    let tree = Tree::new();
    let config = tree.config();
    let root = &config.roots[0];
    let live = session(&root.path, 1, 1024);
    let closed = session(&root.path, 2, 1024);
    let linked = session(&root.path, 3, 1024);
    let outside = tree.0.join("memory.db");
    fs::write(&outside, b"remember this").unwrap();
    let record = root.path.join(&linked).join("metadata/events-0.jsonl");
    fs::remove_file(&record).unwrap();
    symlink(&outside, record).unwrap();
    let before = entries(&tree.0);
    let mut store = Store::open(&root.path).unwrap();
    let report = run::root_with(
        root,
        2_000_000,
        false,
        &mut store,
        || Ok(BTreeSet::from([live.clone()])),
        Instant::now() + RUN_BUDGET,
    );
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.planned, vec![closed.clone()]);
    assert_eq!(report.refused.len(), 1);
    assert!(report.removed.is_empty());
    assert_eq!(entries(&tree.0), before);
    drop(store);
    let mut store = Store::open(&root.path).unwrap();
    let report = run::root_with(
        root,
        2_000_000,
        true,
        &mut store,
        || Ok(BTreeSet::from([live.clone()])),
        Instant::now() + RUN_BUDGET,
    );
    assert_eq!(report.removed, vec![closed.clone()]);
    assert!(root.path.join(live).is_dir());
    assert!(!root.path.join(closed).exists());
    assert!(root.path.join(linked).is_dir());
    assert_eq!(fs::read(outside).unwrap(), b"remember this");
    let receipt = RunReport {
        schema_version: 1,
        config_fingerprint: config.fingerprint(),
        at_unix_s: 2_000_000,
        mode: "apply".into(),
        enabled: true,
        roots: vec![report],
        error: None,
    };
    receipt.validate().unwrap();
    json_file::write(&tree.0.join("last.json"), &receipt).unwrap();
    let restored: RunReport =
        json_file::read(&tree.0.join("last.json"), json_file::REPORT_BYTES).unwrap();
    assert_eq!(restored.roots[0].removed.len(), 1);
}

#[test]
fn real_cli_apply_with_unknown_service_cannot_delete_any_fixture_record() {
    let tree = Tree::new();
    let config = tree.config();
    let id = session(&config.roots[0].path, 1, 100);
    let path = tree.0.join("records.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lamp-records"))
        .args(["check", path.to_str().unwrap(), "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: RunReport = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.roots[0].error.is_some());
    assert!(report.roots[0].planned.is_empty());
    assert!(report.roots[0].removed.is_empty());
    assert!(config.roots[0].path.join(id).is_dir());
    assert!(!config.roots[0].path.join(".lamp-records-trash").exists());
}

#[test]
fn real_cli_sets_aside_a_newer_report_and_refuses_unknown_values_before_cleanup() {
    let tree = Tree::new();
    let mut config = tree.config();
    config.enabled = false;
    session(&config.roots[0].path, 1, 100);
    let untouched = entries(&config.roots[0].path);
    let config_path = tree.0.join("records.json");
    let report_path = tree.0.join("last.json");
    json_file::write(&config_path, &config).unwrap();
    let current = RunReport {
        schema_version: 1,
        config_fingerprint: config.fingerprint(),
        at_unix_s: 17,
        mode: "apply".into(),
        enabled: false,
        roots: vec![],
        error: None,
    };
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_lamp-records"))
            .arg("check")
            .arg(&config_path)
            .arg("--apply")
            .arg("--report")
            .arg(&report_path)
            .output()
            .unwrap()
    };
    let mut later = serde_json::to_value(&current).unwrap();
    later["extra"] = serde_json::json!({"kind": "new_detail"});
    json_file::write(&report_path, &later).unwrap();
    assert!(run().status.success());
    later["schema_version"] = 2.into();
    later["mode"] = "later_mode".into();
    later["roots"] = serde_json::json!([{"later_kind": "new_enum_value"}]);
    let bytes = serde_json::to_vec_pretty(&later).unwrap();
    fs::write(&report_path, &bytes).unwrap();
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let journal = String::from_utf8(output.stderr).unwrap();
    assert_eq!(journal.lines().count(), 1);
    assert!(journal.contains("cleanup result is newer than this build (schema 2)"));
    let archives: Vec<_> = fs::read_dir(&tree.0)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("last.json.newer-2-")
        })
        .collect();
    assert_eq!(archives.len(), 1);
    assert_eq!(fs::read(archives[0].path()).unwrap(), bytes);
    let replacement: RunReport = json_file::read(&report_path, json_file::REPORT_BYTES).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::to_value(&replacement).unwrap()
    );
    assert_eq!(replacement.schema_version, 1);
    assert!(!replacement.enabled);
    assert_eq!(replacement.config_fingerprint, config.fingerprint());
    config.enabled = true;
    json_file::write(&config_path, &config).unwrap();
    // A newer build's plain new mode at this schema is set aside too, and
    // the cleanup runs (#1680); the mode is never in the journal.
    let mut newer_mode = serde_json::to_value(&current).unwrap();
    newer_mode["mode"] = "later_mode".into();
    let bytes = serde_json::to_vec(&newer_mode).unwrap();
    fs::write(&report_path, &bytes).unwrap();
    let output = run();
    let journal = String::from_utf8(output.stderr).unwrap();
    assert!(!output.stdout.is_empty(), "{journal}");
    assert!(
        journal.contains("cleanup result has a mode this build does not know, at its schema 1"),
        "{journal}"
    );
    assert!(!journal.contains("later_mode"), "{journal}");
    let kept: Vec<_> = fs::read_dir(&tree.0)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("last.json.newer-mode-")
        })
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read(kept[0].path()).unwrap(), bytes);
    let replacement: RunReport = json_file::read(&report_path, json_file::REPORT_BYTES).unwrap();
    assert_eq!(replacement.mode, "apply");
    // A mode that is no plain word is still refused before any cleanup.
    let mut unknown = serde_json::to_value(current).unwrap();
    unknown["mode"] = "Later Mode!".into();
    let bytes = serde_json::to_vec(&unknown).unwrap();
    fs::write(&report_path, &bytes).unwrap();
    let refused = run();
    assert_eq!(refused.status.code(), Some(64));
    assert!(refused.stdout.is_empty());
    assert_eq!(fs::read(&report_path).unwrap(), bytes);
    assert_eq!(entries(&config.roots[0].path), untouched);
}

/// #2052's audit, the records rows (#1680): `check`, the timer's run, on a
/// newer build's files. A newer field in `records.json` is read past and
/// named, and the run is as before; a newer `schema_version` succeeds,
/// prunes nothing and writes no result, with one line. Before, every run
/// failed on either.
#[test]
fn real_cli_runs_on_a_newer_builds_config_one_release_ahead() {
    let tree = Tree::new();
    let config = tree.config();
    session(&config.roots[0].path, 1, 100);
    let untouched = entries(&config.roots[0].path);
    let config_path = tree.0.join("records.json");
    let report_path = tree.0.join("last.json");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_lamp-records"))
            .arg("check")
            .arg(&config_path)
            .arg("--apply")
            .arg("--report")
            .arg(&report_path)
            .output()
            .unwrap()
    };
    let mut newer = serde_json::to_value(&config).unwrap();
    newer["roots"][0]["min_free_bytes"] = 1_000_000_000_u64.into();
    json_file::write(&config_path, &newer).unwrap();
    let output = run();
    let journal = String::from_utf8(output.stderr).unwrap();
    assert!(
        journal.contains(&format!(
            "lamp-records: {}: fields this build does not read, ignored: roots[0].min_free_bytes",
            config_path.display()
        )),
        "{journal}"
    );
    let report: RunReport = json_file::read(&report_path, json_file::REPORT_BYTES).unwrap();
    assert_eq!(report.config_fingerprint, config.fingerprint());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
    fs::remove_file(&report_path).unwrap();
    newer["schema_version"] = 2.into();
    newer["roots"][0]["kind"] = "memory_days".into();
    json_file::write(&config_path, &newer).unwrap();
    let output = run();
    let journal = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{journal}");
    assert!(output.stdout.is_empty());
    assert_eq!(journal.lines().count(), 1, "{journal}");
    assert!(
        journal.contains("schema 2 is newer than this build's 1: nothing is pruned"),
        "{journal}"
    );
    assert!(!report_path.exists());
    assert_eq!(entries(&config.roots[0].path), untouched);
}

#[test]
fn diagnostic_is_delivered_over_the_real_local_bus_without_record_contents() {
    let tree = Tree::new();
    let bus = lamp_bus::Bus::new(tree.0.join("bus")).unwrap();
    let mut listener = bus
        .subscribe_typed("diagnostics", "test", lamp_msgs::Diagnostic::TYPE)
        .unwrap();
    let mut reporter = Reporter::new(bus.root()).unwrap();
    let report = diagnostic(None, 10);
    reporter.publish(&report).unwrap();
    let received = listener
        .recv(Duration::from_secs(1))
        .unwrap()
        .expect("one local diagnostic");
    let value: lamp_msgs::Diagnostic = serde_json::from_value(received.json().unwrap()).unwrap();
    assert_eq!(value.name, "records");
    assert_eq!(value.level, lamp_msgs::Level::Error);
    assert!(value.message.contains("no cleanup"));
}

#[test]
fn real_reporter_reloads_switch_and_policy_and_requires_a_matching_receipt() {
    use lamp_msgs::{Diagnostic, Level};
    use records_control::{RunningReporter, Window};

    fn field<'a>(diagnostic: &'a Diagnostic, key: &str) -> Option<&'a str> {
        diagnostic
            .values
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    }

    fn expect_status(
        listener: &mut lamp_bus::Subscriber,
        reporter: &mut RunningReporter,
        level: Level,
        enabled: bool,
        before_bytes: u64,
    ) -> Diagnostic {
        let deadline = Instant::now() + Duration::from_secs(4);
        let window = Window::new(deadline, reporter.elapsed_ns().unwrap()).unwrap();
        reporter.settle_current(&window).unwrap();
        let enabled = enabled.to_string();
        let before_bytes = before_bytes.to_string();
        let mut last = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "no {level:?} diagnostic with enabled={enabled}, before_bytes={before_bytes}; last={last:?}"
            );
            assert!(
                reporter.0.try_wait().unwrap().is_none(),
                "reporter exited before the expected transition; last={last:?}"
            );
            window.receiving(reporter.elapsed_ns().unwrap()).unwrap();
            let Some(message) = listener.recv(Duration::ZERO).unwrap() else {
                reporter.step_due(&window).unwrap();
                continue;
            };
            let diagnostic: Diagnostic = serde_json::from_value(message.json().unwrap()).unwrap();
            if diagnostic.name == "records"
                && diagnostic.level == level
                && field(&diagnostic, "enabled") == Some(enabled.as_str())
                && field(&diagnostic, "before_bytes") == Some(before_bytes.as_str())
            {
                return diagnostic;
            }
            last = Some(diagnostic);
        }
    }

    fn receipt(config: &Config, before_bytes: u64) -> RunReport {
        let roots = if config.enabled {
            vec![RootReport {
                root: config.roots[0].path.clone(),
                unit: config.roots[0].unit.clone(),
                before_bytes,
                planned_bytes: 0,
                removed_bytes: 0,
                remaining_bytes: before_bytes,
                limit_bytes: config.roots[0].max_bytes,
                active: vec![],
                planned: vec![],
                removed: vec![],
                refused: vec![],
                processes: lamp_records::ProcessObservations::default(),
                // Injected fixture counters, never this host's free-space data.
                disk: Some(Disk {
                    total_bytes: 1_000,
                    available_bytes: 500,
                }),
                error: None,
            }]
        } else {
            vec![]
        };
        let report = RunReport {
            schema_version: 1,
            config_fingerprint: config.fingerprint(),
            at_unix_s: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mode: "apply".into(),
            enabled: config.enabled,
            roots,
            error: None,
        };
        report.validate().unwrap();
        report
    }

    let tree = Tree::new();
    let mut config = tree.config();
    config.enabled = false;
    let config_path = tree.0.join("records.json");
    let result_path = tree.0.join("last.json");
    json_file::write(&config_path, &config).unwrap();
    let bus = lamp_bus::Bus::new(&config.bus_root).unwrap();
    // Subscribe before starting the binary so its first publication is visible.
    let mut listener = bus
        .subscribe_typed(Diagnostic::TOPIC, "reload", Diagnostic::TYPE)
        .unwrap();
    let mut reporter = RunningReporter::spawn(
        Command::new(env!("CARGO_BIN_EXE_lamp-records-fixture"))
            .arg("report")
            .arg(&config_path)
            .arg(&result_path)
            .args(["--seconds", "30"])
            .stderr(Stdio::piped()),
    )
    .unwrap();

    let disabled = expect_status(&mut listener, &mut reporter, Level::Ok, false, 0);
    assert!(disabled.message.contains("disabled"));
    assert_eq!(field(&disabled, "roots"), Some("0"));
    assert!(
        !result_path.exists(),
        "disabled reporting must not need a receipt"
    );

    json_file::write(&result_path, &receipt(&config, 0)).unwrap();
    config.enabled = true;
    json_file::write(&config_path, &config).unwrap();
    let missing = expect_status(&mut listener, &mut reporter, Level::Error, true, 0);
    assert_eq!(field(&missing, "roots"), Some("0"));

    let mut additive = serde_json::to_value(receipt(&config, 111)).unwrap();
    additive["later"] = true.into();
    additive["roots"][0]["later"] = true.into();
    additive["roots"][0]["disk"]["later"] = true.into();
    json_file::write(&result_path, &additive).unwrap();
    let healthy = expect_status(&mut listener, &mut reporter, Level::Ok, true, 111);
    assert_eq!(field(&healthy, "roots"), Some("1"));
    assert_eq!(field(&healthy, "disk_free_at_cleanup_pct"), Some("50.00"));

    config.roots[0].max_age_seconds += 1;
    json_file::write(&config_path, &config).unwrap();
    let changed = expect_status(&mut listener, &mut reporter, Level::Error, true, 0);
    assert_eq!(field(&changed, "roots"), Some("0"));

    let current = receipt(&config, 222);
    json_file::write(&result_path, &current).unwrap();
    let healthy = expect_status(&mut listener, &mut reporter, Level::Ok, true, 222);
    assert_eq!(field(&healthy, "disk_free_at_cleanup_pct"), Some("50.00"));

    let mut future = serde_json::to_value(&current).unwrap();
    future["schema_version"] = 2.into();
    future["mode"] = "later_mode".into();
    future["roots"][0]["processes"] = serde_json::json!({"later_kind": "new_enum_value"});
    let future_bytes = serde_json::to_vec_pretty(&future).unwrap();
    fs::write(&result_path, &future_bytes).unwrap();
    for _ in 0..2 {
        let missing = expect_status(&mut listener, &mut reporter, Level::Error, true, 0);
        assert_eq!(field(&missing, "roots"), Some("0"));
        assert!(
            !result_path.exists(),
            "reporting must not manufacture a cleanup receipt"
        );
    }
    let archives: Vec<_> = fs::read_dir(&tree.0)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("last.json.newer-2-")
        })
        .collect();
    assert_eq!(archives.len(), 1);
    assert_eq!(fs::read(archives[0].path()).unwrap(), future_bytes);
    for reason in [
        serde_json::json!("later_reason"),
        serde_json::json!({"kind": "later_reason"}),
    ] {
        let mut unknown = serde_json::to_value(&current).unwrap();
        unknown["roots"][0]["processes"] = serde_json::json!({
            "gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0,
            "examples": [{"pid": 42, "reason": reason, "later": true}]
        });
        let bytes = serde_json::to_vec(&unknown).unwrap();
        fs::write(&result_path, &bytes).unwrap();
        expect_status(&mut listener, &mut reporter, Level::Error, true, 0);
        assert_eq!(fs::read(&result_path).unwrap(), bytes);
    }
    json_file::write(&result_path, &current).unwrap();
    expect_status(&mut listener, &mut reporter, Level::Ok, true, 222);

    config.enabled = false;
    json_file::write(&config_path, &config).unwrap();
    let disabled = expect_status(&mut listener, &mut reporter, Level::Ok, false, 0);
    assert!(disabled.message.contains("disabled"));
    assert_eq!(field(&disabled, "roots"), Some("0"));
    assert_eq!(
        field(&disabled, "disk_free_at_cleanup_pct"),
        Some("unknown")
    );
    let unchanged: RunReport = json_file::read(&result_path, json_file::REPORT_BYTES).unwrap();
    assert!(unchanged.enabled);
    assert_eq!(unchanged.config_fingerprint, current.config_fingerprint);
    assert_eq!(unchanged.roots[0].before_bytes, 222);
    let mut stderr = reporter.0.stderr.take().unwrap();
    drop(reporter);
    let mut journal = String::new();
    stderr.read_to_string(&mut journal).unwrap();
    assert_eq!(
        journal
            .lines()
            .filter(|line| line.contains("cleanup result is newer than this build"))
            .count(),
        1
    );
}

fn distribution(samples: &mut [f64]) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    serde_json::json!({"runs":samples.len(), "first_or_median_not_cold_cache":"cache not evicted",
        "median_ms":samples[samples.len()/2], "p95_ms":samples[(samples.len()*95).div_ceil(100)-1], "worst_ms":samples[samples.len()-1]})
}

#[test]
#[ignore = "finite release timing of disposable records; report machine separately"]
fn records_cost() {
    let tree = Tree::new();
    let config = tree.config();
    let root = &config.roots[0].path;
    for id in 1..=1000 {
        session(root, id, 1024);
    }
    let mut store = Store::open(root).unwrap();
    let first = Instant::now();
    let inventory = store.inventory().unwrap();
    let first_ms = first.elapsed().as_secs_f64() * 1000.;
    assert_eq!(inventory.sessions.len(), 1000);
    let live = BTreeSet::from([format!("{:032x}", 1000)]);
    let mut inventory_ms = Vec::new();
    let mut plan_ms = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let current = store.inventory().unwrap();
        inventory_ms.push(start.elapsed().as_secs_f64() * 1000.);
        let start = Instant::now();
        let plan = plan::make(&current.sessions, &live, 2_000_000, 500_000_000, 1_209_600).unwrap();
        plan_ms.push(start.elapsed().as_secs_f64() * 1000.);
        assert_eq!(plan.remove.len(), 999);
    }
    let mut removal_ms = Vec::new();
    let mut unlinked_bytes = 0;
    for id in 1..=30 {
        let start = Instant::now();
        let removed = store.remove(&format!("{id:032x}")).unwrap();
        removal_ms.push(start.elapsed().as_secs_f64() * 1000.);
        assert!(removed.complete);
        unlinked_bytes += removed.removed_bytes;
    }
    println!(
        "{}",
        serde_json::json!({"fixture_sessions":1000,"files_per_session":5,"logical_bytes_per_file":1024,
        "at_unix_s":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        "first_inventory_ms":first_ms,"inventory":distribution(&mut inventory_ms),"plan":distribution(&mut plan_ms),
        "single_session_removal":distribution(&mut removal_ms),"unlinked_bytes_for_30":unlinked_bytes,
        "guard":"not timed; injected live set","platform":std::env::consts::OS,"architecture":std::env::consts::ARCH})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an isolated disposable Linux container with CAP_SYS_ADMIN"]
fn bind_mounts_are_refused_even_on_the_same_filesystem() {
    struct Mounted(PathBuf);
    impl Drop for Mounted {
        fn drop(&mut self) {
            assert!(
                Command::new("umount")
                    .arg(&self.0)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    fn bind(source: &Path, target: &Path) -> Mounted {
        assert!(
            Command::new("mount")
                .args(["--bind"])
                .arg(source)
                .arg(target)
                .status()
                .unwrap()
                .success()
        );
        Mounted(target.to_owned())
    }
    let tree = Tree::new();
    let config = tree.config();
    let root = &config.roots[0].path;
    let id = session(root, 1, 100);
    let outside = tree.0.join("outside");
    private_dir(&outside);
    let sentinel = outside.join("sentinel");
    fs::write(&sentinel, b"outside must survive").unwrap();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o600)).unwrap();
    let target = root.join(&id).join("metadata/events-0.jsonl");
    assert_eq!(
        fs::metadata(&sentinel).unwrap().dev(),
        fs::metadata(&target).unwrap().dev()
    );
    {
        let _mount = bind(&sentinel, &target);
        let mut store = Store::open(root).unwrap();
        let inventory = store.inventory().unwrap();
        assert!(inventory.sessions.is_empty());
        assert_eq!(inventory.refused.len(), 1);
        assert!(store.remove(&id).is_err());
    }
    {
        let _mount = bind(&outside, &root.join(&id).join("metadata"));
        let mut store = Store::open(root).unwrap();
        let inventory = store.inventory().unwrap();
        assert!(inventory.sessions.is_empty());
        assert_eq!(inventory.refused.len(), 1);
        assert!(store.remove(&id).is_err());
    }
    assert_eq!(fs::read(sentinel).unwrap(), b"outside must survive");
    assert!(root.join(id).exists());
}

/// #1505: a run report as a later build writes it, with a field this build
/// doesn't know, reads as the same report without it. This shows only that
/// the type reads. That `check` does not refuse it is shown by
/// `real_cli_sets_aside_a_newer_report_and_refuses_unknown_values_before_cleanup`
/// (its first run), and that health reads such a receipt as ok by
/// `real_reporter_reloads_switch_and_policy_and_requires_a_matching_receipt`.
#[test]
fn a_report_from_a_later_build_is_read() {
    let tree = Tree::new();
    let config = tree.config();
    let receipt = RunReport {
        schema_version: 1,
        config_fingerprint: config.fingerprint(),
        at_unix_s: 2_000_000,
        mode: "apply".into(),
        enabled: true,
        roots: Vec::new(),
        error: None,
    };
    receipt.validate().unwrap();
    let path = tree.0.join("last.json");
    json_file::write(&path, &receipt).unwrap();
    let read: RunReport = json_file::read(&path, json_file::REPORT_BYTES).unwrap();
    assert_eq!(read.at_unix_s, 2_000_000);
    let mut later: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    later["a_field_from_a_later_build"] = serde_json::json!(true);
    json_file::write(&path, &later).unwrap();
    let read: RunReport = json_file::read(&path, json_file::REPORT_BYTES).unwrap();
    assert_eq!(read.at_unix_s, 2_000_000);
}
