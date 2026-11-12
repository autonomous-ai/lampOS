//! The lamp's name heard in a line the faint floor turned away (#5540).
//!
//! A line opens from lamp-voice's own detector, not from the name. A block
//! quieter than the faint floor (`crate::room_floor`) is not speech whatever
//! the detector makes of it, so a call said just under the floor opens no
//! line, and until now that was said nowhere: on lamp-06c1 on 12 Nov a
//! "Hey Lamp" that `lamp-name` heard at 0.937 left lamp-voice's journal
//! empty for a minute. When `lamp-name` hears the name and the floor turned
//! away speech blocks in the hearing, lamp-voice says so in one line, with
//! the level against the floor. It only says it: nothing is heard that was
//! not heard before.

use std::collections::VecDeque;

/// Its own name on `name/heard`: `voice` is the second look's.
pub const SUBSCRIBER: &str = "voice_faint";

/// The detector's blocks are kept this long: a hearing is said a tenth of a
/// second or more after the name began, and the name is a second or so.
const KEPT_US: u64 = 3_000_000;
/// A block this long before the name's beginning still counts for it: when
/// the name began is the listener's own estimate.
const SLACK_US: u64 = 300_000;

/// What the detector took for speech lately, and the hearing last said.
#[derive(Debug, Default)]
pub struct FaintNamed {
    /// `(read at, level, floor, turned away)` for each block the detector
    /// scored 0.5 or more while the floor was in force.
    blocks: VecDeque<(u64, u8, u8, bool)>,
    /// The end of the last hearing said, so that one hearing is said once.
    said_until_us: Option<u64>,
}

impl FaintNamed {
    /// One block as the detector and the floor judged it.
    pub fn block(
        &mut self,
        at_us: u64,
        probability: f32,
        level: u8,
        floor: Option<u8>,
        under_floor: bool,
    ) {
        while self
            .blocks
            .front()
            .is_some_and(|(at, ..)| at_us.saturating_sub(*at) > KEPT_US)
        {
            self.blocks.pop_front();
        }
        if let Some(floor) = floor
            && probability.is_finite()
            && probability >= 0.5
        {
            self.blocks.push_back((at_us, level, floor, under_floor));
        }
    }

    /// `lamp-name` heard the name, begun at `began_us`, said at `now_us`:
    /// the line to log, if the floor turned away speech in it. A hearing
    /// that begins within one already said is not said again.
    pub fn named(&mut self, began_us: u64, now_us: u64, score: f32) -> Option<String> {
        if self.said_until_us.is_some_and(|until| began_us <= until) {
            return None;
        }
        let from_us = began_us.saturating_sub(SLACK_US);
        let in_it = self
            .blocks
            .iter()
            .filter(|(at, ..)| (from_us..=now_us).contains(at));
        let speech = in_it.clone().count();
        let (loudest, floor) = in_it
            .filter(|(.., under)| *under)
            .map(|(_, level, floor, _)| (*level, *floor))
            .max_by_key(|(level, _)| *level)?;
        let under = self
            .blocks
            .iter()
            .filter(|(at, .., under)| *under && (from_us..=now_us).contains(at))
            .count();
        self.said_until_us = Some(now_us);
        Some(format!(
            "lamp-voice: the lamp's name was heard (score {score:.2}) but the faint floor turned the line away: {under} of {speech} speech blocks were under it, the loudest at {:.1} dBFS (level {loudest}) against a floor of {:.1} dBFS (level {floor})",
            dbfs(loudest),
            dbfs(floor),
        ))
    }
}

/// A level on the voice's scale as dBFS (`ring_face::level_of`).
fn dbfs(level: u8) -> f32 {
    -48.0 + f32::from(level) * 34.0 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_US: u64 = 10_000;

    /// A call said just under a floor of 150 (-28 dBFS), the detector sure
    /// it is speech: 40 blocks from 1 s, the loudest 141.
    fn under_the_floor(heard: &mut FaintNamed) {
        for index in 0..40u64 {
            let level = if index == 20 { 141 } else { 135 };
            heard.block(1_000_000 + index * BLOCK_US, 0.9, level, Some(150), true);
        }
    }

    #[test]
    fn a_name_heard_in_a_line_the_floor_turned_away_is_said_with_its_level() {
        let mut heard = FaintNamed::default();
        under_the_floor(&mut heard);
        let line = heard.named(1_050_000, 1_500_000, 0.937);
        assert_eq!(
            line.as_deref(),
            Some(
                "lamp-voice: the lamp's name was heard (score 0.94) but the faint floor turned the line away: 40 of 40 speech blocks were under it, the loudest at -29.2 dBFS (level 141) against a floor of -28.0 dBFS (level 150)"
            )
        );
    }

    #[test]
    fn one_hearing_is_said_once() {
        let mut heard = FaintNamed::default();
        under_the_floor(&mut heard);
        assert!(heard.named(1_050_000, 1_500_000, 0.937).is_some());
        // The same hearing again, and a later one of the same call.
        assert_eq!(heard.named(1_050_000, 1_500_000, 0.937), None);
        assert_eq!(heard.named(1_200_000, 1_520_000, 0.95), None);
    }

    #[test]
    fn nothing_is_said_without_a_name_or_when_the_floor_turned_nothing_away() {
        let mut heard = FaintNamed::default();
        // A quiet room: blocks under the floor, no name.
        under_the_floor(&mut heard);
        // A name over the floor: the line was heard.
        let mut over = FaintNamed::default();
        for index in 0..40u64 {
            over.block(1_000_000 + index * BLOCK_US, 0.9, 160, Some(150), false);
        }
        assert_eq!(over.named(1_050_000, 1_500_000, 0.99), None);
        // Under the floor but long before the name.
        assert_eq!(heard.named(3_000_000, 3_400_000, 0.99), None);
        // No floor in force.
        let mut no_floor = FaintNamed::default();
        for index in 0..40u64 {
            no_floor.block(1_000_000 + index * BLOCK_US, 0.9, 135, None, false);
        }
        assert_eq!(no_floor.named(1_050_000, 1_500_000, 0.99), None);
    }

    #[test]
    fn levels_read_as_the_voice_records_them() {
        assert!((dbfs(150) - -28.0).abs() < 0.01);
    }
}
