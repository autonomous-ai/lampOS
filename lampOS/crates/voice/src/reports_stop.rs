//! "Stop sending reports" (#1683, `docs/cloud.md`, "Health and crash reports"): a system function the
//! model calls with no arguments when the person asks the lamp to stop
//! sending health and crash reports to the company.
//!
//! The voice asks `lamp-cloud`, which sends them, on the bus
//! (`reports/request`, `{"share": false}`) and answers from what
//! `lamp-cloud` then says on `reports/state`, within [`ANSWER_WITHIN`].
//! The voice can only turn reports off: there is no function to turn them
//! on, which only the person's own screen does. Declared only with
//! `LAMP_REPORTS=on` in the voice's unit; never a skill.
use lamp_gemini::Tool;
use lamp_msgs::{ReportsRequest, ReportsState};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// `on` for the function to be declared. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_REPORTS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--reports-stop";
/// The function the model calls.
pub const NAME: &str = "reports_stop";
/// How long `lamp-cloud` has to say they stopped.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(2);
/// What the person hears back when they stopped.
pub const STOPPED: &str = "I've stopped sending reports.";
/// When they were not being sent.
pub const NOT_SENDING: &str = "I'm not sending any reports.";
/// The voice's name on the bus for its ask.
const SOURCE: &str = "voice";

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

pub fn is_tool(name: &str) -> bool {
    name == NAME
}

pub fn tool() -> Tool {
    Tool {
        name: NAME.into(),
        description: "Call this only when the person asks you to stop sending reports, health \
                      reports or crash reports to Autonomous. It stops them and deletes what \
                      was waiting to be sent. Then say what it gives you as \"say\", in a few \
                      words. You cannot start reports; if asked to, say it is done in the app."
            .into(),
        parameters: None,
        silent: false,
    }
}

/// What the bus gives this function: an ask sent, and the state read.
pub trait Reports {
    /// Puts `{"share": false}` on `reports/request`: how many took it.
    fn ask(&mut self) -> Option<usize>;
    /// `reports/state` now, unless it is past its lifespan.
    fn state(&mut self) -> Option<ReportsState>;
}

/// The lamp's bus.
pub struct Bus(pub Option<lamp_bus::Bus>);

/// A bus of a test's own, in place of the lamp's, for the worker's tests
/// that run the call through its loop.
#[cfg(test)]
pub(crate) static TEST_ROOT: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

impl Bus {
    /// The lamp's bus (`LAMP_BUS_ROOT`, or its default), the call's own.
    pub fn of_the_lamp() -> Self {
        #[cfg(test)]
        if let Some(root) = TEST_ROOT.lock().ok().and_then(|root| root.clone()) {
            return Self(lamp_bus::Bus::new(root).ok());
        }
        Self(lamp_bus::Bus::from_environment().ok())
    }
}

impl Reports for Bus {
    fn ask(&mut self) -> Option<usize> {
        let bus = self.0.as_ref()?;
        let mut out = bus
            .publisher_declared(
                ReportsRequest::TOPIC,
                &ReportsRequest::declaration(),
                SOURCE,
            )
            .ok()?;
        out.publish_json(lamp_ipc::monotonic_us(), &ReportsRequest { share: false })
            .ok()
            .map(|published| published.delivered)
    }

    fn state(&mut self) -> Option<ReportsState> {
        let message = self.0.as_ref()?.latest(ReportsState::TOPIC).ok()??;
        (!message.expired)
            .then(|| message.parse::<ReportsState>().ok())
            .flatten()
    }
}

/// One call: ask, then wait for `lamp-cloud` to say they are off. Only for
/// words a person said: a request the lamp wrote itself stops nothing.
pub fn run(reports: &mut impl Reports, heard: bool, within: Duration) -> (Value, bool) {
    if !heard {
        return (
            json!({"error": "Only a person can stop the reports."}),
            false,
        );
    }
    let before = reports.state();
    // Nobody to take it: `lamp-cloud` is not running, so nothing is being
    // sent now, but nothing could be turned off either. Said truthfully.
    if reports.ask().unwrap_or(0) == 0 {
        return (
            json!({"error": "The part of me that sends reports is not running, so nothing is being sent now, but I could not turn them off. They can be turned off in the app."}),
            false,
        );
    }
    let asked = Instant::now();
    loop {
        if let Some(state) = reports.state()
            && !state.share
        {
            let say = if before.is_some_and(|before| before.share) {
                STOPPED
            } else {
                NOT_SENDING
            };
            return (json!({"ok": true, "say": say}), true);
        }
        if asked.elapsed() >= within {
            return (
                json!({"error": "I asked to stop the reports, but I could not tell whether they stopped. They can be turned off in the app."}),
                false,
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        takers: usize,
        states: Vec<Option<ReportsState>>,
        asked: usize,
    }

    impl Reports for Fake {
        fn ask(&mut self) -> Option<usize> {
            self.asked += 1;
            Some(self.takers)
        }
        fn state(&mut self) -> Option<ReportsState> {
            if self.states.len() > 1 {
                self.states.remove(0)
            } else {
                self.states.first().copied().flatten()
            }
        }
    }

    fn state(share: bool) -> Option<ReportsState> {
        Some(ReportsState { share, queued: 0 })
    }

    #[test]
    fn the_switch_alone_declares_it() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for value in [None, Some("off"), Some("yes"), Some("")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        assert!(tool().validate().is_ok());
        assert!(tool().parameters.is_none(), "no way to say on");
    }

    #[test]
    fn a_stop_is_asked_and_answered_from_what_lamp_cloud_says() {
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true), state(true), state(false)],
            asked: 0,
        };
        let (told, answered) = run(&mut fake, true, ANSWER_WITHIN);
        assert!(answered);
        assert_eq!(told, json!({"ok": true, "say": STOPPED}));
        assert_eq!(fake.asked, 1);
        // Already off.
        let mut fake = Fake {
            takers: 1,
            states: vec![state(false)],
            asked: 0,
        };
        assert_eq!(
            run(&mut fake, true, ANSWER_WITHIN).0,
            json!({"ok": true, "say": NOT_SENDING})
        );
    }

    #[test]
    fn without_lamp_cloud_or_its_answer_it_says_so_and_never_claims_a_stop() {
        let mut fake = Fake {
            takers: 0,
            states: vec![state(true)],
            asked: 0,
        };
        let (told, answered) = run(&mut fake, true, ANSWER_WITHIN);
        assert!(!answered);
        assert!(told["error"].as_str().unwrap().contains("not running"));
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true)],
            asked: 0,
        };
        let (told, answered) = run(&mut fake, true, Duration::from_millis(60));
        assert!(!answered);
        assert!(told.get("ok").is_none());
        // A request the lamp wrote itself stops nothing.
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true)],
            asked: 0,
        };
        assert!(!run(&mut fake, false, ANSWER_WITHIN).1);
        assert_eq!(fake.asked, 0);
    }

    /// Through a real bus: the ask reaches a subscriber as `share: false`.
    #[test]
    fn the_ask_on_the_bus_is_only_ever_a_no() {
        let root = std::env::temp_dir().join(format!("lvrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut taking = bus
            .subscribe_typed(ReportsRequest::TOPIC, "cloud", ReportsRequest::TYPE)
            .unwrap();
        let mut real = Bus(Some(bus.clone()));
        assert_eq!(real.state(), None);
        assert_eq!(real.ask(), Some(1));
        let message = taking.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(
            message.parse::<ReportsRequest>().unwrap(),
            ReportsRequest { share: false }
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
