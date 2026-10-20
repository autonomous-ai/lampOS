//! `lamp-keeper`: watches the lamp's health and, with `--act`, restarts a
//! part that stays unwell; and one place to ask what runs and to start,
//! stop or restart a part or the lamp.
use lamp_bus::{Bus, Publisher};
use lamp_keeper::{
    Keeper, NAME, Notify, PartState, Rules, STATE,
    package::{self, Answer, Release},
    profile,
    units::{self, Systemctl, Systemd},
};
use lamp_msgs::{Diagnostic, Health, Level, Pause, Voice, VoiceState};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

const USAGE: &str = "\
lamp-keeper [serve] [--act] [--unwell-seconds N] [--most N] [--setup-marker PATH]
lamp-keeper status
lamp-keeper health
lamp-keeper prepare
lamp-keeper start|stop|restart PART|lamp
lamp-keeper pause|resume

serve watches lamp/health on the bus (LAMP_BUS_ROOT, or /run/lampos-bus).
Without --act it only says, in its journal, which part it would restart.
With --act it restarts a part in error or stale for --unwell-seconds
(default 30), at most --most times in ten minutes (default 3), and then
leaves it alone and reports it. The voice is restarted only when it says
it is resting: never in a turn.
Started by the old runtime's updater as a service package's bin/service
(AUTONOMOUS_SERVICE_NAME set), serve first stops and masks the old runtime's
units the voice refuses to start beside, makes the bus root root's and the
parts' group's (mode 2770), then brings the lamp up: it links the
units in the release's units/ folder, ties lampos.target to its own unit so
that stopping it stops the lamp, and starts lampos.target. health prints
its ready, version and pid as JSON for the updater: ready once every part the release
carries runs.
prepare, run while the old runtime still owns the lamp (the lamp-os
package's adapter calls it from its own prepare), writes the device key
(llm_api_key of /root/config/config.json) to /etc/lampOS/voice/credential
and /etc/lampOS/policy/jev.key where missing, 0600, with lamp-update
install's code. It stops nothing and touches no hardware or unit. With no
usable key it says so and exits 0, as install does: setup takes the key.
Then, where missing, it writes what a V1 lamp lacks for the generic
package: /etc/lampOS/package-profile.json from a read-only hardware probe
(with /etc/lampOS/body/lamp.env's poses if it has one; without, they are
read seated after cutover by lamp-body-measure.service), the package's
provision/ settings, and the runtime directories. A device it cannot find, or one
found twice, stops it with exit 1 before any of these is written.
status prints every lampOS unit: its state, its build and its health.
start, stop and restart act on lamp-PART.service, or lampos.target for lamp.
pause says on lamp/pause that the lamp is paused: each part that honours it
does no new work and holds what it has (the body rests); resume undoes it.
stop lamp pauses first, waits up to 45 s for the voice to say it is
resting, stops lampos.target, and then clears the pause.
With --setup-marker, a lamp whose marker does not exist yet (not set up for
a home) starts only lamp-setup.service, not lampos.target, and is not ready;
serve starts lampos.target once the marker appears. Installed by the
updater, serve also keeps /usr/local/bin/lamp-keeper pointing at itself.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["status"] => status(),
        ["health"] => {
            let answer = package::read_answer(&data_folder());
            println!("{}", serde_json::to_string(&answer).unwrap_or_default());
            ExitCode::SUCCESS
        }
        ["prepare"] => prepare(),
        ["pause"] => say_pause(true),
        ["resume"] => say_pause(false),
        ["stop", "lamp"] => stop_lamp(),
        [verb @ ("start" | "stop" | "restart"), part] if units::is_part_name(part) => {
            match Systemctl.command(verb, &units::unit_of(part)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(why) => {
                    eprintln!("lamp-keeper: {verb} {part}: {why}");
                    ExitCode::FAILURE
                }
            }
        }
        ["serve", rest @ ..] => serve_with(rest),
        rest => serve_with(rest),
    }
}

