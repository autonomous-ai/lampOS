//! The optional final microphone boundary. Only Voice's fresh lease permits a
//! handle to exist; Controls observations and speaking metadata never do.
use crate::{BLOCK, Heard, Listener, Settings};
use lamp_bus::Message;
use lamp_msgs::MicrophonePrivacy;
use std::{
    collections::BTreeMap,
    io,
    sync::atomic::{AtomicU64, Ordering},
};

pub const FRESH_US: u64 = lamp_msgs::MICROPHONE_PRIVACY_FRESH_US;
const MAX_RETIRED: usize = 32;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The feature is deliberately absent from CLI defaults and file analysis.
/// Only an exact `on` selects it, as in Controls and Voice; any other value,
/// an empty one included, keeps the existing loop rather than stopping Ears.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Lineage {
    session: String,
    controller: String,
    generation: u64,
}

/// Metadata can leave only in the same capture era and before its original
/// lease expires. A later heartbeat must not extend queued metadata's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permit {
    pub epoch: u64,
    pub until_us: u64,
}

/// The Bus pump never owns capture. These atomics let it reject queued output
/// immediately on loss, without waiting for or blocking the microphone owner.
#[derive(Default)]
pub struct PublicationGate {
    fence: AtomicU64,
    epoch: AtomicU64,
    until_us: AtomicU64,
}

impl PublicationGate {
    pub fn fence(&self) -> u64 {
        self.until_us.store(0, Ordering::Release);
        let _ = self
            .fence
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                old.checked_add(1)
            });
        self.generation()
    }

    pub fn generation(&self) -> u64 {
        self.fence.load(Ordering::Acquire)
    }

    pub fn update(&self, permit: Option<Permit>, generation: u64) {
        self.until_us.store(0, Ordering::Release);
        if generation != self.generation() || generation == u64::MAX {
            return;
        }
        if let Some(permit) = permit {
            self.epoch.store(permit.epoch, Ordering::Release);
            self.until_us.store(permit.until_us, Ordering::Release);
        }
    }

    pub fn allows(&self, permit: Permit, generation: u64, now: u64) -> bool {
        generation != u64::MAX
            && generation == self.generation()
            && permit.epoch == self.epoch.load(Ordering::Acquire)
            && now < permit.until_us
            && now < self.until_us.load(Ordering::Acquire)
            && generation == self.generation()
    }
}

pub struct LeaseGate {
    clock: String,
    lineage: Option<Lineage>,
    retired: BTreeMap<String, u64>,
    sequence: Option<u64>,
    at_us: u64,
    observed_us: u64,
    last_now: u64,
    until_us: u64,
    epoch: u64,
    exhausted: bool,
    /// The Voice worker session that has reported a muted switch. Voice ends
    /// such a worker and never grants from it again; Ears holds it to that.
    muted_session: Option<String>,
}

