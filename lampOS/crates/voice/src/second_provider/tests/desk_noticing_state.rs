//! The desk noticing's state on the second route (#7767): on lamp-06c1's
//! re-row of #8674 (26 Nov), with `LAMP_NOTICING_BY_DEFAULT=on` and no kept
//! record, the model answered "are you watching my desk?" with "not right
//! now" while bursts ran, said "nothing was on, so there's nothing to stop"
//! with no call, and "I've stopped watching you" with `calls=none`: it was
//! never told the state, so it answered from earlier turns. A line naming
//! the looking is now told the state read just then (kept record, else the
//! default, as lamp-seen reads it: `seen_yes::state_note_at`), so a change
//! to the files between turns reaches the next one. A line that doesn't
//! name it, or the route without the switch, is told nothing.
use super::*;
use std::path::{Path, PathBuf};

/// The note's own words.
const NOTE: &str = "(For you to know, read just now, not to answer: desk noticing is";

/// A consent folder with the done mark (`lamp_msgs::consent::OLD_NOS_DONE`),
/// so a missing file is the default's yes: its seen activity file.
fn consent(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("second-desk-state-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(lamp_msgs::consent::OLD_NOS_DONE), b"{}").unwrap();
    crate::onboarding::seen_activity_beside(&folder.join("room-sounds.json"))
}

/// Each line of `heard` in its own request (1, 2, ...), `between` run
/// before each request with its number, the state told from `path` (none:
/// the switch off): each chat's desk noticing notes.
fn told(path: Option<&Path>, heard: &[&str], mut between: impl FnMut(u64)) -> Vec<Vec<String>> {
    let script = Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(
            heard
                .iter()
                .map(|_| vec![text("Okay."), Ok(ChatEvent::Done)])
                .collect(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) })),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .telling_desk_noticing(path.map(Path::to_path_buf));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for request in 1..=heard.len() as u64 {
                between(request);
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
                turn_within(&mut engine, Duration::from_secs(5)).await;
            }
        });
    let asked = script.asked.lock().unwrap();
    asked
        .iter()
        .map(|chat| {
            chat.iter()
                .filter(|message| message.role == "system")
                .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
                .filter(|note| note.contains(NOTE))
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// The row's lines with no kept record: each is told noticing is on by
/// default and to call the stop; then, after a stop's kept no written
/// between turns (as the voice's stop or the kit's files), the next is told
/// it is off, and once the no is removed again (the kit's reset), on: the
/// files are read at each line, nothing is cached.
#[test]
fn a_line_naming_the_looking_is_told_the_state_read_then() {
    use lamp_msgs::consent::{BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, file_in, no_bytes};
    let path = consent("row");
    let folder = path.parent().unwrap().to_path_buf();
    let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
    let notes = told(
        Some(&path),
        &[
            "Lamp, are you watching my desk?",
            "Lamp, stop watching me.",
            "Lamp, stop looking.",
            "Lamp, are you watching my desk?",
            "Lamp, what can you notice about my desk?",
        ],
        |request| match request {
            3 => {
                std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
                std::fs::write(&face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
            }
            5 => {
                std::fs::remove_file(&path).unwrap();
                std::fs::remove_file(&face).unwrap();
            }
            _ => {}
        },
    );
    assert_eq!(notes.len(), 5, "{notes:?}");
    for (at, notes) in notes.iter().enumerate() {
        assert_eq!(notes.len(), 1, "line {}: {notes:?}", at + 1);
    }
    for at in [0, 1, 4] {
        let note = &notes[at][0];
        assert!(note.contains("on by default"), "line {}: {note}", at + 1);
        assert!(note.contains(crate::seen_yes::STOP), "{note}");
        assert!(note.contains("whatever earlier turns said"), "{note}");
    }
    for at in [2, 3] {
        let note = &notes[at][0];
        assert!(note.contains("is off"), "line {}: {note}", at + 1);
        assert!(note.contains(crate::seen_yes::LOOK), "{note}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

/// A line that doesn't name the looking, and the route without the switch,
/// are told nothing of it.
#[test]
fn other_lines_and_the_switch_off_are_told_nothing() {
    let path = consent("other");
    let notes = told(Some(&path), &["What's the weather?"], |_| {});
    assert_eq!(notes, [Vec::<String>::new()]);
    let notes = told(None, &["Lamp, are you watching my desk?"], |_| {});
    assert_eq!(notes, [Vec::<String>::new()]);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// `heard` in request 1, answered `reply` with no call, the state told
/// from `path`: the chat's desk noticing notes.
fn replied(path: &Path, heard: &str, reply: &str) -> Vec<String> {
    let script = Arc::new(Script {
        heard: Mutex::new(vec![Ok(heard.to_owned())].into()),
        chats: Mutex::new(vec![vec![text(reply), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) })),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .telling_desk_noticing(Some(path.to_path_buf()));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn_within(&mut engine, Duration::from_secs(5)).await;
        });
    let asked = script.asked.lock().unwrap();
    asked
        .iter()
        .flatten()
        .filter(|message| message.role == "system")
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|note| note.contains(NOTE))
        .map(str::to_owned)
        .collect()
}

/// lamp-06c1's third re-row of #8674 (26 Nov, Q3), whole: desk noticing on
/// by default, the speech-to-text heard "Now stop watching the", and the
/// model said "I've stopped watching your desk…" with `calls=none`, so the
/// bursts went on. The line is told the state, and the reply's claim runs
/// the stop as the call would before it is said: the kept state is no for
/// both, by voice. A reply that says it is on changes nothing.
#[test]
fn a_reply_claiming_a_stop_with_no_call_keeps_the_no() {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in};
    let path = consent("claimed");
    let folder = path.parent().unwrap().to_path_buf();
    let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
    let notes = replied(
        &path,
        "Now stop watching the",
        "I've stopped watching your desk and noticing what you're doing.",
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("on by default"), "{notes:?}");
    assert!(!allowed(&path, SEEN_ACTIVITY, true));
    assert!(!allowed(&face, FACE_EXPRESSIONS, true));
    let _ = std::fs::remove_dir_all(&folder);
    let path = consent("claimed-on");
    let folder = path.parent().unwrap().to_path_buf();
    let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
    replied(&path, "Lamp, are you watching my desk?", "Yes, it's on.");
    assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists() && !face.exists());
    let _ = std::fs::remove_dir_all(folder);
}

/// The Reviewer-lead's must-fix 2 on #8674 (c7185816145): the backstop
/// runs only for a person's spoken turn (`Asker::Spoken`), where the stop
/// call would. A written turn (a skill's say, a look's prompt) whose reply
/// says "I've stopped watching your desk" keeps desk noticing on.
#[test]
fn a_written_turn_claiming_a_stop_runs_nothing() {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in};
    let path = consent("claimed-written");
    let folder = path.parent().unwrap().to_path_buf();
    let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
    let script = Arc::new(Script {
        chats: Mutex::new(
            vec![vec![
                text("I've stopped watching your desk."),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) })),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .telling_desk_noticing(Some(path.clone()));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(written(1, "Say that you stopped watching their desk."));
            turn_within(&mut engine, Duration::from_secs(5)).await;
        });
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists());
    assert!(allowed(&face, FACE_EXPRESSIONS, true) && !face.exists());
    let _ = std::fs::remove_dir_all(folder);
}