/// `lamp-keeper prepare`: V1's key where missing and never written before,
/// by lamp-update install's own code and records; then what a V1 lamp lacks
/// for the generic package (the profile, the settings and the directories),
/// where missing (#2163). Nothing it prints carries the key: the paths only.
fn prepare() -> ExitCode {
    use lamp_update::install;
    let mut say = |line: &str| eprintln!("lamp-keeper: prepare: {line}");
    let mut wrote: Vec<String> =
        match install::provision_device_key(Path::new("/"), install::RECORDS, &mut say) {
            Ok(wrote) => wrote.into_iter().map(str::to_owned).collect(),
            Err(why) => {
                eprintln!("lamp-keeper: prepare: {why}");
                return ExitCode::FAILURE;
            }
        };
    let Some(release) = Release::of_this_program() else {
        eprintln!("lamp-keeper: prepare: cannot tell which release this program is in");
        return ExitCode::FAILURE;
    };
    // The mixer is asked, not set: `sget` only reads.
    let mut has_control = |card: &str, control: &str| {
        std::process::Command::new("amixer")
            .args(["-c", card, "sget", control])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    match profile::prepare(Path::new("/"), &release.root, &mut has_control, &mut say) {
        Ok(more) => wrote.extend(more),
        Err(why) => {
            eprintln!("lamp-keeper: prepare: {why}");
            return ExitCode::FAILURE;
        }
    }
    if wrote.is_empty() {
        eprintln!("lamp-keeper: prepare: nothing written");
    }
    for path in wrote {
        eprintln!("lamp-keeper: prepare: wrote {path}");
    }
    ExitCode::SUCCESS
}

/// Say on `lamp/pause` whether the lamp is paused. The value is kept on the
/// bus until the next, so the parts read it whenever they look.
fn say_pause(paused: bool) -> ExitCode {
    match Bus::from_environment().and_then(|bus| publish_pause(&bus, paused)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("lamp-keeper: lamp/pause: {why}");
            ExitCode::FAILURE
        }
    }
}

fn publish_pause(bus: &Bus, paused: bool) -> std::io::Result<()> {
    bus.publisher_declared(Pause::TOPIC, &Pause::declaration(), NAME)?
        .publish_json(0, &Pause { paused })
        .map(|_| ())
}

/// The longest a stop waits for the voice to finish a turn: task 12's quiet
/// (docs/surround.md, "the owner protocol").
const QUIET: Duration = Duration::from_secs(45);

/// Pause, wait for the voice to rest, stop the lamp, clear the pause: a
/// stop that cuts nobody off.
fn stop_lamp() -> ExitCode {
    let bus = Bus::from_environment().ok();
    if let Some(bus) = &bus {
        if let Err(why) = publish_pause(bus, true) {
            eprintln!("lamp-keeper: lamp/pause: {why}");
        }
        if !wait_until(QUIET, Duration::from_millis(500), || voice_resting(bus)) {
            eprintln!("lamp-keeper: the voice did not rest within 45 s; stopping all the same");
        }
    }
    let stopped = Systemctl.command("stop", units::LAMP);
    // A pause left kept would hold the lamp paused when it is next started.
    if let Some(bus) = &bus {
        let _ = publish_pause(bus, false);
    }
    match stopped {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("lamp-keeper: stop lamp: {why}");
            ExitCode::FAILURE
        }
    }
}

