//! The head tap's "hm?" (#711): a soft hum, two short tones rising like a
//! question, made here and played through the voice's own speaker path as
//! its own turn, so the echo canceller has it as reference.
//!
//! First's limits (#711, 2026-10-15 21:24): rare, at most one in 30 s; well
//! under the voice, starting about 15 dB below its speech, to be tuned by
//! ear on a lamp; under 150 ms from the tap to the first sample; and only
//! when the lamp is quiet, listening is ready and no reply is owed.
//!
//! A hand's sleep (#1362) has two more of the lamp's own sounds, as quiet:
//! a falling breath in the hold's last second ([`breath`]), and this "hm?"
//! as the person wakes it again.
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

/// The falling breath as a hand's sleep settles (#1362): a sigh, an exhale
/// whose breathy noise darkens as it fades, with a faint hum under it
/// falling from [`SIGH_HZ`]'s first to its second, so a small speaker
/// carries it as a creature's and not as a hiss. Never a tone that stands
/// alone: a breath, not a beep.
pub const BREATH_MS: u32 = 900;
/// It comes in over this, then fades over the rest.
pub const BREATH_IN_MS: u32 = 120;
/// The hum under it, falling a fourth, as a voice falls settling down.
pub const SIGH_HZ: [f64; 2] = [247.0, 185.0];
/// Where the breathy noise is brightest at its start, and its end.
pub const BREATH_BRIGHT_HZ: [f64; 2] = [1_400.0, 350.0];
/// The hum's share of the sound, by amplitude: under the breath.
const SIGH_SHARE: f64 = 0.35;

/// The sigh at `dbfs` RMS over its loudest tenth of a second: a whole
/// number of chunks, zero after the end of the sound. The same every time.
pub fn breath(dbfs: f64) -> Vec<i16> {
    let length = (RATE * BREATH_MS / 1_000) as usize;
    let rise = (RATE * BREATH_IN_MS / 1_000) as usize;
    // A small fixed noise source (xorshift), so the sound is the same
    // every time and a test can hold it.
    let mut state: u32 = 0x9E37_79B9;
    let mut noise = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    let mut shaped = Vec::with_capacity(length);
    let (mut low, mut phase) = (0.0_f64, 0.0_f64);
    for n in 0..length {
        let along = n as f64 / length as f64;
        // In fast, out slowly, as breath leaves.
        let gain = if n < rise {
            0.5 - 0.5 * (std::f64::consts::PI * n as f64 / rise as f64).cos()
        } else {
            let out = (n - rise) as f64 / (length - rise) as f64;
            (1.0 - out).powi(2)
        };
        // The noise through a one-pole low-pass whose corner falls.
        let corner = BREATH_BRIGHT_HZ[0] * (BREATH_BRIGHT_HZ[1] / BREATH_BRIGHT_HZ[0]).powf(along);
        let pole = (-2.0 * std::f64::consts::PI * corner / f64::from(RATE)).exp();
        low = (1.0 - pole) * noise() + pole * low;
        let pitch = SIGH_HZ[0] * (SIGH_HZ[1] / SIGH_HZ[0]).powf(along);
        phase += 2.0 * std::f64::consts::PI * pitch / f64::from(RATE);
        shaped.push(gain * ((1.0 - SIGH_SHARE) * low + SIGH_SHARE * phase.sin()));
    }
    // Scaled so that its loudest tenth of a second has the asked-for RMS.
    let window = (RATE / 10) as usize;
    let loudest = (0..=length - window)
        .step_by(CHUNK)
        .map(|start| {
            let sum: f64 = shaped[start..start + window].iter().map(|v| v * v).sum();
            (sum / window as f64).sqrt()
        })
        .fold(0.0_f64, f64::max);
    let scale = 32_767.0 * 10f64.powf(dbfs / 20.0) / loudest;
    let mut samples: Vec<i16> = shaped
        .iter()
        .map(|value| (value * scale).round() as i16)
        .collect();
    samples.resize(length.div_ceil(CHUNK) * CHUNK, 0);
    samples
}

/// A hand on the head this long, of the [`SLEEP_HOLD_US`] that put the
/// lamp to sleep, begins the breath (#1362, the first builder on #1362, 21
/// Oct 10:35): in the hold's last second, so the microphones still close
/// at the sleep's own time and not one sample later. A hand lifted before
/// then gets a sigh and no sleep.
pub const BREATH_FROM_US: u64 = 2_000_000;
/// Controls' hold that puts the lamp to sleep (`lamp_controls::touch`).
pub const SLEEP_HOLD_US: u64 = lamp_controls::touch::HEAD_SLEEP_US;
/// The latest hand time the breath may begin at: while all but its last
/// tenth of a second, already fading, fits before the sleep cuts it. A
/// hand seen later (a busy loop, or a conversation opened late in the hold)
/// gets no breath, never one cut to a tick (the reviewer's P3 on #2487).
pub const BREATH_UNTIL_US: u64 = SLEEP_HOLD_US - BREATH_MS as u64 * 1_000 + 100_000;

