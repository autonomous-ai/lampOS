//! Night sleep's two routes, one night (#940, the first builder's condition
//! 2 on #2602): the same fixture night through Expression's built-in sleep,
//! and through the `night-sleep` skill's trigger in lamp-skills' engine and
//! its rest leased to an Expression that has the skill's claim. Both settle
//! within one look of the eyes (600 ms) of each other, and the same reflexes
//! wake both at the same tick. No bus, lamp or wall-clock wait: both
//! planners and the skills engine are driven by one injected clock.
use lamp_expression::{Catalogue, Engine as Expression, Input, Output};
use lamp_msgs::{
    ClockStamp, ControlGesture, ControlIntent, ControlSource, ExpressionRestOp as Op,
    ExpressionRestPhase as Phase, ExpressionRestRequest as Request, ExpressionSleepClaim, Face,
    Faces, Gesture, JointState, Light, NameHeard, PersonActivity, Room, RoomState, ScheduleDue,
    ScheduleNotice, Voice, VoiceEvent, VoiceProgress, VoiceTiming,
};
use lamp_skills::{Call, Engine as Skills, daily::Window, load};
use std::path::Path;

const CLOCK: &str = "night-sleep-boot";
const SKILL_ROUTE: &str = "expression-skill-route";
const TICK: u64 = 100_000;
const SECOND: u64 = 1_000_000;
const MINUTE: u64 = 60 * SECOND;
/// One look of the eyes with nobody there (docs/eyes.md): the step both
/// routes see the desk by.
const LOOK: u64 = 600_000;
const START: u64 = 1_000 * SECOND;
/// 22:30 local (UTC here) on 2026-01-01.
const START_MINUTE: u64 = 22 * 60 + 30;
const MIDNIGHT_MS: u64 = 1_767_225_600_000;

/// Today's catalogue, as the product installs it.
fn catalogue() -> Catalogue {
    Catalogue::from_json(include_str!(
        "../../../../deploy/product/settings/expression.json"
    ))
    .unwrap()
}

fn claim(on: bool) -> ExpressionSleepClaim {
    ExpressionSleepClaim {
        skill: "night-sleep".into(),
        on,
    }
}

/// When each route fell asleep, began to settle, began to wake, and
/// released a rest (the head rising with no wake clip).
#[derive(Default, Debug)]
struct Seen {
    asleep: Vec<u64>,
    falling: Vec<u64>,
    waking: Vec<u64>,
    releasing: Vec<u64>,
    /// When a stir at its name began (`LAMP_REST_THROUGH_SOUNDS=on`).
    stirring: Vec<u64>,
    was_asleep: bool,
    was_falling: bool,
    was_waking: bool,
    was_releasing: bool,
    was_stirring: bool,
}

impl Seen {
    fn note(&mut self, now: u64, output: &Output) {
        let named = |name: &str| output.state.behaviours.iter().any(|it| it == name);
        let asleep = output.state.asleep == Some(true);
        let falling = named("falling asleep");
        let waking = named("waking");
        let releasing = named("releasing rest");
        let stirring = named("stirring");
        for (on, was, at) in [
            (stirring, self.was_stirring, &mut self.stirring),
            (asleep, self.was_asleep, &mut self.asleep),
            (falling, self.was_falling, &mut self.falling),
            (waking, self.was_waking, &mut self.waking),
            (releasing, self.was_releasing, &mut self.releasing),
        ] {
            if on && !was {
                at.push(now);
            }
        }
        self.was_asleep = asleep;
        self.was_falling = falling;
        self.was_waking = waking;
        self.was_releasing = releasing;
        self.was_stirring = stirring;
    }

    /// One settle, and since then no wake, no release and no settle again:
    /// the head never moved.
    fn still_down(&self) {
        assert!(self.was_asleep, "{self:?}");
        assert_eq!(self.asleep.len(), 1, "{self:?}");
        assert_eq!(self.falling.len(), 1, "settled again: {self:?}");
        assert!(self.waking.is_empty(), "{self:?}");
        assert!(self.releasing.is_empty(), "{self:?}");
    }
}

/// How long Expression holds the night's sleep after its lease lapses
/// (`rest::SLEEP_HOLD_US`).
const HOLD: u64 = 10 * SECOND;

struct Night {
    now: u64,
    sequence: u64,
    voice: Voice,
    turn: u64,
    faces: Vec<Face>,
    built_in: Expression,
    skill_route: Expression,
    /// None: lamp-skills is not running (the fallback's case).
    skills: Option<Skills>,
    episode: u64,
    /// lamp-skills' publisher session: a new one each time it starts.
    session: &'static str,
    resting: bool,
    /// The rest's window, as the skill asked it: its lease ends with it.
    during: Option<Window>,
    request_sequence: u64,
    /// How late each rest reaches Expression: at its freshness bound, every
    /// one is refused as stale.
    lag: u64,
    built_in_seen: Seen,
    skill_seen: Seen,
    /// When the skills engine asked for the night's rest.
    rests: Vec<u64>,
    /// The skills engine hears nothing of `voice/state` (a gap in its
    /// input, reviewer-two's second P2 on #3092).
    voice_unheard: bool,
}

