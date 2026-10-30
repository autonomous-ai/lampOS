//! Opt-in dual-source owner. GPIO is drained before the nonblocking strip
//! socket; no I2C operation, worker wait, recording write or hand log runs here.
use crate::{
    engine::{Config, Engine},
    input::{InputError, Observation, PinReader},
    pace::Pace,
    runtime::{Counts, owner_lock},
    strip_worker::StripReader,
    touch::{TouchEngine, TouchOutput},
    touch_pins::TouchPinsConfig,
    touch_replay::{Input, Record},
};
use lamp_bus::{Bus, Message, Published, Publisher};
use lamp_msgs::{
    ControlFault, ControlGesture, ControlsState, Diagnostic, Hand, HeadPet, Level, TouchEvent,
    TouchState,
};
use std::{fs::File, io, path::Path};

pub const DRAIN_LIMIT: usize = 32;
/// The head's and the buttons' states are said when what a reader acts on
/// changes, with an event or a gesture, and at least this often, as their
/// topics declare: each one wakes every reader, so a still pad is not said
/// fifty times a second (the first builder on #1456, lamp-4ace). With the
/// privacy switch attached, or the privacy route on a lamp with no slide,
/// `controls/state` keeps [`crate::privacy::STATE_EVERY_US`], since its
/// evidence lasts 50 ms.
pub const STATE_EVERY_US: u64 = 100_000;
/// What only grows or moves while a hand stays (a held contact's `held_us`,
/// the strip's position, distance and speed) is said at most this often,
/// the pace both states always had: the pins read every 5 ms, and saying
/// each reading took a held lamp to 200 states a second (the reviewer on
/// #1469). A press, a release or a tier is said at once. A publish the bus
/// refused is tried again this long after.
pub const MOVING_EVERY_US: u64 = 20_000;
/// How often the body's `body/hand` is looked at. The body says it with
/// every step it passes on, every 80 ms.
pub const HAND_EVERY_US: u64 = 20_000;

/// Whether the body feels the head pushed by hand now (#711): a fresh
/// `body/hand` from the body that says so. Missing, expired or from anyone
/// else is not pushed, so a stopped body never silences the head.
pub fn pushed_now(message: Option<&Message>, now_unix_ms: u64) -> bool {
    message.is_some_and(|message| {
        !message.expired
            && !message.header.expired_at(now_unix_ms)
            && message.header.source == "body"
            && message.parse::<Hand>().is_ok_and(|hand| hand.pushed)
    })
}

/// The body's hand, read from the bus at most every [`HAND_EVERY_US`].
struct HandIn {
    bus: Bus,
    looked_us: Option<u64>,
    pushed: bool,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

pub struct Service<G: PinReader, S: StripReader> {
    gpio: G,
    pub strip: S,
    wiring: TouchPinsConfig,
    engine: TouchEngine,
    primary: Option<Primary>,
    state: Publisher,
    events: Publisher,
    /// `controls/pet`, declared only with `LAMP_HEAD_PET` on
    /// ([`Self::head_pet`]).
    pets: Option<Publisher>,
    /// With `LAMP_SHUTDOWN=on`: [`lamp_msgs::ShutdownRequest`], which setup's
    /// watcher acts on (#3697).
    shutdown_requests: Option<Publisher>,
    diagnostic: Publisher,
    last_report_us: Option<u64>,
    pub counts: Counts,
    pub gpio_accepted: u64,
    pub strip_accepted: u64,
    last_strip_error: Option<String>,
    last_gpio_error: Option<String>,
    recorder: Option<crate::record::Recorder>,
    hand: HandIn,
    privacy: Option<crate::privacy::Source>,
    /// The privacy route on a lamp with no slide ([`Self::mark_no_slide`]).
    no_slide: bool,
    /// What the probe at start found on the head pads' lines
    /// ([`Self::head_probed`]).
    head_lines: Option<crate::touch_pins::HeadLines>,
    /// When the head's state and `controls/state` were last said, and what.
    touch_said: Said<TouchState>,
    primary_said: Said<ControlsState>,
    /// The loop's pace ([`Pace`]).
    pub pace: Pace,
    _lock: File,
}

/// What a state last said, and when it is said again (#1456, #1469).
struct Said<T> {
    /// The last said, with its times set aside ([`Timeless`]), the same
    /// with what grows or moves set aside too, and when.
    last: Option<(T, T, u64)>,
    /// When the bus last refused it: it is tried again
    /// [`MOVING_EVERY_US`] after.
    refused_us: Option<u64>,
}

impl<T> Default for Said<T> {
    fn default() -> Self {
        Self {
            last: None,
            refused_us: None,
        }
    }
}

/// A state with its times set aside (when it was said, when each source
/// was read), and with what only grows or moves while a hand stays set
/// aside too.
trait Timeless: Clone + PartialEq {
    fn timeless(&self) -> Self;
    fn still(&self) -> Self;
}

impl Timeless for TouchState {
    fn timeless(&self) -> Self {
        let mut key = self.clone();
        key.stamp.at_us = 0;
        key.stamp.sequence = 0;
        for source in &mut key.sources {
            source.observed_us = source.observed_us.map(|_| 0);
        }
        key
    }

