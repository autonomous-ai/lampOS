//! Physical privacy opens only after a stable permitted observation; closing is
//! immediate. Old, missing or failed observations can never keep input open.
use lamp_turn_owner::MonoTime;

pub const PRIVACY_FRESH_US: u64 = 50_000;
pub const REOPEN_STABLE_US: u64 = 60_000;
/// How long after its conversation loop starts (once its workers are
/// started) a cycle that has not yet allowed the microphone waits past an
/// observation read stale, or any freshness lost, keeping the gate closed,
/// before a gate still without a fresh one ends it: the observation waited
/// in the queue while the voice started its other workers (lamp-4ace, 19 Oct
/// 01:19). A worker that sends one fresh observation and then goes silent
/// therefore ends the cycle at this, not at its 50 ms (the reviewer on
/// #1666). The same second as the service
/// wait's own (`service_privacy`'s `UNAVAILABLE_WAIT`) and the first control
/// message's grace (`transport::FIRST_CONTROL_MAX_AGE_US`).
pub const PRIVACY_START_STALE_US: u64 = 1_000_000;

/// A closed gate is not necessarily a deliberate use of the mute switch.
/// Only a current, ordered physical observation can establish `Muted`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivacyState {
    Unavailable,
    Muted,
    Stabilizing,
    Allowed,
}

#[derive(Default, Debug)]
pub struct PrivacyGate {
    last: Option<(u64, MonoTime)>,
    allowed_since: Option<MonoTime>,
    last_muted: Option<bool>,
    #[cfg(test)]
    fixture_freshness_us: Option<u64>,
}

