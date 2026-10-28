use super::*;
use crate::judge::UnitState;
use std::os::unix::fs::symlink;

/// What a round asked systemd, in order.
#[derive(Default)]
struct Fake {
    asked: Vec<&'static str>,
    show: Option<String>,
    start: Option<Result<(), String>>,
}

impl Systemd for Fake {
    fn states(&mut self, _: &[String]) -> Result<Vec<UnitState>, String> {
        unreachable!("the writer's unit asks no unit states")
    }
    fn self_recovery_ready(&mut self) -> Result<(), String> {
        unreachable!()
    }
    fn reload(&mut self) -> Result<(), String> {
        self.asked.push("reload");
        Ok(())
    }
    fn restart(&mut self, _: &str) -> Result<(), String> {
        unreachable!("the writer's unit is started, never restarted")
    }
    fn last_line(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
    fn writer_unit_properties(&mut self) -> Result<String, String> {
        self.asked.push("show");
        self.show
            .clone()
            .ok_or_else(|| "systemctl failed".to_owned())
    }
    fn start_writer_unit(&mut self) -> Result<(), String> {
        self.asked.push("start");
        self.start.clone().unwrap_or(Ok(()))
    }
}

/// A systemd that knows only the traits' defaults for the writer's unit.
struct Older(Vec<&'static str>);

impl Systemd for Older {
    fn states(&mut self, _: &[String]) -> Result<Vec<UnitState>, String> {
        unreachable!()
    }
    fn self_recovery_ready(&mut self) -> Result<(), String> {
        unreachable!()
    }
    fn reload(&mut self) -> Result<(), String> {
        self.0.push("reload");
        Ok(())
    }
    fn restart(&mut self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn last_line(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
}

const GOOD: &str = "Id=lamp-update-writer.service\n\
    LoadState=loaded\n\
    FragmentPath=/etc/systemd/system/lamp-update-writer.service\n\
    DropInPaths=\n\
    UnitFileState=static\n";

fn lamp() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(FOLDER)).unwrap();
    root
}

fn good() -> Fake {
    Fake {
        show: Some(GOOD.to_owned()),
        ..Fake::default()
    }
}

#[test]
fn the_unit_is_written_systemd_reloaded_its_load_checked_and_then_started() {
    let root = lamp();
    let mut systemd = good();
    run(root.path(), &mut systemd).unwrap();
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    let path = root.path().join(FOLDER).join(UNIT);
    assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o644
    );

    // Written already: no reload, the same check, started again.
    let mut systemd = good();
    run(root.path(), &mut systemd).unwrap();
    assert_eq!(systemd.asked, ["show", "start"]);
}

#[test]
fn another_text_at_the_name_is_replaced_and_systemd_reloaded() {
    let root = lamp();
    let path = root.path().join(FOLDER).join(UNIT);
    fs::write(&path, "[Service]\nExecStart=/bin/true\n").unwrap();
    let mut systemd = good();
    run(root.path(), &mut systemd).unwrap();
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
}

#[test]
fn a_masked_or_linked_unit_is_left_and_nothing_is_asked() {
    for target in ["/dev/null", "elsewhere.service"] {
        let root = lamp();
        let path = root.path().join(FOLDER).join(UNIT);
        symlink(target, &path).unwrap();
        let mut systemd = good();
        let why = run(root.path(), &mut systemd).unwrap_err();
        assert!(why.contains("left as it is"), "{why}");
        assert!(systemd.asked.is_empty());
        assert_eq!(fs::read_link(&path).unwrap(), Path::new(target));
    }
    let root = lamp();
    fs::create_dir(root.path().join(FOLDER).join(UNIT)).unwrap();
    let why = run(root.path(), &mut good()).unwrap_err();
    assert!(why.contains("not a regular file"), "{why}");
}

#[test]
fn any_unit_but_the_one_written_is_never_started() {
    let changed = |from: &str, to: &str| GOOD.replace(from, to);
    let cases = [
        changed(
            "DropInPaths=",
            "DropInPaths=/etc/systemd/system/lamp-update-writer.service.d/x.conf",
        ),
        changed(
            "FragmentPath=/etc/systemd/system/",
            "FragmentPath=/run/systemd/system/",
        ),
        changed("LoadState=loaded", "LoadState=masked"),
        changed("LoadState=loaded", "LoadState=not-found"),
        changed("UnitFileState=static", "UnitFileState=masked"),
        changed("UnitFileState=static", "UnitFileState=enabled"),
        changed("UnitFileState=static", "UnitFileState=generated"),
        changed("UnitFileState=static", "UnitFileState=enabled-runtime"),
        changed(
            "Id=lamp-update-writer.service",
            "Id=lamp-update-back.service",
        ),
        changed("DropInPaths=\n", ""),
        format!("{GOOD}ExecStart=/bin/sh\n"),
        format!("{GOOD}LoadState=loaded\n"),
        GOOD.replace("LoadState=loaded", "LoadState loaded"),
        String::new(),
    ];
    for show in cases {
        let root = lamp();
        let mut systemd = Fake {
            show: Some(show.clone()),
            ..Fake::default()
        };
        let why = run(root.path(), &mut systemd).unwrap_err();
        assert!(why.starts_with("the writer's unit"), "{show:?}: {why}");
        assert_eq!(systemd.asked, ["reload", "show"], "{show:?}");
    }
    // A disabled one-shot is the one written too: static and disabled are
    // both a unit with no [Install] that nothing enabled.
    assert!(loaded_as_written(&GOOD.replace("static", "disabled")).is_ok());
}

#[test]
fn a_failed_show_or_start_is_said_and_a_failed_show_starts_nothing() {
    let root = lamp();
    let mut systemd = Fake::default();
    let why = run(root.path(), &mut systemd).unwrap_err();
    assert_eq!(why, "systemctl failed");
    assert_eq!(systemd.asked, ["reload", "show"]);

    let mut systemd = Fake {
        start: Some(Err(
            "systemctl start lamp-update-writer.service: exit 1".to_owned()
        )),
        ..good()
    };
    let why = run(root.path(), &mut systemd).unwrap_err();
    assert!(why.contains("exit 1"), "{why}");
    assert_eq!(systemd.asked, ["show", "start"]);
}

#[test]
fn a_systemd_without_the_writer_methods_refuses_before_any_start() {
    let root = lamp();
    let mut systemd = Older(Vec::new());
    let why = run(root.path(), &mut systemd).unwrap_err();
    assert!(why.contains("cannot show"), "{why}");
    assert_eq!(systemd.0, ["reload"]);
    assert!(Older(Vec::new()).start_writer_unit().is_err());
}

#[test]
fn the_unit_runs_the_confirmed_updater_as_a_one_shot_that_writes_only_its_folders() {
    let lines: Vec<_> = TEXT
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    let value = |key: &str| {
        let found: Vec<_> = lines
            .iter()
            .filter_map(|line| line.strip_prefix(&format!("{key}=")))
            .collect();
        assert_eq!(found.len(), 1, "{key} once");
        found[0]
    };
    assert_eq!(
        value("ExecStart"),
        "/opt/lampOS/update/current/lamp-update writer-refresh"
    );
    assert_eq!(value("Type"), "oneshot");
    assert_eq!(value("User"), "root");
    assert_eq!(value("ProtectSystem"), "strict");
    assert_eq!(value("NoNewPrivileges"), "yes");
    assert_eq!(value("PrivateNetwork"), "yes");
    assert_eq!(
        value("ReadWritePaths"),
        "/usr/local/lib/lampos /var/lib/lampos-update"
    );
    assert!(
        !TEXT.contains("[Install]"),
        "never enabled: a round starts it"
    );
    assert!(
        !TEXT.contains("/etc/lampOS/update ") && value("ReadOnlyPaths") == "-/etc/lampOS/update",
        "the update key stays read-only"
    );
}
