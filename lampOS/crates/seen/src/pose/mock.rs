//! A local stand-in for the perception service's pose socket, speaking V1's
//! key exchange with the throwaway test key (`tests/fixtures/v1-vector.json`,
//! no server holds it), for the tests only. It logs what it was sent and
//! when the lamp closed the socket.
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand_core::RngCore;
use rsa::{BigUint, Oaep, RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::{
    Message,
    handshake::server::{ErrorResponse, Request, Response},
    http::StatusCode,
};

const VECTOR: &str = include_str!("../../tests/fixtures/v1-vector.json");

fn number(vector: &Value, name: &str) -> BigUint {
    BigUint::parse_bytes(vector[name].as_str().unwrap().as_bytes(), 16).unwrap()
}

pub(crate) fn private() -> RsaPrivateKey {
    let vector: Value = serde_json::from_str(VECTOR).unwrap();
    RsaPrivateKey::from_components(
        number(&vector, "n"),
        number(&vector, "e"),
        number(&vector, "d"),
        vec![number(&vector, "p"), number(&vector, "q")],
    )
    .unwrap()
}

pub(crate) fn public() -> RsaPublicKey {
    private().to_public_key()
}

/// How the stand-in answers one connection.
#[derive(Clone, Debug)]
pub(crate) enum Reply {
    /// Answer the frame with this `ergo` (none: no body found).
    Ergo(Option<Value>),
    Busy,
    /// `{"error":"Processing failed"}`.
    Failed,
    /// Refuse the socket's handshake with this HTTP status.
    Http(u16),
    /// Answer the key exchange with something else.
    RefuseKeyExchange,
    /// Wait this long before accepting the key exchange.
    SlowKeyExchange(Duration),
    /// Take the frame and never answer.
    Silent,
}

/// What the stand-in saw, in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Seen {
    Connected {
        api_key: Option<String>,
        path: String,
    },
    KeyExchange,
    Config(Value),
    Frame(Vec<u8>),
    /// The lamp closed the socket (a close frame or the TCP end).
    Closed,
}

