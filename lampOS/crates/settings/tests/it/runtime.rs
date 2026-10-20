use lamp_settings::{
    client,
    engine::Engine,
    model::Catalog,
    runtime::{self, Config},
    store::{FileStore, Storage},
};
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);

struct Server {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<io::Result<runtime::Finished>>>,
}
impl Server {
    fn start<S: Storage>(dir: tempfile::TempDir, engine: Engine<S>) -> Self {
        let socket = dir.path().join("settings.sock");
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config {
            socket: socket.clone(),
            bus_root: None,
            run_for: Some(Duration::from_secs(30)),
        };
        let flag = stop.clone();
        let task = thread::spawn(move || runtime::serve(engine, config, flag));
        let server = Self {
            _dir: dir,
            socket,
            stop,
            task: Some(task),
        };
        let until = Instant::now() + WAIT;
        loop {
            if client::exchange(
                &server.socket,
                &json!({"op":"status"}),
                Instant::now() + Duration::from_millis(100),
            )
            .is_ok()
            {
                break;
            }
            assert!(Instant::now() < until, "server did not become ready");
            thread::sleep(Duration::from_millis(5));
        }
        server
    }
    fn disk() -> Self {
        let dir = directory();
        let store = FileStore::open(dir.path()).unwrap();
        let engine = Engine::open(store, Catalog::first_three()).unwrap();
        Self::start(dir, engine)
    }
    fn request(&self, request: Value) -> Value {
        call(&self.socket, request)
    }
    fn read(&self) -> Value {
        self.request(json!({"op":"read"}))
    }
    fn finish(mut self) -> runtime::Finished {
        self.stop.store(true, Ordering::Release);
        self.task.take().unwrap().join().unwrap().unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}
fn directory() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("settings-")
        .tempdir_in("/private/tmp")
        .or_else(|_| {
            tempfile::Builder::new()
                .prefix("settings-")
                .tempdir_in("/tmp")
        })
        .unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
fn call(socket: &Path, request: Value) -> Value {
    client::exchange(socket, &request, Instant::now() + WAIT).unwrap()
}
fn change(state: &Value, id: &str, level: i64) -> Value {
    json!({"op":"set","mutation":{"guard":{"store_id":state["state"]["store_id"],"revision":state["state"]["revision"],"request_id":id},
        "changes":[{"name":"light.level","value":{"kind":"integer","value":level}}]}})
}

#[test]
fn concurrent_editors_get_one_commit_and_retry_survives_recovery() {
    let server = Server::disk();
    let initial = server.read();
    let first = change(&initial, "first", 2);
    let second = change(&initial, "second", 3);
    let a = {
        let socket = server.socket.clone();
        let request = first.clone();
        thread::spawn(move || call(&socket, request))
    };
    let b = {
        let socket = server.socket.clone();
        let request = second.clone();
        thread::spawn(move || call(&socket, request))
    };
    let replies = [a.join().unwrap(), b.join().unwrap()];
    assert_eq!(replies.iter().filter(|r| r["ok"] == true).count(), 1);
    assert_eq!(
        replies.iter().filter(|r| r["error"] == "conflict").count(),
        1
    );
    let accepted = if replies[0]["ok"] == true {
        first
    } else {
        second
    };
    let replay = server.request(accepted.clone());
    assert_eq!(replay["commit"]["revision"], 1);
    assert_eq!(replay["commit"]["replayed"], true);
    let path = server._dir.path().to_path_buf();
    server.stop.store(true, Ordering::Release);
    // Keep the TempDir alive while releasing the actual server/store lock.
    let mut server = server;
    assert!(
        server
            .task
            .take()
            .unwrap()
            .join()
            .unwrap()
            .unwrap()
            .owner_stopped
    );
    assert!(!server.socket.exists());
    let reopened = Engine::open(FileStore::open(&path).unwrap(), Catalog::first_three()).unwrap();
    assert_eq!(reopened.snapshot().revision, 1);
    let mut reopened = reopened;
    let mutation = serde_json::from_value(accepted["mutation"].clone()).unwrap();
    assert!(reopened.mutate(mutation).unwrap().replayed);
}

struct Gate {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    fail: bool,
}
#[derive(Clone, Default)]
struct Memory {
    bytes: Arc<Mutex<Option<Vec<u8>>>>,
    gate: Arc<Mutex<Option<Gate>>>,
}
impl Storage for Memory {
    fn load(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.bytes.lock().unwrap().clone())
    }
    fn save(&mut self, bytes: &[u8]) -> io::Result<()> {
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            gate.release.recv_timeout(WAIT).map_err(io::Error::other)?;
            if gate.fail {
                return Err(io::Error::other("injected uncertain save"));
            }
        }
        *self.bytes.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
}
// Dropping the sender also releases a failed test's blocked storage worker.
fn pause(memory: &Memory, fail: bool) -> (mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    *memory.gate.lock().unwrap() = Some(Gate {
        entered: entered_tx,
        release: release_rx,
        fail,
    });
    (entered_rx, release_tx)
}

