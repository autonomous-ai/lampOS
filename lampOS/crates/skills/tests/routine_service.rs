//! The actual Skills sender and presentation wiring, with real bus transport
//! and fake Expression/Say owners. No body, mixer or speaker is opened.
use lamp_bus::{Bus, Declaration, Kind, Publisher, Subscriber};
use lamp_ipc::monotonic_us;
use lamp_msgs::{
    ClockStamp, ExpressionRestOp as Op, ExpressionRestPhase as Phase,
    ExpressionRestRequest as Request, ExpressionRestState as Receipt, ExpressionState, SkillEvent,
    SkillStage, Voice, VoiceState,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener, net::UnixStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CLOCK: &str = "routine-service-host";
const SKILL: &str = "routine-check";
const INPUT: &str = "fixture/routine";
const WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(25);

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    // Kill the child before its sockets and private directory are dropped.
    child: Running,
    bus: Bus,
    input: Publisher,
    voice: Publisher,
    state: Publisher,
    receipt: Publisher,
    requests: Subscriber,
    events: Subscriber,
    say: UnixListener,
    reply: Option<(Request, Phase)>,
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with_permissions(true, true)
    }

    fn with_permissions(body: bool, speak: bool) -> Self {
        Self::with_presentation(body, speak, "cue = \"hum\"")
    }

    fn with_presentation(body: bool, speak: bool, presentation: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("lsr-")
            .tempdir_in("/tmp")
            .unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let private = root.path().join("state");
        fs::create_dir(&private).unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        let folder = root.path().join("skills").join(SKILL);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            format!(
                "---\nname: {SKILL}\ndescription: Check leased rest and its following cue.\n---\n"
            ),
        )
        .unwrap();
        // A short window around actual UTC, also valid across midnight.
        // No clock skip can accidentally stand in for a real wall hour.
        let minute = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 60)
            % 1_440;
        let hhmm = |minute: u64| format!("{:02}:{:02}", minute / 60, minute % 60);
        let from = hhmm((minute + 1_440 - 10) % 1_440);
        let to = hhmm((minute + 10) % 1_440);
        fs::write(
            folder.join("skill.toml"),
            format!(
                r#"
[permissions]
topics = ["{INPUT}", "voice/state"]
body = {body}
speak = {speak}

[[trigger]]
name = "night"
topic = "{INPUT}"
field = "phase"
equals = "night"
rearm = "ends"
during = {{ from = "{from}", to = "{to}" }}
once_per_day = true

[[trigger]]
name = "morning"
topic = "{INPUT}"
field = "phase"
equals = "morning"
rearm = "ends"
during = {{ from = "{from}", to = "{to}" }}
once_per_day = true

[ask]
policy = "nudge"

[[do]]
on = "night"
rest = {{ op = "rest", during = {{ from = "{from}", to = "{to}" }} }}

[[do]]
on = "morning"
rest = {{ op = "wake" }}
{presentation}
"#
            ),
        )
        .unwrap();
        let bus = Bus::new(root.path().join("bus")).unwrap();
        let input = bus
            .publisher_declared(
                INPUT,
                &Declaration::new("fixture/Phase", 1, Kind::Latest),
                "fixture",
            )
            .unwrap();
        let state = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        let voice = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let receipt = bus
            .publisher_declared(Receipt::TOPIC, &Receipt::declaration(), "expression")
            .unwrap();
        let requests = bus.subscribe(Request::TOPIC, "fixture_expression").unwrap();
        let events = bus.subscribe(SkillEvent::TOPIC, "fixture_events").unwrap();
        let say = UnixListener::bind(root.path().join("say.sock")).unwrap();
        say.set_nonblocking(true).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
        command
            .env_clear()
            .env("LAMP_SKILL_ROUTINES", "on")
            .env("LAMP_SKILLS_ASK", "skip")
            .arg("--trust-clock")
            .args(["--zone", "UTC", "--boot-id", CLOCK])
            .arg("--dir")
            .arg(root.path().join("skills"))
            .arg("--bus-root")
            .arg(root.path().join("bus"))
            .arg("--daily-state")
            .arg(private.join("daily.json"))
            .arg("--clock-control")
            .arg(root.path().join("clock.sock"))
            .arg("--say-socket")
            .arg(root.path().join("say.sock"))
            .arg("--actions-socket")
            .arg(root.path().join("actions.sock"))
            .arg("--gaze-socket")
            .arg(root.path().join("gaze.sock"))
            .arg("--mood-socket")
            .arg(root.path().join("mood.sock"))
            .stdout(Stdio::null())
            .stderr(fs::File::create(root.path().join("stderr")).unwrap());
        let child = Running(command.spawn().unwrap());
        let mut fixture = Self {
            child,
            bus,
            input,
            voice,
            state,
            receipt,
            requests,
            events,
            say,
            reply: None,
            root,
        };
        let until = Instant::now() + WAIT;
        if body && speak {
            while [INPUT, VoiceState::TOPIC]
                .iter()
                .any(|topic| fixture.bus.subscribers(topic).unwrap().is_empty())
            {
                fixture.pulse();
                fixture.alive_before(until, "Skills subscribed to the loaded fixture");
                assert!(fixture.events.recv(POLL).unwrap().is_none());
            }
        } else {
            let refused = if !body { "body" } else { "speak" };
            loop {
                fixture.pulse();
                fixture.alive_before(until, "Skills refused the missing permission");
                let journal = fs::read_to_string(fixture.root.path().join("stderr")).unwrap();
                if journal.contains("not loaded:")
                    && journal.contains(&format!("needs {refused} = true"))
                {
                    break;
                }
                assert!(fixture.events.recv(POLL).unwrap().is_none());
            }
        }
        fixture.pulse();
        fixture.barrier();
        fixture
    }

    fn alive_before(&mut self, until: Instant, waiting: &str) {
        let status = self.child.0.try_wait().unwrap();
        assert!(
            status.is_none() && Instant::now() < until,
            "waiting for {waiting}; child={status:?}; stderr={}",
            fs::read_to_string(self.root.path().join("stderr")).unwrap()
        );
    }

    fn barrier(&self) {
        let mut stream = UnixStream::connect(self.root.path().join("clock.sock")).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        writeln!(stream, "{{\"skip_us\":0}}").unwrap();
        let mut line = String::new();
        BufReader::new(stream.take(1_024))
            .read_line(&mut line)
            .unwrap();
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert!(answer["now_us"].is_u64(), "{answer}");
    }

    fn pulse(&mut self) {
        self.voice
            .publish_json(
                monotonic_us(),
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let stamp = ClockStamp {
            clock_id: CLOCK.into(),
            at_us: monotonic_us(),
            sequence: self.state.totals().published + 1,
        };
        self.state
            .publish_json(
                stamp.at_us,
                &ExpressionState {
                    stamp,
                    enabled: true,
                    turn: None,
                    request: None,
                    behaviours: vec![],
                    missing: vec![],
                    warnings: vec![],
                    body: false,
                    light: false,
                },
            )
            .unwrap();
        if let Some((request, phase)) = &self.reply {
            let value = Receipt {
                stamp: ClockStamp {
                    clock_id: CLOCK.into(),
                    at_us: monotonic_us(),
                    sequence: self.receipt.totals().published + 1,
                },
                expression_session_id: request.expression_session_id.clone(),
                enabled: true,
                request: Some(request.clone()),
                phase: *phase,
            };
            value.check().unwrap();
            self.receipt
                .publish_json(value.stamp.at_us, &value)
                .unwrap();
        }
    }

    fn trigger(&mut self, phase: &str) {
        self.pulse();
        // Drain the fresh resting heartbeat before consuming a daily
        // trigger; bus topics are independent and carry no global order.
        self.barrier();
        self.input
            .publish_json(monotonic_us(), &json!({ "phase": phase }))
            .unwrap();
    }

    fn reply(&mut self, request: &Request, phase: Phase) {
        self.reply = Some((request.clone(), phase));
        self.pulse();
    }

    fn request(&mut self, op: Op, after: u64) -> Request {
        let until = Instant::now() + WAIT;
        loop {
            self.pulse();
            self.alive_before(until, "a matching rest request or renewal");
            if let Some(message) = self.requests.recv(POLL).unwrap() {
                let request: Request = message.parse().unwrap();
                request.check().unwrap();
                assert_eq!(message.header.source, Request::SOURCE);
                assert_eq!(request.skill, SKILL);
                assert_eq!(request.skill_session_id, message.header.boot);
                assert_eq!(request.stamp.sequence, message.header.seq);
                assert_eq!(request.stamp.at_us, message.header.at_us);
                assert_eq!(request.stamp.clock_id, CLOCK);
                assert_eq!(request.expression_session_id, self.state.boot());
                if request.op == op && request.stamp.sequence > after {
                    return request;
                }
                assert_ne!(
                    request.op,
                    Op::Cancel,
                    "routine cancelled while awaiting {op:?}"
                );
            }
        }
    }

    fn done(&mut self) -> SkillEvent {
        let until = Instant::now() + WAIT;
        loop {
            self.pulse();
            self.alive_before(until, "the actual skill run's Done event");
            if let Some(message) = self.events.recv(POLL).unwrap() {
                let event: SkillEvent = message.parse().unwrap();
                assert_eq!(event.skill, SKILL);
                if event.stage == SkillStage::Done {
                    return event;
                }
            }
        }
    }

    fn no_say(&self) {
        match self.say.accept() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            other => panic!("Say was contacted before an owned completion: {other:?}"),
        }
    }

    fn hum(&mut self, first_wake: &Request) {
        self.presentation(
            first_wake,
            json!({"cue":"hum", "priority":5, "expires_in_ms":30_000}),
            "sound",
        );
    }

    fn presentation(&mut self, first_wake: &Request, mut expected: Value, what: &str) {
        let until = Instant::now() + WAIT;
        let mut stream = loop {
            self.pulse();
            self.alive_before(until, "the presentation on the actual Say socket");
            match self.say.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("Say accept failed: {error}"),
            }
            if let Some(message) = self.events.recv(POLL).unwrap() {
                let event: SkillEvent = message.parse().unwrap();
                assert_ne!(
                    event.stage,
                    SkillStage::Done,
                    "ended before presentation: {event:?}"
                );
            }
        };
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let mut line = String::new();
        BufReader::new((&stream).take(8_192))
            .read_line(&mut line)
            .unwrap();
        let ask: Value = serde_json::from_str(&line).unwrap();
        let start_before_us = ask["start_before_us"]
            .as_u64()
            .expect("the presentation keeps the original wake deadline");
        assert!(start_before_us > monotonic_us());
        assert!(
            start_before_us <= first_wake.stamp.at_us + 12_000_000,
            "renewal or completion must not grant another 12 seconds"
        );
        expected["skill"] = json!(SKILL);
        expected["start_before_us"] = json!(start_before_us);
        assert_eq!(ask, expected);
        stream
            .write_all(b"{\"event\":\"begun\"}\n{\"outcome\":\"spoken\"}\n")
            .unwrap();
        assert_eq!(self.done().why, format!("{what}: spoken"));
        self.no_say();
    }

    fn begin_wake(&mut self) -> Request {
        self.trigger("night");
        let rest = self.request(Op::Rest, 0);
        self.no_say();
        self.reply(&rest, Phase::Resting);
        assert_eq!(self.done().allowed, None);
        self.trigger("morning");
        let wake = self.request(Op::Wake, rest.stamp.sequence);
        assert_eq!(wake.episode, rest.episode);
        assert_eq!(wake.skill_session_id, rest.skill_session_id);
        self.reply(&wake, Phase::Waking);
        wake
    }
}

