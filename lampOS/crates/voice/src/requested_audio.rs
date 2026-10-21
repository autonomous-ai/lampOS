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
}
