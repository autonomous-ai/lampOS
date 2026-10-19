//! `lamp-setup`: a lamp's first boot and its way back there (#806).
use lamp_bus::Bus;
use lamp_msgs::TouchEvent;
use lamp_setup::{
    light::Light,
    reset::{self, Effects, Plan, System},
    serve::Status,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const NAME: &str = "setup";

const USAGE: &str = "\
usage: lamp-setup COMMAND [OPTIONS]
  serve --config FILE --plan FILE [--seconds N] [--enable]
      The lamp's first boot, started by the keeper when the setup marker is
      missing: finish an interrupted reset, bring up the lamp's hotspot and
      answer V1's setup API until the home's Wi-Fi is joined and the
      person's settings are saved; then write the marker, and go on
      answering for 10 minutes so the page can follow the lamp.
  check --plan FILE
      Check a reset plan and print it. Touches nothing.
  watch-reset --plan FILE [--bus-root PATH] [--clock-id ID] [--seconds N] [--enable]
      Watch controls/touch for the reset button held 5 s and released. With
      --enable: stop the lamp, wipe the person's data, remove the setup
      marker and reboot. Without it, a valid reset is only logged.
  reset-now --plan FILE [--enable]
      Reset now: what watch-reset hands to a unit of its own when the reset
      button is held. Without --enable it does nothing.
  resume-reset --plan FILE [--enable]
      At start, before the lamp: finish a reset a power cut interrupted.
      Without --enable it only says whether one is unfinished.
  change --config FILE [--bus-root PATH] [--enable]
      Root's helper for changes after setup (#1959): one request line on
      standard input from lamp-cloud, a name or the time zone, checked and
      written with setup's own writers; one reply line on standard output.
      A zone change then restarts lamp-schedule once the lamp is quiet.
      Without --enable every request is refused.
  wake
      From power-on until the voice asks for the ring: the waking face
      (#1681). Only with LAMP_WAKING_FACE=on; otherwise it exits at once.

The plan names the setup marker, the reset journal, the paths to wipe and
the stop and reboot commands (docs/setup.md, \"The reset\").
";

fn options(
    given: impl Iterator<Item = String>,
) -> Result<(BTreeMap<String, String>, bool), String> {
    let mut given = given.peekable();
    let mut options = BTreeMap::new();
    let mut enabled = false;
    while let Some(key) = given.next() {
        if key == "--enable" {
            if enabled {
                return Err("--enable given twice".into());
            }
            enabled = true;
            continue;
        }
        if ![
            "--plan",
            "--config",
            "--bus-root",
            "--clock-id",
            "--seconds",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown option {key}"));
        }
        let value = given.next().ok_or(format!("{key} needs a value"))?;
        if options.insert(key.clone(), value).is_some() {
            return Err(format!("{key} given twice"));
        }
    }
    Ok((options, enabled))
}

fn plan(path: &Path) -> Result<Plan, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("the plan exceeds 16 KiB".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes)?;
    plan.check()?;
    Ok(plan)
}

fn clock_id(given: Option<&String>) -> io::Result<String> {
    match given {
        Some(id) => Ok(id.clone()),
        None => Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_owned()),
    }
}

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let mut given = std::env::args().skip(1);
    let command = given.next().ok_or("a command is required")?;
    if command == "--help" || command == "-h" {
        print!("{USAGE}");
        return Ok(true);
    }
    if command == "wake" {
        if given.next().is_some() {
            return Err("wake takes no options".into());
        }
        return wake();
    }
    if command == "change" {
        return change(given);
    }
    let (mut options, enabled) = options(given)?;
    let plan_path = PathBuf::from(options.remove("--plan").ok_or("--plan is required")?);
    let plan = plan(&plan_path)?;
    match command.as_str() {
        "check" => {
            if enabled || !options.is_empty() {
                return Err("check takes only --plan".into());
            }
            println!("{}", serde_json::to_string(&plan)?);
            Ok(true)
        }
        "resume-reset" => {
            if !options.is_empty() {
                return Err("resume-reset takes --plan and --enable".into());
            }
            let unfinished = plan.journal.exists();
            if !enabled {
                println!(
                    "{}",
                    serde_json::json!({"enabled": false, "unfinished": unfinished})
                );
                return Ok(true);
            }
            let steps = reset::resume(&plan, &mut System)?;
            if !steps.is_empty() {
                eprintln!("lamp-setup: finished a reset a power cut had interrupted");
            }
            println!("{}", serde_json::json!({"enabled": true, "steps": steps}));
            Ok(true)
        }
        "watch-reset" => {
            let bus = match options.remove("--bus-root") {
                Some(root) => Bus::new(root)?,
                None => Bus::from_environment()?,
            };
            let clock = clock_id(options.get("--clock-id"))?;
            options.remove("--clock-id");
            let until = match options.remove("--seconds") {
                Some(seconds) => {
                    let seconds: u64 = seconds
                        .parse()
                        .ok()
                        .filter(|n| (1..=86_400).contains(n))
                        .ok_or("--seconds must be 1..86400")?;
                    Some(Instant::now() + Duration::from_secs(seconds))
                }
                None => None,
            };
            if !options.is_empty() {
                return Err("watch-reset does not take that option".into());
            }
            let act = || start_reset(&plan, &plan_path);
            // A power cut right after a reset's journal, before its marker
            // went, leaves both: the lamp started, so carry on with it here.
            if enabled && plan.journal.exists() {
                eprintln!("lamp-setup: an unfinished reset was found; carrying on with it");
                act()?;
                return Ok(true);
            }
            let mut touch = bus.subscribe_typed(TouchEvent::TOPIC, NAME, TouchEvent::TYPE)?;
            let watched = lamp_setup::watch(
                &mut touch,
                &clock,
                enabled,
                until,
                lamp_ipc::monotonic_us,
                |line| eprintln!("lamp-setup: {line}"),
                act,
            )?;
            println!(
                "{}",
                serde_json::json!({"enabled": enabled, "watched": format!("{watched:?}")})
            );
            Ok(true)
        }
        "reset-now" => {
            if !options.is_empty() {
                return Err("reset-now takes --plan and --enable".into());
            }
            if !enabled {
                println!(
                    "{}",
                    serde_json::json!({"enabled": false, "reason": "--enable was not set"})
                );
                return Ok(true);
            }
            let steps = reset::reset(&plan, &mut System)?;
            println!("{}", serde_json::json!({"enabled": true, "steps": steps}));
            Ok(true)
        }
        "serve" => {
            let config_path =
                PathBuf::from(options.remove("--config").ok_or("--config is required")?);
            let seconds = match options.remove("--seconds") {
                Some(seconds) => Some(
                    seconds
                        .parse::<u64>()
                        .ok()
                        .filter(|n| (1..=86_400).contains(n))
                        .ok_or("--seconds must be 1..86400")?,
                ),
                None => None,
            };
            if !options.is_empty() {
                return Err("serve takes --config, --plan, --seconds and --enable".into());
            }
            let config = serve_config(&config_path)?;
            if !enabled {
                println!(
                    "{}",
                    serde_json::json!({"enabled": false, "reason": "--enable was not set"})
                );
                return Ok(true);
            }
            serve(config, plan, seconds)
        }
        _ => Err(format!("unknown command {command}").into()),
    }
}

