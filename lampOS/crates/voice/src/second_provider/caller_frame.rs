//! Sensor 6 replay on the simple voice route. Only an isolated, spoken wake
//! supplies a reference. Nothing in this module returns an admission decision.
mod child;

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::Duration,
};

use tokio::sync::{mpsc as queue, watch};

pub(crate) const SWITCH: &str = "LAMP_CALLER_FRAME_VAD";
pub(crate) const MODEL: &str = "LAMP_CALLER_FRAME_MODEL";
const RATE: usize = 16_000;
const REFERENCE_SAMPLES: usize = 3 * RATE;
const LINE_SECONDS: u64 = 12;
const LINE_US: u64 = LINE_SECONDS * 1_000_000;
const LINE_SAMPLES: usize = LINE_SECONDS as usize * RATE;
// A stopped input or a stalled transcript cannot keep raw PCM indefinitely.
const RESULT_GRACE_US: u64 = 20_000_000;
const WINDOW_US: u64 = crate::follows_on::WITHIN_US;
pub(super) type Shared = Arc<Mutex<Trial>>;
pub(crate) type WakeSamples = Arc<OnceLock<Option<Range<usize>>>>;

worker_global! {
    fn reset_epoch() -> AtomicU64 = AtomicU64::new(0);
}

// A forget or conversation close can run outside the provider's input loop.
pub(crate) fn forgotten() {
    reset_epoch().fetch_add(1, Ordering::AcqRel);
}

#[derive(Clone, Copy)]
pub(super) enum LineKind {
    SpokenCall,
    OtherCall,
    FollowUp,
    PlaybackOnly,
    Other,
}

struct Capture {
    request: u64,
    samples: Option<Arc<Vec<i16>>>,
    ended: bool,
    wake: Option<Arc<[i16]>>,
    began_us: u64,
    submitted_us: u64,
}

