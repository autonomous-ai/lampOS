//! Posture against a local stand-in of the pose socket ([`super::mock`]),
//! speaking V1's key exchange: the time passed in is the test's, the
//! sockets are real.
use super::client::{Address, Client, Failure};
use super::mock::{Mock, Reply, Seen, public};
use super::*;
use lamp_msgs::PostureRegion;
use serde_json::{Value, json};
use std::time::Instant;

const S: u64 = 1_000_000;
const KEY: &str = "test-account-key";

fn pose_for(base: &str) -> Pose {
    let client = Client::new(
        Address::parse(base).unwrap(),
        Zeroizing::new(KEY.to_owned()),
        None,
    )
    .with_public_key(public());
    Pose::with_jitter(client, Box::new(|| 0.5))
}

fn pose(mock: &Mock) -> Pose {
    pose_for(&mock.base())
}

/// A frame's meta at 320×240, the size sent as it came, so [`jpeg`]'s
/// bytes need not decode.
fn meta() -> FrameMeta {
    let mut meta = crate::frames::tests::meta(crate::frames::tests::grant(1));
    (meta.width, meta.height) = crate::scale::SENT;
    meta
}

fn jpeg() -> Vec<u8> {
    vec![0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9]
}

fn good() -> Value {
    json!({"score": 2, "risk_level": 1, "left": {"score": 2, "body_scores": {"neck": 1, "trunk": 1}}, "right": {"score": 2}})
}

fn slumped() -> Value {
    json!({"score": 5, "risk_level": 3, "left": {"score": 5, "body_scores": {"neck": 4, "trunk": 2}}, "right": {"score": 4, "body_scores": {"neck": 3}}})
}

/// Hand a frame at `now_us`, as the frames do once the mark is drawn.
fn send(pose: &mut Pose, now_us: u64) {
    assert!(pose.ready(now_us), "ready at {now_us}");
    pose.frame(now_us, &meta(), &jpeg());
    assert!(pose.in_flight());
}

