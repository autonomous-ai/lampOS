//! Local recordings explicitly requested in a spoken turn (#1685).
//!
//! Loading is bounded and runs off the coordinator. A recording alone grants
//! no output: its original spoken owner, deadline, Skills lease and speaker
//! mute generation must still hold at the final speaker boundary.

use sha2::{Digest, Sha256};
use std::path::Path;

mod runtime;
pub(crate) use runtime::{Service, Ticket};

pub const ENVIRONMENT: &str = "LAMP_REQUESTED_AUDIO";
pub const FLAG: &str = "--requested-audio";
pub const MOST_FRAMES: u32 = crate::say::SOUND_RATE * 60;
pub const MOST_BYTES: u64 = 44 + 2 * MOST_FRAMES as u64 + 4_096;

pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

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

/// What the model is told when a recording offer is refused because the line
/// that led to the call did not ask for one ([`asks_to_play`]).
pub const NOT_ASKED_TO_PLAY: &str = "Nothing is playing and nothing will: a recording plays only when what they said asks you to play it, in words such as \"play some music\" or \"can you put on something relaxing\". Talk about music is not an ask. Do not say you are playing anything or that you will; answer what they said.";

/// What may stand before the ask at a sentence's start.
const BEFORE: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "and", "oh", "now", "please", "just",
];
/// "Can you …", "Could I …", "I'd like to …", "Let's …": a lead-in to the ask.
const LEAD_INS: &[&[&str]] = &[
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["can", "i"],
    &["could", "i"],
    &["may", "i"],
    &["can", "we"],
    &["could", "we"],
    &["i", "want", "to"],
    &["i", "wanna"],
    &["i'd", "like", "to"],
    &["i", "would", "like", "to"],
    &["let's"],
    &["lets"],
    &["let", "me"],
    &["let", "us"],
];
/// The asks themselves, right after any [`BEFORE`] words and one lead-in.
const ASKS: &[&[&str]] = &[
    &["play"],
    &["put", "on"],
    &["put", "some"],
    &["put", "something"],
    &["turn", "on"],
    &["start"],
    &["hear"],
    &["listen", "to"],
    // Vietnamese, accents folded: "mở nhạc", "bật nhạc", "phát nhạc",
    // "cho tôi nghe".
    &["mo", "nhac"],
    &["bat", "nhac"],
    &["phat", "nhac"],
    &["mo", "bai"],
    &["bat", "bai"],
    &["phat", "bai"],
    &["cho", "toi", "nghe"],
    &["cho", "minh", "nghe"],
    &["cho", "em", "nghe"],
];
/// A word that, anywhere in the line, takes the ask back, doubts it or turns
/// it into something else ("don't play anything", "play some music.
/// Actually, don't.", "maybe later", "đừng mở nhạc", "thôi").
const NOT: &[&str] = &[
    "don't",
    "dont",
    "not",
    "never",
    "nevermind",
    "no",
    "stop",
    "cancel",
    "actually",
    "wait",
    "maybe",
    "dung",
    "thoi",
];
/// The recording noun must be the requested object, not a word in a later
/// clause ("turn on the light so I can read sheet music" is not an ask).
const AUDIO: &[&str] = &[
    "music",
    "song",
    "songs",
    "recording",
    "recordings",
    "sound",
    "sounds",
    "track",
    "tracks",
    "tune",
    "tunes",
    "playlist",
    "melody",
    "lullaby",
    "nhac",
    "bai",
];
/// Adjectives allowed directly before a recording noun or after "something".
const MOODS: &[&str] = &[
    "relaxing", "calm", "calming", "quiet", "soft", "soothing", "gentle", "peaceful", "chill",
];

fn starts_with_words(words: &[String], prefix: &[&str]) -> bool {
    words.len() >= prefix.len()
        && words
            .iter()
            .zip(prefix)
            .all(|(word, expected)| word == expected)
}

