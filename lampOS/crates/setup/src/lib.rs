//! A lamp's first boot and its way back there (#806, `docs/setup.md`).
pub mod change;
pub mod http;
pub mod keys;
pub mod light;
pub mod recover;
pub mod reset;
pub mod scan;
pub mod serve;
pub mod shutdown;
pub mod wake;
pub mod wifi;
pub mod wifi_change;

/// The bus name `lamp-setup` subscribes under for touch.
pub const SUBSCRIBER: &str = "setup";

/// Every name this crate subscribes to the bus under. A new subscriber is
/// added here, and a test holds each to the bus's rule for a name.
pub const SUBSCRIBERS: [&str; 2] = [SUBSCRIBER, change::SUBSCRIBER];

use lamp_bus::Subscriber;
use lamp_msgs::TouchEvent;
use std::{
    io,
    time::{Duration, Instant},
};

/// Read a written file one version ahead (#1680, the rule of #1505): drop
/// the top-level fields this build does not know, naming each in `notes`,
/// so the known fields are read as before and a bad known value is still
/// refused. `known` is the struct's fields; a test holds them equal.
pub fn drop_unknown_fields(
    value: &mut serde_json::Value,
    store: &str,
    known: &[&str],
    notes: &mut Vec<String>,
) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|field, _| {
            let keep = known.contains(&field.as_str());
            if !keep {
                notes.push(format!(
                    "{store}: field `{field}` is a newer build's, read past and dropped"
                ));
            }
            keep
        });
    }
}

/// What happened to a reset request while watching.
#[derive(Debug, PartialEq, Eq)]
pub enum Watched {
    /// A valid reset was acted on.
    Reset,
    /// A valid reset was seen with the switch off, and only logged.
    Shadow,
    /// The time to watch ran out.
    Ended,
    /// A valid reset was seen with a plan this build refuses
    /// ([`reset::Refused::Ahead`]): refused, logged and told to the person,
    /// nothing wiped.
    Refused,
}

/// The watcher's part in turning the lamp off (#3697, `docs/shutdown.md`):
/// [`lamp_msgs::ShutdownRequest`]s, whoever asks (the button, the app),
/// each taken by [`lamp_msgs::ShutdownRequest::accept`].
pub struct ShutdownWatch<'a> {
    /// Subscribed to [`lamp_msgs::ShutdownRequest::TOPIC`].
    pub requests: &'a mut Subscriber,
    /// Starts `lamp-setup shutdown`: `Some` with `LAMP_SHUTDOWN` on; `None`
    /// off, when a valid request is only logged.
    pub start: Option<&'a mut dyn FnMut() -> io::Result<()>>,
    /// Whether the runner `start` handed the shutdown to is still at work
    /// (its unit, `lamp-setup-shutdown`, still loaded). Asked before each
    /// start, in place of a flag set when `start` returns (the Reviewer-lead's
    /// P2 on #3729): `start` returning only says the runner was handed the
    /// shutdown, not that it took it. The runner refuses while an update
    /// installs, and ends without powering off when a step fails; a flag
    /// kept from that start would pass over every later hold this boot. The
    /// unit is gone once the runner ends (`--collect`), so a later request
    /// starts it again, and one while it works is `already turning off`.
    pub running: &'a mut dyn FnMut() -> bool,
}

/// `None`: no requests are watched (the reset alone).
pub type Shutdown<'a> = Option<ShutdownWatch<'a>>;

/// Watch `controls/touch` and act once on the first valid reset. With
/// `enabled` false a valid reset is only reported, and watching goes on.
/// `now` is the boot clock in microseconds. The first valid shutdown request
/// starts the shutdown ([`ShutdownWatch`]), and watching goes on.
#[allow(clippy::too_many_arguments)]
pub fn watch(
    touch: &mut Subscriber,
    clock_id: &str,
    enabled: bool,
    until: Option<Instant>,
    now: impl Fn() -> u64,
    log: impl FnMut(&str),
    shutdown: Shutdown<'_>,
    mut act: impl FnMut() -> io::Result<()>,
) -> io::Result<Watched> {
    watch_with(
        touch,
        clock_id,
        enabled,
        until,
        now,
        log,
        shutdown,
        "reset button held 5 s: stopping the lamp and wiping the person's data",
        || act().map(|()| true),
    )
}

