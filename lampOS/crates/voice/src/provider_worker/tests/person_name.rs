//! The typed name told only to the owner's line on the Live route
//! (`LAMP_PERSON_NAME_OWNER_ONLY=on`, #2602), through the worker's own loop
//! against the scripted service (the reviewer's two P2s on #3050): told
//! only once a line it went with is answered, so a line given up before its
//! End leaves it to the next; and a session that may hold it is renewed
//! fresh before a line that may not have it.
use super::*;

/// What the model is told of the typed name, as the service sees it.
const TOLD: &str = "gave their name as";
const NAME: &str = "Example Name";

/// The lamp's eyes and its memory, for one test: a bus with their faces,
/// the known file beside their socket, and a memory that answers anything.
struct Scene {
    root: std::path::PathBuf,
    known: std::path::PathBuf,
    eyes: lamp_bus::Publisher,
    faces: usize,
    face_named: Option<&'static str>,
    _one: OneThread,
    kept_thread: Option<crate::thread::Thread>,
}

impl Scene {
    fn new(tag: &str) -> Self {
        let one = one_thread();
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lpw-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let name = root.join("name");
        std::fs::write(&name, format!("{NAME}\n")).unwrap();
        crate::person::set_for_test(Some(name));
        crate::person::set_bus_for_test(Some(root.clone()));
        // The session is the owner's already, as `Recalling` opens it: the
        // person switch is not what these tests are about.
        let kept_thread = {
            let mut thread = crate::thread::THREAD.lock().unwrap();
            let kept = std::mem::take(&mut *thread);
            thread.at_lamp(Some(crate::known_faces::memory_person(NAME)));
            thread.opening(Instant::now());
            kept
        };
        Self {
            known: crate::known_faces::known_file_of(&root.join("who.sock")),
            root,
            eyes,
            faces: 0,
            face_named: None,
            _one: one,
            kept_thread: Some(kept_thread),
        }
    }