/// A conservative object grammar: an optional recipient and determiner,
/// then a recording noun (with one position word and at most three mood
/// adjectives), or one of
/// the explicit "something ..." constructions heard on the lamp. Never
/// skip an unrelated object, "with me", or "like chess" to find a mood.
fn audio_object(mut words: &[String]) -> bool {
    if words
        .first()
        .is_some_and(|word| matches!(word.as_str(), "me" | "us"))
    {
        words = &words[1..];
    }
    if words
        .first()
        .is_some_and(|word| matches!(word.as_str(), "a" | "an" | "some" | "the"))
    {
        words = &words[1..];
    }
    if words.first().is_some_and(|word| word == "something") {
        let rest = &words[1..];
        let used = if rest
            .first()
            .is_some_and(|word| MOODS.contains(&word.as_str()))
        {
            1
        } else if starts_with_words(rest, &["to", "help", "me"])
            && rest
                .get(3)
                .is_some_and(|word| matches!(word.as_str(), "relax" | "unwind" | "sleep"))
        {
            4
        } else if rest.first().is_some_and(|word| word == "to")
            && rest
                .get(1)
                .is_some_and(|word| matches!(word.as_str(), "relax" | "unwind" | "sleep"))
        {
            2
        } else {
            return false;
        };
        // Do not turn "something calm like chess" into a recording ask.
        return rest[used..]
            .iter()
            .all(|word| matches!(word.as_str(), "please" | "now" | "on"));
    }
    // A bounded position selects a recording, not an unrelated object
    // followed later by music or a mood. It cannot qualify "something".
    if words.first().is_some_and(|word| {
        matches!(
            word.as_str(),
            "first" | "second" | "next" | "previous" | "last" | "another" | "different"
        )
    }) {
        words = &words[1..];
    }
    for _ in 0..3 {
        if words
            .first()
            .is_some_and(|word| MOODS.contains(&word.as_str()))
        {
            words = &words[1..];
        } else {
            break;
        }
    }
    words
        .first()
        .is_some_and(|word| AUDIO.contains(&word.as_str()))
}

/// Whether `said`, the line that led to a requested-audio call, plainly asks
/// to play something (#5764). Some sentence of it must start, past greetings
/// ([`BEFORE`]) and at most one lead-in ([`LEAD_INS`]), with an ask
/// ([`ASKS`]) with a recording object ([`audio_object`]). No word of the
/// whole line may be one of [`NOT`], and "forget it" retracts the ask.
/// On lamp-52e6 on 9 Nov, "Lamp, I
/// listened to some relaxing music on the train this morning, it was really
/// nice." got a `use_skill` call to the relaxing-music skill: talk about
/// music, not an ask, so it is refused here, not left to the skill's text.
/// "Lamp, can you play some relaxing music?" and "Put on something calm,
/// please." ask.
// ponytail: an English and Vietnamese word list, not a classifier; an ask in
// another language or phrasing is refused (the model is told so, and the
// person can say it plainly). Widen ASKS when a lamp run shows one.
pub fn asks_to_play(said: &str) -> bool {
    let whole = crate::known_faces::folded_words(said);
    if whole.iter().any(|word| NOT.contains(&word.as_str()))
        || whole
            .windows(2)
            .any(|words| starts_with_words(words, &["forget", "it"]))
    {
        return false;
    }
    said.split(['.', '!', '?', ';']).any(|sentence| {
        let words = crate::known_faces::folded_words(sentence);
        let starts = |at: usize, phrase: &[&str]| {
            words.len() >= at + phrase.len()
                && words[at..at + phrase.len()]
                    .iter()
                    .zip(phrase)
                    .all(|(word, want)| word == want)
        };
        let mut at = 0;
        while words
            .get(at)
            .is_some_and(|word| BEFORE.contains(&word.as_str()))
        {
            at += 1;
        }
        if let Some(lead) = LEAD_INS.iter().find(|lead| starts(at, lead)) {
            at += lead.len();
            if words.get(at).is_some_and(|word| word == "please") {
                at += 1;
            }
        }
        ASKS.iter().any(|ask| {
            if !starts(at, ask) {
                return false;
            }
            // These prefixes already include the object's first word:
            // "put some/ something" and Vietnamese "mở nhạc / bật bài".
            let includes_object = ask
                .last()
                .is_some_and(|word| matches!(*word, "some" | "something") || AUDIO.contains(word));
            let object_at = at + ask.len() - usize::from(includes_object);
            audio_object(&words[object_at..])
        })
    })
}

/// Samples and the digest of the exact file image from which they were decoded.
/// Later edits to the path cannot change this recording's bytes.
#[derive(Debug)]
pub struct CheckedSound {
    pub samples: Vec<i16>,
    pub sha256: String,
}