    fn still(&self) -> Self {
        let mut key = self.timeless();
        for source in &mut key.sources {
            source.held_us = source.held_us.map(|_| 0);
            source.position = source.position.map(|_| 0.0);
            source.distance = 0.0;
            source.speed = 0.0;
        }
        key
    }
}

impl Timeless for ControlsState {
    fn timeless(&self) -> Self {
        let mut key = self.clone();
        key.stamp.at_us = 0;
        key.stamp.sequence = 0;
        if let Some(privacy) = &mut key.privacy {
            privacy.stamp.at_us = 0;
            privacy.stamp.sequence = 0;
        }
        key
    }

    fn still(&self) -> Self {
        let mut key = self.timeless();
        for button in &mut key.buttons {
            button.held_us = button.held_us.map(|_| 0);
        }
        key.base_held_us = key.base_held_us.map(|_| 0);
        key
    }
}

impl<T: Timeless> Said<T> {
    /// Says `state` at `at_us` through `say` if it is due: with an event
    /// or a gesture (`news`), when what a reader acts on changed, when
    /// only what grows changed and [`MOVING_EVERY_US`] has passed, and
    /// every `every_us` regardless. After a refusal, not again for
    /// [`MOVING_EVERY_US`], but for news.
    fn say(
        &mut self,
        state: &T,
        at_us: u64,
        every_us: u64,
        news: bool,
        say: impl FnOnce() -> io::Result<Published>,
        counts: &mut Counts,
    ) {
        let (timeless, still) = (state.timeless(), state.still());
        let since = |at: u64| at_us.saturating_sub(at);
        let backing_off = self
            .refused_us
            .is_some_and(|at| since(at) < MOVING_EVERY_US);
        let due = news
            || (!backing_off
                && self.last.as_ref().is_none_or(|(whole, held, at)| {
                    *held != still
                        || (*whole != timeless && since(*at) >= MOVING_EVERY_US)
                        || since(*at) >= every_us
                }));
        if !due {
            return;
        }
        let result = say();
        if result.is_ok() {
            self.last = Some((timeless, still, at_us));
            self.refused_us = None;
        } else {
            self.refused_us = Some(at_us);
        }
        account(counts, result);
    }
}

struct Primary {
    engine: Engine,
    state: Publisher,
    gestures: Publisher,
    /// The primary button, if there is one.
    name: Option<String>,
    /// Whether the head pads go to it too ([`TouchPinsConfig::legacy_head`]).
    head: bool,
}

impl<G: PinReader, S: StripReader> Service<G, S> {
    pub fn new(
        bus: &Bus,
        config: Config,
        wiring: TouchPinsConfig,
        gpio: G,
        strip: S,
        clock: &str,
    ) -> io::Result<Self> {
        let touch = wiring.check(&config).map_err(io::Error::other)?;
        let lock = owner_lock(bus.root())?;
        let state =
            bus.publisher_declared(TouchState::TOPIC, &TouchState::declaration(), "controls")?;
        let mut engine = TouchEngine::new(touch, clock, state.boot()).map_err(io::Error::other)?;
        // The primary button held 5 to 15 s turns the lamp off (#3697).
        let shutdown = lamp_msgs::shutdown_switched_on();
        engine.set_shutdown(shutdown);
        if shutdown {
            eprintln!(
                "lamp-controls: LAMP_SHUTDOWN=on: the primary button released after 5 to 15 s asks to turn the lamp off"
            );
        }
        // Preserve the installed Voice receiver's primary-button behavior in
        // touch mode, and with `legacy_head` the head's Pet. Base and reset
        // are never sent to this legacy engine.
        let button = wiring.buttons.first().map(|pin| pin.name.clone());
        // With no head pads, `legacy_head` gives the old engine nothing to
        // read: alone, its refused reads would take the touch path's GPIO
        // down (the reviewer on #1176). Ignored, never refused: a pins file
        // install kept must not stop the button and the reset (#1209).
        let legacy_head = wiring.legacy_head && !wiring.head_pads.is_empty();
        if wiring.legacy_head && !legacy_head {
            eprintln!("lamp-controls: legacy_head ignored: the pins name no head pads");
        }
        let primary = if button.is_some() || legacy_head {
            let state = crate::runtime::state_publisher(bus, clock)?;
            let mut legacy = Engine::new(
                Config {
                    enabled: config.enabled,
                    buttons: button.iter().cloned().collect(),
                    head_pads: if legacy_head {
                        wiring
                            .head_pads
                            .iter()
                            .map(|pin| pin.name.clone())
                            .collect()
                    } else {
                        vec![]
                    },
                    base_selected_mask: None,
                },
                clock,
                state.boot(),
            )
            .map_err(io::Error::other)?;
            legacy.set_shutdown(shutdown);
            Some(Primary {
                engine: legacy,
                state,
                gestures: bus.publisher_declared(
                    ControlGesture::TOPIC,
                    &ControlGesture::declaration(),
                    "controls",
                )?,
                name: button,
                head: legacy_head,
            })
        } else {
            None
        };
        Ok(Self {
            gpio,
            strip,
            wiring,
            engine,
            primary,
            state,
            events: bus.publisher_declared(
                TouchEvent::TOPIC,
                &TouchEvent::declaration(),
                "controls",
            )?,
            pets: None,
            shutdown_requests: shutdown
                .then(|| {
                    bus.publisher_declared(
                        lamp_msgs::ShutdownRequest::TOPIC,
                        &lamp_msgs::ShutdownRequest::declaration(),
                        "controls",
                    )
                })
                .transpose()?,
            diagnostic: bus.publisher_declared(
                Diagnostic::TOPIC,
                &Diagnostic::declaration(),
                "controls",
            )?,
            last_report_us: None,
            counts: Counts::default(),
            gpio_accepted: 0,
            strip_accepted: 0,
            last_strip_error: None,
            last_gpio_error: None,
            recorder: None,
            hand: HandIn {
                bus: bus.clone(),
                looked_us: None,
                pushed: false,
            },
            privacy: None,
            no_slide: false,
            head_lines: None,
            touch_said: Said::default(),
            primary_said: Said::default(),
            pace: Pace::default(),
            _lock: lock,
        })
    }

