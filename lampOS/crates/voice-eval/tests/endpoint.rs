//! `lamp-voice-eval endpoint` as a played test runs it: a session folder
//! with its records in `metadata/`, read at any depth and in any file order,
//! and the exit code saying whether the end of speech worked.
use std::{fs, path::PathBuf, process::Command};

fn session(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lamp-endpoint-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-voice-eval"))
        .arg("endpoint")
        .args(args)
        .output()
        .unwrap()
}

const READY: &str = r#"{"kind":"endpoint_worker","at_us":100,"availability":{"state":"loading","detail":null},"report":null}
{"kind":"endpoint_worker","at_us":2400000,"availability":{"state":"ready","detail":null},"report":{"ready_after_us":2300000}}
"#;

#[test]
fn a_ready_worker_passes_and_its_ends_are_counted() {
    let dir = session(
        "ready",
        &[
            ("metadata/events-0.jsonl", READY),
            (
                "metadata/events-1.jsonl",
                r#"{"kind":"local_endpoint","turn":1,"at_us":3000000,"endpoint":{"basis":"model_complete","silence_ms":400,"probability":0.9,"fallback":null}}
{"kind":"local_endpoint","turn":2,"at_us":4000000,"endpoint":{"basis":"silence_fallback","silence_ms":600,"probability":null,"fallback":"short"}}
"#,
            ),
        ],
    );
    let output = run(&[dir.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert_eq!(
        text,
        "end-of-speech worker: loading, ready\n\
         ends of speech: 2: model_complete 1, silence_fallback 1 (short 1)\n"
    );
}

#[test]
fn a_failed_worker_fails_the_arm_and_says_why() {
    // lamp-4ace from 19 Oct: the old worker refused --no-spin.
    let dir = session(
        "failed",
        &[(
            "metadata/events-0.jsonl",
            r#"{"kind":"endpoint_worker","at_us":100,"availability":{"state":"loading","detail":null},"report":null}
{"kind":"endpoint_worker","at_us":200,"availability":{"state":"failed","detail":"exit status 2"},"report":null}
{"kind":"local_endpoint","turn":1,"at_us":3000000,"endpoint":{"basis":"silence_fallback","silence_ms":600,"probability":null,"fallback":"failed"}}
"#,
        )],
    );
    let output = run(&[dir.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains("FAULT: the end-of-speech worker is failed (exit status 2)"),
        "{text}"
    );
    assert!(
        text.contains("FAULT: 1 of 1 ends of speech fell back because the worker failed"),
        "{text}"
    );
}

#[test]
fn since_counts_only_the_arms_ends() {
    let dir = session(
        "since",
        &[(
            "metadata/events-0.jsonl",
            &format!(
                "{READY}{}\n{}\n",
                r#"{"kind":"local_endpoint","turn":1,"at_us":1000000,"endpoint":{"basis":"silence_fallback","silence_ms":600,"probability":null,"fallback":"failed"}}"#,
                r#"{"kind":"local_endpoint","turn":2,"at_us":9000000,"endpoint":{"basis":"model_complete","silence_ms":400,"probability":0.9,"fallback":null}}"#
            ),
        )],
    );
    let output = run(&[dir.to_str().unwrap(), "--since-us", "5000000"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(
        text.contains("ends of speech: 1: model_complete 1"),
        "{text}"
    );
}

#[test]
fn a_folder_with_no_records_fails_too() {
    // Nothing to show the end of speech worked: the arm fails, as every
    // refused input here does.
    let dir = session("empty", &[("metadata/notes.txt", "nothing")]);
    let output = run(&[dir.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("no events-*.jsonl under it"));
}

#[test]
fn a_link_back_up_the_session_is_not_followed() {
    // A link to the session itself, beside its records: followed, the
    // walk would go round until the stack ran out (the reviewer on #2097).
    let dir = session("loop", &[("metadata/events-0.jsonl", READY)]);
    std::os::unix::fs::symlink(&dir, dir.join("metadata/back-up")).unwrap();
    let output = run(&[dir.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    // The records are read once, not once per loop.
    assert_eq!(
        text,
        "end-of-speech worker: loading, ready\nends of speech: 0: \n"
    );
}
