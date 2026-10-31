//! A session renewed when the executive's catalog changed (#1472), through
//! the worker's own loop against the scripted service (the reviewer on
//! #1484): a session given no light functions, then the catalog rewritten
//! with them, is renewed between turns, once, opened fresh; never while a
//! turn is open, nor while a conversation is going on unless it brings back
//! what the session was told it cannot do (Jarvis on #1513).
//!
//! Each test holds the rig's scenario clock (#1128) on a runtime that is not
//! paused, so the worker's heartbeat bound (250 ms with nothing queued) and
//! the rig's leases count the rig's ticks, not the host's time: a loaded
//! runner that stalls the test between two heartbeats no longer ends the
//! worker (ARM64 on 31 Oct: `provider.c.p` refused, the worker gone). The
//! catalog's looks and bounds, and Tokio's own timers, stay on the host's.
use super::*;
use crate::session_tools::Given;
use std::sync::Arc;

/// Looked at often, so a test waits tens of milliseconds, not seconds.
const LOOK: Duration = Duration::from_millis(20);

struct Folder(std::path::PathBuf);
impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("lv-pw-cat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    /// The executive's catalog, at a time of its own so a rewrite inside the
    /// same second is still seen.
    fn write(&self, names: &[&str], seconds: u64) {
        let path = self.0.join(crate::tools::CATALOG_FILE);
        let tools: Vec<_> = names
            .iter()
            .map(|name| json!({"name": name, "description": format!("Do {name}.")}))
            .collect();
        std::fs::write(&path, json!({"tools": tools}).to_string()).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(
                std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds),
            )
            .unwrap();
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SOUND: [&str; 2] = ["volume_up", "volume_down"];
const BOTH: [&str; 4] = ["volume_up", "volume_down", "light_on", "light_colour"];

/// A worker whose sessions were given the sound's functions alone, as at
/// 11:22:06, with the conversation read from `going_on`.
fn rig(folder: &Folder, going_on: fn() -> bool) -> (LiveRig, Arc<Given>) {
    rig_looking(folder, going_on, LOOK)
}

/// The same, its catalogs looked at every `look`.
fn rig_looking(folder: &Folder, going_on: fn() -> bool, look: Duration) -> (LiveRig, Arc<Given>) {
    folder.write(&SOUND, 0);
    let tools = crate::tools::Settings {
        exec: Some(folder.0.clone()),
        ..Default::default()
    };
    let given = Arc::new(Given::checked(look, going_on));
    // The rig's sessions are set up from its own configuration, not a
    // `Recalling`: what they were given is kept here as `equipped` keeps it.
    given.record(&crate::tools::declared(&tools));
    let options = Options {
        catalog: Some(given.clone()),
        ..typed_with(tools)
    };
    // As the product's `serve` keeps its link (`serving_recovery_policy`):
    // a session renewed with no handle to resume is replaced, not the end.
    let policy = RecoveryPolicy {
        require_context: false,
        ..quick_policy()
    };
    (LiveRig::start_with(policy, options), given)
}

fn talking() -> bool {
    true
}

fn quiet() -> bool {
    false
}

/// The issue's case: the light's functions come back after the session was
/// given none. Between turns, the next look asks for a renewal, the session
/// after it is to be opened fresh, and the worker dials it.
#[tokio::test]
async fn a_catalog_that_gained_the_light_renews_the_session_between_turns_once() {
    let _clock = logical_clock();
    let folder = Folder::new("gained");
    let (mut rig, given) = rig(&folder, quiet);
    let _dial = rig.ready().await;
    rig.pump(LOOK * 5).await;
    assert!(!given.take_fresh(), "renewed with nothing changed");
    folder.write(&BOTH, 1);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !given.take_fresh() {
        assert!(
            Instant::now() < deadline,
            "no renewal for the changed catalog"
        );
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
    }
    // The session ends at its next idle moment and a new one is dialled,
    // set up with what the catalog holds now, as `Recalling` records it.
    given.record(&crate::tools::declared(&crate::tools::Settings {
        exec: Some(folder.0.clone()),
        ..Default::default()
    }));
    // The old session is reported gone for the renewal (the service's own
    // close), then the new one ready.
    let mut next = rig.dial().await;
    assert!(rig.service_message(&mut next).await.get("setup").is_some());
    next.service.send(json!({"setupComplete": {}})).await;
    // The old session is said gone at the service's close, then replaced,
    // fresh, with no handle presented.
    let mut statuses = Vec::new();
    while !statuses
        .iter()
        .any(|status: &String| status.starts_with("Recovered"))
    {
        match rig.wire().await {
            Wire::Status(status) => statuses.push(format!("{status:?}")),
            Wire::Output(other) => panic!("unexpected output: {other:?}"),
        }
    }
    assert_eq!(statuses.len(), 2, "{statuses:?}");
    assert_eq!(statuses[0], "Unavailable { reason: ServerGoAway }");
    assert!(
        statuses[1].contains("context: Fresh") && statuses[1].contains("reason: ServerGoAway"),
        "{statuses:?}"
    );
    // Nothing more to change: not asked again, however many looks follow.
    rig.pump(LOOK * 10).await;
    assert!(!given.take_fresh(), "asked again with nothing changed");
    rig.stop().await;
}

/// The first builder's (b): nothing is renewed while a conversation is going
/// on, however long the catalog has differed, for a change that brings
/// back nothing the session was told it cannot do: another function, or one
/// of a kind it has taken away.
#[tokio::test]
async fn no_renewal_while_a_conversation_is_going_on() {
    let _clock = logical_clock();
    for (name, now) in [
        (
            "talking-other",
            &["volume_up", "volume_down", "weather_now"][..],
        ),
        ("talking-fewer", &["volume_up"][..]),
    ] {
        let folder = Folder::new(name);
        let (mut rig, given) = rig(&folder, talking);
        let _dial = rig.ready().await;
        folder.write(now, 1);
        rig.pump(LOOK * 10).await;
        assert!(!given.take_fresh(), "renewed in a conversation: {now:?}");
        rig.stop().await;
    }
}

/// Jarvis's P2 on #1513: the light back for a session told it cannot change
/// it renews between turns while a conversation is going on, the lamp's own
/// "I can't right now" among it, so it does not keep refusing a light that
/// works.
#[tokio::test]
async fn the_light_back_renews_while_a_conversation_is_going_on() {
    let _clock = logical_clock();
    let folder = Folder::new("talking-light");
    let (mut rig, given) = rig(&folder, talking);
    let _dial = rig.ready().await;
    rig.pump(LOOK * 5).await;
    assert!(!given.take_fresh(), "renewed with nothing changed");
    folder.write(&BOTH, 1);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !given.take_fresh() {
        assert!(
            Instant::now() < deadline,
            "the light back, no renewal in a conversation"
        );
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
}

/// Jarvis on #1588: lamp-actions restarting in a quiet room renews the
/// session for the light gone, and the light back 4 s later renews it again
/// at the next gap, not after the 30 s bound with the lamp refusing.
#[tokio::test]
async fn the_light_back_soon_after_a_renewal_for_it_gone_renews_at_once() {
    let _clock = logical_clock();
    let folder = Folder::new("back-soon");
    let (mut rig, given) = rig(&folder, quiet);
    let settings = crate::tools::Settings {
        exec: Some(folder.0.clone()),
        ..Default::default()
    };
    // The session has the light, as one opened with the ring up.
    folder.write(&BOTH, 1);
    given.record(&crate::tools::declared(&settings));
    let _dial = rig.ready().await;
    rig.pump(LOOK * 5).await;
    assert!(!given.take_fresh(), "renewed with nothing changed");
    // lamp-actions restarts: the light goes, and the session is renewed.
    folder.write(&SOUND, 2);
    let renewed = |given: &Given| given.take_fresh();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !renewed(&given) {
        assert!(Instant::now() < deadline, "no renewal for the light gone");
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
    }
    given.record(&crate::tools::declared(&settings));
    let mut next = rig.dial().await;
    assert!(rig.service_message(&mut next).await.get("setup").is_some());
    next.service.send(json!({"setupComplete": {}})).await;
    let mut statuses = Vec::new();
    while !statuses
        .iter()
        .any(|status: &String| status.starts_with("Recovered"))
    {
        match rig.wire().await {
            Wire::Status(status) => statuses.push(format!("{status:?}")),
            Wire::Output(other) => panic!("unexpected output: {other:?}"),
        }
    }
    // The ring back seconds later: renewed again, well inside 30 s.
    folder.write(&BOTH, 3);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !renewed(&given) {
        assert!(
            Instant::now() < deadline,
            "the light back waited out the bound"
        );
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
}

/// The first builder's condition 3 on #1588: during a conversation, a
/// light that comes and goes (#1514) renews the session once for its
/// return, not at each: gone and back three times, with the session opened
/// in a gap each time so every return is one it was told it can't use,
/// gives one renewal. The run takes under a second, so the 30 s bound alone
/// holds the later returns here; the per-kind minute, which outlasts it, is
/// shown by `a_change_is_ready_again_only_when_it_brings_back_what_the_session_was_told_it_cannot_do`
/// (the reviewer on #1588).
#[tokio::test]
async fn a_light_gone_and_back_three_times_in_a_conversation_renews_once() {
    let _clock = logical_clock();
    let folder = Folder::new("flap");
    let (mut rig, given) = rig(&folder, talking);
    let settings = crate::tools::Settings {
        exec: Some(folder.0.clone()),
        ..Default::default()
    };
    let _dial = rig.ready().await;
    rig.pump(LOOK * 5).await;
    assert!(!given.take_fresh(), "renewed with nothing changed");
    let mut renewals = 0;
    let mut seconds = 0;
    // The sessions dialled, kept open for the test's life.
    let mut dialled = Vec::new();
    for round in 0..3 {
        // Back: renewed the first time only.
        seconds += 1;
        folder.write(&BOTH, seconds);
        let until = Instant::now() + LOOK * 10;
        while Instant::now() < until {
            rig.publish();
            rig.tick(Duration::from_millis(2)).await;
            if given.take_fresh() {
                renewals += 1;
                // Gone again as the next session opens: it is given none
                // of the light, and the next return is one it can't use.
                seconds += 1;
                folder.write(&SOUND, seconds);
                given.record(&crate::tools::declared(&settings));
                let mut next = rig.dial().await;
                assert!(rig.service_message(&mut next).await.get("setup").is_some());
                next.service.send(json!({"setupComplete": {}})).await;
                let mut statuses = Vec::new();
                while !statuses
                    .iter()
                    .any(|status: &String| status.starts_with("Recovered"))
                {
                    match rig.wire().await {
                        Wire::Status(status) => statuses.push(format!("{status:?}")),
                        Wire::Output(other) => panic!("unexpected output: {other:?}"),
                    }
                }
                dialled.push(next);
                break;
            }
        }
        assert_eq!(
            renewals, 1,
            "round {round}: one renewal for the light's return"
        );
        // Gone: no renewal in a conversation for that either.
        seconds += 1;
        folder.write(&SOUND, seconds);
        rig.pump(LOOK * 5).await;
        assert!(
            !given.take_fresh(),
            "round {round}: renewed for the light gone"
        );
    }
    rig.stop().await;
}

/// Jarvis on #1588: in a conversation, turns owned back to back with a gap
/// far shorter than a look (a 60 ms gap, looks every 500 ms) still renew at
/// the first gap once the light is back, not at whichever gap a look
/// happens to land in.
#[tokio::test]
async fn the_light_back_renews_at_the_first_short_gap_between_owned_turns() {
    use lamp_turn_owner::AdmittedInput;
    let _clock = logical_clock();
    let folder = Folder::new("short-gap");
    let (mut rig, given) = rig_looking(&folder, talking, Duration::from_millis(500));
    let _dial = rig.ready().await;
    rig.pump(LOOK * 5).await;
    assert!(!given.take_fresh(), "renewed with nothing changed");
    // A turn owned, and the light back during it: seen at a look, not due.
    let owner = rig
        .owner
        .admit(real_time(), AdmittedInput::NewTurn)
        .unwrap();
    rig.publish();
    folder.write(&BOTH, 1);
    rig.pump(Duration::from_millis(1_200)).await;
    assert!(!given.take_fresh(), "renewed with a turn owned");
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.owner.complete_turn(real_time(), owner).unwrap();
    // The gap, then the next turn.
    let gap = tokio::time::Instant::now() + Duration::from_millis(60);
    let mut renewed = false;
    while tokio::time::Instant::now() < gap && !renewed {
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
        renewed = given.take_fresh();
    }
    assert!(
        renewed,
        "the first gap after the light came back was not taken"
    );
    rig.stop().await;
}

/// Nor while a person's turn is open: the person switch's own condition.
#[tokio::test]
async fn no_renewal_while_a_turn_is_open() {
    let _clock = logical_clock();
    let folder = Folder::new("open");
    let (mut rig, given) = rig(&folder, quiet);
    let mut dial = rig.ready().await;
    rig.speak(&mut dial, false, false).await;
    folder.write(&BOTH, 1);
    rig.pump(LOOK * 10).await;
    assert!(!given.take_fresh(), "renewed with a turn open");
    rig.stop().await;
}
