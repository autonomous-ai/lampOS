//! Room sound classes on `ears/classes` (#1669): speech, music and laughter
//! scores from YAMNet over mic1, and a door (a door, a sliding door, a slam
//! or a knock) and footsteps (#2562). Off unless `LAMP_EARS_CLASSES=on`, and
//! only with `LAMP_PRIVACY_CONTROLS=on` (docs/sound-events.md).
//!
//! Capture hands each admitted 20 ms block to a bounded queue and never waits:
//! a full queue drops the block and marks the window overloaded. One worker
//! thread keeps at most one patch of one uninterrupted capture era (under two
//! seconds), scores it every 500 ms and purges it on any gap, mute, epoch or
//! fence change, overflow or age past two seconds. Its results go to the Bus
//! pump, which checks current authority again before publishing.
//!
//! The frontend and patch bounds are the offline probe's (Sixth, 19 October),
//! checked there against an independent NumPy specification.
use crate::{
    BLOCK, RATE,
    pcm::CaptureStamp,
    privacy::{Permit, PublicationGate},
};
use lamp_msgs::{
    OwnVoiceEvidence, SOUND_RAW_AGE_US, SoundClasses, SoundScores, SoundUnavailable, SoundWindow,
};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::Duration,
};
use tract_onnx::{model::ParseResult, prelude::*};

/// Only an exact `on` selects it, as with the other Ears switches.
pub fn enabled(classes: Option<&str>) -> bool {
    classes == Some("on")
}

/// Whether the classifier starts, from its switch (`LAMP_EARS_CLASSES`),
/// the privacy route (`LAMP_PRIVACY_CONTROLS`) and, on a lamp with no slide,
/// the person's stored yes to room sounds ([`room_sounds_yes`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// The switch is not exactly `on`: nothing to say.
    Off,
    /// The switch is on, but the route is neither `on` (a lamp with a
    /// slide) nor `no-slide`: it never runs, said once.
    NotOnThisRoute,
    /// The switch is on, on a lamp with no slide, and no yes is stored
    /// (the lead's decision, First on #1722 at 07:33): said once.
    NoYes,
    /// It starts.
    Start,
}

/// [`Start`] for `classes` and `route`; `yes` is asked only on a lamp with
/// no slide, with the switch on.
pub fn start(classes: Option<&str>, route: Option<&str>, yes: impl FnOnce() -> bool) -> Start {
    if !enabled(classes) {
        Start::Off
    } else if crate::privacy::slide(route) {
        Start::Start
    } else if route != Some("no-slide") {
        Start::NotOnThisRoute
    } else if yes() {
        Start::Start
    } else {
        Start::NoYes
    }
}

/// Where the voice's onboarding keeps the person's yes to room sounds
/// (Seventh's #2110: `root:lampos` 0640 in a 0750 folder lamp-ears can
/// traverse): `{"room_sounds":"yes", ...}`, present only
/// while the yes stands; "no", silence, a redo, a forget and the reset
/// remove it.
pub const ROOM_SOUNDS_FILE: &str = "/var/lib/lampOS/consent/room-sounds.json";
/// How often a running classifier looks at the yes again, so a forget or a
/// redo of setup stops it within this, not at the next start (Third's P2 on
/// #2109).
pub const YES_EVERY_US: u64 = 2_000_000;
const ROOM_SOUNDS_MOST_BYTES: u64 = 4096;

