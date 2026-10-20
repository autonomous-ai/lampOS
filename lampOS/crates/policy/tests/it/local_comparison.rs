//! Authored cue snapshots illustrating permission differences. These labels
//! are fixture intentions; they cannot measure accuracy on office speech.
use lamp_policy::{
    attention::{self, Answers, Choice},
    decide::Decider,
    policy::{
        baseline::{self, Counts, Room, Snapshot, SnapshotKind},
        judges,
        local::{Cues, Local, RoomCue, Weights},
    },
};
use serde_json::json;
use std::time::Duration;

#[test]
fn authored_snapshots_show_where_local_only_choices_differ_from_the_busy_floor() {
    let mut local = Local::new(Weights::seed());
    let mut counts = Counts::default();
    let mut records = Vec::new();
    let cases = [
        (
            "quiet_short_not_called",
            "okay",
            Room::Quiet,
            Some(false),
            Some(false),
            Some(0.0),
            false,
        ),
        (
            "meeting_name_in_talk",
            "the plan follows tomorrow",
            Room::Busy,
            Some(false),
            Some(false),
            Some(0.993),
            false,
        ),
        (
            "busy_explicit_call_no_pose",
            "Lamp, what time is it",
            Room::Busy,
            Some(false),
            None,
            None,
            true,
        ),
        (
            "busy_explicit_call_facing",
            "Lamp, what time is it",
            Room::Busy,
            Some(false),
            Some(true),
            None,
            true,
        ),
        (
            "busy_followup",
            "and tomorrow",
            Room::Busy,
            Some(true),
            None,
            None,
            true,
        ),
        (
            "unknown_room_scrap",
            "uh huh",
            Room::Unknown,
            None,
            None,
            None,
            false,
        ),
        (
            "busy_colleague_no_call",
            "send the invoices tomorrow",
            Room::Busy,
            Some(false),
            Some(false),
            Some(0.0),
            false,
        ),
        (
            "busy_sure_name_no_pose",
            "what time is it",
            Room::Busy,
            Some(false),
            None,
            Some(0.999),
            true,
        ),
        (
            "quiet_name_heard",
            "what time is it",
            Room::Quiet,
            Some(false),
            None,
            Some(0.99),
            true,
        ),
    ];
    for (id, text, room, conversation, facing, name_score, directed) in cases {
        let snapshot = Snapshot {
            kind: SnapshotKind::SameSnapshot,
            room,
            own_turn: false,
            going_on: conversation.unwrap_or(false),
            named_sure: baseline::sure_name(name_score),
            text: text.into(),
            facing,
        };
        let cues = Cues {
            name_score,
            conversation,
            room: match room {
                Room::Quiet => Some(RoomCue::Quiet),
                Room::Busy => Some(RoomCue::Busy),
                Room::Unknown => None,
            },
            facing,
            face_age_ms: facing.map(|_| 0),
            word_count: Some(text.split_whitespace().count() as u32),
            text_called: Some(baseline::called(text)),
            ..Cues::default()
        };
        let asked = local
            .ask(
                &serde_json::to_string(&cues).unwrap(),
                &attention::questions(),
                Duration::from_secs(1),
            )
            .unwrap();
        let raw = local.weights().judge(&Answers::of(&asked.answers).unwrap());
        let result = judges::combine(local.weights(), &cues, Some(raw), None);
        let permission = baseline::judge(&snapshot);
        counts.note(Some(&snapshot), result.judgement.choice, Some(directed));
        if [
            "quiet_short_not_called",
            "meeting_name_in_talk",
            "unknown_room_scrap",
            "busy_colleague_no_call",
        ]
        .contains(&id)
        {
            assert_eq!(result.judgement.choice, Choice::LookUp);
        }
        records.push(json!({"id":id,"source":"authored numerical cue fixture; not recorded speech",
            "rights":"Apache-2.0 project fixture","directed":directed,"cues":cues,"baseline":snapshot,
            "baseline_permission":permission,"local_probability":raw.probability,"choice":result.judgement.choice,
            "guard":result.guard,"permission_differs":(permission==baseline::Permission::Play)!=(result.judgement.choice==Choice::Answer)}));
    }
    println!(
        "LOCAL_COMPARISON {}",
        json!({"schema":1,"weights":local.weights().name(),"baseline_source":baseline::SOURCE,
        "scope":"local-only final shadow choices with required no-call guard; synthetic records only; no Jev service",
        "records":records,"counts":counts})
    );
    assert_eq!(counts.rows, 9);
    assert_eq!(counts.same_snapshot.compared, 9);
    assert_eq!(counts.same_snapshot.permission_disagreements, 6);
    assert_eq!(counts.same_snapshot.labels.baseline.false_answers, 2);
    assert_eq!(counts.same_snapshot.labels.baseline.missed_calls, 0);
    assert_eq!(counts.same_snapshot.labels.policy.false_answers, 0);
    assert_eq!(counts.same_snapshot.labels.policy.missed_calls, 4);
    assert_eq!(counts.voice_first_output.compared, 0);
}
