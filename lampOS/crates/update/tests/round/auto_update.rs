//! The owner's Auto update (#8769, `LAMP_UPDATE_AUTO`): on, a round with
//! someone talking gives the voice its moment, as Update now does, and
//! switches within 10 s; off (`/etc/lampOS/setup/auto-update-off`), it switches only
//! for Update now. Without the switch, a round waits for quiet as before.
use super::*;
use lamp_update::update_now::NowAsk;

/// The round's side of the app, keeping what the round said of it: an
/// Update now heard once from `from` on (the lamp's clock in ms).
struct App {
    now: Rc<Cell<u64>>,
    from: u64,
    told: bool,
    waiting: Vec<Option<&'static str>>,
    asked: usize,
    auto: usize,
}

impl App {
    fn new(lamp: &Lamp, from: u64) -> Self {
        Self {
            now: Rc::clone(&lamp.now),
            from,
            told: false,
            waiting: Vec::new(),
            asked: 0,
            auto: 0,
        }
    }
}

impl NowAsk for App {
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
    fn auto(&mut self) {
        self.auto += 1;
    }
}

fn round_with(lamp: &mut Lamp, talking: Talk, app: &mut App) -> Outcome {
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
            now: Some(app),
        },
    )
}

fn voice_restarted_at(lamp: &Lamp) -> Option<u64> {
    lamp.systemd
        .restarted_at
        .iter()
        .find(|(unit, _)| unit == "lamp-voice.service")
        .map(|(_, at)| *at)
}

fn turn_off(lamp: &Lamp) {
    let off = lamp
        .root
        .join(lamp_update::auto::OFF.trim_start_matches('/'));
    fs::create_dir_all(off.parent().unwrap()).unwrap();
    fs::write(off, "").unwrap();
}

#[test]
fn auto_update_on_switches_within_ten_seconds_while_someone_talks() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.auto_update = true;
    lamp.config.quiet_wait = Duration::from_secs(3600);
    let start = lamp.now.get();
    let mut app = App::new(&lamp, u64::MAX);
    // Someone talks the whole time: before #8769 the round waited the hour.
    let outcome = round_with(&mut lamp, Box::new(|_| true), &mut app);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    let restarted = voice_restarted_at(&lamp).expect("the voice is restarted");
    assert!(restarted <= start + 12_000, "{restarted} from {start}");
    assert_eq!((app.auto, app.asked), (1, 0));
    assert_eq!(
        app.waiting,
        [
            Some(lamp_msgs::UpdateRound::WAITING_QUIET),
            Some(lamp_msgs::UpdateRound::WAITING_GIVE_WAY),
            None,
            None,
        ]
    );
    for words in ["Auto update is on", "Auto update: no quiet in 10 s"] {
        assert!(
            lamp.said.iter().any(|line| line.contains(words)),
            "{words}: {:?}",
            lamp.said
        );
    }
}

#[test]
fn auto_update_on_in_a_quiet_room_waits_only_for_the_voices_moment() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.auto_update = true;
    let start = lamp.now.get();
    let mut app = App::new(&lamp, u64::MAX);
    let outcome = round_with(&mut lamp, Box::new(|_| false), &mut app);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    let restarted = voice_restarted_at(&lamp).expect("the voice is restarted");
    assert!(restarted < start + 10_000, "{restarted} from {start}");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("Auto update: the voice gave way")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn auto_update_off_switches_nothing_without_update_now() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.auto_update = true;
    turn_off(&lamp);
    let mut app = App::new(&lamp, u64::MAX);
    // A quiet room: before #8769 this round switched.
    let outcome = round_with(&mut lamp, Box::new(|_| false), &mut app);
    assert_eq!(
        outcome,
        Outcome::NotQuiet(lamp_update::auto::OFF_WAITS.to_owned())
    );
    assert_eq!((app.auto, app.asked), (0, 0));
    assert_eq!(voice_restarted_at(&lamp), None);
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
}

#[test]
fn auto_update_off_still_switches_for_update_now() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.auto_update = true;
    turn_off(&lamp);
    let mut app = App::new(&lamp, 0);
    let outcome = round_with(&mut lamp, Box::new(|_| true), &mut app);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!((app.auto, app.asked), (0, 1));
}

#[test]
fn without_the_switch_the_off_mark_changes_nothing_and_talk_is_waited_for() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    turn_off(&lamp);
    lamp.config.quiet_wait = Duration::from_secs(120);
    let mut app = App::new(&lamp, u64::MAX);
    let outcome = round_with(&mut lamp, Box::new(|_| true), &mut app);
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.starts_with("no quiet moment")),
        "{outcome:?}"
    );
    assert_eq!(app.auto, 0);
    // And in a quiet room it switches, mark or not, as before.
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    turn_off(&lamp);
    let mut app = App::new(&lamp, u64::MAX);
    let outcome = round_with(&mut lamp, Box::new(|_| false), &mut app);
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
}
