use super::*;
use std::process::Command;

fn shim(args: &[&str]) -> std::process::Output {
    let folder = tempfile::tempdir().unwrap();
    let place = folder.path().join("harness");
    fs::write(&place, HOOK_SHIM_TEXT).unwrap();
    Command::new("/bin/sh")
        .arg(&place)
        .args(args)
        .output()
        .unwrap()
}

/// (#6530) `harness hook …` is answered with nothing and exit 0, as
/// Harness writes its hooks for each engine; anything else is refused with
/// one line and no Harness state.
#[test]
fn the_stand_in_answers_hooks_with_nothing_and_refuses_the_rest() {
    for args in [
        &["hook", "hermes", "tool"][..],
        &["hook", "hermes", "session-start"],
        &["hook", "openclaw", "tool"],
        &["hook"],
    ] {
        let out = shim(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{args:?}");
    }
    for args in [&["machines"][..], &[], &["--version"], &["hooks", "x"]] {
        let out = shim(args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "harness: not available inside the lamp's agent namespace\n"
        );
    }
    assert!(HOOK_SHIM_TEXT.starts_with("#!/bin/sh\n"));
    assert!(HOOK_SHIM_TEXT[..512.min(HOOK_SHIM_TEXT.len())].contains("Written by lamp-update."));
    // It reads nothing: no state folder, credential or Harness path in it.
    for none in ["/var/lib", "/run", "cat ", "<", "$HOME", "lampos-tasks"] {
        assert!(!HOOK_SHIM_TEXT.contains(none), "{none}");
    }
    assert_eq!(HOOK_SHIM, format!("{BIN}/harness"));
    assert!(PATH_LINES.contains(&format!("PATH={BIN}:${{PATH:-")));
}

/// Engine-agnostic: no engine is named in the shared piece's texts.
#[test]
fn the_shared_piece_names_no_engine() {
    let texts = [HOOK_SHIM_TEXT, PATH_LINES, HOOK_NOTE, BIN, HOOK_SHIM];
    for text in texts {
        for engine in ["hermes", "Hermes", "openclaw", "OpenClaw"] {
            assert!(!text.contains(engine), "{engine} in {text}");
        }
    }
}

/// The `PATH` goes before the last `exec` only; the journal line before
/// the first `LAMP_NS_UID=`; with neither, the text is unchanged.
#[test]
fn the_insertions_go_before_their_anchor_once() {
    let text = "#!/bin/sh\nexec a\nT=1\nLAMP_NS_UID=$(id -u)\nexec b \"$@\"\n";
    assert_eq!(
        with_ns_path(text),
        format!("#!/bin/sh\nexec a\nT=1\nLAMP_NS_UID=$(id -u)\n{PATH_LINES}exec b \"$@\"\n")
    );
    assert_eq!(
        with_hook_note(text, "x"),
        format!(
            "#!/bin/sh\nexec a\nT=1\n{}LAMP_NS_UID=$(id -u)\nexec b \"$@\"\n",
            hook_note_line("x")
        )
    );
    assert_eq!(
        with_ns_path("#!/bin/sh\n  exec a\n"),
        "#!/bin/sh\n  exec a\n"
    );
    assert_eq!(with_hook_note("#!/bin/sh\n", "x"), "#!/bin/sh\n");
}

/// Written root's 0755 in its own 0755 folder (its folder must be there,
/// a link in its place refused), and taken out only with the last
/// namespace script.
#[test]
fn the_stand_in_is_written_0755_and_taken_out_with_the_last_script() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    assert!(write_hook_shim(root).is_err(), "no /usr/local/lib/lampos");
    fs::create_dir_all(root.join("usr/local/lib/lampos")).unwrap();
    std::os::unix::fs::symlink("/tmp", on(root, BIN)).unwrap();
    assert!(write_hook_shim(root).is_err(), "never through a link");
    fs::remove_file(on(root, BIN)).unwrap();
    write_hook_shim(root).unwrap();
    write_hook_shim(root).unwrap();
    assert!(hook_shim_there(root));
    let mode = |path: &str| fs::metadata(on(root, path)).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(BIN), 0o755);
    assert_eq!(mode(HOOK_SHIM), 0o755);
    assert_eq!(fs::read_dir(on(root, BIN)).unwrap().count(), 1);
    let script = "/usr/local/lib/lampos/some-ns";
    fs::write(on(root, script), "").unwrap();
    let mut said = Vec::new();
    remove_hook_shim(root, &[script], &mut |line| said.push(line.to_owned()));
    assert!(hook_shim_there(root), "a script still uses it");
    fs::remove_file(on(root, script)).unwrap();
    remove_hook_shim(root, &[script], &mut |line| said.push(line.to_owned()));
    assert!(!on(root, BIN).exists());
    assert_eq!(said, [format!("{HOOK_SHIM} taken out")]);
}
