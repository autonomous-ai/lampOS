//! #5826: a turn whose [`MAX_ROUNDS`] rounds all ended in calls is asked
//! once more, told no function ([`WORDS_AFTER_CAP_NOTE`]), so the person
//! hears what the calls found; with no words then either, the plain
//! [`UNFINISHED_LINE`]. A round with words and no call still ends the turn
//! with no round added.
use super::*;

/// One turn as it went.
struct Went {
    /// What was said, as `said:` labels' words.
    said: Vec<String>,
    /// How many calls ran.
    ran: usize,
    /// What each chat declaring only some functions was told.
    told_only: Vec<&'static [&'static str]>,
    /// The messages of every round asked.
    asked: Vec<Vec<Message>>,
}

/// A round of one `service_read` and nothing else, after `words`.
fn reading(id: &str, words: Option<&str>) -> Vec<net::Result<ChatEvent>> {
    let read = call(
        id,
        crate::services::READ,
        json!({"service": "gmail", "what": "search", "query": "2420783"}),
    );
    words
        .map(text)
        .into_iter()
        .chain([Ok(ChatEvent::ToolCalls(vec![read])), Ok(ChatEvent::Done)])
        .collect()
}

/// "Find my email with the number … and tell me what its attachment says"
/// answered by the scripted `chats`, every call answered with one mail.
fn asked(chats: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [Ok(
                "Lamp, find my email with the number 2420783 and tell me what its attachment says."
                    .to_owned(),
            )]
            .into(),
        ),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let ran = Arc::new(AtomicUsize::new(0));
    let counted = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, _, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { json!({"quoted": [{"id": "all7", "subject": "Invoice 2420783"}]}) })
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
    let said = labels
        .iter()
        .filter_map(|label| label.strip_prefix("said:"))
        .map(|said| said.trim().to_owned())
        .collect();
    let told_only = script.told_only.lock().unwrap().clone();
    let asked = script.asked.lock().unwrap().clone();
    Went {
        said,
        ran: ran.load(Ordering::Relaxed),
        told_only,
        asked,
    }
}

fn noted(messages: &[Message]) -> bool {
    messages
        .iter()
        .any(|message| *message == Message::system(WORDS_AFTER_CAP_NOTE))
}

/// The lamp's request 17 on build 11830: four rounds of reads, the first
/// after "I'll check your mail.", and nothing more said. Now a fifth round,
/// declaring no function, says what was read.
#[test]
fn four_rounds_of_calls_are_answered_in_words_by_one_more_round_with_no_function() {
    let answer = "Its attachment is the invoice for order 2420783.";
    let went = asked(vec![
        reading("c1", Some("I'll check your mail.")),
        reading("c2", None),
        reading("c3", None),
        reading("c4", None),
        vec![text(answer), Ok(ChatEvent::Done)],
    ]);
    assert_eq!(went.ran, MAX_ROUNDS);
    assert_eq!(went.said, ["I'll check your mail.", answer]);
    // Only the round past the cap is told no function, and told why.
    assert_eq!(went.told_only, [&[] as &[&str]]);
    assert_eq!(went.asked.len(), MAX_ROUNDS + 1);
    assert!(noted(&went.asked[MAX_ROUNDS]));
    assert!(!went.asked[..MAX_ROUNDS].iter().any(|asked| noted(asked)));
}

/// That round with no words, and a call it was never offered: the call is
/// not run, and the plain line is said rather than nothing.
#[test]
fn a_round_past_the_cap_with_no_words_says_the_plain_line_and_runs_no_call() {
    let went = asked(vec![
        reading("c1", Some("I'll check your mail.")),
        reading("c2", None),
        reading("c3", None),
        reading("c4", None),
        reading("c5", None),
    ]);
    assert_eq!(went.ran, MAX_ROUNDS);
    assert_eq!(went.said, ["I'll check your mail.", UNFINISHED_LINE]);
}

/// A round with its own words and no call ends the turn as before: no
/// round is added, and nothing is told no function.
#[test]
fn a_round_with_words_adds_no_round() {
    let answer = "Nothing in your Gmail came up for that number.";
    let went = asked(vec![
        reading("c1", Some("I'll check your mail.")),
        vec![text(answer), Ok(ChatEvent::Done)],
    ]);
    assert_eq!(went.ran, 1);
    assert_eq!(went.said, ["I'll check your mail.", answer]);
    assert!(went.told_only.is_empty());
    assert_eq!(went.asked.len(), 2);
    assert!(!went.asked.iter().any(|asked| noted(asked)));
}
