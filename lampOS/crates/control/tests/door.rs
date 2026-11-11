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
    lamp_made(name, move |_| parts, build).await
}

/// A lamp whose parts are made with the server's own events, so what they
/// tell later (a motion's `done`) reaches the computer.
async fn lamp_made(
    name: &str,
    parts: impl FnOnce(lamp_control::parts::Events) -> Box<dyn session::Parts + Send>,
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
    let shared = server::Shared::new_with(door, identity.pin.clone(), parts);
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

/// The next frame, waiting up to `within`.
async fn next_within(socket: &mut Socket, within: Duration) -> Value {
    loop {
        let message = tokio::time::timeout(within, socket.next())
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
    // The code's end by the wall clock, five minutes from now.
    let now_ms = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let until = issued["until_unix_ms"].as_u64().unwrap();
    assert!(
        until > now_ms + 290_000 && until <= now_ms + 300_000,
        "{until} against {now_ms}"
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
    // Seconds as the file keeps them, and the same in milliseconds.
    let paired_at = listed["clients"][0]["paired_at"].as_u64().unwrap();
    assert!(paired_at > 1_700_000_000, "{paired_at}");
    assert_eq!(
        listed["clients"][0]["paired_at_unix_ms"].as_u64(),
        Some(paired_at * 1_000)
    );
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
    paired_hello(lamp, grants, seed).await.0
}

/// As [`paired`], with hello's answer.
async fn paired_hello(lamp: &Lamp, grants: Value, seed: u8) -> (Socket, Value) {
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
    (socket, hello)
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

/// The app's `control.set_grants` on the admin socket: a computer's grants
/// change with no new code, the answer says what moved, the file keeps it,
/// the computer's connection closes (it reads its grants at its next hello),
/// and a refusal (no such computer, an unknown grant, a file that won't take
/// it) changes nothing.
#[tokio::test]
async fn the_admin_socket_changes_a_computers_grants() {
    let lamp = lamp("set-grants").await;
    let mut socket = paired(&lamp, json!(["light"]), 12).await;
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    // Refusals first: nothing changes.
    let none = admin(
        &lamp,
        json!({"set_grants": {"client": "c_nobody", "grants": ["light"]}}),
    )
    .await;
    assert_eq!(none["error"], "no such computer", "{none}");
    let unknown = admin(
        &lamp,
        json!({"set_grants": {"client": client, "grants": ["light", "teleport"]}}),
    )
    .await;
    assert!(
        unknown["error"].as_str().unwrap().starts_with("grants:"),
        "{unknown}"
    );
    std::fs::create_dir(lamp.dir.join("clients.json.new")).unwrap();
    let unsaved = admin(
        &lamp,
        json!({"set_grants": {"client": client, "grants": ["light", "camera"]}}),
    )
    .await;
    assert!(
        unsaved["error"].as_str().unwrap().starts_with("not saved"),
        "{unsaved}"
    );
    std::fs::remove_dir(lamp.dir.join("clients.json.new")).unwrap();
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert_eq!(listed["clients"][0]["grants"], json!(["light"]));
    // The words for every grant, once per answer, for an app that has no label of its own.
    assert_eq!(
        listed["shown"]["camera"],
        lamp_control::protocol::Grant::Camera.shown()
    );
    // The whole list is asked for: a request with no `grants` changes nothing.
    let missing = admin(&lamp, json!({"set_grants": {"client": client}})).await;
    assert!(
        missing["error"]
            .as_str()
            .unwrap()
            .contains("the whole list"),
        "{missing}"
    );
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert_eq!(listed["clients"][0]["grants"], json!(["light"]));
    // Now the change: camera gained, saved, said.
    let set = admin(
        &lamp,
        json!({"set_grants": {"client": client, "grants": ["light", "camera"]}}),
    )
    .await;
    assert_eq!(set["client"], json!(client), "{set}");
    assert_eq!(set["added"], json!(["camera"]), "{set}");
    assert_eq!(set["removed"], json!([]), "{set}");
    assert_eq!(set["saved"], true, "{set}");
    assert_eq!(
        set["shown"]["microphone"],
        lamp_control::protocol::Grant::Microphone.shown()
    );
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert_eq!(listed["clients"][0]["grants"], json!(["light", "camera"]));
    // Its connection is closed, saying why, so it reads the new grants at its next hello.
    let reason = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(Some(frame)))) => return Some(frame.reason.to_string()),
                None | Some(Err(_) | Ok(Message::Close(None))) => return None,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await
    .expect("the computer's connection closed");
    assert_eq!(reason.as_deref(), Some(server::CLOSED_GRANTS));
}

/// A revoke closes the computer's connection and says why in the close
/// frame, so its SDK can log one line instead of leaving a developer guessing.
#[tokio::test]
async fn a_revoke_says_why_in_the_close_frame() {
    let lamp = lamp("close-reason").await;
    let mut socket = paired(&lamp, json!(["light"]), 14).await;
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    admin(&lamp, json!({"revoke": client})).await;
    let reason = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(Some(frame)))) => return Some(frame.reason.to_string()),
                None | Some(Err(_) | Ok(Message::Close(None))) => return None,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await
    .expect("the connection closes");
    assert_eq!(reason.as_deref(), Some(server::CLOSED_REVOKED));
}