impl PrivacyGate {
    /// Bounded host scheduling tolerance for an explicitly selected integration
    /// fixture, never a runtime setting. The maximum is the existing worker IPC
    /// freshness ceiling; source timestamps and every other gate stay intact.
    #[cfg(test)]
    pub(crate) fn with_fixture_freshness(freshness_us: u64) -> std::io::Result<Self> {
        if !(1..=100_000).contains(&freshness_us) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixture privacy freshness must be in 1..=100000 microseconds",
            ));
        }
        Ok(Self {
            fixture_freshness_us: Some(freshness_us),
            ..Self::default()
        })
    }

    #[inline]
    pub(crate) fn freshness_us(&self) -> u64 {
        #[cfg(test)]
        if let Some(freshness_us) = self.fixture_freshness_us {
            return freshness_us;
        }
        PRIVACY_FRESH_US
    }

    /// Source IDs are supplied by the supervisor; reset this gate when its
    /// privacy worker restarts. `muted` is already normalized for active-low.
    pub fn observe(
        &mut self,
        sequence: u64,
        acquired: MonoTime,
        now: MonoTime,
        muted: bool,
    ) -> bool {
        let freshness_us = self.freshness_us();
        let fresh = now
            .as_micros()
            .checked_sub(acquired.as_micros())
            .is_some_and(|age| age < freshness_us);
        let ordered = sequence > 0
            && self
                .last
                .is_none_or(|(old_seq, old_time)| sequence > old_seq && acquired >= old_time);
        let continuous = self.last.is_none_or(|(_, old_time)| {
            acquired.as_micros().saturating_sub(old_time.as_micros()) < freshness_us
        });
        if !fresh || !ordered {
            self.close();
            return false;
        }
        if !continuous {
            self.allowed_since = None;
        }
        self.last = Some((sequence, acquired));
        self.last_muted = Some(muted);
        if muted {
            self.allowed_since = None;
        } else if self.allowed_since.is_none() {
            self.allowed_since = Some(acquired);
        }
        self.allowed(now)
    }

    pub fn allowed(&self, now: MonoTime) -> bool {
        self.state(now) == PrivacyState::Allowed
    }

    pub fn state(&self, now: MonoTime) -> PrivacyState {
        let fresh = self.last.is_some_and(|(_, at)| {
            now.as_micros()
                .checked_sub(at.as_micros())
                .is_some_and(|age| age < self.freshness_us())
        });
        if !fresh || self.last_muted.is_none() {
            return PrivacyState::Unavailable;
        }
        if self.last_muted == Some(true) {
            return PrivacyState::Muted;
        }
        if self.allowed_since.is_some_and(|since| {
            now.as_micros()
                .checked_sub(since.as_micros())
                .is_some_and(|age| age >= REOPEN_STABLE_US)
        }) {
            PrivacyState::Allowed
        } else {
            PrivacyState::Stabilizing
        }
    }

    pub fn close(&mut self) {
        // Keep sequence high-water: a reordered packet must not reopen privacy.
        self.allowed_since = None;
        self.last_muted = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(us: u64) -> MonoTime {
        MonoTime::from_micros(us)
    }

    fn gate(bound: u64) -> PrivacyGate {
        match bound {
            PRIVACY_FRESH_US => PrivacyGate::default(),
            _ => PrivacyGate::with_fixture_freshness(bound).unwrap(),
        }
    }

    fn opened(bound: u64) -> PrivacyGate {
        let mut gate = gate(bound);
        for (index, at) in (10_000..=70_000).step_by(10_000).enumerate() {
            gate.observe(index as u64 + 1, t(at), t(at), false);
        }
        assert_eq!(gate.state(t(70_000)), PrivacyState::Allowed);
        gate
    }

    #[test]
    fn fixture_freshness_requires_a_positive_value_within_the_ipc_ceiling() {
        for invalid in [0, 100_001, u64::MAX] {
            assert_eq!(
                PrivacyGate::with_fixture_freshness(invalid)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::InvalidInput
            );
        }
        for valid in [1, PRIVACY_FRESH_US, 100_000] {
            assert_eq!(
                PrivacyGate::with_fixture_freshness(valid)
                    .unwrap()
                    .freshness_us(),
                valid
            );
        }
        assert_eq!(PRIVACY_FRESH_US, 50_000);
        assert_eq!(REOPEN_STABLE_US, 60_000);
    }

    #[test]
    fn ordinary_and_fixture_observation_age_and_state_expire_at_their_exact_bounds() {
        for bound in [50_000, 100_000] {
            let mut fresh = gate(bound);
            fresh.observe(1, t(10_000), t(10_000 + bound - 1), true);
            assert_eq!(fresh.state(t(10_000 + bound - 1)), PrivacyState::Muted);
            assert_eq!(fresh.state(t(10_000 + bound)), PrivacyState::Unavailable);

            let mut expired = gate(bound);
            expired.observe(1, t(10_000), t(10_000 + bound), true);
            assert_eq!(expired.state(t(10_000 + bound)), PrivacyState::Unavailable);
        }
    }

    #[test]
    fn ordinary_and_fixture_acquisition_continuity_restart_at_their_exact_bounds() {
        for bound in [50_000, 100_000] {
            let mut continuous = opened(bound);
            let at = 70_000 + bound - 1;
            assert!(continuous.observe(8, t(at), t(at), false));
            assert_eq!(continuous.state(t(at)), PrivacyState::Allowed);

            let mut gap = opened(bound);
            let at = 70_000 + bound;
            assert!(!gap.observe(8, t(at), t(at), false));
            assert_eq!(gap.state(t(at)), PrivacyState::Stabilizing);
        }
    }

    #[test]
    fn fixture_tolerance_never_changes_an_existing_or_later_default_gate() {
        let mut ordinary = opened(50_000);
        {
            let mut tolerant = opened(100_000);
            let at = 70_000 + 75_000;
            assert!(!ordinary.observe(8, t(at), t(at), false));
            assert!(tolerant.observe(8, t(at), t(at), false));
            assert_eq!(ordinary.freshness_us(), 50_000);
            assert_eq!(PrivacyGate::default().freshness_us(), 50_000);
        }
        assert_eq!(PrivacyGate::default().freshness_us(), 50_000);
    }

    #[test]
    fn fixture_tolerance_preserves_the_sixty_millisecond_reopen_and_immediate_mute() {
        for bound in [50_000, 100_000] {
            let mut gate = gate(bound);
            for (index, at) in [10_000, 30_000, 50_000, 69_999].into_iter().enumerate() {
                assert!(!gate.observe(index as u64 + 1, t(at), t(at), false));
            }
            assert_eq!(gate.state(t(69_999)), PrivacyState::Stabilizing);
            assert!(gate.observe(5, t(70_000), t(70_000), false));
            assert_eq!(gate.state(t(70_000)), PrivacyState::Allowed);
            assert!(!gate.observe(6, t(70_001), t(70_001), true));
            assert_eq!(gate.state(t(70_001)), PrivacyState::Muted);
        }
    }

    #[test]
    fn fixture_tolerance_still_closes_on_future_stale_or_reordered_observations() {
        for bound in [50_000, 100_000] {
            for (sequence, acquired, now) in [
                (8, 70_001, 70_000),
                (8, 70_001, 70_001 + bound),
                (7, 70_001, 70_001),
                (8, 69_999, 70_001),
            ] {
                let mut gate = opened(bound);
                assert!(!gate.observe(sequence, t(acquired), t(now), true));
                assert_eq!(gate.state(t(now)), PrivacyState::Unavailable);
            }
        }
    }
}
