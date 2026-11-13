use super::*;
use crate::hermes_install::on as at;
use lamp_msgs::EngineInstall;
use std::{fs, os::unix::fs::MetadataExt, path::Path};

const SHOWN: Shown = Shown {
    skills: true,
    tasks: false,
    one_path: false,
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

/// The engine's own program on `lampos`'s login `PATH`, as after a
/// native install: the wrapper has something to run.
fn own_program(engine: &str) -> Result<Vec<String>, String> {
    Ok(vec![format!("/usr/bin/{engine}")])
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
    let mut said_before = Said::default();
    let mut asked = 0;
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut said_before,
        &mut || {
            asked += 1;
            me()
        },
        &mut wrapper_found,
        &mut own_program,
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
        &mut said_before,
        &mut || {
            asked += 1;
            me()
        },
        &mut wrapper_found,
        &mut own_program,
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
        &mut said_before,
        &mut me,
        &mut wrapper_found,
        &mut own_program,
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
        one_path: false,
    };
    let written = after_round(
        root.path(),
        &said,
        other,
        &mut said_before,
        &mut me,
        &mut wrapper_found,
        &mut own_program,
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
    let mut said_before = Said::default();
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut said_before,
        &mut me,
        &mut wrapper_found,
        &mut own_program,
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
    let mut said_before = Said::default();
    let mut lines = Vec::new();
    let written = after_round(
        root.path(),
        &said,
        SHOWN,
        &mut said_before,
        &mut || Err("id -u lampos gave no user id".to_owned()),
        &mut wrapper_found,
        &mut own_program,
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
    let mut said_before = Said::default();
    after_round(
        root.path(),
        &said,
        SHOWN,
        &mut said_before,
        &mut me,
        &mut wrapper_found,
        &mut own_program,
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
/// (`~/.local/bin`), or none at all: written once and said once, then
/// neither written nor said again while it stays so.
#[test]
fn an_engine_found_before_the_wrapper_is_said_not_written_again() {
    let root = root();
    let said = look(&[("codex", "installed"), ("made-up", "installed")]);
    let mut said_before = Said::default();
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
            &mut said_before,
            &mut me,
            &mut finds,
            &mut own_program,
            &mut |line| lines.push(line.to_owned()),
        );
        if round == 0 {
            assert_eq!(written, ["codex", "made-up"]);
        } else {
            assert!(written.is_empty(), "files there: not written again");
        }
        if round == 0 {
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
        } else {
            assert!(lines.is_empty(), "said once: {lines:?}");
        }
        assert!(!wrapper::present(
            root.path(),
            "codex",
            SHOWN,
            finds("codex").as_deref()
        ));
    }
}

fn run(
    root: &Path,
    said: &HarnessLook,
    before: &mut Said,
    finds: &mut dyn FnMut(&str) -> Option<String>,
) -> (Vec<String>, Vec<String>) {
    run_with(root, said, before, finds, &mut own_program)
}

fn run_with(
    root: &Path,
    said: &HarnessLook,
    before: &mut Said,
    finds: &mut dyn FnMut(&str) -> Option<String>,
    programs: &mut dyn FnMut(&str) -> Result<Vec<String>, String>,
) -> (Vec<String>, Vec<String>) {
    let mut lines = Vec::new();
    let written = after_round(
        root,
        said,
        SHOWN,
        before,
        &mut me,
        finds,
        programs,
        &mut |line| lines.push(line.to_owned()),
    );
    (written, lines)
}

/// A line that stays true is said in the first round only; a changed
/// value (another program found first) is said again, and one that went
/// away and came back is said again too.
#[test]
fn a_line_that_stays_the_same_is_said_once_and_again_when_it_changes() {
    let root = root();
    let said = look(&[("codex", "installed")]);
    let mut before = Said::default();
    let mut found = "/home/lampos/.local/bin/codex".to_owned();
    let (_, first) = run(root.path(), &said, &mut before, &mut |_| {
        Some(found.clone())
    });
    assert_eq!(first.len(), 2, "written, then found first: {first:?}");
    for _ in 0..3 {
        let (_, again) = run(root.path(), &said, &mut before, &mut |_| {
            Some(found.clone())
        });
        assert!(again.is_empty(), "{again:?}");
    }
    found = "/opt/codex/bin/codex".to_owned();
    let (_, changed) = run(root.path(), &said, &mut before, &mut |_| {
        Some(found.clone())
    });
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(
        changed[0].contains("/opt/codex/bin/codex before"),
        "{changed:?}"
    );
    // Found as the wrapper: nothing said, and the line forgotten.
    let (_, fine) = run(root.path(), &said, &mut before, &mut wrapper_found);
    assert!(fine.is_empty(), "{fine:?}");
    let (_, back) = run(root.path(), &said, &mut before, &mut |_| {
        Some(found.clone())
    });
    assert_eq!(back.len(), 1, "said again after it went away: {back:?}");
}

/// The refusal of a file that is not lamp-update's is said once while it
/// stays there.
#[test]
fn a_refusal_is_said_once() {
    let root = root();
    fs::write(
        at(root.path(), "/usr/local/bin/codex"),
        "#!/bin/sh\necho the real codex\n",
    )
    .unwrap();
    let said = look(&[("codex", "installed")]);
    let mut before = Said::default();
    let (_, first) = run(root.path(), &said, &mut before, &mut wrapper_found);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].contains("not lamp-update's; left as it is"));
    let (_, again) = run(root.path(), &said, &mut before, &mut wrapper_found);
    assert!(again.is_empty(), "{again:?}");
}

