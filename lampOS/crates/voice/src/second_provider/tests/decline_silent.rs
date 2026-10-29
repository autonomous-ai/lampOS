//! `LAMP_DECLINE_SILENT` (#3945): a line the model declines says nothing,
//! whatever it wrote before its `not_for_me` call, and "not for me" written
//! as words on any line that may not be for the lamp is the decline.
use super::*;

/// What was heard on the line answered in these tests.
const TALK: &str = "That lamp is cute, where did you get it?";

/// What a turn did: its labels, the verdicts told, and the requests the
/// worker was told declined ([`Did::Declined`]).
type Answered = (Vec<String>, Vec<(u64, lamp_msgs::TurnVerdict)>, Vec<u64>);

/// One spoken line, told `note` (`worker::DECLINE_NOTE`,
/// `worker::CONVERSATION_ALONE` or `worker::BY_NAME_NOTE`), answered with
/// `events`, with the switch `on`; `LAMP_NOT_FOR_ME_WORDS` on as the
/// product runs it, and its words cut at four (`LAMP_SECOND_FIRST_WORDS=4`)
/// as lamp-4ace runs. A piece comes every 40 ms, as a stream's chunks do:
/// with none the writing half never waits, and the saying half never
/// takes a piece before the turn ends.
async fn answered(on: bool, note: &'static str, events: Vec<net::Result<ChatEvent>>) -> Answered {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(TALK.to_owned())].into()),
        chats: Mutex::new([events].into()),
        piece_gap: Some(Duration::from_millis(40)),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let declined: Arc<Mutex<Vec<u64>>> = Arc::default();
    let seen = declined.clone();
    let alone = note == worker::CONVERSATION_ALONE;
    let engine = engine
        .noting_the_conversation_alone(Arc::new(move |_| {
            alone.then_some(worker::CONVERSATION_ALONE)
        }))
        .noting_whether_called(Arc::new(move |_, _| Some(note)))
        .declining_in_words(true)
        .declining_silently(on)
        .cutting_first_words(4)
        .telling_what_it_did(Arc::new(move |did| {
            if let Did::Declined(request) = did {
                seen.lock().unwrap().push(request);
            }
        }));
    let (mut engine, told) = telling(engine);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    let told = told.lock().unwrap().clone();
    let declined = declined.lock().unwrap().clone();
    (labels, told, declined)
}

/// The words a turn's labels say the lamp spoke, joined.
fn spoken(labels: &[String]) -> String {
    labels
        .iter()
        .filter_map(|label| label.strip_prefix("said:"))
        .collect::<Vec<_>>()
        .concat()
}

/// A reply streamed in `pieces`, a word each, then the `not_for_me` call
/// if `declined`, then its end.
fn reply(words: &str, declined: bool) -> Vec<net::Result<ChatEvent>> {
    let mut events: Vec<_> = words.split_inclusive(' ').map(text).collect();
    if declined {
        events.push(Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            NOT_FOR_ME,
            json!({}),
        )])));
    }
    events.push(Ok(ChatEvent::Done));
    events
}

/// Nothing said: the line is told held back, and, with the switch, the
/// worker is told it was declined.
fn silent(on: bool) -> Answered {
    (
        vec![
            "started:1".to_owned(),
            format!("heard:{TALK}"),
            "complete".to_owned(),
        ],
        vec![(1, lamp_msgs::TurnVerdict::HeldBack)],
        if on { vec![1] } else { Vec::new() },
    )
}

/// Words streamed before the `not_for_me` call ("Sure, I'll stay out of
/// it.") were said up to the first clause or four words before the call
/// was known: as the lamp's words, which the thread keeps, and to the
/// voice. With the switch nothing is said, on a line told it may not
/// be for the lamp, and on one let out only as the conversation going on.
#[tokio::test(start_paused = true)]
async fn a_decline_with_words_streamed_before_the_call_says_nothing() {
    for note in [worker::DECLINE_NOTE, worker::CONVERSATION_ALONE] {
        for words in [
            "Sure, I'll stay out of it.",
            "Okay. Leaving you two to it.",
            "Sounds like a nice gift from someone",
        ] {
            let (labels, told, declined) = answered(true, note, reply(words, true)).await;
            assert_eq!((labels, told, declined), silent(true), "{words}");
            // Off, its first words are said, as before.
            let (labels, told, declined) = answered(false, note, reply(words, true)).await;
            assert!(!spoken(&labels).is_empty(), "{words}: {labels:?}");
            assert_eq!(told, [(1, lamp_msgs::TurnVerdict::HeldBack)]);
            assert!(declined.is_empty());
        }
    }
}

