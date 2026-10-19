//! The real provider worker against the in-memory Gemini service. These
//! observations grant no input or speaker output on their own.
use super::*;

fn options(enabled: bool) -> Options {
    Options {
        holding_answer: enabled,
        name_signal: true,
        ..Options::reporting(Reporting::Typed)
    }
}

fn transcript(text: &str) -> serde_json::Value {
    json!({"serverContent":{"outputTranscription":{"text":text}}})
}

async fn ended(rig: &mut LiveRig, request: u64) -> Vec<ProviderOutput> {
    let mut seen = Vec::new();
    loop {
        let event = rig.output().await;
        let terminal = matches!(event, ProviderOutput::TurnComplete { request: of, idle: true } if of == request);
        seen.push(event);
        if terminal {
            return seen;
        }
    }
}

#[test]
fn the_holding_flag_is_opt_in_and_parsed_after_the_cut_in_flag() {
    let dir = Path::new("/run/test");
    assert!(!parse_flags(dir, &[]).unwrap().2.holding_answer);
    assert!(!Options::reporting(Reporting::Typed).holding_answer);
    let flag = crate::holding_answer::FLAG.to_owned();
    assert!(
        parse_flags(dir, std::slice::from_ref(&flag))
            .unwrap()
            .2
            .holding_answer
    );
    let combined = vec![crate::cut_in_told::FLAG.to_owned(), flag.clone()];
    let files = parse_flags(dir, &combined).unwrap().2;
    assert!(files.cut_in_told && files.holding_answer);
    let combined = vec![
        crate::cut_in_told::FLAG.to_owned(),
        crate::misheard_hum::FLAG.to_owned(),
        flag.clone(),
    ];
    let files = parse_flags(dir, &combined).unwrap().2;
    assert!(files.cut_in_told && files.misheard_hum && files.holding_answer);
    assert!(parse_flags(dir, &[flag.clone(), flag]).is_err());
    assert!(parse_flags(dir, &["--holding-answer=on".into()]).is_err());
}

#[tokio::test]
async fn a_complete_chunked_holding_answer_is_reported_before_its_idle_terminal() {
    let mut rig = LiveRig::start_with(quick_policy(), options(true));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    for text in ["Let ", "me ", "think."] {
        dial.service.send(transcript(text)).await;
    }
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, request).await;
    assert_eq!(
        seen.iter()
            .filter(|event| matches!(event, ProviderOutput::HoldingAnswer { .. }))
            .count(),
        1,
        "{seen:?}"
    );
    assert!(
        matches!(seen[seen.len() - 2], ProviderOutput::HoldingAnswer { request: of, session } if of == request && session == dial.session.get()),
        "{seen:?}"
    );
    assert!(
        matches!(seen.last(), Some(ProviderOutput::TurnComplete { request: of, idle: true }) if *of == request)
    );
    // Classifying it only sends an observation to the coordinator. Nothing
    // asks the model to continue from the worker.
    rig.pump(Duration::from_millis(20)).await;
    assert!(dial.service.try_receive(Duration::ZERO).await.is_none());
    assert!(rig.take().is_none());
    rig.stop().await;
}

#[tokio::test]
async fn off_substantive_answers_and_transcript_overflow_emit_no_holding_observation() {
    for (enabled, chunks) in [
        (false, vec!["Let me think.".to_owned()]),
        (
            true,
            vec![
                "Let me think. ".to_owned(),
                "The answer is four.".to_owned(),
            ],
        ),
        (true, vec!["Give me a second opinion.".to_owned()]),
        (
            true,
            vec!["Let me see".to_owned(), " ".repeat(256), ".".to_owned()],
        ),
    ] {
        let mut rig = LiveRig::start_with(quick_policy(), options(enabled));
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        for text in chunks {
            dial.service.send(transcript(&text)).await;
        }
        dial.service.send(idle_complete()).await;
        let seen = ended(&mut rig, request).await;
        assert!(
            !seen
                .iter()
                .any(|event| matches!(event, ProviderOutput::HoldingAnswer { .. })),
            "{seen:?}"
        );
        rig.stop().await;
    }
}

