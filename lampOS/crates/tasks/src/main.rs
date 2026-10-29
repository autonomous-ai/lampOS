use lamp_tasks::runtime::{
    Config, DAEMON_RETRY_AFTER, HarnessSwitches, REPORT_EVERY, SET_UP_LOOK_EVERY, SetUpLater,
    SystemClock, read_targets_or_newer, request, serve,
};
use std::{
    io::{self, Read},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or("usage: lamp-tasks serve [--enable ...] | request --socket ABS")?;
    if command == "request" {
        if args.next().as_deref() != Some("--socket") {
            return Err("request requires --socket ABS".into());
        }
        let path = PathBuf::from(args.next().ok_or("--socket needs a path")?);
        if args.next().is_some() {
            return Err("unexpected request option".into());
        }
        let mut bytes = Vec::new();
        io::stdin()
            .take(lamp_tasks::protocol::MAX_LINE_BYTES as u64 + 2)
            .read_to_end(&mut bytes)?;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.len() > lamp_tasks::protocol::MAX_LINE_BYTES {
            return Err("request exceeds 8192 bytes".into());
        }
        // Validate before converting to Value, whose map would silently keep
        // only the last copy of a duplicated operation or guard field.
        let typed = lamp_tasks::protocol::Request::parse(&bytes).map_err(io::Error::other)?;
        let value = serde_json::to_value(typed)?;
        println!("{}", request(&path, &value, Duration::from_millis(1500))?);
        return Ok(());
    }
    if command != "serve" {
        return Err("expected serve or request".into());
    }
    let mut enabled = false;
    let mut dir = None;
    let mut socket = None;
    let mut voice_socket = None;
    let mut executable = None;
    let mut targets = None;
    let mut boot_id = None;
    let mut bus_root = PathBuf::from(lamp_bus::DEFAULT_ROOT);
    let mut run_for = None;
    let mut as_person = false;
    // On unless `--no-watch`: confirmed on lamp-4ace on 2026-10-15
    // (docs/joining.md). `--watch` is still taken, and changes nothing.
    let mut watch = true;
    let mut progress = false;
    let mut push_wait = false;
    let mut close_told = false;
    let mut set_up_later = false;
    let mut sign_in_by_code = false;
    let mut folder_missing = false;
    let mut route = false;
    let mut engines_ready = false;
    let mut from_machines = None;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        match arg.as_str() {
            "--enable" => enabled = true,
            "--harness-as-person" => as_person = true,
            "--watch" => watch = true,
            "--no-watch" => watch = false,
            "--progress" => progress = true,
            "--push-wait" => push_wait = true,
            "--close-told" => close_told = true,
            "--set-up-later" => set_up_later = true,
            "--sign-in-by-code" => sign_in_by_code = true,
            "--folder-missing" => folder_missing = true,
            "--route" => route = true,
            "--engines-ready" => engines_ready = true,
            "--targets-from-machines" => {
                let engine = args
                    .next()
                    .ok_or("--targets-from-machines needs claude, codex or opencode")?;
                from_machines = Some(
                    serde_json::from_value::<lamp_tasks::harness::EngineKind>(
                        serde_json::Value::String(engine),
                    )
                    .map_err(|_| "--targets-from-machines needs claude, codex or opencode")?,
                );
            }
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a path")?)),
            "--socket" => socket = Some(PathBuf::from(args.next().ok_or("--socket needs a path")?)),
            "--voice-socket" => {
                voice_socket = Some(PathBuf::from(
                    args.next().ok_or("--voice-socket needs a path")?,
                ))
            }
            "--harness" => {
                executable = Some(PathBuf::from(
                    args.next()
                        .ok_or("--harness needs an absolute executable path")?,
                ))
            }
            "--targets" => {
                targets = Some(PathBuf::from(args.next().ok_or("--targets needs a path")?))
            }
            "--bus-root" => bus_root = PathBuf::from(args.next().ok_or("--bus-root needs a path")?),
            "--boot-id" => boot_id = Some(args.next().ok_or("--boot-id needs an identity")?),
            "--run-for-ms" => {
                let ms = args
                    .next()
                    .ok_or("--run-for-ms needs milliseconds")?
                    .parse::<u64>()?;
                if !(1..=86_400_000).contains(&ms) {
                    return Err("--run-for-ms must be 1..86400000".into());
                }
                run_for = Some(Duration::from_millis(ms));
            }
            _ => return Err("unknown option".into()),
        }
    }
    if from_machines.is_some() && !set_up_later {
        return Err("--targets-from-machines writes targets only with --set-up-later".into());
    }
    if push_wait && !watch {
        return Err("--push-wait reads with the watch; it cannot go with --no-watch".into());
    }
    if !enabled {
        println!(
            "{{\"enabled\":false,\"message\":\"Tasks is off; supply --enable to start it.\"}}"
        );
        return Ok(());
    }
    // Targets written for a newer build: the part runs with hand-overs off
    // and says so, rather than stopping and saying nothing (#1505).
    let targets_path = targets.ok_or("--targets is required")?;
    let read = match read_targets_or_newer(&targets_path) {
        // With `--set-up-later` no file is no computer yet: the part runs
        // with hand-overs off, and uses the file once it is written (#1676).
        Err(error) if set_up_later && error.kind() == io::ErrorKind::NotFound => {
            eprintln!("tasks: no task targets yet; hand-overs are off until they are written");
            Ok(Default::default())
        }
        read => read?,
    };
    let (targets, targets_newer) = match read {
        Ok(targets) => (targets, None),
        Err(version) => {
            eprintln!(
                "tasks: the task targets were written for a newer build or mistyped (version {version}); hand-overs are off until they match this build"
            );
            (Default::default(), Some(version))
        }
    };
    let config = Config {
        enabled,
        dir: dir.ok_or("--dir is required")?,
        socket: socket.ok_or("--socket is required")?,
        voice_socket: voice_socket.ok_or("--voice-socket is required")?,
        bus_root,
        targets,
        run_for,
        watch,
        progress,
        push_wait,
        close_told,
        targets_newer,
        set_up_later: set_up_later.then_some(SetUpLater {
            targets: targets_path,
            every: SET_UP_LOOK_EVERY,
            from_machines,
        }),
        sign_in_by_code,
        report_every: REPORT_EVERY,
        daemon_retry_after: DAEMON_RETRY_AFTER,
    };
    let clock = SystemClock::new(boot_id.as_deref())?;
    let executable = executable.ok_or("--harness is required")?;
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    serve(
        config,
        clock,
        executable,
        HarnessSwitches {
            as_person,
            folder_missing,
            engines_ready,
            route,
        },
        stop,
    )?;
    Ok(())
}

