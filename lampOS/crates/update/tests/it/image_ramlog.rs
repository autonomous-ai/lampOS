//! The image's copy-back of the logs after the night's logrotate (#1178):
//! the rotated logs come back to the RAM disk, the journal journald has
//! open never does. And ramlog's own copies (#2602): the boot's copy to the
//! RAM disk and each copy to the eMMC leave the journal out.
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

/// The image's `/etc/default/orangepi-ramlog`, as a lamp reads it.
fn defaults() -> PathBuf {
    rootfs().join("etc/default/orangepi-ramlog")
}

/// Its settings, each once: the base's own (on, a 50M disk, rsync), so
/// orangepi-zram-config's `awk` and the scripts that source it read the
/// same, and the two copies' extra options.
#[test]
fn the_defaults_keep_the_bases_settings() {
    let text = fs::read_to_string(defaults()).unwrap();
    let mut keys = std::collections::BTreeMap::new();
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (key, value) = line.split_once('=').expect("key=value");
        assert!(keys.insert(key, value).is_none(), "{key} twice");
    }
    assert_eq!(
        keys,
        [
            ("ENABLED", "true"),
            ("SIZE", "50M"),
            ("USE_RSYNC", "true"),
            ("XTRA_RSYNC_TO", "(--filter='hide /journal/*')"),
            ("XTRA_RSYNC_FROM", "(--exclude='/journal/*')"),
        ]
        .into()
    );
}

