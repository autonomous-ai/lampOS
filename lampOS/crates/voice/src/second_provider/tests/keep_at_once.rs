//! `LAMP_KEEP_ANSWERS_AT_ONCE` on the second route (C59): a line that only
//! asks to keep something, kept plainly, is answered at once with
//! [`crate::keep_at_once::KEPT`] and no second round. A keep that failed is
//! never said kept, and off nothing changes.
use super::*;
use crate::keep_at_once::KEPT;

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
                json!({"text": "Their dentist is on Tuesday."}),
            )
        })
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

fn words(line: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(line), Ok(ChatEvent::Done)]
}

/// One turn: `heard`, round 1's `first` events and the rounds `later`, each
/// call told `answer_for(name)`, the switch `on`.
fn keep_turn(
    on: bool,
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
    .announcing(false)
    .keeping_at_once(on);
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

fn kept(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": true, "id": 7}),
        _ => json!({"ok": true}),
    }
}

fn failed(name: &str) -> Value {
    match name {
        "remember" => json!({"error": "I could not reach my memory just now."}),
        _ => json!({"ok": true}),
    }
}

fn not_kept(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": false, "note": "Nothing was kept: say so."}),
        _ => json!({"ok": true}),
    }
}

/// A correction: kept, and the old one replaced (Thirteen's P3 on #5565).
fn replaced(name: &str) -> Value {
    match name {
        "remember" => json!({"remembered": true, "id": 8, "replaced": 7}),
        _ => json!({"ok": true}),
    }
}

fn already(name: &str) -> Value {
    match name {
        "remember" => {
            json!({"remembered": true, "note": "Already kept: tell them you already knew it."})
        }
        _ => json!({"ok": true}),
    }
}

const DENTIST: &str = "Remember that my dentist is on Tuesday.";
const ROUND_TWO: &str = "Got it, your dentist is on Tuesday.";

/// A plain keep is answered at once, by the lamp, and no round 2 is asked.
#[test]
fn a_plain_keep_is_answered_at_once_with_no_second_round() {
    let went = keep_turn(
        true,
        DENTIST,
        calling(&["remember"]),
        vec![words(ROUND_TWO)],
        kept,
    );
    assert_eq!(went.said(), vec![KEPT], "{went:?}");
    assert_eq!(went.asked.len(), 1, "{went:?}");
    assert_eq!(went.ran, vec!["remember"]);
    assert_eq!(went.labels.last().map(String::as_str), Some("complete"));
}

/// Off, the same turn asks round 2 and says its words, as today.
#[test]
fn off_a_plain_keep_waits_for_round_two_as_today() {
    let went = keep_turn(
        false,
        DENTIST,
        calling(&["remember"]),
        vec![words(ROUND_TWO)],
        kept,
    );
    assert_eq!(went.said(), vec![ROUND_TWO], "{went:?}");
    assert_eq!(went.asked.len(), 2, "{went:?}");
}

/// A keep that failed, or kept nothing, is never said kept: round 2 is
/// asked with the failure and its words say so.
#[test]
fn a_failed_keep_is_told_never_said_kept() {
    let sorry = "Sorry, I couldn't keep that just now.";
    for (why, answer_for) in [
        ("failed", failed as fn(&str) -> Value),
        ("not kept", not_kept),
    ] {
        let went = keep_turn(
            true,
            DENTIST,
            calling(&["remember"]),
            vec![words(sorry)],
            answer_for,
        );
        assert_eq!(went.said(), vec![sorry], "{why}: {went:?}");
        assert_eq!(went.asked.len(), 2, "{why}: {went:?}");
        // Round 2 was told what the memory answered.
        let told = &went.asked[1];
        let reason = answer_for("remember").to_string();
        assert!(
            told.iter().any(|message| message
                .content
                .as_ref()
                .and_then(|content| content.as_str())
                == Some(reason.as_str())),
            "{why}: {told:?}"
        );
    }
}

