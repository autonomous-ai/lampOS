//! The head tap's "hm?" (#711): a soft hum, two short tones rising like a
//! question, made here and played through the voice's own speaker path as
//! its own turn, so the echo canceller has it as reference.
//!
//! First's limits (#711, 2026-10-15 21:24): rare, at most one in 30 s; well
//! under the voice, starting about 15 dB below its speech, to be tuned by
//! ear on a lamp; under 150 ms from the tap to the first sample; and only
//! when the lamp is quiet, listening is ready and no reply is owed.
//!
//! Nothing here plays, reads a device or keeps a clock: the coordinator
//! gives it times and what it knows.

/// Speaker samples a second.
pub const RATE: u32 = 24_000;
/// Samples in one speaker chunk.
pub const CHUNK: usize = 240;
/// The two tones, the second a major third above the first, as a voice
/// rises on a question. Their overtones carry them on a small speaker.
pub const TONES_HZ: [f64; 2] = [330.0, 415.0];
/// How long each tone lasts, and the breath between them.
pub const TONE_MS: [u32; 2] = [110, 160];
pub const BETWEEN_MS: u32 = 30;
/// Each tone glides up this much across its length: a hum, not a beep.
pub const GLIDE: f64 = 0.04;
/// Fade in and out of each tone, so neither starts or ends with a click.
pub const FADE_MS: u32 = 25;
/// How loud while it sounds, as RMS: the lamp's speech taken as about
/// -20 dBFS, less 15 dB. Not yet measured against a lamp's own speech.
pub const DBFS: f64 = -35.0;
/// `LAMP_HEAD_TAP_CUE_DBFS` replaces [`DBFS`] for tuning by ear, within
/// these bounds; anything else is refused at start.
pub const DBFS_ENVIRONMENT: &str = "LAMP_HEAD_TAP_CUE_DBFS";
pub const DBFS_RANGE: (f64, f64) = (-60.0, -20.0);
/// At most one hum in this long (creature.md: the non-word sounds are kept
/// rare). A tap in between gets no hum.
pub const RARE_US: u64 = 30_000_000;
/// The same hum when the lamp's own "Sorry?" (#926) came back with nothing
/// said (#1417): `LAMP_UNCLEAR_HUM=on`, off unless set.
pub const UNCLEAR_SWITCH: &str = "LAMP_UNCLEAR_HUM";

/// Whether the switch is on in this process's environment.
pub fn unclear_from_environment() -> bool {
    unclear(std::env::var(UNCLEAR_SWITCH).ok().as_deref())
}

/// Whether the switch's value turns it on: only `on`.
pub fn unclear(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The level `LAMP_HEAD_TAP_CUE_DBFS` asks for, or [`DBFS`] when unset.
pub fn level_from_environment() -> Result<f64, String> {
    match std::env::var(DBFS_ENVIRONMENT) {
        Err(_) => Ok(DBFS),
        Ok(text) => level(&text),
    }
}

fn level(text: &str) -> Result<f64, String> {
    let (low, high) = DBFS_RANGE;
    match text.trim().parse::<f64>() {
        Ok(dbfs) if (low..=high).contains(&dbfs) => Ok(dbfs),
        _ => Err(format!(
            "{DBFS_ENVIRONMENT} must be a number of dBFS from {low} to {high}, not {text:?}"
        )),
    }
}

/// The hum at `dbfs` RMS while it sounds: a whole number of chunks, zero
/// after the end of the sound.
pub fn hum(dbfs: f64) -> Vec<i16> {
    // A fundamental with two soft overtones, scaled so that a steady stretch
    // of it has the asked-for RMS.
    const OVERTONES: [f64; 3] = [1.0, 0.35, 0.12];
    let power: f64 = OVERTONES.iter().map(|a| a * a / 2.0).sum();
    let amplitude = 32_767.0 * 10f64.powf(dbfs / 20.0) / power.sqrt();
    let samples_of = |ms: u32| (RATE * ms / 1_000) as usize;
    let fade = samples_of(FADE_MS);
    let mut samples = Vec::new();
    for (index, (hertz, ms)) in TONES_HZ.iter().zip(TONE_MS).enumerate() {
        if index > 0 {
            samples.resize(samples.len() + samples_of(BETWEEN_MS), 0);
        }
        let length = samples_of(ms);
        let mut phase = 0.0_f64;
        for n in 0..length {
            let edge = n.min(length - 1 - n);
            let gain = if edge < fade {
                0.5 - 0.5 * (std::f64::consts::PI * edge as f64 / fade as f64).cos()
            } else {
                1.0
            };
            let pitch = hertz * (1.0 + GLIDE * n as f64 / length as f64);
            phase += 2.0 * std::f64::consts::PI * pitch / f64::from(RATE);
            let value: f64 = OVERTONES
                .iter()
                .enumerate()
                .map(|(k, a)| a * ((k + 1) as f64 * phase).sin())
                .sum();
            samples.push((amplitude * gain * value).round() as i16);
        }
    }
    samples.resize(samples.len().div_ceil(CHUNK) * CHUNK, 0);
    samples
}

/// What the voice knows when a tap comes, on its monotonic clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct Now {
    pub at_us: u64,
    /// When the tapping hand arrived.
    pub began_us: u64,
    /// The microphone is allowed.
    pub allowed: bool,
    /// No turn, input, reply, playback or pause: nothing is going on.
    pub quiet: bool,
    pub listening_ready: bool,
    /// The lamp owes someone something: a look it was asked for.
    pub reply_owed: bool,
    /// The last time the lamp was heard speaking.
    pub last_said_us: Option<u64>,
    /// The last hum.
    pub last_hum_us: Option<u64>,
}