/// Watch `controls/touch` with a plan this build refuses
/// ([`reset::Refused::Ahead`], #2060): each valid reset is refused before
/// its journal and the marker, logged, and told to the person with `tell`
/// ([`reset::SAY_REFUSED`]); nothing is wiped and watching goes on. The
/// shutdown is not the plan's: it is acted on as by [`watch`].
#[allow(clippy::too_many_arguments)]
pub fn watch_refused(
    touch: &mut Subscriber,
    clock_id: &str,
    enabled: bool,
    until: Option<Instant>,
    now: impl Fn() -> u64,
    log: impl FnMut(&str),
    shutdown: Shutdown<'_>,
    mut tell: impl FnMut(),
) -> io::Result<Watched> {
    watch_with(
        touch,
        clock_id,
        enabled,
        until,
        now,
        log,
        shutdown,
        "reset button held 5 s: refused, the reset plan is a newer build's; nothing was wiped (update the lamp, then reset again)",
        || {
            tell();
            Ok(false)
        },
    )
}

/// One message on the shutdown's topic: logged, and the first valid one
/// with the switch on and `--enable` starts the shutdown. A start that fails
/// is said, and watching goes on.
fn shutdown_asked(
    message: &lamp_bus::Message,
    watch: &mut ShutdownWatch<'_>,
    clock_id: &str,
    enabled: bool,
    now: &impl Fn() -> u64,
    log: &mut impl FnMut(&str),
) {
    let request = match message.parse::<lamp_msgs::ShutdownRequest>() {
        Ok(request) => request,
        Err(error) => {
            log(&format!(
                "shutdown request passed over: unreadable: {error}"
            ));
            return;
        }
    };
    let asked = format!(
        "{:?} asks to turn the lamp off ({})",
        request.source, request.reason
    );
    if message.expired {
        log(&format!("{asked}: passed over: expired"));
        return;
    }
    match (
        request.accept(&message.header.source, clock_id, now()),
        watch.start.as_mut(),
    ) {
        (Err(why), _) => log(&format!("{asked}: passed over: {why}")),
        (Ok(()), None) => log(&format!(
            "{asked}: not acted on, {} is not on",
            shutdown::SWITCH
        )),
        (Ok(()), Some(_)) if !enabled => {
            log(&format!("{asked}: not acted on, --enable is not set"))
        }
        (Ok(()), Some(_)) if (watch.running)() => log(&format!("{asked}: already turning off")),
        (Ok(()), Some(start)) => {
            log(&format!("{asked}: turning the lamp off"));
            if let Err(error) = start() {
                log(&format!("the shutdown did not start: {error}"));
            }
        }
    }
}

/// The watch: `held` is logged for each valid reset, then `act`, which says
/// whether the reset was done (and watching ends) or refused (and goes on).
#[allow(clippy::too_many_arguments)]
fn watch_with(
    touch: &mut Subscriber,
    clock_id: &str,
    enabled: bool,
    until: Option<Instant>,
    now: impl Fn() -> u64,
    mut log: impl FnMut(&str),
    mut shutdown: Shutdown<'_>,
    held: &str,
    mut act: impl FnMut() -> io::Result<bool>,
) -> io::Result<Watched> {
    let mut shadow = false;
    let mut refused = false;
    loop {
        if until.is_some_and(|until| Instant::now() >= until) {
            return Ok(if refused {
                Watched::Refused
            } else if shadow {
                Watched::Shadow
            } else {
                Watched::Ended
            });
        }
        let mut wait = Duration::from_millis(100);
        if let Some(watch) = shutdown.as_mut() {
            wait = Duration::from_millis(50);
            if let Some(message) = watch.requests.recv(wait)? {
                shutdown_asked(&message, watch, clock_id, enabled, &now, &mut log);
            }
        }
        let Some(message) = touch.recv(wait)? else {
            continue;
        };
        let Ok(event) = message.parse::<TouchEvent>() else {
            continue;
        };
        match reset::accept(&event, clock_id, now()) {
            Ok(()) if enabled => {
                log(held);
                if act()? {
                    return Ok(Watched::Reset);
                }
                refused = true;
            }
            Ok(()) => {
                log("reset button held 5 s: not acted on, the switch is off");
                shadow = true;
            }
            Err(why) => {
                if event.source == lamp_msgs::TouchSource::Reset {
                    log(&format!("reset event passed over: {why}"));
                }
            }
        }
    }
}