/// Every turn the switch does not answer goes on byte for byte as off:
/// the same labels, the same chats asked with the same messages.
#[test]
fn turns_it_does_not_answer_go_on_byte_for_byte_as_off() {
    type Case = (
        &'static str,
        &'static str,
        fn() -> Vec<net::Result<ChatEvent>>,
        fn(&str) -> Value,
    );
    let cases: [Case; 9] = [
        ("failed", DENTIST, || calling(&["remember"]), failed),
        ("not kept", DENTIST, || calling(&["remember"]), not_kept),
        ("already kept", DENTIST, || calling(&["remember"]), already),
        ("a correction", DENTIST, || calling(&["remember"]), replaced),
        (
            "more asked",
            "Remember that my dentist is on Tuesday. What's the weather?",
            || calling(&["remember"]),
            kept,
        ),
        (
            // The Reviewer-lead's 09:56 P2 on #5594: no comma, and a word
            // the call did not keep.
            "a further ask the call did not keep",
            "Remember that my dentist is on Tuesday and explain gravity to me",
            || calling(&["remember"]),
            kept,
        ),
        (
            "not an ask to keep",
            "My dentist is on Tuesday.",
            || calling(&["remember"]),
            kept,
        ),
        (
            "another call beside it",
            DENTIST,
            || calling(&["remember", "set_timer"]),
            kept,
        ),
        (
            "its own words beside the call",
            DENTIST,
            || {
                let mut first = vec![text("Sure.")];
                first.extend(calling(&["remember"]));
                first
            },
            kept,
        ),
    ];
    for (why, heard, first, answer_for) in cases {
        let off = keep_turn(false, heard, first(), vec![words(ROUND_TWO)], answer_for);
        let on = keep_turn(true, heard, first(), vec![words(ROUND_TWO)], answer_for);
        assert_eq!(on, off, "{why}");
        assert!(!on.said().contains(&KEPT), "{why}: {on:?}");
        assert_eq!(on.asked.len(), 2, "{why}: {on:?}");
    }
    // And a turn with no call at all.
    let off = keep_turn(false, DENTIST, words("Okay."), vec![], kept);
    let on = keep_turn(true, DENTIST, words("Okay."), vec![], kept);
    assert_eq!(on, off);
}

/// C59's timing replay, with the provider latencies measured on lamp-06c1
/// on 5 Nov (the batch-100 runs, medians): a chat round's first event 1250
/// ms for a plain answer and 1656 ms for round 1 of a keep, round 2's 1338
/// ms after the memory's answer (1377 ms after round 1's first event, less
/// the memory's 39 ms), the memory 39 ms, the voice's first audio 404 ms
/// for a plain answer and 462 ms for a keep's. Measured from the words'
/// end (`end`) to the first audio out of the engine: the stretch from
/// speech-to-text's last words to the chat's start is not modelled.
#[tokio::test(start_paused = true)]
async fn the_timing_replay_shows_a_keep_as_fast_as_a_plain_answer_less_round_one() {
    async fn first_audio(
        on: bool,
        first: Vec<net::Result<ChatEvent>>,
        rounds: Vec<Duration>,
        speech: Duration,
    ) -> (Duration, usize) {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(DENTIST.to_owned())].into()),
            chats: Mutex::new([first, words(ROUND_TWO)].into()),
            round_after: Mutex::new(rounds.into()),
            speech_after: Some(speech),
            ..Script::default()
        });
        let tools: ToolRunner = Arc::new(|_, _, name, _| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(39)).await;
                kept(&name)
            })
        });
        let mut engine = Engine::with_clock(
            script.clone(),
            Arc::new(|| "SYSTEM".to_owned().into()),
            tools,
            Arc::new(Instant::now),
        )
        .announcing(false)
        .keeping_at_once(on);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let from = tokio::time::Instant::now();
        let mut at = None;
        loop {
            match tokio::time::timeout(Duration::from_secs(30), engine.next())
                .await
                .expect("the turn ends")
            {
                ProviderOutput::Audio { .. } if at.is_none() => at = Some(from.elapsed()),
                ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. } => break,
                _ => {}
            }
        }
        let asked = script.asked.lock().unwrap().len();
        (at.expect("audio"), asked)
    }
    let ms = Duration::from_millis;
    let (plain, _) = first_audio(true, words("Okay."), vec![ms(1250)], ms(404)).await;
    let (keep_off, off_asked) = first_audio(
        false,
        calling(&["remember"]),
        vec![ms(1656), ms(1338)],
        ms(462),
    )
    .await;
    let (keep_on, on_asked) = first_audio(
        true,
        calling(&["remember"]),
        vec![ms(1656), ms(1338)],
        ms(462),
    )
    .await;
    eprintln!(
        "C59 replay: plain {} ms, keep off {} ms ({off_asked} rounds), keep on {} ms ({on_asked} round)",
        plain.as_millis(),
        keep_off.as_millis(),
        keep_on.as_millis()
    );
    assert_eq!((off_asked, on_asked), (2, 1));
    // Off pays both rounds: round 1, the memory, round 2, the voice.
    assert!(keep_off >= ms(1656 + 39 + 1338 + 462), "{keep_off:?}");
    // On saves the whole second round.
    assert!(keep_off - keep_on >= ms(1338), "{keep_off:?} {keep_on:?}");
    // What is left over a plain answer is round 1's later first event (a
    // call starts later than words, 406 ms), the memory's 39 ms and the
    // voice's 58 ms: none of it a round the lamp asks for.
    assert!(
        keep_on <= plain + ms(406 + 39 + 58 + 20),
        "{plain:?} {keep_on:?}"
    );
}

