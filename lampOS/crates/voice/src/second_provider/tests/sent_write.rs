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
    cut_again_while_writing(first, delay, 1).await
}

/// [`cut_while_writing`], with `cuts` wordless lines, each cutting the one
/// before it while the call is still awaited; the last answers the words.
async fn cut_again_while_writing(
    first: ToolCall,
    delay: Duration,
    cuts: u64,
) -> (Vec<Message>, Vec<String>, Vec<Value>) {
    let heard = std::iter::once(Ok("Go ahead and send it.".to_owned()))
        .chain((0..cuts).map(|_| Ok(String::new())))
        .collect();
    let script = Arc::new(Script {
        heard: Mutex::new(heard),
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
            seen.lock()
                .unwrap()
                .push(crate::services::without_attempt(args));
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
    // A wordless line begins, cuts the turn, and hears no words; each next
    // one cuts the one before it, the call's answer still not come.
    for line in 2..=cuts + 1 {
        if line > 2 {
            let by = tokio::time::Instant::now() + Duration::from_millis(100);
            while tokio::time::Instant::now() < by {
                let _ = tokio::time::timeout_at(by, engine.next()).await;
            }
        }
        engine.submit(start(line));
        engine.retire_through(line - 1);
        engine.submit(audio(line, 1));
        engine.submit(end(line));
    }
    let last = cuts + 1;
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the held line's turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(request),
                text,
                ..
            } if request == last && !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { request, .. } if request == last => break,
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

/// The HOLD on #5863: a second wordless line cuts the first held one while
/// it still waits for the send's answer. The last line is still told the
/// write was sent, given its result, and runs no `service_write`.
#[tokio::test]
async fn a_send_cut_twice_before_its_answer_is_still_answered_as_sent() {
    let send = call(
        "c1",
        crate::services::WRITE,
        json!({"service": "google_drive", "send_id": "s1"}),
    );
    let (held, said, writes) = cut_again_while_writing(send, Duration::from_millis(600), 2).await;
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("c1")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))),
        "the send's sent result goes with the words: {held:?}"
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

/// The 9adf path itself (P3 on #5863): a scoped runner whose send the
/// services made after the cut, its own answer fenced to "not running" as
/// the cut turn's lease is no longer current (`tools::answer_inner`). The
/// held line is told the services' own answer ([`crate::services::send_state`]),
/// "sent", and runs no `service_write`.
#[tokio::test]
async fn a_send_fenced_after_the_cut_is_told_by_the_services_own_answer() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Go ahead and send it.".to_owned()), Ok(String::new())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "send_id": "s1"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "title": "Notes"}),
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
    let fenced = Arc::new(AtomicBool::new(false));
    let fence = fenced.clone();
    let (writing, written) = std::sync::mpsc::channel();
    let plain: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let scoped: ScopedToolRunner = Arc::new(
        move |_, _, name, args, lease: crate::memory_scope::ScopeLease| {
            let writing = writing.clone();
            let fence = fence.clone();
            let marked = args.clone();
            if name == crate::services::WRITE {
                seen.lock().unwrap().push(args);
            }
            Box::pin(async move {
                let _ = writing.send(());
                tokio::time::sleep(Duration::from_millis(300)).await;
                let sent = json!({"sent": true, "service": "google_drive"});
                crate::services::keep_send_for_test(&marked, Some(&sent));
                if lease.is_current() {
                    sent
                } else {
                    fence.store(true, Ordering::SeqCst);
                    crate::tools::unavailable()
                }
            })
        },
    );
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        plain,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .with_memory_scope(
        lamp_turn_owner::BootId::new([9; 16]).unwrap(),
        Arc::new(crate::memory_scope::FaceCache::holding_gaps(None)),
        scoped,
        None,
    );
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        if written.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
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
    assert!(
        fenced.load(Ordering::SeqCst),
        "the cut turn's answer was fenced"
    );
    let held = script.asked.lock().unwrap().get(1).cloned().unwrap();
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("c1")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))),
        "the services' own sent answer goes with the words: {held:?}"
    );
    assert!(
        held.iter().any(|message| message.role == "system"
            && message
                .words()
                .is_some_and(|note| note.contains("already sent"))),
        "the model is told the write was sent: {held:?}"
    );
    assert_eq!(writes.lock().unwrap().len(), 1, "one write, the send");
    assert_eq!(said, ["It's in your Drive now."]);
}

/// The HOLD on #5863 (Third): one batch sends, then tries the same send
/// again, which the gate refuses as nothing waiting, and the cut lands
/// during that second try. The held line is still given the send that
/// went, told it was sent, and runs no `service_write`.
#[tokio::test]
async fn a_refused_second_send_in_one_batch_never_hides_the_first() {
    let send = |id: &str| {
        call(
            id,
            crate::services::WRITE,
            json!({"service": "google_drive", "send_id": "s1"}),
        )
    };
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Go ahead and send it.".to_owned()), Ok(String::new())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![send("c1"), send("duplicate")])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "title": "Notes"}),
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
    let (again, tried) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, name, args| {
        let again = again.clone();
        let mut seen = seen.lock().unwrap();
        if name == crate::services::WRITE {
            seen.push(crate::services::without_attempt(args));
        }
        let attempt = seen.len();
        Box::pin(async move {
            if attempt == 1 {
                return json!({"sent": true, "service": "google_drive", "next": crate::services::SENT_NEXT});
            }
            let _ = again.send(());
            tokio::time::sleep(Duration::from_millis(300)).await;
            json!({"error": crate::services::NOTHING_WAITING})
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
    // Until the second try is under way, its refusal not yet come.
    loop {
        if tried.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
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
    let held = script.asked.lock().unwrap().get(1).cloned().unwrap();
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("c1")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))),
        "the send that went goes with the words: {held:?}"
    );
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("duplicate")),
        "and the batch's refused try with its own result, each call id answered: {held:?}"
    );
    assert!(
        held.iter().any(|message| message.role == "system"
            && message
                .words()
                .is_some_and(|note| note.contains("already sent"))),
        "the model is told the write was sent: {held:?}"
    );
    assert_eq!(
        *writes.lock().unwrap(),
        [
            json!({"service": "google_drive", "send_id": "s1"}),
            json!({"service": "google_drive", "send_id": "s1"}),
        ],
        "the batch's two sends, and no third call: nothing prepared again"
    );
    assert_eq!(said, ["It's in your Drive now."]);
}