/// No ids for `lampos` is said once while it lasts.
#[test]
fn no_ids_is_said_once() {
    let root = root();
    let said = look(&[("codex", "installed")]);
    let mut before = Said::default();
    for round in 0..3 {
        let mut lines = Vec::new();
        after_round(
            root.path(),
            &said,
            SHOWN,
            &mut before,
            &mut || Err("id -u lampos gave no user id".to_owned()),
            &mut wrapper_found,
            &mut own_program,
            &mut |line| lines.push(line.to_owned()),
        );
        assert_eq!(lines.len(), usize::from(round == 0), "{lines:?}");
    }
}

/// An engine Harness said was installed and no longer says so: its
/// wrapper (exactly lamp-update's) is taken out, said once; its home and
/// the namespace script stay. Another engine still said installed keeps
/// its wrapper.
#[test]
fn a_wrapper_whose_engine_harness_no_longer_reports_is_taken_out() {
    let root = root();
    let mut before = Said::default();
    let both = look(&[("codex", "installed"), ("made-up", "installed")]);
    let (written, _) = run(root.path(), &both, &mut before, &mut wrapper_found);
    assert_eq!(written, ["codex", "made-up"]);
    let codex = at(root.path(), "/usr/local/bin/codex");
    for gone in [
        look(&[("made-up", "installed")]),
        look(&[("codex", "not_installed"), ("made-up", "installed")]),
    ] {
        // Reported again: written again when an earlier pass took it out.
        let missing = !codex.exists();
        let (written, _) = run(root.path(), &both, &mut before, &mut wrapper_found);
        assert_eq!(written, ["codex"].repeat(usize::from(missing)));
        let (written, lines) = run(root.path(), &gone, &mut before, &mut wrapper_found);
        assert!(written.is_empty());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("codex: Harness no longer says it is installed"),
            "{lines:?}"
        );
        assert!(!codex.exists());
        assert!(wrapper::present(
            root.path(),
            "made-up",
            SHOWN,
            wrapper_found("made-up").as_deref()
        ));
        assert!(at(root.path(), "/var/lib/lampos-agents/codex").is_dir());
        assert!(at(root.path(), wrapper::NS).exists());
        let (_, again) = run(root.path(), &gone, &mut before, &mut wrapper_found);
        assert!(again.is_empty(), "said once: {again:?}");
    }
}

/// Never taken out: a file at the wrapper's place that is not exactly
/// lamp-update's (the engine's own program, an edited wrapper, a link to
/// one), Hermes's and OpenClaw's, and anything before Harness was reached.
#[test]
fn only_exactly_lamp_updates_wrappers_are_taken_out() {
    let root = root();
    let bin = |name: &str| at(root.path(), &format!("/usr/local/bin/{name}"));
    fs::write(bin("codex"), "#!/bin/sh\necho the real codex\n").unwrap();
    let edited = format!("{}# edited\n", wrapper::wrapper_text("gemini").unwrap());
    fs::write(bin("gemini"), &edited).unwrap();
    fs::write(bin("aider-real"), wrapper::wrapper_text("aider").unwrap()).unwrap();
    std::os::unix::fs::symlink(bin("aider-real"), bin("aider")).unwrap();
    for own in OWN_INSTALLERS {
        // As if this build's wrapper text: still never taken out here.
        fs::write(bin(own), wrapper::wrapper_text(own).unwrap()).unwrap();
    }
    fs::write(bin("cursor"), wrapper::wrapper_text("cursor").unwrap()).unwrap();
    let mut before = Said::default();

    // Before Harness was reached (no machine): nothing taken out.
    let unreached = HarnessLook::default();
    let (_, lines) = run(root.path(), &unreached, &mut before, &mut wrapper_found);
    assert!(lines.is_empty(), "{lines:?}");
    assert!(bin("cursor").exists());

    let none = look(&[]);
    let (_, lines) = run(root.path(), &none, &mut before, &mut wrapper_found);
    assert_eq!(lines.len(), 1, "cursor alone: {lines:?}");
    assert!(lines[0].starts_with("cursor: "), "{lines:?}");
    assert!(!bin("cursor").exists());
    assert_eq!(
        fs::read_to_string(bin("codex")).unwrap(),
        "#!/bin/sh\necho the real codex\n"
    );
    assert_eq!(fs::read_to_string(bin("gemini")).unwrap(), edited);
    assert!(fs::symlink_metadata(bin("aider")).unwrap().is_symlink());
    for own in OWN_INSTALLERS {
        assert!(bin(own).exists(), "{own}");
    }
}

