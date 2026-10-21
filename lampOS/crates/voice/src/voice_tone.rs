//! How a person's voice sounds on a line, against their own way of speaking
//! (`LAMP_VOICE_TONE=on`; "Reads your mood from your face or voice", the
//! first builder on #1722 at 07:49). The voice half of the row: its arousal,
//! calm (-1) to lively (+1), from the line's prosody, and its valence, down
//! (-1) to glad (+1), from the line's own words. Ten's face reader is the
//! other half.
//!
//! **Why prosody and words, not a model:** the dimensional speech-emotion
//! model that fits a lamp (audEERING's Wav2Small) is for research only
//! without a bought licence, and the larger ones (emotion2vec+, 90M+) don't
//! fit its CPU; Hume's API is gone (the research table on #1722 at 07:48).
//! Arousal is what prosody carries well: how loud, how high, how much the
//! pitch moves and how fast the syllables come, each against the person's
//! own baseline, so a loud speaker isn't always "lively". Valence it carries
//! badly, so it comes from the words, with a small word list here: nothing
//! leaves the lamp that didn't already.
//!
//! Numbers only: no sound is kept past the line, and the words are read, not
//! kept.

use serde::Serialize;

pub const ENVIRONMENT: &str = "LAMP_VOICE_TONE";

/// Whether the switch is on: `on`, and nothing else.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// 16 kHz: the voice's capture rate.
const RATE: f32 = 16_000.0;
/// The pitch frame: 30 ms, three of the voice's 10 ms blocks.
const FRAME: usize = 480;
/// The pitches searched: 80 to 400 Hz, a speaking voice's range, as lags.
const LAG_LOW: usize = 40;
const LAG_HIGH: usize = 200;
/// A frame is voiced when its best normalized autocorrelation is at least
/// this: the usual bar for a periodic frame.
const VOICED: f32 = 0.6;
/// The shortest lag whose correlation is within this of the best is the
/// pitch, not the best itself: autocorrelation favours the octave below.
const OCTAVE_GUARD: f32 = 0.9;
/// A line too short to read: under 0.3 s of speech, or under 10 voiced
/// frames.
const FEWEST_BLOCKS: usize = 30;
const FEWEST_VOICED: usize = 10;
/// Syllable peaks in the speech's energy: at least 80 ms apart, and over
/// half again the line's median energy.
const PEAK_GAP_BLOCKS: usize = 8;
const PEAK_OVER_MEDIAN: f32 = 1.5;

/// A line's prosody, measured on its speech blocks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Tone {
    /// The median level of its speech blocks, in dBFS.
    pub level_db: f32,
    /// The median pitch of its voiced frames, in Hz.
    pub pitch_hz: f32,
    /// How much the pitch moves: the spread between its quartiles, in
    /// semitones.
    pub spread_st: f32,
    /// Syllable-like peaks of its energy, a second of speech.
    pub pace_hz: f32,
    /// Its speech blocks and voiced frames.
    pub blocks: u16,
    pub voiced: u16,
}

/// Measures a line's prosody as its speech blocks come, keeping only the
/// last 30 ms of sound and a number or two a block.
#[derive(Default)]
pub struct ToneMeter {
    frame: Vec<i16>,
    levels: Vec<f32>,
    energy: Vec<f32>,
    pitches: Vec<f32>,
}

impl ToneMeter {
    /// One 10 ms block the detector called speech.
    pub fn speech_block(&mut self, block: &[i16]) {
        if block.is_empty() {
            return;
        }
        let power = block
            .iter()
            .map(|sample| (f32::from(*sample) / 32_768.0).powi(2))
            .sum::<f32>()
            / block.len() as f32;
        self.levels.push(10.0 * power.max(1e-12).log10());
        self.energy.push(power.sqrt());
        self.frame.extend_from_slice(block);
        let over = self.frame.len().saturating_sub(FRAME);
        self.frame.drain(..over);
        if self.frame.len() == FRAME
            && let Some(pitch) = pitch_of(&self.frame)
        {
            self.pitches.push(pitch);
        }
    }

    /// The line's tone, if it was long enough to read; and the meter is
    /// ready for the next line either way.
    pub fn finish(&mut self) -> Option<Tone> {
        let levels = std::mem::take(&mut self.levels);
        let energy = std::mem::take(&mut self.energy);
        let mut pitches = std::mem::take(&mut self.pitches);
        self.frame.clear();
        if levels.len() < FEWEST_BLOCKS || pitches.len() < FEWEST_VOICED {
            return None;
        }
        let mut sorted = levels.clone();
        let level_db = median(&mut sorted);
        let pitch_hz = median(&mut pitches.clone());
        pitches.sort_by(f32::total_cmp);
        let quartile = |q: f32| pitches[((pitches.len() - 1) as f32 * q).round() as usize];
        let spread_st = 12.0 * (quartile(0.75) / quartile(0.25)).log2();
        Some(Tone {
            level_db,
            pitch_hz,
            spread_st,
            pace_hz: pace(&energy),
            blocks: u16::try_from(levels.len()).unwrap_or(u16::MAX),
            voiced: u16::try_from(pitches.len()).unwrap_or(u16::MAX),
        })
    }

