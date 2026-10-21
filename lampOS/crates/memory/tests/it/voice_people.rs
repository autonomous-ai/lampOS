//! Each person's memory kept apart (#764): Voice's clients, as a known
//! person at the lamp uses them, against the real store on a socket, in both
//! modes. Two known people and the room; neither is ever answered from the
//! other's scope. All content is synthetic.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Person, Server, Store,
    days::Days,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, FORGET, RECALL, REMEMBER, person_id, run_for};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_millis(1500);
/// A session note's deadline here: what is told, not the voice's 300 ms,
/// which a loaded machine can miss (Voice's own tests bound that).
const SESSION: Duration = Duration::from_secs(10);
static NEXT: AtomicU64 = AtomicU64::new(0);

/// The real store, with each request line and its answer traced.
struct Traced<M> {
    store: M,
    trace: Sender<(Value, Value)>,
}

impl<M: Memory> Memory for Traced<M> {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = self.store.answer(line, now);
        let _ = self.trace.send((
            serde_json::from_slice(line).unwrap(),
            serde_json::from_str(&answer.line).unwrap(),
        ));
        answer
    }
}

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    now: u64,
    trace: Receiver<(Value, Value)>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new<M: Memory + Send + 'static>(open: impl FnOnce(Options, u64) -> M) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-mp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let store = open(
            Options {
                dir: root.join("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(
                    lamp_voice::local_time::offset_here().unwrap_or(0) / 60,
                ),
            },
            now,
        );
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let (tx, trace) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut memory = Traced { store, trace: tx };
            server.run(&mut memory, &mut || now, &mut |_| {}).unwrap();
        });
        Self {
            root,
            socket,
            now,
            trace,
            stop,
            worker: Some(worker),
        }
    }

    /// The next traced request with this op, and the store's answer.
    fn next(&self, op: &str) -> (Value, Value) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let event = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if event.0["op"] == op {
                return event;
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.stop();
        self.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "voice-people-test".into(),
        name: name.into(),
        args,
    }
}

/// Every text a recall told the model, with whether it was marked theirs.
fn found(told: &Value) -> Vec<(String, bool)> {
    told["found"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let text = ["asked_to_remember", "you_said", "recorded_observation"]
                .iter()
                .find_map(|field| item[*field].as_str())
                .unwrap();
            (text.to_owned(), item["theirs"] == true)
        })
        .collect()
}

fn texts(told: &Value) -> Vec<String> {
    found(told).into_iter().map(|(text, _)| text).collect()
}

/// What each of Minh, Sam and nobody known is told from a recall about
/// "coffee" asked through `recall`.
fn recalls(recall: impl Fn(Option<&str>) -> (Value, bool)) -> [Value; 3] {
    let (minh, sam) = (person_id("Minh"), person_id("Sam"));
    [Some(minh.as_str()), Some(sam.as_str()), None].map(|person| {
        let (told, answered) = recall(person);
        assert!(answered, "{told}");
        told
    })
}

