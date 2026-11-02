//! `LAMP_ENDPOINT_PAUSE_JOIN` (#4449): "I need a gift for my sister." then,
//! after a thinking pause, "She likes reading." Smart Turn ended the first
//! piece at 0.98, and its answer began before the person went on. With the
//! switch the coordinator tells the route the line ended on a confident
//! verdict ([`ProviderInput::PauseJoin`]): its answer's first sound, and
//! any call of its chat, wait for the rest of the grace, so a piece begun
//! in it finds the first piece's words unheard and the route's own join
//! (`LAMP_JOIN_CUT_WORDS`) asks the two as one line. A line begun with the
//! lamp's name is never held as a short sound with it.
use super::*;

const FIRST: &str = "I need a gift for my sister.";
const SECOND: &str = "She likes reading.";
const FIRST_ANSWER: &str = "What does she enjoy?";
const SECOND_ANSWER: &str = "A good novel would suit her.";

/// One face seen speaking both, as the join's rule would say it.
const ONE_FACE: Joined = Joined::Same("one face seen speaking both");

fn kept(request: u64) -> ProviderInput {
    ProviderInput::PauseJoin {
        request,
        privacy_generation: 1,
        kept_ms: crate::pause_join::GRACE.as_millis() as u64,
    }
}

/// What a run gave: each output as a short label naming its request, what
/// each chat was asked, the calls run, and the requests unsaid.
struct Run {
    labels: Vec<String>,
    asked: Vec<Vec<Message>>,
    calls: Vec<String>,
    unsaid: Vec<u64>,
}

fn label(event: &ProviderOutput) -> Option<String> {
    match event {
        ProviderOutput::Audio { request, .. } => Some(format!("audio:{request}")),
        ProviderOutput::Transcript {
            request: Some(request),
            text,
            ..
        } if !text.is_empty() => Some(format!("said:{request}:{}", text.trim())),
        ProviderOutput::TurnComplete { request, .. } => Some(format!("complete:{request}")),
        ProviderOutput::TurnFailed { request, .. } => Some(format!("failed:{request}")),
        _ => None,
    }
}

/// Drains the engine's outputs into `labels` for `wait` of the test clock.
async fn drain_for(engine: &mut Engine, labels: &mut Vec<String>, wait: Duration) {
    let until = tokio::time::Instant::now() + wait;
    while let Ok(event) = tokio::time::timeout_at(until, engine.next()).await {
        if let Some(label) = label(&event)
            && labels.last() != Some(&label)
        {
            labels.push(label);
        }
    }
}

/// The first piece, ended on a confident verdict (the coordinator's
/// `PauseJoin`, sent before its `End` as the coordinator queues it, and
/// whether or not the route has the switch: off it is
/// ignored), its chat answering `first` (text, or a call); then, `gap`
/// after its end, the second piece, which `same` says one person said or
/// not. The route has the switch `on`, and the second route's join as the
/// product runs it.
async fn two_pieces(
    on: bool,
    gap: Duration,
    same: Joined,
    first: Vec<net::Result<ChatEvent>>,
) -> Run {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(FIRST.to_owned()), Ok(SECOND.to_owned())].into()),
        chats: Mutex::new(
            [
                first,
                vec![text(SECOND_ANSWER), Ok(ChatEvent::Done)],
                vec![text("Timer set."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, now) = scripted(script.clone());
    let unsaid: Arc<Mutex<Vec<u64>>> = Arc::default();
    let unsaying = unsaid.clone();
    let mut engine = engine
        .announcing(false)
        .joining_cut_words(Arc::new(move |_, _| same.clone()))
        .unsaying_joined(Arc::new(move |request| {
            unsaying.lock().unwrap().push(request)
        }))
        .joining_pauses(on);
    let mut labels = Vec::new();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(kept(1));
    engine.submit(end(1));
    drain_for(&mut engine, &mut labels, gap).await;
    *now.lock().unwrap() += gap;
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), engine.next())
            .await
            .expect("request 2's turn ends");
        if let Some(label) = label(&event)
            && labels.last() != Some(&label)
        {
            labels.push(label);
        }
        if matches!(event, ProviderOutput::TurnComplete { request: 2, .. }) {
            break;
        }
    }
    let asked = script.asked.lock().unwrap().clone();
    let calls = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    let unsaid = unsaid.lock().unwrap().clone();
    Run {
        labels,
        asked,
        calls,
        unsaid,
    }
}