/// Whether a hand on the head `held_us` may begin the breath.
pub fn sighs_at(held_us: u64) -> bool {
    (BREATH_FROM_US..=BREATH_UNTIL_US).contains(&held_us)
}

/// Whether the breath plays now, or why not: the microphone allowed, the
/// lamp quiet and listening ready. Not held to the tap's 30 s: once a
/// hold, which the person made.
pub fn may_sigh(now: &Now) -> Result<(), &'static str> {
    if !now.allowed {
        return Err("microphone_not_allowed");
    }
    if !now.quiet {
        return Err("not_quiet");
    }
    if !now.listening_ready {
        return Err("listening_not_ready");
    }
    Ok(())
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

    /// #1362: the sigh is about 0.9 s of breath in whole chunks, at the
    /// hum's level at its loudest, starting and ending from nothing, under
    /// the speech's level, and falling: quieter and darker as it goes. The
    /// same every time.
    #[test]
    fn the_breath_is_a_quiet_falling_sigh_in_whole_chunks() {
        let sigh = breath(DBFS);
        assert_eq!(sigh, breath(DBFS), "the same every time");
        let sounding = (RATE * BREATH_MS / 1_000) as usize;
        assert_eq!(sigh.len(), sounding.div_ceil(CHUNK) * CHUNK);
        assert!(sigh[sounding..].iter().all(|&s| s == 0));
        assert!(sigh[0].abs() <= 1 && sigh[sounding - 1].abs() <= 2);
        let tenth = (RATE / 10) as usize;
        let loudest = (0..=sounding - tenth)
            .step_by(CHUNK)
            .map(|start| dbfs(&sigh[start..start + tenth]))
            .fold(f64::MIN, f64::max);
        assert!((loudest - DBFS).abs() < 0.5, "{loudest}");
        let peak = sigh.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(
            f64::from(peak) < 32_767.0 * 10f64.powf(-20.0 / 20.0),
            "{peak}"
        );
        // Falling: its second half quieter than its first by far, and its
        // last third darker than its first (fewer zero crossings).
        let half = sounding / 2;
        assert!(dbfs(&sigh[..half]) - dbfs(&sigh[half..sounding]) > 6.0);
        let crossings = |part: &[i16]| {
            part.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count() as f64 / part.len() as f64
        };
        let third = sounding / 3;
        assert!(
            crossings(&sigh[..third]) > crossings(&sigh[2 * third..sounding]) * 1.3,
            "{} {}",
            crossings(&sigh[..third]),
            crossings(&sigh[2 * third..sounding])
        );
        // A quieter level is quieter by as much.
        let quieter = breath(DBFS - 6.0);
        let level = |sound: &[i16]| dbfs(&sound[..tenth * 3]);
        assert!((level(&sigh) - level(&quieter) - 6.0).abs() < 0.5);
    }

    /// #1362: the breath begins in the hold's last second, before Controls'
    /// sleep at three, only while most of it fits before the sleep; and it
    /// sighs only quiet, allowed and ready, however lately it hummed.
    #[test]
    fn it_sighs_in_the_holds_last_second_when_quiet_and_ready() {
        assert_eq!(SLEEP_HOLD_US, 3_000_000);
        assert_eq!(BREATH_UNTIL_US, 2_200_000);
        assert!(!sighs_at(1_999_999));
        assert!(sighs_at(2_000_000));
        assert!(sighs_at(2_200_000));
        assert!(!sighs_at(2_200_001));
        assert!(!sighs_at(2_970_000), "too late: it would be cut to a tick");
        let now = Now {
            last_hum_us: Some(99_999_000),
            reply_owed: true,
            ..ready()
        };
        assert_eq!(may_sigh(&now), Ok(()));
        for (refused, why) in [
            (
                Now {
                    allowed: false,
                    ..now
                },
                "microphone_not_allowed",
            ),
            (
                Now {
                    quiet: false,
                    ..now
                },
                "not_quiet",
            ),
            (
                Now {
                    listening_ready: false,
                    ..now
                },
                "listening_not_ready",
            ),
        ] {
            assert_eq!(may_sigh(&refused), Err(why));
        }
    }
}
