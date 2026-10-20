//! What the room microphone hears, reduced to how loud: the room's level
//! against its own quiet, and sounds that rise suddenly out of it. Nothing
//! of the sound itself is kept by default. Explicit privacy-gated
//! `LAMP_BUS_AUDIO=on` can export bounded raw mic1 blocks. See `docs/ears.md`.
use lamp_msgs::{EARS_SILENCE_DB, RoomSound, SuddenSound};
use std::collections::VecDeque;

pub mod classes;
pub mod pcm;
pub mod privacy;

/// Samples a second.
pub const RATE: u32 = 16_000;
/// Samples in one block: 20 ms.
pub const BLOCK: usize = 320;
const BLOCK_MS: u64 = 20;
/// Blocks in one level said: 100 ms.
const WINDOW_BLOCKS: usize = 5;
/// Levels the quiet is taken from: the last 30 s.
const FLOOR_WINDOWS: usize = 300;
/// Levels needed before a quiet is given: two seconds.
const FLOOR_LEAST: usize = 20;
/// Blocks before a sudden sound that must have been near the quiet: 200 ms.
const BEFORE_BLOCKS: usize = 10;

/// When a sound counts as sudden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// A block this many dB over the quiet may begin one.
    pub rise_db: f32,
    /// And it is at least this loud, in dBFS.
    pub least_db: f32,
    /// The 200 ms before it were no more than this over the quiet.
    pub before_db: f32,
    /// How long it is watched before it is said.
    pub watch_ms: u64,
    /// Loud (10 dB over the quiet) for less than this: short.
    pub short_ms: u64,
    /// No new one for this long after one.
    pub rest_ms: u64,
    /// After the lamp stops speaking, its own voice may still be in the
    /// room for this long: nothing is sudden, and the quiet is not learned.
    pub own_tail_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // 20 dB gave 2 to 7 a minute in an office at work (keys, cups,
            // the start of a word); 30 dB, about one in twenty minutes.
            rise_db: 30.0,
            least_db: -40.0,
            before_db: 10.0,
            watch_ms: 300,
            short_ms: 200,
            rest_ms: 1_000,
            own_tail_ms: 500,
        }
    }
}

/// What one block gave: a level every 100 ms, and a sudden sound once it
/// has been watched.
#[derive(Debug, Default, PartialEq)]
pub struct Heard {
    pub level: Option<RoomSound>,
    pub sudden: Option<SuddenSound>,
}

#[derive(Debug)]
struct Watching {
    began_ms: u64,
    peak_db: f32,
    floor_db: f32,
    loud_blocks: u64,
}

/// Listens to the room microphone a block at a time. Its time is the
/// sound's own: 20 ms a block.
#[derive(Debug)]
pub struct Listener {
    settings: Settings,
    now_ms: u64,
    window_energy: f64,
    window_blocks: usize,
    window_own: bool,
    floor: VecDeque<f32>,
    before: VecDeque<f64>,
    spoke_at_ms: Option<u64>,
    watching: Option<Watching>,
    rest_until_ms: u64,
}

/// The mean square of a block, as a share of full scale squared.
fn energy(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&sample| {
            let share = f64::from(sample) / 32_768.0;
            share * share
        })
        .sum();
    sum / samples.len() as f64
}

fn decibels(energy: f64) -> f32 {
    if energy <= 0.0 {
        return EARS_SILENCE_DB;
    }
    ((10.0 * energy.log10()) as f32).clamp(EARS_SILENCE_DB, 0.0)
}

