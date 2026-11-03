//! The self-test's speaker check (#812): what the voice microphone made of
//! the lamp's own last speaking turn. Nothing is played for it.
//!
//! Each capture block's [`EchoEvidence`] says whether the speaker was being
//! given sound, and the level before and after the echo canceller. While the
//! lamp speaks, the level before rises over the room (the speaker and the
//! microphone work), and the level after stays well under it (the canceller
//! works). Two figures: before minus after at the turn's loudest block (as
//! the tone's was at its peak), and the median per block of before minus
//! after, over the blocks within [`WITHIN_LOUDEST_DB`] of the loudest of
//! their second ([`WINDOW_BLOCKS`]), which decides; the median as
//! `docs/verification/echo-lamp-52e6-20261016/aec-mics.sh` reads it from
//! `duplex_hearing`'s one-second trails.
//!
//! Not a probe: a 600 Hz tone measured 3 to 19 dB on lamp-52e6, as AEC3
//! does not adapt on a narrowband reference; pink noise and a quiet
//! synthetic voice never reached the canceller, as mic2's chip gates them;
//! a louder sound is not wanted from a lamp next to people (#705, #968).
//!
//! Nothing here reads a device or keeps a clock: the coordinator gives it
//! the blocks and when each was read.
use lamp_audio::evidence::{EchoEvidence, Level, QUIET_BLOCKS_CEILING};

/// A turn ends when the speaker has been given nothing for this many blocks.
pub const TURN_ENDS_QUIET_BLOCKS: u8 = QUIET_BLOCKS_CEILING;
/// Fewest blocks of the lamp speaking for a turn to be measured: 4 s.
pub const TURN_BLOCKS: usize = 400;
/// Longest turn kept, in blocks (60 s); the rest of a longer one is not.
pub const KEPT_BLOCKS: usize = 6_000;
/// Blocks before a turn whose capture is the room's level.
pub const ROOM_BLOCKS: usize = 50;
/// Only blocks this close to the loudest of their second are measured, so
/// the quiet ends of words, where the residual is at its floor, do not
/// decide it, and quieter passages count as much as loud ones.
pub const WITHIN_LOUDEST_DB: f32 = 20.0;
/// The window the loudest is taken over: one second, as a trail.
pub const WINDOW_BLOCKS: usize = 100;
/// The rise over the room that counts as the lamp heard.
pub const HEARD_RISE_DB: f32 = 10.0;
/// The median removed must be at least this. The lamp's own speech got 54
/// dB on mic2 and 27 dB on mic1 (lamp-52e6, #705).
pub const CANCELLED_DB: f32 = 20.0;

/// One capture block's levels, in dB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub capture_db: f32,
    pub residual_db: f32,
}

impl Block {
    fn of(echo: &EchoEvidence) -> Self {
        Self {
            capture_db: db(echo.capture_level),
            residual_db: db(echo.residual_level),
        }
    }
}

fn db(level: Level) -> f32 {
    f32::from(level.tenths_of_db()) / 10.0
}

/// What the voice microphone made of one speaking turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    /// Blocks of the lamp speaking.
    pub blocks: u32,
    /// The loudest capture over the room's level before the turn.
    pub rise_db: f32,
    /// Before minus after the canceller at the turn's loudest block.
    pub cancelled_db: f32,
    /// Median of before minus after the canceller, over the blocks within
    /// [`WITHIN_LOUDEST_DB`] of the loudest of their second.
    pub cancelled_median_db: f32,
    /// Some of those blocks (the loudest among them) had the residual at the
    /// evidence's floor, so the true figures are at least these.
    pub lower_bound: bool,
    /// When the turn's last speaking block was read, in the monotonic
    /// microseconds the coordinator gives.
    pub ended_at_us: u64,
}

