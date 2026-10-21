//! `motion-voice` (the first builder on #1722, 21 Oct 05:49) run as the lamp
//! runs it: a sessions folder, a scene file's `#PLAY_US` lines, and the
//! printed table and JSON. Two invented let-go records: one at a played
//! line, about as loud as the asker; one with nobody playing, well under.
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Folder(PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn folder() -> Folder {
    let root = std::env::temp_dir().join(format!("motion-voice-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let meta = root.join("sessions/0123456789abcdef0123456789abcdef/metadata");
    fs::create_dir_all(&meta).unwrap();
    fs::write(
        meta.join("events-1.jsonl"),
        concat!(
            r#"{"service_sequence":1,"kind":"input_let_go_body_moving","at_us":10250000,"turn":3,"onset_from_us":10100000,"motion_voice":"record","asker_level":150,"heard_level":130,"heard_upper_tenth":146,"blocks":9,"would_keep":true}"#,
            "\n",
            r#"{"service_sequence":2,"kind":"input_let_go_body_moving","at_us":30250000,"turn":4,"onset_from_us":30100000,"motion_voice":"record","asker_level":150,"heard_level":20,"heard_upper_tenth":35,"blocks":8,"would_keep":false}"#,
            "\n",
            r#"{"service_sequence":3,"kind":"answer_waits","at_us":31000000}"#,
            "\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("scene.txt"),
        "scene\n#PLAY_US 10000000 A: words never printed\n",
    )
    .unwrap();
    Folder(root)
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lamp-voice-eval"))
        .arg("motion-voice")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn the_reader_lines_up_each_let_go_with_the_plays_and_says_the_bar() {
    let root = folder();
    let sessions = root.0.join("sessions");
    let scene = root.0.join("scene.txt");
    let (ok, text) = run(&[
        sessions.to_str().unwrap(),
        "--plays",
        scene.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    assert!(
        text.contains("10250000\t3\t9\t150\t130\t146\t0.5\ttrue\ttrue"),
        "{text}"
    );
    assert!(
        text.contains("30250000\t4\t8\t150\t20\t35\t15.3\tfalse\tfalse"),
        "{text}"
    );
    assert!(text.contains("judged 2 | no asker's level 0"), "{text}");
    assert!(!text.contains("words never printed"), "{text}");

    let one = sessions.join("0123456789abcdef0123456789abcdef");
    let (ok, text) = run(&[
        one.to_str().unwrap(),
        "--plays",
        scene.to_str().unwrap(),
        "--json",
    ]);
    assert!(ok, "{text}");
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["judged"], 2);
    assert_eq!(report["bar_on_upper_tenth"]["clean"], true);
    assert_eq!(report["bar_on_upper_tenth"]["bar_db"], 0.5);
    assert_eq!(report["bar_on_upper_tenth"]["clean_to_db"], 15.2);

    // With no plays the bar can't be said; with no folder it's refused.
    let (ok, text) = run(&[sessions.to_str().unwrap(), "--json"]);
    assert!(ok);
    assert!(serde_json::from_str::<Value>(&text).unwrap()["bar_on_upper_tenth"].is_null());
    let (ok, _) = run(&[]);
    assert!(!ok);
}