/// The app's grants screen asks whether a camera or microphone stream is
/// usable: false for both by default, what the lamp's wiring and the owner's
/// switch say once set.
#[tokio::test]
async fn the_admin_socket_says_which_streams_are_usable() {
    let lamp = lamp("features").await;
    assert_eq!(
        admin(&lamp, json!({"features": true})).await,
        json!({"camera": false, "microphone": false})
    );
    lamp.shared.set_features(server::Features {
        camera: true,
        microphone: false,
    });
    assert_eq!(
        admin(&lamp, json!({"features": true})).await,
        json!({"camera": true, "microphone": false})
    );
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
    // Its own take meanwhile is told whose lease holds the part, not
    // "another computer" (lamp run, 10 Nov 16:20).
    let again = send(
        &mut socket,
        json!({"id": 5, "op": "lease.take", "parts": ["light"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(again["error"]["code"], "busy", "{again}");
    assert_eq!(
        again["error"]["why"],
        "your lease on the light is suspended while someone talks to the lamp: it is yours again when they stop",
        "{again}"
    );
    lamp.shared.deliver(vec![Out::Resume]);
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "held"})
    );
}

/// What the lamp says goes to a computer only with the `speech` grant it
/// asked for, a line meant for one computer goes to that computer alone, and
/// the journal keeps the kind, never the words.
#[tokio::test]
async fn what_the_lamp_says_reaches_only_a_computer_holding_speech() {
    use lamp_control::watch::{Out, SaidTo};
    let lamp = lamp("said").await;
    // Without the grant the subscription itself is refused.
    let mut plain = paired(&lamp, json!(["look"]), 7).await;
    let refused = send(
        &mut plain,
        json!({"id": 3, "op": "subscribe", "events": ["lamp.said"]}),
    )
    .await;
    assert_eq!(refused["error"]["code"], "no_grant", "{refused}");
    lamp.shared.move_clock_on(Duration::from_millis(1_100));
    let mut reader = paired(&lamp, json!(["speech"]), 8).await;
    let subscribed = send(
        &mut reader,
        json!({"id": 3, "op": "subscribe", "events": ["lamp.said"]}),
    )
    .await;
    assert_eq!(subscribed["ok"], true, "{subscribed}");
    let fields = json!({"text": "Going to sleep now.", "by": "lamp", "source": "cached"});
    lamp.shared.deliver(vec![Out::Said {
        fields: fields.clone(),
        to: SaidTo::Everyone,
    }]);
    let said = next(&mut reader).await;
    assert_eq!(said["ev"], "lamp.said");
    assert_eq!(said["text"], "Going to sleep now.");
    assert_eq!(
        (said["by"].as_str(), said["source"].as_str()),
        (Some("lamp"), Some("cached"))
    );
    assert!(said["at"].as_str().unwrap().ends_with('Z'), "{said}");
    // A line meant for another computer is not this one's.
    lamp.shared.deliver(vec![Out::Said {
        fields: json!({"text": "Not yours.", "by": "computer", "source": "exact"}),
        to: SaidTo::Client("c_other".into()),
    }]);
    // Its own is.
    let listed = admin(&lamp, json!({"clients": true})).await;
    let own = listed["clients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "test laptop")
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| listed["clients"][1]["id"].as_str().unwrap().to_owned());
    lamp.shared.deliver(vec![Out::Said {
        fields: json!({"text": "Build passed.", "by": "computer", "source": "exact"}),
        to: SaidTo::Client(own),
    }]);
    let mine = next(&mut reader).await;
    assert_eq!(mine["text"], "Build passed.", "{mine}");
}

/// A look under a body lease that a conversation suspended is refused as
/// the suspension it is, not as "take a lease on the body first": the SDK's
/// `Lease.state` still read `held` for the moment before its event came
/// (the lead's 4-lamp show, 10 Nov 16:50: no_lease on about 1 turn in 6).
#[tokio::test]
async fn an_op_under_a_suspended_body_lease_says_suspended_not_no_lease() {
    use lamp_control::protocol::LeaseWhy;
    use lamp_control::watch::Out;
    let lamp = lamp("suspended-look").await;
    let mut socket = paired(&lamp, json!(["body", "look"]), 5).await;
    let take = send(
        &mut socket,
        json!({"id": 3, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1", "{take}");
    lamp.shared
        .deliver(vec![Out::Suspend(None, LeaseWhy::Conversation)]);
    assert_eq!(
        next(&mut socket).await,
        json!({"ev": "lease", "lease": "l_1", "state": "suspended", "why": "conversation"})
    );
    let look = send(&mut socket, json!({"id": 4, "op": "look", "at": "ahead"})).await;
    assert_eq!(look["error"]["code"], "busy", "{look}");
    assert_eq!(
        look["error"]["why"],
        "your lease on the body is suspended while someone talks to the lamp: it is yours again when they stop",
        "{look}"
    );
    // Never taken at all: still no_lease.
    lamp.shared.move_clock_on(Duration::from_millis(1_100));
    let mut other = paired(&lamp, json!(["body", "look"]), 6).await;
    let none = send(&mut other, json!({"id": 3, "op": "look", "at": "ahead"})).await;
    assert_eq!(none["error"]["code"], "no_lease", "{none}");
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
        Arc::default(),
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
        Arc::default(),
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
        Arc::default(),
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

/// The hand end to end, on a bus in a folder: `humans/hands/tracked` as Ten
/// publishes it reaches a computer with `look` as a `hand` event, and gone
/// after a second of silence; a `look` at `hand` under the `body` lease
/// tells the body to follow it with the hold, at once, and `stop` ends
/// that.
#[tokio::test]
async fn a_hand_on_the_bus_is_told_and_a_look_at_it_has_the_body_follow() {
    let follow = lamp_control::body_lease::Follower::default();
    let events: lamp_control::parts::Events = Arc::new(|_: &str, _: Value| {});
    let parts = lamp_control::parts::Lamp::new(None, None, None, "boot".into(), events)
        .following(follow.clone());
    let lamp = lamp_with("hand", Box::new(parts)).await;
    let bus = lamp_bus::Bus::new(lamp.dir.join("bus")).unwrap();
    let watch = Arc::new(std::sync::Mutex::new(lamp_control::watch::Watch::new()));
    server::watch_bus(&bus, watch, lamp.shared.clone()).unwrap();
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        follow,
        lamp.shared.events(),
        tokio::sync::mpsc::unbounded_channel().1,
    ));
    let mut socket = paired(&lamp, json!(["body", "look"]), 14).await;
    let subscribed = send(
        &mut socket,
        json!({"id": 3, "op": "subscribe", "events": ["hand"]}),
    )
    .await;
    assert_eq!(subscribed["ok"], true, "{subscribed}");
    let mut hands = bus
        .publisher("humans/hands/tracked", lamp_bus::Kind::Event, "eyes")
        .unwrap();
    let seen =
        json!({"x": 0.42, "y": 0.61, "size": 0.18, "open": true, "confidence": 0.93, "t": 1});
    hands.publish_json(1, &seen).unwrap();
    let told = next(&mut socket).await;
    assert_eq!(told["ev"], "hand", "{told}");
    assert_eq!(
        (
            &told["seen"],
            &told["x"],
            &told["y"],
            &told["size"],
            &told["open"]
        ),
        (
            &json!(true),
            &json!(0.42),
            &json!(0.61),
            &json!(0.18),
            &json!(true)
        ),
        "{told}"
    );
    assert!(
        told.get("t").is_none(),
        "the lamp's clock stays on the lamp"
    );
    let gone = tokio::time::timeout(Duration::from_secs(3), next(&mut socket))
        .await
        .expect("gone after a second's silence");
    assert_eq!(
        (&gone["ev"], &gone["seen"]),
        (&json!("hand"), &json!(false))
    );
    // Following: under the body lease only, said with the hold at once.
    let take = send(
        &mut socket,
        json!({"id": 4, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
        .await
        .expect("the door holds the body")
        .unwrap();
    let (read, _write) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(read).lines();
    assert_eq!(take["lease"], "l_1", "{take}");
    async fn line(
        lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
    ) -> String {
        tokio::time::timeout(Duration::from_secs(1), lines.next_line())
            .await
            .expect("said within a second, not at the next renewal")
            .unwrap()
            .unwrap()
    }
    assert_eq!(line(&mut lines).await, r#"{"hold":{"for_ms":30000}}"#);
    let look = send(&mut socket, json!({"id": 5, "op": "look", "at": "hand"})).await;
    assert_eq!(look["ok"], true, "{look}");
    assert_eq!(
        line(&mut lines).await,
        r#"{"hold":{"for_ms":30000,"follow":"hand"}}"#
    );
    // `stop` ends it, never rated as a look is (one motion a second).
    let stopped = send(&mut socket, json!({"id": 6, "op": "stop"})).await;
    assert_eq!(stopped["ok"], true, "{stopped}");
    assert_eq!(line(&mut lines).await, r#"{"hold":{"for_ms":30000}}"#);
}

/// `get` `body` follows the body as it moves (the local leader's run on
/// lamp-75c9, 4 Nov): `body/joint_states` as lamp-body publishes it, 12.5 a
/// second with its declaration and a second's lifespan, each new position
/// shown within a few of them, never held at an old one.
#[tokio::test]
async fn get_body_follows_the_joint_states_on_the_bus() {
    let watch = Arc::new(std::sync::Mutex::new(lamp_control::watch::Watch::new()));
    let events: lamp_control::parts::Events = Arc::new(|_: &str, _: Value| {});
    let parts = lamp_control::parts::Lamp::new(None, None, None, "boot".into(), events)
        .watching(watch.clone());
    let lamp = lamp_with("joints", Box::new(parts)).await;
    let bus = lamp_bus::Bus::new(lamp.dir.join("bus")).unwrap();
    server::watch_bus(&bus, watch, lamp.shared.clone()).unwrap();
    let mut limits = bus
        .publisher_declared(
            lamp_msgs::BodyLimits::TOPIC,
            &lamp_msgs::BodyLimits::declaration(),
            "body",
        )
        .unwrap();
    let mut states = bus
        .publisher_declared(
            lamp_msgs::JointState::TOPIC,
            &lamp_msgs::JointState::declaration(),
            "body",
        )
        .unwrap();
    states.expires_after(Some(Duration::from_secs(1)));
    let limit = json!({"joints": [{"name": "base_yaw", "min": 1975, "max": 2055, "home": 2012}],
                       "speed": 50.0, "acceleration": 400.0, "force": 600, "heat_stop": 62, "stands": true});
    let radians = |counts: f64| (counts - 2_048.0) * std::f64::consts::TAU / 4_096.0;
    let mut socket = paired(&lamp, json!(["body"]), 15).await;
    let yaw = |body: &Value| body["state"]["body"]["joints"]["yaw"]["counts"].as_f64();
    let started = std::time::Instant::now();
    let mut at_us = 1;
    for counts in [1_999.0, 2_021.0, 2_007.0, 1_998.0, 2_030.0] {
        // 20 steps (1.6 s) at each position, as a body holding there says.
        let mut seen = None;
        for step in 0..20 {
            if step % 10 == 0 {
                limits.publish_json(at_us, &limit).unwrap();
            }
            let state = json!({"name": ["base_yaw"], "position": [radians(counts)],
                               "velocity": [], "effort": [], "commanded": [radians(counts)]});
            states.publish_json(at_us, &state).unwrap();
            at_us += 80_000;
            tokio::time::sleep(Duration::from_millis(80)).await;
            let body = send(
                &mut socket,
                json!({"id": at_us, "op": "get", "state": ["body"]}),
            )
            .await;
            seen = yaw(&body);
            if step == 5 {
                assert_eq!(
                    seen,
                    Some(counts),
                    "at {counts} for 6 steps, {:?} in: {body}",
                    started.elapsed()
                );
            }
        }
        assert_eq!(seen, Some(counts));
    }
}

/// The local leader's P3: A follows the hand, A's lease ends, B takes the
/// body; B's hold never says follow, whether the door let go between them
/// or B took the body before a check saw it free.
#[tokio::test]
async fn a_follow_ends_with_its_clients_lease_and_never_passes_to_the_next_holder() {
    let follow = lamp_control::body_lease::Follower::default();
    let events: lamp_control::parts::Events = Arc::new(|_: &str, _: Value| {});
    let parts = lamp_control::parts::Lamp::new(None, None, None, "boot".into(), events)
        .following(follow.clone());
    let lamp = lamp_with("hand-next", Box::new(parts)).await;
    let path = lamp.dir.join("lease.sock");
    let body = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(lamp_control::body_lease::hold(
        path.clone(),
        lamp.shared.clone(),
        follow,
        lamp.shared.events(),
        tokio::sync::mpsc::unbounded_channel().1,
    ));
    let take = |id: u64| json!({"id": id, "op": "lease.take", "parts": ["body"], "for_ms": 30000});
    let accept = || async {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(2), body.accept())
            .await
            .expect("the door holds the body")
            .unwrap();
        let (read, write) = stream.into_split();
        (tokio::io::BufReader::new(read).lines(), write)
    };
    async fn line(
        lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
    ) -> Option<String> {
        tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .expect("said in time")
            .unwrap()
    }
    let mut a = paired(&lamp, json!(["body", "look"]), 15).await;
    // One pairing try a second: the door's clock moves on past it.
    lamp.shared.move_clock_on(Duration::from_millis(1_100));
    let mut b = paired(&lamp, json!(["body", "look"]), 16).await;
    assert_eq!(send(&mut a, take(3)).await["lease"], "l_1");
    let (mut lines, _write) = accept().await;
    assert_eq!(
        line(&mut lines).await.unwrap(),
        r#"{"hold":{"for_ms":30000}}"#
    );
    let look = send(&mut a, json!({"id": 4, "op": "look", "at": "hand"})).await;
    assert_eq!(look["ok"], true, "{look}");
    assert_eq!(
        line(&mut lines).await.unwrap(),
        r#"{"hold":{"for_ms":30000,"follow":"hand"}}"#
    );
    // A's lease ends, and B takes the body at once.
    let released = send(
        &mut a,
        json!({"id": 5, "op": "lease.release", "lease": "l_1"}),
    )
    .await;
    assert_eq!(released["ok"], true, "{released}");
    assert_eq!(send(&mut b, take(3)).await["lease"], "l_2");
    let next = line(&mut lines).await;
    let first_for_b = match next.as_deref() {
        // The door let go between them: B's hold comes on a new connection.
        Some(r#"{"let_go":true}"#) | None => {
            let (mut lines, _write) = accept().await;
            line(&mut lines).await.unwrap()
        }
        // B took the body before a check saw it free.
        Some(said) => said.to_owned(),
    };
    assert_eq!(first_for_b, r#"{"hold":{"for_ms":30000}}"#);
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
        Arc::default(),
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
        Arc::default(),
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
    manage_helper_gated(socket, state, install_takes, None)
}

/// Held open until the test opens it (#1806): a list answers then, not
/// after a fixed wait. Ten seconds at most, so a test that never opens it
/// fails rather than hangs.
type Gate = Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>;

fn open_gate(gate: &Gate) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

/// [`manage_helper`], with each list held at `gate` when one is given.
fn manage_helper_gated(
    socket: &std::path::Path,
    state: &std::path::Path,
    install_takes: Duration,
    gate: Option<Gate>,
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
            let gate = gate.clone();
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
                    match &gate {
                        Some(gate) => {
                            let (open, opened) = &**gate;
                            let _held = opened
                                .wait_timeout_while(
                                    open.lock().unwrap(),
                                    Duration::from_secs(10),
                                    |open| !*open,
                                )
                                .unwrap();
                        }
                        None => std::thread::sleep(install_takes),
                    }
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

/// Waits until the helper has been asked `request` `times` times, within
/// 10 s.
async fn until_asked_times(helper: &std::sync::Mutex<Helper>, request: &Value, times: usize) {
    for _ in 0..500 {
        let asked = helper.lock().unwrap().asked.clone();
        if asked.iter().filter(|seen| *seen == request).count() >= times {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "never asked {request} {times} times: {:?}",
        helper.lock().unwrap().asked
    );
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

/// A package as `lamp-skills pack` lays it out, tar of one skill's folder.
fn tar_of(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in files {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, &bytes[..]).unwrap();
    }
    builder.into_inner().unwrap()
}

/// A recorded move: an 8 s lean of `yaw` counts and back.
fn lean(name: &str, yaw: i16) -> Vec<u8> {
    let frames: Vec<[i16; 5]> = [0, yaw / 2, yaw, yaw / 2, 0, yaw / 2, yaw, yaw / 2, 0]
        .iter()
        .map(|yaw| [0, 0, *yaw, 0, 0])
        .collect();
    json!({"name": name, "step_ms": 1000, "frames": frames})
        .to_string()
        .into_bytes()
}

/// docs/control.md, "Play a recorded move", end to end: the install keeps
/// the package's `moves/` (the answer names the kept and the left out),
/// `play` sends one to the body as the `move` it is (the same lease and
/// the same datagram), a removed skill's moves and a revoked computer's are
/// gone, and a push again replaces them.
#[tokio::test]
async fn a_recorded_move_of_a_pushed_skill_is_played_as_a_move_and_goes_with_its_skill_and_pairing()
{
    let dir = std::env::temp_dir().join(format!(
        "lamp-control-door-mood-for-play-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mood = std::os::unix::net::UnixDatagram::bind(dir.join("mood.sock")).unwrap();
    mood.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let parts = lamp_control::parts::Lamp::new(
        Some(dir.join("mood.sock")),
        None,
        None,
        "boot".into(),
        Arc::new(|_, _| {}),
    );
    let lamp = lamp_built("play", Box::new(parts), |door, dir| {
        door.with_push_folder(dir.to_path_buf())
    })
    .await;
    let _helper = manage_helper(&lamp.dir.join("manage.sock"), &lamp.dir, Duration::ZERO);
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut socket = paired(&lamp, json!(["body", "skills"]), 44).await;
    let package = tar_of(&[
        ("desk-wave/skill.toml", b"kind = \"installable\"".to_vec()),
        ("desk-wave/moves/wave.json", lean("wave", 100)),
        ("desk-wave/moves/far.json", lean("far", 600)),
    ]);
    let sha256: String = {
        use sha2::Digest as _;
        sha2::Sha256::digest(&package)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let push = |id: u64| {
        json!({"id": id, "op": "skill.push", "name": "desk-wave", "sha256": sha256,
            "size": package.len(), "part": 0, "parts": 1, "data": STANDARD.encode(&package)})
    };
    // Before any push: nothing to play, and no lease needed to be told so.
    let none = send(&mut socket, json!({"id": 2, "op": "play", "name": "wave"})).await;
    assert_eq!(none["error"]["code"], "no_lease", "{none}");
    let installed = send(&mut socket, push(3)).await;
    assert_eq!(installed["skill"]["name"], "desk-wave", "{installed}");
    assert_eq!(installed["moves"]["kept"], json!(["wave"]), "{installed}");
    assert_eq!(
        installed["moves"]["left_out"][0]["name"], "far",
        "{installed}"
    );
    assert!(
        installed["moves"]["left_out"][0]["why"]
            .as_str()
            .unwrap()
            .contains("farther than"),
        "{installed}"
    );
    let take = send(
        &mut socket,
        json!({"id": 4, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["lease"], "l_1", "{take}");
    let unknown = send(
        &mut socket,
        json!({"id": 5, "op": "play", "name": "nothing"}),
    )
    .await;
    assert_eq!(unknown["error"]["code"], "refused", "{unknown}");
    assert!(
        unknown["error"]["why"]
            .as_str()
            .unwrap()
            .starts_with("no move nothing in your pushed skills"),
        "{unknown}"
    );
    let played = send(&mut socket, json!({"id": 6, "op": "play", "name": "wave"})).await;
    assert_eq!(played["to"], "body", "{played}");
    let mut datagram = vec![0u8; 16 * 1024];
    let len = mood.recv(&mut datagram).unwrap();
    let sent: Value = serde_json::from_slice(&datagram[..len]).unwrap();
    assert_eq!(sent["move"]["name"], "control");
    assert_eq!(sent["move"]["step_ms"], 1000);
    assert_eq!(sent["move"]["frames"][2], json!([0, 0, 100, 0, 0]));
    // The kept file is the computer's, beside the packages, and the package
    // itself is gone as before.
    let client = std::fs::read_dir(lamp.dir.join("moves"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    assert!(
        lamp.dir
            .join("moves")
            .join(&client)
            .join("desk-wave.json")
            .exists()
    );
    assert!(!lamp.dir.join("push/package.tar").exists());
    // The skill removed: its moves go too (past the motion rate's second).
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let removed = send(
        &mut socket,
        json!({"id": 7, "op": "skill.remove", "name": "desk-wave"}),
    )
    .await;
    assert_eq!(removed["ok"], true, "{removed}");
    assert!(
        !lamp
            .dir
            .join("moves")
            .join(&client)
            .join("desk-wave.json")
            .exists()
    );
    let gone = send(&mut socket, json!({"id": 8, "op": "play", "name": "wave"})).await;
    assert_eq!(gone["error"]["code"], "refused", "{gone}");
    // The pairing revoked: the computer's folder goes.
    let revoked = admin(&lamp, json!({"revoke": client.to_str().unwrap()})).await;
    assert_eq!(revoked["revoked"], true, "{revoked}");
    assert!(!lamp.dir.join("moves").join(&client).exists());
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
    // The first off came while the install was still with the helper; the
    // second once it answered. Waited for, not slept for: under load the
    // install's answer can land over 2 s after the first off.
    until_asked_times(&helper, &off, 2).await;
    let asked = helper.lock().unwrap().asked.clone();
    assert_eq!(asked, [install, off.clone(), off], "{asked:?}");
}

/// Nine's P2 on #6039: a revoke while an install is with the helper deletes
/// the computer's folder of moves, and the install's answer must not write
/// the moves back for a pairing that is gone.
#[tokio::test]
async fn a_revoke_during_an_install_leaves_no_moves_of_that_computer() {
    let lamp = lamp_built(
        "push-revoke-moves",
        Box::new(session::Unrouted),
        |door, dir| door.with_push_folder(dir.to_path_buf()),
    )
    .await;
    let helper = manage_helper(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::from_millis(1_500),
    );
    lamp.shared.set_manage_socket(lamp.dir.join("manage.sock"));
    let mut socket = paired(&lamp, json!(["body", "skills"]), 44).await;
    let package = tar_of(&[
        ("desk-wave/skill.toml", b"kind = \"installable\"".to_vec()),
        ("desk-wave/moves/wave.json", lean("wave", 100)),
    ]);
    let sha256: String = {
        use sha2::Digest as _;
        sha2::Sha256::digest(&package)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    socket
        .send(Message::text(
            json!({"id": 3, "op": "skill.push", "name": "desk-wave", "sha256": sha256,
                "size": package.len(), "part": 0, "parts": 1, "data": STANDARD.encode(&package)})
            .to_string(),
        ))
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
    // The install's answer lands after the revoke: waited for through the
    // second `off`, which the door sends once the install is answered.
    let off = json!({"off_pushed_by": client});
    until_asked_times(&helper, &off, 2).await;
    assert!(
        !lamp.dir.join("moves").join(&client).exists(),
        "a revoked computer's moves were kept"
    );
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
    // The helper holds the four lists until the test opens its gate (#1806).
    let gate: Gate = Arc::default();
    let helper = manage_helper_gated(
        &lamp.dir.join("manage.sock"),
        &lamp.dir,
        Duration::ZERO,
        Some(gate.clone()),
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
            // One pairing try a second: the door's clock moves on past it
            // rather than the test sleeping it (#1806).
            lamp.shared.move_clock_on(Duration::from_millis(1_100));
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
    open_gate(&gate);
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

/// #4209: a computer's motion is told back. On a bus in a folder, with the
/// body's mood socket: hello says `motion_done`; a gesture the body begins
/// and ends is `finished`, at its `over`; a move the body drops is
/// `dropped` with the body's why; one the body never begins is `dropped`
/// after 10 s. A lamp that can't read the body's word says nothing of it.
#[tokio::test]
async fn a_motion_is_told_finished_or_dropped_as_the_body_says() {
    let dir = std::env::temp_dir().join(format!("lamp-control-motion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mood = dir.join("mood.sock");
    let body = std::os::unix::net::UnixDatagram::bind(&mood).unwrap();
    let bus = lamp_bus::Bus::new(dir.join("bus")).unwrap();
    let gestures = bus.subscribe("body/gesture", "control").unwrap();
    let mut said = bus
        .publisher("body/gesture", lamp_bus::Kind::Event, "body")
        .unwrap();
    let told = lamp_made(
        "motion-done",
        move |events| {
            Box::new(
                lamp_control::parts::Lamp::new(Some(mood), None, None, "boot".into(), events)
                    .telling_motions(gestures),
            )
        },
        // As the product: rates off (lamp-control's default).
        |door, _| door.with_rates_off(),
    )
    .await;
    let (mut socket, hello) = paired_hello(&told, json!(["body"]), 41).await;
    assert_eq!(hello["limits"]["motion_done"], true, "{hello}");
    let mut buffer = [0u8; 4096];
    let nod = send(
        &mut socket,
        json!({"id": 5, "op": "gesture", "name": "nod"}),
    )
    .await;
    assert_eq!(nod["to"], "body", "{nod}");
    let length = body.recv(&mut buffer).unwrap();
    let asked: Value = serde_json::from_slice(&buffer[..length]).unwrap();
    assert_eq!(asked["gesture"], "nod");
    said.publish_json(1, &json!({"name": "nod", "state": "began"}))
        .unwrap();
    said.publish_json(2, &json!({"name": "nod", "state": "over"}))
        .unwrap();
    let done = next(&mut socket).await;
    assert_eq!(done, json!({"ev": "done", "re": 5, "how": "finished"}));
    let take = send(
        &mut socket,
        json!({"id": 10, "op": "lease.take", "parts": ["body"], "for_ms": 30000}),
    )
    .await;
    assert_eq!(take["ok"], true, "{take}");
    let frames = json!([
        [0, 0, 0, 0, 0],
        [0, 0, 40, 0, 0],
        [0, 0, 40, 0, 0],
        [0, 0, 0, 0, 0]
    ]);
    let moved = send(
        &mut socket,
        json!({"id": 6, "op": "move", "step_ms": 500, "frames": frames}),
    )
    .await;
    assert_eq!(moved["to"], "body", "{moved}");
    let drop = json!({"name": "control", "state": "dropped",
        "why": "dropped: the person is using the lamp"});
    said.publish_json(3, &drop).unwrap();
    let done = next(&mut socket).await;
    assert_eq!(
        done,
        json!({"ev": "done", "re": 6, "how": "dropped",
            "why": "dropped: the person is using the lamp"})
    );
    let shy = send(
        &mut socket,
        json!({"id": 7, "op": "gesture", "name": "shy"}),
    )
    .await;
    assert_eq!(shy["to"], "body", "{shy}");
    let done = tokio::time::timeout(Duration::from_secs(13), async {
        loop {
            let frame = next_within(&mut socket, Duration::from_secs(13)).await;
            if frame["ev"] == "done" {
                return frame;
            }
        }
    })
    .await
    .expect("a done for the gesture never begun");
    assert_eq!(
        done,
        json!({"ev": "done", "re": 7, "how": "dropped",
            "why": "the body did not begin it within 10 s"})
    );
    let _ = std::fs::remove_dir_all(&dir);
    let plain = lamp("motion-done-none").await;
    let (_, hello) = paired_hello(&plain, json!(["body"]), 42).await;
    assert!(hello["limits"].get("motion_done").is_none(), "{hello}");
}

/// Another connection of an already paired computer (`seed`'s key): its
/// own welcome, signed as that computer.
async fn hello_again(lamp: &Lamp, seed: u8) -> Socket {
    let keys = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
    let key: [u8; 32] = keys.public_key().as_ref().try_into().unwrap();
    let client = pairing::client_id(&key);
    let mut socket = connect(lamp, "/v1").await.unwrap();
    let welcome = next(&mut socket).await;
    let nonce: [u8; 32] = pairing::bytes_of(welcome["nonce"].as_str().unwrap()).unwrap();
    let sig = keys.sign(&[pairing::HELLO_CONTEXT, &nonce].concat());
    let hello = send(
        &mut socket,
        json!({"id": 1, "op": "hello", "client": client, "sig": STANDARD.encode(sig)}),
    )
    .await;
    assert_eq!(hello["ok"], true, "{hello}");
    socket
}

/// Second's P2 on #5835: two programs of one paired computer, each on its
/// own connection with its own request numbers, both ask request 5: A a
/// nod, B a headshake. The body begins and ends the nod alone: A is told
/// its `done`, and B, whose headshake hasn't played, is told nothing.
#[tokio::test]
async fn a_motions_done_goes_to_the_connection_that_asked_it_alone() {
    let dir = std::env::temp_dir().join(format!("lamp-control-motion2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mood = dir.join("mood.sock");
    let body = std::os::unix::net::UnixDatagram::bind(&mood).unwrap();
    let bus = lamp_bus::Bus::new(dir.join("bus")).unwrap();
    let gestures = bus.subscribe("body/gesture", "control").unwrap();
    let mut said = bus
        .publisher("body/gesture", lamp_bus::Kind::Event, "body")
        .unwrap();
    let told = lamp_made(
        "motion-two",
        move |events| {
            Box::new(
                lamp_control::parts::Lamp::new(Some(mood), None, None, "boot".into(), events)
                    .telling_motions(gestures),
            )
        },
        |door, _| door.with_rates_off(),
    )
    .await;
    let mut a = paired(&told, json!(["body"]), 45).await;
    let mut b = hello_again(&told, 45).await;
    let nod = send(&mut a, json!({"id": 5, "op": "gesture", "name": "nod"})).await;
    assert_eq!(nod["to"], "body", "{nod}");
    let shake = send(
        &mut b,
        json!({"id": 5, "op": "gesture", "name": "headshake"}),
    )
    .await;
    assert_eq!(shake["to"], "body", "{shake}");
    let mut buffer = [0u8; 4096];
    for _ in 0..2 {
        body.recv(&mut buffer).unwrap();
    }
    said.publish_json(1, &json!({"name": "nod", "state": "began"}))
        .unwrap();
    said.publish_json(2, &json!({"name": "nod", "state": "over"}))
        .unwrap();
    assert_eq!(
        next(&mut a).await,
        json!({"ev": "done", "re": 5, "how": "finished"})
    );
    let nothing = tokio::time::timeout(Duration::from_millis(500), b.next()).await;
    assert!(nothing.is_err(), "B was told: {nothing:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// docs/control.md, "Turns and listening", end to end: `listen.set false`
/// is answered on its own connection once the voice has taken the mute, and
/// the say socket connection holding it closes when the computer's last
/// connection does, and when it is revoked; `turn.end` with no turn of its
/// own is refused without asking the voice.
#[tokio::test]
async fn a_computers_mute_is_held_while_it_is_here_and_let_go_when_it_goes() {
    use tokio::io::AsyncReadExt as _;
    let dir = std::env::temp_dir().join(format!("lamp-control-door-voice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("say.sock");
    let voice = tokio::net::UnixListener::bind(&path).unwrap();
    let parts =
        lamp_control::parts::Lamp::new(None, None, Some(path), "boot".into(), Arc::new(|_, _| {}));
    let lamp = lamp_with("mute", Box::new(parts)).await;
    // A line from the door, and the connection kept to answer on.
    async fn asked(voice: &tokio::net::UnixListener) -> (tokio::net::UnixStream, Value) {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), voice.accept())
            .await
            .expect("the door asks the voice")
            .unwrap();
        let mut stream = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        (stream.into_inner(), serde_json::from_str(&line).unwrap())
    }
    async fn closes(mut stream: tokio::net::UnixStream) {
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest))
            .await
            .expect("the door closes the mute's connection")
            .unwrap();
    }
    let mut socket = paired(&lamp, json!(["speak", "hear"]), 21).await;
    let refused = send(&mut socket, json!({"id": 3, "op": "turn.end"})).await;
    assert_eq!(refused["error"]["code"], "busy", "{refused}");
    assert_eq!(refused["error"]["why"], "not this computer's turn");
    socket
        .send(Message::text(
            json!({"id": 4, "op": "listen.set", "on": false}).to_string(),
        ))
        .await
        .unwrap();
    let (mut held, line) = asked(&voice).await;
    assert_eq!(line, json!({"skill": "control", "listen": false}));
    held.write_all(b"{\"outcome\":\"muted\"}\n").await.unwrap();
    assert_eq!(
        next(&mut socket).await,
        json!({"re": 4, "ok": true, "mute": "held"})
    );
    // Its last connection closes: so does the mute's.
    socket.close(None).await.unwrap();
    closes(held).await;

    // Held again by another computer, then revoked.
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let mut socket = paired(&lamp, json!(["hear"]), 22).await;
    socket
        .send(Message::text(
            json!({"id": 5, "op": "listen.set", "on": false}).to_string(),
        ))
        .await
        .unwrap();
    let (mut held, _) = asked(&voice).await;
    held.write_all(b"{\"outcome\":\"muted\"}\n").await.unwrap();
    assert_eq!(next(&mut socket).await["mute"], "held");
    let client = admin(&lamp, json!({"clients": true})).await["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .next_back()
        .unwrap();
    assert_eq!(
        admin(&lamp, json!({"revoke": client})).await["revoked"],
        true
    );
    closes(held).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Parts that keep each line a `say` asks the voice for.
struct Says(Arc<std::sync::Mutex<Vec<(String, String)>>>);

impl session::Parts for Says {
    fn act(
        &mut self,
        client: &str,
        _: u64,
        op: &lamp_control::protocol::Op,
        _: u64,
    ) -> Result<Value, lamp_control::protocol::Error> {
        if let lamp_control::protocol::Op::Say { text, .. } = op {
            self.0
                .lock()
                .unwrap()
                .push((client.to_owned(), text.clone()));
        }
        Ok(json!({"to": "voice"}))
    }
    fn state(&self, _: lamp_control::protocol::StateKind) -> Value {
        Value::Null
    }
    fn stop(&mut self, _: &str, _: u64) {}
}

/// The spoken pin check (#5299, Faye 5 Nov 08:18): asked with `say_pin`,
/// the lamp itself says the first 8 characters of its pin aloud, in two
/// groups of four, and the answer says the voice took it. A code the app
/// cancels (its check didn't match) is dropped: pairing with it then fails.
#[tokio::test]
async fn the_lamp_says_its_pin_check_and_a_cancelled_code_pairs_nobody() {
    let says = Arc::new(std::sync::Mutex::new(Vec::new()));
    let lamp = lamp_with("pincheck", Box::new(Says(says.clone()))).await;
    let issued = admin(
        &lamp,
        json!({"code": {"grants": ["light"], "say_pin": true}}),
    )
    .await;
    assert_eq!(issued["pin_said"], true, "{issued}");
    let pin = lamp.identity.pin.to_ascii_uppercase();
    let first: Vec<String> = pin.chars().take(8).map(String::from).collect();
    let expected = format!(
        "My pairing check is {}, {}.",
        first[..4].join(" "),
        first[4..].join(" ")
    );
    assert_eq!(
        *says.lock().unwrap(),
        [("lamp".to_owned(), expected.clone())]
    );
    assert_eq!(server::spoken_pin(&lamp.identity.pin), expected);
    // Without `say_pin`, nothing is said (the person's own `lamp-control
    // code` at the lamp), and no `pin_said`.
    let quiet = admin(&lamp, json!({"code": {"grants": ["light"]}})).await;
    assert!(quiet.get("pin_said").is_none(), "{quiet}");
    assert_eq!(says.lock().unwrap().len(), 1);

    // The app's check didn't match: the code out is dropped.
    let code = quiet["code"].as_str().unwrap().to_owned();
    assert_eq!(
        admin(&lamp, json!({"cancel_code": true})).await,
        json!({"dropped": true})
    );
    assert_eq!(
        admin(&lamp, json!({"cancel_code": true})).await,
        json!({"dropped": false})
    );
    // A computer that heard the dropped code can't pair with it.
    let keys = Ed25519KeyPair::from_seed_unchecked(&[11; 32]).unwrap();
    let key: [u8; 32] = keys.public_key().as_ref().try_into().unwrap();
    let mut socket = connect(&lamp, "/v1").await.unwrap();
    let welcome = next(&mut socket).await;
    let nonce: [u8; 32] = pairing::bytes_of(welcome["nonce"].as_str().unwrap()).unwrap();
    let proof = pairing::pair_proof(&code, &lamp.identity.pin_bytes(), &key, &nonce);
    let refused = send(
        &mut socket,
        json!({"id": 1, "op": "pair", "name": "late", "key": STANDARD.encode(key), "proof": STANDARD.encode(proof)}),
    )
    .await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        refused["error"],
        json!({"code": "unpaired", "why": "no pairing code is out: ask for one in the app"})
    );
    assert!(
        admin(&lamp, json!({"clients": true})).await["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// Heron's and the Reviewer-lead's P3 on #4579: the owner's app takes the
/// lamp back. Every lease of the computer is lost as `taken_by_owner_app`,
/// told to it, and the computer stays paired; for one computer, only its
/// leases end.
#[tokio::test]
async fn the_owner_takes_the_lamp_back_and_the_computers_stay_paired() {
    let lamp = lamp("takeback").await;
    let mut first = paired(&lamp, json!(["light"]), 21).await;
    let held = send(
        &mut first,
        json!({"id": 3, "op": "lease.take", "parts": ["light"], "for_ms": 30000}),
    )
    .await;
    let lease = held["lease"].as_str().unwrap().to_owned();
    // One computer by id: its lease ends, and it is told why.
    let clients = admin(&lamp, json!({"clients": true})).await;
    let client = clients["clients"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        admin(&lamp, json!({"take_back": true, "client": client})).await,
        json!({"taken": 1})
    );
    assert_eq!(
        next(&mut first).await,
        json!({"ev": "lease", "lease": lease, "state": "lost", "why": "taken_by_owner_app"})
    );
    // Nothing held now: a second take back ends nothing.
    assert_eq!(
        admin(&lamp, json!({"take_back": true})).await,
        json!({"taken": 0})
    );
    // Still paired: listed, and a new lease is given.
    assert_eq!(
        admin(&lamp, json!({"clients": true})).await["clients"][0]["id"],
        client
    );
    let again = send(
        &mut first,
        json!({"id": 4, "op": "lease.take", "parts": ["light"], "for_ms": 30000}),
    )
    .await;
    assert!(again["lease"].is_string(), "{again}");
    // A client that isn't an id string is refused.
    assert_eq!(
        admin(&lamp, json!({"take_back": true, "client": 7})).await,
        json!({"error": "client: a computer's id"})
    );
}

/// ten's 75c9 case (9 Nov, stable 10824): a computer paired with `hear`,
/// subscribed to `phrase.heard` alone with one phrase, hears it when the
/// voice publishes `voice/heard` as it does on a lamp: a declared
/// `lamp_msgs/Speech`, finished, with its `request`, through the real bus
/// and `watch_bus`, not handed to `deliver` directly.
#[tokio::test]
async fn a_phrase_heard_on_the_bus_reaches_its_subscriber() {
    let lamp = lamp("phrase-bus").await;
    let bus = lamp_bus::Bus::new(lamp.dir.join("bus")).unwrap();
    let watch = Arc::new(std::sync::Mutex::new(lamp_control::watch::Watch::new()));
    server::watch_bus(&bus, watch, lamp.shared.clone()).unwrap();
    let mut socket = paired(&lamp, json!(["body", "light", "look", "hear"]), 43).await;
    let subscribed = send(
        &mut socket,
        json!({"id": 3, "op": "subscribe", "events": ["phrase.heard"], "phrases": ["good night lamp"]}),
    )
    .await;
    assert_eq!(subscribed["ok"], true, "{subscribed}");
    let mut heard = bus
        .publisher_declared("voice/heard", &lamp_msgs::Speech::declaration(), "voice")
        .unwrap();
    let line = lamp_msgs::Speech {
        text: "Lamp. Good night lamp.".into(),
        finished: true,
        locale: None,
        request: Some(148),
    };
    heard.publish_json(1, &line).unwrap();
    let told = next(&mut socket).await;
    assert_eq!(
        (&told["ev"], &told["phrase"]),
        (&json!("phrase.heard"), &json!("good night lamp")),
        "{told}"
    );
}

// The local agent (docs/control.md, "The local agent"): a unix socket for one
// named user, no pairing, the grants the owner left it.

type AgentSocket = tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>;

/// This test process's own uid: the folder it made is its own.
fn my_uid(lamp: &Lamp) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(&lamp.dir).unwrap().uid()
}

/// The agent socket for `user`, whom the lamp takes as `named_uid`; its path
/// and the record's client id.
async fn agent_socket(lamp: &Lamp, user: &str, named_uid: u32) -> (std::path::PathBuf, String) {
    let client = lamp.shared.ensure_local(user, 1).unwrap();
    let path = lamp.dir.join("agent.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(server::serve_agent(
        listener,
        named_uid,
        user.to_owned(),
        client.clone(),
        lamp.shared.clone(),
    ));
    (path, client)
}

async fn agent_connect(
    path: &std::path::Path,
) -> Result<AgentSocket, tokio_tungstenite::tungstenite::Error> {
    let stream = tokio::net::UnixStream::connect(path).await.unwrap();
    tokio_tungstenite::client_async("ws://lamp.local/v1", stream)
        .await
        .map(|(socket, _)| socket)
}

async fn agent_next(socket: &mut AgentSocket) -> Option<Value> {
    loop {
        match tokio::time::timeout(Duration::from_secs(3), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return Some(serde_json::from_str(&text).unwrap()),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

async fn agent_hello(socket: &mut AgentSocket) -> Value {
    let welcome = agent_next(socket).await.expect("a welcome");
    assert_eq!(welcome["ev"], "welcome");
    socket
        .send(Message::text(
            // Well formed and meaningless: the socket's peer check is what says who it is.
            json!({"id": 1, "op": "hello", "client": "c_0", "sig": STANDARD.encode([0u8; 64])})
                .to_string(),
        ))
        .await
        .unwrap();
    agent_next(socket).await.expect("hello's answer")
}

#[tokio::test]
async fn the_local_agent_says_hello_with_no_key_and_has_only_the_first_grants() {
    let lamp = lamp("agent-hello").await;
    let (path, _client) = agent_socket(&lamp, "agent", my_uid(&lamp)).await;
    let mut socket = agent_connect(&path).await.unwrap();
    let hello = agent_hello(&mut socket).await;
    assert_eq!(hello["ok"], true, "{hello}");
    let mut grants: Vec<String> = hello["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap().to_owned())
        .collect();
    grants.sort();
    assert_eq!(
        grants,
        ["body", "light", "look", "skills", "speak"],
        "{hello}"
    );
    // It is listed as a client of the lamp like any, named for its user.
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert_eq!(
        listed["clients"][0]["name"], "local agent agent",
        "{listed}"
    );
    // Not a socket for pairing.
    socket
        .send(Message::text(
            // A well-formed pair (a name, a 32-byte key, a 32-byte proof), so it is
            // the socket's rule that refuses it, not the frame's shape.
            json!({"id": 2, "op": "pair", "name": "x", "key": STANDARD.encode([1u8; 32]), "proof": STANDARD.encode([2u8; 32])}).to_string(),
        ))
        .await
        .unwrap();
    let pair = agent_next(&mut socket).await.unwrap();
    assert_eq!(pair["ok"], false, "{pair}");
    assert!(
        pair["error"]["why"]
            .as_str()
            .unwrap()
            .contains("not for pairing"),
        "{pair}"
    );
    // What it was not given is refused in the lamp's words.
    socket
        .send(Message::text(
            json!({"id": 3, "op": "camera.snapshot"}).to_string(),
        ))
        .await
        .unwrap();
    let camera = agent_next(&mut socket).await.unwrap();
    assert_eq!(camera["error"]["code"], "no_grant", "{camera}");
}

/// The guard is the peer's uid, not the socket's group: `lampos` (whose
/// processes run the lamp's skills) is in the socket's group, and still
/// refused (the lead, 12:4x).
#[test]
fn only_the_named_users_uid_passes_not_lamposs() {
    // lampos 994, the agent 1001: the group is shared, the uid is the guard.
    assert!(!server::agent_peer_ok(994, 1001));
    assert!(!server::agent_peer_ok(0, 1001));
    assert!(server::agent_peer_ok(1001, 1001));
}

#[tokio::test]
async fn a_peer_with_another_uid_is_closed_before_a_byte_is_read() {
    let lamp = lamp("agent-uid").await;
    // The lamp names a uid that is not this process's: this peer is "lampos".
    let (path, _client) = agent_socket(&lamp, "agent", my_uid(&lamp) + 1).await;
    let refused = tokio::time::timeout(Duration::from_secs(3), agent_connect(&path))
        .await
        .expect("closed in time");
    assert!(refused.is_err(), "a peer with another uid reached the door");
}

#[tokio::test]
async fn the_record_without_a_key_is_never_reached_by_a_wss_hello() {
    let lamp = lamp("agent-wss").await;
    let (_path, client) = agent_socket(&lamp, "agent", my_uid(&lamp)).await;
    let mut socket = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut socket).await;
    // Any signature: an empty one, a made one.
    for sig in [STANDARD.encode([0u8; 64]), STANDARD.encode([7u8; 64])] {
        let hello = send(
            &mut socket,
            json!({"id": 1, "op": "hello", "client": client, "sig": sig}),
        )
        .await;
        assert_eq!(hello["error"]["code"], "unpaired", "{hello}");
        // A refused hello closes the connection: a new one for the next try.
        socket = connect(&lamp, "/v1").await.unwrap();
        let _welcome = next(&mut socket).await;
    }
}

#[tokio::test]
async fn a_revoke_leaves_the_local_agent_nothing_and_a_restart_does_not_give_it_back() {
    let lamp = lamp("agent-revoke").await;
    let (path, client) = agent_socket(&lamp, "agent", my_uid(&lamp)).await;
    let mut socket = agent_connect(&path).await.unwrap();
    assert_eq!(agent_hello(&mut socket).await["ok"], true);
    let revoked = admin(&lamp, json!({"revoke": client})).await;
    assert_eq!(revoked["revoked"], true, "{revoked}");
    // Its connection closes at once, and a new one is refused in words.
    assert!(agent_next(&mut socket).await.is_none());
    let mut again = agent_connect(&path).await.unwrap();
    let refused = agent_hello(&mut again).await;
    assert_eq!(refused["error"]["code"], "unpaired", "{refused}");
    // The record stays, with no grants; a restart reads the file and makes
    // no new one with the first grants.
    let mut restarted = pairing::Clients::load(&lamp.dir.join("clients.json")).unwrap();
    let kept = restarted.get(&client).expect("the record is kept");
    assert!(kept.grants.is_empty() && kept.is_local());
    let (again_id, made) = restarted.ensure_local("agent", 2);
    assert_eq!((again_id.clone(), made), (client, false));
    assert!(restarted.get(&again_id).unwrap().grants.is_empty());
}

// --- The run socket (#6071, PR 1): one-run tokens minted from the owner's yes ---

/// The owner's yes for each skill, as the test has it: its grants, or the
/// words a mint is refused in.
struct FakeYes(
    std::collections::BTreeMap<
        &'static str,
        Result<Vec<lamp_control::protocol::Grant>, &'static str>,
    >,
);

impl lamp_control::runs::SkillYes for FakeYes {
    fn grants(
        &self,
        skill: &str,
    ) -> Result<std::collections::BTreeSet<lamp_control::protocol::Grant>, String> {
        match self.0.get(skill) {
            Some(Ok(grants)) => Ok(grants.iter().copied().collect()),
            Some(Err(why)) => Err((*why).to_owned()),
            None => Err("no such skill".to_owned()),
        }
    }
}

fn the_owners_yes() -> Arc<FakeYes> {
    Arc::new(FakeYes(
        [
            ("booth", Ok(vec![lamp_control::protocol::Grant::Light])),
            (
                "changed",
                Err("the skill changed since the owner said yes: ask again"),
            ),
            ("empty", Ok(vec![])),
        ]
        .into(),
    ))
}

/// The run socket for the lamp, taking `named_uid`, with the owner's yes.
async fn run_socket(lamp: &Lamp, named_uid: u32) -> std::path::PathBuf {
    let path = lamp.dir.join("control.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    lamp.shared.set_skill_yes(the_owners_yes());
    tokio::spawn(server::serve_run(
        listener,
        named_uid,
        "lampos".to_owned(),
        lamp.shared.clone(),
    ));
    path
}

/// Hello on the run socket, with `token` (or none).
async fn run_hello(socket: &mut AgentSocket, token: Option<&str>) -> Value {
    let welcome = agent_next(socket).await.expect("a welcome");
    assert_eq!(welcome["ev"], "welcome");
    let mut hello =
        json!({"id": 1, "op": "hello", "client": "c_0", "sig": STANDARD.encode([0u8; 64])});
    if let Some(token) = token {
        hello["token"] = json!(token);
    }
    socket.send(Message::text(hello.to_string())).await.unwrap();
    agent_next(socket).await.expect("hello's answer")
}

async fn mint(lamp: &Lamp, skill: &str, run: &str) -> Value {
    admin(lamp, json!({"mint": {"skill": skill, "run": run}})).await
}

#[tokio::test]
async fn a_token_with_light_sets_the_light_and_is_refused_the_camera() {
    let lamp = lamp("run-light").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let minted = mint(&lamp, "booth", "r1").await;
    assert_eq!(minted["grants"], json!(["light"]), "{minted}");
    let token = minted["token"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 43, "{token}");
    let mut socket = agent_connect(&path).await.unwrap();
    let hello = run_hello(&mut socket, Some(&token)).await;
    assert_eq!(hello["ok"], true, "{hello}");
    assert_eq!(hello["grants"], json!(["light"]), "{hello}");
    assert_eq!(hello["run"], "r1", "{hello}");
    // The light is its grant: the lamp's own part answers (here, not routed),
    // not the door's refusal.
    socket
        .send(Message::text(
            json!({"id": 2, "op": "light", "colour": "warm", "level": 2, "for_ms": 1000})
                .to_string(),
        ))
        .await
        .unwrap();
    let light = agent_next(&mut socket).await.unwrap();
    assert_ne!(light["error"]["code"], "no_grant", "{light}");
    // The camera is not.
    socket
        .send(Message::text(
            json!({"id": 3, "op": "camera.snapshot"}).to_string(),
        ))
        .await
        .unwrap();
    let camera = agent_next(&mut socket).await.unwrap();
    assert_eq!(camera["error"]["code"], "no_grant", "{camera}");
    // It is no paired computer: not in the app's list, nor in the file.
    let listed = admin(&lamp, json!({"clients": true})).await;
    assert!(listed["clients"].as_array().unwrap().is_empty(), "{listed}");
    assert!(!lamp.dir.join("clients.json").exists());
}

#[tokio::test]
async fn no_token_a_wrong_one_and_an_ended_one_are_refused() {
    let lamp = lamp("run-none").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    // No token at all, and one that is no run's.
    for token in [None, Some("A".repeat(43))] {
        let mut socket = agent_connect(&path).await.unwrap();
        let hello = run_hello(&mut socket, token.as_deref()).await;
        assert_eq!(hello["error"]["code"], "no_grant", "{hello}");
        assert_eq!(hello["error"]["why"], "no token for this run", "{hello}");
        assert!(
            agent_next(&mut socket).await.is_none(),
            "the connection closed"
        );
    }
    // A token that is not the right shape is a bad request, before any lookup.
    let mut socket = agent_connect(&path).await.unwrap();
    let hello = run_hello(&mut socket, Some("short")).await;
    assert_eq!(hello["ok"], false, "{hello}");
    // An ended run's token: refused, and its open connection closes at once.
    let minted = mint(&lamp, "booth", "r2").await;
    let token = minted["token"].as_str().unwrap().to_owned();
    let mut open = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut open, Some(&token)).await["ok"], true);
    let ended = admin(&lamp, json!({"end": {"run": "r2"}})).await;
    assert_eq!(ended["ended"], true, "{ended}");
    assert!(
        agent_next(&mut open).await.is_none(),
        "the run's connection closed with it"
    );
    let mut late = agent_connect(&path).await.unwrap();
    let hello = run_hello(&mut late, Some(&token)).await;
    assert_eq!(hello["error"]["code"], "no_grant", "{hello}");
    assert_eq!(
        admin(&lamp, json!({"end": {"run": "r2"}})).await["ended"],
        false
    );
}

#[tokio::test]
async fn a_token_is_minted_only_from_the_owners_yes() {
    let lamp = lamp("run-yes").await;
    // Before the run socket is on: off.
    let off = mint(&lamp, "booth", "r1").await;
    assert_eq!(off["error"], "skill runs are off on this lamp", "{off}");
    let _path = run_socket(&lamp, my_uid(&lamp)).await;
    // A skill whose folder changed since the yes: the yes's own words.
    let changed = mint(&lamp, "changed", "r1").await;
    assert!(
        changed["error"].as_str().unwrap().contains("ask again"),
        "{changed}"
    );
    let none = mint(&lamp, "nothing", "r1").await;
    assert_eq!(none["error"], "no such skill", "{none}");
    let empty = mint(&lamp, "empty", "r1").await;
    assert!(
        empty["error"].as_str().unwrap().contains("no grant"),
        "{empty}"
    );
    // Names that could carry a path or a space are refused.
    for (skill, run) in [("../x", "r"), ("booth", "a b"), ("booth", "")] {
        let bad = mint(&lamp, skill, run).await;
        assert!(bad["error"].as_str().unwrap().starts_with("mint:"), "{bad}");
    }
    // What lamp-tasks says of the grants is no part of it: the yes decides.
    let said = admin(
        &lamp,
        json!({"mint": {"skill": "booth", "run": "r9", "grants": ["camera"]}}),
    )
    .await;
    assert_eq!(said["grants"], json!(["light"]), "{said}");
}

#[tokio::test]
async fn the_run_socket_is_not_for_pairing_and_the_other_doors_take_no_token() {
    let lamp = lamp("run-pair").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let minted = mint(&lamp, "booth", "r1").await;
    let token = minted["token"].as_str().unwrap().to_owned();
    let mut socket = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut socket, Some(&token)).await["ok"], true);
    socket
        .send(Message::text(
            json!({"id": 2, "op": "pair", "name": "x", "key": STANDARD.encode([1u8; 32]), "proof": STANDARD.encode([2u8; 32])}).to_string(),
        ))
        .await
        .unwrap();
    let pair = agent_next(&mut socket).await.unwrap();
    assert!(
        pair["error"]["why"]
            .as_str()
            .unwrap()
            .contains("not for pairing"),
        "{pair}"
    );
    // The TLS door takes no token: a hello with one is refused, never read.
    let mut wss = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut wss).await;
    let hello = send(
        &mut wss,
        json!({"id": 1, "op": "hello", "client": "c_0", "sig": STANDARD.encode([0u8; 64]), "token": token}),
    )
    .await;
    assert_eq!(hello["ok"], false, "{hello}");
    assert!(
        hello["error"]["why"]
            .as_str()
            .unwrap()
            .contains("only on a skill run's socket"),
        "{hello}"
    );
    // Nor the local agent's socket.
    let (agent_path, _client) = agent_socket(&lamp, "agent", my_uid(&lamp)).await;
    let mut agent = agent_connect(&agent_path).await.unwrap();
    let _welcome = agent_next(&mut agent).await;
    agent
        .send(Message::text(
            json!({"id": 1, "op": "hello", "client": "c_0", "sig": STANDARD.encode([0u8; 64]), "token": token}).to_string(),
        ))
        .await
        .unwrap();
    let refused = agent_next(&mut agent).await.unwrap();
    assert!(
        refused["error"]["why"]
            .as_str()
            .unwrap()
            .contains("only on a skill run's socket"),
        "{refused}"
    );
}

/// The hello an SDK sends on the run socket (client c_0, a zero sig, the
/// token) is the run socket's alone: the TLS door and the agent socket refuse
/// the same frame, and the run socket refuses a real client's id or
/// signature beside a valid token.
#[tokio::test]
async fn the_run_hello_is_accepted_only_on_the_run_socket() {
    let lamp = lamp("run-only").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let minted = mint(&lamp, "booth", "r1").await;
    let token = minted["token"].as_str().unwrap().to_owned();
    let frame = json!({"id": 1, "op": "hello", "client": "c_0", "sig": STANDARD.encode([0u8; 64]), "token": token});
    // The TLS door: refused.
    let mut wss = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut wss).await;
    let refused = send(&mut wss, frame.clone()).await;
    assert_eq!(refused["ok"], false, "{refused}");
    // The same frame without a token on the TLS door: unpaired, as ever.
    let mut bare = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut bare).await;
    let mut no_token = frame.clone();
    no_token.as_object_mut().unwrap().remove("token");
    let unpaired = send(&mut bare, no_token).await;
    assert_eq!(unpaired["error"]["code"], "unpaired", "{unpaired}");
    // The agent socket: refused (with the local agent on).
    let (agent_path, _client) = agent_socket(&lamp, "agent", my_uid(&lamp)).await;
    let mut agent = agent_connect(&agent_path).await.unwrap();
    let _welcome = agent_next(&mut agent).await;
    agent.send(Message::text(frame.to_string())).await.unwrap();
    let refused = agent_next(&mut agent).await.unwrap();
    assert_eq!(refused["ok"], false, "{refused}");
    // The run socket: accepted.
    let mut run = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut run, Some(&token)).await["ok"], true);
    // A real client's id, or a real-looking signature, beside the token: refused.
    for (client, sig) in [
        ("c_1234abcd", STANDARD.encode([0u8; 64])),
        ("c_0", STANDARD.encode([7u8; 64])),
    ] {
        let mut socket = agent_connect(&path).await.unwrap();
        let _welcome = agent_next(&mut socket).await;
        socket
            .send(Message::text(
                json!({"id": 1, "op": "hello", "client": client, "sig": sig, "token": token})
                    .to_string(),
            ))
            .await
            .unwrap();
        let refused = agent_next(&mut socket).await.unwrap();
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["error"]["code"], "bad_request", "{refused}");
        assert!(agent_next(&mut socket).await.is_none(), "closed");
    }
}

#[tokio::test]
async fn a_second_mint_for_a_run_closes_the_first_and_its_token_is_dead() {
    let lamp = lamp("run-twice").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let first = mint(&lamp, "booth", "r1").await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut open = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut open, Some(&first)).await["ok"], true);
    let second = mint(&lamp, "booth", "r1").await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first, second);
    assert!(
        agent_next(&mut open).await.is_none(),
        "the first token's connection closed"
    );
    let mut old = agent_connect(&path).await.unwrap();
    assert_eq!(
        run_hello(&mut old, Some(&first)).await["error"]["code"],
        "no_grant"
    );
    let mut new = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut new, Some(&second)).await["ok"], true);
}

#[tokio::test]
async fn a_run_ends_at_its_time_and_a_peer_with_another_uid_never_reaches_the_door() {
    let lamp = lamp("run-time").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let minted = admin(
        &lamp,
        json!({"mint": {"skill": "booth", "run": "r1", "expires_in_ms": 1000}}),
    )
    .await;
    let token = minted["token"].as_str().unwrap().to_owned();
    let mut open = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut open, Some(&token)).await["ok"], true);
    // Its time passes on the door's clock: the sweep ends the run and its
    // connection, and the token is no token.
    lamp.shared.move_clock_on(Duration::from_secs(2));
    assert!(
        agent_next(&mut open).await.is_none(),
        "closed when its time was up"
    );
    let mut late = agent_connect(&path).await.unwrap();
    assert_eq!(
        run_hello(&mut late, Some(&token)).await["error"]["code"],
        "no_grant"
    );
    // The lamp names a uid that is not this process's: closed before a byte.
    let other = lamp_run_other_uid().await;
    assert!(other, "a peer with another uid reached the door");
}

/// A run socket taking another uid than this process's refuses its peer
/// before any byte is read.
async fn lamp_run_other_uid() -> bool {
    let lamp = lamp("run-uid").await;
    let path = run_socket(&lamp, my_uid(&lamp) + 1).await;
    tokio::time::timeout(Duration::from_secs(3), agent_connect(&path))
        .await
        .expect("closed in time")
        .is_err()
}

/// A run's connection says why it closed, as a revoke's does: the run ended
/// (`end`), and a newer token for the same run (its old one's connection).
#[tokio::test]
async fn a_runs_connection_says_why_it_closed() {
    let lamp = lamp("run-close").await;
    let path = run_socket(&lamp, my_uid(&lamp)).await;
    let closed = |mut socket: AgentSocket| async move {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match socket.next().await {
                    Some(Ok(Message::Close(Some(frame)))) => return Some(frame.reason.to_string()),
                    None | Some(Err(_) | Ok(Message::Close(None))) => return None,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await
        .expect("the connection closes")
    };
    let first = mint(&lamp, "booth", "r1").await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut one = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut one, Some(&first)).await["ok"], true);
    let second = mint(&lamp, "booth", "r1").await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        closed(one).await.as_deref(),
        Some(lamp_control::runs::CLOSED_AGAIN)
    );
    let mut two = agent_connect(&path).await.unwrap();
    assert_eq!(run_hello(&mut two, Some(&second)).await["ok"], true);
    admin(&lamp, json!({"end": {"run": "r1"}})).await;
    assert_eq!(
        closed(two).await.as_deref(),
        Some(lamp_control::runs::CLOSED_ENDED)
    );
}

/// The agent's record is kept before anything runs on it: when it cannot be
/// written, the record made in memory is taken out again, so the error is
/// told on a retry too and no unsaved record is ever used (eighth's P3 on #6380).
#[test]
fn a_local_record_that_could_not_be_kept_is_not_left_in_memory() {
    let missing = std::env::temp_dir()
        .join(format!("lamp-control-nokeep-{}", std::process::id()))
        .join("no-such-folder")
        .join("clients.json");
    let mut door = session::Door::new(
        "lamp-test",
        "Lamp",
        [0u8; 32],
        pairing::Clients::default(),
        Some(missing),
    );
    assert!(door.ensure_local("agent", 1).is_err());
    assert!(door.clients().clients.is_empty(), "nothing left in memory");
    assert!(
        door.ensure_local("agent", 2).is_err(),
        "told again, not taken as made"
    );
}

/// Parts that route only `speaker.stream`, to a real speaker with a fake voice
/// and publisher: what the door does with a computer's binary messages.
struct Speaking {
    speaker: Arc<lamp_control::speaker::Speaker>,
    events: lamp_control::parts::Events,
    asking: Option<u64>,
}

impl session::Parts for Speaking {
    fn act(
        &mut self,
        client: &str,
        _: u64,
        op: &lamp_control::protocol::Op,
        _: u64,
    ) -> Result<Value, lamp_control::protocol::Error> {
        match op {
            lamp_control::protocol::Op::SpeakerStream { on, sample_rate } => self.speaker.stream(
                self.asking.expect("a connection"),
                client,
                client,
                *on,
                *sample_rate,
                self.events.clone(),
            ),
            _ => Err(lamp_control::protocol::Error::new(
                lamp_control::protocol::ErrorCode::BadRequest,
                "not an action",
            )),
        }
    }
    fn state(&self, _: lamp_control::protocol::StateKind) -> Value {
        Value::Null
    }
    fn stop(&mut self, client: &str, _: u64) {
        self.speaker.end_client(client);
    }
    fn asking(&mut self, connection: Option<u64>) {
        self.asking = connection;
    }
}

struct OpenVoice;

struct OpenStream;

impl lamp_control::speaker::Stream for OpenStream {
    fn poll(&mut self) -> Option<lamp_control::speaker::Note> {
        None
    }
}

impl lamp_control::speaker::Voice for OpenVoice {
    fn open(
        &self,
        _: &str,
    ) -> Result<Box<dyn lamp_control::speaker::Stream>, lamp_control::protocol::Error> {
        Ok(Box::new(OpenStream))
    }
}

/// A lamp whose parts are [`Speaking`], with the blocks its stream publishes kept.
async fn speaking_lamp(
    name: &str,
) -> (
    Lamp,
    Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    Arc<lamp_control::speaker::Speaker>,
) {
    speaking_lamp_switched(name, true).await
}

/// [`speaking_lamp`] with the owner's `--speaker` switch as given.
async fn speaking_lamp_switched(
    name: &str,
    switched: bool,
) -> (
    Lamp,
    Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    Arc<lamp_control::speaker::Speaker>,
) {
    let blocks = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let kept = blocks.clone();
    let speaker = Arc::new(
        lamp_control::speaker::Speaker::new(Arc::new(|| false), Arc::new(|| false))
            .playing(
                Arc::new(OpenVoice),
                Arc::new(move |payload| {
                    kept.lock().unwrap().push(payload.to_vec());
                    Ok(())
                }),
            )
            .marking(
                Arc::new(|_, _| {}),
                Arc::new(|_| Some(std::time::Instant::now())),
            )
            .switched(switched),
    );
    let for_parts = speaker.clone();
    let lamp = lamp_made(
        name,
        move |events| {
            Box::new(Speaking {
                speaker: for_parts,
                events,
                asking: None,
            })
        },
        |door, _| door,
    )
    .await;
    lamp.shared.set_speaker(speaker.clone());
    (lamp, blocks, speaker)
}

#[tokio::test]
async fn a_computers_binary_messages_play_through_its_own_stream_and_nothing_else_is_answered() {
    let (lamp, blocks, _) = speaking_lamp("speaker-binary").await;
    // A frame as docs/control.md gives it: the header's length (4 bytes,
    // big-endian), the JSON header, then the samples.
    let samples = |count: usize| -> Vec<u8> {
        (0..count)
            .flat_map(|n| ((n as i16) * 3).to_le_bytes())
            .collect()
    };
    let frame = |count: usize| -> Vec<u8> {
        let header = br#"{"ev":"speaker.audio","sequence":5}"#;
        let mut frame = (header.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(header);
        frame.extend_from_slice(&samples(count));
        frame
    };
    // Before hello, and with no stream: binary is dropped, with no word back.
    let mut early = connect(&lamp, "/v1").await.unwrap();
    let _welcome = next(&mut early).await;
    early.send(Message::binary(frame(480))).await.unwrap();
    let mut socket = paired(&lamp, json!(["speaker"]), 7).await;
    socket.send(Message::binary(frame(480))).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), socket.next())
            .await
            .is_err(),
        "a stray binary message gets no answer"
    );
    assert!(blocks.lock().unwrap().is_empty());
    // The stream on: its frames play, one block each, 24 kHz as they came.
    let answer = send(
        &mut socket,
        json!({"id": 3, "op": "speaker.stream", "on": true, "sample_rate": 24000}),
    )
    .await;
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["stream"], "on");
    socket.send(Message::binary(frame(480))).await.unwrap();
    socket.send(Message::binary(frame(480))).await.unwrap();
    for _ in 0..100 {
        if blocks.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(blocks.lock().unwrap().len(), 2, "two frames, two blocks");
    // A frame with its header plays only its samples: the length and the JSON
    // are not sound (the Reviewer-lead's P2 on S2).
    {
        let blocks = blocks.lock().unwrap();
        let (_, played) = lamp_msgs::Audio::split(&blocks[0]).unwrap();
        assert_eq!(played, &samples(480)[..], "the header is not played");
    }
    // A message with no header of ours is stray, whatever its size: bare
    // samples play nothing and the stream's count of blocks stays.
    socket.send(Message::binary(samples(480))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(blocks.lock().unwrap().len(), 2, "bare samples are no frame");
    // A second connection of the same computer is not the stream's: its frames
    // are stray, played nowhere.
    let (mut second, _) = {
        let mut other = connect(&lamp, "/v1").await.unwrap();
        let welcome = next(&mut other).await;
        let _ = welcome;
        (other, ())
    };
    second.send(Message::binary(frame(480))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(blocks.lock().unwrap().len(), 2);
    // Its own off ends it: later frames play nothing.
    let off = send(
        &mut socket,
        json!({"id": 4, "op": "speaker.stream", "on": false}),
    )
    .await;
    assert_eq!(off["stream"], "off");
    socket.send(Message::binary(frame(480))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(blocks.lock().unwrap().len(), 2);
}

/// The Reviewer-lead's P3 on S2: through the door, `speaker.stream` is refused
/// without the `speaker` grant and, on a lamp whose owner has not switched it
/// on, in the owner's words; neither opens a stream or plays a block. (A
/// computer beyond the lamp's network is refused in `session.rs`' unit test
/// `the_speaker_stream_needs_its_grant_and_is_never_given_beyond_the_lamps_network`:
/// the door's own peers here are all local.)
#[tokio::test]
async fn the_speaker_stream_is_refused_without_its_grant_and_with_the_switch_off() {
    let (lamp, blocks, speaker) = speaking_lamp("speaker-refused").await;
    let mut plain = paired(&lamp, json!(["light", "camera", "microphone"]), 11).await;
    let refused = send(
        &mut plain,
        json!({"id": 3, "op": "speaker.stream", "on": true}),
    )
    .await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "no_grant", "{refused}");
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    assert!(!speaker.streaming(&client), "no stream without the grant");
    assert!(blocks.lock().unwrap().is_empty());
    // With the grant but the owner's switch off.
    let (lamp, blocks, _) = speaking_lamp_switched("speaker-off", false).await;
    let mut socket = paired(&lamp, json!(["speaker"]), 12).await;
    let off = send(
        &mut socket,
        json!({"id": 3, "op": "speaker.stream", "on": true}),
    )
    .await;
    assert_eq!(off["ok"], false, "{off}");
    assert_eq!(
        off["error"]["why"],
        lamp_control::speaker::SWITCHED_OFF,
        "{off}"
    );
    socket.send(Message::binary(vec![0u8; 960])).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(blocks.lock().unwrap().is_empty());
}

/// The app's `control.set_grants` taking `speaker` from a computer whose stream
/// runs ends that stream at once, as it ends a camera's or a microphone's (the
/// door's media list in `Session::set_grants`, nine's merge-order P2 on S2).
#[tokio::test]
async fn the_apps_set_grants_without_speaker_ends_its_stream() {
    let (lamp, _, speaker) = speaking_lamp("speaker-set-grants").await;
    let mut socket = paired(&lamp, json!(["light", "speaker", "body"]), 9).await;
    let answer = send(
        &mut socket,
        json!({"id": 3, "op": "speaker.stream", "on": true, "sample_rate": 24000}),
    )
    .await;
    assert_eq!(answer["ok"], true, "{answer}");
    let listed = admin(&lamp, json!({"clients": true})).await;
    let client = listed["clients"][0]["id"].as_str().unwrap().to_owned();
    assert!(speaker.streaming(&client), "the stream runs");
    // The speaker taken away: the stream ends now.
    let set = admin(
        &lamp,
        json!({"set_grants": {"client": client, "grants": ["light", "body"]}}),
    )
    .await;
    assert_eq!(set["removed"], json!(["speaker"]), "{set}");
    // Checked the moment the app has its answer. The computer's connection is also
    // closed by any grant change, which ends its stream too, so this end-to-end check
    // cannot tell the two apart; `Door::set_grants`' media list is pinned by the unit
    // test `the_apps_grant_change_that_drops_the_speaker_lists_its_stream_to_end`.
    assert!(
        !speaker.streaming(&client),
        "the stream ended with its grant"
    );
}