#[test]
fn read_keeps_progressing_while_storage_waits_and_failure_is_not_rollback() {
    let memory = Memory::default();
    let engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
    let server = Server::start(directory(), engine);
    let initial = server.read();
    let (entered, release) = pause(&memory, true);
    let write = {
        let socket = server.socket.clone();
        let request = change(&initial, "slow", 2);
        thread::spawn(move || call(&socket, request))
    };
    entered.recv_timeout(WAIT).unwrap();
    // No sleep/performance assertion: storage is still blocked at this point.
    let read = server.read();
    assert_eq!(read["pending"], true);
    assert_eq!(read["reliable"], true);
    assert_eq!(read["state"]["revision"], 0);
    release.send(()).unwrap();
    assert_eq!(write.join().unwrap()["error"], "indeterminate");
    let read = server.read();
    assert_eq!(read["reliable"], false);
    assert_eq!(read["state"]["revision"], 0);
    assert_eq!(
        server.request(change(&initial, "later", 3))["error"],
        "restart_required"
    );
    let finished = server.finish();
    assert!(finished.owner_stopped);
    assert!(!finished.reliable);
}

#[test]
fn expired_client_does_not_cancel_a_started_durable_save() {
    let memory = Memory::default();
    let engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
    let server = Server::start(directory(), engine);
    let initial = server.read();
    let request = change(&initial, "unknown", 4);
    let (entered, release) = pause(&memory, false);
    let write = {
        let socket = server.socket.clone();
        let request = request.clone();
        thread::spawn(move || call(&socket, request))
    };
    entered.recv_timeout(WAIT).unwrap();
    assert_eq!(write.join().unwrap()["error"], "indeterminate");
    assert_eq!(server.read()["pending"], true);
    release.send(()).unwrap();
    let replay = server.request(request);
    assert_eq!(replay["ok"], true);
    assert_eq!(replay["commit"]["replayed"], true);
    assert_eq!(replay["commit"]["revision"], 1);
    assert!(server.finish().reliable);
}

#[test]
fn slow_input_and_bad_lines_do_not_block_another_reader() {
    use std::io::{Read, Write};
    let server = Server::disk();
    let mut slow = UnixStream::connect(&server.socket).unwrap();
    slow.write_all(b"{").unwrap();
    assert_eq!(server.read()["reliable"], true);
    for line in [
        b"{\"op\":\"read\",\"op\":\"status\"}\n".as_slice(),
        b"{\"op\":\"read\"}\n{}\n",
    ] {
        let mut stream = UnixStream::connect(&server.socket).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.write_all(line).unwrap();
        let mut output = String::new();
        stream.read_to_string(&mut output).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap()["error"],
            "bad_request"
        );
    }
    assert_eq!(server.read()["state"]["revision"], 0);
    assert!(server.finish().owner_stopped);
}

/// Host measurement, not a lamp result: one durable save through the socket,
/// from the client's send to its confirmed reply, against the 250 ms target.
#[test]
#[ignore = "host measurement: socket and private-directory save on this machine"]
fn save_round_trip_latency_on_this_machine() {
    let server = Server::disk();
    let mut times = Vec::new();
    for attempt in 0..101 {
        let state = server.read();
        let level = 1 + attempt % 5;
        let request = change(&state, &format!("latency-{attempt}"), level);
        let started = Instant::now();
        let reply = server.request(request.clone());
        let took = started.elapsed().as_micros() as u64;
        let parsed: lamp_settings::protocol::Request = serde_json::from_value(request).unwrap();
        assert!(
            lamp_settings::protocol::confirms(&parsed, &reply),
            "{reply}"
        );
        times.push(took);
    }
    let first = times.remove(0);
    times.sort_unstable();
    println!(
        "{}",
        json!({"boundary":"client send through durable save to confirmed reply, private directory on this machine",
            "first_us":first,"repeated_count":times.len(),"repeated_p50_us":times[49],
            "repeated_p95_us":times[94],"repeated_max_us":times[99],"target_us":250_000,
            "hardware_measured":false})
    );
    assert!(server.finish().reliable);
}
