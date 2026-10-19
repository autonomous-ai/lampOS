//! The image's copy-back of the logs after the night's logrotate (#1178):
//! the rotated logs come back to the RAM disk, the journal journald has
//! open never does.
use std::{fs, path::PathBuf, process::Command};

fn rootfs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../robots/lamp/rootfs")
}

/// The drop-in clears the base's copy-back and runs ours instead, with bash.
#[test]
fn logrotate_runs_the_lamps_copy_back_in_place_of_the_bases() {
    let text = fs::read_to_string(
        rootfs().join("etc/systemd/system/logrotate.service.d/zz-lamp-ramlog.conf"),
    )
    .unwrap();
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        lines,
        [
            "[Service]",
            "ExecStartPost=",
            "ExecStartPost=/bin/bash /usr/local/libexec/lamp-ramlog-postrotate",
        ]
    );
    assert!(
        rootfs()
            .join("usr/local/libexec/lamp-ramlog-postrotate")
            .is_file()
    );
}

/// Run on a copy shaped as the lamp's: the text logs come back as rotated,
/// the journal's files stay as journald wrote them, and compressed,
/// numbered and orangepi-ramlog's own files stay out, as the base has it.
/// Off when the defaults say the RAM disk is off.
#[test]
fn the_copy_back_leaves_the_journal_alone() {
    let root = std::env::temp_dir().join(format!("lamp-ramlog-{}", std::process::id()));
    let (hdd, ram) = (root.join("hdd"), root.join("ram"));
    let journal = "journal/0123456789abcdef/system.journal";
    for side in [&hdd, &ram] {
        fs::create_dir_all(side.join("journal/0123456789abcdef")).unwrap();
        fs::create_dir_all(side.join("apt")).unwrap();
    }
    for (name, text) in [
        ("syslog", "rotated"),
        ("apt/history.log", "rotated"),
        ("syslog.1", "old"),
        ("dpkg.log.2.gz", "old"),
        ("orangepi-ramlog.log", "its own"),
        (journal, "the copy from before"),
        (
            "journal/0123456789abcdef/user-1000.journal",
            "the copy from before",
        ),
    ] {
        fs::write(hdd.join(name), text).unwrap();
    }
    for name in [
        "syslog",
        "apt/history.log",
        journal,
        "journal/0123456789abcdef/user-1000.journal",
    ] {
        fs::write(ram.join(name), "live").unwrap();
    }
    let defaults = root.join("orangepi-ramlog");
    let run = |enabled: bool| {
        fs::write(
            &defaults,
            format!("ENABLED={enabled}\nSIZE=50M\nXTRA_RSYNC_FROM=()\n"),
        )
        .unwrap();
        let out = Command::new("bash")
            .arg(rootfs().join("usr/local/libexec/lamp-ramlog-postrotate"))
            .env("LAMP_RAMLOG_DEFAULTS", &defaults)
            .env("LAMP_RAMLOG_HDD", &hdd)
            .env("LAMP_RAMLOG_RAM", &ram)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let read = |name: &str| fs::read_to_string(ram.join(name)).ok();
    run(false);
    assert_eq!(
        read("syslog").as_deref(),
        Some("live"),
        "off: nothing copied"
    );
    run(true);
    assert_eq!(read("syslog").as_deref(), Some("rotated"));
    assert_eq!(read("apt/history.log").as_deref(), Some("rotated"));
    assert_eq!(read(journal).as_deref(), Some("live"));
    assert_eq!(
        read("journal/0123456789abcdef/user-1000.journal").as_deref(),
        Some("live")
    );
    for name in ["syslog.1", "dpkg.log.2.gz", "orangepi-ramlog.log"] {
        assert_eq!(read(name), None, "{name}");
    }
    fs::remove_dir_all(&root).unwrap();
}