#[tokio::test]
async fn even_a_silent_name_function_disqualifies_a_holding_answer() {
    let mut rig = LiveRig::start_with(quick_policy(), options(true));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(transcript("One moment.")).await;
    dial.service
        .send(function_call("name-1", crate::attention::HEARD_MY_NAME))
        .await;
    let response = rig.service_message(&mut dial).await;
    assert_eq!(
        response["toolResponse"]["functionResponses"][0]["scheduling"],
        "SILENT"
    );
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, request).await;
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, ProviderOutput::HoldingAnswer { .. })),
        "{seen:?}"
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_holding_answer_withheld_by_attention_does_not_request_a_continuation() {
    let mut config = options(true);
    config.attention = Some(|| (crate::room_told::Room::Busy, false));
    config.spoken_to = true;
    let mut rig = LiveRig::start_with(quick_policy(), config);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(json!({"serverContent":{"inputTranscription":{"text":"Did you get the agenda?"}}}))
        .await;
    dial.service.send(transcript("Let me think.")).await;
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, request).await;
    assert!(
        !seen.iter().any(|event| matches!(
            event,
            ProviderOutput::HoldingAnswer { .. }
                | ProviderOutput::Transcript {
                    request: Some(_),
                    ..
                }
        )),
        "{seen:?}"
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_substantive_continuation_after_generation_end_prevents_a_holding_observation() {
    let mut rig = LiveRig::start_with(quick_policy(), options(true));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(transcript("Let me see. ")).await;
    dial.service.send(json!({"serverContent":{"generationComplete":true,"turnComplete":true,"interactionStatus":"IN_PROGRESS"}})).await;
    dial.service.send(transcript("It is yellow.")).await;
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, request).await;
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, ProviderOutput::HoldingAnswer { .. })),
        "{seen:?}"
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_reminders_holding_words_grant_nothing_and_the_next_heard_request_still_qualifies() {
    use std::os::unix::net::UnixListener;

    let home = crate::process::SessionDirectory::create().unwrap();
    let executive = UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET)).unwrap();
    executive.set_nonblocking(true).unwrap();
    let mut config = options(true);
    config.tools.schedule_speak = true;
    config.tools.exec = Some(home.path.clone());
    let mut rig = LiveRig::start_with(quick_policy(), config);
    let mut dial = rig.ready().await;
    rig.owner
        .set_camera_permission(real_time(), Permission::Denied)
        .unwrap();
    let reminder = rig
        .owner
        .admit(real_time(), AdmittedInput::NewTurn)
        .unwrap();
    rig.owner.input_ended(real_time(), reminder).unwrap();
    rig.publish();
    let request = reminder.turn();
    rig.parent
        .data
        .send(ProviderInput::Reminder {
            request,
            session: dial.session.get(),
            privacy_generation: rig.snapshot.microphone_generation(),
            text: "Let me think.".into(),
            late: false,
            late_by_ms: Some(0),
        })
        .unwrap();
    assert!(
        matches!(rig.output().await, ProviderOutput::Started { request: of, .. } if of == request)
    );
    let written = rig.service_message(&mut dial).await;
    let parts = written["clientContent"]["turns"][0]["parts"]
        .as_array()
        .unwrap();
    assert_eq!(parts.len(), 1, "{written}");
    let note = parts[0]["text"].as_str().unwrap();
    let data: serde_json::Value =
        serde_json::from_str(note.split_once("Reminder data: ").unwrap().1).unwrap();
    assert_eq!(data, json!({"text":"Let me think.","late":false}));
    // No function was called: without the reminder-origin fence these words
    // alone would incorrectly qualify for a fresh conversational request.
    dial.service.send(transcript("Let me think.")).await;
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, request).await;
    assert!(
        seen.iter().all(|event| matches!(
            event,
            ProviderOutput::Transcript { request: Some(of), .. }
                | ProviderOutput::TurnComplete { request: of, .. } if *of == request
        )),
        "{seen:?}"
    );
    rig.pump(Duration::from_millis(20)).await;
    assert!(rig.take().is_none());
    assert!(dial.service.try_receive(Duration::ZERO).await.is_none());
    assert_eq!(
        executive.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(rig.snapshot.camera_permission(), Permission::Denied);
    assert_eq!(rig.snapshot.owner(), Some(reminder));

    rig.owner.complete_turn(real_time(), reminder).unwrap();
    rig.publish();
    let next = rig.speak(&mut dial, false, true).await;
    dial.service.send(transcript("One moment.")).await;
    dial.service.send(idle_complete()).await;
    let seen = ended(&mut rig, next).await;
    assert_eq!(
        seen.iter()
            .filter(|event| matches!(
                event,
                ProviderOutput::HoldingAnswer { request: of, session }
                    if *of == next && *session == dial.session.get()
            ))
            .count(),
        1,
        "{seen:?}"
    );
    assert_eq!(rig.snapshot.camera_permission(), Permission::Denied);
    rig.stop().await;
}
