//! The door end to end on localhost: TLS pinned to the lamp's own
//! certificate, the WebSocket at `/v1`, a code from the owner's admin
//! socket, pair, hello, an action, a lease lapsing and its event.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures_util::{SinkExt as _, StreamExt as _};
use lamp_control::{cert, pairing, server, session};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
use tokio_tungstenite::tungstenite::Message;

struct Lamp {
    address: std::net::SocketAddr,
    admin: std::path::PathBuf,
    identity: cert::Identity,
    dir: std::path::PathBuf,
    shared: Arc<server::Shared>,
}

async fn lamp(name: &str) -> Lamp {
    lamp_with(name, Box::new(session::Unrouted)).await
}

async fn lamp_with(name: &str, parts: Box<dyn session::Parts + Send>) -> Lamp {
    lamp_built(name, parts, |door, _| door).await
}

async fn lamp_built(
    name: &str,
    parts: Box<dyn session::Parts + Send>,
    build: impl FnOnce(session::Door, &std::path::Path) -> session::Door,
) -> Lamp {
    let dir = std::env::temp_dir().join(format!("lamp-control-door-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let identity = cert::load_or_make(&dir, "lamp-test").unwrap();
    let door = session::Door::new(
        "lamp-test",
        "Lamp",
        identity.pin_bytes(),
        pairing::Clients::default(),
        Some(dir.join("clients.json")),
    );
    let door = build(door, &dir);
    let shared = server::Shared::new(door, parts, identity.pin.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let admin = dir.join("admin.sock");
    let owner = tokio::net::UnixListener::bind(&admin).unwrap();
    tokio::spawn(server::admin(owner, shared.clone()));
    tokio::spawn(server::serve(
        listener,
        server::acceptor(&identity).unwrap(),
        shared.clone(),
    ));
    Lamp {
        address,
        admin,
        identity,
        dir,
        shared,
    }
}

impl Drop for Lamp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

/// A client trusting only the lamp's own certificate.
async fn connect(lamp: &Lamp, path: &str) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(
            lamp.identity.cert.clone(),
        ))
        .unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::net::TcpStream::connect(lamp.address).await.unwrap();
    let name = rustls::pki_types::ServerName::try_from("lamp-test.local").unwrap();
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .unwrap();
    let url = format!("wss://lamp-test.local:{}{path}", lamp.address.port());
    tokio_tungstenite::client_async(url, tls)
        .await
        .map(|(socket, _)| socket)
}

async fn next(socket: &mut Socket) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a frame in time");
        match message {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            other => panic!("{other:?}"),
        }
    }
}

async fn send(socket: &mut Socket, frame: Value) -> Value {
    socket.send(Message::text(frame.to_string())).await.unwrap();
    next(socket).await
}

async fn admin(lamp: &Lamp, asked: Value) -> Value {
    let stream = tokio::net::UnixStream::connect(&lamp.admin).await.unwrap();
    let (read, mut write) = stream.into_split();
    write
        .write_all(format!("{asked}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::io::BufReader::new(read)
        .read_line(&mut line)
        .await
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn a_computer_pairs_says_hello_and_is_answered_over_tls() {
    let lamp = lamp("pair").await;
    let keys = Ed25519KeyPair::from_seed_unchecked(&[3; 32]).unwrap();
    let key: [u8; 32] = keys.public_key().as_ref().try_into().unwrap();
    let issued = admin(&lamp, json!({"code": {"grants": ["body", "light"]}})).await;
    let code = issued["code"].as_str().unwrap().to_owned();
    // The pin the app shows with the code is the certificate the TLS serves.
    assert_eq!(issued["pin"], lamp.identity.pin);
    assert_eq!(
        issued["shown"],
        json!(["can move the lamp's body", "can change the light"])
    );

    let mut socket = connect(&lamp, "/v1").await.unwrap();
    let welcome = next(&mut socket).await;
    assert_eq!(welcome["ev"], "welcome");
    let nonce: [u8; 32] = pairing::bytes_of(welcome["nonce"].as_str().unwrap()).unwrap();
    // The computer checked the TLS certificate against this pin before
    // sending anything (the client config trusts only that certificate).
    let pin: [u8; 32] = (0..32)
        .map(|at| {
            u8::from_str_radix(&issued["pin"].as_str().unwrap()[at * 2..at * 2 + 2], 16).unwrap()
        })
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap();
    let proof = pairing::pair_proof(&code, &pin, &key, &nonce);
    let paired = send(
        &mut socket,
        json!({"id": 1, "op": "pair", "name": "test laptop", "key": STANDARD.encode(key), "proof": STANDARD.encode(proof)}),
    )
    .await;
    assert_eq!(paired["ok"], true, "{paired}");
    let client = paired["client"].as_str().unwrap().to_owned();
    let sig = keys.sign(&[pairing::HELLO_CONTEXT, &nonce].concat());
    let hello = send(
        &mut socket,
        json!({"id": 2, "op": "hello", "client": client, "sig": STANDARD.encode(sig)}),
    )
    .await;
    assert_eq!(hello["grants"], json!(["body", "light"]));
    // Until the parts are routed, an action is refused with a why.
    let gesture = send(
        &mut socket,
        json!({"id": 3, "op": "gesture", "name": "nod"}),
    )
    .await;
    assert_eq!(gesture["error"]["code"], "refused");
    // A short lease lapses, and its holder is told.
    let take = send(
        &mut socket,
        json!({"id": 4, "op": "lease.take", "parts": ["light"], "for_ms": 300}),
    )
    .await;
    assert_eq!(take["lease"], "l_1");
    let lapsed = next(&mut socket).await;
    assert_eq!(
        lapsed,
        json!({"ev": "lease", "lease": "l_1", "state": "lost", "why": "expired"})
    );
    let held = send(
        &mut socket,
        json!({"id": 5, "op": "lease.take", "parts": ["light"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(held["lease"], "l_2");
    // The owner sees the computer, and a revoke closes it out.
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert_eq!(listed["clients"][0]["name"], "test laptop");
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await,
        json!({"revoked": true, "saved": true})
    );
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_2", "state": "lost", "why": "revoked"})
    );
    // Closed at once while it only listens, sending nothing
    // (harness-reviewer's P2 on #3778).
    let closed = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                other => return other,
            }
        }
    })
    .await
    .expect("closed at once, not at its next frame");
    assert!(
        matches!(closed, None | Some(Ok(Message::Close(_))) | Some(Err(_))),
        "{closed:?}"
    );
    // The pairing was kept on disk, 0600, and is gone after the revoke.
    let kept = pairing::Clients::load(&lamp.dir.join("clients.json")).unwrap();
    assert!(kept.clients.is_empty());
}

#[tokio::test]
async fn another_path_or_an_oversized_frame_is_refused() {
    let lamp = lamp("refuse").await;
    assert!(connect(&lamp, "/v2").await.is_err());
    let mut socket = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut socket).await;
    let huge = "x".repeat(server::FRAME_MAX + 1);
    let _ = socket.send(Message::text(huge)).await;
    let closed = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("closed in time");
    assert!(!matches!(closed, Some(Ok(Message::Text(_)))), "{closed:?}");
}