struct Reference {
    request: u64,
    samples: Arc<[i16]>,
    until_us: u64,
    answer: u64,
    opened: bool,
    answer_by_us: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Observation {
    generation: u64,
    caller: Option<u64>,
    request: u64,
    capture_end_us: Option<u64>,
    queued_us: Option<u64>,
}

struct Pair {
    observation: Observation,
    reference: Arc<[i16]>,
    line: Arc<Vec<i16>>,
}

#[derive(Debug)]
struct ResultRow {
    observation: Observation,
    outcome: &'static str,
    score: Option<f32>,
    ready_us: Option<u64>,
    verdict_us: Option<u64>,
}

struct Pending {
    observation: Observation,
    verdict_us: Option<u64>,
    ready: Option<ResultRow>,
}

pub(super) struct Trial {
    jobs: queue::Sender<Pair>,
    results: queue::Receiver<ResultRow>,
    generation: watch::Sender<u64>,
    prewarm: watch::Sender<Option<u64>>,
    rows: SyncSender<ResultRow>,
    epoch: &'static AtomicU64,
    seen_epoch: u64,
    reference: Option<Reference>,
    capture: Option<Capture>,
    pending: Option<Pending>,
}

impl Trial {
    pub(super) fn from_environment() -> Option<Shared> {
        let model = PathBuf::from(std::env::var_os(MODEL)?);
        if !enabled(std::env::var(SWITCH).ok().as_deref(), &model) {
            return None;
        }
        let program = crate::process::own_binary()
            .ok()?
            .with_file_name("lamp-same-voice");
        if !program.is_file() {
            return None;
        }
        let (jobs, receiver) = queue::channel(1);
        let (told, results) = queue::channel(1);
        let (generation, invalidated) = watch::channel(0);
        let (prewarm, warming) = watch::channel(None);
        let (rows, journal) = mpsc::sync_channel::<ResultRow>(32);
        std::thread::Builder::new()
            .name("caller-frame-journal".into())
            .spawn(move || {
                let clock_id = lamp_bus::kernel_boot_id();
                let worker_epoch = format!("{}-{}", std::process::id(), lamp_ipc::monotonic_us());
                while let Ok(row) = journal.recv() {
                    let o = row.observation;
                    eprintln!(
                        "lamp-voice caller-frame: {}",
                        serde_json::json!({
                            "clock_id": clock_id, "worker_epoch": worker_epoch,
                            "caller_request_id": o.caller.map(|id| id.to_string()), "request_id": o.request.to_string(),
                            "capture_end_us": o.capture_end_us, "queued_us": o.queued_us,
                            "ready_us": row.ready_us, "simple_verdict_us": row.verdict_us,
                            "output": row.outcome, "score": row.score
                        })
                    );
                }
            })
            .ok()?;
        // Model spawning, pipe conversion and timeouts all run off the voice reactor.
        std::thread::Builder::new()
            .name("caller-frame-model".into())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(child::run(
                        program,
                        model,
                        receiver,
                        told,
                        invalidated,
                        warming,
                    ));
                }
            })
            .ok()?;
        Some(Arc::new(Mutex::new(Self::new(
            jobs, results, generation, prewarm, rows,
        ))))
    }

    fn new(
        jobs: queue::Sender<Pair>,
        results: queue::Receiver<ResultRow>,
        generation: watch::Sender<u64>,
        prewarm: watch::Sender<Option<u64>>,
        rows: SyncSender<ResultRow>,
    ) -> Self {
        let epoch = reset_epoch();
        Self {
            jobs,
            results,
            generation,
            prewarm,
            rows,
            epoch,
            seen_epoch: epoch.load(Ordering::Acquire),
            reference: None,
            capture: None,
            pending: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.capture = None;
        self.discard_reference();
    }

    fn discard_reference(&mut self) {
        self.reference = None;
        self.pending = None;
        self.generation
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    fn observe_epoch(&mut self) {
        let epoch = self.epoch.load(Ordering::Acquire);
        if epoch != self.seen_epoch {
            self.clear();
            self.seen_epoch = epoch;
        }
    }

    // A computer opening its turn always supersedes a prior caller; it cannot
    // revive or retire a newer microphone request if its control arrives late.
    pub(super) fn computer_opened(&mut self, request: u64) {
        if self
            .capture
            .as_ref()
            .is_none_or(|line| line.request <= request)
            && self.reference.as_ref().is_none_or(|r| r.request <= request)
        {
            self.clear();
        }
    }

    pub(super) fn begin(&mut self, request: u64, clean: bool, now: u64) {
        self.tick(now);
        self.capture = None;
        self.expire(now);
        self.cancel_pending();
        self.capture = Some(Capture {
            request,
            samples: clean.then(|| Arc::new(Vec::new())),
            ended: false,
            began_us: now,
            submitted_us: 0,
            wake: None,
        });
    }

    pub(super) fn audio(&mut self, request: u64, samples: &[i16], fresh: bool, end_us: u64) {
        self.observe_epoch();
        if let Some(line) = self
            .capture
            .as_mut()
            .filter(|line| line.request == request && !line.ended)
        {
            let advancing = end_us != 0 && end_us > line.submitted_us;
            line.submitted_us = end_us;
            if let Some(kept) = line.samples.as_mut().and_then(Arc::get_mut) {
                if fresh
                    && advancing
                    && !samples.is_empty()
                    && samples.len() <= LINE_SAMPLES - kept.len()
                {
                    kept.extend_from_slice(samples);
                } else {
                    line.samples = None;
                }
            }
        }
    }

    // The final PCM block was sent to the existing speech listener. Queue the
    // standalone model immediately, without waiting for transcripts or admission.
    pub(super) fn pcm_ended(&mut self, request: u64, began_us: u64, now: u64) {
        self.observe_epoch();
        let Some(line) = self
            .capture
            .as_mut()
            .filter(|line| line.request == request && !line.ended)
        else {
            return;
        };
        line.ended = true;
        if began_us == 0 || line.submitted_us < began_us || now < line.submitted_us {
            line.samples = None;
            line.submitted_us = 0;
            return;
        }
        line.began_us = began_us;
        let Some(reference) = self
            .reference
            .as_ref()
            .filter(|r| r.opened && began_us <= r.until_us)
        else {
            return;
        };
        let Some(samples) = line.samples.as_ref().filter(|samples| !samples.is_empty()) else {
            return;
        };
        let observation = Observation {
            generation: *self.generation.borrow(),
            caller: Some(reference.request),
            request,
            capture_end_us: (line.submitted_us > 0).then_some(line.submitted_us),
            queued_us: Some(now),
        };
        let job = Pair {
            observation,
            reference: reference.samples.clone(),
            line: samples.clone(),
        };
        if self.jobs.try_send(job).is_ok() {
            self.pending = Some(Pending {
                observation,
                verdict_us: None,
                ready: None,
            });
        }
    }

    pub(super) fn words(
        &mut self,
        request: u64,
        words: Option<&str>,
        aligned: Option<Range<usize>>,
    ) {
        self.observe_epoch();
        if let Some(line) = self
            .capture
            .as_mut()
            .filter(|line| line.request == request && line.ended)
        {
            // Release the rest of a compound utterance on the speech task.
            // Admission only moves the already-isolated wake into the window.
            line.wake = line.samples.take().and_then(|samples| {
                let wake = wake_audio(&samples, words?, aligned)?;
                Some(Arc::from(&samples[wake]))
            });
        }
    }

    pub(super) fn judged(&mut self, request: u64, kind: LineKind, began_us: u64, now: u64) {
        self.observe_epoch();
        let Some(line) = self.capture.take_if(|line| line.request == request) else {
            return;
        };
        match kind {
            LineKind::SpokenCall | LineKind::OtherCall => {
                // The model retains only its weights between independent pairs.
                self.discard_reference();
                if matches!(kind, LineKind::SpokenCall)
                    && line.ended
                    && now <= line.began_us.saturating_add(LINE_US + RESULT_GRACE_US)
                    && let Some(wake) = line.wake
                {
                    self.reference = Some(Reference {
                        request,
                        samples: wake,
                        answer: request,
                        opened: false,
                        until_us: now.saturating_add(WINDOW_US),
                        answer_by_us: Some(now.saturating_add(RESULT_GRACE_US)),
                    });
                    // Only weights may be loaded at enrollment. The separate
                    // notification can never take the slot for a real PCM pair.
                    self.prewarm.send_replace(Some(*self.generation.borrow()));
                }
            }
            LineKind::FollowUp => {
                let reference = self
                    .reference
                    .as_mut()
                    .filter(|r| r.opened && began_us <= r.until_us);
                let caller = reference.as_deref().map(|r| r.request);
                if let Some(r) = reference {
                    r.answer = request;
                    r.answer_by_us = Some(now.saturating_add(RESULT_GRACE_US));
                }
                if let Some(pending) = self
                    .pending
                    .as_mut()
                    .filter(|p| p.observation.request == request && p.observation.caller == caller)
                {
                    pending.verdict_us = Some(now);
                    self.publish_ready();
                } else {
                    self.cancel_pending();
                    let _ = self.rows.try_send(ResultRow {
                        observation: Observation {
                            generation: *self.generation.borrow(),
                            caller,
                            request,
                            capture_end_us: (line.submitted_us > 0).then_some(line.submitted_us),
                            queued_us: None,
                        },
                        outcome: "unknown",
                        score: None,
                        ready_us: None,
                        verdict_us: Some(now),
                    });
                }
            }
            LineKind::PlaybackOnly => {
                // The base also answers a pending confirmation or a continuation
                // of an opening call. Its playback can keep the original caller
                // current without classifying those lines.
                if let Some(reference) = self.reference.as_mut().filter(|r| began_us <= r.until_us)
                {
                    reference.answer = request;
                    reference.answer_by_us = Some(now.saturating_add(RESULT_GRACE_US));
                }
                self.cancel_pending();
            }
            LineKind::Other => self.cancel_pending(),
        }
    }

    fn cancel_pending(&mut self) {
        if self.pending.take().is_some() {
            self.generation
                .send_modify(|value| *value = value.wrapping_add(1));
        }
    }

    fn publish_ready(&mut self) {
        if let Some(pending) = self
            .pending
            .take_if(|p| p.verdict_us.is_some() && p.ready.is_some())
            && let Some(mut row) = pending.ready
        {
            row.verdict_us = pending.verdict_us;
            let _ = self.rows.try_send(row);
        }
    }

    pub(super) fn answered(&mut self, request: u64, until_us: u64, now: u64) {
        self.observe_epoch();
        if let Some(reference) = self.reference.as_mut().filter(|r| {
            r.answer == request
                && (now <= r.until_us || r.answer_by_us.is_some_and(|end| now <= end))
        }) {
            reference.until_us = reference.until_us.max(until_us.saturating_add(WINDOW_US));
            reference.answer_by_us = None;
            reference.opened = true;
        }
    }

    fn expire(&mut self, now: u64) {
        if self
            .capture
            .as_ref()
            .is_some_and(|line| now > line.began_us.saturating_add(LINE_US + RESULT_GRACE_US))
        {
            self.capture = None;
            self.cancel_pending();
        }
        if self.reference.as_ref().is_some_and(|r| {
            now > r.until_us
                && self
                    .capture
                    .as_ref()
                    .is_none_or(|line| line.began_us > r.until_us)
                && r.answer_by_us.is_none_or(|end| now > end)
                && self.pending.as_ref().is_none_or(|p| {
                    p.observation
                        .queued_us
                        .is_none_or(|queued| now > queued.saturating_add(RESULT_GRACE_US))
                })
        }) {
            self.discard_reference();
        }
    }

    pub(super) fn tick(&mut self, now: u64) {
        self.observe_epoch();
        self.expire(now);
        while let Ok(result) = self.results.try_recv() {
            if let Some(pending) = self
                .pending
                .as_mut()
                .filter(|p| p.observation == result.observation)
            {
                pending.ready = Some(result);
                self.publish_ready();
            }
        }
    }
}

fn enabled(switch: Option<&str>, model: &Path) -> bool {
    switch == Some("on") && model.is_absolute() && model.join("caller-frame.f32").is_file()
}

fn wake_audio(pcm: &[i16], words: &str, range: Option<Range<usize>>) -> Option<Range<usize>> {
    let mut words = words
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty());
    if !words.next()?.eq_ignore_ascii_case("hey") || !words.next()?.eq_ignore_ascii_case("lamp") {
        return None;
    }
    let wake = match range {
        Some(range) if pcm.get(range.clone()).is_some() => range,
        Some(_) => return None,
        None if words.next().is_none() => 0..pcm.len(),
        None => return None,
    };
    (!wake.is_empty() && wake.len() <= REFERENCE_SAMPLES).then_some(wake)
}

