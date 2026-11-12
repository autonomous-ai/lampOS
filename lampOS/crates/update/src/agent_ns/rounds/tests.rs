use super::*;
use crate::hermes_install::on as at;
use lamp_msgs::EngineInstall;
use std::{fs, os::unix::fs::MetadataExt};

const SHOWN: Shown = Shown {
    skills: true,
    tasks: false,
};

fn engine(state: &str) -> EngineInstall {
    EngineInstall {
        state: state.to_owned(),
        version: None,
        reason: None,
        at_unix_ms: None,
    }
}

/// Harness's look as `harness machines engines` gives it, `said` each
/// engine's state.
fn look(said: &[(&str, &str)]) -> HarnessLook {
    HarnessLook {
        machine: Some("lamp".to_owned()),
        engines: said
            .iter()
            .map(|(name, state)| ((*name).to_owned(), engine(state)))
            .collect(),
    }
}

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for dir in ["usr/local/bin", "usr/local/lib/lampos", "var/lib"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    root
}

/// The login shell finding each engine's wrapper first, as on a lamp
/// where nothing comes before `/usr/local/bin`.
fn wrapper_found(engine: &str) -> Option<String> {
    wrapper::wrapper_path(engine).ok()
}

fn me() -> Result<(u32, u32), String> {
    let me = tempfile::tempfile().unwrap().metadata().unwrap();
    Ok((me.uid(), me.gid()))
}

#[test]
fn the_switch_is_on_only_when_on_exactly() {
    assert!(on(Some("on")));
    for value in [
        None,
        Some(""),
        Some("off"),
        Some("ON"),
        Some("on "),
        Some("1"),
    ] {
        assert!(!on(value), "{value:?}");
    }
}

/// Only engines Harness says are installed, plain, and not Hermes or
/// OpenClaw (their own installers keep them until step 2); no engine is
/// named but those two.
#[test]
fn only_installed_plain_engines_without_their_own_installer_are_written() {
    let said = look(&[
        ("hermes", "installed"),
        ("openclaw", "installed"),
        ("claude-code", "installed"),
        ("codex", "installed"),
        ("made-up-engine9", "installed"),
        ("opencode", "not_installed"),
        ("gemini", "failed"),
        ("Bad_Name", "installed"),
    ]);
    assert_eq!(
        engines_to_write(&said),
        ["claude-code", "codex", "made-up-engine9"]
    );
    assert!(engines_to_write(&HarnessLook::default()).is_empty());
}

/// A round writes each engine's wrapper, the namespace script and its
/// home; the next round, with all there, writes nothing and asks no ids.
/// One changed or taken out is written again.
#[test]
fn a_round_writes_each_engine_and_again_only_when_not_there() {
    let root = root();
    let said = look(&[
        ("hermes", "installed"),
        ("openclaw", "installed"),
        ("codex", "installed"),
        ("made-up", "installed"),
        ("opencode", "not_installed"),
    ]);
    let mut asked = 0;
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut || {
            asked += 1;
            me()
        },
        &mut wrapper_found,
        &mut |line| lines.push(line.to_owned()),
    );
    assert_eq!(written, ["codex", "made-up"]);
    assert_eq!(asked, 1, "lampos's ids asked once a round");
    assert_eq!(lines.len(), 2, "{lines:?}");
    for engine in ["codex", "made-up"] {
        assert!(
            wrapper::present(root.path(), engine, SHOWN, wrapper_found(engine).as_deref()),
            "{engine}"
        );
        let home = at(root.path(), &wrapper::home_of(engine).unwrap());
        assert_eq!(fs::metadata(&home).unwrap().mode() & 0o7777, 0o700);
    }
    for engine in ["hermes", "openclaw", "opencode"] {
        assert!(
            !at(root.path(), &format!("/usr/local/bin/{engine}")).exists(),
            "{engine}"
        );
        assert!(
            !at(root.path(), &format!("/var/lib/lampos-agents/{engine}")).exists(),
            "{engine}"
        );
    }

    // All there: nothing written, no ids asked, nothing said.
    let mut asked = 0;
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut || {
            asked += 1;
            me()
        },
        &mut wrapper_found,
        &mut |line| lines.push(line.to_owned()),
    );
    assert!(written.is_empty() && lines.is_empty() && asked == 0);

    // One wrapper changed: that engine alone is written again.
    let wrapper = at(root.path(), "/usr/local/bin/codex");
    fs::write(&wrapper, format!("#!/bin/sh\n# {}\n", wrapper::MARK)).unwrap();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut me,
        &mut wrapper_found,
        &mut |_| {},
    );
    assert_eq!(written, ["codex"]);
    assert!(wrapper::present(
        root.path(),
        "codex",
        SHOWN,
        wrapper_found("codex").as_deref()
    ));

    // Another switch: the shared namespace script is not as this build
    // writes it for that; the first engine writes it again (with its own
    // wrapper), and the next then holds.
    let other = Shown {
        skills: false,
        tasks: true,
    };
    let written = after_round(
        root.path(),
        &said,
        other,
        &mut me,
        &mut wrapper_found,
        &mut |_| {},
    );
    assert_eq!(written, ["codex"]);
    assert!(wrapper::present(
        root.path(),
        "codex",
        other,
        wrapper_found("codex").as_deref()
    ));
    assert!(wrapper::present(
        root.path(),
        "made-up",
        other,
        wrapper_found("made-up").as_deref()
    ));
}

