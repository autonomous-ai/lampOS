use crate::{
    engine::{Config, Engine, Output},
    input::{InputError, PinReader},
    pace::Pace,
};
use lamp_bus::{Bus, Published, Publisher};
use lamp_msgs::{ControlFault, ControlGesture, ControlsState, Diagnostic, Level};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};

pub const POLL_US: u64 = 2_000;
pub const DRAIN_LIMIT: usize = 64;

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub accepted: u64,
    pub rejected: u64,
    pub reader_errors: u64,
    pub drain_overflows: u64,
    pub published: u64,
    pub dropped: u64,
    pub publish_errors: u64,
    pub steps: u64,
    pub longest_step_us: u64,
}

/// The shipped binary cannot pretend that hardware was fitted. First replaces
/// this adapter with one for the explicitly verified board, using this trait.
pub struct MissingPins;
impl PinReader for MissingPins {
    fn read(&mut self, _: u64) -> Result<Option<crate::input::Observation>, InputError> {
        Err(InputError::new(
            ControlFault::Missing,
            "no physical controls adapter is installed",
        ))
    }
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

pub fn clock_id(given: Option<&str>) -> io::Result<String> {
    let id = match given {
        Some(id) => id.to_owned(),
        None => fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_owned(),
    };
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    {
        return Err(invalid(
            "clock identity needs 1..128 letters, digits, '.', '_' or '-'",
        ));
    }
    Ok(id)
}

pub(crate) fn owner_lock(root: &Path) -> io::Result<File> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o770)
        .create(root)?;
    let path = root.join("controls.lock");
    if let Ok(meta) = fs::symlink_metadata(&path)
        && !meta.is_file()
    {
        return Err(invalid("controls owner lock is not a regular file"));
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.try_lock().map_err(|error| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("controls owner lock: {error}"),
        )
    })?;
    Ok(file)
}

pub struct Service<R: PinReader> {
    reader: R,
    engine: Engine,
    state: Publisher,
    gestures: Publisher,
    diagnostic: Publisher,
    last_state: Option<u64>,
    last_fault: Option<ControlFault>,
    last_report: Option<u64>,
    last_feedback: Option<Feedback>,
    pub counts: Counts,
    pub pace: Pace,
    recorder: Option<crate::record::Recorder>,
    privacy: Option<crate::privacy::Source>,
    /// The privacy route on a lamp with no slide ([`Self::mark_no_slide`]).
    no_slide: bool,
    privacy_touch_recording: bool,
    _lock: File,
}

#[derive(PartialEq, Eq)]
struct Feedback {
    buttons: Vec<(bool, bool, lamp_msgs::HoldTier)>,
    head_active: bool,
    base_ready: bool,
    base_tier: lamp_msgs::HoldTier,
    privacy: Option<(Option<bool>, Option<ControlFault>, String)>,
}

impl From<&ControlsState> for Feedback {
    fn from(state: &ControlsState) -> Self {
        Self {
            buttons: state
                .buttons
                .iter()
                .map(|button| (button.pressed, button.armed, button.tier))
                .collect(),
            head_active: state.head_active,
            base_ready: state.base_ready,
            base_tier: state.base_tier,
            privacy: state
                .privacy
                .as_ref()
                .map(|value| (value.muted, value.fault, value.reader_session_id.clone())),
        }
    }
}

/// The `controls/state` publisher every serve mode publishes on, the plain
/// runtime's and `--touch`'s alike. A state's `at_us` is its stamp's,
/// CLOCK_MONOTONIC of the boot named by `clock`. When that is this boot's,
/// the header says so, and the voice judges the privacy state by it and not
/// by a wall clock that chrony steps some 46 s into a cold boot (queue
/// #3997, C).
pub(crate) fn state_publisher(bus: &Bus, clock: &str) -> io::Result<Publisher> {
    let mut state = bus.publisher_declared(
        ControlsState::TOPIC,
        &ControlsState::declaration(),
        "controls",
    )?;
    if bus.this_boot() == Some(clock) {
        state.stamp_monotonic_clock();
    }
    Ok(state)
}

