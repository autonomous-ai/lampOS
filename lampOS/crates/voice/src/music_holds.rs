//! Music that opens a turn is held, not answered ("Not spoken to", #2602).
//!
//! The detector takes tonal music for speech at any level: of six made-up
//! tunes at three levels, 17 of 18 opened a turn as long as the tune
//! (`tests/it/music_turns.rs`), and on lamp-c0a4 a 6 s tune of two-tone
//! notes over a hum opened an admitted turn at 914 ms (Thirteen on #2602).
//! With `LAMP_MUSIC_HOLDS=on` a line whose sound `ears/classes` scored as
//! music over [`MUSIC_OVER`], with no speech over [`SPEECH_UNDER`],
//! throughout is held at its first output, as a
//! busy room's line is held, unless something shows it was said to the lamp:
//! its name, its words calling it, a face turned to it, said close to it
//! with a close face turned to it (`crate::addressed`), a tap, or a
//! conversation going on ([`crate::attention::Attention::hold_music`]).
//! Someone humming or singing to the lamp is let out by any of those (El on
//! #2602), and so is someone speaking over the music, whose words YAMNet
//! scores as speech. The room microphone's rise alone is no sign here:
//! music raises it over the room's quiet, the tenth percentile of the last
//! 30 s.
//!
//! On the same tunes, YAMNet scored music over 0.6 in every window of four
//! of them, 6 of 16 of the arpeggio's and none of a bright line's (a known
//! miss: a lone synth line is not music to it); and in none of 80 windows
//! of 40.8 s of Piper's speech, at most 0.02.
//! Off unless set, and only with `LAMP_ATTENTION`, which judges the lines,
//! and `ears/classes` on the bus (`LAMP_EARS_CLASSES=on`): with no scores,
//! nothing is held.
use lamp_msgs::{OwnVoiceEvidence, SoundClasses};
use std::collections::VecDeque;

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_MUSIC_HOLDS";
/// The workers' flag for it, after `--live-scoped-session`.
pub const FLAG: &str = "--music-holds";
/// Music over this in a window is music: Music sway's bar
/// (`skills/music-sway`).
pub const MUSIC_OVER: f32 = 0.6;
/// Speech over this in a window is someone speaking over the music, and
/// the line is not music alone. YAMNet scored speech at most 0.02 in 82
/// windows of six made-up tunes; a Piper line over the held chords, at the
/// music's level and 6 and 12 dB over it, reached 0.06 or more in 26 of 27
/// lines, and in both lines whose music stayed over 0.6 throughout (0.17
/// and 0.99). Its one line under it, 0.03, had music under 0.6 too.
pub const SPEECH_UNDER: f32 = 0.05;
/// A line is music only with this many windows read over its sound, every
/// one of them music: one window is under a second of it.
pub const LEAST_WINDOWS: usize = 2;
/// A window is of the 0.96 s before it was scored (`lamp-ears`'s patch),
/// so those read this long into the line are of its own sound.
pub const PATCH_US: u64 = 960_000;
/// Windows read up to this long after the line's sound ended still cover
/// it: scored every 500 ms, read every 100 ms.
pub const AFTER_US: u64 = 600_000;
/// A message older than this is not read: `ears/classes` lasts 1.5 s.
pub const FRESH_MS: u64 = 1_500;
/// Windows are kept this long.
const KEPT_US: u64 = 30_000_000;
/// The verdict's reason.
pub const HELD: &str = "music is playing, and nothing shows it was said to the lamp: no name, no call in its words, nobody turned to it or close and facing it, no conversation going on";

/// Whether the switch is on: `on` only.
pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The workers' argument for it, from the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if enabled(value) {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The music scores read off `ears/classes`, by when each was read.
#[derive(Default)]
pub struct Music {
    /// When each window was read, by the worker's clock, its end on the
    /// capture clock (to read each once), and its music and speech scores.
    windows: VecDeque<(u64, u64, f32, f32)>,
}

