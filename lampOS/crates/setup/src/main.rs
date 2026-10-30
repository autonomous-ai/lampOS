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

const NAME: &str = lamp_setup::SUBSCRIBER;

const USAGE: &str = "\
usage: lamp-setup COMMAND [OPTIONS]
  recover --config FILE [--enable]
      With LAMP_WIFI_RECOVERY=on (#3020), on a set-up lamp: 90 s off every
      known network, and not joining one, opens setup's hotspot and page as
      a fresh lamp's, its marker and the person's data kept; the saved
      network is tried every 2 min with no phone on it; back on a network,
      recovery ends. Off, it says so and exits 0.
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
      The app's restart and shutdown too (#2193, #3697), refused while an
      update installs: answered first, then lampos.target is restarted once
      the voice rests (45 s at most), or `shutdown` is started.
      Without --enable every request is refused.
  wifi --config FILE [--enable]
      Root's helper for the Wi-Fi changed after setup (#1959): one request
      line on standard input from lamp-cloud, the new network and the broker
      it is on; the new network is kept only once joined with an address and
      the broker reached within 60 s, and the app's confirm line within
      120 s after the joined line, else the old one comes back. One reply
      line on standard output, at the end; then the lamp says what happened.
      The password is never printed.
      Without --enable every request is refused.
  wifi-restore [--boot]
      A Wi-Fi change cut before it ended (#1959): the old network's file kept
      beside the live one goes back. After the helper's stop (ExecStopPost)
      wpa_supplicant re-reads it and the radio restarts; with --boot, before
      wpa_supplicant starts, only the file.
  keys [--folder DIR] [--bus-root PATH] [--enable]
      Root's helper for a person's own DeepSeek and Deepgram keys (#2094):
      one request line on standard input from lamp-cloud, a key to check and
      keep, one to remove, or whose each service uses. A key is kept only
      once its service took it, in DIR (/etc/lampOS/voice/own), and the
      voice is restarted at a quiet moment to take it. One reply line on
      standard output. A key is never printed.
      Without --enable every request is refused.
  wake
      From power-on until the voice asks for the ring: the waking face
      (#1681). Only with LAMP_WAKING_FACE=on; otherwise it exits at once.
  shutdown [--dry-run]
      Hand shutdown-now to a unit of its own (lamp-setup-shutdown), outside
      lampos.target, so stopping the lamp cannot stop it (#3697).
  shutdown-now [--dry-run]
      Turn the lamp off safely (docs/shutdown.md): refused while an update
      installs; else the lamp is paused and the body rests (still 2 s, at
      most 15 s waited), every part is stopped and read back stopped, 0.4 s
      later the body lets go (torque off on all five, each read back, only
      for a lamp found down), the ring shows it is safe to unplug, the disks
      are synced and the machine powers off. --dry-run does all of it but
      the poweroff, and says it instead. One journal line a step.

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
            "--folder",
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
    // One version ahead (#1680): read, with what was passed over said.
    let (plan, notes) = reset::read(&bytes)?;
    for note in notes {
        eprintln!("lamp-setup: {}: {note}", path.display());
    }
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
    if command == "wifi" {
        return wifi_change(given);
    }
    if command == "keys" {
        return own_keys(given);
    }
    if command == "shutdown" || command == "shutdown-now" {
        let dry_run = match given.next().as_deref() {
            None => false,
            Some("--dry-run") if given.next().is_none() => true,
            Some(_) => return Err(format!("{command} takes only --dry-run").into()),
        };
        return if command == "shutdown" {
            start_shutdown(dry_run)
        } else {
            shutdown_now(dry_run)
        };
    }
    if command == "wifi-restore" {
        let boot = match given.next().as_deref() {
            None => false,
            Some("--boot") if given.next().is_none() => true,
            Some(_) => return Err("wifi-restore takes only --boot".into()),
        };
        // A reset journalled: its wipe of the Wi-Fi is not undone (Third on #3579).
        if !reset::wifi_restore_allowed(Path::new(reset::PRODUCT_JOURNAL))? {
            eprintln!(
                "lamp-setup: wifi-restore: a reset is journalled, the old network is not put back"
            );
            return Ok(true);
        }
        let mut host = lamp_setup::wifi_change::ImageRejoin::new();
        let restored = lamp_setup::wifi_change::restore(&mut host, !boot);
        eprintln!("lamp-setup: wifi-restore: {restored:?}");
        return Ok(matches!(
            restored,
            lamp_setup::wifi_change::Restored::Put | lamp_setup::wifi_change::Restored::Nothing
        ));
    }
    let (mut options, enabled) = options(given)?;
    // Recovery takes no plan (#3020): its unit gives `--config` and
    // `--enable` only, as its usage says. Matched after the plan was
    // required, it never ran on a lamp: lamp-52e6 on build 7071 restarted
    // its unit 76 times in 85 min, each start "--plan is required" (the
    // lead's agent on #1711, 27 Oct).
    if command == "recover" {
        let config_path = PathBuf::from(options.remove("--config").ok_or("--config is required")?);
        if !options.is_empty() {
            return Err("recover takes --config and --enable".into());
        }
        let config = serve_config(&config_path)?;
        if !enabled {
            println!(
                "{}",
                serde_json::json!({"enabled": false, "reason": "--enable was not set"})
            );
            return Ok(true);
        }
        return recover(config);
    }
    let plan_path = PathBuf::from(options.remove("--plan").ok_or("--plan is required")?);
    let plan = match plan(&plan_path) {
        Ok(plan) => plan,
        Err(error) => {
            // A newer build's plan this build refuses (#2060): no reset runs,
            // and the watcher stays to tell the person who holds the button.
            if command == "watch-reset"
                && let Some(reset::Refused::Ahead(_)) = error.downcast_ref::<reset::Refused>()
            {
                eprintln!("lamp-setup: {}: {error}", plan_path.display());
                return watch_refused(options, enabled);
            }
            return Err(error);
        }
    };
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
            // Enabled, it hands the reset to a unit of its own (`detach`),
            // since lamp-setup-reset.service is read-only but the bus
            // (#3710): a reset run in here would fail on its journal after
            // the button, wiping nothing. So a plan without `detach` is
            // refused now, at start, where the unit shows failed (Fourteen's
            // P3 on #4048), before the options are read: the clock's id is
            // read from /proc, which a Mac has not. Every shipped plan has it.
            if enabled && plan.detach.is_empty() {
                return Err(format!(
                    "{}: watch-reset --enable needs the plan's detach, the unit the reset runs in \
                     (as the shipped plan's systemd-run): without it the reset would run in this \
                     read-only unit and wipe nothing",
                    plan_path.display()
                )
                .into());
            }
            let (bus, clock, until) = watch_options(&mut options)?;
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

/// Setup's page on the hotspot while recovery has it open: its stop, the
/// thread serving, and the status the page answers with.
struct Recovering {
    stop: tokio::sync::watch::Sender<bool>,
    serving: std::thread::JoinHandle<()>,
    status: Arc<Mutex<Status>>,
}

impl Recovering {
    /// Opens the hotspot and serves setup's page, as `serve` does, with
    /// the marker kept and no light (the ring is the voice's).
    fn open(config: &lamp_setup::serve::Config) -> Self {
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let status = Arc::new(Mutex::new(Status::new(String::new())));
        let serving = std::thread::spawn({
            let config = config.clone();
            let status = Arc::clone(&status);
            move || {
                let served = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(Box::<dyn std::error::Error>::from)
                    .and_then(|runtime| answer(config, None, status, None, runtime, stopped, true));
                if let Err(error) = served {
                    eprintln!("lamp-setup: recover: serving setup's page: {error}");
                }
            }
        });
        Self {
            stop,
            serving,
            status,
        }
    }

    /// A setup run in progress on the page: never cut by a retry.
    fn running(&self) -> bool {
        self.status.lock().unwrap().phase == lamp_setup::serve::Phase::Connecting
    }

    fn close(self) {
        let _ = self.stop.send(true);
        let _ = self.serving.join();
    }
}

/// `recover` (#3020): a set-up lamp whose saved network is gone opens
/// setup's hotspot after 90 s, only with its switch on. One look every 5 s.
fn recover(config: lamp_setup::serve::Config) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::recover::{Act, Recovery, SWITCH, Seen, on};
    use lamp_setup::wifi::{Host, ImageHost};
    if !on(std::env::var(SWITCH).ok().as_deref()) {
        eprintln!("lamp-setup: recover: off ({SWITCH} is not on)");
        return Ok(true);
    }
    if !config.marker.exists() {
        eprintln!("lamp-setup: recover: this lamp is not set up; setup itself serves");
        return Ok(true);
    }
    // For lamp-cloud's `recovery`; the runtime folder goes with the unit.
    if let Err(error) = std::fs::write(lamp_setup::recover::ON_FILE, "") {
        eprintln!(
            "lamp-setup: recover: {}: {error}",
            lamp_setup::recover::ON_FILE
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    stop_on_terminate(&runtime, stop)?;
    let begun = std::time::Instant::now();
    let mut policy = Recovery::default();
    let mut open: Option<Recovering> = None;
    eprintln!("lamp-setup: recover: watching {}", config.interface);
    loop {
        if runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), stopped.wait_for(|stop| *stop))
                .await
                .is_ok()
        }) {
            if let Some(open) = open.take() {
                open.close();
            }
            return Ok(true);
        }
        // A 5 s reset removes the marker before its stop and reboot (El's
        // P3 on #3024): setup's own then serves, not this.
        if !config.marker.exists() {
            eprintln!("lamp-setup: recover: the marker is gone (a reset): setup itself serves");
            if let Some(open) = open.take() {
                open.close();
            }
            return Ok(true);
        }
        let mut host = ImageHost::new();
        let online =
            lamp_setup::serve::station_ip(&config.interface, &config.hotspot_address).is_some();
        let seen = Seen {
            online,
            associated: !online && host.wpa_state() == "COMPLETED",
            busy: open.as_ref().is_some_and(Recovering::running)
                || System
                    .run(&[
                        "/usr/bin/systemctl".into(),
                        "is-active".into(),
                        "--quiet".into(),
                        "lamp-setup.service".into(),
                    ])
                    .is_ok()
                || lamp_setup::wifi_change::ImageRejoin::new()
                    .previous
                    .exists(),
            phones: if open.is_some() {
                lamp_setup::serve::phones(&config.interface)
            } else {
                Some(0)
            },
        };
        let was = policy.recovering();
        let now_ms = u64::try_from(begun.elapsed().as_millis()).unwrap_or(u64::MAX);
        let act = policy.step(now_ms, seen);
        let say_opened = policy.says_opened(act);
        match act {
            Some(Act::OpenHotspot) => {
                eprintln!(
                    "lamp-setup: recover: off every known network for {} s and not joining one: opening setup's hotspot",
                    lamp_setup::recover::LOSS_GRACE_MS / 1000
                );
                if let Some(open) = open.take() {
                    open.close();
                }
                open = Some(Recovering::open(&config));
                if say_opened {
                    // No network, so the recording only, never words for
                    // the model (#3743). Not played: journalled, and the
                    // hotspot stays open all the same.
                    if let Some(why) = lamp_setup::wifi_change::SayService::product()
                        .play_recording(lamp_setup::recover::RECORDING_OPENED)
                    {
                        eprintln!("lamp-setup: recover: {why}");
                    }
                }
            }
            Some(Act::Rejoin) => {
                eprintln!("lamp-setup: recover: no phone on the hotspot: trying the saved network");
                if let Some(open) = open.take() {
                    open.close();
                }
                if let Err(error) = host.station() {
                    eprintln!("lamp-setup: recover: the station could not start: {error}");
                }
            }
            None => {}
        }
        if was && !policy.recovering() {
            if seen.online {
                eprintln!("lamp-setup: recover: back on a network; recovery ends");
            } else {
                eprintln!(
                    "lamp-setup: recover: the try is waiting for an address, or the radio is busy: lost afresh, the hotspot after 90 s more without a network"
                );
            }
            if let Some(open) = open.take() {
                open.close();
            }
        }
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
    // Only lamp-cloud may ask on the socket, as for the keys: any other
    // peer is refused before its line is read.
    if lamp_setup::keys::socket_peer_refused() {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", serde_json::json!({"refused": NOT_THE_CLOUD}))?;
        out.flush()?;
        eprintln!(
            "lamp-setup: change refused: the peer is not {}",
            lamp_setup::keys::ASKER_UNIT
        );
        return Ok(true);
    }
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(change::LINE_MOST as u64 + 1)
        .read_line(&mut line)?;
    let records = lamp_update::records::Records::new(PathBuf::from(lamp_update::install::RECORDS));
    if let Some((power, reply, lock)) = change::power(&line, enabled, &records) {
        {
            let mut out = io::stdout().lock();
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
        eprintln!("lamp-setup: change: {power:?} asked: {reply}");
        let Some(lock) = lock else {
            return Ok(true);
        };
        return match power {
            change::Power::Shutdown => {
                // The runner takes the update's lock itself.
                drop(lock);
                start_shutdown(false)
            }
            change::Power::Restart => {
                // Held to the end, so no update starts under the restart.
                let _lock = lock;
                let bus = match bus_root {
                    Some(root) => Bus::new(root)?,
                    None => Bus::from_environment()?,
                };
                let mut ears =
                    change::BusEars::new(&bus, &mut |line| eprintln!("lamp-setup: change: {line}"));
                let said = change::restart_when_rested(&mut ears, || {
                    match std::process::Command::new("/usr/bin/systemctl")
                        .args(["restart", "lampos.target"])
                        .status()
                    {
                        Ok(status) if status.success() => Ok(()),
                        Ok(status) => Err(status.to_string()),
                        Err(error) => Err(error.to_string()),
                    }
                });
                eprintln!("lamp-setup: change: {said}");
                Ok(true)
            }
        };
    }
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

/// `keys`: one request about a person's own keys from `lamp-cloud`, on the
/// socket systemd hands over as standard input and output
/// (`lamp-setup-keys@.service`, #2094).
fn own_keys(given: impl Iterator<Item = String>) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::{change, keys};
    use std::io::{BufRead, Write};
    let (mut options, enabled) = options(given)?;
    let folder = PathBuf::from(
        options
            .remove("--folder")
            .unwrap_or_else(|| keys::FOLDER.to_owned()),
    );
    let bus_root = options.remove("--bus-root");
    if !options.is_empty() || !folder.is_absolute() {
        return Err("keys takes an absolute --folder, --bus-root and --enable".into());
    }
    // Only lamp-cloud may ask: any other peer is refused before its line
    // is read (Third's privacy read on #2094).
    if !keys::peer_is_asker() {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", keys::refused(keys::NOT_THE_ASKER, "not_allowed"))?;
        out.flush()?;
        eprintln!(
            "lamp-setup: keys refused: the peer is not {}",
            lamp_setup::keys::ASKER_UNIT
        );
        return Ok(true);
    }
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(keys::LINE_MOST as u64 + 1)
        .read_line(&mut line)?;
    let (reply, done) = keys::answer(
        &line,
        &keys::Store(folder),
        &mut keys::HttpCheck::default(),
        enabled,
    );
    {
        let mut out = io::stdout().lock();
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    // Which service and what happened, never a key: the journal is not the person's.
    match (&done, reply.get("refused")) {
        (Some(done), _) => eprintln!("lamp-setup: keys: {}", done.said),
        (None, Some(why)) => {
            eprintln!("lamp-setup: keys refused: {}", why.as_str().unwrap_or(""))
        }
        _ => {}
    }
    if !done.is_some_and(|done| done.restart_voice) {
        return Ok(true);
    }
    let bus = match bus_root {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let mut ears = change::BusEars::new(&bus, &mut |line| eprintln!("lamp-setup: keys: {line}"));
    match change::wait_for_quiet(&mut ears, change::QUIET_MOST) {
        Ok(()) => match std::process::Command::new("/usr/bin/systemctl")
            .args(["try-restart", keys::VOICE_UNIT])
            .status()
        {
            Ok(status) if status.success() => {
                eprintln!("lamp-setup: keys: the voice restarted to take them, at a quiet moment")
            }
            Ok(status) => eprintln!("lamp-setup: keys: the voice's restart ended with {status}"),
            Err(error) => eprintln!("lamp-setup: keys: the voice could not be restarted: {error}"),
        },
        Err(why) => eprintln!(
            "lamp-setup: keys: the lamp was not quiet within {} min ({why}); the voice takes them at its next start",
            change::QUIET_MOST.as_secs() / 60
        ),
    }
    Ok(true)
}

/// The change and Wi-Fi helpers' refusal to a socket peer that isn't
/// lamp-cloud (`keys::socket_peer_refused`).
const NOT_THE_CLOUD: &str = "only the app's link may change the lamp's settings";

/// `wifi`: one Wi-Fi change from `lamp-cloud`, on the socket systemd hands
/// over as standard input and output (`lamp-setup-wifi@.service`).
fn wifi_change(given: impl Iterator<Item = String>) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::wifi_change;
    use std::io::{BufRead, Write};
    let (mut options, enabled) = options(given)?;
    let config_path = PathBuf::from(options.remove("--config").ok_or("--config is required")?);
    if !options.is_empty() {
        return Err("wifi takes --config and --enable".into());
    }
    let config = serve_config(&config_path)?;
    // Only lamp-cloud may ask on the socket, and so confirm, as for the
    // keys: any other peer is refused before its line is read.
    if lamp_setup::keys::socket_peer_refused() {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", serde_json::json!({"refused": NOT_THE_CLOUD}))?;
        out.flush()?;
        eprintln!(
            "lamp-setup: wifi refused: the peer is not {}",
            lamp_setup::keys::ASKER_UNIT
        );
        return Ok(true);
    }
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(wifi_change::LINE_MOST as u64 + 1)
        .read_line(&mut line)?;
    let mut host = wifi_change::ImageRejoin::new();
    let answered = wifi_change::answer(&line, &config.country, enabled, &mut host);
    // Never the password, and never the network's name: `said` holds neither.
    eprintln!("lamp-setup: wifi: {}", answered.said);
    {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", answered.reply)?;
        out.flush()?;
    }
    // Then the lamp says what happened, never the network's name.
    if let Some((changed, broker)) = &answered.changed {
        let mut mouth = wifi_change::SayService::product();
        let said = wifi_change::say_after(&mut host, &mut mouth, changed, broker);
        eprintln!("lamp-setup: wifi: {said} line(s) asked of the voice");
    }
    Ok(true)
}

/// `watch-reset`'s own options: the bus, this boot's clock and how long.
fn watch_options(
    options: &mut BTreeMap<String, String>,
) -> Result<(Bus, String, Option<Instant>), Box<dyn std::error::Error>> {
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
    Ok((bus, clock, until))
}

/// `watch-reset` with a plan this build refuses (#2060): each valid reset is
/// refused before its journal and the marker, logged, and said to the person
/// through the voice's say service ([`reset::SAY_REFUSED`]). The reset does
/// not speak otherwise, so this is its first line to the person; it is words
/// for the voice's model, said only while the lamp has a network and the say
/// service is on, with no recording behind it.
fn watch_refused(
    mut options: BTreeMap<String, String>,
    enabled: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let (bus, clock, until) = watch_options(&mut options)?;
    let mut touch = bus.subscribe_typed(TouchEvent::TOPIC, NAME, TouchEvent::TYPE)?;
    let mouth = lamp_setup::wifi_change::SayService::product();
    let watched = lamp_setup::watch_refused(
        &mut touch,
        &clock,
        enabled,
        until,
        lamp_ipc::monotonic_us,
        |line| eprintln!("lamp-setup: {line}"),
        || mouth.say_as("reset", reset::SAY_REFUSED),
    )?;
    println!(
        "{}",
        serde_json::json!({"enabled": enabled, "watched": format!("{watched:?}")})
    );
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

/// Hand the shutdown to a unit of its own, outside `lampos.target`, as the
/// reset is handed to one (`start_reset`): stopping the lamp must not stop it.
fn start_shutdown(dry_run: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let mut argv: Vec<String> = [
        "/usr/bin/systemd-run",
        "--no-block",
        "--collect",
        "--unit=lamp-setup-shutdown",
        "--property=Type=oneshot",
    ]
    .map(str::to_owned)
    .into();
    argv.push(std::env::current_exe()?.to_string_lossy().into_owned());
    argv.push("shutdown-now".to_owned());
    if dry_run {
        argv.push("--dry-run".to_owned());
    }
    System.run(&argv)?;
    Ok(true)
}

/// `shutdown-now`: the update's lock held to the end, then
/// `lamp_setup::shutdown::run` on this lamp.
fn shutdown_now(dry_run: bool) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_setup::shutdown;
    let records = lamp_update::records::Records::new(PathBuf::from(lamp_update::install::RECORDS));
    // Waited for a moment: the change helper that started this runner lets
    // go of the lock as it exits.
    let _lock = match shutdown::update_lock_within(&records, shutdown::LOCK_WAIT) {
        Ok(lock) => lock,
        Err(why) => {
            eprintln!("lamp-setup: shutdown: refused: {why}");
            println!("{}", serde_json::json!({ "refused": why }));
            return Ok(false);
        }
    };
    let mut host = Shutdown {
        started: Instant::now(),
        bus: None,
    };
    let steps = shutdown::run(&mut host, dry_run, &mut |line| {
        eprintln!("lamp-setup: shutdown: {line}")
    });
    println!(
        "{}",
        serde_json::json!({"dry_run": dry_run, "steps": steps})
    );
    Ok(true)
}

/// The shutdown's effects on this lamp.
struct Shutdown {
    started: Instant,
    bus: Option<Bus>,
}

impl lamp_setup::shutdown::Host for Shutdown {
    /// The voice's say service, at the socket setup's other lines use.
    fn say_aloud(&mut self, line: &str, within: Duration) -> Result<(), String> {
        let socket = lamp_setup::wifi_change::SayService::product().socket;
        lamp_setup::shutdown::say_through(&socket, line, within)
    }
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
    fn sleep(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }
    /// The keeper's own pause (`lamp-keeper pause`), which owns `lamp/pause`.
    fn pause(&mut self, paused: bool) -> io::Result<()> {
        let verb = if paused { "pause" } else { "resume" };
        System.run(&["/usr/local/bin/lamp-keeper".to_owned(), verb.to_owned()])
    }
    fn joints(&mut self) -> Option<Vec<f64>> {
        if self.bus.is_none() {
            self.bus = Bus::from_environment().ok();
        }
        let message = self
            .bus
            .as_ref()?
            .latest(lamp_msgs::JointState::TOPIC)
            .ok()??;
        (!message.expired)
            .then(|| message.parse::<lamp_msgs::JointState>().ok())
            .flatten()
            .map(|state| state.position)
    }
    fn stop(&mut self) -> io::Result<()> {
        // The voice first, while the switch it reads is still there; one
        // that will not stop is left to the target's stop below.
        if let Err(why) = System.run(&lamp_setup::shutdown::VOICE_FIRST.map(str::to_owned)) {
            eprintln!("lamp-setup: shutdown: the voice did not stop first ({why}); stopping all");
        }
        reset::stop_and_wait(&lamp_setup::shutdown::STOP.map(str::to_owned), &mut System)
    }
    /// The active `lamp*` units. An accepted connection's instance (`@`)
    /// is left out: its socket is what is started again.
    fn active(&mut self) -> Vec<String> {
        let argv = [
            "/usr/bin/systemctl",
            "list-units",
            "--state=active",
            "--plain",
            "--no-legend",
            "--full",
            "lamp*",
        ]
        .map(str::to_owned);
        System
            .read(&argv)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_whitespace().find(|word| *word != "\u{25cf}"))
            .filter(|unit| !unit.contains('@'))
            .map(str::to_owned)
            .collect()
    }
    fn start(&mut self, units: &[String]) -> io::Result<()> {
        let mut argv = vec!["/usr/bin/systemctl".to_owned(), "start".to_owned()];
        argv.extend(units.iter().cloned());
        System.run(&argv)
    }
    /// `lamp-body-measure let-go` as the body's user, as the body runs
    /// (the servo port's owner refuses root), on the body's own device,
    /// lock and poses.
    fn let_go(&mut self) -> Result<String, String> {
        let output = std::process::Command::new("/usr/bin/setpriv")
            .args([
                "--reuid=lampos",
                "--regid=lampos",
                "--groups=dialout",
                "--",
                "/opt/lampOS/body/current/lamp-body-measure",
                "let-go",
                "--device",
                "/dev/ttyACM0",
                "--unit-id",
                &host_name(),
                "--lock-directory",
                "/run/lampos-locks/motor",
                "--lamp-env",
                "/etc/lampOS/body/lamp.env",
            ])
            .output()
            .map_err(|why| why.to_string())?;
        let said = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() {
            Ok(said)
        } else {
            Err(format!(
                "{}: {said} {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
    fn light(&mut self) -> io::Result<()> {
        unplug_light()
    }
    fn sync(&mut self) {
        rustix::fs::sync();
    }
    /// As the reset's reboot: a poweroff that fails is forced, once synced.
    fn power_off(&mut self) -> io::Result<()> {
        let failed = System.run(&lamp_setup::shutdown::POWER_OFF.map(str::to_owned));
        if failed.is_err() {
            rustix::fs::sync();
            #[cfg(target_os = "linux")]
            let _ = rustix::system::reboot(rustix::system::RebootCommand::PowerOff);
        }
        failed
    }
}

/// The ring's safe-to-unplug light, left on: the voice has let the ring go
/// with its stop.
#[cfg(target_os = "linux")]
fn unplug_light() -> io::Result<()> {
    use lamp_ring::setup::SetupLook;
    let mut ring = lamp_ring::linux::open_setup()?;
    ring.show(SetupLook::Unplug, SetupLook::Unplug, 1.0, 1.0)?;
    ring.leave();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn unplug_light() -> io::Result<()> {
    Err(io::Error::other("there is no lamp's ring here"))
}

fn serve_config(path: &Path) -> Result<lamp_setup::serve::Config, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("the configuration exceeds 16 KiB".into());
    }
    let (config, notes) = lamp_setup::serve::read(&bytes)?;
    for note in notes {
        eprintln!("lamp-setup: {}: {note}", path.display());
    }
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
    let (stop, stopped) = tokio::sync::watch::channel(false);
    stop_on_terminate(&runtime, stop)?;
    // The setup light from here to the marker (`lamp_setup::light`): the
    // voice is not running, and a dark, silent lamp looks broken.
    let status = Arc::new(Mutex::new(Status::new(String::new())));
    let light = setup_light(config.light, &status).map(Arc::new);
    let served = answer(
        config,
        seconds,
        status,
        light.clone(),
        runtime,
        stopped,
        false,
    );
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
    mut stopped: tokio::sync::watch::Receiver<bool>,
    recovering: bool,
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
    // Said from the start, so the app knows before a setup whether to ask
    // for own keys (#2094). Recovery saves the network only, so never.
    status.lock().unwrap().takes_own_keys =
        lamp_setup::serve::takes_own_keys_at_start(&Files(&config), recovering);
    // Recovery's (#3020): the lamp is set up, its network gone; the page
    // says so, and setting a new network keeps everything else.
    if recovering {
        let mut status = status.lock().unwrap();
        status.runtime_phase = lamp_setup::recover::RECOVERING.into();
        // What the owner kept, which a recovery request never changes.
        status.share_reports = lamp_setup::serve::kept_share_reports(&config.reports);
    }
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
                    let station = move || lamp_setup::serve::station_ip(&interface, &hotspot);
                    if recovering {
                        // A set-up lamp's: the network only, every other
                        // field ignored (El's P1 on #3024).
                        lamp_setup::serve::run(
                            &status,
                            &accepted,
                            &config.country,
                            &mut ImageHost::new(),
                            &mut lamp_setup::serve::NetworkOnly(Files(&config)),
                            station,
                        );
                    } else {
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
                            station,
                        );
                    }
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
                    _ = stopped.wait_for(|stop| *stop) => {
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
    stopped: &mut tokio::sync::watch::Receiver<bool>,
) -> bool {
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(20), stopped.wait_for(|stop| *stop))
            .await
            .is_ok_and(|seen| seen.is_ok())
    })
}

/// A stop (`systemctl stop`, a shutdown) told on `stop`, from now on: it
/// is heard while the runtime runs, which is whenever serving waits.
fn stop_on_terminate(
    runtime: &tokio::runtime::Runtime,
    stop: tokio::sync::watch::Sender<bool>,
) -> std::io::Result<()> {
    let _inside = runtime.enter();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    runtime.spawn(async move {
        terminate.recv().await;
        let _ = stop.send(true);
    });
    Ok(())
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