fn main() {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    if let Err(error) = run() {
        // An error is also a shutdown path. A full stderr pipe or blocked
        // logging filesystem must not keep the service alive after it has
        // withdrawn readiness. Exit even if this one writer cannot finish.
        // One deadline covers both the writer and the relay: the line is
        // queued before an empty relay is taken as finished (#876).
        let deadline = Instant::now() + Duration::from_millis(500);
        let message = format!("tasks: {error}");
        let (done, received) = std::sync::mpsc::sync_channel(1);
        if std::thread::Builder::new()
            .name("tasks-exit-report".into())
            .spawn(move || {
                // Test seam (debug builds only): a writer that is slow to run.
                #[cfg(debug_assertions)]
                if let Some(ms) = std::env::var("LAMP_TASKS_TEST_SLOW_EXIT_REPORT_MS")
                    .ok()
                    .and_then(|ms| ms.parse().ok())
                {
                    std::thread::sleep(Duration::from_millis(ms));
                }
                eprintln!("{message}");
                let _ = done.try_send(());
            })
            .is_ok()
        {
            let _ = received.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        }
        // `exit` runs no destructor, so the guard would not pass the line on.
        lamp_ipc::log_relay::finish(deadline.saturating_duration_since(Instant::now()));
        std::process::exit(1);
    }
}
