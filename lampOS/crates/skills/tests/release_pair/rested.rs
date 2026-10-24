//! A persisted night is history, never a restored active Expression lease.
//! Every state write/read is made by a pinned released Skills executable.
use super::*;
use lamp_ipc::monotonic_us;
use lamp_msgs::{
    BodyGesture, ClockStamp, ExpressionRestOp as Op, ExpressionRestPhase as Phase,
    ExpressionRestRequest as Request, ExpressionRestState as Receipt, ExpressionState,
    GestureState, Voice, VoiceState,
};
use std::collections::VecDeque;

const SKILL: &str = "rested-check";
const CLOCK: &str = "rested-release-pair-host";
const STALE_US: u64 = 1_000_000;

struct Running {
    process: OwnedChild,
    clock: PathBuf,
    traps: Traps,
    offset: i32,
    day: i64,
}

struct Fixture {
    // Kill/reap a failing child before releasing any of its private peers.
    running: Option<Running>,
    case: Case,
    morning_offset: i32,
    manifests: BTreeMap<String, Vec<u8>>,
    bus: Bus,
    input: Publisher,
    voice: Publisher,
    state: Publisher,
    receipt: Publisher,
    body: Publisher,
    requests: Subscriber,
    events: Subscriber,
    pending_requests: VecDeque<Request>,
    pending_events: Vec<SkillEvent>,
    reply: Option<(Request, bool)>,
    fresh_owner: bool,
    stretches: usize,
}

impl Fixture {
    fn new(writer: Release, reader: Release) -> Self {
        assert_eq!(std::env::consts::OS, "linux", "manual native fixture");
        assert_eq!(std::env::consts::ARCH, "aarch64", "manual native fixture");
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute() && parent.is_dir());
        let root = tempfile::Builder::new()
            .prefix("kn-")
            .tempdir_in(parent)
            .unwrap()
            .keep();
        assert!(root.as_os_str().as_encoded_bytes().len() <= 40);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        directory(&root.join("skills"));
        directory(&root.join("state"));
        writer.copy(&root);
        reader.copy(&root);