/// `change`: one request from `lamp-cloud`, on the socket systemd hands
/// over as standard input and output (`lamp-setup-change@.service`).
fn change(given: impl Iterator<Item = String>) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::change;
    use std::io::{BufRead, Write};
    let (mut options, enabled) = options(given)?;
    let config_path = PathBuf::from(options.remove("--config").ok_or("--config is required")?);
    let bus_root = options.remove("--bus-root");
    if !options.is_empty() {
        return Err("change takes --config, --bus-root and --enable".into());
    }
    let config = serve_config(&config_path)?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(change::LINE_MOST as u64 + 1)
        .read_line(&mut line)?;
    let (reply, done) = change::answer(&line, &config, enabled);
    {
        let mut out = io::stdout().lock();
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    // What was changed, never the value: the journal is not the person's.
    match (&done, reply.get("refused")) {
        (Some(done), _) => eprintln!("lamp-setup: change: {}", done.said),
        (None, Some(why)) => {
            eprintln!("lamp-setup: change refused: {}", why.as_str().unwrap_or(""))
        }
        _ => {}
    }
    if !done.is_some_and(|done| done.restart_schedule) {
        return Ok(true);
    }
    let bus = match bus_root {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let mut ears = change::BusEars::new(&bus, &mut |line| eprintln!("lamp-setup: change: {line}"));
    match change::wait_for_quiet(&mut ears, change::QUIET_MOST) {
        Ok(()) => {
            let status = std::process::Command::new("/usr/bin/systemctl")
                .args(["try-restart", change::SCHEDULE_UNIT])
                .status();
            match status {
                Ok(status) if status.success() => {
                    eprintln!(
                        "lamp-setup: change: lamp-schedule restarted on the new zone, at a quiet moment"
                    );
                }
                Ok(status) => {
                    eprintln!("lamp-setup: change: lamp-schedule's restart ended with {status}")
                }
                Err(error) => {
                    eprintln!("lamp-setup: change: lamp-schedule could not be restarted: {error}")
                }
            }
        }
        Err(why) => eprintln!(
            "lamp-setup: change: the lamp was not quiet within {} min ({why}); lamp-schedule keeps the old zone until it next starts",
            change::QUIET_MOST.as_secs() / 60
        ),
    }
    Ok(true)
}

/// Hand the reset to a unit of its own (`detach`), outside `lampos.target`:
/// the keeper's stop would otherwise end this watcher, and the reset with it,
/// after the marker is gone and before the wipe and the reboot.
fn start_reset(plan: &Plan, plan_path: &Path) -> io::Result<()> {
    if plan.detach.is_empty() {
        let steps = reset::reset(plan, &mut System)?;
        eprintln!("lamp-setup: {}", serde_json::to_string(&steps)?);
        return Ok(());
    }
    let mut argv = plan.detach.clone();
    argv.push(std::env::current_exe()?.to_string_lossy().into_owned());
    argv.extend([
        "reset-now".to_owned(),
        "--plan".to_owned(),
        plan_path.to_string_lossy().into_owned(),
        "--enable".to_owned(),
    ]);
    System.run(&argv)
}

fn serve_config(path: &Path) -> Result<lamp_setup::serve::Config, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("the configuration exceeds 16 KiB".into());
    }
    let config: lamp_setup::serve::Config = serde_json::from_slice(&bytes)?;
    config.check()?;
    Ok(config)
}

