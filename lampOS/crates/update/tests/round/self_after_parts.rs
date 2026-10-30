//! A build that changes the parts and lamp-update too: the parts switch
//! and pass their watch first, and only then does lamp-update switch
//! itself, which is the round's one end (lamp-9adf's 8999 → 9063, 30 Oct:
//! no `switched … every part is well`). A build whose parts are put back
//! never switches the updater.
use super::*;

/// Build `sequence` on every part, with lamp-update's `updater` and a
/// lamp-body that crashes whenever it is started.
fn product_build_with_crashing_body(lamp: &mut Lamp, sequence: u64, updater: &str) {
    let texts: Vec<(String, String)> = lamp_update::install::parts()
        .into_iter()
        .map(|part| part.binary)
        .chain(["lamp-turn-end".to_owned()])
        .map(|name| {
            let text = match name.as_str() {
                "lamp-update" => updater.to_owned(),
                "lamp-skills" => skills_with_checks(&sequence.to_string(), ""),
                "lamp-body" => format!("lamp-body {sequence}, which crashes"),
                _ => format!("{name} {sequence}"),
            };
            (name, text)
        })
        .collect();
    let binaries: Vec<(&str, &str)> = texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    lamp.publish(sequence, &binaries, &[]);
}

#[test]
fn a_build_changing_the_parts_and_the_updater_ends_with_the_self_switch_after_the_parts() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let outcome = lamp.round();
    assert_eq!(
        outcome,
        Outcome::SelfSwitched {
            sequence: 2,
            from: old,
            to: sha("lamp-update 2")
        },
        "{:?}",
        lamp.said
    );
    // The parts run build 2: switched, watched and kept, before the
    // updater's own switch.
    assert_eq!(
        lamp.read("/opt/lampOS/body/current/lamp-body"),
        "lamp-body 2"
    );
    let at = |line: &str| lamp.said.iter().position(|said| said.starts_with(line));
    let switching = at("switching to build 2").expect("switching");
    let own = at("lamp-update switches to its build").expect("its own switch");
    assert!(switching < own, "{:?}", lamp.said);
    // One end only, the self-switch's: never the parts' own.
    assert!(
        !lamp
            .said
            .iter()
            .any(|said| said.starts_with("switched to build")),
        "{:?}",
        lamp.said
    );
    assert_eq!(
        outcome.to_string(),
        format!(
            "build 2 runs; lamp-update switches from its build {} to {}, which must confirm itself",
            &sha("lamp-update 1")[..12],
            &sha("lamp-update 2")[..12]
        )
    );
}

#[test]
fn a_build_whose_parts_are_put_back_never_switches_the_updater() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_crashing_body(&mut lamp, 2, "lamp-update 2");
    let outcome = lamp.round();
    assert!(
        matches!(
            outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}: {:?}",
        lamp.said
    );
    assert_eq!(
        lamp.read("/opt/lampOS/body/current/lamp-body"),
        "lamp-body 1"
    );
    assert_eq!(lamp.own_build(), old);
    assert_eq!(
        lamp.read("/opt/lampOS/update/current/lamp-update"),
        "lamp-update 1"
    );
    assert!(lamp.pending().is_none());
    assert!(
        !lamp
            .said
            .iter()
            .any(|said| said.starts_with("lamp-update switches")),
        "{:?}",
        lamp.said
    );
    // Refused from then on: the next round takes neither its parts nor its
    // updater.
    let next = lamp.round();
    assert!(
        matches!(&next, Outcome::Refused(why) if why.contains("was put back")),
        "{next:?}: {:?}",
        lamp.said
    );
    assert_eq!(
        lamp.read("/opt/lampOS/body/current/lamp-body"),
        "lamp-body 1"
    );
    assert_eq!(lamp.own_build(), old);
}
