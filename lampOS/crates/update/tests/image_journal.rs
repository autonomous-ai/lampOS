//! The image's journald drop-in (#1178): the lamp's journal kept well inside
//! its RAM disk, read as systemd reads it. Every lamp's image gets it from
//! `robots/lamp/rootfs`, whatever its profile (no override has one).
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const DROP_IN: &str = "etc/systemd/journald.conf.d/zz-lamp-journal.conf";
/// The image's `/var/log` RAM disk (`orangepi-ramlog`'s `SIZE`, as on
/// lamp-75c9's image as installed).
const RAM_DISK: u64 = 50 << 20;

fn image_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../robots/lamp/rootfs")
        .join(DROP_IN)
}

/// journald's sizes: a number with an optional K, M, G or T (1024 each).
fn bytes(value: &str) -> u64 {
    let (number, unit) = value.split_at(value.trim_end_matches(char::is_alphabetic).len());
    let shift = match unit {
        "" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        other => panic!("{value}: unit {other} is not journald's"),
    };
    number
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("{value} is no size"))
        << shift
}

/// Its `[Journal]` keys, each once, and only keys journald knows.
fn keys(text: &str) -> BTreeMap<String, String> {
    let mut section = None;
    let mut keys = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            section = Some(line.to_owned());
            continue;
        }
        assert_eq!(section.as_deref(), Some("[Journal]"), "{line}");
        let (key, value) = line.split_once('=').expect("key=value");
        assert!(
            [
                "SystemMaxUse",
                "SystemMaxFileSize",
                "SystemKeepFree",
                "RuntimeMaxUse",
                "RuntimeMaxFileSize",
            ]
            .contains(&key),
            "{key} is not one this drop-in means to set"
        );
        assert!(
            keys.insert(key.to_owned(), value.to_owned()).is_none(),
            "{key} twice"
        );
    }
    keys
}

#[test]
fn the_journal_is_kept_well_inside_the_ram_disk() {
    let keys = keys(&std::fs::read_to_string(image_file()).unwrap());
    let size = |key: &str| bytes(&keys[key]);
    // The journal and the space it leaves free fit the disk, with room for
    // the other logs beside it.
    assert!(size("SystemMaxUse") + size("SystemKeepFree") <= RAM_DISK * 4 / 5);
    // Files small enough that a night's copy of the active one is small, and
    // several of them, so a vacuum drops old ones rather than the whole.
    assert!(size("SystemMaxFileSize") * 4 <= size("SystemMaxUse"));
    // A journal kept in /run (another RAM disk, before /var/log is up or
    // with Storage=volatile) is never larger than the one on /var/log.
    assert!(size("RuntimeMaxUse") <= size("SystemMaxUse"));
    assert!(size("RuntimeMaxFileSize") * 4 <= size("RuntimeMaxUse"));
}

/// As systemd reads it: beside a drop-in that allows more (the Pi image's
/// `persistent.conf`, 100M), this one is read last, so its numbers win.
/// Skipped where there is no `systemd-analyze` (macOS).
#[test]
fn systemd_reads_it_last_beside_another_drop_in() {
    let Ok(version) = Command::new("systemd-analyze").arg("--version").output() else {
        eprintln!("no systemd-analyze here: skipped");
        return;
    };
    assert!(version.status.success());
    let root = std::env::temp_dir().join(format!("lamp-journal-{}", std::process::id()));
    let dir = root.join("etc/systemd/journald.conf.d");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(image_file(), root.join(DROP_IN)).unwrap();
    std::fs::write(
        dir.join("persistent.conf"),
        "[Journal]\nStorage=persistent\nSystemMaxUse=100M\n",
    )
    .unwrap();
    let read = Command::new("systemd-analyze")
        .arg(format!("--root={}", root.display()))
        .args(["cat-config", "systemd/journald.conf"])
        .output()
        .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let text = String::from_utf8(read.stdout).unwrap();
    let last = |key: &str| {
        text.lines()
            .filter_map(|line| line.strip_prefix(&format!("{key}=")))
            .next_back()
            .map(str::to_owned)
    };
    let ours = keys(&std::fs::read_to_string(image_file()).unwrap());
    for (key, value) in &ours {
        assert_eq!(last(key).as_ref(), Some(value), "{key}\n{text}");
    }
}