/// The lamp's name as the kernel has it, which is the name its `.local`
/// address is announced under.
fn host_name() -> String {
    rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .trim()
        .to_owned()
}

/// How long the page may still need the server after setup is done.
const LINGER: Duration = Duration::from_secs(600);

fn serve(
    config: lamp_setup::serve::Config,
    plan: Plan,
    seconds: Option<u64>,
) -> Result<bool, Box<dyn std::error::Error>> {
    // A reset a power cut or a failed stop left unfinished comes first.
    let finished = reset::resume(&plan, &mut System)?;
    if !finished.is_empty() {
        eprintln!("lamp-setup: finished an interrupted reset");
    }
    if config.marker.exists() {
        eprintln!("lamp-setup: this lamp is already set up; nothing to serve");
        return Ok(true);
    }
    // A temporary file a crash left holds a secret: never kept.
    for path in lamp_setup::serve::leftover_temporaries(&config) {
        let _ = std::fs::remove_file(path);
    }
    // The runtime the server answers on, and the stop (`systemctl stop`, a
    // shutdown), asked for before the light starts: from here on a stop
    // ends serving as its end does, so the light is put out with black.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let stopped = {
        let _inside = runtime.enter();
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?
    };
    // The setup light from here to the marker (`lamp_setup::light`): the
    // voice is not running, and a dark, silent lamp looks broken.
    let status = Arc::new(Mutex::new(Status::new(String::new())));
    let light = setup_light(config.light, &status).map(Arc::new);
    let served = answer(config, seconds, status, light.clone(), runtime, stopped);
    if let Some(light) = &light {
        light.stop();
    }
    served
}

