//! A session renewed when the executive's catalog changed (#1472), through
//! the worker's own loop against the scripted service (the reviewer on
//! #1484): a session given no light functions, then the catalog rewritten
//! with them, is renewed between turns, once, opened fresh; never while a
//! conversation is going on or a turn is open.
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
    folder.write(&SOUND, 0);
    let tools = crate::tools::Settings {
        exec: Some(folder.0.clone()),
        ..Default::default()
    };
    let given = Arc::new(Given::checked(LOOK, going_on));
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
/// on, however long the catalog has differed.
#[tokio::test]
async fn no_renewal_while_a_conversation_is_going_on() {
    let folder = Folder::new("talking");
    let (mut rig, given) = rig(&folder, talking);
    let _dial = rig.ready().await;
    folder.write(&BOTH, 1);
    rig.pump(LOOK * 10).await;
    assert!(!given.take_fresh(), "renewed in a conversation");
    rig.stop().await;
}

/// Nor while a person's turn is open: the person switch's own condition.
#[tokio::test]
async fn no_renewal_while_a_turn_is_open() {
    let folder = Folder::new("open");
    let (mut rig, given) = rig(&folder, quiet);
    let mut dial = rig.ready().await;
    rig.speak(&mut dial, false, false).await;
    folder.write(&BOTH, 1);
    rig.pump(LOOK * 10).await;
    assert!(!given.take_fresh(), "renewed with a turn open");
    rig.stop().await;
}
