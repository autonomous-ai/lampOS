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
    let client = send(&mut socket, pair).await["client"]
        .as_str()
        .unwrap()
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
