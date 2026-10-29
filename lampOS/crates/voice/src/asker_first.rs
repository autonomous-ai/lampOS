//! While the lamp has an answer in hand for someone, that person comes first
//! (`LAMP_ASKER_FIRST=on`, #829).
//!
//! Why: on lamp-4ace on 2026-10-16 at 14:21, in the busy office after
//! lunch, "Hey Lamp, I got the job today!" was answered by nothing for
//! 4.7 s, and then "¿Qué?" from across the room was taken as a new request
//! and replaced it (`user_interrupted`). The busy room's floor hold keeps
//! the turn for only 1.8 s after the asker stops. A person who has been
//! asked something goes on with whoever asked it, and leaves the talk
//! across the room alone.
//!
//! **The rule.** While an answer is awaited, the asker's speech has ended
//! and the lamp is not sounding, for at most [`KEEPS_FOR_US`] after the
//! asker stopped, new speech takes the turn only if it is about as loud at
//! the microphone as the asker was: no more than [`MARGIN`] under the
//! middle of the asker's own speech blocks. Quieter than that, it is
//! someone else's talk: it takes no turn, and the lamp keeps the answer.
//! The lamp's name heard (`name/rising`) still takes the turn. Over the
//! lamp's own voice nothing changes: talk from across the room is mostly
//! cancelled with the echo there, and the duplex rule judges what is left.
//!
//! - **The asker's level** is the middle of their own turn's speech blocks
//!   while they said it, none within [`ECHO_TAIL_US`] of the lamp's sound,
//!   so neither the lamp's echo nor talk before the turn counts (the
//!   reviewer on #1092: the median of every speech block in the last 8 s
//!   had both).
//! - **New speech is judged on its own blocks**, from its own start, never
//!   on talk just before it. Speech well under the asker waits as long as
//!   its candidate can (until its deadline, its oldest block's read
//!   history, or its end is near), kept as it is: nothing of it is thrown
//!   away, so if it then takes the turn (the name came, or louder speech)
//!   it begins at its start. On the reviewer's probe on #1092 an asker's follow-up
//!   that opened softly ("um, and…") lost 0.4 s of its start, and the asker
//!   just after someone across the room lost 0.22 s, because a judgement
//!   at 60 ms over the last half second cleared them. While the rule waits
//!   on speech, the answer's first block waits for its judgement, no later
//!   than its last chance, so it is judged in the clear: kept then, it is
//!   let go and the answer plays (left to the duplex rule, it was admitted
//!   a block into the answer and replaced it; judged over the answer's
//!   start, the asker's soft "um" was let go on that alone).
//! - **It ends** [`KEEPS_FOR_US`] after the asker stopped, played or not:
//!   an answer held back never plays, and a person who took their question
//!   back ("never mind") is heard again then. A line it kept is kept until
//!   it pauses ([`LINE_PAUSE_US`]), so it never takes the turn halfway,
//!   for at most [`LINE_MOST_US`] past the 5 s.
//! - **Off, record or on** ([`Mode`]): `record` judges and writes the same
//!   record but never keeps the turn, so the margin can be set from the
//!   lamp's own levels before the rule acts (the first builder at 21:35).
//!
//! Levels are on the face's scale (`ring_face::level_of`): 0 is -48 dBFS
//! or under, 255 is -14 dBFS or over, 7.5 to a decibel.

use std::collections::VecDeque;

/// Where the coordinator reads whether the rule is applied: `on`, or
/// `record` to judge without acting.
pub const ENVIRONMENT: &str = "LAMP_ASKER_FIRST";

/// How much quieter than the asker new speech may be and still take the
/// turn: 6 dB. Not yet measured on the voice's microphone: a start from
/// the room microphone's rises in `docs/ears.md`, 19.5 to 26.5 dB for
/// people at the desk and 9.8 to 15.1 dB for people farther off. Those
/// ranges overlap (desk talk to someone else, 14.7 to 23.5 dB), which is
/// why `record` comes first.
pub const MARGIN: u8 = 45;

/// The rule holds this long after the asker's speech ended, and no longer.
/// On lamp-4ace at 14:21 the sound from across the room came 4.7 s after
/// the asker, with the answer still not begun. The busy room's floor hold
/// stops at 1.8 s for the same reason this does: an answer that never
/// plays must not leave the lamp deaf to the people at it.
pub const KEEPS_FOR_US: u64 = 5_000_000;

