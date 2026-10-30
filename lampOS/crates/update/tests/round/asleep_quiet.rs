//! A lamp asleep, or muted for privacy, says no state on the bus, only its
//! mute (#3770). Its microphones are closed, so no turn can be cut: it is
//! quiet, and the round switches without waiting. A voice that says nothing
//! at all is still waited for.
use super::*;
use lamp_msgs::{MuteReason, VoiceMute};

/// A muted voice: its mute once a second, if it says one, and no state.
struct MutedVoice {
    ears: FakeEars,
    mute: Option<VoiceMute>,
}

impl Ears for MutedVoice {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let from = self.ears.now.get();
        let to = from + wait.as_millis() as u64;
        self.ears.now.set(to);
        ((from / 1000 + 1)..=(to / 1000))
            .filter_map(|_| self.mute.map(Heard::Mute))
            .collect()
    }

    fn health(&mut self) -> Option<Health> {
        self.ears.health()
    }
}

fn round_muted(lamp: &mut Lamp, mute: Option<VoiceMute>) -> Outcome {
    let mut ears = MutedVoice {
        ears: FakeEars {
            now: Rc::clone(&lamp.now),
            root: lamp.root.clone(),
            talking: Box::new(|_| false),
        },
        mute,
    };
    let clock = FakeClock(Rc::clone(&lamp.now));
    let disk = FakeDisk(lamp.free);
    let _rounding = rounding();
    round(
        &lamp.config,
        &mut World {
            channel: &mut lamp.channel,
            systemd: &mut lamp.systemd,
            ears: &mut ears,
            disk: &disk,
            clock: &clock,
            harness: &mut lamp.harness,
            say: &mut |line| lamp.said.push(line.to_owned()),
            phase: &mut |phase| lamp.phases.push(phase),
        },
    )
}

#[test]
fn a_lamp_asleep_or_muted_for_privacy_is_switched_without_waiting() {
    for reason in [Some(MuteReason::Sleep), None] {
        let mut lamp = Lamp::new();
        build_two(&mut lamp);
        let start = lamp.now.get();
        let outcome = round_muted(
            &mut lamp,
            Some(VoiceMute {
                muted: true,
                reason,
                muted_by: None,
            }),
        );
        assert_eq!(outcome, Outcome::Switched(2), "{reason:?}: {:?}", lamp.said);
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-voice.service"),
            voice_unit(&sha("voice 2"))
        );
        // Not after 45 s of watching: as soon as the mute is heard. The
        // 60 s watch after it hears no voice/state either, and passes on
        // the health the asleep voice keeps saying.
        let voice_restarted = lamp
            .systemd
            .restarted_at
            .iter()
            .find(|(unit, _)| unit == "lamp-voice.service")
            .map(|(_, at)| *at)
            .expect("the voice is restarted");
        assert!(
            voice_restarted < start + 10_000,
            "{voice_restarted} from {start}"
        );
    }
}

#[test]
fn a_voice_that_says_nothing_is_still_waited_for() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.quiet_wait = Duration::from_secs(120);
    let outcome = round_muted(&mut lamp, None);
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("cannot be judged")),
        "{outcome:?}"
    );
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
    // An awake voice, unmuted, says its state; one that only says it is
    // open is not judged by the mute either.
    let outcome = round_muted(
        &mut lamp,
        Some(VoiceMute {
            muted: false,
            reason: None,
            muted_by: None,
        }),
    );
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("cannot be judged")),
        "{outcome:?}"
    );
}
