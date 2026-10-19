//! Request-bound voice enrollment and shadow identification (#1675).
//!
//! This is identity infrastructure, not an autonomous behaviour. Until the
//! office's real voices have been measured, a match is only a journal result:
//! it never selects a memory scope, changes a session, or produces a greeting.
//! Audio is retained only for the current admitted, quiet spoken request.
use crate::same_voice::{self, Printer};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex, Weak, mpsc},
    thread,
    time::{Duration, Instant},
};

pub const ENVIRONMENT: &str = "LAMP_VOICE_PERSON";
/// A separate, deliberately non-authoritative mode; `on` is not accepted.
pub const MODE: &str = "shadow";
/// CAM++'s output shape. The eyes validate and normalize it before persistence.
pub const DIMENSIONS: usize = 512;
/// An enrollment call must belong to the same recently ended spoken turn.
pub const FRESH: Duration = Duration::from_secs(10);
const MATCH_WITHIN: Duration = Duration::from_millis(200);
const POLL: Duration = Duration::from_millis(10);

static ACTIVE: Mutex<Weak<Shared>> = Mutex::new(Weak::new());

pub fn enabled() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some(MODE)
}

struct Clip {
    request: u64,
    samples: Vec<i16>,
    ended: Option<Instant>,
    print: Option<Vec<f32>>,
}

#[derive(Default)]
struct State {
    epoch: u64,
    clip: Option<Clip>,
}

struct Job {
    epoch: u64,
    request: u64,
    samples: Vec<i16>,
    ended: Instant,
}

struct Shared {
    state: Mutex<State>,
    jobs: mpsc::SyncSender<Job>,
}

/// Owned by the provider loop. Dropping it invalidates evidence and closes
/// the bounded work queue; no process-global strong reference keeps it alive.
pub struct Guard(Arc<Shared>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

/// Start only in shadow mode, with the existing model and eyes socket. No
/// inference, disk access or socket exchange takes place on the voice loop.
pub fn start(model: Option<&Path>, who: Option<&Path>) -> Option<Guard> {
    if !enabled() {
        return None;
    }
    let (Some(model), Some(who)) = (model, who) else {
        eprintln!("lamp-voice: voice identity shadow needs the same-voice model and eyes socket");
        return None;
    };
    let mut active = ACTIVE.lock().ok()?;
    if active.upgrade().is_some() {
        return None;
    }
    let model = model.to_owned();
    let socket = who.to_owned();
    let (shared, jobs) = Shared::new();
    let weak = Arc::downgrade(&shared);
    thread::Builder::new()
        .name("voice-person-shadow".into())
        .spawn(move || {
            if weak.upgrade().is_none() {
                return;
            }
            let Ok(printer) = same_voice::Prints::beside_voice_pinned(model) else {
                return;
            };
            work(printer, jobs, weak, move |print| {
                crate::tools::exchange(&socket, &json!({"voice_match": print}), MATCH_WITHIN)
            });
        })
        .ok()?;
    *active = Arc::downgrade(&shared);
    Some(Guard(shared))
}

impl Shared {
    fn new() -> (Arc<Self>, mpsc::Receiver<Job>) {
        let (send, receive) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                state: Mutex::new(State::default()),
                jobs: send,
            }),
            receive,
        )
    }

    fn began(&self, request: u64, eligible: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.epoch = state.epoch.wrapping_add(1);
            state.clip = eligible.then(|| Clip {
                request,
                samples: Vec::with_capacity(same_voice::MOST_SAMPLES),
                ended: None,
                print: None,
            });
        }
    }

    fn heard(&self, request: u64, samples: &[i16]) {
        if let Ok(mut state) = self.state.lock()
            && let Some(clip) = state
                .clip
                .as_mut()
                .filter(|clip| clip.request == request && clip.ended.is_none())
        {
            let room = same_voice::MOST_SAMPLES - clip.samples.len();
            clip.samples
                .extend_from_slice(&samples[..samples.len().min(room)]);
        }
    }

    fn ended(&self, request: u64, sound: bool, now: Instant) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let epoch = state.epoch;
        let Some(clip) = state.clip.as_mut().filter(|clip| clip.request == request) else {
            return;
        };
        if sound {
            state.clip = None;
            return;
        }
        if clip.ended.is_some() {
            return;
        }
        // As with the existing same-voice check, short/noisy clips remain
        // unknown. Never borrow a previous turn's print for a short yes.
        if clip.samples.len() < same_voice::LEAST_SAMPLES {
            state.clip = None;
            return;
        }
        clip.ended = Some(now);
        let job = Job {
            epoch,
            request,
            samples: std::mem::take(&mut clip.samples),
            ended: now,
        };
        if self.jobs.try_send(job).is_err() {
            state.clip = None;
        }
    }

    fn current(&self, job: &Job, now: Instant) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.epoch == job.epoch
                && state.clip.as_ref().is_some_and(|clip| {
                    clip.request == job.request
                        && clip
                            .ended
                            .is_some_and(|ended| now.duration_since(ended) <= FRESH)
                })
        })
    }

    fn completed(&self, job: &Job, print: Vec<f32>, now: Instant) -> bool {
        if !valid(&print) {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.epoch != job.epoch {
            return false;
        }
        let Some(clip) = state.clip.as_mut().filter(|clip| {
            clip.request == job.request
                && clip
                    .ended
                    .is_some_and(|ended| now.duration_since(ended) <= FRESH)
        }) else {
            return false;
        };
        clip.print = Some(print);
        true
    }

    fn enrollment(&self, request: u64, heard: bool, now: Instant) -> Option<Vec<f32>> {
        if !heard {
            return None;
        }
        let state = self.state.lock().ok()?;
        let clip = state.clip.as_ref().filter(|clip| {
            clip.request == request
                && clip
                    .ended
                    .is_some_and(|ended| now.duration_since(ended) <= FRESH)
        })?;
        clip.print.clone()
    }

    fn retire_through(&self, through: u64) {
        if let Ok(mut state) = self.state.lock()
            && state
                .clip
                .as_ref()
                .is_some_and(|clip| clip.request <= through)
        {
            state.epoch = state.epoch.wrapping_add(1);
            state.clip = None;
        }
    }

    fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.epoch = state.epoch.wrapping_add(1);
            state.clip = None;
        }
    }
}