/// Judges one turn's blocks, with the room's blocks before it; the turn's
/// last block was read at `ended_at_us`.
pub fn measure(room: &[Block], turn: &[Block], ended_at_us: u64) -> Option<Measured> {
    if turn.len() < TURN_BLOCKS || room.is_empty() {
        return None;
    }
    let room_db = room.iter().map(|b| b.capture_db).sum::<f32>() / room.len() as f32;
    let loudest = turn
        .iter()
        .max_by(|a, b| a.capture_db.total_cmp(&b.capture_db))?;
    let floor = db(Level::FLOOR);
    let near: Vec<&Block> = turn
        .chunks(WINDOW_BLOCKS)
        .flat_map(|second| {
            let top = second
                .iter()
                .map(|b| b.capture_db)
                .fold(f32::NEG_INFINITY, f32::max);
            second
                .iter()
                .filter(move |b| b.capture_db >= top - WITHIN_LOUDEST_DB)
        })
        .collect();
    let mut removed: Vec<f32> = near.iter().map(|b| b.capture_db - b.residual_db).collect();
    removed.sort_by(f32::total_cmp);
    Some(Measured {
        blocks: turn.len() as u32,
        rise_db: loudest.capture_db - room_db,
        cancelled_db: loudest.capture_db - loudest.residual_db,
        cancelled_median_db: removed[removed.len() / 2],
        lower_bound: near.iter().any(|b| b.residual_db <= floor),
        ended_at_us,
    })
}

/// The blocks of the lamp's speaking turns, and the last one measured.
#[derive(Default)]
pub struct Turns {
    room: std::collections::VecDeque<Block>,
    current: Vec<Block>,
    current_room: Vec<Block>,
    current_ended_at_us: u64,
    last: Option<Measured>,
}

impl Turns {
    /// One capture block's evidence, read at `read_at_us`.
    pub fn observe(&mut self, echo: &EchoEvidence, read_at_us: u64) {
        let block = Block::of(echo);
        if echo.render_quiet_blocks == 0 {
            if self.current.is_empty() {
                self.current_room = self.room.iter().copied().collect();
            }
            if self.current.len() < KEPT_BLOCKS {
                self.current.push(block);
            }
            self.current_ended_at_us = read_at_us;
            return;
        }
        if echo.render_quiet_blocks >= TURN_ENDS_QUIET_BLOCKS {
            if let Some(measured) =
                measure(&self.current_room, &self.current, self.current_ended_at_us)
            {
                self.last = Some(measured);
            }
            self.current.clear();
            self.room.push_back(block);
            while self.room.len() > ROOM_BLOCKS {
                self.room.pop_front();
            }
        }
    }

    /// The last speaking turn of [`TURN_BLOCKS`] or more, measured.
    pub fn last(&self) -> Option<&Measured> {
        self.last.as_ref()
    }
}

