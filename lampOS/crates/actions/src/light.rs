//! The lamp's light: asked of the ring's owner, and read back from it.
//!
//! The executive does not write the ring. It publishes the light it wants
//! on the bus (`light/command`, the newest value kept) and reads what the
//! ring's owner says it is showing (`light/state`), both
//! `lamp_msgs::Light`: on or off, one of ten colours by name, a level
//! from 1 to 5. A lamp whose ring owner has never said what it shows, or
//! stopped saying it more than [`LIGHT_GRACE`] ago, has no light to ask
//! for, and the light's functions are not offered.
//!
//! The rules: on and off keep the colour and the level; a colour switches
//! the light on; brighter and dimmer are one level, and stop at the ends;
//! warmer and cooler are one step along white, warm and orange, keep the
//! level, and stop at the ends.
//! A light that is off is not dark: the ring's owner keeps a faint glow,
//! which is the sign that the lamp can hear, and the answer says so.
//!
//! While the ring shows something over the person's light (a skill's glow
//! on `light/request`, #940, or a self-test's colour), the light it draws
//! differs from the person's: then nothing is "already" so, and the
//! person's light is asked for even when it is theirs, since a request
//! from the person is what ends the glow (the reviewer on #993).

use crate::Outcome;
use lamp_msgs::Light as Wanted;
use std::time::{Duration, Instant};

/// A light that can be asked for and read back.
pub trait Light {
    /// Whether its owner is there to carry a request out.
    fn there(&mut self) -> bool;
    /// What it shows now, as its owner reports it.
    fn shown(&mut self) -> Option<Wanted>;
    /// Ask for a light. False when the request could not even be made.
    fn ask(&mut self, light: &Wanted) -> bool;
    /// Whether the ring draws something other than the person's light now
    /// (a glow or a test over it). None known is not covered.
    fn covered(&mut self) -> bool {
        false
    }
}

/// How long the owner has to show what was asked for.
pub const CONFIRM_WAIT: Duration = Duration::from_millis(600);
/// How often what the owner shows is read again while waiting for it: the
/// ring's own pass, 2 ms. The voice's answer waits for this read, so the
/// person hears its gap on every light call. It was 15 ms (#1806).
const CONFIRM_LOOKS: Duration = Duration::from_millis(2);
const UNCONFIRMED: &str = "I asked for it but could not confirm my light changed.";
const UNREAD: &str = "I could not tell what my light is showing.";
const OFF: &str = "My light is off. The faint glow that is left shows I am still listening.";

/// Ask, then read back until the owner shows it or the wait is over.
fn set<L: Light>(light: &mut L, wanted: Wanted, said: String) -> Outcome {
    if !light.ask(&wanted) {
        return Outcome::Failed("I could not ask for my light to change.".into());
    }
    let started = Instant::now();
    loop {
        if light.shown().as_ref() == Some(&wanted) {
            return Outcome::Done(said);
        }
        if started.elapsed() >= CONFIRM_WAIT {
            return Outcome::Failed(UNCONFIRMED.into());
        }
        std::thread::sleep(CONFIRM_LOOKS);
    }
}

/// On, in the colour and at the level it had.
pub fn on<L: Light>(light: &mut L) -> Outcome {
    match light.shown() {
        None => Outcome::Failed(UNREAD.into()),
        Some(now) if now.on && !light.covered() => {
            Outcome::Already("My light is already on.".into())
        }
        Some(now) => {
            let said = format!("My light is on, {}.", spoken(&now.colour));
            set(light, Wanted { on: true, ..now }, said)
        }
    }
}

pub fn off<L: Light>(light: &mut L) -> Outcome {
    match light.shown() {
        None => Outcome::Failed(UNREAD.into()),
        Some(now) if !now.on && !light.covered() => {
            Outcome::Already("My light is already off.".into())
        }
        Some(now) => set(light, Wanted { on: false, ..now }, OFF.into()),
    }
}

/// One of the ten colours, on, at the level it had.
pub fn colour<L: Light>(light: &mut L, name: &str) -> Outcome {
    if !Wanted::is_colour(name) {
        return Outcome::Failed("That is not a colour my light can show.".into());
    }
    match light.shown() {
        None => Outcome::Failed(UNREAD.into()),
        Some(now) if now.on && now.colour == name && !light.covered() => {
            Outcome::Already(format!("My light is already {}.", spoken(name)))
        }
        Some(now) => set(
            light,
            Wanted {
                on: true,
                colour: name.to_owned(),
                level: now.level,
            },
            format!("My light is {} now.", spoken(name)),
        ),
    }
}

