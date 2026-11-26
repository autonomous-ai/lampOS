//! The lamp's name heard in a line the faint floor turned away (#5540).
//!
//! A line opens from lamp-voice's own detector, not from the name. A block
//! quieter than the faint floor (`crate::room_floor`) is not speech whatever
//! the detector makes of it, so a call said just under the floor opens no
//! line, and until now that was said nowhere: on lamp-06c1 on 12 Nov a
//! "Hey Lamp" that `lamp-name` heard at 0.937 left lamp-voice's journal
//! empty for a minute. When `lamp-name` hears the name and the floor turned
//! away speech blocks in the hearing, lamp-voice says so in one line: the
//! blocks over and under the floor, the call's own loudest block and the
//! loudest under the floor, and whether the floor could have kept a line
//! from opening (#8728). It only says it: nothing is heard that was not
//! heard before.

use std::collections::VecDeque;

use crate::activity::{START_BLOCKS, START_PROBABILITY};

/// Its own name on `name/heard`: `voice` is the second look's.
pub const SUBSCRIBER: &str = "voice_faint";

/// The detector's blocks are kept this long. A hearing is said a tenth of a
/// second or more after the name began, but a woken grid's late score can
/// date it about 3.4 s back (`hear.rs`' late cut in lamp-name, #6114), and a
/// hearing runs up to 800 ms (lamp-name's `said::LONGEST`). Neither is
/// reachable from this crate, so 5 s covers both with the 0.3 s slack.
const KEPT_US: u64 = 5_000_000;
/// A block this long before the name's beginning still counts for it: when
/// the name began is the listener's own estimate.
const SLACK_US: u64 = 300_000;

/// What the detector took for speech lately, and the hearing last said.
#[derive(Debug, Default)]
pub struct FaintNamed {
    /// Each block the detector scored 0.5 or more while the floor was in
    /// force.
    blocks: VecDeque<Block>,
    /// The end of the last hearing said, so that one hearing is said once.
    said_until_us: Option<u64>,
    /// Blocks in a row, up to the last one, scored
    /// [`START_PROBABILITY`] or more: over the floor, and whatever the floor.
    run_over: u16,
    run_any: u16,
}

/// One block as the detector and the floor judged it.
#[derive(Debug, Clone, Copy)]
struct Block {
    at_us: u64,
    level: u8,
    floor: u8,
    under: bool,
    /// The runs ([`FaintNamed::run_over`], [`FaintNamed::run_any`]) this
    /// block ends.
    run_over: u16,
    run_any: u16,
}

