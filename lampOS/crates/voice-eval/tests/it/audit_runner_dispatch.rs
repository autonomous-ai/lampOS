//! Final-drain audit on the production runner using a virtual clock; no audio.
use crate::common;
use lamp_voice_eval::{
    Result,
    events::{ClockDomain, EventKind, EventSource, RuntimeEvent},
    plan::{LoadedPlan, Scenario, Step},
    record::{StepStatus, StimulusSource, Stratum},
    runner::{self, AttemptContext, Backend, Begin, Finished, Injection},
    stimulus::{SceneTiming, StimulusCatalog},
};
use serde_json::{Value, json};
use std::collections::VecDeque;
#[derive(Clone, Copy)]
enum Mode {
    ClockCost,
    Retirement,
}
struct Rig {
    at: u64,
    events: VecDeque<RuntimeEvent>,
    seen: Vec<RuntimeEvent>,
    injections: Vec<u64>,
    mode: Mode,
    arm: bool,
    delivered: bool,
}
fn event(kind: EventKind, turn: Option<u64>, at: u64) -> RuntimeEvent {
    let mut e = RuntimeEvent::new(kind, turn, at, ClockDomain::Virtual, EventSource::Trace);
    if matches!(
        e.kind,
        EventKind::SpeakerFirstWrite | EventKind::SpeechRetired
    ) {
        e.playback_sequence = Some(1)
    }
    e
}
impl Backend for Rig {
    fn describe(&self) -> Value {
        json!({"kind":"audit_virtual_final_drain"})
    }
    fn stratum(&self, _: &Scenario) -> Stratum {
        Stratum::FakeTiming
    }
    fn source(&self) -> StimulusSource {
        StimulusSource::DeclaredTiming
    }
    fn domain(&self) -> ClockDomain {
        ClockDomain::Virtual
    }
    fn unsupported(&self, _: &Scenario) -> Option<String> {
        None
    }
    fn begin(&mut self, _: &AttemptContext) -> Result<Begin> {
        Ok(Begin::Ready)
    }
    fn now_us(&mut self) -> u64 {
        self.at
    }
    fn next_event(&mut self, until: u64) -> Result<Option<RuntimeEvent>> {
        let e = if self.arm && !self.delivered && until == self.at {
            self.delivered = true;
            Some(match self.mode {
                Mode::ClockCost => {
                    self.at += 200_000;
                    event(
                        EventKind::InputTranscript {
                            text: "finite queued work".into(),
                            finished: true,
                        },
                        None,
                        self.at,
                    )
                }
                Mode::Retirement => event(EventKind::SpeechRetired, Some(1), self.at),
            })
        } else if self
            .events
            .front()
            .is_some_and(|e| e.at_us.unwrap() <= until)
        {
            let e = self.events.pop_front().unwrap();
            self.at = self.at.max(e.at_us.unwrap());
            Some(e)
        } else {
            self.at = self.at.max(until);
            self.arm = match self.mode {
                Mode::ClockCost => self.injections.is_empty(),
                Mode::Retirement => self.injections.len() == 1,
            };
            None
        };
        if let Some(e) = &e {
            self.seen.push(e.clone());
        }
        Ok(e)
    }
    fn runner_time(&self, e: &RuntimeEvent) -> (u64, String) {
        (e.at_us.unwrap(), "audit virtual clock".into())
    }
    fn inject(&mut self, _: &Step, _: &str, _: &SceneTiming, _: u64) -> Result<Injection> {
        self.injections.push(self.at);
        Ok(Injection {
            withheld: None,
            started_us: self.at,
            receipt: json!({"audit_no_audio":true}),
        })
    }
    fn finish(&mut self, _: &AttemptContext) -> Result<Finished> {
        Ok(Finished {
            events: self.seen.clone(),
            clock: None,
            evidence: Value::Null,
            aborted: None,
            unmapped: 0,
            spoken_failure_notice: None,
            corrected_starts: vec![],
            failed_deliveries: vec![],
        })
    }
    fn reproduce(&self, _: &AttemptContext) -> String {
        "audit only".into()
    }
}
fn run(mode: Mode) -> (Vec<u64>, Vec<lamp_voice_eval::record::StepRecord>) {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::default_plan(&catalog).unwrap();
    let name = match mode {
        Mode::ClockCost => "quick-chat",
        Mode::Retirement => "topic-change",
    };
    let scenario = plan.scenario(name).unwrap();
    let mut events = VecDeque::from([event(EventKind::ListeningReady, None, 1000)]);
    if matches!(mode, Mode::Retirement) {
        assert!(scenario.steps[1].trigger.require_lamp_speaking);
        events.extend([
            event(
                EventKind::InputAdmitted {
                    prefix_first_read_us: None,
                    candidate: None,
                    basis: None,
                },
                Some(1),
                1_000_000,
            ),
            event(
                EventKind::LocalEndpoint {
                    last_block_read_us: Some(1_499_000),
                    silence_wait_ms: None,
                },
                Some(1),
                1_500_000,
            ),
            event(EventKind::SpeakerFirstWrite, Some(1), 2_000_000),
        ]);
    }
    let mut rig = Rig {
        at: 0,
        events,
        seen: vec![],
        injections: vec![],
        mode,
        arm: false,
        delivered: false,
    };
    let dir = common::Private::new("finaldrain");
    let ctx = AttemptContext {
        attempt_id: "audit".into(),
        run_id: "audit".into(),
        scenario,
        repetition: 0,
        order: 0,
        seed: 1,
        run_directory: dir.path.clone(),
    };
    let record = runner::run_attempt(&mut rig, &catalog, &ctx).unwrap();
    assert!(rig.delivered, "final-drain event must have been consumed");
    eprintln!(
        "audit injections={:?}; steps={:?}",
        rig.injections, record.steps
    );
    (rig.injections, record.steps)
}
#[test]
fn final_drain_must_recheck_trigger_lateness_before_injection() {
    let (calls, steps) = run(Mode::ClockCost);
    assert!(
        calls.is_empty(),
        "late injection after final drain: {calls:?}"
    );
    assert_eq!(steps[0].status, StepStatus::TriggerStale);
}
#[test]
fn final_drain_must_recheck_speaking_precondition_before_injection() {
    let (calls, steps) = run(Mode::Retirement);
    assert_eq!(
        calls.len(),
        1,
        "second step injected after consumed retirement: {calls:?}"
    );
    assert_eq!(steps[1].status, StepStatus::PreconditionLost);
}