/// The base's two copies (`/usr/lib/orangepi/orangepi-ramlog` on lamp-06c1,
/// its `syncFromDisk` and `syncToDisk` with rsync), run with the image's
/// defaults on copies shaped as the lamp's. At boot the journal's folder
/// comes to the RAM disk but none of its files; a copy to the eMMC copies
/// the text logs, never the journal, and removes the eMMC's old journal.
/// Skipped where there is no GNU rsync (macOS has openrsync).
#[test]
fn ramlogs_copies_leave_the_journal_out() {
    let version = Command::new("rsync").arg("--version").output();
    if !version.is_ok_and(|out| String::from_utf8_lossy(&out.stdout).starts_with("rsync  version"))
    {
        eprintln!("no GNU rsync here: skipped");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let (hdd, ram) = (root.path().join("hdd"), root.path().join("ram"));
    let id = "journal/0123456789abcdef";
    fs::create_dir_all(hdd.join(id)).unwrap();
    fs::create_dir_all(&ram).unwrap();
    for (name, text) in [
        ("syslog", "from before"),
        ("orangepi-ramlog.log", "its own"),
        ("journal/0123456789abcdef/system.journal", "an earlier boot"),
        (
            "journal/0123456789abcdef/user-1000.journal",
            "an earlier boot",
        ),
    ] {
        fs::write(hdd.join(name), text).unwrap();
    }
    let copy = |script: &str| {
        let out = Command::new("bash")
            .arg("-c")
            .arg(format!(". \"$0\"; {script}"))
            .arg(defaults())
            .env("HDD_LOG", format!("{}/", hdd.display()))
            .env("RAM_LOG", format!("{}/", ram.display()))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    // syncFromDisk, as the base runs it.
    copy(
        "rsync -aXWv --delete --exclude \"lost+found\" --exclude orangepi-ramlog.log \
         --exclude '*.gz' --exclude '*.xz' --exclude='*.[0-9]' --links \
         ${XTRA_RSYNC_FROM[@]+\"${XTRA_RSYNC_FROM[@]}\"} \"$HDD_LOG\" \"$RAM_LOG\"",
    );
    assert_eq!(
        fs::read_to_string(ram.join("syslog")).unwrap(),
        "from before"
    );
    assert!(
        ram.join("journal").is_dir(),
        "journald keeps using /var/log"
    );
    assert_eq!(fs::read_dir(ram.join("journal")).unwrap().count(), 0);
    // This boot's journal, then syncToDisk, as the base runs it.
    fs::create_dir_all(ram.join("journal/fedcba9876543210")).unwrap();
    fs::write(ram.join("journal/fedcba9876543210/system.journal"), "live").unwrap();
    fs::write(ram.join("syslog"), "today").unwrap();
    copy(
        "rsync -aXWv --exclude \"lost+found\" --exclude orangepi-ramlog.log --delete --links \
         ${XTRA_RSYNC_TO[@]+\"${XTRA_RSYNC_TO[@]}\"} \"$RAM_LOG\" \"$HDD_LOG\"",
    );
    assert_eq!(fs::read_to_string(hdd.join("syslog")).unwrap(), "today");
    assert_eq!(
        fs::read_to_string(hdd.join("orangepi-ramlog.log")).unwrap(),
        "its own"
    );
    assert!(hdd.join("journal").is_dir());
    assert_eq!(fs::read_dir(hdd.join("journal")).unwrap().count(), 0);
    assert_eq!(
        fs::read_to_string(ram.join("journal/fedcba9876543210/system.journal")).unwrap(),
        "live"
    );
}

/// The drop-in on the base's ramlog unit: the keep runs before the base's
/// own stop (which unmounts the RAM disk), the links at boot, and neither
/// may fail ramlog.
#[test]
fn ramlogs_stop_keeps_the_journal_before_its_own_stop() {
    let text = fs::read_to_string(
        rootfs().join("etc/systemd/system/orangepi-ramlog.service.d/zz-lamp-journal-keep.conf"),
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
            "ExecStartPost=-/bin/bash /usr/local/libexec/lamp-journal-keep link",
            "ExecStop=",
            "ExecStop=-/bin/bash /usr/local/libexec/lamp-journal-keep keep",
            "ExecStop=/usr/lib/orangepi/orangepi-ramlog stop",
        ]
    );
}

/// Runs the keep script as ramlog's stop or start would, on folders shaped
/// as the lamp's.
fn journal_keep(
    step: &str,
    state: &str,
    from: &std::path::Path,
    keep: &std::path::Path,
    run: &std::path::Path,
) {
    let out = Command::new("bash")
        .arg(rootfs().join("usr/local/libexec/lamp-journal-keep"))
        .arg(step)
        .env("LAMP_JOURNAL_STATE", state)
        .env("LAMP_JOURNAL_FROM", from)
        .env("LAMP_JOURNAL_KEEP", keep)
        .env("LAMP_JOURNAL_RUN", run)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One boot's journal folder: closed files of these sizes, oldest first,
/// and the open ones journald starts after the rotation.
fn a_boot(from: &std::path::Path, boot: &str, sizes: &[usize]) {
    let _ = fs::remove_dir_all(from);
    fs::create_dir_all(from).unwrap();
    let start = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for (n, size) in sizes.iter().enumerate() {
        let path = from.join(format!("system@{boot}-{n:016x}-0000000000000000.journal"));
        fs::write(&path, vec![b'j'; *size]).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(start + std::time::Duration::from_secs(60 * n as u64))
            .unwrap();
    }
    fs::write(from.join("system.journal"), "open").unwrap();
    fs::write(from.join("user-1000.journal"), "open").unwrap();
}

/// What a kept boot holds, by name.
fn kept(keep: &std::path::Path, slot: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(keep.join(slot))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A clean shutdown or reboot only ("stopping"), each boot's newest closed
/// files up to 5 MB without a gap, the last 2 boots, so 10 MB at most; and
/// at boot each kept file is linked where journalctl reads, with a name no
/// vacuum removes.
#[test]
fn the_last_two_boots_are_kept_at_shutdown_only() {
    let root = tempfile::tempdir().unwrap();
    let (from, keep, run) = (
        root.path().join("var/log/journal/0123456789abcdef"),
        root.path().join("var/lib/lamp-journal"),
        root.path().join("run/log/journal"),
    );
    let mb = 1_000_000;
    // Not at a `systemctl stop`, nor while running.
    a_boot(&from, "a", &[mb]);
    for state in ["running", "degraded", ""] {
        journal_keep("keep", state, &from, &keep, &run);
    }
    assert!(!keep.exists());
    // First shutdown: the newest 1.5 MB files that fit in 5 MB, newest first.
    a_boot(&from, "a", &[mb * 3 / 2; 5]);
    journal_keep("keep", "stopping", &from, &keep, &run);
    assert_eq!(
        kept(&keep, "last"),
        (2..5)
            .map(|n| format!("system@a-{n:016x}-0000000000000000.journal"))
            .collect::<Vec<_>>()
    );
    assert!(!keep.join("previous").exists());
    // The next: a 4 MB file after a 2 MB one keeps the 4 MB one only.
    a_boot(&from, "b", &[mb / 2, 2 * mb, 4 * mb]);
    journal_keep("keep", "stopping", &from, &keep, &run);
    assert_eq!(
        kept(&keep, "last"),
        ["system@b-0000000000000002-0000000000000000.journal"]
    );
    assert_eq!(kept(&keep, "previous").len(), 3);
    // A third: the first boot is gone, 2 boots, at most 10 MB.
    a_boot(&from, "c", &[5 * mb]);
    journal_keep("keep", "stopping", &from, &keep, &run);
    assert_eq!(
        kept(&keep, "last"),
        ["system@c-0000000000000000-0000000000000000.journal"]
    );
    assert_eq!(
        kept(&keep, "previous"),
        ["system@b-0000000000000002-0000000000000000.journal"]
    );
    let names: Vec<_> = fs::read_dir(&keep)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let total: u64 = ["last", "previous"]
        .iter()
        .flat_map(|slot| fs::read_dir(keep.join(slot)).unwrap())
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert!(total <= 10 * mb as u64, "{total}");
    // A boot with nothing closed leaves the kept ones as they are.
    a_boot(&from, "d", &[]);
    journal_keep("keep", "stopping", &from, &keep, &run);
    assert_eq!(kept(&keep, "previous").len(), 1);
    assert!(kept(&keep, "last")[0].starts_with("system@c-"));
    // At boot: a link for each kept file, none with "@", an old link gone.
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("lamp-kept-last-gone.journal"), "").unwrap();
    journal_keep("link", "", &from, &keep, &run);
    let mut links: Vec<(String, PathBuf)> = fs::read_dir(&run)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read_link(entry.path()).unwrap(),
            )
        })
        .collect();
    links.sort();
    assert_eq!(
        links,
        [
            (
                "lamp-kept-last-system.c-0000000000000000-0000000000000000.journal".to_owned(),
                keep.join("last/system@c-0000000000000000-0000000000000000.journal")
            ),
            (
                "lamp-kept-previous-system.b-0000000000000002-0000000000000000.journal".to_owned(),
                keep.join("previous/system@b-0000000000000002-0000000000000000.journal")
            ),
        ]
    );
}