/// Second's trace on #5863: an older request's slow send (R) is still
/// under way when a newer write is prepared, read back and sent (S); S is
/// cut, its own answer fenced to "not running", and R's answer comes
/// after S's but before the recovery reads it. The services keep each
/// request's send, so S's held line is still told "sent", and nothing is
/// prepared again.
#[tokio::test]
async fn an_older_slow_send_finishing_late_never_hides_a_newer_one() {
    let send = |id: &str, send_id: &str| {
        call(
            id,
            crate::services::WRITE,
            json!({"service": "google_drive", "send_id": send_id}),
        )
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Go ahead and send it.".to_owned()),
                Ok("Save a note called Notes, buy milk.".to_owned()),
                Ok("Go ahead and send it.".to_owned()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![send("r1", "s0")])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "p1",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "title": "Notes", "text": "Buy milk"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("A file Notes: Buy milk. Say go ahead and send it."),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![send("s1", "s2")])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "p2",
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
    let late = Arc::new(tokio::sync::Notify::new());
    let (sending_s, s_under_way) = std::sync::mpsc::channel();
    let r_done = Arc::new(AtomicBool::new(false));
    let r_finished = r_done.clone();
    let runner: ToolRunner = Arc::new(move |_, _, name, args| {
        if name == crate::services::WRITE {
            seen.lock().unwrap().push(args.clone());
        }
        let marked = args.clone();
        let r_finished = r_finished.clone();
        let late = late.clone();
        let sending_s = sending_s.clone();
        let send_id = args["send_id"].as_str().map(str::to_owned);
        Box::pin(async move {
            match send_id.as_deref() {
                // R: answered only once S's answer is in.
                Some("s0") => {
                    let _ = tokio::time::timeout(Duration::from_secs(3), late.notified()).await;
                    let told = json!({"sent": false, "error": "too slow"});
                    crate::services::keep_send_for_test(&marked, Some(&told));
                    r_finished.store(true, Ordering::SeqCst);
                    told
                }
                // S: sent, then fenced, R finishing in between.
                Some(_) => {
                    let _ = sending_s.send(());
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let sent = json!({"sent": true, "service": "google_drive"});
                    crate::services::keep_send_for_test(&marked, Some(&sent));
                    late.notify_one();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    crate::tools::unavailable()
                }
                None => json!({"read_back": "A file Notes: Buy milk", "send_id": "s2"}),
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
    let until = async |engine: &mut Engine, request: u64| loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: of, .. } if of == request) {
            break;
        }
    };
    // R begins, and a newer line cuts it while its send is under way.
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    while writes.lock().unwrap().is_empty() {
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    until(&mut engine, 2).await;
    // The read-back's "send", cut while S is under way.
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    loop {
        if s_under_way.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    engine.submit(start(4));
    engine.retire_through(3);
    engine.submit(audio(4, 1));
    engine.submit(end(4));
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the held line's turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(4),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { request: 4, .. } => break,
            _ => {}
        }
    }
    assert!(r_done.load(Ordering::SeqCst), "R finished after S");
    let asked = script.asked.lock().unwrap();
    let held = asked.last().unwrap();
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("s1")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))),
        "S's own sent answer goes with the words: {held:?}"
    );
    assert!(
        held.iter().any(|message| message.role == "system"
            && message
                .words()
                .is_some_and(|note| note.contains("already sent"))),
        "the model is told the write was sent: {held:?}"
    );
    assert_eq!(
        writes.lock().unwrap().len(),
        3,
        "R, the prepare and S: nothing prepared again {:?}",
        writes.lock().unwrap()
    );
    assert_eq!(said, ["It's in your Drive now."]);
}

