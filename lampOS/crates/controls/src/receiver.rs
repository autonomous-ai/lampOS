//! Optional integration helper for the owners. Accepting a request is not
//! evidence that sound stopped, motion stopped, or input permission changed.
use crate::mute::MuteRecord;
use lamp_msgs::{CONTROL_EVENT_TTL_US, ControlGesture, ControlIntent, ControlsState};
use std::{
    collections::BTreeSet,
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const STATE_FRESH_US: u64 = 300_000;
const MAX_RETIRED: usize = 32;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// Combined legacy effect: a fresh Stop or a newly accepted one-way mute.
    pub stop: bool,
    /// Only a fresh, deduplicated Stop, for owners with an independent switch.
    pub stop_requested: bool,
    /// Revoke immediately, then persist the record before acknowledging mute.
    pub mute_changed: bool,
    /// A separate fresh hold requested clear. The live latch remains muted.
    pub clear_requested: bool,
    pub software_muted: bool,
}

/// A proposed clear, not a permission or proof of persistence. The owner must
/// save `record()` successfully, then commit this exact proposal. Keep that
/// sequence synchronous in the quiescent, no-audio owner with one serialized
/// writer. A generation check cannot undo an obsolete asynchronous disk write.
#[derive(Debug)]
pub struct PreparedClear {
    instance: u64,
    expected: MuteRecord,
    record: MuteRecord,
    generation: u64,
}

impl PreparedClear {
    pub fn record(&self) -> &MuteRecord {
        &self.record
    }
}

pub struct Receiver {
    instance: u64,
    record: MuteRecord,
    session: Option<String>,
    retired: BTreeSet<String>,
    state_sequence: Option<u64>,
    event_sequence: u64,
    pet_sequence: u64,
    state_at_us: Option<u64>,
    last_now: Option<u64>,
    exhausted: bool,
    clear_after_us: Option<u64>,
    pending_clear_release_us: Option<u64>,
    generation: u64,
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

impl Receiver {
    pub fn new(record: MuteRecord) -> io::Result<Self> {
        record.check()?;
        let instance = NEXT_INSTANCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |at| at.checked_add(1))
            .map_err(|_| invalid("controls receiver identities exhausted"))?;
        Ok(Self {
            instance,
            record,
            session: None,
            retired: BTreeSet::new(),
            state_sequence: None,
            event_sequence: 0,
            pet_sequence: 0,
            state_at_us: None,
            last_now: None,
            exhausted: false,
            clear_after_us: None,
            pending_clear_release_us: None,
            generation: 0,
        })
    }

    /// Explicit owner opt-in. `new()` remains one-way. A retained hold or a
    /// contact already in progress at restore/start can never request clear.
    pub fn with_hold_clear(record: MuteRecord, started_us: u64) -> io::Result<Self> {
        if record.through_us > started_us {
            return Err(invalid("mute watermark is after the receiver start"));
        }
        let mut receiver = Self::new(record)?;
        receiver.last_now = Some(started_us);
        receiver.clear_after_us = Some(started_us);
        Ok(receiver)
    }

    pub fn mute_record(&self) -> &MuteRecord {
        &self.record
    }

    /// A request is pending, not necessarily still fresh when it is prepared.
    pub fn clear_pending(&self) -> bool {
        self.pending_clear_release_us.is_some()
    }

    /// On entry into a quiescent owner phase, require a new local hold. This
    /// does not enable clearing for a receiver created with `new()`.
    pub fn fence_hold_clear(&mut self, now_us: u64) -> io::Result<()> {
        self.clock(now_us)?;
        self.fence_clear(now_us)
    }

    /// Consume one eligible request without releasing the live software latch.
    /// Do not call save or commit in the urgent audio/coordinator path.
    pub fn prepare_clear(&mut self, now_us: u64) -> io::Result<PreparedClear> {
        self.clock(now_us)?;
        let released_us = self
            .pending_clear_release_us
            .take()
            .ok_or_else(|| invalid("no eligible local hold requested clear"))?;
        if !self.record.muted
            || now_us
                .checked_sub(released_us)
                .is_none_or(|age| age >= CONTROL_EVENT_TTL_US)
        {
            return Err(invalid("local hold clear expired or mute is no longer set"));
        }
        // Reserve the next generation before allowing a caller to save a clear.
        self.generation
            .checked_add(1)
            .ok_or_else(|| invalid("controls clear generation exhausted"))?;
        let mut record = self.record.clone();
        record.clear_after_explicit_user_request(now_us)?;
        Ok(PreparedClear {
            instance: self.instance,
            expected: self.record.clone(),
            record,
            generation: self.generation,
        })
    }

    /// Call only after saving the proposal successfully, without interleaving
    /// observations or other writers between preparation, save and commit.
    /// Stale completion never clears the live latch. This is not a disk CAS.
    pub fn commit_clear(&mut self, prepared: PreparedClear) -> io::Result<()> {
        if self.exhausted
            || self.instance != prepared.instance
            || self.generation != prepared.generation
            || self.record != prepared.expected
        {
            return Err(invalid("prepared controls clear is no longer current"));
        }
        self.advance_generation()?;
        if let Some(fence) = &mut self.clear_after_us {
            *fence = (*fence).max(prepared.record.through_us);
        }
        self.record = prepared.record;
        Ok(())
    }

