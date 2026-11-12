//! What a round said is kept in `log.jsonl` for a round that changed something or
//! failed (#6266, 5b): the journal is RAM on the product lamp and holds about the
//! last 40 minutes, and nothing of it is there after a reboot.
use super::*;

fn publish(lamp: &mut Lamp, sequence: u64, voice: &str) {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", voice),
            ("lamp-turn-end", &format!("turn-end {sequence}")),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
}

fn log(lamp: &Lamp) -> Vec<serde_json::Value> {
    lamp.read("/var/lib/lampos-update/log.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn kept_lines(lamp: &Lamp) -> Vec<(String, Vec<String>)> {
    log(lamp)
        .into_iter()
        .filter_map(|entry| {
            let lines = entry["lines"].as_array()?;
            Some((
                entry["event"].as_str()?.to_owned(),
                lines
                    .iter()
                    .map(|l| l.as_str().unwrap().to_owned())
                    .collect(),
            ))
        })
        .collect()
}

/// A switch keeps its lines, before its end; a round after it that has nothing to
/// do, and one that is only refused, keep none.
#[test]
fn a_round_that_switched_keeps_what_it_said_and_a_quiet_one_does_not() {
    let mut lamp = Lamp::new();
    publish(&mut lamp, 2, "voice 2");
    assert_eq!(lamp.round(), Outcome::Switched(2));
    let kept = kept_lines(&lamp);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].0, "switched");
    assert!(!kept[0].1.is_empty());
    // What was kept was said: each line is one the round said.
    for line in &kept[0].1 {
        assert!(lamp.said.iter().any(|said| said == line), "{line}");
    }
    // The lines come before the round's end, which `last_round` still finds.
    let entries = log(&lamp);
    let at = |pred: &dyn Fn(&serde_json::Value) -> bool| entries.iter().position(pred).unwrap();
    assert!(at(&|e| e.get("lines").is_some()) < at(&|e| e["round"] == true));
    assert_eq!(
        lamp_update::records::Records::new(lamp.records())
            .last_round()
            .map(|round| round.0),
        Some("switched".to_owned())
    );
    // Nothing to do: only an end.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(kept_lines(&lamp).len(), 1);
}

/// A failed switch, put back, keeps what it said, the put-back's lines among them;
/// the same build refused again every round keeps only its end.
#[test]
fn a_round_that_put_a_build_back_keeps_what_it_said_and_a_refusal_said_again_does_not() {
    let mut lamp = Lamp::new();
    publish(&mut lamp, 2, "voice 2, which crashes");
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    let kept = kept_lines(&lamp);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].0, "rolled-back");
    assert!(
        kept[0]
            .1
            .iter()
            .any(|line| line.contains("putting build 2 back")),
        "{:?}",
        kept[0].1
    );
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    assert_eq!(
        kept_lines(&lamp).len(),
        1,
        "a refusal said again keeps no lines"
    );
}