fn active() -> Option<Arc<Shared>> {
    ACTIVE.lock().ok()?.upgrade()
}

pub fn began(request: u64, eligible: bool) {
    if let Some(shared) = active() {
        shared.began(request, eligible);
    }
}
pub fn heard(request: u64, samples: &[i16]) {
    if let Some(shared) = active() {
        shared.heard(request, samples);
    }
}
pub fn ended(request: u64, sound: bool) {
    if let Some(shared) = active() {
        shared.ended(request, sound, Instant::now());
    }
}
pub fn retire_through(through: u64) {
    if let Some(shared) = active() {
        shared.retire_through(through);
    }
}
pub fn clear() {
    if let Some(shared) = active() {
        shared.clear();
    }
}
/// Called only after the model supplied explicit consent, never with a print
/// supplied by the model. It never waits for a child or another request.
pub fn enrollment(request: u64, heard: bool) -> Option<Vec<f32>> {
    active()?.enrollment(request, heard, Instant::now())
}

fn valid(print: &[f32]) -> bool {
    print.len() == DIMENSIONS
        && print.iter().all(|value| value.is_finite())
        && print
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            > 1e-12
}

fn current(shared: &Weak<Shared>, job: &Job) -> bool {
    shared
        .upgrade()
        .is_some_and(|shared| shared.current(job, Instant::now()))
}