/// A line the rule kept goes on being kept until it pauses this long, past
/// [`KEEPS_FOR_US`] too: someone who is ignoring a sentence doesn't start
/// listening halfway through it. On the reviewer's probe on #1092 a line
/// across the room begun 4.7 s after the asker was kept at its start, and
/// its next candidate took the turn at 5 s, 0.29 s into it.
pub const LINE_PAUSE_US: u64 = 300_000;

/// A kept line keeps the rule at most this long past [`KEEPS_FOR_US`], and
/// then it ends though the line goes on: steady talk across the room that
/// never pauses for 0.3 s would otherwise keep it with no end (the first
/// builder on #1092). A line begun near the 5 s has 2 s more to end; the
/// rule never holds past 7 s after the asker stopped.
pub const LINE_MOST_US: u64 = 2_000_000;

/// Fewer speech blocks than this say nothing of a level: as many as begin
/// an utterance.
pub const ENOUGH_BLOCKS: usize = 6;

/// A speech block this soon after the lamp's sound may be its echo, and is
/// not the asker's level.
pub const ECHO_TAIL_US: u64 = 300_000;

/// Speech blocks kept, at most: four seconds' worth for each of the two.
const MOST_BLOCKS: usize = 400;

/// What the environment asks of the rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    /// Judge and record, never keep the turn.
    Record,
    On,
}

impl Mode {
    pub fn from_environment() -> Self {
        Self::from(std::env::var(ENVIRONMENT).ok().as_deref())
    }

    fn from(value: Option<&str>) -> Self {
        match value {
            Some("on") => Self::On,
            Some("record") => Self::Record,
            _ => Self::Off,
        }
    }

    pub fn applies(self) -> bool {
        self != Self::Off
    }
}

/// The asker's level and what has been heard since their speech ended.
#[derive(Debug, Default)]
pub struct AskerFirst {
    /// The turn being said and its own speech levels, away from the lamp's
    /// sound.
    asking: Option<(u64, VecDeque<u8>)>,
    asker: Option<Asker>,
    /// Speech blocks heard since, with when each was read.
    since: VecDeque<(u64, u8)>,
    /// The lamp's name was heard beginning after the asker's speech ended.
    named: bool,
    /// The turn the rule last kept, so that it is said once.
    kept: Option<u64>,
    /// The newest speech block of a line the rule kept, while it goes on.
    line_us: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct Asker {
    turn: u64,
    level: u8,
    ended_at_us: u64,
}

/// What the rule kept the turn for: the asker's level, the new speech's
/// middle and upper tenth, and how many of its blocks were judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kept {
    pub asker: u8,
    pub heard: u8,
    pub upper: u8,
    pub blocks: usize,
}

/// How loud new speech has been against the asker: the asker's level, the
/// speech's middle and upper tenth, and how many of its blocks were heard
/// (`motion_voice`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levels {
    pub asker: u8,
    pub heard: u8,
    pub upper: u8,
    pub blocks: usize,
}

/// What becomes of new speech while an answer is awaited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgement {
    /// It takes the turn, as without the rule.
    Take,
    /// Too little of it yet: it waits, kept as it is.
    Wait,
    /// Well under the asker: it takes no turn.
    Keep(Kept),
}

/// A level on the face's scale in dBFS: 0 is -48 dBFS, 7.5 to a decibel.
pub fn level_db(level: u8) -> f32 {
    -48.0 + f32::from(level) / 7.5
}

fn median(mut levels: Vec<u8>) -> u8 {
    levels.sort_unstable();
    levels[levels.len() / 2]
}

/// The middle and the upper tenth of nonempty `levels`.
fn middle_and_upper(mut levels: Vec<u8>) -> (u8, u8) {
    levels.sort_unstable();
    (levels[levels.len() / 2], levels[levels.len() * 9 / 10])
}

impl AskerFirst {
    /// A speech block of `turn` at `level`, heard while it is being said,
    /// away from the lamp's sound.
    pub fn asking(&mut self, turn: u64, level: u8) {
        match &mut self.asking {
            Some((of, levels)) if *of == turn => {
                if levels.len() == MOST_BLOCKS {
                    levels.pop_front();
                }
                levels.push_back(level);
            }
            _ => self.asking = Some((turn, VecDeque::from([level]))),
        }
    }