/// Whether the hum plays for this tap, or why not.
pub fn may_hum(now: &Now) -> Result<(), &'static str> {
    if !now.allowed {
        return Err("microphone_not_allowed");
    }
    // The lamp was talking when the hand arrived: that touch stopped it,
    // and stopping is the whole answer.
    if now.last_said_us.is_some_and(|said| said >= now.began_us) {
        return Err("was_speaking");
    }
    if !now.quiet {
        return Err("not_quiet");
    }
    if !now.listening_ready {
        return Err("listening_not_ready");
    }
    if now.reply_owed {
        return Err("reply_owed");
    }
    if now
        .last_hum_us
        .is_some_and(|last| now.at_us.saturating_sub(last) < RARE_US)
    {
        return Err("hummed_recently");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[i16]) -> f64 {
        let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    fn dbfs(samples: &[i16]) -> f64 {
        20.0 * (rms(samples) / 32_767.0).log10()
    }

    #[test]
    fn the_hum_is_two_quiet_rising_tones_in_whole_chunks_with_no_click() {
        let hum = hum(DBFS);
        assert_eq!(hum.len() % CHUNK, 0);
        // 110 + 30 + 160 ms of sound, then silence to the chunk's end.
        let sounding = (RATE * (110 + 30 + 160) / 1_000) as usize;
        assert_eq!(hum.len(), sounding.div_ceil(CHUNK) * CHUNK);
        assert!(hum[sounding..].iter().all(|&s| s == 0));
        // The steady middle of each tone is at the asked-for level.
        let first = &hum[(RATE * 40 / 1_000) as usize..(RATE * 80 / 1_000) as usize];
        let second = &hum[(RATE * 180 / 1_000) as usize..(RATE * 260 / 1_000) as usize];
        for tone in [first, second] {
            assert!((dbfs(tone) - DBFS).abs() < 0.5, "{}", dbfs(tone));
        }
        // Silent between the tones, and starting and ending from nothing.
        let between = &hum[(RATE * 110 / 1_000) as usize..(RATE * 140 / 1_000) as usize];
        assert!(between.iter().all(|&s| s == 0));
        assert!(hum[0].abs() <= 1 && hum[sounding - 1].abs() <= 1);
        let peak = hum.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(
            f64::from(peak) < 32_767.0 * 10f64.powf(-20.0 / 20.0),
            "{peak}"
        );
        // The second tone is higher: count its zero crossings.
        let crossings = |tone: &[i16]| {
            tone.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count() as f64 / tone.len() as f64
        };
        assert!(crossings(second) > crossings(first) * 1.15);
    }

    #[test]
    fn a_quieter_level_is_quieter_by_as_much() {
        let middle =
            |hum: &[i16]| dbfs(&hum[(RATE * 40 / 1_000) as usize..(RATE * 80 / 1_000) as usize]);
        let (loud, quiet) = (hum(-30.0), hum(-45.0));
        assert!((middle(&loud) - middle(&quiet) - 15.0).abs() < 0.5);
    }

    #[test]
    fn the_level_from_the_environment_is_bounded() {
        assert_eq!(level("-40"), Ok(-40.0));
        assert_eq!(level(" -20 "), Ok(-20.0));
        for wrong in ["-19.5", "-61", "loud", "", "NaN"] {
            assert!(level(wrong).is_err(), "{wrong}");
        }
    }

    fn ready() -> Now {
        Now {
            at_us: 100_000_000,
            began_us: 99_700_000,
            allowed: true,
            quiet: true,
            listening_ready: true,
            reply_owed: false,
            last_said_us: Some(90_000_000),
            last_hum_us: Some(60_000_000),
        }
    }

    #[test]
    fn it_hums_only_quiet_ready_owing_nothing_and_rarely() {
        assert_eq!(may_hum(&ready()), Ok(()));
        let refused = [
            (
                Now {
                    allowed: false,
                    ..ready()
                },
                "microphone_not_allowed",
            ),
            (
                Now {
                    last_said_us: Some(99_800_000),
                    ..ready()
                },
                "was_speaking",
            ),
            (
                Now {
                    last_said_us: Some(99_700_000),
                    ..ready()
                },
                "was_speaking",
            ),
            (
                Now {
                    quiet: false,
                    ..ready()
                },
                "not_quiet",
            ),
            (
                Now {
                    listening_ready: false,
                    ..ready()
                },
                "listening_not_ready",
            ),
            (
                Now {
                    reply_owed: true,
                    ..ready()
                },
                "reply_owed",
            ),
            (
                Now {
                    last_hum_us: Some(70_000_001),
                    ..ready()
                },
                "hummed_recently",
            ),
        ];
        for (now, why) in refused {
            assert_eq!(may_hum(&now), Err(why));
        }
        // Thirty seconds after the last, it may hum again; never said, too.
        assert_eq!(
            may_hum(&Now {
                last_hum_us: Some(70_000_000),
                ..ready()
            }),
            Ok(())
        );
        assert_eq!(
            may_hum(&Now {
                last_said_us: None,
                last_hum_us: None,
                ..ready()
            }),
            Ok(())
        );
    }
}
