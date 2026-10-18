//! Optional owner-side validation, not an output executor or privacy permit.
//!
//! Subscribe to both new topics and pass qualified state before events. The
//! runtime publishes state first; topic delivery may still reorder, in which
//! case an event without its qualifying state is refused. Event loss for
//! retained requests is recovered by the next state, without renewing age.
//!
//! Lifecycle/reset need a whole new contact after receiver/source initialization
//! or a freshness fault. Sleep and Wake are one-way requests, never toggles.
//! Owners must additionally persist
//! exact (boot, Controls session, source reader, event sequence) deduplication
//! before acting, across their own process restarts. This in-memory helper is
//! not proof of persistence or exactly-once applied output. Validate the bus
//! envelope/type/expiry separately: event and state publishers have different
//! bus boot IDs; their shared payload session is the Controls owner identity.
use lamp_msgs::{
    ClockStamp, Invalid, TOUCH_INPUT_TTL_US, TOUCH_STATE_TTL_US, TouchEvent, TouchGesture,
    TouchIntent, TouchSource, TouchSourceState, TouchState,
};
use std::collections::{BTreeMap, BTreeSet};

const MAX_RETIRED: usize = 32;
#[derive(Clone)]
struct SourceFence {
    state: TouchSourceState,
    retired: BTreeSet<String>,
    through_sequence: u64,
    sensitive_after_us: u64,
}

pub struct TouchReceiver {
    clock_id: String,
    session: Option<String>,
    retired: BTreeSet<String>,
    sources: BTreeMap<TouchSource, SourceFence>,
    state: Option<TouchState>,
    state_valid: bool,
    last_now: Option<u64>,
    exhausted: bool,
    /// [`Self::allow_late_sleep`]: once, on the first state taken, a Sleep
    /// hold that began after this is taken however old it is.
    late_sleep_after_us: Option<u64>,
}

fn invalid(message: &'static str) -> Invalid {
    Invalid(message.into())
}
fn sensitive(event: &TouchEvent) -> bool {
    event.when_asleep == Some(TouchIntent::Wake)
        || matches!(
            event.when_speaking,
            TouchIntent::Sleep | TouchIntent::FactoryReset
        )
}

impl TouchReceiver {
    pub fn new(clock_id: impl Into<String>) -> Result<Self, Invalid> {
        let clock_id = clock_id.into();
        ClockStamp {
            clock_id: clock_id.clone(),
            at_us: 0,
            sequence: 0,
        }
        .check()?;
        Ok(Self {
            clock_id,
            session: None,
            retired: BTreeSet::new(),
            sources: BTreeMap::new(),
            state: None,
            state_valid: false,
            last_now: None,
            exhausted: false,
            late_sleep_after_us: None,
        })
    }

    /// For a new owner, before its first state (#1607): a hand held on the
    /// head to put the lamp to sleep while no owner ran (a restart's 4 s) is
    /// in the state's retained `latest`, but older than any event this
    /// receiver otherwise takes, and begun before it saw its source. On the
    /// first state it takes, such a Sleep hold that began after `after_us`
    /// (the owner's record watermark and its last save) is accepted once all
    /// the same. Only Sleep: a late Wake, tap or reset never is, so this can
    /// only close the microphones, never open them.
    pub fn allow_late_sleep(&mut self, after_us: u64) {
        self.late_sleep_after_us = Some(after_us);
    }