#[test]
fn legacy_two_known_people_and_the_room_are_never_answered_from_each_other() {
    let fixture = Fixture::new(|options, now| Store::open(options, now).unwrap());
    let (minh, sam) = (person_id("Minh"), person_id("Sam"));
    for (person, text) in [
        (Some(minh.as_str()), "Minh takes coffee black"),
        (Some(sam.as_str()), "Sam takes coffee with milk"),
        (None, "the coffee machine is on the left"),
    ] {
        let (told, ok) = run_for(
            &fixture.socket,
            &call(REMEMBER, json!({"text":text})),
            BUDGET,
            person,
        );
        assert!(ok && told["remembered"] == true, "{told}");
        assert_eq!(fixture.next("remember").1["ok"], true);
    }
    let [to_minh, to_sam, to_room] = recalls(|person| {
        run_for(
            &fixture.socket,
            &call(RECALL, json!({"about":"coffee"})),
            BUDGET,
            person,
        )
    });
    assert_eq!(
        found(&to_minh),
        [
            ("Minh takes coffee black".into(), true),
            ("the coffee machine is on the left".into(), false)
        ]
    );
    assert_eq!(
        found(&to_sam),
        [
            ("Sam takes coffee with milk".into(), true),
            ("the coffee machine is on the left".into(), false)
        ]
    );
    assert_eq!(texts(&to_room), ["the coffee machine is on the left"]);
    // Minh forgetting the last thing they asked to keep is Minh's alone.
    let (told, ok) = run_for(
        &fixture.socket,
        &call(FORGET, json!({})),
        BUDGET,
        Some(&minh),
    );
    assert!(ok && told["forgotten"] == true, "{told}");
    let [to_minh, to_sam, _] = recalls(|person| {
        run_for(
            &fixture.socket,
            &call(RECALL, json!({"about":"coffee"})),
            BUDGET,
            person,
        )
    });
    assert_eq!(texts(&to_minh), ["the coffee machine is on the left"]);
    assert_eq!(
        texts(&to_sam),
        [
            "Sam takes coffee with milk",
            "the coffee machine is on the left"
        ]
    );
}

#[test]
fn days_two_known_people_and_the_room_are_never_answered_from_each_other() {
    let fixture = Fixture::new(|options, now| {
        let mut store = Days::open(options, now).unwrap();
        store
            .remember(
                &Person::everyone(),
                now,
                "the coffee machine is on the left",
            )
            .unwrap();
        store
    });
    let (minh, sam) = (person_id("Minh"), person_id("Sam"));
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    for (person, text) in [
        (minh.as_str(), "Minh takes coffee black"),
        (sam.as_str(), "Sam takes coffee with milk"),
    ] {
        let (told, ok) =
            client.run_for(&call(REMEMBER, json!({"text":text})), BUDGET, Some(person));
        assert!(ok && told["remembered"] == true, "{told}");
    }
    // A session opened with Minh at the lamp is told the room's and Minh's.
    let started = Instant::now();
    let Some((room, theirs)) = client.open_session_within(1, SESSION, Some(&minh)) else {
        let seen: Vec<_> = fixture.trace.try_iter().collect();
        panic!(
            "no session after {:?}; the store saw {seen:?}",
            started.elapsed()
        );
    };
    let theirs = theirs.unwrap();
    assert!(room["note"].as_str().unwrap().contains("coffee machine"));
    assert!(theirs["note"].as_str().unwrap().contains("Minh takes"));
    for note in [&room, &theirs] {
        assert!(!note["note"].as_str().unwrap().contains("Sam"), "{note}");
    }
    assert!(!room["note"].as_str().unwrap().contains("Minh"));
    // Each one's exchange goes to their own journal, linked to their own.
    assert!(client.finished_exchange_for(
        fixture.now - 2_000,
        "how do I take my coffee",
        "black, Minh",
        Some(&minh)
    ));
    assert!(client.finished_exchange_for(
        fixture.now - 1_000,
        "and my coffee",
        "with milk, Sam",
        Some(&sam)
    ));
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(
        (line["person"].as_str(), &answer["ok"]),
        (Some(minh.as_str()), &json!(true))
    );
    assert_eq!(
        line["source_ids"],
        json!([1]),
        "Minh's kept thing, in Minh's numbering"
    );
    // Sam coming to the lamp ends Minh's conversation, in Minh's scope.
    let (line, answer) = fixture.next("conversation_end");
    assert_eq!(line["person"].as_str(), Some(minh.as_str()));
    assert_eq!(answer["ok"], true, "{answer} {line}");
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(
        (line["person"].as_str(), &answer["ok"]),
        (Some(sam.as_str()), &json!(true))
    );
    assert_eq!(
        line["source_ids"],
        json!([1]),
        "Sam's own kept thing, in Sam's numbering"
    );
    let [to_minh, to_sam, to_room] =
        recalls(|person| client.run_for(&call(RECALL, json!({"about":"coffee"})), BUDGET, person));
    let minh_said = texts(&to_minh);
    let sam_said = texts(&to_sam);
    assert!(
        minh_said.contains(&"Minh takes coffee black".into()),
        "{to_minh}"
    );
    assert!(minh_said.contains(&"black, Minh".into()), "{to_minh}");
    assert!(
        sam_said.contains(&"Sam takes coffee with milk".into()),
        "{to_sam}"
    );
    assert!(sam_said.contains(&"with milk, Sam".into()), "{to_sam}");
    for (told, other) in [(&to_minh, "Sam"), (&to_sam, "Minh")] {
        assert!(
            texts(told).iter().all(|text| !text.contains(other)),
            "{told}"
        );
        assert!(texts(told).contains(&"the coffee machine is on the left".into()));
        // Theirs first, the room's after, unmarked.
        let marks: Vec<bool> = found(told).into_iter().map(|(_, theirs)| theirs).collect();
        assert!(marks.windows(2).all(|pair| pair[0] >= pair[1]), "{told}");
        assert_eq!(marks.last(), Some(&false));
    }
    assert_eq!(texts(&to_room), ["the coffee machine is on the left"]);
    assert_eq!(to_room["scope"], "said in this room; speaker unknown");
    // Minh forgetting is Minh's alone.
    let (told, ok) = client.run_for(&call(FORGET, json!({})), BUDGET, Some(&minh));
    assert!(ok && told["forgotten"] == true, "{told}");
    let [to_minh, to_sam, _] =
        recalls(|person| client.run_for(&call(RECALL, json!({"about":"coffee"})), BUDGET, person));
    assert!(!texts(&to_minh).contains(&"Minh takes coffee black".into()));
    assert!(texts(&to_sam).contains(&"Sam takes coffee with milk".into()));
}

