use super::*;
use std::{
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SHOWN: [Shown; 4] = [
    Shown {
        skills: false,
        tasks: false,
        one_path: false,
    },
    Shown {
        skills: true,
        tasks: false,
        one_path: false,
    },
    Shown {
        skills: false,
        tasks: true,
        one_path: false,
    },
    Shown {
        skills: true,
        tasks: true,
        one_path: false,
    },
];

fn sh_n(text: &str) {
    let folder = tempfile::tempdir().unwrap();
    let place = folder.path().join("script");
    fs::write(&place, text).unwrap();
    let out = Command::new("/bin/sh")
        .arg("-n")
        .arg(&place)
        .output()
        .unwrap();
    assert!(out.status.success(), "sh -n: {out:?}\n{text}");
}

#[test]
fn engine_names_are_refused_unless_plain() {
    let longest = "a".repeat(ENGINE_MAX);
    for engine in ["hermes", "openclaw", "claude-code", "codex2", "a", &longest] {
        assert!(engine_ok(engine), "{engine}");
        assert!(wrapper_text(engine).is_ok() && wrapper_path(engine).is_ok());
        assert!(home_of(engine).is_ok());
    }
    let too_long = "a".repeat(ENGINE_MAX + 1);
    for engine in [
        "", "-", "-x", "--help", "Hermes", "a/b", "../x", "..", "a b", "a;b", "a\nb", "$(id)",
        "a_b", "é", "a.b", &too_long,
    ] {
        assert!(!engine_ok(engine), "{engine:?}");
        assert!(wrapper_text(engine).is_err(), "{engine:?}");
        assert!(wrapper_path(engine).is_err(), "{engine:?}");
        assert!(home_of(engine).is_err(), "{engine:?}");
        let root = tempfile::tempdir().unwrap();
        assert!(write_engine(root.path(), engine, (0, 0), SHOWN[0]).is_err());
        assert!(!present(root.path(), engine, SHOWN[0], None));
    }
}

#[test]
fn the_wrapper_carries_the_mark_in_its_first_512_bytes() {
    for engine in ["a", "hermes", &"z".repeat(ENGINE_MAX)] {
        let text = wrapper_text(engine).unwrap();
        assert!(text.starts_with("#!/bin/sh\n"));
        assert!(
            text.as_bytes()[..MARK_WITHIN.min(text.len())]
                .windows(MARK.len())
                .any(|w| w == MARK.as_bytes())
        );
        // The mark the wrapper looks for to skip other wrappers is this one.
        assert!(text.contains(&format!("grep -qF \"{MARK}\"")));
        assert!(text.contains(&format!("head -c {MARK_WITHIN} ")));
        sh_n(&text);
    }
}

/// The engine's program is found by name on `PATH`: no path to it, and no
/// `exec` but the namespace's `unshare`.
#[test]
fn the_wrapper_never_names_the_engines_program_by_a_path() {
    let engine = "someengine";
    let text = wrapper_text(engine).unwrap();
    let paths: Vec<&str> = text
        .match_indices(&format!("/{engine}"))
        .map(|(at, _)| &text[..at])
        .collect();
    for before in &paths {
        assert!(before.ends_with("/var/lib/lampos-agents"), "{before}");
    }
    for word in [
        ".local/bin",
        "/opt/",
        "lampos-hermes",
        "node_modules",
        "/usr/bin/",
    ] {
        let hits: Vec<&str> = text.lines().filter(|l| l.contains(word)).collect();
        if word == "/usr/bin/" {
            assert_eq!(hits.len(), 1, "{hits:?}");
            assert!(hits[0].starts_with("exec /usr/bin/unshare "));
        } else {
            assert!(hits.is_empty(), "{word}: {hits:?}");
        }
    }
    let execs: Vec<&str> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("exec "))
        .collect();
    assert_eq!(execs.len(), 1);
    assert!(execs[0].ends_with(&format!("{NS} \"$@\"")));
}

/// The token is read into a variable and exported alone: never a command's
/// argument in the wrapper's own text. The run lines are Hermes's own.
#[test]
fn the_token_is_only_assigned_and_exported() {
    let text = wrapper_text("eng").unwrap();
    let uses: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("$C") || l.contains("${C"))
        .map(str::trim)
        .collect();
    assert_eq!(
        uses,
        [
            "[ -n \"$C\" ] || { echo \"${0##*/}: no token for run $R\" >&2; exit 1; }",
            "LAMP_CONTROL_TOKEN=$C LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock \
             PYTHONPATH=/usr/local/lib/lampos/sdk-python LAMP_NS_NET=run",
        ]
    );
    assert!(text.contains(&format!("export {TOKEN_ENV} ")));
    assert!(text.contains(&format!("F={RUNS}/$R\n")));
    assert_eq!(text.matches(hermes_install::RUN_LINES).count(), 1);
    assert!(!text.contains("LAMP_RUN_ID"));
}

