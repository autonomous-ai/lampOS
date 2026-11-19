//! The sender of forgets to the account's history (#7771,
//! [`crate::history_forget`]): each waiting delete, one at a time, to
//! `<base>/history/turns/delete` on the lamp's own cloud API, with the
//! lamp's key in `x-api-key`, on the second route's HTTPS client. Runs on
//! the worker's runtime only with `LAMP_HISTORY_FORGET=on`.
//!
//! A call is done, and waits no more, when the server answers 2xx: its
//! `deleted` and `missing` (`missing` is already gone, or never saved),
//! `deleted_count`, or `{"enabled":false}` (the server keeps no history).
//! A 400 never goes through, so it is let go too, said in the journal. No
//! answer, 5xx and any other refusal keep it, tried again after a wait that
//! doubles from [`Pace::first`] to [`Pace::most`]. Only counts and statuses
//! reach the journal, never an id, a key or an address.
use super::{Error, chunk, http_client, post};
use crate::history_forget::{Call, Queue};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// The most of an answer read.
const MOST_ANSWER: usize = 64 * 1024;

/// Where deletes go, from the route's `llm_base_url`: the lamp's cloud API
/// (`…/api/v1/ai`), with or without its `/v1`.
pub fn url(llm_base_url: &str) -> String {
    let base = llm_base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    format!("{base}/history/turns/delete")
}

/// How long a kept call waits before it is tried again.
#[derive(Debug, Clone, Copy)]
pub struct Pace {
    pub first: Duration,
    pub most: Duration,
}

impl Pace {
    pub const PRODUCT: Self = Self {
        first: Duration::from_secs(5),
        most: Duration::from_secs(15 * 60),
    };
}

/// What one try came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Done: it waits no more. How the server said so.
    Done(&'static str),
    /// It will never go through: let go, with why.
    Dropped(String),
    /// Kept, to be tried again, with why.
    Kept(String),
}

/// An answer's body read as done: `deleted`/`missing`, `deleted_count`, or
/// the server's history switched off. Read at the top or in `data`.
fn done_by(answer: &Value) -> &'static str {
    let read = |answer: &Value| {
        if answer.get("enabled") == Some(&Value::Bool(false)) {
            Some("the account keeps no history")
        } else if answer.get("deleted").is_some() || answer.get("missing").is_some() {
            Some("deleted or already gone")
        } else if answer.get("deleted_count").is_some() {
            Some("deleted")
        } else {
            None
        }
    };
    read(answer)
        .or_else(|| answer.get("data").and_then(read))
        .unwrap_or("answered")
}

/// One try of `call`.
pub async fn send(client: &reqwest::Client, url: &str, key: &str, call: &Call) -> Outcome {
    let mut response = match post(client, url, ("x-api-key", key), &call.body()).await {
        Ok(response) => response,
        Err(Error::Status(400)) => return Outcome::Dropped("HTTP 400".into()),
        Err(Error::Status(status)) => return Outcome::Kept(format!("HTTP {status}")),
        Err(Error::UsageLimit(_)) => return Outcome::Kept("HTTP 429".into()),
        Err(Error::Timeout(_)) => return Outcome::Kept("no answer in time".into()),
        Err(_) => return Outcome::Kept("no connection".into()),
    };
    let mut bytes = Vec::new();
    loop {
        match chunk(&mut response).await {
            Ok(Some(piece)) => {
                bytes.extend(piece);
                if bytes.len() > MOST_ANSWER {
                    break;
                }
            }
            Ok(None) => break,
            // The server said 2xx: the delete was taken.
            Err(_) => break,
        }
    }
    Outcome::Done(
        serde_json::from_slice::<Value>(&bytes)
            .map(|answer| done_by(&answer))
            .unwrap_or("answered"),
    )
}

