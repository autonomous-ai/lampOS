//! Local diagnostic retention after the existing Eyes/Ears admission fences.
//! This crate never opens camera/microphone devices or grants permission.
pub mod archive;
pub mod consent;
pub mod runtime;

use lamp_bus::{Kind, Message};
use lamp_camera::frames::FrameMeta;
use lamp_ears::privacy::{LeaseGate, Permit};
use lamp_msgs::{Audio, AudioCapturePermit};
use lamp_turn_owner::{BootId, CameraGrant};
use serde_json::{Value, json};
use std::io;

pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_AGE_US: u64 = 50_000;
pub const SOURCE_STALL_US: u64 = 1_000_000;
pub const INITIAL_LEASE_WAIT_US: u64 = 1_000_000;
pub const LEASE_WARMUP_US: u64 = 50_000;

pub fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

/// One continuous permission era. Any observed loss is terminal for a clip;
/// new heartbeats, restarted publishers or a reopened camera cannot resume it.
pub struct Admission {
    clock_id: String,
    lease: LeaseGate,
    epoch: Option<u64>,
    closed: bool,
    started_us: u64,
    camera: Option<(CameraGrant, BootId, u64, u64, u64)>,
    audio: Option<(String, u64, u64, u64, u64)>,
    evidence: Option<Value>,
}

/// Conservative startup fence in addition to each Ears block's required
/// original capture ticket. This cannot reconstruct missing source authority.
/// Loss during warmup is terminal; this is not a permission source or retry.
pub struct Warmup {
    waiting_since_us: u64,
    first_valid_us: Option<u64>,
    last_now_us: u64,
}

impl Warmup {
    pub fn new(now: u64) -> Self {
        Self {
            waiting_since_us: now,
            first_valid_us: None,
            last_now_us: now,
        }
    }

    pub fn step(&mut self, admission: &mut Admission, now: u64) -> io::Result<Option<u64>> {
        if now < self.last_now_us
            || now.saturating_sub(self.waiting_since_us) >= INITIAL_LEASE_WAIT_US
        {
            admission.close();
            return Err(invalid(
                "initial privacy warmup timed out or clock regressed",
            ));
        }
        self.last_now_us = now;
        if admission.closed {
            return Err(invalid("privacy warmup is terminally closed"));
        }
        if admission.epoch.is_none() {
            return Ok(None);
        }
        admission.ticket(now)?;
        let first = *self.first_valid_us.get_or_insert(now);
        if now - first < LEASE_WARMUP_US {
            return Ok(None);
        }
        if admission.camera.is_some() || admission.audio.is_some() {
            admission.close();
            return Err(invalid("media appeared before warmup completed"));
        }
        admission.started_us = now;
        Ok(Some(now))
    }
}

impl Admission {
    pub fn new(clock: String, started_us: u64) -> Self {
        Self {
            clock_id: clock.clone(),
            lease: LeaseGate::new(clock),
            epoch: None,
            closed: false,
            started_us,
            camera: None,
            audio: None,
            evidence: None,
        }
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.lease.close();
    }

    pub fn observe(&mut self, message: &Message, now: u64, unix_ms: u64) -> io::Result<()> {
        if self.closed {
            return Err(invalid("clip admission is closed"));
        }
        if let Err(error) = self.lease.observe(message, now, unix_ms) {
            self.close();
            return Err(error);
        }
        let Some(permit) = self.lease.permit(now)? else {
            self.close();
            return Err(invalid("privacy not permitted"));
        };
        if self.epoch.is_some_and(|old| old != permit.epoch) {
            self.close();
            return Err(invalid("privacy lineage changed"));
        }
        self.epoch = Some(permit.epoch);
        self.evidence =
            Some(json!({"header":message.header,"lease":message.json()?,"received_us":now}));
        Ok(())
    }

    pub fn ticket(&mut self, now: u64) -> io::Result<Permit> {
        if self.closed {
            return Err(invalid("clip admission is closed"));
        }
        match self.lease.permit(now)? {
            Some(permit) if self.epoch == Some(permit.epoch) => Ok(permit),
            _ => {
                self.close();
                Err(invalid("privacy missing or stale"))
            }
        }
    }

    /// Final check before transfer to the bounded archival queue. Once this
    /// succeeds, those bytes are explicitly retained; the child is only a
    /// writer and can never admit another source frame.
    pub fn final_check(
        &mut self,
        ticket: Permit,
        source_completed_us: u64,
        source_permit: Option<&AudioCapturePermit>,
        now: u64,
    ) -> io::Result<()> {
        let current = self.ticket(now)?;
        if current.epoch != ticket.epoch || now >= ticket.until_us {
            self.close();
            return Err(invalid(
                "queued input outlived its original admission ticket",
            ));
        }
        if source_completed_us < self.started_us
            || now
                .checked_sub(source_completed_us)
                .is_none_or(|age| age >= MAX_AGE_US)
        {
            self.close();
            return Err(invalid("source expired while preparing archival admission"));
        }
        if let Some(source) = source_permit
            && (source.clock_id != self.clock_id
                || source.epoch == 0
                || self
                    .audio
                    .as_ref()
                    .is_none_or(|(_, _, _, _, epoch)| *epoch != source.epoch)
                || source.until_us <= source_completed_us
                || now >= source.until_us)
        {
            self.close();
            return Err(invalid(
                "original Ears capture ticket changed or expired before retention",
            ));
        }
        Ok(())
    }

