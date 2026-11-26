//! Live's state note (#7767, the Reviewer-lead's c7203343369, item 3),
//! through the worker's own loop against the scripted service: a heard line
//! naming the looking is told the desk noticing's state beside its sound,
//! before its end, as the second route tells it (`seen_yes::state_note`).
use super::*;

/// A consent folder whose earlier nos are kept, so a missing file is on by
/// default, and seen activity's file in it.
fn consent(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let folder =
        std::env::temp_dir().join(format!("live-desk-noticing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(lamp_msgs::consent::OLD_NOS_DONE), b"{}").unwrap();
    let path = crate::onboarding::seen_activity_beside(&folder.join("room-sounds.json"));
    (folder, path)
}

/// The first request of a rig with desk noticing's file (`path`; `None`:
/// the switch off), heard as `before` in pieces before its end is asked
/// for and as `after` once its end was sent: the service messages up to
/// the request's end, and any in the 300 ms after `after`.
async fn heard_around_the_end(
    path: Option<std::path::PathBuf>,
    before: &[&str],
    after: &[&str],
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut options = typed_with(crate::tools::Settings::default());
    options.desk_noticing = path;
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, false).await;
    let heard = |piece: &str| json!({"serverContent":{"inputTranscription":{"text":piece}}});
    for piece in before {
        dial.service.send(heard(piece)).await;
    }
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    // The pieces come on the service's socket and the end on the parent's
    // channel, read in no set order: the pieces are given time first.
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        rig.publish();
    }
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let mut sent = Vec::new();
    while sent
        .last()
        .is_none_or(|last: &serde_json::Value| last["realtimeInput"].get("activityEnd").is_none())
    {
        sent.push(rig.service_message(&mut dial).await);
    }
    for piece in after {
        dial.service.send(heard(piece)).await;
    }
    let mut later = Vec::new();
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        rig.publish();
        if let Some(message) = dial.service.try_receive(Duration::from_millis(5)).await {
            later.push(message);
        }
    }
    rig.stop().await;
    (sent, later)
}

/// A heard line naming the looking gets the state read then, on by default
/// with no file and off once a no is kept, before the request's end.
#[tokio::test]
async fn a_heard_line_naming_the_looking_is_told_the_state_before_its_end() {
    let (folder, path) = consent("told");
    let (sent, _) = heard_around_the_end(
        Some(path.clone()),
        &["Lamp, are you", " watching my desk?"],
        &[],
    )
    .await;
    assert_eq!(sent.len(), 2, "{sent:?}");
    let on = sent[0]["realtimeInput"]["text"].as_str().unwrap();
    assert_eq!(on, crate::seen_yes::state_note_at(&path, true));
    assert!(on.contains("on by default"), "{on}");
    for (key, file) in [
        (lamp_msgs::consent::SEEN_ACTIVITY, path.clone()),
        (
            lamp_msgs::consent::FACE_EXPRESSIONS,
            crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json")),
        ),
    ] {
        std::fs::write(
            file,
            lamp_msgs::consent::no_bytes(key, 5, lamp_msgs::consent::BY_VOICE),
        )
        .unwrap();
    }
    let (sent, _) =
        heard_around_the_end(Some(path.clone()), &["Stop looking at my desk."], &[]).await;
    assert_eq!(sent.len(), 2, "{sent:?}");
    let off = sent[0]["realtimeInput"]["text"].as_str().unwrap();
    assert!(off.contains("is off"), "{off}");
    assert!(off.contains(crate::seen_yes::LOOK), "{off}");
    let _ = std::fs::remove_dir_all(folder);
}

/// A line naming none of it is told nothing; nor is one naming it with the
/// switch off.
#[tokio::test]
async fn no_state_without_the_looking_named_or_without_the_switch() {
    let (folder, path) = consent("untold");
    let (sent, _) =
        heard_around_the_end(Some(path), &["What's the weather", " in Hanoi?"], &[]).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let (sent, _) = heard_around_the_end(None, &["Are you watching my desk?"], &[]).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let _ = std::fs::remove_dir_all(folder);
}

/// Words naming the looking only after the request's end was sent are
/// late: nothing more is sent for it.
#[tokio::test]
async fn words_naming_the_looking_after_the_end_are_late() {
    let (folder, path) = consent("late");
    let (sent, later) =
        heard_around_the_end(Some(path), &["Hey Lamp,"], &[" are you watching my desk?"]).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(later.is_empty(), "{later:?}");
    let _ = std::fs::remove_dir_all(folder);
}

/// A written turn (a reminder's is one) hears no person: words naming the
/// looking that come with it are told nothing.
#[tokio::test]
async fn a_written_turn_is_told_nothing() {
    let (folder, path) = consent("written");
    let mut options = typed_with(crate::tools::Settings::default());
    options.desk_noticing = Some(path);
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let owner = rig
        .owner
        .admit(real_time(), lamp_turn_owner::AdmittedInput::NewTurn)
        .unwrap();
    let news = owner.turn();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::Written {
            request: news,
            session: dial.session.get(),
            privacy_generation: rig.snapshot.microphone_generation(),
            text: "Remind them to check whether the camera on the desk is watching.".to_owned(),
            look: false,
            search: false,
            quoted: false,
        })
        .unwrap();
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Started { request, .. } if request == news
    ));
    let written = rig.service_message(&mut dial).await;
    assert!(written.get("clientContent").is_some(), "{written}");
    dial.service
        .send(json!({"serverContent":{"inputTranscription":{"text":"Are you watching my desk?"}}}))
        .await;
    let until = Instant::now() + Duration::from_millis(300);
    let mut later = Vec::new();
    while Instant::now() < until {
        rig.publish();
        if let Some(message) = dial.service.try_receive(Duration::from_millis(5)).await {
            later.push(message);
        }
    }
    assert!(later.is_empty(), "{later:?}");
    rig.stop().await;
    let _ = std::fs::remove_dir_all(folder);
}