    /// A retained head hold asking for Sleep, from this Controls session and
    /// source reader, that began after `after_us` and was seen by `now`.
    fn late_sleep(&self, now: u64, event: &TouchEvent, after_us: u64) -> bool {
        event.check().is_ok()
            && event.source == TouchSource::Head
            && event.gesture == TouchGesture::Hold
            && event.when_speaking == TouchIntent::Sleep
            && event.when_asleep != Some(TouchIntent::Wake)
            && event.began_us > after_us
            && event.observed_us <= now
            && event.stamp.clock_id == self.clock_id
            && self.session.as_deref() == Some(&event.session_id)
            && self.sources.get(&event.source).is_some_and(|source| {
                source.state.reader_session_id.as_deref() == Some(&event.reader_session_id)
                    && source.state.name == event.name
                    && event.stamp.sequence > source.through_sequence
            })
    }
    fn clock(&mut self, now: u64) -> Result<(), Invalid> {
        if self.exhausted || self.last_now.is_some_and(|last| now < last) {
            self.exhausted = true;
            return Err(invalid(
                "touch receiver clock/session fence failed; restart required",
            ));
        }
        self.last_now = Some(now);
        Ok(())
    }
    fn inhibit(&mut self, now: u64) {
        for source in self.sources.values_mut() {
            source.sensitive_after_us = now.max(source.sensitive_after_us);
        }
        self.state_valid = false;
    }

    /// Returns deduplicated requests, in event order. Every result is still
    /// conditional on the final owner's live speech/privacy/output policy.
    pub fn observe(&mut self, now_us: u64, state: &TouchState) -> Result<Vec<TouchEvent>, Invalid> {
        self.clock(now_us)?;
        let result = self.observe_checked(now_us, state);
        if result.is_err() {
            self.inhibit(now_us);
        }
        result
    }
    fn observe_checked(
        &mut self,
        now: u64,
        state: &TouchState,
    ) -> Result<Vec<TouchEvent>, Invalid> {
        state.check()?;
        if !state.enabled
            || state.stamp.clock_id != self.clock_id
            || state.stamp.at_us > now
            || now - state.stamp.at_us >= TOUCH_STATE_TTL_US
        {
            return Err(invalid(
                "touch state is disabled, foreign-clock, future or stale",
            ));
        }
        let changed = self.session.as_deref() != Some(&state.session_id);
        if let Some(previous) = &self.state {
            if state.stamp.at_us < previous.stamp.at_us
                || (changed && state.stamp.at_us == previous.stamp.at_us)
            {
                return Err(invalid("touch state observation time moved backwards"));
            }
            if !changed {
                if state.stamp.sequence < previous.stamp.sequence
                    || state.event_sequence < previous.event_sequence
                {
                    return Err(invalid("touch state identity moved backwards"));
                }
                if state.stamp.sequence == previous.stamp.sequence {
                    return if state == previous && self.state_valid {
                        Ok(Vec::new())
                    } else {
                        Err(invalid(
                            "touch state identity was reused or no longer qualifies",
                        ))
                    };
                }
            }
        }
        if changed {
            if self.retired.contains(&state.session_id) {
                return Err(invalid("retired Controls session cannot return"));
            }
            if self.session.is_some() && self.retired.len() == MAX_RETIRED {
                self.exhausted = true;
                return Err(invalid("Controls session fence is full"));
            }
            if let Some(previous) = self.session.replace(state.session_id.clone()) {
                self.retired.insert(previous);
            }
            self.sources.clear();
        } else if self.sources.len() != state.sources.len()
            || state.sources.iter().any(|source| {
                self.sources
                    .get(&source.source)
                    .is_none_or(|old| old.state.name != source.name)
            })
        {
            return Err(invalid(
                "touch source configuration changed within Controls session",
            ));
        }
        // Validate source transitions on a bounded copy before installing it.
        let state_gap = self
            .state
            .as_ref()
            .is_some_and(|old| now - old.stamp.at_us >= TOUCH_STATE_TTL_US);
        let mut sources = self.sources.clone();
        for source in &state.sources {
            let Some(previous) = sources.get_mut(&source.source) else {
                sources.insert(
                    source.source,
                    SourceFence {
                        state: source.clone(),
                        retired: BTreeSet::new(),
                        through_sequence: 0,
                        sensitive_after_us: now,
                    },
                );
                continue;
            };
            if source.observed_us < previous.state.observed_us {
                return Err(invalid("touch source acquisition time moved backwards"));
            }
            if source.reader_session_id != previous.state.reader_session_id {
                let Some(reader) = &source.reader_session_id else {
                    return Err(invalid("touch reader identity disappeared"));
                };
                if previous.retired.contains(reader) {
                    return Err(invalid("retired touch source cannot return"));
                }
                if previous.state.reader_session_id.is_some() {
                    if previous.retired.len() == MAX_RETIRED {
                        self.exhausted = true;
                        return Err(invalid("touch source session fence is full"));
                    }
                    if source.observed_us <= previous.state.observed_us {
                        return Err(invalid("new touch source predates its predecessor"));
                    }
                    previous.retired.insert(
                        previous
                            .state
                            .reader_session_id
                            .clone()
                            .expect("present reader"),
                    );
                }
                previous.sensitive_after_us = now;
            }
            if !source.ready
                || !previous.state.ready
                || state_gap
                || source
                    .observed_us
                    .is_none_or(|at| now - at >= TOUCH_INPUT_TTL_US)
            {
                previous.sensitive_after_us = now;
            }
            previous.state = source.clone();
        }
        self.sources = sources;
        self.state = Some(state.clone());
        self.state_valid = true;
        let mut latest = state.latest.iter().collect::<Vec<_>>();
        latest.sort_by_key(|event| event.stamp.sequence);
        let late = self.late_sleep_after_us.take();
        let mut accepted = Vec::with_capacity(latest.len());
        for event in latest {
            if let Some(after) = late
                && event.stamp.sequence <= state.event_sequence
                && self.late_sleep(now, event, after)
            {
                if let Some(source) = self.sources.get_mut(&event.source) {
                    source.through_sequence = event.stamp.sequence;
                }
                accepted.push(event.clone());
                continue;
            }
            if let Some(event) = self.accept_event(now, event, false)? {
                accepted.push(event);
            }
        }
        Ok(accepted)
    }