/// One level up, keeping the colour. A light that is off comes on as it
/// was: asked for more light, the first thing is to have any.
pub fn brighter<L: Light>(light: &mut L) -> Outcome {
    let Some(now) = light.shown() else {
        return Outcome::Failed(UNREAD.into());
    };
    if !now.on {
        let said = format!(
            "My light is on, at level {} of {}.",
            now.level,
            Wanted::BRIGHTEST
        );
        return set(light, Wanted { on: true, ..now }, said);
    }
    if now.level >= Wanted::BRIGHTEST {
        if light.covered() {
            return set(light, now, "My light is at its brightest.".into());
        }
        return Outcome::Already("My light is already at its brightest.".into());
    }
    let level = now.level + 1;
    set(
        light,
        Wanted { level, ..now },
        format!(
            "My light is a step brighter, at level {level} of {}.",
            Wanted::BRIGHTEST
        ),
    )
}

/// One level down, keeping the colour. The lowest level is not off:
/// darkness is asked for by name.
pub fn dimmer<L: Light>(light: &mut L) -> Outcome {
    let Some(now) = light.shown() else {
        return Outcome::Failed(UNREAD.into());
    };
    if !now.on {
        if light.covered() {
            return set(light, now, OFF.into());
        }
        return Outcome::Already("My light is already off.".into());
    }
    if now.level <= 1 {
        if light.covered() {
            return set(light, now, "My light is at its dimmest.".into());
        }
        return Outcome::Already(
            "My light is already at its dimmest. I can switch it off if you like.".into(),
        );
    }
    let level = now.level - 1;
    set(
        light,
        Wanted { level, ..now },
        format!(
            "My light is a step dimmer, at level {level} of {}.",
            Wanted::BRIGHTEST
        ),
    )
}

/// The light's warmth, coolest first: white, the amber warm, and orange,
/// each with less green and blue than the one before. "Warmer" and "cooler"
/// are a step along these (the first builder, 17 Oct 14:16: "make your
/// light a little warmer" had no tool that does it).
pub const WARMTH: [&str; 3] = ["white", "warm", "orange"];

/// A colour as every answer says it: `warm` is a warm white, so the lamp
/// calls one colour by one name (El on #1476). Said bare, "a step cooler, warm now" came back as "Okay, warmed
/// it up a step" (the first builder's light scene on lamp-4ace, 18 Oct, on
/// #1367): the model heard the colour's name as the way it went.
fn spoken(colour: &str) -> &str {
    if colour == "warm" {
        "warm white"
    } else {
        colour
    }
}

/// One step warmer: white to warm, warm to orange. A light in another
/// colour, asked for warmth, becomes warm. It comes on, at its level.
pub fn warmer<L: Light>(light: &mut L) -> Outcome {
    warmth(light, true)
}

/// One step cooler: orange to warm, warm to white. A light in another
/// colour, asked for a cooler or whiter light, becomes white.
pub fn cooler<L: Light>(light: &mut L) -> Outcome {
    warmth(light, false)
}

fn warmth<L: Light>(light: &mut L, warmer: bool) -> Outcome {
    let Some(now) = light.shown() else {
        return Outcome::Failed(UNREAD.into());
    };
    let way = if warmer { "warmer" } else { "cooler" };
    // A colour off the scale goes to warm or white, which is no step from
    // it: then the answer says only what it is now (the reviewer on #1316).
    let on_scale = WARMTH.contains(&now.colour.as_str());
    let next = match WARMTH.iter().position(|shade| *shade == now.colour) {
        None if warmer => Some("warm"),
        None => Some("white"),
        Some(at) if warmer => WARMTH.get(at + 1).copied(),
        Some(at) => at.checked_sub(1).map(|below| WARMTH[below]),
    };
    let Some(colour) = next else {
        let end = if warmer { "warm" } else { "cool" };
        if now.on && !light.covered() {
            return Outcome::Already(format!(
                "My light is already as {end} as it goes, {}.",
                now.colour
            ));
        }
        let said = format!("My light is on, as {end} as it goes, {}.", now.colour);
        return set(light, Wanted { on: true, ..now }, said);
    };
    let said = if on_scale {
        format!(
            "My light is a step {way}, from {} to {}.",
            spoken(&now.colour),
            spoken(colour)
        )
    } else {
        format!("My light is {} now.", spoken(colour))
    };
    set(
        light,
        Wanted {
            on: true,
            colour: colour.to_owned(),
            level: now.level,
        },
        said,
    )
}

