//! `LAMP_SAME_VOICE_CONFIRMS` (the memory owner's decision of 4 Nov: CAM++
//! one way only): the known person's own voice confirms their line as
//! seen said by them while their face is the one face in view, and a score
//! under the bar does nothing.
use super::*;
use crate::same_voice::{CONFIRM_BAR, OWNER_LEAST_PRINTS, OWNER_PRINTS, OwnerVoice};

const FACE: &str = "b1/f3";
const PERSON: &str = "person_owner";

/// Their own voice, another 0.95 like it, and one 0.20 like it.
const OWN: [f32; 3] = [1.0, 0.0, 0.0];
const NEAR: [f32; 3] = [0.95, 0.312, 0.0];
const FAR: [f32; 3] = [0.2, 0.98, 0.0];

/// A confirm at `bar` whose every line's print is `print`.
fn confirms(bar: f32, print: Option<[f32; 3]>) -> VoiceConfirms {
    VoiceConfirms {
        owner: Arc::new(OwnerVoice::new(bar)),
        print_of: Arc::new(move |_| print.map(|print| print.to_vec())),
        setup: None,
    }
}

/// `confirms` taught their voice by `OWNER_LEAST_PRINTS` lines their face
/// alone was seen speaking, the fewest any line is judged against.
fn taught(confirms: &VoiceConfirms) {
    for request in 101..101 + OWNER_LEAST_PRINTS as u64 {
        assert!(confirms.owner.learn(PERSON, request, OWN.to_vec()));
    }
}

fn confirm(
    confirms: Option<&VoiceConfirms>,
    request: u64,
    speakers: &[&str],
    in_view: bool,
) -> Vec<String> {
    confirmed_by_voice(
        confirms,
        request,
        PERSON,
        speakers.iter().map(|id| (*id).to_owned()).collect(),
        Some(FACE),
        |face| in_view && face == FACE,
    )
}

/// The known file's name for them, and their face.
fn known() -> Option<(String, String)> {
    Some(("Linh".to_owned(), FACE.to_owned()))
}

/// A high match, their face the one face in view: their face is seen
/// saying it, so every "seen saying" check reads it as theirs.
#[test]
fn a_high_match_with_their_face_in_view_is_seen_said_by_them() {
    let confirms = confirms(CONFIRM_BAR, Some(NEAR));
    taught(&confirms);
    let speakers = confirm(Some(&confirms), 7, &[], true);
    assert_eq!(speakers, vec![FACE.to_owned()]);
    // Read as the scope reads it, for the known file's person.
    let person = crate::known_faces::memory_person("Linh");
    assert!(crate::speaking_faces::seen_saying(
        &speakers,
        &person,
        known()
    ));
    assert!(!crate::speaking_faces::not_theirs(
        &speakers,
        &person,
        known(),
        true
    ));
    // Unconfirmed, the same line is not theirs with the scope seen.
    assert!(crate::speaking_faces::not_theirs(
        &[],
        &person,
        known(),
        true
    ));
}

/// A high match with no face in view, or another face in it, names nobody.
#[test]
fn a_high_match_with_no_face_in_view_does_nothing() {
    let confirms = confirms(CONFIRM_BAR, Some(NEAR));
    taught(&confirms);
    assert!(confirm(Some(&confirms), 7, &[], false).is_empty());
    // No face of theirs known: nothing either.
    assert!(confirmed_by_voice(Some(&confirms), 7, PERSON, Vec::new(), None, |_| true).is_empty());
    // Another face seen speaking it stays another face.
    assert_eq!(
        confirm(Some(&confirms), 7, &["b1/f9"], true),
        vec!["b1/f9".to_owned()]
    );
}

/// A score under the bar changes nothing: the speakers come back as they
/// went in, so the line stays as it was without the switch, never held or
/// ended by it (the voices judge nothing: `same_voice`'s own tests).
#[test]
fn a_low_match_does_nothing() {
    let confirms = confirms(CONFIRM_BAR, Some(FAR));
    taught(&confirms);
    assert!(confirm(Some(&confirms), 7, &[], true).is_empty());
    // No print, or none of theirs kept yet: nothing.
    let unprinted = self::confirms(CONFIRM_BAR, None);
    taught(&unprinted);
    assert!(confirm(Some(&unprinted), 7, &[], true).is_empty());
    let untaught = self::confirms(CONFIRM_BAR, Some(OWN));
    assert!(confirm(Some(&untaught), 7, &[], true).is_empty());
}