/// A computer paired with `grants` and said hello, as the SDK would.
async fn paired(lamp: &Lamp, grants: Value, seed: u8) -> Socket {
    let keys = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
    let key: [u8; 32] = keys.public_key().as_ref().try_into().unwrap();
    let issued = admin(lamp, json!({"code": {"grants": grants}})).await;
    let code = issued["code"].as_str().unwrap().to_owned();
    let mut socket = connect(lamp, "/v1").await.unwrap();
    let welcome = next(&mut socket).await;
    let nonce: [u8; 32] = pairing::bytes_of(welcome["nonce"].as_str().unwrap()).unwrap();
    let proof = pairing::pair_proof(&code, &lamp.identity.pin_bytes(), &key, &nonce);
    let pair = json!({"id": 1, "op": "pair", "name": "test", "key": STANDARD.encode(key), "proof": STANDARD.encode(proof)});
    let paired = send(&mut socket, pair).await;
    let client = paired["client"]
        .as_str()
        .unwrap_or_else(|| panic!("{paired}"))
        .to_owned();
    let sig = keys.sign(&[pairing::HELLO_CONTEXT, &nonce].concat());
    let hello = send(
        &mut socket,
        json!({"id": 2, "op": "hello", "client": client, "sig": STANDARD.encode(sig)}),
    )
    .await;
    assert_eq!(hello["ok"], true, "{hello}");
    socket
}

/// The Reviewer-lead's P2 on #3778: silent TCP peers that never send a TLS
/// hello hold an address's places only until the handshake's time is up;
/// past an address's bound a new connection is closed at once, and the
/// owner gets in once the silent ones are gone.
#[tokio::test]
async fn silent_peers_are_dropped_at_the_handshake_time_and_never_hold_the_door() {
    let lamp = lamp("silent").await;
    lamp.shared.set_limits(server::Limits {
        handshake: Duration::from_millis(300),
        total: 32,
        per_peer: 2,
        quiet: server::QUIET_MAX,
        hello: server::HELLO_WITHIN,
    });
    let _one = tokio::net::TcpStream::connect(lamp.address).await.unwrap();
    let _two = tokio::net::TcpStream::connect(lamp.address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Both places of 127.0.0.1 are held: the TLS handshake fails at once.
    let refused = tokio::time::timeout(Duration::from_secs(2), async {
        let tcp = tokio::net::TcpStream::connect(lamp.address).await.unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(
                lamp.identity.cert.clone(),
            ))
            .unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from("lamp-test.local").unwrap();
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
    })
    .await
    .expect("refused at once, not left hanging");
    assert!(refused.is_err());
    // Past the handshake time the silent ones are gone and the owner gets in.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut socket = connect(&lamp, "/v1").await.unwrap();
    assert_eq!(next(&mut socket).await["ev"], "welcome");
}

/// blue's P2 on #3778: a client that only listens, holding no lease and
/// saying nothing, stays open while its keepalive pings come; one that goes
/// silent is closed after the quiet time.
#[tokio::test]
async fn a_listener_kept_alive_by_pings_stays_and_a_silent_one_goes() {
    let lamp = lamp("pings").await;
    lamp.shared.set_limits(server::Limits {
        handshake: server::HANDSHAKE_WITHIN,
        total: 32,
        per_peer: 4,
        quiet: Duration::from_millis(400),
        hello: server::HELLO_WITHIN,
    });
    let mut pinging = paired(&lamp, json!(["light"]), 5).await;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        pinging
            .send(Message::Ping(Vec::new().into()))
            .await
            .unwrap();
    }
    let answer = send(
        &mut pinging,
        json!({"id": 9, "op": "get", "state": ["lamp"]}),
    )
    .await;
    assert_eq!(answer["ok"], true, "{answer}");
    let mut silent = paired(&lamp, json!(["light"]), 6).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match silent.next().await {
                Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => continue,
                other => return other,
            }
        }
    })
    .await
    .expect("closed in time");
    assert!(!matches!(closed, Some(Ok(Message::Text(_)))), "{closed:?}");
}

