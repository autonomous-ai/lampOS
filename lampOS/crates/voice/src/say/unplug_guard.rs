//! Request-local unplug proof. Bus I/O stays on the existing bounded say client
//! worker. The coordinator only reads its atomic deadline. Once evidence fails,
//! this request can never regain authority, even if a later ready packet arrives.
use lamp_body::unplug::{Phase, State};
use lamp_bus::Bus;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const BODY_FRESH_US: u64 = 500_000;
// A stalled reader must not keep speech authorized just because its last body
// packet has not expired. Normal monitoring is every 20 ms.
const MONITOR_FRESH_US: u64 = 100_000;

#[derive(Clone)]
pub(super) struct Guard(Arc<AtomicU64>);
impl Guard {
    #[cfg(test)]
    pub(super) fn for_test(deadline: u64) -> (Self, Arc<AtomicU64>) {
        let shared = Arc::new(AtomicU64::new(deadline));
        (Self(Arc::clone(&shared)), shared)
    }

    pub(super) fn valid(&self, now_us: u64) -> bool {
        now_us < self.0.load(Ordering::Acquire)
    }
}

pub(super) struct Monitor {
    bus: Bus,
    request: String,
    guard: Guard,
    failed: bool,
}
impl Monitor {
    pub(super) fn for_request(request: Option<&str>) -> Result<Option<Self>, &'static str> {
        let Some(request) = request else {
            return Ok(None);
        };
        if !lamp_body::unplug::enabled() {
            return Err("unplug preparation is disabled");
        }
        let bus = Bus::from_environment().map_err(|_| "unplug bus unavailable")?;
        let mut monitor = Self {
            bus,
            request: request.to_owned(),
            guard: Guard(Arc::new(AtomicU64::new(0))),
            failed: false,
        };
        monitor.read();
        if monitor.failed {
            return Err("unplug_authority_lost");
        }
        Ok(Some(monitor))
    }

    pub(super) fn guard(&self) -> Guard {
        self.guard.clone()
    }

    pub(super) fn refresh(&mut self) {
        if self.failed {
            return;
        }
        if !self.guard.valid(lamp_ipc::monotonic_us()) {
            self.invalidate();
            return;
        }
        self.read();
    }

    fn read(&mut self) {
        let previous_deadline = self.guard.0.load(Ordering::Acquire);
        let deadline = self
            .bus
            .latest(State::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| {
                let state = message.parse::<State>().ok()?;
                let now_us = lamp_ipc::monotonic_us();
                if message.expired
                    || message.header.source != "body"
                    || message.header.type_name.as_deref() != Some(State::TYPE)
                {
                    return None;
                }
                deadline(&self.request, &state, message.header.at_us, now_us)
            });
        if previous_deadline != 0 && lamp_ipc::monotonic_us() >= previous_deadline {
            self.invalidate();
            return;
        }
        match deadline {
            Some(deadline) => self.guard.0.store(deadline, Ordering::Release),
            None => self.invalidate(),
        }
    }

    fn invalidate(&mut self) {
        self.failed = true;
        self.guard.0.store(0, Ordering::Release);
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.invalidate();
    }
}