    /// Before the head's samples: a touch that begins while the head is
    /// pushed asks nothing from its first sample.
    fn look_at_hand(&mut self, now_us: u64) {
        if self
            .hand
            .looked_us
            .is_some_and(|at| now_us.saturating_sub(at) < HAND_EVERY_US)
        {
            return;
        }
        self.hand.looked_us = Some(now_us);
        let latest = self.hand.bus.latest(Hand::TOPIC).ok().flatten();
        let pushed = pushed_now(latest.as_ref(), unix_ms());
        if pushed != self.hand.pushed {
            self.hand.pushed = pushed;
            self.record(now_us, Input::HeadPushed { pushed });
            self.engine.set_head_pushed(pushed);
        }
    }

    pub fn record_to(&mut self, path: &Path) -> io::Result<()> {
        if self.recorder.is_some() {
            return Err(io::Error::other("touch recorder is already attached"));
        }
        self.recorder = Some(crate::record::Recorder::create_touch(path)?);
        Ok(())
    }

    /// `LAMP_HEAD_PET` on (#3997): a hand petting the head is said on
    /// `controls/pet`, and a kept hand's sleep is as `hold` says. Never
    /// called with it off, so `controls/pet` is not even declared then.
    pub fn head_pet(&mut self, bus: &Bus, hold: crate::touch::HoldSleeps) -> io::Result<()> {
        self.pets =
            Some(bus.publisher_declared(HeadPet::TOPIC, &HeadPet::declaration(), "controls")?);
        self.engine.set_head_pet(Some(hold));
        Ok(())
    }

