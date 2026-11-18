//! #7685: `lamp-ears-body-kit` on a bus of its own: the office hour logs
//! body scores only, `suggest` reads the clips' scores and that log, and
//! `delete` removes the kit's files and the folder, and nothing else.
use lamp_msgs::{OwnVoiceEvidence, SoundClasses, SoundScores, SoundWindow};
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};

const KIT: &str = env!("CARGO_BIN_EXE_lamp-ears-body-kit");

#[test]
fn the_office_hour_logs_scores_suggest_reads_them_and_delete_leaves_nothing_of_the_kit() {
    let root = tempfile_dir("bus");
    let dir = tempfile_dir("kit").join("session");
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let office = Command::new(KIT)
        .args(["office", dir.to_str().unwrap(), "--minutes", "0.1"])
        .args(["--bus-root", root.to_str().unwrap()])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ears = bus
        .publisher_declared(SoundClasses::TOPIC, &SoundClasses::declaration(), "ears")
        .unwrap();
    // Wait for the kit's subscription, then a window every 100 ms.
    for _ in 0..50 {
        if !bus.subscribers(SoundClasses::TOPIC).unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for n in 0..20u64 {
        let now = lamp_ipc::monotonic_us();
        let message = SoundClasses::Scored {
            model_id: "fixture".into(),
            window: SoundWindow {
                start_us: now - 980_000,
                end_us: now - 10_000,
                completed_us: now,
                capture_epoch: 1,
            },
            scores: SoundScores {
                cough: if n == 5 { 0.9 } else { 0.05 },
                ..SoundScores::default()
            },
            own_voice: OwnVoiceEvidence::Unknown,
        };
        ears.publish_json(now - 10_000, &message).unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    let out = office.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = fs::read_to_string(dir.join("office.csv")).unwrap();
    assert!(log.starts_with("end_us,own_voice,cough,"), "{log}");
    assert!(log.lines().count() > 10, "{log}");
    assert!(log.contains(",unknown,0.9000,"), "{log}");
    assert!(!log.contains("speech"), "scores of the body sounds only");

    // Clips' scores as `take` writes them, and a file not the kit's.
    let clip = |score: f32| {
        format!("end_s,cough,throat_clear,sneeze,sniff,wheeze\n0.97,{score},0,0,0,0\n")
    };
    for n in 1..=10 {
        fs::write(dir.join(format!("cough-{n:02}.csv")), clip(0.8)).unwrap();
        fs::write(dir.join(format!("cough-{n:02}.wav")), b"RIFF").unwrap();
    }
    fs::write(dir.join("speech-01.csv"), clip(0.3)).unwrap();
    fs::write(dir.join("notes.txt"), b"mine").unwrap();
    let suggest = Command::new(KIT)
        .args(["suggest", dir.to_str().unwrap()])
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&suggest.stdout);
    assert!(suggest.status.success(), "{report}");
    assert!(
        report.starts_with("11 clips: 10 of the person's body sounds, 1 other sounds"),
        "{report}"
    );
    assert!(report.contains("cough         0.31"), "{report}");

    let delete = |expect_gone: bool| {
        let out = Command::new(KIT)
            .args(["delete", dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            !dir.exists(),
            expect_gone,
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    };
    delete(false);
    let left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(left, ["notes.txt"]);
    fs::remove_file(dir.join("notes.txt")).unwrap();
    delete(true);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_folder_that_is_not_absolute_is_refused() {
    for args in [["delete", "relative"], ["suggest", "/"]] {
        let out = Command::new(KIT).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

fn tempfile_dir(what: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("/tmp").join(format!(
        "body-kit-{what}-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_us()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
