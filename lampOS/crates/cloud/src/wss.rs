//! MQTT over WSS when the broker's TCP port keeps failing (#6256, F9): a
//! network that drops port 1883 (or the TLS port) still lets HTTPS through,
//! and the backend takes the same login on `mqtt_ws_url` from the ping's
//! reply (B11). rumqttc is kept as it is, with no websocket feature of its
//! own: while on WSS it connects to a bridge on 127.0.0.1, and the bridge
//! carries the MQTT bytes as binary WebSocket frames, sub-protocol `mqtt`,
//! over TLS with the ping's roots. The bridge lives only while on WSS, and
//! it tries the broker's TCP port again every [`TCP_AGAIN`].

use futures_util::{SinkExt, StreamExt};
use rumqttc::tokio_rustls::TlsConnector;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

/// Failed connections in a row over TCP, none of them reaching the broker's
/// CONNACK, before the lamp goes over WSS.
pub const FAILURES: u32 = 3;

/// How long one connection may take, TCP to CONNACK, on either transport.
pub const CONNECT: Duration = Duration::from_secs(10);

/// How often a lamp on WSS tries the broker's TCP port again.
pub const TCP_AGAIN: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    Tcp,
    Wss,
}

/// The transport after `tcp_failures` failed connections in a row, with
/// `ws_url` the ping's last `mqtt_ws_url`: WSS only with a `wss://` address.
pub fn over(tcp_failures: u32, ws_url: &str) -> Over {
    if tcp_failures >= FAILURES && usable(ws_url) {
        Over::Wss
    } else {
        Over::Tcp
    }
}

/// `wss://` only; the tests' local server is plain `ws://`.
fn usable(url: &str) -> bool {
    url.starts_with("wss://") || (cfg!(test) && url.starts_with("ws://"))
}

/// The bridge while on WSS: rumqttc connects to `127.0.0.1:port`. Dropped,
/// it stops carrying and stops trying TCP.
pub struct Bridge {
    pub port: u16,
    /// Told once the broker's TCP port took a connection again.
    pub tcp_back: Arc<Notify>,
    task: JoinHandle<()>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Bridge {
    /// A bridge to `url`, trying `tcp` (the broker's host and port) again
    /// every `again`.
    pub async fn open(url: String, tcp: (String, u16), again: Duration) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let tcp_back = Arc::new(Notify::new());
        let told = tcp_back.clone();
        let task = tokio::spawn(async move {
            // One connection at a time: rumqttc makes them one after another.
            let carrying = async {
                while let Ok((local, _)) = listener.accept().await {
                    carry(local, &url).await;
                }
            };
            let probing = async {
                // ponytail: a TCP connect, not a whole MQTT CONNECT; if the
                // network lets the SYN through but not MQTT, the lamp falls
                // back to WSS again after FAILURES.
                loop {
                    tokio::time::sleep(again).await;
                    let trying = TcpStream::connect((tcp.0.as_str(), tcp.1));
                    if let Ok(Ok(_)) = tokio::time::timeout(CONNECT, trying).await {
                        told.notify_one();
                        return;
                    }
                }
            };
            tokio::join!(carrying, probing);
        });
        Ok(Self {
            port,
            tcp_back,
            task,
        })
    }
}

/// Carries one rumqttc connection to `url` until either side ends it.
async fn carry(local: TcpStream, url: &str) {
    let Ok(mut request) = url.into_client_request() else {
        return;
    };
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("mqtt"));
    let Some(host) = request.uri().host().map(str::to_owned) else {
        return;
    };
    let tls = request.uri().scheme_str() == Some("wss");
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if tls { 443 } else { 80 });
    let Ok(Ok(tcp)) =
        tokio::time::timeout(CONNECT, TcpStream::connect((host.as_str(), port))).await
    else {
        return;
    };
    if !tls {
        return pump(local, request, tcp).await;
    }
    let (Ok(config), Ok(name)) = (
        crate::beat::tls(),
        rustls::pki_types::ServerName::try_from(host),
    ) else {
        return;
    };
    if let Ok(stream) = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
    {
        pump(local, request, stream).await;
    }
}

