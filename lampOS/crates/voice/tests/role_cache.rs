use lamp_gemini::ToolCall;
use lamp_voice::remember::{self, DaysClient, role};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
const WAIT: Duration = Duration::from_secs(3);
struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reply(mut stream: UnixStream, answer: &Value) {
    stream.set_write_timeout(Some(WAIT)).unwrap();
    writeln!(stream, "{answer}").unwrap();
}
// Integration tests link the production library, whose role cache is shared
// across threads. Each mode runs in a fresh process to isolate global switches.
const MODE: &str = "LAMP_ROLE_CACHE_TEST_MODE";

#[test]
fn role_cache_rejects_a_read_overtaken_by_forget() {
    let root = std::path::PathBuf::from("/tmp").join(format!(
        "role-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    for mode in ["control", "forget"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "role_cache_child", "--nocapture"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", &root)
            .env("LAMP_MEMORY_DAYS", "on")
            .env("LAMP_MEMORY_ROLE", "on")
            .env("LAMP_MEMORY_FORGET_ALL", "on")
            .env(MODE, mode)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(20);
        let timed_out = loop {
            if child.try_wait().unwrap().is_some() {
                break false;
            }
            if Instant::now() >= until {
                let _ = child.kill();
                break true;
            }
            thread::sleep(Duration::from_millis(20));
        };
        let output = child.wait_with_output().unwrap();
        assert!(
            !timed_out && output.status.success(),
            "role cache {mode}: timed_out={timed_out}, status={}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn role_cache_child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let forget = match mode.as_str() {
        "forget" => true,
        "control" => false,
        _ => panic!("expected control or forget child mode"),
    };
    for flag in [
        "LAMP_MEMORY_DAYS",
        "LAMP_MEMORY_ROLE",
        "LAMP_MEMORY_FORGET_ALL",
    ] {
        assert_eq!(std::env::var(flag).as_deref(), Ok("on"), "{flag}");
    }
    // The public client has no override for these cleanup paths. Refuse a
    // host that could contain product consent data, including dangling links.
    assert!(
        matches!(fs::symlink_metadata("/var/lib/lampOS"), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
        "refusing host with existing, linked, or unreadable /var/lib/lampOS"
    );
    let root = std::env::temp_dir().join(format!("role-race-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    let socket = root.join("m.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let (entered, arrived) = mpsc::channel();
    let (release, released) = mpsc::channel();
    // The synthetic server parks one reply while serving forget. Memory's
    // real server serializes requests; the equivalent production race pauses
    // the reader after receiving its old reply but before publishing its role.
    let server = thread::spawn(move || {
        let limit = Instant::now() + Duration::from_secs(12);
        let mut released = Some(released);
        let mut delayed = None;
        while !stopping.load(Ordering::Acquire) && Instant::now() < limit {
            let (stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("accept: {e}"),
            };
            stream.set_read_timeout(Some(WAIT)).unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let answer = match request["op"].as_str() {
                Some("session_open") => {
                    let personal = request["person"].is_string();
                    let scope = if personal {
                        "caller-supplied person; identity not verified"
                    } else {
                        "said in this room; speaker unknown"
                    };
                    let mut answer = json!({"ok":true,"snapshot":{"session":request["session"],
                        "opened_unix_ms":1,"generation":2,"invalidated":false,"source_ids":[],
                        "note":json!({"scope":scope,"untrusted":true,"facts":[]}).to_string()}});
                    if personal {
                        assert_eq!(request["role"], true);
                        answer["role"] = json!("Harry Potter");
                    }
                    if personal && request["session"].as_str().unwrap().ends_with("-2") {
                        let wait = released.take().expect("one delayed personal read");
                        entered.send(()).unwrap();
                        delayed = Some(thread::spawn(move || {
                            wait.recv_timeout(WAIT).expect("release stale reply");
                            reply(stream, &answer);
                        }));
                        continue;
                    }
                    answer
                }
                Some("forget_all") => json!({"ok":true,"removed":1}),
                Some("session_close") => json!({"ok":true}),
                other => panic!("unexpected fake memory operation: {other:?}"),
            };
            reply(stream, &answer);
        }
        if let Some(delayed) = delayed {
            delayed.join().unwrap();
        }
    });
    remember::start(socket.clone(), None);
    assert!(role::on());
    let client = Arc::new(DaysClient::new(socket).unwrap());
    let person = lamp_msgs::known::person_id("Synthetic Role Person");
    assert!(client.open_session_within(1, WAIT, Some(&person)).is_some());
    assert_eq!(role::cached(&person).as_deref(), Some("Harry Potter"));
    let reading = {
        let client = client.clone();
        let person = person.clone();
        thread::spawn(move || client.read_note_within(2, Some(&person), false, WAIT, (true, false)))
    };
    arrived
        .recv_timeout(WAIT)
        .expect("personal old role response captured before forget");
    if forget {
        let call = ToolCall {
            id: "forget".into(),
            name: remember::FORGET_EVERYTHING.into(),
            args: json!({}),
        };
        let (answer, _) = client.run_for(&call, Duration::from_secs(1), Some(&person));
        println!("forget result: {answer}");
        assert_eq!(answer["forgotten"], true);
        assert_eq!(answer["everything"], true);
        assert!(
            role::cached(&person).is_none(),
            "successful forget clears cache before delayed reply"
        );
    }
    release.send(()).unwrap();
    let read = reading.join().unwrap();
    let complete = read.complete();
    let current = read.current();
    let adopted = read.adopt().is_some();
    let cached = role::cached(&person);
    let told = role::told(true, Some(&person), &[], false);
    println!(
        "mode={} complete={complete} current={current} adopted={adopted} cached={cached:?} told={told:?}",
        if forget { "forget" } else { "control" }
    );
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert!(complete);
    assert_eq!(current, !forget, "stale note must be rejected");
    assert_eq!(adopted, !forget, "stale snapshot must not be adopted");
    if forget {
        assert!(
            cached.is_none() && told.is_empty(),
            "late session_open restored forgotten role"
        );
    } else {
        assert_eq!(cached.as_deref(), Some("Harry Potter"));
        assert!(!told.is_empty());
    }
}
