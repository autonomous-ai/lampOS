//! `LAMP_NOT_FOR_ME_WORDS`: "not for me" written as words on a line told it
//! may be people's talk (lamp-4ace, r06, request 131).
use super::*;

/// What was heard on the line answered in these tests.
const CUTE: &str = "That lamp is cute, where did you get it?";

/// One spoken line, with the switch `on`, told it may be people's talk
/// (`alone`, `worker::CONVERSATION_ALONE`) or not, answered with `events`:
/// its labels and the verdicts told.
async fn answered(
    on: bool,
    alone: bool,
    events: Vec<net::Result<ChatEvent>>,
) -> (Vec<String>, Vec<(u64, lamp_msgs::TurnVerdict)>) {
    answered_tagged(on, alone, None, events).await
}

/// [`answered`], with speech tags (`LAMP_SECOND_SPEECH_TAGS`) as `tags`.
async fn answered_tagged(
    on: bool,
    alone: bool,
    tags: Option<bool>,
    events: Vec<net::Result<ChatEvent>>,
) -> (Vec<String>, Vec<(u64, lamp_msgs::TurnVerdict)>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(CUTE.to_owned())].into()),
        chats: Mutex::new([events].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let engine = engine
        .noting_the_conversation_alone(Arc::new(move |_| {
            alone.then_some(worker::CONVERSATION_ALONE)
        }))
        .declining_in_words(on)
        .speaking_tags(tags);
    let (mut engine, told) = telling(engine);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    let told = told.lock().unwrap().clone();
    (labels, told)
}

/// The words a turn's labels say the lamp spoke, joined.
fn spoken(labels: &[String]) -> String {
    labels
        .iter()
        .filter_map(|label| label.strip_prefix("said:"))
        .collect::<Vec<_>>()
        .concat()
}

/// A reply streamed in `pieces`, then its end.
fn pieces(pieces: &[&str]) -> Vec<net::Result<ChatEvent>> {
    pieces
        .iter()
        .map(|piece| text(piece))
        .chain([Ok(ChatEvent::Done)])
        .collect()
}

fn decline() -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            NOT_FOR_ME,
            json!({}),
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// Request 131: on a line told it may be people's talk, "Not for me —
/// that's about me, not to me." written as words, however it is cut into
/// pieces, is the decline: nothing said, no announcement, and the line told
/// held back, exactly as the `not_for_me` call ends it.
#[tokio::test]
async fn not_for_me_written_as_words_alone_is_the_decline() {
    let (called, called_told) = answered(false, true, decline()).await;
    assert_eq!(
        called,
        [
            "started:1".to_owned(),
            format!("heard:{CUTE}"),
            "complete".to_owned()
        ]
    );
    assert_eq!(called_told, [(1, lamp_msgs::TurnVerdict::HeldBack)]);
    for cut in [
        &["Not for me \u{2014} that's about me, not to me."][..],
        &[
            "Not",
            " for",
            " me",
            " \u{2014} that's about me,",
            " not to me.",
        ],
        &["\u{201c}not for", " me.\u{201d}"],
        &["Not for me"],
        &["Not for me", " "],
        &["Not for me."],
        &["Not for me", " to answer."],
        &["Not for me to answer"],
        &["Not for me to ", "answer", "\u{201d}", "."],
        &["Not for me, ", "not to ", "me."],
    ] {
        let (labels, told) = answered(true, true, pieces(cut)).await;
        assert_eq!(labels, called, "{cut:?}");
        assert_eq!(told, called_told, "{cut:?}");
    }
    // A speech tag before the words (`LAMP_SECOND_SPEECH_TAGS`), whole or
    // cut, is no opening of its own.
    for cut in [
        &["[softly] Not for me."][..],
        &["[sof", "tly]", " Not for", " me."],
    ] {
        for tags in [Some(true), Some(false), None] {
            let (labels, told) = answered_tagged(true, true, tags, pieces(cut)).await;
            assert_eq!(labels, called, "{cut:?}, tags {tags:?}");
            assert_eq!(told, called_told, "{cut:?}, tags {tags:?}");
        }
    }
}

/// "Not for me to say" and "Not for me to answer, but…": the idiom opens
/// a real answer, said on a line told it may be people's talk however it
/// is cut, the "to" or the comma in a later piece included; and a speech
/// tag before ordinary words leaves them said.
#[tokio::test]
async fn not_for_me_to_say_and_a_tag_before_words_are_said() {
    for idiom in [
        "Not for me to say, but it's lovely.",
        "Not for me to answer, but I think it's lovely.",
    ] {
        let words: Vec<&str> = idiom.split_inclusive(' ').collect();
        let (head, tail) = idiom.split_at(idiom.find(',').unwrap());
        for cut in [
            &[idiom][..],
            &words,
            &["Not for me", &idiom["Not for me".len()..]],
            &[head, tail],
        ] {
            let (labels, told) = answered(true, true, pieces(cut)).await;
            assert_eq!(spoken(&labels), idiom, "{cut:?}: {labels:?}");
            assert!(told.is_empty(), "{cut:?}: {told:?}");
        }
    }
    let (labels, told) =
        answered_tagged(true, true, Some(false), pieces(&["[softly] Sure."])).await;
    assert_eq!(spoken(&labels), "Sure.", "{labels:?}");
    assert!(told.is_empty(), "{told:?}");
}

