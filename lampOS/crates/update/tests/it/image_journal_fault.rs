//! A fault's last lines, kept on the eMMC past a power cut (#8713): a lamp
//! is unplugged and never shut down, so the kept boots (`keep`, at a clean
//! shutdown only) miss the boot a part crashed in. The voice's drop-in
//! `lamp-fault-keep.conf` runs `lamp-journal-keep fault UNIT` at each stop;
//! these run it on folders shaped as the lamp's, with a `journalctl` that
//! prints a part's lines, and pin the drop-in.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const UNIT: &str = "lamp-voice.service";

fn rootfs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../robots/lamp/rootfs")
}

/// The lamp's folders for one test, and a `bin` whose `journalctl` prints
/// `lines` lines of `width` bytes and notes what it was asked, whose
/// `timeout` runs its command (the Mac has none), and whose `systemctl`,
/// `pkill` and `killall` fail the run if called.
struct Lamp {
    _root: tempfile::TempDir,
    keep: PathBuf,
    run: PathBuf,
    boot: PathBuf,
    bin: PathBuf,
    asked: PathBuf,
    called: PathBuf,
}

impl Lamp {
    fn new(lines: usize, width: usize) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let at = |relative: &str| root.path().join(relative);
        let lamp = Self {
            keep: at("var/lib/lamp-journal"),
            run: at("run/log/journal"),
            boot: at("boot_id"),
            bin: at("bin"),
            asked: at("asked"),
            called: at("called"),
            _root: root,
        };
        fs::create_dir_all(&lamp.bin).unwrap();
        fs::write(&lamp.boot, "boot-1\n").unwrap();
        let mut tools = vec![
            (
                "journalctl".to_owned(),
                format!(
                    "#!/bin/sh\necho \"$*\" >>'{}'\ni=0\nwhile [ $i -lt {lines} ]; do printf '%0{width}d\\n' $i; i=$((i + 1)); done\n",
                    lamp.asked.display(),
                    width = width.max(1) - 1,
                ),
            ),
            ("timeout".to_owned(), "#!/bin/sh\nshift\nexec \"$@\"\n".to_owned()),
        ];
        for tool in ["systemctl", "pkill", "killall"] {
            tools.push((
                tool.to_owned(),
                format!(
                    "#!/bin/sh\necho \"{tool} $*\" >>'{}'\nexit 1\n",
                    lamp.called.display()
                ),
            ));
        }
        for (tool, text) in tools {
            let path = lamp.bin.join(tool);
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        lamp
    }

    /// One stop of `UNIT` with systemd's `$SERVICE_RESULT` (none: `None`),
    /// as its `ExecStopPost=` runs it. What the script said.
    fn stop(&self, result: Option<&str>) -> String {
        self.step(&["fault", UNIT], result)
    }