/// The setup light on the lamp's ring, when setup's `light` is on.
#[cfg(target_os = "linux")]
fn setup_light(on: bool, status: &Arc<Mutex<Status>>) -> Option<Light> {
    Light::spawn_if(
        on,
        Arc::clone(status),
        lamp_ring::linux::open_setup,
        |line| eprintln!("lamp-setup: {line}"),
    )
}

#[cfg(not(target_os = "linux"))]
fn setup_light(on: bool, _: &Arc<Mutex<Status>>) -> Option<Light> {
    if on {
        eprintln!("lamp-setup: the setup light needs a lamp's ring, and there is none here");
    }
    None
}

/// `wake`: the waking face until the voice asks for the ring
/// (`lamp_setup::wake`), only with its switch on.
fn wake() -> Result<bool, Box<dyn std::error::Error>> {
    let switch = std::env::var(lamp_setup::wake::SWITCH).ok();
    if !lamp_setup::wake::switched_on(switch.as_deref()) {
        eprintln!(
            "lamp-setup: the waking face is off ({} is not on); the ring is the voice's",
            lamp_setup::wake::SWITCH
        );
        return Ok(true);
    }
    let socket = Path::new(lamp_ring::waking::RELEASE_SOCKET);
    // One left from an earlier start; the unit's runtime folder is fresh.
    if std::fs::symlink_metadata(socket)
        .is_ok_and(|metadata| std::os::unix::fs::FileTypeExt::is_socket(&metadata.file_type()))
    {
        std::fs::remove_file(socket)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(socket)?;
    let ended = wake_on_ring(listener);
    // Gone, so a later open of the voice's goes on at once.
    let _ = std::fs::remove_file(socket);
    eprintln!("lamp-setup: the waking face has ended: {ended:?}");
    Ok(ended == lamp_setup::wake::Ended::Released)
}

#[cfg(target_os = "linux")]
fn wake_on_ring(listener: std::os::unix::net::UnixListener) -> lamp_setup::wake::Ended {
    lamp_setup::wake::run(
        listener,
        lamp_ring::linux::open_waking,
        |line| eprintln!("lamp-setup: {line}"),
        lamp_setup::wake::RETRY,
    )
}

#[cfg(not(target_os = "linux"))]
fn wake_on_ring(_: std::os::unix::net::UnixListener) -> lamp_setup::wake::Ended {
    eprintln!("lamp-setup: the waking face needs a lamp's ring, and there is none here");
    lamp_setup::wake::Ended::SocketFailed
}

/// Serving itself, from the hotspot until it is time to stop.
fn answer(
    config: lamp_setup::serve::Config,
    seconds: Option<u64>,
    status: Arc<Mutex<Status>>,
    light: Option<Arc<Light>>,
    runtime: tokio::runtime::Runtime,
    mut stopped: tokio::signal::unix::Signal,
) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::{
        http::{Shared, router},
        light::ReleasingSave,
        serve::Files,
        wifi::ImageHost,
    };
    // One look at the networks while the radio is still a station: as the
    // hotspot it may refuse to scan (V1 lists networks in station mode only).
    // A radio that is down fails at once; the page then waits on its own scans.
    // At most 4 s: a stuck `iw` must not hold the hotspot back.
    let looked = std::time::Instant::now();
    let before_hotspot = lamp_setup::scan::scan_within(&config.interface, Duration::from_secs(4));
    match &before_hotspot {
        Ok(list) => eprintln!(
            "lamp-setup: {} networks seen before the hotspot, in {} ms",
            list.len(),
            looked.elapsed().as_millis()
        ),
        Err(error) => eprintln!(
            "lamp-setup: no networks seen before the hotspot ({} ms): {error}",
            looked.elapsed().as_millis()
        ),
    }
    let before_hotspot = before_hotspot.ok().filter(|list| !list.is_empty());
    // A stop during the look: no hotspot. One while the hotspot comes up
    // ends serving at once, once it has begun, or is the hotspot's failure.
    let Some(started) = lamp_setup::serve::start_unless_stopped(
        || asked_to_stop(&runtime, &mut stopped),
        // The hotspot first: on a fresh image it is what names the lamp.
        || {
            lamp_setup::serve::start(
                &config,
                &mut ImageHost::new(),
                |argv| System.run(argv),
                host_name,
                |line| eprintln!("lamp-setup: {line}"),
            )
        },
        |line| eprintln!("lamp-setup: {line}"),
    )?
    else {
        return Ok(true);
    };
    *status.lock().unwrap() = started;
    eprintln!(
        "lamp-setup: the lamp's hotspot is up; answering on {}",
        config.listen
    );
    let shared = Arc::new(Shared {
        config: config.clone(),
        status: Arc::clone(&status),
        start: Box::new({
            let status = Arc::clone(&status);
            let config = config.clone();
            move |accepted| {
                let status = Arc::clone(&status);
                let config = config.clone();
                let light = light.clone();
                std::thread::spawn(move || {
                    // V1 replies first and starts 2 s later, so the reply
                    // reaches the phone before the hotspot goes.
                    std::thread::sleep(Duration::from_secs(2));
                    let interface = config.interface.clone();
                    let hotspot = config.hotspot_address.clone();
                    lamp_setup::serve::run(
                        &status,
                        &accepted,
                        &config.country,
                        &mut ImageHost::new(),
                        // The ring let go just before the marker starts the lamp.
                        &mut ReleasingSave {
                            save: Files(&config),
                            light: light.as_deref(),
                        },
                        move || lamp_setup::serve::station_ip(&interface, &hotspot),
                    );
                    let now = status.lock().unwrap().clone();
                    eprintln!(
                        "lamp-setup: run {} ended {:?}{}",
                        now.run,
                        now.phase,
                        if now.error.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", now.error)
                        }
                    );
                });
            }
        }),
        scan: Box::new({
            let interface = config.interface.clone();
            move || lamp_setup::scan::scan(&interface)
        }),
        scanned: Mutex::new(None),
        last_good: Mutex::new(before_hotspot),
    });
    // A few scans as the hotspot before anyone asks, so the page's first ask
    // finds a fresh list: early, while nobody is likely on the hotspot yet.
    std::thread::spawn({
        let shared = Arc::clone(&shared);
        move || lamp_setup::http::warm(&shared, 3, Duration::from_secs(5))
    });
    let web_stop = config.web_stop.clone();
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&config.listen).await?;
        let limit = seconds.map(Duration::from_secs);
        let started = tokio::time::Instant::now();
        let done = async move {
            let mut completed_at = None;
            loop {
                tokio::select! {
                    _ = stopped.recv() => {
                        eprintln!("lamp-setup: asked to stop");
                        return;
                    }
                    () = tokio::time::sleep(Duration::from_millis(500)) => {}
                }
                if limit.is_some_and(|limit| started.elapsed() >= limit) {
                    return;
                }
                if status.lock().unwrap().set_up_completed
                    && completed_at
                        .get_or_insert_with(tokio::time::Instant::now)
                        .elapsed()
                        >= LINGER
                {
                    return;
                }
            }
        };
        axum::serve(listener, router(shared))
            .with_graceful_shutdown(done)
            .await
    })?;
    if !web_stop.is_empty() {
        System.run(&web_stop)?;
    }
    Ok(true)
}

/// Whether a stop has come. The short wait lets the runtime take in a
/// signal that came while it was not running.
fn asked_to_stop(
    runtime: &tokio::runtime::Runtime,
    stopped: &mut tokio::signal::unix::Signal,
) -> bool {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(20), stopped.recv())
            .await
            .is_ok()
    })
}

fn main() -> ExitCode {
    // Its log lines never block it or end it (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("lamp-setup: {error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