/// #813: a correction kept in one session is told in the next, to that
/// person only; listing and forgetting are theirs alone; a recall never
/// returns a rule. Voice's Days client against the real store.
#[test]
fn days_a_rule_is_told_in_the_next_session_to_that_person_only_until_it_is_forgotten() {
    use lamp_voice::rules::{FORGET_RULE, KEEP_RULE, LIST_RULES};
    let fixture = Fixture::new(|options, now| Days::open(options, now).unwrap());
    let (minh, sam) = (person_id("Minh"), person_id("Sam"));
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let rule = "Do not tell them the weather in the morning.";
    let (told, ok) = client.rule_for(
        &call(
            KEEP_RULE,
            json!({"rule":rule,"speaking_up":"not_first_in_mornings"}),
        ),
        BUDGET,
        Some(&minh),
    );
    assert!(ok && told["kept_rule"] == true, "{told}");
    let (line, answer) = fixture.next("remember");
    assert_eq!(line["person"], format!("r-{}", &minh[2..]));
    let number = answer["id"].as_u64().unwrap();
    // The next session with Minh at the lamp is told it; Sam's and the
    // room's are not, and without the switch nobody is.
    let to_minh = client.told_for_session_within(1, Some(&minh), true, SESSION);
    assert!(
        to_minh.contains(&format!("- ({number}) {rule}\n")),
        "{to_minh}"
    );
    assert!(!to_minh.contains("[quiet_mornings]"), "{to_minh}");
    for note in [
        client.told_for_session_within(2, Some(&sam), true, SESSION),
        client.told_for_session_within(3, None, true, SESSION),
        client.told_for_session_within(4, Some(&minh), false, SESSION),
    ] {
        assert!(!note.contains("weather"), "{note}");
    }
    // Asked, Minh hears it back; Sam hears there is none.
    let list = |person: &str| client.rule_for(&call(LIST_RULES, json!({})), BUDGET, Some(person));
    let (told, ok) = list(&minh);
    assert!(ok, "{told}");
    assert_eq!(told["rules"][0]["rule"], rule);
    assert_eq!(told["rules"][0]["number"], number);
    assert_eq!(list(&sam).0["rules"], json!([]));
    // A recall in Minh's memory never returns the rule.
    let (told, ok) = client.run_for(
        &call(RECALL, json!({"about":"weather"})),
        BUDGET,
        Some(&minh),
    );
    assert!(ok && texts(&told).is_empty(), "{told}");
    // Sam cannot take Minh's rule back; Minh can, and it is never told again.
    let forget = |person: &str| {
        client.rule_for(
            &call(FORGET_RULE, json!({"number":number})),
            BUDGET,
            Some(person),
        )
    };
    assert_eq!(forget(&sam).0["forgotten"], false);
    assert_eq!(forget(&minh).0["forgotten"], true);
    let to_minh = client.told_for_session_within(5, Some(&minh), true, SESSION);
    assert!(!to_minh.contains("weather"), "{to_minh}");
    assert_eq!(list(&minh).0["rules"], json!([]));
}