    fn step(&self, args: &[&str], result: Option<&str>) -> String {
        let mut command = Command::new("bash");
        command
            .arg(rootfs().join("usr/local/libexec/lamp-journal-keep"))
            .args(args)
            .env(
                "PATH",
                format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("LAMP_JOURNAL_KEEP", &self.keep)
            .env("LAMP_JOURNAL_RUN", &self.run)
            .env("LAMP_JOURNAL_BOOT_ID", &self.boot)
            .env("LAMP_JOURNAL_FROM", self.keep.join("no-journal"))
            .env("LAMP_JOURNAL_SETUP_DONE", self.keep.join("no-setup-done"))
            .env_remove("SERVICE_RESULT");
        if let Some(result) = result {
            command.env("SERVICE_RESULT", result);
        }
        let out = command.output().unwrap();
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

    /// The fault's kept files, by name.
    fn kept(&self) -> Vec<String> {
        names(&self.keep.join("fault"))
    }

    /// What `journalctl` was asked, one line a call.
    fn asked(&self) -> Vec<String> {
        fs::read_to_string(&self.asked)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The voice's drop-in: the fault keep at every stop, as root past the
/// unit's sandbox, never failing the stop; a drop-in of its own, and no
/// exit-note.conf of the image's names it.
#[test]
fn the_voice_runs_the_fault_keep_at_its_stop_from_a_drop_in_of_its_own() {
    let text =
        fs::read_to_string(rootfs().join("etc/systemd/system/lamp-voice.service.d/lamp-fault-keep.conf"))
            .unwrap();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        lines,
        [
            "[Service]",
            "ExecStopPost=-+/bin/bash /usr/local/libexec/lamp-journal-keep fault %n",
        ]
    );
    let product = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product/exit-note.conf");
    assert!(!fs::read_to_string(product).unwrap().contains("lamp-journal-keep"));
}

/// A success, or a stop with no result, keeps nothing and asks the journal
/// nothing; a fault keeps the unit's last 300 lines of this boot, whole, and
/// says where.
#[test]
fn a_fault_keeps_the_units_last_lines_and_a_success_nothing() {
    let lamp = Lamp::new(300, 40);
    for result in [None, Some("success")] {
        lamp.stop(result);
    }
    assert_eq!(lamp.kept(), Vec::<String>::new());
    assert_eq!(lamp.asked(), Vec::<String>::new());
    let said = lamp.stop(Some("signal"));
    let name = format!("boot-1.1.{UNIT}.log");
    assert_eq!(lamp.kept(), [name.clone()]);
    assert!(said.contains(&name), "{said}");
    assert_eq!(
        lamp.asked(),
        [format!("-u {UNIT} -b -n 300 -o short-iso --no-pager")]
    );
    let kept = fs::read_to_string(lamp.keep.join("fault").join(&name)).unwrap();
    assert_eq!(kept.lines().count(), 300);
    // Each result systemd gives a stop that isn't a success.
    for result in ["exit-code", "core-dump", "watchdog"] {
        lamp.stop(Some(result));
    }
    assert_eq!(lamp.kept().len(), 4);
}

/// The eMMC's bounds in a crash loop: at most 5 files a boot, said once the
/// cap is met, and 64 KB a file whatever the lines' length; the next boot
/// keeps again.
#[test]
fn a_crash_loop_stops_at_5_files_a_boot_of_64_kb_each() {
    let lamp = Lamp::new(300, 1000);
    for _ in 0..5 {
        lamp.stop(Some("signal"));
    }
    let said = lamp.stop(Some("signal"));
    assert!(said.contains("this boot has kept 5"), "{said}");
    let kept = lamp.kept();
    assert_eq!(kept.len(), 5);
    for name in &kept {
        assert_eq!(
            fs::metadata(lamp.keep.join("fault").join(name)).unwrap().len(),
            65536
        );
    }
    fs::write(&lamp.boot, "boot-2\n").unwrap();
    lamp.stop(Some("signal"));
    assert_eq!(lamp.kept().len(), 6);
    assert!(lamp.kept().contains(&format!("boot-2.1.{UNIT}.log")));
}

/// The owner's "forget everything" (or a reset) leaves `.forgotten-boot`
/// with this boot's id: no fault of that boot is kept after it; a mark of
/// an earlier boot keeps nothing from the next.
#[test]
fn a_boot_the_owner_forgot_keeps_no_fault() {
    let lamp = Lamp::new(10, 10);
    fs::create_dir_all(&lamp.keep).unwrap();
    fs::write(lamp.keep.join(".forgotten-boot"), "boot-1").unwrap();
    lamp.stop(Some("signal"));
    assert_eq!(lamp.kept(), Vec::<String>::new());
    assert_eq!(lamp.asked(), Vec::<String>::new());
    fs::write(&lamp.boot, "boot-2\n").unwrap();
    lamp.stop(Some("signal"));
    assert_eq!(lamp.kept(), [format!("boot-2.1.{UNIT}.log")]);
}

/// A journal that gives nothing keeps no empty file.
#[test]
fn nothing_read_keeps_nothing() {
    let lamp = Lamp::new(0, 10);
    lamp.stop(Some("signal"));
    assert_eq!(lamp.kept(), Vec::<String>::new());
}

/// No fault's file is kept past 72 hours: the boot's `link` and every
/// `setup` run delete any older and say so, as for the kept boots. GNU
/// `stat -c`, as on the lamp: Linux only.
#[cfg(target_os = "linux")]
#[test]
fn a_faults_lines_are_kept_72_hours_at_most() {
    use std::time::{Duration, SystemTime};
    let lamp = Lamp::new(10, 10);
    lamp.stop(Some("signal"));
    let file = lamp.keep.join("fault").join(format!("boot-1.1.{UNIT}.log"));
    let age = |hours: u64| {
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(hours * 3600))
            .unwrap();
    };
    age(71);
    lamp.step(&["setup"], None);
    assert!(file.exists());
    age(72);
    let said = lamp.step(&["link"], None);
    assert!(said.contains("deleted after 72 hours"), "{said}");
    assert!(!file.exists());
}
