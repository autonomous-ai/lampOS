//! The level under which sound is not speech, following the room.
//!
//! A fixed floor is right for one room at one hour. On lamp-4ace on
//! 2026-10-13 the talk of a working office reached the lamp at a median of
//! -27 to -35 dBFS and people speaking to it at its desk at -15 to -25: at
//! -40 the lamp heard, lit up for and sometimes answered the whole office,
//! and at -28, set by hand, it stopped. But -28 in an empty room in the
//! evening turns away a quiet voice for nothing.
//!
//! So the floor moves between a lowest and a highest level. It follows how
//! loud the speech is that was not for the lamp: what the detector scores as
//! speech outside any turn (too quiet to begin one, or under the floor
//! already), and what was heard in turns that ended by themselves with the
//! lamp saying nothing. Speech in turns the lamp answered is never counted,
//! nor is anything heard while the lamp itself is speaking, nor a turn that
//! was replaced by more talk (that may be one person pausing). The floor
//! sits three decibels above the middle of that, so that most of such talk
//! falls under it, and goes back to the lowest level when there has been
//! too little of it lately to say.
//!
//! Levels are on the face's scale (`ring_face::level_of`): 0 is -48 dBFS or
//! under, 255 is -14 dBFS or over, 7.5 to a decibel.

/// Where the coordinator reads the highest level the floor may rise to, in
/// dBFS (-47 to -20). The lowest is `--faint-floor-db`. Unset, or not above
/// it: the floor stays where that option puts it.
pub const ENVIRONMENT: &str = "LAMP_FAINT_FLOOR_MAX_DB";

/// The floor for a service given `--faint-floor-db` as `lowest` on the
/// face's scale, with the environment's highest level if there is one.
pub fn from_environment(lowest: u8, now_us: u64) -> RoomFloor {
    let highest = std::env::var(ENVIRONMENT)
        .ok()
        .and_then(|value| value.parse::<i16>().ok())
        .filter(|db| (-47..=-20).contains(db))
        .map_or(lowest, |db| ((db + 48) * 255 / 34) as u8);
    RoomFloor::new(lowest, highest, now_us)
}

/// Where the coordinator reads how long a block under the floor still
/// counts as someone at the lamp after a block over it, in milliseconds
/// (0 to 1,000). Unset or 0: each block is judged alone, as it always was.
pub const HOLD_ENVIRONMENT: &str = "LAMP_FAINT_HOLD_MS";

/// Whether a block is loud enough to be someone at the lamp, judged over a
/// moment and not one block at a time. Speech at a desk trails off: on
/// lamp-4ace on 2026-10-13 one "Good night, Lamp." had 50 blocks the
/// detector scored as speech, began at about -20 dBFS and ended near -30,
/// and 28 of the 50 were turned away for being 1 to 4 dB under a floor of
/// -28. The 22 left were fewer than an utterance needs, and the whole of it
/// was thrown out as a sound.
///
/// So a block under the floor is still heard for a while after a run of
/// blocks over it. The run has to be [`EARN_BLOCKS`] long, as many as begin
/// an utterance: a voice across the room, which the floor is there for, has
/// a block over it now and then and no such run, and gets nothing by this.
///
/// Asking that each block of the run be 3 dB clear of the floor was tried
/// for an hour on lamp-4ace and taken out. It had looked free: that first
/// recorded phrase counted 49 blocks either way. The next "Good night,
/// Lamp." it met began with four blocks 3 dB over, one just over, two more
/// 3 dB over and one just over: eight in a row over the floor, never six in
/// a row 3 dB over it, and it was thrown out again with 9 and 12 blocks
/// counted. One recording was not enough to choose a threshold by.
#[derive(Debug, Default)]
pub struct Near {
    hold_us: u64,
    /// Blocks over the floor in a row, up to [`EARN_BLOCKS`].
    run: u8,
    /// Until when a block under the floor is heard.
    held_until_us: Option<u64>,
}

/// Blocks over the floor in a row that earn the hold.
pub const EARN_BLOCKS: u8 = 6;