impl Music {
    /// Read the newest `ears/classes` message, sent at `sent_unix_ms`, at
    /// `now_us`: a fresh scored window not already read, and not one the
    /// lamp spoke into, is kept.
    pub fn read(
        &mut self,
        classes: &SoundClasses,
        sent_unix_ms: u64,
        now_unix_ms: u64,
        now_us: u64,
    ) {
        while self
            .windows
            .front()
            .is_some_and(|(at, ..)| now_us.saturating_sub(*at) > KEPT_US)
        {
            self.windows.pop_front();
        }
        let SoundClasses::Scored {
            window,
            scores,
            own_voice,
            ..
        } = classes
        else {
            return;
        };
        if now_unix_ms.saturating_sub(sent_unix_ms) >= FRESH_MS
            || *own_voice == OwnVoiceEvidence::Present
            || self
                .windows
                .back()
                .is_some_and(|(_, end, ..)| *end == window.end_us)
        {
            return;
        }
        self.windows
            .push_back((now_us, window.end_us, scores.music, scores.speech));
    }

    /// Whether a line whose sound began at `began` and ended at `ended`
    /// (still going at `now_us` if not) was music alone throughout: at least
    /// [`LEAST_WINDOWS`] windows of its own sound, each music over
    /// [`MUSIC_OVER`] and speech at most [`SPEECH_UNDER`]. No window read
    /// over it (the classifier off, stopped, or its messages stale) is no
    /// music.
    pub fn throughout(&self, (began, ended): (u64, Option<u64>), now_us: u64) -> bool {
        let until = ended.map_or(now_us, |ended| ended + AFTER_US);
        let mut windows = self
            .windows
            .iter()
            .filter(|(at, ..)| (began + PATCH_US..=until).contains(at))
            .peekable();
        windows.peek().is_some()
            && windows
                .try_fold(0, |n, (_, _, music, speech)| {
                    (*music > MUSIC_OVER && *speech <= SPEECH_UNDER).then_some(n + 1)
                })
                .is_some_and(|n| n >= LEAST_WINDOWS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{SoundScores, SoundWindow};

    fn scored(end_us: u64, music: f32, own_voice: OwnVoiceEvidence) -> SoundClasses {
        scored_with(end_us, (music, 0.0), own_voice)
    }

    fn scored_with(
        end_us: u64,
        (music, speech): (f32, f32),
        own_voice: OwnVoiceEvidence,
    ) -> SoundClasses {
        SoundClasses::Scored {
            model_id: "yamnet-qai-v0.59.0".into(),
            window: SoundWindow {
                start_us: end_us.saturating_sub(960_000),
                end_us,
                completed_us: end_us + 10_000,
                capture_epoch: 1,
            },
            scores: SoundScores {
                speech,
                music,
                laughter: 0.0,
            },
            own_voice,
        }
    }

    /// Windows read every 500 ms from `from_us` with these music scores.
    fn music(from_us: u64, scores: &[f32]) -> Music {
        let both = scores.iter().map(|music| (*music, 0.0)).collect::<Vec<_>>();
        music_and_speech(from_us, &both)
    }

    /// The same, with each window's music and speech.
    fn music_and_speech(from_us: u64, scores: &[(f32, f32)]) -> Music {
        let mut music = Music::default();
        for (k, both) in scores.iter().enumerate() {
            let at = from_us + k as u64 * 500_000;
            music.read(
                &scored_with(at, *both, OwnVoiceEvidence::Unknown),
                0,
                100,
                at,
            );
        }
        music
    }

    #[test]
    fn only_an_exact_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!enabled(value));
            assert!(arguments(value).is_empty());
        }
        let flags = arguments(Some("on"));
        assert_eq!(flags, [FLAG]);
        let rest = ["--next".to_owned()];
        let both = [flags, rest.to_vec()].concat();
        assert_eq!(take(&both), (true, &rest[..]));
        assert_eq!(take(&rest), (false, &rest[..]));
    }

    /// A 6 s tune from 1 s, a line from 1.3 s to 7 s: music throughout.
    /// One window under the bar, or too few, and it is not.
    #[test]
    fn a_line_is_music_only_with_every_window_of_it_over_the_bar() {
        let line = (1_300_000, Some(7_000_000));
        assert!(music(1_000_000, &[0.9; 13]).throughout(line, 8_000_000));
        let mut talk = [0.9; 13];
        talk[6] = 0.4;
        assert!(!music(1_000_000, &talk).throughout(line, 8_000_000));
        // Windows before its sound, or long after it, are not its own.
        assert!(
            !music(0, &[0.9, 0.9, 0.9, 0.2, 0.2, 0.2, 0.2, 0.2])
                .throughout((2_000_000, Some(4_000_000)), 5_000_000)
        );
        assert!(!music(8_000_000, &[0.9; 4]).throughout(line, 10_000_000));
        // One window is too short to say.
        assert!(!music(2_500_000, &[0.9]).throughout((1_300_000, Some(2_400_000)), 3_000_000));
        // Nothing read: no music.
        assert!(!Music::default().throughout(line, 8_000_000));
        // A line still going counts what was read so far.
        assert!(music(1_000_000, &[0.9; 6]).throughout((1_300_000, None), 4_000_000));
    }

    /// El on #2602: someone speaking over music is not music alone. Two
    /// Piper lines over the held chords whose music stayed over 0.6 in
    /// every window, as YAMNet scored them (`lamp-ears`'s `score_wav`, the
    /// pinned model): at the music's level, and 12 dB over it. Each has a
    /// window with speech over the bar, so neither is held; the tunes'
    /// windows, speech at 0.02 at most, are.
    #[test]
    fn speech_over_music_is_not_music_alone() {
        let line = (1_000_000, Some(3_000_000));
        let at_its_level = [(0.73, 0.17), (0.69, 0.02), (0.64, 0.01), (0.71, 0.0)];
        let over_it = [(0.74, 0.99), (0.74, 0.85), (0.90, 0.16), (0.76, 0.15)];
        let a_tune = [(0.81, 0.02), (0.92, 0.0), (0.88, 0.01), (0.95, 0.0)];
        assert!(!music_and_speech(2_000_000, &at_its_level).throughout(line, 4_000_000));
        assert!(!music_and_speech(2_000_000, &over_it).throughout(line, 4_000_000));
        assert!(music_and_speech(2_000_000, &a_tune).throughout(line, 4_000_000));
    }

    /// El on #2602: never held on old data. The classifier stopped before
    /// the line (no window read over it), or its messages stale through it:
    /// no music, whatever came before.
    #[test]
    fn with_no_fresh_scores_over_the_line_there_is_no_music() {
        let mut stopped = music(0, &[0.9; 10]);
        assert!(!stopped.throughout((6_000_000, Some(9_000_000)), 9_500_000));
        for k in 0..6u64 {
            let at = 6_000_000 + k * 500_000;
            stopped.read(&scored(at, 0.9, OwnVoiceEvidence::Unknown), 0, FRESH_MS, at);
        }
        assert!(!stopped.throughout((6_000_000, Some(9_000_000)), 9_500_000));
    }

    #[test]
    fn a_stale_or_repeated_window_or_one_the_lamp_spoke_into_is_not_read() {
        let mut music = Music::default();
        music.read(
            &scored(3_000_000, 0.9, OwnVoiceEvidence::Unknown),
            0,
            FRESH_MS,
            3_000_000,
        );
        music.read(
            &scored(3_500_000, 0.9, OwnVoiceEvidence::Present),
            0,
            100,
            3_500_000,
        );
        music.read(
            &scored(4_000_000, 0.9, OwnVoiceEvidence::Unknown),
            0,
            100,
            4_000_000,
        );
        music.read(
            &scored(4_000_000, 0.9, OwnVoiceEvidence::Unknown),
            0,
            100,
            4_100_000,
        );
        music.read(
            &SoundClasses::Unavailable {
                reason: lamp_msgs::SoundUnavailable::WarmingUp,
            },
            0,
            100,
            4_200_000,
        );
        assert_eq!(music.windows.len(), 1);
        // Kept 30 s.
        music.read(
            &scored(40_000_000, 0.9, OwnVoiceEvidence::Clear),
            0,
            100,
            40_000_000,
        );
        assert_eq!(music.windows.len(), 1);
        assert_eq!(music.windows[0].1, 40_000_000);
    }
}