impl Night {
    fn new(claimed: Option<ExpressionSleepClaim>, skills: bool) -> Self {
        let built_in = Expression::new(catalogue(), true, CLOCK).unwrap();
        let mut skill_route = Expression::new(catalogue(), true, CLOCK)
            .unwrap()
            .with_session_id(SKILL_ROUTE)
            .unwrap()
            .with_rest_requests();
        skill_route.set_sleep_claim(START, claimed.as_ref());
        let skills = skills.then(|| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/night-sleep");
            let skill = load(&path).unwrap();
            assert!(skill.enabled);
            // Not skipping the ask: night sleep asks nobody.
            let mut engine = Skills::new(vec![skill], false);
            engine.clock(0, START, MIDNIGHT_MS + START_MINUTE * 60_000);
            engine
        });
        let mut night = Self {
            now: START,
            sequence: 0,
            voice: Voice::Resting,
            turn: 1,
            faces: Vec::new(),
            built_in,
            skill_route,
            skills,
            episode: 0,
            session: "skills-one",
            resting: false,
            during: None,
            request_sequence: 0,
            lag: 0,
            built_in_seen: Seen::default(),
            skill_seen: Seen::default(),
            rests: Vec::new(),
            voice_unheard: false,
        };
        let stamp = night.stamp();
        night.send(Input::Light {
            stamp,
            value: Light::new(true, "warm", 1).unwrap(),
        });
        night.daytime();
        night.parts();
        night
    }

    /// The skills engine and both Expressions with
    /// `LAMP_REST_THROUGH_SOUNDS=on` (#1672).
    fn through_sounds(mut self) -> Self {
        self.skills = self.skills.take().map(Skills::resting_through_sounds);
        self.built_in = self.built_in.with_rest_through_sounds();
        self.skill_route = self.skill_route.with_rest_through_sounds();
        self
    }

    fn stamp(&mut self) -> ClockStamp {
        self.sequence += 1;
        ClockStamp {
            clock_id: CLOCK.into(),
            at_us: self.now,
            sequence: self.sequence,
        }
    }

    /// The same input to both planners.
    fn send(&mut self, input: Input) {
        self.built_in.ingest(self.now, input.clone()).unwrap();
        self.skill_route.ingest(self.now, input).unwrap();
    }

    fn minute(&self) -> u64 {
        (START_MINUTE + (self.now - START) / MINUTE) % 1_440
    }

    fn daytime(&mut self) {
        let stamp = self.stamp();
        let minute = self.minute() as u16;
        self.send(Input::DayTime { stamp, minute });
    }

    fn say_voice(&mut self) {
        let timing = VoiceTiming {
            stamp: self.stamp(),
            session_id: "voice-one".into(),
            turn: self.turn,
            request: self.turn * 10 + 1,
            state: self.voice,
            accepting_input: true,
            event: VoiceEvent::State,
            first_audio_us: None,
        };
        self.send(Input::Progress(VoiceProgress {
            timing,
            audio_generation: 1,
            held: false,
        }));
    }

    /// The voice, fresh joints and a quiet room, as said every 100 ms.
    fn parts(&mut self) {
        self.say_voice();
        let stamp = self.stamp();
        self.send(Input::Joints {
            stamp,
            value: JointState {
                name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                    .map(str::to_owned)
                    .to_vec(),
                position: vec![0.0; 5],
                commanded: vec![],
                velocity: vec![],
                effort: vec![],
            },
        });
        let stamp = self.stamp();
        self.send(Input::Room {
            stamp,
            value: RoomState { state: Room::Quiet },
        });
    }

    fn set_voice(&mut self, state: Voice) {
        self.voice = state;
        self.turn += 1;
        self.say_voice();
        self.observe("voice/state", &serde_json::json!({ "state": state }), None);
    }

    fn observe(&mut self, topic: &str, value: &serde_json::Value, expires_ms: Option<u64>) {
        let Some(skills) = &mut self.skills else {
            return;
        };
        let runs = skills.observe(
            topic,
            self.now,
            &serde_json::to_vec(value).unwrap(),
            expires_ms,
        );
        for run in runs {
            for call in run.calls {
                match call {
                    Call::Rest(rest) if rest.op == Op::Rest => {
                        self.rests.push(self.now);
                        self.during = rest.during;
                        self.rest(true);
                    }
                    other => panic!("night sleep made {other:?}"),
                }
            }
        }
    }

    /// The routine manager's lease, as `routine.rs` sends it: a new
    /// episode, then a renewal every second while Expression says resting,
    /// none past its window's end (`Window::rest_remaining_ms`).
    fn rest(&mut self, new: bool) {
        let left_us = self.during.map_or(Some(u64::MAX), |during| {
            during
                .rest_remaining_ms(self.minute() as u16, (self.now - START) % MINUTE / 1_000)
                .map(|(ms, _)| ms * 1_000)
        });
        if new {
            self.episode += 1;
        }
        self.request_sequence += 1;
        let request = Request {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: self.now - self.lag,
                sequence: self.request_sequence,
            },
            skill: "night-sleep".into(),
            skill_session_id: self.session.into(),
            expression_session_id: SKILL_ROUTE.into(),
            episode: self.episode,
            op: Op::Rest,
            valid_until_us: self.now - self.lag + left_us.unwrap_or(0).min(3 * SECOND),
        };
        self.resting = self.skill_route.ingest_rest(self.now, request).is_ok();
    }

    fn step(&mut self) {
        self.now += TICK;
        let since = self.now - START;
        self.parts();
        if since.is_multiple_of(MINUTE) {
            self.daytime();
        }
        if since.is_multiple_of(LOOK) {
            let faces = Faces {
                faces: self.faces.clone(),
            };
            let stamp = self.stamp();
            self.send(Input::Faces {
                stamp,
                value: faces.clone(),
            });
            self.observe(Faces::TOPIC, &serde_json::to_value(&faces).unwrap(), None);
        }
        if since.is_multiple_of(SECOND) {
            let voice = self.voice;
            if !self.voice_unheard {
                self.observe("voice/state", &serde_json::json!({ "state": voice }), None);
            }
            if self.resting {
                let receipt = self.skill_route.rest_state(self.now).unwrap();
                if receipt.phase == Phase::Resting {
                    self.rest(false);
                } else {
                    self.resting = false;
                }
            }
        }
        let built_in = self.built_in.tick(self.now);
        let skill_route = self.skill_route.tick(self.now);
        self.built_in_seen.note(self.now, &built_in);
        self.skill_seen.note(self.now, &skill_route);
        if since.is_multiple_of(2 * TICK) {
            let state = serde_json::to_value(&skill_route.state).unwrap();
            self.observe("expression/state", &state, Some(500));
        }
    }

    /// Run until local `hour:minute` (past midnight counts on).
    fn until(&mut self, hour: u64, minute: u64) {
        let mut at = hour * 60 + minute;
        if at < START_MINUTE {
            at += 1_440;
        }
        let until = START + (at - START_MINUTE) * MINUTE;
        while self.now < until {
            self.step();
        }
    }

    fn run(&mut self, micros: u64) {
        let until = self.now + micros;
        while self.now < until {
            self.step();
        }
    }

    fn touch(&mut self) {
        let stamp = self.stamp();
        let value = ControlGesture {
            stamp: stamp.clone(),
            session_id: "controls-one".into(),
            source: ControlSource::Base,
            name: "base".into(),
            gesture: Gesture::Tap,
            intent: ControlIntent::Stop,
            duration_us: 120_000,
            released_us: self.now - 10_000,
            enabled: true,
        };
        self.send(Input::Touch { stamp, value });
    }

    fn due(&mut self, id: u64) {
        let stamp = self.stamp();
        let value = ScheduleDue {
            stamp: stamp.clone(),
            store_id: "0123456789abcdef0123456789abcdef".into(),
            revision: id,
            enabled: true,
            ready: true,
            pending: 1,
            waiting_for_clock: 0,
            oldest: Some(ScheduleNotice {
                id,
                delivery: 1,
                due_unix_ms: 1_760_000_000_000,
                late_by_ms: None,
                late: false,
            }),
        };
        self.send(Input::Due { stamp, value });
    }

    /// Its name, heard with the person's speech (`name_requires_speech`).
    fn name(&mut self) {
        self.set_voice(Voice::Listening);
        let mut stamp = self.stamp();
        let activity = PersonActivity {
            stamp: stamp.clone(),
            session_id: "voice-one".into(),
            turn: self.turn,
            request: self.turn * 10 + 1,
            speaking: true,
        };
        self.send(Input::Activity(activity));
        self.run(4 * TICK);
        stamp = self.stamp();
        self.send(Input::Name {
            stamp,
            value: NameHeard {
                score: 0.97,
                ago_ms: 400,
            },
        });
    }
}

