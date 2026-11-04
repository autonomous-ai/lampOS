//! A first-time setup's journal, kept on the eMMC past the 20M cap
//! (#5144): a tester's setup on lamp-0c4e (3 Nov) was gone from the journal
//! within 4 hours, so its failed face step couldn't be read back. The
//! image's timer runs `lamp-journal-keep setup` every 3 minutes; these run
//! it on folders shaped as the lamp's, and pin the units that run it.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

const MB: usize = 1_000_000;

fn rootfs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../robots/lamp/rootfs")
}

fn script() -> PathBuf {
    rootfs().join("usr/local/libexec/lamp-journal-keep")
}

/// A unit's lines, without comments or blank lines.
fn lines(relative: &str) -> Vec<String> {
    fs::read_to_string(rootfs().join(relative))
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// The lamp's folders for one test, and a `bin` whose `journalctl`,
/// `systemctl` and `pkill` fail the run if called: the setup keep never
/// asks journald for anything, nor restarts or signals it.
struct Lamp {
    _root: tempfile::TempDir,
    from: PathBuf,
    keep: PathBuf,
    run: PathBuf,
    done: PathBuf,
    progress: PathBuf,
    bin: PathBuf,
    called: PathBuf,
}

impl Lamp {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let at = |relative: &str| root.path().join(relative);
        let lamp = Self {
            from: at("var/log/journal/0123456789abcdef"),
            keep: at("var/lib/lamp-journal"),
            run: at("run/log/journal"),
            done: at("var/lib/lampos-setup/done"),
            progress: at("var/lib/lampOS/voice/onboarding/progress.json"),
            bin: at("bin"),
            called: at("called"),
            _root: root,
        };
        fs::create_dir_all(&lamp.from).unwrap();
        fs::create_dir_all(&lamp.bin).unwrap();
        for tool in ["journalctl", "systemctl", "pkill", "killall"] {
            let path = lamp.bin.join(tool);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\necho \"{tool} $*\" >>'{}'\nexit 1\n",
                    lamp.called.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        lamp
    }

    /// Setup's end, as `lamp-setup` marks it: the walk may begin.
    fn set_up(&self) {
        fs::create_dir_all(self.done.parent().unwrap()).unwrap();
        fs::write(&self.done, "").unwrap();
    }

    /// The voice's onboarding file with these steps' outcomes, written
    /// compact as the voice writes it.
    fn walked(&self, steps: &[&str]) {
        let steps: serde_json::Map<String, serde_json::Value> = steps
            .iter()
            .map(|step| {
                (
                    (*step).to_owned(),
                    serde_json::json!({"outcome":"done","at_unix_s":1}),
                )
            })
            .collect();
        fs::create_dir_all(self.progress.parent().unwrap()).unwrap();
        fs::write(
            &self.progress,
            serde_json::to_vec(&serde_json::json!({"steps": steps, "tried": ["who_you_are"]}))
                .unwrap(),
        )
        .unwrap();
    }

    /// One closed journal file of `size` bytes, closed `at_s` seconds from
    /// now (an archived name has "@").
    fn closed(&self, n: u64, size: usize, at_s: i64) -> String {
        let name = format!("system@0123-{n:016x}-0000000000000000.journal");
        let path = self.from.join(&name);
        fs::write(&path, vec![b'j'; size]).unwrap();
        let at = if at_s >= 0 {
            SystemTime::now() + Duration::from_secs(at_s as u64)
        } else {
            SystemTime::now() - Duration::from_secs((-at_s) as u64)
        };
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
        fs::write(self.from.join("system.journal"), "open").unwrap();
        name
    }

    /// One run of the timer's service, as the lamp runs it.
    fn tick(&self) -> String {
        self.step("setup")
    }

    fn step(&self, step: &str) -> String {
        let out = Command::new("bash")
            .arg(script())
            .arg(step)
            .env(
                "PATH",
                format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("LAMP_JOURNAL_FROM", &self.from)
            .env("LAMP_JOURNAL_KEEP", &self.keep)
            .env("LAMP_JOURNAL_RUN", &self.run)
            .env("LAMP_JOURNAL_SETUP_DONE", &self.done)
            .env("LAMP_JOURNAL_PROGRESS", &self.progress)
            .env("LAMP_JOURNAL_STATE", "running")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !self.called.exists(),
            "{}",
            fs::read_to_string(&self.called).unwrap_or_default()
        );
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    /// The setup's kept files, by name.
    fn kept(&self) -> Vec<String> {
        names(&self.keep.join("setup"), |name| !name.starts_with('.'))
    }

    /// The setup's links where journalctl reads, each to its kept file.
    fn links(&self) -> Vec<(String, PathBuf)> {
        let mut links: Vec<(String, PathBuf)> = fs::read_dir(&self.run)
            .map(|entries| {
                entries
                    .map(|entry| {
                        let entry = entry.unwrap();
                        (
                            entry.file_name().to_string_lossy().into_owned(),
                            fs::read_link(entry.path()).unwrap(),
                        )
                    })
                    .filter(|(name, _)| name.starts_with("lamp-kept-setup-"))
                    .collect()
            })
            .unwrap_or_default();
        links.sort();
        links
    }
}

fn names(folder: &Path, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| keep(name))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The script's steps are the walk's, as lamp-update's `walk.rs` reads it.
#[test]
fn the_setup_keep_knows_the_walks_steps() {
    let text = fs::read_to_string(script()).unwrap();
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("steps=\""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("a steps= line");
    assert_eq!(
        line.split(' ').collect::<Vec<_>>(),
        lamp_msgs::ONBOARDING_STEPS
    );
}

/// The units: the timer every 3 minutes from a minute after boot, with no
/// enable of its own (ramlog's drop-in wants it), and the service a oneshot
/// of the script's `setup` that can fail nothing and names no journald.
#[test]
fn the_timer_runs_the_setup_keep_every_3_minutes_and_ramlog_wants_it() {
    assert_eq!(
        lines("etc/systemd/system/lamp-journal-setup.timer"),
        [
            "[Unit]",
            "Description=Keep a first-time setup's journal on the eMMC, every 3 minutes",
            "[Timer]",
            "OnBootSec=1min",
            "OnUnitActiveSec=3min",
            "AccuracySec=30s",
        ]
    );
    let service = lines("etc/systemd/system/lamp-journal-setup.service");
    assert!(service.contains(&"Type=oneshot".to_owned()), "{service:?}");
    assert!(
        service.contains(
            &"ExecStart=-/bin/bash /usr/local/libexec/lamp-journal-keep setup".to_owned()
        ),
        "{service:?}"
    );
    assert!(!service.iter().any(|line| line.contains("journald")));
    assert!(!service.iter().any(|line| line.starts_with("[Install]")));
    let ramlog = lines("etc/systemd/system/orangepi-ramlog.service.d/zz-lamp-journal-keep.conf");
    assert_eq!(ramlog[..2], ["[Unit]", "Wants=lamp-journal-setup.timer"]);
    // Where systemd-analyze is, it reads both units without a word.
    if Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
    {
        let out = Command::new("systemd-analyze")
            .args(["verify", "--man=no"])
            .arg(rootfs().join("etc/systemd/system/lamp-journal-setup.timer"))
            .arg(rootfs().join("etc/systemd/system/lamp-journal-setup.service"))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            !said
                .lines()
                .any(|line| line.contains("lamp-journal-setup") && !line.contains("bash")),
            "{said}"
        );
    }
}

/// Before setup is done, or for a walk that ended before the keep ever
/// ran (a lamp set up on an older image), nothing is kept.
#[test]
fn nothing_is_kept_without_a_walk_under_way() {
    let lamp = Lamp::new();
    lamp.closed(1, MB, 60);
    lamp.tick();
    assert!(!lamp.keep.exists());
    lamp.set_up();
    lamp.walked(&lamp_msgs::ONBOARDING_STEPS);
    for _ in 0..30 {
        lamp.tick();
    }
    assert!(!lamp.keep.join("setup").exists());
    // An onboarding file that isn't the walk's is no walk either.
    fs::write(&lamp.progress, "not json").unwrap();
    lamp.tick();
    assert!(!lamp.keep.join("setup").exists());
}

/// While the walk is under way, each closed file is kept once, oldest
/// first, none closed before the walk was seen; each is linked where
/// journalctl reads only once its original is gone, so no line is read
/// twice; and at boot every kept file is linked.
#[test]
fn a_walk_under_way_keeps_each_closed_file_once_and_links_it_once_gone() {
    let lamp = Lamp::new();
    lamp.set_up();
    let before = lamp.closed(1, MB, -3600);
    // Closed in the run's gap before the walk was first seen: its start.
    let gap = lamp.closed(9, MB, -120);
    // No onboarding file yet: the walk's hello, before its first step.
    lamp.tick();
    assert_eq!(lamp.kept(), std::slice::from_ref(&gap));
    let first = lamp.closed(2, MB, 60);
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    let second = lamp.closed(3, 2 * MB, 120);
    lamp.tick();
    lamp.tick();
    assert_eq!(lamp.kept(), [first.clone(), second.clone(), gap.clone()]);
    assert!(!lamp.kept().contains(&before));
    assert_eq!(lamp.links(), []);
    // The journal's vacuum drops the oldest: its copy is linked.
    fs::remove_file(lamp.from.join(&first)).unwrap();
    lamp.tick();
    assert_eq!(
        lamp.links(),
        [(
            format!("lamp-kept-setup-{}", first.replace('@', ".")),
            lamp.keep.join("setup").join(&first)
        )]
    );
    // A reboot: a new boot's folder, and `link` links every kept file.
    fs::remove_dir_all(&lamp.from).unwrap();
    fs::create_dir_all(&lamp.from).unwrap();
    fs::remove_dir_all(&lamp.run).unwrap();
    lamp.step("link");
    assert_eq!(lamp.links().len(), 3);
}

/// At most 24 MB, the setup's start without a gap: at the first file that
/// doesn't fit, the keep ends and says so.
#[test]
fn the_setup_kept_is_24_mb_at_most() {
    let lamp = Lamp::new();
    lamp.set_up();
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    for n in 0..8 {
        lamp.closed(n, 4 * MB, 60 + 60 * n as i64);
    }
    let said = lamp.tick();
    assert!(said.contains("the setup's journal kept is full"), "{said}");
    assert_eq!(lamp.kept().len(), 6);
    let total: u64 = fs::read_dir(lamp.keep.join("setup"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert!(total <= 24 * MB as u64, "{total}");
    lamp.closed(9, 1000, 1000);
    lamp.tick();
    assert_eq!(lamp.kept().len(), 6);
}

/// The walk's end: kept for 20 runs (an hour at the timer's 3 minutes),
/// then the first file closed after that hour, which holds the walk's end,
/// and nothing more. A walk begun again (a redo, a reset) replaces it.
#[test]
fn the_hour_after_the_walk_is_kept_then_nothing_until_a_walk_begins_again() {
    let lamp = Lamp::new();
    lamp.set_up();
    lamp.walked(&["where_you_sit", "who_you_are"]);
    lamp.tick();
    // The walk was first seen two hours ago, as the clock moving on would.
    fs::File::options()
        .write(true)
        .open(lamp.keep.join("setup/.started"))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(7200))
        .unwrap();
    let during = lamp.closed(1, MB, -5400);
    lamp.tick();
    lamp.walked(&lamp_msgs::ONBOARDING_STEPS);
    for _ in 0..19 {
        lamp.tick();
    }
    let after = lamp.closed(2, MB, -600);
    lamp.tick();
    assert_eq!(lamp.kept(), [during.clone(), after.clone()]);
    // The 20th run marks the hour's end; the file closed after it is kept.
    let ending = lamp.closed(3, MB, 3600);
    lamp.tick();
    assert_eq!(lamp.kept(), [during.clone(), after.clone(), ending.clone()]);
    let later = lamp.closed(4, MB, 7200);
    for _ in 0..5 {
        lamp.tick();
    }
    assert!(!lamp.kept().contains(&later));
    // Still linked as its originals go, after the end.
    fs::remove_file(lamp.from.join(&during)).unwrap();
    lamp.tick();
    assert_eq!(lamp.links().len(), 1);
    // A redo, the next day: the walk is under way again, and the old setup
    // is replaced (this test's files closed "later" are its future, gone).
    for name in [&after, &ending, &later] {
        fs::remove_file(lamp.from.join(name)).unwrap();
    }
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    assert_eq!(lamp.kept(), Vec::<String>::new());
    assert_eq!(lamp.links(), []);
    let redone = lamp.closed(5, MB, 10_000);
    lamp.tick();
    assert_eq!(lamp.kept(), [redone]);
}

/// Sets a file's time `ago_s` seconds back, as the clock moving on would.
fn age(path: &Path, ago_s: u64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(ago_s))
        .unwrap();
}

/// The PM's privacy call on #5342: the setup's lines can hold what the
/// person said, so the kept copy is deleted 72 hours after the walk was
/// first seen, links and all, and says so; nothing comes back for that
/// walk. A start dated before the clock was set is dated anew instead.
#[test]
fn the_setup_kept_is_deleted_after_72_hours() {
    let lamp = Lamp::new();
    lamp.set_up();
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    let first = lamp.closed(1, MB, 60);
    lamp.tick();
    lamp.walked(&lamp_msgs::ONBOARDING_STEPS);
    fs::remove_file(lamp.from.join(&first)).unwrap();
    lamp.tick();
    assert_eq!(lamp.kept(), [first]);
    assert_eq!(lamp.links().len(), 1);
    let started = lamp.keep.join("setup/.started");
    // 71 hours on: still kept.
    age(&started, 71 * 3600);
    lamp.tick();
    assert_eq!(lamp.kept().len(), 1);
    // 72 hours on: gone, links too, and said.
    age(&started, 72 * 3600);
    let said = lamp.tick();
    assert!(said.contains("deleted after 72 hours"), "{said}");
    assert!(!lamp.keep.join("setup").exists());
    assert_eq!(lamp.links(), []);
    lamp.closed(2, MB, 120);
    for _ in 0..3 {
        lamp.tick();
    }
    assert!(!lamp.keep.join("setup").exists());
    // A new lamp's clock not set yet when the walk was first seen (1970):
    // dated anew once it is, never taken for 72 hours past.
    let lamp = Lamp::new();
    lamp.set_up();
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    let started = lamp.keep.join("setup/.started");
    fs::File::options()
        .write(true)
        .open(&started)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(3600))
        .unwrap();
    lamp.tick();
    assert!(started.exists());
    let dated = fs::metadata(&started).unwrap().modified().unwrap();
    assert!(dated > SystemTime::now() - Duration::from_secs(60));
}

/// The owner's "forget everything" (lamp-voice's `setup_journal`) leaves
/// `.forgotten` where the copies were: the keep copies nothing more of that
/// walk, even one it was copying then; a redo after that walk ends starts
/// anew.
#[test]
fn a_forgotten_setup_keeps_nothing_more_of_that_walk() {
    let lamp = Lamp::new();
    lamp.set_up();
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    let first = lamp.closed(1, MB, 60);
    lamp.tick();
    assert_eq!(lamp.kept(), std::slice::from_ref(&first));
    // What setup_journal.rs does: the mark, then the copies and links go.
    let slot = lamp.keep.join("setup");
    fs::write(slot.join(".forgotten"), "").unwrap();
    fs::remove_file(slot.join(&first)).unwrap();
    lamp.closed(2, MB, 120);
    lamp.tick();
    lamp.walked(&lamp_msgs::ONBOARDING_STEPS);
    for _ in 0..25 {
        lamp.tick();
    }
    lamp.closed(3, MB, 3600);
    lamp.tick();
    assert_eq!(lamp.kept(), Vec::<String>::new());
    assert_eq!(lamp.links(), []);
    // A redo: a new walk, kept anew.
    for n in 1..=3 {
        let _ = fs::remove_file(
            lamp.from
                .join(format!("system@0123-{n:016x}-0000000000000000.journal")),
        );
    }
    lamp.walked(&["where_you_sit"]);
    lamp.tick();
    assert!(!slot.join(".forgotten").exists());
    let redone = lamp.closed(4, MB, 7200);
    lamp.tick();
    assert_eq!(lamp.kept(), [redone]);
}