impl Near {
    pub fn new(hold_ms: u64) -> Self {
        Self {
            hold_us: hold_ms.min(1_000) * 1_000,
            run: 0,
            held_until_us: None,
        }
    }

    /// For what the environment says.
    pub fn from_environment() -> Self {
        Self::new(
            std::env::var(HOLD_ENVIRONMENT)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
        )
    }

    /// Whether a block of this level, read at this time, is at or over the
    /// floor, or follows closely enough on a run of blocks that were.
    pub fn hears(&mut self, level: u8, floor: u8, at_us: u64) -> bool {
        if level >= floor {
            self.run = (self.run + 1).min(EARN_BLOCKS);
            if self.run == EARN_BLOCKS && self.hold_us > 0 {
                self.held_until_us = Some(at_us + self.hold_us);
            }
            return true;
        }
        self.run = 0;
        self.held_until_us.is_some_and(|until| at_us <= until)
    }
}

/// How far the room microphone rose over its quiet (`room/sound`), sampled
/// every 100 ms as the provider worker samples it for `busy-near`, kept 30 s:
/// whether an utterance was said close to the lamp. Without the bus, never.
pub struct Rise {
    bus: Option<lamp_bus::Bus>,
    sampled_us: u64,
    rises: std::collections::VecDeque<(u64, f32)>,
}

impl Rise {
    pub fn from_environment() -> Self {
        let bus = (std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() == Some("on"))
            .then(|| {
                std::env::var_os(lamp_bus::ROOT_VARIABLE)
                    .filter(|root| !root.is_empty())
                    .unwrap_or_else(|| lamp_bus::DEFAULT_ROOT.into())
            })
            .and_then(|root| lamp_bus::Bus::new(std::path::Path::new(&root)).ok());
        Self::on(bus)
    }

    fn on(bus: Option<lamp_bus::Bus>) -> Self {
        Self {
            bus,
            sampled_us: 0,
            rises: std::collections::VecDeque::new(),
        }
    }

    /// Call often; reads the bus at most every 100 ms.
    pub fn sample(&mut self, now_us: u64) {
        let Some(bus) = &self.bus else { return };
        if now_us.saturating_sub(self.sampled_us) < 100_000 {
            return;
        }
        self.sampled_us = now_us;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        if let Some(level) = bus
            .latest(lamp_msgs::RoomSound::TOPIC)
            .ok()
            .flatten()
            .filter(|message| now_ms.saturating_sub(message.header.at_unix_ms) < 1_000)
            .and_then(|message| message.parse::<lamp_msgs::RoomSound>().ok())
            .filter(|level| !level.own_voice)
            && let Some(floor) = level.floor_db
        {
            self.rises.push_back((now_us, level.level_db - floor));
        }
        while self
            .rises
            .front()
            .is_some_and(|(at, _)| now_us.saturating_sub(*at) > 30_000_000)
        {
            self.rises.pop_front();
        }
    }

    /// Whether the room microphone rose the near rule's bar
    /// ([`crate::attention::near_rise_db`]) or more since `from_us`.
    pub fn close_since(&self, from_us: u64) -> bool {
        let bar = crate::attention::near_rise_db();
        self.rises
            .iter()
            .any(|(at, rise)| *at >= from_us && *rise >= bar)
    }
}

/// How many blocks of other people's speech, as it fades, there must be to
/// move the floor: about three seconds' worth.
const ENOUGH_BLOCKS: f32 = 300.0;
/// Three decibels above the middle of it.
const ABOVE: u8 = 23;
/// What has been heard fades by half in about two and a half minutes.
const FADE_A_SECOND: f32 = 0.9954;
const BINS: usize = 64;

pub struct RoomFloor {
    lowest: u8,
    highest: u8,
    /// Faded counts of blocks by level, four steps of the scale to a bin.
    heard: [f32; BINS],
    faded_us: u64,
    floor: u8,
    /// Where the room's talk alone would put it; none with too little.
    unbounded: Option<u8>,
}