/// The Reviewer-lead's P3 on #3780: a revoke the file can't take yet is in
/// effect, and the owner is told it isn't saved.
#[tokio::test]
async fn a_revoke_the_file_cannot_take_yet_says_so() {
    let lamp = lamp("unsaved").await;
    let _socket = paired(&lamp, json!(["light"]), 9).await;
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    std::fs::create_dir(lamp.dir.join("clients.json.new")).unwrap();
    let answer = admin(&lamp, json!({"revoke": client})).await;
    assert_eq!(answer["revoked"], true, "{answer}");
    assert_eq!(answer["saved"], false, "{answer}");
    assert!(
        answer["error"].as_str().unwrap().contains("restart"),
        "{answer}"
    );
    std::fs::remove_dir(lamp.dir.join("clients.json.new")).unwrap();
}

/// Fourteen's P3 on #3778: frames before `hello` don't move its deadline;
/// a client that never says it is closed at the hello time from its welcome.
#[tokio::test]
async fn frames_before_hello_buy_no_time() {
    let lamp = lamp("hello-time").await;
    lamp.shared.set_limits(server::Limits {
        handshake: server::HANDSHAKE_WITHIN,
        total: 32,
        per_peer: 4,
        quiet: server::QUIET_MAX,
        hello: Duration::from_millis(500),
    });
    let mut socket = connect(&lamp, "/v1").await.unwrap();
    assert_eq!(next(&mut socket).await["ev"], "welcome");
    let started = std::time::Instant::now();
    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if socket
                .send(Message::text(
                    json!({"id": 1, "op": "get", "state": ["lamp"]}).to_string(),
                ))
                .await
                .is_err()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            while let Ok(Some(frame)) =
                tokio::time::timeout(Duration::from_millis(1), socket.next()).await
            {
                if !matches!(frame, Ok(Message::Text(_))) {
                    return;
                }
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "still open after 3 s of frames before hello"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        started.elapsed()
    );
}

/// Parts that keep each client's `stop`.
struct Stops(Arc<std::sync::Mutex<Vec<String>>>);

impl session::Parts for Stops {
    fn act(
        &mut self,
        _: &str,
        _: u64,
        _: &lamp_control::protocol::Op,
        _: u64,
    ) -> Result<Value, lamp_control::protocol::Error> {
        Ok(Value::Null)
    }
    fn state(&self, _: lamp_control::protocol::StateKind) -> Value {
        Value::Null
    }
    fn stop(&mut self, client: &str, _: u64) {
        self.0.lock().unwrap().push(client.to_owned());
    }
}

/// The Reviewer-lead's P3 on #3736 and #3778: a computer's last connection
/// closing stops what it asked for, as its `stop` would; so does a revoke.
#[tokio::test]
async fn the_last_connection_closing_or_a_revoke_stops_its_actions() {
    let stops = Arc::new(std::sync::Mutex::new(Vec::new()));
    let lamp = lamp_with("stops", Box::new(Stops(stops.clone()))).await;
    let mut first = paired(&lamp, json!(["light"]), 7).await;
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    first.close(None).await.unwrap();
    for _ in 0..100 {
        if !stops.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(*stops.lock().unwrap(), std::slice::from_ref(&client));
    // One pairing try a second, door-wide.
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let _second = paired(&lamp, json!(["light"]), 8).await;
    let other = admin(&lamp, json!({"clients": true})).await["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .find(|id| *id != client)
        .unwrap();
    assert_eq!(
        admin(&lamp, json!({"revoke": other})).await,
        json!({"revoked": true, "saved": true})
    );
    assert_eq!(stops.lock().unwrap().last(), Some(&other));
}

#[tokio::test]
async fn the_bus_reaches_a_subscriber_as_events_phrases_only_and_lease_signals() {
    use lamp_control::protocol::{EventKind, LeaseWhy};
    use lamp_control::watch::Out;
    let lamp = lamp("events").await;
    let mut socket = paired(&lamp, json!(["look", "hear", "light"]), 4).await;
    let subscribed = send(
        &mut socket,
        json!({"id": 3, "op": "subscribe", "events": ["person.arrived", "phrase.heard"], "phrases": ["lights please"]}),
    )
    .await;
    assert_eq!(subscribed["ok"], true, "{subscribed}");
    // Not subscribed: never sent.
    lamp.shared
        .deliver(vec![Out::Event(EventKind::Touch, json!({"kind": "tap"}))]);
    lamp.shared.deliver(vec![Out::Event(
        EventKind::PersonArrived,
        json!({"count": 1, "near": 0.3}),
    )]);
    let arrived = next(&mut socket).await;
    assert_eq!(arrived["ev"], "person.arrived");
    assert_eq!(arrived["count"], 1);
    assert!(arrived["at"].as_str().unwrap().ends_with('Z'));
    // The words heard never leave the lamp: only the phrase.
    lamp.shared
        .deliver(vec![Out::Heard("ok the lights please now".into())]);
    let heard = next(&mut socket).await;
    assert_eq!(heard["ev"], "phrase.heard");
    assert_eq!(heard["phrase"], "lights please");
    assert!(!heard.to_string().contains("now"), "{heard}");
    // A conversation suspends its lease; resting holds it again.
    let take = send(
        &mut socket,
        json!({"id": 4, "op": "lease.take", "parts": ["light"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1");
    lamp.shared
        .deliver(vec![Out::Suspend(None, LeaseWhy::Conversation)]);
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "suspended", "why": "conversation"})
    );
    lamp.shared.deliver(vec![Out::Resume]);
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "held"})
    );
}