fn answered() -> Vec<net::Result<ChatEvent>> {
    vec![text(FIRST_ANSWER), Ok(ChatEvent::Done)]
}

fn joined() -> Message {
    Message::user(format!("{FIRST} {SECOND}"))
}

/// (a) The second piece begun 1.0 s after the first ended, one person
/// saying both: the two are asked as one line, and the first answer, its
/// chat already begun, never plays, says no word and leaves the thread.
/// Off (as before), the first answer plays at once and the second piece
/// is asked alone.
#[tokio::test(start_paused = true)]
async fn a_piece_begun_in_the_grace_is_joined_and_the_first_answer_never_plays() {
    let on = two_pieces(true, Duration::from_millis(1_000), ONE_FACE, answered()).await;
    assert_eq!(
        on.asked.last().unwrap().last(),
        Some(&joined()),
        "{:?}",
        on.asked
    );
    assert!(
        !on.labels
            .iter()
            .any(|l| l == "audio:1" || l.starts_with("said:1:")),
        "nothing of the dropped first answer goes out: {:?}",
        on.labels
    );
    assert!(
        on.labels.contains(&format!("said:2:{SECOND_ANSWER}"))
            && on.labels.contains(&"audio:2".to_owned()),
        "{:?}",
        on.labels
    );
    assert_eq!(on.unsaid, [1], "the dropped answer leaves the thread");

    let off = two_pieces(false, Duration::from_millis(1_000), ONE_FACE, answered()).await;
    let first_audio = off.labels.iter().position(|l| l == "audio:1");
    assert!(
        first_audio.is_some(),
        "off: the first answer plays: {:?}",
        off.labels
    );
    assert_eq!(
        off.asked.last().unwrap().last(),
        Some(&Message::user(SECOND)),
        "off: the second piece is asked alone"
    );
}

/// With nothing in the grace the answer plays as before, no sooner than
/// the grace's end; a piece begun after it (2.0 s) is a new turn, asked
/// alone, the first answer kept.
#[tokio::test(start_paused = true)]
async fn after_the_grace_the_answer_plays_and_the_next_piece_is_a_new_turn() {
    let began = tokio::time::Instant::now();
    let run = two_pieces(true, Duration::from_millis(2_000), ONE_FACE, answered()).await;
    assert!(
        run.labels.starts_with(&[
            format!("said:1:{FIRST_ANSWER}"),
            "audio:1".to_owned(),
            "complete:1".to_owned()
        ]),
        "{:?}",
        run.labels
    );
    assert_eq!(
        run.asked.last().unwrap().last(),
        Some(&Message::user(SECOND))
    );
    assert!(run.unsaid.is_empty());
    assert!(tokio::time::Instant::now() - began >= Duration::from_millis(2_000));
}

/// The answer's first sound waits for the grace, measured from when the
/// coordinator told the route; off, it goes out at once. The added wait is
/// the grace less the time the answer took anyway.
#[tokio::test(start_paused = true)]
async fn the_answer_waits_only_for_the_rest_of_the_grace() {
    for on in [false, true] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(FIRST.to_owned())].into()),
            chats: Mutex::new([answered()].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine.announcing(false).joining_pauses(on);
        let began = tokio::time::Instant::now();
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(kept(1));
        engine.submit(end(1));
        let first_sound = loop {
            let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
                .await
                .expect("the answer comes");
            if matches!(event, ProviderOutput::Audio { request: 1, .. }) {
                break tokio::time::Instant::now() - began;
            }
        };
        if on {
            assert!(first_sound >= crate::pause_join::GRACE, "{first_sound:?}");
            assert!(
                first_sound < crate::pause_join::GRACE + Duration::from_millis(50),
                "{first_sound:?}"
            );
        } else {
            assert!(
                first_sound < Duration::from_millis(50),
                "off: {first_sound:?}"
            );
        }
    }
}