impl RoomFloor {
    /// A floor that stays between `lowest` and `highest`. Equal: fixed.
    pub fn new(lowest: u8, highest: u8, now_us: u64) -> Self {
        Self {
            lowest,
            highest: highest.max(lowest),
            heard: [0.0; BINS],
            faded_us: now_us,
            floor: lowest,
            unbounded: None,
        }
    }

    /// One 10 ms block of speech, this loud, that was not for the lamp.
    pub fn not_for_the_lamp(&mut self, level: u8) {
        self.heard[usize::from(level) * BINS / 256] += 1.0;
    }

    /// Call often. Fades what was heard once a second and says where the
    /// floor is now.
    pub fn at(&mut self, now_us: u64) -> u8 {
        let seconds = now_us.saturating_sub(self.faded_us) / 1_000_000;
        if seconds > 0 {
            self.faded_us += seconds * 1_000_000;
            let fade = FADE_A_SECOND.powi(seconds.min(3_600) as i32);
            let mut total = 0.0;
            for count in &mut self.heard {
                *count *= fade;
                total += *count;
            }
            self.unbounded = if total < ENOUGH_BLOCKS {
                None
            } else {
                // The level half of it is under.
                let mut under = 0.0;
                let mut middle = 0usize;
                for (bin, count) in self.heard.iter().enumerate() {
                    under += count;
                    if under >= total / 2.0 {
                        middle = bin;
                        break;
                    }
                }
                let level = (middle * 256 / BINS + 256 / BINS / 2) as u8;
                Some(level.saturating_add(ABOVE))
            };
            self.floor = self
                .unbounded
                .map_or(self.lowest, |level| level.clamp(self.lowest, self.highest));
        }
        self.floor
    }

