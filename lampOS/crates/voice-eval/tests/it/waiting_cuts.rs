//! `waiting-cuts` (#1866) on an invented journal excerpt and two invented
//! sessions (`tests/data/waiting-cuts/`): the first run on the live route with
//! the second standing in, `LAMP_WAIT_KEEPS_ANSWER=on`; the second on the
//! second route only, with `words`. Every count and time on both routes, and
//! no word of the fixture in what is printed.
use lamp_voice_eval::waiting_cuts::{self, Session};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn data() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/waiting-cuts")
}

/// The fixture's sessions, each made when `sessions.json` says (a checkout
/// sets no folder's time).
fn sessions() -> Vec<Session> {
    let starts: Value =
        serde_json::from_str(&fs::read_to_string(data().join("sessions.json")).unwrap()).unwrap();
    starts
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, start)| {
            let (at, _) = waiting_cuts::parse_time(start.as_str().unwrap()).unwrap();
            (data().join("sessions").join(name), at, at + 60)
        })
        .collect()
}

fn journal() -> String {
    fs::read_to_string(data().join("journal.txt")).unwrap()
}

fn counts(run: &Value, route: &str) -> Value {
    run["counts"][route].clone()
}

#[test]
fn both_routes_count_every_wait_cut_stop_and_time() {
    let report = waiting_cuts::report(
        &waiting_cuts::count(&journal(), &sessions(), None, None),
        "fixture",
    );
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let (one, two) = (&runs[0], &runs[1]);
    assert_eq!(one["started"], json!("2026-10-19 20:00:00"));
    assert_eq!(
        (&one["route_at_start"], &one["wait_mode"], &one["sessions"]),
        (&json!("live"), &json!("on"), &json!(1))
    );
    assert_eq!(
        (&two["route_at_start"], &two["wait_mode"], &two["sessions"]),
        (&json!("second"), &json!("words"), &json!(1))
    );
    assert_eq!(
        counts(one, "live"),
        json!({"cut_waiting_by_no_words": 1, "cut_waiting_by_no_words_after_a_wait": 1,
            "cut_waiting_by_words": 1, "short_follow_up_kept": 1, "cut_while_answer_sounds": 1,
            "wait_held_a_sound": 2, "wait_let_go_quiet": 1, "wait_took_turn": 1,
            "stop_hand": 1, "went_on_after_early_end": 1})
    );
    assert_eq!(
        counts(one, "second"),
        json!({"wait_held_a_sound": 1, "wait_let_go_ended": 1,
            "second_heard_words": 1, "second_goes_on": 1})
    );
    assert_eq!(counts(two, "live"), json!({}));
    assert_eq!(
        counts(two, "second"),
        json!({"cut_waiting_by_words": 1, "cut_waiting_by_words_after_a_wait": 1,
            "short_follow_up_kept": 1, "wait_held_a_sound": 4, "wait_took_turn": 1,
            "wait_stop_said": 1, "wait_let_go_quiet": 2, "wait_words_asking": 2,
            "wait_words_asking_nothing": 1, "wait_words_none_in_time": 1, "stop_spoken": 1,
            "second_heard_words": 1, "second_heard_no_words": 1, "second_first_audio": 1})
    );
    let s = |n: u64, median: f64, p90: f64, max: f64| json!({"n": n, "median": median, "p90": p90, "max": max});
    assert_eq!(
        one["times_ms"],
        json!({
            "live": {
                "awaited_before_cut_ms": s(2, 1250.0, 1500.0, 1500.0),
                "stop_dispatch_ms": s(1, 3.0, 3.0, 3.0),
                "wait_hold_ms": s(2, 850.1, 1000.1, 1000.1),
            },
            "second": {
                "second_words_after_line_ms": s(1, 210.0, 210.0, 210.0),
                "wait_hold_ms": s(1, 300.0, 300.0, 300.0),
            },
        })
    );
    assert_eq!(two["times_ms"]["live"], json!({}));
    assert_eq!(
        two["times_ms"]["second"],
        json!({
            "awaited_before_cut_ms": s(1, 1250.1, 1250.1, 1250.1),
            "second_first_audio_ms": s(1, 900.0, 900.0, 900.0),
            "second_words_after_line_ms": s(2, 115.0, 180.0, 180.0),
            "stop_dispatch_ms": s(1, 2.0, 2.0, 2.0),
            "stop_phrase_acts_ms": s(1, 120.0, 120.0, 120.0),
            "wait_hold_ms": s(4, 600.0, 750.1, 750.1),
            "wait_stop_to_cancel_ms": s(1, 10.0, 10.0, 10.0),
            "wait_words_ms": s(4, 100.0, 150.0, 150.0),
            "wait_words_stream_ready_ms": s(1, 640.0, 640.0, 640.0),
        })
    );
    assert_eq!(one["journal"], json!({"stop_words_ready": 1}));
    assert_eq!(
        two["journal"],
        json!({"route_second_only": 1, "wait_words_ready": 1, "wait_words_stream_ready": 1,
            "yield_words_stop": 1, "stop_phrase_acts": 1, "short_sound_after_stop": 1})
    );
    assert_eq!(one["unclassified"], json!({"answer_waits": 1}));
    assert_eq!(two["unclassified"], json!({"journal:request": 2}));

    // Nothing said, and no free text, ever reaches the output.
    let printed = format!("{}\n{report}\n{report:#}", waiting_cuts::table(&report));
    for word in [
        "quokka",
        "zebrafinch",
        "marmalade",
        "Pemberton",
        "lighthouse",
        "remember_",
        "relay",
        "usb",
    ] {
        assert!(!printed.contains(word), "{word} printed");
    }
    assert!(printed.contains("cut_waiting_by_words"));
}

#[test]
fn since_and_until_filter_the_journals_own_stamps() {
    let since = waiting_cuts::wall_bound("2026-10-19 20:30").unwrap();
    let runs = waiting_cuts::count(&journal(), &sessions(), Some(&since), None);
    let report = waiting_cuts::report(&runs, "fixture");
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["route_at_start"], json!("second"));
    assert_eq!(runs[0]["sessions"], json!(1));
    assert_eq!(runs[0]["wait_mode"], json!("words"));

    let until = waiting_cuts::wall_bound("2026-10-19T20:10:00").unwrap();
    let runs = waiting_cuts::count(&journal(), &sessions(), None, Some(&until));
    let report = waiting_cuts::report(&runs, "fixture");
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["route_at_start"], json!("live"));
    assert_eq!(runs[0]["wait_mode"], json!("on"));
}

#[test]
fn without_session_records_the_table_says_what_it_needs() {
    let report = waiting_cuts::report(&waiting_cuts::count(&journal(), &[], None, None), "nowhere");
    let table = waiting_cuts::table(&report);
    assert!(table.contains("no session records"), "{table}");
    assert_eq!(report["runs"][0]["wait_mode"], Value::Null);
}
