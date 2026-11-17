//! The owner's Update now (#7517): a round waiting for its quiet moment is
//! taken over when the owner asks, gives the voice its moment (at most
//! 10 s) and switches, talking or not; a round it asked from the start
//! waits for no 45 s of quiet. Everything after the switch is as any round.
use super::*;
use lamp_update::update_now::NowAsk;

/// The round's side of an Update now, heard once from `from` on (the
/// lamp's clock in ms), keeping what the round said of it.
struct Asks {
    now: Rc<Cell<u64>>,
    from: u64,
    told: bool,
    waiting: Vec<Option<&'static str>>,
    asked: usize,
}

impl NowAsk for Asks {
    fn heard(&mut self) -> bool {
        if !self.told && self.now.get() >= self.from {
            self.told = true;
            return true;
        }
        false
    }
    fn waiting(&mut self, waiting: Option<&'static str>) {
        self.waiting.push(waiting);
    }
    fn asked(&mut self) {
        self.asked += 1;
    }
}

fn round_asked(lamp: &mut Lamp, talking: Talk, asks: &mut Asks) -> Outcome {
    let mut ears = FakeEars {
        now: Rc::clone(&lamp.now),
        root: lamp.root.clone(),
        talking,
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
            say: &mut |line| lamp.said.push(line.to_owned()),
            phase: &mut |phase| lamp.phases.push(phase),
            target: &mut |_| {},
            now: Some(asks),
        },
    )
}

fn voice_restarted_at(lamp: &Lamp) -> u64 {
    lamp.systemd
        .restarted_at
        .iter()
        .find(|(unit, _)| unit == "lamp-voice.service")
        .map(|(_, at)| *at)
        .expect("the voice is restarted")
}

#[test]
fn update_now_takes_over_a_wait_for_quiet_and_switches_within_ten_seconds_while_someone_talks() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.quiet_wait = Duration::from_secs(3600);
    let start = lamp.now.get();
    let mut asks = Asks {
        now: Rc::clone(&lamp.now),
        from: start + 120_000,
        told: false,
        waiting: Vec::new(),
        asked: 0,
    };
    // Someone talks the whole time: a plain round would wait the hour.
    let outcome = round_asked(&mut lamp, Box::new(|_| true), &mut asks);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    let restarted = voice_restarted_at(&lamp);
    assert!(
        restarted >= asks.from && restarted <= asks.from + 12_000,
        "asked at {}, the voice restarted at {restarted}",
        asks.from
    );
    assert_eq!(asks.asked, 1);
    assert_eq!(
        asks.waiting,
        [
            Some(lamp_msgs::UpdateRound::WAITING_QUIET),
            Some(lamp_msgs::UpdateRound::WAITING_GIVE_WAY),
            None,
            None,
        ]
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("switching anyway")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn update_now_from_the_start_waits_only_for_the_voices_moment() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    let start = lamp.now.get();
    let mut asks = Asks {
        now: Rc::clone(&lamp.now),
        from: 0,
        told: false,
        waiting: Vec::new(),
        asked: 0,
    };
    // A quiet room: the voice's 3 s, not 45 s of quiet.
    let outcome = round_asked(&mut lamp, Box::new(|_| false), &mut asks);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    let restarted = voice_restarted_at(&lamp);
    assert!(restarted < start + 10_000, "{restarted} from {start}");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("the voice gave way")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn without_update_now_a_talking_room_is_still_waited_for() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.quiet_wait = Duration::from_secs(120);
    let mut asks = Asks {
        now: Rc::clone(&lamp.now),
        from: u64::MAX,
        told: false,
        waiting: Vec::new(),
        asked: 0,
    };
    let outcome = round_asked(&mut lamp, Box::new(|_| true), &mut asks);
    assert!(matches!(outcome, Outcome::NotQuiet(_)), "{outcome:?}");
    assert_eq!(asks.asked, 0);
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
}
