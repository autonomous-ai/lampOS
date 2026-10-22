//! `LAMP_SECOND_NAME_CARRIES`: a line that is only the lamp's name waits for
//! the line after it, which is then said by it. In a played round on
//! lamp-4ace on 22 Oct, "Lamp, I need a gift for my sister … What should I
//! get her?" with a pause after "Lamp," came as pieces: the name's piece
//! (heard as "Life." or "Louvre,", or as the name with more) was held, as a
//! short sound or as said to nobody, and each piece after it as said to
//! nobody, so the whole request was lost.
use super::*;

/// When the first sound of the name's line was read, by this process's
/// clock, and when the name was heard in it, as `lamp-name` dates it.
const NAME_READ_US: u64 = 1_000_000;
const NAME_HEARD_US: u64 = 1_100_000;

/// What the rule was asked of each line: its request, window and words.
type Judged = Arc<Mutex<Vec<(u64, (u64, u64), String)>>>;

/// The engine as the lamp runs it: short sounds checked, the switch as
/// given, and a rule that holds a line whose words don't begin with the
/// name. The name note is the worker's own reading of it
/// (`worker::Heeding::note_the_name`): given when the rule's window for the
/// line takes in the name's hearing.
fn lamp(script: &Arc<Script>, on: bool, judged: &Judged) -> (Engine, Arc<Mutex<Instant>>) {
    let (engine, _, now) = scripted(script.clone());
    let seen = judged.clone();
    let windows = judged.clone();
    let engine = engine
        .checking_sounds(true)
        .carrying_the_name(on)
        .heeding(Arc::new(move |request, window, words, _| {
            seen.lock()
                .unwrap()
                .push((request, window, words.to_owned()));
            (!crate::attention::begins_with_the_name(words))
                .then_some(crate::attention::NOT_SPOKEN_TO)
        }))
        .noting_the_name(Arc::new(move |request| {
            windows
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(judged, _, _)| *judged == request)
                .filter(|(_, (began, ended), _)| {
                    *began <= NAME_HEARD_US + crate::called::BEFORE_US && NAME_HEARD_US <= *ended
                })
                .map(|_| crate::called::NOTE)
        }));
    (engine, now)
}

fn script(heard: &[&str]) -> Arc<Script> {
    Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(
            heard
                .iter()
                .map(|_| vec![text("Yes?"), Ok(ChatEvent::Done)])
                .collect(),
        ),
        ..Script::default()
    })
}

/// A line's sound, first read at `at_us`.
fn audio_at(request: u64, at_us: u64) -> ProviderInput {
    ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence: 1,
        read_completed_at_us: at_us,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    }
}

fn ended(request: u64, sound: bool) -> ProviderInput {
    ProviderInput::End {
        request,
        privacy_generation: 1,
        sound,
        cut_in: false,
    }
}

/// The name's line: its sound, the coordinator's `Called` when `called`,
/// and its end, marked as a short sound when `sound`.
fn say_the_name(engine: &mut Engine, called: bool, sound: bool) {
    engine.submit(start(1));
    engine.submit(audio_at(1, NAME_READ_US));
    if called {
        engine.submit(ProviderInput::Called {
            request: 1,
            privacy_generation: 1,
        });
    }
    engine.submit(ended(1, sound));
}

/// The line after it, `after` its end on both clocks: the runtime's and
/// the engine's, which the kept sound reads.
async fn say_after(engine: &mut Engine, now: &Arc<Mutex<Instant>>, after: Duration) {
    tokio::time::sleep(after).await;
    *now.lock().unwrap() += after;
    engine.submit(start(2));
    engine.submit(audio_at(2, NAME_READ_US + after.as_micros() as u64));
    engine.submit(ended(2, false));
}

fn asked(script: &Script) -> Vec<Vec<Message>> {
    script.asked.lock().unwrap().clone()
}

fn last_words(asked: &[Message]) -> Option<&Message> {
    asked.last()
}

