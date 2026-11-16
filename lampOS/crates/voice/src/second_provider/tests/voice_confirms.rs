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

/// A pending print cannot teach the old identity or confirm against a new
/// enrollment after forget. The print callback models the exact blocking
/// boundary without sleeps or real biometric data.
#[test]
fn forgetting_during_a_print_fences_learning_and_confirmation() {
    for seen in [true, false] {
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        for request in 1..=4 {
            assert!(owner.learn(PERSON, request, OWN.to_vec()));
        }
        let before = owner.generation();
        let during = owner.clone();
        let confirms = VoiceConfirms {
            owner: owner.clone(),
            print_of: Arc::new(move |_| {
                during.forget_person(PERSON);
                // Fresh observations may teach the same identity again, but
                // the old confirmation must not use their new prints.
                for request in 11..=14 {
                    assert!(during.learn(PERSON, request, OWN.to_vec()));
                }
                Some(OWN.to_vec())
            }),
        };
        let speakers = if seen { vec![FACE.to_owned()] } else { vec![] };
        let actual = confirmed_by_voice_at(
            Some(&confirms),
            Some(before),
            20,
            PERSON,
            speakers.clone(),
            Some(FACE),
            |_| true,
        );
        assert_eq!(actual, speakers);
        assert!(!owner.learn_at(before, PERSON, 21, OWN.to_vec()));
        assert_eq!(
            owner.judge_at(before, PERSON, &OWN),
            crate::same_voice::Confirmed::Unknown
        );
        assert_eq!(owner.known(PERSON), 4);
    }
}

#[test]
fn a_late_face_observation_keeps_its_pre_forget_fence() {
    let confirms = confirms(CONFIRM_BAR, Some(OWN));
    let pending_face = confirms.owner.generation();
    confirms.owner.forget_person(PERSON);
    let speakers = vec![FACE.to_owned()];
    assert_eq!(
        confirmed_by_voice_at(
            Some(&confirms),
            Some(pending_face),
            1,
            PERSON,
            speakers.clone(),
            Some(FACE),
            |_| true
        ),
        speakers
    );
    assert_eq!(confirms.owner.known(PERSON), 0);
    // A genuinely new observed line is still learnable.
    confirm(Some(&confirms), 2, &[FACE], true);
    assert_eq!(confirms.owner.known(PERSON), 1);
}

#[test]
fn successful_face_forget_clears_only_its_person_and_fences_pending_prints() {
    let owner = OwnerVoice::new(CONFIRM_BAR);
    for request in 1..=4 {
        assert!(owner.learn(PERSON, request, OWN.to_vec()));
    }
    let pending = owner.generation();
    let forget = |target: &str, heard, told: Value| {
        crate::tools::forget_confirmation_voice(
            &owner,
            crate::known_faces::FORGET,
            Some(target),
            heard,
            None,
            &told,
        )
    };
    forget(PERSON, true, json!({"error":"refused"}));
    forget(PERSON, false, json!({"ok":true}));
    assert_eq!(owner.generation(), pending);
    assert_eq!(owner.known(PERSON), 4);
    forget("person_else", true, json!({"ok":true}));
    assert_eq!(owner.known(PERSON), 4);
    forget(PERSON, true, json!({"ok":true}));
    assert_eq!(owner.known(PERSON), 0);
    assert!(!owner.learn_at(pending, PERSON, 5, OWN.to_vec()));
    assert!(owner.learn(PERSON, 6, OWN.to_vec()));
}

#[test]
fn an_unadmitted_whole_forget_receipt_does_not_clear_confirmation_prints() {
    let owner = OwnerVoice::new(CONFIRM_BAR);
    assert!(owner.learn(PERSON, 1, OWN.to_vec()));
    let before = owner.generation();
    crate::tools::forget_confirmation_voice(
        &owner,
        crate::remember::FORGET_EVERYTHING,
        Some(PERSON),
        true,
        None,
        &json!({"forgotten":true}),
    );
    assert_eq!(owner.known(PERSON), 1);
    assert_eq!(owner.generation(), before);
}

