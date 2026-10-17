//! `lamp-cloud [--bus-root ABSOLUTE_DIR]`: the lamp on the company broker
//! (#1396). Off unless `LAMP_CLOUD=on`. Its unit hands it, through
//! `LoadCredential=`, setup's account (`account`), the voice's key (`key`)
//! and `lamp-update`'s state (`build`), so it runs as `lampos` and no file
//! is widened. A login a ping rotated is kept in its own state folder.
use lamp_cloud::account::{self, Account};
use lamp_cloud::lamp::{self, Places, Real};
use lamp_cloud::wire::Identity;
use lamp_cloud::{beat, online, ping};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio::sync::watch;

const USAGE: &str = "usage: lamp-cloud [--bus-root ABSOLUTE_DIR]\n\
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

fn main() -> ExitCode {
    let mut bus_root = None;
    let mut arguments = std::env::args().skip(1);
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
    let mut lamp = Real {
        places: Places::default(),
        bus,
        device_id: account.device_id.clone(),
        build,
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
        tokio::select! {
            () = pinging => {}
            () = staying => {}
            () = looks => {}
        }
    });
    ExitCode::from(1)
}
