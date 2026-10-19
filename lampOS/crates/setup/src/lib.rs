//! A lamp's first boot and its way back there (#806, `docs/setup.md`).
pub mod http;
pub mod light;
pub mod reset;
pub mod scan;
pub mod serve;
pub mod wake;
pub mod wifi;

use lamp_bus::Subscriber;
use lamp_msgs::TouchEvent;
use std::{
    io,
    time::{Duration, Instant},
};

/// What happened to a reset request while watching.
#[derive(Debug, PartialEq, Eq)]
pub enum Watched {
    /// A valid reset was acted on.
    Reset,
    /// A valid reset was seen with the switch off, and only logged.
    Shadow,
    /// The time to watch ran out.
    Ended,
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
    mut log: impl FnMut(&str),
    mut act: impl FnMut() -> io::Result<()>,
) -> io::Result<Watched> {
    let mut shadow = false;
    loop {
        if until.is_some_and(|until| Instant::now() >= until) {
            return Ok(if shadow {
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
                log("reset button held 5 s: stopping the lamp and wiping the person's data");
                act()?;
                return Ok(Watched::Reset);
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