async fn pump<S: AsyncRead + AsyncWrite + Unpin>(
    mut local: TcpStream,
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    stream: S,
) {
    let Ok((ws, _)) = tokio_tungstenite::client_async(request, stream).await else {
        return;
    };
    let (mut sink, mut frames) = ws.split();
    let (mut read, mut write) = local.split();
    let up = async {
        let mut buffer = vec![0u8; 16 * 1024];
        while let Ok(n) = read.read(&mut buffer).await {
            if n == 0
                || sink
                    .send(Message::binary(buffer[..n].to_vec()))
                    .await
                    .is_err()
            {
                break;
            }
        }
        let _ = sink.close().await;
    };
    let down = async {
        while let Some(Ok(frame)) = frames.next().await {
            match frame {
                Message::Binary(bytes) => {
                    if write.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    tokio::select! {
        () = up => {}
        () = down => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    #[test]
    fn wss_only_after_the_failures_and_with_an_address() {
        let url = "wss://broker.test/mqtt";
        assert_eq!(over(0, url), Over::Tcp);
        assert_eq!(over(FAILURES - 1, url), Over::Tcp);
        assert_eq!(over(FAILURES, url), Over::Wss);
        assert_eq!(over(FAILURES + 5, url), Over::Wss);
        assert_eq!(over(FAILURES, ""), Over::Tcp, "no address: TCP");
        assert_eq!(over(FAILURES, "https://x/mqtt"), Over::Tcp);
    }

    /// A WebSocket server on a free local port that answers `mqtt` and
    /// sends back each binary frame reversed. tungstenite's handshake
    /// callback has a large `Err` by its own signature, as in lamp-control.
    #[allow(clippy::result_large_err)]
    async fn server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/mqtt", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_hdr_async(
                tcp,
                |asked: &Request, mut answer: Response| {
                    assert_eq!(asked.uri().path(), "/mqtt");
                    assert_eq!(asked.headers()["Sec-WebSocket-Protocol"], "mqtt");
                    answer
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("mqtt"));
                    Ok(answer)
                },
            )
            .await
            .unwrap();
            let (mut sink, mut frames) = ws.split();
            while let Some(Ok(Message::Binary(bytes))) = frames.next().await {
                let mut back = bytes.to_vec();
                back.reverse();
                sink.send(Message::binary(back)).await.unwrap();
            }
        });
        url
    }

    #[tokio::test]
    async fn the_bridge_carries_mqtt_bytes_both_ways() {
        let url = server().await;
        let bridge = Bridge::open(url, ("127.0.0.1".into(), 9), Duration::from_secs(3600))
            .await
            .unwrap();
        let mut local = TcpStream::connect(("127.0.0.1", bridge.port))
            .await
            .unwrap();
        // An MQTT PINGREQ and some more, as rumqttc would write them.
        local.write_all(&[0xC0, 0x00, 1, 2, 3]).await.unwrap();
        let mut back = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(5), local.read_exact(&mut back))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(back, [3, 2, 1, 0x00, 0xC0]);
    }

    #[tokio::test]
    async fn the_bridge_says_tcp_is_back_once_the_port_takes_a_connection() {
        let broker = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = broker.local_addr().unwrap().port();
        let bridge = Bridge::open(
            "ws://127.0.0.1:9/mqtt".into(),
            ("127.0.0.1".into(), port),
            Duration::from_millis(50),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), bridge.tcp_back.notified())
            .await
            .expect("TCP back after the retry interval");
    }

    #[tokio::test]
    async fn a_dropped_bridge_stops_listening() {
        let bridge = Bridge::open(
            "ws://127.0.0.1:9/mqtt".into(),
            ("127.0.0.1".into(), 9),
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        let port = bridge.port;
        drop(bridge);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }
}
