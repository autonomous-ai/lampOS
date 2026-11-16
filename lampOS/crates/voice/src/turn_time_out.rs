//! Each answer's wait told on the bus (`voice/turn_time`, #6256 F8): the
//! same milliseconds the trace's `answer_first_audio` records as
//! `after_decision_ms`, for `lamp-cloud`'s opt-in report counters. A number
//! only. Off unless [`ENVIRONMENT`] is `on`; a bus that can't be had leaves
//! it off. A publish never waits for a reader (`lamp_bus`).
use lamp_msgs::VoiceTurnTime;

/// `on` publishes; anything else: nothing.
pub const ENVIRONMENT: &str = "LAMP_REPORT_COUNTERS";

pub struct TurnTimeOut(Option<lamp_bus::Publisher>);

impl TurnTimeOut {
    /// On with the switch and the lamp's bus.
    pub fn from_environment() -> Self {
        if std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
            && let Ok(bus) = lamp_bus::Bus::from_environment()
        {
            return Self::on(&bus);
        }
        Self(None)
    }

    /// On `bus`; off when the topic can't be declared.
    pub fn on(bus: &lamp_bus::Bus) -> Self {
        Self(
            bus.publisher_declared(
                VoiceTurnTime::TOPIC,
                &VoiceTurnTime::declaration(),
                crate::bus_out::SOURCE,
            )
            .ok(),
        )
    }

    /// An answer's first audio came `after_ms` after the end of speech was
    /// decided.
    pub fn told(&mut self, after_ms: u64) {
        if let Some(out) = self.0.as_mut() {
            let _ = out.publish_json(lamp_ipc::monotonic_us(), &VoiceTurnTime { after_ms });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_answers_wait_reaches_a_reader_and_off_says_nothing() {
        let root = std::env::temp_dir().join(format!("lvtt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut taking = bus
            .subscribe_typed(VoiceTurnTime::TOPIC, "cloud", VoiceTurnTime::TYPE)
            .unwrap();
        TurnTimeOut::on(&bus).told(1_240);
        let message = taking.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(
            message.parse::<VoiceTurnTime>().unwrap(),
            VoiceTurnTime { after_ms: 1_240 }
        );
        TurnTimeOut(None).told(900);
        assert!(taking.recv(Duration::from_millis(50)).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