        // Derive the pair jointly. Choosing two independent nearest offsets
        // can map morning to the wrong date when UTC is near noon.
        let wall = wall_ms();
        let minute = (wall / 60_000).rem_euclid(1440);
        let offset = ((1410 - minute + 720).rem_euclid(1440) - 720) as i32;
        let morning_offset = offset + 60;
        assert!((-720..=840).contains(&offset));
        assert!((-720..=840).contains(&morning_offset));
        let night = wall + i64::from(offset) * 60_000;
        let morning = wall + i64::from(morning_offset) * 60_000;
        let day = night.div_euclid(86_400_000);
        assert_eq!((night / 60_000).rem_euclid(1440), 1410);
        assert_eq!((morning / 60_000).rem_euclid(1440), 30);
        assert_eq!(morning.div_euclid(86_400_000), day + 1);
        json_file(
            &root.join("case.json"),
            &json!({
                "kind":"rested-night","writer":writer.number(),"reader":reader.number(),
                "return_reader":writer.number(),"writer_sha256":writer.pin().0,
                "reader_sha256":reader.pin().0,"writer_bytes":writer.pin().1,
                "reader_bytes":reader.pin().1,"writer_source":writer.pin().2,
                "reader_source":reader.pin().2,"sampled_unix_ms":wall,
                "night_utc_offset_minutes":offset,"morning_utc_offset_minutes":morning_offset,
                "night_day":day,"morning_day":day+1,"clock_id":CLOCK,
                "night_trigger_window":{"from":"23:00","to":"00:00"},
                "rest_window":{"from":"23:00","to":"01:00"},
                "morning_trigger_window":{"from":"00:00","to":"01:00"},
                "boundary":"actual released rested/daily readers and writers; synthetic Expression and Body receipts",
                "excluded":["physical output","provider","updater install","clock synchronization","power-loss durability"]
            }),
        );
        println!("skills rested release-pair evidence: {}", root.display());
        let mut case = Case {
            transcript: output(&root.join("transcript.jsonl")),
            root,
            records: 0,
            deadline: Instant::now() + Duration::from_secs(90),
            offset,
            day,
        };
        let folder = case.root.join("skills").join(SKILL);
        directory(&folder);
        let mut manifest = format!(
            "kind = \"core\"\nversion = \"1.0.0\"\nenabled = true\n\n[permissions]\ntopics = [\"{INPUT}\", \"voice/state\"]\nbody = true\n\n"
        );
        for phase in [
            "night_stale",
            "night_unmatched",
            "night",
            "morning",
            "probe",
        ] {
            manifest.push_str(&format!(
                "[[trigger]]\nname = \"{phase}\"\ntopic = \"{INPUT}\"\nfield = \"phase\"\nequals = \"{phase}\"\nrearm = \"ends\"\n"
            ));
            if phase != "probe" {
                let (from, to) = if phase == "morning" {
                    ("00:00", "01:00")
                } else {
                    ("23:00", "00:00")
                };
                manifest.push_str(&format!(
                    "during = {{ from = \"{from}\", to = \"{to}\" }}\nonce_per_day = true\n"
                ));
            }
            manifest.push('\n');
        }
        manifest.push_str("[ask]\npolicy = \"nudge\"\n\n");
        for phase in [
            "night_stale",
            "night_unmatched",
            "night",
            "morning",
            "probe",
        ] {
            let rest = match phase {
                "morning" => "{ op = \"wake\" }",
                "probe" => "{ op = \"cancel\" }",
                _ => "{ op = \"rest\", during = { from = \"23:00\", to = \"01:00\" } }",
            };
            manifest.push_str(&format!("[[do]]\non = \"{phase}\"\nrest = {rest}\n\n"));
        }
        let manifests = BTreeMap::from([
            ("skill.toml".into(), manifest.into_bytes()),
            (
                "SKILL.md".into(),
                format!(
                    "---\nname: {SKILL}\ndescription: A synthetic persisted night check.\n---\n"
                )
                .into_bytes(),
            ),
        ]);
        for (name, bytes) in &manifests {
            write(&folder.join(name), bytes);
            case.record(json!({"event":"manifest_frozen","file":name,"sha256":sha(bytes),"bytes":bytes.len()}));
        }
        let bus = Bus::new(case.root.join("b"))
            .unwrap()
            .rescan_every(Duration::ZERO);
        let input = bus
            .publisher_declared(
                INPUT,
                &Declaration::new("fixture/Phase", 1, Kind::Latest),
                "fixture",
            )
            .unwrap();
        let voice = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let state = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        let receipt = bus
            .publisher_declared(Receipt::TOPIC, &Receipt::declaration(), "expression")
            .unwrap();
        let body = bus
            .publisher_declared(BodyGesture::TOPIC, &BodyGesture::declaration(), "body")
            .unwrap();
        let requests = bus.subscribe(Request::TOPIC, "pair").unwrap();
        let events = bus.subscribe(SkillEvent::TOPIC, "pair").unwrap();
        let mut fixture = Self {
            running: None,
            case,
            morning_offset,
            manifests,
            bus,
            input,
            voice,
            state,
            receipt,
            body,
            requests,
            events,
            pending_requests: VecDeque::new(),
            pending_events: Vec::new(),
            reply: None,
            fresh_owner: true,
            stretches: 0,
        };
        fixture
            .input
            .publish_json(monotonic_us(), &json!({"phase":"idle"}))
            .unwrap();
        fixture.pulse();
        fixture
    }

    fn unchanged_manifests(&self) {
        for (name, expected) in &self.manifests {
            let actual = fs::read(self.case.root.join("skills").join(SKILL).join(name)).unwrap();
            assert_eq!(&actual, expected, "immutable skill file {name}");
        }
    }

    fn clock_unchanged(&self) {
        assert!(Instant::now() < self.case.deadline, "case deadline");
        let running = self.running.as_ref().unwrap();
        let local = wall_ms() + i64::from(running.offset) * 60_000;
        assert_eq!(local.div_euclid(86_400_000), running.day);
        let minute = (local / 60_000).rem_euclid(1440);
        let window = if running.day == self.case.day {
            1400..1420
        } else {
            20..40
        };
        assert!(window.contains(&minute), "left frozen night/morning margin");
    }

    fn pulse(&mut self) {
        let at = monotonic_us();
        self.voice
            .publish_json(
                at,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let state = ExpressionState {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: if self.fresh_owner {
                    at
                } else {
                    at.saturating_sub(STALE_US)
                },
                sequence: self.state.totals().published + 1,
            },
            enabled: true,
            turn: None,
            request: None,
            behaviours: vec![],
            missing: vec![],
            warnings: vec![],
            body: false,
            light: false,
            asleep: None,
        };
        state.check().unwrap();
        self.state.publish_json(at, &state).unwrap();
        let value = Receipt {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: if self.reply.as_ref().is_some_and(|(_, stale)| *stale) {
                    at.saturating_sub(STALE_US)
                } else {
                    at
                },
                sequence: self.receipt.totals().published + 1,
            },
            expression_session_id: self.state.boot().into(),
            enabled: true,
            request: self.reply.as_ref().map(|(request, _)| request.clone()),
            phase: if self.reply.is_some() {
                Phase::Resting
            } else {
                Phase::Idle
            },
            reason: None,
        };
        value.check().unwrap();
        // The transport is fresh even in the stale-payload counterexample.
        self.receipt.publish_json(at, &value).unwrap();
    }

    fn reply(&mut self, request: Request, stale: bool) {
        self.case.record(json!({"event":"expression_reply_selected","request":request,"payload_age_us":if stale { STALE_US } else { 0 }}));
        self.reply = Some((request, stale));
        self.pulse();
        self.owner_evidence("reply_published");
    }

    fn owner_evidence(&mut self, boundary: &str) {
        for topic in [ExpressionState::TOPIC, Receipt::TOPIC] {
            let message = self.bus.latest(topic).unwrap().unwrap();
            self.case.record(json!({
                "event":"synthetic_expression_evidence","boundary":boundary,"topic":topic,
                "source":message.header.source,"publisher":message.header.boot,
                "sequence":message.header.seq,"at_us":message.header.at_us,
                "payload":message.json().unwrap()
            }));
        }
    }

    fn spawn(&mut self, release: Release, stage: &str, morning: bool) {
        assert!(self.running.is_none());
        self.unchanged_manifests();
        assert!(self.pending_events.is_empty() && self.pending_requests.is_empty());
        // Process sockets must not share the bus root, named `b`.
        let private = self.case.root.join(format!("child-{stage}"));
        directory(&private);
        let traps = Traps::new(&private);
        let offset = if morning {
            self.morning_offset
        } else {
            self.case.offset
        };
        let mut command = Command::new(self.case.binary(release));
        command
            .env_clear()
            .env("LAMP_SKILL_ROUTINES", "on")
            .env("LAMP_SKILLS_ASK", "skip")
            .env("LAMP_SKILLS_DAILY_FILE", "state/daily.json")
            .args([
                "--dir",
                "skills",
                "--state-dir",
                "state",
                "--trust-clock",
                "--boot-id",
                CLOCK,
                "--utc-offset-minutes",
            ])
            .arg(offset.to_string())
            .arg("--bus-root")
            .arg(self.case.root.join("b"))
            .arg("--clock-control")
            .arg(private.join("clock"))
            .arg("--gaze-socket")
            .arg(private.join("gaze"))
            .arg("--mood-socket")
            .arg(private.join("mood"))
            .arg("--actions-socket")
            .arg(private.join("actions"))
            .arg("--say-socket")
            .arg(private.join("say"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(output(
                &self.case.root.join(format!("{stage}.stdout")),
            )));
        let process = spawn_recorded(&mut self.case, release, stage, command);
        self.running = Some(Running {
            process,
            clock: private.join("clock"),
            traps,
            offset,
            day: self.case.day + i64::from(morning),
        });
        let until = self.case.within();
        loop {
            self.progress(until, false);
            let ready = self.running.as_ref().unwrap().clock.exists()
                && [INPUT, VoiceState::TOPIC].iter().all(|topic| {
                    self.bus
                        .subscribers(topic)
                        .unwrap()
                        .iter()
                        .any(|listener| listener.listening)
                });
            if ready {
                break;
            }
            std::thread::sleep(PAUSE);
        }
        self.barrier();
        self.collect();
        assert!(
            self.pending_events.is_empty() && self.pending_requests.is_empty(),
            "restart must not restore active ownership"
        );
        if morning {
            let expected = format!("[(\"{SKILL}\", {})]", self.case.day);
            assert!(
                self.running
                    .as_ref()
                    .unwrap()
                    .process
                    .log()
                    .contains(&expected),
                "released reader reported the exact persisted night"
            );
        }
    }

    fn collect(&mut self) {
        let stage = &self.running.as_ref().unwrap().process.stage;
        while let Some(message) = self.requests.recv(Duration::ZERO).unwrap() {
            assert_eq!(message.gap, 0, "lost rest request");
            let request: Request = message.parse().unwrap();
            request.check().unwrap();
            assert_eq!(message.header.source, Request::SOURCE);
            assert_eq!(request.skill, SKILL);
            assert_eq!(request.skill_session_id, message.header.boot);
            assert_eq!(request.stamp.sequence, message.header.seq);
            assert_eq!(request.stamp.at_us, message.header.at_us);
            assert_eq!(request.stamp.clock_id, CLOCK);
            assert_eq!(request.expression_session_id, self.state.boot());
            self.case
                .record(json!({"event":"expression_request","stage":stage,"request":request}));
            self.pending_requests.push_back(request);
            assert!(self.pending_requests.len() < 32);
        }
        while let Some(message) = self.events.recv(Duration::ZERO).unwrap() {
            assert_eq!(message.gap, 0, "lost Skills event");
            assert_eq!(message.header.source, "skills");
            let event: SkillEvent = message.parse().unwrap();
            assert_eq!(event.skill, SKILL);
            self.case
                .record(json!({"event":"skills_event","stage":stage,
                "publisher":message.header.boot,"sequence":message.header.seq,
                "at_us":message.header.at_us,"message":event}));
            self.pending_events.push(event);
            assert!(self.pending_events.len() < 32);
        }
    }

    fn progress(&mut self, until: Instant, expect_stretch: bool) {
        self.clock_unchanged();
        self.pulse();
        self.running
            .as_mut()
            .unwrap()
            .process
            .alive(until, "released routine progress");
        self.collect();
        let traps = &self.running.as_ref().unwrap().traps;
        for (index, socket) in traps.datagrams.iter().enumerate() {
            if expect_stretch && index == 1 {
                continue;
            }
            let mut bytes = [0; 4096];
            assert!(
                matches!(socket.recv(&mut bytes), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "unexpected output datagram"
            );
        }
        for socket in &traps.streams {
            assert!(
                matches!(socket.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "unexpected Actions/Say connection"
            );
        }
    }

    fn barrier(&mut self) {
        let until = self.case.within();
        self.clock_unchanged();
        self.pulse();
        let running = self.running.as_mut().unwrap();
        running
            .process
            .alive(self.case.within(), "input drain barrier");
        let mut stream = UnixStream::connect(&running.clock).unwrap();
        stream.set_read_timeout(Some(IO_WITHIN)).unwrap();
        stream.set_write_timeout(Some(IO_WITHIN)).unwrap();
        let deadline = until.min(Instant::now() + IO_WITHIN);
        stream.write_all(b"{\"skip_us\":0}\n").unwrap();
        let answer = read_answer(stream, 1024, deadline);
        assert!(answer["now_us"].is_u64());
        // This orders input only. Admission/ownership use actual requests,
        // receipts and Done below, since Manager::tick follows this reply.
        self.case
            .record(json!({"event":"input_barrier","stage":running.process.stage,"answer":answer}));
    }

    fn input(&mut self, phase: &str) {
        self.barrier();
        let sent = self
            .input
            .publish_json(monotonic_us(), &json!({"phase":phase}))
            .unwrap();
        assert_eq!(sent.delivered, 1);
        assert_eq!(sent.dropped, 0);
        self.case.record(json!({"event":"input","stage":self.running.as_ref().unwrap().process.stage,"phase":phase,"sequence":self.input.totals().published}));
        self.barrier();
        self.collect();
    }

    fn trigger(&mut self, phase: &str) {
        self.collect();
        assert!(self.pending_events.is_empty() && self.pending_requests.is_empty());
        self.input(phase);
    }

    fn request(&mut self, op: Op) -> Request {
        let until = self.case.within();
        loop {
            self.progress(until, false);
            if let Some(request) = self.pending_requests.pop_front() {
                assert_eq!(request.op, op, "unexpected routine operation");
                return request;
            }
            assert!(
                !self
                    .pending_events
                    .iter()
                    .any(|event| event.stage == SkillStage::Done),
                "run ended before its Expression request: {:?}",
                self.pending_events
            );
            std::thread::sleep(PAUSE);
        }
    }

    fn done(&mut self, allowed: Option<bool>, why: &str) {
        let until = self.case.within();
        loop {
            self.progress(until, false);
            if self
                .pending_events
                .iter()
                .any(|event| event.stage == SkillStage::Done)
            {
                break;
            }
            std::thread::sleep(PAUSE);
        }
        let events = std::mem::take(&mut self.pending_events);
        assert_eq!(
            events.iter().map(|event| event.stage).collect::<Vec<_>>(),
            [
                SkillStage::Triggered,
                SkillStage::Asked,
                SkillStage::Answered,
                SkillStage::Done
            ],
            "one complete actual run: {events:?}"
        );
        assert_eq!(events[2].allowed, Some(true), "explicit fixture skip mode");
        assert!(
            events[2]
                .why
                .starts_with("the ask is skipped (LAMP_SKILLS_ASK=skip)")
        );
        assert_eq!(events[3].allowed, allowed, "{events:?}");
        if why == "unconfirmed" {
            assert!(
                matches!(
                    events[3].why.as_str(),
                    "rest transition was not confirmed in time" | "rest was cancelled or refused"
                ),
                "{events:?}"
            );
        } else {
            assert_eq!(events[3].why, why);
        }
    }

    fn neutral(&mut self, phase: &str, daily: bool) {
        assert!(self.pending_events.is_empty());
        self.input("idle");
        if daily {
            assert_eq!(
                self.pending_events.len(),
                1,
                "only the consumed watch reset"
            );
            let reset = self.pending_events.pop().unwrap();
            assert_eq!(reset.stage, SkillStage::Answered);
            assert_eq!(reset.allowed, Some(false));
            let prefix = format!("{phase} watch reset after ");
            let suffix = format!(" s: stopped holding: {INPUT} phase no longer {phase}");
            let elapsed = reset
                .why
                .strip_prefix(&prefix)
                .and_then(|line| line.strip_suffix(&suffix))
                .expect("exact phase-specific watch reset");
            assert!(elapsed.parse::<u64>().is_ok());
        } else {
            assert!(self.pending_events.is_empty());
        }
    }

    fn declined_rest(&mut self, phase: &str, stale: bool) {
        self.trigger(phase);
        let request = self.request(Op::Rest);
        let mut wrong = request.clone();
        if !stale {
            wrong.episode += 1;
        }
        self.reply(wrong, stale);
        self.done(Some(false), "unconfirmed");
        let cancel = self.request(Op::Cancel);
        same_episode(&request, &cancel);
        assert!(cancel.stamp.sequence > request.stamp.sequence);
        assert!(
            !self.case.root.join("state/rested").exists(),
            "no night without a matched fresh admission"
        );
        self.reply = None;
        self.pulse();
        self.neutral(phase, true);
        assert!(self.pending_requests.is_empty());
    }

    fn probe(&mut self, fresh: bool) {
        self.fresh_owner = fresh;
        self.pulse();
        self.case.record(json!({"event":"probe_owner_freshness","fresh":fresh,"owner":self.state.boot(),"stamp_age_us":if fresh { 0 } else { STALE_US }}));
        self.owner_evidence("cancel_probe");
        self.trigger("probe");
        if fresh {
            self.done(None, "");
        } else {
            self.done(Some(false), "Expression is not fresh and enabled");
        }
        assert!(
            self.pending_requests.is_empty(),
            "no active rest was restored"
        );
        self.neutral("probe", false);
    }

    fn stretch(&mut self) {
        let until = self.case.within();
        loop {
            self.progress(until, true);
            assert!(
                self.pending_requests.is_empty(),
                "persisted history must not rebind an active lease"
            );
            let mut bytes = [0; 4096];
            match self.running.as_ref().unwrap().traps.datagrams[1].recv(&mut bytes) {
                Ok(length) => {
                    let ask: Value = serde_json::from_slice(&bytes[..length]).unwrap();
                    assert_eq!(ask, json!({"gesture":"stretching"}));
                    self.stretches += 1;
                    assert_eq!(self.stretches, 1, "one morning stretch in the entire case");
                    self.case
                        .record(json!({"event":"private_body_datagram","request":ask}));
                    let over = BodyGesture {
                        name: "stretching".into(),
                        state: GestureState::Over,
                        source: None,
                        why: None,
                        by: None,
                    };
                    let sent = self.body.publish_json(monotonic_us(), &over).unwrap();
                    assert_eq!(
                        sent.delivered, 1,
                        "actual run subscribed before sending its gesture"
                    );
                    assert_eq!(sent.dropped, 0);
                    self.case.record(json!({"event":"synthetic_body_over","message":over,"delivered":sent.delivered}));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("private Body datagram: {error}"),
            }
            assert!(
                !self
                    .pending_events
                    .iter()
                    .any(|event| event.stage == SkillStage::Done),
                "morning ended without its stretch: {:?}",
                self.pending_events
            );
            std::thread::sleep(PAUSE);
        }
    }

    fn snapshot(&mut self, name: &str) -> BTreeMap<String, Vec<u8>> {
        self.unchanged_manifests();
        let dest = self.case.root.join(name);
        directory(&dest);
        let mut kept = BTreeMap::new();
        let mut files = Vec::new();
        for name in ["daily.json", "rested", "on", "off"] {
            let path = self.case.root.join("state").join(name);
            match fs::read(&path) {
                Ok(bytes) => {
                    assert!(bytes.len() <= 1024 * 1024);
                    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o7777;
                    if name == "daily.json" {
                        assert_eq!(mode, 0o600);
                    }
                    write(&dest.join(name), &bytes);
                    files.push(
                        json!({"file":name,"bytes":bytes.len(),"mode":mode,"sha256":sha(&bytes)}),
                    );
                    kept.insert(name.into(), bytes);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    files.push(json!({"file":name,"absent":true}))
                }
                Err(error) => panic!("state snapshot: {error}"),
            }
        }
        for (name, bytes) in &self.manifests {
            write(&dest.join(name), bytes);
            files.push(
                json!({"file":name,"bytes":bytes.len(),"sha256":sha(bytes),"unchanged":true}),
            );
        }
        json_file(&dest.join("files.json"), &json!(files));
        self.case
            .record(json!({"event":"snapshot","name":name,"files":files}));
        kept
    }

    fn stop(&mut self, active: Option<&Request>) {
        self.barrier();
        self.running.as_mut().unwrap().process.kill(&mut self.case);
        self.collect();
        assert!(self.pending_events.is_empty());
        for request in self.pending_requests.drain(..) {
            let previous = active.expect("no active ownership in a restarted process");
            same_episode(previous, &request);
            assert_eq!(request.op, Op::Rest);
            assert!(request.stamp.sequence > previous.stamp.sequence);
        }
        let running = self.running.take().unwrap();
        running.traps.empty();
        let log = running.process.log();
        for failure in [
            "not loaded:",
            "daily chances disabled:",
            "not kept in",
            "the nights rested not read:",
        ] {
            assert!(!log.contains(failure), "{log}");
        }
        self.unchanged_manifests();
    }
}

fn same_episode(first: &Request, next: &Request) {
    assert_eq!(next.skill, first.skill);
    assert_eq!(next.skill_session_id, first.skill_session_id);
    assert_eq!(next.expression_session_id, first.expression_session_id);
    assert_eq!(next.episode, first.episode);
    assert_eq!(next.stamp.clock_id, first.stamp.clock_id);
}

fn claimed_days(bytes: &[u8], expected: &[(&str, i64)]) {
    // Raw documented bitmaps, never today's Journal standing in for either
    // released reader. The failed admissions also consumed their daily chance.
    let value: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(value["schema"], 2);
    let entries = value["consumed"].as_array().unwrap();
    assert_eq!(entries.len(), expected.len());
    let mut found = BTreeMap::new();
    for entry in entries {
        assert_eq!(entry["skill"], SKILL);
        let name = entry["trigger"].as_str().unwrap();
        let hex = entry["days"].as_str().unwrap().as_bytes();
        assert!(hex.len().is_multiple_of(2));
        let mut days = Vec::new();
        for (index, pair) in hex.chunks_exact(2).enumerate() {
            let byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    days.push(18_260 + (index * 8 + bit) as i64);
                }
            }
        }
        assert!(found.insert(name, days).is_none());
    }
    let expected: BTreeMap<_, _> = expected
        .iter()
        .map(|(name, day)| (*name, vec![*day]))
        .collect();
    assert_eq!(found, expected);
}

pub fn check(writer: Release, reader: Release) {
    let mut fixture = Fixture::new(writer, reader);
    let day = fixture.case.day;
    let expected_night = format!("{SKILL} {day}\n").into_bytes();
    fixture.spawn(writer, "a", false);
    fixture.declined_rest("night_stale", true);
    fixture.declined_rest("night_unmatched", false);
    fixture.trigger("night");
    let accepted = fixture.request(Op::Rest);
    fixture.reply(accepted.clone(), false);
    fixture.done(None, "");
    let until = fixture.case.within();
    loop {
        fixture.progress(until, false);
        match fs::read(fixture.case.root.join("state/rested")) {
            Ok(bytes) => {
                assert_eq!(bytes, expected_night);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => std::thread::sleep(PAUSE),
            Err(error) => panic!("persisted admission: {error}"),
        }
    }
    let renewed = fixture.request(Op::Rest);
    same_episode(&accepted, &renewed);
    assert!(renewed.stamp.sequence > accepted.stamp.sequence);
    assert!(renewed.stamp.at_us >= accepted.stamp.at_us + 1_000_000);
    fixture.reply(renewed.clone(), false);
    fixture.case.record(json!({"event":"admitted_night_before_cut","rested_sha256":sha(&expected_night),"request":renewed,"owner":fixture.state.boot()}));
    fixture.neutral("night", true);
    fixture.stop(Some(&renewed));
    let a = fixture.snapshot("after-a");
    assert_eq!(a.len(), 2, "only daily.json and rested are written");
    assert_eq!(a["rested"], expected_night);
    claimed_days(
        &a["daily.json"],
        &[
            ("night_stale", day),
            ("night_unmatched", day),
            ("night", day),
        ],
    );

    // Keep the same fake Expression publisher and fresh old-A receipt.
    // Neither can make a new Skills process own the pre-cut episode.
    fixture.spawn(reader, "b", true);
    fixture.probe(true);
    assert_eq!(fixture.snapshot("b-read-only-probe"), a);
    fixture.trigger("morning");
    fixture.stretch();
    fixture.done(None, "gesture stretching: told the body");
    assert!(fixture.pending_requests.is_empty());
    fixture.neutral("morning", true);
    fixture.stop(None);
    let b = fixture.snapshot("after-b");
    assert_eq!(b.len(), 2, "no on/off side effects");
    assert_eq!(b["rested"], a["rested"]);
    claimed_days(
        &b["daily.json"],
        &[
            ("night_stale", day),
            ("night_unmatched", day),
            ("night", day),
            ("morning", day + 1),
        ],
    );

    fixture.spawn(writer, "a-return", true);
    fixture.trigger("morning");
    let until = fixture.case.within();
    loop {
        fixture.progress(until, false);
        assert!(fixture.pending_requests.is_empty() && fixture.pending_events.is_empty());
        if fixture
            .running
            .as_ref()
            .unwrap()
            .process
            .log()
            .contains("rested-check not run: its day already claimed")
        {
            break;
        }
        std::thread::sleep(PAUSE);
    }
    fixture.case.record(
        json!({"event":"morning_claim_refused_on_return","reason":"its day already claimed"}),
    );
    assert_eq!(fixture.snapshot("return-suppressed"), b);
    fixture.neutral("morning", true);
    // A live Manager needs fresh Expression readiness even for a no-owner
    // cancel. This is a positive run barrier after daily suppression.
    fixture.probe(false);
    fixture.probe(true);
    fixture.stop(None);
    assert_eq!(fixture.snapshot("after-a-return"), b);
    assert_eq!(fixture.stretches, 1);
    fixture.case.finish();
}