    /// Where the room's talk alone would put the floor, before its lowest
    /// and highest levels are applied; nothing when too little was heard.
    /// For the record: it shows what a wider range would do in a room
    /// before anyone gives it one.
    pub fn unbounded(&self) -> Option<u8> {
        self.unbounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rise is taken from `room/sound` at most every 100 ms, never while
    /// the lamp's own voice is in it, and kept 30 s; close since a moment
    /// means a big enough rise at or after it.
    #[test]
    fn a_rise_is_taken_from_the_room_sound_without_the_lamps_own_voice_and_kept_thirty_seconds() {
        let root = std::path::PathBuf::from(format!("/tmp/lvr-rise-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut sound = bus
            .publisher_declared(
                lamp_msgs::RoomSound::TOPIC,
                &lamp_msgs::RoomSound::declaration(),
                "test",
            )
            .unwrap();
        let mut say = |level_db: f32, own_voice: bool| {
            let level = lamp_msgs::RoomSound {
                level_db,
                floor_db: Some(-60.0),
                own_voice,
            };
            sound.publish_json(1, &level).unwrap();
        };
        let mut rise = Rise::on(Some(lamp_bus::Bus::new(&root).unwrap()));
        let near = crate::attention::NEAR_RISE_DB;
        say(-60.0 + near, false);
        rise.sample(1_000_000);
        assert!(rise.close_since(1_000_000));
        assert!(!rise.close_since(1_000_001));
        // Within 100 ms of the last sample, the bus is not read.
        say(-60.0 + near + 10.0, false);
        rise.sample(1_050_000);
        assert!(!rise.close_since(1_050_000));
        // The lamp's own voice is no one close.
        say(-30.0, true);
        rise.sample(1_200_000);
        assert!(!rise.close_since(1_100_000));
        // A small rise is not close; the big one is gone after 30 s.
        say(-60.0 + near - 1.0, false);
        rise.sample(31_100_000);
        assert!(!rise.close_since(0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_quiet_stretch_is_heard_after_a_run_over_the_floor_and_not_after_a_block_or_two() {
        let floor = 150;
        let block = |n: u64| n * 10_000;
        // Judged alone, as it always was.
        let mut alone = Near::new(0);
        for n in 0..10 {
            assert!(alone.hears(200, floor, block(n)));
        }
        assert!(!alone.hears(149, floor, block(10)));
        // A voice across the room: a block over the floor now and then
        // earns nothing.
        let mut far = Near::new(400);
        for n in 0..40 {
            let over = n % 4 == 0;
            assert_eq!(
                far.hears(if over { 200 } else { 140 }, floor, block(n)),
                over,
                "block {n}"
            );
        }
        // The phrase that a stricter run threw out on the lamp: four blocks
        // well over the floor, one just over, two well over, one just over,
        // then a tail under it. Eight in a row over the floor earn the hold.
        let mut phrase = Near::new(400);
        for (n, level) in [190, 195, 190, 180, 160, 175, 172, 152]
            .into_iter()
            .enumerate()
        {
            assert!(phrase.hears(level, floor, block(n as u64)));
        }
        for n in 8..30 {
            assert!(phrase.hears(120, floor, block(n)), "block {n}");
        }
        // Someone at the desk: six over the floor, then what trails under
        // it is heard for the hold and no longer.
        let mut near = Near::new(400);
        for n in 0..5 {
            assert!(near.hears(180, floor, block(n)));
        }
        assert!(!Near::new(400).hears(140, floor, 0));
        assert!(near.hears(180, floor, block(5)));
        for n in 6..=45 {
            assert!(near.hears(120, floor, block(n)), "block {n}");
        }
        assert!(!near.hears(120, floor, block(46)));
        // A run under six after that does not renew it.
        for n in 47..50 {
            assert!(near.hears(180, floor, block(n)));
        }
        assert!(!near.hears(120, floor, block(50)));
        // An hour is not a hold.
        assert_eq!(Near::new(3_600_000).hold_us, 1_000_000);
    }

    const S: u64 = 1_000_000;
    /// The face's scale for a level in dBFS.
    fn level(db: f32) -> u8 {
        ((db + 48.0) * 255.0 / 34.0).round() as u8
    }
    fn db(level: u8) -> f32 {
        -48.0 + f32::from(level) * 34.0 / 255.0
    }

    #[test]
    fn it_rises_over_a_room_that_is_talking_and_comes_down_when_the_room_goes_quiet() {
        let mut floor = RoomFloor::new(level(-40.0), level(-26.0), 0);
        let mut now = 0;
        // An empty room: the lowest level.
        assert_eq!(floor.at(60 * S), level(-40.0));
        assert_eq!(floor.unbounded(), None);
        // A second of someone else's talk is not enough to say.
        for _ in 0..100 {
            floor.not_for_the_lamp(level(-31.0));
        }
        now += 61 * S;
        assert_eq!(floor.at(now), level(-40.0));
        // Colleagues talking for a minute, around -31 dBFS: the floor comes
        // to about three decibels over that.
        for second in 0..60 {
            for block in 0..40 {
                floor.not_for_the_lamp(level(-34.0 + (block % 7) as f32));
            }
            now += S;
            floor.at(now);
            let _ = second;
        }
        let risen = db(floor.at(now));
        assert!((-29.5..=-26.5).contains(&risen), "{risen}");
        // It holds while they go on, and never passes its highest.
        for _ in 0..120 {
            for _ in 0..40 {
                floor.not_for_the_lamp(level(-22.0));
            }
            now += S;
            floor.at(now);
        }
        assert_eq!(floor.at(now), level(-26.0));
        // They leave. Twenty minutes later it is back where it began.
        now += 1_200 * S;
        assert_eq!(floor.at(now), level(-40.0));
    }

    #[test]
    fn with_one_level_for_both_ends_it_never_moves() {
        let mut floor = RoomFloor::new(level(-28.0), level(-28.0), 0);
        for second in 1..=300u64 {
            for _ in 0..80 {
                floor.not_for_the_lamp(level(-20.0));
            }
            assert_eq!(floor.at(second * S), level(-28.0));
        }
        // What the room alone would ask for is still known.
        assert!(
            floor
                .unbounded()
                .is_some_and(|wanted| wanted > level(-28.0))
        );
        // A highest under the lowest is the lowest.
        let mut floor = RoomFloor::new(level(-30.0), level(-45.0), 0);
        for _ in 0..1_000 {
            floor.not_for_the_lamp(255);
        }
        assert_eq!(floor.at(5 * S), level(-30.0));
    }
}