impl FaintNamed {
    /// One block as the detector and the floor judged it. Told of every
    /// block, so that its runs are runs.
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
            .is_some_and(|block| at_us.saturating_sub(block.at_us) > KEPT_US)
        {
            self.blocks.pop_front();
        }
        let sure = floor.is_some() && probability.is_finite() && probability >= START_PROBABILITY;
        self.run_any = if sure { self.run_any.saturating_add(1) } else { 0 };
        self.run_over = if sure && !under_floor {
            self.run_over.saturating_add(1)
        } else {
            0
        };
        if let Some(floor) = floor
            && probability.is_finite()
            && probability >= 0.5
        {
            self.blocks.push_back(Block {
                at_us,
                level,
                floor,
                under: under_floor,
                run_over: self.run_over,
                run_any: self.run_any,
            });
        }
    }

    /// `lamp-name` heard the name, begun at `began_us`, said at `now_us`:
    /// the line to log, if the floor turned away speech in it. A hearing
    /// that begins within one already said is not said again.
    ///
    /// Whether a line opened is the coordinator's, not known here. What is
    /// known is whether the floor could have kept one from opening: a line
    /// opens on [`START_BLOCKS`] blocks in a row scored
    /// [`START_PROBABILITY`] or more after the floor (`activity.rs`). With
    /// such a run over the floor, the floor did not keep the line shut;
    /// with one only when the floor is set aside, it did; with neither, the
    /// detector alone opened nothing.
    pub fn named(&mut self, began_us: u64, now_us: u64, score: f32) -> Option<String> {
        if self.said_until_us.is_some_and(|until| began_us <= until) {
            return None;
        }
        let from_us = began_us.saturating_sub(SLACK_US);
        let in_it: Vec<Block> = self
            .blocks
            .iter()
            .filter(|block| (from_us..=now_us).contains(&block.at_us))
            .copied()
            .collect();
        let loudest_under = in_it
            .iter()
            .filter(|block| block.under)
            .max_by_key(|block| block.level)?;
        let speech = in_it.len();
        let under = in_it.iter().filter(|block| block.under).count();
        let over = speech - under;
        let loudest = in_it.iter().map(|block| block.level).max().unwrap_or(0);
        let run_over = in_it.iter().map(|block| block.run_over).max().unwrap_or(0);
        let run_any = in_it.iter().map(|block| block.run_any).max().unwrap_or(0);
        let floor = loudest_under.floor;
        let levels = format!(
            "{over} of {speech} speech blocks were over it and {under} under, the call's loudest at {:.1} dBFS (level {loudest}) and the loudest under it at {:.1} dBFS (level {}) against a floor of {:.1} dBFS (level {floor})",
            dbfs(loudest),
            dbfs(loudest_under.level),
            loudest_under.level,
            dbfs(floor),
        );
        self.said_until_us = Some(now_us);
        let needs = START_BLOCKS;
        Some(if run_over >= needs {
            format!(
                "lamp-voice: the lamp's name was heard (score {score:.2}) and the faint floor left a line free to open: {levels}; the longest run over it was {run_over} blocks (a line needs {needs})"
            )
        } else if run_any >= needs {
            format!(
                "lamp-voice: the lamp's name was heard (score {score:.2}) but the faint floor turned the line away: {levels}; the longest run over it was {run_over} blocks (a line needs {needs}), {run_any} without it"
            )
        } else {
            format!(
                "lamp-voice: the lamp's name was heard (score {score:.2}) but no line could open, floor or not: {levels}; the longest run the detector was sure of was {run_any} blocks (a line needs {needs})"
            )
        })
    }
}