fn face() -> Face {
    Face {
        id: "face-one".into(),
        xmin: 0.4,
        ymin: 0.3,
        xmax: 0.6,
        ymax: 0.6,
        confidence: 0.9,
        yaw: None,
        facing: None,
        speaking: None,
    }
}

fn clock(now: u64) -> String {
    let minute = (START_MINUTE + (now - START) / MINUTE) % 1_440;
    let second = (now - START) % MINUTE;
    format!(
        "{:02}:{:02}:{:04.1}",
        minute / 60,
        minute % 60,
        second as f64 / 1e6
    )
}

#[test]
fn the_skill_route_settles_with_the_built_in_sleep_and_wakes_on_the_same_reflexes() {
    let mut night = Night::new(Some(claim(true)), true);
    // Gone from the desk at 22:30: asleep at 23:00 by both.
    night.until(23, 10);
    // A sound the voice only listens to wakes neither.
    night.set_voice(Voice::Listening);
    night.run(3 * SECOND);
    night.set_voice(Voice::Resting);
    night.until(23, 20);
    night.touch();
    night.until(23, 40);
    night.due(7);
    night.until(0, 0);
    // A conversation.
    night.set_voice(Voice::Thinking);
    night.run(2 * SECOND);
    night.set_voice(Voice::Speaking);
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    night.until(0, 30);
    night.name();
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    // The first face after 06:00.
    night.until(6, 1);
    night.faces = vec![face()];
    night.until(6, 5);
    let (built_in, skill) = (&night.built_in_seen, &night.skill_seen);
    let said = |seen: &Seen| {
        (
            seen.asleep.iter().map(|at| clock(*at)).collect::<Vec<_>>(),
            seen.waking.iter().map(|at| clock(*at)).collect::<Vec<_>>(),
        )
    };
    eprintln!("built-in {:?}\nskill    {:?}", said(built_in), said(skill));
    // Asleep at 23:00, and again after each of the four night wakes.
    assert_eq!(built_in.asleep.len(), 5, "{:?}", said(built_in));
    assert_eq!(
        skill.asleep.len(),
        built_in.asleep.len(),
        "{:?}",
        said(skill)
    );
    for (skill, built_in) in skill.asleep.iter().zip(&built_in.asleep) {
        assert!(
            skill.abs_diff(*built_in) <= LOOK,
            "settled at {} by the skill, {} built in",
            clock(*skill),
            clock(*built_in)
        );
    }
    // A touch, a reminder, a conversation, its name, the morning face.
    assert_eq!(built_in.waking.len(), 5, "{:?}", said(built_in));
    assert_eq!(skill.waking, built_in.waking, "{:?}", said(skill));
    assert_eq!(clock(built_in.asleep[0]), "23:00:00.0");
}