/// Second's HOLD on #5863: a batch sends `c1` to Google Drive, then makes
/// eight more tries naming Gmail with wrong send ids, each refused; the
/// last is cut while it waits. The held line is given `c1` with its own
/// sent result, no refused try shows a success, the note is
/// [`SENT_WRITE_NOTE`], and no `service_write` runs.
#[tokio::test]
async fn a_ninth_send_try_never_takes_the_first_ones_receipt() {
    let mut batch = vec![call(
        "c1",
        crate::services::WRITE,
        json!({"service": "google_drive", "send_id": "s1"}),
    )];
    batch.extend((1..=8).map(|n| {
        call(
            &format!("refused-{n}"),
            crate::services::WRITE,
            json!({"service": "gmail", "send_id": format!("wrong-{n}")}),
        )
    }));
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Go ahead and send it.".to_owned()), Ok(String::new())].into()),
        chats: Mutex::new(
            [
                vec![Ok(ChatEvent::ToolCalls(batch)), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "title": "Notes"}),
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
    let (last, tried) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, name, args| {
        let last = last.clone();
        let marked = args.clone();
        if name == crate::services::WRITE {
            seen.lock().unwrap().push(args.clone());
        }
        let id = args["send_id"].as_str().unwrap_or_default().to_owned();
        Box::pin(async move {
            if id == "s1" {
                let sent = json!({"sent": true, "service": "google_drive"});
                crate::services::keep_send_for_test(&marked, Some(&sent));
                return sent;
            }
            if id == "wrong-8" {
                let _ = last.send(());
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            json!({"error": crate::services::NOTHING_WAITING})
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
    loop {
        if tried.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the held line's turn ends");
        if let ProviderOutput::TurnComplete { request: 2, .. } = event {
            break;
        }
    }
    let held = script.asked.lock().unwrap().get(1).cloned().unwrap();
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("c1")
                && message.words().is_some_and(
                    |told| told.contains("\"sent\":true") && told.contains("google_drive")
                )),
        "c1 goes with its own sent result: {held:?}"
    );
    assert!(
        held.iter().all(|message| !(message
            .tool_call_id
            .as_deref()
            .is_some_and(|id| id.starts_with("refused-"))
            && message
                .words()
                .is_some_and(|told| told.contains("\"sent\":true")))),
        "no refused try shows a success: {held:?}"
    );
    assert!(
        held.iter()
            .any(|message| message.role == "system" && message.words() == Some(SENT_WRITE_NOTE)),
        "the note is the sent one: {held:?}"
    );
    assert_eq!(
        writes.lock().unwrap().len(),
        9,
        "no service_write ran after the batch"
    );
}

/// The P3 on #5863: a finished clock call `call_0` is carried with its
/// round, then a later round's send reuses the id `call_0` and is cut
/// before it is carried. The held line still gets the send's own receipt
/// under that id, beside the clock's, so the sent note matches what it is
/// given.
#[tokio::test]
async fn a_send_reusing_a_carried_calls_id_still_brings_its_receipt() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Go ahead and send it.".to_owned()), Ok(String::new())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "call_0",
                        crate::tools::THE_TIME,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "call_0",
                        crate::services::WRITE,
                        json!({"service": "google_drive", "send_id": "s1"}),
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
    let (sending, under_way) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, name, args| {
        let sending = sending.clone();
        let send = name == crate::services::WRITE;
        if send {
            seen.lock()
                .unwrap()
                .push(crate::services::without_attempt(args));
        }
        Box::pin(async move {
            if !send {
                return json!({"time": "10:00"});
            }
            let _ = sending.send(());
            tokio::time::sleep(Duration::from_millis(300)).await;
            json!({"sent": true, "service": "google_drive"})
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
    loop {
        if under_way.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("the held line's turn ends");
        if let ProviderOutput::TurnComplete { request: 2, .. } = event {
            break;
        }
    }
    let held = script.asked.lock().unwrap().last().cloned().unwrap();
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("call_0")
                && message.words().is_some_and(|told| told.contains("10:00"))),
        "the clock's carried round stays: {held:?}"
    );
    assert!(
        held.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("call_0")
                && message
                    .words()
                    .is_some_and(|told| told.contains("\"sent\":true"))),
        "the send's own receipt goes with the words: {held:?}"
    );
    assert!(
        held.iter()
            .any(|message| message.role == "system" && message.words() == Some(SENT_WRITE_NOTE)),
        "the note matches what is given: {held:?}"
    );
    assert_eq!(
        writes.lock().unwrap().len(),
        1,
        "the one send, nothing prepared again"
    );
}

/// A completed send's receipt must not restore private payloads after an
/// admitted forget is interrupted, but its no-repeat accounting must survive.
#[tokio::test]
async fn a_send_then_cut_forget_then_cut_never_restores_payload_or_repeats_send() {
    const ARG: &str = "private-send-argument-canary";
    const RESULT: &str = "private-send-result-canary";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Go ahead and send it, then forget my face.".to_owned()),
                Ok(String::new()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "sent-private",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "send_id":ARG}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "erase-face",
                        crate::known_faces::FORGET,
                        json!({"name": "A"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "repeat-send",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "title":"Notes", "text":"New copy"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("I will report only what is confirmed."),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let writes = Arc::new(AtomicUsize::new(0));
    let seen = writes.clone();
    let (entered, admissions) = std::sync::mpsc::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let release_wait = Arc::new(Mutex::new(Some(released)));
    let runner: ToolRunner = Arc::new(move |_, _, name, _| {
        let entered = entered.clone();
        if name == crate::services::WRITE {
            seen.fetch_add(1, Ordering::SeqCst);
            let wait = release_wait.lock().unwrap().take();
            Box::pin(async move {
                entered.send("send").unwrap();
                if let Some(wait) = wait {
                    wait.await.unwrap();
                }
                json!({"sent":true,"service":"google_drive","detail":RESULT})
            })
        } else {
            assert_eq!(name, crate::known_faces::FORGET);
            Box::pin(async move {
                entered.send("forget").unwrap();
                std::future::pending::<Value>().await
            })
        }
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
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "send");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the original send is in flight");
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "forget");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the resumed words admit forget and block in its runner");
    let before = script.asked.lock().unwrap().len();
    let prior = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(
        prior.contains(ARG) && prior.contains(RESULT),
        "receipt must really have reached the model"
    );
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                engine.next().await,
                ProviderOutput::TurnComplete { request: 3, .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("the second resumed turn completes");
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "forget must not allow a duplicate send or prepare"
    );
    let asked = script.asked.lock().unwrap();
    assert!(asked.len() > before);
    let resumed = serde_json::to_string(&asked[before..]).unwrap();
    for canary in [ARG, RESULT] {
        assert!(
            !resumed.contains(canary),
            "{canary} restored after interrupted forget: {resumed}"
        );
    }
}

/// An unasked face forget must preserve both the receipt payload and repeat protection.
#[tokio::test]
async fn an_unadmitted_face_forget_keeps_send_payload_and_repeat_protection() {
    const ARG: &str = "private-send-argument-canary";
    const RESULT: &str = "private-send-result-canary";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Go ahead and send it.".to_owned()),
                Ok(String::new()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "sent-private",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "send_id":ARG}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "erase-face",
                        crate::known_faces::FORGET,
                        json!({"name": "A"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "repeat-send",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "title":"Notes", "text":"New copy"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("I will report only what is confirmed."),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        round_after: Mutex::new(
            [
                Duration::ZERO,
                Duration::ZERO,
                Duration::from_secs(60),
                Duration::ZERO,
                Duration::ZERO,
            ]
            .into(),
        ),
        ..Script::default()
    });
    let writes = Arc::new(AtomicUsize::new(0));
    let seen = writes.clone();
    let (entered, admissions) = std::sync::mpsc::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let release_wait = Arc::new(Mutex::new(Some(released)));
    let runner: ToolRunner = Arc::new(move |_, _, name, _| {
        let entered = entered.clone();
        if name == crate::services::WRITE {
            seen.fetch_add(1, Ordering::SeqCst);
            let wait = release_wait.lock().unwrap().take();
            Box::pin(async move {
                entered.send("send").unwrap();
                if let Some(wait) = wait {
                    wait.await.unwrap();
                }
                json!({"sent":true,"service":"google_drive","detail":RESULT})
            })
        } else {
            panic!("unadmitted face forget reached runner: {name}")
        }
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
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "send");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the original send is in flight");
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if script.asked.lock().unwrap().len() == 3 {
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the refused forget is answered before the parked next model round");
    assert!(admissions.try_recv().is_err());
    assert!(
        script.asked.lock().unwrap()[2]
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("erase-face")),
        "the face-forget call must actually be evaluated"
    );
    let before = script.asked.lock().unwrap().len();
    let prior = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(
        prior.contains(ARG) && prior.contains(RESULT),
        "receipt must really have reached the model"
    );
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                engine.next().await,
                ProviderOutput::TurnComplete { request: 3, .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("the second resumed turn completes");
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "forget must not allow a duplicate send or prepare"
    );
    let asked = script.asked.lock().unwrap();
    assert!(asked.len() > before);
    let resumed = serde_json::to_string(&asked[before..]).unwrap();
    for canary in [ARG, RESULT] {
        assert!(
            resumed.contains(canary),
            "{canary} incorrectly scrubbed by unadmitted forget: {resumed}"
        );
    }
}

/// A result arriving after actual forget admission cannot refill scrubbed send payloads.
#[tokio::test]
async fn a_late_send_result_after_forget_updates_status_without_restoring_payload() {
    const CALL_ID: &str = "private-send-call-id-canary";
    const ARG: &str = "private-send-argument-canary";
    const RESULT: &str = "private-send-result-canary";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Go ahead and send it, then forget my face.".to_owned()),
                Ok(String::new()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        CALL_ID,
                        crate::services::WRITE,
                        json!({"service":"google_drive", "send_id":ARG}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "erase-face",
                        crate::known_faces::FORGET,
                        json!({"name": "A"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "repeat-send",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "title":"Notes", "text":"New copy"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("I will report only what is confirmed."),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let writes = Arc::new(AtomicUsize::new(0));
    let seen = writes.clone();
    let (entered, admissions) = std::sync::mpsc::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let release_wait = Arc::new(Mutex::new(Some(released)));
    let runner: ToolRunner = Arc::new(move |_, _, name, _| {
        let entered = entered.clone();
        if name == crate::services::WRITE {
            seen.fetch_add(1, Ordering::SeqCst);
            let wait = release_wait.lock().unwrap().take();
            Box::pin(async move {
                entered.send("send").unwrap();
                if let Some(wait) = wait {
                    wait.await.unwrap();
                }
                json!({"sent":true,"service":"google_drive","detail":RESULT})
            })
        } else {
            assert_eq!(name, crate::known_faces::FORGET);
            Box::pin(async move {
                entered.send("forget").unwrap();
                std::future::pending::<Value>().await
            })
        }
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
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "send");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the original send is in flight");
    // Model expiry without measuring a real 20-second deadline. Only the retained
    // receipt wait expires; the actual send remains blocked on its oneshot.
    let mut late_result = {
        let mut sends = engine.shared.sent_write.lock().unwrap();
        assert_eq!(sends.len(), 1);
        sends[0].began = tokio::time::Instant::now() - crate::services::DEADLINE;
        sends[0].told.clone()
    };
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "forget");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the resumed words admit forget and block in its runner");
    let before = script.asked.lock().unwrap().len();
    let prior = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(
        prior.contains(CALL_ID) && prior.contains(ARG) && !prior.contains(RESULT),
        "the unanswered send call must really have reached the model"
    );
    {
        let asked = script.asked.lock().unwrap();
        assert!(
            asked[1]
                .iter()
                .any(|message| message.words() == Some(SEND_NOT_KNOWN_NOTE))
        );
        assert!(
            !asked[1].iter().any(|message| matches!(
                message.words(),
                Some(SENT_WRITE_NOTE | SENT_UNKNOWN_NOTE)
            ))
        );
    }
    // The actual forget runner was entered above. Only now may the old send finish.
    release.send(()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        late_result.wait_for(Option::is_some),
    )
    .await
    .expect("old send completes after forget admission")
    .expect("send result sender remains alive");
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                engine.next().await,
                ProviderOutput::TurnComplete { request: 3, .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("the second resumed turn completes");
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "forget must not allow a duplicate send or prepare"
    );
    let asked = script.asked.lock().unwrap();
    assert!(asked.len() > before);
    let resumed = serde_json::to_string(&asked[before..]).unwrap();
    for canary in [CALL_ID, ARG, RESULT] {
        assert!(
            !resumed.contains(canary),
            "{canary} restored after interrupted forget: {resumed}"
        );
    }
    // A genuine late receipt may update the fixed accounting status. It must
    // not claim that discarded call arguments and result payload are attached.
    assert!(
        !asked[before..]
            .iter()
            .flatten()
            .any(|message| message.words() == Some(SENT_WRITE_NOTE)),
        "the redacted receipt must not promise its discarded call and result"
    );
}

/// After forget admission, a fenced route's unavailable result must use the
/// services' receipt for accounting only, never restore its discarded payload.
/// Tokio's current-thread runtime keeps the cfg(test) receipt store thread-local.
#[tokio::test]
async fn a_services_fallback_after_admitted_forget_keeps_status_without_private_payload() {
    const CALL_ID: &str = "private-send-call-id-canary";
    const ARG: &str = "private-send-argument-canary";
    const RESULT: &str = "private-send-result-canary";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Go ahead and send it, then forget my face.".to_owned()),
                Ok(String::new()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        CALL_ID,
                        crate::services::WRITE,
                        json!({"service":"google_drive", "send_id":ARG}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "erase-face",
                        crate::known_faces::FORGET,
                        json!({"name": "A"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "repeat-send",
                        crate::services::WRITE,
                        json!({"service":"google_drive", "title":"Notes", "text":"New copy"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("I will report only what is confirmed."),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let writes = Arc::new(AtomicUsize::new(0));
    let seen = writes.clone();
    let (entered, admissions) = std::sync::mpsc::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let release_wait = Arc::new(Mutex::new(Some(released)));
    let runner: ScopedToolRunner = Arc::new(
        move |_, _, name, args, lease: crate::memory_scope::ScopeLease| {
            let marked = args.clone();
            let entered = entered.clone();
            if name == crate::services::WRITE {
                seen.fetch_add(1, Ordering::SeqCst);
                let wait = release_wait.lock().unwrap().take();
                Box::pin(async move {
                    entered.send("send").unwrap();
                    if let Some(wait) = wait {
                        wait.await.unwrap();
                    }
                    assert!(
                        !lease.is_current(),
                        "the original route must really be fenced"
                    );
                    let sent = json!({"sent":true,"service":"google_drive","detail":RESULT});
                    crate::services::keep_send_for_test(&marked, Some(&sent));
                    crate::tools::unavailable()
                })
            } else {
                assert_eq!(name, crate::known_faces::FORGET);
                Box::pin(async move {
                    assert!(lease.is_current(), "forget must enter on a current scope");
                    entered.send("forget").unwrap();
                    std::future::pending::<Value>().await
                })
            }
        },
    );
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| panic!("this fixture must use the scoped runner")),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .with_memory_scope(
        lamp_turn_owner::BootId::new([9; 16]).unwrap(),
        Arc::new(crate::memory_scope::FaceCache::holding_gaps(None)),
        runner,
        None,
    );
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "send");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the original send is in flight");
    // Model expiry without measuring a real 20-second deadline. Only the retained
    // receipt wait expires; the actual send remains blocked on its oneshot.
    let (attempt, mut late_result) = {
        let mut sends = engine.shared.sent_write.lock().unwrap();
        assert_eq!(sends.len(), 1);
        sends[0].began = tokio::time::Instant::now() - crate::services::DEADLINE;
        (sends[0].attempt, sends[0].told.clone())
    };
    assert!(
        crate::services::send_state(attempt).is_none(),
        "no fallback receipt exists before the controlled send finishes"
    );
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(name) = admissions.try_recv() {
                assert_eq!(name, "forget");
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the resumed words admit forget and block in its runner");
    let before = script.asked.lock().unwrap().len();
    let prior = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(
        prior.contains(CALL_ID) && prior.contains(ARG) && !prior.contains(RESULT),
        "the unanswered send call must really have reached the model"
    );
    {
        let asked = script.asked.lock().unwrap();
        assert!(
            asked[1]
                .iter()
                .any(|message| message.words() == Some(SEND_NOT_KNOWN_NOTE))
        );
        assert!(
            !asked[1].iter().any(|message| matches!(
                message.words(),
                Some(SENT_WRITE_NOTE | SENT_UNKNOWN_NOTE)
            ))
        );
    }
    // The actual forget runner was entered above. Only now may the old send finish.
    release.send(()).unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        late_result.wait_for(Option::is_some),
    )
    .await
    .expect("old send completes after forget admission")
    .expect("send result sender remains alive");
    assert_eq!(
        *late_result.borrow(),
        Some(crate::tools::unavailable()),
        "the watch must not contain the service success"
    );
    assert!(
        matches!(
            crate::services::send_state(attempt),
            Some(crate::services::SendState::Told(told)) if told["sent"] == true
        ),
        "the real services fallback must retain confirmed status, possibly already redacted"
    );
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                engine.next().await,
                ProviderOutput::TurnComplete { request: 3, .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("the second resumed turn completes");
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "forget must not allow a duplicate send or prepare"
    );
    let asked = script.asked.lock().unwrap();
    assert!(asked.len() > before);
    let resumed = serde_json::to_string(&asked[before..]).unwrap();
    // The watch contains unavailable, so a definite-sent repeat refusal can
    // only come from the Told receipt looked up by this numeric attempt.
    assert!(
        asked[before..].iter().flatten().any(|message| {
            message.tool_call_id.as_deref() == Some("repeat-send")
                && message.words().is_some_and(|words| {
                    serde_json::from_str::<Value>(words)
                        .ok()
                        .is_some_and(|answer| answer["error"] == ALREADY_SENT)
                })
        }),
        "the resumed Engine must consume the successful fallback for its repeat guard"
    );
    for canary in [CALL_ID, ARG, RESULT] {
        assert!(
            !resumed.contains(canary),
            "{canary} restored after interrupted forget: {resumed}"
        );
    }
    // A genuine late receipt may update the fixed accounting status. It must
    // not claim that discarded call arguments and result payload are attached.
    assert!(
        !asked[before..]
            .iter()
            .flatten()
            .any(|message| message.words() == Some(SENT_WRITE_NOTE)),
        "the redacted receipt must not promise its discarded call and result"
    );
}

// Append inside second_provider::tests::sent_write after the one-argument
// forget_carried_context API lands. No network, clock sleeps, or model needed.
fn ledger_boundary_engine() -> Engine {
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    // Isolate these receipts from the cfg(test) services store's other fixtures.
    engine
        .shared
        .send_attempts
        .store(0x5878_0000, Ordering::Relaxed);
    engine
}

fn ledger_boundary_call() -> ToolCall {
    call(
        "ledger-private-call-canary",
        crate::services::WRITE,
        json!({"service":"google_drive", "send_id":"ledger-private-argument-canary"}),
    )
}

/// Even an evicted ledger entry's captured watch and completion handle retain
/// the old epoch; finishing its actual runner cannot publish private content.
#[tokio::test]
async fn forget_invalidates_a_snapshotted_send_handle_after_ledger_eviction() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let call = ledger_boundary_call();
    let (attempt, _) = send_attempt(shared, 100, &call);
    assert!(attempt.is_some());
    let (release, released) = tokio::sync::oneshot::channel::<Value>();
    let running = sending(
        shared,
        (100, attempt),
        &call,
        Box::pin(async move { released.await.unwrap() }),
        Value::clone,
    );
    let (content, told) = {
        let sends = shared.sent_write.lock().unwrap();
        (sends[0].content.clone(), sends[0].told.clone())
    };
    assert!(content.lock().unwrap().call.is_some());
    drop_sent_write(shared, Some(100));
    assert!(shared.sent_write.lock().unwrap().is_empty());
    forget_carried_context(shared);
    release
        .send(json!({"sent":true,"detail":"ledger-private-result-canary"}))
        .unwrap();
    running.await;
    assert!(content.lock().unwrap().call.is_none());
    assert_eq!(*told.borrow(), Some(json!({"sent":true})));
}

/// An attempt is captured before constructing the runner. A boundary in that
/// interval must redact registration as well as the later completion.
#[tokio::test]
async fn forget_redacts_late_registration_of_a_captured_send_attempt() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let call = ledger_boundary_call();
    let (attempt, _) = send_attempt(shared, 101, &call);
    assert!(attempt.is_some());
    forget_carried_context(shared);
    let running = sending(
        shared,
        (101, attempt),
        &call,
        Box::pin(async { json!({"sent":true,"detail":"ledger-private-result-canary"}) }),
        Value::clone,
    );
    {
        let sends = shared.sent_write.lock().unwrap();
        assert_eq!(sends.len(), 1);
        assert!(sends[0].content.lock().unwrap().call.is_none());
    }
    running.await;
    let sends = shared.sent_write.lock().unwrap();
    assert_eq!(*sends[0].told.borrow(), Some(json!({"sent":true})));
}

/// A newer line may already hold old rounds when the prior forget is admitted.
/// Both the installed copy and a subsequently restored stale snapshot lose
/// tool rounds; neither transition discards the person's words or identity.
#[tokio::test]
async fn forget_clears_installed_and_later_restored_carried_rounds_but_keeps_words() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let mut newer = Carried::own(
        103,
        "Please finish my question.".into(),
        Instant::now(),
        102,
    );
    let recall = call(
        "ledger-private-recall-call",
        crate::remember::RECALL,
        json!({"query":"private"}),
    );
    newer.rounds = vec![
        Message::assistant_calls(None, &[recall]),
        Message::tool("ledger-private-recall-call", "ledger-private-recall-result"),
    ];
    put_in_flight(shared, newer);
    let stale = shared.in_flight.lock().unwrap().as_ref().unwrap().clone();
    assert!(!carried_rounds(shared, 103).is_empty());
    forget_carried_context(shared);
    {
        let installed = shared.in_flight.lock().unwrap();
        let current = installed.as_ref().unwrap();
        assert_eq!(current.request, 103);
        assert_eq!(current.said_by, 102);
        assert_eq!(current.text(), "Please finish my question.");
        assert!(current.rounds.is_empty());
    }
    assert!(
        !stale.rounds.is_empty(),
        "the restored snapshot really predates admission"
    );
    put_in_flight(shared, stale);
    let installed = shared.in_flight.lock().unwrap();
    let restored = installed.as_ref().unwrap();
    assert_eq!(restored.request, 103);
    assert_eq!(restored.said_by, 102);
    assert_eq!(restored.text(), "Please finish my question.");
    assert!(restored.rounds.is_empty());
}

/// Real receipt/runner states drive recovery after admission. Confirmed sends
/// still prevent duplicates, uncertainty never claims success, and a refused
/// send stays refused. None of the four paths publishes discarded tool data.
#[tokio::test]
async fn forgotten_send_outcomes_keep_only_fixed_notes_and_the_right_repeat_guard() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    for (index, state, expected_note, expected_guard) in [
        (0, "sent", SENT_REDACTED_NOTE, Some(ALREADY_SENT)),
        (1, "asked", SEND_NOT_KNOWN_NOTE, Some(SEND_NOT_KNOWN)),
        (2, "unknown", SEND_NOT_KNOWN_NOTE, Some(SEND_NOT_KNOWN)),
        (3, "refused", SEND_REDACTED_REFUSED_NOTE, None),
    ] {
        let origin = 110 + index * 2;
        let resumed = origin + 1;
        let call = ledger_boundary_call();
        let (attempt, args) = send_attempt(shared, origin, &call);
        assert!(attempt.is_some());
        let (release, released) = tokio::sync::oneshot::channel::<Value>();
        let running = sending(
            shared,
            (origin, attempt),
            &call,
            Box::pin(async move { released.await.unwrap() }),
            Value::clone,
        );
        let completed = match state {
            "sent" => Some(json!({"sent":true,"detail":"ledger-private-result-canary"})),
            "refused" => Some(json!({"error":"ledger-private-refusal-canary"})),
            _ => None,
        };
        // The pending cases expire only the accounting wait, not the runner.
        // This exercises recovery without waiting for a real service deadline.
        if completed.is_none() {
            let mut sends = shared.sent_write.lock().unwrap();
            sends.last_mut().unwrap().began =
                tokio::time::Instant::now() - crate::services::DEADLINE;
        }
        if state == "asked" {
            crate::services::keep_send_for_test(&args, None);
        }
        let mut completion = Some((release, running));
        if let Some(result) = completed {
            let (release, running) = completion.take().unwrap();
            release.send(result).unwrap();
            running.await;
        }
        put_in_flight(
            shared,
            Carried::own(
                resumed,
                "Go ahead and send it.".into(),
                Instant::now(),
                origin,
            ),
        );
        forget_carried_context(shared);
        match sent_with_words(shared, &[origin], resumed).await {
            SentWith::Told(note) => assert_eq!(note, Some(expected_note), "{state}"),
            _ => panic!("{state}: recovery did not produce its accounting note"),
        }
        assert_eq!(
            sent_already(shared, resumed, crate::services::WRITE),
            expected_guard.map(|error| json!({"error":error})),
            "{state}"
        );
        assert!(
            carried_rounds(shared, resumed).is_empty(),
            "{state}: private tool round restored"
        );
        assert_eq!(
            shared.in_flight.lock().unwrap().as_ref().unwrap().text(),
            "Go ahead and send it."
        );
        if let Some((release, running)) = completion {
            release.send(crate::tools::unavailable()).unwrap();
            running.await;
        }
        drop_sent_write(shared, Some(origin));
    }
}

/// A batch already running at admission cannot publish into the fresh context.
#[tokio::test]
async fn forget_rejects_an_older_batches_late_carried_round() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    put_in_flight(
        shared,
        Carried::own(120, "My question".into(), Instant::now(), 120),
    );
    let old_epoch = shared.send_content_epoch.load(Ordering::Acquire);
    let old_round = vec![Message::tool("old-call-canary", "old-result-canary")];
    forget_carried_context(shared);
    carry_round(shared, 120, old_epoch, &old_round);
    assert!(carried_rounds(shared, 120).is_empty());
    let fresh_epoch = shared.send_content_epoch.load(Ordering::Acquire);
    let fresh_round = vec![Message::tool("fresh-call", "fresh-result")];
    carry_round(shared, 120, fresh_epoch, &fresh_round);
    assert_eq!(carried_rounds(shared, 120).len(), 1);
}
/// Host-only admission timing, not device or end-to-end latency. The target is
/// <1 ms for the synchronous boundary, reported rather than asserted. Engine,
/// payloads, channels and ledger installation are prepared outside each timer.
/// No active readers/writers contend for the locks in this diagnostic.
#[tokio::test]
#[ignore = "host timing diagnostic; run explicitly with --ignored --nocapture"]
async fn forget_ledger_host_admission_latency_diagnostic() {
    const EMPTY_SAMPLES: usize = 256;
    const FULL_SAMPLES: usize = 64;
    const ARG_BYTES: usize = 1_024;
    const RESULT_BYTES: usize = 4_096;

    fn report(label: &str, mut samples: Vec<Duration>) {
        samples.sort_unstable();
        let count = samples.len();
        let median = samples[(count - 1) / 2];
        let p95 = samples[(count * 95).div_ceil(100) - 1];
        let max = samples[count - 1];
        eprintln!(
            "forget admission host {label}: n={count} median={:.3} us p95={:.3} us max={:.3} us; target <1000 us (diagnostic only)",
            median.as_secs_f64() * 1_000_000.0,
            p95.as_secs_f64() * 1_000_000.0,
            max.as_secs_f64() * 1_000_000.0,
        );
    }

    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let shared = &engine.shared;
    assert!(shared.sent_write.lock().unwrap().is_empty());
    assert!(shared.in_flight.lock().unwrap().is_none());
    // This is the first boundary on this already-constructed engine. It does
    // not include process startup, runtime initialization or cold device I/O.
    let first_started = Instant::now();
    forget_carried_context(shared);
    let first = first_started.elapsed();
    report("first call after engine setup, empty", vec![first]);

    let mut empty = Vec::with_capacity(EMPTY_SAMPLES);
    for _ in 0..EMPTY_SAMPLES {
        let started = Instant::now();
        forget_carried_context(shared);
        empty.push(started.elapsed());
    }
    report("repeated empty", empty);

    let entries = SENDS_KEPT * net::MOST_CALLS;
    let argument = "a".repeat(ARG_BYTES);
    let result = "r".repeat(RESULT_BYTES);
    let mut full = Vec::with_capacity(FULL_SAMPLES);
    for _ in 0..FULL_SAMPLES {
        let epoch = shared.send_content_epoch.load(Ordering::Acquire);
        let mut ledger = Vec::with_capacity(entries);
        for index in 0..entries {
            let call = call(
                &format!("timing-call-{index}"),
                crate::services::WRITE,
                json!({"service":"google_drive","send_id":format!("timing-send-{index}"),"text":argument.clone()}),
            );
            let receipt = json!({"sent":true,"service":"google_drive","detail":result.clone()});
            let (answered, told) = tokio::sync::watch::channel(Some(receipt));
            ledger.push(SentWrite {
                request: 1 + (index / net::MOST_CALLS) as u64,
                attempt: 1 + index as u64,
                content: Arc::new(Mutex::new(SendContent {
                    call: Some(call),
                    epoch,
                    current: shared.send_content_epoch.clone(),
                    answered,
                })),
                began: tokio::time::Instant::now(),
                told,
            });
        }
        // Replacing and dropping the preceding sample's ledger is untimed.
        *shared.sent_write.lock().unwrap() = ledger;
        let started = Instant::now();
        forget_carried_context(shared);
        full.push(started.elapsed());
        // Outside the timer, exercise recovery of every origin: the old
        // payloads must not be published even though admission did not sweep
        // them. Each confirmed send must still suppress a duplicate dispatch.
        for origin in 1..=SENDS_KEPT as u64 {
            let resumed = 1_000 + origin;
            put_in_flight(
                shared,
                Carried::own(resumed, "My question".into(), Instant::now(), origin),
            );
            match sent_with_words(shared, &[origin], resumed).await {
                SentWith::Told(note) => assert_eq!(note, Some(SENT_REDACTED_NOTE)),
                _ => panic!("the saturated ledger lost its send accounting"),
            }
            assert!(carried_rounds(shared, resumed).is_empty());
            assert_eq!(
                sent_already(shared, resumed, crate::services::WRITE),
                Some(json!({"error":ALREADY_SENT}))
            );
        }
        *shared.in_flight.lock().unwrap() = None;
        // Actual consumption scrubbed every entry without exposing its data.
        let ledger = shared.sent_write.lock().unwrap();
        assert_eq!(ledger.len(), entries);
        for sent in ledger.iter() {
            assert!(sent.content.lock().unwrap().call.is_none());
            assert_eq!(*sent.told.borrow(), Some(json!({"sent":true})));
        }
    }
    eprintln!(
        "forget admission host inputs: {SENDS_KEPT} requests x {} calls = {entries} receipt entries; each has {ARG_BYTES} ASCII argument bytes and {RESULT_BYTES} ASCII result bytes plus JSON/IDs; {} variable-content bytes total; no carried rounds, no lock contention; representative bounded payload, not maximum wire-response size",
        net::MOST_CALLS,
        entries * (ARG_BYTES + RESULT_BYTES),
    );
    report("saturated ledger", full);
}

/// A stale model batch remains old even when its send starts after admission.
#[tokio::test]
async fn forget_redacts_an_old_contexts_send_started_after_admission() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let epoch = shared.send_content_epoch.load(Ordering::Acquire);
    forget_carried_context(shared);
    let call = ledger_boundary_call();
    let (attempt, _) = send_attempt_in_context(shared, 130, &call, epoch);
    sending(
        shared,
        (130, attempt),
        &call,
        Box::pin(async { json!({"sent": true, "detail": "old-context-result-canary"}) }),
        Value::clone,
    )
    .await;
    let ledger = shared.sent_write.lock().unwrap();
    assert!(ledger[0].content.lock().unwrap().call.is_none());
    assert_eq!(*ledger[0].told.borrow(), Some(json!({"sent": true})));
}

/// Without a recovery already holding the publication lock, admission must
/// not directly wait for a producer holding its private receipt-content lock.
#[tokio::test]
async fn forget_admission_does_not_directly_wait_for_a_busy_receipt() {
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let call = ledger_boundary_call();
    let (attempt, _) = send_attempt(shared, 150, &call);
    sending(
        shared,
        (150, attempt),
        &call,
        Box::pin(async { json!({"sent":true,"detail":"busy-receipt-canary"}) }),
        Value::clone,
    )
    .await;
    let content = shared.sent_write.lock().unwrap()[0].content.clone();
    let busy = content.lock().unwrap();
    let admitted_before_release = std::thread::scope(|scope| {
        let (done, received) = std::sync::mpsc::channel();
        let admitting = scope.spawn(move || {
            forget_carried_context(shared);
            done.send(()).unwrap();
        });
        let admitted = received.recv_timeout(Duration::from_secs(1)).is_ok();
        // Release even on failure, so the negative proof cannot hang its join.
        drop(busy);
        admitting.join().unwrap();
        admitted
    });
    assert!(
        admitted_before_release,
        "admission waited for the receipt-content lock"
    );
    put_in_flight(
        shared,
        Carried::own(151, "My question".into(), Instant::now(), 150),
    );
    match sent_with_words(shared, &[150], 151).await {
        SentWith::Told(note) => assert_eq!(note, Some(SENT_REDACTED_NOTE)),
        _ => panic!("the earlier send lost its accounting"),
    }
    assert!(carried_rounds(shared, 151).is_empty());
    assert_eq!(
        sent_already(shared, 151, crate::services::WRITE),
        Some(json!({"error":ALREADY_SENT}))
    );
}

/// Recovery has already copied a completed raw receipt when another try makes
/// it wait. Eviction and forget during that wait must still invalidate the
/// copied completed receipt, including with no ledger entry left to sweep.
#[tokio::test]
async fn forget_fences_a_completed_evicted_receipt_already_copied_by_recovery() {
    use std::future::Future;
    let engine = ledger_boundary_engine();
    let shared = &engine.shared;
    let first = ledger_boundary_call();
    let (attempt, _) = send_attempt(shared, 160, &first);
    sending(
        shared,
        (160, attempt),
        &first,
        Box::pin(async { json!({"sent":true,"detail":"completed-evicted-canary"}) }),
        Value::clone,
    )
    .await;
    let completed = shared.sent_write.lock().unwrap()[0].told.clone();
    assert_eq!(
        completed.borrow().as_ref().unwrap()["detail"],
        "completed-evicted-canary"
    );
    let mut second = ledger_boundary_call();
    second.id = "later-pending-try".into();
    let (attempt, _) = send_attempt(shared, 160, &second);
    let (release, released) = tokio::sync::oneshot::channel::<Value>();
    let pending = sending(
        shared,
        (160, attempt),
        &second,
        Box::pin(async move { released.await.unwrap() }),
        Value::clone,
    );
    put_in_flight(
        shared,
        Carried::own(161, "My question".into(), Instant::now(), 160),
    );
    let origins = [160];
    let recovery = sent_with_words(shared, &origins, 161);
    tokio::pin!(recovery);
    // Poll through the completed first receipt to the second receipt's wait.
    std::future::poll_fn(|cx| {
        assert!(recovery.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop_sent_write(shared, Some(160));
    assert!(shared.sent_write.lock().unwrap().is_empty());
    forget_carried_context(shared);
    release
        .send(json!({"sent":false,"detail":"later-refusal-canary"}))
        .unwrap();
    pending.await;
    match recovery.await {
        SentWith::Told(note) => assert_eq!(note, Some(SENT_REDACTED_NOTE)),
        _ => panic!("the completed send lost its accounting after eviction"),
    }
    assert!(carried_rounds(shared, 161).is_empty());
    assert_eq!(*completed.borrow(), Some(json!({"sent":true})));
    assert_eq!(
        sent_already(shared, 161, crate::services::WRITE),
        Some(json!({"error":ALREADY_SENT}))
    );
}
