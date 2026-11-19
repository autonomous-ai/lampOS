//! #7976, lamp-9adf, 19 Nov 18:34 (`LAMP_MAKE_NO_ASKS=on`): "Lamp, make a
//! skill called tea reminder: every day at five in the afternoon, glow
//! green and say time for tea." came as two lines, cut at the colon.
//! Request 16 began 0.76 s after request 15 ended and cut it short before
//! any of its answer; the join refused them ("no voice check, and no one
//! face was seen speaking both; no face in view at request 15's end, 1 face
//! in view at request 16's, in a room not known to be quiet"), so request
//! 16 was asked alone, and its first round called `make_skill`'s turn off
//! of `time-for-tea`, a skill made with them minutes before: "Time for tea
//! is off". A split make is still one make, and its second half never acts
//! on another skill by itself (the first builder's ruling).
//!
//! And on 20 Nov 01:01 one line, "Lamp, make a skill called tea reminder:
//! every afternoon at three, remind me to drink a cup of green tea.", came
//! as three, each cut short by the next; the last was asked alone, its
//! first call `list_reminders`, and the make was lost. So with the switch,
//! a line that goes on from words cut short just before it, not joined to
//! them and not told apart from them, is told those words, all of a chain
//! of them, beside its own in its first chat round
//! ([`crate::make_no_asks::split_note`]), whatever the model calls first,
//! and the model judges whether they are one ask.
use super::*;

const FIRST: &str = "Lamp, make a skill called tea reminder";
const SECOND: &str = "every day at five in the afternoon, glow green and say time for tea.";
const WHOLE: &str =
    "tea reminder: every day at five in the afternoon, glow green and say time for tea";

/// What the join said on 9adf: nothing to tell whether one person said both.
const NOTHING_TO_TELL: &str = "no voice check, and no one face was seen speaking both; no face in view at request 1's end, 1 face in view at request 2's, in a room not known to be quiet";

fn nothing_to_tell() -> Joined {
    Joined::Not(NOTHING_TO_TELL.to_owned())
}

/// What each line cut short begins to say, its speech held so none of it
/// is heard.
const CUT_ANSWERS: [&str; 2] = ["Sure.", "Okay."];

/// Requests 1.. say `lines`, each but the last answered by a line whose
/// speech is never heard, and each after the first begun `gaps_ms` after
/// the one before it ended, cutting it short; the join's verdict is
/// `verdict` ([`nothing_to_tell`] on 9adf), and the last line's rounds are
/// `rounds`. With `on`, `LAMP_MAKE_NO_ASKS`. The calls run (name and
/// arguments) and what each chat was asked.
async fn split_lines(
    on: bool,
    lines: &[&'static str],
    gaps_ms: &[u64],
    verdict: Joined,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<(String, Value)>, Vec<Vec<Message>>) {
    assert_eq!(gaps_ms.len() + 1, lines.len());
    assert!(lines.len() <= CUT_ANSWERS.len() + 1);
    crate::make_no_asks::set_here(Some(on));
    let mut chats: Vec<Vec<net::Result<ChatEvent>>> = CUT_ANSWERS[..lines.len() - 1]
        .iter()
        .map(|answer| vec![text(answer), Ok(ChatEvent::Done)])
        .collect();
    chats.extend(rounds);
    let script = Arc::new(Script {
        heard: Mutex::new(lines.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, args| {
        runs.lock().unwrap().push((name, args));
        Box::pin(async { json!({"ok": true}) })
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: Vec::new(),
        }),
        tools,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .joining_cut_words(Arc::new(move |_, _| verdict.clone()));
    let mut releases = Vec::new();
    for (at, answer) in CUT_ANSWERS[..lines.len() - 1].iter().enumerate() {
        let request = at as u64 + 1;
        let (release, hold) = tokio::sync::oneshot::channel();
        releases.push(release);
        *script.speech_hold.lock().unwrap() = Some((answer, hold));
        if request > 1 {
            *now.lock().unwrap() += Duration::from_millis(gaps_ms[at - 1]);
        }
        engine.submit(start(request));
        if request > 1 {
            engine.retire_through(request - 1);
        }
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
                .await
                .expect("the cut line's text comes");
            if matches!(&event, ProviderOutput::Transcript { request: Some(r), text, .. } if *r == request && text.trim() == *answer)
            {
                break;
            }
        }
        while tokio::time::timeout(Duration::from_millis(100), engine.next())
            .await
            .is_ok()
        {}
    }
    let last = lines.len() as u64;
    *now.lock().unwrap() += Duration::from_millis(gaps_ms[gaps_ms.len() - 1]);
    engine.submit(start(last));
    engine.retire_through(last - 1);
    engine.submit(audio(last, 1));
    engine.submit(end(last));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the last line's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request, .. } if request == last) {
            break;
        }
    }
    drop(releases);
    crate::make_no_asks::set_here(None);
    let ran = ran.lock().unwrap().clone();
    let asked = script.asked.lock().unwrap().clone();
    (ran, asked)
}