/// FALLBACK (#940, for one release): no claim, or one gone stale (the
/// runtime's `sleep_claim_in`), and lamp-skills is not there: Expression
/// sleeps by its own schedule, at the built-in's own moment.
#[test]
fn with_no_claim_the_fallback_sleeps_as_the_built_in_one() {
    let mut night = Night::new(None, false);
    night.until(23, 30);
    assert_eq!(night.skill_seen.asleep, night.built_in_seen.asleep);
    assert_eq!(night.skill_seen.asleep.len(), 1);
}

/// A claim that comes back while the fallback sleeps leaves that sleep, and
/// the first face of the morning still wakes it.
#[test]
fn a_claim_back_mid_sleep_keeps_the_fallbacks_sleep_to_the_morning() {
    let mut night = Night::new(None, false);
    night.until(23, 30);
    night
        .skill_route
        .set_sleep_claim(night.now, Some(&claim(true)));
    night.until(6, 1);
    night.faces = vec![face()];
    night.until(6, 3);
    assert_eq!(night.skill_seen.asleep.len(), 1);
    assert_eq!(night.skill_seen.waking, night.built_in_seen.waking);
    assert_eq!(night.skill_seen.waking.len(), 1);
}

/// Turned off by a person, night sleep still claims the night (`on` false):
/// Expression does not sleep by itself, and nothing sleeps.
#[test]
fn night_sleep_turned_off_claims_the_night_and_nothing_sleeps() {
    let mut night = Night::new(Some(claim(false)), false);
    night.until(1, 0);
    assert_eq!(night.built_in_seen.asleep.len(), 1);
    assert!(night.skill_seen.asleep.is_empty());
}

/// The shipped night-sleep folder copied into `dir`: its `skill.toml`.
fn shipped_night_sleep(dir: &Path) -> String {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/night-sleep");
    let folder = dir.join(lamp_skills::routine::NIGHT_SLEEP);
    std::fs::create_dir(&folder).unwrap();
    std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    let text = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    std::fs::write(folder.join("skill.toml"), &text).unwrap();
    text
}

/// What lamp-skills claims: the shipped folder on, off when a person turned
/// it off or the folder is disabled, and nothing with no folder. Nothing
/// either while it cannot rest, unless a person turned it off (#2744).
#[test]
fn the_claim_names_night_sleep_on_or_off_and_nothing_without_it() {
    use lamp_skills::routine::{NIGHT_SLEEP, NOT_CONNECTED, sleep_claim};
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(sleep_claim(dir.path(), false, None), None);
    let text = shipped_night_sleep(dir.path());
    let folder = dir.path().join(NIGHT_SLEEP);
    assert_eq!(sleep_claim(dir.path(), false, None), Some(claim(true)));
    assert_eq!(sleep_claim(dir.path(), true, None), Some(claim(false)));
    assert_eq!(sleep_claim(dir.path(), false, Some(NOT_CONNECTED)), None);
    assert_eq!(
        sleep_claim(dir.path(), true, Some(NOT_CONNECTED)),
        Some(claim(false))
    );
    std::fs::write(
        folder.join("skill.toml"),
        text.replace("enabled = true", "enabled = false"),
    )
    .unwrap();
    assert_eq!(sleep_claim(dir.path(), false, None), Some(claim(false)));
}

/// The first builder's P2 on #2744: night sleep's rest is always refused,
/// here each ask reaching Expression stale (its clock and lamp-skills' far
/// apart, a refusal that repeats). The lamp does not sleep while the claim
/// stands; the claim is then not said, and once the last one goes stale
/// Expression's own sleep takes the night. A claim from the file alone
/// would keep the lamp awake all night.
#[test]
fn a_rest_always_refused_gives_the_night_to_the_fallback() {
    use lamp_skills::routine::{refusal_repeats, sleep_claim};
    let dir = tempfile::tempdir().unwrap();
    shipped_night_sleep(dir.path());
    let mut night = Night::new(sleep_claim(dir.path(), false, None), true);
    night.lag = lamp_msgs::EXPRESSION_REST_FRESH_US;
    night.until(23, 5);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.phase, Phase::Refused);
    let why = state.reason.unwrap();
    assert!(refusal_repeats(&why), "{why}");
    assert!(night.skill_seen.asleep.is_empty(), "{:?}", night.skill_seen);
    let claimed = sleep_claim(dir.path(), false, Some(&why));
    assert_eq!(claimed, None);
    // The last claim said goes stale (180 s): Expression reads none.
    night.run(3 * MINUTE);
    night
        .skill_route
        .set_sleep_claim(night.now, claimed.as_ref());
    night.until(23, 45);
    assert_eq!(night.skill_seen.asleep.len(), 1, "{:?}", night.skill_seen);
    assert!(night.skill_seen.was_asleep);
}

