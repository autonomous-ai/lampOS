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
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("cgroup")).unwrap();
        Self { dir }
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
            egress: None,
            cgroups: crate::run_cgroup::RunCgroups::Fake(self.dir.path().join("cgroup")),
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

/// Each of [`CONTROL_EDITS`] and [`CAPS_EDIT`] (and, for `net`,
/// [`RELAY_EDITS`]) is in the gateway's fence exactly once, so the run
/// step's fence is the gateway's with lamp-control's socket alone shown
/// again: staged before anything is hidden, moved back after, and nothing
/// else changed.
#[test]
fn the_fence_is_the_gateways_with_its_control_socket_shown_alone() {
    for net in [false, true] {
        let gateway = crate::gateway::fence_text(net);
        let mut back = gateway.clone();
        let relay: &[(&str, &str)] = if net { &RELAY_EDITS } else { &[] };
        for (from, to) in CONTROL_EDITS.iter().chain(relay).chain([&CAPS_EDIT]) {
            assert_eq!(back.matches(from).count(), 1, "{from}");
            back = back.replacen(from, to, 1);
        }
        assert_eq!(back, fence_text(net));
        assert_eq!(
            fence_text(net).contains(crate::gateway::RELAY),
            net,
            "only a net run's fence shows lamp-tasks' relay"
        );
    }
    for (_, to) in RELAY_EDITS {
        assert!(to.contains(crate::gateway::RELAY), "{to}");
    }
    let text = fence_text(false);
    let staged = text.find("mount -n --bind \"$so\" \"$t3\"").unwrap();
    let hidden = text.find("for h in \"$@\"; do").unwrap();
    let moved = text.find("mount -n --move \"$t3\" \"$so\"").unwrap();
    assert!(staged < hidden && hidden < moved, "{text}");
    // The rest is the gateway's own: one exec, in a user namespace of its
    // own, the token from fd 3.
    assert_eq!(text.matches("exec nice -n 10 unshare --user").count(), 1);
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
    let args_of = |egress: Option<&Path>| -> Vec<String> {
        runner
            .args(
                &run,
                Path::new("/var/lib/lampos-tasks/runs/run-task-1-2-3"),
                egress,
            )
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect()
    };
    let args = args_of(None);
    for namespace in ["--user", "--net", "--mount", "--pid", "--kill-child"] {
        assert!(args.contains(&namespace.to_owned()), "{namespace}");
    }
    let text = fence_text(false).replace("\"$SCRIPT_PATH\"", crate::gateway::SCRIPT_PATH);
    assert!(args.contains(&text));
    // Granted `net`: the fence with its way out, its socket never an
    // argument (it comes on fd 5).
    let socket = Path::new("/run/lampos-tasks/egress/skill-task-1-2-3.sock");
    let with_net = args_of(Some(socket));
    let net_text = fence_text(true).replace("\"$SCRIPT_PATH\"", crate::gateway::SCRIPT_PATH);
    assert!(with_net.contains(&net_text));
    assert!(net_text.contains("HTTPS_PROXY=http://127.0.0.1:3128"));
    assert!(net_text.contains("/proc/self/fd/5"));
    assert!(
        !with_net
            .iter()
            .any(|arg| arg.contains("skill-task-1-2-3.sock"))
    );
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
    assert!(runner.run(&run, &|| {}).is_ok());
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

/// #8180: a run step runs in its own cgroup, which its fence's first
/// process joins before its exec (`0` written to its `cgroup.procs`), and
/// which is ended with the run however it ended: well, failed or past
/// its bound.
#[test]
fn a_run_step_runs_in_its_own_cgroup_ended_with_it() {
    let place = Place::new();
    let runner = place.runner(FAKE_UNSHARE);
    let mut over = run_of(&place.hand("sleep 5\n"), "task-8-1-3");
    over.deadline = Duration::from_millis(200);
    assert!(runner.run(&over, &|| {}).is_err());
    let well = run_of(&place.hand("exit 0\n"), "task-8-1-1");
    assert!(runner.run(&well, &|| {}).is_ok());
    let failed = run_of(&place.hand("exit 3\n"), "task-8-1-2");
    assert!(runner.run(&failed, &|| {}).is_err());
    let cgroups = place.dir.path().join("cgroup");
    for id in ["task-8-1-1", "task-8-1-2", "task-8-1-3"] {
        assert!(!cgroups.join(format!("run-{id}")).exists(), "{id}");
        let joined = std::fs::read_to_string(cgroups.join(format!("done-{id}/cgroup.procs")));
        assert_eq!(joined.unwrap(), "0", "{id}");
        let max = std::fs::read_to_string(cgroups.join(format!("done-{id}/memory.max")));
        assert_eq!(max.unwrap(), "512M", "{id}");
    }
}

/// #8180 (fail closed): a run step whose own cgroup can't be made is
/// refused with [`crate::run_cgroup::NO_CGROUP`], its script never run.
#[test]
fn a_run_step_without_its_own_cgroup_is_refused_and_never_runs() {
    let place = Place::new();
    let out = place.dir.path().join("ran");
    let hand = place.hand(&format!("touch {}\n", out.display()));
    let mut runner = place.runner(FAKE_UNSHARE);
    runner.cgroups = crate::run_cgroup::RunCgroups::Fake(place.dir.path().join("no-such-root"));
    let run = run_of(&hand, "task-8-2-1");
    assert_eq!(
        runner.run(&run, &|| {}),
        Err(crate::run_cgroup::NO_CGROUP.to_owned())
    );
    assert!(!out.exists());
    assert!(!run.started.load(Ordering::Acquire));
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
    /// The owner's yes names `net` for the folder as it is.
    net_yes: bool,
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

    fn net_agreed(&self, _: &str, _: &str, grants: &[String]) -> bool {
        self.net_yes
            && grants
                .iter()
                .any(|grant| grant == crate::skill_task::NET_GRANT)
    }

    fn end_said(&self, run: &str) -> Option<u32> {
        self.end(run);
        self.said
    }
}

/// A runner that only says how it ended; it keeps whether each run was
/// granted `net`.
struct Ends(
    Result<u64, String>,
    bool,
    RefCell<usize>,
    RefCell<Vec<bool>>,
);

fn ends(ran: Result<u64, String>, began: bool) -> Ends {
    Ends(ran, began, RefCell::default(), RefCell::default())
}

impl Runner for Ends {
    fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
        *self.2.borrow_mut() += 1;
        self.3.borrow_mut().push(run.net);
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
        let answered = answer(&hand, &control, &ends(ran, began), false);
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
/// than the skill's is ended at once and nothing runs; a run step whose
/// skill names keys is refused before any token (no keys are given). (lamp-control refuses the
/// script's own call outside its grants: `crates/control/tests/door.rs`'s
/// `a_token_with_light_sets_the_light_and_is_refused_the_camera`.)
#[test]
fn a_run_step_gets_its_skills_grants_and_nothing_more() {
    let place = Place::new();
    let hand = place.hand("exit 0\n");
    let runner = ends(Ok(0), true);
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
    assert!(control.minted.borrow().is_empty());
    assert_eq!(*runner.2.borrow(), 0);
}

/// A run step granted `net` runs with its way out only with the owner's
/// yes naming net for its folder as it is, the same yes as a net task's:
/// without it, refused before any token and its runner never runs; with
/// it, its token names the skill's other grants (net is lamp-tasks' own)
/// and its run is granted net. Keys stay refused.
#[test]
fn a_net_run_step_gets_its_egress_socket_only_with_its_yes() {
    let place = Place::new();
    let mut hand = place.hand("exit 0\n");
    hand.grants.push(crate::skill_task::NET_GRANT.into());
    let control = |net_yes| Control {
        grants: vec!["speak".into()],
        said: Some(1),
        net_yes,
        ..Control::default()
    };
    let runner = ends(Ok(0), true);
    let no = control(false);
    assert_eq!(
        answer(&hand, &no, &runner, false),
        refusal("no_token", NO_NET_YES)
    );
    assert!(no.minted.borrow().is_empty());
    assert_eq!(*runner.2.borrow(), 0);
    let yes = control(true);
    assert_eq!(
        answer(&hand, &yes, &runner, false),
        json!({"ok": true, "id": 0, "said": 1})
    );
    assert_eq!(*runner.3.borrow(), [true]);
    assert_eq!(
        answer(&hand, &yes, &runner, true),
        refusal("bad_request", NO_KEYS_HERE)
    );
    // A run step not granted net is never granted it.
    let plain = place.hand("exit 0\n");
    assert_eq!(answer(&plain, &yes, &runner, false)["ok"], true);
    assert_eq!(*runner.3.borrow(), [true, false]);
}

/// The runner gives a net run its own egress socket, on fd 5 of its fence,
/// only when it holds lamp-tasks' egress grant (`LAMP_TASK_NET` on); with
/// none the run is refused before its fence starts, nothing made.
#[test]
fn a_net_run_without_lamp_tasks_egress_is_refused_before_its_fence() {
    let place = Place::new();
    let seen = place.dir.path().join("seen");
    let hand = place.hand(&format!("echo ran > {}\n", seen.display()));
    let runner = place.runner(FAKE_UNSHARE);
    let mut run = run_of(&hand, "task-1-5-1");
    run.net = true;
    let refused = runner.run(&run, &|| {}).unwrap_err();
    assert!(refused.contains(NET_SWITCH), "{refused}");
    assert!(!run.started.load(Ordering::Acquire));
    assert!(!seen.exists());
}

/// On Linux, with lamp-tasks' egress grant, a net run's fence holds its
/// own egress socket on fd 5 while it runs, and the socket is gone after.
#[cfg(target_os = "linux")]
#[test]
fn a_net_run_holds_its_own_egress_socket_on_fd_5() {
    use std::os::unix::fs::DirBuilderExt;
    let place = Place::new();
    let seen = place.dir.path().join("seen");
    let unshare = FAKE_UNSHARE.replacen(
        "IFS= read",
        &format!(
            "if [ -S /proc/self/fd/5 ]; then echo socket > {seen}; fi\nIFS= read",
            seen = seen.display()
        ),
        1,
    );
    let egress = place.dir.path().join("egress");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&egress)
        .unwrap();
    let mut runner = place.runner(&unshare);
    runner.egress = Some(crate::egress::Grant {
        dir: egress.clone(),
        ..crate::egress::Grant::lamp()
    });
    let mut run = run_of(&place.hand("exit 0\n"), "task-1-5-2");
    run.net = true;
    assert!(runner.run(&run, &|| {}).is_ok());
    assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), "socket");
    assert!(!egress.join("skill-task-1-5-2.sock").exists());
}

