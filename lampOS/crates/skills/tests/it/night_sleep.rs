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
use lamp_skills::{Call, Engine as Skills, load};
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

/// When each route fell asleep and began to wake.
#[derive(Default, Debug)]
struct Seen {
    asleep: Vec<u64>,
    waking: Vec<u64>,
    was_asleep: bool,
    was_waking: bool,
}

impl Seen {
    fn note(&mut self, now: u64, output: &Output) {
        let asleep = output.state.asleep == Some(true);
        let waking = output.state.behaviours.iter().any(|name| name == "waking");
        if asleep && !self.was_asleep {
            self.asleep.push(now);
        }
        if waking && !self.was_waking {
            self.waking.push(now);
        }
        self.was_asleep = asleep;
        self.was_waking = waking;
    }
}

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
    request_sequence: u64,
    built_in_seen: Seen,
    skill_seen: Seen,
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
            request_sequence: 0,
            built_in_seen: Seen::default(),
            skill_seen: Seen::default(),
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
                    Call::Rest(rest) if rest.op == Op::Rest => self.rest(true),
                    other => panic!("night sleep made {other:?}"),
                }
            }
        }
    }

    /// The routine manager's lease, as `routine.rs` sends it: a new
    /// episode, then a renewal every second while Expression says resting.
    fn rest(&mut self, new: bool) {
        if new {
            self.episode += 1;
        }
        self.request_sequence += 1;
        let request = Request {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: self.now,
                sequence: self.request_sequence,
            },
            skill: "night-sleep".into(),
            skill_session_id: self.session.into(),
            expression_session_id: SKILL_ROUTE.into(),
            episode: self.episode,
            op: Op::Rest,
            valid_until_us: self.now + 3 * SECOND,
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
            self.observe("voice/state", &serde_json::json!({ "state": voice }), None);
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

/// What lamp-skills claims: the shipped folder on, off when a person turned
/// it off or the folder is disabled, and nothing with no folder.
#[test]
fn the_claim_names_night_sleep_on_or_off_and_nothing_without_it() {
    use lamp_skills::routine::{NIGHT_SLEEP, sleep_claim};
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(sleep_claim(dir.path(), false), None);
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/night-sleep");
    let folder = dir.path().join(NIGHT_SLEEP);
    std::fs::create_dir(&folder).unwrap();
    std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    let text = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    std::fs::write(folder.join("skill.toml"), &text).unwrap();
    assert_eq!(sleep_claim(dir.path(), false), Some(claim(true)));
    assert_eq!(sleep_claim(dir.path(), true), Some(claim(false)));
    std::fs::write(
        folder.join("skill.toml"),
        text.replace("enabled = true", "enabled = false"),
    )
    .unwrap();
    assert_eq!(sleep_claim(dir.path(), false), Some(claim(false)));
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

/// Row 7 on lamp-06c1, finding 2 (#1672): lamp-skills restarts after a rest
/// began. Nothing renews the dead session's rest, so Expression lets it go
/// when its 3 s lease lapses, long before the claim would go stale (180 s):
/// the head rises over the settle (4 s), and the new session's rest is
/// refused only until it has. Then it is taken.
#[test]
fn a_rest_from_restarted_skills_is_taken_once_the_dead_sessions_lease_lapses() {
    let mut night = Night::new(Some(claim(true)), true);
    night.until(23, 10);
    assert!(night.resting && night.skill_seen.was_asleep);
    // Renewed at this whole second, then lamp-skills stops. Its claim,
    // said every 60 s, is still fresh.
    let renewed = night.now;
    night.resting = false;
    night.skills = None;
    night.run(3 * SECOND + 2 * TICK);
    let state = night.skill_route.rest_state(night.now).unwrap();
    assert_eq!(state.phase, Phase::Cancelled);
    assert!(
        !night.skill_seen.was_asleep,
        "the dead session's rest is held"
    );
    // The new session, while the head still rises: refused.
    night.session = "skills-two";
    night.request_sequence = 0;
    night.run(SECOND);
    night.rest(true);
    assert!(!night.resting, "taken before the dead rest was let go");
    // Once it has risen: taken, and renewed from then on.
    night.run(renewed + 8 * SECOND - night.now);
    night.rest(true);
    assert!(
        night.resting,
        "{:?}",
        night.skill_route.rest_state(night.now)
    );
    night.run(10 * SECOND);
    assert!(night.resting && night.skill_seen.was_asleep);
    assert_eq!(night.skill_seen.asleep.len(), 2);
}