/// The sleep grant is a core skill's, asks nobody, and allows only rest.
#[test]
fn the_sleep_grant_is_core_rest_alone_and_asks_nobody() {
    let dir = tempfile::tempdir().unwrap();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/night-sleep");
    let text = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    let folder = dir.path().join(lamp_skills::routine::NIGHT_SLEEP);
    std::fs::create_dir(&folder).unwrap();
    std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    for (wrong, why) in [
        (
            text.replace("kind = \"core\"", "kind = \"installable\""),
            "requires kind = core",
        ),
        (
            format!("{text}\n[ask]\npolicy = \"nudge\"\n"),
            "asks nobody",
        ),
        (
            text.replace("body = true", "body = true\nspeak = true") + "\n[[do]]\ncue = \"hum\"\n",
            "only rest steps",
        ),
    ] {
        std::fs::write(folder.join("skill.toml"), wrong).unwrap();
        let error = load(&folder).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
}

/// Row 7 on lamp-06c1, finding 2 (#1672), and the first builder's P2 on
/// #2710: lamp-skills restarts mid-night, as an update round does. The dead
/// session's lease lapses after 3 s, and Expression holds the sleep down
/// for its hold; the new session's first rest takes it over there. The head
/// never moves: no release, no wake and no second settle.
#[test]
fn a_skills_restart_mid_night_hands_the_sleep_over_with_the_head_down() {
    let mut night = Night::new(Some(claim(true)), true);
    night.until(23, 10);
    assert!(night.resting && night.skill_seen.was_asleep);
    // Renewed at this whole second, then lamp-skills stops. Its claim,
    // said every 60 s, is still fresh.
    night.resting = false;
    night.skills = None;
    night.run(5 * SECOND);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.phase, Phase::Resting, "held past its lease");
    night.skill_seen.still_down();
    // The new session's first rest, inside the hold: taken over.
    night.session = "skills-two";
    night.request_sequence = 0;
    night.rest(true);
    assert!(
        night.resting,
        "{:?}",
        night.skill_route.rest_state(night.now)
    );
    night.run(HOLD + 30 * SECOND);
    assert!(night.resting);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.request.unwrap().skill_session_id, "skills-two");
    night.skill_seen.still_down();
}

/// The window's edge (#940, the Reviewer-lead's point on #2710): a newer
/// episode of the same session takes the night's sleep over as it is, with
/// no release in between.
#[test]
fn a_newer_episode_takes_the_night_over_with_no_lift() {
    let mut night = Night::new(Some(claim(true)), true);
    night.until(23, 10);
    assert!(night.resting && night.skill_seen.was_asleep);
    let episode = night.episode;
    night.rest(true);
    assert!(night.resting);
    night.run(30 * SECOND);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.phase, Phase::Resting);
    assert_eq!(state.request.unwrap().episode, episode + 1);
    night.skill_seen.still_down();
}

/// A dead claimer, and no new session: past the hold Expression lets the
/// skill's rest go, and the built-in sleep keeps the night as it is (inside
/// the night, nobody seen), with no lift. The claim going stale changes
/// nothing, and the first face of the morning wakes it.
#[test]
fn a_dead_claimer_passes_the_night_to_the_fallback_with_no_lift() {
    let mut night = Night::new(Some(claim(true)), true);
    night.until(23, 10);
    assert!(night.resting && night.skill_seen.was_asleep);
    night.resting = false;
    night.skills = None;
    night.run(3 * SECOND + HOLD + SECOND);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.phase, Phase::Cancelled);
    night.skill_seen.still_down();
    // Its claim goes stale (180 s): the fallback's own schedule.
    night.run(3 * MINUTE);
    night.skill_route.set_sleep_claim(night.now, None);
    night.until(6, 1);
    night.skill_seen.still_down();
    night.faces = vec![face()];
    night.until(6, 3);
    assert_eq!(night.skill_seen.waking, night.built_in_seen.waking);
    assert_eq!(night.skill_seen.waking.len(), 1);
    assert!(night.skill_seen.releasing.is_empty());
}

/// Nobody seen for a day (#940, the Reviewer-lead's point on #2710): the
/// rest's window, 23:00 to 22:59, ends with the lamp still asleep. Its rest
/// is carried into the next night, so the head stays down through 22:59 and
/// 23:00 and does not rise to settle again 15 minutes later.
#[test]
fn a_lamp_nobody_saw_all_day_keeps_its_head_down_into_the_next_night() {
    let mut night = Night::new(Some(claim(true)), true);
    night.until(23, 10);
    assert!(night.resting && night.skill_seen.was_asleep);
    // To 23:05 the next night.
    night.run(23 * 60 * MINUTE + 55 * MINUTE);
    assert_eq!(clock(night.now), "23:05:00.0");
    night.skill_seen.still_down();
    assert!(night.resting);
}

/// The night's hours are written twice (the Reviewer-lead's P3 on #2710):
/// the skill's `[active]` window and the product catalogue's
/// `alive.sleep.night`, which still wakes the morning face. They match, as
/// does the skill's nobody-for with the catalogue's `nobody_us`.
#[test]
fn the_skill_and_the_product_catalogue_name_the_same_night() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/night-sleep/skill.toml");
    let skill: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let minutes = |at: &toml::Value| {
        let (hour, minute) = at.as_str().unwrap().split_once(':').unwrap();
        hour.parse::<u64>().unwrap() * 60 + minute.parse::<u64>().unwrap()
    };
    let window = [
        minutes(&skill["active"]["from"]),
        minutes(&skill["active"]["to"]),
    ];
    let catalogue: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../deploy/product/settings/expression.json"
    ))
    .unwrap();
    let sleep = &catalogue["alive"]["sleep"];
    let night: Vec<u64> = sleep["night"]
        .as_array()
        .unwrap()
        .iter()
        .map(|minute| minute.as_u64().unwrap())
        .collect();
    assert_eq!(night, window);
    assert_eq!(skill["trigger"][0]["for"].as_str(), Some("15m"));
    assert_eq!(sleep["nobody_us"].as_u64(), Some(15 * MINUTE));
}