    pub fn attach_privacy(&mut self, bus: &Bus, source: crate::privacy::Source) -> io::Result<()> {
        self.ensure_primary(bus, source.monitor.clock())?;
        self.privacy = Some(source);
        Ok(())
    }

    /// What the probe at start found on the head pads' lines (#1798):
    /// health says it beside Controls' own report, with any pad left out
    /// since.
    pub fn head_probed(&mut self, lines: crate::touch_pins::HeadLines) {
        self.head_lines = Some(lines);
    }

    /// The privacy route on a lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`):
    /// nothing is read, and each `controls/state` says so
    /// (`no_privacy_slide`) at the route's cadence, as when a slide reader is
    /// attached, with a state owner made for it if the pins give none.
    /// Refused beside a slide reader; a reader attached later wins, and no
    /// marker is said.
    pub fn mark_no_slide(&mut self, bus: &Bus, clock: &str) -> io::Result<()> {
        if self.privacy.is_some() {
            return Err(io::Error::other(
                "a lamp with a slide reader is not a lamp with no slide",
            ));
        }
        self.ensure_primary(bus, clock)?;
        self.no_slide = true;
        Ok(())
    }

    /// The `controls/state` owner the privacy route publishes on, made with
    /// no button or head if the pins give it none.
    fn ensure_primary(&mut self, bus: &Bus, clock: &str) -> io::Result<()> {
        if self.primary.is_none() {
            let state = crate::runtime::state_publisher(bus, clock)?;
            self.primary = Some(Primary {
                engine: Engine::new(
                    Config {
                        enabled: self.engine.config().enabled,
                        ..Config::default()
                    },
                    clock,
                    state.boot(),
                )
                .map_err(io::Error::other)?,
                state,
                gestures: bus.publisher_declared(
                    ControlGesture::TOPIC,
                    &ControlGesture::declaration(),
                    "controls",
                )?,
                name: None,
                head: false,
            });
        }
        Ok(())
    }

    /// The privacy route is on: a slide reader, or a lamp with no slide.
    fn privacy_route(&self) -> bool {
        self.privacy.is_some() || self.no_slide
    }

    pub fn finish_recording(&mut self) -> Option<crate::record::RecordStats> {
        self.recorder.take().map(crate::record::Recorder::finish)
    }

    /// [`TouchEngine::reset_refused`].
    pub fn reset_refused(&self) -> bool {
        self.engine.reset_refused()
    }

    pub fn poll_us(&self) -> u64 {
        if self.privacy_route() {
            crate::privacy::STEP_US
        } else {
            crate::runtime::POLL_US
        }
    }

    fn record(&mut self, now_us: u64, input: Input) {
        if let Some(recorder) = &mut self.recorder {
            recorder.submit_touch(&Record { now_us, input });
        }
    }

    fn gpio_fault(&mut self, now_us: u64, fault: ControlFault) {
        self.record(now_us, Input::GpioFault { fault });
        self.engine.gpio_fault(now_us, fault);
        // As for a sample: a legacy engine with the button or the head pads
        // (`legacy_head`) reads GPIO, so it hears the fault too.
        if let Some(primary) = &mut self.primary
            && (primary.name.is_some() || primary.head)
        {
            primary.engine.input_fault(now_us, fault);
        }
    }

