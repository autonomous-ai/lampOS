use crate::{
    Admission, INITIAL_LEASE_WAIT_US, LEASE_WARMUP_US, MAX_AGE_US, MAX_FRAME_BYTES,
    SOURCE_STALL_US, Warmup,
    archive::{Archive, Packet},
    consent, invalid,
};
use lamp_bus::{Bus, Message, Notice, Subscriber};
use lamp_camera::frames::{FrameClient, FrameMeta, READ_SLICE_BYTES, Role};
use lamp_ears::privacy::LeaseGate;
use lamp_msgs::{Audio, AudioCapturePermit, MicrophonePrivacy};
use serde_json::json;
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Options {
    pub bus_root: PathBuf,
    pub link_dir: PathBuf,
    pub output: PathBuf,
    pub consent_record: PathBuf,
    pub consent_text: PathBuf,
    pub consent_sha256: String,
    pub consent_alias: String,
    pub operator_alias: String,
    pub seconds: u64,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Clone, Copy)]
struct Time {
    monotonic_us: u64,
    unix_ms: u64,
}

fn current_time() -> Time {
    Time {
        monotonic_us: lamp_ipc::monotonic_us(),
        unix_ms: unix_ms(),
    }
}

/// A status check may finish a legitimate outstanding request. Keep that
/// frame in one bounded slot until the normal consumer takes it; never poll
/// a source frame into a throwaway buffer to check for EOF.
struct CameraInput {
    client: FrameClient,
    pending: Option<(FrameMeta, Vec<u8>)>,
    scratch: Vec<u8>,
}

impl CameraInput {
    fn new(client: FrameClient) -> Self {
        Self {
            client,
            pending: None,
            scratch: Vec::new(),
        }
    }

    fn probe(&mut self) -> io::Result<()> {
        // One maximum-sized pending frame, metadata, then a queued EOF.
        // FrameClient::poll is itself nonblocking and slice-bounded.
        for _ in 0..MAX_FRAME_BYTES.div_ceil(READ_SLICE_BYTES) + 2 {
            if let Some(meta) = self.client.poll(&mut self.scratch)? {
                if self.pending.is_some() {
                    return Err(invalid("more than one pending camera frame"));
                }
                self.pending = Some((meta, std::mem::take(&mut self.scratch)));
            }
        }
        Ok(())
    }

    fn take(&mut self) -> Option<(FrameMeta, Vec<u8>)> {
        self.pending.take()
    }

    fn request_if_idle(&mut self) -> io::Result<()> {
        if self.client.is_connected()
            && self.client.outstanding().is_none()
            && self.pending.is_none()
        {
            self.client.request(MAX_AGE_US as u32)?;
        }
        Ok(())
    }

    fn disconnect(&mut self) {
        self.client.disconnect();
        if let Some((_, bytes)) = self.pending.as_mut() {
            bytes.fill(0);
        }
        self.pending = None;
        self.scratch.fill(0);
        self.scratch.clear();
    }
}