/// With the switch off a round never runs: [`after_round`] is the only
/// way in, and `serve` calls it only with [`SWITCH`] on. Off, a wrapper
/// already written stays, whatever Harness says.
#[test]
fn switch_off_takes_nothing_out() {
    let root = root();
    let mut before = Said::default();
    let (written, _) = run(
        root.path(),
        &look(&[("codex", "installed")]),
        &mut before,
        &mut wrapper_found,
    );
    assert_eq!(written, ["codex"]);
    assert!(!on(Some("off")) && !on(None));
    // The round's switch off: nothing calls after_round, so nothing is
    // taken out even though Harness no longer says codex.
    let gone = look(&[]);
    assert!(wrapper::ours(root.path(), "codex"));
    assert_eq!(engines_to_remove(root.path(), &gone), ["codex"]);
}

/// No program of the engine's own left on `lampos`'s login `PATH` (it was
/// uninstalled; only the wrapper keeps Harness saying it is installed):
/// the wrapper is taken out, said once, and not written back while none
/// is there; one coming back has it written again.
#[test]
fn a_wrapper_with_no_program_left_is_taken_out_while_harness_says_installed() {
    let root = root();
    let said = look(&[("codex", "installed"), ("made-up", "installed")]);
    let mut before = Said::default();
    let (written, _) = run(root.path(), &said, &mut before, &mut wrapper_found);
    assert_eq!(written, ["codex", "made-up"]);
    let asked = std::cell::RefCell::new(Vec::new());
    let mut gone = |engine: &str| {
        asked.borrow_mut().push(engine.to_owned());
        if engine == "codex" {
            Ok(Vec::new())
        } else {
            own_program(engine)
        }
    };
    let (written, lines) = run_with(
        root.path(),
        &said,
        &mut before,
        &mut wrapper_found,
        &mut gone,
    );
    assert!(written.is_empty());
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("codex: no program of its own is left on lampos's login PATH, so lamp-update's wrapper /usr/local/bin/codex is taken out"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("codex: not written"), "{lines:?}");
    assert_eq!(
        *asked.borrow(),
        ["codex", "made-up"],
        "asked once an engine a round"
    );
    assert!(!at(root.path(), "/usr/local/bin/codex").exists());
    assert!(wrapper::ours(root.path(), "made-up"));
    for _ in 0..2 {
        let (written, lines) = run_with(
            root.path(),
            &said,
            &mut before,
            &mut wrapper_found,
            &mut gone,
        );
        assert!(written.is_empty() && lines.is_empty(), "{lines:?}");
    }
    // Installed again: written again.
    let (written, _) = run(root.path(), &said, &mut before, &mut wrapper_found);
    assert_eq!(written, ["codex"]);
}

/// The engine's own program in `~/.local/bin` (the look finds it): the
/// wrapper is kept.
#[test]
fn a_wrapper_with_a_program_in_local_bin_is_kept() {
    let root = root();
    let said = look(&[("codex", "installed")]);
    let mut before = Said::default();
    let mut local = |_: &str| Ok(vec!["/home/lampos/.local/bin/codex".to_owned()]);
    let (written, _) = run_with(
        root.path(),
        &said,
        &mut before,
        &mut wrapper_found,
        &mut local,
    );
    assert_eq!(written, ["codex"]);
    for _ in 0..2 {
        let (written, lines) = run_with(
            root.path(),
            &said,
            &mut before,
            &mut wrapper_found,
            &mut local,
        );
        assert!(written.is_empty() && lines.is_empty(), "{lines:?}");
        assert!(wrapper::ours(root.path(), "codex"));
    }
}

/// A look that fails, runs past its time, or cannot be run: nothing is
/// known, so the wrapper is kept, and that is said once while it lasts.
/// It is never a reason not to write one either.
#[test]
fn a_failed_look_keeps_the_wrapper_and_is_said_once() {
    let root = root();
    let said = look(&[("codex", "installed")]);
    let mut before = Said::default();
    let mut failed = |_: &str| Err("lampos's login shell gave no answer within 5 s".to_owned());
    let (written, _) = run_with(
        root.path(),
        &said,
        &mut before,
        &mut wrapper_found,
        &mut failed,
    );
    assert_eq!(written, ["codex"], "written: the look is no answer");
    let (_, first) = run_with(
        root.path(),
        &said,
        &mut before,
        &mut wrapper_found,
        &mut failed,
    );
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first[0].starts_with("codex: lamp-update's wrapper /usr/local/bin/codex kept")
            && first[0].contains("no answer within 5 s"),
        "{first:?}"
    );
    for _ in 0..2 {
        let (_, again) = run_with(
            root.path(),
            &said,
            &mut before,
            &mut wrapper_found,
            &mut failed,
        );
        assert!(again.is_empty(), "said once: {again:?}");
    }
    assert!(wrapper::ours(root.path(), "codex"));
}
