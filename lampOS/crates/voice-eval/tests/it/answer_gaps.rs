//! Exact-file CLI reporting, privacy, evidence failures and bounded local I/O.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

struct Input(PathBuf);
impl Input {
    fn new(name: &str, text: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lamp-answer-gaps-{name}-{}.jsonl",
            std::process::id()
        ));
        fs::write(&path, text).unwrap();
        Self(path)
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A directory made for one test and removed after it.
struct Directory(PathBuf);
impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("lamp-answer-gaps-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-voice-eval"))
        .arg("answer-gaps")
        .args(args)
        .output()
        .unwrap()
}

fn pair(phase: &str) -> String {
    let event = |phase: &str, frames: u64| {
        let last = 1_000_000 + frames * 1_000_000 / 24_000;
        json!({"kind":"playback_gap", "phase":phase, "at_us":last, "gap":{
            "owner":{"boot":vec![1;16], "turn":1, "generation":1},
            "first_sample":{"epoch":1,"frame":100},
            "end_sample":{"epoch":1,"frame":100+frames},
            "started_at_us":1_000_000,"last_zero_accepted_at_us":last,"zero_frames":frames
        }})
    };
    format!("{}\n{}\n", event("started", 240), event(phase, 9600))
}

#[test]
fn answer_gaps_cli_keeps_files_separate_and_has_readable_and_json_reports() {
    let resumed = Input::new("resumed", &pair("resumed"));
    let cancelled = Input::new("cancelled", &pair("cancelled"));
    let paths = [resumed.0.to_str().unwrap(), cancelled.0.to_str().unwrap()];
    let output = run(&[paths[0], "--json", paths[1]]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["sample_rate_hz"], 24_000);
    assert_eq!(report["threshold_ms"], 300);
    assert_eq!(report["threshold_comparison"], "strictly_greater");
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["input"], paths[0]);
    assert_eq!(files[0]["over_300_ms"]["resumed"]["gaps"], 1);
    assert_eq!(files[1]["over_300_ms"]["cancelled"]["gaps"], 1);
    assert!(report.get("sessions").is_none());
    assert!(report.get("answered_replies").is_none());
    let output = run(&paths);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("resumed 1 gaps / 1 replies; cancelled 0 gaps / 0 replies"));
    assert!(text.contains("resumed 0 gaps / 0 replies; cancelled 1 gaps / 1 replies"));
}

#[test]
fn answer_gaps_cli_reports_bad_and_incomplete_evidence_without_words() {
    let private = "PRIVATE_SENTINEL";
    let text = format!(
        "{}\n{{\"kind\":\"transcript\",\"text\":\"{private}\"}}\n{private}\n",
        pair("resumed").lines().next().unwrap()
    );
    let input = Input::new("private", &text);
    for json in [false, true] {
        let mut args = vec![input.0.to_str().unwrap()];
        if json {
            args.push("--json");
        }
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
        for bytes in [&output.stdout, &output.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains(private));
        }
        if json {
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["files"][0]["incomplete"]["gaps"], 1);
            assert_eq!(report["files"][0]["problems"]["malformed_json"], 1);
            assert_eq!(report["files"][0]["evidence_complete"], false);
        }
    }
}

#[test]
fn answer_gaps_cli_rejects_directories_links_empty_and_oversized_inputs() {
    let empty = Input::new("empty", "");
    // A directory of the test's own: the temporary folder itself can be a
    // symlink (`TMPDIR=/tmp/` on macOS, where `/tmp` links to
    // `/private/tmp`), and the CLI refuses a symlink at `open`, before it
    // could tell a directory.
    let directory = Directory::new("directory");
    let directory = &directory.0;
    let link = Input::new("link", "");
    fs::remove_file(&link.0).unwrap();
    std::os::unix::fs::symlink(&empty.0, &link.0).unwrap();
    let oversized = Input::new("oversized", "");
    fs::File::options()
        .write(true)
        .open(&oversized.0)
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    let output = run(&[
        directory.to_str().unwrap(),
        empty.0.to_str().unwrap(),
        link.0.to_str().unwrap(),
        oversized.0.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let files = report["files"].as_array().unwrap();
    assert_eq!(files[0]["problems"]["input_error"], "not_regular_file");
    assert_eq!(files[1]["problems"]["no_gap_records"], true);
    assert_eq!(files[2]["problems"]["input_error"], "open");
    assert_eq!(files[3]["problems"]["input_error"], "file_limit");
    assert!(files.iter().all(|file| file["evidence_complete"] == false));
}

#[test]
fn answer_gaps_cli_requires_explicit_bounded_file_arguments() {
    for args in [
        vec![],
        vec!["--json"],
        vec!["--unsupported"],
        vec!["file"; 65],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
    }
}