/// "Lamp." and a line the rule would hold, begun 1.2 s after its end: the
/// line is let out as said by the name, judged with the name's window and
/// told the name note, and the name itself gets no answer. At 1.2 s the
/// name's words already begin the line (the kept sound, `KeptSound`); at
/// 2 s, past its 1.5 s, the name is put before them. A name misheard, or
/// the name with more, with the coordinator's `Called`, carries the same.
#[tokio::test(start_paused = true)]
async fn a_line_after_the_lamps_name_alone_is_said_by_it() {
    let note = Message::system(crate::called::NOTE);
    for (name, called, after, asked_as) in [
        ("Lamp.", false, 1_200, "Lamp. I need a gift for my sister."),
        ("Lamp.", false, 2_000, "Lamp, I need a gift for my sister."),
        (
            "Hey lamp",
            false,
            2_000,
            "Lamp, I need a gift for my sister.",
        ),
        ("Life.", true, 1_200, "Lamp, I need a gift for my sister."),
        ("Louvre,", true, 2_400, "Lamp, I need a gift for my sister."),
        (
            "Lamp. So",
            true,
            2_000,
            "Lamp, I need a gift for my sister.",
        ),
    ] {
        let played = script(&[name, "I need a gift for my sister."]);
        let judged = Judged::default();
        let (mut engine, now) = lamp(&played, true, &judged);
        say_the_name(&mut engine, called, true);
        say_after(&mut engine, &now, Duration::from_millis(after)).await;
        let heard = turn(&mut engine).await;
        assert_eq!(
            heard.last().unwrap(),
            "complete",
            "{name} {after}: {heard:?}"
        );
        let asked = asked(&played);
        assert_eq!(asked.len(), 1, "{name} {after}: the name got no answer");
        assert_eq!(
            last_words(&asked[0]),
            Some(&Message::user(asked_as)),
            "{name} {after}"
        );
        assert!(asked[0].contains(&note), "{name} {after}: {:?}", asked[0]);
        assert!(
            !asked[0].contains(&Message::system(crate::provider_worker::CALLED_NOTE)),
            "{name} {after}: not asked as a call"
        );
        let judged = judged.lock().unwrap().clone();
        assert_eq!(judged.len(), 1, "{name} {after}: the name is never judged");
        let (request, (began, _), words) = &judged[0];
        assert_eq!((*request, *began), (2, NAME_READ_US), "{name} {after}");
        assert_eq!(words, asked_as, "{name} {after}");
    }
}

/// "Lamp." alone, or "Hey Lamp", and nothing after: answered as a call,
/// told so ([`crate::provider_worker::CALLED_NOTE`]), no sooner than 2.5 s
/// after its end, and never held as the short sound its end marks. So is a
/// line the coordinator heard as the name (`Called`): a name misheard as a
/// short word, or none, is asked as the name; more words as they are.
#[tokio::test(start_paused = true)]
async fn the_lamps_name_alone_is_answered_as_a_call_after_the_wait() {
    let called = Message::system(crate::provider_worker::CALLED_NOTE);
    for (name, coordinator_called, asked_as) in [
        ("Lamp.", false, "Lamp."),
        ("hey, LAMP!", false, "hey, LAMP!"),
        ("Lamp", true, "Lamp"),
        ("Life.", true, super::super::CALLED_WORDS),
        ("Month.", true, super::super::CALLED_WORDS),
        ("", true, super::super::CALLED_WORDS),
        ("Lamp. So", true, "Lamp. So"),
    ] {
        let alone = script(&[name]);
        let judged = Judged::default();
        let (mut engine, _) = lamp(&alone, true, &judged);
        say_the_name(&mut engine, coordinator_called, true);
        tokio::time::sleep(Duration::from_millis(2_400)).await;
        assert!(asked(&alone).is_empty(), "{name:?}: still waiting at 2.4 s");
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let asked = asked(&alone);
        assert_eq!(asked.len(), 1, "{name:?}");
        assert_eq!(asked[0][asked[0].len() - 2], called, "{name:?}");
        assert_eq!(last_words(&asked[0]), Some(&Message::user(asked_as)));
    }
}