    /// Lets go of the line in hand, as a line abandoned does.
    pub fn clear(&mut self) {
        let _ = self.finish();
    }
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// A 30 ms frame's pitch by normalized autocorrelation, if it is voiced:
/// the top of the first peak that comes within [`OCTAVE_GUARD`] of the
/// best, so the octave below is not taken for it, and the peak's own top,
/// not its rising edge, is the lag.
fn pitch_of(frame: &[i16]) -> Option<f32> {
    let x: Vec<f32> = frame.iter().map(|s| f32::from(*s)).collect();
    let mean = x.iter().sum::<f32>() / x.len() as f32;
    let x: Vec<f32> = x.iter().map(|v| v - mean).collect();
    let correlation = |lag: usize| {
        let (mut cross, mut early, mut late) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..x.len() - lag {
            cross += x[i] * x[i + lag];
            early += x[i] * x[i];
            late += x[i + lag] * x[i + lag];
        }
        if early <= 0.0 || late <= 0.0 {
            0.0
        } else {
            cross / (early * late).sqrt()
        }
    };
    let scores: Vec<(usize, f32)> = (LAG_LOW..=LAG_HIGH)
        .map(|lag| (lag, correlation(lag)))
        .collect();
    let best = scores.iter().map(|(_, r)| *r).fold(f32::MIN, f32::max);
    if best < VOICED {
        return None;
    }
    let mut at = scores.iter().position(|(_, r)| *r >= best * OCTAVE_GUARD)?;
    while scores
        .get(at + 1)
        .is_some_and(|(_, next)| *next >= scores[at].1)
    {
        at += 1;
    }
    Some(RATE / scores[at].0 as f32)
}

/// Syllable-like peaks a second: local peaks of the smoothed energy at
/// least [`PEAK_GAP_BLOCKS`] apart and over [`PEAK_OVER_MEDIAN`] of its
/// median.
fn pace(energy: &[f32]) -> f32 {
    if energy.len() < 3 {
        return 0.0;
    }
    let smooth: Vec<f32> = (0..energy.len())
        .map(|i| {
            let from = i.saturating_sub(1);
            let to = (i + 2).min(energy.len());
            energy[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect();
    let floor = median(&mut smooth.clone()) * PEAK_OVER_MEDIAN;
    let mut peaks = 0usize;
    let mut last: Option<usize> = None;
    for i in 1..smooth.len() - 1 {
        if smooth[i] > floor
            && smooth[i] >= smooth[i - 1]
            && smooth[i] > smooth[i + 1]
            && last.is_none_or(|last| i - last >= PEAK_GAP_BLOCKS)
        {
            peaks += 1;
            last = Some(i);
        }
    }
    peaks as f32 / (energy.len() as f32 * 0.01)
}

/// How far each measure must move from the baseline to count as one step:
/// 6 dB of level, 3 semitones of pitch, 2 of spread, 1.5 syllables a second.
const LEVEL_STEP_DB: f32 = 6.0;
const PITCH_STEP_ST: f32 = 3.0;
const SPREAD_STEP_ST: f32 = 2.0;
const PACE_STEP_HZ: f32 = 1.5;
/// The baseline settles over this many lines, and is sure after this many.
const SETTLES_OVER: f32 = 10.0;
const SURE_AFTER: f32 = 5.0;

/// A person's own way of speaking: each measure's running middle, over
/// their lines so far.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Baseline {
    lines: u32,
    level_db: f32,
    pitch_st: f32,
    spread_st: f32,
    pace_hz: f32,
}

/// What a line's tone says against the baseline, before it is taken in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Reading {
    /// Calm (-1) to lively (+1).
    pub arousal: f32,
    /// How sure: the baseline's lines and the line's voiced frames.
    pub sure: f32,
}

/// A pitch in semitones over 100 Hz, so a move is the same at any voice.
fn semitones(hz: f32) -> f32 {
    12.0 * (hz / 100.0).log2()
}

impl Baseline {
    /// How `tone` sits against this baseline, then the baseline takes it
    /// in. The first line says nothing (`sure` 0) and sets it.
    pub fn hear(&mut self, tone: &Tone) -> Reading {
        let pitch_st = semitones(tone.pitch_hz);
        let reading = if self.lines == 0 {
            Reading {
                arousal: 0.0,
                sure: 0.0,
            }
        } else {
            let steps = 0.35 * (tone.level_db - self.level_db) / LEVEL_STEP_DB
                + 0.30 * (pitch_st - self.pitch_st) / PITCH_STEP_ST
                + 0.20 * (tone.spread_st - self.spread_st) / SPREAD_STEP_ST
                + 0.15 * (tone.pace_hz - self.pace_hz) / PACE_STEP_HZ;
            Reading {
                arousal: steps.tanh(),
                sure: (self.lines as f32 / SURE_AFTER).min(1.0)
                    * (f32::from(tone.voiced) / 30.0).min(1.0),
            }
        };
        self.lines = self.lines.saturating_add(1);
        let weight = 1.0 / (self.lines as f32).min(SETTLES_OVER);
        let toward = |from: f32, to: f32| from + (to - from) * weight;
        self.level_db = toward(self.level_db, tone.level_db);
        self.pitch_st = toward(self.pitch_st, pitch_st);
        self.spread_st = toward(self.spread_st, tone.spread_st);
        self.pace_hz = toward(self.pace_hz, tone.pace_hz);
        reading
    }

    /// The lines taken in so far.
    pub fn lines(&self) -> u32 {
        self.lines
    }
}

/// What `voice/tone` says of the line `request`: its reading and its words'
/// valence, if any.
pub fn told(request: u64, reading: Reading, valence: Option<f32>) -> lamp_msgs::VoiceTone {
    lamp_msgs::VoiceTone {
        request,
        arousal: reading.arousal,
        sure: reading.sure,
        valence,
    }
}

/// Words that say a line is glad or down, and words that turn them: a small
/// list of the common ones, so the valence needs no model and no words
/// leave the lamp. English only.
const GLAD: &[&str] = &[
    "glad",
    "happy",
    "great",
    "good",
    "love",
    "lovely",
    "wonderful",
    "excited",
    "amazing",
    "awesome",
    "fantastic",
    "nice",
    "fun",
    "thanks",
    "thank",
    "yay",
    "brilliant",
    "perfect",
    "enjoy",
    "enjoyed",
    "proud",
    "relieved",
    "beautiful",
    "cool",
    "best",
    "won",
    "finally",
];
const DOWN: &[&str] = &[
    "sad",
    "tired",
    "bad",
    "awful",
    "terrible",
    "hate",
    "angry",
    "upset",
    "worried",
    "stressed",
    "annoyed",
    "lonely",
    "sick",
    "hurt",
    "exhausted",
    "frustrated",
    "scared",
    "afraid",
    "anxious",
    "miserable",
    "depressed",
    "horrible",
    "worst",
    "ugh",
    "sorry",
    "lost",
    "failed",
    "crying",
    "cry",
    "bored",
    "nervous",
];
const NOT: &[&str] = &[
    "not", "no", "never", "don't", "dont", "isn't", "isnt", "wasn't", "wasnt", "can't", "cant",
    "didn't", "didnt", "aren't", "arent", "won't", "wont",
];

/// The line's valence from its words, down (-1) to glad (+1), if any word
/// says it; a turning word just before one ("not good") turns it.
pub fn valence_of(words: &str) -> Option<f32> {
    let words: Vec<String> = words
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut sum = 0.0f32;
    let mut said = 0u32;
    for (at, word) in words.iter().enumerate() {
        let sign = if GLAD.contains(&word.as_str()) {
            1.0
        } else if DOWN.contains(&word.as_str()) {
            -1.0
        } else {
            continue;
        };
        let turned = at
            .checked_sub(1)
            .and_then(|before| words.get(before))
            .is_some_and(|before| NOT.contains(&before.as_str()))
            || at
                .checked_sub(2)
                .and_then(|before| words.get(before))
                .is_some_and(|before| NOT.contains(&before.as_str()));
        sum += if turned { -0.5 * sign } else { sign };
        said += 1;
    }
    (said > 0).then(|| (sum / said as f32).clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voiced line: a pitch that moves by `swing` semitones about `hz`,
    /// at `db` dBFS, with syllables `pace` times a second, for `seconds`.
    fn voice(hz: f32, swing: f32, db: f32, pace: f32, seconds: f32) -> Vec<Vec<i16>> {
        let amplitude = 32_768.0 * 10f32.powf(db / 20.0) * std::f32::consts::SQRT_2;
        let samples = (RATE * seconds) as usize;
        let mut phase = 0.0f32;
        let line: Vec<i16> = (0..samples)
            .map(|i| {
                let t = i as f32 / RATE;
                let f = hz * 2f32.powf(swing / 12.0 * (2.0 * std::f32::consts::PI * 0.7 * t).sin());
                phase += 2.0 * std::f32::consts::PI * f / RATE;
                // Syllables: the level rises and falls `pace` times a second.
                let syllable = 0.55 + 0.45 * (2.0 * std::f32::consts::PI * pace * t).sin();
                let wave = phase.sin() + 0.5 * (2.0 * phase).sin() + 0.25 * (3.0 * phase).sin();
                (amplitude * syllable * wave / 1.75).round() as i16
            })
            .collect();
        line.chunks(160).map(<[i16]>::to_vec).collect()
    }

    fn tone_of(blocks: &[Vec<i16>]) -> Option<Tone> {
        let mut meter = ToneMeter::default();
        for block in blocks {
            meter.speech_block(block);
        }
        meter.finish()
    }

    /// The meter reads a voice's pitch, its spread, its level and its pace
    /// near what was made; a line too short, or unvoiced, says nothing.
    #[test]
    fn a_lines_pitch_spread_level_and_pace_are_read() {
        let tone = tone_of(&voice(120.0, 0.0, -26.0, 4.0, 1.5)).unwrap();
        assert!((tone.pitch_hz - 120.0).abs() < 2.0, "{tone:?}");
        assert!(tone.spread_st < 0.6, "{tone:?}");
        assert!((tone.pace_hz - 4.0).abs() < 1.2, "{tone:?}");
        // Under the level made: the syllables' rise and fall and the voice's
        // harmonics take some of it.
        assert!((-40.0..-26.0).contains(&tone.level_db), "{tone:?}");
        let moving = tone_of(&voice(200.0, 4.0, -20.0, 6.0, 1.5)).unwrap();
        assert!((moving.pitch_hz - 200.0).abs() < 25.0, "{moving:?}");
        assert!(moving.spread_st > 2.0, "{moving:?}");
        // 6 dB louder as made, 6 dB louder as read.
        assert!(
            (moving.level_db - tone.level_db - 6.0).abs() < 1.0,
            "{moving:?}"
        );
        assert!(moving.pace_hz > tone.pace_hz, "{moving:?}");
        assert_eq!(
            tone_of(&voice(120.0, 0.0, -26.0, 4.0, 0.2)),
            None,
            "too short"
        );
        // A hiss: xorshift noise, no period in it.
        let mut state = 0x9e37_79b9u32;
        let hiss: Vec<Vec<i16>> = (0..100)
            .map(|_| {
                (0..160)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state >> 16) as i16 / 16
                    })
                    .collect()
            })
            .collect();
        assert_eq!(tone_of(&hiss), None, "nothing voiced");
    }