/// A regular, bounded WAV with the short-cue format and amplitude limits.
/// Only its duration/byte budget is extended. Paths are checked by the Skills
/// grant issuer before this reader; callers must also compare the exact digest.
pub fn read_sound(path: &Path) -> Result<CheckedSound, String> {
    let bytes = crate::say::read_sound_bytes(path, MOST_BYTES)?;
    let samples = crate::say::decode_sound_with_limit(&bytes, MOST_FRAMES)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(CheckedSound { samples, sha256 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Cursor};

    fn wav(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut out,
                hound::WavSpec {
                    channels,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for sample in samples {
                writer.write_sample(*sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn only_exact_on_is_forwarded_and_the_following_worker_flags_survive() {
        for value in [None, Some(""), Some("true"), Some("ON"), Some("on ")] {
            assert!(!enabled(value));
            assert!(arguments(value).is_empty());
        }
        let mut flags = arguments(Some("on"));
        flags.push("--after".to_owned());
        let (settings, rest) = crate::tools::take(&flags).unwrap();
        assert!(settings.requested_audio);
        assert_eq!(rest, ["--after"]);
        assert!(!crate::tools::Settings::default().requested_audio);
    }

    #[test]
    fn the_duration_extension_does_not_extend_autonomous_cues() {
        let bytes = wav(&vec![100; crate::say::SOUND_RATE as usize * 4], 24_000, 1);
        assert!(crate::say::decode_sound(&bytes).is_err());
        assert_eq!(
            crate::say::decode_sound_with_limit(&bytes, MOST_FRAMES)
                .unwrap()
                .len(),
            96_000
        );
        let longest = wav(&vec![100; MOST_FRAMES as usize], 24_000, 1);
        assert!(crate::say::decode_sound_with_limit(&longest, MOST_FRAMES).is_ok());
        let too_long = wav(&vec![100; MOST_FRAMES as usize + 1], 24_000, 1);
        assert!(crate::say::decode_sound_with_limit(&too_long, MOST_FRAMES).is_err());
    }

    #[test]
    fn recordings_keep_the_existing_format_peak_and_rms_bounds() {
        for bytes in [
            wav(&[100; 480], 16_000, 1),
            wav(&[100; 480], 24_000, 2),
            wav(&[12_000], 24_000, 1),
            wav(&[2_000; 240], 24_000, 1),
            wav(&[], 24_000, 1),
        ] {
            assert!(crate::say::decode_sound_with_limit(&bytes, MOST_FRAMES).is_err());
        }
    }

    #[test]
    fn samples_and_digest_are_from_the_same_immutable_file_image() {
        let dir = std::env::temp_dir().join(format!("requested-audio-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("recording.wav");
        let bytes = wav(&[100; 480], 24_000, 1);
        fs::write(&file, &bytes).unwrap();
        let checked = read_sound(&file).unwrap();
        fs::write(&file, wav(&[200; 480], 24_000, 1)).unwrap();
        assert_eq!(checked.samples, [100; 480]);
        assert_eq!(checked.sha256, format!("{:x}", Sha256::digest(&bytes)));
        assert_ne!(checked.sha256, read_sound(&file).unwrap().sha256);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #5764: case on3a's two lines talk about music and ask nothing; the
    /// first got a `use_skill` call on lamp-52e6 on 9 Nov (request 15).
    #[test]
    fn only_a_line_that_asks_to_play_is_an_ask() {
        for talk in [
            "Lamp, I listened to some relaxing music on the train this morning, it was really nice.",
            "Lamp, what kind of music helps people relax?",
            "I love relaxing music.",
            "Lamp, don't play anything, I'm on a call.",
            "Can you not play music right now?",
            "My sister plays the piano every night.",
            "I want to know which songs calm a baby.",
            "Tôi đã nghe nhạc trên tàu sáng nay.",
            "Đừng mở nhạc nhé.",
            // #5802: a later cancel in the line, and asks for something else.
            "Lamp, play some relaxing music. Actually, don't.",
            "Play some music; stop, never mind.",
            "Lamp, turn on the light.",
            "Lamp, play chess with me.",
            "Play something with me.",
            "Lamp, turn on the light so I can read my sheet music.",
            "Can you play chess while I listen to music?",
            "Play something like chess to relax.",
            "Play something with me to relax.",
            "Play the second game while I listen to music.",
            "Play another game to relax.",
            "Play the next something to relax.",
            "Lamp, play some music. Forget it.",
            "",
        ] {
            assert!(!asks_to_play(talk), "{talk}");
        }
        for ask in [
            "Lamp, can you play some relaxing music?",
            "Play some relaxing music.",
            "Hey lamp, please play something calm.",
            "Put on something relaxing, please.",
            "Could you please put some music on?",
            "I'd like to hear some relaxing music.",
            "I'm stressed. Play something quiet.",
            "Let's listen to some music.",
            "Please play a quiet recording.",
            "Now play the second recording.",
            "Play the first recording.",
            "Play the next recording.",
            "Play the previous recording.",
            "Play the last recording.",
            "Play another recording.",
            "Play a different recording.",
            "Lamp, mở nhạc thư giãn đi.",
            "Bật nhạc cho mình nhé.",
            // #5802: lamp-52e6 on 10 Nov, clips relax and relax2.
            "Lamp, play me something to relax.",
            "Lamp, can you play some relaxing music?",
            "Play something to help me sleep.",
            "Put on something calm.",
            "Could you put on something to help me unwind?",
        ] {
            assert!(asks_to_play(ask), "{ask}");
        }
    }
}