fn timer_call() -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            "set_timer",
            json!({"minutes": 5}),
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// Reviewer-two's 1 on #4449: no call runs inside the grace. The first
/// piece's chat calls `set_timer` at once; the second piece, begun in the
/// grace, drops it, and the timer never runs. With nothing in the grace it
/// runs at the grace's end; off it runs at once, as before.
#[tokio::test(start_paused = true)]
async fn a_call_of_the_first_piece_never_runs_inside_the_grace() {
    let on = two_pieces(true, Duration::from_millis(1_000), ONE_FACE, timer_call()).await;
    assert!(
        on.calls.is_empty(),
        "the half request never acts: {:?}",
        on.calls
    );
    assert_eq!(on.asked.last().unwrap().last(), Some(&joined()));

    let after = two_pieces(true, Duration::from_millis(2_000), ONE_FACE, timer_call()).await;
    assert_eq!(after.calls, ["set_timer"], "past the grace the call runs");

    let off = two_pieces(false, Duration::from_millis(1_000), ONE_FACE, timer_call()).await;
    assert_eq!(off.calls, ["set_timer"], "off: it runs at once");
}

/// Reviewer-two's 2 on #4449: only the same person's pieces are joined,
/// by the route's own join and its guards. Sixth's P2 on #4916: "What time
/// is it?" and another person speaking 1 s after it, not joined: the
/// second line is asked alone, but the first, whose answer the grace kept
/// back and the cut dropped, is still answered, first, in the second's
/// answer (the model told its words), never lost. The first answer itself
/// never plays (it was cut short unheard).
#[tokio::test(start_paused = true)]
async fn pieces_said_by_two_people_are_not_joined_and_the_first_is_still_answered() {
    let run = two_pieces(
        true,
        Duration::from_millis(1_000),
        Joined::Not("two voices (cosine 0.12)".to_owned()),
        answered(),
    )
    .await;
    let last = run.asked.last().unwrap();
    assert_eq!(last.last(), Some(&Message::user(SECOND)), "{last:?}");
    assert!(
        !run.asked
            .iter()
            .any(|asked| asked.last() == Some(&joined())),
        "{:?}",
        run.asked
    );
    assert!(
        last.contains(&Message::system(crate::pause_join::earlier_line(FIRST))),
        "the first line is answered first: {last:?}"
    );
    assert!(
        !run.labels.contains(&"audio:1".to_owned()),
        "{:?}",
        run.labels
    );
    assert!(
        run.labels.contains(&"audio:2".to_owned()),
        "{:?}",
        run.labels
    );
    // Past the grace, or with the switch off, nothing is told: the cut is
    // the route's as before.
    for (on, gap) in [(true, 2_000), (false, 1_000)] {
        let run = two_pieces(
            on,
            Duration::from_millis(gap),
            Joined::Not("two voices (cosine 0.12)".to_owned()),
            answered(),
        )
        .await;
        assert!(
            !format!("{:?}", run.asked).contains("someone asked you something"),
            "{on} {gap}: {:?}",
            run.asked
        );
    }
}

/// A line ended as a short sound, heard as `heard`, with the switch `on`
/// and a rule `heed` judging its words: whether it was answered.
async fn sound_answered(on: bool, heard: &str, heed: Heed) -> bool {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Yes?"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine
        .announcing(false)
        .checking_sounds(true)
        .heeding(heed)
        .joining_pauses(on);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound: true,
        cut_in: false,
    });
    let labels = turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().len();
    asked > 0 && labels.iter().any(|label| label.starts_with("audio:"))
}

fn letting_all_out() -> Heed {
    Arc::new(|_, _, _, _, _| None)
}

/// (b), Reviewer-two's 3 on #4449: "Lamp, can you tell me", four words
/// past the name, ended as a short sound, is answered with the switch and
/// held without it. The name later in a line (a television's "the lamp
/// is…") or not set off ("Lamp is broken") is still held, and so is a
/// sound with no name; the name's carry ([`NAME_FIRST_MORE`]) is
/// unchanged.
#[tokio::test(start_paused = true)]
async fn a_line_begun_with_the_name_is_never_held_as_a_short_sound() {
    for named in ["Lamp, can you tell me", "Hey Lamp, can you tell me what"] {
        assert!(
            sound_answered(true, named, letting_all_out()).await,
            "{named}"
        );
        assert!(
            !sound_answered(false, named, letting_all_out()).await,
            "off: {named}"
        );
    }
    for unnamed in [
        "and the lamp is on sale now",
        "Lamp is broken again today",
        "¿Qué pasa con eso?",
    ] {
        assert!(
            !sound_answered(true, unnamed, letting_all_out()).await,
            "{unnamed}"
        );
    }
    assert_eq!(NAME_FIRST_MORE, 3, "the name's carry is unchanged");
}