    /// Against a person's own calm lines, a livelier line reads lively and a
    /// calmer one calm; the first line says nothing, and the baseline is
    /// sure only after a few lines.
    #[test]
    fn a_line_reads_lively_or_calm_against_the_persons_own_way() {
        let calm = tone_of(&voice(120.0, 1.0, -26.0, 4.0, 1.5)).unwrap();
        let mut baseline = Baseline::default();
        let first = baseline.hear(&calm);
        assert_eq!((first.arousal, first.sure), (0.0, 0.0));
        for _ in 0..5 {
            baseline.hear(&calm);
        }
        let lively = tone_of(&voice(170.0, 4.0, -18.0, 6.0, 1.5)).unwrap();
        let reading = baseline.clone().hear(&lively);
        assert!(reading.arousal > 0.5, "{reading:?}");
        assert!(reading.sure > 0.9, "{reading:?}");
        let quiet = tone_of(&voice(105.0, 0.3, -34.0, 3.0, 1.5)).unwrap();
        assert!(baseline.clone().hear(&quiet).arousal < -0.3);
        assert!(baseline.clone().hear(&calm).arousal.abs() < 0.1);
        // A loud speaker's own loud lines are their calm.
        let mut loud = Baseline::default();
        let theirs = tone_of(&voice(170.0, 4.0, -18.0, 6.0, 1.5)).unwrap();
        for _ in 0..6 {
            loud.hear(&theirs);
        }
        assert!(loud.hear(&theirs).arousal.abs() < 0.1);
    }

    /// The words' valence: glad and down words, turned by a "not" just
    /// before them; nothing when no word says it.
    #[test]
    fn a_lines_words_say_glad_or_down() {
        assert_eq!(valence_of("I'm so happy today!"), Some(1.0));
        assert_eq!(valence_of("I'm really tired and stressed."), Some(-1.0));
        assert!(valence_of("I'm not happy about it").unwrap() < 0.0);
        assert!(valence_of("It's not bad at all").unwrap() > 0.0);
        assert_eq!(valence_of("What time is it?"), None);
        assert_eq!(valence_of(""), None);
        let mixed = valence_of("Great news, but I'm tired").unwrap();
        assert!(mixed.abs() < 0.01, "{mixed}");
    }

    #[test]
    fn only_on_turns_it_on() {
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on(value), "{value:?}");
        }
        assert!(on(Some("on")));
    }
}
