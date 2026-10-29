//! `lamp-cloud [--bus-root ABSOLUTE_DIR]`: the lamp on the company broker
//! (#1396). Off unless `LAMP_CLOUD=on`. Its unit hands it, through
//! `LoadCredential=`, setup's account (`account`), the voice's key (`key`)
//! and `lamp-update`'s state (`build`), so it runs as `lampos` and no file
//! is widened. A login a ping rotated is kept in its own state folder.
//!
//! `lamp-cloud exit-note UNIT`: run by systemd as `ExecStopPost=` of a
//! lampOS unit (`deploy/product/exit-note.conf`), it keeps what systemd
//! says of that stop for the reports (#1683), and nothing for a `success`.
use lamp_cloud::account::{self, Account};
use lamp_cloud::lamp::{self, Places, Real, Ring, Texts, Wifi};
use lamp_cloud::wire::Identity;
use lamp_cloud::{beat, online, ping, reports, skills};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use tokio::sync::watch;

const USAGE: &str = "usage: lamp-cloud [--bus-root ABSOLUTE_DIR] | lamp-cloud exit-note UNIT\n\
  | lamp-cloud make-skill make WORDS REQUEST_ID | made REQUEST_ID (the played check, #2813)\n\
  With LAMP_CLOUD=on, and CREDENTIALS_DIRECTORY holding account, key and build\n\
  (the unit's LoadCredential=); a rotated login is kept in STATE_DIRECTORY.";

/// How often the lamp's address and zone are looked at again for `info`
/// and the ping.
const IDENTITY_EVERY: Duration = Duration::from_secs(15);

fn say(line: &str) {
    eprintln!("lamp-cloud: {line}");
}

fn identity(lamp: &Real) -> Identity {
    Identity {
        device: "lamp".into(),
        version: lamp.build.clone(),
        id: lamp.device_id.clone(),
        mac: lamp.hostname(),
        timezone: lamp.timezone(),
        local_ip: lamp.local_ip(),
    }
}