/// The script's own caps, as a gateway agent's script gets them, set in
/// its fence just before its exec, after its relay: its memory
/// ([`SCRIPT_MEMORY_KB`]), its CPU time ([`SCRIPT_CPU_SECS`], its bound's)
/// and its niceness ([`SCRIPT_NICE`]) for each process, and the whole
/// run's processes and threads ([`SCRIPT_PROCESSES`], #8180), so the
/// per-process caps bound the whole; never lamp-tasks' whole budget.
#[test]
fn a_run_steps_script_has_its_own_memory_and_cpu_caps() {
    assert_eq!(SCRIPT_CPU_SECS, RUN_WITHIN.as_secs());
    for net in [false, true] {
        let text = fence_text(net);
        let memory = format!("ulimit -v {SCRIPT_MEMORY_KB} ||");
        let cpu = format!("ulimit -t {SCRIPT_CPU_SECS} ||");
        let nice = format!("exec nice -n {SCRIPT_NICE} unshare --user");
        let processes =
            format!("prlimit --nproc={SCRIPT_PROCESSES}:{SCRIPT_PROCESSES} --pid $$ ||");
        for cap in ["ulimit -c 0 ||", &memory, &cpu, &processes, &nice] {
            assert_eq!(text.matches(cap).count(), 1, "{cap}\n{text}");
        }
        let capped = text.find(&memory).unwrap();
        assert!(capped > text.find("cd \"$r\"").unwrap(), "{text}");
        // The process cap is set just before the script's own user
        // namespace is made, so it is counted in that namespace alone.
        assert!(text.find(&processes).unwrap() < text.find(&nice).unwrap());
        // Never `ulimit -u`: dash, a lamp's /bin/sh, has no such flag.
        assert!(!text.contains("ulimit -u"), "{text}");
        if net {
            assert!(capped > text.find("net-relay").unwrap(), "{text}");
        }
        assert_eq!(text.matches("exec unshare --user").count(), 0);
    }
    let place = Place::new();
    let run = run_of(&place.hand("exit 0\n"), "task-1-5-3");
    let args = DirectRunner::lamp().args(&run, Path::new("/var/lib/lampos-tasks/runs/x"), None);
    assert!(args.iter().any(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg.contains("ulimit -v 786432"))
    }));
}

