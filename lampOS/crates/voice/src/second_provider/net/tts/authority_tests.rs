//! Per-send socket controls use real V3 authority and actual local transports.
use super::*;
use crate::second_provider::services::tts_authority_tests::{Authority, Http};
use std::sync::atomic::AtomicUsize;

struct SocketServer(Option<tokio::task::JoinHandle<usize>>);
impl SocketServer {
    async fn finish(&mut self) -> usize {
        let count = tokio::time::timeout(Duration::from_secs(2), self.0.as_mut().unwrap())
            .await
            .unwrap()
            .unwrap();
        self.0.take();
        count
    }
}
impl Drop for SocketServer {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

#[tokio::test]
async fn real_socket_failure_and_each_later_tts_send_recheck_the_kernel_ticket() {
    for at_message in [false, true] {
        for revoke in [false, true] {
            for per_send in [false, true] {
                let authority = Arc::new(Authority::new());
                let permission = authority.permission();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("ws://{}/speech", listener.local_addr().unwrap());
                let mut server = SocketServer(Some(tokio::spawn(async move {
                    tokio::time::timeout(Duration::from_secs(2), async {
                        let (socket, _) = listener.accept().await.unwrap();
                        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                        let mut received = 0;
                        while let Some(frame) = socket.next().await {
                            match frame {
                                Ok(Frame::Text(text)) => {
                                    received += 1;
                                    if received == 1 {
                                        assert!(text.contains("PRIVATE_SOCKET_CANARY"));
                                    }
                                    if received == 2 {
                                        let _ = socket.send(Frame::Close(None)).await;
                                        break;
                                    }
                                }
                                Ok(Frame::Close(_)) | Err(_) => break,
                                _ => {}
                            }
                        }
                        received
                    })
                    .await
                    .expect("bounded real websocket fixture")
                })));
                let (socket, _) = tokio::time::timeout(
                    Duration::from_secs(2),
                    tokio_tungstenite::connect_async(&url),
                )
                .await
                .unwrap()
                .unwrap();
                let mut http = Http::start(200).await;
                let send_polls = Arc::new(AtomicUsize::new(0));
                let mut tts = Tts::with(TtsProvider::ElevenLabs, &http.url, "fixture", None)
                    .unwrap()
                    .counting_send_polls(send_polls.clone());
                *tts.ahead.lock().unwrap() = Some(Ahead::Open(7, Box::new(socket), Instant::now()));
                let calls = Arc::new(AtomicUsize::new(0));
                let observed = calls.clone();
                let close = authority.clone();
                let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    if observed.fetch_add(1, Ordering::AcqRel) == 0 && revoke {
                        close.revoke();
                    }
                });
                if at_message {
                    tts.after_socket_message = Some(hook);
                } else {
                    tts.after_socket_attempt = Some(hook);
                }
                let spoken = tokio::time::timeout(Duration::from_secs(3), async {
                    let mut speech = tts
                        .speak_permitted(
                            "PRIVATE_SOCKET_CANARY",
                            Some(Turn {
                                request: 7,
                                heard_at: Instant::now(),
                            }),
                            per_send.then_some(&permission),
                        )
                        .await
                        .unwrap();
                    let mut packets = 0;
                    if let Some(speech) = speech.as_mut() {
                        while speech.next().await.unwrap().is_some() {
                            packets += 1;
                        }
                    }
                    packets
                })
                .await
                .unwrap();
                let messages = server.finish().await;
                http.finish().await;
                let blocked = revoke && per_send;
                assert_eq!(
                    messages,
                    if blocked && at_message { 1 } else { 2 },
                    "at_message={at_message},revoke={revoke},per_send={per_send}"
                );
                assert_eq!(send_polls.load(Ordering::Acquire), usize::from(!blocked));
                assert_eq!(http.bodies.lock().unwrap().len(), usize::from(!blocked));
                assert_eq!(spoken, usize::from(!blocked));
                if !blocked {
                    assert!(
                        String::from_utf8_lossy(&http.bodies.lock().unwrap()[0])
                            .contains("PRIVATE_SOCKET_CANARY")
                    );
                }
            }
        }
    }
}