fn deadline(request: &str, state: &State, at_us: u64, now_us: u64) -> Option<u64> {
    if state.request != request
        || state.phase != Phase::Ready
        || at_us == 0
        || at_us > now_us
        || now_us - at_us >= BODY_FRESH_US
    {
        return None;
    }
    Some(
        at_us
            .saturating_add(BODY_FRESH_US)
            .min(now_us.saturating_add(MONITOR_FRESH_US)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_matches_request_ready_phase_and_both_freshness_bounds() {
        let mut state = State {
            request: "one".into(),
            phase: Phase::Ready,
            why: String::new(),
        };
        assert_eq!(
            deadline("one", &state, 1_000_000, 1_000_000),
            Some(1_100_000)
        );
        assert_eq!(
            deadline("one", &state, 1_000_000, 1_450_000),
            Some(1_500_000)
        );
        assert_eq!(deadline("one", &state, 1_000_000, 1_500_000), None);
        assert_eq!(deadline("two", &state, 1_000_000, 1_000_000), None);
        assert_eq!(deadline("one", &state, 1_000_001, 1_000_000), None);
        for phase in [Phase::Folding, Phase::Failed] {
            state.phase = phase;
            assert_eq!(deadline("one", &state, 1_000_000, 1_000_000), None);
        }
    }

    #[test]
    fn failed_monitor_never_recovers_and_drop_revokes_a_retained_ticket() {
        let make = || Monitor {
            bus: Bus::new("/tmp/unused-say-guard-bus").unwrap(),
            request: "one".into(),
            guard: Guard(Arc::new(AtomicU64::new(u64::MAX))),
            failed: false,
        };
        let mut monitor = make();
        let ticket = monitor.guard();
        monitor.invalidate();
        monitor.refresh();
        assert!(monitor.failed);
        assert!(!ticket.valid(1));
        let monitor = make();
        let ticket = monitor.guard();
        drop(monitor);
        assert!(!ticket.valid(1));
    }

    /// A monitor on a bus of its own for `one`, after one read of the
    /// body's state as `source` published it, `age_us` ago.
    fn monitor_after(
        name: &str,
        source: &str,
        state: State,
        age_us: u64,
    ) -> (Monitor, lamp_bus::Publisher) {
        let root = std::env::temp_dir().join(format!(
            "say-guard-{name}-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let mut publisher = bus
            .publisher_declared(State::TOPIC, &State::declaration(), source)
            .unwrap();
        let at_us = lamp_ipc::monotonic_us().saturating_sub(age_us);
        publisher.publish_json(at_us, &state).unwrap();
        let mut monitor = Monitor {
            bus,
            request: "one".into(),
            guard: Guard(Arc::new(AtomicU64::new(0))),
            failed: false,
        };
        monitor.read();
        (monitor, publisher)
    }

    fn ready(request: &str) -> State {
        State {
            request: request.into(),
            phase: Phase::Ready,
            why: String::new(),
        }
    }

    /// The reviewer on #1699: each freshness rule fails through the real
    /// `Monitor` reading a real bus, not only the raw deadline: the body's
    /// state 500 ms old or more, from another source, for another request,
    /// or not ready, and a monitor not refreshed within 100 ms.
    #[test]
    fn the_monitor_refuses_a_stale_foreign_other_or_unrefreshed_proof() {
        let (fresh, _publisher) = monitor_after("fresh", "body", ready("one"), 0);
        assert!(!fresh.failed);
        assert!(fresh.guard().valid(lamp_ipc::monotonic_us()));
        for (name, source, state, age_us) in [
            ("stale", "body", ready("one"), BODY_FRESH_US),
            ("foreign", "expression", ready("one"), 0),
            ("other", "body", ready("two"), 0),
            (
                "folding",
                "body",
                State {
                    phase: Phase::Folding,
                    ..ready("one")
                },
                0,
            ),
        ] {
            let (monitor, _publisher) = monitor_after(name, source, state, age_us);
            assert!(monitor.failed, "{name}");
            assert!(!monitor.guard().valid(lamp_ipc::monotonic_us()), "{name}");
            drop(monitor);
        }
        // A monitor not refreshed within 100 ms lapses on its own, and a
        // fresh, ready proof read afterwards does not bring it back.
        let (mut late, mut publisher) = monitor_after("late", "body", ready("one"), 0);
        let ticket = late.guard();
        std::thread::sleep(std::time::Duration::from_micros(MONITOR_FRESH_US + 20_000));
        assert!(!ticket.valid(lamp_ipc::monotonic_us()));
        publisher
            .publish_json(lamp_ipc::monotonic_us(), &ready("one"))
            .unwrap();
        late.refresh();
        assert!(late.failed);
        assert!(!ticket.valid(lamp_ipc::monotonic_us()));
    }

    #[test]
    fn guard_expires_without_another_worker_read_and_revokes_all_clones() {
        let guard = Guard(Arc::new(AtomicU64::new(100)));
        let ticket = guard.clone();
        assert!(ticket.valid(99));
        assert!(!ticket.valid(100));
        guard.0.store(0, Ordering::Release);
        assert!(!ticket.valid(1));
    }
}