fn work(
    mut printer: impl Printer,
    jobs: mpsc::Receiver<Job>,
    shared: Weak<Shared>,
    mut identify: impl FnMut(&[f32]) -> Option<Value>,
) {
    while let Ok(mut job) = jobs.recv() {
        if !current(&shared, &job) {
            continue;
        }
        // Drain a cancelled request's late answer before asking for this one.
        while printer.answer().is_some() {}
        if !printer.ask(job.request, &job.samples) {
            eprintln!(
                "lamp-voice: request {}: voice identity unprinted (child unavailable)",
                job.request
            );
            continue;
        }
        job.samples.clear();
        let asked = Instant::now();
        while current(&shared, &job) && asked.elapsed() < same_voice::ANSWER_WITHIN {
            if let Some((request, result)) = printer.answer() {
                if request != job.request {
                    continue;
                }
                if let Ok(print) = result
                    && shared
                        .upgrade()
                        .is_some_and(|shared| shared.completed(&job, print.clone(), Instant::now()))
                {
                    let answer = identify(&print);
                    if current(&shared, &job) {
                        // No name, PCM or biometric vector in logs. These
                        // provisional matches never leave this diagnostic path.
                        let candidate = answer
                            .as_ref()
                            .filter(|answer| answer.get("shadow") == Some(&Value::Bool(true)))
                            .and_then(|answer| answer.get("candidate"))
                            .and_then(Value::as_str)
                            .map(crate::remember::person_id);
                        let score = answer
                            .as_ref()
                            .and_then(|a| a.get("cosine"))
                            .and_then(Value::as_f64);
                        let runner_up = answer
                            .as_ref()
                            .and_then(|a| a.get("runner_up"))
                            .and_then(Value::as_f64);
                        let matched = answer
                            .as_ref()
                            .is_some_and(|a| a.get("shadow") == Some(&Value::Bool(true)));
                        eprintln!(
                            "lamp-voice: request {}: voice identity shadow candidate={} cosine={score:?} runner_up={runner_up:?} match_available={matched} after_end_ms={}",
                            job.request,
                            candidate.as_deref().unwrap_or("unknown"),
                            job.ended.elapsed().as_millis()
                        );
                    }
                }
                break;
            }
            thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn print() -> Vec<f32> {
        let mut p = vec![0.0; DIMENSIONS];
        p[0] = 1.0;
        p
    }
    fn clip(shared: &Shared, request: u64, now: Instant) {
        shared.began(request, true);
        shared.heard(request, &vec![1; same_voice::MOST_SAMPLES + 500]);
        shared.ended(request, false, now);
    }

    #[test]
    fn only_the_same_ended_spoken_request_can_enroll_and_it_expires() {
        let (shared, jobs) = Shared::new();
        let now = Instant::now();
        clip(&shared, 1, now);
        let job = jobs.try_recv().unwrap();
        assert_eq!(job.samples.len(), same_voice::MOST_SAMPLES);
        assert!(shared.enrollment(1, true, now).is_none());
        assert!(shared.completed(&job, print(), now));
        assert!(shared.enrollment(1, true, now).is_some());
        assert!(shared.enrollment(2, true, now).is_none());
        assert!(shared.enrollment(1, false, now).is_none());
        assert!(shared.enrollment(1, true, now + FRESH + POLL).is_none());
        shared.began(2, true);
        assert!(shared.enrollment(1, true, now).is_none());
        assert!(!shared.completed(&job, print(), now));
    }

    #[test]
    fn noise_short_overlapping_and_retired_lines_never_supply_a_print() {
        let (shared, jobs) = Shared::new();
        let now = Instant::now();
        for (request, eligible, samples, sound) in [
            (1, false, same_voice::MOST_SAMPLES, false),
            (2, true, same_voice::LEAST_SAMPLES - 1, false),
            (3, true, same_voice::MOST_SAMPLES, true),
        ] {
            shared.began(request, eligible);
            shared.heard(request, &vec![1; samples]);
            shared.ended(request, sound, now);
            assert!(jobs.try_recv().is_err());
            assert!(shared.enrollment(request, true, now).is_none());
        }
        clip(&shared, 4, now);
        let job = jobs.try_recv().unwrap();
        shared.retire_through(4);
        assert!(!shared.completed(&job, print(), now));
        clip(&shared, 5, now);
        let job = jobs.try_recv().unwrap();
        shared.clear();
        assert!(!shared.completed(&job, print(), now));
    }

    #[test]
    fn busy_work_is_bounded_and_malformed_or_duplicate_results_cannot_enroll() {
        let (shared, jobs) = Shared::new();
        let now = Instant::now();
        clip(&shared, 1, now);
        clip(&shared, 2, now); // One queued: the new clip is discarded, not buffered.
        let stale = jobs.try_recv().unwrap();
        assert!(!shared.completed(&stale, print(), now));
        assert!(jobs.try_recv().is_err());
        clip(&shared, 3, now);
        shared.ended(3, false, now);
        let job = jobs.try_recv().unwrap();
        assert!(jobs.try_recv().is_err());
        for bad in [
            vec![],
            vec![0.0; DIMENSIONS],
            vec![f32::NAN; DIMENSIONS],
            vec![1.0; DIMENSIONS - 1],
        ] {
            assert!(!shared.completed(&job, bad, now));
        }
        shared.began(3, true); // A reused id has a new generation.
        assert!(!shared.completed(&job, print(), now));
    }

    struct Immediate {
        answer: Option<(u64, Result<Vec<f32>, String>)>,
    }
    impl Printer for Immediate {
        fn ask(&mut self, request: u64, _: &[i16]) -> bool {
            self.answer = Some((request, Ok(print())));
            true
        }
        fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
            self.answer.take()
        }
    }

    #[test]
    fn the_background_path_publishes_only_the_request_print_and_stops_with_its_guard() {
        let (shared, jobs) = Shared::new();
        let guard = Guard(shared.clone());
        let weak = Arc::downgrade(&shared);
        let (seen, observation) = mpsc::channel();
        let worker = thread::spawn(move || {
            work(Immediate { answer: None }, jobs, weak, |print| {
                seen.send(print.len()).unwrap();
                Some(json!({"candidate": "Example", "cosine": 1.0, "shadow": true}))
            });
        });
        let now = Instant::now();
        clip(&shared, 8, now);
        assert_eq!(
            observation.recv_timeout(Duration::from_secs(2)).unwrap(),
            DIMENSIONS
        );
        assert_eq!(shared.enrollment(8, true, Instant::now()), Some(print()));
        assert!(shared.enrollment(7, true, Instant::now()).is_none());
        drop(shared);
        drop(guard);
        worker.join().unwrap();
    }

    #[test]
    fn capture_work_does_not_wait_for_a_busy_printer_or_the_match_socket() {
        let (shared, jobs) = Shared::new();
        let guard = Guard(shared.clone());
        let weak = Arc::downgrade(&shared);
        let (entered, notified) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = thread::spawn(move || {
            work(Immediate { answer: None }, jobs, weak, |_| {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                None
            });
        });
        let now = Instant::now();
        clip(&shared, 1, now);
        notified.recv_timeout(Duration::from_secs(2)).unwrap();
        // The matcher is held on a rendezvous. Capturing and retiring must
        // still finish without releasing it, and the prior print must vanish.
        clip(&shared, 2, now);
        shared.retire_through(2);
        assert!(shared.enrollment(1, true, now).is_none());
        assert!(shared.enrollment(2, true, now).is_none());
        release.send(()).unwrap();
        drop(shared);
        drop(guard);
        worker.join().unwrap();
    }

    #[test]
    fn capture_bookkeeping_cost_is_reported_separately_from_inference() {
        let (shared, _jobs) = Shared::new();
        let samples = [1; lamp_audio::CAPTURE_SAMPLES];
        let mut starts = Vec::new();
        let mut blocks = Vec::new();
        for request in 1..=100 {
            let before = Instant::now();
            shared.began(request, true);
            starts.push(before.elapsed().as_nanos());
            for _ in 0..300 {
                let before = Instant::now();
                shared.heard(request, &samples);
                blocks.push(before.elapsed().as_nanos());
            }
        }
        starts.sort_unstable();
        blocks.sort_unstable();
        eprintln!(
            "voice identity capture bookkeeping (Mac/host only): start p50={}ns max={}ns; block p50={}ns p95={}ns max={}ns; 100 starts, 30000 blocks; excludes inference/IPC and global dispatch",
            starts[50], starts[99], blocks[15000], blocks[28500], blocks[29999]
        );
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap()
                .clip
                .as_ref()
                .unwrap()
                .samples
                .len(),
            same_voice::MOST_SAMPLES
        );
    }

    #[test]
    fn dropping_the_guard_revokes_evidence_and_disconnects_the_work_queue() {
        let (shared, jobs) = Shared::new();
        let guard = Guard(shared);
        let weak = Arc::downgrade(&guard.0);
        drop(guard);
        assert!(weak.upgrade().is_none());
        assert!(matches!(
            jobs.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
