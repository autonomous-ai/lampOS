use super::*;
use std::{
    os::unix::fs::{MetadataExt, symlink},
    path::PathBuf,
    process::{Command, Output},
};

const SHOWN: [Shown; 4] = [
    Shown {
        skills: false,
        tasks: false,
    },
    Shown {
        skills: true,
        tasks: false,
    },
    Shown {
        skills: false,
        tasks: true,
    },
    Shown {
        skills: true,
        tasks: true,
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
        "", "Hermes", "a/b", "../x", "..", "a b", "a;b", "a\nb", "$(id)", "a_b", "é", "a.b",
        &too_long,
    ] {
        assert!(!engine_ok(engine), "{engine:?}");
        assert!(wrapper_text(engine).is_err(), "{engine:?}");
        assert!(wrapper_path(engine).is_err(), "{engine:?}");
        assert!(home_of(engine).is_err(), "{engine:?}");
        let root = tempfile::tempdir().unwrap();
        assert!(write_engine(root.path(), engine, (0, 0), SHOWN[0]).is_err());
        assert!(!present(root.path(), engine, SHOWN[0]));
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
/// argument in the wrapper's own text.
#[test]
fn the_token_is_only_assigned_and_exported() {
    let text = wrapper_text("eng").unwrap();
    let uses: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("$T") || l.contains("${T"))
        .map(str::trim)
        .collect();
    assert_eq!(
        uses,
        [
            "[ -n \"$T\" ] || fail \"no token for run $LAMP_RUN_ID\"".to_owned(),
            format!("{TOKEN_ENV}=$T")
        ]
    );
    assert!(text.contains(&format!("export {TOKEN_ENV}\n")));
    assert!(text.contains(&format!("F={RUNS}/${RUN_ID_ENV}\n")));
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
    let folder = tempfile::tempdir().unwrap();
    let top = fs::canonicalize(folder.path()).unwrap();
    for dir in ["runs", "bin1", "bin2", "bin3", "out"] {
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
    let text = wrapper_text(engine)
        .unwrap()
        .replace(RUNS, top.join("runs").to_str().unwrap())
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
    fn run(&self, env: &[(&str, &str)], path: &str) -> Output {
        let _ = fs::remove_file(self.top.join("out/argv"));
        let _ = fs::remove_file(self.top.join("out/env"));
        Command::new(&self.wrapper)
            .args(["chat", "-q", "hi"])
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

/// (#6510) A skill's run: the token file is read once and deleted, the
/// token reaches the engine in its environment only, never in argv; the
/// run gets `--net`; the program is the first `<engine>` on `PATH` that is
/// neither the wrapper nor another marked wrapper.
#[test]
fn a_runs_token_file_is_read_once_deleted_and_never_in_argv() {
    let lamp = lamp("eng");
    let file = lamp.token_file("run-1");
    let out = lamp.run(&[(RUN_ID_ENV, "run-1")], &lamp.path());
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
    assert!(
        lines.contains(&format!("{TOKEN_ENV}={TOKEN}").as_str()),
        "{env}"
    );
    assert!(lines.contains(&"LAMP_NS_NET=run"));
    assert!(lines.contains(&"LAMP_NS_ENGINE=eng"));
    assert!(lines.contains(&format!("LAMP_NS_ENGINE_BIN={}", lamp.real.display()).as_str()));
    assert!(!env.contains(RUN_ID_ENV), "{env}");

    // Once only: the same run again finds no token and runs nothing.
    let again = lamp.run(&[(RUN_ID_ENV, "run-1")], &lamp.path());
    assert!(!again.status.success());
    assert!(lamp.read("argv").is_none());
    assert!(String::from_utf8_lossy(&again.stderr).contains("no token for run run-1"));
}

/// Not a run: no token (even one already in the caller's environment), no
/// `--net` (even `LAMP_NS_NET=run` from the caller), and no run file read.
#[test]
fn without_a_run_there_is_no_token_and_no_net() {
    let lamp = lamp("eng");
    let file = lamp.token_file("run-2");
    let out = lamp.run(
        &[(TOKEN_ENV, "from-the-caller"), ("LAMP_NS_NET", "run")],
        &lamp.path(),
    );
    assert!(out.status.success(), "{out:?}");
    assert!(file.exists(), "another run's file is left alone");
    let argv = lamp.read("argv").unwrap();
    let env = lamp.read("env").unwrap();
    assert!(!argv.contains("--net"));
    assert!(
        !env.contains(TOKEN_ENV) && !env.contains("LAMP_NS_NET"),
        "{env}"
    );
}

#[test]
fn a_run_id_that_is_not_plain_or_a_linked_token_file_runs_nothing() {
    let lamp = lamp("eng");
    for run in ["", "../x", "Run", "a/b", &"a".repeat(65)] {
        let out = lamp.run(&[(RUN_ID_ENV, run)], &lamp.path());
        assert!(!out.status.success(), "{run:?}");
        assert!(lamp.read("argv").is_none(), "{run:?}");
    }
    // A link in the runs folder is never read through, and is removed.
    let elsewhere = lamp.top.join("elsewhere");
    fs::write(&elsewhere, TOKEN).unwrap();
    let link = lamp.top.join("runs/run-3");
    symlink(&elsewhere, &link).unwrap();
    let out = lamp.run(&[(RUN_ID_ENV, "run-3")], &lamp.path());
    assert!(!out.status.success());
    assert!(lamp.read("argv").is_none());
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
        assert!(!present(root.path(), engine, shown));
        write_engine(root.path(), engine, me(), shown).unwrap();
        assert!(present(root.path(), engine, shown));
        assert!(
            !present(root.path(), engine, SHOWN[0]),
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
    assert!(!present(root.path(), "eng", shown));
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
