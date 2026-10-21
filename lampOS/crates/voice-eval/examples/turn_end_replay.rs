//! Replays recorded speech through the voice's own end-of-turn detector,
//! with the Smart Turn model in this process, for each profile given, and
//! says where each turn ended and why (#782).
//!
//! `cargo run --release -p lamp-voice-eval --example turn_end_replay --
//!  MODEL CLIPS.json [--end-silence-ms 600] [--verdict-ms 130]
//!  [--speech-level-db DB] [--faint-floor-db DB [--faint-hold-ms MS]]
//!  --profile NAME:COMMIT_MS:COMPLETE_P:LEAST_SPEECH_MS[:recheck][:short] ...`
//!
//! `CLIPS.json` is `[{"id": "...", "wav": "/path.wav"}, ...]`, each a 16 kHz
//! mono 16-bit WAV. Each clip gets 600 ms of lead-in and 2.5 s of tail at a
//! -63 dBFS noise floor, as docs/endpoint-detection.md replayed its human
//! clips. Every verdict is computed at once and handed to the detector a
//! fixed `--verdict-ms` later on the replay's own clock, so a run is the
//! same every time and the host's speed does not enter it. With
//! `--speech-level-db` each clip is first scaled so that the median level of
//! its speech blocks is that, as the voice's level is measured
//! (`ring_face::level_of`); with `--faint-floor-db` a block under that level
//! counts as no speech, with the voice's own hold after six blocks over it
//! (`room_floor::Near`, `LAMP_FAINT_HOLD_MS`), as `--faint-floor-db` does in
//! the voice. Prints one JSON
//! line a clip and profile: each turn's start and end in milliseconds of
//! the clip's own audio, and the decision that ended it. Nothing is played.
use lamp_audio::{CAPTURE_SAMPLES, CaptureBlock};
use lamp_turn_end::{FeatureExtractor, SMART_TURN_V3_2, SmartTurn};
use lamp_turn_end_wire::{
    Availability, Classifier, Outcome, Submit, Token, Verdict, WINDOW_SAMPLES,
};
use lamp_voice::activity::{
    Activity, ConfirmPolicy, ObservedAudio, SpeechProbability, TurnDetector,
};
use lamp_voice::{ring_face::level_of, room_floor::Near};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const BLOCK_US: u64 = 10_000;
const LEAD_BLOCKS: usize = 60;
const TAIL_BLOCKS: usize = 250;
/// -63 dBFS as the RMS of a uniform noise: 32768 * 10^(-63/20) * sqrt(3).
const NOISE_PEAK: f64 = 40.0;

type Error = Box<dyn std::error::Error>;

/// The model, scored at once, its verdict released `delay` blocks later.
struct Delayed {
    model: Arc<Mutex<SmartTurn>>,
    extractor: FeatureExtractor,
    clock: Arc<AtomicU64>,
    delay: u64,
    pending: Option<(u64, Outcome)>,
    request: u64,
    /// Every verdict: the block it was asked at, and its probability.
    asked: Arc<Mutex<Vec<(u64, f32)>>>,
}

impl Classifier for Delayed {
    fn submit(&mut self, token: Token, pcm: &[i16]) -> Submit {
        if self.pending.is_some() {
            return Submit::Busy;
        }
        let window = &pcm[pcm.len().saturating_sub(WINDOW_SAMPLES)..];
        let mut model = self.model.lock().expect("model");
        let result =
            lamp_turn_end::classify(&mut *model, &mut self.extractor, window).map(|done| Verdict {
                probability: done.probability,
                features_us: done.features_us,
                inference_us: done.inference_us,
                ..Verdict::default()
            });
        self.request += 1;
        if let Ok(verdict) = &result {
            self.asked
                .lock()
                .expect("asked")
                .push((self.clock.load(Ordering::Relaxed), verdict.probability));
        }
        let release = self.clock.load(Ordering::Relaxed) + self.delay;
        self.pending = Some((
            release,
            Outcome {
                token,
                worker: 1,
                request: self.request,
                result,
            },
        ));
        Submit::Accepted
    }

    fn poll(&mut self) -> Option<Outcome> {
        let now = self.clock.load(Ordering::Relaxed);
        match &self.pending {
            Some((release, _)) if now >= *release => {
                self.pending.take().map(|(_, outcome)| outcome)
            }
            _ => None,
        }
    }

    fn availability(&self) -> Availability {
        Availability::Ready
    }

    fn cancel(&mut self) {
        self.pending = None;
    }
}