/// #1672 on a real lamp, 23:26 to 23:41: small room sounds the voice heard
/// as no words, each listening a second or two. Someone at the desk until
/// 23:20, then nobody; the 15 minutes are up at 23:35.
const SOUNDS: [(u64, u64); 6] = [(23, 22), (23, 24), (23, 25), (23, 28), (23, 30), (23, 33)];

/// A night with someone at the desk until 23:20, and the time the 15
/// minutes of nobody after it are up.
fn gone_at_twenty_past(through: bool) -> Night {
    let mut night = Night::new(Some(claim(true)), true);
    if through {
        night = night.through_sounds();
    }
    night.faces = vec![face()];
    night.until(23, 20);
    night.faces = Vec::new();
    night
}

/// The local time of the night's first rest, if it came.
fn first_rest(night: &Night) -> Option<String> {
    night.rests.first().map(|at| clock(*at))
}

/// A sound heard as no words: the voice listening two seconds, then
/// resting. One of them is said on `voice/heard` with no letter in it.
fn sounds(night: &mut Night) {
    for (index, (hour, minute)) in SOUNDS.into_iter().enumerate() {
        night.until(hour, minute);
        night.set_voice(Voice::Listening);
        if index == 2 {
            night.observe(
                "voice/heard",
                &serde_json::json!({"text": " ... ", "finished": true}),
                None,
            );
        }
        night.run(2 * SECOND);
        night.set_voice(Voice::Resting);
    }
}

/// Switch on: the six sounds are slept through, and the rest comes at the
/// 15 minutes, within one look of the eyes and the voice's next second.
#[test]
fn with_the_switch_a_sound_heard_as_no_words_is_slept_through() {
    let mut night = gone_at_twenty_past(true);
    sounds(&mut night);
    night.until(23, 37);
    let at = night.rests.first().copied().expect("no rest by 23:37");
    let due = START + (23 * 60 + 35 - START_MINUTE) * MINUTE;
    assert!(
        (due..=due + LOOK + SECOND).contains(&at),
        "rested at {}",
        clock(at)
    );
}

/// Switch off: as today, each sound begins the 15 minutes again, so there
/// is no rest until 15 minutes after the last one (23:33).
#[test]
fn without_the_switch_each_sound_begins_the_fifteen_minutes_again() {
    let mut night = gone_at_twenty_past(false);
    sounds(&mut night);
    night.until(23, 48);
    assert_eq!(first_rest(&night), None);
    night.until(23, 50);
    assert!(
        first_rest(&night).is_some_and(|at| at.as_str() >= "23:48:02.0"),
        "{:?}",
        first_rest(&night)
    );
}

/// What begins it again with the switch on, at 23:28: no rest at 23:35,
/// and one once 15 minutes have passed since.
fn begun_again_at_twenty_eight(what: impl FnOnce(&mut Night)) {
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    what(&mut night);
    night.until(23, 43);
    assert_eq!(first_rest(&night), None);
    night.until(23, 45);
    assert!(
        first_rest(&night).is_some_and(|at| at.as_str() >= "23:43:00.0"),
        "{:?}",
        first_rest(&night)
    );
}

/// A line with words.
#[test]
fn with_the_switch_words_heard_begin_the_fifteen_minutes_again() {
    begun_again_at_twenty_eight(|night| {
        night.set_voice(Voice::Listening);
        night.run(SECOND);
        night.observe(
            "voice/heard",
            &serde_json::json!({"text": "hey lamp", "finished": true}),
            None,
        );
        night.run(SECOND);
        night.set_voice(Voice::Resting);
    });
}

/// Words written down only after the voice is back to resting (a late
/// transcript, El's P3 on #3092).
#[test]
fn with_the_switch_words_heard_after_the_voice_rests_begin_it_again() {
    begun_again_at_twenty_eight(|night| {
        night.set_voice(Voice::Listening);
        night.run(SECOND);
        night.set_voice(Voice::Resting);
        night.run(SECOND);
        night.observe(
            "voice/heard",
            &serde_json::json!({"text": "hey lamp", "finished": true}),
            None,
        );
    });
}

/// The lamp's name heard with no words written down ("Hey Lamp" from out
/// of view): El's and reviewer-two's P2 on #3092.
#[test]
fn with_the_switch_the_name_heard_with_no_words_begins_it_again() {
    begun_again_at_twenty_eight(|night| {
        night.set_voice(Voice::Listening);
        night.run(SECOND);
        night.observe(
            "name/heard",
            &serde_json::json!({"score": 0.97, "ago_ms": 400}),
            None,
        );
        night.run(SECOND);
        night.set_voice(Voice::Resting);
    });
}

/// A spell of thinking alone, with no words, is slept through too (El's P3
/// on #3092): the rest comes at 23:35, as with no sound.
#[test]
fn with_the_switch_a_spell_of_thinking_alone_is_slept_through() {
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    night.set_voice(Voice::Thinking);
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    night.until(23, 37);
    let at = night.rests.first().copied().expect("no rest by 23:37");
    assert!(clock(at).as_str() < "23:36:00.0", "rested at {}", clock(at));
}

/// A gap in what the skills engine hears of the voice, from a listening
/// spell at 23:28 to resting again at 23:36, is not carried through: the 15
/// minutes begin again at 23:36, as with the switch off (reviewer-two's
/// second P2 on #3092).
#[test]
fn with_the_switch_a_gap_in_the_voice_begins_the_fifteen_minutes_again() {
    for through in [false, true] {
        let mut night = gone_at_twenty_past(through);
        night.until(23, 28);
        night.set_voice(Voice::Listening);
        night.run(SECOND);
        night.voice_unheard = true;
        night.until(23, 36);
        night.voice_unheard = false;
        night.set_voice(Voice::Resting);
        night.until(23, 50);
        assert_eq!(first_rest(&night), None, "through {through}");
        night.until(23, 53);
        assert!(
            first_rest(&night).is_some_and(|at| at.as_str() >= "23:51:00.0"),
            "through {through}: {:?}",
            first_rest(&night)
        );
    }
}