fn keep(client: &DaysClient, person: &str, args: Value) -> Value {
    let (told, ok) = client.rule_for(
        &call(lamp_voice::rules::KEEP_RULE, args),
        BUDGET,
        Some(person),
    );
    assert!(ok, "{told}");
    told
}

fn listed(client: &DaysClient, person: &str) -> Vec<(u64, String)> {
    let (told, ok) = client.rule_for(
        &call(lamp_voice::rules::LIST_RULES, json!({})),
        BUDGET,
        Some(person),
    );
    assert!(ok, "{told}");
    told["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| {
            (
                rule["number"].as_u64().unwrap(),
                rule["rule"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// lamp-52e6, PR #822: "forget that rule" took back the last rule kept,
/// not the one meant. A rule is taken back by its number only; no number,
/// or one that is not theirs, removes nothing.
#[test]
fn days_a_rule_is_taken_back_by_its_number_and_never_by_default() {
    let fixture = Fixture::new(|options, now| Days::open(options, now).unwrap());
    let minh = person_id("Minh");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let weather = "Never tell them the weather unless they ask.";
    let buddy = "Do not call them buddy.";
    for rule in [weather, buddy] {
        assert_eq!(
            keep(&client, &minh, json!({"rule":rule}))["kept_rule"],
            true
        );
    }
    let rules = listed(&client, &minh);
    assert_eq!(
        rules
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>(),
        [weather, buddy]
    );
    let forget = |args: Value| {
        client.rule_for(
            &call(lamp_voice::rules::FORGET_RULE, args),
            BUDGET,
            Some(&minh),
        )
    };
    for args in [
        json!({}),
        json!({"number":0}),
        json!({"number":-1}),
        json!({"number":"first"}),
        json!({"number":rules[1].0 + 100}),
    ] {
        let (told, ok) = forget(args.clone());
        assert!(ok && told["forgotten"] == false, "{args}: {told}");
        assert!(told["note"].as_str().unwrap().contains("list_rules"));
        assert_eq!(listed(&client, &minh), rules, "{args}");
    }
    let (told, _) = forget(json!({"number":rules[0].0}));
    assert_eq!(told["forgotten"], true, "{told}");
    assert_eq!(listed(&client, &minh), [rules[1].clone()]);
}

/// Every rule kept is listed, told and counted: a seventh is refused with
/// words to say, and no older rule is dropped to make room for it.
#[test]
fn days_every_rule_kept_is_told_and_a_rule_past_the_most_is_refused() {
    let fixture = Fixture::new(|options, now| Days::open(options, now).unwrap());
    let minh = person_id("Minh");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    assert_eq!(
        keep(
            &client,
            &minh,
            json!({"rule":"Never speak first.","speaking_up":"never_first"})
        )["kept_rule"],
        true
    );
    for n in 1..lamp_voice::rules::MOST_RULES {
        let long = format!("Rule {n}: {}.", "x".repeat(185));
        assert!(long.len() <= 200);
        assert_eq!(
            keep(&client, &minh, json!({"rule":long}))["kept_rule"],
            true
        );
    }
    let told = keep(&client, &minh, json!({"rule":"One more."}));
    assert_eq!(told["kept_rule"], false, "{told}");
    let rules = listed(&client, &minh);
    assert_eq!(rules.len(), lamp_voice::rules::MOST_RULES);
    assert_eq!(rules[0].1, "Never speak first.");
    let note = client.told_for_session_within(1, Some(&minh), true, SESSION);
    for (number, text) in &rules {
        assert!(note.contains(&format!("- ({number}) {text}\n")), "{note}");
    }
    assert!(!note.contains("One more"));
}

/// Eight keep_rule calls at once, as ToolDesk can run them, with five rules
/// kept: one more is kept, never past the most.
#[test]
fn days_rules_kept_at_once_never_pass_the_most() {
    let fixture = Fixture::new(|options, now| Days::open(options, now).unwrap());
    let minh = person_id("Minh");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    for n in 1..lamp_voice::rules::MOST_RULES {
        keep(&client, &minh, json!({"rule":format!("Rule {n}.")}));
    }
    let kept: Vec<Value> = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..8)
            .map(|n| {
                let (client, minh) = (&client, &minh);
                scope.spawn(move || keep(client, minh, json!({"rule":format!("At once {n}.")})))
            })
            .collect();
        calls.into_iter().map(|call| call.join().unwrap()).collect()
    });
    assert_eq!(
        kept.iter().filter(|told| told["kept_rule"] == true).count(),
        1,
        "{kept:?}"
    );
    assert_eq!(listed(&client, &minh).len(), lamp_voice::rules::MOST_RULES);
}

/// An answer given under a rule is journalled, linked to that rule in the
/// person's own rules scope: it is recalled the next day, forgetting the
/// rule removes it and nothing else, what follows in that session's stale
/// context is refused rather than kept without it, and another person's
/// rules are never linked.
#[test]
fn days_an_answer_under_a_rule_is_journalled_and_goes_when_the_rule_is_forgotten() {
    const DAY: u64 = 86_400_000;
    let fixture = Fixture::new(|options, now| Days::open(options, now).unwrap());
    let (minh, sam) = (person_id("Minh"), person_id("Sam"));
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    // Before any rule, a session told none: its answer links no rule.
    assert!(
        !client
            .told_for_session_within(9, Some(&minh), true, SESSION)
            .contains("How you were")
    );
    assert!(client.finished_exchange_for(
        fixture.now - DAY - 1_000,
        "what is the time",
        "Half past ten.",
        Some(&minh)
    ));
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(line.get("rule_ids").is_none(), "{line}");
    keep(&client, &minh, json!({"rule":"Address them as Orchid."}));
    keep(&client, &sam, json!({"rule":"Address them as Fern."}));
    let number = listed(&client, &minh)[0].0;
    // Yesterday, a session told Minh's rule: an answer under it is
    // journalled, linked to Minh's rule only.
    assert!(
        client
            .told_for_session_within(1, Some(&minh), true, SESSION)
            .contains("Orchid")
    );
    assert!(client.finished_exchange_for(
        fixture.now - DAY,
        "who am I",
        "You are Orchid.",
        Some(&minh)
    ));
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(line["rule_ids"], json!([number]), "{line}");
    // Sam's session links Sam's own rule, never Minh's.
    assert!(
        client
            .told_for_session_within(2, Some(&sam), true, SESSION)
            .contains("Fern")
    );
    assert!(client.finished_exchange_for(
        fixture.now - DAY + 2_000,
        "and me",
        "You are Fern.",
        Some(&sam)
    ));
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(line["person"], sam.as_str());
    assert_eq!(
        line["rule_ids"],
        json!([1]),
        "Sam's own rule 1, in Sam's scope"
    );
    // Recalled the next day.
    let recall = |person: &str, about: &str| {
        let (told, ok) = client.run_for(
            &call(RECALL, json!({"about":about,"days_ago":1})),
            BUDGET,
            Some(person),
        );
        assert!(ok, "{told}");
        texts(&told)
    };
    assert!(recall(&minh, "Orchid").contains(&"You are Orchid.".into()));
    // Minh takes the rule back in a new session: the answer under it goes,
    // the other answer and Sam's stay.
    assert!(
        client
            .told_for_session_within(3, Some(&minh), true, SESSION)
            .contains("Orchid")
    );
    let (told, _) = client.rule_for(
        &call(lamp_voice::rules::FORGET_RULE, json!({"number":number})),
        BUDGET,
        Some(&minh),
    );
    assert_eq!(told["forgotten"], true, "{told}");
    assert!(!recall(&minh, "Orchid").contains(&"You are Orchid.".into()));
    assert!(recall(&minh, "time").contains(&"Half past ten.".into()));
    assert!(recall(&sam, "Fern").contains(&"You are Fern.".into()));
    // The provider's context still holds the rule, as after `forget`: this
    // worker journals nothing more, so it cannot come back unlinked.
    assert!(!client.finished_exchange_for(
        fixture.now - 1_000,
        "say my name",
        "Orchid.",
        Some(&minh)
    ));
    assert!(
        !client
            .told_for_session_within(4, Some(&minh), true, SESSION)
            .contains("Orchid")
    );
    assert!(!client.finished_exchange_for(fixture.now - 500, "again", "Orchid.", Some(&minh)));
    let (told, _) = client.run_for(
        &call(RECALL, json!({"about":"Orchid"})),
        BUDGET,
        Some(&minh),
    );
    assert!(texts(&told).is_empty(), "{told}");
    // A fresh worker told no rule journals as before, with no rule_ids.
    drop(client);
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    assert!(
        !client
            .told_for_session_within(5, Some(&minh), true, SESSION)
            .contains("Orchid")
    );
    assert!(client.finished_exchange_for(fixture.now, "thanks", "You are welcome.", Some(&minh)));
    let (line, answer) = fixture.next("append_linked");
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(line.get("rule_ids").is_none(), "{line}");
}

/// The store links an exchange only to its own person's rules scope.
#[test]
fn days_store_never_links_another_persons_rule() {
    let fixture = Fixture::new(|options, now| {
        let mut store = Days::open(options, now).unwrap();
        let minh = Person::new(&person_id("Minh")).unwrap();
        let theirs = minh.rules().unwrap();
        store
            .remember(&theirs, now, "Address them as Orchid.")
            .unwrap();
        // A kept thing of Minh's own scope is not a rule.
        store.remember(&minh, now, "locker 417").unwrap();
        let sam = Person::new(&person_id("Sam")).unwrap();
        assert!(
            store
                .append_ruled(&sam, now, "hi", "hello", &[], &[1])
                .is_err()
        );
        assert!(
            store
                .append_ruled(&minh, now, "hi", "hello", &[], &[2])
                .is_err()
        );
        assert!(
            store
                .append_ruled(&minh, now, "hi", "Orchid", &[], &[1])
                .is_ok()
        );
        store
    });
    drop(fixture);
}

/// The real store, with the answer to the first rule kept held back past
/// Voice's 1.5 s: the rule is committed, its acknowledgement lost.
struct LateRuleAck<M> {
    store: M,
    held: bool,
}

impl<M: Memory> Memory for LateRuleAck<M> {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = self.store.answer(line, now);
        let request: Value = serde_json::from_slice(line).unwrap();
        if !self.held
            && request["op"] == "remember"
            && request["person"]
                .as_str()
                .is_some_and(|p| p.starts_with("r-"))
        {
            self.held = true;
            std::thread::sleep(Duration::from_millis(1_700));
        }
        answer
    }
}