/// One keep turn on [`DENTIST`], kept plainly, the switch on, in the
/// owner's `language`. With `rendered`, [`KEPT`]'s clip is rendered ahead
/// first, and the voice then never answers that line, so the line can be
/// said only from the clip. Without, in a language, the voice never gives
/// the line any sound, so it is never rendered. With what the voice was
/// asked to say.
async fn keep_turn_in(
    language: Option<crate::language::Language>,
    rendered: bool,
) -> (Went, Vec<String>) {
    let line = crate::language::fixed_in(language, KEPT);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(DENTIST.to_owned())].into()),
        chats: Mutex::new([calling(&["remember"]), words(ROUND_TWO)].into()),
        silent_once: Mutex::new(if rendered || language.is_none() {
            vec![]
        } else {
            vec![line; 4]
        }),
        ..Script::default()
    });
    let ran: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        let answer = kept(&name);
        seen.lock().unwrap().push(name);
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .keeping_at_once(true)
    .in_language(language);
    let _release = if rendered {
        engine.render_lines();
        tokio::time::timeout(Duration::from_secs(2), async {
            while engine.shared.fixed_clips.kept(KEPT).is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the keep's line is rendered ahead in the owner's language");
        let (release, stuck) = tokio::sync::oneshot::channel::<()>();
        *script.speech_hold.lock().unwrap() = Some((line, stuck));
        Some(release)
    } else {
        None
    };
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().clone();
    let ran = ran.lock().unwrap().clone();
    let spoken = script.spoken.lock().unwrap().clone();
    (Went { labels, asked, ran }, spoken)
}

/// With an owner's language, the keep's line rendered ahead in it is said
/// at once from its clip, and no round 2 is asked: "Được, mình sẽ nhớ điều
/// đó." for a Vietnamese owner.
#[tokio::test]
async fn in_the_owners_language_the_rendered_line_is_said_with_no_second_round() {
    use crate::language::Language;
    let line = crate::language::fixed_in(Some(Language::Vietnamese), KEPT);
    assert_eq!(line, "Được, mình sẽ nhớ điều đó.");
    let (went, _) = keep_turn_in(Some(Language::Vietnamese), true).await;
    assert_eq!(went.said(), vec![line], "{went:?}");
    assert!(
        went.labels.contains(&format!("audio:{}", line.len())),
        "{went:?}"
    );
    assert!(
        !went.labels.iter().any(|label| label.contains(KEPT)),
        "{went:?}"
    );
    assert_eq!(went.asked.len(), 1, "{went:?}");
    assert_eq!(went.ran, vec!["remember"]);
    assert_eq!(went.labels.last().map(String::as_str), Some("complete"));
}

/// Its line not rendered yet in the owner's language: the turn waits for
/// round 2 as without the switch, never the English line.
#[tokio::test]
async fn in_the_owners_language_a_line_not_rendered_waits_for_round_two() {
    use crate::language::Language;
    let (went, _) = keep_turn_in(Some(Language::Vietnamese), false).await;
    assert_eq!(went.said(), vec![ROUND_TWO], "{went:?}");
    assert_eq!(went.asked.len(), 2, "{went:?}");
    assert_eq!(went.ran, vec!["remember"]);
}

/// In English nothing changes: the line is asked of the voice as the
/// turn's piece, at once, with no round 2.
#[tokio::test]
async fn in_english_the_line_is_said_by_the_voice_as_before() {
    let (went, spoken) = keep_turn_in(None, false).await;
    assert_eq!(went.said(), vec![KEPT], "{went:?}");
    assert_eq!(went.asked.len(), 1, "{went:?}");
    assert_eq!(spoken, vec![KEPT.to_owned()], "{went:?}");
}