/// The same words on a line not told it may be people's talk (the name, a
/// call, a turned face or an answer to the lamp's own question, none of
/// which earns `worker::CONVERSATION_ALONE`: see the worker's
/// `a_line_let_out_only_as_the_conversation_going_on_earns_the_note`), or
/// with the switch off, are said as before; so is a reply alone that only
/// begins like them.
#[tokio::test]
async fn not_for_me_words_are_said_unless_alone_with_the_switch() {
    for (on, alone) in [(true, false), (false, true), (false, false)] {
        let (labels, told) =
            answered(on, alone, pieces(&["Not", " for", " me,", " thanks."])).await;
        assert_eq!(
            spoken(&labels),
            "Not for me, thanks.",
            "on {on}, alone {alone}: {labels:?}"
        );
        assert_eq!(labels.last().unwrap(), "complete");
        assert!(told.is_empty(), "on {on}, alone {alone}: {told:?}");
    }
    for reply in [
        "Nothing for me to add, but it is lovely.",
        "Note for me: ask them.",
        "Not sure, maybe the shop downstairs.",
        "Not for",
        // A follow-up to the lamp ("do you like jazz?") answered: said.
        "Not for me, thanks, I'm more of a lo-fi lamp.",
        "Not for me either.",
        // "Not." waits past "For " for "example", which decides it.
        "Not. For example, it was a gift.",
    ] {
        let cut: Vec<&str> = reply.split_inclusive(' ').collect();
        let (labels, told) = answered(true, true, pieces(&cut)).await;
        assert_eq!(spoken(&labels), reply, "{labels:?}");
        assert!(told.is_empty(), "{reply}: {told:?}");
    }
}

/// Request 86: words then the `not_for_me` call end the turn in silence, as
/// before, with the switch on or off and on any line.
#[tokio::test]
async fn words_then_the_not_for_me_call_are_unchanged() {
    for words in ["Sure", "Not for me."] {
        for (on, alone) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut events = vec![text(words)];
            events.extend(decline());
            let (labels, told) = answered(on, alone, events).await;
            assert_eq!(
                labels,
                [
                    "started:1".to_owned(),
                    format!("heard:{CUTE}"),
                    "complete".to_owned()
                ],
                "{words}: on {on}, alone {alone}"
            );
            assert_eq!(told, [(1, lamp_msgs::TurnVerdict::HeldBack)]);
        }
    }
}

/// What the check costs the first sound: typical replies to a line told it
/// may be people's talk, streamed a piece (a word) every 40 ms and cut at
/// the first clause or at four words (`LAMP_SECOND_FIRST_WORDS=4`, as
/// lamp-4ace runs), each timed
/// on the test's clock from the line's end to its first audio (the
/// announcement, before the words' own), with the switch off and on. A
/// first word other than "not" decides the opening at once, so nothing
/// waits; "Not sure, …" waits one piece, 40 ms, for "sure", and an answer
/// opening "Not for me" waits for the word after it. Shown with
/// `--nocapture`.
#[tokio::test(start_paused = true)]
async fn the_check_costs_the_first_audio_only_the_pieces_its_opening_waits_for() {
    const GAP: Duration = Duration::from_millis(40);
    async fn first_said(on: bool, first_words: Option<usize>, reply: &str) -> Duration {
        let cut: Vec<&str> = reply.split_inclusive(' ').collect();
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(CUTE.to_owned())].into()),
            chats: Mutex::new([pieces(&cut)].into()),
            piece_gap: Some(GAP),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine
            .noting_the_conversation_alone(Arc::new(|_| Some(worker::CONVERSATION_ALONE)))
            .declining_in_words(on);
        if let Some(words) = first_words {
            engine = engine.cutting_first_words(words);
        }
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let ended = tokio::time::Instant::now();
        loop {
            match engine.next().await {
                // The first sound itself: production emits a piece's
                // words before its audio.
                ProviderOutput::Audio { samples, .. } if !samples.is_empty() => {
                    return ended.elapsed();
                }
                ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. } => {
                    panic!("{reply}: nothing said")
                }
                _ => {}
            }
        }
    }
    for reply in [
        "Sure, it was a gift from a friend.",
        "Thanks! It came from the shop downstairs.",
        "I think it was ordered online.",
        "No, I don't know where it came from.",
        "Not sure, maybe ask the office.",
        "Nothing special, just a desk lamp.",
        "Haha, I'm glad you like it.",
        // Answers that open with the words themselves.
        "Not for me, thanks, I'm more of a lo-fi lamp.",
        "Not for me to say, but it's lovely.",
        "Not for me to answer, but I think it's lovely.",
        "Not for me either.",
    ] {
        // The pieces past the first that the opening waits for: up to the
        // one that decides it.
        let cut: Vec<&str> = reply.split_inclusive(' ').collect();
        let deciding = (1..=cut.len())
            .find(|&n| crate::not_speech::opens_not_for_me(&cut[..n].concat(), false).is_some())
            .unwrap_or(cut.len());
        let held = GAP * (deciding as u32 - 1);
        for first_words in [None, Some(4)] {
            let off = first_said(false, first_words, reply).await;
            let on = first_said(true, first_words, reply).await;
            eprintln!(
                "LAMP_NOT_FOR_ME_WORDS first audio, first words {first_words:?}: off {} ms, on {} ms: {reply}",
                off.as_millis(),
                on.as_millis()
            );
            assert_eq!(on, off + held, "{first_words:?}: {reply}");
        }
    }
}