pub(crate) struct Mock {
    pub(crate) address: SocketAddr,
    replies: Arc<Mutex<VecDeque<Reply>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    /// Listening on this computer, answering each connection by the next
    /// reply (the last one repeats).
    pub(crate) fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (queue, log) = (Arc::clone(&replies), Arc::clone(&seen));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let reply = {
                    let mut queue = queue.lock().unwrap();
                    if queue.len() > 1 {
                        queue.pop_front().unwrap()
                    } else {
                        queue.front().cloned().unwrap_or(Reply::Busy)
                    }
                };
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, reply, log));
            }
        });
        Self {
            address,
            replies,
            seen,
        }
    }

    pub(crate) fn base(&self) -> String {
        format!("http://127.0.0.1:{}/api/v1/ai", self.address.port())
    }

    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn then(&self, reply: Reply) {
        let mut queue = self.replies.lock().unwrap();
        queue.clear();
        queue.push_back(reply);
    }

    /// Wait until `test` holds of what was seen, up to 5 s.
    pub(crate) fn wait_for(&self, test: impl Fn(&[Seen]) -> bool) -> Vec<Seen> {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = self.seen();
            if test(&seen) || Instant::now() > until {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Server {
    cipher: Option<Aes256Gcm>,
}

impl Server {
    fn open(&self, raw: &str) -> Value {
        let message: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(message["type"], "encrypted", "nothing in the clear");
        let nonce = STANDARD.decode(message["nonce"].as_str().unwrap()).unwrap();
        let sealed = STANDARD
            .decode(message["cipher_data"].as_str().unwrap())
            .unwrap();
        let plain = self
            .cipher
            .as_ref()
            .unwrap()
            .decrypt(Nonce::from_slice(&nonce), sealed.as_slice())
            .unwrap();
        serde_json::from_slice(&plain).unwrap()
    }

    fn seal(&self, value: &Value) -> String {
        let mut nonce = [0u8; 12];
        rand_core::OsRng.fill_bytes(&mut nonce);
        let sealed = self
            .cipher
            .as_ref()
            .unwrap()
            .encrypt(Nonce::from_slice(&nonce), value.to_string().as_bytes())
            .unwrap();
        json!({"nonce": STANDARD.encode(nonce), "cipher_data": STANDARD.encode(sealed), "type": "encrypted"})
            .to_string()
    }
}

// tungstenite's handshake callback returns its own `ErrorResponse` (an HTTP
// response) as the error; its size is not ours to choose.
#[allow(clippy::result_large_err)]
fn serve(stream: TcpStream, reply: Reply, log: Arc<Mutex<Vec<Seen>>>) {
    let push = |seen: Seen| log.lock().unwrap().push(seen);
    let refuse = match reply {
        Reply::Http(status) => Some(status),
        _ => None,
    };
    let connected = Arc::clone(&log);
    let callback = move |request: &Request, response: Response| {
        connected.lock().unwrap().push(Seen::Connected {
            api_key: request
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            path: request.uri().path().to_owned(),
        });
        match refuse {
            Some(status) => {
                let mut refused = ErrorResponse::new(None);
                *refused.status_mut() = StatusCode::from_u16(status).unwrap();
                Err(refused)
            }
            None => Ok(response),
        }
    };
    let socket = tungstenite::accept_hdr(stream, callback);
    let Ok(mut socket) = socket else {
        return;
    };
    let mut server = Server { cipher: None };
    loop {
        let message = match socket.read() {
            Ok(Message::Text(text)) => text.as_str().to_owned(),
            Ok(Message::Close(_)) | Err(_) => {
                push(Seen::Closed);
                // Let the close handshake finish.
                let _ = socket.flush();
                return;
            }
            Ok(_) => continue,
        };
        if server.cipher.is_none() {
            let exchange: Value = serde_json::from_str(&message).unwrap();
            assert_eq!(exchange["type"], "key_exchange");
            let wrapped = STANDARD
                .decode(exchange["encrypted_key"].as_str().unwrap())
                .unwrap();
            let key = private()
                .decrypt(Oaep::new::<sha2::Sha256>(), &wrapped)
                .unwrap();
            server.cipher = Some(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)));
            push(Seen::KeyExchange);
            if let Reply::SlowKeyExchange(wait) = reply {
                std::thread::sleep(wait);
            }
            let ok = match reply {
                Reply::RefuseKeyExchange => json!({"status": "nope"}),
                _ => json!({"status": "key_exchange_ok"}),
            };
            if socket.send(Message::text(ok.to_string())).is_err() {
                push(Seen::Closed);
                return;
            }
            continue;
        }
        let plain = server.open(&message);
        let answer = match plain["type"].as_str() {
            Some("config") => {
                push(Seen::Config(plain.clone()));
                json!({"status": "config_updated"})
            }
            Some("frame") => {
                assert_eq!(plain["task"], "pose");
                let frame = STANDARD
                    .decode(plain["frame_b64"].as_str().unwrap())
                    .unwrap();
                push(Seen::Frame(frame));
                match &reply {
                    Reply::Ergo(Some(ergo)) => {
                        json!({"pose_2d": {"graph_type": "coco", "joints": [[1, 2]], "confs": [0.9]}, "ergo": ergo})
                    }
                    Reply::Ergo(None) => {
                        json!({"pose_2d": {"graph_type": "coco", "joints": [], "confs": []}})
                    }
                    Reply::Busy => json!({"error": "busy"}),
                    Reply::Failed => json!({"error": "Processing failed"}),
                    Reply::Silent => continue,
                    _ => json!({"error": "unexpected"}),
                }
            }
            _ => json!({"error": [{"msg": "unknown"}]}),
        };
        if socket.send(Message::text(server.seal(&answer))).is_err() {
            push(Seen::Closed);
            return;
        }
    }
}

/// A plain HTTP stand-in for `GET <base>/crypto/public-key`: one answer per
/// connection; returns its address and the requests' heads.
pub(crate) fn public_key_server(answer: Vec<u8>) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&heads);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&head).into_owned());
            let _ = stream.write_all(&answer);
        }
    });
    (address, heads)
}