/// Part 6: while a computer holds `body`, the door holds the body's own
/// lease; the body's `lost` ends the computer's lease with the same why
/// (`lapsed` as `expired`), and releasing `body` lets the body go.
#[tokio::test]
async fn a_body_lease_holds_the_body_and_the_bodys_end_is_told() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let lamp = lamp("body-lease").await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        lamp.shared.events(),
        tokio::sync::mpsc::unbounded_channel().1,
    ));
    let mut socket = paired(&lamp, json!(["body"]), 11).await;
    let take = send(
        &mut socket,
        json!({"id": 3, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1", "{take}");
    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
        .await
        .expect("the door holds the body")
        .unwrap();
    let (read, mut write) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(read).lines();
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        r#"{"hold":{"for_ms":30000}}"#
    );
    write.write_all(b"{\"lease\":\"held\"}\n").await.unwrap();
    // A hand on the head: the body's end is the computer's.
    write
        .write_all(b"{\"lease\":\"lost\",\"why\":\"hand\"}\n")
        .await
        .unwrap();
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "lost", "why": "hand"})
    );
    // Nobody holds `body` now: the door lets the body go and closes.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .expect("let go in time")
            .unwrap()
            .as_deref(),
        Some(r#"{"let_go":true}"#)
    );
    assert_eq!(lines.next_line().await.unwrap(), None);
}

/// A computer's `look` (#3504, Nine on #3997): on the body's lease
/// connection, answered by the body once, and told as the look's `done`
/// with the angles reached; one still owed when the door lets go is cut
/// short, `released`.
#[tokio::test]
async fn a_look_goes_on_the_bodys_lease_and_its_end_is_the_computers_done() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let (looks, looked) = tokio::sync::mpsc::unbounded_channel();
    let parts =
        lamp_control::parts::Lamp::new(None, None, None, "boot".into(), Arc::new(|_, _| {}))
            .looking(looks);
    let lamp = lamp_with("body-look", Box::new(parts)).await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        lamp.shared.events(),
        looked,
    ));
    let mut socket = paired(&lamp, json!(["body"]), 13).await;
    // No lease: refused before it reaches the body.
    let refused = send(&mut socket, json!({"id": 2, "op": "look", "at": "ahead"})).await;
    assert_eq!(refused["error"]["code"], "no_lease", "{refused}");
    let take = send(
        &mut socket,
        json!({"id": 3, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1", "{take}");
    // Asked at once, before the door's check takes the body's hold: it
    // waits for the hold, not dropped (Nine's P3 on #4023).
    let answer = send(
        &mut socket,
        json!({"id": 4, "op": "look", "at": {"yaw": -10, "pitch": 5}}),
    )
    .await;
    assert_eq!(answer["to"], "body", "{answer}");
    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
        .await
        .expect("the door holds the body")
        .unwrap();
    let (read, mut write) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(read).lines();
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        r#"{"hold":{"for_ms":30000}}"#
    );
    write.write_all(b"{\"lease\":\"held\"}\n").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&lines.next_line().await.unwrap().unwrap()).unwrap(),
        json!({"look": {"yaw": -10.0, "pitch": 5.0}})
    );
    write
        .write_all(b"{\"look\":\"there\",\"yaw\":-10.0,\"pitch\":5.0,\"clamped\":false}\n")
        .await
        .unwrap();
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "done", "re": 4, "how": "finished", "yaw": -10.0, "pitch": 5.0, "clamped": false})
    );
    // Another, past the motion rate's 2 s, then the computer lets go before
    // the body answers: cut short.
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    let answer = send(&mut socket, json!({"id": 5, "op": "look", "at": "ahead"})).await;
    assert_eq!(answer["to"], "body", "{answer}");
    assert_eq!(
        lines.next_line().await.unwrap().as_deref(),
        Some(r#"{"look":"ahead"}"#)
    );
    send(
        &mut socket,
        json!({"id": 6, "op": "lease.release", "lease": "l_1"}),
    )
    .await;
    let mut done = next(&mut socket).await;
    while done["ev"] != "done" {
        done = next(&mut socket).await;
    }
    assert_eq!(
        done,
        json!({"ev": "done", "re": 5, "how": "cut_short", "why": "released"})
    );
}