    /// `turn`'s speech ended at `ended_at_us`. Its level, the middle of its
    /// own speech blocks, if enough were heard; with too few, nothing is
    /// judged against it.
    pub fn asked(&mut self, turn: u64, ended_at_us: u64) -> Option<u8> {
        let level = match self.asking.take() {
            Some((of, levels)) if of == turn && levels.len() >= ENOUGH_BLOCKS => {
                Some(median(levels.into_iter().collect()))
            }
            _ => None,
        };
        self.asker = level.map(|level| Asker {
            turn,
            level,
            ended_at_us,
        });
        self.since.clear();
        self.named = false;
        self.line_us = None;
        level
    }

    /// The level of `turn`'s asker, the middle of their own speech blocks,
    /// when it was measured for that turn ([`Self::asked`]); `None` for
    /// another turn's or none.
    pub fn level_of(&self, turn: u64) -> Option<u8> {
        self.asker
            .filter(|asker| asker.turn == turn)
            .map(|asker| asker.level)
    }

    /// A speech block heard, at `level`, while an answer is awaited with
    /// the lamp silent.
    pub fn heard(&mut self, at_us: u64, level: u8) {
        if self.asker.is_none_or(|asker| at_us <= asker.ended_at_us) {
            return;
        }
        if self.since.len() == MOST_BLOCKS {
            self.since.pop_front();
        }
        self.since.push_back((at_us, level));
        if self
            .line_us
            .is_some_and(|last| at_us.saturating_sub(last) <= LINE_PAUSE_US)
        {
            self.line_us = Some(at_us);
        }
    }

    /// The rule kept new speech at `now_us`: what follows it without a
    /// pause is the same line, and is kept too ([`LINE_PAUSE_US`]).
    pub fn kept_line(&mut self, now_us: u64) {
        self.line_us = Some(now_us);
    }

    /// The lamp's name was heard beginning at `began_us`.
    pub fn named(&mut self, began_us: u64) {
        if self.asker.is_some_and(|asker| began_us > asker.ended_at_us) {
            self.named = true;
        }
    }

    /// What becomes of new speech that began at `from_us`, heard by
    /// `now_us`, while `turn`'s answer is awaited. `last_chance`: its
    /// candidate cannot wait longer, so it is judged on what there is.
    /// Speech whose upper tenth is about as loud as the asker takes the
    /// turn as soon as enough is heard to say so: the asker going on waits
    /// no longer than that, and a soft first word ("um, and…") before their
    /// usual voice does not keep them out. Speech nearly all well under the
    /// asker waits as long as its candidate can, so that the lamp's name
    /// may still come (announced 78 to 639 ms after it begins,
    /// `docs/name.md`), and is kept from the turn only at its last chance.
    pub fn judge(&self, turn: u64, from_us: u64, now_us: u64, last_chance: bool) -> Judgement {
        let Some(asker) = self.asker.filter(|asker| asker.turn == turn) else {
            return Judgement::Take;
        };
        let since_us = now_us.saturating_sub(asker.ended_at_us);
        let in_line = self
            .line_us
            .is_some_and(|last| now_us.saturating_sub(last) <= LINE_PAUSE_US)
            && since_us <= KEEPS_FOR_US + LINE_MOST_US;
        if self.named || (since_us > KEEPS_FOR_US && !in_line) {
            return Judgement::Take;
        }
        let levels: Vec<u8> = self
            .since
            .iter()
            .filter(|(at, _)| *at >= from_us && *at <= now_us)
            .map(|(_, level)| *level)
            .collect();
        let blocks = levels.len();
        if blocks < ENOUGH_BLOCKS {
            return if last_chance {
                Judgement::Take
            } else {
                Judgement::Wait
            };
        }
        let (heard, upper) = middle_and_upper(levels);
        if upper >= asker.level.saturating_sub(MARGIN) {
            Judgement::Take
        } else if !last_chance {
            Judgement::Wait
        } else {
            Judgement::Keep(Kept {
                asker: asker.level,
                heard,
                upper,
                blocks,
            })
        }
    }