#[test]
fn denied_body_or_speak_never_reaches_expression_or_say() {
    for presentation in [
        "cue = \"hum\"",
        "say = { text = \"Good morning.\", priority = 8, expires = \"1h\" }",
    ] {
        for (body, speak) in [(false, true), (true, false)] {
            let mut fixture = Fixture::with_presentation(body, speak, presentation);
            for phase in ["night", "morning"] {
                fixture.trigger(phase);
                fixture.barrier();
                assert!(fixture.requests.recv(Duration::ZERO).unwrap().is_none());
                fixture.no_say();
            }
            assert!(fixture.bus.subscribers(INPUT).unwrap().is_empty());
            assert!(fixture.events.recv(Duration::ZERO).unwrap().is_none());
        }
    }
}

#[test]
fn the_process_waits_for_its_completed_wake_before_sending_the_hum() {
    let mut fixture = Fixture::new();
    let wake = fixture.begin_wake();
    // A renewal is positive evidence the main loop processed Waking and
    // kept the run waiting. No timing guess is used to assert silence.
    let renewed = fixture.request(Op::Wake, wake.stamp.sequence);
    fixture.no_say();
    fixture.reply(&renewed, Phase::Completed);
    fixture.hum(&wake);
}

#[test]
fn reloading_an_unrelated_disabled_skill_keeps_the_same_admitted_rest() {
    let mut fixture = Fixture::new();
    fixture.trigger("night");
    let rest = fixture.request(Op::Rest, 0);
    fixture.reply(&rest, Phase::Resting);
    assert_eq!(fixture.done().allowed, None);
    let journal = fixture.root.path().join("stderr");
    let loaded = fs::read_to_string(&journal)
        .unwrap()
        .matches("lamp-skills: loaded ")
        .count();
    let other = fixture.root.path().join("skills/unrelated");
    fs::create_dir(&other).unwrap();
    fs::write(
        other.join("SKILL.md"),
        "---\nname: unrelated\ndescription: A disabled fixture.\n---\n",
    )
    .unwrap();
    let manifest = fs::read_to_string(
        fixture
            .root
            .path()
            .join("skills")
            .join(SKILL)
            .join("skill.toml"),
    )
    .unwrap();
    fs::write(
        other.join("skill.toml"),
        format!("enabled = false\n{manifest}"),
    )
    .unwrap();
    let until = Instant::now() + WAIT;
    let mut last = rest.clone();
    while fs::read_to_string(&journal)
        .unwrap()
        .matches("lamp-skills: loaded ")
        .count()
        == loaded
    {
        fixture.alive_before(until, "the unrelated definition reload");
        last = fixture.request(Op::Rest, last.stamp.sequence);
        fixture.reply(&last, Phase::Resting);
    }
    let renewed = fixture.request(Op::Rest, last.stamp.sequence);
    assert_eq!(renewed.episode, rest.episode);
    assert_eq!(renewed.skill_session_id, rest.skill_session_id);
    fixture.no_say();
    fixture.reply(&renewed, Phase::Resting);
    fixture.trigger("morning");
    let wake = fixture.request(Op::Wake, renewed.stamp.sequence);
    assert_eq!(wake.episode, rest.episode);
    fixture.reply(&wake, Phase::Completed);
    fixture.hum(&wake);
}