impl FaintNamed {
    /// Whether the floor turned away speech in a hearing begun at
    /// `began_us`, said at `now_us`: the hearing [`Self::named`] says, said
    /// before or not (`LAMP_NAME_FACE_OR_WORDS`, #8136).
    pub fn turned_away(&self, began_us: u64, now_us: u64) -> bool {
        let from_us = began_us.saturating_sub(SLACK_US);
        self.blocks
            .iter()
            .any(|block| block.under && (from_us..=now_us).contains(&block.at_us))
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
                "lamp-voice: the lamp's name was heard (score 0.94) but the faint floor turned the line away: 0 of 40 speech blocks were over it and 40 under, the call's loudest at -29.2 dBFS (level 141) and the loudest under it at -29.2 dBFS (level 141) against a floor of -28.0 dBFS (level 150); the longest run over it was 0 blocks (a line needs 6), 40 without it"
            )
        );
    }

    /// #8136, from #8114's shape 6 (lamp-74ff, 20 Nov 17:32:11: the loudest
    /// at -28.1 dBFS against a floor of -28.0): a hearing over blocks the
    /// floor turned away is one it turned away, said once or not; one over
    /// blocks it let by is not.
    #[test]
    fn a_hearing_over_blocks_the_floor_turned_away_is_turned_away() {
        let mut heard = FaintNamed::default();
        under_the_floor(&mut heard);
        assert!(heard.turned_away(1_050_000, 1_500_000));
        assert!(heard.named(1_050_000, 1_500_000, 0.904).is_some());
        assert!(heard.turned_away(1_050_000, 1_500_000));
        let mut heard = FaintNamed::default();
        for index in 0..40u64 {
            heard.block(1_000_000 + index * BLOCK_US, 0.9, 170, Some(150), false);
        }
        assert!(!heard.turned_away(1_050_000, 1_500_000));
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
        // A quiet room: blocks under the floor, no name. Nothing is said
        // until a name comes, however long the blocks go on.
        under_the_floor(&mut heard);
        for index in 40..400u64 {
            heard.block(1_000_000 + index * BLOCK_US, 0.9, 135, Some(150), true);
        }
        assert_eq!(heard.said_until_us, None, "no line without a name");
        let mut heard = FaintNamed::default();
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
    fn a_late_hearing_dated_far_back_is_still_said() {
        // A woken grid's late score dates the name 3.8 s back (#6114): the
        // call's blocks, turned away, are still kept when it is said.
        let mut heard = FaintNamed::default();
        under_the_floor(&mut heard);
        let began = 1_050_000;
        let now = began + 3_800_000;
        for index in 0..345u64 {
            heard.block(1_400_000 + index * BLOCK_US, 0.1, 40, Some(150), true);
        }
        let line = heard.named(began, now, 0.95);
        assert!(
            line.as_deref()
                .is_some_and(|line| line.contains("0 of 40 speech blocks were over it and 40 under")),
            "{line:?}"
        );
    }

    /// Blocks from 1 s, 10 ms apart, scored 0.9 against a floor of 150:
    /// `o` over it at level 170, `u` under it at level 140.
    fn blocks(heard: &mut FaintNamed, pattern: &str) {
        for (index, kind) in pattern.chars().enumerate() {
            let under = kind == 'u';
            let level = if under { 140 } else { 170 };
            heard.block(1_000_000 + index as u64 * BLOCK_US, 0.9, level, Some(150), under);
        }
    }

    /// #8728: lamp-74ff, 26 Nov 12:24:13, "2 of 22 speech blocks were under
    /// it, the loudest at -30.4 dBFS" was said as turned away, the loudest
    /// of the two under the floor given as the call's.
    #[test]
    fn a_call_mostly_over_the_floor_is_not_said_turned_away() {
        let mut heard = FaintNamed::default();
        blocks(&mut heard, "uuoooooooooooooooooooo");
        let line = heard.named(1_050_000, 1_500_000, 0.95).expect("said");
        assert!(!line.contains("turned the line away"), "{line}");
        assert!(
            line.contains(
                "the faint floor left a line free to open: 20 of 22 speech blocks were over it and 2 under, the call's loudest at -25.3 dBFS (level 170) and the loudest under it at -29.3 dBFS (level 140)"
            ),
            "{line}"
        );
        assert!(line.contains("the longest run over it was 20 blocks (a line needs 6)"), "{line}");
    }

    #[test]
    fn a_run_of_six_over_the_floor_frees_a_line_and_five_does_not() {
        let mut five = FaintNamed::default();
        blocks(&mut five, "ooooouooooouooooou");
        let line = five.named(1_050_000, 1_500_000, 0.95).expect("said");
        assert!(line.contains("but the faint floor turned the line away"), "{line}");
        assert!(line.contains("the longest run over it was 5 blocks (a line needs 6), 18 without it"), "{line}");
        let mut six = FaintNamed::default();
        blocks(&mut six, "oooooouoooooou");
        let line = six.named(1_050_000, 1_500_000, 0.95).expect("said");
        assert!(line.contains("the faint floor left a line free to open"), "{line}");
        assert!(line.contains("the longest run over it was 6 blocks"), "{line}");
    }

    /// Blocks the detector was not sure of: no line would open with the
    /// floor set aside, so the floor turned nothing away that opens lines.
    #[test]
    fn a_call_the_detector_was_unsure_of_is_not_blamed_on_the_floor() {
        let mut heard = FaintNamed::default();
        for index in 0..40u64 {
            heard.block(1_000_000 + index * BLOCK_US, 0.6, 140, Some(150), true);
        }
        let line = heard.named(1_050_000, 1_500_000, 0.95).expect("said");
        assert!(!line.contains("turned the line away"), "{line}");
        assert!(line.contains("but no line could open, floor or not"), "{line}");
        assert!(line.contains("the detector was sure of was 0 blocks"), "{line}");
    }

    #[test]
    fn levels_read_as_the_voice_records_them() {
        assert!((dbfs(150) - -28.0).abs() < 0.01);
    }
}
