//! The exact opt-in operator executable, on a local bus and HTTP fixture.
use lamp_bus::Bus;
use lamp_msgs::{Decision, Diagnostic, Speech, VoiceState};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-policy-judges");
const WAIT: Duration = Duration::from_secs(5);
struct Place(PathBuf);
impl Place {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(format!(
            "/tmp/lpjp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(PROGRAM);
        cmd.arg("--log-dir")
            .arg(self.0.join("logs"))
            .arg("--bus-root")
            .arg(self.0.join("bus"))
            .env_remove("LAMP_BUS_ROOT")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}
impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Running(Option<Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn disabled_program_and_invalid_arguments_create_no_bus_or_log() {
    for flags in [
        vec!["--offline"],
        vec!["--local-judge", "--offline", "--seconds", "0"],
        vec![
            "--local-judge",
            "--offline",
            "--key-file",
            "/not-a-real-key",
        ],
    ] {
        let place = Place::new();
        let output = place.command().args(flags).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!place.0.join("logs").exists());
        assert!(!place.0.join("bus").exists());
    }
}

#[test]
fn subscription_failure_cancels_the_sixty_second_timer() {
    let place = Place::new();
    let bus = Bus::new(place.0.join("bus")).unwrap();
    let _wrong = bus
        .publisher_declared(Speech::TOPICS[0], &VoiceState::declaration(), "wrong")
        .unwrap();
    let child = place
        .command()
        .args(["--local-judge", "--offline"])
        .spawn()
        .unwrap();
    let mut running = Running(Some(child));
    let end = Instant::now() + WAIT;
    loop {
        if let Some(status) = running.0.as_mut().unwrap().try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < end,
            "startup failure left the qualification timer waiting"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn service() -> (u16, mpsc::Receiver<Value>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let (sent, received) = mpsc::channel();
    let worker = thread::spawn(move || {
        let end = Instant::now() + WAIT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < end,
                        "program never asked the loopback fixture"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fixture accept: {error}"),
            }
        };
        // On macOS the accepted stream keeps the listener's O_NONBLOCK (#913).
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let mut held = Vec::new();
        let mut chunk = [0; 4096];
        let (body_start, length) = loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            held.extend_from_slice(&chunk[..n]);
            assert!(held.len() <= 64 * 1024);
            if let Some(end) = held.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = std::str::from_utf8(&held[..end])
                    .unwrap()
                    .to_ascii_lowercase();
                let length: usize = header
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:")?.trim().parse().ok())
                    .unwrap();
                break (end + 4, length);
            }
        };
        assert!(body_start + length <= 64 * 1024);
        while held.len() < body_start + length {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            held.extend_from_slice(&chunk[..n]);
        }
        let request: Value =
            serde_json::from_slice(&held[body_start..body_start + length]).unwrap();
        let answers: serde_json::Map<String, Value> = request["questions"]
            .as_object()
            .unwrap()
            .keys()
            .map(|name| (name.clone(), json!({"type":"noul","noul":0.9})))
            .collect();
        let response = json!({"model":"private-local-fixture-name","answers":answers}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
        sent.send(request).unwrap();
    });
    (port, received, worker)
}

#[test]
fn one_command_shows_both_judges_and_exits_with_a_truthful_private_log_receipt() {
    let place = Place::new();
    fs::write(
        place.0.join("fixture-key"),
        "not-a-provider-key-local-fixture\n",
    )
    .unwrap();
    let bus = Bus::new(place.0.join("bus")).unwrap();
    let mut heard = bus
        .publisher_declared(Speech::TOPICS[0], &Speech::declaration(), "voice")
        .unwrap();
    let mut decisions = bus
        .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
        .unwrap();
    let mut health = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let (port, requests, worker) = service();
    let mut running = Running(Some(
        place
            .command()
            .args([
                "--local-judge",
                "--show-judges",
                "--seconds",
                "2",
                "--deadline-ms",
                "1000",
                "--request-ms",
                "2000",
                "--endpoint",
            ])
            .arg(format!("http://127.0.0.1:{port}/decisions"))
            .arg("--key-file")
            .arg(place.0.join("fixture-key"))
            .spawn()
            .unwrap(),
    ));
    health.recv(WAIT).unwrap().expect("program ready");
    heard
        .publish_json(
            0,
            &Speech {
                text: "Lamp, private authored utterance".into(),
                finished: true,
                locale: None,
                request: Some(1),
            },
        )
        .unwrap();
    let decision: Decision = decisions
        .recv(WAIT)
        .unwrap()
        .expect("decision")
        .parse()
        .unwrap();
    assert_eq!(decision.by, "local+jev");
    assert!(decision.shadow);
    assert_eq!(
        requests.recv_timeout(WAIT).unwrap()["questions"]
            .as_object()
            .unwrap()
            .len(),
        7
    );
    worker.join().unwrap();
    let output = running.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8(output.stderr).unwrap();
    assert!(
        printed.contains("judge=local")
            && printed.contains("judge=slow")
            && printed.contains("calls_lamp=")
    );
    assert!(printed.contains("drained=true"));
    for private in [
        "private authored utterance",
        "not-a-provider-key-local-fixture",
        "private-local-fixture-name",
    ] {
        assert!(!printed.contains(private));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(private));
    }
}
