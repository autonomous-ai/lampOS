//! A keep refused for want of a known face (`LAMP_MEMORY_PERSONAL_NEEDS_FACE`,
//! #7877): on lamp-e82d, 19 Nov, "remember that …" lines said with nobody
//! known kept nothing, and the reply was the model's own, which may say "Got
//! it". A line that only asks to keep, whose every `remember` came back
//! refused so, is answered at once with
//! [`crate::remember::needs_face::REFUSED_LINE`], one true line that asks
//! nothing, and no second round. Anything else goes on as before.
use super::*;
use crate::remember::needs_face::{NOT_KEPT, REFUSED_LINE};

/// One turn as it went: every label, and every chat's messages.
#[derive(Debug, PartialEq)]
struct Went {
    labels: Vec<String>,
    asked: Vec<Vec<Message>>,
    ran: Vec<String>,
}

impl Went {
    fn said(&self) -> Vec<&str> {
        self.labels
            .iter()
            .filter_map(|label| label.strip_prefix("said:"))
            .map(str::trim)
            .collect()
    }
}

fn calling(names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            call(
                &format!("c{at}"),
                name,
                json!({"text": "Their car is at level B2, spot 4T."}),
            )
        })
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

fn words(line: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(line), Ok(ChatEvent::Done)]
}

/// One turn: `heard`, round 1's `first` events and the rounds `later`, each
/// call told `answer_for(name)`.
fn keep_turn(
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
    answer_for: fn(&str) -> Value,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        ..Script::default()
    });
    let ran: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        let answer = answer_for(&name);
        seen.lock().unwrap().push(name);
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let asked = script.asked.lock().unwrap().clone();
    let ran = ran.lock().unwrap().clone();
    Went { labels, asked, ran }
}

/// What `needs_face::withheld_as` tells the model for a personal keep while
/// nobody is known.
fn refused(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": false, "note": NOT_KEPT}),
        _ => json!({"ok": true}),
    }
}

fn kept(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": true, "id": 7}),
        _ => json!({"ok": true}),
    }
}

fn not_kept_otherwise(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": false, "note": "Nothing was kept: say so."}),
        _ => json!({"ok": true}),
    }
}

const CAR: &str = "Remember that my car is at level B2, spot 4T.";
const GOT_IT: &str = "Got it, level B2, spot 4T.";

/// The refused keep is said as the one true line, never the model's "Got
/// it", and no round 2 is asked.
#[test]
fn a_keep_refused_for_no_known_face_is_said_plainly_never_got_it() {
    let went = keep_turn(CAR, calling(&["remember"]), vec![words(GOT_IT)], refused);
    assert_eq!(went.said(), vec![REFUSED_LINE], "{went:?}");
    assert_eq!(went.asked.len(), 1, "{went:?}");
    assert_eq!(went.ran, vec!["remember"]);
    assert_eq!(went.labels.last().map(String::as_str), Some("complete"));
}

/// lamp-e82d, 19 Nov 11:03 (Migrate's ROW at b586bc319a): "Lamp, remember
/// that my car is at level B2." went to round 2. The line as heard there,
/// with the lamp's name or its own name as heard, and a detail clause after
/// the keep ("… B2, spot 4T", which `keep_at_once::only_keeps` alone
/// refused): each the one line, no round 2.
#[test]
fn the_rows_line_and_its_shapes_say_the_line_with_no_round_two() {
    for heard in [
        "Lamp, remember that my car is at level B2.",
        "Lamp, remember that my car is at level B 2.",
        "Lamp remember that my car is at level B2",
        "Lynn, remember that my car is at level B2.",
        "Hey, Liz. Remember that my car is at level B2.",
        CAR,
    ] {
        let went = keep_turn(heard, calling(&["remember"]), vec![words(GOT_IT)], refused);
        assert_eq!(went.said(), vec![REFUSED_LINE], "{heard}: {went:?}");
        assert_eq!(went.asked.len(), 1, "{heard}: {went:?}");
    }
}

/// Which lines count as only a keep for the refusal: a question or a
/// further ask anywhere never does.
#[test]
fn only_a_keep_with_its_details_counts_for_the_refusal() {
    use crate::keep_at_once::only_keeps_refused;
    for yes in [
        "Lamp, remember that my car is at level B2.",
        "Lynn, remember that my car is at level B2.",
        "Hey, Liz. Remember that my car is at level B2.",
        CAR,
        "Remember that my car is at level B2, thanks.",
    ] {
        assert!(only_keeps_refused(yes), "{yes}");
    }
    for no in [
        "Remember that my car is at level B2. What's the weather?",
        "Remember that my car is at level B2, and tell me a joke.",
        "Remember that my car is at level B2, then set a timer.",
        "My car is at level B2.",
        "Do you remember where my car is?",
        "I parked downstairs today, remember that my car is at level B2.",
        "Liz. Tell me a joke. Remember that my car is at level B2.",
    ] {
        assert!(!only_keeps_refused(no), "{no}");
    }
}

/// Two refused keeps of one line: still the one line, once.
#[test]
fn two_refused_keeps_of_one_line_say_the_line_once() {
    let went = keep_turn(
        "Remember that my car is at level B2, spot 4T. Remember my car is at level B2.",
        calling(&["remember", "remember"]),
        vec![words(GOT_IT)],
        refused,
    );
    assert_eq!(went.said(), vec![REFUSED_LINE], "{went:?}");
    assert_eq!(went.asked.len(), 1, "{went:?}");
}

/// The line asks nothing (the no-asks rule), never says it kept, and never
/// says the face is unknown: a known person not seen saying it is refused
/// the same way, and must not hear that the lamp forgot them (the first
/// builder's HOLD on #7885).
#[test]
fn the_refused_line_asks_nothing_and_claims_no_keep() {
    assert_eq!(
        REFUSED_LINE,
        "I'm not sure who's talking, so I can't keep that."
    );
    assert!(!REFUSED_LINE.to_lowercase().contains("face"));
    assert!(!REFUSED_LINE.contains('?'));
    assert_eq!(crate::remember::keep_claim(REFUSED_LINE), None);
    assert!(!REFUSED_LINE.to_lowercase().contains("got it"));
}

/// Every turn it does not answer goes on as before: round 2 is asked and
/// its words said.
#[test]
fn turns_it_does_not_answer_go_on_to_round_two() {
    type Case = (
        &'static str,
        &'static str,
        fn() -> Vec<net::Result<ChatEvent>>,
        fn(&str) -> Value,
    );
    let cases: [Case; 5] = [
        ("kept", CAR, || calling(&["remember"]), kept),
        (
            "refused for another reason",
            CAR,
            || calling(&["remember"]),
            not_kept_otherwise,
        ),
        (
            "more asked",
            "Remember that my car is at level B2, spot 4T. What's the weather?",
            || calling(&["remember"]),
            refused,
        ),
        (
            "not an ask to keep",
            "My car is at level B2, spot 4T.",
            || calling(&["remember"]),
            refused,
        ),
        (
            "another call beside it",
            CAR,
            || calling(&["remember", "set_timer"]),
            refused,
        ),
    ];
    let round_two = "I can't keep that one.";
    for (why, heard, first, answer_for) in cases {
        let went = keep_turn(heard, first(), vec![words(round_two)], answer_for);
        assert_eq!(went.said(), vec![round_two], "{why}: {went:?}");
        assert_eq!(went.asked.len(), 2, "{why}: {went:?}");
    }
}
