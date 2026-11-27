//! Skills follow the person's quiet hours (#8866), the one window Actions
//! keeps and says on `quiet/window` (`lamp_msgs::QuietWindow`), set from
//! the app with `quiet.set`.
//!
//! A manifest names the window instead of fixed hours in two places:
//!
//! - `[limits] during = "quiet"`: the skill's output ceilings hold while
//!   the window is on and the local time is inside it, and not at all
//!   while it is off ([`night`], `crate::limits`).
//! - A trigger's `during = { after = "quiet_start", for = "2h" }` or
//!   `{ after = "quiet_end", for = "5h" }`: the trigger may hold from the
//!   window's start, or its end, for that long, whether the window is on
//!   or off ([`Follows`]). Off lifts the ceilings only; the evening glow
//!   and the morning hum keep their hours.
//!
//! The old `{ from = "HH:MM", to = "HH:MM" }` form is unchanged and never
//! follows the window. With no window heard yet, the window is the lamp's
//! old fixed night ([`QuietWindow::default`], on, 21:00 to 06:00), so a
//! lamp nobody set behaves as before; a window heard is kept while Actions
//! is down, since it is the person's setting, not a reading.
use crate::{Skill, daily::Window};
use lamp_bus::Bus;
use lamp_msgs::{MINUTES_A_DAY, QuietWindow};
use std::time::{Duration, Instant};

/// How often the kept `quiet/window` is read: as often as Actions says it.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);

/// `[limits] during`'s word for the window.
pub const LIMITS_WORD: &str = "quiet";

/// Which end of the window a trigger's hours begin at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// `after = "quiet_start"`.
    Start,
    /// `after = "quiet_end"`.
    End,
}

/// A trigger's hours from one end of the window, `minutes` long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follows {
    pub after: Anchor,
    pub minutes: u16,
}

impl Follows {
    /// `after` is `quiet_start` or `quiet_end`, and `lasting` a whole
    /// number of minutes or hours, from a minute to less than a day
    /// (`30m`, `2h`), so the hours it makes always have a start and an end
    /// that differ.
    pub fn parse(after: &str, lasting: &str) -> Result<Self, String> {
        let after = match after {
            "quiet_start" => Anchor::Start,
            "quiet_end" => Anchor::End,
            other => {
                return Err(format!(
                    "after {other:?}: quiet_start or quiet_end, the quiet hours' start or end"
                ));
            }
        };
        let us = crate::duration_us(lasting)?;
        let minutes = (us % 60_000_000 == 0)
            .then_some(us / 60_000_000)
            .filter(|minutes| (1..u64::from(MINUTES_A_DAY)).contains(minutes))
            .ok_or_else(|| format!("for {lasting:?}: whole minutes, from 1m to less than a day"))?;
        Ok(Self {
            after,
            minutes: minutes as u16,
        })
    }

    /// The hours these make with `quiet`, on or off.
    pub fn window(&self, quiet: &QuietWindow) -> Window {
        let from = match self.after {
            Anchor::Start => quiet.start,
            Anchor::End => quiet.end,
        } % MINUTES_A_DAY;
        Window::of_minutes(from, (from + self.minutes) % MINUTES_A_DAY)
            .expect("a checked length below a day makes a window")
    }
}

/// The night ceilings' hours: the window while it is on, none while off.
pub fn night(quiet: &QuietWindow) -> Option<Window> {
    if !quiet.on || quiet.check().is_err() {
        return None;
    }
    Window::of_minutes(quiet.start, quiet.end)
}

/// Each trigger that follows the window takes the hours it makes now. One
/// with fixed hours is left as it is.
pub fn follow(skills: &mut [Skill], quiet: &QuietWindow) {
    for trigger in skills
        .iter_mut()
        .flat_map(|skill| skill.triggers.iter_mut())
    {
        if let Some(follows) = trigger.follows_quiet {
            trigger.during = Some(follows.window(quiet));
        }
    }
}

/// Skills' end of `quiet/window`: the newest window Actions said, read at
/// most once a second, kept when nothing new can be read.
pub struct Heard {
    bus: Bus,
    looked: Option<Instant>,
    window: QuietWindow,
}