/// A rule committed whose acknowledgement never came: the call says the
/// memory is not answering, and nothing said afterwards is journalled
/// unlinked, so forgetting the rule leaves nothing of it to recall.
#[test]
fn days_a_rule_kept_without_its_acknowledgement_lets_nothing_unlinked_be_journalled() {
    let fixture = Fixture::new(|options, now| LateRuleAck {
        store: Traced {
            store: Days::open(options, now).unwrap(),
            trace: mpsc::channel().0,
        },
        held: false,
    });
    let minh = person_id("Minh");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let (told, ok) = client.rule_for(
        &call(
            lamp_voice::rules::KEEP_RULE,
            json!({"rule":"Address them as Orchid."}),
        ),
        BUDGET,
        Some(&minh),
    );
    assert!(!ok, "{told}");
    // Committed all the same.
    let rules = listed(&client, &minh);
    assert_eq!(rules.len(), 1, "{rules:?}");
    // An answer under it is not journalled unlinked.
    assert!(!client.finished_exchange_for(
        fixture.now - 1_000,
        "who am I",
        "You are Orchid.",
        Some(&minh)
    ));
    let (told, _) = client.rule_for(
        &call(lamp_voice::rules::FORGET_RULE, json!({"number":rules[0].0})),
        BUDGET,
        Some(&minh),
    );
    assert_eq!(told["forgotten"], true, "{told}");
    let (told, ok) = client.run_for(
        &call(RECALL, json!({"about":"Orchid"})),
        BUDGET,
        Some(&minh),
    );
    assert!(ok && texts(&told).is_empty(), "{told}");
}

