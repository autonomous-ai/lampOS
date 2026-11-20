//! The apps' three folders, made by `lamp-apps-manage folders` (#7920): it
//! takes no argument, reads nothing and makes exactly those three paths.
use lamp_apps::{folders::make, layout::Layout};
use std::{cell::RefCell, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn tree(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut open = vec![root.to_path_buf()];
    while let Some(folder) = open.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            found.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                open.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_three_folders_are_made_with_their_modes_and_only_staging_is_handed_over() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let given = RefCell::new(Vec::new());
    let said = make(&layout, |path| {
        given.borrow_mut().push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert!(said.iter().any(|line| line.contains(" made")), "{said:?}");
    assert_eq!(mode(&layout.code), 0o755);
    assert_eq!(mode(&layout.state), 0o755);
    assert_eq!(mode(&layout.staging), 0o700);
    assert_eq!(*given.borrow(), vec![layout.staging.clone()]);
    // Exactly those paths and their parents: nothing else is made.
    let mut wanted: Vec<String> = [&layout.code, &layout.state, &layout.staging]
        .iter()
        .flat_map(|path| {
            let mut parts = Vec::new();
            let mut at = path.strip_prefix(root.path()).unwrap();
            loop {
                parts.push(at.display().to_string());
                match at.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => at = parent,
                    _ => break,
                }
            }
            parts
        })
        .collect();
    wanted.sort();
    wanted.dedup();
    assert_eq!(tree(root.path()), wanted);
}

#[test]
fn a_second_run_changes_nothing_and_puts_a_wrong_mode_right() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    make(&layout, |_| Ok(())).unwrap();
    fs::write(layout.state.join("kept"), b"x").unwrap();
    fs::set_permissions(&layout.staging, fs::Permissions::from_mode(0o777)).unwrap();
    let said = make(&layout, |_| Ok(())).unwrap();
    assert!(!said.iter().any(|line| line.contains(" made")), "{said:?}");
    assert_eq!(mode(&layout.staging), 0o700);
    assert!(
        layout.state.join("kept").is_file(),
        "a folder's contents are left"
    );
}

#[test]
fn a_failed_handover_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    assert!(make(&layout, |_| Err("no lampos".into())).is_err());
}

/// The real binary: anything after the name is refused with exit 2 before a
/// thing is made (were it to run, it would make folders under `/opt` and fail
/// here, so a regression that skips the check cannot pass), and the refusal
/// says so on standard error and nothing on standard output.
#[test]
fn the_real_folders_command_refuses_any_argument() {
    for extra in [
        vec!["--help"],
        vec!["/tmp/x"],
        vec!["start-all"],
        vec![""],
        vec!["a", "b"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_lamp-apps-manage"))
            .arg("folders")
            .args(&extra)
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{extra:?}");
        assert!(out.stdout.is_empty(), "{extra:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("takes no argument"),
            "{extra:?}"
        );
    }
}