/// The light through the bus.
pub struct BusLight {
    bus: lamp_bus::Bus,
    wanted: lamp_bus::Publisher,
    started: Instant,
    /// When a state was last fresh, and how long after it the light is
    /// still offered ([`LIGHT_GRACE`]).
    seen: Option<Instant>,
    grace: Duration,
}

/// How long the light is still offered after its state was last fresh. The
/// ring is the voice's: a voice restart stops `light/state` for a second or
/// two, and the light's functions dropped out of the catalog for that gap,
/// so a session set up in it had none (the first builder's lamp run on
/// #1495, 13:57:19–21; Ten's (a), the first builder's yes at 14:05). A call
/// in the gap still answers truthfully: with the state stale, every light
/// function stops at its first read with "I could not tell what my light is
/// showing", and nothing is asked of the ring. A ring really gone drops out
/// after it.
pub const LIGHT_GRACE: Duration = Duration::from_secs(15);

/// Where the executive asks, and where the ring's owner answers.
pub const WANTED: &str = Wanted::TOPICS[0];
pub const STATE: &str = Wanted::TOPICS[1];
/// What the ring last drew at rest, from its own write receipts.
pub const DRAWN: &str = "light/drawn";
/// A state older than this, by the wall clock, is nobody's: its owner has
/// stopped saying it.
const STATE_LASTS_MS: u64 = 3_000;

impl BusLight {
    pub fn new(bus: lamp_bus::Bus) -> std::io::Result<Self> {
        Self::graced(bus, LIGHT_GRACE)
    }