struct Workers {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    privacy: Receiver<Message>,
    audio: Receiver<Message>,
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn counters(subscriber: &Subscriber) -> (u64, u64, u64, u64) {
    (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
        subscriber.repeated(),
    )
}

fn spawn_input(
    bus: Bus,
    privacy: bool,
    clock: String,
    sender: SyncSender<Message>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> io::Result<()> {
    lamp_ipc::spawn_without_signals(
        if privacy {
            "sensor5-privacy"
        } else {
            "sensor5-audio"
        },
        move || {
            let result = (|| -> io::Result<()> {
                let (topic, kind, name) = if privacy {
                    (
                        MicrophonePrivacy::TOPIC,
                        MicrophonePrivacy::TYPE,
                        "sensor5_privacy",
                    )
                } else {
                    ("mic1/audio", Audio::TYPE, "sensor5_audio")
                };
                let mut subscriber = bus.subscribe_typed(topic, name, kind)?;
                let mut gate = LeaseGate::new(clock);
                let mut epoch = None;
                while !stop.load(Ordering::Acquire) {
                    let before = counters(&subscriber);
                    let message = subscriber.recv(Duration::from_millis(2))?;
                    if before != counters(&subscriber)
                        || subscriber
                            .notices()
                            .iter()
                            .any(|n| matches!(n, Notice::SourceGone | Notice::Late))
                    {
                        return Err(invalid("source malformed, expired, repeated, lost or late"));
                    }
                    if let Some(message) = message {
                        if privacy {
                            gate.observe(&message, lamp_ipc::monotonic_us(), unix_ms())?;
                            let permit = gate
                                .permit(lamp_ipc::monotonic_us())?
                                .ok_or_else(|| invalid("privacy denied"))?;
                            if epoch.is_some_and(|old| old != permit.epoch) {
                                return Err(invalid("privacy era changed"));
                            }
                            epoch = Some(permit.epoch);
                        }
                        sender
                            .try_send(message)
                            .map_err(|_| invalid("bounded source queue full or reader gone"))?;
                    }
                }
                Ok(())
            })();
            if result.is_err() {
                failed.store(true, Ordering::Release);
            }
        },
    )?;
    Ok(())
}

fn drain(admission: &mut Admission, workers: &Workers) -> io::Result<bool> {
    drain_at(admission, workers, &current_time)
}

fn drain_at(
    admission: &mut Admission,
    workers: &Workers,
    clock: &impl Fn() -> Time,
) -> io::Result<bool> {
    if workers.failed.load(Ordering::Acquire) {
        admission.close();
        return Err(invalid("source observer failed; closed"));
    }
    let mut observed = false;
    for _ in 0..16 {
        match workers.privacy.try_recv() {
            Ok(message) => {
                let now = clock();
                admission.observe(&message, now.monotonic_us, now.unix_ms)?;
                observed = true;
            }
            Err(TryRecvError::Empty) => return Ok(observed),
            Err(TryRecvError::Disconnected) => {
                admission.close();
                return Err(invalid("privacy observer ended"));
            }
        }
    }
    admission.close();
    Err(invalid("privacy drain exhausted bounded budget"))
}

/// Shared final camera/privacy fence for packet retention and a successful
/// terminal marker. Observe pending EOF after any slow preparation, and give
/// observer loss/revoke precedence over both retention and duration success.
fn live_fence(
    admission: &mut Admission,
    workers: &Workers,
    camera: &mut CameraInput,
    clock: &impl Fn() -> Time,
) -> io::Result<()> {
    drain_at(admission, workers, clock)?;
    if let Err(error) = camera.probe() {
        admission.close();
        return Err(error);
    }
    if workers.stop.load(Ordering::Acquire) || workers.failed.load(Ordering::Acquire) {
        admission.close();
        return Err(invalid(
            "stop or observer failure won at the final source fence",
        ));
    }
    admission.ticket(clock().monotonic_us)?;
    Ok(())
}

fn duration_complete(
    admission: &mut Admission,
    workers: &Workers,
    camera: &mut CameraInput,
    begin: u64,
    duration_us: u64,
    counts: (u64, u64),
    clock: &impl Fn() -> Time,
) -> io::Result<bool> {
    if clock().monotonic_us.saturating_sub(begin) < duration_us {
        return Ok(false);
    }
    live_fence(admission, workers, camera, clock)?;
    if counts.0 == 0 || counts.1 == 0 {
        return Err(invalid("duration ended without both media sources"));
    }
    Ok(true)
}

struct PreparedMedia {
    packet: Packet,
    source_completed_us: u64,
    source_permit: Option<AudioCapturePermit>,
}

fn admit_packet(
    admission: &mut Admission,
    workers: &Workers,
    archive: &mut Archive,
    camera: &mut CameraInput,
    media: PreparedMedia,
) -> io::Result<()> {
    admit_packet_with(
        admission,
        workers,
        camera,
        media,
        &current_time,
        Packet::encode,
        |encoded| archive.enqueue(encoded),
    )
}

fn admit_packet_with(
    admission: &mut Admission,
    workers: &Workers,
    camera: &mut CameraInput,
    media: PreparedMedia,
    clock: &impl Fn() -> Time,
    encode: impl FnOnce(Packet) -> io::Result<Vec<u8>>,
    enqueue: impl FnOnce(Vec<u8>) -> io::Result<()>,
) -> io::Result<()> {
    let ticket = admission.ticket(clock().monotonic_us)?;
    let encoded = encode(media.packet)?;
    live_fence(admission, workers, camera, clock)?;
    admission.final_check(
        ticket,
        media.source_completed_us,
        media.source_permit.as_ref(),
        clock().monotonic_us,
    )?;
    enqueue(encoded)
}

pub fn record(options: Options) -> io::Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(invalid(
            "live source recording is Linux-only; no device was opened",
        ));
    }
    let cgroup = fs::read_to_string("/proc/self/cgroup")?;
    if lamp_ipc::caller::unit_of(&cgroup).as_deref()
        != Some(lamp_eyes::frame_link::SENSOR5_READER_UNIT)
    {
        return Err(invalid(
            "record must run in the explicitly authorized lamp-sensor5-capture.service; no source was opened",
        ));
    }
    if !(1..=120).contains(&options.seconds) {
        return Err(invalid("record duration is 1 through 120 seconds"));
    }
    let accepted = consent::load(
        &options.consent_record,
        &options.consent_text,
        &options.consent_sha256,
        &options.consent_alias,
        &options.operator_alias,
    )?;
    let socket = options.link_dir.join("camera.frames");
    let (controller, reader) =
        lamp_eyes::frame_link::boots_from(&options.link_dir.join("frame-link.env"), &socket)?
            .ok_or_else(|| invalid("private prepared link boots missing"))?;
    if options.output.exists() {
        return Err(invalid("output already exists; use a fresh directory"));
    }
    let clock = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned();
    if clock.len() != 36 {
        return Err(invalid("unknown kernel clock identity"));
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let mut archive = Archive::spawn(&std::env::current_exe()?, &options.output)?;
    let session = json!({"schema_version":1,"tool":"lamp-sensor5-recorder","participant_alias":options.consent_alias,
        "operator_alias":options.operator_alias,"consent_record_sha256":accepted.record_sha256,
        "consent_text_sha256":accepted.record.consent_text_sha256,"clock_id":clock,
        "physical_sync_validated":false,"duration_limit_seconds":options.seconds,
        "camera_source":"existing_eyes_frame_link","audio_source":"existing_ears_mic1_audio",
        "camera_cadence":"native_variable_pull_cadence_not_guaranteed_25fps",
        "audio_timestamp_boundary":"host_capture_read_completion_not_ADC",
        "retention_boundary":"parent_final_admission_before_bounded_archive_queue",
        "max_source_age_us":MAX_AGE_US,"source_stall_us":SOURCE_STALL_US,
        "initial_lease_wait_limit_us":INITIAL_LEASE_WAIT_US,"continuous_lease_warmup_us":LEASE_WARMUP_US,
        "duration_begins_after_warmup":true,"ears_original_permit_epoch_in_payload":true});
    archive.enqueue(Packet{header:json!({"kind":"start","session":session,
        "consent_record":String::from_utf8(accepted.record_bytes).map_err(|_|invalid("consent JSON is not UTF-8"))?,
        "consent_text":String::from_utf8(accepted.text_bytes).map_err(|_|invalid("consent text is not UTF-8"))?}),bytes:vec![]}.encode()?)?;
    let setup_deadline = Instant::now() + Duration::from_secs(5);
    while !archive.ready {
        if stop.load(Ordering::Acquire) || Instant::now() >= setup_deadline {
            return Err(invalid(
                "archive setup cancelled or stalled; no sources admitted",
            ));
        }
        archive.poll()?;
        std::thread::sleep(Duration::from_millis(2));
    }
    let started = lamp_ipc::monotonic_us();
    let mut admission = Admission::new(clock.clone(), started);
    let bus = Bus::new(&options.bus_root)?;
    let failed = Arc::new(AtomicBool::new(false));
    let (privacy_tx, privacy) = mpsc::sync_channel(16);
    let (audio_tx, audio) = mpsc::sync_channel(32);
    spawn_input(
        bus.clone(),
        true,
        clock.clone(),
        privacy_tx,
        stop.clone(),
        failed.clone(),
    )?;
    let workers = Workers {
        stop: stop.clone(),
        failed,
        privacy,
        audio,
    };
    let mut warmup = Warmup::new(started);
    let mut capture_started_us = None;
    let mut frames = CameraInput::new(FrameClient::new(
        &socket,
        Role::Perception,
        reader,
        controller,
        MAX_FRAME_BYTES,
    ));
    let (mut frame_count, mut audio_count) = (0u64, 0u64);
    let (mut last_frame, mut last_audio) = (started, started);
    let mut audio_tx = Some(audio_tx);
    let run = (|| -> io::Result<&'static str> {
        loop {
            let now = lamp_ipc::monotonic_us();
            if stop.load(Ordering::Acquire) {
                return Ok("operator_stop");
            }
            drain(&mut admission, &workers)?;
            if capture_started_us.is_none() {
                let Some(begin) = warmup.step(&mut admission, lamp_ipc::monotonic_us())? else {
                    archive.poll()?;
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                capture_started_us = Some(begin);
                last_frame = begin;
                last_audio = begin;
                spawn_input(
                    bus.clone(),
                    false,
                    clock.clone(),
                    audio_tx.take().expect("starts once"),
                    stop.clone(),
                    workers.failed.clone(),
                )?;
            }
            admission.ticket(lamp_ipc::monotonic_us())?;
            if !frames.client.is_connected() {
                frames.client.connect()?;
            }
            if duration_complete(
                &mut admission,
                &workers,
                &mut frames,
                capture_started_us.expect("warmup completed"),
                options.seconds * 1_000_000,
                (frame_count, audio_count),
                &current_time,
            )? {
                return Ok("duration_elapsed");
            }
            frames.probe()?;
            if let Some((meta, jpeg)) = frames.take() {
                let header = admission.frame(&meta, &jpeg, lamp_ipc::monotonic_us())?;
                admit_packet(
                    &mut admission,
                    &workers,
                    &mut archive,
                    &mut frames,
                    PreparedMedia {
                        packet: Packet {
                            header,
                            bytes: jpeg,
                        },
                        source_completed_us: meta.dequeue_completed_at_us,
                        source_permit: None,
                    },
                )?;
                frame_count += 1;
                last_frame = now;
            }
            for _ in 0..4 {
                let message = match workers.audio.try_recv() {
                    Ok(message) => message,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Err(invalid("audio observer ended")),
                };
                let header = admission.audio(&message, &clock, lamp_ipc::monotonic_us())?;
                let (_, source_permit, samples) =
                    Audio::split_with_capture_permit(&message.payload)?;
                admit_packet(
                    &mut admission,
                    &workers,
                    &mut archive,
                    &mut frames,
                    PreparedMedia {
                        packet: Packet {
                            header,
                            bytes: samples.to_vec(),
                        },
                        source_completed_us: message.header.at_us,
                        source_permit: Some(
                            source_permit
                                .ok_or_else(|| invalid("PCM original capture ticket missing"))?,
                        ),
                    },
                )?;
                audio_count += 1;
                last_audio = now;
            }
            if now.saturating_sub(last_frame) >= SOURCE_STALL_US
                || now.saturating_sub(last_audio) >= SOURCE_STALL_US
            {
                return Err(invalid(
                    "camera or PCM source stalled; incomplete clip retained",
                ));
            }
            drain(&mut admission, &workers)?;
            admission.ticket(lamp_ipc::monotonic_us())?;
            frames.request_if_idle()?;
            archive.poll()?;
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    admission.close();
    stop.store(true, Ordering::Release);
    frames.disconnect();
    let reason = match &run {
        Ok(reason) => (*reason).to_owned(),
        Err(error) => error.to_string(),
    };
    let finish = archive.stop(
        json!({"reason":reason,"frames_admitted":frame_count,"audio_blocks_admitted":audio_count,
        "physical_sync_validated":false,"capture_started_us":capture_started_us,
        "source_admission_closed_us":lamp_ipc::monotonic_us()}),
    );
    eprintln!(
        "sensor5: source admission closed; {frame_count} frames and {audio_count} PCM blocks admitted, {} archive records acknowledged; partial evidence retained",
        archive.acknowledged
    );
    match (run, finish) {
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok("duration_elapsed"), Ok(())) if frame_count > 0 && audio_count > 0 => Ok(()),
        _ => Err(invalid(
            "recording stopped without a complete nonempty clip",
        )),
    }
}

#[cfg(test)]
mod tests;