    pub fn event(
        &mut self,
        now_us: u64,
        event: &TouchEvent,
    ) -> Result<Option<TouchEvent>, Invalid> {
        self.clock(now_us)?;
        self.accept_event(now_us, event, true)
    }
    fn accept_event(
        &mut self,
        now: u64,
        event: &TouchEvent,
        direct: bool,
    ) -> Result<Option<TouchEvent>, Invalid> {
        event.check()?;
        if !self.state_valid {
            return Err(invalid("touch event has no accepted current state"));
        }
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| invalid("touch event has no accepted current state"))?;
        if self.session.as_deref() != Some(&event.session_id)
            || event.stamp.clock_id != self.clock_id
        {
            return Err(invalid("touch event belongs to another owner or clock"));
        }
        let source = self
            .sources
            .get_mut(&event.source)
            .ok_or_else(|| invalid("touch event source is not configured"))?;
        if source.state.reader_session_id.as_deref() != Some(&event.reader_session_id)
            || source.state.name != event.name
        {
            return Err(invalid("touch event belongs to another source reader"));
        }
        let fresh = event.fresh_at(&self.clock_id, now)
            && source.state.ready
            && now - state.stamp.at_us < TOUCH_STATE_TTL_US
            && source
                .state
                .observed_us
                .is_some_and(|at| now - at < TOUCH_INPUT_TTL_US && at >= event.observed_us)
            && event.stamp.sequence <= state.event_sequence;
        if !fresh {
            return if direct {
                Err(invalid("touch event lacks fresh source/state evidence"))
            } else {
                Ok(None)
            };
        }
        // Fence actual delivered records, not the state's cumulative counter.
        // A delayed older Hold must not follow a newer Touch or Slide from the
        // same source merely because the intents use different retained slots.
        if event.stamp.sequence <= source.through_sequence {
            return Ok(None);
        }
        source.through_sequence = event.stamp.sequence;
        if sensitive(event) && event.began_us <= source.sensitive_after_us {
            return Ok(None);
        }
        Ok(Some(event.clone()))
    }
}