/// The real Start arm calls its clock before begin when wordless resumes
/// is enabled. Forget/relearn there deterministically models Start pausing
/// across deletion, without a production-only testing hook.
#[tokio::test]
async fn start_processing_keeps_its_fence_across_an_early_callback() {
    use crate::memory_scope::FaceCache;
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    let person = crate::remember::person_id("Linh");
    let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
    let script = Arc::new(Script {
        heard: Mutex::new(VecDeque::from([
            Ok("Tell me a story.".into()),
            Ok("Tell me another story.".into()),
        ])),
        chats: Mutex::new(VecDeque::from([
            vec![text("Here is a story."), Ok(ChatEvent::Done)],
            vec![text("Here is another story."), Ok(ChatEvent::Done)],
        ])),
        ..Script::default()
    });
    let eyes = Arc::new(SpeakingFaces::default());
    eyes.look(
        Instant::now(),
        Some((("start-fence", 0), &lamp_msgs::Faces { faces: vec![] })),
    );
    std::thread::sleep(Duration::from_millis(20));
    let path = std::env::temp_dir().join(format!(
        "confirmation-start-fence-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "Linh").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    let cache = Arc::new(FaceCache::default());
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    let named = Arc::new(Mutex::new(Vec::new()));
    let told = named.clone();
    let (engine, _, _) = scripted(script);
    let printed = Arc::new(AtomicUsize::new(0));
    let read_print = printed.clone();
    let mut engine = engine
        .confirming_by_voice(VoiceConfirms {
            owner: owner.clone(),
            print_of: Arc::new(move |_| {
                read_print.fetch_add(1, Ordering::SeqCst);
                Some(OWN.to_vec())
            }),
        })
        .with_memory_scope(
            lamp_turn_owner::BootId::new([29; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| Box::pin(async { json!({"ok":true}) })),
            Some(Arc::new(move |lease| {
                told.lock().unwrap().push(
                    lease
                        .scope()
                        .and_then(|scope| scope.person())
                        .map(str::to_owned),
                );
                "SYSTEM".to_owned().into()
            })),
        )
        .seeing_who_speaks(eyes.clone(), path.clone())
        .scoping_by_sight(true)
        .resuming_after_wordless_cut_ins(true)
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    let armed = Arc::new(AtomicBool::new(true));
    let fired = armed.clone();
    let forgotten = owner.clone();
    let forgotten_person = person.clone();
    Arc::get_mut(&mut engine.shared).unwrap().clock = Arc::new(move || {
        if fired.swap(false, Ordering::SeqCst) {
            forgotten.forget_person(&forgotten_person);
            for request in 100..104 {
                assert!(forgotten.learn(&forgotten_person, request, OWN.to_vec()));
            }
        }
        Instant::now()
    });
    let done = Arc::new(AtomicBool::new(false));
    let feed_done = done.clone();
    let feeder = std::thread::spawn(move || {
        // Also stop if a test assertion unwinds before normal cleanup.
        let until = Instant::now() + Duration::from_secs(30);
        let mut frame = 1;
        while !feed_done.load(Ordering::SeqCst) && Instant::now() < until {
            eyes.look(
                Instant::now(),
                Some((
                    ("start-fence", frame),
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
                                state: lamp_msgs::SpeakingState::No,
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
    for request in [45_300, 45_301] {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(request));
        turn(&mut engine).await;
        engine.retire_through(request);
    }
    done.store(true, Ordering::SeqCst);
    feeder.join().unwrap();
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(path);
    assert!(
        !armed.load(Ordering::SeqCst),
        "Start callback must have run"
    );
    assert_eq!(
        printed.load(Ordering::SeqCst),
        2,
        "both turns must reach actual confirmation"
    );
    assert_eq!(
        *named.lock().unwrap(),
        vec![None, Some(person)],
        "the old Start cannot confirm against fresh prints; the fresh Start can"
    );
}