/// A 16 kHz mono 16-bit WAV's samples.
fn read_wav(path: &Path) -> Result<Vec<i16>, Error> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{}: not a WAV", path.display()).into());
    }
    let mut at = 12;
    let mut format = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
        let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            let mut kind = u16::from_le_bytes(body[0..2].try_into()?);
            // WAVE_FORMAT_EXTENSIBLE (`afconvert` writes it): the format is
            // the subformat's first two bytes.
            if kind == 0xfffe && body.len() >= 26 {
                kind = u16::from_le_bytes(body[24..26].try_into()?);
            }
            let channels = u16::from_le_bytes(body[2..4].try_into()?);
            let rate = u32::from_le_bytes(body[4..8].try_into()?);
            let bits = u16::from_le_bytes(body[14..16].try_into()?);
            format = Some((kind, channels, rate, bits));
        } else if id == b"data" {
            if format != Some((1, 1, 16_000, 16)) {
                return Err(
                    format!("{}: not 16 kHz mono 16-bit PCM: {format:?}", path.display()).into(),
                );
            }
            return Ok(body
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect());
        }
        at += 8 + size + size % 2;
    }
    Err(format!("{}: no data", path.display()).into())
}

struct Profile {
    name: String,
    policy: ConfirmPolicy,
}

fn profile(text: &str) -> Result<Profile, Error> {
    let parts: Vec<&str> = text.split(':').collect();
    let [name, commit, complete, least, ref flags @ ..] = parts[..] else {
        return Err(format!(
            "profile {text}: NAME:COMMIT_MS:COMPLETE_P:LEAST_SPEECH_MS[:recheck][:short]"
        )
        .into());
    };
    let blocks = |ms: &str| -> Result<u16, Error> { Ok(u16::try_from(ms.parse::<u64>()? / 10)?) };
    let mut policy = ConfirmPolicy {
        commit_blocks: blocks(commit)?,
        complete_probability: complete.parse()?,
        least_speech_blocks: blocks(least)?,
        ..ConfirmPolicy::default()
    };
    // The voice's switches, as the lamp sets them: an uncertain verdict
    // asked again (`recheck_uncertain`), and lines under the floor asked
    // from 0.2 s (`LAMP_ENDPOINT_SHORT_VERDICT`).
    for flag in flags {
        match *flag {
            "recheck" => policy.recheck_uncertain = true,
            "short" => policy.short_verdict_blocks = 20,
            other => return Err(format!("profile {text}: unknown flag {other}").into()),
        }
    }
    Ok(Profile {
        name: name.into(),
        policy,
    })
}