/// Sends what waits in `queue` until it is empty, then waits for `bell`;
/// a kept call is tried again after its wait (or at once when something
/// new is queued). Runs as long as the runtime does.
pub async fn task(url: String, key: String, queue: &'static Queue, bell: Arc<Notify>, pace: Pace) {
    let client = match http_client() {
        Ok(client) => client,
        Err(_) => {
            eprintln!("lamp-voice history: no HTTPS client; forgets wait on the lamp");
            return;
        }
    };
    let mut wait = pace.first;
    loop {
        let Some(call) = queue.pending().next() else {
            bell.notified().await;
            continue;
        };
        match send(&client, &url, &key, &call).await {
            Outcome::Done(how) => {
                wait = pace.first;
                let _ = queue.change(|pending| pending.done(&call));
                eprintln!(
                    "lamp-voice history: a forget reached the account's history ({}, {how})",
                    told(&call)
                );
            }
            Outcome::Dropped(why) => {
                let _ = queue.change(|pending| pending.done(&call));
                eprintln!(
                    "lamp-voice history: a forget was refused for good ({}, {why}); let go",
                    told(&call)
                );
            }
            Outcome::Kept(why) => {
                eprintln!(
                    "lamp-voice history: a forget is kept to try again in {} ms ({}, {why})",
                    wait.as_millis(),
                    told(&call)
                );
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = bell.notified() => {}
                }
                wait = (wait * 2).min(pace.most);
            }
        }
    }
}