/// `heard` in request 1 (`None`: a written turn), answered `reply` with no
/// call, the state told from `path`: what the lamp said, as `said:` labels.
fn said_to(path: &Path, heard: Option<&str>, reply: &str) -> Vec<String> {
    let script = Arc::new(Script {
        heard: Mutex::new(heard.map(|line| Ok(line.to_owned())).into_iter().collect()),
        chats: Mutex::new(vec![vec![text(reply), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) })),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .telling_desk_noticing(Some(path.to_path_buf()));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            if heard.is_some() {
                engine.submit(start(1));
                engine.submit(audio(1, 1));
                engine.submit(end(1));
            } else {
                engine.submit(written(1, "Say that you are noticing their desk again."));
            }
            turn_within(&mut engine, Duration::from_secs(5)).await
        })
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect()
}

/// A stop's kept no for both, by voice: desk noticing off.
fn turned_off(path: &Path) {
    use lamp_msgs::consent::{BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, file_in, no_bytes};
    let face = file_in(path.parent().unwrap(), FACE_EXPRESSIONS).unwrap();
    std::fs::write(path, no_bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
    std::fs::write(face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
}

/// #8683's ROW (c7200527698, F4's first try on lamp-06c1, 27 Nov): with
/// desk noticing off, "Lamp, desk noticing on." got "I'm noticing your
/// desk again." with `calls=none`. The reply is said, then corrected with
/// the existing not-done line (`action_claim::NOT_DONE_LINE`), and nothing
/// is turned on: `look_at_my_desk` never runs (the Reviewer-lead's
/// c7202262971 and the first builder's c7202392079 on #7767). With it on,
/// the same reply is true and said alone.
#[test]
fn a_claimed_start_with_noticing_off_is_corrected_after_the_reply() {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in};
    const HEARD: &str = "Lamp, desk noticing on.";
    const REPLY: &str = "I'm noticing your desk again.";
    let path = consent("claimed-start");
    let folder = path.parent().unwrap().to_path_buf();
    let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
    turned_off(&path);
    let before = (std::fs::read(&path).unwrap(), std::fs::read(&face).unwrap());
    assert_eq!(
        said_to(&path, Some(HEARD), REPLY),
        [
            format!("said:{REPLY}"),
            // Each piece after the first is said after a space.
            format!("said: {}", crate::action_claim::NOT_DONE_LINE),
        ]
    );
    assert!(!allowed(&path, SEEN_ACTIVITY, true));
    assert!(!allowed(&face, FACE_EXPRESSIONS, true));
    assert_eq!(
        (std::fs::read(&path).unwrap(), std::fs::read(&face).unwrap()),
        before
    );
    let _ = std::fs::remove_dir_all(&folder);
    let path = consent("claimed-start-on");
    let folder = path.parent().unwrap().to_path_buf();
    assert_eq!(
        said_to(&path, Some(HEARD), REPLY),
        [format!("said:{REPLY}")]
    );
    assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists());
    let _ = std::fs::remove_dir_all(folder);
}

/// A written turn claiming a start is never corrected, and an offer to
/// start ("whenever you want") is no claim: each is said alone.
#[test]
fn a_written_turn_or_an_offer_to_start_is_not_corrected() {
    let path = consent("claimed-start-written");
    turned_off(&path);
    let reply = "I'm noticing your desk again.";
    assert_eq!(said_to(&path, None, reply), [format!("said:{reply}")]);
    let offer = "I'll start looking again whenever you want.";
    assert_eq!(
        said_to(&path, Some("Lamp, desk noticing on."), offer),
        [format!("said:{offer}")]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
