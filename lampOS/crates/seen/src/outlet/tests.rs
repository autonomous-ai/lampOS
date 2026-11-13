//! Activity and posture together behind the ring's mark, each against its
//! local stand-in server: one mark for each frame, and no frame twice.
use super::*;
use crate::activity::tests::{Mock as ActivityMock, Reply, Script, pace};
use crate::gate::Closed;
use crate::mark::{BLINK_US, FRESH_US, Marked, tests::FakeRing};
use crate::pose::client::{Address, Client};
use crate::pose::mock::{Mock as PoseMock, Reply as PoseReply, Seen, public};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const S: u64 = 1_000_000;
const MS: u64 = 1_000;

fn pose(mock: &PoseMock) -> Pose {
    let client = Client::new(
        Address::parse(&mock.base()).unwrap(),
        Zeroizing::new("test-account-key".to_owned()),
        None,
    )
    .with_public_key(public());
    Pose::with_jitter(client, Box::new(|| 0.5))
}

/// A frame the camera captured at `at_us`.
fn captured(at_us: u64) -> FrameMeta {
    let mut meta = crate::frames::tests::meta(crate::frames::tests::grant(1));
    // 320×240, sent as it came, so the test's bytes need not decode.
    (meta.width, meta.height) = crate::scale::SENT;
    meta.dequeue_completed_at_us = at_us;
    meta
}

/// Ask through the mark at `at_us`, have the ring draw it, and pass one
/// frame captured inside the blink: whether a frame was wanted (a mark
/// asked).
fn one_frame(marked: &mut Marked<FakeRing, Outlet>, at_us: u64, jpeg: &[u8]) -> bool {
    let asks = marked.ring_mut().asks.len();
    assert!(
        !marked.ready(at_us),
        "no frame before the ring drew the mark"
    );
    if marked.ring_mut().asks.len() == asks {
        return false;
    }
    marked.ring_mut().draws(at_us);
    assert!(marked.ready(at_us + FRESH_US), "drawn: a frame is asked");
    marked.frame(at_us + FRESH_US + MS, &captured(at_us + FRESH_US), jpeg);
    true
}

#[test]
fn activity_and_posture_share_one_mark_and_neither_takes_a_frame_twice() {
    let activity_server = ActivityMock::start(vec![Script::Answer(vec![Reply::Labels(vec![])])]);
    let pose_server = PoseMock::start(vec![PoseReply::Ergo(None)]);
    let mut pose = pose(&pose_server);
    // Posture's gate open: its line said.
    pose.step(0, Ok(()), false);
    let outlet = Outlet {
        activity: Some(Box::new(Activity::new(activity_server.endpoint(), pace()))),
        pose: Some(Box::new(pose)),
    };
    let mut marked = Marked::new(FakeRing::default(), outlet);

    // Both want the first frame: one ask of the ring, one frame to each.
    let first = [0xff, 0xd8, 1, 0xff, 0xd9];
    assert!(one_frame(&mut marked, S, &first));
    assert_eq!(marked.ring_mut().asks.len(), 1, "one mark for both");
    assert_eq!(marked.counts().passed, 1);
    let inner = marked.inner();
    assert_eq!(inner.activity.as_ref().unwrap().counts().frames, 1);
    assert_eq!(inner.pose.as_ref().unwrap().counts().sent, 1);

    // The burst's next frames: activity's alone, each its own mark;
    // posture, not due for 30 s, takes none of them.
    let mut at = S;
    for n in 2..=4u8 {
        at += crate::activity::SPACING_US;
        assert!(one_frame(&mut marked, at, &[0xff, 0xd8, n, 0xff, 0xd9]));
        assert_eq!(marked.ring_mut().asks.len(), usize::from(n));
    }
    let inner = marked.inner();
    assert_eq!(inner.activity.as_ref().unwrap().counts().frames, 4);
    assert_eq!(inner.pose.as_ref().unwrap().counts().sent, 1, "no second");

    // What each server got: posture the first frame once, activity each
    // frame once.
    let seen = pose_server.wait_for(|seen| seen.contains(&Seen::Closed));
    let pose_frames: Vec<_> = seen
        .iter()
        .filter_map(|seen| match seen {
            Seen::Frame(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(pose_frames, [first.to_vec()]);
    activity_server.wait("four frames", |seen| seen.frames.len() == 4);
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    activity_server.seen(|seen| {
        assert_eq!(seen.frames.len(), 4, "no frame twice");
        assert_eq!(seen.frames[0], first);
    });

    // The gate closing stops both and lets the mark go.
    marked.stopped();
    assert!(!marked.holds_mark());
    let pose = marked.inner_mut().pose.as_mut().unwrap();
    pose.step(at + BLINK_US, Err(Closed::PrivacyClosed), false);
    assert!(!pose.ready(at + 60 * S), "closed: posture takes none");
}
