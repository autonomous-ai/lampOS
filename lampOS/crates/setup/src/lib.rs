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

/// Watch `controls/touch` and act once on the first valid reset. With
/// `enabled` false a valid reset is only reported, and watching goes on.
/// `now` is the boot clock in microseconds.
pub fn watch(
    touch: &mut Subscriber,
    clock_id: &str,
    enabled: bool,
    until: Option<Instant>,
    now: impl Fn() -> u64,
    log: impl FnMut(&str),
    mut act: impl FnMut() -> io::Result<()>,
) -> io::Result<Watched> {
    watch_with(
        touch,
        clock_id,
        enabled,
        until,
        now,
        log,
        "reset button held 5 s: stopping the lamp and wiping the person's data",
        || act().map(|()| true),
    )
}

/// Watch `controls/touch` with a plan this build refuses
/// ([`reset::Refused::Ahead`], #2060): each valid reset is refused before
/// its journal and the marker, logged, and told to the person with `tell`
/// ([`reset::SAY_REFUSED`]); nothing is wiped and watching goes on.
pub fn watch_refused(
    touch: &mut Subscriber,
    clock_id: &str,
    enabled: bool,
    until: Option<Instant>,
    now: impl Fn() -> u64,
    log: impl FnMut(&str),
    mut tell: impl FnMut(),
) -> io::Result<Watched> {
    watch_with(
        touch,
        clock_id,
        enabled,
        until,
        now,
        log,
        "reset button held 5 s: refused, the reset plan is a newer build's; nothing was wiped (update the lamp, then reset again)",
        || {
            tell();
            Ok(false)
        },
    )
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
        let Some(message) = touch.recv(Duration::from_millis(100))? else {
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