/// The real store, with the answer to each rule write (`remember` or
/// `forget` in an `r-` scope) replaced after it committed.
struct MangledRuleAck<M> {
    store: M,
    mangle: fn(&str, Value) -> Value,
}

impl<M: Memory> Memory for MangledRuleAck<M> {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let mut answer = self.store.answer(line, now);
        let request: Value = serde_json::from_slice(line).unwrap();
        let op = request["op"].as_str().unwrap_or_default();
        if matches!(op, "remember" | "forget")
            && request["person"]
                .as_str()
                .is_some_and(|p| p.starts_with("r-"))
        {
            answer.line =
                (self.mangle)(op, serde_json::from_str(&answer.line).unwrap()).to_string();
        }
        answer
    }
}

/// A rule write committed with an acknowledgement that is not one (an ID
/// of 0 or past `i64::MAX`, a `removed` that is not a count) is told as
/// not answering, and journalling stays fenced.
#[test]
fn days_a_malformed_rule_acknowledgement_is_not_taken_and_journals_nothing() {
    for (keep_id, removed) in [(json!(0), json!(false)), (json!(u64::MAX), json!("1"))] {
        // The server runs on its own thread, and the mangler is a plain fn:
        // the values go by a static, set before the server starts.
        static BAD: std::sync::Mutex<Option<(Value, Value)>> = std::sync::Mutex::new(None);
        *BAD.lock().unwrap() = Some((keep_id.clone(), removed.clone()));
        let fixture = Fixture::new(|options, now| MangledRuleAck {
            store: Days::open(options, now).unwrap(),
            mangle: |op, mut answer| {
                let (id, removed) = BAD.lock().unwrap().clone().unwrap();
                if op == "remember" {
                    answer["id"] = id;
                } else {
                    answer["removed"] = removed;
                }
                answer
            },
        });
        let minh = person_id("Minh");
        let client = DaysClient::new(fixture.socket.clone()).unwrap();
        let (told, ok) = client.rule_for(
            &call(
                lamp_voice::rules::KEEP_RULE,
                json!({"rule":"Address them as Orchid."}),
            ),
            BUDGET,
            Some(&minh),
        );
        assert!(!ok, "{keep_id}: {told}");
        assert!(!client.finished_exchange_for(
            fixture.now - 1_000,
            "who am I",
            "You are Orchid.",
            Some(&minh)
        ));
        let number = listed(&client, &minh)[0].0;
        let (told, ok) = client.rule_for(
            &call(lamp_voice::rules::FORGET_RULE, json!({"number":number})),
            BUDGET,
            Some(&minh),
        );
        assert!(!ok, "{removed}: {told}");
        assert!(!client.finished_exchange_for(fixture.now - 500, "again", "Orchid.", Some(&minh)));
    }
}