    /// The same, the light still offered `grace` after its state was last
    /// fresh.
    pub fn graced(bus: lamp_bus::Bus, grace: Duration) -> std::io::Result<Self> {
        let wanted = bus.publisher_declared(WANTED, &Wanted::declaration(), "actions")?;
        Ok(Self {
            bus,
            wanted,
            started: Instant::now(),
            seen: None,
            grace,
        })
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Light for BusLight {
    /// A light whose state is fresh, or was within [`LIGHT_GRACE`].
    fn there(&mut self) -> bool {
        self.shown().is_some() || self.seen.is_some_and(|at| at.elapsed() < self.grace)
    }
    fn shown(&mut self) -> Option<Wanted> {
        let state = self.bus.latest(STATE).ok()??;
        let lasts = state.header.expires_ms.unwrap_or(STATE_LASTS_MS);
        if unix_ms() > state.header.at_unix_ms.saturating_add(lasts) {
            return None;
        }
        // A state that is not a light there is, is no light to change.
        let shown = state
            .parse::<Wanted>()
            .ok()
            .filter(|light| light.check().is_ok());
        if shown.is_some() {
            self.seen = Some(Instant::now());
        }
        shown
    }
    fn ask(&mut self, light: &Wanted) -> bool {
        let at_us = self.started.elapsed().as_micros() as u64;
        self.wanted.publish_json(at_us, light).is_ok()
    }
    /// The newest light the ring drew, whatever its age: during a turn the
    /// ring draws the turn's faces and says nothing of its light, so what
    /// it last drew at rest stands. At worst a stale one asks once more for
    /// the light that is already the person's, which changes nothing.
    fn covered(&mut self) -> bool {
        let drawn = self
            .bus
            .latest(DRAWN)
            .ok()
            .flatten()
            .and_then(|message| message.parse::<Wanted>().ok());
        match (drawn, self.shown()) {
            (Some(drawn), Some(shown)) => drawn != shown,
            _ => false,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn light(on: bool, colour: &str, level: u8) -> Wanted {
        Wanted {
            on,
            colour: colour.into(),
            level,
        }
    }

    /// A ring owner that shows what it is asked, or does not.
    pub(crate) struct Fake {
        pub shown: Option<Wanted>,
        pub obeys: bool,
        pub asked: Vec<Wanted>,
        /// A glow or a test drawn over the person's light.
        pub covered: bool,
    }
    impl Fake {
        pub(crate) fn showing(shown: Wanted) -> Self {
            Self {
                shown: Some(shown),
                obeys: true,
                asked: Vec::new(),
                covered: false,
            }
        }
    }
    impl Light for Fake {
        fn there(&mut self) -> bool {
            self.shown.is_some()
        }
        fn shown(&mut self) -> Option<Wanted> {
            self.shown.clone()
        }
        fn ask(&mut self, wanted: &Wanted) -> bool {
            self.asked.push(wanted.clone());
            if self.obeys {
                self.shown = Some(wanted.clone());
            }
            true
        }
        fn covered(&mut self) -> bool {
            self.covered
        }
    }

    /// A ring owner that shows what it is asked only from its second read
    /// after the ask, as the ring does when its pass comes after the read.
    struct Late {
        shown: Wanted,
        asked: Option<Wanted>,
        reads: u32,
    }
    impl Light for Late {
        fn there(&mut self) -> bool {
            true
        }
        fn shown(&mut self) -> Option<Wanted> {
            self.reads += 1;
            if self.reads >= 2
                && let Some(asked) = self.asked.take()
            {
                self.shown = asked;
            }
            Some(self.shown.clone())
        }
        fn ask(&mut self, wanted: &Wanted) -> bool {
            self.asked = Some(wanted.clone());
            self.reads = 0;
            true
        }
    }

    /// A light the owner shows a moment after the ask is confirmed at the
    /// next read, `CONFIRM_LOOKS` (2 ms) on, not 15 ms on: the voice's
    /// answer waits for this read (#1806).
    #[test]
    fn a_light_shown_a_moment_after_the_ask_is_confirmed_at_the_next_read() {
        let mut ring = Late {
            shown: light(true, "blue", 3),
            asked: None,
            reads: 0,
        };
        let fastest = (0..5)
            .map(|call| {
                let colour = if call % 2 == 0 { "red" } else { "blue" };
                let started = Instant::now();
                let outcome = set(&mut ring, light(true, colour, 3), String::new());
                assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
                started.elapsed()
            })
            .min()
            .unwrap();
        assert!(fastest < Duration::from_millis(10), "{fastest:?}");
    }

    /// The reviewer on #993: with a skill's glow drawn over the person's
    /// light, asking for the light that is theirs is still asked, so the
    /// ring hears the person and the glow ends, and nothing is said to be
    /// "already" so.
    #[test]
    fn a_glow_over_the_light_is_ended_by_asking_for_the_persons_own_light() {
        let covered = |shown: Wanted| Fake {
            covered: true,
            ..Fake::showing(shown)
        };
        let mut lamp = covered(light(true, "blue", 3));
        assert_eq!(said(colour(&mut lamp, "blue")), "My light is blue now.");
        assert_eq!(said(on(&mut lamp)), "My light is on, blue.");
        assert_eq!(lamp.asked, [light(true, "blue", 3), light(true, "blue", 3)]);
        let mut dark = covered(light(false, "warm", 1));
        assert_eq!(
            said(off(&mut dark)),
            "My light is off. The faint glow that is left shows I am still listening."
        );
        assert_eq!(dark.asked, [light(false, "warm", 1)]);
        assert_eq!(
            said(dimmer(&mut dark)),
            "My light is off. The faint glow that is left shows I am still listening."
        );
        let mut top = covered(light(true, "pink", 5));
        assert_eq!(said(brighter(&mut top)), "My light is at its brightest.");
        let mut bottom = covered(light(true, "pink", 1));
        assert_eq!(said(dimmer(&mut bottom)), "My light is at its dimmest.");
        assert_eq!((top.asked.len(), bottom.asked.len()), (1, 1));
        // At either end of the warmth too (the reviewer on #1316).
        let mut warmest = covered(light(true, "orange", 3));
        assert_eq!(
            said(warmer(&mut warmest)),
            "My light is on, as warm as it goes, orange."
        );
        let mut coolest = covered(light(true, "white", 3));
        assert_eq!(
            said(cooler(&mut coolest)),
            "My light is on, as cool as it goes, white."
        );
        assert_eq!((warmest.asked.len(), coolest.asked.len()), (1, 1));
    }
    fn said(outcome: Outcome) -> String {
        match outcome {
            Outcome::Done(said) | Outcome::Already(said) => said,
            Outcome::Failed(said) => panic!("expected it done, was told: {said}"),
        }
    }
    /// Said, and nothing changed: an `Already`, never answered short (#1520).
    fn already(outcome: Outcome) -> String {
        match outcome {
            Outcome::Already(said) => said,
            other => panic!("expected it already so, was told: {other:?}"),
        }
    }

    /// The first builder, 17 Oct 14:16: "Can you make your light a little
    /// warmer?" got "my light is a fixed color". Warmer and cooler step
    /// along white, warm and orange at the light's level, stop at the ends,
    /// bring another colour to warm or white, and switch a light on.
    #[test]
    fn warmer_and_cooler_step_along_white_warm_and_orange() {
        let mut lamp = Fake::showing(light(true, "white", 3));
        assert_eq!(
            said(warmer(&mut lamp)),
            "My light is a step warmer, from white to warm white."
        );
        assert_eq!(
            said(warmer(&mut lamp)),
            "My light is a step warmer, from warm white to orange."
        );
        assert_eq!(
            already(warmer(&mut lamp)),
            "My light is already as warm as it goes, orange."
        );
        // Cooler from orange is said with its way first and warm as a warm
        // white, never as "warm now" (#1367's scene: "warmed it up a step").
        assert_eq!(
            said(cooler(&mut lamp)),
            "My light is a step cooler, from orange to warm white."
        );
        assert_eq!(
            said(cooler(&mut lamp)),
            "My light is a step cooler, from warm white to white."
        );
        assert_eq!(
            already(cooler(&mut lamp)),
            "My light is already as cool as it goes, white."
        );
        assert_eq!(
            lamp.asked,
            [
                light(true, "warm", 3),
                light(true, "orange", 3),
                light(true, "warm", 3),
                light(true, "white", 3),
            ]
        );
        // Off the scale, no step: only what it is now.
        let mut blue = Fake::showing(light(true, "blue", 2));
        assert_eq!(said(warmer(&mut blue)), "My light is warm white now.");
        let mut green = Fake::showing(light(true, "green", 4));
        assert_eq!(said(cooler(&mut green)), "My light is white now.");
        let mut dark = Fake::showing(light(false, "white", 1));
        assert_eq!(
            said(warmer(&mut dark)),
            "My light is a step warmer, from white to warm white."
        );
        assert_eq!(dark.asked, [light(true, "warm", 1)]);
        let mut dark_end = Fake::showing(light(false, "orange", 2));
        assert_eq!(
            said(warmer(&mut dark_end)),
            "My light is on, as warm as it goes, orange."
        );
    }

    #[test]
    fn on_and_off_keep_the_colour_and_the_level_and_neither_is_asked_twice() {
        let mut lamp = Fake::showing(light(false, "blue", 3));
        assert_eq!(said(on(&mut lamp)), "My light is on, blue.");
        assert_eq!(lamp.shown, Some(light(true, "blue", 3)));
        assert_eq!(already(on(&mut lamp)), "My light is already on.");
        // Nothing changed: an `Already`, never answered short (#1520).
        assert!(matches!(on(&mut lamp), Outcome::Already(_)));
        assert_eq!(
            said(off(&mut lamp)),
            "My light is off. The faint glow that is left shows I am still listening."
        );
        assert_eq!(lamp.shown, Some(light(false, "blue", 3)));
        assert_eq!(already(off(&mut lamp)), "My light is already off.");
        assert_eq!(
            lamp.asked,
            [light(true, "blue", 3), light(false, "blue", 3)]
        );
    }

    #[test]
    fn a_colour_is_set_by_name_said_by_name_and_switches_the_light_on() {
        let mut lamp = Fake::showing(light(true, "warm", 2));
        assert_eq!(said(colour(&mut lamp, "blue")), "My light is blue now.");
        assert_eq!(lamp.shown, Some(light(true, "blue", 2)));
        assert_eq!(
            already(colour(&mut lamp, "blue")),
            "My light is already blue."
        );
        assert_eq!(lamp.asked.len(), 1);
        // Off and already that colour: asked for a colour, it comes on.
        let mut dark = Fake::showing(light(false, "blue", 4));
        assert_eq!(said(colour(&mut dark, "blue")), "My light is blue now.");
        assert_eq!(dark.shown, Some(light(true, "blue", 4)));
        // Warm is said as a warm white in every answer, as after a step (El
        // on #1476): one colour, one name.
        let mut warm = Fake::showing(light(false, "blue", 2));
        assert_eq!(
            said(colour(&mut warm, "warm")),
            "My light is warm white now."
        );
        assert_eq!(warm.shown, Some(light(true, "warm", 2)));
        assert_eq!(
            said(colour(&mut warm, "warm")),
            "My light is already warm white."
        );
        let mut off_warm = Fake::showing(light(false, "warm", 2));
        assert_eq!(said(on(&mut off_warm)), "My light is on, warm white.");
        // A word that is not one of the ten is refused before anything is
        // read or asked.
        let mut lamp = Fake::showing(light(true, "warm", 2));
        for odd in ["beige", "", "Blue", "#0000ff"] {
            assert_eq!(
                colour(&mut lamp, odd),
                Outcome::Failed("That is not a colour my light can show.".into())
            );
        }
        assert!(lamp.asked.is_empty());
    }

    #[test]
    fn brighter_and_dimmer_are_one_level_keep_the_colour_and_stop_at_the_ends() {
        let mut lamp = Fake::showing(light(true, "pink", 3));
        assert_eq!(
            said(brighter(&mut lamp)),
            "My light is a step brighter, at level 4 of 5."
        );
        assert_eq!(
            said(brighter(&mut lamp)),
            "My light is a step brighter, at level 5 of 5."
        );
        assert_eq!(lamp.shown, Some(light(true, "pink", 5)));
        assert_eq!(
            already(brighter(&mut lamp)),
            "My light is already at its brightest."
        );
        for level in [4, 3, 2, 1] {
            assert_eq!(
                said(dimmer(&mut lamp)),
                format!("My light is a step dimmer, at level {level} of 5.")
            );
        }
        // The lowest level is not off, and dimmer does not switch it off.
        assert_eq!(
            already(dimmer(&mut lamp)),
            "My light is already at its dimmest. I can switch it off if you like."
        );
        assert_eq!(lamp.shown, Some(light(true, "pink", 1)));
        assert_eq!(lamp.asked.len(), 6);
        // Off: brighter brings it on as it was, and dimmer has nothing to do.
        let mut dark = Fake::showing(light(false, "green", 2));
        assert_eq!(already(dimmer(&mut dark)), "My light is already off.");
        assert_eq!(
            said(brighter(&mut dark)),
            "My light is on, at level 2 of 5."
        );
        assert_eq!(dark.shown, Some(light(true, "green", 2)));
    }

    /// A ring owner that is not saying what it shows, and one that takes a
    /// request and does not show it: each is said as what it is, the second
    /// after the wait and no longer.
    #[test]
    fn a_light_that_cannot_be_read_or_does_not_change_is_said_so() {
        let failed = |outcome: Outcome| match outcome {
            Outcome::Failed(said) => said,
            Outcome::Done(said) | Outcome::Already(said) => {
                panic!("expected a failure, was told: {said}")
            }
        };
        let mut gone = Fake {
            shown: None,
            obeys: true,
            asked: Vec::new(),
            covered: false,
        };
        assert_eq!(failed(on(&mut gone)), UNREAD);
        assert_eq!(failed(off(&mut gone)), UNREAD);
        assert_eq!(failed(colour(&mut gone, "red")), UNREAD);
        assert_eq!(failed(brighter(&mut gone)), UNREAD);
        assert_eq!(failed(dimmer(&mut gone)), UNREAD);
        assert!(gone.asked.is_empty());
        let mut deaf = Fake {
            shown: Some(light(false, "warm", 1)),
            obeys: false,
            asked: Vec::new(),
            covered: false,
        };
        let started = Instant::now();
        assert_eq!(failed(on(&mut deaf)), UNCONFIRMED);
        let waited = started.elapsed();
        assert!(waited >= CONFIRM_WAIT && waited < CONFIRM_WAIT + Duration::from_millis(300));
        assert_eq!(deaf.asked, [light(true, "warm", 1)]);
    }

    /// Through a real bus in a directory of its own: the request is kept as
    /// the topic's newest value, the state is read back from the owner's
    /// topic, and a state its owner has stopped saying is nobody's.
    #[test]
    fn through_the_bus_a_request_is_kept_and_a_stale_state_is_no_light() {
        let root = std::env::temp_dir().join(format!(
            "lamp-actions-light-{}-{:x}",
            std::process::id(),
            unix_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut lamp =
            BusLight::graced(lamp_bus::Bus::new(&root).unwrap(), Duration::ZERO).unwrap();
        // Nobody says what the ring shows: there is no light to ask for.
        assert!(!lamp.there());
        let mut ring = bus
            .publisher_declared(STATE, &Wanted::declaration(), "ring")
            .unwrap();
        ring.publish_json(1, &light(true, "warm", 1)).unwrap();
        assert!(lamp.there());
        assert_eq!(lamp.shown(), Some(light(true, "warm", 1)));
        assert!(lamp.ask(&light(true, "blue", 1)));
        let wanted: Wanted = bus.latest(WANTED).unwrap().unwrap().parse().unwrap();
        assert_eq!(wanted, light(true, "blue", 1));
        assert_eq!(
            bus.declaration(WANTED).unwrap(),
            Some(Wanted::declaration())
        );
        // A state that is not a light there is, is no light.
        ring.publish_json(2, &light(true, "blue", 9)).unwrap();
        assert_eq!(lamp.shown(), None);
        // The owner said so with a life of half a second, and then stopped.
        // The life is read on the wall clock from the publication on: long
        // enough that a runner held up between the publication and the
        // read still reads it in its life (#1121; 20 ms was not).
        ring.expires_after(Some(Duration::from_millis(500)));
        ring.publish_json(3, &light(true, "blue", 1)).unwrap();
        assert_eq!(lamp.shown(), Some(light(true, "blue", 1)));
        std::thread::sleep(Duration::from_millis(540));
        assert_eq!(lamp.shown(), None);
        assert!(!lamp.there());
        // What the ring drew, against the person's light: a glow over it is
        // covered, the same light is not, and none drawn is not.
        ring.expires_after(None);
        ring.publish_json(4, &light(true, "blue", 1)).unwrap();
        assert!(!lamp.covered(), "nothing drawn yet");
        let mut drew = bus
            .publisher_declared(DRAWN, &Wanted::declaration(), "ring")
            .unwrap();
        drew.publish_json(1, &light(true, "purple", 4)).unwrap();
        assert!(lamp.covered());
        drew.publish_json(2, &light(true, "blue", 1)).unwrap();
        assert!(!lamp.covered());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The first builder's lamp run on #1495: a voice restart stopped
    /// `light/state` for about 2 s, and the light's functions dropped out.
    /// A light whose state was fresh is still there for its grace, and gone
    /// after it; one never seen is not there at all.
    #[test]
    fn a_light_stays_there_through_its_grace_and_goes_after_it() {
        let root = std::env::temp_dir().join(format!(
            "lamp-actions-grace-{}-{:x}",
            std::process::id(),
            unix_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        // Seconds apart from the 300 ms steps, so a stalled thread cannot
        // carry a "within" look past it (Ten on #1514); the "past" look
        // needs only to come after.
        let grace = Duration::from_secs(2);
        let mut lamp = BusLight::graced(lamp_bus::Bus::new(&root).unwrap(), grace).unwrap();
        assert!(!lamp.there(), "never seen");
        let mut ring = bus
            .publisher_declared(STATE, &Wanted::declaration(), "ring")
            .unwrap();
        ring.expires_after(Some(Duration::from_millis(200)));
        ring.publish_json(1, &light(true, "warm", 2)).unwrap();
        assert!(lamp.there());
        // The ring stopped: its state goes stale, and the light stays.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(lamp.shown(), None);
        assert!(lamp.there(), "within its grace");
        // A call within the grace, the state stale, says it can't tell and
        // asks nothing (the reviewer on #1514).
        assert_eq!(
            on(&mut lamp),
            Outcome::Failed("I could not tell what my light is showing.".into())
        );
        assert!(bus.latest(WANTED).unwrap().is_none(), "nothing asked");
        // Back within the grace: fresh again, and the grace starts over.
        ring.publish_json(2, &light(true, "warm", 2)).unwrap();
        assert!(lamp.there());
        std::thread::sleep(Duration::from_millis(300));
        assert!(lamp.there());
        // Gone for longer than the grace: not there.
        std::thread::sleep(grace);
        assert!(!lamp.there(), "past its grace");
        assert_eq!(LIGHT_GRACE, Duration::from_secs(15));
        std::fs::remove_dir_all(root).unwrap();
    }
}
