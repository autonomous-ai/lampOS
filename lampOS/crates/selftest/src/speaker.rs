//! The speaker and the voice microphone (`docs/speaker-check.md`): the
//! voice is asked on `selftest/speaker/request` and answers on
//! `selftest/speaker` what its microphone made of the lamp's own last
//! speaking turn. Nothing is played. Only at the factory and when asked.
use crate::Check;
use lamp_bus::Bus;
use lamp_msgs::{Occasion, PartResult, SpeakerCheck, SpeakerCheckRequest, Verdict};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The voice answers within this: it answers from what it already has, on
/// its next loop.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(2);
/// How often the answer is looked for.
const LOOK_EVERY: Duration = Duration::from_millis(50);

/// Judges the voice's answer.
pub fn judge(answer: Option<&SpeakerCheck>) -> PartResult {
    let part = "speaker";
    let Some(answer) = answer else {
        return PartResult::new(
            part,
            Verdict::NotChecked,
            "lamp-voice did not answer: does its unit have LAMP_SELFTEST_SPEAKER=on?",
        );
    };
    let verdict = match answer.verdict.as_str() {
        "pass" => Verdict::Pass,
        "fail" => Verdict::Fail,
        _ => Verdict::NotChecked,
    };
    let mut result = PartResult::new(part, verdict, &answer.reason);
    if let Some(rise) = answer.rise_db {
        result = result.with("voice_rise_db", format!("{rise:.1}"));
    }
    if let Some(delay) = answer.delay_ms {
        result = result.with("delay_ms", delay);
    }
    if let Some(cancelled) = answer.cancelled_db {
        result = result.with("cancelled_db", format!("{cancelled:.1}"));
    }
    if let Some(median) = answer.cancelled_median_db {
        result = result.with("cancelled_median_db", format!("{median:.1}"));
    }
    if answer.cancelled_lower_bound == Some(true) {
        // A residual at the evidence's floor: the figures are at least these.
        result = result.with("cancelled_lower_bound", true);
    }
    if let Some(age) = answer.turn_age_s {
        result = result.with("turn_age_s", age);
    }
    result
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Speaker<'a> {
    pub bus: &'a Bus,
    pub occasion: Occasion,
    pub answer_within: Duration,
}

