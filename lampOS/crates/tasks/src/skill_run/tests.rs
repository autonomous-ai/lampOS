use super::*;
use crate::skill_task::{Minted, Op, RunToken};
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

/// A stand-in for `unshare` where namespaces can't be made (a test's
/// machine): it takes the fence's arguments as the real one is given them,
/// makes no namespace, and does what the fence's last line does: the token
/// read from fd 3, `started` said on fd 4, the script run with the
/// fence's environment in the run's folder.
const FAKE_UNSHARE: &str = r#"#!/bin/sh
while [ "$1" != "--" ]; do shift; done
shift 4
r=$5 so=$7 py=$8 i=$9 s=${10}
IFS= read -r t <&3 || exit 1
exec 3<&-
printf 'started\n' >&4 || exit 1
exec 4>&-
cd "$r" || exit 1
exec env -i PATH=/usr/bin:/bin HOME="$r" LAMP_CONTROL_SOCKET="$so" PYTHONPATH="$py" LAMP_CONTROL_TOKEN="$t" "$i" "$s"
"#;

/// One that fails as a fence that couldn't be made, before `started`.
const FAILING_UNSHARE: &str =
    "#!/bin/sh\necho \"the run's fence was not made (/tmp): the script was not run\" >&2\nexit 1\n";

struct Place {
    dir: tempfile::TempDir,
}

impl Place {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn file(&self, name: &str, text: &str, mode: u32) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    /// A runner through `unshare`, its script run by `/bin/sh`.
    fn runner(&self, unshare: &str) -> DirectRunner {
        DirectRunner {
            unshare: self.file("unshare", unshare, 0o755),
            project: self.dir.path().join("runs"),
            hidden: vec![PathBuf::from("/nowhere")],
            interpreter: "/bin/sh".into(),
            tick: Duration::from_millis(5),
            most_output: MOST_OUTPUT,
        }
    }

    /// The skill `tide` with `script` as its `scripts/main.py`.
    fn hand(&self, script: &str) -> HandOver {
        self.file("skills/tide/scripts/main.py", script, 0o644);
        HandOver {
            op: Op::SkillRun,
            skill: "tide".into(),
            task: "run its own script".into(),
            run: "scripts/main.py".into(),
            folder: self.dir.path().join("skills/tide"),
            sha256: "a".repeat(64),
            grants: vec!["speak".into()],
        }
    }
}

fn run_of(hand: &HandOver, id: &str) -> SkillRun {
    let mut run = SkillRun::of_run(hand, id, &RunToken::new("tok-1".into()));
    run.deadline = RUN_WITHIN;
    run
}

/// Each of [`CONTROL_EDITS`] is in the gateway's fence exactly once, so
/// the run step's fence is the gateway's with lamp-control's socket alone
/// shown again: staged before anything is hidden, moved back after, and
/// nothing else changed.
#[test]
fn the_fence_is_the_gateways_with_its_control_socket_shown_alone() {
    let gateway = crate::gateway::fence_text(false);
    let mut back = gateway.clone();
    for (from, to) in CONTROL_EDITS {
        assert_eq!(gateway.matches(from).count(), 1, "{from}");
        back = back.replacen(from, to, 1);
    }
    assert_eq!(back, fence_text());
    let text = fence_text();
    let staged = text.find("mount -n --bind \"$so\" \"$t3\"").unwrap();
    let hidden = text.find("for h in \"$@\"; do").unwrap();
    let moved = text.find("mount -n --move \"$t3\" \"$so\"").unwrap();
    assert!(staged < hidden && hidden < moved, "{text}");
    // The rest is the gateway's own: one exec, in a user namespace of its
    // own, the token from fd 3.
    assert_eq!(text.matches("exec unshare --user").count(), 1);
    assert!(text.contains("IFS= read -r t <&3"));
    // `/var` and `/run` among what is hidden; lamp-control's admin socket
    // is under `/run`, never shown again.
    assert!(HIDDEN.contains(&"/var") && HIDDEN.contains(&"/run"));
    assert!(!text.contains("admin.sock"));
    assert!(Path::new(PROJECT).starts_with("/var"));
}

