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
/// Each line kept for the one-speaker check before it was judged: its
/// request and window.
type Kept = Arc<Mutex<Vec<(u64, (u64, u64))>>>;

/// One face seen speaking both, as the rule would say it.
const ONE_FACE: Joined = Joined::Same("one face seen speaking both");

/// [`lamp_said`], one person shown to have said both.
fn lamp(script: &Arc<Script>, on: bool, judged: &Judged) -> (Engine, Arc<Mutex<Instant>>) {
    lamp_said(script, on, judged, Some(ONE_FACE), &Kept::default())
}

/// The engine as the lamp runs it: short sounds checked, the switch as
/// given, a rule that holds a line whose words don't begin with the name,
/// and `said` for whether one person said the name and the line after it
/// (none: nothing tells it). The name note is the worker's own reading of
/// it (`worker::Heeding::note_the_name`): given when the rule's window for
/// the line takes in the name's hearing.
fn lamp_said(
    script: &Arc<Script>,
    on: bool,
    judged: &Judged,
    said: Option<Joined>,
    kept: &Kept,
) -> (Engine, Arc<Mutex<Instant>>) {
    let (engine, _, now) = scripted(script.clone());
    let seen = judged.clone();
    let windows = judged.clone();
    let mut engine = engine.checking_sounds(true).carrying_the_name(on);
    if let Some(said) = said {
        let kept = kept.clone();
        engine = engine.telling_one_speaker(
            Arc::new(move |_, _| said.clone()),
            Arc::new(move |request, window| kept.lock().unwrap().push((request, window))),
        );
    }
    let engine = engine
        .heeding(Arc::new(move |request, window, words, _, _| {
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
/// 2 s, past its 1.5 s, this route puts them before it. A name misheard,
/// with the coordinator's `Called`, carries the same, "Lamp, " before the
/// line; and so does the name with more, its words before the line.
#[tokio::test(start_paused = true)]
async fn a_line_after_the_lamps_name_alone_is_said_by_it() {
    let note = Message::system(crate::called::NOTE);
    for (name, called, after, asked_as) in [
        ("Lamp.", false, 1_200, "Lamp. I need a gift for my sister."),
        ("Lamp.", false, 2_000, "Lamp. I need a gift for my sister."),
        (
            "Hey lamp",
            false,
            2_000,
            "Hey lamp I need a gift for my sister.",
        ),
        ("Life.", true, 1_200, "Lamp, I need a gift for my sister."),
        ("Louvre,", true, 2_400, "Lamp, I need a gift for my sister."),
        (
            "Lamp. So",
            true,
            2_000,
            "Lamp. So I need a gift for my sister.",
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
    // Both lines are kept for the one-speaker check, each with its own
    // window, the name's first.
    let played = script(&["Lamp.", "I need a gift for my sister."]);
    let (judged, kept) = (Judged::default(), Kept::default());
    let (mut engine, now) = lamp_said(&played, true, &judged, Some(ONE_FACE), &kept);
    say_the_name(&mut engine, false, true);
    say_after(&mut engine, &now, Duration::from_millis(2_000)).await;
    turn(&mut engine).await;
    assert_eq!(
        kept.lock().unwrap().clone(),
        [
            (1, (NAME_READ_US, NAME_READ_US)),
            (2, (NAME_READ_US + 2_000_000, NAME_READ_US + 2_000_000))
        ]
    );
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

/// A short sound that doesn't begin with the name is still held at once
/// without the coordinator's `Called`: a sound, a word that isn't the name,
/// "So I"; and a line that begins with the name and goes on past 3 words
/// is answered at once, no wait.
#[tokio::test(start_paused = true)]
async fn a_short_sound_that_is_no_name_is_still_held() {
    for (heard, called, sound) in [
        ("Hmm.", false, true),
        ("Life.", false, true),
        ("So I", false, true),
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

/// A short line begun with the name ("Lamp. So", held as a short sound in
/// the played round, the rest of the request then held for no name) waits
/// as the name alone does: the rest, 1.0 s later, is asked as one named
/// line, the short line's words before its own. With nothing after it, the
/// short line is answered as it is, not as a call. "So I", with no name
/// and no `Called`, is held at once as before, and so is the rest.
#[tokio::test(start_paused = true)]
async fn a_short_line_begun_with_the_name_carries_it_too() {
    let played = script(&["Lamp. So", "I have a meeting at three, any tips?"]);
    let judged = Judged::default();
    let (mut engine, now) = lamp(&played, true, &judged);
    say_the_name(&mut engine, false, true);
    say_after(&mut engine, &now, Duration::from_millis(1_000)).await;
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let asked_once = asked(&played);
    assert_eq!(asked_once.len(), 1, "the short line got no answer");
    assert_eq!(
        last_words(&asked_once[0]),
        Some(&Message::user(
            "Lamp. So I have a meeting at three, any tips?"
        ))
    );
    assert!(asked_once[0].contains(&Message::system(crate::called::NOTE)));
    let judged = judged.lock().unwrap().clone();
    assert_eq!(judged.len(), 1, "the short line is never judged");
    assert_eq!((judged[0].0, (judged[0].1).0), (2, NAME_READ_US));
    // Nothing after it: answered as it is, no sooner than 2.5 s.
    let alone = script(&["Lamp, can"]);
    let judged = Judged::default();
    let (mut engine, _) = lamp(&alone, true, &judged);
    say_the_name(&mut engine, false, true);
    tokio::time::sleep(Duration::from_millis(2_400)).await;
    assert!(asked(&alone).is_empty(), "still waiting at 2.4 s");
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let asked_alone = asked(&alone);
    assert_eq!(
        last_words(&asked_alone[0]),
        Some(&Message::user("Lamp, can"))
    );
    assert!(
        !asked_alone[0].contains(&Message::system(crate::provider_worker::CALLED_NOTE)),
        "not a call"
    );
    // No name, no `Called`: as before.
    let played = script(&["So I", "have a meeting at three, any tips?"]);
    let judged = Judged::default();
    let (mut engine, now) = lamp(&played, true, &judged);
    say_the_name(&mut engine, false, true);
    assert_eq!(
        turn_within(&mut engine, Duration::from_millis(100))
            .await
            .last()
            .unwrap(),
        "complete",
        "held at once"
    );
    say_after(&mut engine, &now, Duration::from_millis(1_000)).await;
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert!(asked(&played).is_empty());
    let judged = judged.lock().unwrap().clone();
    assert_eq!(
        judged
            .last()
            .map(|(request, (began, _), words)| (*request, *began, words.as_str())),
        Some((
            2,
            NAME_READ_US + 1_000_000,
            "have a meeting at three, any tips?"
        ))
    );
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
            soft_lead_us: 0,
        });
        engine.submit(audio_at(2, 3_000_000));
        engine.submit(ended(2, false));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        assert!(asked(&played).is_empty(), "{session} {after_stop}");
    }
}

/// Not without a sign of one speaker (El's P1s): two faces, two voices,
/// a busy or unknown room with nothing else, or no rule to tell it. The
/// line after the name is judged as its own, with its own words and
/// window, and held as it would be; the name, dropped by it, is answered
/// in its held place as words cut short are, as the name ("Lamp." for a
/// name misheard).
#[tokio::test(start_paused = true)]
async fn a_name_carries_only_on_a_sign_of_one_speaker() {
    for (name, called, said, asked_as) in [
        (
            "Lamp.",
            false,
            Some(Joined::Not("two faces were seen speaking them".to_owned())),
            Some("Lamp."),
        ),
        (
            "Life.",
            true,
            Some(Joined::Not("two voices (cosine 0.12)".to_owned())),
            Some("Lamp."),
        ),
        ("Lamp. So", false, None, Some("Lamp. So")),
        (
            "Lamp.",
            false,
            Some(Joined::NoSecond("no sign of a second person".to_owned())),
            None,
        ),
    ] {
        let played = script(&[name, "I need a gift for my sister."]);
        let judged = Judged::default();
        let (mut engine, now) = lamp_said(&played, true, &judged, said.clone(), &Kept::default());
        say_the_name(&mut engine, called, true);
        say_after(&mut engine, &now, Duration::from_millis(2_000)).await;
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let judged = judged.lock().unwrap().clone();
        let asked = asked(&played);
        match asked_as {
            Some(asked_as) => {
                assert_eq!(
                    judged.last().map(|(request, (began, _), words)| (
                        *request,
                        *began,
                        words.as_str()
                    )),
                    Some((2, NAME_READ_US + 2_000_000, "I need a gift for my sister.")),
                    "{name} {said:?}: judged as its own"
                );
                assert_eq!(asked.len(), 1, "{name} {said:?}");
                assert_eq!(
                    last_words(&asked[0]),
                    Some(&Message::user(asked_as)),
                    "{name} {said:?}: the name in the held line's place"
                );
                // It is told words came after it, never which, and asks.
                assert!(
                    asked[0].contains(&Message::system(
                        crate::second_provider::worker::NAME_THEN_HELD
                    )),
                    "{name} {said:?}: told words came after the name"
                );
                assert!(
                    !asked[0]
                        .iter()
                        .any(|message| format!("{message:?}").contains("gift for my sister")),
                    "{name} {said:?}: the held words are never told"
                );
            }
            None => {
                assert_eq!(
                    last_words(&asked[0]),
                    Some(&Message::user("Lamp. I need a gift for my sister.")),
                    "no sign of a second person, as the rule says it in a quiet room"
                );
                assert!(
                    !asked[0].contains(&Message::system(
                        crate::second_provider::worker::NAME_THEN_HELD
                    )),
                    "carried: nothing was held"
                );
            }
        }
    }
}

/// El's P2: the coordinator's call whose words are not back when the next
/// line begins is kept running for them, its newer owner's authority
/// retiring it first or not, and the line is said by it once they come
/// within 2.5 s of its end. The call itself says nothing. Words later than
/// that carry nothing, and the line is judged as its own.
#[tokio::test(start_paused = true)]
async fn a_calls_words_back_after_the_next_line_began_still_carry() {
    for (retired, back_after, carried) in
        [(false, 600, true), (true, 600, true), (true, 3_000, false)]
    {
        let played = Arc::new(Script {
            finish_after: Some(Duration::from_millis(back_after)),
            ..Arc::into_inner(script(&["Life.", "I need a gift for my sister."])).unwrap()
        });
        let judged = Judged::default();
        let (mut engine, now) = lamp(&played, true, &judged);
        say_the_name(&mut engine, true, true);
        tokio::time::sleep(Duration::from_millis(300)).await;
        *now.lock().unwrap() += Duration::from_millis(300);
        if retired {
            engine.retire_through(1);
        }
        engine.submit(start(2));
        engine.submit(audio_at(2, NAME_READ_US + 300_000));
        engine.submit(ended(2, false));
        let heard = turn_within(&mut engine, Duration::from_secs(5)).await;
        assert_eq!(heard.last().unwrap(), "complete", "{retired} {back_after}");
        assert!(
            !heard.iter().any(|label| label == "heard:Life."),
            "{retired} {back_after}: the call says nothing: {heard:?}"
        );
        let asked = asked(&played);
        if carried {
            assert_eq!(asked.len(), 1, "{retired} {back_after}");
            assert_eq!(
                last_words(&asked[0]),
                Some(&Message::user("Lamp, I need a gift for my sister.")),
                "{retired} {back_after}"
            );
            let judged = judged.lock().unwrap().clone();
            assert_eq!((judged[0].0, (judged[0].1).0), (2, NAME_READ_US));
        } else {
            assert!(asked.is_empty(), "{retired} {back_after}: {asked:?}");
        }
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

/// A short line begun with the name: the name first, set off as a call,
/// at most 3 words after it, and no question it ends ("Lamp?" is still the
/// name alone).
#[test]
fn a_short_line_begun_with_the_name_is_told_apart() {
    use super::super::name_first;
    for short in [
        "Lamp.",
        "Hey Lamp",
        "Lamp. So",
        "Lamp, can",
        "Hey Lamp, what's",
        "Lamp, what's the",
        "OK lamp, um, so",
        "Lamp?",
        "Lamp. So.",
        "Lamp, thanks.",
    ] {
        assert!(name_first(short), "{short}");
    }
    for not in [
        "",
        "So I",
        "Life.",
        "Lamp is broken.",
        "Lamp's on.",
        "Lamp, what time is it?",
        "Hey Lamp, what's the time?",
        "I need the lamp.",
        "Lamp, stop.",
        "Lamp, wait",
        "Hey Lamp, never mind",
        "Lamp, lights off.",
        "Lamp, what?",
        "Lamp, music on!",
    ] {
        assert!(!name_first(not), "{not}");
    }
}

/// El's P2: a whole short command begun with the name is answered at once,
/// never held 2.5 s for a line after it: a stop ("Lamp, stop."), with the
/// coordinator's `Called` too, and a sentence ended past the name ("Lamp,
/// lights off."). "Lamp. So" still waits. (A stop is done by the stop
/// words before any of this; a line marked a short sound that isn't one
/// of the short words is held as before.)
#[tokio::test(start_paused = true)]
async fn a_whole_short_command_is_answered_at_once() {
    for (heard, called) in [
        ("Lamp, stop.", false),
        ("Lamp, stop.", true),
        ("Lamp, lights off.", false),
    ] {
        let at_once = script(&[heard]);
        let judged = Judged::default();
        let (mut engine, _) = lamp(&at_once, true, &judged);
        say_the_name(&mut engine, called, false);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let asked = asked(&at_once);
        assert_eq!(asked.len(), 1, "{heard} {called}: asked at once");
        assert_eq!(last_words(&asked[0]), Some(&Message::user(heard)));
        turn(&mut engine).await;
    }
    let waits = script(&["Lamp. So"]);
    let judged = Judged::default();
    let (mut engine, _) = lamp(&waits, true, &judged);
    say_the_name(&mut engine, false, true);
    tokio::time::sleep(Duration::from_millis(2_400)).await;
    assert!(asked(&waits).is_empty(), "\"Lamp. So\" still waits");
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(asked(&waits).len(), 1);
}

/// El's P3: the name's words, waiting as words cut short because the line
/// after it was not shown to be its person's, are never joined to that
/// line, even when the join itself sees no sign of a second person: the
/// line let out is asked with its own words alone, as it is when it is
/// said by the name and begins with it already.
#[tokio::test(start_paused = true)]
async fn a_refused_names_words_are_never_joined_to_the_line() {
    let line = "Lamp, what's the weather in Hanoi tomorrow?";
    for refused in [true, false] {
        let played = script(&["Lamp. So", line]);
        let judged = Judged::default();
        let said = if refused {
            Joined::Not("one face in view at each, but not both in a quiet room".to_owned())
        } else {
            ONE_FACE
        };
        let (engine, now) = lamp_said(&played, true, &judged, Some(said), &Kept::default());
        let mut engine = engine.joining_cut_words(Arc::new(|_, _| {
            Joined::NoSecond("no sign of a second person: 1 face in view at each".to_owned())
        }));
        say_the_name(&mut engine, false, true);
        say_after(&mut engine, &now, Duration::from_millis(2_000)).await;
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let asked = asked(&played);
        assert_eq!(asked.len(), 1, "{refused}");
        assert_eq!(
            last_words(&asked[0]),
            Some(&Message::user(line)),
            "{refused}"
        );
    }
}

/// With `LAMP_JOIN_CUT_WORDS` too: a chain goes on across a carried name.
/// "Lamp." alone, then "I've been tired lately." said by it, then "Like,
/// every afternoon," and "why?", each begun while the one before was still
/// being answered, are asked as one named line, the name's words first.
#[tokio::test(start_paused = true)]
async fn a_request_in_pieces_after_the_name_is_asked_as_one_named_line() {
    let pieces = [
        "Lamp.",
        "I've been tired lately.",
        "Like, every afternoon,",
        "why?",
    ];
    let searching_chat = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c",
                "web_search",
                json!({"query": "tired"}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let played = Arc::new(Script {
        heard: Mutex::new(pieces.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(
            [
                searching_chat(),
                searching_chat(),
                vec![text("Maybe your sleep."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    // A search under way for each piece but the last: none of its answer
    // is heard before the next piece begins.
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            std::future::pending::<()>().await;
            json!({})
        })
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        played.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .checking_sounds(true)
    .carrying_the_name(true)
    .telling_one_speaker(Arc::new(|_, _| ONE_FACE), Arc::new(|_, _| {}))
    .joining_cut_words(Arc::new(|_, _| ONE_FACE))
    .heeding(Arc::new(|_, _, _, _, _| None));
    say_the_name(&mut engine, false, true);
    say_after(&mut engine, &now, Duration::from_millis(1_200)).await;
    for request in 3..=4 {
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while searched.try_recv().is_err() {
            assert!(
                std::time::Instant::now() < until,
                "no search before {request}"
            );
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
        *now.lock().unwrap() += Duration::from_millis(1_500);
        engine.submit(start(request));
        engine.retire_through(request - 1);
        engine.submit(audio_at(request, NAME_READ_US + request * 1_500_000));
        engine.submit(ended(request, false));
    }
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the last piece's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: 4, .. }) {
            break;
        }
    }
    let asked = asked(&played);
    assert_eq!(asked.len(), 3, "the name got no answer of its own");
    assert_eq!(
        last_words(&asked[0]),
        Some(&Message::user("Lamp. I've been tired lately."))
    );
    assert_eq!(
        last_words(&asked[2]),
        Some(&Message::user(
            "Lamp. I've been tired lately. Like, every afternoon, why?"
        ))
    );
}

/// Reviewer-three's P2 (23 Oct): a line said by the name goes on only from
/// the name, never from a line before it, often the end of other talk.
/// "yeah, I'll send it after lunch", still being answered, then "Lamp."
/// and "what's on my calendar?", one face seen speaking each: the named
/// line is asked without the talk before the name.
#[tokio::test(start_paused = true)]
async fn a_line_said_by_the_name_never_goes_on_from_talk_before_the_name() {
    let played = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("yeah, I'll send it after lunch".to_owned()),
                Ok("Lamp.".to_owned()),
                Ok("what's on my calendar?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c",
                        "web_search",
                        json!({"query": "lunch"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("A meeting at three."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            std::future::pending::<()>().await;
            json!({})
        })
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        played.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .checking_sounds(true)
    .carrying_the_name(true)
    .telling_one_speaker(Arc::new(|_, _| ONE_FACE), Arc::new(|_, _| {}))
    .joining_cut_words(Arc::new(|_, _| ONE_FACE))
    .heeding(Arc::new(|_, _, _, _, _| None));
    engine.submit(start(1));
    engine.submit(audio_at(1, NAME_READ_US / 5));
    engine.submit(ended(1, false));
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while searched.try_recv().is_err() {
        assert!(std::time::Instant::now() < until, "no search for the talk");
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio_at(2, NAME_READ_US));
    engine.submit(ended(2, true));
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    *now.lock().unwrap() += Duration::from_millis(1_200);
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio_at(3, NAME_READ_US + 1_200_000));
    engine.submit(ended(3, false));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the named line's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: 3, .. }) {
            break;
        }
    }
    let asked = asked(&played);
    assert_eq!(
        last_words(asked.last().unwrap()),
        Some(&Message::user("Lamp. what's on my calendar?"))
    );
}

/// #4226 (busy_cook's c11 on lamp-4ace, played again on lamp-c0a4 on 29
/// Oct): a short sound held with no words, then "Lamp, I want to cook
/// something new this weekend." 1 s after it. The held sound goes first in
/// the line's stream, but the line is judged from its own first sound: the
/// name said at its start is at its start, not seconds into it from when
/// the held sound began.
#[tokio::test(start_paused = true)]
async fn a_sound_carried_with_no_name_leaves_the_line_dated_from_its_own_start() {
    let played = script(&["", "Lamp, I want to cook something new this weekend."]);
    let judged = Judged::default();
    let (mut engine, now) = lamp(&played, true, &judged);
    say_the_name(&mut engine, false, true);
    tokio::time::sleep(Duration::from_millis(100)).await;
    say_after(&mut engine, &now, Duration::from_millis(1_000)).await;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the line after's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: 2, .. }) {
            break;
        }
    }
    let windows = judged.lock().unwrap().clone();
    let (_, window, words) = windows
        .iter()
        .find(|(request, _, _)| *request == 2)
        .expect("the line after is judged");
    assert_eq!(words, "Lamp, I want to cook something new this weekend.");
    assert_eq!(window.0, NAME_READ_US + 1_000_000, "{windows:?}");
}

/// Each line's soft lead as the rule was given it: its request and lead.
type Leads = Arc<Mutex<Vec<(u64, u64)>>>;

/// [`lamp`], each line's soft lead kept as the rule was given it: the same
/// rule, given again.
fn lamp_leads(
    script: &Arc<Script>,
    judged: &Judged,
    leads: &Leads,
) -> (Engine, Arc<Mutex<Instant>>) {
    let (engine, now) = lamp(script, true, judged);
    let (seen, given) = (judged.clone(), leads.clone());
    let engine = engine.heeding(Arc::new(move |request, window, words, _, lead| {
        seen.lock()
            .unwrap()
            .push((request, window, words.to_owned()));
        given.lock().unwrap().push((request, lead));
        (!crate::attention::begins_with_the_name(words)).then_some(crate::attention::NOT_SPOKEN_TO)
    }));
    (engine, now)
}

/// A line's Start, its sound begun `lead_us` before its usual start.
fn start_led(request: u64, lead_us: u64) -> ProviderInput {
    let mut start = start(request);
    if let ProviderInput::Start { soft_lead_us, .. } = &mut start {
        *soft_lead_us = lead_us;
    }
    start
}

async fn completed(engine: &mut Engine, request: u64) {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the line's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: done, .. } if done == request) {
            break;
        }
    }
}

/// #4226: a line's soft lead (`Start`'s `soft_lead_us`) reaches the rule
/// only while the line is judged from its own first sound: a line alone,
/// or one after a short sound carried with no name in it. A line said by
/// the lamp's name before it, its window that line's, is given none: the
/// lead says nothing of where the name's line began.
#[tokio::test(start_paused = true)]
async fn a_lines_soft_lead_reaches_the_rule_only_from_its_own_first_sound() {
    const LEAD: u64 = 300_000;
    let played = script(&["Lamp, what time is it?"]);
    let (judged, leads) = (Judged::default(), Leads::default());
    let (mut engine, _) = lamp_leads(&played, &judged, &leads);
    engine.submit(start_led(1, LEAD));
    engine.submit(audio_at(1, NAME_READ_US));
    engine.submit(ended(1, false));
    completed(&mut engine, 1).await;
    assert_eq!(leads.lock().unwrap().clone(), [(1, LEAD)]);
    // After a short sound carried with no name heard: its own first sound.
    // After the lamp's name alone, carried at 1.2 s or put before the line
    // at 2 s: the name's line's, so none.
    for (first, after, lead) in [("", 1_000, LEAD), ("Lamp.", 1_200, 0), ("Lamp.", 2_000, 0)] {
        let line = if first.is_empty() {
            "Lamp, I want to cook something new this weekend."
        } else {
            "I want to cook something new this weekend."
        };
        let played = script(&[first, line]);
        let (judged, leads) = (Judged::default(), Leads::default());
        let (mut engine, now) = lamp_leads(&played, &judged, &leads);
        say_the_name(&mut engine, false, true);
        if first.is_empty() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let after = Duration::from_millis(after);
        tokio::time::sleep(after).await;
        *now.lock().unwrap() += after;
        engine.submit(start_led(2, LEAD));
        engine.submit(audio_at(2, NAME_READ_US + after.as_micros() as u64));
        engine.submit(ended(2, false));
        completed(&mut engine, 2).await;
        let given = leads.lock().unwrap().clone();
        assert_eq!(
            given.iter().find(|(request, _)| *request == 2),
            Some(&(2, lead)),
            "{first:?} {after:?}: {given:?}"
        );
    }
}