    pub fn frame(&mut self, meta: &FrameMeta, bytes: &[u8], now: u64) -> io::Result<Value> {
        self.ticket(now)?;
        if meta.source != "lamp-eyes"
            || meta.format != *b"MJPG"
            || meta.derived.is_some()
            || meta.width == 0
            || meta.height == 0
            || meta.frame_sequence == 0
            || bytes.len() < 4
            || bytes.len() > MAX_FRAME_BYTES
            || bytes[..2] != [0xff, 0xd8]
            || bytes[bytes.len() - 2..] != [0xff, 0xd9]
            || meta.dequeue_started_at_us > meta.dequeue_completed_at_us
            || meta.dequeue_completed_at_us > meta.copied_at_us
            || meta.dequeue_completed_at_us < self.started_us
            || meta.copied_at_us > now
            || now.saturating_sub(meta.dequeue_completed_at_us) >= MAX_AGE_US
        {
            self.close();
            return Err(invalid(
                "camera input is malformed, late or predates this clip",
            ));
        }
        let mut gap = 0;
        if let Some((grant, worker, epoch, sequence, at)) = self.camera {
            if grant != meta.grant
                || worker != meta.camera_worker
                || epoch != meta.capture_epoch
                || sequence >= meta.frame_sequence
                || at >= meta.dequeue_completed_at_us
            {
                self.close();
                return Err(invalid("camera lineage or native frame order changed"));
            }
            gap = meta.frame_sequence - sequence - 1;
        }
        self.camera = Some((
            meta.grant,
            meta.camera_worker,
            meta.capture_epoch,
            meta.frame_sequence,
            meta.dequeue_completed_at_us,
        ));
        Ok(
            json!({"kind":"frame", "frame":meta, "received_us":now,"capture_started_us":self.started_us,
            "privacy_evidence":self.evidence,
            "source_frames_between":gap, "sequence_gaps_are_not_durations":true}),
        )
    }

    pub fn audio(&mut self, message: &Message, clock: &str, now: u64) -> io::Result<Value> {
        self.ticket(now)?;
        let h = &message.header;
        let (audio, source_permit, samples) =
            match Audio::split_with_capture_permit(&message.payload) {
                Ok(value) => value,
                Err(error) => {
                    self.close();
                    return Err(error);
                }
            };
        let Some(source_permit) = source_permit else {
            self.close();
            return Err(invalid(
                "PCM has no original capture ticket; diagnostic Ears export is required",
            ));
        };
        if h.topic != "mic1/audio"
            || h.source != "ears"
            || h.type_name.as_deref() != Some(Audio::TYPE)
            || h.enc != Audio::ENCODING
            || h.kind != Kind::Event
            || h.expires_ms != Some(50)
            || h.seq == 0
            || h.boot.len() != 16
            || !h.boot.bytes().all(|b| b.is_ascii_hexdigit())
            || message.expired
            || message.publisher_restarted
            || h.clock.as_deref().is_some_and(|value| value != clock)
            || h.at_us < self.started_us
            || h.at_us > now
            || now.saturating_sub(h.at_us) >= MAX_AGE_US
            || audio.sample_rate != 16_000
            || audio.channels != 1
            || audio.sample_format != Audio::S16LE
            || samples.len() != 640
            || clock != self.clock_id
            || source_permit.clock_id != self.clock_id
            || source_permit.epoch == 0
            || source_permit.until_us <= h.at_us
            || now >= source_permit.until_us
        {
            self.close();
            return Err(invalid(
                "PCM input is malformed, late or not the existing Ears export",
            ));
        }
        let mut gap = 0;
        if let Some((boot, sequence, bus_sequence, at, source_epoch)) = &self.audio {
            if boot != &h.boot
                || *sequence >= audio.sequence
                || *bus_sequence >= h.seq
                || *at >= h.at_us
                || *source_epoch != source_permit.epoch
            {
                self.close();
                return Err(invalid("PCM publisher or native ordering changed"));
            }
            gap = audio.sequence - sequence - 1;
        }
        self.audio = Some((
            h.boot.clone(),
            audio.sequence,
            h.seq,
            h.at_us,
            source_permit.epoch,
        ));
        Ok(
            json!({"kind":"audio", "header":h, "audio":audio, "received_us":now,"capture_started_us":self.started_us,
            "capture_permit":source_permit,
            "privacy_evidence":self.evidence,
            "bus_gap":message.gap, "audio_sequence_gap":gap,
            "timestamp_boundary":"ears_capture_read_completed", "adc_timestamp":false,
            "clock_provenance":if h.clock.is_some() {"bus_header_and_original_ears_capture_permit"} else {"original_ears_capture_permit"},
            "sequence_gaps_are_not_durations":true}),
        )
    }
}
