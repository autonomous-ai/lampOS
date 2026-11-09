//! #5853, lamp-9adf 9 Nov 17:18: the person's "go ahead and send it" made
//! a `service_write` with its `send_id`, the services answered slowly, and
//! a wordless line cut the turn before the answer. The write was sent, but
//! the held line answering the cut words in its place gave the model no
//! trace of it, so it prepared a new write and read it back: a second
//! "send" would have made the file twice. Now the held line's answer is
//! given the send's own result, and no `service_write` runs in it.
use super::*;

/// What the test's services saw: each `service_write`'s arguments.
type Writes = Arc<Mutex<Vec<Value>>>;

/// One line whose turn calls `first` (answered only after `delay`), cut by
/// a wordless line once the call is under way; the held line's model then
/// calls `service_write` to prepare again, and says "It's in your Drive
/// now.". What the held line's first round was asked, what was said, and
/// every write the services saw.
async fn cut_while_writing(
    first: ToolCall,
    delay: Duration,
) -> (Vec<Message>, Vec<String>, Vec<Value>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Go ahead and send it.".to_owned()), Ok(String::new())].into()),
        chats: Mutex::new(
            [
                vec![Ok(ChatEvent::ToolCalls(vec![first])), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "title": "Notes", "text": "Buy milk"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("It's in your Drive now."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let writes: Writes = Arc::default();
    let seen = writes.clone();
    let (writing, written) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, name, args| {
        let writing = writing.clone();
        let sending = args.get("send_id").is_some();
        if name == crate::services::WRITE {
            seen.lock().unwrap().push(args);
        }
        Box::pin(async move {
            let _ = writing.send(());
            if sending {
                tokio::time::sleep(delay).await;
                json!({"sent": true, "service": "google_drive", "next": crate::services::SENT_NEXT})
            } else {
                tokio::time::sleep(delay).await;
                json!({"read_back": "A file Notes: Buy milk", "send_id": "s2"})
            }
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    // Until the first call is under way, its answer not yet come.
    loop {
        if written.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    // A wordless line begins, cuts the turn, and hears no words.
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the held line's turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(2),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { request: 2, .. } => break,
            _ => {}
        }
    }
    let asked = script.asked.lock().unwrap();
    let held = asked.get(1).cloned().expect("the held line was asked");
    let writes = writes.lock().unwrap().clone();
    (held, said, writes)
}

/// The held line's model is given the send's call and its "sent" result
/// after the words cut short, is told the write was sent, and its own
/// `service_write` is answered as already sent, never run: the services
/// saw the one send and nothing prepared again.
#[tokio::test]
async fn a_send_cut_in_flight_is_answered_with_its_result_and_never_prepared_again() {
    let send = call(
        "c1",
        crate::services::WRITE,
        json!({"service": "google_drive", "send_id": "s1"}),
    );
    let (held, said, writes) = cut_while_writing(send, Duration::from_millis(300)).await;
    let words = held
        .iter()
        .position(|message| message == &Message::user("Go ahead and send it."))
        .expect("the words cut short are asked again");
    let carried = &held[words + 1..];
    assert!(
        carried.first().is_some_and(|message| message
            .tool_calls
            .iter()
            .any(|call| call["function"]["name"] == crate::services::WRITE)),
        "the send's call follows the words: {carried:?}"
    );
    assert!(
        carried.get(1).is_some_and(|message| {
            message.tool_call_id.as_deref() == Some("c1")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))
        }),
        "and its sent result: {carried:?}"
    );
    assert!(
        held.iter().any(|message| message.role == "system"
            && message
                .words()
                .is_some_and(|note| note.contains("already sent"))),
        "the model is told the write was sent: {held:?}"
    );
    assert_eq!(
        writes,
        [json!({"service": "google_drive", "send_id": "s1"})],
        "the services saw the one send, nothing prepared again"
    );
    assert_eq!(said, ["It's in your Drive now."]);
}

/// Control: with no send in flight (the cut call only prepares), the held
/// line is asked with the words alone, as before, and its write runs.
#[tokio::test]
async fn a_prepare_cut_in_flight_is_answered_as_before() {
    let prepare = call(
        "c1",
        crate::services::WRITE,
        json!({"service": "google_drive", "title": "Notes", "text": "Buy milk"}),
    );
    let (held, said, writes) = cut_while_writing(prepare, Duration::from_millis(300)).await;
    assert_eq!(
        held.last(),
        Some(&Message::user("Go ahead and send it.")),
        "the words alone, nothing carried: {held:?}"
    );
    assert!(
        held.iter().all(|message| !message
            .words()
            .is_some_and(|note| note.contains("already sent"))),
        "no sent note: {held:?}"
    );
    assert_eq!(
        writes.len(),
        2,
        "the held line's own write runs: {writes:?}"
    );
    assert_eq!(said, ["It's in your Drive now."]);
}
