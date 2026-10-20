//! Linux capture with a Voice-owned lease. Bus/filesystem work stays on one
//! bounded pump; it cannot stall revocation in the microphone owner.
use crate::{Arguments, NAME, log};
use alsa::{
    Direction, ValueOr,
    pcm::{Access, Format, HwParams, PCM},
};
use lamp_bus::{Bus, Message, Notice, Subscriber};
use lamp_ears::{
    BLOCK, RATE, Settings,
    classes::Classified,
    privacy::{Capture, CaptureOwner, Emission, LeaseGate, PublicationGate},
};
use lamp_msgs::{
    Audio, Diagnostic, Level, MicrophonePrivacy, RoomSound, SoundClasses, SuddenSound, Voice,
    VoiceState,
};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TICK: Duration = Duration::from_millis(2);
const BATCH: usize = 16;
const CAPACITY: usize = 16;
const RETRY_US: u64 = 2_000_000;

struct Device(PCM);

impl Device {
    fn open(name: &str) -> io::Result<Self> {
        // Unlike the compatibility path, no read waits for a complete block.
        let pcm = PCM::new(name, Direction::Capture, true).map_err(io::Error::other)?;
        {
            let hardware = HwParams::any(&pcm).map_err(io::Error::other)?;
            hardware
                .set_access(Access::RWInterleaved)
                .map_err(io::Error::other)?;
            hardware
                .set_format(Format::s16())
                .map_err(io::Error::other)?;
            hardware.set_channels(1).map_err(io::Error::other)?;
            hardware
                .set_rate(RATE, ValueOr::Nearest)
                .map_err(io::Error::other)?;
            hardware
                .set_period_size_near(BLOCK as i64, ValueOr::Nearest)
                .map_err(io::Error::other)?;
            hardware
                .set_buffer_size_near(BLOCK as i64 * 10)
                .map_err(io::Error::other)?;
            pcm.hw_params(&hardware).map_err(io::Error::other)?;
        }
        if pcm
            .hw_params_current()
            .map_err(io::Error::other)?
            .get_rate()
            .map_err(io::Error::other)?
            != RATE
        {
            return Err(io::Error::other("room microphone rate is not 16 kHz"));
        }
        pcm.start().map_err(io::Error::other)?;
        Ok(Self(pcm))
    }
}

impl Capture for Device {
    fn read(&mut self, samples: &mut [i16]) -> io::Result<usize> {
        match self.0.io_i16().and_then(|reader| reader.readi(samples)) {
            Ok(count) => Ok(count),
            Err(error)
                if io::Error::from_raw_os_error(error.errno()).kind()
                    == io::ErrorKind::WouldBlock =>
            {
                Ok(0)
            }
            Err(error) => Err(io::Error::other(error)),
        }
    }

    fn discard(&mut self) -> io::Result<()> {
        self.0.drop().map_err(io::Error::other)
    }
}

struct Input {
    message: Message,
    fence: u64,
}

enum Output {
    Sound {
        emission: Emission,
        fence: u64,
        at_us: u64,
    },
    Health {
        open: bool,
        failures: u64,
        observed_us: u64,
    },
}

struct Shared {
    export: bool,
    pcm: lamp_ears::pcm::SharedMailbox,
    publication: PublicationGate,
    speaking: AtomicBool,
    stop: AtomicBool,
}

