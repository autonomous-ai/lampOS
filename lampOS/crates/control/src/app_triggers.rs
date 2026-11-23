//! What lamp-control tells the voice of the apps' spoken triggers (#7415,
//! piece 3b-2b, `docs/apps.md`): each running app's manifest triggers on
//! `apps/triggers`, and a turn an app took on `apps/took`. It listens on
//! `apps/trigger`, where the voice says it holds a line that spells one, and
//! offers it to the app (`app.trigger`; the app answers `app.took`).
//!
//! Only apps running now are told: an app with no socket up is never named,
//! and a trigger of an app that is not running goes the model's way.
//! Everything here is behind `LAMP_STORE_APPS` like the app socket.
use crate::server::Shared;
use lamp_msgs::{AppTook, AppTrigger, AppTriggers, AppsTriggers};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the list is said again when nothing changed (the topic is
/// read as old after 30 s, `AppTriggers::declaration`).
const SAY_EVERY: Duration = Duration::from_secs(5);
/// How often it is looked at for a change.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// The triggers of `running` apps, from the manifests under `apps`: apps
/// with none left out, and one whose manifest the voice's topic would not
/// take is left out whole.
pub fn list_of(apps: &Path, running: &[String]) -> AppTriggers {
    let mut list = AppTriggers { apps: Vec::new() };
    for name in running {
        let Some(manifest) = crate::app_yes::installed(apps, name) else {
            continue;
        };
        if manifest.triggers.is_empty() {
            continue;
        }
        let mut app = AppsTriggers {
            app: name.clone(),
            triggers: manifest.triggers,
        };
        app.triggers.truncate(AppTriggers::MAX_TRIGGERS);
        let one = AppTriggers {
            apps: vec![app.clone()],
        };
        if one.check().is_ok() && list.apps.len() < AppTriggers::MAX_APPS {
            list.apps.push(app);
        }
    }
    list
}

/// Starts the bus side: the list said as it changes and now and then, the
/// voice's `apps/trigger` offered to the app, and a turn taken told on
/// `apps/took`. Each part that can't start is said and the rest goes on.
pub fn start(bus: &lamp_bus::Bus, shared: Arc<Shared>) {
    match bus.publisher_declared(AppTriggers::TOPIC, &AppTriggers::declaration(), "control") {
        Ok(mut publisher) => {
            let shared = shared.clone();
            let spawned = std::thread::Builder::new()
                .name("app-triggers".into())
                .spawn(move || {
                    let mut said: Option<(AppTriggers, Instant)> = None;
                    loop {
                        let now = Instant::now();
                        let list = shared.running_triggers();
                        let due = match &said {
                            Some((last, at)) => {
                                *last != list || now.duration_since(*at) >= SAY_EVERY
                            }
                            None => true,
                        };
                        if due {
                            match publisher.publish_json(lamp_ipc::monotonic_us(), &list) {
                                Ok(_) => said = Some((list, now)),
                                Err(error) => {
                                    eprintln!(
                                        "lamp-control: the apps' triggers were not told: {error}"
                                    )
                                }
                            }
                        }
                        std::thread::sleep(LOOK_EVERY);
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no app triggers are told: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no app triggers are told: {error}"),
    }
    match bus.publisher_declared(AppTook::TOPIC, &AppTook::declaration(), "control") {
        Ok(publisher) => {
            let publisher = Mutex::new(publisher);
            shared.set_took_sink(Arc::new(move |app: &str, turn: u64| {
                let took = AppTook {
                    turn,
                    app: app.to_owned(),
                };
                if let Ok(mut publisher) = publisher.lock()
                    && let Err(error) = publisher.publish_json(lamp_ipc::monotonic_us(), &took)
                {
                    eprintln!(
                        "lamp-control: {app} took turn {turn}, the voice was not told: {error}"
                    );
                }
            }));
        }
        Err(error) => eprintln!("lamp-control: no turn taken is told: {error}"),
    }
    match bus.subscribe(AppTrigger::TOPIC, "control") {
        Ok(mut subscriber) => {
            let spawned = std::thread::Builder::new()
                .name("app-trigger".into())
                .spawn(move || {
                    let mut unread_said = false;
                    loop {
                        match subscriber.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => match message.parse::<AppTrigger>() {
                                Ok(said) if said.check().is_ok() => {
                                    let offered = shared.offer_trigger(&said);
                                    eprintln!(
                                        "lamp-control: app {}: trigger for turn {}: {}",
                                        said.app,
                                        said.turn,
                                        if offered { "offered" } else { "not offered" }
                                    );
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    if !unread_said {
                                        unread_said = true;
                                        eprintln!(
                                            "lamp-control: apps/trigger: a message not read, passed over: {error}"
                                        );
                                    }
                                }
                            },
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-control: apps/trigger can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no trigger is offered to an app: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no trigger is offered to an app: {error}"),
    }
}