/// Two lines, the second begun `gap_ms` after the first ended.
async fn split(
    on: bool,
    (first, second): (&'static str, &'static str),
    gap_ms: u64,
    verdict: Joined,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<(String, Value)>, Vec<Vec<Message>>) {
    split_lines(on, &[first, second], &[gap_ms], verdict, rounds).await
}

fn turn_off() -> ToolCall {
    call(
        "c1",
        crate::made_skill::MAKE,
        json!({ crate::made_turn::TURN_OFF: "time for tea" }),
    )
}

fn make_whole(id: &str) -> ToolCall {
    call(id, crate::made_skill::MAKE, json!({ "request": WHOLE }))
}

/// The one run of `make_skill` with `args`.
fn ran_once(args: Value) -> [(String, Value); 1] {
    [(crate::made_skill::MAKE.to_owned(), args)]
}

/// The system notes of the chats asked for the last line (`lines` lines
/// in all, one chat for each line before it), its first round first.
fn last_line_notes(asked: &[Vec<Message>], lines: usize) -> Vec<Vec<String>> {
    asked[lines - 1..]
        .iter()
        .map(|chat| {
            chat.iter()
                .filter(|message| message.role == "system")
                .filter_map(Message::words)
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// The note in `notes` telling the cut words, if any.
fn split_told(notes: &[String]) -> Option<&String> {
    notes
        .iter()
        .find(|note| note.contains(crate::make_no_asks::SPLIT_NEXT))
}

/// The 9adf shape of 18:34: the model is told the first half in its first
/// round, makes one skill with both, and nothing is turned off; no round is
/// held for it.
#[tokio::test]
async fn a_make_cut_at_its_colon_makes_one_skill_in_its_first_round() {
    let (ran, asked) = split(
        true,
        (FIRST, SECOND),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![make_whole("c1")])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made tea reminder."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(ran, ran_once(json!({ "request": WHOLE })));
    let notes = last_line_notes(&asked, 2);
    assert_eq!(notes.len(), 2, "no round held: {notes:?}");
    let told = split_told(&notes[0]).expect("round 1 is told the cut words");
    assert!(told.contains(FIRST), "{told}");
    assert!(told.contains("760 ms"), "{told}");
}

/// 20 Nov 01:01, 9adf: the model's first call is `list_reminders`, not
/// `make_skill`; it is still told the cut words, in that first round.
#[tokio::test]
async fn a_split_line_is_told_the_cut_words_whatever_its_first_call() {
    let (ran, asked) = split(
        true,
        (
            FIRST,
            "every afternoon at three, remind me to drink a cup of green tea.",
        ),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    "list_reminders",
                    json!({}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made tea reminder."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(ran, [("list_reminders".to_owned(), json!({}))]);
    let notes = last_line_notes(&asked, 2);
    let told = split_told(&notes[0]).expect("round 1 is told the cut words");
    assert!(told.contains(FIRST), "{told}");
}

/// 20 Nov 01:01, 9adf: one line came as three, each cut short by the
/// next. The last is told both earlier parts, oldest first; a link past
/// the join's window ends the chain there.
#[tokio::test]
async fn a_line_split_in_three_is_told_both_earlier_parts_in_order() {
    const MIDDLE: &str = "every afternoon at three";
    const LAST: &str = "remind me to drink a cup of green tea.";
    let rounds = || vec![vec![text("Made tea reminder."), Ok(ChatEvent::Done)]];
    let (_, asked) = split_lines(
        true,
        &[FIRST, MIDDLE, LAST],
        &[700, 900],
        nothing_to_tell(),
        rounds(),
    )
    .await;
    let notes = last_line_notes(&asked, 3);
    let told = split_told(&notes[0]).expect("told the cut words");
    let first = told.find(FIRST).expect("the first part is told");
    let middle = told.find(MIDDLE).expect("the middle part is told");
    assert!(first < middle, "oldest first: {told}");
    assert!(told.contains("900 ms"), "{told}");

    let (_, asked) = split_lines(
        true,
        &[FIRST, MIDDLE, LAST],
        &[4_000, 900],
        nothing_to_tell(),
        rounds(),
    )
    .await;
    let notes = last_line_notes(&asked, 3);
    let told = split_told(&notes[0]).expect("told the middle part");
    assert!(told.contains(MIDDLE), "{told}");
    assert!(!told.contains(FIRST), "past the window: {told}");
}

/// The model, told, still judges the line asks the turn off itself: its
/// call runs at once, never held.
#[tokio::test]
async fn a_turn_off_asked_by_the_line_itself_runs_in_its_first_round() {
    let (ran, asked) = split(
        true,
        (FIRST, "And turn off my time for tea skill."),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![turn_off()])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Time for tea is off."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        ran,
        ran_once(json!({ crate::made_turn::TURN_OFF: "time for tea" }))
    );
    let notes = last_line_notes(&asked, 2);
    assert_eq!(notes.len(), 2, "no round held: {notes:?}");
    assert!(split_told(&notes[0]).is_some());
}

/// Nothing changes where nothing was cut just before it, or where the join
/// told the lines apart: past the join's window, joined, with the switch
/// off, or another person by the voice prints or two speaking faces
/// (Heron's P2 on #7990): no note, no extra round, the call run at once.
#[tokio::test]
async fn a_line_not_going_on_from_cut_words_is_asked_as_before() {
    for (on, gap_ms, verdict) in [
        (true, 4_000, nothing_to_tell()),
        (true, 760, Joined::Same("the two lines' voices")),
        (false, 760, nothing_to_tell()),
        (true, 760, Joined::Not(format!("{TWO_VOICES} 0.12)"))),
        (true, 760, Joined::Not(TWO_FACES_SPEAKING.to_owned())),
    ] {
        let case = format!("on {on}, {gap_ms} ms, {verdict:?}");
        let (ran, asked) = split(
            on,
            (FIRST, SECOND),
            gap_ms,
            verdict,
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![turn_off()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Time for tea is off."), Ok(ChatEvent::Done)],
            ],
        )
        .await;
        assert_eq!(
            ran,
            ran_once(json!({ crate::made_turn::TURN_OFF: "time for tea" })),
            "{case}"
        );
        let notes = last_line_notes(&asked, 2);
        assert_eq!(notes.len(), 2, "{case}: {notes:?}");
        assert!(
            notes.iter().all(|notes| split_told(notes).is_none()),
            "{case}: {notes:?}"
        );
    }
}

/// Only a voice print or two speaking faces tell two lines apart; a want
/// of either, or a likeness between the bounds, does not.
#[test]
fn only_a_print_or_two_speaking_faces_tell_the_lines_apart() {
    assert!(Joined::Not(format!("{TWO_VOICES} 0.12)")).told_apart());
    assert!(Joined::Not(TWO_FACES_SPEAKING.to_owned()).told_apart());
    assert!(!nothing_to_tell().told_apart());
    assert!(
        !Joined::Not(
            "a likeness between the bars (cosine 0.50), and no one face was seen speaking both; \
             1 face in view at request 1's end"
                .to_owned()
        )
        .told_apart()
    );
    assert!(!Joined::Same("the two lines' voices").told_apart());
}

/// The note names every part, oldest first, and the gap, then asks the
/// model to judge; never a list of words.
#[test]
fn the_note_tells_every_part_oldest_first() {
    let note = crate::make_no_asks::split_note(
        &[FIRST, "every afternoon at three"],
        Duration::from_millis(820),
    );
    assert!(
        note.contains(&format!("\"{FIRST}\", then \"every afternoon at three\"")),
        "{note}"
    );
    assert!(note.contains("820 ms"), "{note}");
    assert!(note.ends_with(crate::make_no_asks::SPLIT_NEXT), "{note}");
}