/// The engine's own program at the wrapper's place is never replaced; the
/// refusal is said, and the other engines are still written.
#[test]
fn a_file_that_is_not_lamp_updates_is_left_and_the_rest_written() {
    let root = root();
    let own = at(root.path(), "/usr/local/bin/codex");
    fs::write(&own, "#!/bin/sh\necho the real codex\n").unwrap();
    let said = look(&[("codex", "installed"), ("made-up", "installed")]);
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut me,
        &mut wrapper_found,
        &mut |line| lines.push(line.to_owned()),
    );
    assert_eq!(written, ["made-up"]);
    assert_eq!(
        fs::read_to_string(&own).unwrap(),
        "#!/bin/sh\necho the real codex\n"
    );
    assert!(
        lines.iter().any(
            |line| line.starts_with("codex: not written") && line.contains("not lamp-update's")
        ),
        "{lines:?}"
    );
}

/// No ids for `lampos`: nothing is written, and that is said once.
#[test]
fn without_lampos_ids_nothing_is_written() {
    let root = root();
    let said = look(&[("codex", "installed"), ("made-up", "installed")]);
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut || Err("id -u lampos gave no user id".to_owned()),
        &mut wrapper_found,
        &mut |line| lines.push(line.to_owned()),
    );
    assert!(written.is_empty());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(!at(root.path(), wrapper::NS).exists());
    assert!(!at(root.path(), "/usr/local/bin/codex").exists());
}

/// Hermes or OpenClaw taken out never takes the hook stand-in from the
/// engines written here: their namespace script keeps it.
#[test]
fn the_hook_stand_in_stays_while_the_shared_namespace_script_is_there() {
    let root = root();
    let said = look(&[("codex", "installed")]);
    after_round(
        root.path(),
        &said,
        SHOWN,
        &mut me,
        &mut wrapper_found,
        &mut |_| {},
    );
    assert!(crate::agent_ns::hook_shim_there(root.path()));
    crate::agent_ns::remove_hook_shim(root.path(), &crate::hermes_install::NS_SCRIPTS, &mut |_| {});
    assert!(crate::agent_ns::hook_shim_there(root.path()));
    assert!(wrapper::present(
        root.path(),
        "codex",
        SHOWN,
        wrapper_found("codex").as_deref()
    ));
}

/// An engine whose own program the login shell finds before the wrapper
/// (`~/.local/bin`), or none at all: written once, then said each round
/// and never written again for it.
#[test]
fn an_engine_found_before_the_wrapper_is_said_not_written_again() {
    let root = root();
    let said = look(&[("codex", "installed"), ("made-up", "installed")]);
    let mut finds = |engine: &str| match engine {
        "codex" => Some("/home/lampos/.local/bin/codex".to_owned()),
        _ => None,
    };
    for round in 0..2 {
        let mut lines = Vec::new();
        let written = after_round(
            root.path(),
            &said,
            SHOWN,
            &mut me,
            &mut finds,
            &mut |line| lines.push(line.to_owned()),
        );
        if round == 0 {
            assert_eq!(written, ["codex", "made-up"]);
        } else {
            assert!(written.is_empty(), "files there: not written again");
        }
        assert!(
            lines.iter().any(|line| line.starts_with("codex: lampos's login shell finds /home/lampos/.local/bin/codex before /usr/local/bin/codex")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("made-up: lampos's login shell finds no made-up")),
            "{lines:?}"
        );
        assert!(!wrapper::present(
            root.path(),
            "codex",
            SHOWN,
            finds("codex").as_deref()
        ));
    }
}