impl Shared {
    fn new(export: bool) -> Self {
        Self {
            export,
            pcm: lamp_ears::pcm::SharedMailbox::new(export),
            publication: PublicationGate::default(),
            speaking: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        }
    }
    fn revoke(&self) {
        self.publication.fence();
        self.pcm.invalidate();
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

fn input(
    subscriber: &mut Subscriber,
    gate: &mut LeaseGate,
    sender: &SyncSender<Input>,
    shared: &Shared,
) -> io::Result<bool> {
    let before = (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
        subscriber.repeated(),
    );
    let message = subscriber.recv(Duration::ZERO)?;
    let after = (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
        subscriber.repeated(),
    );
    if before != after {
        gate.close();
        shared.revoke();
    }
    if subscriber
        .notices()
        .iter()
        .any(|notice| matches!(notice, Notice::SourceGone | Notice::Late))
    {
        gate.close();
        shared.revoke();
    }
    let Some(message) = message else {
        return Ok(false);
    };
    let previous = gate.epoch();
    let valid = gate
        .observe(&message, lamp_ipc::monotonic_us(), unix_ms())
        .is_ok();
    if !valid || previous != gate.epoch() || message.publisher_restarted {
        shared.revoke();
    }
    if !valid {
        return Ok(true);
    }
    let fence = shared.publication.generation();
    match sender.try_send(Input { message, fence }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            gate.close();
            shared.revoke();
        }
        Err(TrySendError::Disconnected(_)) => return Err(io::Error::other("capture owner ended")),
    }
    Ok(true)
}

fn connected(
    bus: &Bus,
    clock_id: &str,
    gate: &mut LeaseGate,
    sender: &SyncSender<Input>,
    output: &Receiver<Output>,
    classes: Option<&Receiver<Classified>>,
    shared: &Shared,
) -> io::Result<()> {
    let mut privacy = bus.subscribe_typed(
        MicrophonePrivacy::TOPIC,
        "ears_privacy",
        MicrophonePrivacy::TYPE,
    )?;
    let mut levels = bus.publisher_declared(RoomSound::TOPIC, &RoomSound::declaration(), NAME)?;
    let mut sudden =
        bus.publisher_declared(SuddenSound::TOPIC, &SuddenSound::declaration(), NAME)?;
    let mut health = bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), NAME)?;
    let mut classes = match classes {
        Some(receiver) => Some((
            receiver,
            bus.publisher_declared(SoundClasses::TOPIC, &SoundClasses::declaration(), NAME)?,
        )),
        None => None,
    };
    let mut audio = None;
    let mut audio_retry = lamp_ears::pcm::ExportRetry::default();
    let mut looked = 0;
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let drained = lamp_ears::pcm::drain_privacy(|| input(&mut privacy, gate, sender, shared))?;
        if !drained {
            gate.close();
            shared.revoke();
        }
        // Optional export faults must not tear down healthy metadata/capture.
        // Filesystem work precedes PCM access, followed by another bounded
        // privacy drain: a mute queued during slow preparation wins.
        if shared.export && drained && audio_retry.ready(lamp_ipc::monotonic_us()) {
            let prepared = (|| {
                if audio.is_none() {
                    shared.pcm.invalidate(); // No replay across publisher recovery.
                    audio = Some(lamp_ears::pcm::audio_publisher(bus, NAME)?);
                }
                audio.as_mut().expect("created").prepare_guarded_event()
            })();
            let publication = match prepared {
                Err(error) => Err(error),
                Ok(()) => {
                    let drained = lamp_ears::pcm::drain_privacy(|| {
                        input(&mut privacy, gate, sender, shared)
                    })?;
                    if !drained {
                        gate.close();
                        shared.revoke();
                        Ok(())
                    } else {
                        (|| {
                            if let Some(pcm) = shared
                                .pcm
                                .take(&shared.publication, lamp_ipc::monotonic_us())
                            {
                                let at_us = pcm.captured_at_us();
                                if let Some(packet) =
                                    pcm.pack(&shared.publication, lamp_ipc::monotonic_us())?
                                    && let Some(bytes) = packet.bytes_if_allowed(
                                        &shared.publication,
                                        lamp_ipc::monotonic_us(),
                                    )
                                {
                                    match audio.as_mut().expect("prepared").publish_bytes_guarded(
                                        at_us,
                                        Audio::ENCODING,
                                        bytes,
                                        || {
                                            packet.allows(
                                                &shared.publication,
                                                lamp_ipc::monotonic_us(),
                                            )
                                        },
                                    ) {
                                        Ok(_) => {}
                                        // Never retry a possibly partially delivered block.
                                        Err(error)
                                            if error.kind() == io::ErrorKind::PermissionDenied => {}
                                        Err(error) => return Err(error),
                                    }
                                }
                            }
                            Ok(())
                        })()
                    }
                }
            };
            match publication {
                Ok(()) => {
                    if audio_retry.recovered() {
                        log("optional mic1 audio export recovered");
                    }
                }
                Err(_) => {
                    audio = None;
                    shared.pcm.invalidate();
                    if audio_retry.failed(lamp_ipc::monotonic_us()) {
                        log(
                            "optional mic1 audio export unavailable; metadata continues; retry in 2 s",
                        );
                    }
                }
            }
        } else if shared.export {
            shared.pcm.invalidate();
        }
        // Only bounded metadata is queued, never PCM. A blocked publication
        // stops lease delivery; the independent owner then closes on expiry.
        for _ in 0..BATCH {
            match output.try_recv() {
                Ok(Output::Sound {
                    emission,
                    fence,
                    at_us,
                }) => {
                    if let Some(level) = emission.heard.level
                        && shared.publication.allows(
                            emission.permit,
                            fence,
                            lamp_ipc::monotonic_us(),
                        )
                    {
                        levels.publish_json(at_us, &level)?;
                    }
                    if let Some(sound) = emission.heard.sudden
                        && shared.publication.allows(
                            emission.permit,
                            fence,
                            lamp_ipc::monotonic_us(),
                        )
                    {
                        sudden.publish_json(at_us, &sound)?;
                    }
                }
                Ok(Output::Health {
                    open,
                    failures,
                    observed_us,
                }) => {
                    let message = if open {
                        "microphone observed open"
                    } else {
                        "microphone observed closed"
                    };
                    let report = Diagnostic::new(Level::Ok, NAME, message)
                        .with("privacy_controls", "on")
                        .with("capture_open_observed", open)
                        .with("capture_observed_us", observed_us)
                        .with("capture_clock_id", clock_id)
                        .with("capture_failures", failures);
                    health.publish_json(observed_us, &report)?;
                    sudden.alive()?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        // Scores leave only while their capture era still holds authority
        // now; the header keeps the audio's end, never the publish time.
        if let Some((receiver, publisher)) = classes.as_mut() {
            for _ in 0..BATCH {
                let Ok(classified) = receiver.try_recv() else {
                    break;
                };
                let now = lamp_ipc::monotonic_us();
                if classified.publishable(&shared.publication, now) {
                    let at_us = match &classified.classes {
                        SoundClasses::Scored { window, .. } => window.end_us,
                        SoundClasses::Unavailable { .. } => now,
                    };
                    publisher.publish_json(at_us, &classified.classes)?;
                }
            }
        }
        let now = lamp_ipc::monotonic_us();
        if now.saturating_sub(looked) >= 100_000 {
            looked = now;
            let speaking = bus.latest(VoiceState::TOPIC)?.is_some_and(|message| {
                unix_ms()
                    .checked_sub(message.header.at_unix_ms)
                    .is_some_and(|age| age < 3_000)
                    && message
                        .parse::<VoiceState>()
                        .is_ok_and(|state| state.state == Voice::Speaking)
            });
            shared.speaking.store(speaking, Ordering::Release);
        }
        thread::park_timeout(TICK);
    }
}

fn pump(
    bus: Bus,
    clock_id: String,
    sender: SyncSender<Input>,
    output: Receiver<Output>,
    classes: Option<Receiver<Classified>>,
    shared: Arc<Shared>,
) {
    let mut gate = LeaseGate::new(clock_id.clone());
    while !shared.stop.load(Ordering::Acquire) {
        gate.close();
        shared.revoke();
        let _ = connected(
            &bus,
            &clock_id,
            &mut gate,
            &sender,
            &output,
            classes.as_ref(),
            &shared,
        );
        shared.revoke();
        shared.speaking.store(false, Ordering::Release);
        if !shared.stop.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(250));
        }
    }
}