/// #4701's `LAMP_NAME_REPEAT_NOT_A_CALL` with (b): a line that opens with
/// the name but repeats the line before's end is no call by the rule, and
/// (b) never makes it one: (b) only lifts the short-sound hold, the rule
/// still judges the words. The rule here is the product's own
/// ([`crate::attention::Attention`], the repeat switch on), told the line
/// before, "I told her, lamp, turn it off.", and judging "Lamp, turn it
/// off, she said." begun 0.3 s after it. A line that is a call stays one.
#[tokio::test(start_paused = true)]
async fn a_repeated_name_stays_no_call_with_both_switches() {
    use crate::attention::{Attention, Verdict};
    use crate::room_told::Room;
    const S: u64 = 1_000_000;
    let judged_with = |before: &'static str, repeat: bool| -> Heed {
        Arc::new(move |_, _, words, _, _| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.ignore_name_in_passing(true);
            attention.count_stand_alone_ins(true);
            attention.not_calling_on_a_repeat(repeat);
            attention.began_at(70, 7, 10 * S);
            attention.heard(before);
            attention.ended(70, 11 * S + 200_000);
            attention.first_output_busy(70, Room::Busy, false, false, None);
            attention.began_at(71, 7, 11 * S + 500_000);
            attention.heard(words);
            attention.ended(71, 13 * S);
            match attention.first_output_busy(71, Room::Busy, false, false, None) {
                Some(Verdict::Hold(why)) => Some(why),
                _ => None,
            }
        })
    };
    let repeat = "Lamp, turn it off, she said.";
    let before = "I told her, lamp, turn it off.";
    assert!(
        !sound_answered(true, repeat, judged_with(before, true)).await,
        "both switches: the repeat stays no call"
    );
    assert!(
        sound_answered(true, repeat, judged_with(before, false)).await,
        "the repeat switch off: the name at its start is a call, and (b) lets it out"
    );
    assert!(
        sound_answered(
            true,
            "Lamp, what time is it?",
            judged_with("I need to finish the report.", true)
        )
        .await,
        "both switches: a call after an unrelated line is answered"
    );
}

/// Lines said 1.0 s apart, each in the grace of the one before, every one
/// but the last ended on a confident verdict ([`kept`]): `heard` their
/// words, `chats` each chat's answer in order, `same` whether one person
/// said a pair, `setup` the engine's other switches. The route has the
/// switch on and the second route's join, until the last line's turn ends.
async fn lines_in_the_grace(
    heard: &[&str],
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    same: SameSpeaker,
    setup: impl FnOnce(Engine) -> Engine,
) -> Run {
    let script = Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|words| Ok((*words).to_owned())).collect()),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let (engine, calls, now) = scripted(script.clone());
    let mut engine = setup(
        engine
            .announcing(false)
            .joining_cut_words(same)
            .unsaying_joined(Arc::new(|_| {}))
            .joining_pauses(true),
    );
    let last = heard.len() as u64;
    let mut labels = Vec::new();
    for request in 1..=last {
        if request > 1 {
            drain_for(&mut engine, &mut labels, Duration::from_millis(1_000)).await;
            *now.lock().unwrap() += Duration::from_millis(1_000);
        }
        engine.submit(start(request));
        if request > 1 {
            engine.retire_through(request - 1);
        }
        engine.submit(audio(request, 1));
        if request < last {
            engine.submit(kept(request));
        }
        engine.submit(end(request));
    }
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), engine.next())
            .await
            .expect("the last line's turn ends");
        if let Some(label) = label(&event)
            && labels.last() != Some(&label)
        {
            labels.push(label);
        }
        if matches!(event, ProviderOutput::TurnComplete { request, .. } if request == last) {
            break;
        }
    }
    let asked = script.asked.lock().unwrap().clone();
    let calls = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    Run {
        labels,
        asked,
        calls,
        unsaid: Vec::new(),
    }
}