/// The verdict on a measured turn, or why none: `pass`, `fail` or
/// `not_checked`, and the reason.
pub fn verdict(last: Option<&Measured>) -> (&'static str, String) {
    match last {
        None => (
            "not_checked",
            format!(
                "the lamp has not spoken for {} s at a stretch since the voice started",
                TURN_BLOCKS / 100
            ),
        ),
        Some(m) if m.rise_db < HEARD_RISE_DB => (
            "fail",
            format!(
                "the voice microphone did not hear the lamp speak: {:.0} dB over the room at most",
                m.rise_db
            ),
        ),
        Some(m) if m.cancelled_median_db < CANCELLED_DB => (
            "fail",
            format!(
                "the echo canceller took a median of only {:.1} dB off the lamp's own speech",
                m.cancelled_median_db
            ),
        ),
        Some(_) => ("pass", String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(quiet: u8, capture_db: f32, residual_db: f32) -> EchoEvidence {
        EchoEvidence {
            blocks: 1,
            render_active_blocks: 0,
            render_quiet_blocks: quiet,
            capture_level: Level::from_tenths_of_db((capture_db * 10.0) as i16),
            residual_level: Level::from_tenths_of_db((residual_db * 10.0) as i16),
            echo: None,
            silent_at_echo: None,
            echo_delay_blocks: None,
        }
    }

    /// The room, then `blocks` of the lamp speaking, then quiet long enough
    /// to end the turn.
    fn turn(turns: &mut Turns, blocks: usize, capture_db: f32, residual_db: f32) {
        for _ in 0..ROOM_BLOCKS {
            turns.observe(&evidence(QUIET_BLOCKS_CEILING, -60.0, -61.0), 0);
        }
        for n in 0..blocks {
            // Every tenth block is a quiet end of a word, far under the
            // loudest: not measured.
            let quiet = n % 10 == 9;
            turns.observe(
                &evidence(
                    0,
                    if quiet { capture_db - 30.0 } else { capture_db },
                    if quiet { -61.0 } else { residual_db },
                ),
                n as u64 * 10_000,
            );
        }
        for quiet in 1..=QUIET_BLOCKS_CEILING {
            turns.observe(&evidence(quiet, -60.0, -61.0), 99_000_000);
        }
    }

    #[test]
    fn a_speaking_turn_gives_the_median_removed_near_its_loudest_and_its_rise() {
        let mut turns = Turns::default();
        turn(&mut turns, 500, -15.0, -65.0);
        let last = turns.last().unwrap();
        assert_eq!(last.blocks, 500);
        assert_eq!(last.cancelled_median_db, 50.0);
        assert_eq!(last.cancelled_db, 50.0);
        assert_eq!(last.rise_db, 45.0);
        assert_eq!(last.ended_at_us, 499 * 10_000);
        assert!(!last.lower_bound);
        assert_eq!(verdict(Some(last)).0, "pass");
    }

    #[test]
    fn a_quieter_second_counts_against_its_own_loudest() {
        let room: Vec<Block> = (0..ROOM_BLOCKS)
            .map(|_| Block {
                capture_db: -60.0,
                residual_db: -61.0,
            })
            .collect();
        // Two loud seconds removing 50 dB, three quiet ones removing 30 dB:
        // against the turn's loudest the quiet ones would not count.
        let turn: Vec<Block> = (0..500)
            .map(|n| {
                if n < 200 {
                    Block {
                        capture_db: -10.0,
                        residual_db: -60.0,
                    }
                } else {
                    Block {
                        capture_db: -35.0,
                        residual_db: -65.0,
                    }
                }
            })
            .collect();
        let measured = measure(&room, &turn, 0).unwrap();
        assert_eq!(measured.cancelled_median_db, 30.0);
        // At the loudest block, as the tone's peak was.
        assert_eq!(measured.cancelled_db, 50.0);
    }

    #[test]
    fn a_residual_at_the_floor_makes_the_figure_a_lower_bound() {
        let mut turns = Turns::default();
        turn(&mut turns, 500, -15.0, -200.0);
        let last = turns.last().unwrap();
        assert!(last.lower_bound);
        assert_eq!(last.cancelled_median_db, -15.0 - db(Level::FLOOR));
    }

    #[test]
    fn a_short_turn_is_not_measured_and_the_last_long_one_stands() {
        let mut turns = Turns::default();
        turn(&mut turns, 300, -15.0, -65.0);
        assert!(turns.last().is_none());
        assert!(verdict(None).1.contains("4 s"));
        turn(&mut turns, 450, -15.0, -65.0);
        turn(&mut turns, 100, -15.0, -20.0);
        assert_eq!(turns.last().unwrap().blocks, 450);
    }

    #[test]
    fn an_unheard_lamp_or_a_poor_canceller_fails() {
        let mut turns = Turns::default();
        turn(&mut turns, 500, -55.0, -65.0);
        assert!(verdict(turns.last()).1.contains("did not hear"));
        turn(&mut turns, 500, -15.0, -25.0);
        assert!(verdict(turns.last()).1.contains("10.0 dB"));
    }
}