    /// A hand kept three seconds on the head asks for sleep (#711): the same
    /// one-way mute, with the reason `sleep`. Call only with a head hold that
    /// `TouchReceiver` accepted, and persist it as any newly accepted mute.
    /// The lamp is awake or asleep as it was when that hand arrived: a
    /// contact begun before the last mute change or clear asks nothing, so a
    /// hand that woke it cannot send it back. A muted lamp keeps its mute and
    /// its reason: sleep never takes over a person's privacy mute.
    pub fn request_sleep(&mut self, now_us: u64, began_us: u64, at_us: u64) -> io::Result<bool> {
        self.clock(now_us)?;
        if !self.may_sleep(began_us) || at_us > now_us || !self.record.request_sleep(at_us) {
            return Ok(false);
        }
        self.fence_clear(now_us)?;
        Ok(true)
    }

    /// Whether a hold whose contact began at `began_us` could put the lamp
    /// to sleep, as [`Self::request_sleep`] fences it: awake, and the hand
    /// arrived after the last mute change or clear, so not a hand that woke
    /// it (#3748: its sleep cue is not played either).
    pub fn may_sleep(&self, began_us: u64) -> bool {
        !self.record.muted && began_us > self.record.through_us
    }

    /// A tap on the head wakes a lamp put to sleep (#711): an explicit local
    /// clear, as a fresh hold is, but only of a mute whose reason is `sleep`;
    /// a person's privacy mute stays until their hold. Call only with a head
    /// tap that `TouchReceiver` accepted. As for a hold, the tap must begin
    /// after this owner's clear fence and the sleep, and have been released
    /// within the event lifetime. It only makes the clear pending: the
    /// quiescent owner prepares, saves and commits it as for a hold. Without
    /// `with_hold_clear`, nothing clears.
    pub fn request_wake(
        &mut self,
        now_us: u64,
        began_us: u64,
        released_us: u64,
    ) -> io::Result<bool> {
        let asleep = self.record.asleep();
        self.wake(now_us, began_us, released_us, asleep)
    }

    /// The same tap clears a mute the owner knows was left by its restart
    /// guard after a fault, not chosen by a person (#960, the voice's
    /// `LAMP_RESTORED_MUTE=on`): a lamp brought up deaf by a crash can be
    /// woken at the lamp. Call only when the owner has established that; a
    /// person's own mute must keep to `request_wake`, which leaves it.
    pub fn request_wake_after_fault(
        &mut self,
        now_us: u64,
        began_us: u64,
        released_us: u64,
    ) -> io::Result<bool> {
        let muted = self.record.muted;
        self.wake(now_us, began_us, released_us, muted)
    }

    fn wake(
        &mut self,
        now_us: u64,
        began_us: u64,
        released_us: u64,
        clearable: bool,
    ) -> io::Result<bool> {
        self.clock(now_us)?;
        let eligible = clearable
            && began_us <= released_us
            && released_us <= now_us
            && now_us - released_us < CONTROL_EVENT_TTL_US
            && self
                .clear_after_us
                .is_some_and(|fence| began_us > fence.max(self.record.through_us));
        if eligible {
            self.pending_clear_release_us = Some(released_us);
        }
        Ok(eligible)
    }

    /// Must only be invoked for a separate explicit user request. This does
    /// not inspect or clear physical privacy and cannot return a capture permit.
    /// This explicit disk transaction belongs outside the urgent event loop.
    /// A save failure leaves the live inhibit intact.
    pub fn clear_after_explicit_user_request(
        &mut self,
        now_us: u64,
        path: &Path,
    ) -> io::Result<()> {
        self.clock(now_us)?;
        // Invalidate any earlier prepared clear before beginning this separate
        // explicit transaction. A failed save still leaves live mute intact.
        self.fence_clear(now_us)?;
        let mut proposed = self.record.clone();
        proposed.clear_after_explicit_user_request(now_us)?;
        proposed.save(path)?;
        self.record = proposed;
        Ok(())
    }

    fn clock(&mut self, now_us: u64) -> io::Result<()> {
        if self.exhausted || self.last_now.is_some_and(|last| now_us < last) {
            self.exhausted = true;
            self.pending_clear_release_us = None;
            return Err(invalid(
                "controls receiver clock/session fence failed; restart with restored mute",
            ));
        }
        self.last_now = Some(now_us);
        Ok(())
    }

    fn advance_generation(&mut self) -> io::Result<()> {
        self.pending_clear_release_us = None;
        let Some(next) = self.generation.checked_add(1) else {
            self.exhausted = true;
            return Err(invalid("controls clear generation exhausted"));
        };
        self.generation = next;
        Ok(())
    }

    fn fence_clear(&mut self, now_us: u64) -> io::Result<()> {
        if let Some(fence) = &mut self.clear_after_us {
            *fence = (*fence).max(now_us);
        }
        self.advance_generation()
    }