/// The fence's arguments: the gateway's namespaces (a network of `lo`
/// alone, a PID namespace that dies with it), the run step's fence text,
/// the skill's folder, its script and every hidden path; never the token.
#[test]
fn the_fences_arguments_hold_its_namespaces_and_never_the_token() {
    let place = Place::new();
    let runner = DirectRunner::lamp();
    let hand = place.hand("exit 0\n");
    let run = run_of(&hand, "task-1-2-3");
    let args: Vec<String> = runner
        .args(&run, Path::new("/var/lib/lampos-tasks/runs/run-task-1-2-3"))
        .into_iter()
        .map(|arg| arg.into_string().unwrap())
        .collect();
    for namespace in ["--user", "--net", "--mount", "--pid", "--kill-child"] {
        assert!(args.contains(&namespace.to_owned()), "{namespace}");
    }
    let text = fence_text().replace("\"$SCRIPT_PATH\"", crate::gateway::SCRIPT_PATH);
    assert!(args.contains(&text));
    assert!(args.contains(&hand.folder.display().to_string()));
    assert!(args.contains(&hand.folder.join("scripts/main.py").display().to_string()));
    assert!(args.contains(&"python3".to_owned()));
    for hidden in HIDDEN {
        assert!(args.contains(&hidden.to_owned()), "{hidden}");
    }
    assert!(!args.iter().any(|arg| arg.contains("tok-1")));
}