/// The Reviewer-lead's P2 on #4023: a look asked right after `lease.take`,
/// its lease released before the door's check, ends for its asker and is
/// never sent under a later hold.
#[tokio::test]
async fn a_waiting_look_ends_with_its_lease_and_never_goes_under_the_next() {
    use tokio::io::AsyncBufReadExt as _;
    let (looks, looked) = tokio::sync::mpsc::unbounded_channel();
    let parts =
        lamp_control::parts::Lamp::new(None, None, None, "boot".into(), Arc::new(|_, _| {}))
            .looking(looks);
    let lamp = lamp_with("body-look-ended", Box::new(parts)).await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        lamp.shared.events(),
        looked,
    ));
    let mut socket = paired(&lamp, json!(["body"]), 14).await;
    let take = |id: u64| json!({"id": id, "op": "lease.take", "parts": ["body"], "for_ms": 30000});
    assert_eq!(send(&mut socket, take(3)).await["lease"], "l_1");
    send(&mut socket, json!({"id": 4, "op": "look", "at": "ahead"})).await;
    send(
        &mut socket,
        json!({"id": 5, "op": "lease.release", "lease": "l_1"}),
    )
    .await;
    // However the door's 250 ms check fell, the look ends for its asker:
    // dropped before the hold, or cut short when the door let go.
    let mut done = next(&mut socket).await;
    while done["ev"] != "done" {
        done = next(&mut socket).await;
    }
    assert_eq!(done["re"], 4, "{done}");
    assert!(
        done == json!({"ev": "done", "re": 4, "how": "dropped", "why": "the body lease ended"})
            || done == json!({"ev": "done", "re": 4, "how": "cut_short", "why": "released"}),
        "{done}"
    );
    // A later hold carries only its own lines.
    let accepted = tokio::time::timeout(Duration::from_millis(600), body.accept()).await;
    drop(accepted);
    assert_eq!(send(&mut socket, take(6)).await["lease"], "l_2");
    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
        .await
        .expect("held again")
        .unwrap();
    let (read, _write) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(read).lines();
    assert_eq!(
        lines.next_line().await.unwrap().as_deref(),
        Some(r#"{"hold":{"for_ms":30000}}"#)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(400), lines.next_line())
            .await
            .is_err(),
        "nothing after the hold"
    );
}

/// Sixth's and Fourteen's P3s on #3900: a body that restarts (its end of the
/// socket closes with nothing said) is held again at the next check, not at
/// the next renewal 10 s on; and a hold the body refuses ends the computer's
/// `body` lease, as `released`, so it never thinks the lamp stands back
/// when it doesn't.
#[tokio::test]
async fn a_restarted_body_is_held_again_at_once_and_a_refused_hold_is_told() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let lamp = lamp("body-lease-again").await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        lamp.shared.events(),
        tokio::sync::mpsc::unbounded_channel().1,
    ));
    let mut socket = paired(&lamp, json!(["body"]), 12).await;
    let take = send(
        &mut socket,
        json!({"id": 3, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1", "{take}");
    let accept = || async {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
            .await
            .expect("the door holds the body")
            .unwrap();
        let (read, write) = stream.into_split();
        let mut lines = tokio::io::BufReader::new(read).lines();
        assert_eq!(
            lines.next_line().await.unwrap().unwrap(),
            r#"{"hold":{"for_ms":30000}}"#
        );
        (lines, write)
    };
    // The body restarts: its end closes, saying nothing.
    let first = accept().await;
    let restarted = std::time::Instant::now();
    drop(first);
    let (_lines, mut write) = accept().await;
    assert!(
        restarted.elapsed() < Duration::from_secs(2),
        "held again in {:?}, not at the next renewal",
        restarted.elapsed()
    );
    // The new connection's hold is refused: the computer is told.
    write
        .write_all(b"{\"lease\":\"refused\",\"why\":\"one of hold or let_go\"}\n")
        .await
        .unwrap();
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "lost", "why": "released"})
    );
}

/// Fourteen's P3 on #3915: a late line from a connection the door dropped
/// never ends a newer holder's lease. A computer takes `body`, releases it
/// (the door lets go), takes it again; the old connection then says a late
/// `lost`, and the new lease stands.
#[tokio::test]
async fn a_late_line_from_a_dropped_body_connection_never_ends_a_newer_lease() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let lamp = lamp("body-lease-late").await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        lamp.shared.events(),
        tokio::sync::mpsc::unbounded_channel().1,
    ));
    let mut socket = paired(&lamp, json!(["body"]), 13).await;
    let take = |id: u64| json!({"id": id, "op": "lease.take", "parts": ["body"], "for_ms": 30000});
    let accept = || async {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
            .await
            .expect("the door holds the body")
            .unwrap();
        let (read, write) = stream.into_split();
        let mut lines = tokio::io::BufReader::new(read).lines();
        assert_eq!(
            lines.next_line().await.unwrap().unwrap(),
            r#"{"hold":{"for_ms":30000}}"#
        );
        (lines, write)
    };
    assert_eq!(send(&mut socket, take(3)).await["lease"], "l_1");
    let (mut old_lines, mut old_write) = accept().await;
    let released = send(
        &mut socket,
        json!({"id": 4, "op": "lease.release", "lease": "l_1"}),
    )
    .await;
    assert_eq!(released["ok"], true, "{released}");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), old_lines.next_line())
            .await
            .expect("let go in time")
            .unwrap()
            .as_deref(),
        Some(r#"{"let_go":true}"#)
    );
    assert_eq!(send(&mut socket, take(5)).await["lease"], "l_2");
    let _new = accept().await;
    // The old connection's late word.
    let _ = old_write
        .write_all(b"{\"lease\":\"lost\",\"why\":\"hand\"}\n")
        .await;
    let told = tokio::time::timeout(Duration::from_millis(1_200), next(&mut socket)).await;
    assert!(
        told.is_err(),
        "the new lease was ended by a late line: {told:?}"
    );
}