/// Seventh's P3 on #4916: A's "What time is it?" is cut in its grace by
/// B's line, another person's, so B's answer is told A's words first; B,
/// whose answer the grace keeps back too, goes on 1 s later. B's two
/// pieces are joined, and A's words are still told first in their answer,
/// kept with B's words in flight, not lost to B's second cut.
#[tokio::test(start_paused = true)]
async fn earlier_words_told_first_stay_in_flight_through_a_later_cut() {
    const THIRD: &str = "Mostly mysteries.";
    let run = lines_in_the_grace(
        &[FIRST, SECOND, THIRD],
        vec![
            answered(),
            vec![text(SECOND_ANSWER), Ok(ChatEvent::Done)],
            vec![text("A mystery, then."), Ok(ChatEvent::Done)],
        ],
        Arc::new(|before, _| {
            if before == 1 {
                Joined::Not("two voices (cosine 0.12)".to_owned())
            } else {
                ONE_FACE
            }
        }),
        |engine| engine,
    )
    .await;
    let last = run.asked.last().unwrap();
    assert_eq!(
        last.last(),
        Some(&Message::user(format!("{SECOND} {THIRD}"))),
        "{last:?}"
    );
    assert!(
        last.contains(&Message::system(crate::pause_join::earlier_line(FIRST))),
        "A's words are still answered first: {last:?}"
    );
    assert!(
        !last
            .iter()
            .any(|message| message.role == "user"
                && format!("{message:?}").contains("What does she")),
        "{last:?}"
    );
    assert!(
        !run.labels.iter().any(|l| l == "audio:1" || l == "audio:2"),
        "{:?}",
        run.labels
    );
}

/// A call run, and the person whose scope it ran in (none: the room's).
type RanIn = (String, Option<String>);

/// Seventh's P2 on #4916: a guest off camera says "Remember my birthday is
/// in May." and the owner, in view, speaks in its grace, not joined. The
/// owner's line is told the guest's words first, and its `remember` runs
/// in the room's scope, never the owner's. Without the guest's words, the
/// same line's `remember` runs as the owner's. On the real clock: the
/// face cache's sightings are.
#[tokio::test]
async fn earlier_words_told_first_are_kept_as_the_rooms() {
    use crate::memory_scope::{FaceCache, GAP, ScopeLease};
    const GUEST: &str = "Remember my birthday is in May.";
    const OWNER: &str = "What time is it?";
    let remember = || {
        vec![
            vec![text("Happy to."), Ok(ChatEvent::Done)],
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "k1",
                    crate::remember::REMEMBER,
                    json!({"fact": "birthday is in May"}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Kept. It is noon."), Ok(ChatEvent::Done)],
        ]
    };
    let run = |heard: &'static [&'static str], chats| async move {
        let path = std::env::temp_dir().join(format!(
            "pause-join-scope-{}-{}",
            std::process::id(),
            heard.len()
        ));
        std::fs::write(&path, "Maya").unwrap();
        let cache = Arc::new(FaceCache::holding_gaps(Some(GAP)));
        let seen = crate::known_faces::FaceObservation::read(&path).unwrap();
        let scopes: Arc<Mutex<Vec<RanIn>>> = Arc::default();
        let ran = scopes.clone();
        let runner: ScopedToolRunner = Arc::new(move |_, _, name: String, _, lease: ScopeLease| {
            let person = lease
                .scope()
                .and_then(|scope| scope.person())
                .map(str::to_owned);
            ran.lock().unwrap().push((name, person));
            Box::pin(async { json!({"ok": true}) })
        });
        let outcome = lines_in_the_grace(
            heard,
            chats,
            Arc::new(|_, _| Joined::Not("two voices (cosine 0.12)".to_owned())),
            move |engine| {
                let mut engine = engine.with_memory_scope(
                    lamp_turn_owner::BootId::new([9; 16]).unwrap(),
                    cache.clone(),
                    runner,
                    None,
                );
                engine.memory_camera(true, 1);
                // The owner's face in view, known, the guest off camera.
                cache.replace_seen(cache.begin_refresh(), Some(seen), Some(1));
                engine
            },
        )
        .await;
        std::fs::remove_file(&path).unwrap();
        let scopes = scopes.lock().unwrap().clone();
        (outcome, scopes)
    };
    let (told, scopes) = run(&[GUEST, OWNER], remember()).await;
    assert!(
        told.asked
            .iter()
            .any(|asked| asked.contains(&Message::system(crate::pause_join::earlier_line(GUEST)))),
        "{:?}",
        told.asked
    );
    assert_eq!(
        scopes,
        [(crate::remember::REMEMBER.to_owned(), None)],
        "the guest's words are kept as the room's"
    );
    // The owner's own line alone: its keep is the owner's.
    let mut alone = remember();
    alone.remove(0);
    let (_, scopes) = run(&[OWNER], alone).await;
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert!(scopes[0].1.is_some(), "{scopes:?}");
}