    fn gpio_sample(&mut self, now_us: u64, sample: Observation) -> Result<(), InputError> {
        let gpio = match self.wiring.gpio_sample(sample.clone()) {
            Ok(gpio) => gpio,
            Err(error) => {
                self.gpio_fault(now_us, error.fault);
                return Err(error);
            }
        };
        self.record(
            now_us,
            Input::Gpio {
                sample: gpio.clone(),
            },
        );
        // Validate the complete acquisition before allowing the legacy primary
        // owner to retain a Stop or mute from any part of this observation.
        if let Err(error) = self.engine.ingest_gpio(now_us, gpio) {
            if let Some(primary) = &mut self.primary {
                primary.engine.input_fault(now_us, error.fault);
            }
            return Err(error);
        }
        // A privacy-only legacy engine has no button and no head to read.
        if let Some(primary) = &mut self.primary
            && (primary.name.is_some() || primary.head)
        {
            let mut button = sample.clone();
            button
                .buttons
                .retain(|level| primary.name.as_ref() == Some(&level.name));
            if !primary.head {
                button.head_pads.clear();
            }
            button.base_mask = None;
            if let Err(error) = primary.engine.ingest(now_us, button) {
                self.record(now_us, Input::GpioFault { fault: error.fault });
                self.engine.gpio_fault(now_us, error.fault);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Failure to spawn the optional I2C child leaves healthy GPIO running.
    /// No automatic replacement worker is started after any strip failure.
    pub fn strip_unavailable(&mut self, now_us: u64, message: &str) {
        self.strip_error(now_us, InputError::new(ControlFault::Missing, message));
    }

    fn strip_error(&mut self, now_us: u64, error: InputError) {
        self.counts.reader_errors += 1;
        self.last_strip_error = Some(error_detail(&error));
        self.record(now_us, Input::StripFault { fault: error.fault });
        self.engine.strip_fault(now_us, error.fault);
    }

    pub fn step(&mut self, now: impl Fn() -> u64) -> TouchOutput {
        let began = now();
        if let Some(line) = self.pace.step(began) {
            eprintln!("lamp-controls: {line}");
        }
        if let Some(privacy) = &mut self.privacy
            && let Some((received, sample)) = privacy.poll(&now)
        {
            self.record(received, Input::Privacy { sample });
        }
        self.look_at_hand(began);
        let mut drained = 0;
        for _ in 0..DRAIN_LIMIT {
            match self.gpio.read(now()) {
                Ok(Some(sample)) => {
                    drained += 1;
                    match self.gpio_sample(now(), sample) {
                        Ok(()) => {
                            self.counts.accepted += 1;
                            self.gpio_accepted += 1;
                        }
                        Err(error) => {
                            self.counts.rejected += 1;
                            self.last_gpio_error = Some(error_detail(&error));
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    self.counts.reader_errors += 1;
                    self.last_gpio_error = Some(error_detail(&error));
                    self.gpio_fault(now(), error.fault);
                    break;
                }
            }
        }
        if drained == DRAIN_LIMIT {
            self.counts.drain_overflows += 1;
            self.gpio_fault(now(), ControlFault::Overflow);
        }
        let mut drained = 0;
        for _ in 0..DRAIN_LIMIT {
            match self.strip.read(now()) {
                Ok(Some(sample)) => {
                    drained += 1;
                    let received_us = now();
                    self.record(
                        received_us,
                        Input::Strip {
                            sample: sample.clone(),
                        },
                    );
                    match self.engine.ingest_strip(received_us, sample) {
                        Ok(()) => {
                            self.counts.accepted += 1;
                            self.strip_accepted += 1;
                        }
                        Err(_) => self.counts.rejected += 1,
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let received_us = now();
                    self.strip_error(received_us, error);
                    break;
                }
            }
        }
        if drained == DRAIN_LIMIT {
            self.counts.drain_overflows += 1;
            let received_us = now();
            self.record(
                received_us,
                Input::StripFault {
                    fault: ControlFault::Overflow,
                },
            );
            self.engine.strip_fault(received_us, ControlFault::Overflow);
        }
        let at_us = now();
        let output = self.engine.tick(at_us);
        let state = &mut self.state;
        self.touch_said.say(
            &output.state,
            at_us,
            STATE_EVERY_US,
            !output.events.is_empty(),
            || state.publish_json(at_us, &output.state),
            &mut self.counts,
        );
        for event in &output.events {
            account(&mut self.counts, self.events.publish_json(at_us, event));
            if let Some(requests) = &mut self.shutdown_requests
                && let Some(request) = crate::touch::shutdown_request(event)
            {
                eprintln!(
                    "lamp-controls: {}: asking to turn the lamp off",
                    request.reason
                );
                account(&mut self.counts, requests.publish_json(at_us, &request));
            }
        }
        if let Some(pets) = &mut self.pets {
            for pet in &output.pets {
                if pet.petted_us == pet.observed_us {
                    eprintln!(
                        "lamp-controls: a head pet (contact {}, {} swaps)",
                        pet.contact_id, pet.swaps
                    );
                }
                account(&mut self.counts, pets.publish_json(at_us, pet));
            }
        }
        let privacy_route = self.privacy_route();
        if let Some(primary) = &mut self.primary {
            let mut output = primary.engine.tick(at_us);
            if let Some(privacy) = &mut self.privacy {
                output.state.privacy = Some(privacy.monitor.snapshot(at_us));
            } else if self.no_slide {
                output.state.no_privacy_slide = true;
            }
            let every_us = if privacy_route {
                crate::privacy::STATE_EVERY_US
            } else {
                STATE_EVERY_US
            };
            let state = &mut primary.state;
            self.primary_said.say(
                &output.state,
                at_us,
                every_us,
                !output.gestures.is_empty(),
                || state.publish_json(at_us, &output.state),
                &mut self.counts,
            );
            for gesture in &output.gestures {
                account(
                    &mut self.counts,
                    primary.gestures.publish_json(at_us, gesture),
                );
            }
        }
        if self
            .last_report_us
            .is_none_or(|last| at_us.saturating_sub(last) >= 1_000_000)
        {
            if self.events.alive().is_err() {
                self.counts.publish_errors += 1;
            }
            if let Some(primary) = &mut self.primary
                && primary.gestures.alive().is_err()
            {
                self.counts.publish_errors += 1;
            }
            let privacy = self
                .privacy
                .as_mut()
                .map(|source| source.monitor.snapshot(at_us));
            let ready = output.state.sources.iter().all(|source| source.ready)
                && privacy
                    .as_ref()
                    .is_none_or(|sample| sample.fresh_at(&output.state.stamp.clock_id, at_us));
            let recording = self
                .recorder
                .as_ref()
                .map(crate::record::Recorder::stats)
                .unwrap_or_default();
            let report = Diagnostic::new(
                if ready && self.counts.publish_errors == 0 && !recording.failed {
                    Level::Ok
                } else {
                    Level::Error
                },
                "controls",
                if ready {
                    "touch evidence published; owner application is not verified"
                } else {
                    "a configured touch source is unavailable; other sources continue"
                },
            )
            .with("touch_enabled", output.state.enabled)
            .with("gpio_accepted", self.gpio_accepted)
            .with("strip_accepted", self.strip_accepted)
            .with(
                "last_strip_error",
                self.last_strip_error.as_deref().unwrap_or("none"),
            )
            .with(
                "last_gpio_error",
                self.last_gpio_error.as_deref().unwrap_or("none"),
            )
            .with("rejected", self.counts.rejected)
            .with("reader_errors", self.counts.reader_errors)
            .with("publish_errors", self.counts.publish_errors)
            .with("dropped", self.counts.dropped)
            .with("longest_step_us", self.counts.longest_step_us)
            .with("longest_gap_us", self.pace.longest_gap_us)
            .with("long_gaps", self.pace.long_gaps)
            .with("slow_steps", self.pace.slow_steps)
            .with("record_dropped", recording.dropped)
            .with("record_failed", recording.failed)
            .with("physical_stop_verified", false)
            .with("capture_permission", "not_owned");
            let report = if let Some(privacy) = privacy {
                report
                    .with("privacy_muted", format!("{:?}", privacy.muted))
                    .with("privacy_fault", format!("{:?}", privacy.fault))
            } else {
                report
            };
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
            // The head pads' lines, and any left out (#1798).
            if let Some(lines) = &self.head_lines {
                let left_out = self.engine.head_left_out();
                let left_out: Vec<u32> = self
                    .wiring
                    .head_pads
                    .iter()
                    .filter(|pin| left_out.contains(&pin.name))
                    .map(|pin| pin.line)
                    .collect();
                let report = lines.health(&left_out);
                account(
                    &mut self.counts,
                    self.diagnostic.publish_json(at_us, &report),
                );
            }
            self.last_report_us = Some(at_us);
        }
        self.counts.steps += 1;
        let worked = now().saturating_sub(began);
        self.counts.longest_step_us = self.counts.longest_step_us.max(worked);
        self.pace.worked(worked);
        output
    }
}

fn account(counts: &mut Counts, result: io::Result<Published>) {
    match result {
        Ok(published) => {
            counts.published += 1;
            counts.dropped += published.dropped as u64;
        }
        Err(_) => counts.publish_errors += 1,
    }
}

fn error_detail(error: &InputError) -> String {
    error
        .to_string()
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}
