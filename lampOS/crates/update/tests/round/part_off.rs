//! A part whose switch is off ends at once after its restart, exit 0,
//! saying `{"enabled":false,…}`: off, not stopped, so its build stands (El's
//! P3, the first builder's word on 8389's put-back on lamp-06c1, 29 Oct).
use super::*;

const BODY: &str = "lamp-body.service";

/// A lamp whose body, running before the round, ends at once on the new
/// build with `line` as its last.
fn body_ends_saying(line: &str) -> Lamp {
    let mut lamp = Lamp::new();
    lamp.systemd.condition_skipped.push(BODY.into());
    lamp.systemd.lines.insert(BODY.into(), line.into());
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    lamp
}

#[test]
fn a_part_that_ends_at_once_saying_it_is_off_is_off_and_its_build_stands() {
    let mut lamp = body_ends_saying(
        r#"{"enabled":false,"message":"Body is off; supply --enable to start it."}"#,
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert!(
        lamp.said.iter().any(|line| line.starts_with(&format!(
            "{BODY} is off by its own switch since its restart"
        ))),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_part_that_ends_at_once_saying_anything_else_stopped_and_is_put_back() {
    for line in [
        "body: the store is newer than this build",
        r#"{"enabled":true,"message":"Body is on"}"#,
        r#"{"enabled":"false"}"#,
    ] {
        let mut lamp = body_ends_saying(line);
        let outcome = lamp.round();
        assert!(
            matches!(outcome, Outcome::RolledBack { .. }),
            "{line}: {outcome:?}"
        );
        assert!(
            outcome.to_string().contains(&format!("{BODY} stopped")),
            "{line}: {outcome}"
        );
    }
}