    /// How loud the speech that began at `from_us`, heard by `now_us`, has
    /// been against `turn`'s asker: none without the asker's level for that
    /// turn, or with no speech block of its own yet. It judges nothing and
    /// keeps nothing (`motion_voice`, #1712's B).
    pub fn levels_since(&self, turn: u64, from_us: u64, now_us: u64) -> Option<Levels> {
        let asker = self.asker.filter(|asker| asker.turn == turn)?;
        let levels: Vec<u8> = self
            .since
            .iter()
            .filter(|(at, _)| *at >= from_us && *at <= now_us)
            .map(|(_, level)| *level)
            .collect();
        if levels.is_empty() {
            return None;
        }
        let blocks = levels.len();
        let (heard, upper) = middle_and_upper(levels);
        Some(Levels {
            asker: asker.level,
            heard,
            upper,
            blocks,
        })
    }

    /// Whether the rule has not yet said it kept `turn`; after this, it has.
    pub fn first_for(&mut self, turn: u64) -> bool {
        self.kept.replace(turn) != Some(turn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000;

    /// Speech blocks every 10 ms from `from_ms`, at `level`.
    fn speech(rule: &mut AskerFirst, from_ms: u64, blocks: u64, level: u8) -> u64 {
        for block in 0..blocks {
            rule.heard((from_ms + block * 10) * MS, level);
        }
        (from_ms + blocks * 10) * MS
    }

    /// The asker said `turn` at `level` for a second, ending at `ended_ms`.
    fn asked(rule: &mut AskerFirst, turn: u64, level: u8, ended_ms: u64) {
        for _ in 0..100 {
            rule.asking(turn, level);
        }
        assert_eq!(rule.asked(turn, ended_ms * MS), Some(level));
    }

    /// `motion_voice`'s reading (#1712's B): the levels of speech begun at
    /// a time against the turn's asker, only its own blocks, judging
    /// nothing; none for another turn, before the asker's level, or with no
    /// block of its own yet.
    #[test]
    fn levels_since_reads_its_own_blocks_against_the_asker() {
        let mut rule = AskerFirst::default();
        assert_eq!(rule.levels_since(1, 0, 10_000 * MS), None);
        asked(&mut rule, 1, 150, 1_000);
        assert_eq!(rule.levels_since(1, 1_100 * MS, 2_000 * MS), None);
        // Room talk before it, then its own blocks.
        speech(&mut rule, 1_100, 10, 60);
        let end = speech(&mut rule, 1_300, 10, 140);
        let levels = rule.levels_since(1, 1_300 * MS, end).unwrap();
        assert_eq!(
            levels,
            Levels {
                asker: 150,
                heard: 140,
                upper: 140,
                blocks: 10
            }
        );
        assert_eq!(rule.levels_since(2, 1_300 * MS, end), None);
        // It judges nothing: the rule's own record is untouched.
        assert!(rule.first_for(1));
    }

    #[test]
    fn on_or_record_switches_it_on() {
        assert_eq!(Mode::from(Some("on")), Mode::On);
        assert_eq!(Mode::from(Some("record")), Mode::Record);
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert_eq!(Mode::from(value), Mode::Off, "{value:?}");
        }
        assert!(Mode::On.applies() && Mode::Record.applies() && !Mode::Off.applies());
    }

    /// 14:21 on lamp-4ace: the answer to the person at the desk is awaited,
    /// and someone across the room, well under them, says "¿Qué?". It waits
    /// until a quarter of a second of it is heard, then takes no turn. The
    /// asker, as loud as before, takes it as soon as six blocks are heard.
    #[test]
    fn quieter_talk_takes_no_turn_from_the_answer_and_the_asker_still_does() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 7, 180, 1_000);
        let quiet = 180 - MARGIN - 10;
        let now = speech(&mut rule, 2_000, 10, quiet);
        assert_eq!(rule.judge(7, 2_000 * MS, now, false), Judgement::Wait);
        // It waits as long as its candidate can: its name may still come.
        let now = speech(&mut rule, 2_100, 15, quiet);
        assert_eq!(rule.judge(7, 2_000 * MS, now, false), Judgement::Wait);
        // At its last chance, it takes no turn.
        assert_eq!(
            rule.judge(7, 2_000 * MS, now, true),
            Judgement::Keep(Kept {
                asker: 180,
                heard: quiet,
                upper: quiet,
                blocks: 25
            })
        );
        // Only for the turn it was asked in.
        assert_eq!(rule.judge(8, 2_000 * MS, now, false), Judgement::Take);
        // The asker again, a little quieter than before: within the margin,
        // and taken at once.
        let now = speech(&mut rule, 3_000, 6, 180 - MARGIN + 5);
        assert_eq!(rule.judge(7, 3_000 * MS, now, false), Judgement::Take);
    }

    /// The reviewer's probe on #1092: speech is judged on its own blocks.
    /// Someone across the room for half a second, then the asker 0.1 s
    /// later: the asker's own blocks take the turn. And the asker's own
    /// follow-up that opens softly ("um, and…") waits, and is taken once
    /// it is at their level.
    #[test]
    fn new_speech_is_judged_on_its_own_blocks_and_a_soft_start_waits() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 2, 178, 1_000);
        speech(&mut rule, 1_500, 50, 54);
        let now = speech(&mut rule, 2_100, 6, 178);
        assert_eq!(rule.judge(2, 2_100 * MS, now, false), Judgement::Take);
        let mut rule = AskerFirst::default();
        asked(&mut rule, 2, 178, 1_000);
        let now = speech(&mut rule, 2_500, 20, 54);
        assert_eq!(rule.judge(2, 2_500 * MS, now, false), Judgement::Wait);
        // Five blocks at their level make the upper tenth theirs.
        let now = speech(&mut rule, 2_700, 5, 178);
        assert_eq!(rule.judge(2, 2_500 * MS, now, false), Judgement::Take);
    }

    /// The reviewer's P3 on #1092: the asker leaning back from 30 to 60 cm
    /// is about 6 dB down; 8 dB down, they take no turn. That is the rule's
    /// cost.
    #[test]
    fn the_asker_leaning_back_8_db_takes_no_turn() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 3, 178, 1_000);
        let now = speech(&mut rule, 2_000, 30, 178 - 60);
        assert!(matches!(
            rule.judge(3, 2_000 * MS, now, true),
            Judgement::Keep(_)
        ));
    }

    /// Too little heard to judge: it waits, and when its time is up it
    /// takes the turn, as without the rule. Nothing is judged at all past
    /// `KEEPS_FOR_US`, nor with too few of the asker's own blocks.
    #[test]
    fn too_little_heard_waits_and_the_rule_ends_after_its_time() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 3, 200, 1_000);
        // Heard before the asker's speech ended: the asker's own.
        speech(&mut rule, 900, 10, 20);
        assert_eq!(rule.judge(3, 900 * MS, 1_000 * MS, false), Judgement::Wait);
        let now = speech(&mut rule, 1_500, ENOUGH_BLOCKS as u64 - 1, 20);
        assert_eq!(rule.judge(3, 1_500 * MS, now, false), Judgement::Wait);
        assert_eq!(rule.judge(3, 1_500 * MS, now, true), Judgement::Take);
        // At its last chance, enough blocks well under: kept.
        let now = speech(&mut rule, 1_550, 5, 20);
        assert!(matches!(
            rule.judge(3, 1_500 * MS, now, true),
            Judgement::Keep(_)
        ));
        // Past the rule's time, whatever is heard takes the turn.
        let late = 1_000 * MS + KEEPS_FOR_US + MS;
        let now = speech(&mut rule, late / MS, 30, 20);
        assert_eq!(rule.judge(3, late, now, false), Judgement::Take);
        // An asker with too few blocks of their own: no rule.
        let mut rule = AskerFirst::default();
        rule.asking(4, 200);
        assert_eq!(rule.asked(4, 1_000 * MS), None);
        let now = speech(&mut rule, 1_500, 30, 20);
        assert_eq!(rule.judge(4, 1_500 * MS, now, false), Judgement::Take);
    }

    /// The asker's level is their own turn's: blocks of another turn are
    /// not counted.
    #[test]
    fn the_askers_level_is_their_own_turns() {
        let mut rule = AskerFirst::default();
        for _ in 0..50 {
            rule.asking(1, 250);
        }
        for _ in 0..20 {
            rule.asking(2, 120);
        }
        assert_eq!(rule.asked(2, 1_000 * MS), Some(120));
    }

    /// "Lamp, …" said quietly takes the turn: the name heard after the
    /// asker's speech ended. One heard before then counts for nothing.
    #[test]
    fn the_name_said_after_the_asker_takes_the_turn() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 4, 200, 1_000);
        rule.named(900 * MS);
        let now = speech(&mut rule, 1_500, 30, 60);
        assert!(matches!(
            rule.judge(4, 1_500 * MS, now, true),
            Judgement::Keep(_)
        ));
        rule.named(1_510 * MS);
        assert_eq!(rule.judge(4, 1_500 * MS, now, true), Judgement::Take);
        // A new request begins afresh.
        asked(&mut rule, 5, 200, 2_000);
        let now = speech(&mut rule, 2_500, 30, 60);
        assert!(matches!(
            rule.judge(5, 2_500 * MS, now, true),
            Judgement::Keep(_)
        ));
    }

    /// The first builder on #1092: the name is announced 78 to 639 ms after
    /// it begins. A quiet "Lamp, …" waits for it while its candidate can,
    /// and is taken when it comes.
    #[test]
    fn a_quiet_line_waits_for_its_name() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 6, 200, 1_000);
        let now = speech(&mut rule, 2_000, 30, 60);
        assert_eq!(rule.judge(6, 2_000 * MS, now, false), Judgement::Wait);
        rule.named(2_000 * MS + 50 * MS);
        let now = speech(&mut rule, 2_300, 15, 60);
        assert_eq!(rule.judge(6, 2_000 * MS, now, false), Judgement::Take);
    }

    /// The reviewer on #1092: a line kept near the end of the 5 s is kept
    /// until it pauses, and is never taken halfway through; after its
    /// pause, later speech is heard again.
    #[test]
    fn a_kept_line_is_kept_until_it_pauses_past_the_five_seconds() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 9, 178, 1_000);
        // Begun 4.7 s after the asker, kept at its last chance.
        let begin = 1_000 + 4_700;
        let now = speech(&mut rule, begin, 20, 54);
        assert!(matches!(
            rule.judge(9, begin * MS, now, true),
            Judgement::Keep(_)
        ));
        rule.kept_line(now);
        // Its next candidate, past the 5 s, in the same line: kept.
        let next = begin + 210;
        let now = speech(&mut rule, begin + 200, 30, 54);
        assert!(now > 1_000 * MS + KEEPS_FOR_US);
        assert!(matches!(
            rule.judge(9, next * MS, now, true),
            Judgement::Keep(_)
        ));
        // It pauses for half a second; what is said after is heard.
        let after = begin + 1_000;
        let now = speech(&mut rule, after, 20, 54);
        assert_eq!(rule.judge(9, after * MS, now, true), Judgement::Take);
    }

    /// The first builder on #1092: a kept line that never pauses keeps the
    /// rule 2 s past its time, and no longer.
    #[test]
    fn a_kept_line_that_never_pauses_keeps_the_rule_at_most_two_seconds_more() {
        let mut rule = AskerFirst::default();
        asked(&mut rule, 9, 178, 1_000);
        let begin = 1_000 + 4_700;
        let now = speech(&mut rule, begin, 20, 54);
        assert!(matches!(
            rule.judge(9, begin * MS, now, true),
            Judgement::Keep(_)
        ));
        rule.kept_line(now);
        // Talk across the room goes on with no pause until 8 s after the
        // asker: still kept at 6.9 s, heard again past 7 s.
        let at_6_9 = speech(&mut rule, begin + 200, 200, 54);
        assert_eq!(at_6_9, (1_000 + 6_900) * MS);
        assert!(matches!(
            rule.judge(9, (1_000 + 6_700) * MS, at_6_9, true),
            Judgement::Keep(_)
        ));
        let at_7_1 = speech(&mut rule, 1_000 + 6_900, 20, 54);
        assert_eq!(
            rule.judge(9, (1_000 + 6_900) * MS, at_7_1, true),
            Judgement::Take
        );
    }

    #[test]
    fn it_says_so_once_a_turn() {
        let mut rule = AskerFirst::default();
        assert!(rule.first_for(1));
        assert!(!rule.first_for(1));
        assert!(rule.first_for(2));
    }
}