    fn reject(&mut self, now_us: u64, why: impl Into<String>) -> io::Result<Effects> {
        self.fence_clear(now_us)?;
        Err(invalid(why))
    }

    /// Consume the retained state as well as subscribing to gesture events.
    /// Pet cannot overwrite its independent stop and mute records. A recent
    /// state can carry an older one-way mute after a lost event; a Stop expires.
    pub fn observe(&mut self, now_us: u64, state: &ControlsState) -> io::Result<Effects> {
        self.clock(now_us)?;
        if let Err(error) = state.check() {
            return self.reject(now_us, error.to_string());
        }
        if !state.enabled
            || state.stamp.clock_id != self.record.clock_id
            || state.stamp.at_us > now_us
            || now_us - state.stamp.at_us >= STATE_FRESH_US
        {
            return self.reject(
                now_us,
                "disabled, foreign-clock, future or stale controls state",
            );
        }
        if self.state_at_us.is_some_and(|at| state.stamp.at_us < at) {
            return self.reject(now_us, "controls state observation time moved backwards");
        }
        if self.session.as_deref() != Some(&state.session_id) {
            if self.retired.contains(&state.session_id) || self.retired.len() == MAX_RETIRED {
                if self.retired.len() == MAX_RETIRED {
                    self.exhausted = true;
                }
                return self.reject(now_us, "retired controls session or full session fence");
            }
            if let Some(old) = self.session.replace(state.session_id.clone()) {
                self.retired.insert(old);
            }
            self.state_sequence = None;
            self.event_sequence = 0;
            self.pet_sequence = 0;
            self.fence_clear(now_us)?;
        }
        if !state.ready
            || self
                .state_at_us
                .is_some_and(|at| now_us - at >= STATE_FRESH_US)
        {
            self.fence_clear(now_us)?;
        }
        let mut effects = Effects {
            software_muted: self.record.muted,
            ..Effects::default()
        };
        if let Some(last) = self.state_sequence {
            if state.stamp.sequence < last {
                return self.reject(now_us, "controls state sequence moved backwards");
            }
            if state.stamp.sequence == last {
                return Ok(effects);
            }
        }
        if state.event_sequence < self.event_sequence {
            return self.reject(now_us, "controls cumulative event sequence moved backwards");
        }
        self.state_sequence = Some(state.stamp.sequence);
        self.state_at_us = Some(state.stamp.at_us);
        if let Some(mute) = &state.latest_mute {
            let clear = self.record.muted
                && state.ready
                && mute.stamp.sequence > self.event_sequence
                && mute.fresh_at(&self.record.clock_id, now_us)
                && now_us - mute.released_us < CONTROL_EVENT_TTL_US
                && self.clear_after_us.is_some_and(|fence| {
                    mute.released_us - mute.duration_us > fence.max(self.record.through_us)
                });
            if self.record.request_mute(mute.released_us) {
                self.fence_clear(now_us)?;
                effects.mute_changed = true;
                effects.stop = true;
                if clear {
                    self.pending_clear_release_us = Some(mute.released_us);
                    effects.clear_requested = true;
                }
            }
        }
        if let Some(stop) = &state.latest_stop
            && stop.stamp.sequence > self.event_sequence
            && stop.fresh_at(&self.record.clock_id, now_us)
        {
            effects.stop = true;
            effects.stop_requested = true;
            // A later panic tap must not be followed by a pending clear. An
            // older retained Stop in the same snapshot does not cancel a later
            // qualifying hold that has already superseded it.
            if state
                .latest_mute
                .as_ref()
                .is_none_or(|mute| stop.stamp.sequence > mute.stamp.sequence)
            {
                self.fence_clear(now_us)?;
                effects.clear_requested = false;
            }
        }
        // State is cumulative. Advancing across a missing event prevents a
        // late duplicate from causing another action.
        self.event_sequence = self.event_sequence.max(state.event_sequence);
        effects.software_muted = self.record.muted;
        Ok(effects)
    }

    /// Pet is advisory only and requires a previously accepted owner state.
    /// Stop/mute are consumed through observe(), so event loss is recoverable.
    pub fn pet(&mut self, now_us: u64, event: &ControlGesture) -> io::Result<bool> {
        self.clock(now_us)?;
        if event.intent != ControlIntent::Pet
            || !event.fresh_at(&self.record.clock_id, now_us)
            || self.session.as_deref() != Some(&event.session_id)
            || self
                .state_at_us
                .is_none_or(|at| now_us - at >= STATE_FRESH_US)
        {
            return Err(invalid(
                "pet is not a fresh event from the accepted controls owner",
            ));
        }
        // Event and state have independent delivery. A pet included only in
        // state's event_sequence is still not evidence that it was delivered.
        // Use a separate event consumer for optional pet reactions; state
        // intentionally contains no latched pet that could replay on startup.
        if event.stamp.sequence <= self.pet_sequence {
            return Ok(false);
        }
        self.pet_sequence = event.stamp.sequence;
        Ok(true)
    }
}