/// The switch off: the speakers are returned untouched, whatever they are.
#[test]
fn the_switch_off_changes_nothing() {
    for speakers in [&[][..], &[FACE][..], &["b1/f9"][..], &[FACE, "b1/f9"][..]] {
        for in_view in [false, true] {
            let expected: Vec<String> = speakers.iter().map(|id| (*id).to_owned()).collect();
            assert_eq!(confirm(None, 7, speakers, in_view), expected);
        }
    }
}

/// The bar is the one given, not a fixed one: 0.95 like theirs passes a
/// bar of 0.9 and fails one of 0.96.
#[test]
fn the_bar_given_is_the_bar() {
    let low = confirms(0.9, Some(NEAR));
    taught(&low);
    assert_eq!(confirm(Some(&low), 7, &[], true), vec![FACE.to_owned()]);
    let high = confirms(0.96, Some(NEAR));
    taught(&high);
    assert!(confirm(Some(&high), 7, &[], true).is_empty());
}

/// Their voice is learnt only from lines their face alone was seen
/// speaking, their face the one face in view, at most `OWNER_PRINTS`, and
/// never from a line the voice itself confirmed.
#[test]
fn their_voice_is_learnt_only_from_lines_seen_spoken_by_them_alone() {
    let confirms = confirms(CONFIRM_BAR, Some(OWN));
    // A face seen speaking it, but another face in view: not learnt.
    confirm(Some(&confirms), 1, &[FACE], false);
    assert_eq!(confirms.owner.known(PERSON), 0);
    // Two faces seen speaking: not learnt.
    confirm(Some(&confirms), 2, &[FACE, "b1/f9"], true);
    assert_eq!(confirms.owner.known(PERSON), 0);
    for request in 3..3 + OWNER_PRINTS as u64 + 2 {
        confirm(Some(&confirms), request, &[FACE], true);
    }
    assert_eq!(confirms.owner.known(PERSON), OWNER_PRINTS);
    // A confirmed line is not learnt from.
    let fresh = self::confirms(CONFIRM_BAR, Some(OWN));
    // Fewer than `OWNER_LEAST_PRINTS` kept: their own voice confirms nothing.
    for request in 1..OWNER_LEAST_PRINTS as u64 {
        assert!(fresh.owner.learn(PERSON, request, OWN.to_vec()));
    }
    assert!(confirm(Some(&fresh), 50, &[], true).is_empty());
    assert!(fresh.owner.learn(PERSON, 99, OWN.to_vec()));
    assert_eq!(confirm(Some(&fresh), 51, &[], true), vec![FACE.to_owned()]);
    assert_eq!(fresh.owner.known(PERSON), OWNER_LEAST_PRINTS);
}

/// A line already spoken in the old walk must not enter a redo merely
/// because its STT finishes late. Synthetic eyes/prints, real Engine and
/// validated redo mailbox post; the progress write models its consumer.
#[tokio::test]
async fn a_setup_line_waiting_for_stt_cannot_enrol_in_a_redone_walk() {
    setup_line_across_redo(false).await;
}

#[tokio::test]
async fn a_setup_start_keeps_its_generation_across_an_early_clock_callback() {
    setup_line_across_redo(true).await;
}

