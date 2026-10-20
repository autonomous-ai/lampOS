//! Unplug's asleep picture uses the existing ring writer and its authority.
//! A receipt is emitted only after that writer accepted the physical frame.
use lamp_body::unplug::{Phase, Request, State};
use lamp_bus::{Bus, Kind, Message, Publisher};
use lamp_ring::{PIXEL_COUNT, Rgb};
use std::{io, path::Path, time::Duration};

pub const STATE_TOPIC: &str = "ring/unplug_state";
const EXPRESSION_TOPIC: &str = "expression/unplug_state";

fn fresh(message: &Message, source: &str, since: u64, now: u64) -> bool {
    !message.expired
        && message.header.source == source
        && message.header.at_us >= since
        && message.header.at_us <= now
        && now - message.header.at_us <= 500_000
        && message
            .header
            .expires_ms
            .is_some_and(|ms| ms > 0 && ms <= 500)
}

/// Current body proof and Expression's matching presentation intent. This is
/// not proof the ring drew it: consumers must also check `STATE_TOPIC`.
pub fn ready_request(bus: &Bus, at_us: u64) -> Option<String> {
    let request_message = bus.latest(Request::TOPIC).ok().flatten()?;
    let request = request_message.parse::<Request>().ok()?;
    if !request.valid() {
        return None;
    }
    let since = request_message.header.at_us;
    let body = bus.latest(State::TOPIC).ok().flatten()?;
    if !fresh(&body, "body", since, at_us) {
        return None;
    }
    let state = body.parse::<State>().ok()?;
    if state.request != request.request || state.phase != Phase::Ready {
        return None;
    }
    let expression = bus.latest(EXPRESSION_TOPIC).ok().flatten()?;
    if !fresh(&expression, "expression", since, at_us) {
        return None;
    }
    let value = expression.json().ok()?;
    (value["request"] == request.request && value["asleep"] == true).then_some(request.request)
}

pub struct Presentation {
    request: String,
    pub pixels: [Rgb; PIXEL_COUNT],
}

