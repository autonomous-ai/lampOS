//! The lamp's OpenClaw Gateway, waited for before an OpenClaw launch
//! (#6226). OpenClaw's TUI panes talk to one running Gateway (loopback
//! [`PORT`]), which Harness never starts. lamp-update's Gateway unit
//! runs it always while
//! `LAMP_OPENCLAW_ON_LAMP` is on (the owner's decision: every lamp has
//! 6 GB); lamp-tasks never starts or stops it. A TUI started while the
//! Gateway is still coming up loses its first prompt, and Harness neither
//! retries nor says so (lamp-c0a4), so before `harness new openclaw`
//! lamp-tasks waits until the Gateway takes a connection and answers, at
//! most [`READY_WITHIN`]; else the hand-over is not sent, with
//! [`NOT_READY`], and the person is told it failed.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use crate::harness::{EngineKind, Launch, LaunchOutcome};

/// Where the Gateway listens, on loopback.
pub const PORT: u16 = 18789;
/// How long a launch waits for a Gateway not answering yet before it is
/// given up: the Gateway runs always, so this covers only its own start
/// (27-50 s on lamp-c0a4 with two plugins) after a boot or a restart.
pub const READY_WITHIN: Duration = Duration::from_secs(60);
/// Between two looks at the port while it is waited for.
const READY_EVERY: Duration = Duration::from_millis(250);
/// One connection try's bound.
const CONNECT_WITHIN: Duration = Duration::from_millis(500);
/// How long an answer to one look is waited for.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);
/// The code a hand-over is not sent with when the Gateway did not answer.
pub const NOT_READY: &str = "openclaw_gateway_not_ready";

/// What the part's targets read of OpenClaw: whether the Harness on the
/// lamp is too old for it (`refused`, before 0.6.11), so OpenClaw is not
/// the lamp's engine.
#[derive(Debug, Default)]
pub struct Shared {
    pub refused: AtomicBool,
}

/// How the Gateway is looked at. Only looked at: there is nothing here to
/// start or stop it with.
pub trait Probe: Send {
    /// Whether the Gateway takes a connection and answers now.
    fn ready(&mut self) -> bool;
}

/// The real Gateway on loopback [`PORT`] ([`answers`]).
pub struct Loopback;

impl Probe for Loopback {
    fn ready(&mut self) -> bool {
        answers(PORT)
    }
}

/// The wait before an OpenClaw launch.
pub struct Gateway {
    probe: Box<dyn Probe>,
    ready_within: Duration,
    ready_every: Duration,
}

impl Gateway {
    pub fn new(probe: Box<dyn Probe>) -> Self {
        Self::with_bounds(probe, READY_WITHIN, READY_EVERY)
    }

    pub fn with_bounds(
        probe: Box<dyn Probe>,
        ready_within: Duration,
        ready_every: Duration,
    ) -> Self {
        Self {
            probe,
            ready_within,
            ready_every,
        }
    }

    /// Ready now, or waited for, at most the ready bound; plain words why
    /// not when it did not answer.
    pub fn before_launch(&mut self) -> Result<(), String> {
        let began = Instant::now();
        loop {
            if self.probe.ready() {
                return Ok(());
            }
            if began.elapsed() >= self.ready_within {
                return Err(format!(
                    "the OpenClaw Gateway did not answer on 127.0.0.1:{PORT} within {} s",
                    self.ready_within.as_secs()
                ));
            }
            thread::sleep(self.ready_every);
        }
    }
}

/// `launch` run by `run`, an OpenClaw one only once the Gateway answers;
/// not sent ([`NOT_READY`]) when it does not, with one journal line.
pub fn launch_with(
    gateway: Option<&mut Gateway>,
    launch: &Launch,
    run: impl FnOnce() -> LaunchOutcome,
) -> LaunchOutcome {
    if launch.target.engine == EngineKind::Openclaw
        && let Some(gateway) = gateway
        && let Err(why) = gateway.before_launch()
    {
        eprintln!("tasks: {why}; the hand-over is not sent");
        return LaunchOutcome::NotSent {
            code: NOT_READY.into(),
        };
    }
    run()
}