async fn setup_line_across_redo(early: bool) {
    use crate::memory_scope::FaceCache;
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    use std::os::unix::fs::MetadataExt;
    let dir = std::env::temp_dir().join(format!(
        "setup-start-gap-{early}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let progress = dir.join("onboarding.json");
    std::fs::write(&progress, r#"{"steps":{}}"#).unwrap();
    let store = crate::owner_voice::Store {
        path: dir.join(crate::setup_voice::FILE),
        uid: std::fs::metadata(&progress).unwrap().uid(),
    };
    let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
    let journal = Arc::new(Mutex::new(Vec::<String>::new()));
    let logged = journal.clone();
    let setup = Arc::new(crate::setup_voice::SetupVoice::new(
        progress.clone(),
        store,
        owner.clone(),
        Arc::new(move |line| logged.lock().unwrap().push(line.to_owned())),
        crate::setup_voice::Timing {
            print_within: Duration::from_millis(400),
            settle_within: Duration::from_secs(5),
            every: Duration::from_millis(5),
        },
    ));
    crate::setup_voice::register(
        crate::owner_voice::Store {
            path: dir.join(crate::setup_voice::FILE),
            uid: std::fs::metadata(&progress).unwrap().uid(),
        },
        Some(setup.clone()),
    );
    let gate = Arc::new(FinishGate::default());
    let script = Arc::new(Script {
        heard: Mutex::new(VecDeque::from([
            Ok("My name is Linh.".into()),
            Ok("I am still Linh.".into()),
        ])),
        chats: Mutex::new(VecDeque::from([
            vec![text("Hello."), Ok(ChatEvent::Done)],
            vec![text("Hello again."), Ok(ChatEvent::Done)],
        ])),
        finish_gate: Some(gate.clone()),
        ..Script::default()
    });
    let eyes = Arc::new(SpeakingFaces::default());
    eyes.look(
        Instant::now(),
        Some((("setup-gap", 0), &lamp_msgs::Faces { faces: vec![] })),
    );
    std::thread::sleep(Duration::from_millis(20));
    let known = dir.join("known");
    std::fs::write(&known, "Linh").unwrap();
    std::fs::write(known_face_of(&known), "f1\n").unwrap();
    let cache = Arc::new(FaceCache::default());
    let (engine, _, _) = scripted(script);
    let mut engine = engine
        .confirming_by_voice(VoiceConfirms {
            owner,
            setup: Some(setup),
            print_of: Arc::new(|_| Some(OWN.to_vec())),
        })
        .with_memory_scope(
            lamp_turn_owner::BootId::new([30; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| Box::pin(async { json!({"ok":true}) })),
            Some(Arc::new(|_| "SYSTEM".to_owned().into())),
        )
        .seeing_who_speaks(eyes.clone(), known.clone())
        .resuming_after_wordless_cut_ins(early)
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known),
    );
    // Install after camera/cache setup so the first invocation belongs to
    // actual spoken Start processing. The callback uses no Engine clock.
    let fired = Arc::new(AtomicBool::new(false));
    if early {
        let fired = fired.clone();
        let progress = progress.clone();
        Arc::get_mut(&mut engine.shared).unwrap().clock = Arc::new(move || {
            if !fired.swap(true, Ordering::SeqCst) {
                let told = crate::onboarding::redo_for_setup_test(&progress, "who_you_are");
                assert_eq!(told.0["ok"], true);
                assert!(
                    std::fs::read_to_string(crate::onboarding::mailbox_of(&progress))
                        .unwrap()
                        .contains("who_you_are")
                );
                std::fs::write(&progress, r#"{"steps":{}}"#).unwrap();
            }
            Instant::now()
        });
    }
    let done = Arc::new(AtomicBool::new(false));
    let stopped = done.clone();
    let feeder = std::thread::spawn(move || {
        let mut frame = 1;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
            eyes.look(
                Instant::now(),
                Some((
                    ("setup-gap", frame),
                    &lamp_msgs::Faces {
                        faces: vec![lamp_msgs::Face {
                            id: "f1".into(),
                            xmin: 0.1,
                            ymin: 0.1,
                            xmax: 0.3,
                            ymax: 0.4,
                            confidence: 1.0,
                            yaw: None,
                            facing: None,
                            follows: None,
                            speaking: Some(lamp_msgs::Speaking {
                                state: lamp_msgs::SpeakingState::Yes,
                                score: 0.9,
                                from: None,
                            }),
                        }],
                    },
                )),
            );
            frame += 1;
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    for request in [45_400, 45_401] {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(request));
        tokio::time::timeout(Duration::from_secs(2), gate.entered.notified())
            .await
            .unwrap();
        if request == 45_400 && !early {
            let told = crate::onboarding::redo_for_setup_test(&progress, "who_you_are");
            assert_eq!(told.0["ok"], true);
            assert!(
                std::fs::read_to_string(crate::onboarding::mailbox_of(&progress))
                    .unwrap()
                    .contains("who_you_are")
            );
            std::fs::write(&progress, r#"{"steps":{}}"#).unwrap();
        }
        gate.resume.notify_one();
        tokio::time::timeout(Duration::from_secs(2), turn(&mut engine))
            .await
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !journal
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(&format!("request {request}:")))
            && Instant::now() < until
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        engine.retire_through(request);
    }
    let lines = journal.lock().unwrap().clone();
    // Release all held work before asserting, including on the failing head.
    crate::setup_voice::owner_face_gone(&progress);
    done.store(true, Ordering::SeqCst);
    feeder.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(fired.load(Ordering::SeqCst), early, "early clock callback");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("request 45401: its print held")),
        "fresh control: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("request 45400:")),
        "old reader must finish: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("request 45400: its print held")),
        "pre-redo audio entered the fresh walk: {lines:?}"
    );
}