impl<R: PinReader> Service<R> {
    pub fn new(bus: &Bus, config: Config, reader: R, clock: &str) -> io::Result<Self> {
        config.check().map_err(|e| invalid(e.to_string()))?;
        let lock = owner_lock(bus.root())?;
        let state = state_publisher(bus, clock)?;
        let engine =
            Engine::new(config, clock, state.boot()).map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            reader,
            engine,
            state,
            gestures: bus.publisher_declared(
                ControlGesture::TOPIC,
                &ControlGesture::declaration(),
                "controls",
            )?,
            diagnostic: bus.publisher_declared(
                Diagnostic::TOPIC,
                &Diagnostic::declaration(),
                "controls",
            )?,
            last_state: None,
            last_fault: None,
            last_report: None,
            last_feedback: None,
            counts: Counts::default(),
            pace: Pace::default(),
            recorder: None,
            privacy: None,
            no_slide: false,
            privacy_touch_recording: false,
            _lock: lock,
        })
    }

    pub fn record_to(&mut self, path: &Path) -> io::Result<()> {
        if self.recorder.is_some() {
            return Err(invalid("an acquisition recorder is already attached"));
        }
        self.recorder = Some(crate::record::Recorder::create(path)?);
        Ok(())
    }

    pub fn attach_privacy(&mut self, source: crate::privacy::Source) {
        self.privacy = Some(source);
    }

    /// The privacy route on a lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`):
    /// nothing is read, and each state says so (`no_privacy_slide`) at the
    /// route's cadence, as when a slide reader is attached. Refused beside a
    /// slide reader; a reader attached later wins, and no marker is said.
    pub fn mark_no_slide(&mut self) -> io::Result<()> {
        if self.privacy.is_some() {
            return Err(invalid(
                "a lamp with a slide reader is not a lamp with no slide",
            ));
        }
        self.no_slide = true;
        Ok(())
    }

    /// The privacy route is on: a slide reader, or a lamp with no slide.
    fn privacy_route(&self) -> bool {
        self.privacy.is_some() || self.no_slide
    }

    /// The --touch privacy-only route records the tagged touch envelope without
    /// manufacturing GPIO or strip acquisitions for absent hand hardware.
    pub fn record_privacy_touch_to(&mut self, path: &Path) -> io::Result<()> {
        let config = self.engine.config();
        if self.recorder.is_some()
            || self.privacy.is_none()
            || !config.buttons.is_empty()
            || !config.head_pads.is_empty()
            || config.base_selected_mask.is_some()
        {
            return Err(invalid(
                "touch privacy recording requires only an attached privacy source",
            ));
        }
        self.recorder = Some(crate::record::Recorder::create_touch(path)?);
        self.privacy_touch_recording = true;
        Ok(())
    }

    pub fn poll_us(&self) -> u64 {
        if self.privacy_route() {
            crate::privacy::STEP_US
        } else {
            POLL_US
        }
    }

    pub fn finish_recording(&mut self) -> Option<crate::record::RecordStats> {
        self.recorder.take().map(crate::record::Recorder::finish)
    }

    /// [`Self::finish_recording`] with the writer's finish window given
    /// ([`crate::record::Recorder::finish_within`]), for a test.
    pub fn finish_recording_within(
        &mut self,
        window: std::time::Duration,
    ) -> Option<crate::record::RecordStats> {
        self.recorder
            .take()
            .map(|recorder| recorder.finish_within(window))
    }

    fn record(
        &mut self,
        now_us: u64,
        observation: Option<crate::input::Observation>,
        fault: Option<ControlFault>,
    ) {
        if self.privacy_touch_recording {
            return;
        }
        if let Some(recorder) = &mut self.recorder {
            recorder.submit(&crate::replay::Record {
                now_us,
                observation,
                fault,
                privacy: None,
                privacy_fault: None,
            });
        }
    }

    /// A bounded drain; obtain time again after reading so an unexpectedly
    /// slow adapter cannot make its old samples fresh by taking time to read.
    pub fn step(&mut self, now: impl Fn() -> u64) -> Output {
        let start = now();
        if let Some(line) = self.pace.step(start) {
            eprintln!("lamp-controls: {line}");
        }
        if let Some(privacy) = &mut self.privacy
            && let Some((received, sample)) = privacy.poll(&now)
            && let Some(recorder) = &mut self.recorder
        {
            if self.privacy_touch_recording {
                recorder.submit_touch(&crate::touch_replay::Record {
                    now_us: received,
                    input: crate::touch_replay::Input::Privacy { sample },
                });
            } else {
                recorder.submit(&crate::replay::Record {
                    now_us: received,
                    observation: None,
                    fault: None,
                    privacy: Some(sample),
                    privacy_fault: None,
                });
            }
        }
        let mut drained = 0;
        for _ in 0..DRAIN_LIMIT {
            match self.reader.read(now()) {
                Ok(Some(sample)) => {
                    drained += 1;
                    let received = now();
                    if sample.check().is_ok() {
                        self.record(received, Some(sample.clone()), None);
                    } else {
                        self.record(received, None, Some(ControlFault::Invalid));
                    }
                    if self.engine.ingest(received, sample).is_ok() {
                        self.counts.accepted += 1;
                    } else {
                        self.counts.rejected += 1;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    self.counts.reader_errors += 1;
                    let received = now();
                    self.record(received, None, Some(error.fault));
                    self.engine.input_fault(received, error.fault);
                    break;
                }
            }
        }
        if drained == DRAIN_LIMIT {
            self.counts.drain_overflows += 1;
            let received = now();
            self.record(received, None, Some(ControlFault::Overflow));
            self.engine.input_fault(received, ControlFault::Overflow);
        }
        let at_us = now();
        let mut output = self.engine.tick(at_us);
        if let Some(privacy) = &mut self.privacy {
            output.state.privacy = Some(privacy.monitor.snapshot(at_us));
        } else if self.no_slide {
            output.state.no_privacy_slide = true;
        }
        let feedback = Feedback::from(&output.state);
        if !output.gestures.is_empty()
            || self.last_state.is_none_or(|at| {
                at_us.saturating_sub(at)
                    >= if self.privacy_route() {
                        crate::privacy::STATE_EVERY_US
                    } else {
                        100_000
                    }
            })
            || self.last_fault != output.state.fault
            || self.last_feedback.as_ref() != Some(&feedback)
        {
            // Publish the session-bearing retained state before advisory
            // events. Receivers still handle loss/order independently.
            account(
                &mut self.counts,
                self.state.publish_json(at_us, &output.state),
            );
            self.last_state = Some(at_us);
            self.last_fault = output.state.fault;
            self.last_feedback = Some(feedback);
        }
        for gesture in &output.gestures {
            account(&mut self.counts, self.gestures.publish_json(at_us, gesture));
        }
        if self
            .last_report
            .is_none_or(|at| at_us.saturating_sub(at) >= 1_000_000)
        {
            if self.gestures.alive().is_err() {
                self.counts.publish_errors += 1;
            }
            let report = self.report(&output);
            account(
                &mut self.counts,
                self.diagnostic.publish_json(at_us, &report),
            );
            // The switch's own item, beside Controls' (#2143).
            if let Some(privacy) = &mut self.privacy {
                let switch = privacy.health(at_us);
                account(
                    &mut self.counts,
                    self.diagnostic.publish_json(at_us, &switch),
                );
            }
            self.last_report = Some(at_us);
        }
        self.counts.steps += 1;
        let elapsed = now().saturating_sub(start);
        self.counts.longest_step_us = self.counts.longest_step_us.max(elapsed);
        self.pace.worked(elapsed);
        output
    }

    pub fn report(&self, output: &Output) -> Diagnostic {
        let privacy = output.state.privacy.as_ref();
        let config = self.engine.config();
        let privacy_only = privacy.is_some()
            && config.buttons.is_empty()
            && config.head_pads.is_empty()
            && config.base_selected_mask.is_none();
        let (level, message) = if privacy.is_some_and(|sample| {
            !sample.fresh_at(&output.state.stamp.clock_id, output.state.stamp.at_us)
        }) {
            (
                Level::Error,
                "physical privacy evidence is unavailable or invalid",
            )
        } else if privacy_only {
            (
                Level::Ok,
                "physical privacy evidence published; capture is not owned",
            )
        } else if !output.state.ready {
            (Level::Error, "controls input is unavailable or invalid")
        } else if !output.state.enabled {
            // Watching only is how the controls are meant to run until
            // something acts on them: well, and said so. As a warning it
            // made the whole lamp report one, all day (lamp-4ace,
            // 2026-10-14, the morning the controls were first installed).
            (Level::Ok, "shadow mode; gestures are not commands")
        } else {
            (
                Level::Ok,
                "requests published; owner application is not verified",
            )
        };
        let record = self
            .recorder
            .as_ref()
            .map(crate::record::Recorder::stats)
            .unwrap_or_default();
        let level = if self.counts.publish_errors > 0 || record.failed {
            Level::Error
        } else {
            level
        };
        let report = Diagnostic::new(level, "controls", message)
            .with("enabled", output.state.enabled)
            .with("input_ready", output.state.ready)
            .with("input_fault", format!("{:?}", output.state.fault))
            .with("accepted", self.counts.accepted)
            .with("rejected", self.counts.rejected)
            .with("reader_errors", self.counts.reader_errors)
            .with("drain_overflows", self.counts.drain_overflows)
            .with("publish_errors", self.counts.publish_errors)
            .with("dropped", self.counts.dropped)
            .with("longest_step_us", self.counts.longest_step_us)
            .with("longest_gap_us", self.pace.longest_gap_us)
            .with("long_gaps", self.pace.long_gaps)
            .with("slow_steps", self.pace.slow_steps)
            .with("record_dropped", record.dropped)
            .with("record_failed", record.failed)
            .with("physical_stop_verified", false)
            .with("capture_permission", "not_owned");
        if let Some(privacy) = privacy {
            report
                .with("privacy_muted", format!("{:?}", privacy.muted))
                .with("privacy_fault", format!("{:?}", privacy.fault))
        } else {
            report
        }
    }
}

fn account(counts: &mut Counts, result: io::Result<Published>) {
    match result {
        Ok(result) => {
            counts.published += 1;
            counts.dropped += result.dropped as u64;
        }
        Err(_) => counts.publish_errors += 1,
    }
}
