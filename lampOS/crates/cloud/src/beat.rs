//! The backend's ping, every 15 s (`crate::ping`), sent as V1's
//! `status_reporter` sends it: the voice's key as the bearer, the lamp's
//! state in the body. A reply that hands out a new broker replaces the
//! account, which `crate::online::run` then connects with; the caller keeps
//! it. A ping that fails is said once, and once again when it answers.
use crate::account::Account;
use crate::online::Say;
use crate::ping::{self, Payload};
use crate::wire::Identity;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// How long a ping may take, from its connection to its last byte.
pub const WITHIN: Duration = Duration::from_secs(10);
/// The most of a reply read: V1's is a few fields.
const MOST_REPLY: usize = 64 * 1024;

/// TLS 1.2 and 1.3 with the `ring` provider and the webpki roots, as the
/// other lampOS clients have it.
fn tls() -> Result<rustls::ClientConfig, String> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS setup".to_owned())?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// A client with no connection kept from before: made again when the
/// lamp's address moves, so no ping waits on a path that is gone (V1's
/// `CloseIdleConnections`). HTTPS only, except to the tests' local server.
pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls()?)
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(WITHIN)
        .build()
        .map_err(|_| "HTTPS client setup".to_owned())
}

/// One ping: the reply's body, or why there is none. The key is only in the
/// request's header, never in what is returned.
pub async fn once(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    payload: &Payload,
) -> Result<Vec<u8>, String> {
    let body = serde_json::to_string(payload).map_err(|_| "the body".to_owned())?;
    let mut response = client
        .post(url)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "no answer in time".to_owned()
            } else {
                "no connection".to_owned()
            }
        })?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status().as_u16()));
    }
    let mut read = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "the reply broke off".to_owned())?
    {
        read.extend_from_slice(&chunk);
        if read.len() > MOST_REPLY {
            return Err("the reply is too long".to_owned());
        }
    }
    Ok(read)
}

/// Pings every `every` with the newest account and identity until
/// `accounts` closes. `key` is the bearer. `keep` is given each account a
/// reply moved to, before it is sent on.
pub async fn pings(
    accounts: watch::Sender<Option<Account>>,
    identity: watch::Receiver<Identity>,
    key: String,
    every: Duration,
    say: &mut impl Say,
    keep: &mut impl FnMut(&Account),
) {
    let mut client = match client() {
        Ok(client) => client,
        Err(why) => {
            say.line(&format!("no backend ping: {why}"));
            return;
        }
    };
    let mut address = identity.borrow().local_ip.clone();
    // The ping's last state, said only when it changes.
    let mut failing: Option<String> = None;
    loop {
        if accounts.is_closed() {
            return;
        }
        let account = accounts.borrow().clone();
        let me = identity.borrow().clone();
        if me.local_ip != address {
            address = me.local_ip.clone();
            if let Ok(fresh) = self::client() {
                client = fresh;
            }
        }
        if let Some(account) = account
            && let Some(url) = ping::url(&account)
        {
            match once(&client, &url, &key, &Payload::of(&me)).await {
                Ok(body) => {
                    if failing.take().is_some() {
                        say.line("the backend's ping answers again");
                    }
                    if let Some(next) =
                        ping::reply(&body).and_then(|reply| ping::after(&account, &reply))
                    {
                        keep(&next);
                        say.line(&format!(
                            "the backend gave a new broker: {}:{}",
                            next.mqtt_endpoint, next.mqtt_port
                        ));
                        let _ = accounts.send(Some(next));
                    }
                }
                Err(why) => {
                    if failing.as_deref() != Some(why.as_str()) {
                        say.line(&format!("the backend's ping fails: {why}"));
                        failing = Some(why);
                    }
                }
            }
        }
        tokio::time::sleep(every).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A backend on a free local port: it keeps each request and answers
    /// with `status` and `body`, then, after `fails` requests, 200 and
    /// `{}`.
    async fn backend(answers: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = requests.clone();
        tokio::spawn(async move {
            let mut answers = answers.into_iter();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                // Headers, then the body's length of bytes.
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|n| n.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let (status, body) = answers.next().unwrap_or((200, "{}"));
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (base, requests)
    }

    fn account(base: &str) -> Account {
        Account {
            device_id: "d-42".into(),
            mqtt_endpoint: "broker.test".into(),
            mqtt_port: 1883,
            mqtt_username: "lamp".into(),
            mqtt_password: "s3cret-pw".into(),
            fa_channel: "fa/d-42".into(),
            fd_channel: "fd/d-42".into(),
            llm_base_url: base.into(),
        }
    }

    /// #1396, the first builder's 22:00: the ping is V1's, with the voice's
    /// key as the bearer; failures are said once and the recovery once; a
    /// reply's new broker is kept and replaces the account.
    #[tokio::test]
    async fn the_ping_says_the_lamp_and_takes_a_new_broker_from_its_reply() {
        let (base, requests) = backend(vec![
            (401, ""),
            (401, ""),
            (
                200,
                r#"{"status":"ok","mqtt":{"mqtt_server":"b2.test","mqtt_port":"1884","mqtt_usr":"u2","mqtt_pwd":"p2"}}"#,
            ),
        ])
        .await;
        let (accounts, mut watched) = watch::channel(Some(account(&base)));
        let (_me_tx, me) = watch::channel(Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: String::new(),
            local_ip: "192.168.1.42".into(),
        });
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = Arc::new(Mutex::new(Vec::<Account>::new()));
        let (said, keeping) = (lines.clone(), kept.clone());
        let pinging = tokio::spawn(async move {
            let mut say = |line: &str| said.lock().unwrap().push(line.to_owned());
            let mut keep = |account: &Account| keeping.lock().unwrap().push(account.clone());
            pings(
                accounts,
                me,
                "voice-key".into(),
                Duration::from_millis(20),
                &mut say,
                &mut keep,
            )
            .await;
        });
        tokio::time::timeout(Duration::from_secs(5), watched.changed())
            .await
            .unwrap()
            .unwrap();
        let next = watched.borrow().clone().unwrap();
        assert_eq!(
            (next.mqtt_endpoint.as_str(), next.mqtt_port),
            ("b2.test", 1884)
        );
        drop(watched);
        pinging.await.unwrap();
        let requests = requests.lock().unwrap().clone();
        assert!(requests.len() >= 3, "{requests:?}");
        let first = &requests[0];
        assert!(first.starts_with("POST /ping HTTP/1.1"), "{first}");
        assert!(
            first
                .to_lowercase()
                .contains("authorization: bearer voice-key"),
            "{first}"
        );
        assert!(
            first.contains(r#""status":"working""#)
                && first.contains(r#""local_ip":"192.168.1.42""#)
        );
        assert_eq!(kept.lock().unwrap().as_slice(), [next]);
        let lines = lines.lock().unwrap().clone();
        assert_eq!(
            lines,
            [
                "the backend's ping fails: status 401",
                "the backend's ping answers again",
                "the backend gave a new broker: b2.test:1884",
            ]
        );
        for line in &lines {
            assert!(
                !line.contains("voice-key") && !line.contains("p2") && !line.contains("s3cret")
            );
        }
    }
}