/// A touch the controls say is on begins it again (a tap on an awake lamp,
/// reviewer-two on #3092); one they say is off does not.
#[test]
fn with_the_switch_a_touch_begins_the_fifteen_minutes_again_and_one_off_does_not() {
    let touch = |enabled: bool| {
        serde_json::to_value(lamp_msgs::TouchEvent {
            stamp: lamp_msgs::ClockStamp {
                clock_id: "boot".into(),
                at_us: 1_100_000,
                sequence: 1,
            },
            session_id: "controls".into(),
            reader_session_id: "gpio".into(),
            enabled,
            source: lamp_msgs::TouchSource::Head,
            name: "head".into(),
            contact_id: 1,
            gesture: lamp_msgs::TouchGesture::Touch,
            when_speaking: lamp_msgs::TouchIntent::Stop,
            when_quiet: lamp_msgs::TouchIntent::Notice,
            when_asleep: None,
            began_us: 1_000_000,
            observed_us: 1_100_000,
            released_us: None,
            active_mask: 3,
            active_names: vec!["pad37".into(), "pad96".into()],
            position: None,
            toward_pad: None,
            distance: 0.0,
            speed: 0.0,
        })
        .unwrap()
    };
    begun_again_at_twenty_eight(|night| night.observe("controls/touch", &touch(true), None));
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    night.observe("controls/touch", &touch(false), None);
    night.until(23, 37);
    let at = night.rests.first().copied().expect("no rest by 23:37");
    assert!(clock(at).as_str() < "23:36:00.0", "rested at {}", clock(at));
}

/// The lamp's own speech: it is not resting.
#[test]
fn with_the_switch_the_lamp_speaking_begins_the_fifteen_minutes_again() {
    begun_again_at_twenty_eight(|night| {
        night.set_voice(Voice::Thinking);
        night.run(SECOND);
        night.set_voice(Voice::Speaking);
        night.run(2 * SECOND);
        night.set_voice(Voice::Resting);
    });
}

/// The body's `body/hand`, pushed or not.
fn hand(pushed: bool) -> serde_json::Value {
    serde_json::json!({"pushed": pushed, "since_us": u64::from(pushed)})
}

/// `body/hand` said every second until local `hour:minute`, as lamp-body
/// says it steadily (every 100 ms on a lamp).
fn hand_until(night: &mut Night, pushed: bool, hour: u64, minute: u64) {
    let mut at = hour * 60 + minute;
    if at < START_MINUTE {
        at += 1_440;
    }
    let until = START + (at - START_MINUTE) * MINUTE;
    while night.now < until {
        night.observe("body/hand", &hand(pushed), None);
        night.run(SECOND);
    }
}

/// A push begins it again; the body saying no hand does not.
#[test]
fn with_the_switch_a_push_begins_the_fifteen_minutes_again_and_no_push_does_not() {
    begun_again_at_twenty_eight(|night| night.observe("body/hand", &hand(true), None));
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    night.observe("body/hand", &hand(false), None);
    night.until(23, 37);
    let at = night.rests.first().copied().expect("no rest by 23:37");
    assert!(clock(at).as_str() < "23:36:00.0", "rested at {}", clock(at));
}

/// A push held from 23:28 to 23:50, said at every message: it begins the 15
/// minutes again once, as it begins, so the rest comes at 23:43, not never
/// (#1672's lamp row on lamp-06c1: a push held or reported steadily
/// restarted the clock at every message).
#[test]
fn with_the_switch_a_push_held_begins_the_fifteen_minutes_again_once_at_its_start() {
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    hand_until(&mut night, true, 23, 43);
    assert_eq!(first_rest(&night), None);
    hand_until(&mut night, true, 23, 50);
    let at = night.rests.first().copied().expect("no rest by 23:50");
    let due = START + (23 * 60 + 43 - START_MINUTE) * MINUTE;
    assert!(
        (due..=due + LOOK + SECOND).contains(&at),
        "rested at {}",
        clock(at)
    );
}

/// A push, let go, then a push again at 23:30: the second begins it again
/// too, so no rest until 23:45.
#[test]
fn with_the_switch_a_push_let_go_and_pushed_again_begins_it_again() {
    let mut night = gone_at_twenty_past(true);
    night.until(23, 28);
    hand_until(&mut night, true, 23, 29);
    hand_until(&mut night, false, 23, 30);
    hand_until(&mut night, true, 23, 31);
    hand_until(&mut night, false, 23, 32);
    night.until(23, 45);
    assert_eq!(first_rest(&night), None);
    night.until(23, 47);
    assert!(
        first_rest(&night).is_some_and(|at| at.as_str() >= "23:45:00.0"),
        "{:?}",
        first_rest(&night)
    );
}

/// Asleep since 23:00 by both routes, at 23:10 (#1672's wakes, night row of
/// 28 to 29 Oct on lamp-06c1).
fn asleep_at_ten_past(through: bool) -> Night {
    let mut night = Night::new(Some(claim(true)), true);
    if through {
        night = night.through_sounds();
    }
    night.until(23, 10);
    assert_eq!(night.skill_seen.asleep.len(), 1, "{:?}", night.skill_seen);
    assert_eq!(night.built_in_seen.asleep.len(), 1);
    night
}