/// What the stand-in manage helper saw: each request as it came, and the
/// kept package and `push.json` as it read them for an install.
#[derive(Default)]
struct Helper {
    asked: Vec<Value>,
    kept: Option<(Vec<u8>, Value)>,
}

/// A stand-in for root's manage helper, serving each connection on its own
/// thread (nothing says the real one is serial), answering as lamp-skills
/// would, `{"done": …}` or `{"refused": WHY}`. An install reads the kept
/// files from `state`, as the helper does; an install and a list answer
/// after `install_takes`.
fn manage_helper(
    socket: &std::path::Path,
    state: &std::path::Path,
    install_takes: Duration,
) -> Arc<std::sync::Mutex<Helper>> {
    use std::io::{BufRead as _, Write as _};
    let seen = Arc::new(std::sync::Mutex::new(Helper::default()));
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    let keep = seen.clone();
    let state = state.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let keep = keep.clone();
            let state = state.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                keep.lock().unwrap().asked.push(request.clone());
                let answer = if request.get("install_pushed").is_some() {
                    let package = std::fs::read(state.join("push/package.tar")).unwrap();
                    let said: Value = serde_json::from_slice(
                        &std::fs::read(state.join("push/push.json")).unwrap(),
                    )
                    .unwrap();
                    keep.lock().unwrap().kept = Some((package, said));
                    std::thread::sleep(install_takes);
                    json!({"done": {"name": "desk-wave", "version": "0.1.0", "on": true, "held": null}})
                } else if request.get("list_pushed_by").is_some() {
                    std::thread::sleep(install_takes);
                    json!({"done": [{"name": "desk-wave", "version": "0.1.0", "on": true}]})
                } else if request["remove_pushed"]["name"] == "desk-wave" {
                    json!({"done": "removed desk-wave"})
                } else if request.get("remove_pushed").is_some() {
                    json!({"refused": "no skill of this computer's has that name"})
                } else {
                    json!({"done": "turned off"})
                };
                let _ = writeln!(stream, "{answer}");
            });
        }
    });
    seen
}

