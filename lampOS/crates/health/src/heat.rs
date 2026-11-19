//! How hot the board is for the work that can wait (`board/heat`, #7879):
//! ok, warm or hot, from the hotter processor zone, with hysteresis.
//!
//! The lamp's processors slow themselves down at their zone's second
//! passive trip point, `T`: 90 °C on the lamps, where e82d ran under full
//! speed in 8 of 20 samples at 88 °C on average. Parts whose work nobody
//! waits for can ease off before that, and come back to full speed once the
//! board has cooled. The lines are:
//!
//! - warm once the zone has been at `T`−6 or above for 30 s;
//! - hot once it has been at `T`−2 or above for 10 s, or at once when the
//!   board holds its processors back to cool ([`Reading::throttled`]);
//! - hot back to warm once it has been under `T`−5 for 60 s, not held back;
//! - warm, or hot, back to ok once it has been under `T`−10 for 60 s, not
//!   held back.
//!
//! Each line holds for its time without a break: one reading off the line
//! starts its time again. Between the lines the level stays as it was, so
//! a zone near one does not flip the level at each reading, and the level
//! changes at most once in 10 to 60 s.
//!
//! Everything here is arithmetic on readings and the times they were
//! taken, so a test can walk a zone through each line from both sides.
use crate::board::{ERROR_WITHOUT_TRIPS_C, Reading, Zone};
use lamp_msgs::{BoardHeat, HeatLevel};
use std::{
    fmt,
    time::{Duration, Instant},
};

/// `T` for a zone that names no second passive trip point: where the
/// lamps' processor zones have theirs.
pub const TRIP_WITHOUT_SECOND_C: i64 = ERROR_WITHOUT_TRIPS_C;
/// Warm from this far below `T`, after [`WARM_AFTER`].
pub const WARM_BELOW_TRIP_C: i64 = 6;
pub const WARM_AFTER: Duration = Duration::from_secs(30);
/// Hot from this far below `T`, after [`HOT_AFTER`].
pub const HOT_BELOW_TRIP_C: i64 = 2;
pub const HOT_AFTER: Duration = Duration::from_secs(10);
/// Hot ends under this far below `T`, after [`ENDS_AFTER`].
pub const HOT_ENDS_BELOW_TRIP_C: i64 = 5;
/// Warm ends under this far below `T`, after [`ENDS_AFTER`].
pub const WARM_ENDS_BELOW_TRIP_C: i64 = 10;
pub const ENDS_AFTER: Duration = Duration::from_secs(60);

/// `T` for `zone`, in thousandths of a degree: its second passive trip
/// point, or [`TRIP_WITHOUT_SECOND_C`].
pub fn trip_milli_c(zone: &Zone) -> i64 {
    zone.passive_milli_c
        .get(1)
        .copied()
        .unwrap_or(TRIP_WITHOUT_SECOND_C * 1_000)
}

/// The level became another: what the journal is told, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub from: HeatLevel,
    pub to: HeatLevel,
    /// The zone read, with its temperature and `T`, in thousandths of a
    /// degree.
    pub zone: String,
    pub milli_c: i64,
    pub trip_milli_c: i64,
    /// How long the line that made the change had held; nothing when the
    /// board holding its processors back made it hot.
    pub held: Option<Duration>,
    /// The busiest processes since the last reading, as the board's heat
    /// finding names them (`lamp-eyes 44%, lamp-voice 20%`); empty when
    /// not known.
    pub busiest: String,
}

/// Whole degrees, for a person.
fn degrees(milli_c: i64) -> String {
    format!("{:.0}", milli_c as f64 / 1_000.0)
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (zone, at) = (&self.zone, degrees(self.milli_c));
        let held = self.held.map(|held| held.as_secs());
        if self.to == HeatLevel::Ok {
            write!(f, "the board is cool again: {zone} {at} °C")?;
            if let Some(held) = held {
                write!(f, " for {held} s")?;
            }
            return f.write_str("; background work back at full pace");
        }
        write!(f, "the board is {}: {zone} {at} °C", self.to.as_str())?;
        match held {
            Some(held) => write!(f, " for {held} s")?,
            None => f.write_str(" and held back to cool")?,
        }
        write!(
            f,
            " (it slows itself at {}); background work eases off",
            degrees(self.trip_milli_c)
        )?;
        if !self.busiest.is_empty() {
            write!(f, "; busiest: {}", self.busiest)?;
        }
        Ok(())
    }
}