pub struct RingUnplug {
    bus: Bus,
    out: Publisher,
}
impl RingUnplug {
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let mut out = bus.publisher(STATE_TOPIC, Kind::Latest, "ring")?;
        out.expires_after(Some(Duration::from_millis(500)));
        Ok(Self { bus, out })
    }

    pub fn prepare(&self, at_us: u64) -> Option<Presentation> {
        let request = ready_request(&self.bus, at_us)?;
        // An explicit off is never replaced with unplug's warm picture. The
        // worker also checks its current effective light (including kept state).
        for topic in lamp_msgs::Light::TOPICS {
            if let Some(message) = self.bus.latest(topic).ok()? {
                let light = message.parse::<lamp_msgs::Light>().ok()?;
                if light.check().is_err() || !light.on {
                    return None;
                }
            }
        }
        let breath =
            (1.0 - (std::f32::consts::TAU * (at_us % 4_000_000) as f32 / 4_000_000.0).cos()) / 2.0;
        Some(Presentation {
            request,
            pixels: lamp_ring::asleep::asleep_pixels(breath),
        })
    }

    /// Revalidate after the sink write, so a stalled write cannot renew stale
    /// evidence. The caller must supply only a successful write receipt's time.
    pub fn drew(&mut self, at_us: u64, presentation: &Presentation) -> io::Result<()> {
        if self
            .prepare(at_us)
            .is_some_and(|current| current.request == presentation.request)
        {
            self.out.publish_json(
                at_us,
                &serde_json::json!({
                    "request": presentation.request, "asleep": true
                }),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ring_wire::{RingFrame, RingPhase},
        ring_worker::{RingLink, Senses, run_with_sink_senses},
        wire::{Control, WorkerEvent},
    };
    use lamp_ring::{ChannelCeiling, EncodedFrame, FrameSink};
    use lamp_turn_owner::{
        AdmissionState, AdmittedInput, CaptureState, Controller, MonoTime, OutputKind, Permission,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        root: std::path::PathBuf,
        bus: Bus,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "ring-unplug-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let bus = Bus::new(&root).unwrap();
            Self { root, bus }
        }
        fn say(&self, topic: &str, source: &str, at: u64, value: serde_json::Value) {
            let mut out = self.bus.publisher(topic, Kind::Latest, source).unwrap();
            out.expires_after(Some(Duration::from_millis(500)));
            out.publish_json(at, &value).unwrap();
        }
        fn ready(&self, phase: &str, request: &str, at: u64) {
            self.say(
                Request::TOPIC,
                "skills",
                at,
                serde_json::json!({"request":"r1"}),
            );
            self.say(
                State::TOPIC,
                "body",
                at,
                serde_json::json!({"request":request,"phase":phase,"why":""}),
            );
            self.say(
                EXPRESSION_TOPIC,
                "expression",
                at,
                serde_json::json!({"request":"r1","asleep":true}),
            );
        }
        fn acknowledged(&self) -> bool {
            self.bus.latest(STATE_TOPIC).unwrap().is_some()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn stale_mismatched_and_failed_proof_never_prepares_a_picture() {
        for (phase, request, age) in [
            ("failed", "r1", 0),
            ("folding", "r1", 0),
            ("ready", "other", 0),
            ("ready", "r1", 500_001),
        ] {
            let fixture = Fixture::new();
            fixture.ready(phase, request, 1_000_000);
            assert!(
                RingUnplug::on(&fixture.root)
                    .unwrap()
                    .prepare(1_000_000 + age)
                    .is_none()
            );
            assert!(!fixture.acknowledged());
        }
    }

    #[test]
    fn explicit_light_off_cannot_prepare_or_acknowledge_asleep() {
        let fixture = Fixture::new();
        fixture.ready("ready", "r1", 1_000_000);
        let ring = RingUnplug::on(&fixture.root).unwrap();
        assert!(ring.prepare(1_000_000).is_some());
        fixture.say(
            lamp_msgs::Light::TOPICS[0],
            "actions",
            1_000_000,
            serde_json::to_value(lamp_msgs::Light::new(false, "warm", 2).unwrap()).unwrap(),
        );
        assert!(ring.prepare(1_000_000).is_none());
        assert!(!fixture.acknowledged());
    }

    struct Link {
        control: Option<Control>,
        frame: Option<RingFrame>,
        calls: usize,
    }
    impl RingLink for Link {
        fn receive_control(&mut self) -> io::Result<Option<Control>> {
            self.calls += 1;
            Ok(self
                .control
                .take()
                .or_else(|| (self.calls > 8).then_some(Control::Stop)))
        }
        fn receive_frame(&mut self) -> io::Result<Option<RingFrame>> {
            Ok(self.frame.take())
        }
        fn publish(&mut self, _: WorkerEvent) -> io::Result<()> {
            Ok(())
        }
    }
    struct Sink {
        frames: Arc<Mutex<Vec<EncodedFrame>>>,
        fail: bool,
    }
    impl FrameSink for Sink {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            let mut frames = self.frames.lock().unwrap();
            if self.fail && !frames.is_empty() {
                return Err(io::Error::other("injected sink failure"));
            }
            frames.push(frame.clone());
            Ok(())
        }
    }
    fn now() -> MonoTime {
        MonoTime::from_micros(lamp_ipc::monotonic_us())
    }

    // Exercises the actual worker loop and existing guarded sink, including its
    // two rendering paths, not a duplicate of the acknowledgement decision.
    fn run(
        speaking: bool,
        enabled: bool,
        permission: Permission,
        fail: bool,
        light_off: bool,
    ) -> (bool, Vec<EncodedFrame>) {
        let fixture = Fixture::new();
        fixture.ready("ready", "r1", lamp_ipc::monotonic_us());
        if light_off {
            fixture.say(
                lamp_msgs::Light::TOPICS[0],
                "actions",
                lamp_ipc::monotonic_us(),
                serde_json::to_value(lamp_msgs::Light::new(false, "warm", 2).unwrap()).unwrap(),
            );
        }
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let frame = if speaking {
            let owner = controller.admit(now(), AdmittedInput::NewTurn).unwrap();
            controller.input_ended(now(), owner).unwrap();
            let speech = controller
                .plan_output(now(), owner, OutputKind::Speech, None)
                .unwrap();
            let speech = controller.issue_output(now(), speech, 100_000).unwrap();
            controller.playback_started(now(), speech).unwrap();
            let plan = controller
                .plan_output(now(), owner, OutputKind::Light, None)
                .unwrap();
            let permit = controller.issue_output(now(), plan, 100_000).unwrap();
            let snapshot = controller.snapshot(now()).unwrap();
            Some(RingFrame {
                snapshot,
                permit,
                phase: RingPhase::from_snapshot(snapshot).unwrap(),
                ceiling: 40,
                requested_at_us: lamp_ipc::monotonic_us(),
                level: 0,
                attending: false,
                offline: false,
                listening: false,
                accent: 0,
                accent_at_us: 0,
            })
        } else {
            None
        };
        controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        let link = Link {
            control: Some(Control::Authority {
                snapshot: controller.snapshot(now()).unwrap(),
            }),
            frame,
            calls: 0,
        };
        let frames = Arc::new(Mutex::new(Vec::new()));
        let senses = Senses {
            unplug: enabled.then(|| RingUnplug::on(&fixture.root).unwrap()),
            ..Senses::default()
        };
        let result = run_with_sink_senses(
            link,
            boot,
            Sink {
                frames: frames.clone(),
                fail,
            },
            Some(40),
            senses,
        );
        if fail {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        let recorded = frames.lock().unwrap().clone();
        (fixture.acknowledged(), recorded)
    }

    #[test]
    fn actual_resting_and_speaking_frames_show_asleep_before_receipt() {
        for speaking in [false, true] {
            let (ack, frames) = run(speaking, true, Permission::Allowed, false, false);
            assert!(ack);
            let pictures: Vec<_> = (0..=1000)
                .map(|step| {
                    lamp_ring::encode(
                        &lamp_ring::asleep::asleep_pixels(step as f32 / 1000.0),
                        ChannelCeiling::new(40).unwrap(),
                    )
                })
                .collect();
            assert!(
                frames.iter().any(|frame| pictures.contains(frame)),
                "actual sink must receive asleep pixels"
            );
        }
    }

    #[test]
    fn off_privacy_and_failed_writes_never_acknowledge() {
        for speaking in [false, true] {
            assert!(!run(speaking, false, Permission::Allowed, false, false).0);
            assert!(!run(speaking, true, Permission::Denied, false, false).0);
            assert!(!run(speaking, true, Permission::Allowed, false, true).0);
            assert!(!run(speaking, true, Permission::Allowed, true, false).0);
        }
    }
}
