//! Authored comparisons of decision rules, not replayed or labelled lamp data.
use lamp_policy::{
    attention::{Choice, Context, Room as PolicyRoom},
    policy::{
        Seen,
        baseline::{
            Counts, MOST_TEXT_BYTES, Permission, Room, Snapshot, SnapshotKind, called, judge,
            sure_name,
        },
    },
};
use serde_json::{Value, json};

fn snapshot() -> Snapshot {
    Snapshot {
        kind: SnapshotKind::SameSnapshot,
        room: Room::Busy,
        own_turn: false,
        going_on: false,
        named_sure: false,
        text: String::new(),
        facing: None,
    }
}

#[test]
fn strict_busy_truth_table_covers_each_permission_and_unknown() {
    // The actual first-output rule includes its separate sure-name latch.
    for kind in [SnapshotKind::SameSnapshot, SnapshotKind::VoiceFirstOutput] {
        for room in [Room::Busy, Room::Quiet, Room::Unknown] {
            for own_turn in [false, true] {
                for going_on in [false, true] {
                    for named_sure in [false, true] {
                        for text_called in [false, true] {
                            for facing in [None, Some(false), Some(true)] {
                                let row = Snapshot {
                                    kind,
                                    room,
                                    own_turn,
                                    going_on,
                                    named_sure,
                                    text: if text_called { "Lamp" } else { "a report" }.into(),
                                    facing,
                                };
                                let expected = match (
                                    room,
                                    own_turn,
                                    going_on,
                                    named_sure,
                                    text_called,
                                    facing,
                                ) {
                                    (
                                        Room::Busy,
                                        false,
                                        false,
                                        false,
                                        false,
                                        None | Some(false),
                                    ) => Permission::Hold,
                                    _ => Permission::Play,
                                };
                                assert_eq!(judge(&row), expected, "{row:?}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn sure_name_threshold_preserves_unknown_and_the_upstream_score_boundary() {
    for score in [
        None,
        Some(f32::NAN),
        Some(f32::INFINITY),
        Some(-0.1),
        Some(1.1),
        Some(0.997),
        Some(0.9989999),
    ] {
        assert!(!sure_name(score), "{score:?}");
    }
    for score in [Some(0.999), Some(1.0)] {
        assert!(sure_name(score), "{score:?}");
        let row = Snapshot {
            named_sure: sure_name(score),
            ..snapshot()
        };
        assert_eq!(judge(&row), Permission::Play);
    }
    // This is an explicitly supplied latch, not recomputed from the latest
    // score. Voice retains a qualifying hearing even if a weaker one follows.
    let row = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        named_sure: true,
        ..snapshot()
    };
    assert_eq!(judge(&row), Permission::Play);
}

#[test]
fn literal_name_is_a_whole_word_anywhere() {
    for text in [
        "Lamp",
        "lAmP",
        "tell me, LAMB!",
        "Can you help, Lamp?",
        "Đèn Lamp ơi",
        "not for lamp's ears",
        "123 lamp 456",
        "\0Lamp\0",
    ] {
        assert!(called(text), "{text:?}");
    }
    for text in [
        "lamps",
        "lamping",
        "clamp",
        "lamp2",
        "lamp模型",
        "LÁMP",
        "La\u{301}mp",
        "ｌａｍｐ",
        "",
        "...",
    ] {
        assert!(!called(text), "{text:?}");
    }
}

#[test]
fn only_the_exact_misheard_aliases_in_the_first_non_greeting_position_match() {
    let aliases = [
        "lam", "lan", "milan", "man", "plan", "helen", "mom", "amen", "weiland",
    ];
    for alias in aliases {
        assert!(called(alias), "{alias}");
        assert!(called(&format!("{}, help", alias.to_ascii_uppercase())));
        for greeting in ["hey", "hi", "ok", "okay", "oh", "hello"] {
            assert!(called(&format!("{greeting}, {alias}: help")));
        }
        assert!(called(&format!("Hey hi, OK okay oh hello {alias}")));
        assert!(!called(&format!("ask {alias} about it")));
        assert!(!called(&format!("hello there {alias}")));
    }
    // These occur in other name lists or resemble an alias, but the pinned
    // Voice predicate does not recognize them here.
    for text in [
        "Hey Liam",
        "Wei Lan",
        "Weimar",
        "good morning Helen",
        "hey hi okay",
        "please plan",
        "planning",
        "woman",
        "Lantern",
    ] {
        assert!(!called(text), "{text:?}");
    }
    assert!(called("ask Helen to turn on Lamp"));
}

#[test]
fn unknown_room_plays_but_unknown_eyes_do_not_open_a_busy_room() {
    let mut row = snapshot();
    assert_eq!(judge(&row), Permission::Hold);
    row.facing = Some(false);
    assert_eq!(judge(&row), Permission::Hold);
    row.room = Room::Unknown;
    assert_eq!(judge(&row), Permission::Play);
    row.facing = None;
    assert_eq!(judge(&row), Permission::Play);
}

#[test]
fn a_later_transcript_must_not_be_substituted_for_the_first_output_prefix() {
    let first = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        text: "Good morning".into(),
        ..snapshot()
    };
    let completed = Snapshot {
        kind: SnapshotKind::SameSnapshot,
        text: "Good morning, Lamp".into(),
        ..first.clone()
    };
    assert_eq!(judge(&first), Permission::Hold);
    assert_eq!(judge(&completed), Permission::Play);
    let mut counts = Counts::default();
    counts.note(Some(&first), Choice::Answer, None);
    counts.note(Some(&completed), Choice::Answer, None);
    assert_eq!(counts.voice_first_output.permission_disagreements, 1);
    assert_eq!(counts.same_snapshot.permission_disagreements, 0);
}

#[test]
fn a_small_facing_face_opens_voice_even_when_the_policy_near_count_is_zero() {
    let face = lamp_msgs::Face {
        speaking: None,
        id: "authored-distant-face".into(),
        xmin: 0.4,
        ymin: 0.4,
        xmax: 0.5,
        ymax: 0.45,
        confidence: 1.0,
        yaw: Some(0.0),
        facing: Some(true),
    };
    face.check().unwrap();
    assert!(face.ymax - face.ymin < 0.15);
    // These are the two different aggregations of this authored face. No
    // geometry or pose accuracy is established by supplying the boolean.
    let seen = Seen {
        faces: 1,
        near: 0,
        facing: 0,
        age_ms: 0,
    };
    let voice = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        facing: Some(face.facing == Some(true)),
        ..snapshot()
    };
    let policy_proxy = Snapshot {
        kind: SnapshotKind::SameSnapshot,
        facing: Some(seen.facing > 0),
        ..voice.clone()
    };
    assert_eq!(judge(&voice), Permission::Play);
    assert_eq!(judge(&policy_proxy), Permission::Hold);
}

#[test]
fn a_30_second_exchange_is_not_the_policy_20_second_conversation_flag() {
    let context = Context {
        heard: "And tomorrow?".into(),
        room: PolicyRoom::Busy,
        lamp_is_speaking: false,
        lamp_last_said: Some("Rain this afternoon.".into()),
        seconds_since_lamp_spoke: Some(30.0),
        name_heard: None,
    };
    assert!(!context.in_conversation());
    // The fixture explicitly supplies Voice's 45-second exchange flag.
    // The comparator must not recompute it from Policy's different context.
    let voice = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        going_on: true,
        text: context.heard.clone(),
        ..snapshot()
    };
    let policy_proxy = Snapshot {
        kind: SnapshotKind::SameSnapshot,
        going_on: context.in_conversation(),
        ..voice.clone()
    };
    assert_eq!(judge(&voice), Permission::Play);
    assert_eq!(judge(&policy_proxy), Permission::Hold);
}

#[test]
fn every_snapshot_field_is_required_and_explicit_null_facing_round_trips() {
    let value = serde_json::to_value(snapshot()).unwrap();
    assert_eq!(value["facing"], Value::Null);
    assert_eq!(value["kind"], "same_snapshot");
    assert_eq!(value["room"], "busy");
    let decoded: Snapshot = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, snapshot());
    for field in [
        "kind",
        "room",
        "own_turn",
        "going_on",
        "named_sure",
        "text",
        "facing",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Snapshot>(missing).is_err(),
            "missing {field}"
        );
    }
    for facing in [Some(false), Some(true)] {
        let row = Snapshot {
            kind: SnapshotKind::VoiceFirstOutput,
            facing,
            ..snapshot()
        };
        let decoded =
            serde_json::from_str::<Snapshot>(&serde_json::to_string(&row).unwrap()).unwrap();
        assert_eq!(decoded, row);
    }
}

#[test]
fn unknown_duplicate_mistyped_and_invalid_unicode_fields_are_rejected() {
    let good = serde_json::to_value(snapshot()).unwrap();
    for (field, wrong) in [
        ("kind", json!("live")),
        ("room", json!("calm")),
        ("own_turn", json!(0)),
        ("going_on", Value::Null),
        ("named_sure", Value::Null),
        ("text", json!([])),
        ("facing", json!("unknown")),
        ("name_heard", json!(0.99)),
    ] {
        let mut value = good.clone();
        value[field] = wrong;
        assert!(
            serde_json::from_value::<Snapshot>(value).is_err(),
            "{field}"
        );
    }
    let text = serde_json::to_string(&snapshot()).unwrap();
    let duplicate = text.replacen('{', "{\"room\":\"busy\",", 1);
    assert!(serde_json::from_str::<Snapshot>(&duplicate).is_err());
    let text_start = text.find("\"text\":\"\"").unwrap() + "\"text\":\"".len();
    let mut invalid_utf8 = text.into_bytes();
    invalid_utf8.insert(text_start, 0xff);
    assert!(serde_json::from_slice::<Snapshot>(&invalid_utf8).is_err());
    let unpaired_surrogate = serde_json::to_string(&snapshot())
        .unwrap()
        .replace("\"text\":\"\"", "\"text\":\"\\uD800\"");
    assert!(serde_json::from_str::<Snapshot>(&unpaired_surrogate).is_err());
}

#[test]
fn text_bound_is_utf8_bytes_and_never_silently_truncates() {
    for text in ["x".repeat(MOST_TEXT_BYTES), "đ".repeat(MOST_TEXT_BYTES / 2)] {
        let row = Snapshot { text, ..snapshot() };
        row.validate().unwrap();
        let decoded =
            serde_json::from_str::<Snapshot>(&serde_json::to_string(&row).unwrap()).unwrap();
        assert_eq!(decoded, row);
    }
    for text in [
        "x".repeat(MOST_TEXT_BYTES + 1),
        "đ".repeat(MOST_TEXT_BYTES / 2 + 1),
        format!("{} Lamp", "x".repeat(MOST_TEXT_BYTES)),
    ] {
        let row = Snapshot { text, ..snapshot() };
        assert!(row.validate().is_err());
        assert!(serde_json::from_str::<Snapshot>(&serde_json::to_string(&row).unwrap()).is_err());
    }
}

#[test]
fn six_authored_rows_preserve_all_choices_and_known_label_denominators() {
    let play = Snapshot {
        room: Room::Quiet,
        ..snapshot()
    };
    let hold = snapshot();
    let mut counts = Counts::default();
    // Authored labels exercise metric arithmetic; they are not field data.
    for (row, choice, label) in [
        (&play, Choice::Answer, true),
        (&play, Choice::LookUp, false),
        (&play, Choice::StayQuiet, true),
        (&hold, Choice::Answer, false),
        (&hold, Choice::LookUp, true),
        (&hold, Choice::StayQuiet, false),
    ] {
        counts.note(Some(row), choice, Some(label));
    }
    let track = &counts.same_snapshot;
    assert_eq!(counts.rows, 6);
    assert_eq!(track.compared, 6);
    assert_eq!(
        serde_json::to_value(&track.table).unwrap(),
        json!({
            "play": { "answer": 1, "look_up": 1, "stay_quiet": 1 },
            "hold": { "answer": 1, "look_up": 1, "stay_quiet": 1 }
        })
    );
    assert_eq!(track.permission_disagreements, 3);
    assert_eq!(track.labels.for_lamp, 3);
    assert_eq!(track.labels.not_for_lamp, 3);
    assert_eq!(track.labels.unknown, 0);
    assert_eq!(track.labels.policy.false_answers, 1);
    assert_eq!(track.labels.policy.missed_calls, 2);
    assert_eq!(track.labels.baseline.false_answers, 1);
    assert_eq!(track.labels.baseline.missed_calls, 1);
    assert_eq!(counts.voice_first_output.compared, 0);
}

#[test]
fn first_output_and_same_snapshot_comparisons_have_independent_totals() {
    let mut counts = Counts::default();
    counts.note(Some(&snapshot()), Choice::LookUp, Some(false));
    let first_output = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        ..snapshot()
    };
    counts.note(Some(&first_output), Choice::Answer, Some(true));
    assert_eq!(counts.rows, 2);
    assert_eq!(counts.same_snapshot.compared, 1);
    assert_eq!(counts.same_snapshot.permission_disagreements, 0);
    assert_eq!(counts.same_snapshot.labels.not_for_lamp, 1);
    assert_eq!(counts.same_snapshot.labels.baseline.missed_calls, 0);
    assert_eq!(counts.voice_first_output.compared, 1);
    assert_eq!(counts.voice_first_output.permission_disagreements, 1);
    assert_eq!(counts.voice_first_output.labels.for_lamp, 1);
    assert_eq!(counts.voice_first_output.labels.baseline.missed_calls, 1);
}

#[test]
fn unknown_labels_compare_permissions_without_creating_errors() {
    let mut counts = Counts::default();
    let play = Snapshot {
        own_turn: true,
        ..snapshot()
    };
    counts.note(Some(&play), Choice::StayQuiet, None);
    counts.note(Some(&snapshot()), Choice::Answer, None);
    let track = &counts.same_snapshot;
    assert_eq!(track.compared, 2);
    assert_eq!(track.permission_disagreements, 2);
    assert_eq!(track.labels.unknown, 2);
    assert_eq!(track.labels.for_lamp, 0);
    assert_eq!(track.labels.not_for_lamp, 0);
    assert_eq!(track.labels.policy.false_answers, 0);
    assert_eq!(track.labels.policy.missed_calls, 0);
    assert_eq!(track.labels.baseline.false_answers, 0);
    assert_eq!(track.labels.baseline.missed_calls, 0);
}

#[test]
fn absent_or_invalid_snapshot_is_excluded_even_with_a_label() {
    let mut counts = Counts::default();
    counts.note(None, Choice::Answer, Some(false));
    let invalid = Snapshot {
        kind: SnapshotKind::VoiceFirstOutput,
        text: "x".repeat(MOST_TEXT_BYTES + 1),
        ..snapshot()
    };
    counts.note(Some(&invalid), Choice::StayQuiet, Some(true));
    assert_eq!(counts.rows, 2);
    assert_eq!(counts.missing_snapshot, 1);
    assert_eq!(counts.invalid_snapshot, 1);
    assert_eq!(counts.same_snapshot, Default::default());
    assert_eq!(counts.voice_first_output, Default::default());
}

#[test]
fn counts_saturate_instead_of_wrapping_or_panicking() {
    let mut counts = Counts {
        rows: u64::MAX,
        missing_snapshot: u64::MAX,
        invalid_snapshot: u64::MAX,
        ..Counts::default()
    };
    counts.note(None, Choice::Answer, None);
    let invalid = Snapshot {
        text: "x".repeat(MOST_TEXT_BYTES + 1),
        ..snapshot()
    };
    counts.note(Some(&invalid), Choice::Answer, None);
    let track = &mut counts.same_snapshot;
    track.compared = u64::MAX;
    track.table.hold.answer = u64::MAX;
    track.permission_disagreements = u64::MAX;
    track.labels.not_for_lamp = u64::MAX;
    track.labels.policy.false_answers = u64::MAX;
    counts.note(Some(&snapshot()), Choice::Answer, Some(false));
    assert_eq!(counts.rows, u64::MAX);
    assert_eq!(counts.missing_snapshot, u64::MAX);
    assert_eq!(counts.invalid_snapshot, u64::MAX);
    assert_eq!(counts.same_snapshot.compared, u64::MAX);
    assert_eq!(counts.same_snapshot.table.hold.answer, u64::MAX);
    assert_eq!(counts.same_snapshot.permission_disagreements, u64::MAX);
    assert_eq!(counts.same_snapshot.labels.not_for_lamp, u64::MAX);
    assert_eq!(counts.same_snapshot.labels.policy.false_answers, u64::MAX);
}
