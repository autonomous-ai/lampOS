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
//!
//! Beside it, [`STATE_NAME`]: whether reports are sent, and when they were
//! stopped and by whom, read from `reports/state` and told to the model
//! only when the person asks about reports (the first builder on #1722,
//! 18:44, on Third's read of #1880): a stop from a line whose answer was
//! held is then found, and never announced unasked.
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
/// The function the model calls when asked about reports.
pub const STATE_NAME: &str = "reports_state";
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

pub fn is_state_tool(name: &str) -> bool {
    name == STATE_NAME
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

/// Whether reports are sent, and how and when they were stopped: only when
/// asked, never a word on its own.
pub fn state_tool() -> Tool {
    Tool {
        name: STATE_NAME.into(),
        description: "Call this only when the person asks about reports: whether you send health \
                      or crash reports to Autonomous, or whether and when they were stopped. Then \
                      say what it gives you as \"say\", in a few words. Never bring reports up \
                      unless you were asked about them."
            .into(),
        parameters: None,
        silent: false,
    }
}

/// What the bus gives this function: an ask sent, and the state read.
pub trait Reports {
    /// Puts `{"share": false}` on `reports/request`, with `held` when the
    /// line's answer is held: how many took it.
    fn ask(&mut self, held: bool) -> Option<usize>;
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
    fn ask(&mut self, held: bool) -> Option<usize> {
        let bus = self.0.as_ref()?;
        let mut out = bus
            .publisher_declared(
                ReportsRequest::TOPIC,
                &ReportsRequest::declaration(),
                SOURCE,
            )
            .ok()?;
        out.publish_json(
            lamp_ipc::monotonic_us(),
            &ReportsRequest { share: false, held },
        )
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
/// `held`: the line's answer is held, and `lamp-cloud` keeps the stop as
/// such.
pub fn run(reports: &mut impl Reports, heard: bool, held: bool, within: Duration) -> (Value, bool) {
    if !heard {
        return (
            json!({"error": "Only a person can stop the reports."}),
            false,
        );
    }
    let before = reports.state();
    // Nobody to take it: `lamp-cloud` is not running, so nothing is being
    // sent now, but nothing could be turned off either. Said truthfully.
    if reports.ask(held).unwrap_or(0) == 0 {
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

/// What the model is told when asked about reports: whether they are sent,
/// and when they are not, who stopped them and when, in the lamp's own time
/// (`offset_s` east of UTC). Turning them back on is the app's: the voice
/// can only turn them off (#1683, #1755).
pub fn told(state: Option<&ReportsState>, now_s: u64, offset_s: i32) -> Value {
    use lamp_msgs::ReportsBy;
    let Some(state) = state else {
        return json!({"error": "The part of me that sends reports is not running, so nothing is being sent now."});
    };
    if state.share {
        return json!({"sending": true, "say": "I'm sending health and crash reports to Autonomous. Ask me to stop them, or turn them off in the app."});
    }
    let when = state
        .at_s
        .map(|at| when_said(at, now_s, offset_s))
        .unwrap_or_default();
    let say = match state.by {
        Some(ReportsBy::Voice) => format!(
            "I'm not sending reports. Someone asked me to stop them{when}. They can be turned back on in the app."
        ),
        Some(ReportsBy::App) => format!(
            "I'm not sending reports. They were turned off in the app{when}, and can be turned back on there."
        ),
        Some(ReportsBy::Setup) | None => {
            "I'm not sending reports. They can be turned on in the app.".to_owned()
        }
    };
    json!({"sending": false, "say": say})
}

/// " at 3:04 pm today", " at 9:30 am yesterday", or " on 12 October at
/// 3:04 pm", in the lamp's own time.
fn when_said(at_s: u64, now_s: u64, offset_s: i32) -> String {
    let local = |s: u64| i64::try_from(s).unwrap_or(i64::MAX) + i64::from(offset_s);
    let (at, now) = (local(at_s), local(now_s));
    let (day, today) = (at.div_euclid(86_400), now.div_euclid(86_400));
    let of_day = at.rem_euclid(86_400);
    let (hour, minute) = (of_day / 3_600, of_day % 3_600 / 60);
    let clock = format!(
        "{}:{minute:02} {}",
        (hour + 11) % 12 + 1,
        if hour < 12 { "am" } else { "pm" }
    );
    match today - day {
        0 => format!(" at {clock} today"),
        1 => format!(" at {clock} yesterday"),
        _ => {
            let (_, month, date) = crate::local_time::civil(day);
            const MONTHS: [&str; 12] = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            format!(" on {date} {} at {clock}", MONTHS[month - 1])
        }
    }
}

/// [`told`] from the lamp's bus, now.
pub fn state_now(reports: &mut impl Reports) -> Value {
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    told(
        reports.state().as_ref(),
        now_s,
        crate::local_time::offset_here().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        takers: usize,
        states: Vec<Option<ReportsState>>,
        asked: usize,
        held: bool,
    }

    impl Reports for Fake {
        fn ask(&mut self, held: bool) -> Option<usize> {
            self.asked += 1;
            self.held = held;
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
        Some(ReportsState {
            share,
            ..Default::default()
        })
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
            ..Default::default()
        };
        let (told, answered) = run(&mut fake, true, false, ANSWER_WITHIN);
        assert!(answered);
        assert_eq!(told, json!({"ok": true, "say": STOPPED}));
        assert_eq!(fake.asked, 1);
        // Already off.
        let mut fake = Fake {
            takers: 1,
            states: vec![state(false)],
            ..Default::default()
        };
        assert_eq!(
            run(&mut fake, true, false, ANSWER_WITHIN).0,
            json!({"ok": true, "say": NOT_SENDING})
        );
    }

    #[test]
    fn without_lamp_cloud_or_its_answer_it_says_so_and_never_claims_a_stop() {
        let mut fake = Fake {
            takers: 0,
            states: vec![state(true)],
            ..Default::default()
        };
        let (told, answered) = run(&mut fake, true, false, ANSWER_WITHIN);
        assert!(!answered);
        assert!(told["error"].as_str().unwrap().contains("not running"));
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true)],
            ..Default::default()
        };
        let (told, answered) = run(&mut fake, true, false, Duration::from_millis(60));
        assert!(!answered);
        assert!(told.get("ok").is_none());
        // A request the lamp wrote itself stops nothing.
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true)],
            ..Default::default()
        };
        assert!(!run(&mut fake, false, false, ANSWER_WITHIN).1);
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
        assert_eq!(real.ask(false), Some(1));
        let message = taking.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(
            message.parse::<ReportsRequest>().unwrap(),
            ReportsRequest {
                share: false,
                held: false
            }
        );
        // A held line's stop says so, for `lamp-cloud` to keep.
        assert_eq!(real.ask(true), Some(1));
        let message = taking.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(
            message.parse::<ReportsRequest>().unwrap(),
            ReportsRequest {
                share: false,
                held: true
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_held_stop_is_asked_as_held() {
        let mut fake = Fake {
            takers: 1,
            states: vec![state(true), state(false)],
            ..Default::default()
        };
        assert!(run(&mut fake, true, true, ANSWER_WITHIN).1);
        assert!(fake.held);
    }

    /// The first builder on #1722, 18:44: asked about reports, the voice
    /// says someone asked it to stop them, and when, and that they are
    /// turned back on in the app (the voice can only turn them off).
    #[test]
    fn asked_about_reports_it_says_who_stopped_them_and_when() {
        use lamp_msgs::ReportsBy;
        // 2026-10-19 08:04 UTC, 15:04 at +07.
        let at = 1_792_397_040;
        let seven = 7 * 3_600;
        let stopped = |by, held, at_s| ReportsState {
            share: false,
            queued: 1,
            by,
            held,
            at_s,
        };
        let say = |state: ReportsState, now| told(Some(&state), now, seven)["say"].clone();
        assert_eq!(
            say(stopped(Some(ReportsBy::Voice), true, Some(at)), at + 600),
            json!(
                "I'm not sending reports. Someone asked me to stop them at 3:04 pm today. They can be turned back on in the app."
            )
        );
        assert_eq!(
            say(
                stopped(Some(ReportsBy::Voice), false, Some(at)),
                at + 86_400
            ),
            json!(
                "I'm not sending reports. Someone asked me to stop them at 3:04 pm yesterday. They can be turned back on in the app."
            )
        );
        assert_eq!(
            say(
                stopped(Some(ReportsBy::App), false, Some(at)),
                at + 5 * 86_400
            ),
            json!(
                "I'm not sending reports. They were turned off in the app on 19 October at 3:04 pm, and can be turned back on there."
            )
        );
        assert_eq!(
            say(stopped(None, false, None), at),
            json!("I'm not sending reports. They can be turned on in the app.")
        );
        assert_eq!(
            told(Some(&stopped(None, false, None)), at, seven)["sending"],
            false
        );
        let on = ReportsState {
            share: true,
            ..Default::default()
        };
        assert_eq!(told(Some(&on), at, seven)["sending"], true);
        assert!(told(None, at, seven)["error"].is_string());
        // Midnight and noon on a twelve-hour clock.
        assert_eq!(
            when_said(1_792_368_000 - seven as u64, 1_792_368_000, seven),
            " at 12:00 am today"
        );
        assert_eq!(
            when_said(1_792_368_000 + 5 * 3_600, 1_792_368_000 + 5 * 3_600, seven),
            " at 12:00 pm today"
        );
    }

    /// Nothing about reports is said unasked: only [`STATE_NAME`]'s answer
    /// carries how they were stopped, and the model is told to call it only
    /// when asked, and to call the stop only when asked to stop.
    #[test]
    fn reports_are_told_only_when_asked() {
        let state = state_tool();
        assert!(state.validate().is_ok());
        assert!(state.parameters.is_none());
        assert!(
            state
                .description
                .starts_with("Call this only when the person asks about reports")
        );
        assert!(
            state
                .description
                .contains("Never bring reports up unless you were asked")
        );
        assert!(
            tool()
                .description
                .starts_with("Call this only when the person asks you to stop")
        );
        assert!(is_state_tool(STATE_NAME) && !is_state_tool(NAME) && !is_tool(STATE_NAME));
    }
}