/// A lamp laid out under a folder, with the wrapper's fixed paths moved
/// into it and `unshare` a stub that writes its argv and environment.
struct Lamp {
    _folder: tempfile::TempDir,
    top: PathBuf,
    wrapper: PathBuf,
    real: PathBuf,
}

const TOKEN: &str = "s3cret-token-0123456789";

fn lamp(engine: &str) -> Lamp {
    lamp_of(engine, &wrapper_text(engine).unwrap())
}

/// [`lamp`] with `text` as the wrapper.
fn lamp_of(engine: &str, text: &str) -> Lamp {
    let folder = tempfile::tempdir().unwrap();
    let top = fs::canonicalize(folder.path()).unwrap();
    for dir in [
        "runs",
        "project",
        "elsewhere",
        "bin1",
        "bin2",
        "bin3",
        "out",
    ] {
        fs::create_dir(top.join(dir)).unwrap();
    }
    fs::set_permissions(top.join("runs"), fs::Permissions::from_mode(0o700)).unwrap();
    let stub = top.join("unshare");
    fs::write(
        &stub,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$OUT/argv\"\nenv > \"$OUT/env\"\n",
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let text = text
        .replace(RUNS, top.join("runs").to_str().unwrap())
        .replace(
            hermes_install::RUN_FOLDERS,
            top.join("project").to_str().unwrap(),
        )
        .replace("/usr/bin/unshare", stub.to_str().unwrap());
    let wrapper = top.join("bin1").join(engine);
    fs::write(&wrapper, text).unwrap();
    // Another lamp-update wrapper of the same name, later on PATH: skipped.
    let other = top.join("bin2").join(engine);
    fs::write(&other, format!("#!/bin/sh\n# {MARK}\nexit 9\n")).unwrap();
    let real = top.join("bin3").join(engine);
    fs::write(&real, "#!/bin/sh\nexit 0\n").unwrap();
    for file in [&wrapper, &other, &real] {
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    Lamp {
        _folder: folder,
        top,
        wrapper,
        real,
    }
}

impl Lamp {
    /// The wrapper run in `top`'s `elsewhere`, a folder that is no run's.
    fn run(&self, env: &[(&str, &str)], path: &str) -> Output {
        self.run_in(&self.top.join("elsewhere"), env, path)
    }

    /// A run's own folder, `project/skill-<run>`, made when missing.
    fn folder(&self, run: &str) -> PathBuf {
        let folder = self.top.join("project").join(format!("skill-{run}"));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn run_in(&self, cwd: &Path, env: &[(&str, &str)], path: &str) -> Output {
        let _ = fs::remove_file(self.top.join("out/argv"));
        let _ = fs::remove_file(self.top.join("out/env"));
        Command::new(&self.wrapper)
            .args(["chat", "-q", "hi"])
            .current_dir(cwd)
            .env_clear()
            .env("PATH", path)
            .env("OUT", self.top.join("out"))
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    fn path(&self) -> String {
        format!(
            "relative:{}:{}:{}:/usr/bin:/bin",
            self.top.join("bin1").display(),
            self.top.join("bin2").display(),
            self.top.join("bin3").display()
        )
    }

    fn token_file(&self, run: &str) -> PathBuf {
        let place = self.top.join("runs").join(run);
        fs::write(&place, TOKEN).unwrap();
        fs::set_permissions(&place, fs::Permissions::from_mode(0o600)).unwrap();
        place
    }

    fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.top.join("out").join(name)).ok()
    }
}

/// (#6510 step 2) A skill's run, found by its folder: the token file is
/// read once and deleted, the token reaches the engine in its environment
/// only, never in argv, with the socket and the SDK's `PYTHONPATH`; the run
/// gets `--net`; the program is the first `<engine>` on `PATH` that is
/// neither the wrapper nor another marked wrapper. A second start in the
/// same folder (a Harness relaunch) finds no token and runs nothing.
#[test]
fn a_runs_token_file_is_read_once_deleted_and_never_in_argv() {
    let lamp = lamp("eng");
    let file = lamp.token_file("run-1");
    let folder = lamp.folder("run-1");
    let out = lamp.run_in(&folder, &[], &lamp.path());
    assert!(out.status.success(), "{out:?}");
    assert!(!file.exists(), "the token file is deleted");
    let argv = lamp.read("argv").unwrap();
    let env = lamp.read("env").unwrap();
    assert!(!argv.contains(TOKEN), "{argv}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(TOKEN));
    assert_eq!(
        argv.lines().collect::<Vec<_>>(),
        [
            "--map-root-user",
            "--net",
            "--mount",
            "--pid",
            "--kill-child",
            "--mount-proc",
            "--propagation",
            "private",
            NS,
            "chat",
            "-q",
            "hi"
        ]
    );
    let lines: Vec<&str> = env.lines().collect();
    for line in [
        format!("{TOKEN_ENV}={TOKEN}"),
        format!("LAMP_CONTROL_SOCKET={}", hermes_install::CONTROL_SOCKET),
        format!("PYTHONPATH={}", hermes_install::SDK_PYTHONPATH),
        "LAMP_NS_NET=run".to_owned(),
        "LAMP_NS_ENGINE=eng".to_owned(),
        format!("LAMP_NS_ENGINE_BIN={}", lamp.real.display()),
    ] {
        assert!(lines.contains(&line.as_str()), "{line}: {env}");
    }

    // Once only: the same folder again finds no token and runs nothing.
    let again = lamp.run_in(&folder, &[], &lamp.path());
    assert!(!again.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(String::from_utf8_lossy(&again.stderr).contains("eng: no token for run run-1"));
}

/// Not a run's folder: no token (even one already in the caller's
/// environment, or the old `LAMP_RUN_ID`), no `--net` (even
/// `LAMP_NS_NET=run` from the caller), and no run file read.
#[test]
fn without_a_run_there_is_no_token_and_no_net() {
    let lamp = lamp("eng");
    let file = lamp.token_file("run-2");
    let project = lamp.top.join("project");
    let lamps = project.join("lamp");
    fs::create_dir_all(&lamps).unwrap();
    let nested = lamp.folder("run-2").join("deeper");
    fs::create_dir_all(&nested).unwrap();
    for cwd in [lamp.top.join("elsewhere"), project, lamps, nested] {
        let out = lamp.run_in(
            &cwd,
            &[
                (TOKEN_ENV, "from-the-caller"),
                ("LAMP_NS_NET", "run"),
                ("LAMP_RUN_ID", "run-2"),
            ],
            &lamp.path(),
        );
        assert!(out.status.success(), "{cwd:?}: {out:?}");
        assert!(file.exists(), "another run's file is left alone");
        let argv = lamp.read("argv").unwrap();
        let env = lamp.read("env").unwrap();
        assert!(!argv.contains("--net"), "{cwd:?}");
        assert!(
            !env.contains(TOKEN_ENV) && !env.contains("LAMP_NS_NET"),
            "{cwd:?}: {env}"
        );
    }
}

/// A folder named like a run's with an id that is not plain is no run's:
/// as any other folder. A link at the token's place is never read
/// through, is removed, and the run ends with no token.
#[test]
fn a_run_id_that_is_not_plain_or_a_linked_token_file_runs_nothing() {
    let lamp = lamp("eng");
    for run in ["Run", "a_b", "a.b", &"a".repeat(65)] {
        let file = lamp.token_file(run);
        let out = lamp.run_in(&lamp.folder(run), &[], &lamp.path());
        assert!(out.status.success(), "{run:?}: {out:?}");
        assert!(!lamp.read("argv").unwrap().contains("--net"), "{run:?}");
        assert!(file.exists(), "{run:?}: not read");
    }
    let elsewhere = lamp.top.join("elsewhere/token");
    fs::write(&elsewhere, TOKEN).unwrap();
    let link = lamp.top.join("runs/run-3");
    symlink(&elsewhere, &link).unwrap();
    let out = lamp.run_in(&lamp.folder("run-3"), &[], &lamp.path());
    assert!(!out.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no token for run run-3"));
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(elsewhere.exists());
}

#[test]
fn only_wrappers_on_path_run_nothing() {
    let lamp = lamp("eng");
    let path = format!(
        "{}:{}:/usr/bin:/bin",
        lamp.top.join("bin1").display(),
        lamp.top.join("bin2").display()
    );
    let out = lamp.run(&[], &path);
    assert!(!out.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no eng on PATH"));
}

/// (Reviewer-lead's P3) A run whose wrapper finds no engine program still
/// has its token file deleted: the run-id block comes first.
#[test]
fn a_run_with_no_engine_program_leaves_no_token_file() {
    let lamp = lamp("eng");
    let file = lamp.token_file("run-2");
    let path = format!("{}:/usr/bin:/bin", lamp.top.join("bin1").display());
    let out = lamp.run_in(&lamp.folder("run-2"), &[], &path);
    assert!(!out.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(!file.exists());
}

/// The namespace script is Hermes's with the engine made data: every edit
/// found once for every switch, no engine named, `HOME` the engine's own.
#[test]
fn the_namespace_script_names_no_engine_and_homes_are_per_engine() {
    for shown in SHOWN {
        let mut text = hermes_install::ns_text(shown.skills, shown.tasks);
        assert_eq!(text.matches(hermes_header()).count(), 1, "{shown:?}");
        text = text.replacen(hermes_header(), NS_HEADER, 1);
        for (from, to) in NS_EDITS {
            assert_eq!(text.matches(from).count(), 1, "{shown:?}: {from}");
            text = text.replacen(from, to, 1);
        }
        let text = ns_text(shown);
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("hermes") && !lower.contains("openclaw"),
            "{text}"
        );
        assert!(text.starts_with("#!/bin/sh\n"));
        assert!(text.contains(MARK));
        assert!(text.contains("H=/var/lib/lampos-agents/$E "));
        assert!(text.contains("HOME=$H\n"));
        assert_eq!(text.matches(super::super::PATH_LINES).count(), 1);
        let last = text.trim_end().lines().last().unwrap();
        assert!(last.ends_with(" \"$X\" \"$@\""), "{last}");
        assert!(text[..text.rfind("\nexec ").unwrap()].ends_with("export PATH"));
        assert_eq!(text.contains("control.sock"), shown.tasks);
        assert_eq!(text.contains("/run/lampos-skills"), shown.skills);
        sh_n(&text);
    }
    assert_ne!(home_of("a").unwrap(), home_of("b").unwrap());
    assert_eq!(home_of("codex").unwrap(), "/var/lib/lampos-agents/codex");
    assert!(
        wrapper_text("codex")
            .unwrap()
            .contains("HOME /var/lib/lampos-agents/codex:")
    );
}

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for dir in ["usr/local/bin", "usr/local/lib/lampos", "var/lib"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    root
}

fn me() -> (u32, u32) {
    let me = tempfile::tempfile().unwrap().metadata().unwrap();
    (me.uid(), me.gid())
}

#[test]
fn write_engine_writes_the_entry_points_and_a_0700_home() {
    let root = root();
    let shown = SHOWN[3];
    for engine in ["eng", "other"] {
        let found = wrapper_path(engine).unwrap();
        let found = Some(found.as_str());
        assert!(!present(root.path(), engine, shown, found));
        write_engine(root.path(), engine, me(), shown).unwrap();
        assert!(present(root.path(), engine, shown, found));
        assert!(
            !present(root.path(), engine, SHOWN[0], found),
            "another switch: written again"
        );
        let home = on(root.path(), &home_of(engine).unwrap());
        assert_eq!(fs::metadata(&home).unwrap().mode() & 0o7777, 0o700);
        let wrapper = on(root.path(), &wrapper_path(engine).unwrap());
        assert_eq!(fs::metadata(&wrapper).unwrap().mode() & 0o7777, 0o755);
        assert!(marked(&wrapper));
    }
    assert_eq!(
        fs::metadata(on(root.path(), HOMES)).unwrap().mode() & 0o7777,
        0o755
    );
    // A changed wrapper is the engine missing.
    let wrapper = on(root.path(), "/usr/local/bin/eng");
    fs::write(&wrapper, format!("#!/bin/sh\n# {MARK}\n")).unwrap();
    assert!(!present(
        root.path(),
        "eng",
        shown,
        Some("/usr/local/bin/eng")
    ));
}

/// (Heron's read) The wrapper written but the engine's own program found
/// first by `lampos`'s login shell (`~/.local/bin/<engine>`, earlier on
/// Harness's `PATH`), or nothing found: not present, so not offered.
#[test]
fn an_engine_found_before_the_wrapper_is_not_present() {
    let root = root();
    let shown = SHOWN[3];
    write_engine(root.path(), "eng", me(), shown).unwrap();
    assert!(present(
        root.path(),
        "eng",
        shown,
        Some("/usr/local/bin/eng")
    ));
    for found in [
        Some("/home/lampos/.local/bin/eng"),
        Some("/var/lib/lampos/.local/bin/eng"),
        Some("/usr/bin/eng"),
        Some("/usr/local/bin/eng2"),
        Some(""),
        None,
    ] {
        assert!(!present(root.path(), "eng", shown, found), "{found:?}");
        assert!(!found_first("eng", found), "{found:?}");
    }
    assert!(found_first("eng", Some("/usr/local/bin/eng")));
}

/// Harness's lookup: `lampos`'s login shell, the engine one plain word.
#[test]
fn the_login_lookup_is_lamposs_login_shell_and_refuses_unplain_names() {
    assert_eq!(
        login_lookup_argv("claude").unwrap(),
        [
            "timeout",
            "5",
            "runuser",
            "-l",
            "lampos",
            "-c",
            "command -v claude"
        ]
    );
    for engine in ["-l", "a;b", "$(id)", "a b", ""] {
        assert!(login_lookup_argv(engine).is_err(), "{engine:?}");
        assert!(login_finds(engine).is_none(), "{engine:?}");
    }
}

/// The engine's own program (or anyone's file) at the wrapper's place is
/// never replaced.
#[test]
fn write_engine_never_replaces_a_file_that_is_not_lamp_updates() {
    let root = root();
    let place = on(root.path(), "/usr/local/bin/eng");
    fs::write(&place, "#!/bin/sh\necho the real one\n").unwrap();
    let error = write_engine(root.path(), "eng", me(), SHOWN[0]).unwrap_err();
    assert!(error.contains("not lamp-update's"), "{error}");
    assert_eq!(
        fs::read_to_string(&place).unwrap(),
        "#!/bin/sh\necho the real one\n"
    );
    assert!(!on(root.path(), NS).exists());
    fs::remove_file(&place).unwrap();
    symlink("/bin/sh", &place).unwrap();
    assert!(write_engine(root.path(), "eng", me(), SHOWN[0]).is_err());
}

/// The look for an engine's own programs: `lampos`'s login shell, bounded,
/// the engine the only word put in a fixed text; refused unless plain.
#[test]
fn the_programs_look_is_bounded_fixed_and_refuses_unplain_names() {
    let argv = login_programs_argv("codex").unwrap();
    assert_eq!(
        argv[..6],
        ["timeout", LOOKUP_SECS, "runuser", "-l", "lampos", "-c"]
    );
    assert_eq!(argv.len(), 7);
    assert!(argv[6].starts_with("E=codex; "));
    assert_eq!(
        argv[6].replacen("codex", "other", 1),
        login_programs_argv("other").unwrap()[6]
    );
    sh_n(&argv[6]);
    for engine in ["", "-x", "a b", "$(id)", "a;b"] {
        assert!(login_programs_argv(engine).is_err(), "{engine:?}");
    }
}

/// Run where `PATH` holds a lamp-update wrapper, a folder that is not
/// full, a file that is not executable and the engine's own program: the
/// look prints the program alone. With none, nothing, and it ends 0.
#[test]
fn the_programs_look_lists_only_the_engines_own_programs() {
    let folder = tempfile::tempdir().unwrap();
    let dir = |name: &str| {
        let dir = folder.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    };
    let put = |dir: &PathBuf, text: &str, mode: u32| {
        let file = dir.join("codex");
        fs::write(&file, text).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
    };
    let wrapper = dir("wrap");
    put(&wrapper, &wrapper_text("codex").unwrap(), 0o755);
    let plain_file = dir("plain");
    put(&plain_file, "#!/bin/sh\n", 0o644);
    let own = dir("own");
    put(&own, "#!/bin/sh\necho codex\n", 0o755);
    let look = |path: String| {
        let argv = login_programs_argv("codex").unwrap();
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(&argv[6])
            .env("PATH", path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        programs_in("codex", &String::from_utf8_lossy(&out.stdout))
    };
    let path = |dirs: &[&PathBuf]| {
        dirs.iter()
            .map(|dir| dir.display().to_string())
            .chain([
                "relative".to_owned(),
                "/usr/bin".to_owned(),
                "/bin".to_owned(),
            ])
            .collect::<Vec<_>>()
            .join(":")
    };
    let own_codex = own.join("codex").display().to_string();
    assert_eq!(look(path(&[&wrapper, &plain_file, &own])), [own_codex]);
    assert!(look(path(&[&wrapper, &plain_file])).is_empty());
}

/// Only full paths ending in the engine's name are its programs: a login
/// shell's rc files may print anything else.
#[test]
fn programs_in_passes_over_other_lines() {
    let said = "welcome to the lamp\n/home/lampos/.local/bin/codex\n codex\n/usr/bin/codex-x\n/opt/x/codex\n";
    assert_eq!(
        programs_in("codex", said),
        ["/home/lampos/.local/bin/codex", "/opt/x/codex"]
    );
    assert!(programs_in("codex", "").is_empty());
}

/// The agents the one run path is tried with: Hermes (its own data) and
/// two plain ones.
fn one_path_agents() -> Vec<Agent> {
    vec![
        hermes_install::agent(),
        agent_of("claude").unwrap(),
        agent_of("codex").unwrap(),
    ]
}

/// #6685: the one run path's wrapper is the same bytes for every agent but
/// its data block: the header before it and the run path after it (the run
/// lines, the `--net` lines and the `unshare` line among them) are equal
/// for Hermes, Claude Code and Codex, and name no agent.
#[test]
fn the_one_run_path_is_the_same_bytes_for_every_agent() {
    let texts: Vec<String> = one_path_agents()
        .iter()
        .map(|agent| one_path_text(agent).unwrap())
        .collect();
    let split = |text: &str| {
        let data = text.find("\nE=").unwrap() + 1;
        let run = text.find(ONE_PATH_RUN_LINE).unwrap();
        (text[..data].to_owned(), text[run..].to_owned())
    };
    let (header, run) = split(&texts[0]);
    assert_eq!(run, format!("{ONE_PATH_RUN_LINE}{ONE_PATH_RUN}"));
    for text in &texts {
        assert_eq!(split(text), (header.clone(), run.clone()));
        assert!(text.starts_with(&format!("#!/bin/sh\n{ONE_PATH_MARK}")));
        assert!(text[..MARK_WITHIN].contains(MARK));
        assert_eq!(text.matches(hermes_install::ONE_RUN_LINES).count(), 1);
        sh_n(text);
    }
    let lower = format!("{header}{run}").to_lowercase();
    for name in ["hermes", "claude", "codex", "openclaw", "yolo"] {
        assert!(!lower.contains(name), "{name} in the shared lines");
    }
    for line in [
        "unset LAMP_CONTROL_TOKEN LAMP_NS_NET\n",
        "N=\n[ \"${LAMP_NS_NET-}\" != run ] || N=--net\n",
        "exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/agent-ns \"$@\"\n",
        hermes_install::ONE_RUN_LINES,
    ] {
        assert_eq!(run.matches(line).count(), 1, "{line}");
    }
    assert!(run.contains(&format!("\"{}\"", super::super::HOOK_NOTE)));
    // Hermes's approvals are its own data, set only in a skill's run.
    assert!(!hermes_install::ONE_RUN_LINES.contains("HERMES"));
    assert!(texts[0].contains(&format!(
        "\nE=hermes\nH={}\nX={}\nagent_env() {{\n{}  :\n}}\nagent_run_env() {{\n{}  :\n}}\n",
        hermes_install::PLACE,
        hermes_install::HERMES,
        hermes_install::AGENT_ENV,
        hermes_install::AGENT_RUN_ENV
    )));
    assert!(texts[1].contains(
        "\nE=claude\nH=/var/lib/lampos-agents/claude\nX=\nagent_env() {\n  :\n}\nagent_run_env() {\n  :\n}\n"
    ));
    // The pinned texts lamp-tasks checks its capability check against.
    assert_eq!(texts[0], include_str!("one-path-hermes.sh"));
    assert_eq!(texts[1], include_str!("one-path-claude.sh"));
}

/// An agent's data is refused unless plain: the engine's name, its home
/// (its own under [`HOMES`] or one of [`OWN_HOMES`], never another lamp
/// service's state), its program and its lines.
#[test]
fn the_one_path_refuses_data_that_is_not_plain() {
    let good = agent_of("claude").unwrap();
    assert!(one_path_text(&good).is_ok());
    let refused = |change: &dyn Fn(&mut Agent)| {
        let mut agent = good.clone();
        change(&mut agent);
        assert!(one_path_text(&agent).is_err(), "{agent:?}");
    };
    refused(&|agent| agent.engine = "Claude".into());
    refused(&|agent| agent.engine = "a b".into());
    refused(&|agent| agent.home = "/var/lib/lampos-cloud".into());
    refused(&|agent| agent.home = "/var/lib/lampos-agents/codex".into());
    refused(&|agent| agent.home = "/var/lib/lampos-agents/claude/..".into());
    refused(&|agent| agent.program = Some("bin/claude".into()));
    refused(&|agent| agent.program = Some("/opt/a b/claude".into()));
    refused(&|agent| agent.program = Some("/opt/../claude".into()));
    refused(&|agent| agent.program = Some("/opt/$(id)".into()));
    refused(&|agent| agent.env = "X=1\n".into());
    refused(&|agent| agent.run_env = "  X=1".into());
    let mut hermes_home = good.clone();
    hermes_home.home = hermes_install::PLACE.into();
    assert!(
        one_path_text(&hermes_home).is_err(),
        "an own home is its engine's alone"
    );
    assert!(
        one_path_text(&hermes_install::agent()).is_ok(),
        "Hermes's own home"
    );
    assert_eq!(
        text_for("claude", SHOWN[0]).unwrap(),
        wrapper_text("claude").unwrap()
    );
    let on = Shown {
        one_path: true,
        ..SHOWN[3]
    };
    assert_eq!(
        text_for("claude", on).unwrap(),
        one_path_text(&good).unwrap()
    );
}

/// (#6685) A plain agent on the one run path does as before: a run's token
/// file read once and deleted, the token in its environment only, `--net`
/// in a run alone, the program the first on `PATH` that is no wrapper,
/// and its home given to the namespace.
#[test]
fn a_plain_agent_on_the_one_path_reads_its_runs_token_once() {
    let text = one_path_text(&agent_of("eng").unwrap()).unwrap();
    let lamp = lamp_of("eng", &text);
    let file = lamp.token_file("run-1");
    let folder = lamp.folder("run-1");
    let out = lamp.run_in(&folder, &[("HERMES_YOLO_MODE", "1")], &lamp.path());
    assert!(out.status.success(), "{out:?}");
    assert!(!file.exists());
    let argv = lamp.read("argv").unwrap();
    assert!(!argv.contains(TOKEN));
    assert!(argv.lines().any(|line| line == "--net"));
    let env = lamp.read("env").unwrap();
    let lines: Vec<&str> = env.lines().collect();
    for line in [
        format!("{TOKEN_ENV}={TOKEN}"),
        "LAMP_NS_NET=run".to_owned(),
        "LAMP_NS_ENGINE=eng".to_owned(),
        "LAMP_NS_HOME=/var/lib/lampos-agents/eng".to_owned(),
        format!("LAMP_NS_ENGINE_BIN={}", lamp.real.display()),
    ] {
        assert!(lines.contains(&line.as_str()), "{line}: {env}");
    }
    let again = lamp.run_in(&folder, &[], &lamp.path());
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("eng: no token for run run-1"));
    // Not a run's folder: no token, no `--net`, whatever the caller says.
    let out = lamp.run(
        &[(TOKEN_ENV, "given"), ("LAMP_NS_NET", "run")],
        &lamp.path(),
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!lamp.read("argv").unwrap().contains("--net"));
    let env = lamp.read("env").unwrap();
    assert!(
        !env.contains(TOKEN_ENV) && !env.contains("LAMP_NS_NET"),
        "{env}"
    );
}

/// (#6685) Hermes on the one run path: its own program and home from its
/// data, its forwarder token and `HERMES_HOME` at every start (none ends
/// it, the run's token file already gone), and `HERMES_YOLO_MODE` set in
/// a skill's run only, an inherited one dropped elsewhere.
#[test]
fn hermes_on_the_one_path_has_its_data_and_approvals_off_only_in_a_run() {
    let folder = tempfile::tempdir().unwrap();
    let top = fs::canonicalize(folder.path()).unwrap();
    let place = top.join("hermes-home");
    let program = place.join(".local/bin/hermes");
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let forwarder = top.join("hermes-token");
    fs::write(&forwarder, "tok_forwarder\n").unwrap();
    let text = one_path_text(&hermes_install::agent())
        .unwrap()
        .replace(hermes_install::TOKEN, forwarder.to_str().unwrap())
        .replace(hermes_install::PLACE, place.to_str().unwrap());
    let lamp = lamp_of("hermes", &text);
    let file = lamp.token_file("run-1");
    let out = lamp.run_in(&lamp.folder("run-1"), &[], "/usr/bin:/bin");
    assert!(out.status.success(), "{out:?}");
    assert!(!file.exists());
    let env = lamp.read("env").unwrap();
    let lines: Vec<&str> = env.lines().collect();
    for line in [
        format!("{TOKEN_ENV}={TOKEN}"),
        "HERMES_YOLO_MODE=1".to_owned(),
        "LAMP_HERMES_TOKEN=tok_forwarder".to_owned(),
        format!("HERMES_HOME={}/.hermes", place.display()),
        format!("LAMP_NS_HOME={}", place.display()),
        format!("LAMP_NS_ENGINE_BIN={}", program.display()),
        "LAMP_NS_ENGINE=hermes".to_owned(),
    ] {
        assert!(lines.contains(&line.as_str()), "{line}: {env}");
    }
    assert!(
        lamp.read("argv")
            .unwrap()
            .lines()
            .any(|line| line == "--net")
    );
    let out = lamp.run(&[("HERMES_YOLO_MODE", "1")], "/usr/bin:/bin");
    assert!(out.status.success(), "{out:?}");
    let env = lamp.read("env").unwrap();
    assert!(!env.contains("HERMES_YOLO_MODE"), "{env}");
    assert!(env.contains("LAMP_HERMES_TOKEN=tok_forwarder"));
    // No forwarder token: nothing runs, and a run's file is gone first.
    fs::remove_file(&forwarder).unwrap();
    let file = lamp.token_file("run-2");
    let out = lamp.run_in(&lamp.folder("run-2"), &[], "/usr/bin:/bin");
    assert!(!out.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(!file.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("hermes: no forwarder token"));
}

/// (#6685) With the one run path, the namespace script takes the agent's
/// home from its wrapper: every edit found once for every switch, the home
/// checked against the agents' homes (its own under [`HOMES`], or its own in
/// [`OWN_HOMES`]) before anything is mounted, and the only agent's word in
/// it the homes' list. Off, it is as before.
#[test]
fn the_one_path_namespace_takes_the_home_from_the_wrapper_and_checks_it() {
    for shown in SHOWN {
        let off = ns_text(shown);
        let on = Shown {
            one_path: true,
            ..shown
        };
        for (from, _) in ONE_PATH_NS_EDITS {
            assert_eq!(off.matches(from).count(), 1, "{shown:?}: {from}");
        }
        assert_eq!(off.matches(ONE_PATH_HOME_ANCHOR).count(), 1);
        let text = ns_text(on);
        assert_ne!(text, off);
        assert!(text.contains("\nH=${LAMP_NS_HOME-} W="));
        assert!(!text.contains("H=/var/lib/lampos-agents/$E "));
        assert!(text.contains(&one_path_home_check()));
        assert!(text.contains(" LAMP_NS_HOME\n"));
        assert!(text.find("LAMP_NS_HOME is not").unwrap() < text.find("mount -t tmpfs").unwrap());
        let named: Vec<&str> = text
            .lines()
            .filter(|line| line.to_lowercase().contains("hermes"))
            .collect();
        assert_eq!(named, [one_path_home_check().lines().nth(1).unwrap()]);
        assert_eq!(text.contains("control.sock"), shown.tasks);
        sh_n(&text);
    }
    // The check itself, run with the homes moved under a folder.
    let folder = tempfile::tempdir().unwrap();
    let top = fs::canonicalize(folder.path()).unwrap();
    let homes = top.join("agents");
    let own = top.join("own");
    fs::create_dir_all(homes.join("claude")).unwrap();
    fs::create_dir_all(&own).unwrap();
    symlink(&own, homes.join("linked")).unwrap();
    let check = one_path_home_check()
        .replace(HOMES, homes.to_str().unwrap())
        .replace(OWN_HOMES[0].1, own.to_str().unwrap());
    let check = check.split_once('\n').unwrap().1.to_owned();
    let ok = |engine: &str, home: &Path| {
        Command::new("/bin/sh")
            .args([
                "-c",
                &format!(
                    "fail() {{ echo \"$1\"; exit 1; }}\nE={engine} H={}\n{check}",
                    home.display()
                ),
            ])
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(ok("claude", &homes.join("claude")));
    assert!(ok(OWN_HOMES[0].0, &own), "an installer's own home");
    assert!(!ok("claude", &own), "another engine's own home");
    assert!(!ok("codex", &homes.join("claude")), "another agent's home");
    assert!(!ok("linked", &homes.join("linked")), "a link");
    assert!(!ok("claude", &top), "no agent's home");
    assert!(!ok("claude", &homes.join("claude/..")));
    assert!(!ok("missing", &homes.join("missing")), "not there");
}

/// (#6685) A round with the one run path writes it for a plain agent, and
/// the namespace script with it; either text this build writes is ours,
/// so turning the switch either way writes the wrapper again.
#[test]
fn write_engine_writes_the_one_path_with_its_switch() {
    let root = root();
    let on = Shown {
        one_path: true,
        ..SHOWN[3]
    };
    let found = Some("/usr/local/bin/eng");
    write_engine(root.path(), "eng", me(), on).unwrap();
    assert_eq!(
        fs::read_to_string(on_path(root.path(), "/usr/local/bin/eng")).unwrap(),
        one_path_text(&agent_of("eng").unwrap()).unwrap()
    );
    assert_eq!(
        fs::read_to_string(on_path(root.path(), NS)).unwrap(),
        ns_text(on)
    );
    assert!(present(root.path(), "eng", on, found));
    assert!(!present(root.path(), "eng", SHOWN[3], found));
    assert!(ours(root.path(), "eng"));
    assert_eq!(ours_written(root.path()), ["eng"]);
    write_engine(root.path(), "eng", me(), SHOWN[3]).unwrap();
    assert!(present(root.path(), "eng", SHOWN[3], found));
    assert!(ours(root.path(), "eng"));
}

fn on_path(root: &Path, path: &str) -> PathBuf {
    on(root, path)
}