// Only the first final segment can supply the opening two words. The normal
// recognizer timestamps start at the first PCM sample sent to this listener.
pub(super) fn wake_range(result: &str) -> Option<Range<usize>> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    let words = value.pointer("/channel/alternatives/0/words")?.as_array()?;
    let hey = words.first()?;
    let lamp = words.get(1)?;
    if !hey["word"].as_str()?.eq_ignore_ascii_case("hey")
        || !lamp["word"].as_str()?.eq_ignore_ascii_case("lamp")
    {
        return None;
    }
    let start = hey["start"].as_f64()?;
    let mid = hey["end"].as_f64()?;
    let next = lamp["start"].as_f64()?;
    let end = lamp["end"].as_f64()?;
    if !(0.0 <= start && start < mid && mid <= next && next < end && end <= 12.0) {
        return None;
    }
    let range = ((start * RATE as f64).ceil() as usize)..((end * RATE as f64).floor() as usize);
    (!range.is_empty() && range.len() <= REFERENCE_SAMPLES).then_some(range)
}

#[cfg(test)]
pub(super) fn with_negative_model() -> (Shared, impl FnMut() -> Option<(u64, u64)>) {
    let (jobs, mut taken) = queue::channel::<Pair>(1);
    let (told, results) = queue::channel(1);
    let (generation, _invalidated) = watch::channel(0);
    let (prewarm, _) = watch::channel(None);
    let (rows, _journal) = mpsc::sync_channel(1);
    let model = move || {
        let job = taken.try_recv().ok()?;
        let o = job.observation;
        told.try_send(ResultRow {
            observation: o,
            outcome: "different",
            score: Some(0.26),
            ready_us: Some(o.queued_us? + 200_000),
            verdict_us: None,
        })
        .ok()?;
        Some((o.caller?, o.request))
    };
    (
        Arc::new(Mutex::new(Trial::new(
            jobs, results, generation, prewarm, rows,
        ))),
        model,
    )
}

#[cfg(test)]
mod tests;