    /// The worker, with the switch `owner_only`, its eyes and memory here.
    fn rig(&self, owner_only: bool) -> LiveRig {
        crate::person::set_owner_only_for_test(Some(owner_only));
        let memory = self.root.join("memory.sock");
        let listener = std::os::unix::net::UnixListener::bind(&memory).unwrap();
        std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = std::io::BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
                let _ = stream.write_all(b"{\"ok\":true,\"entries\":[]}\n");
            }
        });
        let mut rig = LiveRig::start_with(
            RecoveryPolicy {
                require_context: false,
                ..quick_policy()
            },
            typed_with(crate::tools::Settings {
                who: Some(self.root.join("who.sock")),
                memory: Some(memory),
                clock: false,
                ..Default::default()
            }),
        );
        // Leases a second long, as the burst test's (#1569): `drain()` and the
        // waits for the service's messages up to activityEnd send no heartbeat
        // (`rig.wire()` does), and a stall past the rig's 100 ms there on a
        // loaded serial run revoked the line's owner (None at `end_line`,
        // #3180's gate; pinned by
        // `a_stall_before_a_lines_end_loses_its_owner_only_past_the_lease`).
        // These tests are about the name.
        rig.lease_us = 1_000_000;
        rig.owner
            .set_camera_permission(real_time(), Permission::Allowed)
            .unwrap();
        rig.publish();
        rig
    }

    /// The eyes' newest look: `faces` faces, the known one named.
    fn look(&mut self) {
        self.eyes
            .publish_json(
                crate::scenario_clock::monotonic_us(),
                &lamp_msgs::Faces {
                    faces: (0..self.faces)
                        .map(|_| lamp_msgs::Face {
                            id: "f".into(),
                            xmin: 0.4,
                            ymin: 0.0,
                            xmax: 0.7,
                            ymax: 0.3,
                            confidence: 0.9,
                            yaw: None,
                            facing: None,
                            speaking: None,
                        })
                        .collect(),
                },
            )
            .unwrap();
        match self.face_named {
            Some(name) => std::fs::write(&self.known, name).unwrap(),
            None => {
                let _ = std::fs::remove_file(&self.known);
            }
        }
    }

    fn see(&mut self, faces: usize, named: Option<&'static str>) {
        self.faces = faces;
        self.face_named = named;
        self.look();
    }

    /// Heartbeats for `duration`, the eyes looking again every 100 ms.
    async fn pump(&mut self, rig: &mut LiveRig, duration: Duration) {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.look();
            rig.pump(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        crate::person::set_for_test(None);
        crate::person::set_bus_for_test(None);
        crate::person::set_owner_only_for_test(None);
        if let Some(kept) = self.kept_thread.take()
            && let Ok(mut thread) = crate::thread::THREAD.lock()
        {
            *thread = kept;
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A line begun on `dial`'s session, its Start and first audio sent, and
/// its Started seen; what the service sees of it is left to the test.
async fn begin(rig: &mut LiveRig, dial: &Dial) -> u64 {
    let owner = rig
        .owner
        .admit(real_time(), AdmittedInput::NewTurn)
        .unwrap();
    let request = owner.turn();
    rig.publish();
    let privacy_generation = rig.snapshot.microphone_generation();
    rig.parent
        .data
        .send(ProviderInput::Start {
            session: dial.session.get(),
            request,
            privacy_generation,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        })
        .unwrap();
    rig.parent
        .data
        .send(ProviderInput::Audio {
            request,
            privacy_generation,
            sequence: 1,
            read_completed_at_us: monotonic_us(),
            samples: vec![3; 160],
        })
        .unwrap();
    loop {
        match rig.wire().await {
            Wire::Output(ProviderOutput::Started {
                request: started, ..
            }) if started == request => {
                return request;
            }
            Wire::Output(_) => {}
            Wire::Status(status) => panic!("unexpected status: {status:?}"),
        }
    }
}

/// End the open line and give back everything the service saw through its
/// activityEnd, all it saw since the line began included.
async fn end_line(
    rig: &mut LiveRig,
    dial: &mut Dial,
    request: u64,
    mut seen: Vec<String>,
) -> Vec<String> {
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    loop {
        let message = rig.service_message(dial).await;
        let end = message["realtimeInput"].get("activityEnd").is_some();
        seen.push(message.to_string());
        if end {
            return seen;
        }
    }
}

/// What the service has seen so far, without waiting.
async fn drain(dial: &mut Dial) -> Vec<String> {
    let mut seen = Vec::new();
    while let Some(message) = dial.service.try_receive(Duration::from_millis(5)).await {
        seen.push(message.to_string());
    }
    seen
}

/// The service answers the line, and the turn is over.
async fn answer(rig: &mut LiveRig, dial: &mut Dial) {
    dial.service.send(idle_complete()).await;
    loop {
        match rig.wire().await {
            Wire::Output(ProviderOutput::TurnComplete { .. }) => break,
            Wire::Output(_) => {}
            Wire::Status(status) => panic!("unexpected status: {status:?}"),
        }
    }
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.complete_turn(real_time(), owner).unwrap();
    rig.publish();
}

/// One whole line by whoever the scene shows: what the service saw of it.
async fn line(scene: &mut Scene, rig: &mut LiveRig, dial: &mut Dial) -> Vec<String> {
    let request = begin(rig, dial).await;
    scene.pump(rig, Duration::from_millis(700)).await;
    let seen = drain(dial).await;
    end_line(rig, dial, request, seen).await
}

/// #3198's control, pinned (reviewer-two's P3): a stall of 150 ms with no
/// heartbeat between a line's drain and its end, then one heartbeat, loses
/// the line's owner under the rig's old 100 ms lease, as #3180's full run
/// did (None at `end_line`), and keeps it under the scene's 1 s lease.
#[tokio::test]
async fn a_stall_before_a_lines_end_loses_its_owner_only_past_the_lease() {
    for (lease_us, kept) in [(100_000, false), (1_000_000, true)] {
        let mut scene = Scene::new("stall");
        let mut rig = scene.rig(true);
        rig.lease_us = lease_us;
        let mut dial = rig.ready().await;
        scene.see(1, Some(NAME));
        scene.pump(&mut rig, Duration::from_millis(300)).await;
        let request = begin(&mut rig, &dial).await;
        scene.pump(&mut rig, Duration::from_millis(300)).await;
        let seen = drain(&mut dial).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        rig.publish();
        assert_eq!(rig.snapshot.owner().is_some(), kept, "lease {lease_us} us");
        if kept {
            end_line(&mut rig, &mut dial, request, seen).await;
        }
        rig.stop().await;
    }
}

fn told(seen: &[String]) -> bool {
    seen.iter().any(|message| message.contains(TOLD))
}

/// Controls: the owner's own known face is told the name once a session;
/// with no look, or with the switch off, never.
#[tokio::test]
async fn the_owners_face_is_told_once_a_session_and_no_look_or_the_switch_off_never() {
    let mut scene = Scene::new("owner");
    let mut rig = scene.rig(true);
    let mut dial = rig.ready().await;
    scene.see(1, Some(NAME));
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    assert!(
        told(&line(&mut scene, &mut rig, &mut dial).await),
        "the owner's first line"
    );
    answer(&mut rig, &mut dial).await;
    assert!(
        !told(&line(&mut scene, &mut rig, &mut dial).await),
        "told twice in a session"
    );
    rig.stop().await;
    // The rig's hold on the process's thread goes before the scene's,
    // which holds the lock itself.
    drop(rig);
    drop(scene);

    let scene = Scene::new("no-look");
    let mut rig = scene.rig(true);
    let mut dial = rig.ready().await;
    // The known file, but no look on the bus at all.
    std::fs::write(&scene.known, NAME).unwrap();
    rig.pump(Duration::from_millis(700)).await;
    let request = begin(&mut rig, &dial).await;
    for _ in 0..7 {
        std::fs::write(&scene.known, NAME).unwrap();
        rig.pump(Duration::from_millis(100)).await;
    }
    let seen = drain(&mut dial).await;
    assert!(
        !told(&end_line(&mut rig, &mut dial, request, seen).await),
        "no look"
    );
    rig.stop().await;
    // The rig's hold on the process's thread goes before the scene's,
    // which holds the lock itself.
    drop(rig);
    drop(scene);

    let mut scene = Scene::new("off");
    let mut rig = scene.rig(false);
    let mut dial = rig.ready().await;
    scene.see(1, Some(NAME));
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    assert!(
        !told(&line(&mut scene, &mut rig, &mut dial).await),
        "the switch off"
    );
    rig.stop().await;
}

/// The reviewer's first P2 on #3050, (a): the owner's line is offered the
/// name, then given up before its End, so its note is never written. The
/// owner's next line in the same connection is told it.
#[tokio::test]
async fn a_name_offered_to_a_line_given_up_before_its_end_is_told_to_the_next() {
    let mut scene = Scene::new("given-up");
    let mut rig = scene.rig(true);
    let mut dial = rig.ready().await;
    scene.see(1, Some(NAME));
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    let session = dial.session;
    let first = begin(&mut rig, &dial).await;
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.cancel_turn(real_time(), owner).unwrap();
    scene.pump(&mut rig, Duration::from_millis(200)).await;
    let before = drain(&mut dial).await;
    assert!(
        !before.iter().any(|message| message.contains("activityEnd")),
        "the first line was committed: {before:?}"
    );
    assert!(!told(&before), "a note was written without a commit");
    let next = begin(&mut rig, &dial).await;
    assert!(next > first);
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    let seen = drain(&mut dial).await;
    let seen = end_line(&mut rig, &mut dial, next, seen).await;
    assert_eq!(rig.dials.try_recv().ok().map(|dial| dial.session), None);
    assert_eq!(dial.session, session, "the same connection");
    assert!(told(&seen), "the next line was not told: {seen:?}");
    rig.stop().await;
}

/// The reviewer's second P2 on #3050, (b): the owner's line is told the
/// name and answered. The owner goes and an unknown face is at the lamp,
/// the line's person `None` as before: the session is renewed fresh before
/// the visitor speaks, and nothing the visitor's request goes into holds
/// the name. The owner back in the fresh session is told it again (the
/// connection-renewal control).
#[tokio::test]
async fn an_unknown_face_after_the_owner_was_told_speaks_into_a_fresh_session() {
    let mut scene = Scene::new("visitor");
    let mut rig = scene.rig(true);
    let mut dial = rig.ready().await;
    scene.see(1, Some(NAME));
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    assert!(
        told(&line(&mut scene, &mut rig, &mut dial).await),
        "the owner"
    );
    answer(&mut rig, &mut dial).await;
    // The owner goes; a face the lamp doesn't know is in view.
    let asked = Instant::now();
    scene.see(1, None);
    let mut next = loop {
        scene.look();
        rig.publish();
        if let Ok(next) = rig.dials.try_recv() {
            break next;
        }
        assert!(
            asked.elapsed() < Duration::from_secs(3),
            "no fresh session before the visitor's line"
        );
        rig.tick(Duration::from_millis(2)).await;
    };
    let setup = rig.service_message(&mut next).await;
    assert!(setup.get("setup").is_some());
    assert!(!setup.to_string().contains(NAME), "{setup}");
    assert!(setup["setup"].get("sessionResumption").is_none() || !next.resumed);
    next.service.send(json!({"setupComplete": {}})).await;
    loop {
        match rig.wire().await {
            Wire::Status(ProviderStatus::Recovered { context, .. }) => {
                assert_eq!(context, RecoveredContext::Fresh);
                break;
            }
            Wire::Status(ProviderStatus::Unavailable { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    eprintln!(
        "person_name: the face changed to the fresh session ready: {} ms",
        asked.elapsed().as_millis()
    );
    // Nothing of the visitor's line went to the old session.
    assert!(
        drain(&mut dial)
            .await
            .iter()
            .all(|message| !message.contains("activityStart"))
    );
    let seen = line(&mut scene, &mut rig, &mut next).await;
    assert!(
        !told(&seen) && !seen.iter().any(|message| message.contains(NAME)),
        "{seen:?}"
    );
    answer(&mut rig, &mut next).await;
    // The owner back, in the fresh session: told again.
    scene.see(1, Some(NAME));
    scene.pump(&mut rig, Duration::from_millis(700)).await;
    assert!(
        told(&line(&mut scene, &mut rig, &mut next).await),
        "the owner back"
    );
    assert!(rig.dials.try_recv().is_err(), "renewed with the owner back");
    rig.stop().await;
}

/// The backstop at a line's Start (the reviewer's second P2 on #3050): in
/// a session that may hold the name, a line that may not have it (another
/// known face, alone in view) is not sent, and fails as not delivered; the
/// owner's line is sent. In a session that holds nothing, either is sent.
#[test]
fn a_line_that_may_not_have_the_name_is_not_sent_into_a_session_that_may_hold_it() {
    let mut scene = Scene::new("backstop");
    for (holds, face, sent) in [
        (false, "Someone Else", true),
        (true, "Someone Else", false),
        (true, NAME, true),
    ] {
        crate::person::live_connected(1, false);
        if holds {
            crate::person::live_offered(1, 1_000);
        }
        let mut owner = controller();
        let mut handoff = memory_handoff();
        handoff.tolerate_outage = true;
        handoff.setup = Some((NAME.to_owned(), lamp_bus::Bus::new(&scene.root).unwrap()));
        let sink = RecordingSink::default();
        let request = authorize(&mut owner, &mut handoff);
        cache_face(&handoff, face);
        scene.see(1, None);
        handoff
            .submit(start(request, &handoff), NOW, &sink)
            .unwrap();
        assert_eq!(
            sink.calls.borrow().contains(&Call::Start(request)),
            sent,
            "{holds} {face}"
        );
        assert_eq!(
            handoff.undelivered,
            (!sent).then_some(request),
            "{holds} {face}"
        );
    }
    crate::person::live_connected(1, false);
}