/// #8180 (Heron 3): on Linux, a script that starts more processes than
/// its run's cap ([`SCRIPT_PROCESSES`], the fence's own `prlimit` line,
/// set as the fence sets it: in a user namespace, before the script's
/// nested one) is refused the extra ones and its run ends `run_failed`,
/// while another task in lamp-tasks still starts and answers. Skipped
/// where nested user namespaces, `prlimit` or `python3` can't be had.
#[cfg(target_os = "linux")]
#[test]
fn a_script_past_its_process_cap_fails_its_run_while_another_task_answers() {
    use std::process::Command;
    let works = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    if !works(
        "unshare",
        &[
            "--user",
            "--map-root-user",
            "--",
            "unshare",
            "--user",
            "--",
            "true",
        ],
    ) || !works("prlimit", &["--version"])
        || !works("python3", &["-c", "pass"])
    {
        eprintln!("skipped: no nested user namespace, prlimit or python3 here");
        return;
    }
    let place = Place::new();
    // The fence's own cap line, as its text has it.
    let cap = CAPS_EDIT
        .1
        .lines()
        .find(|line| line.starts_with("prlimit"))
        .unwrap();
    let inner = place.file(
        "inner.sh",
        &format!(
            "no() {{ echo \"$1\" >&2; exit 1; }}\n{cap}\nexec unshare --user --map-user=\"$1\" --map-group=\"$2\" -- env -i PATH=/usr/bin:/bin \"$3\" \"$4\"\n"
        ),
        0o644,
    );
    let unshare = FAKE_UNSHARE.replacen(
        "exec env -i PATH=/usr/bin:/bin HOME=\"$r\" LAMP_CONTROL_SOCKET=\"$so\" PYTHONPATH=\"$py\" LAMP_CONTROL_TOKEN=\"$t\" \"$i\" \"$s\"",
        &format!(
            "exec unshare --user --map-root-user -- /bin/sh {} \"$(id -u)\" \"$(id -g)\" \"$i\" \"$s\"",
            inner.display()
        ),
        1,
    );
    assert_ne!(unshare, FAKE_UNSHARE);
    let refused = place.dir.path().join("refused");
    let mut forks = place.runner(&unshare);
    forks.interpreter = "python3".into();
    let hand = place.hand(&format!(
        "import os, sys, time\n\
         for n in range({}):\n    \
             try:\n        pid = os.fork()\n    \
             except OSError:\n        \
                 open({:?}, 'w').write(str(n))\n        time.sleep(2)\n        sys.exit(3)\n    \
             if pid == 0:\n        time.sleep(20)\n        os._exit(0)\n\
         sys.exit(0)\n",
        SCRIPT_PROCESSES * 4,
        refused.display().to_string()
    ));
    let forked = std::thread::spawn(move || {
        let control = Control {
            grants: vec!["speak".into()],
            said: Some(1),
            ..Control::default()
        };
        answer(&hand, &control, &forks, false)
    });
    let began = Instant::now();
    while !refused.exists() {
        assert!(began.elapsed() < Duration::from_secs(20), "never refused");
        std::thread::sleep(Duration::from_millis(20));
    }
    let refused_at: u64 = std::fs::read_to_string(&refused).unwrap().parse().unwrap();
    assert!(refused_at < SCRIPT_PROCESSES, "{refused_at}");
    // Meanwhile another task still starts and answers.
    let other = Place::new();
    let runner = other.runner(FAKE_UNSHARE);
    let quick = Instant::now();
    assert!(
        runner
            .run(&run_of(&other.hand("exit 0\n"), "task-8-1-80"), &|| {})
            .is_ok()
    );
    assert!(quick.elapsed() < Duration::from_secs(5));
    assert_eq!(
        forked.join().unwrap(),
        refusal("run_failed", "its script ended with status 3")
    );
}