/// A call in the journal's words: counts only.
fn told(call: &Call) -> String {
    match call {
        Call::All { .. } => "a person's whole history".into(),
        Call::Ids { ids, .. } => format!("{} turns", ids.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_forget::{Pending, turn_id};
    use serde_json::json;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const MINH: &str = "p-0123456789abcdef";

    /// One scripted answer: a status and a body, or the connection closed
    /// unanswered.
    #[derive(Clone)]
    enum Say {
        Answer(u16, &'static str),
        Hang,
    }

    /// A fake history server: what it was sent (the request line, the key
    /// header, the body) and its scripted answers, the last repeated.
    struct Fake {
        url: String,
        seen: Arc<Mutex<Vec<(String, String, Value)>>>,
    }

    async fn fake(script: Vec<Say>) -> Fake {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/v1/ai", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            let mut index = 0;
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let say = script[index.min(script.len() - 1)].clone();
                index += 1;
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let (head, body) = loop {
                    let n = stream.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        break (String::new(), Vec::new());
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let head = text[..end].to_owned();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break (head, bytes[end + 4..end + 4 + length].to_vec());
                        }
                    }
                };
                let line = head.lines().next().unwrap_or_default().to_owned();
                let key = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("x-api-key:")
                            .map(|value| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                kept.lock().unwrap().push((
                    line,
                    key,
                    serde_json::from_slice(&body).unwrap_or(Value::Null),
                ));
                match say {
                    Say::Hang => drop(stream),
                    Say::Answer(status, body) => {
                        let answer = format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(answer.as_bytes()).await;
                    }
                }
            }
        });
        Fake { url, seen }
    }

    fn client() -> reqwest::Client {
        http_client().unwrap()
    }

    fn ids() -> Call {
        Call::Ids {
            person: MINH.into(),
            ids: vec![turn_id(MINH, 4)],
        }
    }

    fn queue(name: &str) -> &'static Queue {
        let dir =
            std::env::temp_dir().join(format!("lamp-history-sender-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Box::leak(Box::new(Queue::at(dir.join("history-forget.json"))))
    }

    const FAST: Pace = Pace {
        first: Duration::from_millis(20),
        most: Duration::from_millis(80),
    };

    async fn until_empty(queue: &Queue) -> bool {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !queue.pending().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }

    #[test]
    fn the_url_is_the_cloud_apis_with_or_without_v1() {
        for base in [
            "https://campaign-api.autonomous.ai/api/v1/ai",
            "https://campaign-api.autonomous.ai/api/v1/ai/",
            "https://campaign-api.autonomous.ai/api/v1/ai/v1",
        ] {
            assert_eq!(
                url(base),
                "https://campaign-api.autonomous.ai/api/v1/ai/history/turns/delete"
            );
        }
    }

    /// `missing` counts as done, as `deleted` does; so does the server's
    /// history switched off; the key goes in `x-api-key`, to the delete's
    /// path.
    #[tokio::test]
    async fn missing_and_switched_off_count_as_done() {
        for body in [
            r#"{"deleted":[],"missing":["p-0123456789abcdef:4"]}"#,
            r#"{"deleted":["p-0123456789abcdef:4"],"missing":[]}"#,
            r#"{"enabled":false}"#,
            r#"{"deleted_count":3}"#,
            r#"{"status":1,"data":{"missing":["x"]}}"#,
        ] {
            let server = fake(vec![Say::Answer(200, body)]).await;
            let outcome = send(&client(), &url(&server.url), "k-lamp", &ids()).await;
            assert!(matches!(outcome, Outcome::Done(_)), "{body}: {outcome:?}");
            let seen = server.seen.lock().unwrap().clone();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].0, "POST /api/v1/ai/history/turns/delete HTTP/1.1");
            assert_eq!(seen[0].1, "k-lamp");
            assert_eq!(seen[0].2, json!({"ids":["p-0123456789abcdef:4"]}));
        }
    }

    /// No answer and 5xx keep the call; a 400 lets it go.
    #[tokio::test]
    async fn a_network_error_or_5xx_keeps_the_call() {
        let hung = fake(vec![Say::Hang]).await;
        let outcome = send(&client(), &url(&hung.url), "k", &ids()).await;
        assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
        let failing = fake(vec![Say::Answer(503, "{}")]).await;
        let outcome = send(&client(), &url(&failing.url), "k", &ids()).await;
        assert_eq!(outcome, Outcome::Kept("HTTP 503".into()));
        let refused = fake(vec![Say::Answer(400, r#"{"error":"person_required"}"#)]).await;
        let outcome = send(&client(), &url(&refused.url), "k", &ids()).await;
        assert_eq!(outcome, Outcome::Dropped("HTTP 400".into()));
    }

    /// The task: a call kept after a dropped connection and a 5xx is sent
    /// again, and goes once the server answers; a person's whole history
    /// goes first, with that person named; then the queue is empty.
    #[tokio::test]
    async fn the_retry_sends_a_kept_call_again() {
        let server = fake(vec![
            Say::Hang,
            Say::Answer(502, "{}"),
            Say::Answer(200, r#"{"deleted_count":2}"#),
            Say::Answer(200, r#"{"deleted":[],"missing":["p-0123456789abcdef:4"]}"#),
        ])
        .await;
        let queue = queue("retry");
        queue
            .change(|pending: &mut Pending| {
                pending.add_ids("", [turn_id("everyone", 4)]);
                pending.add_all(MINH);
            })
            .unwrap();
        let bell = Arc::new(Notify::new());
        let task = tokio::spawn(task(
            url(&server.url),
            "k".into(),
            queue,
            bell.clone(),
            FAST,
        ));
        assert!(until_empty(queue).await, "{:?}", queue.pending());
        let seen: Vec<Value> = server
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|seen| seen.2.clone())
            .collect();
        assert_eq!(
            seen,
            [
                json!({"all":true,"person":MINH}),
                json!({"all":true,"person":MINH}),
                json!({"all":true,"person":MINH}),
                json!({"ids":["everyone:4"]}),
            ]
        );
        // Something new queued wakes it.
        queue
            .change(|pending| pending.add_ids(MINH, [turn_id(MINH, 9)]))
            .unwrap();
        bell.notify_one();
        assert!(until_empty(queue).await);
        assert_eq!(server.seen.lock().unwrap().len(), 5);
        task.abort();
    }
}
