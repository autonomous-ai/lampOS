use super::*;
use std::sync::mpsc;

fn apps() -> Vec<AppsTriggers> {
    vec![
        AppsTriggers {
            app: "english-lessons".into(),
            triggers: vec![
                "start my english lesson".into(),
                "begin the english lesson".into(),
            ],
        },
        AppsTriggers {
            app: "store-test".into(),
            triggers: vec!["start the store test".into()],
        },
    ]
}

#[test]
fn a_whole_line_that_says_a_trigger_matches_its_app() {
    for (line, app, trigger) in [
        (
            "Start my English lesson.",
            "english-lessons",
            "start my english lesson",
        ),
        (
            "Please start my english lesson",
            "english-lessons",
            "start my english lesson",
        ),
        (
            "Lamp, can you begin the English lesson please?",
            "english-lessons",
            "begin the english lesson",
        ),
        ("start the store test", "store-test", "start the store test"),
    ] {
        assert_eq!(
            matched(&apps(), line),
            Some((app.to_owned(), trigger.to_owned())),
            "{line}"
        );
    }
}

#[test]
fn a_line_with_more_than_a_trigger_in_it_is_no_trigger() {
    for line in [
        "Don't start my English lesson.",
        "I want to start my english lesson tomorrow",
        "start my english lesson and turn the light on",
        "start my lesson",
        "",
        "yes",
    ] {
        assert_eq!(matched(&apps(), line), None, "{line}");
    }
    assert_eq!(matched(&[], "start my english lesson"), None);
}

#[test]
fn the_app_that_takes_the_line_ends_the_hold_and_another_request_does_not() {
    let (send, took) = mpsc::channel();
    let told = std::cell::RefCell::new(None);
    // A took left from an earlier line, and one for another request.
    send.send(AppTook {
        turn: 3,
        app: "english-lessons".into(),
    })
    .unwrap();
    let stray = send.clone();
    let taken = {
        let publish = |trigger: AppTrigger| {
            // The app answers once told: another request's took first.
            stray
                .send(AppTook {
                    turn: 8,
                    app: "english-lessons".into(),
                })
                .unwrap();
            stray
                .send(AppTook {
                    turn: 9,
                    app: "english-lessons".into(),
                })
                .unwrap();
            *told.borrow_mut() = Some(trigger);
        };
        told_and_taken(
            publish,
            &took,
            9,
            ("english-lessons".into(), "start my english lesson".into()),
            1234,
            Duration::from_secs(2),
        )
    };
    assert!(taken);
    let told = told.into_inner().unwrap();
    assert_eq!(
        (told.turn, told.app.as_str(), told.at_ms),
        (9, "english-lessons", 1234)
    );
    // Only the manifest's own words, never the line around them.
    assert_eq!(told.phrase, "start my english lesson");
    assert_eq!(told.trigger, told.phrase);
}

#[test]
fn a_line_nothing_takes_goes_back_to_the_model_after_the_hold() {
    let (_send, took) = mpsc::channel::<AppTook>();
    let started = std::time::Instant::now();
    let taken = told_and_taken(
        |_| {},
        &took,
        4,
        ("store-test".into(), "start the store test".into()),
        1,
        Duration::from_millis(150),
    );
    assert!(!taken);
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_switch_is_on_for_on_and_nothing_else() {
    // Not set here: off.
    assert_eq!(ENVIRONMENT, "LAMP_APP_TRIGGERS");
    assert!(!on() || std::env::var(ENVIRONMENT).as_deref() == Ok("on"));
}

#[test]
fn the_app_is_told_the_trigger_and_never_the_words_around_it() {
    let (send, took) = mpsc::channel();
    let told = std::cell::RefCell::new(None);
    let stray = send.clone();
    assert!(told_and_taken(
        |trigger| {
            stray
                .send(AppTook {
                    turn: 2,
                    app: "store-test".into(),
                })
                .unwrap();
            *told.borrow_mut() = Some(trigger);
        },
        &took,
        2,
        ("store-test".into(), "start the store test".into()),
        1,
        Duration::from_secs(2),
    ));
    drop(send);
    assert_eq!(told.into_inner().unwrap().phrase, "start the store test");
}

#[test]
fn a_trigger_is_heard_run_together_or_apart_and_with_or_without_apostrophes() {
    let apps = vec![AppsTriggers {
        app: "night-light".into(),
        triggers: vec!["good night lamp".into(), "what's up lamp".into()],
    }];
    for line in [
        "Goodnight lamp.",
        "good night, lamp",
        "Whats up lamp?",
        "what's up Lamp",
    ] {
        assert!(matched(&apps, line).is_some(), "{line}");
    }
    assert_eq!(matched(&apps, "goodnight to everyone"), None);
}

/// One line's wait never delays another's: two matched lines at once are each
/// answered by their own took, and neither waits for the other.
#[test]
fn two_lines_waiting_at_once_each_get_their_own_took_without_waiting_on_each_other() {
    use lamp_bus::Bus;
    let dir = std::env::temp_dir().join(format!("at{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut triggers = bus
        .publisher_declared(AppTriggers::TOPIC, &AppTriggers::declaration(), "control")
        .unwrap();
    let mut took = bus
        .publisher_declared(AppTook::TOPIC, &AppTook::declaration(), "control")
        .unwrap();
    triggers
        .publish_json(
            lamp_ipc::monotonic_us(),
            &AppTriggers {
                apps: vec![AppsTriggers {
                    app: "store-test".into(),
                    triggers: vec!["start the store test".into()],
                }],
            },
        )
        .unwrap();
    let take = line_taker(&dir).expect("a taker on a real bus");
    let started = std::time::Instant::now();
    let lines: Vec<_> = [5u64, 6]
        .into_iter()
        .map(|request| {
            let take = take.clone();
            std::thread::spawn(move || take(request, "start the store test"))
        })
        .collect();
    // The app answers the second request first, then the first, 300 ms apart.
    std::thread::sleep(Duration::from_millis(500));
    for turn in [6u64, 5] {
        took.publish_json(
            lamp_ipc::monotonic_us(),
            &AppTook {
                turn,
                app: "store-test".into(),
            },
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    let taken: Vec<bool> = lines.into_iter().map(|line| line.join().unwrap()).collect();
    assert_eq!(taken, [true, true]);
    assert!(
        started.elapsed() < Duration::from_millis(2_500),
        "the two waits overlap: {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_trigger_made_only_of_the_lamps_own_words_is_never_matched() {
    // An app whose manifest says these must not take the person's reply to
    // the lamp's read-back, its forget or its stop.
    let apps = vec![AppsTriggers {
        app: "greedy-app".into(),
        triggers: vec![
            "yes go ahead".into(),
            "send it now".into(),
            "forget everything".into(),
            "stop it please".into(),
            "start the greedy thing".into(),
        ],
    }];
    for line in [
        "yes go ahead",
        "Send it now.",
        "forget everything",
        "stop it please",
    ] {
        assert_eq!(matched(&apps, line), None, "{line}");
    }
    // One of its own words among others still matches.
    assert!(matched(&apps, "start the greedy thing").is_some());
}