/// Since when a line has held without a break, if it holds now.
fn holding(since: &mut Option<Instant>, holds: bool, now: Instant) -> Option<Duration> {
    if holds {
        Some(now.saturating_duration_since(*since.get_or_insert(now)))
    } else {
        *since = None;
        None
    }
}

/// The level and since when each of its lines has held. The lamp's loop
/// keeps one ([`crate::serve`]).
#[derive(Clone, Debug)]
pub struct Heat {
    level: HeatLevel,
    warm_since: Option<Instant>,
    hot_since: Option<Instant>,
    hot_ends_since: Option<Instant>,
    warm_ends_since: Option<Instant>,
}

impl Default for Heat {
    fn default() -> Self {
        Self {
            level: HeatLevel::Ok,
            warm_since: None,
            hot_since: None,
            hot_ends_since: None,
            warm_ends_since: None,
        }
    }
}

impl Heat {
    pub fn level(&self) -> HeatLevel {
        self.level
    }

    /// Take one reading of `zone` at `now`, `throttled` if the board holds
    /// its processors back to cool. The change, if the level became
    /// another; `busiest` is left empty.
    pub fn observe(&mut self, zone: &Zone, throttled: bool, now: Instant) -> Option<Change> {
        let trip = trip_milli_c(zone);
        let at = zone.milli_c;
        let warm = holding(
            &mut self.warm_since,
            at >= trip - WARM_BELOW_TRIP_C * 1_000,
            now,
        );
        let hot = holding(
            &mut self.hot_since,
            at >= trip - HOT_BELOW_TRIP_C * 1_000,
            now,
        );
        let hot_ends = holding(
            &mut self.hot_ends_since,
            !throttled && at < trip - HOT_ENDS_BELOW_TRIP_C * 1_000,
            now,
        );
        let warm_ends = holding(
            &mut self.warm_ends_since,
            !throttled && at < trip - WARM_ENDS_BELOW_TRIP_C * 1_000,
            now,
        );
        let after = |held: Option<Duration>, time: Duration| held.filter(|held| *held >= time);
        let (to, held) = match self.level {
            HeatLevel::Ok | HeatLevel::Warm if throttled => (HeatLevel::Hot, None),
            HeatLevel::Ok | HeatLevel::Warm if after(hot, HOT_AFTER).is_some() => {
                (HeatLevel::Hot, hot)
            }
            // Cool enough to end warm ends hot as well: the board need not
            // say it is warm on its way down.
            HeatLevel::Warm | HeatLevel::Hot if after(warm_ends, ENDS_AFTER).is_some() => {
                (HeatLevel::Ok, warm_ends)
            }
            HeatLevel::Hot if after(hot_ends, ENDS_AFTER).is_some() => (HeatLevel::Warm, hot_ends),
            HeatLevel::Ok if after(warm, WARM_AFTER).is_some() => (HeatLevel::Warm, warm),
            level => (level, None),
        };
        if to == self.level {
            return None;
        }
        let from = std::mem::replace(&mut self.level, to);
        Some(Change {
            from,
            to,
            zone: zone.name.clone(),
            milli_c: at,
            trip_milli_c: trip,
            held,
            busiest: String::new(),
        })
    }

    /// Take a reading of the board at `now`: its heat as `board/heat`
    /// carries it, and the change if the level became another, naming the
    /// busiest processes. Nothing when the board has no processor zone.
    pub fn read(&mut self, reading: &Reading, now: Instant) -> Option<(BoardHeat, Option<Change>)> {
        let zone = reading.hottest_cpu()?;
        let change = self
            .observe(zone, reading.throttled().is_some(), now)
            .map(|change| Change {
                busiest: reading
                    .busiest
                    .iter()
                    .map(|(name, share)| format!("{name} {:.0}%", share * 100.0))
                    .collect::<Vec<_>>()
                    .join(", "),
                ..change
            });
        let heat = BoardHeat {
            level: self.level,
            zone: zone.name.clone(),
            c: zone.celsius(),
            trip_c: trip_milli_c(zone) as f32 / 1_000.0,
        };
        Some((heat, change))
    }
}