/// A package of `size` bytes, its SHA-256, and its parts as `skill.push`
/// frames from id `first`.
fn package_frames(size: u32, first: usize) -> (Vec<u8>, String, Vec<Value>) {
    let package: Vec<u8> = (0..size).map(|n| (n % 251) as u8).collect();
    let sha256: String = {
        use sha2::Digest as _;
        sha2::Sha256::digest(&package)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let chunks: Vec<&[u8]> = package.chunks(36 * 1024).collect();
    let frames = chunks
        .iter()
        .enumerate()
        .map(|(n, chunk)| {
            json!({"id": first + n, "op": "skill.push", "name": "desk-wave", "sha256": sha256,
                "size": package.len(), "part": n, "parts": chunks.len(), "data": STANDARD.encode(chunk)})
        })
        .collect();
    (package, sha256, frames)
}

/// Waits until the helper has been asked `request`.
async fn until_asked(helper: &std::sync::Mutex<Helper>, request: &Value) {
    for _ in 0..150 {
        if helper.lock().unwrap().asked.contains(request) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never asked {request}: {:?}", helper.lock().unwrap().asked);
}

/// #3869's wire: a `skills` grant pushes a package in parts, kept with
/// `push.json` for the helper and installed by it; list and remove go to
/// the helper as this computer's own; a revoke turns its pushed skills off.
#[tokio::test]
async fn a_skill_is_pushed_in_parts_installed_by_the_helper_listed_removed_and_turned_off_by_a_revoke()
 {
    let lamp = lamp_built("push", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(&lamp.dir.join("manage.sock"), &lamp.dir, Duration::ZERO);
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    // Without the grant: refused, as every op is.
    let mut without = paired(&lamp, json!(["body"]), 41).await;
    let refused = send(&mut without, json!({"id": 3, "op": "skill.list"})).await;
    assert_eq!(refused["error"]["code"], "no_grant", "{refused}");
    drop(without);
    // One pairing try a second.
    tokio::time::sleep(Duration::from_millis(1_100)).await;

    let mut socket = paired(&lamp, json!(["body", "light", "skills"]), 42).await;
    let (package, sha256, frames) = package_frames(60_000, 10);
    assert_eq!(frames.len(), 2);
    let got = send(&mut socket, frames[0].clone()).await;
    assert_eq!(got, json!({"re": 10, "ok": true, "got": 1}));
    let installed = send(&mut socket, frames[1].clone()).await;
    assert_eq!(installed["re"], 11, "{installed}");
    assert_eq!(installed["skill"]["name"], "desk-wave", "{installed}");
    // Kept for the helper, as it read them: the bytes, and push.json with
    // this computer and its grants now; then removed once it answered.
    let (kept, said) = helper
        .lock()
        .unwrap()
        .kept
        .clone()
        .expect("read by the helper");
    assert_eq!(kept, package);
    assert_eq!(said["sha256"], sha256);
    assert_eq!(said["grants"], json!(["body", "light", "skills"]));
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let at = said["at_unix_ms"].as_u64().unwrap();
    assert!(at <= now_ms && now_ms - at < 10_000, "{said}");
    assert!(!lamp.dir.join("push/package.tar").exists());
    assert!(!lamp.dir.join("push/push.json").exists());
    let client = said["client"].as_str().unwrap().to_owned();
    // A second push within 10 s is over its rate, before any byte is kept.
    let rate = send(&mut socket, frames[0].clone()).await;
    assert_eq!(rate["error"]["code"], "rate", "{rate}");

    let listed = send(&mut socket, json!({"id": 20, "op": "skill.list"})).await;
    assert_eq!(listed["skills"][0]["name"], "desk-wave", "{listed}");
    let removed = send(
        &mut socket,
        json!({"id": 21, "op": "skill.remove", "name": "desk-wave"}),
    )
    .await;
    assert_eq!(removed, json!({"re": 21, "ok": true}));
    let not_its = send(
        &mut socket,
        json!({"id": 22, "op": "skill.remove", "name": "other"}),
    )
    .await;
    assert_eq!(not_its["error"]["code"], "refused", "{not_its}");
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await["revoked"],
        true
    );
    let off = json!({"off_pushed_by": client});
    until_asked(&helper, &off).await;
    assert_eq!(
        helper.lock().unwrap().asked,
        [
            json!({"install_pushed": sha256}),
            json!({"list_pushed_by": client}),
            json!({"remove_pushed": {"client": client, "name": "desk-wave"}}),
            json!({"remove_pushed": {"client": client, "name": "other"}}),
            off,
        ]
    );
}

/// Fourteen's P2 on #3910: a revoke while an install is with the helper.
/// The revoke's `off_pushed_by` can be served before the install finishes,
/// so the door sends it again once the install is answered: the last word
/// the helper has for that computer is always "off".
#[tokio::test]
async fn a_revoke_during_an_install_turns_the_skill_off_after_it() {
    let lamp = lamp_built("push-revoke", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::from_millis(1_500),
    );
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut socket = paired(&lamp, json!(["body", "skills"]), 44).await;
    let (_, sha256, frames) = package_frames(1_000, 10);
    socket
        .send(Message::text(frames[0].to_string()))
        .await
        .unwrap();
    let install = json!({"install_pushed": sha256});
    until_asked(&helper, &install).await;
    let client = helper.lock().unwrap().kept.clone().unwrap().1["client"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await["revoked"],
        true
    );
    let off = json!({"off_pushed_by": client});
    until_asked(&helper, &off).await;
    // The first off came while the install was still with the helper.
    tokio::time::sleep(Duration::from_millis(2_000)).await;
    let asked = helper.lock().unwrap().asked.clone();
    assert_eq!(asked, [install, off.clone(), off], "{asked:?}");
}

/// Fourteen's and Reviewer-two's P2 on #3910: revoked during an install,
/// then the same key paired again with fewer grants. The same id is back,
/// so "still paired" would have said the install was fine; the pairing's
/// generation says it isn't, and after the install the helper is held to
/// the new grants.
#[tokio::test]
async fn a_re_pair_during_an_install_holds_the_skill_to_the_new_grants_after_it() {
    let lamp = lamp_built("push-repair", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::from_millis(2_500),
    );
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut socket = paired(&lamp, json!(["body", "skills"]), 46).await;
    let (_, sha256, frames) = package_frames(1_000, 10);
    socket
        .send(Message::text(frames[0].to_string()))
        .await
        .unwrap();
    let install = json!({"install_pushed": sha256});
    until_asked(&helper, &install).await;
    let client = helper.lock().unwrap().kept.clone().unwrap().1["client"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await["revoked"],
        true
    );
    let off = json!({"off_pushed_by": client});
    until_asked(&helper, &off).await;
    // One pairing try a second; the same key, now with "light" only.
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let _again = paired(&lamp, json!(["light"]), 46).await;
    let held = json!({"grants_pushed_by": {"client": client, "grants": ["light"]}});
    until_asked(&helper, &held).await;
    let asked = helper.lock().unwrap().asked.clone();
    assert_eq!(asked, [install, off, held], "{asked:?}");
}

/// Reviewer-two's P2 on #3910: the same key paired again with fewer grants,
/// no revoke between, has its pushed skills held to the new grants; with
/// the same grants, nothing is asked.
#[tokio::test]
async fn a_re_pair_with_other_grants_holds_pushed_skills_to_them() {
    let lamp = lamp_built("push-grants", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(&lamp.dir.join("manage.sock"), &lamp.dir, Duration::ZERO);
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut first = paired(&lamp, json!(["body", "skills"]), 47).await;
    let hello = send(&mut first, json!({"id": 2, "op": "skill.list"})).await;
    assert_eq!(hello["re"], 2, "{hello}");
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let _same = paired(&lamp, json!(["body", "skills"]), 47).await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let _fewer = paired(&lamp, json!(["skills"]), 47).await;
    let client = helper.lock().unwrap().asked[0]["list_pushed_by"]
        .as_str()
        .unwrap()
        .to_owned();
    let held = json!({"grants_pushed_by": {"client": client, "grants": ["skills"]}});
    until_asked(&helper, &held).await;
    let asked = helper.lock().unwrap().asked.clone();
    assert_eq!(
        asked,
        [json!({"list_pushed_by": client}), held],
        "{asked:?}"
    );
}

/// Reviewer-two's P2 on #3910: `skill.list` and `skill.remove` are bounded
/// before the helper is asked: one at a time for a computer (`busy`), and
/// 6 a minute (`rate`).
#[tokio::test]
async fn skill_requests_to_the_helper_are_one_at_a_time_and_rated() {
    let lamp = lamp_built("push-bound", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::from_millis(300),
    );
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut socket = paired(&lamp, json!(["skills"]), 48).await;
    socket
        .send(Message::text(
            json!({"id": 2, "op": "skill.list"}).to_string(),
        ))
        .await
        .unwrap();
    let busy = send(
        &mut socket,
        json!({"id": 3, "op": "skill.remove", "name": "desk-wave"}),
    )
    .await;
    assert_eq!(busy["re"], 3, "{busy}");
    assert_eq!(busy["error"]["code"], "busy", "{busy}");
    let listed = next(&mut socket).await;
    assert_eq!(listed["re"], 2, "{listed}");
    assert_eq!(listed["ok"], true, "{listed}");
    for id in 4..9 {
        let listed = send(&mut socket, json!({"id": id, "op": "skill.list"})).await;
        assert_eq!(listed["ok"], true, "{listed}");
    }
    let rate = send(&mut socket, json!({"id": 9, "op": "skill.list"})).await;
    assert_eq!(rate["error"]["code"], "rate", "{rate}");
    assert_eq!(
        rate["error"]["why"], "skill.list and skill.remove: 6 a minute",
        "{rate}"
    );
    // The refused ones never reached the helper.
    assert_eq!(helper.lock().unwrap().asked.len(), 6);
}

/// Reviewer-two's and harness-reviewer's P3s on #3910: with the helper
/// stalled, four computers' requests are with it and a fifth's is `busy`
/// before anything is asked, and `stop` is still answered at once.
#[tokio::test]
async fn a_stalled_helper_takes_four_requests_and_stop_still_answers() {
    let lamp = lamp_built("push-cap", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let helper = manage_helper(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::from_secs(3),
    );
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    // Five computers, all from this one address.
    lamp.shared.set_limits(server::Limits {
        per_peer: 8,
        ..server::Limits::default()
    });
    let mut sockets = Vec::new();
    for seed in 50..55 {
        if seed > 50 {
            // One pairing try a second.
            tokio::time::sleep(Duration::from_millis(1_100)).await;
        }
        sockets.push(paired(&lamp, json!(["skills"]), seed).await);
    }
    for socket in &mut sockets[..4] {
        socket
            .send(Message::text(
                json!({"id": 2, "op": "skill.list"}).to_string(),
            ))
            .await
            .unwrap();
    }
    for _ in 0..100 {
        if helper.lock().unwrap().asked.len() == 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let fifth = &mut sockets[4];
    let busy = send(fifth, json!({"id": 2, "op": "skill.list"})).await;
    assert_eq!(busy["error"]["code"], "busy", "{busy}");
    assert_eq!(
        busy["error"]["why"], "the lamp's skill helper is busy: try again",
        "{busy}"
    );
    let asked = std::time::Instant::now();
    let stopped = send(fifth, json!({"id": 3, "op": "stop"})).await;
    assert_eq!(stopped, json!({"re": 3, "ok": true}));
    assert!(
        asked.elapsed() < Duration::from_millis(500),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(helper.lock().unwrap().asked.len(), 4);
    // Each of the four is answered once the helper is.
    for socket in &mut sockets[..4] {
        let listed = next(socket).await;
        assert_eq!(listed["ok"], true, "{listed}");
    }
}

/// Fourteen's P3 on #3910: a change the helper gives no answer to is asked
/// again (after `RETRY_AFTER`), so a revoke's `off_pushed_by` isn't lost to
/// one dropped connection.
#[tokio::test]
async fn a_change_the_helper_does_not_answer_is_asked_again() {
    use std::io::{BufRead as _, Write as _};
    let lamp = lamp_built("push-retry", Box::new(session::Unrouted), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let socket_path = lamp.dir.join("manage.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    let asked = Arc::new(std::sync::Mutex::new(
        Vec::<(std::time::Instant, Value)>::new(),
    ));
    let keep = asked.clone();
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            keep.lock().unwrap().push((
                std::time::Instant::now(),
                serde_json::from_str(&line).unwrap(),
            ));
            // The first is dropped unanswered, as a helper that dies would.
            if n > 0 {
                let _ = writeln!(stream, "{}", json!({"done": {"off": []}}));
            }
        }
    });
    lamp.shared.set_manage_socket(socket_path);
    let _socket = paired(&lamp, json!(["skills"]), 56).await;
    let client = {
        let clients = admin(&lamp, json!({"clients": true})).await;
        clients["clients"][0]["id"].as_str().unwrap().to_owned()
    };
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await["revoked"],
        true
    );
    for _ in 0..250 {
        if asked.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let asked = asked.lock().unwrap().clone();
    let off = json!({"off_pushed_by": client});
    assert_eq!(
        asked
            .iter()
            .map(|(_, request)| request.clone())
            .collect::<Vec<_>>(),
        [off.clone(), off],
        "{asked:?}"
    );
    let waited = asked[1].0 - asked[0].0;
    assert!(waited >= server::RETRY_AFTER[0], "{waited:?}");
}

/// Without a manage socket, skill push is off on the lamp and says so.
#[tokio::test]
async fn skill_push_is_off_without_the_helper() {
    let lamp = lamp("push-off").await;
    let mut socket = paired(&lamp, json!(["skills"]), 43).await;
    let refused = send(&mut socket, json!({"id": 3, "op": "skill.list"})).await;
    assert_eq!(
        refused["error"],
        json!({"code": "refused", "why": session::PUSH_OFF})
    );
}