#[test]
fn exact_words_after_wake_keep_their_priority_and_original_start_deadline() {
    const WORDS: &str = "Good morning. Take a small stretch.";
    let mut fixture = Fixture::with_presentation(
        true,
        true,
        &format!("say = {{ text = \"{WORDS}\", priority = 8, expires = \"1h\" }}"),
    );
    let wake = fixture.begin_wake();
    // Waking is not completion, even for the highest skill priority.
    let renewed = fixture.request(Op::Wake, wake.stamp.sequence);
    fixture.no_say();
    fixture.reply(&renewed, Phase::Completed);
    fixture.presentation(
        &wake,
        json!({"text":WORDS, "priority":8, "expires_in_ms":3_600_000}),
        "say",
    );
}

#[test]
fn cancellation_and_a_replaced_expression_owner_end_the_run_without_a_hum() {
    for replaced_owner in [false, true] {
        let mut fixture = Fixture::new();
        let wake = fixture.begin_wake();
        if replaced_owner {
            let old = fixture.state.boot().to_owned();
            fixture.state = fixture
                .bus
                .publisher_declared(
                    ExpressionState::TOPIC,
                    &ExpressionState::declaration(),
                    "expression",
                )
                .unwrap();
            assert_ne!(fixture.state.boot(), old);
            // The old owner's fresh Completed must lose to the current
            // Expression identity, even when both arrive in the same pass.
            fixture.reply(&wake, Phase::Completed);
        } else {
            fixture.reply(&wake, Phase::Cancelled);
        }
        let done = fixture.done();
        assert_eq!(done.allowed, Some(false), "{done:?}");
        fixture.no_say();
        // A late completion cannot revive a run whose Done was observed.
        fixture.reply(&wake, Phase::Completed);
        fixture.barrier();
        fixture.no_say();
    }
}