/// "Not for me…" written as words, with no call, on a line let out by
/// anything but the conversation alone (a face turned to the lamp, a line
/// begun over its answer, one that asks for its work: each told
/// `worker::DECLINE_NOTE`) is said with `LAMP_NOT_FOR_ME_WORDS` alone; with
/// this switch it is the decline. A real answer opening the same way is
/// still said whole.
#[tokio::test(start_paused = true)]
async fn a_not_for_me_written_on_a_line_not_let_out_alone_says_nothing() {
    for words in [
        "Not for me \u{2014} that's about me, not to me.",
        "Not for me.",
        "Not for me to answer.",
    ] {
        let (labels, told, declined) =
            answered(true, worker::DECLINE_NOTE, reply(words, false)).await;
        assert_eq!((labels, told, declined), silent(true), "{words}");
        let (labels, told, _) = answered(false, worker::DECLINE_NOTE, reply(words, false)).await;
        assert_eq!(spoken(&labels), words, "{words}: off");
        assert!(told.is_empty(), "{words}: off");
    }
    for words in [
        "Not for me, thanks, I'm more of a lo-fi lamp.",
        "Sure, it was a gift from a friend.",
        "Not for me to say, but it's lovely.",
    ] {
        let (labels, told, declined) =
            answered(true, worker::DECLINE_NOTE, reply(words, false)).await;
        assert_eq!(spoken(&labels), words, "{labels:?}");
        assert_eq!(labels.last().unwrap(), "complete");
        assert!(told.is_empty() && declined.is_empty(), "{words}");
    }
}

/// The switch off keeps today's behaviour on every line: a line said by
/// name is never held, with it or not; and a reply that is no decline is
/// said in the same words with the switch on or off.
#[tokio::test(start_paused = true)]
async fn the_switch_off_and_a_line_said_by_name_keep_todays_behaviour() {
    for (note, words) in [
        (worker::BY_NAME_NOTE, "It came from the shop downstairs."),
        (worker::DECLINE_NOTE, "It came from the shop downstairs."),
        (worker::CONVERSATION_ALONE, "Haha, I'm glad you like it."),
    ] {
        let (off, off_told, off_declined) = answered(false, note, reply(words, false)).await;
        let (on, on_told, on_declined) = answered(true, note, reply(words, false)).await;
        assert_eq!(spoken(&off), words);
        assert_eq!(spoken(&on), words);
        assert!(off_told.is_empty() && on_told.is_empty());
        assert!(off_declined.is_empty() && on_declined.is_empty());
    }
    // Said by name, words before a decline are not held: as before.
    let words = "Sure, I'll stay out of it.";
    let (by_name, _, _) = answered(true, worker::BY_NAME_NOTE, reply(words, true)).await;
    let (off, _, _) = answered(false, worker::BY_NAME_NOTE, reply(words, true)).await;
    assert_eq!(by_name, off);
}

/// What the switch costs the first sound, timed on the test's clock from
/// the line's end, a piece (a word) every 40 ms, its words cut at four: on
/// a line said by name nothing, and on a line that may not be for the lamp
/// the pieces after the first (round 1's whole reply). Shown with
/// `--nocapture`.
#[tokio::test(start_paused = true)]
async fn the_hold_costs_the_first_audio_only_on_lines_that_may_be_declined() {
    const GAP: Duration = Duration::from_millis(40);
    async fn first_said(on: bool, note: &'static str, reply: &str) -> Duration {
        let cut: Vec<&str> = reply.split_inclusive(' ').collect();
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(TALK.to_owned())].into()),
            chats: Mutex::new(
                [cut.iter()
                    .map(|piece| text(piece))
                    .chain([Ok(ChatEvent::Done)])
                    .collect()]
                .into(),
            ),
            piece_gap: Some(GAP),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine
            .noting_whether_called(Arc::new(move |_, _| Some(note)))
            .declining_silently(on)
            .cutting_first_words(4);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let ended = tokio::time::Instant::now();
        loop {
            match engine.next().await {
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
        "I think it was ordered online, from a shop in town.",
    ] {
        let pieces = reply.split_inclusive(' ').count() as u32;
        let named_off = first_said(false, worker::BY_NAME_NOTE, reply).await;
        let named_on = first_said(true, worker::BY_NAME_NOTE, reply).await;
        let off = first_said(false, worker::DECLINE_NOTE, reply).await;
        let on = first_said(true, worker::DECLINE_NOTE, reply).await;
        eprintln!(
            "LAMP_DECLINE_SILENT first audio: by name off {} ms, on {} ms; may be declined off {} ms, on {} ms: {reply}",
            named_off.as_millis(),
            named_on.as_millis(),
            off.as_millis(),
            on.as_millis()
        );
        assert_eq!(named_on, named_off, "{reply}");
        assert!(on > off, "{reply}");
        assert!(on <= off + GAP * pieces, "{reply}");
    }
}
