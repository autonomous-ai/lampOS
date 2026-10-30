//! A prerequisite can finish restarting just as someone begins a sentence.
//! Since #3689 the voice is stopped, after the quiet check, before any part
//! it reads restarts: a clean stop with Controls still up, so no build's
//! voice arms its restart guard (the 7071 lamps' first home update). A
//! sentence begun meanwhile reaches no voice, so it doesn't hold the switch:
//! the new voice starts right after its prerequisites.
use super::*;

const MEMORY: &str = "lamp-memory.service";
const VOICE: &str = "lamp-voice.service";

struct SpeechDuringRestart<'a> {
    systemd: &'a mut FakeSystemd,
    queued: Rc<Cell<bool>>,
    began: Rc<Cell<Option<u64>>>,
}

impl Systemd for SpeechDuringRestart<'_> {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        self.systemd.states(units)
    }

    fn self_recovery_ready(&mut self) -> Result<(), String> {
        self.systemd.self_recovery_ready()
    }

    fn reload(&mut self) -> Result<(), String> {
        self.systemd.reload()
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        let result = self.systemd.restart(unit);
        if unit == MEMORY && result.is_ok() && self.began.get().is_none() {
            self.began.set(Some(self.systemd.now.get()));
            self.queued.set(true);
        }
        result
    }

    fn stop(&mut self, unit: &str) -> Result<(), String> {
        self.systemd.stop(unit)
    }

    fn last_line(&mut self, unit: &str) -> Option<String> {
        self.systemd.last_line(unit)
    }
}

struct QueuedSpeech {
    ears: FakeEars,
    queued: Rc<Cell<bool>>,
}

impl Ears for QueuedSpeech {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let mut heard = Vec::new();
        if self.queued.replace(false) {
            heard.extend([Heard::Speech, Heard::State(Voice::Speaking)]);
        }
        heard.extend(self.ears.listen(wait));
        heard
    }

    fn health(&mut self) -> Option<Health> {
        self.ears.health()
    }
}

fn a_sentence_during_the_prerequisite(never_quiet: bool) {
    let mut lamp = Lamp::new();
    let old_memory = sha("memory 1");
    let folder = lamp.root.join(format!("opt/lampOS/memory/{old_memory}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-memory"), "memory 1").unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system").join(MEMORY),
        format!("[Service]\nExecStart=/opt/lampOS/memory/{old_memory}/lamp-memory\n"),
    )
    .unwrap();
    let voice =
        voice_unit(&sha("voice 1")).replace("[Unit]\n", &format!("[Unit]\nAfter={MEMORY}\n"));
    fs::write(lamp.root.join("etc/systemd/system").join(VOICE), voice).unwrap();
    lamp.systemd.start(MEMORY);
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-memory", "memory 2"),
        ],
        &[],
    );
    let before = lamp.lamp_files();
    let queued = Rc::new(Cell::new(false));
    let began = Rc::new(Cell::new(None::<u64>));
    let talking_began = Rc::clone(&began);
    let mut ears = QueuedSpeech {
        ears: FakeEars {
            now: Rc::clone(&lamp.now),
            root: lamp.root.clone(),
            talking: Box::new(move |now| {
                talking_began
                    .get()
                    .is_some_and(|at| never_quiet || now <= at + 5_000)
            }),
        },
        queued: Rc::clone(&queued),
    };
    let clock = FakeClock(Rc::clone(&lamp.now));
    let disk = FakeDisk(lamp.free);
    let outcome = {
        let _rounding = rounding();
        let mut systemd = SpeechDuringRestart {
            systemd: &mut lamp.systemd,
            queued,
            began: Rc::clone(&began),
        };
        round(
            &lamp.config,
            &mut World {
                channel: &mut lamp.channel,
                systemd: &mut systemd,
                ears: &mut ears,
                disk: &disk,
                clock: &clock,
                harness: &mut lamp.harness,
                say: &mut |line| lamp.said.push(line.to_owned()),
                phase: &mut |phase| lamp.phases.push(phase),
            },
        )
    };
    let began = began.get().expect("the prerequisite must restart");
    let voice_restarts: Vec<u64> = lamp
        .systemd
        .restarted_at
        .iter()
        .filter_map(|(unit, at)| (unit == VOICE).then_some(*at))
        .collect();
    // Either way: the voice was stopped before Memory restarted, and is
    // started right after it, not held for the speech.
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    let calls = lamp.calls();
    let stopped = calls
        .iter()
        .position(|call| call == &format!("stop {VOICE}"));
    let memory = calls
        .iter()
        .position(|call| call == &format!("restart {MEMORY}"));
    assert!(stopped.is_some() && stopped < memory, "{calls:?}");
    assert_eq!(voice_restarts, [began], "{:?}", lamp.said);
    assert_ne!(lamp.lamp_files(), before);
}

#[test]
fn a_sentence_begun_during_a_prerequisite_does_not_hold_the_stopped_voice() {
    a_sentence_during_the_prerequisite(false);
}

#[test]
fn speech_that_goes_on_during_a_prerequisite_does_not_hold_the_stopped_voice() {
    a_sentence_during_the_prerequisite(true);
}