/// Step at `now_us` under `gate` until the request ends (real time, 5 s at
/// most); what was said meanwhile.
fn settle(pose: &mut Pose, now_us: u64, gate: Result<(), Closed>) -> Vec<HumanPosture> {
    let until = Instant::now() + Duration::from_secs(5);
    let mut said = Vec::new();
    loop {
        said.extend(pose.step(now_us, gate, false));
        if !pose.in_flight() || Instant::now() > until {
            return said;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn closed_count(seen: &[Seen]) -> usize {
    seen.iter().filter(|seen| **seen == Seen::Closed).count()
}

#[test]
fn one_frame_every_30_s_each_on_its_own_socket_closed_after() {
    let mock = Mock::start(vec![Reply::Ergo(Some(good()))]);
    let mut pose = pose(&mock);
    assert!(
        pose.step(0, Ok(()), false).is_none(),
        "nothing said before a sample"
    );
    send(&mut pose, 0);
    let said = settle(&mut pose, S, Ok(()));
    assert_eq!(
        said.iter().map(|p| p.window_s).collect::<Vec<_>>(),
        [1],
        "said with the sitting's first sample"
    );
    assert_eq!(pose.counts().answered, 1);
    assert_eq!(pose.samples(), 1);
    for at in [S, 10 * S, 30 * S - 1] {
        assert!(!pose.ready(at), "not before 30 s: {at}");
    }
    // A frame that comes before its time is dropped, never sent.
    pose.frame(15 * S, &meta(), &jpeg());
    assert!(!pose.in_flight());
    assert_eq!(pose.counts().dropped, 1);
    send(&mut pose, 30 * S);
    settle(&mut pose, 31 * S, Ok(()));
    assert_eq!(pose.counts().sent, 2);
    assert_eq!(pose.counts().answered, 2);

    let seen = mock.wait_for(|seen| closed_count(seen) == 2);
    let path = "/api/v1/ai/v1/ws/hal/api/dl/pose-estimation/ws".to_owned();
    let one = vec![
        Seen::Connected {
            api_key: Some(KEY.to_owned()),
            path: path.clone(),
        },
        Seen::KeyExchange,
        Seen::Config(
            json!({"type": "config", "task": "pose", "frame_interval": 1.0, "confidence_threshold_2d": 0.3, "min_valid_keypoints": 5}),
        ),
        Seen::Frame(jpeg()),
        Seen::Closed,
    ];
    let mut both = one.clone();
    both.extend(one);
    assert_eq!(seen, both, "two sockets, each closed after its one frame");
}

#[test]
fn the_window_reaches_the_topic_every_30_s() {
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let mut pose = pose(&mock);
    let mut said = Vec::new();
    for at in [0, 30, 60] {
        said.extend(pose.step(at * S, Ok(()), false));
        send(&mut pose, at * S);
        said.extend(settle(&mut pose, at * S, Ok(())));
    }
    // Said with the first sample at 0 s, then at 30 and 60 s: under three
    // samples until the third.
    assert_eq!(said.len(), 3);
    assert!(said.iter().all(|p| p.bad_share == 0.0));
    assert!(
        pose.step(89 * S, Ok(()), false).is_none(),
        "not before 30 s"
    );
    let at_90 = pose.step(90 * S, Ok(()), false).unwrap();
    assert_eq!(at_90.window_s, 90);
    assert_eq!(at_90.bad_share, 1.0);
    assert_eq!(at_90.region, PostureRegion::Neck);
    // A body not found is answered, but no sample.
    mock.then(Reply::Ergo(None));
    send(&mut pose, 90 * S);
    settle(&mut pose, 91 * S, Ok(()));
    assert_eq!(pose.counts().no_body, 1);
    assert_eq!(pose.samples(), 3);
}

#[test]
fn a_gate_closed_before_the_frame_goes_sends_none() {
    let mock = Mock::start(vec![Reply::SlowKeyExchange(Duration::from_millis(400))]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    send(&mut pose, 0);
    mock.wait_for(|seen| seen.contains(&Seen::KeyExchange));
    // The frames tell the sink the gate closed (privacy, here).
    pose.stopped();
    settle(&mut pose, S, Err(Closed::PrivacyClosed));
    assert_eq!(pose.counts().cancelled, 1);
    let seen = mock.wait_for(|seen| closed_count(seen) == 1);
    assert!(seen.contains(&Seen::Closed), "{seen:?}");
    assert!(
        !seen
            .iter()
            .any(|seen| matches!(seen, Seen::Frame(_) | Seen::Config(_))),
        "no frame after the gate closed: {seen:?}"
    );
    assert!(!pose.warn(), "a cancel is no failure");
}

#[test]
fn a_gate_closed_while_waiting_closes_the_socket_and_drops_the_answer() {
    let mock = Mock::start(vec![Reply::Silent]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    send(&mut pose, 0);
    mock.wait_for(|seen| seen.iter().any(|seen| matches!(seen, Seen::Frame(_))));
    let started = Instant::now();
    // Nobody at the desk: the request stops and the window is forgotten.
    let said = settle(&mut pose, 2 * S, Err(Closed::Nobody));
    assert!(said.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "stopped at once"
    );
    assert_eq!(pose.counts().cancelled, 1);
    assert_eq!(pose.samples(), 0);
    let seen = mock.wait_for(|seen| closed_count(seen) == 1);
    assert_eq!(closed_count(&seen), 1, "{seen:?}");
    assert!(!pose.warn());
}

#[test]
fn busy_drops_and_three_in_a_row_back_off_with_warn() {
    let mock = Mock::start(vec![Reply::Busy]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    for at in [0, 30] {
        send(&mut pose, at * S);
        settle(&mut pose, at * S, Ok(()));
        assert_eq!(pose.state(at * S + 1), "idle", "a busy frame is dropped");
    }
    send(&mut pose, 60 * S);
    settle(&mut pose, 60 * S, Ok(()));
    assert_eq!(pose.counts().busy, 3);
    assert_eq!(pose.samples(), 0);
    // 60 s with no jitter (the draw is 0.5).
    assert_eq!(pose.paused_until(60 * S), Some(120 * S));
    assert!(!pose.ready(90 * S));
    assert_eq!(pose.state(90 * S), "backing_off");
    assert!(pose.warn());

    // A refused handshake (503) pauses at once, twice as long.
    mock.then(Reply::Http(503));
    send(&mut pose, 120 * S);
    settle(&mut pose, 120 * S, Ok(()));
    assert_eq!(pose.last_failure(), Some(Failure::Handshake(Some(503))));
    assert_eq!(pose.paused_until(120 * S), Some(240 * S));

    // The server's own error pauses too.
    mock.then(Reply::Failed);
    send(&mut pose, 240 * S);
    settle(&mut pose, 240 * S, Ok(()));
    assert_eq!(pose.last_failure(), Some(Failure::Server));
    assert_eq!(pose.paused_until(240 * S), Some(480 * S));

    // An answer clears it.
    mock.then(Reply::Ergo(Some(good())));
    send(&mut pose, 480 * S);
    settle(&mut pose, 480 * S, Ok(()));
    assert!(!pose.warn());
    assert_eq!(pose.state(481 * S), "idle");
    assert!(pose.ready(510 * S));
}

#[test]
fn no_server_or_a_refused_key_exchange_backs_off() {
    // A port nobody listens on.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let mut offline = pose_for(&format!("http://127.0.0.1:{port}"));
    offline.step(0, Ok(()), false);
    send(&mut offline, 0);
    settle(&mut offline, 0, Ok(()));
    assert_eq!(offline.last_failure(), Some(Failure::Connect));
    assert_eq!(offline.paused_until(0), Some(60 * S));

    let mock = Mock::start(vec![Reply::RefuseKeyExchange]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    send(&mut pose, 0);
    settle(&mut pose, 0, Ok(()));
    assert_eq!(pose.last_failure(), Some(Failure::KeyExchange));
    let seen = mock.wait_for(|seen| closed_count(seen) == 1);
    assert!(
        !seen.iter().any(|seen| matches!(seen, Seen::Frame(_))),
        "{seen:?}"
    );
}

#[test]
fn the_window_resets_whenever_the_gate_closes_or_on_standing_up() {
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let mut pose = pose(&mock);
    let fill = |pose: &mut Pose, from: u64| {
        for index in 0..3 {
            let at = from + index * 30 * S;
            pose.step(at, Ok(()), false);
            send(pose, at);
            settle(pose, at, Ok(()));
        }
        assert_eq!(pose.samples(), 3);
    };
    let mut at = 0;
    // The privacy and sleep included: no old sample is said under them.
    for closed in [
        Closed::PrivacyClosed,
        Closed::PrivacyUnknown,
        Closed::PresenceUnknown,
        Closed::Asleep,
        Closed::SleepUnknown,
        Closed::Stranger,
        Closed::Nobody,
        Closed::NoOptIn,
        Closed::NotSaidYet,
    ] {
        fill(&mut pose, at);
        let resets = pose.counts().resets;
        assert!(pose.step(at + 70 * S, Err(closed), false).is_none());
        assert_eq!(pose.samples(), 0, "{closed:?}");
        assert_eq!(pose.counts().resets, resets + 1);
        assert!(!pose.ready(at + 200 * S), "no frame taken: {closed:?}");
        // Nothing said while it stays closed, though the 30 s came, nor
        // once open again until a sample lands.
        assert!(pose.step(at + 100 * S, Err(closed), false).is_none());
        assert!(pose.step(at + 201 * S, Ok(()), false).is_none());
        send(&mut pose, at + 201 * S);
        let again = settle(&mut pose, at + 201 * S, Ok(()));
        assert_eq!(
            again
                .iter()
                .map(|p| (p.window_s, p.bad_share))
                .collect::<Vec<_>>(),
            [(0, 0.0)],
            "{closed:?}"
        );
        at += 1_000 * S;
    }
    fill(&mut pose, at);
    assert!(pose.step(at + 70 * S, Ok(()), true).is_none());
    assert_eq!(pose.samples(), 0, "stood up");
    assert!(
        !pose.ready(at + 70 * S),
        "the last frame went at {at} + 60 s"
    );
    send(&mut pose, at + 90 * S);
    let fresh = settle(&mut pose, at + 90 * S, Ok(()));
    assert_eq!(
        fresh.iter().map(|p| p.window_s).collect::<Vec<_>>(),
        [20],
        "a new sitting from 70 s"
    );
}

#[test]
fn nothing_is_said_while_the_window_holds_no_sample() {
    // The server busy, a failure, or no body found: the topic runs out.
    for reply in [Reply::Busy, Reply::Ergo(None)] {
        let mock = Mock::start(vec![reply]);
        let mut pose = pose(&mock);
        let mut said = Vec::new();
        for at in [0, 30, 60, 90] {
            said.extend(pose.step(at * S, Ok(()), false));
            if pose.ready(at * S) {
                send(&mut pose, at * S);
                said.extend(settle(&mut pose, at * S, Ok(())));
            }
        }
        assert_eq!(pose.samples(), 0);
        assert!(said.is_empty(), "{said:?}");
    }
    // Frames not going out at all (the opt-in off, the slide shut): the
    // gate open and due, but no sample, so nothing said.
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let mut pose = pose(&mock);
    for at in [0, 30, 60, 90, 120] {
        assert!(pose.step(at * S, Ok(()), false).is_none(), "{at}");
    }
    // A sample: said at once, then every 30 s.
    send(&mut pose, 121 * S);
    let said = settle(&mut pose, 121 * S, Ok(()));
    assert_eq!(said.len(), 1);
    assert!(pose.step(150 * S, Ok(()), false).is_none());
    assert!(pose.step(151 * S, Ok(()), false).is_some());
    // Wiped (a stood-up, or any closing): no longer said.
    assert!(pose.step(152 * S, Ok(()), true).is_none());
    for at in [181, 211, 241] {
        assert!(pose.step(at * S, Ok(()), false).is_none(), "{at}");
    }
}

#[test]
fn nothing_is_said_while_the_privacy_is_on() {
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let mut pose = pose(&mock);
    for at in [0, 30, 60] {
        pose.step(at * S, Ok(()), false);
        send(&mut pose, at * S);
        settle(&mut pose, at * S, Ok(()));
    }
    assert_eq!(pose.samples(), 3);
    // The slide closed at 61 s: the window, slumped, is forgotten, and
    // nothing is said at 90 s or after while it stays closed.
    for at in [61, 90, 120, 150] {
        assert!(
            pose.step(at * S, Err(Closed::PrivacyClosed), false)
                .is_none(),
            "{at}"
        );
    }
    assert_eq!(pose.samples(), 0);
    // Open again: nothing said until a sample, then a new window, nothing
    // slumped carried.
    assert!(pose.step(151 * S, Ok(()), false).is_none());
    send(&mut pose, 151 * S);
    let back = settle(&mut pose, 151 * S, Ok(()));
    assert_eq!(
        back.iter()
            .map(|p| (p.window_s, p.bad_share))
            .collect::<Vec<_>>(),
        [(0, 0.0)]
    );
}

#[test]
fn posture_takes_no_frame_until_its_line_was_said() {
    let mock = Mock::start(vec![Reply::Ergo(Some(good()))]);
    let mut pose = pose(&mock);
    // Activity's gate open, but only the plain on line said.
    let plain = gate(Ok(()), false);
    assert_eq!(plain, Err(Closed::NotSaidYet));
    assert!(pose.step(0, plain, false).is_none());
    assert!(!pose.ready(0), "no mark asked for posture");
    pose.frame(0, &meta(), &jpeg());
    assert!(!pose.in_flight());
    assert_eq!(pose.counts().sent, 0);
    assert_eq!(pose.counts().dropped, 1);
    // A closed frames' gate stays its own reason.
    assert_eq!(
        gate(Err(Closed::PrivacyClosed), true),
        Err(Closed::PrivacyClosed)
    );
    // The posture line said: frames flow.
    let said = gate(Ok(()), true);
    assert_eq!(said, Ok(()));
    assert!(pose.step(S, said, false).is_none(), "no sample yet");
    send(&mut pose, S);
    assert_eq!(settle(&mut pose, S, said).len(), 1);
    assert_eq!(pose.counts().answered, 1);
    let seen = mock.wait_for(|seen| closed_count(seen) == 1);
    assert_eq!(
        seen.iter()
            .filter(|seen| matches!(seen, Seen::Frame(_)))
            .count(),
        1
    );
}

#[test]
fn a_frame_not_the_cameras_own_jpeg_is_skipped_without_backing_off() {
    let mock = Mock::start(vec![Reply::Ergo(Some(good()))]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    let odd = |change: fn(&mut FrameMeta)| {
        let mut frame = meta();
        change(&mut frame);
        frame
    };
    for frame in [
        odd(|m| m.width = 160),
        odd(|m| m.height = 120),
        odd(|m| m.format = *b"YUYV"),
        odd(|m| {
            m.derived = Some(lamp_camera::frames::DerivedImage {
                source_width: 640,
                source_height: 480,
                quality: 80,
            })
        }),
    ] {
        pose.frame(0, &frame, &jpeg());
        assert!(!pose.in_flight(), "never sent");
    }
    assert_eq!(pose.counts().wrong_frame, 4);
    assert_eq!(pose.counts().sent, 0);
    assert!(!pose.warn(), "no backoff");
    assert_eq!(pose.paused_until(0), None);
    // The next frame, the camera's own, goes at once.
    send(&mut pose, 0);
    settle(&mut pose, 0, Ok(()));
    assert_eq!(pose.counts().answered, 1);
    assert!(mock.seen().iter().all(|seen| match seen {
        Seen::Frame(bytes) => *bytes == jpeg(),
        _ => true,
    }));
}

#[test]
fn the_cameras_1280_by_960_goes_at_320_by_240_and_a_bad_one_is_skipped() {
    use crate::scale::tests::{camera_jpeg, decoded};
    let mock = Mock::start(vec![Reply::Ergo(Some(good()))]);
    let mut pose = pose(&mock);
    pose.step(0, Ok(()), false);
    let sized = |width: u32, height: u32| {
        let mut frame = meta();
        (frame.width, frame.height) = (width, height);
        frame
    };
    // Said 1280×960, but not a JPEG that decodes: skipped, no backoff.
    pose.frame(0, &sized(1280, 960), &jpeg());
    assert!(!pose.in_flight());
    assert_eq!(pose.counts().wrong_frame, 1);
    assert!(!pose.warn());
    // The lamp's camera at 1280×960: sent, scaled to 320×240.
    assert!(pose.ready(0));
    pose.frame(0, &sized(1280, 960), &camera_jpeg(1280, 960));
    assert!(pose.in_flight());
    settle(&mut pose, 0, Ok(()));
    assert_eq!(pose.counts().answered, 1);
    let frames: Vec<Vec<u8>> = mock
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Frame(bytes) => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 1);
    assert_eq!(decoded(&frames[0]).0, (320, 240));
}

#[test]
fn said_while_the_gate_is_open_and_not_after() {
    let mock = Mock::start(vec![Reply::Ergo(Some(good()))]);
    let mut pose = pose(&mock);
    assert!(pose.step(0, Err(Closed::PrivacyClosed), false).is_none());
    assert!(pose.step(S, Ok(()), false).is_none(), "no sample yet");
    send(&mut pose, S);
    assert_eq!(settle(&mut pose, S, Ok(())).len(), 1);
    assert!(pose.step(2 * S, Ok(()), false).is_none());
    assert!(pose.step(31 * S, Ok(()), false).is_some());
    // Asleep: forgotten, and nothing said while it lasts.
    assert!(pose.step(61 * S, Err(Closed::Asleep), false).is_none());
    assert!(pose.step(91 * S, Err(Closed::Asleep), false).is_none());
    // Awake: nothing said until a new sample, in a new window.
    assert!(pose.step(122 * S, Ok(()), false).is_none());
    send(&mut pose, 122 * S);
    let back = settle(&mut pose, 122 * S, Ok(()));
    assert_eq!(
        back.iter().map(|p| p.window_s).collect::<Vec<_>>(),
        [0],
        "a new window"
    );
}

/// What `serve` does with the pause's verdict, posture and the sitting's
/// timers together: what is said this step.
fn as_serve(
    pose: &mut Pose,
    sitting: &mut Option<crate::sitting::Sitting>,
    verdict: crate::pause::Verdict,
    gate: Result<(), Closed>,
    now: u64,
) -> (Option<HumanPosture>, Option<lamp_msgs::HumanDoing>) {
    if verdict.wipe {
        *sitting = None;
        pose.forget();
    }
    if !verdict.paused && matches!(gate, Err(Closed::Nobody | Closed::Stranger)) {
        *sitting = None;
    }
    if gate.is_ok() && sitting.is_none() {
        *sitting = Some(crate::sitting::Sitting::new(now));
    }
    let posture = if verdict.paused {
        pose.hold(now);
        None
    } else {
        pose.step(now, gate, false)
    };
    let doing = sitting
        .as_ref()
        .filter(|_| !verdict.paused)
        .map(|sitting| sitting.doing(now));
    (posture, doing)
}

/// The owner at the desk from 0 s, three slumped samples at 0, 30 and 60 s
/// and a drink seen at 60 s; the posture said at 60 s last.
fn sat_and_drank(
    mock: &Mock,
) -> (
    Pose,
    Option<crate::sitting::Sitting>,
    crate::pause::tests::Scene,
) {
    use crate::pause::tests::Scene;
    let mut pose = pose(mock);
    let mut sitting = None;
    let mut scene = Scene::owner_at(0);
    for at in [0, 30 * S, 60 * S] {
        scene.faces(&[("f1", 0.4)], at);
        scene.known(at);
        let verdict = scene.step(at);
        as_serve(&mut pose, &mut sitting, verdict, scene.gate(at), at);
        send(&mut pose, at);
        settle(&mut pose, at, Ok(()));
    }
    assert_eq!(pose.samples(), 3);
    let drink = lamp_msgs::DoingSeen {
        what: lamp_msgs::Doing::Drinking,
        confidence: 0.9,
    };
    sitting.as_mut().unwrap().saw(&[drink], 60 * S);
    (pose, sitting, scene)
}

#[test]
fn known_misread_known_within_5_s_keeps_the_window_and_the_drink() {
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let (mut pose, mut sitting, mut scene) = sat_and_drank(&mock);
    // The owner looks down at 89 s: paused, through the posture due at 90 s.
    let verdict = scene.misread(89 * S);
    assert!(verdict.paused);
    for at in [89 * S, 90 * S, 91 * S, 92 * S] {
        scene.faces(&[("f1", 0.42)], at);
        let verdict = scene.step(at);
        assert!(verdict.paused && !verdict.wipe, "{at}");
        let said = as_serve(&mut pose, &mut sitting, verdict, scene.gate(at), at);
        assert_eq!(said, (None, None), "nothing said while paused: {at}");
        assert!(!pose.ready(at), "no frame taken while paused");
    }
    assert_eq!(pose.samples(), 3, "the window kept, in memory");
    // Recognised again at 93 s on the same face: the sitting goes on.
    scene.faces(&[("f1", 0.4)], 93 * S);
    scene.known(93 * S);
    let verdict = scene.step(93 * S);
    assert_eq!(verdict, crate::pause::Verdict::default());
    let (posture, doing) = as_serve(&mut pose, &mut sitting, verdict, scene.gate(93 * S), 93 * S);
    let posture = posture.expect("said at once, the 90 s one owed");
    assert_eq!(posture.window_s, 93, "the window from 0 s, not reset");
    assert!(posture.bad_share > 0.0, "the slumped samples kept");
    assert_eq!(doing.unwrap().drank_s_ago, 33, "the drink at 60 s kept");
}

#[test]
fn the_privacy_during_a_pause_wipes_and_says_nothing() {
    let mock = Mock::start(vec![Reply::Ergo(Some(slumped()))]);
    let (mut pose, mut sitting, mut scene) = sat_and_drank(&mock);
    assert!(scene.misread(89 * S).paused);
    let resets = pose.counts().resets;
    scene.privacy = false;
    for at in [90 * S, 95 * S, 120 * S] {
        scene.faces(&[("f1", 0.4)], at);
        let verdict = scene.step(at);
        let said = as_serve(&mut pose, &mut sitting, verdict, scene.gate(at), at);
        assert_eq!(said.0, None, "no posture under the privacy: {at}");
        assert_eq!(pose.samples(), 0, "wiped: {at}");
    }
    assert_eq!(pose.counts().resets, resets + 1);
    // Open again, recognised: a fresh window and timers, nothing carried.
    scene.privacy = true;
    scene.faces(&[("f1", 0.4)], 121 * S);
    scene.known(121 * S);
    let verdict = scene.step(121 * S);
    let (posture, doing) = as_serve(
        &mut pose,
        &mut sitting,
        verdict,
        scene.gate(121 * S),
        121 * S,
    );
    assert_eq!(posture, None, "no sample in the fresh window yet");
    assert_eq!(doing.unwrap().drank_s_ago, 0, "from the new arrival");
    send(&mut pose, 121 * S);
    let said = settle(&mut pose, 121 * S, Ok(()));
    assert_eq!(
        pose.samples(),
        1,
        "the fresh window holds only the new sample"
    );
    assert!(
        said.iter().all(|p| p.window_s == 0 && p.bad_share == 0.0),
        "nothing slumped carried: {said:?}"
    );
}