/// Whether `path` holds a yes: a JSON object whose `room_sounds` is exactly
/// `"yes"`. Missing, unreadable, over 4096 bytes, not JSON, or anything
/// else is no; `version` and other fields are not read.
pub fn room_sounds_yes(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut text = String::new();
    if file
        .take(ROOM_SOUNDS_MOST_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
        || text.len() as u64 > ROOM_SOUNDS_MOST_BYTES
    {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .is_ok_and(|value| value.get("room_sounds").and_then(|v| v.as_str()) == Some("yes"))
}

/// Qualcomm AI Hub's YAMNet ONNX float export, release v0.59.0
/// (`yamnet-onnx-float.zip`): the graph and its external weights beside it.
pub const MODEL_ID: &str = "yamnet-qai-v0.59.0";
const GRAPH_BYTES: u64 = 25_789;
const GRAPH_SHA256: &str = "970ea63f4dc1aa33c86ef7d61d854319fdfe6bdd078f5b3d674ee1db5dd96471";
/// The graph names its weights by this file name, beside it.
pub const DATA_NAME: &str = "yamnet.data";
const DATA_BYTES: u64 = 14_915_108;
const DATA_SHA256: &str = "1d6dff488da4712190e10f1c841252aa58011381132d0126dcd5ee56a4e7ed31";
/// AudioSet indices in the bundled label map (`yamnet_class_map.csv`, as
/// qai-hub-models v0.59.0's `labels/yamnet_labels.txt`): Speech, Music,
/// Laughter, and "Walk, footsteps" (#2562).
const SPEECH: usize = 0;
const MUSIC: usize = 132;
const LAUGHTER: usize = 13;
const FOOTSTEPS: usize = 48;
/// The door group, whose highest score is `door` (#2562): Door (348),
/// Sliding door (351), Slam (352) and Knock (353). A knock on a door is
/// AudioSet's Knock, not Door. Tap (354) is left out: desk taps and typing
/// raise it all day.
const DOOR_GROUP: [usize; 4] = [348, 351, 352, 353];
const CLASSES: usize = 521;

pub const FFT: usize = 512;
const WINDOW: usize = 400;
pub const HOP: usize = 160;
pub const FRAMES: usize = 96;
pub const MELS: usize = 64;
/// 50 frames: a patch every 500 ms.
pub const PATCH_HOP: usize = 50;
/// Blocks the queue holds: 500 ms. Longer inference overflows and purges.
pub const QUEUE: usize = 25;
const BLOCK_US: u64 = 20_000;
/// No block for this long and the window is gone: closed or a device gap.
const SILENT_US: u64 = 100_000;
/// The contract's retention bound, in samples: two seconds.
const RETAIN: usize = 2 * RATE as usize;

/// One admitted block, owned. Neither Clone nor Debug; zeroed on drop with the
/// same best-effort limits as [`crate::pcm::Pcm`].
pub struct Block {
    samples: [i16; BLOCK],
    stamp: CaptureStamp,
    /// The voice said, freshly, that the lamp was speaking as this block
    /// was captured: its own sound may be in it.
    own: bool,
}

impl Drop for Block {
    fn drop(&mut self) {
        self.samples.fill(0);
    }
}

/// The capture side. `offer` copies 640 bytes into a preallocated bounded
/// queue and never blocks or allocates.
pub struct Handoff {
    sender: SyncSender<Block>,
    overflowed: Arc<AtomicBool>,
}

/// The worker side of the queue.
pub struct Intake {
    receiver: Receiver<Block>,
    overflowed: Arc<AtomicBool>,
}

pub fn handoff() -> (Handoff, Intake) {
    let (sender, receiver) = mpsc::sync_channel(QUEUE);
    let overflowed = Arc::new(AtomicBool::new(false));
    (
        Handoff {
            sender,
            overflowed: overflowed.clone(),
        },
        Intake {
            receiver,
            overflowed,
        },
    )
}

impl Handoff {
    /// Only a block whose own permit and fence still hold is copied. A full
    /// queue drops it and marks the worker's window overloaded. `own`: the
    /// voice said, freshly, that the lamp was speaking as it was captured.
    pub fn offer(
        &self,
        samples: &[i16; BLOCK],
        gate: &PublicationGate,
        stamp: CaptureStamp,
        own: bool,
        now_us: u64,
    ) -> bool {
        if stamp.captured_at_us > now_us || !gate.allows(stamp.permit, stamp.fence, now_us) {
            return false;
        }
        match self.sender.try_send(Block {
            samples: *samples,
            stamp,
            own,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.overflowed.store(true, Ordering::Release);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

/// A patch's samples, by index from the start of its capture era.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub first_frame: usize,
    pub start_sample: usize,
    pub end_sample: usize,
}

/// The full centered 512-sample FFT extent of 96 frames 160 samples apart.
/// The first patch of an era reflects at its start (15456 samples); later
/// ones take 15712 real samples. No end is ever padded.
pub fn bounds(first_frame: usize) -> Bounds {
    Bounds {
        first_frame,
        start_sample: (first_frame * HOP).saturating_sub(FFT / 2),
        end_sample: (first_frame + FRAMES - 1) * HOP + FFT / 2,
    }
}

/// The torch_audioset proxy's log-mel frontend, as pinned by the release:
/// periodic Hann 400 centered in FFT 512, hop 160, centered reflect padding,
/// magnitude, 64 HTK mel bands 125-7500 Hz with slopes linear in Hz,
/// natural log(mel + 0.001).
pub struct Frontend {
    fft: Arc<dyn Fft<f32>>,
    window: [f32; FFT],
    mel: Vec<[f32; MELS]>,
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new()
    }
}

impl Frontend {
    pub fn new() -> Self {
        let mut window = [0.0; FFT];
        // torch.hann_window defaults to periodic; torch.stft centers this
        // shorter window inside its n_fft window (56 zeros each side).
        for n in 0..WINDOW {
            window[(FFT - WINDOW) / 2 + n] =
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / WINDOW as f32).cos();
        }
        let hz_to_mel = |hz: f64| 2595.0 * (1.0 + hz / 700.0).log10();
        let low = hz_to_mel(125.0);
        let high = hz_to_mel(7500.0);
        // torch.linspace makes float32 mel points, converted back in float32.
        let edges: Vec<f32> = (0..MELS + 2)
            .map(|i| {
                let mel = (low + (high - low) * i as f64 / (MELS + 1) as f64) as f32;
                700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
            })
            .collect();
        let mel = (0..=FFT / 2)
            .map(|bin| {
                let hz = bin as f32 * RATE as f32 / FFT as f32;
                std::array::from_fn(|band| {
                    let lower = (hz - edges[band]) / (edges[band + 1] - edges[band]);
                    let upper = (edges[band + 2] - hz) / (edges[band + 2] - edges[band + 1]);
                    lower.min(upper).max(0.0)
                })
            })
            .collect();
        Self {
            fft: FftPlanner::new().plan_fft_forward(FFT),
            window,
            mel,
        }
    }

    /// 96x64 features for `bounds`; `pcm[0]` is the era's sample `base`.
    pub fn features(&self, pcm: &[f32], base: usize, bounds: Bounds) -> Result<Vec<f32>, String> {
        if bounds != self::bounds(bounds.first_frame)
            || bounds.start_sample < base
            || bounds.end_sample > base + pcm.len()
            || (bounds.start_sample == 0 && base != 0)
        {
            return Err("patch lacks its centered FFT extent".into());
        }
        let mut features = Vec::with_capacity(FRAMES * MELS);
        let mut spectrum = vec![Complex::new(0.0, 0.0); FFT];
        let mut scratch = vec![Complex::new(0.0, 0.0); self.fft.get_inplace_scratch_len()];
        for frame in bounds.first_frame..bounds.first_frame + FRAMES {
            let center = frame * HOP;
            for (n, value) in spectrum.iter_mut().enumerate() {
                // Reflection excludes sample zero itself: -1 is sample 1.
                let index = (center as isize + n as isize - (FFT / 2) as isize).unsigned_abs();
                *value = Complex::new(pcm[index - base] * self.window[n], 0.0);
            }
            self.fft.process_with_scratch(&mut spectrum, &mut scratch);
            let mut energies = [0.0_f32; MELS];
            for (bin, value) in spectrum[..=FFT / 2].iter().enumerate() {
                let magnitude = value.norm_sqr().sqrt();
                for (energy, weight) in energies.iter_mut().zip(self.mel[bin]) {
                    *energy += magnitude * weight;
                }
            }
            features.extend(energies.map(|energy| (energy + 0.001).ln()));
        }
        // What the FFT made of the samples goes as they do (the reviewer on
        // #1923): zeroed before it is freed, best effort as theirs.
        spectrum.fill(Complex::new(0.0, 0.0));
        scratch.fill(Complex::new(0.0, 0.0));
        Ok(features)
    }
}

/// The door group's highest score, from the model's 521 logits.
fn door(logits: &[f32]) -> f32 {
    DOOR_GROUP
        .iter()
        .map(|&index| sigmoid(logits[index]))
        .fold(0.0, f32::max)
}

/// Logits get independent sigmoids: scores are multi-label, never a softmax.
pub fn sigmoid(logit: f32) -> f32 {
    if logit >= 0.0 {
        1.0 / (1.0 + (-logit).exp())
    } else {
        let x = logit.exp();
        x / (1.0 + x)
    }
}

fn read_pinned(path: &Path, bytes: u64, sha256: &str) -> Result<Vec<u8>, String> {
    let said = |error: std::io::Error| format!("{}: {error}", path.display());
    let file = std::fs::File::open(path).map_err(said)?;
    if file.metadata().map_err(said)?.len() != bytes {
        return Err(format!("{} has an unexpected size", path.display()));
    }
    let mut contents = Vec::with_capacity(bytes as usize);
    file.take(bytes + 1)
        .read_to_end(&mut contents)
        .map_err(said)?;
    let digest = Sha256::digest(&contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if contents.len() as u64 != bytes || digest != sha256 {
        return Err(format!(
            "{} does not match the pinned SHA256",
            path.display()
        ));
    }
    Ok(contents)
}

/// The parser reads weights only from the verified bytes in memory, never by
/// path again, so the files cannot change between the check and the load.
struct PinnedData(Vec<u8>);

const VIRTUAL_DIR: &str = "pinned";

impl tract_onnx::data_resolver::ModelDataResolver for PinnedData {
    fn read_bytes_from_path(
        &self,
        buf: &mut Vec<u8>,
        p: &Path,
        offset: usize,
        length: Option<usize>,
    ) -> TractResult<()> {
        if p != Path::new(VIRTUAL_DIR).join(DATA_NAME) {
            return Err(TractError::msg("the graph names other weights"));
        }
        let end = match length {
            Some(length) => offset.checked_add(length),
            None => Some(self.0.len()),
        };
        let Some(range) = end.filter(|end| offset <= *end && *end <= self.0.len()) else {
            return Err(TractError::msg("external data range is out of bounds"));
        };
        buf.extend_from_slice(&self.0[offset..range]);
        Ok(())
    }
}

pub struct Model {
    plan: Arc<TypedRunnableModel>,
}

impl Model {
    /// `graph` is `yamnet.onnx`; its weights are `yamnet.data` beside it.
    /// Both must match their pinned size and SHA256 before parsing.
    pub fn load(graph: &Path) -> Result<Self, String> {
        let graph_bytes = read_pinned(graph, GRAPH_BYTES, GRAPH_SHA256)?;
        let data = read_pinned(&graph.with_file_name(DATA_NAME), DATA_BYTES, DATA_SHA256)?;
        let said = |error: TractError| format!("{}: {error}", graph.display());
        let onnx = tract_onnx::Onnx {
            provider: Arc::new(PinnedData(data)),
            ..tract_onnx::onnx()
        };
        let proto = onnx
            .proto_model_for_read(&mut graph_bytes.as_slice())
            .map_err(said)?;
        let ParseResult {
            model,
            unresolved_inputs,
            ..
        } = onnx.parse(&proto, Some(VIRTUAL_DIR)).map_err(said)?;
        if !unresolved_inputs.is_empty() {
            return Err(format!("{}: unresolved inputs", graph.display()));
        }
        let plan = model
            .into_optimized()
            .and_then(|model| model.into_runnable())
            .map_err(said)?;
        Ok(Self { plan })
    }

    pub fn score(&self, features: &[f32]) -> Result<SoundScores, String> {
        let input = Tensor::from_shape(&[1, 1, FRAMES, MELS], features)
            .map_err(|error| error.to_string())?;
        let output = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|error| error.to_string())?;
        let view = output[0]
            .to_plain_array_view::<f32>()
            .map_err(|error| error.to_string())?;
        if view.shape() != [1, CLASSES] || !view.iter().all(|x| x.is_finite()) {
            return Err("invalid classifier output".into());
        }
        let logits = view
            .as_slice()
            .ok_or("classifier output is not contiguous")?;
        Ok(SoundScores {
            speech: sigmoid(logits[SPEECH]),
            music: sigmoid(logits[MUSIC]),
            // PARKED (#1669): no skill reads it since the laugh glance was
            // dropped (the owner, 21 Oct); kept, it is one more sigmoid.
            laughter: sigmoid(logits[LAUGHTER]),
            // Guard mode's arrivals by sound (#2562), read only with
            // LAMP_GUARD_SOUNDS=on in lamp-skills.
            door: door(logits),
            footsteps: sigmoid(logits[FOOTSTEPS]),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Era {
    epoch: u64,
    fence: u64,
    next_sequence: u64,
}

/// A patch ready to score, with its source window on the capture clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patch {
    pub bounds: Bounds,
    pub start_us: u64,
    pub end_us: u64,
    pub capture_epoch: u64,
    pub fence: u64,
    /// A block of it was captured while the lamp was speaking (#1757's read).
    pub own: bool,
}

/// The samples of one uninterrupted capture era, from the next patch's start.
/// Whole blocks are kept, so a block's read-completion time stays with it.
#[derive(Default)]
pub struct Window {
    samples: Vec<f32>,
    times: VecDeque<u64>,
    /// Beside each block's time, whether the lamp was speaking as it came.
    own: VecDeque<bool>,
    base: usize,
    first_frame: usize,
    era: Option<Era>,
}

impl Drop for Window {
    fn drop(&mut self) {
        self.purge();
    }
}

fn current(gate: &PublicationGate, epoch: u64, fence: u64, now_us: u64) -> bool {
    // Same epoch and fence with a live lease now: no gap, mute or expiry has
    // closed this era since its samples were admitted.
    gate.allows(
        Permit {
            epoch,
            until_us: u64::MAX,
        },
        fence,
        now_us,
    )
}

impl Window {
    pub fn purge(&mut self) -> bool {
        self.samples.fill(0.0);
        self.samples.clear();
        self.times.clear();
        self.own.clear();
        self.base = 0;
        self.first_frame = 0;
        self.era.take().is_some()
    }

    pub fn held(&self) -> usize {
        self.samples.len()
    }

    /// Warming up: an era began and has not yet completed its first patch.
    pub fn warming(&self) -> bool {
        self.era.is_some() && self.first_frame == 0
    }

    /// Takes a block; returns why an earlier window was purged, if one was.
    /// A block after a gap begins a new era; it never extends the old one.
    pub fn push(
        &mut self,
        block: &Block,
        gate: &PublicationGate,
        now_us: u64,
    ) -> Option<SoundUnavailable> {
        let stamp = block.stamp;
        let mut purged = self.maintain(gate, now_us);
        if let Some(era) = self.era {
            let reason = if era.epoch != stamp.permit.epoch || era.fence != stamp.fence {
                Some(SoundUnavailable::Privacy)
            } else if era.next_sequence != stamp.sequence {
                Some(SoundUnavailable::DeviceGap)
            } else {
                None
            };
            if reason.is_some() {
                self.purge();
                purged = purged.or(reason);
            }
        }
        if stamp.captured_at_us > now_us || !current(gate, stamp.permit.epoch, stamp.fence, now_us)
        {
            self.purge();
            return purged.or(Some(SoundUnavailable::Privacy));
        }
        let Some(next_sequence) = stamp.sequence.checked_add(1) else {
            self.purge();
            return purged.or(Some(SoundUnavailable::DeviceGap));
        };
        self.era = Some(Era {
            epoch: stamp.permit.epoch,
            fence: stamp.fence,
            next_sequence,
        });
        self.samples
            .extend(block.samples.iter().map(|&x| f32::from(x) / 32768.0));
        self.times.push_back(stamp.captured_at_us);
        self.own.push_back(block.own);
        if self.samples.len() > RETAIN {
            self.purge();
            return purged.or(Some(SoundUnavailable::Overloaded));
        }
        purged
    }

    /// Purges a window whose authority lapsed or whose oldest sample passed
    /// the two-second raw age bound. Run on every worker wake.
    pub fn maintain(&mut self, gate: &PublicationGate, now_us: u64) -> Option<SoundUnavailable> {
        let era = self.era?;
        let reason = if !current(gate, era.epoch, era.fence, now_us) {
            SoundUnavailable::Privacy
        } else if self.times.front().is_some_and(|&at| {
            now_us.saturating_sub(at.saturating_sub(BLOCK_US)) > SOUND_RAW_AGE_US
        }) {
            SoundUnavailable::Overloaded
        } else {
            return None;
        };
        self.purge();
        Some(reason)
    }

    pub fn ready(&self) -> Option<Patch> {
        let era = self.era?;
        let bounds = bounds(self.first_frame);
        if bounds.end_sample > self.base + self.samples.len() {
            return None;
        }
        let first = (bounds.start_sample - self.base) / BLOCK;
        let last = (bounds.end_sample - 1 - self.base) / BLOCK;
        Some(Patch {
            bounds,
            start_us: self.times[first].saturating_sub(BLOCK_US),
            end_us: self.times[last],
            capture_epoch: era.epoch,
            fence: era.fence,
            own: self.own.range(first..=last).any(|&own| own),
        })
    }

    pub fn features(&self, frontend: &Frontend, patch: &Patch) -> Result<Vec<f32>, String> {
        frontend.features(&self.samples, self.base, patch.bounds)
    }

    /// Drops every whole block before the next patch's start.
    pub fn advance(&mut self) {
        self.first_frame += PATCH_HOP;
        let start = bounds(self.first_frame).start_sample;
        let blocks = (start.saturating_sub(self.base) / BLOCK).min(self.times.len());
        self.samples[..blocks * BLOCK].fill(0.0);
        self.samples.drain(..blocks * BLOCK);
        self.times.drain(..blocks);
        self.own.drain(..blocks);
        self.base += blocks * BLOCK;
    }
}

/// A message for the pump, with the fence its scores were admitted under.
pub struct Classified {
    pub classes: SoundClasses,
    pub fence: u64,
}

impl Classified {
    /// The pump's last check before publishing: scores leave only while their
    /// era still holds authority now and inside the two-second age bound.
    pub fn publishable(&self, gate: &PublicationGate, now_us: u64) -> bool {
        match &self.classes {
            SoundClasses::Unavailable { .. } => true,
            SoundClasses::Scored { window, .. } => {
                current(gate, window.capture_epoch, self.fence, now_us)
                    && self.classes.check_at(now_us).is_ok()
            }
        }
    }
}

/// When to say the topic is unavailable: at once after scores, otherwise
/// every 500 ms (the topic's period), and at most every 100 ms.
struct Status {
    reason: SoundUnavailable,
    sent_us: Option<u64>,
    scored: bool,
}

impl Status {
    fn since(&self, now_us: u64) -> u64 {
        self.sent_us
            .map_or(u64::MAX, |at| now_us.saturating_sub(at))
    }

    fn send(&mut self, now_us: u64, send: &mut impl FnMut(Classified)) {
        self.sent_us = Some(now_us);
        self.scored = false;
        send(Classified {
            classes: SoundClasses::Unavailable {
                reason: self.reason,
            },
            fence: 0,
        });
    }

    fn purged(&mut self, reason: SoundUnavailable, now_us: u64, send: &mut impl FnMut(Classified)) {
        self.reason = reason;
        if self.scored || self.since(now_us) >= 100_000 {
            self.send(now_us, send);
        }
    }

    fn idle(&mut self, now_us: u64, send: &mut impl FnMut(Classified)) {
        // A scored era overdue by a whole patch is behind: overloaded.
        let due = if self.scored { 1_000_000 } else { 500_000 };
        if self.since(now_us) >= due {
            if self.scored {
                self.reason = SoundUnavailable::Overloaded;
            }
            self.send(now_us, send);
        }
    }
}

/// The worker thread's loop. `score` is `None` for a model that failed its
/// pins: then no block is retained and the topic says `invalid_model`.
/// Ends on `stop`, when capture drops its [`Handoff`], or when `yes`, asked
/// every [`YES_EVERY_US`], no longer holds: then the window is dropped and
/// the topic says `privacy` before the thread ends.
pub fn serve(
    intake: Intake,
    gate: &PublicationGate,
    stop: &AtomicBool,
    mut now: impl FnMut() -> u64,
    mut score: Option<impl FnMut(&[f32]) -> Result<SoundScores, String>>,
    mut send: impl FnMut(Classified),
    mut yes: impl FnMut() -> bool,
) {
    let frontend = Frontend::new();
    let mut window = Window::default();
    let mut status = Status {
        reason: if score.is_some() {
            SoundUnavailable::WarmingUp
        } else {
            SoundUnavailable::InvalidModel
        },
        sent_us: None,
        scored: false,
    };
    let mut heard_us = now();
    let mut asked_us = heard_us;
    while !stop.load(Ordering::Acquire) {
        let received = intake.receiver.recv_timeout(Duration::from_millis(20));
        let at = now();
        if at.saturating_sub(asked_us) >= YES_EVERY_US {
            asked_us = at;
            if !yes() {
                window.purge();
                status.reason = SoundUnavailable::Privacy;
                status.send(at, &mut send);
                return;
            }
        }
        match received {
            Ok(block) if score.is_some() => {
                heard_us = at;
                if intake.overflowed.swap(false, Ordering::AcqRel) && window.purge() {
                    status.purged(SoundUnavailable::Overloaded, at, &mut send);
                }
                if let Some(reason) = window.push(&block, gate, at) {
                    status.purged(reason, at, &mut send);
                }
                if window.warming() {
                    status.reason = SoundUnavailable::WarmingUp;
                }
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if at.saturating_sub(heard_us) >= SILENT_US {
                    // No blocks and no live lease: closed for privacy, as
                    // for a lamp muted from the start (Third on #1923).
                    let reason = window.maintain(gate, at).unwrap_or(if gate.live(at) {
                        SoundUnavailable::DeviceGap
                    } else {
                        SoundUnavailable::Privacy
                    });
                    if window.purge() || reason != SoundUnavailable::DeviceGap {
                        status.purged(reason, at, &mut send);
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Some(reason) = window.maintain(gate, at) {
            status.purged(reason, at, &mut send);
        }
        if let (Some(score), Some(patch)) = (score.as_mut(), window.ready()) {
            let scores = window.features(&frontend, &patch).and_then(|mut features| {
                // A log-mel patch is about a second of the room, roughly
                // invertible: zeroed once scored (the reviewer on #1923).
                let scored = score(&features);
                features.fill(0.0);
                scored
            });
            window.advance();
            let completed_us = now();
            match scores {
                Ok(scores) => {
                    status.sent_us = Some(completed_us);
                    status.scored = true;
                    send(Classified {
                        classes: SoundClasses::Scored {
                            model_id: MODEL_ID.into(),
                            window: SoundWindow {
                                start_us: patch.start_us,
                                end_us: patch.end_us,
                                completed_us,
                                capture_epoch: patch.capture_epoch,
                            },
                            scores,
                            // A patch the lamp spoke into is its own sound
                            // (Laugh glance's guard, docs/skills.md). One it
                            // didn't is still no evidence of a quiet lamp:
                            // ears' speaking flag maps a stale voice state to
                            // false, so it stays unknown.
                            own_voice: if patch.own {
                                OwnVoiceEvidence::Present
                            } else {
                                OwnVoiceEvidence::Unknown
                            },
                        },
                        fence: patch.fence,
                    });
                }
                Err(_) => {
                    window.purge();
                    status.purged(SoundUnavailable::InferenceFailed, completed_us, &mut send);
                }
            }
        }
        status.idle(now(), &mut send);
    }
}

/// The pinned model at `LAMP_EARS_CLASSES_MODEL`, or why there is none.
pub fn model_from(path: Option<&str>) -> Result<Model, String> {
    let path = path
        .filter(|path| Path::new(path).is_absolute())
        .map(PathBuf::from)
        .ok_or("LAMP_EARS_CLASSES_MODEL is not an absolute path to yamnet.onnx")?;
    Model::load(&path)
}

#[cfg(test)]
mod tests;