/// Room sound classes (#1669): model loading and inference run on their own
/// thread, never on capture or the pump; capture only offers blocks. Off
/// unless `LAMP_EARS_CLASSES` is exactly `on`, and never on a lamp with no
/// slide (`route`, `LAMP_PRIVACY_CONTROLS=no-slide`; the reviewer's P2 on
/// #2086): then no thread, no model, no handoff for capture and no
/// publisher for the pump.
fn start_classes(
    switch: Option<&str>,
    route: Option<&str>,
    model: Option<String>,
    shared: &Arc<Shared>,
) -> io::Result<(
    Option<lamp_ears::classes::Handoff>,
    Option<Receiver<Classified>>,
)> {
    match lamp_ears::classes::start(switch, route) {
        lamp_ears::classes::Start::Off => return Ok((None, None)),
        lamp_ears::classes::Start::NotOnThisRoute => {
            log(
                "LAMP_EARS_CLASSES=on needs LAMP_PRIVACY_CONTROLS=on, a lamp with a slide; with no-slide no classifier runs",
            );
            return Ok((None, None));
        }
        lamp_ears::classes::Start::Start => {}
    }
    let (handoff, intake) = lamp_ears::classes::handoff();
    let (send_classes, receive_classes) = mpsc::sync_channel(4);
    let worker_shared = shared.clone();
    thread::Builder::new()
        .name("ears-classes".into())
        .spawn(move || {
            let model = lamp_ears::classes::model_from(model.as_deref())
                .inspect_err(|error| log(&format!("ears/classes says invalid_model: {error}")))
                .ok();
            lamp_ears::classes::serve(
                intake,
                &worker_shared.publication,
                &worker_shared.stop,
                lamp_ipc::monotonic_us,
                model
                    .as_ref()
                    .map(|model| |features: &[f32]| model.score(features)),
                |classified| {
                    // A stalled pump drops results; they would go stale.
                    let _ = send_classes.try_send(classified);
                },
            );
        })?;
    log(
        "LAMP_EARS_CLASSES=on: ears/classes scores mic1 locally; at most one patch of one capture era (under 2 s) is held in memory, none written",
    );
    Ok((Some(handoff), Some(receive_classes)))
}