/// A line heard as the voice hears it at night: listening while it lasts,
/// thinking while its words are written down at its end, then resting with
/// no answer. `words`: what `voice/heard` wrote down, if anything.
fn line_with_no_answer(night: &mut Night, words: Option<&str>) {
    night.set_voice(Voice::Listening);
    night.run(2 * SECOND);
    night.set_voice(Voice::Thinking);
    if let Some(text) = words {
        night.observe(
            "voice/heard",
            &serde_json::json!({"text": text, "finished": true}),
            None,
        );
    }
    night.run(SECOND);
    night.set_voice(Voice::Resting);
}

/// (a) A sound heard as no words, its line ending in thinking (00:43:38 and
/// 01:51:37 on lamp-06c1): with the switch, both routes sleep on, the head
/// never lifting. Without it, both wake, as before.
#[test]
fn with_the_switch_a_sound_of_no_words_ending_in_thinking_does_not_wake_the_lamp() {
    let mut night = asleep_at_ten_past(true);
    line_with_no_answer(&mut night, None);
    night.until(23, 20);
    night.skill_seen.still_down();
    night.built_in_seen.still_down();
    assert!(night.resting, "the skill's rest was ended");

    let mut night = asleep_at_ten_past(false);
    line_with_no_answer(&mut night, None);
    night.until(23, 20);
    assert_eq!(night.skill_seen.waking.len(), 1, "{:?}", night.skill_seen);
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);
}

/// (b) A line with words not said to the lamp, its answer held back
/// (03:35:52 on lamp-06c1): the voice thinks and never speaks, and the
/// lamp sleeps on.
#[test]
fn with_the_switch_a_line_not_said_to_the_lamp_does_not_wake_it() {
    let mut night = asleep_at_ten_past(true);
    line_with_no_answer(&mut night, Some("put it there"));
    night.until(23, 20);
    night.skill_seen.still_down();
    night.built_in_seen.still_down();
}

/// (c) The lamp's own answer wakes it, as it begins to speak, by both
/// routes at the same tick.
#[test]
fn with_the_switch_its_own_answer_wakes_it() {
    let mut night = asleep_at_ten_past(true);
    night.set_voice(Voice::Listening);
    night.run(2 * SECOND);
    night.set_voice(Voice::Thinking);
    night.run(SECOND);
    night.skill_seen.still_down();
    let speaks = night.now;
    night.set_voice(Voice::Speaking);
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    night.run(5 * SECOND);
    let waking = &night.skill_seen.waking;
    assert_eq!(waking.len(), 1, "{:?}", night.skill_seen);
    assert!(
        (speaks..=speaks + 2 * TICK).contains(&waking[0]),
        "woke at {}",
        clock(waking[0])
    );
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);
}

/// (e) The first builder on #4285 (03:5x): "stir at the name, wake on the
/// answer". Its name, counted (heard with the person's speech), stirs both
/// routes within half a second and wakes neither; with no answer they
/// settle back and the night's sleep is unbroken: no wake, the skill's
/// rest still held. Its answer within the 5 s wakes both at the same tick.
/// Without the switch, the name wakes both, as before.
#[test]
fn with_the_switch_its_name_stirs_it_and_only_the_answer_wakes_it() {
    let mut night = asleep_at_ten_past(true);
    night.name();
    let named = night.now;
    night.run(2 * SECOND);
    night.set_voice(Voice::Thinking);
    night.run(SECOND);
    night.set_voice(Voice::Resting);
    night.until(23, 20);
    for seen in [&night.skill_seen, &night.built_in_seen] {
        assert_eq!(seen.stirring.len(), 1, "{seen:?}");
        assert!(
            seen.stirring[0] - named <= 5 * TICK,
            "stirred at +{} us",
            seen.stirring[0] - named
        );
        assert!(!seen.was_stirring, "still stirred: {seen:?}");
        seen.still_down();
    }
    assert!(night.resting, "the skill's rest was ended");

    let mut night = asleep_at_ten_past(true);
    night.name();
    night.run(2 * SECOND);
    let speaks = night.now;
    night.set_voice(Voice::Speaking);
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    night.run(5 * SECOND);
    assert_eq!(night.skill_seen.stirring.len(), 1, "{:?}", night.skill_seen);
    let waking = &night.skill_seen.waking;
    assert_eq!(waking.len(), 1, "{:?}", night.skill_seen);
    assert!((speaks..=speaks + 2 * TICK).contains(&waking[0]));
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);

    let mut night = asleep_at_ten_past(false);
    night.name();
    night.run(2 * SECOND);
    night.set_voice(Voice::Resting);
    night.run(5 * SECOND);
    assert!(
        night.skill_seen.stirring.is_empty(),
        "{:?}",
        night.skill_seen
    );
    assert_eq!(night.skill_seen.waking.len(), 1, "{:?}", night.skill_seen);
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);
}

/// (d) A tap wakes it, and a reminder newly due, as before.
#[test]
fn with_the_switch_a_tap_or_a_reminder_wakes_it() {
    let mut night = asleep_at_ten_past(true);
    night.touch();
    night.run(5 * SECOND);
    assert_eq!(night.skill_seen.waking.len(), 1, "{:?}", night.skill_seen);
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);

    let mut night = asleep_at_ten_past(true);
    night.due(7);
    night.run(5 * SECOND);
    assert_eq!(night.skill_seen.waking.len(), 1, "{:?}", night.skill_seen);
    assert_eq!(night.built_in_seen.waking, night.skill_seen.waking);
}