impl Check for Speaker<'_> {
    fn part(&self) -> &str {
        "speaker"
    }

    fn run(&mut self, deadline: Instant) -> PartResult {
        let occasion = match self.occasion {
            Occasion::Factory => "factory",
            Occasion::OnDemand => "on_demand",
            Occasion::FirstBoot => {
                return PartResult::new(
                    "speaker",
                    Verdict::NotChecked,
                    "asked only at the factory and on demand: a passive run asks the voice nothing",
                );
            }
        };
        if Instant::now() + self.answer_within > deadline {
            return PartResult::new(
                "speaker",
                Verdict::NotChecked,
                "the self-test ran out of time before the voice could be asked",
            );
        }
        let not_asked = |error: std::io::Error| {
            PartResult::new(
                "speaker",
                Verdict::NotChecked,
                &format!("{} cannot be asked: {error}", SpeakerCheckRequest::TOPIC),
            )
        };
        let mut asks = match self.bus.publisher_declared(
            SpeakerCheckRequest::TOPIC,
            &SpeakerCheckRequest::declaration(),
            "selftest",
        ) {
            Ok(asks) => asks,
            Err(error) => return not_asked(error),
        };
        let id = format!("selftest-{}", unix_ms());
        let sending_ms = unix_ms();
        let request = SpeakerCheckRequest {
            id: id.clone(),
            occasion: occasion.to_owned(),
        };
        if let Err(error) = asks.publish_json(1, &request) {
            return not_asked(error);
        }
        let until = Instant::now() + self.answer_within;
        while Instant::now() < until {
            if let Ok(Some(message)) = self.bus.latest(SpeakerCheck::TOPIC)
                && message.header.at_unix_ms >= sending_ms
                && let Ok(said) = message.parse::<SpeakerCheck>()
                && said.id == id
            {
                return judge(Some(&said));
            }
            std::thread::sleep(LOOK_EVERY);
        }
        judge(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(verdict: &str, reason: &str) -> SpeakerCheck {
        SpeakerCheck {
            id: "x".into(),
            verdict: verdict.into(),
            reason: reason.into(),
            rise_db: (verdict == "pass").then_some(61.0),
            delay_ms: None,
            cancelled_db: (verdict == "pass").then_some(60.0),
            cancelled_median_db: (verdict == "pass").then_some(54.0),
            cancelled_lower_bound: (verdict == "pass").then_some(true),
            turn_age_s: (verdict == "pass").then_some(75),
        }
    }

    fn value<'a>(result: &'a PartResult, key: &str) -> Option<&'a str> {
        result
            .values
            .iter()
            .find(|v| v.key == key)
            .map(|v| v.value.as_str())
    }

    fn quiet_bus(name: &str) -> (Bus, std::path::PathBuf) {
        // Not ring.rs's `/tmp/ls-`: the crate's unit tests are one process.
        let root = std::path::PathBuf::from(format!("/tmp/lsk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (Bus::new(&root).unwrap(), root)
    }

    #[test]
    fn the_answer_gives_the_verdict_and_what_the_voice_measured() {
        let passed = judge(Some(&answer("pass", "")));
        assert_eq!(passed.verdict, Verdict::Pass, "{passed:?}");
        assert_eq!(value(&passed, "voice_rise_db"), Some("61.0"));
        assert_eq!(value(&passed, "cancelled_db"), Some("60.0"));
        assert_eq!(value(&passed, "cancelled_median_db"), Some("54.0"));
        assert_eq!(value(&passed, "cancelled_lower_bound"), Some("true"));
        assert_eq!(value(&passed, "turn_age_s"), Some("75"));
        assert_eq!(value(&passed, "delay_ms"), None);
        let early = judge(Some(&answer("not_checked", "the lamp has not spoken")));
        assert_eq!(early.verdict, Verdict::NotChecked);
        assert_eq!(early.reason, "the lamp has not spoken");
    }

    #[test]
    fn no_answer_is_not_checked_and_first_boot_asks_nothing() {
        assert!(judge(None).reason.contains("LAMP_SELFTEST_SPEAKER"));
        let (bus, root) = quiet_bus("spk-boot");
        let result = Speaker {
            bus: &bus,
            occasion: Occasion::FirstBoot,
            answer_within: ANSWER_WITHIN,
        }
        .run(Instant::now() + Duration::from_secs(5));
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(bus.latest(SpeakerCheckRequest::TOPIC).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_voice_is_asked_and_its_answer_for_that_request_is_taken() {
        let (bus, root) = quiet_bus("spk");
        let voice_bus = Bus::new(&root).unwrap();
        let mut requests = voice_bus
            .subscribe_typed(
                SpeakerCheckRequest::TOPIC,
                "voice",
                SpeakerCheckRequest::TYPE,
            )
            .unwrap();
        let mut answers = voice_bus
            .publisher_declared(SpeakerCheck::TOPIC, &SpeakerCheck::declaration(), "voice")
            .unwrap();
        let voice = std::thread::spawn(move || {
            let message = loop {
                if let Some(message) = requests.recv(Duration::from_millis(20)).unwrap() {
                    break message;
                }
            };
            let request: SpeakerCheckRequest = message.parse().unwrap();
            assert_eq!(request.occasion, "on_demand");
            answers
                .publish_json(
                    1,
                    &SpeakerCheck {
                        id: request.id,
                        ..answer("pass", "")
                    },
                )
                .unwrap();
            answers
        });
        let result = Speaker {
            bus: &bus,
            occasion: Occasion::OnDemand,
            // The voice thread answers only after it has the request, so the
            // order is fixed; the only race left is a loaded runner starving
            // it past the window. `run` returns at the answer, so a window no
            // runner outlasts costs nothing (#705).
            answer_within: Duration::from_secs(120),
        }
        .run(Instant::now() + Duration::from_secs(240));
        let _held = voice.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
    }

    #[test]
    fn with_too_little_time_left_no_request_is_made() {
        let (bus, root) = quiet_bus("spk-late");
        let mut hears = bus
            .subscribe_typed(
                SpeakerCheckRequest::TOPIC,
                "voice",
                SpeakerCheckRequest::TYPE,
            )
            .unwrap();
        let result = Speaker {
            bus: &bus,
            occasion: Occasion::OnDemand,
            answer_within: ANSWER_WITHIN,
        }
        .run(Instant::now() + Duration::from_millis(100));
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(
            result.reason.contains("ran out of time"),
            "{}",
            result.reason
        );
        assert!(hears.recv(Duration::from_millis(200)).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