impl Listener {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            now_ms: 0,
            window_energy: 0.0,
            window_blocks: 0,
            window_own: false,
            floor: VecDeque::with_capacity(FLOOR_WINDOWS),
            before: VecDeque::with_capacity(BEFORE_BLOCKS),
            spoke_at_ms: None,
            watching: None,
            rest_until_ms: 0,
        }
    }

    /// The room's quiet now: the tenth percentile of the last 30 s heard
    /// while the lamp was not speaking.
    pub fn floor_db(&self) -> Option<f32> {
        if self.floor.len() < FLOOR_LEAST {
            return None;
        }
        let mut sorted: Vec<f32> = self.floor.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        Some(sorted[sorted.len() / 10])
    }

    /// The sound's own time, in milliseconds since the first block.
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// One block of 20 ms, and whether the lamp was speaking during it.
    pub fn block(&mut self, samples: &[i16], lamp_speaking: bool) -> Heard {
        self.now_ms += BLOCK_MS;
        let now = self.now_ms;
        if lamp_speaking {
            self.spoke_at_ms = Some(now);
        }
        let own = self
            .spoke_at_ms
            .is_some_and(|at| now.saturating_sub(at) < self.settings.own_tail_ms);
        let block_energy = energy(samples);
        let db = decibels(block_energy);
        let floor = self.floor_db();
        let mut heard = Heard::default();

        if own {
            // The lamp's own voice: whatever was rising is not the room's.
            self.watching = None;
        } else if let Some(watching) = &mut self.watching {
            watching.peak_db = watching.peak_db.max(db);
            if db >= watching.floor_db + self.settings.before_db {
                watching.loud_blocks += 1;
            }
            let watched = now - watching.began_ms;
            if watched >= self.settings.watch_ms {
                let loud_ms = watching.loud_blocks * BLOCK_MS;
                heard.sudden = Some(SuddenSound {
                    peak_db: watching.peak_db,
                    floor_db: watching.floor_db,
                    loud_ms: loud_ms as u32,
                    short: loud_ms < self.settings.short_ms,
                    ago_ms: watched as u32,
                });
                self.watching = None;
                self.rest_until_ms = now + self.settings.rest_ms;
            }
        } else if let Some(floor) = floor
            && now >= self.rest_until_ms
            && db >= floor + self.settings.rise_db
            && db >= self.settings.least_db
            && self.before.len() == BEFORE_BLOCKS
            && decibels(self.before.iter().sum::<f64>() / BEFORE_BLOCKS as f64)
                < floor + self.settings.before_db
        {
            self.watching = Some(Watching {
                // It began at the start of this block.
                began_ms: now - BLOCK_MS,
                peak_db: db,
                floor_db: floor,
                loud_blocks: 1,
            });
        }

        if self.before.len() == BEFORE_BLOCKS {
            self.before.pop_front();
        }
        self.before.push_back(block_energy);

        self.window_energy += block_energy;
        self.window_blocks += 1;
        self.window_own |= own;
        if self.window_blocks == WINDOW_BLOCKS {
            let level_db = decibels(self.window_energy / WINDOW_BLOCKS as f64);
            if !self.window_own {
                if self.floor.len() == FLOOR_WINDOWS {
                    self.floor.pop_front();
                }
                self.floor.push_back(level_db);
            }
            heard.level = Some(RoomSound {
                level_db,
                floor_db: self.floor_db(),
                own_voice: self.window_own,
            });
            self.window_energy = 0.0;
            self.window_blocks = 0;
            self.window_own = false;
        }
        heard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Noise at about `dbfs`, the same every time.
    fn noise(dbfs: f32, blocks: usize, seed: &mut u64) -> Vec<Vec<i16>> {
        // A uniform sample of amplitude A has an RMS of A / sqrt(3).
        let amplitude = 10f64.powf(f64::from(dbfs) / 20.0) * 3f64.sqrt() * 32_767.0;
        (0..blocks)
            .map(|_| {
                (0..BLOCK)
                    .map(|_| {
                        *seed = seed
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1_442_695_040_888_963_407);
                        let unit = ((*seed >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
                        (unit * amplitude).clamp(-32_768.0, 32_767.0) as i16
                    })
                    .collect()
            })
            .collect()
    }

    struct Run {
        listener: Listener,
        seed: u64,
        sudden: Vec<(u64, SuddenSound)>,
        levels: Vec<RoomSound>,
    }

    impl Run {
        fn new() -> Self {
            Self {
                listener: Listener::new(Settings::default()),
                seed: 7,
                sudden: Vec::new(),
                levels: Vec::new(),
            }
        }

        fn play(&mut self, dbfs: f32, ms: usize, lamp_speaking: bool) {
            for block in noise(dbfs, ms / 20, &mut self.seed) {
                let heard = self.listener.block(&block, lamp_speaking);
                if let Some(sudden) = heard.sudden {
                    self.sudden.push((self.listener.now_ms(), sudden));
                }
                self.levels.extend(heard.level);
            }
        }
    }

    #[test]
    fn a_quiet_room_has_a_floor_near_its_level_and_nothing_sudden() {
        let mut run = Run::new();
        run.play(-52.0, 10_000, false);
        let floor = run.listener.floor_db().unwrap();
        assert!((floor + 52.0).abs() < 1.0, "{floor}");
        assert!(run.sudden.is_empty());
        assert_eq!(run.levels.len(), 100);
        assert_eq!(run.levels[0].floor_db, None);
        // Two seconds of levels: the twentieth is the first with a quiet.
        assert!(run.levels[18].floor_db.is_none() && run.levels[19].floor_db.is_some());
    }

    #[test]
    fn a_knock_is_one_short_sudden_sound_said_after_it_was_watched() {
        let mut run = Run::new();
        run.play(-52.0, 3_000, false);
        run.play(-14.0, 60, false);
        run.play(-52.0, 2_000, false);
        assert_eq!(run.sudden.len(), 1, "{:?}", run.sudden);
        let (at, knock) = run.sudden[0];
        assert!(knock.short && knock.loud_ms == 60, "{knock:?}");
        assert!((knock.peak_db + 14.0).abs() < 1.0 && (knock.floor_db + 52.0).abs() < 1.0);
        assert_eq!((at, knock.ago_ms), (3_300, 300));
    }

    #[test]
    fn a_voice_beginning_close_by_is_sudden_and_not_short() {
        let mut run = Run::new();
        run.play(-52.0, 3_000, false);
        run.play(-18.0, 1_500, false);
        assert_eq!(run.sudden.len(), 1);
        let voice = run.sudden[0].1;
        assert!(!voice.short && voice.loud_ms == 300, "{voice:?}");
    }

    #[test]
    fn a_sound_that_grows_slowly_or_a_room_already_loud_is_not_sudden() {
        let mut run = Run::new();
        run.play(-52.0, 3_000, false);
        for step in 0..20 {
            run.play(-52.0 + step as f32 * 2.0, 200, false);
        }
        assert!(run.sudden.is_empty(), "{:?}", run.sudden);
        let mut loud = Run::new();
        loud.play(-30.0, 3_000, false);
        loud.play(-14.0, 60, false);
        assert!(loud.sudden.is_empty());
    }

    #[test]
    fn nothing_is_sudden_while_the_lamp_speaks_or_just_after_and_its_voice_is_not_the_quiet() {
        let mut run = Run::new();
        run.play(-52.0, 3_000, false);
        run.play(-15.0, 4_000, true);
        run.play(-14.0, 60, false);
        run.play(-52.0, 400, false);
        assert!(run.sudden.is_empty(), "{:?}", run.sudden);
        let floor = run.listener.floor_db().unwrap();
        assert!((floor + 52.0).abs() < 1.0, "{floor}");
        assert!(run.levels.iter().any(|level| level.own_voice));
        // Half a second after it stopped, a knock is the room's again.
        run.play(-52.0, 600, false);
        run.play(-14.0, 60, false);
        run.play(-52.0, 600, false);
        assert_eq!(run.sudden.len(), 1);
    }

    #[test]
    fn a_second_sound_inside_the_rest_is_not_said_again() {
        let mut run = Run::new();
        run.play(-52.0, 3_000, false);
        run.play(-14.0, 60, false);
        run.play(-52.0, 600, false);
        run.play(-14.0, 60, false);
        run.play(-52.0, 1_500, false);
        assert_eq!(run.sudden.len(), 1);
        run.play(-14.0, 60, false);
        run.play(-52.0, 600, false);
        assert_eq!(run.sudden.len(), 2);
    }

    #[test]
    fn nothing_is_sudden_before_the_quiet_is_known() {
        let mut run = Run::new();
        run.play(-52.0, 1_000, false);
        run.play(-14.0, 60, false);
        run.play(-52.0, 600, false);
        assert!(run.sudden.is_empty());
    }

    #[test]
    fn silence_is_the_quietest_level_and_a_full_block_is_zero() {
        assert_eq!(decibels(energy(&[0; BLOCK])), EARS_SILENCE_DB);
        assert_eq!(decibels(energy(&[i16::MIN; BLOCK])), 0.0);
    }
}
