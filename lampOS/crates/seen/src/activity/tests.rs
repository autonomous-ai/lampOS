//! Against a local WebSocket server that speaks V1's side: it opens the
//! key exchange with the test key's private half (`tests/fixtures`, a
//! throwaway key no server holds), then answers each frame as scripted,
//! with labels, `busy`, an error, or nothing.
use super::*;
use crate::{
    crypto::{Session, public_key_from_pem},
    frames::tests::{grant, meta},
    wire,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use lamp_msgs::Doing;
use rsa::{BigUint, Oaep, RsaPrivateKey};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{ErrorResponse, Request, Response},
};

const VECTOR: &str = include_str!("../../tests/fixtures/v1-vector.json");
const PUBLIC: &str = include_str!("../../tests/fixtures/test-server-public.pem");
const S: u64 = 1_000_000;

fn private() -> RsaPrivateKey {
    let vector: Value = serde_json::from_str(VECTOR).unwrap();
    let number =
        |name: &str| BigUint::parse_bytes(vector[name].as_str().unwrap().as_bytes(), 16).unwrap();
    RsaPrivateKey::from_components(
        number("n"),
        number("e"),
        number("d"),
        vec![number("p"), number("q")],
    )
    .unwrap()
}

/// The server's answer to one frame.
#[derive(Clone, Debug)]
pub(crate) enum Reply {
    Labels(Vec<(&'static str, f64)>),
    Busy,
    Error,
    Silent,
}

/// What the server does with each connection, in turn; the last repeats.
#[derive(Clone, Debug)]
pub(crate) enum Script {
    /// Upgrade, and answer the frames in turn (the last answer repeats).
    Answer(Vec<Reply>),
    /// Refuse the upgrade with this status.
    Refuse(u16),
}

/// What the server saw.
#[derive(Default)]
pub(crate) struct Seen {
    connections: usize,
    api_keys: Vec<String>,
    /// Each connection's plaintexts after the key exchange, in order.
    plain: Vec<Vec<Value>>,
    /// The frames' JPEGs, over all connections.
    pub(crate) frames: Vec<Vec<u8>>,
    /// Connections that ended.
    closed: usize,
}

pub(crate) struct Mock {
    addr: SocketAddr,
    seen: Arc<Mutex<Seen>>,
}

impl Mock {
    pub(crate) fn start(scripts: Vec<Script>) -> Self {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let shared = seen.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                let mut index = 0;
                while let Ok((stream, _)) = listener.accept().await {
                    let script = scripts[index.min(scripts.len() - 1)].clone();
                    index += 1;
                    tokio::spawn(serve(stream, script, shared.clone()));
                }
            });
        });
        Self {
            addr: addr_rx.recv().unwrap(),
            seen,
        }
    }

    pub(crate) fn endpoint(&self) -> Endpoint {
        let mut endpoint = Endpoint::new(&format!("http://{}", self.addr), "test-key").unwrap();
        endpoint.public_key = Some(public_key_from_pem(PUBLIC).unwrap());
        endpoint
    }

    pub(crate) fn seen<T>(&self, read: impl FnOnce(&Seen) -> T) -> T {
        read(&self.seen.lock().unwrap())
    }

    /// Wait until `ready` holds of what the server saw.
    pub(crate) fn wait(&self, what: &str, ready: impl Fn(&Seen) -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !self.seen(&ready) {
            assert!(Instant::now() < until, "the server never saw: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

async fn serve(stream: tokio::net::TcpStream, script: Script, seen: Arc<Mutex<Seen>>) {
    let refuse = match &script {
        Script::Refuse(status) => Some(*status),
        Script::Answer(_) => None,
    };
    let key_seen = seen.clone();
    // The handshake callback's error type is tungstenite's, not ours.
    #[allow(clippy::result_large_err)]
    let callback =
        move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            let key = request
                .headers()
                .get("x-api-key")
                .and_then(|key| key.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let mut seen = key_seen.lock().unwrap();
            seen.connections += 1;
            seen.api_keys.push(key);
            seen.plain.push(Vec::new());
            match refuse {
                Some(status) => Err(tokio_tungstenite::tungstenite::http::Response::builder()
                    .status(status)
                    .body(None)
                    .unwrap()),
                None => Ok(response),
            }
        };
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    let Script::Answer(replies) = script else {
        return;
    };
    let connection = seen.lock().unwrap().connections - 1;
    // The key exchange, as lbserver opens it.
    let session = match socket.next().await {
        Some(Ok(Message::Text(text))) => {
            let exchange: Value = serde_json::from_str(text.as_str()).unwrap();
            assert_eq!(exchange["type"], "key_exchange");
            let sealed = STANDARD
                .decode(exchange["encrypted_key"].as_str().unwrap())
                .unwrap();
            let key: [u8; 32] = private()
                .decrypt(Oaep::new::<sha2::Sha256>(), &sealed)
                .unwrap()
                .try_into()
                .unwrap();
            let public = public_key_from_pem(PUBLIC).unwrap();
            socket
                .send(Message::text(r#"{"status":"key_exchange_ok"}"#))
                .await
                .unwrap();
            Session::with_key(&key, &public, &mut rand_core::OsRng).unwrap()
        }
        _ => return,
    };
    let mut frames = 0;
    loop {
        let text = match socket.next().await {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => break,
        };
        let plain: Value =
            serde_json::from_str(&session.unwrap_ws_message(text.as_str()).unwrap()).unwrap();
        seen.lock().unwrap().plain[connection].push(plain.clone());
        let answer = match plain["type"].as_str() {
            Some("config") => Some(json!({"status": "config_updated"})),
            Some("frame") => {
                let jpeg = STANDARD
                    .decode(plain["frame_b64"].as_str().unwrap())
                    .unwrap();
                seen.lock().unwrap().frames.push(jpeg);
                let reply = replies[frames.min(replies.len() - 1)].clone();
                frames += 1;
                match reply {
                    Reply::Labels(labels) => Some(json!({
                        "detected_classes": labels
                            .iter()
                            .map(|(name, conf)| json!({"class_name": name, "conf": conf}))
                            .collect::<Vec<_>>()
                    })),
                    Reply::Busy => Some(json!({"error": "busy"})),
                    Reply::Error => Some(json!({"error": "Processing failed"})),
                    Reply::Silent => None,
                }
            }
            _ => Some(json!({"status": "ok"})),
        };
        if let Some(answer) = answer {
            let wrapped = session
                .wrap_ws_message(&answer.to_string(), &mut rand_core::OsRng)
                .unwrap();
            if socket.send(Message::text(wrapped)).await.is_err() {
                break;
            }
        }
    }
    seen.lock().unwrap().closed += 1;
}

pub(crate) fn pace() -> Pace {
    Pace {
        gap: Duration::from_millis(5),
        connect_within: Duration::from_secs(2),
        answer_within: Duration::from_secs(2),
        frame_within: Duration::from_secs(2),
    }
}

fn jpeg(n: u8) -> Vec<u8> {
    vec![0xff, 0xd8, n, n, n, 0xff, 0xd9]
}

/// A frame's meta at 320×240, the size sent as it came, so [`jpeg`]'s
/// bytes need not decode.
fn sent_meta() -> FrameMeta {
    let mut meta = meta(grant(1));
    (meta.width, meta.height) = crate::scale::SENT;
    meta
}

/// Poll until the burst ends.
fn finish(activity: &mut Activity, now_us: u64) -> Finished {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(finished) = activity.poll(now_us) {
            return finished;
        }
        assert!(Instant::now() < until, "the burst never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// One burst of `frames` frames from `start_us`.
fn burst(activity: &mut Activity, start_us: u64, frames: u8) {
    let meta = sent_meta();
    for n in 0..frames {
        let now = start_us + u64::from(n) * SPACING_US;
        assert!(activity.ready(now), "frame {n} wanted");
        activity.frame(now, &meta, &jpeg(n));
    }
}

/// Offer `frames` frames from `start_us`, wanted or not.
fn offer(activity: &mut Activity, start_us: u64, frames: u8) {
    let meta = sent_meta();
    for n in 0..frames {
        activity.frame(start_us + u64::from(n) * SPACING_US, &meta, &jpeg(n));
    }
}

fn wait_wiped(activity: &Activity, count: u64) {
    let until = Instant::now() + Duration::from_secs(10);
    while activity.wiped() < count {
        assert!(Instant::now() < until, "copies never wiped");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(activity.wiped(), count);
}

#[test]
fn a_full_burst_goes_on_v1s_wire_and_closes_after() {
    let mut replies = vec![Reply::Labels(vec![]); 6];
    replies.push(Reply::Labels(vec![
        ("drinking", 0.42),
        ("using computer", 0.9),
    ]));
    replies.push(Reply::Labels(vec![("drinking", 0.81)]));
    let mock = Mock::start(vec![Script::Answer(replies)]);
    let mut activity = Activity::new(mock.endpoint(), pace());
    assert!(activity.ready(0), "the first burst is due at once");
    burst(&mut activity, 0, 8);
    assert!(!activity.ready(8 * SPACING_US), "eight frames: no more");
    let finished = finish(&mut activity, 10 * S);
    assert_eq!(finished.outcome, Outcome::Answered);
    assert_eq!(finished.seen.len(), 1);
    assert_eq!(finished.seen[0].what, Doing::Drinking);
    assert!((finished.seen[0].confidence - 0.81).abs() < 1e-6);
    // The socket closed after the burst: the server keeps nothing.
    mock.wait("the socket closed", |seen| seen.closed == 1);
    mock.seen(|seen| {
        assert_eq!(seen.connections, 1);
        assert_eq!(seen.api_keys, ["test-key"]);
        let plain = &seen.plain[0];
        assert_eq!(plain.len(), 9, "the config, then eight frames");
        // V1's config exactly, then V1's frame messages.
        assert_eq!(
            plain[0],
            serde_json::to_value(wire::v1_action_config()).unwrap()
        );
        assert_eq!(plain[1]["type"], "frame");
        assert_eq!(plain[1]["task"], "action");
        assert_eq!(seen.frames, (0..8).map(jpeg).collect::<Vec<_>>());
    });
    // Every copy wiped; the next burst a minute after this one began.
    wait_wiped(&activity, 8);
    assert!(!activity.sending());
    assert!(!activity.ready(EVERY_US - 1));
    assert!(activity.ready(EVERY_US));
    assert!(!activity.retreat().warn());
    let counts = activity.counts();
    assert_eq!((counts.bursts, counts.answered, counts.frames), (1, 1, 8));
}

#[test]
fn a_gate_close_mid_burst_aborts_it_and_closes_the_socket() {
    let mock = Mock::start(vec![Script::Answer(vec![Reply::Labels(vec![(
        "drinking", 0.9,
    )])])]);
    let mut activity = Activity::new(mock.endpoint(), pace());
    burst(&mut activity, 0, 3);
    mock.wait("a frame", |seen| !seen.frames.is_empty());
    // The gate closes: the frames' taker tells the sink.
    activity.stopped();
    assert!(!activity.sending());
    mock.wait("the socket closed", |seen| seen.closed == 1);
    assert!(mock.seen(|seen| seen.frames.len()) <= 3);
    wait_wiped(&activity, 3);
    // Nothing is said of an aborted burst, and it is not a failure.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(activity.poll(5 * S), None);
    assert!(!activity.retreat().warn());
    assert_eq!(activity.counts().aborted, 1);
    assert!(!activity.ready(EVERY_US - 1), "the next burst in its turn");
}

#[test]
fn an_abort_does_not_wait_for_a_silent_server() {
    let mock = Mock::start(vec![Script::Answer(vec![Reply::Silent])]);
    let mut slow = pace();
    slow.answer_within = Duration::from_secs(60);
    let mut activity = Activity::new(mock.endpoint(), slow);
    burst(&mut activity, 0, 1);
    mock.wait("a frame", |seen| seen.frames.len() == 1);
    let aborted = Instant::now();
    activity.stopped();
    mock.wait("the socket closed", |seen| seen.closed == 1);
    assert!(aborted.elapsed() < Duration::from_secs(5), "at once");
    wait_wiped(&activity, 1);
}

#[test]
fn busy_drops_the_burst_and_three_in_a_row_retreat() {
    let busy = Script::Answer(vec![Reply::Busy]);
    let answered = Script::Answer(vec![Reply::Labels(vec![])]);
    let mock = Mock::start(vec![
        busy.clone(),
        busy.clone(),
        busy.clone(),
        busy,
        answered,
    ]);
    let mut activity = Activity::new(mock.endpoint(), pace());
    for (turn, start) in [0, 60 * S].into_iter().enumerate() {
        offer(&mut activity, start, 2);
        let finished = finish(&mut activity, start + 3 * S);
        assert_eq!(finished.outcome, Outcome::Busy, "turn {turn}");
        assert!(finished.seen.is_empty());
        // Dropped at once: no retry inside the burst, and no more frames.
        assert!(!activity.ready(start + 2 * SPACING_US));
        assert!(!activity.retreat().warn(), "turn {turn}");
        assert!(activity.ready(start + EVERY_US));
    }
    mock.wait("two sockets closed", |seen| seen.closed == 2);
    mock.seen(|seen| {
        assert_eq!(seen.connections, 2);
        // The config and the first frame, answered busy; the second frame,
        // already handed over, is never sent.
        assert!(seen.plain.iter().all(|plain| plain.len() == 2));
        assert_eq!(seen.frames.len(), 2);
    });
    // The third in a row: the bursts retreat a minute, ±20 %, and warn.
    burst(&mut activity, 120 * S, 1);
    assert_eq!(finish(&mut activity, 121 * S).outcome, Outcome::Busy);
    assert!(activity.retreat().warn());
    let until = activity.retreat().until_us().unwrap();
    assert!((121 * S + 48 * S..=121 * S + 72 * S).contains(&until));
    assert!(!activity.ready(until.max(180 * S) - 1));
    assert!(activity.ready(until.max(180 * S)));
    // A fourth: the wait doubles.
    let start = until.max(180 * S);
    burst(&mut activity, start, 1);
    assert_eq!(finish(&mut activity, start + S).outcome, Outcome::Busy);
    let until = activity.retreat().until_us().unwrap();
    assert!((start + S + 96 * S..=start + S + 144 * S).contains(&until));
    assert!(!activity.ready(until - 1));
    // An answered burst ends the retreat and the warning.
    burst(&mut activity, until, 1);
    activity.end_burst();
    assert_eq!(finish(&mut activity, until + S).outcome, Outcome::Answered);
    assert!(!activity.retreat().warn());
}

impl Activity {
    /// End the burst under way after the frames taken.
    fn end_burst(&mut self) {
        if let Some(running) = self.running.as_mut() {
            running.burst.end();
        }
    }
}

#[test]
fn a_refused_connection_or_a_server_error_retreats_at_once() {
    for (script, failure) in [
        (Script::Refuse(503), Failure::Refused(503)),
        (Script::Answer(vec![Reply::Error]), Failure::Server),
    ] {
        let mock = Mock::start(vec![script]);
        let mut activity = Activity::new(mock.endpoint(), pace());
        burst(&mut activity, 0, 1);
        let finished = finish(&mut activity, S);
        assert_eq!(finished.outcome, Outcome::Failed(failure));
        assert!(activity.retreat().warn(), "{failure:?}");
        assert_eq!(activity.last_failure(), Some(failure));
        assert!(!activity.ready(S + 48 * S - 1));
        wait_wiped(&activity, 1);
    }
    // Nothing listening.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut endpoint = Endpoint::new(
        &format!("http://{}", closed.local_addr().unwrap()),
        "test-key",
    )
    .unwrap();
    drop(closed);
    endpoint.public_key = Some(public_key_from_pem(PUBLIC).unwrap());
    let mut activity = Activity::new(endpoint, pace());
    burst(&mut activity, 0, 1);
    assert_eq!(
        finish(&mut activity, S).outcome,
        Outcome::Failed(Failure::Connect)
    );
    assert!(activity.retreat().warn());
}

#[test]
fn only_the_cameras_own_jpeg_of_at_least_320_by_240_goes_scaled_to_it() {
    use crate::scale::tests::{camera_jpeg, decoded};
    let mock = Mock::start(vec![Script::Answer(vec![Reply::Labels(vec![])])]);
    let mut activity = Activity::new(mock.endpoint(), pace());
    let sized = |width: u32, height: u32| {
        let mut frame = meta(grant(1));
        (frame.width, frame.height) = (width, height);
        frame
    };
    let refused = |change: fn(&mut FrameMeta)| {
        let mut frame = sized(640, 480);
        change(&mut frame);
        frame
    };
    for (frame, bytes) in [
        (sized(160, 120), camera_jpeg(160, 120)),
        (refused(|m| m.format = *b"YUYV"), camera_jpeg(640, 480)),
        (
            refused(|m| {
                m.derived = Some(lamp_camera::frames::DerivedImage {
                    source_width: 1280,
                    source_height: 960,
                    quality: 80,
                })
            }),
            camera_jpeg(640, 480),
        ),
        // Said 640×480, but not a JPEG that decodes.
        (sized(640, 480), jpeg(1)),
    ] {
        activity.frame(0, &frame, &bytes);
    }
    assert_eq!(activity.counts().wrong_frame, 4);
    assert!(!activity.sending(), "no burst for them");
    assert_eq!(activity.wiped(), 0, "never copied");
    assert_eq!(mock.seen(|seen| seen.connections), 0);
    // The lamp's camera at 1280×960, V1's 640×480, and lamp-eyes' 320×240
    // as it came: all sent at 320×240.
    let small = camera_jpeg(320, 240);
    for (n, (frame, bytes)) in [
        (sized(1280, 960), camera_jpeg(1280, 960)),
        (sized(640, 480), camera_jpeg(640, 480)),
        (sized(320, 240), small.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        activity.frame(n as u64 * SPACING_US, &frame, &bytes);
    }
    activity.end_burst();
    assert_eq!(finish(&mut activity, S).outcome, Outcome::Answered);
    assert_eq!(activity.counts().wrong_frame, 4);
    let frames = mock.seen(|seen| seen.frames.clone());
    assert_eq!(frames.len(), 3);
    for sent in &frames {
        assert_eq!(decoded(sent).0, (320, 240));
    }
    assert_eq!(frames[2], small, "320×240 goes as it came");
    wait_wiped(&activity, 3);
}