/// The voice's level scale (`ring_face::level_of`): -48 dBFS is 0, -14 is 255.
fn level_for_db(db: f64) -> u8 {
    (((db + 48.0) / 34.0).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rms_db(samples: &[i16]) -> f64 {
    let power = samples
        .iter()
        .map(|sample| (f64::from(*sample) / 32_768.0).powi(2))
        .sum::<f64>()
        / samples.len().max(1) as f64;
    10.0 * power.max(1e-12).log10()
}

/// The clip scaled so that the median level of its speech blocks is `target`.
fn scaled(samples: &[i16], target: f64) -> Vec<i16> {
    let mut vad = SpeechProbability::default();
    let mut levels: Vec<f64> = samples
        .chunks_exact(CAPTURE_SAMPLES)
        .filter_map(|chunk| {
            let block: CaptureBlock = chunk.try_into().ok()?;
            (vad.analyze(&block) >= 0.5).then(|| rms_db(chunk))
        })
        .collect();
    if levels.is_empty() {
        return samples.to_vec();
    }
    levels.sort_by(f64::total_cmp);
    let gain = 10f64.powf((target - levels[levels.len() / 2]) / 20.0);
    samples
        .iter()
        .map(|sample| (f64::from(*sample) * gain).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

#[derive(Clone, Copy)]
struct Room {
    speech_level_db: Option<f64>,
    faint_floor_db: Option<f64>,
    faint_hold_ms: u64,
}

fn replay(
    samples: &[i16],
    policy: ConfirmPolicy,
    model: &Arc<Mutex<SmartTurn>>,
    end_silence_blocks: u16,
    delay: u64,
    room: Room,
) -> Result<(Vec<Value>, Vec<Value>), Error> {
    let samples = match room.speech_level_db {
        Some(target) => scaled(samples, target),
        None => samples.to_vec(),
    };
    let floor = room.faint_floor_db.map(level_for_db);
    let mut near = Near::new(room.faint_hold_ms);
    let clock = Arc::new(AtomicU64::new(0));
    let asked = Arc::new(Mutex::new(Vec::new()));
    let classifier = Delayed {
        asked: asked.clone(),
        model: model.clone(),
        extractor: FeatureExtractor::default(),
        clock: clock.clone(),
        delay,
        pending: None,
        request: 0,
    };
    let mut detector = TurnDetector::default()
        .with_end_silence(end_silence_blocks)?
        .with_confirmation(Box::new(classifier), policy)?;
    let mut vad = SpeechProbability::default();
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    let mut audio = vec![0i16; LEAD_BLOCKS * CAPTURE_SAMPLES];
    audio.extend_from_slice(&samples);
    audio.resize(
        audio.len().div_ceil(CAPTURE_SAMPLES) * CAPTURE_SAMPLES + TAIL_BLOCKS * CAPTURE_SAMPLES,
        0,
    );
    let lead_us = (LEAD_BLOCKS as u64) * BLOCK_US;
    let mut turns = Vec::new();
    let mut started = None;
    for (index, chunk) in audio.chunks_exact(CAPTURE_SAMPLES).enumerate() {
        let mut block: CaptureBlock = [0; CAPTURE_SAMPLES];
        for (out, sample) in block.iter_mut().zip(chunk) {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            let floor = ((noise >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * NOISE_PEAK;
            *out = (f64::from(*sample) + floor).clamp(-32768.0, 32767.0) as i16;
        }
        clock.store(index as u64, Ordering::Relaxed);
        let at_us = index as u64 * BLOCK_US;
        let mut probability = vad.analyze(&block);
        if let Some(floor) = floor
            && !near.hears(level_of(&block), floor, at_us)
        {
            probability = 0.0;
        }
        // The detector takes sequence 0 for a missing one: blocks count from 1.
        let frame = ObservedAudio {
            sequence: index as u64 + 1,
            captured_at_us: at_us,
            samples: block,
        };
        match detector.push(frame, probability) {
            Activity::Start(prefix) => {
                started = prefix
                    .first()
                    .map(|first| first.captured_at_us)
                    .or(Some(at_us));
            }
            Activity::End(frame) => {
                let decision = detector.last_endpoint();
                turns.push(json!({
                    "start_ms": started.map(|start| (start as i64 - lead_us as i64) / 1000),
                    "end_ms": (frame.captured_at_us as i64 - lead_us as i64) / 1000,
                    "basis": decision.map(|decision| decision.basis),
                    "silence_ms": decision.map(|decision| decision.silence_ms),
                    "p": decision.and_then(|decision| decision.probability),
                    "first_p": decision.and_then(|decision| decision.rechecked_from),
                    "short_asks": decision.map_or(0, |decision| decision.short_asks),
                }));
                started = None;
            }
            Activity::Fault(why) => return Err(why.into()),
            Activity::Quiet | Activity::Continue(_) => {}
        }
    }
    let asked = asked
        .lock()
        .expect("asked")
        .iter()
        .map(|(block, probability)| json!({"at_ms": (*block as i64 * 10) - lead_us as i64 / 1000, "p": probability}))
        .collect();
    Ok((turns, asked))
}

fn main() -> Result<(), Error> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [model_path, clips_path, rest @ ..] = &arguments[..] else {
        return Err("turn_end_replay MODEL CLIPS.json [--end-silence-ms MS] [--verdict-ms MS] --profile NAME:COMMIT_MS:COMPLETE_P:LEAST_SPEECH_MS ...".into());
    };
    let mut end_silence_ms = 600u64;
    let mut verdict_ms = 130u64;
    let mut room = Room {
        speech_level_db: None,
        faint_floor_db: None,
        faint_hold_ms: 0,
    };
    let mut profiles = Vec::new();
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        let value = flags
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--end-silence-ms" => end_silence_ms = value.parse()?,
            "--verdict-ms" => verdict_ms = value.parse()?,
            "--profile" => profiles.push(profile(value)?),
            "--speech-level-db" => room.speech_level_db = Some(value.parse()?),
            "--faint-floor-db" => room.faint_floor_db = Some(value.parse()?),
            "--faint-hold-ms" => room.faint_hold_ms = value.parse()?,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if profiles.is_empty() {
        return Err("at least one --profile".into());
    }
    let model = SmartTurn::load(Path::new(model_path), &SMART_TURN_V3_2)
        .map_err(|error| error.to_string())?;
    let model = Arc::new(Mutex::new(model));
    let clips: Vec<Value> = serde_json::from_slice(&std::fs::read(clips_path)?)?;
    let end_silence_blocks = u16::try_from(end_silence_ms / 10)?;
    for clip in &clips {
        let id = clip["id"].as_str().ok_or("a clip without an id")?;
        let samples = read_wav(Path::new(
            clip["wav"].as_str().ok_or("a clip without a wav")?,
        ))?;
        for profile in &profiles {
            let (turns, asked) = replay(
                &samples,
                profile.policy,
                &model,
                end_silence_blocks,
                verdict_ms / 10,
                room,
            )?;
            println!(
                "{}",
                json!({"clip": id, "profile": profile.name, "audio_ms": samples.len() as u64 / 16, "turns": turns, "verdicts": asked})
            );
        }
    }
    Ok(())
}
