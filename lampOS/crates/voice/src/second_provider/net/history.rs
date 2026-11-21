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

/// Where deletes go, from the route's `llm_base_url`: the lamp's own cloud
/// API (`https://campaign-api.autonomous.ai/…/api/v1/ai`, with or without
/// its `/v1`), the host the turn's save goes to too. Any other base (a
/// direct provider, a test address) is `None`: nothing is sent, and the
/// forgets wait on the lamp (the first builder's P2 on #7935).
pub fn url(llm_base_url: &str) -> Option<String> {
    let base = llm_base_url.trim().trim_end_matches('/');
    let host = base.strip_prefix("https://")?.split('/').next()?;
    if host != crate::second_config::COMPANY_RELAY_HOST {
        return None;
    }
    let base = base.strip_suffix("/v1").unwrap_or(base);
    Some(format!("{base}/history/turns/delete"))
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
    task_while(url, key, queue, bell, pace, Arc::new(|| true)).await;
}

/// How often a closed privacy switch is looked at again.
pub const PRIVACY_AGAIN: Duration = Duration::from_secs(1);

/// [`task`], sending only while `open` (the privacy switch open,
/// [`crate::history_privacy::open`]): closed, the queue waits, looked at
/// again every [`PRIVACY_AGAIN`] (the first builder's privacy read on
/// #8125, item 3).
pub async fn task_while(
    url: String,
    key: String,
    queue: &'static Queue,
    bell: Arc<Notify>,
    pace: Pace,
    open: Arc<dyn Fn() -> bool + Send + Sync>,
) {
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
        if !open() {
            tokio::time::sleep(PRIVACY_AGAIN).await;
            continue;
        }
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
        Call::Lamp => "the lamp's whole history".into(),
        Call::All { .. } => "a person's whole history".into(),
        Call::Ids { ids, .. } => format!("{} turns", ids.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_forget::Pending;

    fn turn_id(scope: &str, id: u64) -> String {
        lamp_msgs::history_turn_id("0a1b2c3d", scope, id)
    }
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

    /// The fake's delete address: [`url`] takes only the lamp's own cloud
    /// API, so a test server is addressed as the path it would give.
    fn delete_at(base: &str) -> String {
        format!("{base}/history/turns/delete")
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
                url(base).as_deref(),
                Some("https://campaign-api.autonomous.ai/api/v1/ai/history/turns/delete")
            );
        }
    }

    /// With a direct base, or any host but the lamp's own cloud API,
    /// nothing is sent: there is no address to send to.
    #[test]
    fn a_direct_base_sends_nothing() {
        for base in [
            "https://api.deepseek.com",
            "https://generativelanguage.googleapis.com/v1beta",
            "http://campaign-api.autonomous.ai/api/v1/ai",
            "https://campaign-api.autonomous.ai.evil.example/api/v1/ai",
            "http://127.0.0.1:9/api/v1/ai",
            "",
        ] {
            assert_eq!(url(base), None, "{base}");
        }
    }

    /// `missing` counts as done, as `deleted` does; so does the server's
    /// history switched off; the key goes in `x-api-key`, to the delete's
    /// path.
    #[tokio::test]
    async fn missing_and_switched_off_count_as_done() {
        for body in [
            r#"{"deleted":[],"missing":["0a1b2c3d.p-0123456789abcdef:4"]}"#,
            r#"{"deleted":["0a1b2c3d.p-0123456789abcdef:4"],"missing":[]}"#,
            r#"{"enabled":false}"#,
            r#"{"deleted_count":3}"#,
            r#"{"status":1,"data":{"missing":["x"]}}"#,
        ] {
            let server = fake(vec![Say::Answer(200, body)]).await;
            let outcome = send(&client(), &delete_at(&server.url), "k-lamp", &ids()).await;
            assert!(matches!(outcome, Outcome::Done(_)), "{body}: {outcome:?}");
            let seen = server.seen.lock().unwrap().clone();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].0, "POST /api/v1/ai/history/turns/delete HTTP/1.1");
            assert_eq!(seen[0].1, "k-lamp");
            assert_eq!(seen[0].2, json!({"ids":["0a1b2c3d.p-0123456789abcdef:4"]}));
        }
    }

    /// No answer and 5xx keep the call; a 400 lets it go.
    #[tokio::test]
    async fn a_network_error_or_5xx_keeps_the_call() {
        let hung = fake(vec![Say::Hang]).await;
        let outcome = send(&client(), &delete_at(&hung.url), "k", &ids()).await;
        assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
        let failing = fake(vec![Say::Answer(503, "{}")]).await;
        let outcome = send(&client(), &delete_at(&failing.url), "k", &ids()).await;
        assert_eq!(outcome, Outcome::Kept("HTTP 503".into()));
        let refused = fake(vec![Say::Answer(400, r#"{"error":"person_required"}"#)]).await;
        let outcome = send(&client(), &delete_at(&refused.url), "k", &ids()).await;
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
            Say::Answer(
                200,
                r#"{"deleted":[],"missing":["0a1b2c3d.p-0123456789abcdef:4"]}"#,
            ),
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
            delete_at(&server.url),
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
                json!({"ids":["0a1b2c3d.everyone:4"]}),
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

    /// The first builder's condition on #8086, end to end: a forget the
    /// memory kept and the voice never queued (it restarted between) is
    /// read at the next start and reaches the server, and once sent no
    /// later start sends it again.
    #[tokio::test]
    async fn a_forget_lost_to_a_restart_reaches_the_server_after_start() {
        let server = fake(vec![Say::Answer(200, r#"{"deleted_count":3}"#)]).await;
        let queue = queue("restart");
        let memory = crate::history_forget::FakeMemory::default();
        memory.kept(MINH, &[4, 5, 6].map(|id| Some(turn_id(MINH, id))));
        let start = || {
            crate::history_forget::recover(
                queue,
                &Notify::new(),
                |line| memory.answer(line),
                |_| {},
            )
        };
        assert!(start());
        let bell = Arc::new(Notify::new());
        let task = tokio::spawn(task(
            delete_at(&server.url),
            "k".into(),
            queue,
            bell.clone(),
            FAST,
        ));
        assert!(until_empty(queue).await, "{:?}", queue.pending());
        assert!(start());
        assert!(queue.pending().is_empty());
        let seen: Vec<Value> = server
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|seen| seen.2.clone())
            .collect();
        assert_eq!(
            seen,
            [json!({"ids":[turn_id(MINH, 4), turn_id(MINH, 5), turn_id(MINH, 6)]})]
        );
        task.abort();
    }

    /// The Reviewer-lead's HOLD on #8125, end to end: the app removes a
    /// person (lamp-cloud's `forget_person` on the memory's forget socket,
    /// which never passes the voice). The real memory keeps their whole
    /// history; its `memory/forgot` on the bus has the voice's worker read
    /// it, onto the real queue, and the sender deletes that person's whole
    /// history from the account.
    #[tokio::test]
    async fn a_person_removed_in_the_app_is_deleted_from_the_account() {
        let server = fake(vec![Say::Answer(200, r#"{"deleted_count":2}"#)]).await;
        let queue = queue("removed");
        let dir = std::env::temp_dir().join(format!("lamp-history-removed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = lamp_memory::days::Days::open(
            lamp_memory::store::Options {
                dir: dir.join("memory"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            1_760_356_800_000,
        )
        .unwrap();
        let store = Arc::new(Mutex::new(store));
        let ask = |store: &Mutex<lamp_memory::days::Days>, line: &Value| -> Value {
            let answer = lamp_memory::days_protocol::answer(
                &mut store.lock().unwrap(),
                line.to_string().as_bytes(),
                1_760_356_800_000,
            );
            serde_json::from_str(answer.line.trim()).unwrap()
        };
        for n in 1..=2 {
            let kept = ask(
                &store,
                &json!({"op":"append","person":MINH,"at_unix_ms":1_760_356_800_000u64 + n,"heard":"h","said":"s"}),
            );
            assert_eq!(kept["ok"], true, "{kept}");
        }
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // One bell for the worker and the sender, as the product's
        // `history_forget::queue()` gives both.
        let bell = Arc::new(Notify::new());
        let worker: &'static crate::history_forget::Worker = Box::leak(Box::new(
            crate::history_forget::Worker::new(queue, bell.clone(), {
                let (store, asked) = (store.clone(), asked.clone());
                Arc::new(move |_: &std::path::Path, line: &Value| {
                    asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Some(ask(&store, line))
                })
            }),
        ));
        let root = dir.join("bus");
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut memory_bus = bus
            .publisher_declared(
                lamp_msgs::MemoryForgot::TOPIC,
                &lamp_msgs::MemoryForgot::declaration(),
                "memory",
            )
            .unwrap();
        let stop: &'static std::sync::atomic::AtomicBool =
            Box::leak(Box::new(std::sync::atomic::AtomicBool::new(false)));
        let socket = std::path::Path::new("/nonexistent/memory.sock");
        assert!(crate::history_forget::watch_removals_with(
            worker, &root, socket, stop
        ));
        // Subscribed: its first read of what the memory kept is done.
        let started = std::time::Instant::now();
        while asked.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            assert!(started.elapsed() < Duration::from_secs(10));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(queue.pending().is_empty());
        // The app's removal, as lamp-cloud sends it, then told on the bus.
        let removed = ask(&store, &json!({"op":"forget_person","person":MINH}));
        assert_eq!(removed["ok"], true, "{removed}");
        memory_bus
            .publish_json(
                lamp_ipc::monotonic_us(),
                &lamp_msgs::MemoryForgot {
                    person: Some(MINH.into()),
                },
            )
            .unwrap();
        let task = tokio::spawn(task(delete_at(&server.url), "k".into(), queue, bell, FAST));
        let started = std::time::Instant::now();
        while server.seen.lock().unwrap().is_empty() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{:?}",
                queue.pending()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(until_empty(queue).await, "{:?}", queue.pending());
        let seen: Vec<Value> = server
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|seen| seen.2.clone())
            .collect();
        assert_eq!(seen, [json!({"all":true,"person":MINH})]);
        // Queued, so the memory let it go.
        let left = ask(&store, &json!({"op":"history_unsent","after":0}));
        assert_eq!(left["all"], json!([]), "{left}");
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        task.abort();
    }

    /// The first builder's privacy read on #8125, item 1: a forget while
    /// the server is down is kept, and reaches it after a restart.
    #[tokio::test]
    async fn a_forget_while_the_server_is_down_reaches_it_after_a_restart() {
        let down = fake(vec![Say::Answer(503, "{}")]).await;
        let queue = queue("down-restart");
        queue
            .change(|pending: &mut Pending| pending.add_ids(MINH, [turn_id(MINH, 7)]))
            .unwrap();
        let before = tokio::spawn(task(
            delete_at(&down.url),
            "k".into(),
            queue,
            Arc::new(Notify::new()),
            FAST,
        ));
        let started = std::time::Instant::now();
        while down.seen.lock().unwrap().len() < 2 {
            assert!(started.elapsed() < Duration::from_secs(10));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The voice restarts: the delete is still waiting on disk.
        before.abort();
        assert_eq!(queue.pending().ids[MINH].len(), 1);
        let up = fake(vec![Say::Answer(200, r#"{"deleted_count":1}"#)]).await;
        let after = tokio::spawn(task(
            delete_at(&up.url),
            "k".into(),
            queue,
            Arc::new(Notify::new()),
            FAST,
        ));
        assert!(until_empty(queue).await, "{:?}", queue.pending());
        assert_eq!(
            up.seen.lock().unwrap()[0].2,
            json!({"ids":[turn_id(MINH, 7)]})
        );
        after.abort();
    }

    /// The first builder's privacy read on #8125, items 2 and 3: nothing is
    /// deleted while the privacy switch is closed; once it opens, the
    /// lamp's whole history goes first.
    #[tokio::test]
    async fn nothing_is_sent_while_the_switch_is_closed_and_the_lamps_history_goes_first() {
        let server = fake(vec![Say::Answer(200, r#"{"deleted_count":1}"#)]).await;
        let queue = queue("closed");
        queue
            .change(|pending: &mut Pending| {
                pending.add_lamp();
                pending.add_ids(MINH, [turn_id(MINH, 8)]);
            })
            .unwrap();
        let open = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = open.clone();
        let task = tokio::spawn(task_while(
            delete_at(&server.url),
            "k".into(),
            queue,
            Arc::new(Notify::new()),
            FAST,
            Arc::new(move || seen.load(std::sync::atomic::Ordering::SeqCst)),
        ));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(server.seen.lock().unwrap().is_empty());
        open.store(true, std::sync::atomic::Ordering::SeqCst);
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
                serde_json::from_str::<Value>(crate::history_forget::WHOLE_LAMP_BODY).unwrap(),
                json!({"ids":[turn_id(MINH, 8)]}),
            ]
        );
        task.abort();
    }
}