/// Whether something on loopback `port` takes a connection and answers an
/// HTTP request with at least one byte, each step bounded: a Gateway still
/// starting may take the connection before it can answer, and a TUI that
/// connects then loses its first prompt (#6226, lamp-c0a4).
pub fn answers(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_WITHIN) else {
        return false;
    };
    if stream.set_read_timeout(Some(ANSWER_WITHIN)).is_err()
        || stream.set_write_timeout(Some(ANSWER_WITHIN)).is_err()
    {
        return false;
    }
    let asked = write!(
        stream,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    let mut first = [0u8; 1];
    asked.is_ok() && matches!(stream.read(&mut first), Ok(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Target;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// How many times it was looked at, and from which look on it answers.
    #[derive(Clone, Default)]
    struct Fake {
        looks: Arc<Mutex<u32>>,
        ready_from: Option<u32>,
    }

    impl Probe for Fake {
        fn ready(&mut self) -> bool {
            let mut looks = self.looks.lock().unwrap();
            *looks += 1;
            self.ready_from.is_some_and(|from| *looks >= from)
        }
    }

    fn gateway(fake: &Fake) -> Gateway {
        Gateway::with_bounds(
            Box::new(fake.clone()),
            Duration::from_millis(50),
            Duration::from_millis(1),
        )
    }

    fn launch(engine: EngineKind) -> Launch {
        Launch {
            name: "lamp-x".into(),
            prompt: "p".into(),
            target: Target {
                machine_id: "a".repeat(32),
                project: "~/harnesses/lamp-tasks/lamp".into(),
                engine,
            },
            session: None,
        }
    }

    #[test]
    fn a_port_is_ready_only_once_it_answers() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // Taken but never answered: not ready.
        let silent = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(2500));
            drop(stream);
            listener
        });
        assert!(!answers(port));
        let listener = silent.join().unwrap();
        let answering = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = [0u8; 64];
            let _ = stream.read(&mut asked);
            stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").unwrap();
        });
        assert!(answers(port));
        answering.join().unwrap();
    }

    /// #6226, lamp-c0a4: `harness new openclaw` runs only once the Gateway
    /// answers; one coming up is waited for; another engine's launch never
    /// looks at it.
    #[test]
    fn an_openclaw_launch_waits_for_the_gateway_to_answer() {
        let fake = Fake {
            ready_from: Some(3),
            ..Fake::default()
        };
        let mut waited = gateway(&fake);
        let looks = fake.looks.clone();
        let outcome = launch_with(Some(&mut waited), &launch(EngineKind::Openclaw), || {
            assert_eq!(*looks.lock().unwrap(), 3, "launched only once it answered");
            LaunchOutcome::Created {
                agent_id: "x".into(),
            }
        });
        assert!(matches!(outcome, LaunchOutcome::Created { .. }));
        let other = Fake::default();
        let mut idle = gateway(&other);
        launch_with(Some(&mut idle), &launch(EngineKind::Hermes), || {
            LaunchOutcome::Created {
                agent_id: "z".into(),
            }
        });
        assert_eq!(*other.looks.lock().unwrap(), 0);
    }

    /// #6226: a Gateway that never answers: no `new`, the hand-over not
    /// sent with a code the person is told, within the bound.
    #[test]
    fn a_gateway_that_does_not_answer_is_not_sent_within_the_bound() {
        let fake = Fake::default();
        let mut waited = gateway(&fake);
        let began = Instant::now();
        let outcome = launch_with(Some(&mut waited), &launch(EngineKind::Openclaw), || {
            panic!("never launched")
        });
        assert_eq!(
            outcome,
            LaunchOutcome::NotSent {
                code: NOT_READY.into()
            }
        );
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(
            *fake.looks.lock().unwrap() > 1,
            "looked at again while waited"
        );
        assert!(READY_WITHIN <= Duration::from_secs(60));
    }

    /// #6226, the owner: the Gateway runs always while
    /// `LAMP_OPENCLAW_ON_LAMP` is on, so a task never starts or stops its
    /// unit: lamp-tasks has no code that does ([`Probe`] only looks).
    #[test]
    fn a_task_never_starts_or_stops_the_gateway_unit() {
        let unit = concat!("lamp-openclaw-", "gateway");
        let manager = concat!("system", "ctl");
        for (name, source, any_manager) in [
            ("gateway.rs", include_str!("gateway.rs"), false),
            ("runtime.rs", include_str!("runtime.rs"), false),
            ("main.rs", include_str!("main.rs"), false),
            // Harness's own daemon is reached through the user manager
            // (#5385); only the Gateway's unit is never named there.
            ("harness.rs", include_str!("harness.rs"), true),
        ] {
            assert!(!source.contains(unit), "{name} names the Gateway's unit");
            assert!(
                any_manager || !source.contains(manager),
                "{name} runs a unit manager"
            );
        }
    }
}