/// A short sound that is not only the name is still held at once without
/// the coordinator's `Called`: a sound, a word that isn't the name, or the
/// name and more; and a line that begins with the name is answered at
/// once, no wait.
#[tokio::test(start_paused = true)]
async fn a_short_sound_that_is_no_name_is_still_held() {
    for (heard, called, sound) in [
        ("Hmm.", false, true),
        ("Life.", false, true),
        ("Lamp. So", false, true),
    ] {
        let held = script(&[heard]);
        let judged = Judged::default();
        let (mut engine, _) = lamp(&held, true, &judged);
        say_the_name(&mut engine, called, sound);
        assert_eq!(
            turn_within(&mut engine, Duration::from_millis(100))
                .await
                .last()
                .unwrap(),
            "complete",
            "{heard}: no wait"
        );
        assert!(asked(&held).is_empty(), "{heard}");
    }
    let asks = script(&["Lamp, what time is it?"]);
    let judged = Judged::default();
    let (mut engine, _) = lamp(&asks, true, &judged);
    say_the_name(&mut engine, false, false);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(asked(&asks).len(), 1, "asked at once");
    turn(&mut engine).await;
}

/// The name answered as a call once its wait is over: a line begun 3 s
/// after its end is not said by it, and the rule holds it as its own.
#[tokio::test(start_paused = true)]
async fn a_line_past_the_wait_is_not_said_by_the_name() {
    let played = script(&["Lamp.", "I need a gift for my sister."]);
    let judged = Judged::default();
    let (mut engine, now) = lamp(&played, true, &judged);
    say_the_name(&mut engine, false, true);
    tokio::time::sleep(Duration::from_millis(2_400)).await;
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    say_after(&mut engine, &now, Duration::from_millis(600)).await;
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let asked = asked(&played);
    assert_eq!(asked.len(), 1, "only the call: {asked:?}");
    assert_eq!(last_words(&asked[0]), Some(&Message::user("Lamp.")));
    let judged = judged.lock().unwrap().clone();
    assert_eq!(
        judged
            .last()
            .map(|(request, (began, _), words)| (*request, *began, words.as_str())),
        Some((2, NAME_READ_US + 600_000, "I need a gift for my sister."))
    );
}

/// Not in another session, nor after a stop: the newer line drops the
/// name's request, as any newer line does, and is judged as its own.
#[tokio::test(start_paused = true)]
async fn a_name_is_never_carried_across_sessions_or_a_stop() {
    for (session, after_stop) in [(2, false), (1, true)] {
        let played = script(&["Lamp.", "I need a gift for my sister."]);
        let judged = Judged::default();
        let (mut engine, now) = lamp(&played, true, &judged);
        say_the_name(&mut engine, false, true);
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        *now.lock().unwrap() += Duration::from_millis(2_000);
        engine.submit(ProviderInput::Start {
            request: 2,
            session,
            privacy_generation: 1,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
        });
        engine.submit(audio_at(2, 3_000_000));
        engine.submit(ended(2, false));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        assert!(asked(&played).is_empty(), "{session} {after_stop}");
    }
}