impl LeaseGate {
    pub fn new(clock_id: String) -> Self {
        Self {
            clock: clock_id,
            lineage: None,
            retired: BTreeMap::new(),
            sequence: None,
            at_us: 0,
            observed_us: 0,
            last_now: 0,
            until_us: 0,
            epoch: 0,
            exhausted: false,
            muted_session: None,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn close(&mut self) {
        self.until_us = 0;
        match self.epoch.checked_add(1) {
            Some(next) => self.epoch = next,
            None => self.exhausted = true,
        }
    }

    fn clock(&mut self, now: u64) -> io::Result<()> {
        if self.exhausted || now < self.last_now {
            self.exhausted = true;
            self.close();
            return Err(invalid("privacy clock or counter exhausted"));
        }
        self.last_now = now;
        // No previously published lease can outlive retirement + FRESH_US:
        // publication cannot be in the future, and its lifetime is immutable.
        // Keep the global publication high-water even after forgetting an ID.
        self.retired.retain(|_, expires_at| now < *expires_at);
        Ok(())
    }

    pub fn permit(&mut self, now: u64) -> io::Result<Option<Permit>> {
        self.clock(now)?;
        if self.until_us != 0 && now >= self.until_us {
            self.close();
        }
        Ok((self.until_us > now).then_some(Permit {
            epoch: self.epoch,
            until_us: self.until_us,
        }))
    }

    pub fn observe(&mut self, message: &Message, now: u64, unix_ms: u64) -> io::Result<()> {
        let result = self.observe_inner(message, now, unix_ms);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn observe_inner(&mut self, message: &Message, now: u64, unix_ms: u64) -> io::Result<()> {
        self.clock(now)?;
        // Expiry is a discontinuity even if a fresh replacement is already
        // queued. Close the old handle before that replacement can reopen it.
        let _ = self.permit(now)?;
        let lease = MicrophonePrivacy::from_message(message, &self.clock, now, unix_ms)
            .map_err(|_| invalid("invalid microphone privacy envelope or lease"))?;
        let next = Lineage {
            session: lease.session_id.clone(),
            controller: lease.controller_boot.clone(),
            generation: lease.microphone_generation,
        };
        let session_changed = self
            .lineage
            .as_ref()
            .is_some_and(|old| old.session != next.session);
        if self.lineage.is_some() && lease.stamp.at_us < self.at_us {
            return Err(invalid("privacy publication time regressed"));
        }
        if session_changed
            && (self.retired.contains_key(&next.session) || self.retired.len() == MAX_RETIRED)
        {
            return Err(invalid("retired privacy session or full session fence"));
        }
        if let Some(old) = &self.lineage
            && old.session == next.session
            && old.controller == next.controller
            && (next.generation < old.generation
                || lease.physical_muted.is_some() && lease.observed_us < self.observed_us)
        {
            return Err(invalid("privacy observation or generation regressed"));
        }
        let previous_sequence = if session_changed { None } else { self.sequence };
        if previous_sequence.is_some_and(|old| lease.stamp.sequence <= old) {
            return Err(invalid("duplicate or reordered privacy sequence"));
        }
        let gap = message.gap != 0
            || previous_sequence
                .is_some_and(|old| old.checked_add(1) != Some(lease.stamp.sequence));
        if session_changed {
            let Some(expires_at) = now.checked_add(FRESH_US) else {
                self.exhausted = true;
                return Err(invalid("privacy retirement clock exhausted"));
            };
            self.retired.insert(
                self.lineage.as_ref().expect("checked").session.clone(),
                expires_at,
            );
        }
        if self.lineage.as_ref() != Some(&next) || message.publisher_restarted {
            self.close();
        }
        self.lineage = Some(next);
        self.sequence = Some(lease.stamp.sequence);
        self.at_us = lease.stamp.at_us;
        if lease.physical_muted.is_some() {
            self.observed_us = lease.observed_us;
        }
        if lease.physical_muted == Some(true) {
            self.muted_session = Some(lease.session_id.clone());
        }
        if gap {
            // Keep this high-water so the following contiguous heartbeat can
            // recover, after the caller has destroyed the old capture era.
            return Err(invalid("privacy sequence gap"));
        }
        if lease.permitted && self.muted_session.as_deref() == Some(lease.session_id.as_str()) {
            self.close();
            return Err(invalid(
                "a privacy worker that reported muted cannot permit capture again",
            ));
        }
        if lease.permitted {
            self.until_us = lease
                .expires_at_us
                .min(lease.observed_us.saturating_add(FRESH_US));
        } else {
            self.close();
        }
        Ok(())
    }
}

/// `read` must be nonblocking and bounded. Dropping a handle must release it;
/// `discard` abandons driver buffers before that release, never drains audio.
pub trait Capture {
    fn read(&mut self, samples: &mut [i16]) -> io::Result<usize>;
    fn discard(&mut self) -> io::Result<()>;
}

pub struct Emission {
    pub heard: Heard,
    pub permit: Permit,
}

/// Exactly one handle and one bounded partial block. This owner performs no
/// Bus, filesystem, network or publication I/O.
pub struct CaptureOwner<D: Capture> {
    gate: LeaseGate,
    handle: Option<D>,
    handle_epoch: u64,
    settings: Settings,
    listener: Listener,
    samples: [i16; BLOCK],
    filled: usize,
    partial_permit: Option<Permit>,
    sequence: u64,
    before_close: Option<Box<dyn FnMut()>>,
}

impl<D: Capture> CaptureOwner<D> {
    pub fn new(clock_id: String, settings: Settings) -> Self {
        Self {
            gate: LeaseGate::new(clock_id),
            handle: None,
            handle_epoch: 0,
            settings,
            listener: Listener::new(settings),
            samples: [0; BLOCK],
            filled: 0,
            partial_permit: None,
            sequence: 0,
            before_close: None,
        }
    }

    /// A bounded, nonblocking revocation hook, called before driver discard on
    /// every close path, including errors inside read/observe/maintain. No I/O.
    pub fn before_close(&mut self, hook: impl FnMut() + 'static) {
        self.before_close = Some(Box::new(hook));
    }

    pub fn is_open(&self) -> bool {
        self.handle.is_some()
    }

    pub fn floor_db(&self) -> Option<f32> {
        self.listener.floor_db()
    }

    fn close_handle(&mut self) -> io::Result<()> {
        if let Some(revoke) = self.before_close.as_mut() {
            revoke();
        }
        // Audio has no capture-epoch field. Reserve one missing sequence for
        // every real discontinuity, even between complete blocks, so a short
        // mute/reopen cannot look like contiguous PCM to a recorder.
        let discontinuity = self.handle.is_some() || self.filled != 0;
        let overflow = discontinuity && self.sequence == u64::MAX;
        if discontinuity {
            self.sequence = self.sequence.saturating_add(1);
        }
        self.samples.fill(0);
        self.filled = 0;
        self.partial_permit = None;
        self.listener = Listener::new(self.settings);
        // Take first: even discard failure must drop the actual handle.
        self.handle
            .take()
            .map_or(Ok(()), |mut handle| handle.discard())
            .and(if overflow {
                Err(invalid("PCM sequence exhausted"))
            } else {
                Ok(())
            })
    }

    pub fn invalidate(&mut self) -> io::Result<()> {
        self.gate.close();
        self.close_handle()
    }

    pub fn observe(&mut self, message: &Message, now: u64, unix_ms: u64) -> io::Result<()> {
        let result = self.gate.observe(message, now, unix_ms);
        if result.is_err() {
            let close = self.invalidate();
            return close.and(result);
        }
        self.maintain(now).map(|_| ())
    }

    /// Must run on every bounded owner tick, including while there is no input.
    pub fn maintain(&mut self, now: u64) -> io::Result<Option<Permit>> {
        if self.sequence == u64::MAX {
            self.close_handle()?;
            return Err(invalid("PCM sequence exhausted"));
        }
        let permit = match self.gate.permit(now) {
            Ok(permit) => permit,
            Err(error) => {
                self.close_handle()?;
                return Err(error);
            }
        };
        let reset = permit.is_none()
            || self.handle.is_some() && self.handle_epoch != self.gate.epoch
            || self.partial_permit.is_some_and(|held| now >= held.until_us);
        if reset {
            self.close_handle()?;
        }
        Ok(permit)
    }

    /// At most one open and one nonblocking read. Clock checks after each
    /// potentially slow device call prevent a late result escaping its lease.
    pub fn tick(
        &mut self,
        now: impl FnMut() -> u64,
        open: impl FnMut() -> io::Result<D>,
        lamp_speaking: bool,
    ) -> io::Result<Option<Emission>> {
        self.tick_while(now, open, lamp_speaking, || true)
    }

    /// An independent receiver may revoke while a device call is in progress.
    /// Check that fence at each boundary as well as the original lease clock.
    pub fn tick_while(
        &mut self,
        now: impl FnMut() -> u64,
        open: impl FnMut() -> io::Result<D>,
        lamp_speaking: bool,
        current: impl FnMut() -> bool,
    ) -> io::Result<Option<Emission>> {
        self.tick_export_while(now, open, lamp_speaking, current, |_, _, _, _| {})
    }

    /// Optional borrowed PCM handoff, only after the complete read passed its
    /// original lease and independent fence. The callback must not block or do I/O.
    pub fn tick_export_while(
        &mut self,
        mut now: impl FnMut() -> u64,
        mut open: impl FnMut() -> io::Result<D>,
        lamp_speaking: bool,
        mut current: impl FnMut() -> bool,
        mut export: impl FnMut(&[i16; BLOCK], Permit, u64, u64),
    ) -> io::Result<Option<Emission>> {
        if !current() {
            self.invalidate()?;
            return Ok(None);
        }
        let Some(mut permit) = self.maintain(now())? else {
            return Ok(None);
        };
        if self.handle.is_none() {
            if !current() {
                self.invalidate()?;
                return Ok(None);
            }
            self.handle = Some(open()?);
            self.handle_epoch = permit.epoch;
            if !current() {
                self.invalidate()?;
                return Ok(None);
            }
            let Some(current) = self.maintain(now())? else {
                return Ok(None);
            };
            permit = current;
        }
        if self.filled == 0 {
            self.partial_permit = Some(permit);
        }
        if !current() {
            self.invalidate()?;
            return Ok(None);
        }
        let count = match self
            .handle
            .as_mut()
            .expect("opened under a fresh permit")
            .read(&mut self.samples[self.filled..])
        {
            Ok(count) if count <= BLOCK - self.filled => count,
            Ok(_) => {
                self.close_handle()?;
                return Err(invalid("capture read exceeded its buffer"));
            }
            Err(error) => {
                self.close_handle()?;
                return Err(error);
            }
        };
        let read_completed_us = now();
        if !current() {
            self.invalidate()?;
            return Ok(None);
        }
        let Some(current) = self.maintain(now())? else {
            return Ok(None);
        };
        if !self.is_open() {
            return Ok(None);
        }
        self.filled += count;
        if self.filled != BLOCK {
            return Ok(None);
        }
        let held = self.partial_permit.take().expect("block has a lease");
        let permit = Permit {
            epoch: current.epoch,
            until_us: current.until_us.min(held.until_us),
        };
        let sequence = self.sequence;
        let Some(next) = sequence.checked_add(1) else {
            self.invalidate()?;
            return Err(invalid("PCM sequence exhausted"));
        };
        self.sequence = next;
        export(&self.samples, permit, sequence, read_completed_us);
        let heard = self.listener.block(&self.samples, lamp_speaking);
        self.samples.fill(0);
        self.filled = 0;
        Ok(Some(Emission { heard, permit }))
    }
}

impl<D: Capture> Drop for CaptureOwner<D> {
    fn drop(&mut self) {
        let _ = self.close_handle();
    }
}

#[cfg(test)]
mod tests;
