//! The lamp's own light, told to the voice's second route
//! (`LAMP_LIGHT_TOLD`).
//!
//! lamp-52e6 at build 9928 (4 Nov 09:30): asked "what colour is the light?"
//! three times while `light/state` said warm at level 1 the whole run, the
//! lamp answered "White — my light is on in plain white". The voice had no
//! way to read its light (the executive's catalog has only the light's
//! setters) and no turn was told it, so the model guessed. With the switch,
//! each second-route turn is told the light as the ring says it shows it
//! now (`light/state`, [`crate::ring_light`]), in one short line: on or off,
//! and when on its colour by name and its level of [`Light::BRIGHTEST`].
//! Never a function: past 32 the voice silently drops tools.
//!
//! The light is read from the bus's kept latest value as the turn's system
//! message is made: one small file read in `/run`, never a wait. A state
//! the ring has stopped saying (it says it each second, each good for 3 s,
//! `ring_light::STATE_LASTS`), one with no lifetime, one that is no light
//! there is, or none at all, says nothing: never a guess.
//!
//! Off unless the service's environment says exactly `on`.
use lamp_bus::Bus;
use lamp_msgs::Light;

/// Where the second worker reads whether the light is told. `on` only.
pub const ENVIRONMENT: &str = "LAMP_LIGHT_TOLD";

/// What the model is told of a light that is off: never a colour.
pub const OFF: &str = "Your light now: off.";

/// Whether the switch is on in this process's environment.
pub fn on_from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What the model is told of `light`: off, or on with its colour and its
/// level; nothing for one that is no light there is (a colour not in
/// [`Light::COLOURS`], a level outside 1 to [`Light::BRIGHTEST`]).
pub fn line(light: &Light) -> Option<String> {
    if !light.on {
        return Some(OFF.to_owned());
    }
    if !Light::COLOURS.contains(&light.colour.as_str())
        || !(1..=Light::BRIGHTEST).contains(&light.level)
    {
        return None;
    }
    Some(format!(
        "Your light now: on, {}, level {} of {}.",
        light.colour,
        light.level,
        Light::BRIGHTEST
    ))
}

/// The line for the light `light/state` says on `bus` now; nothing when it
/// says none, says it no longer (expired), was said with no lifetime, or
/// is no light there is.
pub fn now(bus: &Bus) -> Option<String> {
    let message = bus.latest(Light::TOPICS[1]).ok().flatten()?;
    if message.expired || message.header.expires_ms.is_none() {
        return None;
    }
    line(&message.parse::<Light>().ok()?)
}

/// [`now`] as the second route's system message carries it, after the
/// other facts (`worker::facts`): empty with the switch off (no bus given)
/// or nothing to tell, so the message is byte for byte as before.
pub fn fact(bus: Option<&Bus>) -> String {
    bus.and_then(now)
        .map_or(String::new(), |line| format!("\n\n{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Root(std::path::PathBuf);
    impl Root {
        fn new(name: &str) -> Self {
            let root = std::path::PathBuf::from(format!("/tmp/lt-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light {
            on,
            colour: colour.into(),
            level,
        }
    }

    /// `light` said on `light/state` under `root` as the ring says it,
    /// good for `lasting`.
    fn said(root: &Root, light: &Light, lasting: Option<Duration>) -> Bus {
        let bus = Bus::new(&root.0).unwrap();
        let mut ring = bus
            .publisher_declared(Light::TOPICS[1], &Light::declaration(), "ring")
            .unwrap();
        ring.expires_after(lasting);
        ring.publish_json(lamp_ipc::monotonic_us(), light).unwrap();
        bus
    }

    const LASTS: Option<Duration> = Some(Duration::from_secs(3));

    /// lamp-52e6 at 9928: `light/state` warm at level 1, the turn is told
    /// so, in one line after the other facts.
    #[test]
    fn a_warm_light_at_level_one_is_told_as_it_is() {
        let root = Root::new("warm");
        let bus = said(&root, &light(true, "warm", 1), LASTS);
        assert_eq!(
            now(&bus).as_deref(),
            Some("Your light now: on, warm, level 1 of 5.")
        );
        assert_eq!(
            fact(Some(&bus)),
            "\n\nYour light now: on, warm, level 1 of 5."
        );
    }

    /// Off says off, and never a colour or a level.
    #[test]
    fn a_light_that_is_off_is_told_off_with_no_colour() {
        let root = Root::new("off");
        let bus = said(&root, &light(false, "red", 4), LASTS);
        assert_eq!(now(&bus).as_deref(), Some(OFF));
        assert!(!fact(Some(&bus)).contains("red"));
    }

    /// Nothing said, a state the ring no longer says, one with no lifetime
    /// or one that is no light there is: nothing told, never a guess.
    #[test]
    fn a_missing_stale_or_unknown_light_says_nothing() {
        let none = Root::new("none");
        assert_eq!(now(&Bus::new(&none.0).unwrap()), None);
        let stale = Root::new("stale");
        let bus = said(
            &stale,
            &light(true, "warm", 1),
            Some(Duration::from_millis(1)),
        );
        // Past its lifetime and the bus's 50 ms grace.
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(now(&bus), None);
        assert_eq!(fact(Some(&bus)), "");
        let forever = Root::new("forever");
        assert_eq!(now(&said(&forever, &light(true, "warm", 1), None)), None);
        for (at, wrong) in [light(true, "white-ish", 1), light(true, "warm", 9)]
            .into_iter()
            .enumerate()
        {
            let root = Root::new(&format!("wrong{at}"));
            assert_eq!(now(&said(&root, &wrong, LASTS)), None, "{wrong:?}");
        }
    }

    /// The switch off gives no bus to read, and the system message is byte
    /// for byte as before; on only with exactly `on`.
    #[test]
    fn the_switch_off_adds_nothing() {
        assert_eq!(fact(None), "");
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("yes"), Some("ON"), Some("on ")] {
            assert!(!on(value), "{value:?}");
        }
        for level in 1..=Light::BRIGHTEST {
            assert!(line(&light(true, "blue", level)).is_some());
        }
    }
}