/// How long the route may go with no permission from Voice before it is
/// said, once.
const NO_PERMISSION_SAID_AFTER_US: u64 = 10_000_000;

/// The once-only line for a route that never opens (Third's yes on #1722,
/// 04:37): with no lease from Voice for 10 s since the start or the last
/// one, it is said once in the process's life, so a lamp whose Controls has
/// no privacy source (no slide in its pins) says why it hears nothing,
/// instead of waiting silently.
#[derive(Default)]
struct Unpermitted {
    since_us: Option<u64>,
    said: bool,
}

impl Unpermitted {
    fn note(&mut self, permitted: bool, now_us: u64) -> Option<&'static str> {
        if permitted {
            self.since_us = None;
            return None;
        }
        let since = *self.since_us.get_or_insert(now_us);
        if self.said || now_us.saturating_sub(since) < NO_PERMISSION_SAID_AFTER_US {
            return None;
        }
        self.said = true;
        Some(
            "no permission from Voice for 10 s: the microphone stays closed until one comes (muted, \
             or Controls has no privacy source, as with no privacy slide in its pins)",
        )
    }
}

pub(super) fn run(arguments: &Arguments) -> io::Result<()> {
    let bus = match &arguments.bus {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    }
    // The ordinary Bus discovery interval is 200 ms, longer than a privacy
    // lease. This selected reader must find new publishers before 50 ms.
    .rescan_every(Duration::from_millis(5));
    let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned();
    let mut owner = CaptureOwner::<Device>::new(
        clock_id.clone(),
        Settings {
            rise_db: arguments.rise_db,
            ..Settings::default()
        },
    );
    let export = lamp_ears::pcm::export_enabled(
        std::env::var("LAMP_BUS_AUDIO").ok().as_deref(),
        std::env::var("LAMP_PRIVACY_CONTROLS").ok().as_deref(),
    )?;
    let shared = Arc::new(Shared::new(export));
    let revoke_before_close = shared.clone();
    owner.before_close(move || {
        // This also covers discard performed inside read/observe failures:
        // the pump must lose authority before a driver can block teardown.
        revoke_before_close
            .publication
            .update(None, revoke_before_close.publication.generation());
        revoke_before_close.pcm.invalidate();
    });
    let (send_input, input) = mpsc::sync_channel(CAPACITY);
    let (output, receive_output) = mpsc::sync_channel(CAPACITY);
    let (classes, receive_classes) = start_classes(
        std::env::var("LAMP_EARS_CLASSES").ok().as_deref(),
        std::env::var("LAMP_PRIVACY_CONTROLS").ok().as_deref(),
        std::env::var("LAMP_EARS_CLASSES_MODEL").ok(),
        &shared,
    )?;
    let pump_shared = shared.clone();
    let worker = thread::Builder::new()
        .name("ears-privacy-bus".into())
        .spawn(move || {
            pump(
                bus,
                clock_id,
                send_input,
                receive_output,
                receive_classes,
                pump_shared,
            )
        })?;
    log("privacy controls on; microphone waits for Voice's fresh permission");
    if export {
        log(
            "LAMP_BUS_AUDIO=on: raw mic1/audio export enabled under original privacy leases; bus recorders may retain PCM",
        );
    }
    let result = (|| {
        let mut seen_fence = shared.publication.generation();
        let mut failures = 0u64;
        let mut retry_at = 0u64;
        let mut reported = 0u64;
        let mut unpermitted = Unpermitted::default();
        loop {
            let fence = shared.publication.generation();
            if fence != seen_fence || fence == u64::MAX {
                owner.invalidate()?;
                shared.pcm.invalidate();
                seen_fence = fence;
            }
            let mut drained = false;
            for _ in 0..BATCH {
                match input.try_recv() {
                    Ok(input) if input.fence == seen_fence => {
                        // Malformed input is optional loss, but capture is
                        // mandatory closed until a later fresh valid lease.
                        if owner
                            .observe(&input.message, lamp_ipc::monotonic_us(), unix_ms())
                            .is_err()
                        {
                            shared.publication.update(None, seen_fence);
                            owner.invalidate()?;
                            shared.pcm.invalidate();
                        }
                    }
                    Ok(_) => {
                        shared.publication.update(None, seen_fence);
                        owner.invalidate()?;
                        shared.pcm.invalidate();
                    }
                    Err(TryRecvError::Empty) => {
                        drained = true;
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Err(io::Error::other("privacy pump ended"));
                    }
                }
            }
            if !drained || seen_fence != shared.publication.generation() {
                shared.publication.update(None, seen_fence);
                owner.invalidate()?;
                shared.pcm.invalidate();
            }
            let permit = owner.maintain(lamp_ipc::monotonic_us())?;
            if let Some(line) = unpermitted.note(permit.is_some(), lamp_ipc::monotonic_us()) {
                log(line);
            }
            shared.publication.update(permit, seen_fence);
            shared
                .pcm
                .discard_invalid(&shared.publication, lamp_ipc::monotonic_us());
            if lamp_ipc::monotonic_us() >= retry_at {
                match owner.tick_export_while(
                    lamp_ipc::monotonic_us,
                    || Device::open(&arguments.device),
                    shared.speaking.load(Ordering::Acquire),
                    || seen_fence == shared.publication.generation(),
                    |samples, permit, sequence, captured_at_us| {
                        let stamp = lamp_ears::pcm::CaptureStamp {
                            permit,
                            fence: seen_fence,
                            sequence,
                            captured_at_us,
                        };
                        shared.pcm.offer(
                            samples,
                            &shared.publication,
                            stamp,
                            lamp_ipc::monotonic_us(),
                        );
                        if let Some(classes) = &classes {
                            classes.offer(
                                samples,
                                &shared.publication,
                                stamp,
                                lamp_ipc::monotonic_us(),
                            );
                        }
                    },
                ) {
                    Ok(Some(emission)) => {
                        let permit = owner.maintain(lamp_ipc::monotonic_us())?;
                        shared.publication.update(permit, seen_fence);
                        if shared.publication.allows(
                            emission.permit,
                            seen_fence,
                            lamp_ipc::monotonic_us(),
                        ) {
                            let _ = output.try_send(Output::Sound {
                                emission,
                                fence: seen_fence,
                                at_us: lamp_ipc::monotonic_us(),
                            });
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        retry_at = lamp_ipc::monotonic_us().saturating_add(RETRY_US);
                        shared.publication.update(None, seen_fence);
                        owner.invalidate()?;
                        shared.pcm.invalidate();
                    }
                }
            }
            if lamp_ipc::monotonic_us().saturating_sub(reported) >= 950_000 {
                reported = lamp_ipc::monotonic_us();
                let _ = output.try_send(Output::Health {
                    open: owner.is_open(),
                    failures,
                    observed_us: reported,
                });
            }
            thread::park_timeout(TICK);
        }
    })();
    shared.revoke();
    let close = owner.invalidate();
    shared.stop.store(true, Ordering::Release);
    worker.thread().unpark();
    // Bus filesystem work may be blocked. The handle is already gone; process
    // exit, rather than an unbounded join, ends that one remaining thread.
    if worker.is_finished() {
        let _ = worker.join();
    }
    close.and(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reviewer on #1923 (P3): with the switch anything but exactly
    /// `on`, no classifier thread, no handoff for capture and no publisher
    /// for the pump.
    #[test]
    fn classes_start_nothing_unless_the_switch_is_on() {
        let shared = Arc::new(Shared::new(false));
        for switch in [None, Some(""), Some("off"), Some("ON"), Some("on ")] {
            let (handoff, receiver) = start_classes(switch, Some("on"), None, &shared).unwrap();
            assert!(handoff.is_none() && receiver.is_none(), "{switch:?}");
        }
        assert_eq!(
            Arc::strong_count(&shared),
            1,
            "no thread holds the shared state"
        );
    }

    /// The reviewer's P2 on #2086: on a lamp with no slide
    /// (`LAMP_PRIVACY_CONTROLS=no-slide`) the switch on starts no
    /// classifier: no thread, no handoff, no publisher. Only `on` does.
    #[test]
    fn with_no_slide_classes_start_nothing() {
        let shared = Arc::new(Shared::new(false));
        for route in [Some("no-slide"), None, Some("off")] {
            let (handoff, receiver) = start_classes(Some("on"), route, None, &shared).unwrap();
            assert!(handoff.is_none() && receiver.is_none(), "{route:?}");
        }
        assert_eq!(
            Arc::strong_count(&shared),
            1,
            "no thread holds the shared state"
        );
    }

    /// Third's yes (04:37): said once, after 10 s with no permission, and
    /// never again in the process's life; a permission in between starts the
    /// 10 s again.
    #[test]
    fn no_permission_is_said_once_after_ten_seconds() {
        let mut unpermitted = Unpermitted::default();
        let s = |seconds: u64| seconds * 1_000_000;
        assert_eq!(unpermitted.note(false, s(0)), None);
        assert_eq!(unpermitted.note(true, s(9)), None);
        assert_eq!(unpermitted.note(false, s(10)), None, "permitted at 9 s");
        assert_eq!(unpermitted.note(false, s(19)), None);
        assert!(
            unpermitted
                .note(false, s(20))
                .is_some_and(|line| line.contains("no permission from Voice"))
        );
        assert_eq!(unpermitted.note(false, s(60)), None, "once only");
        assert_eq!(unpermitted.note(true, s(61)), None);
        assert_eq!(
            unpermitted.note(false, s(200)),
            None,
            "once in the process's life"
        );
    }
}