/// True as soon as `done` is, looking every `every`; false after `longest`.
fn wait_until(longest: Duration, every: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + longest;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(every);
    }
}

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn serve_with(options: &[&str]) -> ExitCode {
    let mut rules = Rules::default();
    let mut setup_marker: Option<PathBuf> = None;
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let mut number = || options.next().and_then(|value| value.parse::<u64>().ok());
        match *option {
            "--act" => rules.act = true,
            "--unwell-seconds" => match number().filter(|n| (1..=3600).contains(n)) {
                Some(seconds) => rules.unwell_for = Duration::from_secs(seconds),
                None => return usage(),
            },
            "--most" => match number().filter(|n| (1..=100).contains(n)) {
                Some(most) => rules.most = most as usize,
                None => return usage(),
            },
            "--setup-marker" => match options.next().map(PathBuf::from) {
                Some(path) if path.is_absolute() => setup_marker = Some(path),
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    // Installed by the updater: the lamp is this service's to bring up.
    let package = match std::env::var(package::NAME_VARIABLE) {
        Ok(service) => {
            let Some(release) = Release::of_this_program() else {
                eprintln!("lamp-keeper: cannot tell which release it runs from");
                return ExitCode::FAILURE;
            };
            // Not ready until this run has looked: an answer left by the run
            // before must not be taken for this one.
            let not_yet = Answer {
                ready: false,
                version: release.version(),
                pid: std::process::id(),
                waiting_for: Vec::new(),
                legacy_stopped: Vec::new(),
            };
            if let Err(why) = package::write_answer(&data_folder(), &not_yet) {
                eprintln!("lamp-keeper: {why}");
            }
            // Before any part starts: the voice refuses to start while one
            // of these runs, and the parts not run as root need the bus root
            // their group's.
            let legacy_stopped = package::clear_legacy(&mut Systemctl);
            make_bus_root(&release);
            let drop_ins = Path::new("/etc/systemd/system").join(format!("{}.d", units::LAMP));
            let started = match package::bring_up(
                &mut Systemctl,
                &release,
                &service,
                &drop_ins,
                setup_marker.as_deref(),
            ) {
                Ok(started) => started,
                Err(why) => {
                    eprintln!("lamp-keeper: bringing the lamp up: {why}");
                    return ExitCode::FAILURE;
                }
            };
            if started {
                eprintln!(
                    "lamp-keeper: the lamp is started from {}",
                    release.root.display()
                );
            } else {
                eprintln!(
                    "lamp-keeper: not set up yet ({} absent): only {} is started",
                    setup_marker.as_deref().unwrap_or(Path::new("")).display(),
                    package::SETUP
                );
            }
            if let Err(why) = package::link_command(&release, Path::new(package::COMMAND_LINK)) {
                eprintln!("lamp-keeper: {why}");
            }
            let waiting_setup = if started { None } else { setup_marker };
            Some((release, legacy_stopped, waiting_setup))
        }
        Err(_) => None,
    };
    serve(rules, package)
}

/// The bus root, root's and the parts' group's, mode 2770. A failure is
/// said and the lamp started anyway: the voice, as root, works without it.
fn make_bus_root(release: &Release) {
    let Some(group) = package::parts_group(release) else {
        eprintln!("lamp-keeper: no part runs in a group of its own; the bus root is left");
        return;
    };
    let Some(gid) = std::fs::read_to_string("/etc/group")
        .ok()
        .and_then(|groups| package::gid_of(&groups, &group))
    else {
        eprintln!("lamp-keeper: no group {group}; the bus root is left");
        return;
    };
    let root = match Bus::from_environment() {
        Ok(bus) => bus.root().to_owned(),
        Err(why) => {
            eprintln!("lamp-keeper: the bus root: {why}");
            return;
        }
    };
    match package::make_bus_root(&root, gid, |path, uid, gid| {
        std::os::unix::fs::chown(path, Some(uid), Some(gid))
    }) {
        Ok(()) => eprintln!("lamp-keeper: {} is root:{group}, 2770", root.display()),
        Err(why) => eprintln!("lamp-keeper: the bus root: {why}"),
    }
}

/// Where `serve` leaves its answer for `health`: the service's data folder,
/// or where it was started.
fn data_folder() -> PathBuf {
    std::env::var_os(package::DATA_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The lamp's health, if someone keeps it fresh.
fn health(bus: &Bus) -> Option<Health> {
    let message = bus.latest(Health::TOPIC).ok().flatten()?;
    (!message.expired).then(|| message.parse().ok()).flatten()
}

/// True only when the voice says it is resting: a voice that says nothing,
/// or anything else, may be in someone's turn.
fn voice_resting(bus: &Bus) -> bool {
    bus.latest(VoiceState::TOPIC)
        .ok()
        .flatten()
        .and_then(|message| message.parse::<VoiceState>().ok())
        .is_some_and(|said| said.state == Voice::Resting)
}

fn serve(rules: Rules, mut package: Option<(Release, Vec<String>, Option<PathBuf>)>) -> ExitCode {
    // ponytail: no signal handling (the standard library has none, as for
    // lamp-health): SIGTERM ends it where it stands, between two looks.
    let notify = Notify::from_environment();
    let mut keeper = Keeper::new(rules);
    let mut out: Option<Publisher> = None;
    let started = Instant::now();
    eprintln!(
        "lamp-keeper: watching lamp/health; {}",
        if rules.act { "acting" } else { "watching only" }
    );
    notify.ready();
    // The bus is made by the voice; a keeper that started the lamp itself
    // may look before it exists.
    let mut bus: Option<Bus> = None;
    loop {
        let now = Instant::now();
        if bus.is_none() {
            bus = Bus::from_environment().ok();
        }
        if let Some((release, legacy_stopped, waiting_setup)) = &mut package {
            if let Some(marker) = waiting_setup.as_deref() {
                match package::start_when_set_up(&mut Systemctl, marker) {
                    Ok(true) => {
                        eprintln!("lamp-keeper: set up; the lamp is started");
                        *waiting_setup = None;
                    }
                    Ok(false) => {}
                    Err(why) => eprintln!("lamp-keeper: starting the lamp after setup: {why}"),
                }
            }
            let waiting_for = if waiting_setup.is_some() {
                vec!["setup".to_owned()]
            } else {
                package::waiting_for(&mut Systemctl, release)
            };
            let answer = Answer {
                ready: waiting_for.is_empty(),
                version: release.version(),
                pid: std::process::id(),
                waiting_for,
                legacy_stopped: legacy_stopped.clone(),
            };
            if let Err(why) = package::write_answer(&data_folder(), &answer) {
                eprintln!("lamp-keeper: {why}");
            }
        }
        let health = bus.as_ref().and_then(health);
        if let (Some(health), Some(bus)) = (&health, &bus) {
            for done in keeper.look(health, voice_resting(bus), now, &mut Systemctl) {
                eprintln!("lamp-keeper: {done}");
            }
        }
        // Its own report, as every part's.
        let given_up = keeper.given_up(now);
        let (level, message) = if !given_up.is_empty() {
            (
                Level::Warn,
                format!("will not stay up: {}", given_up.join(", ")),
            )
        } else if health.is_none() {
            (Level::Warn, "lamp/health is not heard".to_owned())
        } else {
            (Level::Ok, String::new())
        };
        let report = Diagnostic::new(level, NAME, &message)
            .with(STATE, PartState::Ready.as_str())
            .with("act", rules.act)
            .with("restarts_last_ten_minutes", keeper.restarts(now));
        if out.is_none() {
            out = bus.as_ref().and_then(|bus| {
                bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), NAME)
                    .ok()
            });
        }
        let at_us = started.elapsed().as_micros() as u64;
        if out
            .as_mut()
            .is_some_and(|out| out.publish_json(at_us, &report).is_err())
        {
            // Made again next time round.
            out = None;
        }
        notify.alive();
        thread::sleep(Duration::from_secs(1));
    }
}

fn status() -> ExitCode {
    let text = match Systemctl.show(&units::PATTERNS) {
        Ok(text) => text,
        Err(why) => {
            eprintln!("lamp-keeper: systemctl show: {why}");
            return ExitCode::FAILURE;
        }
    };
    let health = Bus::from_environment().ok().and_then(|bus| health(&bus));
    print!(
        "{}",
        units::status_lines(&units::parse_show(&text), health.as_ref())
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pause_is_kept_on_the_bus_until_resumed() {
        let root = std::env::temp_dir().join(format!("lamp-keeper-pause-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let kept = || {
            bus.latest(Pause::TOPIC)
                .unwrap()
                .map(|message| message.parse::<Pause>().unwrap().paused)
        };
        assert_eq!(kept(), None);
        publish_pause(&bus, true).unwrap();
        assert_eq!(kept(), Some(true));
        publish_pause(&bus, false).unwrap();
        assert_eq!(kept(), Some(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn waiting_ends_when_it_is_done_or_after_the_longest() {
        let mut looks = 0;
        assert!(wait_until(Duration::from_secs(5), Duration::ZERO, || {
            looks += 1;
            looks == 3
        }));
        assert_eq!(looks, 3);
        let started = Instant::now();
        assert!(!wait_until(
            Duration::from_millis(30),
            Duration::from_millis(5),
            || false
        ));
        assert!(started.elapsed() >= Duration::from_millis(30));
    }
}