/// A script that ends well runs in its own folder with the fence's
/// environment alone, its token among it (from the pipe, never an
/// argument); the folder is gone after. The run's start is timed: from
/// the runner's call to the script's exec (#8158's latency, rule 7).
#[test]
fn a_script_that_ends_well_runs_in_its_own_folder_with_its_token() {
    let place = Place::new();
    let out = place.dir.path().join("seen");
    let hand = place.hand(&format!(
        "pwd > {out}; env | sort >> {out}\n",
        out = out.display()
    ));
    let runner = place.runner(FAKE_UNSHARE);
    let run = run_of(&hand, "task-1-2-3");
    let began = Instant::now();
    assert_eq!(runner.run(&run, &|| {}), Ok(0));
    let took = began.elapsed();
    assert!(run.started.load(Ordering::Acquire));
    let seen = std::fs::read_to_string(&out).unwrap();
    assert!(seen.contains("run-task-1-2-3"), "{seen}");
    assert!(seen.contains("LAMP_CONTROL_TOKEN=tok-1"), "{seen}");
    assert!(
        seen.contains(&format!(
            "LAMP_CONTROL_SOCKET={}",
            crate::skill_task::CONTROL_SOCKET
        )),
        "{seen}"
    );
    assert!(!place.dir.path().join("runs/run-task-1-2-3").exists());
    eprintln!(
        "#8158 timing: runner call to script end, fake fence: {} ms",
        took.as_millis()
    );
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// A script that fails, outlives its bound, or writes past its output
/// cap is ended and told so; one whose fence was never made never ran.
#[test]
fn a_script_that_fails_overruns_or_floods_is_ended_and_told_why() {
    let place = Place::new();
    let runner = place.runner(FAKE_UNSHARE);
    let failed = place.hand("exit 3\n");
    assert_eq!(
        runner.run(&run_of(&failed, "task-1-1-1"), &|| {}),
        Err("its script ended with status 3".into())
    );
    let slow = place.hand("sleep 30\n");
    let mut run = run_of(&slow, "task-1-1-2");
    run.deadline = Duration::from_millis(300);
    let began = Instant::now();
    assert_eq!(
        runner.run(&run, &|| {}),
        Err("its script was ended at its 0 s limit".into())
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    let mut small = place.runner(FAKE_UNSHARE);
    small.most_output = 1024;
    let flood = place.hand("while :; do echo 0123456789012345678901234567890123456789; done\n");
    assert_eq!(
        small.run(&run_of(&flood, "task-1-1-3"), &|| {}),
        Err("its script wrote more than 1 KiB, so it was ended".into())
    );
    let unmade = place.runner(FAILING_UNSHARE);
    let run = run_of(&place.hand("exit 0\n"), "task-1-1-4");
    let refused = unmade.run(&run, &|| {}).unwrap_err();
    assert!(!run.started.load(Ordering::Acquire));
    assert!(refused.starts_with("its fence was not made"), "{refused}");
    assert!(refused.contains("(/tmp)"), "{refused}");
}

/// lamp-control as a test sees it: each mint names `grants`, each end
/// says `said`.
#[derive(Default)]
struct Control {
    grants: Vec<String>,
    said: Option<u32>,
    minted: RefCell<Vec<String>>,
    ended: RefCell<Vec<String>>,
}

impl Tokens for Control {
    fn mint(&self, _skill: &str, run: &str) -> Result<Minted, String> {
        self.minted.borrow_mut().push(run.to_owned());
        Ok(Minted {
            token: RunToken::new("tok-1".into()),
            grants: self.grants.clone(),
        })
    }

    fn end(&self, run: &str) {
        self.ended.borrow_mut().push(run.to_owned());
    }

    fn end_said(&self, run: &str) -> Option<u32> {
        self.end(run);
        self.said
    }
}

/// A runner that only says how it ended.
struct Ends(Result<u64, String>, bool, RefCell<usize>);

impl Runner for Ends {
    fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
        *self.2.borrow_mut() += 1;
        assert!(run.prompt.is_empty(), "no agent, no words");
        assert_eq!(run.deadline, RUN_WITHIN);
        run.started.store(self.1, Ordering::Release);
        self.0.clone()
    }
}

/// A run step succeeds only when its script exited 0 and something it
/// asked `lamp.say` for was said; its token is minted from the owner's yes,
/// held to exactly the skill's grants, and ended however it ends.
#[test]
fn a_run_succeeds_only_when_its_script_ended_well_and_said_something() {
    let place = Place::new();
    let hand = place.hand("exit 0\n");
    let said = |ran: Result<u64, String>, began: bool, said: Option<u32>| {
        let control = Control {
            grants: vec!["speak".into()],
            said,
            ..Control::default()
        };
        let answered = answer(
            &hand,
            &control,
            &Ends(ran, began, RefCell::default()),
            false,
        );
        assert_eq!(*control.ended.borrow(), *control.minted.borrow());
        answered
    };
    assert_eq!(
        said(Ok(0), true, Some(2)),
        json!({"ok": true, "id": 0, "said": 2})
    );
    assert_eq!(
        said(Ok(0), true, Some(0)),
        refusal("run_failed", SAID_NOTHING)
    );
    assert_eq!(said(Ok(0), true, None)["error"], "run_failed");
    assert_eq!(
        said(Err("its script ended with status 3".into()), true, Some(1)),
        refusal("run_failed", "its script ended with status 3")
    );
    assert_eq!(
        said(Err("its fence was not made".into()), false, Some(0))["error"],
        "not_started"
    );
}

/// What its grants don't name, it never gets: a token naming other grants
/// than the skill's is ended at once and nothing runs; a run step granted
/// `net`, or whose skill names keys, is refused before any token (its
/// fence has only `lo`, and no keys are given). (lamp-control refuses the
/// script's own call outside its grants: `crates/control/tests/door.rs`'s
/// `a_token_with_light_sets_the_light_and_is_refused_the_camera`.)
#[test]
fn a_run_step_gets_its_skills_grants_and_nothing_more() {
    let place = Place::new();
    let mut hand = place.hand("exit 0\n");
    let runner = Ends(Ok(0), true, RefCell::default());
    let wider = Control {
        grants: vec!["speak".into(), "camera".into()],
        said: Some(1),
        ..Control::default()
    };
    assert_eq!(answer(&hand, &wider, &runner, false)["error"], "no_token");
    assert_eq!(*wider.ended.borrow(), *wider.minted.borrow());
    let control = Control {
        grants: vec!["speak".into()],
        said: Some(1),
        ..Control::default()
    };
    assert_eq!(
        answer(&hand, &control, &runner, true),
        refusal("bad_request", NO_KEYS_HERE)
    );
    hand.grants.push(crate::skill_task::NET_GRANT.into());
    assert_eq!(
        answer(&hand, &control, &runner, false),
        refusal("bad_request", NO_NET_HERE)
    );
    assert!(control.minted.borrow().is_empty());
    assert_eq!(*runner.2.borrow(), 0);
}