/// Never into a line the eyes saw a face speak that they did not see say
/// the name; the same face, or one seen only on the line, carries it.
#[tokio::test(start_paused = true)]
async fn a_name_is_not_carried_to_a_line_another_face_spoke() {
    use crate::speaking_faces::SpeakingFaces;
    let mut frame = 0;
    let mut seen_speaking = |eyes: &SpeakingFaces, id: Option<&str>| {
        for _ in 0..4 {
            frame += 1;
            let faces = lamp_msgs::Faces {
                faces: id
                    .map(|id| lamp_msgs::Face {
                        id: id.to_owned(),
                        xmin: 0.1,
                        ymin: 0.1,
                        xmax: 0.3,
                        ymax: 0.4,
                        confidence: 1.0,
                        yaw: None,
                        facing: None,
                        speaking: Some(lamp_msgs::Speaking {
                            state: lamp_msgs::SpeakingState::Yes,
                            score: 0.9,
                            from: None,
                        }),
                    })
                    .into_iter()
                    .collect(),
            };
            eyes.look(Instant::now(), Some((("eyes", frame), &faces)));
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    for (said_the_name, spoke, carried) in [
        (Some("f1"), Some("f2"), false),
        (Some("f1"), Some("f1"), true),
        (None, Some("f2"), true),
    ] {
        let played = script(&["Lamp.", "I need a gift for my sister."]);
        let judged = Judged::default();
        let (engine, now) = lamp(&played, true, &judged);
        let eyes = Arc::new(SpeakingFaces::default());
        let mut engine =
            engine.confirming_forgets_by_sight(eyes.clone(), std::path::PathBuf::from("/nowhere"));
        engine.submit(start(1));
        engine.submit(audio_at(1, NAME_READ_US));
        tokio::time::sleep(Duration::from_millis(10)).await;
        seen_speaking(&eyes, said_the_name);
        engine.submit(ended(1, true));
        tokio::time::sleep(Duration::from_millis(2_000)).await;
        *now.lock().unwrap() += Duration::from_millis(2_000);
        engine.submit(start(2));
        engine.submit(audio_at(2, 3_000_000));
        tokio::time::sleep(Duration::from_millis(10)).await;
        seen_speaking(&eyes, spoke);
        engine.submit(ended(2, false));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        assert_eq!(
            asked(&played).len(),
            usize::from(carried),
            "{said_the_name:?} then {spoke:?}"
        );
    }
}

/// With the switch off, as before: the name misheard ("Life.", with the
/// coordinator's `Called`) and marked a short sound is held at once, and
/// the line 2 s after it is held as said to nobody, the request lost as in
/// the played round; "Lamp.", a short word, is asked at once, marked or
/// not, told it was a call only with `Called`.
#[tokio::test(start_paused = true)]
async fn with_the_switch_off_nothing_changes() {
    let played = script(&["Life.", "I need a gift for my sister."]);
    let judged = Judged::default();
    let (mut engine, now) = lamp(&played, false, &judged);
    say_the_name(&mut engine, true, true);
    assert_eq!(
        turn_within(&mut engine, Duration::from_millis(100))
            .await
            .last()
            .unwrap(),
        "complete"
    );
    say_after(&mut engine, &now, Duration::from_millis(2_000)).await;
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert!(asked(&played).is_empty());
    let judged = judged.lock().unwrap().clone();
    assert_eq!(
        judged
            .last()
            .map(|(request, (began, _), words)| (*request, *began, words.as_str())),
        Some((2, NAME_READ_US + 2_000_000, "I need a gift for my sister."))
    );
    for (called, sound) in [(false, false), (true, false), (false, true)] {
        let at_once = script(&["Lamp."]);
        let judged = Judged::default();
        let (mut engine, _) = lamp(&at_once, false, &judged);
        say_the_name(&mut engine, called, sound);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let asked = asked(&at_once);
        assert_eq!(asked.len(), 1, "{called} {sound}: asked at once");
        assert_eq!(
            asked[0].contains(&Message::system(crate::provider_worker::CALLED_NOTE)),
            called
        );
        turn(&mut engine).await;
    }
}

/// What counts as the name alone: [`net::key_terms::NAME`] in any case and
/// with any punctuation, "hey" or "ok" before it at most.
#[test]
fn only_the_name_is_the_name_alone() {
    use super::super::name_alone;
    for alone in [
        "Lamp.",
        "lamp",
        "LAMP!",
        "Hey Lamp.",
        "hey, lamp?",
        "OK lamp",
        "Okay, Lamp.",
    ] {
        assert!(name_alone(alone), "{alone}");
    }
    for not in [
        "",
        "Hey.",
        "Life.",
        "Lamp, hi.",
        "Lamp lamp",
        "Hi Lamp",
        "Lamp's on.",
        "Lamp, what time is it?",
    ] {
        assert!(!name_alone(not), "{not}");
    }
}

/// The faces: another face is one the eyes saw speak the line and not say
/// the name; nobody seen saying the name tells nothing.
#[test]
fn another_face_is_one_not_seen_saying_the_name() {
    use super::super::another_face;
    let faces = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    assert!(another_face(&faces(&["e/f1"]), &faces(&["e/f2"])));
    assert!(another_face(&faces(&["e/f1"]), &faces(&["e/f1", "e/f2"])));
    assert!(!another_face(&faces(&["e/f1"]), &faces(&["e/f1"])));
    assert!(!another_face(&faces(&["e/f1", "e/f2"]), &faces(&["e/f2"])));
    assert!(!another_face(&faces(&["e/f1"]), &faces(&[])));
    assert!(!another_face(&faces(&[]), &faces(&["e/f2"])));
}