impl Heard {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            window: QuietWindow::default(),
        }
    }

    /// The window now. Never waits longer than a file read.
    pub fn now(&mut self) -> QuietWindow {
        let now = Instant::now();
        if self
            .looked
            .is_none_or(|at| now.duration_since(at) >= LOOK_EVERY)
        {
            self.looked = Some(now);
            if let Some(window) = self
                .bus
                .latest(QuietWindow::TOPIC)
                .ok()
                .flatten()
                .as_ref()
                .and_then(QuietWindow::from_message)
            {
                self.window = window;
            }
        }
        self.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATE: QuietWindow = QuietWindow {
        on: true,
        start: 22 * 60,
        end: 7 * 60,
    };

    #[test]
    fn hours_follow_either_end_and_may_cross_midnight() {
        let evening = Follows::parse("quiet_start", "2h").unwrap();
        let morning = Follows::parse("quiet_end", "5h").unwrap();
        let old = QuietWindow::default();
        assert_eq!(
            evening.window(&old),
            Window::parse("21:00", "23:00").unwrap()
        );
        assert_eq!(
            morning.window(&old),
            Window::parse("06:00", "11:00").unwrap()
        );
        assert_eq!(
            evening.window(&LATE),
            Window::parse("22:00", "00:00").unwrap()
        );
        assert_eq!(
            morning.window(&LATE),
            Window::parse("07:00", "12:00").unwrap()
        );
        // Off moves nothing: the glow and the hum keep their hours.
        let off = QuietWindow { on: false, ..LATE };
        assert_eq!(evening.window(&off), evening.window(&LATE));
        let late_start = QuietWindow {
            on: true,
            start: 23 * 60 + 30,
            end: 6 * 60,
        };
        assert_eq!(
            evening.window(&late_start),
            Window::parse("23:30", "01:30").unwrap()
        );
        assert_eq!(
            Follows::parse("quiet_end", "90m").unwrap().minutes,
            90,
            "minutes too"
        );
    }

    #[test]
    fn a_follow_names_an_end_and_a_length_under_a_day() {
        for (after, lasting) in [
            ("quiet", "2h"),
            ("start", "2h"),
            ("quiet_start", "24h"),
            ("quiet_start", "1440m"),
            ("quiet_start", "0m"),
            ("quiet_start", "90s"),
            ("quiet_start", "two hours"),
        ] {
            assert!(Follows::parse(after, lasting).is_err(), "{after} {lasting}");
        }
        assert!(Follows::parse("quiet_start", "1439m").is_ok());
        assert!(Follows::parse("quiet_end", "1m").is_ok());
    }

    #[test]
    fn the_night_is_the_window_only_while_it_is_on() {
        assert_eq!(night(&LATE), Some(Window::parse("22:00", "07:00").unwrap()));
        assert_eq!(night(&QuietWindow { on: false, ..LATE }), None);
        assert_eq!(
            night(&QuietWindow::default()),
            Some(Window::parse("21:00", "06:00").unwrap())
        );
    }

    #[test]
    fn the_window_heard_is_kept_and_the_default_comes_first() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut heard = Heard::on(bus.clone());
        assert_eq!(heard.now(), QuietWindow::default());
        let mut actions = bus
            .publisher_declared(
                QuietWindow::TOPIC,
                &QuietWindow::declaration(),
                QuietWindow::SOURCE,
            )
            .unwrap();
        actions.publish_json(1, &LATE).unwrap();
        // Read again only once a second has passed.
        assert_eq!(heard.now(), QuietWindow::default());
        heard.looked = None;
        assert_eq!(heard.now(), LATE);
        // Something that is not a window keeps the last one heard.
        actions
            .publish_json(2, &serde_json::json!({"on": true}))
            .unwrap();
        heard.looked = None;
        assert_eq!(heard.now(), LATE);
        std::fs::remove_dir_all(root.path().join(QuietWindow::TOPIC)).unwrap();
        heard.looked = None;
        assert_eq!(heard.now(), LATE, "Actions gone: the person's window stays");
    }
}
