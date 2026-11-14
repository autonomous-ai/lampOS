//! A round holds before it fetches while the setup walk is under way, at
//! most eight rounds on a lamp (#3689, crate::walk): the ten 7071 lamps
//! update within the hour of their setup, during their first walk.
use super::*;
use lamp_update::walk;

/// A 7071 walk two steps in, as its voice keeps it.
const TWO_STEPS_IN: &str = r#"{"steps":{"where_you_sit":{"outcome":"done","at_unix_s":1761450000},"who_you_are":{"outcome":"done","at_unix_s":1761450010}},"privacy_invitation_tries":0,"privacy_discard_pending":false}"#;

/// A walk not yet begun: its file there, no step kept.
const NOT_BEGUN: &str =
    r#"{"steps":{},"privacy_invitation_tries":0,"privacy_discard_pending":false}"#;

fn put(lamp: &Lamp, path: &str, text: &str) {
    let path = lamp.root.join(path.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn set_up(lamp: &Lamp) {
    put(lamp, "/var/lib/lampos-setup/done", "");
}

/// Every step with an outcome, `later` among them as a newer voice keeps it.
fn walked() -> String {
    let steps: Vec<String> = lamp_msgs::ONBOARDING_STEPS
        .iter()
        .map(|step| format!(r#""{step}":{{"outcome":"later","at_unix_s":1}}"#))
        .collect();
    format!(r#"{{"steps":{{{}}}}}"#, steps.join(","))
}

fn held(round: u32) -> Outcome {
    Outcome::NotQuiet(format!(
        "the setup walk is under way: held, round {round} of 8"
    ))
}

#[test]
fn a_walk_under_way_holds_the_round_before_it_fetches_eight_times_at_most() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    set_up(&lamp);
    put(&lamp, walk::PROGRESS, TWO_STEPS_IN);
    // A dry round is never held: a climbed updater confirms itself by one
    // (crate::own), so a walk can't keep it unconfirmed until it's put back.
    lamp.config.apply = false;
    assert!(matches!(lamp.round(), Outcome::Would(_)));
    assert!(!lamp.exists("/var/lib/lampos-update/walk-held"));
    lamp.config.apply = true;
    for round in 1..=walk::MOST_HELD {
        assert_eq!(lamp.round(), held(round));
        // Nothing fetched, written or restarted.
        assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
        assert!(!lamp.exists(&format!("/opt/lampOS/releases/{}", sha("voice 2"))));
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-voice.service"),
            voice_unit(&sha("voice 1"))
        );
    }
    // A walk left half done holds updates no longer.
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        lamp.read("/var/lib/lampos-update/walk-held").trim(),
        walk::MOST_HELD.to_string()
    );
}

#[test]
fn a_walk_ended_held_once_goes_on_and_its_count_is_kept() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    set_up(&lamp);
    put(&lamp, walk::PROGRESS, TWO_STEPS_IN);
    assert_eq!(lamp.round(), held(1));
    // The walk ends before the next round: the switch goes ahead.
    put(&lamp, walk::PROGRESS, &walked());
    assert_eq!(lamp.round(), Outcome::Switched(2));
    // A redo begun later is held for the rounds left, not eight again.
    put(&lamp, walk::PROGRESS, TWO_STEPS_IN);
    lamp.publish(
        3,
        &[
            ("lamp-voice", "voice 3"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    assert_eq!(lamp.round(), held(2));
}

#[test]
fn no_walk_is_read_without_setup_done_or_a_readable_file() {
    // Setup not done (a lamp set up before its marker, or a bench rig).
    let mut bench = Lamp::new();
    build_two(&mut bench);
    put(&bench, walk::PROGRESS, TWO_STEPS_IN);
    assert_eq!(bench.round(), Outcome::Switched(2));
    // No file on a lamp whose voice unit doesn't walk (onboarding off).
    let mut off = Lamp::new();
    build_two(&mut off);
    set_up(&off);
    assert_eq!(off.round(), Outcome::Switched(2));
    // A file that isn't the walk's: updates never stop for it.
    for text in ["{", r#"{"steps":3}"#, "[]"] {
        let mut odd = Lamp::new();
        build_two(&mut odd);
        set_up(&odd);
        put(&odd, walk::PROGRESS, text);
        assert_eq!(odd.round(), Outcome::Switched(2), "{text}");
    }
    // A count that can't be read holds nothing.
    let mut unread = Lamp::new();
    build_two(&mut unread);
    set_up(&unread);
    put(&unread, walk::PROGRESS, TWO_STEPS_IN);
    put(&unread, "/var/lib/lampos-update/walk-held", "many");
    assert_eq!(unread.round(), Outcome::Switched(2));
}

/// Tests' P2 on #4455: a round that began before the walk's file (its first
/// step, or before setup's `done`) is held at the switch's wait, with the
/// build installed beside and nothing switched; the next round after the
/// walk switches to it.
#[test]
fn a_walk_begun_while_the_round_fetched_holds_it_at_the_switch() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    set_up(&lamp);
    let progress = lamp.root.join(walk::PROGRESS.trim_start_matches('/'));
    lamp.channel.on_fetch = Some((progress, TWO_STEPS_IN.to_owned()));
    assert_eq!(lamp.round(), held(1));
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    assert!(lamp.exists(&format!(
        "/opt/lampOS/releases/{}/lamp-voice",
        sha("voice 2")
    )));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
    put(&lamp, walk::PROGRESS, &walked());
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// The installed voice unit walks, as the product's does, perhaps keeping
/// its walk at `file`.
fn walks(lamp: &Lamp, file: Option<&str>) {
    let path = lamp.root.join("etc/systemd/system/lamp-voice.service");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("Environment=LAMP_ONBOARDING=on\n");
    if let Some(file) = file {
        text.push_str(&format!("Environment=LAMP_ONBOARDING_FILE={file}\n"));
    }
    fs::write(path, text).unwrap();
}

/// Tests' and Third's P2 on #4455: the walk's first step has no file yet
/// (7071 writes it at that step's end), so a voice unit that walks with no
/// file is a walk under way; and the file is the one its unit names.
#[test]
fn a_walk_not_written_yet_holds_when_the_voice_walks() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    set_up(&lamp);
    walks(&lamp, None);
    assert_eq!(lamp.round(), held(1));
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    // The first step ends, and later the walk: the switch goes ahead.
    put(&lamp, walk::PROGRESS, TWO_STEPS_IN);
    assert_eq!(lamp.round(), held(2));
    put(&lamp, walk::PROGRESS, &walked());
    assert_eq!(lamp.round(), Outcome::Switched(2));

    // A unit that keeps its walk elsewhere is read there, not at PROGRESS.
    let mut elsewhere = Lamp::new();
    build_two(&mut elsewhere);
    set_up(&elsewhere);
    walks(
        &elsewhere,
        Some("/var/lib/lampOS/voice/other/progress.json"),
    );
    put(&elsewhere, walk::PROGRESS, &walked());
    put(
        &elsewhere,
        "/var/lib/lampOS/voice/other/progress.json",
        TWO_STEPS_IN,
    );
    assert_eq!(elsewhere.round(), held(1));
}

/// When the walk ended, for the Hermes install's window (#5978): its
/// file's `done_at_unix_ms`, only once the walk is not under way; none
/// without the file, without the field, or for a file that isn't JSON.
#[test]
fn the_walks_end_is_its_done_line_once_it_is_not_under_way() {
    let lamp = Lamp::new();
    assert_eq!(walk::ended_at_unix_ms(&lamp.config), None);
    set_up(&lamp);
    put(&lamp, walk::PROGRESS, &walked());
    assert_eq!(walk::ended_at_unix_ms(&lamp.config), None, "not dated");
    let dated = walked().replacen('{', r#"{"done_at_unix_ms":1794310000000,"#, 1);
    put(&lamp, walk::PROGRESS, &dated);
    assert_eq!(
        walk::ended_at_unix_ms(&lamp.config),
        Some(1_794_310_000_000)
    );
    let redo = TWO_STEPS_IN.replacen('{', r#"{"done_at_unix_ms":1794310000000,"#, 1);
    put(&lamp, walk::PROGRESS, &redo);
    assert_eq!(
        walk::ended_at_unix_ms(&lamp.config),
        None,
        "a redo under way"
    );
    put(&lamp, walk::PROGRESS, "not json");
    assert_eq!(walk::ended_at_unix_ms(&lamp.config), None);
}

/// #6284: while the first update after setup is owed, with
/// `LAMP_FIRST_UPDATE=on`, and the walk has not begun (no step kept), the
/// walk holds no round; switch off, the file past its 20 minutes, or a walk
/// begun, even at the switch's wait (V2's P2 on #6299), it holds as before.
#[test]
fn the_first_update_owed_is_not_held_by_the_walk() {
    let first = |at_unix_ms: u64| format!(r#"{{"at_unix_ms":{at_unix_ms}}}"#);
    let now = lamp_update::records::unix_ms();

    let mut off = Lamp::new();
    build_two(&mut off);
    set_up(&off);
    put(&off, walk::PROGRESS, TWO_STEPS_IN);
    put(&off, lamp_msgs::FIRST_UPDATE_FILE, &first(now));
    assert_eq!(off.round(), held(1));

    let mut on = Lamp::new();
    build_two(&mut on);
    set_up(&on);
    on.config.first_update = true;
    put(&on, walk::PROGRESS, NOT_BEGUN);
    put(&on, lamp_msgs::FIRST_UPDATE_FILE, &first(now));
    assert_eq!(on.round(), Outcome::Switched(2));
    assert!(!on.exists("/var/lib/lampos-update/walk-held"));
    // The round leaves the file to `serve` (crate::first_update).
    assert!(on.exists(lamp_msgs::FIRST_UPDATE_FILE));

    // A walk begun at the switch's wait: held, the lamp never switched
    // mid-walk.
    let mut waited = Lamp::new();
    build_two(&mut waited);
    set_up(&waited);
    waited.config.first_update = true;
    put(&waited, lamp_msgs::FIRST_UPDATE_FILE, &first(now));
    let progress = waited.root.join(walk::PROGRESS.trim_start_matches('/'));
    waited.channel.on_fetch = Some((progress, TWO_STEPS_IN.to_owned()));
    assert_eq!(waited.round(), held(1));

    // Past its 10 minutes: held as before.
    let mut stale = Lamp::new();
    build_two(&mut stale);
    set_up(&stale);
    stale.config.first_update = true;
    put(&stale, walk::PROGRESS, TWO_STEPS_IN);
    put(
        &stale,
        lamp_msgs::FIRST_UPDATE_FILE,
        &first(now - lamp_msgs::FIRST_UPDATE_WITHIN_MS),
    );
    assert_eq!(stale.round(), held(1));
}

/// The first update after setup is forced (#6284, the product's call on
/// 12 Nov): it switches with someone heard all the time, where any other
/// round never finds its quiet moment.
#[test]
fn the_first_update_owed_switches_without_a_quiet_moment() {
    let first = |at_unix_ms: u64| format!(r#"{{"at_unix_ms":{at_unix_ms}}}"#);
    let now = lamp_update::records::unix_ms();
    let lamp = |on: bool| {
        let mut lamp = Lamp::new();
        build_two(&mut lamp);
        set_up(&lamp);
        lamp.config.first_update = on;
        lamp.config.quiet_wait = std::time::Duration::from_secs(120);
        lamp.talking = Some(Box::new(|_| true));
        put(&lamp, lamp_msgs::FIRST_UPDATE_FILE, &first(now));
        lamp
    };
    let mut off = lamp(false);
    let outcome = off.round();
    assert!(matches!(outcome, Outcome::NotQuiet(_)), "{outcome:?}");
    let mut on = lamp(true);
    assert_eq!(on.round(), Outcome::Switched(2), "{:?}", on.said);
}

/// V2's P2 on #6299: a first update still owed once the walk has begun (a
/// first round that ended, then the walk, then a second owed round) is
/// neither forced nor let past the walk: it is held as any round is, and
/// once let go it waits for its quiet moment.
#[test]
fn a_first_update_owed_after_the_walk_began_keeps_every_rule_of_a_round() {
    let first = format!(r#"{{"at_unix_ms":{}}}"#, lamp_update::records::unix_ms());
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    set_up(&lamp);
    lamp.config.first_update = true;
    lamp.config.quiet_wait = std::time::Duration::from_secs(120);
    lamp.talking = Some(Box::new(|_| true));
    put(&lamp, lamp_msgs::FIRST_UPDATE_FILE, &first);
    put(&lamp, walk::PROGRESS, TWO_STEPS_IN);
    assert!(lamp_update::first_update::owed(
        &lamp.config,
        lamp_update::records::unix_ms()
    ));
    assert!(walk::begun(&lamp.config));
    assert_eq!(lamp.round(), held(1), "{:?}", lamp.said);
    put(&lamp, walk::PROGRESS, &walked());
    let outcome = lamp.round();
    assert!(matches!(outcome, Outcome::NotQuiet(_)), "{outcome:?}");
}