/// `exit-note UNIT`: systemd's `$SERVICE_RESULT`, `$EXIT_CODE` and
/// `$EXIT_STATUS` for one stop of `UNIT`, with `systemctl show`'s restarts
/// and run time, kept in `/run/lampos-exits`. Always succeeds: a note that
/// cannot be kept must never make a stop fail.
fn exit_note(unit: &str) -> ExitCode {
    let given = |name: &str| std::env::var(name).unwrap_or_default();
    let (result, code, status) = (
        given("SERVICE_RESULT"),
        given("EXIT_CODE"),
        given("EXIT_STATUS"),
    );
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|boot| boot.trim().to_owned())
        .unwrap_or_default();
    let at_us = lamp_ipc::monotonic_us();
    let none = BTreeMap::new();
    if reports::exit_note(unit, &result, &code, &status, at_us, &boot, &none).is_none() {
        return ExitCode::SUCCESS;
    }
    let shown = reports::bounded_output(
        Path::new("/usr/bin/systemctl"),
        &[
            "show",
            unit,
            "-p",
            "NRestarts",
            "-p",
            "ExecMainStartTimestampMonotonic",
            "-p",
            "ExecMainExitTimestampMonotonic",
        ],
    )
    .map(|text| reports::shown(&text))
    .unwrap_or_default();
    if let Some(note) = reports::exit_note(unit, &result, &code, &status, at_us, &boot, &shown)
        && let Err(error) = reports::write_exit_note(Path::new(reports::EXITS), &note)
    {
        say(&format!(
            "the exit note could not be kept ({})",
            error.kind()
        ));
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let mut bus_root = None;
    let mut arguments = std::env::args().skip(1);
    // The played check's own ask of lamp-voice's app-make socket, as the
    // app's `skills.make` and `skills.made` make it (#2813): no switch,
    // since it is run by hand, as lampos (lamp-cloud's own user, so the
    // socket's group is what lets it in), on a lamp under test.
    if std::env::args().nth(1).as_deref() == Some("make-skill") {
        let rest: Vec<String> = std::env::args().skip(2).collect();
        let (told, code) = lamp_cloud::make_skill::command(&rest);
        println!("{told}");
        return ExitCode::from(code);
    }
    if std::env::args().nth(1).as_deref() == Some("exit-note") {
        return match std::env::args().nth(2) {
            Some(unit) => exit_note(&unit),
            None => {
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        };
    }
    while let Some(argument) = arguments.next() {
        match (argument.as_str(), arguments.next()) {
            ("--bus-root", Some(root)) if root.starts_with('/') => {
                bus_root = Some(PathBuf::from(root))
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    if std::env::var("LAMP_CLOUD").ok().as_deref() != Some("on") {
        say("off: LAMP_CLOUD is not on");
        return ExitCode::SUCCESS;
    }
    let Some(credentials) = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from) else {
        say("no CREDENTIALS_DIRECTORY: run it from its unit");
        return ExitCode::from(2);
    };
    // What setup kept; a login a ping rotated since, in place of it.
    let rotated =
        std::env::var_os("STATE_DIRECTORY").map(|dir| PathBuf::from(dir).join("rotated.json"));
    let Some(setup) = account::kept(&credentials.join("account")) else {
        say("setup's account cannot be read");
        return ExitCode::from(2);
    };
    let account = rotated
        .as_deref()
        .and_then(account::kept)
        .filter(|rotated| rotated.device_id == setup.device_id)
        .unwrap_or(setup);
    if !account.complete() {
        say("setup's account has no broker yet: only the ping, which may give one");
    }
    let key = std::fs::read_to_string(credentials.join("key"))
        .map(|key| key.trim().to_owned())
        .unwrap_or_default();
    let build = std::fs::read_to_string(credentials.join("build"))
        .map(|state| lamp::build(&state))
        .unwrap_or_default();
    let bus = match bus_root {
        Some(root) => lamp_bus::Bus::new(&root).ok(),
        None => lamp_bus::Bus::from_environment().ok(),
    };
    // Health and crash reports (#1683): only with LAMP_REPORTS=on too, and
    // then only with the person's yes, read at each send.
    let shared = reports::on_from_environment()
        .then(|| std::env::var_os("STATE_DIRECTORY"))
        .flatten()
        .map(|state| {
            reports::Shared::new(reports::Reports::open(reports::Places::product(Path::new(
                &state,
            ))))
        });
    let reporting = shared
        .clone()
        .map(|shared| (shared, reports::Lamp::new(bus.clone())));
    let report_build = reports::Build::of(&build);
    let mut lamp = Real {
        places: Places::default(),
        bus,
        device_id: account.device_id.clone(),
        build,
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::on(lamp_cloud::chat::on_from_environment())
            .knowing(std::env::var_os(lamp_cloud::lamp::KNOWN_FILE_ENVIRONMENT).map(PathBuf::from)),
        reports: shared,
        // The app's skills page (#1722): only with LAMP_SKILLS_PAGE=on too.
        skills: skills::on_from_environment().then(skills::Places::product),
        // Names and the zone after setup (#1959): only with LAMP_SETTINGS_CHANGE=on too.
        // The Wi-Fi too only with LAMP_SETTINGS_WIFI=on (#1959's second piece).
        // A person's own keys only with LAMP_OWN_KEYS=on (#2094).
        // The owner's place only with LAMP_OWNER_PLACE=on (#4333).
        changes: lamp_cloud::settings::on_from_environment().then(|| {
            lamp_cloud::settings::Changes::product(
                lamp_cloud::settings::wifi_on_from_environment(),
                lamp_cloud::settings::keys_on_from_environment(),
            )
            .taking_the_place(lamp_cloud::settings::place_on_from_environment())
        }),
        // A face from the app's photo (#1968): only with LAMP_PHOTO_FACES=on.
        photos: lamp_cloud::photos::Place::from_environment(),
        // The faces it knows, and removing one (#2192): only with LAMP_FACES_PAGE=on.
        faces: lamp_cloud::faces::Eyes::from_environment(),
        // The lamp's timers and reminders for the app (#2195): only with LAMP_APP_SCHEDULE=on.
        schedule: lamp_cloud::schedule::from_environment(),
        // The lamp's software update for the app (#2194): only with LAMP_APP_UPDATE=on.
        update_page: lamp_cloud::update::from_environment(),
        update_checks: Default::default(),
        // A restart from the app, asked of setup's change helper (#2193): only with LAMP_APP_RESTART=on.
        restart: (std::env::var(lamp_cloud::lamp::RESTART_ENVIRONMENT)
            .ok()
            .as_deref()
            == Some("on"))
        .then(|| lamp_cloud::settings::SOCKET.into()),
        // The lamp turned off from the app, asked of setup's change helper (#3697): only with LAMP_SHUTDOWN=on.
        shutdown: lamp_cloud::lamp::Shutdown::from_environment(),
        // Guard mode on and off from the owner's app (#2562): only with LAMP_GUARD_FROM_APP=on.
        guard: lamp_cloud::guard::from_environment(),
        // A skill typed in the app (#2813): only with LAMP_APP_MAKE_SKILL=on and the skills page.
        make_skill: skills::on_from_environment()
            .then(lamp_cloud::make_skill::from_environment)
            .flatten(),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            say("no runtime");
            return ExitCode::from(1);
        }
    };
    let (accounts_tx, accounts) = watch::channel(Some(account));
    let (identity_tx, me) = watch::channel(identity(&lamp));
    let looking = Real {
        places: lamp.places.clone(),
        bus: None,
        device_id: lamp.device_id.clone(),
        build: lamp.build.clone(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    runtime.block_on(async move {
        let looks = async move {
            loop {
                tokio::time::sleep(IDENTITY_EVERY).await;
                identity_tx.send_if_modified(|me| {
                    let now = identity(&looking);
                    let changed = *me != now;
                    *me = now;
                    changed
                });
            }
        };
        let mut said = |line: &str| say(line);
        let mut keep = |next: &Account| {
            if let Some(path) = &rotated
                && account::keep(path, next).is_err()
            {
                say("the rotated login could not be kept: it lasts until a restart");
            }
        };
        let mut said_too = |line: &str| say(line);
        let pinging = beat::pings(
            accounts_tx,
            me.clone(),
            key,
            ping::EVERY,
            &mut said,
            &mut keep,
        );
        let staying = online::run(accounts, me, &mut lamp, &mut said_too, ping::FIRST);
        let reporting = async move {
            match reporting {
                Some((shared, sources)) => reports::run(shared, sources, report_build).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = pinging => {}
            () = staying => {}
            () = looks => {}
            () = reporting => {}
        }
    });
    ExitCode::from(1)
}
