use lamp_surround::{configuration::Generation, owner::*, reset, setup, startup, update};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

const NOW: u64 = 100_000_000;
const CLOCK: &str = "boot-clock";
const SESSION: &str = "coordinator";

#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl Clock {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(NOW)))
    }
    fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
    fn callback(&self) -> Arc<dyn Fn() -> u64 + Send + Sync> {
        let now = self.0.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    }
}

struct Root {
    _temp: tempfile::TempDir,
    path: PathBuf,
}
impl Root {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("sur-own-")
            .tempdir_in("/tmp")
            .unwrap();
        let path = temp.path().canonicalize().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self { _temp: temp, path }
    }
    fn socket(&self) -> PathBuf {
        self.path.join("owner.sock")
    }
    fn bind(&self) -> UnixListener {
        let listener = UnixListener::bind(self.socket()).unwrap();
        fs::set_permissions(self.socket(), fs::Permissions::from_mode(0o600)).unwrap();
        listener
    }
    fn client(&self, enabled: bool) -> Client {
        Client::new(self.socket(), enabled, CLOCK.into(), SESSION.into()).unwrap()
    }
}

fn generation(digit: char) -> Generation {
    Generation {
        id: digit.to_string().repeat(32),
        document_sha256: digit.to_string().repeat(64),
    }
}
fn voice(at: u64) -> startup::Receipt {
    startup::Receipt {
        component: startup::Component::Voice,
        generation: 1,
        owner_session: "voice-new".into(),
        boot_id: "boot".into(),
        clock_id: CLOCK.into(),
        sequence: 1,
        observed_us: at,
        capability: startup::Capability::Listening,
    }
}
fn update_action() -> update::Action {
    update::Action {
        operation_id: "upgrade".into(),
        step_id: 1,
        clock_id: CLOCK.into(),
        coordinator_session: SESSION.into(),
        issued_us: NOW,
        deadline_us: NOW + ACTION_TIMEOUT_US,
        kind: update::ActionKind::Inspect {
            uncertain_step: None,
        },
        guard: None,
    }
}
fn reset_action() -> reset::Action {
    reset::Action {
        operation_id: "erase".into(),
        request_id: "erase:Voice:Prepare".into(),
        clock_id: CLOCK.into(),
        coordinator_session: SESSION.into(),
        store: reset::Store::Voice,
        kind: reset::Kind::Prepare,
        expected_owner_session: None,
        issued_us: NOW,
        not_after_us: NOW + reset::QUIET_TIMEOUT_US,
    }
}
fn configure() -> Command {
    Command::Configure {
        operation_id: "settings".into(),
        expected: Some(generation('a')),
        target: generation('b'),
    }
}
fn barrier(at: u64) -> ConfigurationBarrier {
    ConfigurationBarrier {
        owner_session: Some("voice-old".into()),
        exclusive_token: "fence".into(),
        observed_since_us: at - CONFIGURATION_QUIET_US,
        observed_us: at,
        last_heard_us: None,
        last_said_us: None,
        complete: true,
        admission_closed: true,
        workers_gone: true,
    }
}
fn configuration_reply(
    at: u64,
    operation_id: String,
    expected: Option<Generation>,
    target: Generation,
) -> ConfigurationReceipt {
    ConfigurationReceipt {
        operation_id,
        expected: expected.clone(),
        target: target.clone(),
        supervisor_session: "supervisor".into(),
        observed_us: at,
        outcome: ConfigurationOutcome::Applied {
            barrier: barrier(at),
            selection: ConfigurationSelection {
                from: expected,
                to: Some(target.clone()),
                exclusive_token: "fence".into(),
                observed_us: at,
                durable: true,
            },
            ready: ConfigurationReady {
                owner_session: "voice-new".into(),
                generation: target,
                observed_us: at,
                capability: VoiceCapability::Listening,
            },
        },
    }
}
fn valid_reply(request: &RequestEnvelope) -> ReplyEnvelope {
    let at = request.issued_us;
    let reply = match &request.command {
        Command::Probe => Reply::Probe(Probe {
            unix_seconds: 1_700_000_000,
            time_synchronized: true,
            observed_us: at,
            runtime: setup::RuntimeState::Ready,
            startup: vec![voice(at)],
        }),
        Command::Update(action) => Reply::Update(update::Receipt {
            operation_id: action.operation_id.clone(),
            step_id: action.step_id,
            clock_id: action.clock_id.clone(),
            coordinator_session: action.coordinator_session.clone(),
            observed_us: at,
            guard: action.guard.clone(),
            evidence: update::Evidence::Inspected {
                inventory: update::Inventory {
                    current: None,
                    supervisor_session: "supervisor".into(),
                    exclusive_token: "fence".into(),
                    voice: None,
                    body: None,
                    eyes: None,
                    absent_workers_gone: true,
                    body_safe_when_absent: false,
                },
            },
        }),
        Command::Reset(action) => Reply::Reset(reset::Receipt {
            operation_id: action.operation_id.clone(),
            request_id: action.request_id.clone(),
            clock_id: action.clock_id.clone(),
            coordinator_session: action.coordinator_session.clone(),
            owner_session: "voice-old".into(),
            observed_us: at,
            outcome: reset::Outcome::Prepared {
                writes_frozen: true,
                durable_barrier: true,
                voice_quiet_and_closed: true,
            },
        }),
        Command::Configure {
            operation_id,
            expected,
            target,
        } => Reply::Configure(configuration_reply(
            at,
            operation_id.clone(),
            expected.clone(),
            target.clone(),
        )),
    };
    ReplyEnvelope {
        schema_version: 1,
        id: request.id.clone(),
        clock_id: request.clock_id.clone(),
        coordinator_session: request.coordinator_session.clone(),
        observed_us: at,
        reply,
    }
}
fn frame(reply: &ReplyEnvelope) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(reply).unwrap();
    bytes.push(b'\n');
    bytes
}
async fn read_request(stream: &mut tokio::net::UnixStream) -> RequestEnvelope {
    let mut bytes = Vec::new();
    stream
        .take((MAX_FRAME + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .unwrap();
    assert!(bytes.len() <= MAX_FRAME && bytes.last() == Some(&b'\n'));
    serde_json::from_slice(&bytes).unwrap()
}
async fn scripted(
    command: Command,
    reply: impl FnOnce(&RequestEnvelope, &Clock) -> Vec<u8> + Send + 'static,
) -> Result<Reply, Error> {
    let root = Root::new();
    let listener = root.bind();
    let clock = Clock::new();
    let server_clock = clock.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        let bytes = reply(&request, &server_clock);
        let _ = stream.write_all(&bytes).await;
    });
    let answer = root
        .client(true)
        .call("request-one", command, clock.callback())
        .await;
    server.await.unwrap();
    answer
}
async fn changed(
    command: Command,
    change: impl FnOnce(&mut ReplyEnvelope) + Send + 'static,
) -> Result<Reply, Error> {
    scripted(command, move |request, _| {
        let mut reply = valid_reply(request);
        change(&mut reply);
        frame(&reply)
    })
    .await
}

#[tokio::test]
async fn four_typed_roundtrips_preserve_commands_and_required_deadlines() {
    for command in [
        Command::Probe,
        Command::Update(update_action()),
        Command::Reset(reset_action()),
        configure(),
    ] {
        let original = command.clone();
        let result = scripted(command, move |request, _| {
            assert_eq!(request.schema_version, 1);
            assert_eq!(request.command, original);
            assert_eq!(request.clock_id, CLOCK);
            assert_eq!(request.coordinator_session, SESSION);
            assert_eq!(
                request.expires_us - request.issued_us,
                if matches!(original, Command::Probe) {
                    PROBE_TIMEOUT_US
                } else if matches!(original, Command::Configure { .. } | Command::Reset(_)) {
                    QUIET_TIMEOUT_US
                } else {
                    ACTION_TIMEOUT_US
                }
            );
            frame(&valid_reply(request))
        })
        .await
        .unwrap();
        if let Reply::Configure(receipt) = result {
            assert!(receipt.applied());
        }
    }
}

#[tokio::test]
async fn disabled_never_connects_and_constructor_only_checks_configuration() {
    let root = Root::new();
    let listener = root.bind();
    let client = root.client(false);
    assert_eq!(
        client
            .call(
                "request",
                Command::Probe,
                Arc::new(|| panic!("disabled read clock"))
            )
            .await,
        Err(Error::Disabled)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(15), listener.accept())
            .await
            .is_err()
    );
    assert!(
        Client::new(
            root.path.join("absent").join("owner.sock"),
            true,
            CLOCK.into(),
            SESSION.into()
        )
        .is_ok()
    );
    for path in [
        PathBuf::from("relative.sock"),
        root.path.join("../outside.sock"),
        root.path.join("a".repeat(101)),
    ] {
        assert!(Client::new(path, true, CLOCK.into(), SESSION.into()).is_err());
    }
}

#[tokio::test]
async fn missing_refused_regular_file_and_insecure_sockets_remain_unavailable_or_unsafe() {
    let root = Root::new();
    assert_eq!(
        root.client(true)
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::Unavailable)
    );
    let listener = root.bind();
    drop(listener);
    assert_eq!(
        root.client(true)
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::Unavailable)
    );
    fs::remove_file(root.socket()).unwrap();
    fs::write(root.socket(), b"not a socket").unwrap();
    fs::set_permissions(root.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        root.client(true)
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::UnsafeEndpoint)
    );
    fs::remove_file(root.socket()).unwrap();
    let _listener = root.bind();
    for mode in [0o666, 0o660, 0o700, 0o400] {
        fs::set_permissions(root.socket(), fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            root.client(true)
                .call("request", Command::Probe, Clock::new().callback())
                .await,
            Err(Error::UnsafeEndpoint)
        );
    }
}

#[tokio::test]
async fn socket_links_parent_links_and_public_parent_directories_are_rejected() {
    let root = Root::new();
    let _listener = root.bind();
    let alias = root.path.join("alias.sock");
    symlink(root.socket(), &alias).unwrap();
    let client = Client::new(alias, true, CLOCK.into(), SESSION.into()).unwrap();
    assert_eq!(
        client
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::UnsafeEndpoint)
    );
    let alias_root = Root::new();
    let parent_alias = alias_root.path.join("parent");
    symlink(&root.path, &parent_alias).unwrap();
    let client = Client::new(
        parent_alias.join("owner.sock"),
        true,
        CLOCK.into(),
        SESSION.into(),
    )
    .unwrap();
    assert_eq!(
        client
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::UnsafeEndpoint)
    );
    fs::set_permissions(&root.path, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        root.client(true)
            .call("request", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::UnsafeEndpoint)
    );
}

#[tokio::test]
async fn changing_socket_inode_after_connect_refuses_before_any_command_is_sent() {
    let root = Root::new();
    let listener = root.bind();
    let socket = root.socket();
    let clock = Clock::new();
    let callback = clock.callback();
    let calls = AtomicU64::new(0);
    let retained = Arc::new(Mutex::new(None));
    let held = retained.clone();
    let observed = Arc::new(move || {
        // Initial stamp, then pre-connect check. Replace before connection so
        // pre/post endpoint identities differ without relying on scheduling.
        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
            fs::remove_file(&socket).unwrap();
            let replacement = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
            *held.lock().unwrap() = Some(replacement);
        }
        callback()
    });
    assert_eq!(
        root.client(true)
            .call("request", Command::Probe, observed)
            .await,
        Err(Error::UnsafeEndpoint)
    );
    drop(listener);
}

#[tokio::test]
async fn foreign_version_id_clock_session_and_reply_kind_are_rejected() {
    for case in 0..5 {
        let result = changed(Command::Probe, move |reply| match case {
            0 => reply.schema_version = 2,
            1 => reply.id = "foreign".into(),
            2 => reply.clock_id = "previous-boot".into(),
            3 => reply.coordinator_session = "retired".into(),
            _ => {
                reply.reply = valid_reply(&RequestEnvelope {
                    schema_version: 1,
                    id: "x".into(),
                    clock_id: CLOCK.into(),
                    coordinator_session: SESSION.into(),
                    issued_us: NOW,
                    expires_us: NOW + ACTION_TIMEOUT_US,
                    command: Command::Reset(reset_action()),
                })
                .reply
            }
        })
        .await;
        assert_eq!(result, Err(Error::InvalidReply), "case {case}");
    }
}

#[tokio::test]
async fn malformed_unknown_fields_oversized_missing_newline_and_extra_frames_are_rejected() {
    for case in 0..7 {
        let result = scripted(Command::Probe, move |request, _| {
            let valid = frame(&valid_reply(request));
            match case {
                0 => b"{not-json}\n".to_vec(),
                1 => {
                    let mut value = serde_json::to_value(valid_reply(request)).unwrap();
                    value["unexpected"] = true.into();
                    format!("{value}\n").into_bytes()
                }
                2 => vec![b'x'; MAX_FRAME + 1],
                3 => valid[..valid.len() - 1].to_vec(),
                4 => [valid.clone(), valid].concat(),
                5 => [valid, b"secret trailing junk".to_vec()].concat(),
                _ => vec![],
            }
        })
        .await;
        assert_eq!(
            result,
            Err(if case == 2 {
                Error::Oversized
            } else {
                Error::InvalidReply
            }),
            "case {case}"
        );
    }
}

#[tokio::test]
async fn exactly_bounded_single_frame_is_accepted_and_old_periodic_samples_keep_their_own_age() {
    let reply = scripted(Command::Probe, |request, _| {
        let mut value = valid_reply(request);
        if let Reply::Probe(probe) = &mut value.reply {
            probe.startup[0].observed_us = NOW - RECEIPT_AGE_US + 1;
        }
        let mut bytes = frame(&value);
        bytes.pop();
        bytes.resize(MAX_FRAME - 1, b' ');
        bytes.push(b'\n');
        bytes
    })
    .await
    .unwrap();
    let Reply::Probe(probe) = reply else {
        panic!("wrong reply")
    };
    assert_eq!(probe.startup[0].observed_us, NOW - RECEIPT_AGE_US + 1);
}

#[tokio::test]
async fn future_pre_request_stale_and_nested_receipts_are_checked_at_arrival() {
    for case in 0..6 {
        let result = scripted(Command::Update(update_action()), move |request, clock| {
            let mut reply = valid_reply(request);
            match case {
                0 => reply.observed_us += 1,
                1 => reply.observed_us -= 1,
                2 => clock.set(NOW + RECEIPT_AGE_US),
                3 => {
                    clock.set(NOW + RECEIPT_AGE_US);
                    reply.observed_us = NOW + RECEIPT_AGE_US;
                }
                4 => {
                    if let Reply::Update(r) = &mut reply.reply {
                        r.observed_us += 1;
                    }
                }
                _ => {
                    if let Reply::Update(r) = &mut reply.reply {
                        r.observed_us -= 1;
                    }
                }
            }
            frame(&reply)
        })
        .await;
        assert_eq!(result, Err(Error::InvalidReply), "case {case}");
    }
    assert!(
        scripted(Command::Update(update_action()), |request, clock| {
            clock.set(NOW + RECEIPT_AGE_US - 1);
            frame(&valid_reply(request))
        })
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn monotonic_reversal_and_expired_intents_refuse_dispatch_and_late_receipts() {
    let root = Root::new();
    let client = root.client(true);
    let clock = Clock::new();
    assert_eq!(
        client
            .call("request", Command::Probe, clock.callback())
            .await,
        Err(Error::Unavailable)
    );
    clock.set(NOW - 1);
    assert_eq!(
        client
            .call("request", Command::Probe, clock.callback())
            .await,
        Err(Error::ClockChanged)
    );
    let mut action = update_action();
    action.deadline_us = NOW;
    assert_eq!(
        root.client(true)
            .call("request", Command::Update(action), Clock::new().callback())
            .await,
        Err(Error::InvalidRequest)
    );
    assert_eq!(
        scripted(Command::Update(update_action()), |request, clock| {
            clock.set(request.expires_us);
            frame(&valid_reply(request))
        })
        .await,
        Err(Error::Timeout)
    );
    assert_eq!(
        scripted(Command::Update(update_action()), |request, clock| {
            clock.set(NOW - 1);
            frame(&valid_reply(request))
        })
        .await,
        Err(Error::ClockChanged)
    );
}

#[tokio::test]
async fn timeout_caps_stalled_clock_while_other_async_work_remains_responsive() {
    let root = Root::new();
    let listener = root.bind();
    let (received, receipt) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        received.send(request).unwrap();
        std::future::pending::<()>().await;
    });
    let mut action = update_action();
    action.deadline_us = NOW + 100_000;
    let client = Arc::new(root.client(true));
    let concurrent = client.clone();
    let began = Instant::now();
    let call = tokio::spawn(async move {
        concurrent
            .call("request", Command::Update(action), Clock::new().callback())
            .await
    });
    let request = receipt.await.unwrap();
    assert_eq!(request.expires_us, NOW + 100_000);
    assert_eq!(
        client
            .call("second", Command::Probe, Clock::new().callback())
            .await,
        Err(Error::Busy)
    );
    tokio::time::sleep(Duration::from_millis(2)).await;
    assert!(!call.is_finished(), "owner wait blocked other async work");
    assert_eq!(call.await.unwrap(), Err(Error::Timeout));
    assert!(began.elapsed() < Duration::from_secs(3));
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn newline_without_end_of_response_does_not_escape_the_deadline() {
    let root = Root::new();
    let listener = root.bind();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        stream
            .write_all(&frame(&valid_reply(&request)))
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    let mut action = update_action();
    action.deadline_us = NOW + 100_000;
    assert_eq!(
        root.client(true)
            .call("request", Command::Update(action), Clock::new().callback())
            .await,
        Err(Error::Timeout)
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn quiet_waits_can_accumulate_45_seconds_after_admission_without_extending_the_120_second_limit()
 {
    for elapsed in [45_000_001, QUIET_TIMEOUT_US - 1, QUIET_TIMEOUT_US] {
        let mut action = update_action();
        action.kind = update::ActionKind::ObserveQuiet;
        action.deadline_us = NOW + QUIET_TIMEOUT_US;
        for command in [
            Command::Update(action),
            configure(),
            Command::Reset(reset_action()),
        ] {
            let result = scripted(command, move |request, clock| {
                assert_eq!(request.expires_us, NOW + QUIET_TIMEOUT_US);
                let at = NOW + elapsed;
                clock.set(at);
                let mut reply = valid_reply(request);
                reply.observed_us = at;
                match &mut reply.reply {
                    Reply::Update(receipt) => {
                        receipt.observed_us = at;
                        receipt.evidence = update::Evidence::Quiet {
                            observation: update::Quiet {
                                voice_session: "voice-old".into(),
                                observed_since_us: NOW,
                                last_heard_us: Some(NOW),
                                last_said_us: Some(NOW),
                                complete: true,
                            },
                        };
                    }
                    Reply::Configure(receipt) => {
                        receipt.observed_us = at;
                        if let ConfigurationOutcome::Applied {
                            barrier,
                            selection,
                            ready,
                        } = &mut receipt.outcome
                        {
                            barrier.observed_since_us = NOW;
                            barrier.observed_us = at;
                            barrier.last_heard_us = Some(NOW);
                            selection.observed_us = at;
                            ready.observed_us = at;
                        }
                    }
                    Reply::Reset(receipt) => receipt.observed_us = at,
                    _ => panic!("wrong command"),
                }
                frame(&reply)
            })
            .await;
            if elapsed == QUIET_TIMEOUT_US {
                assert_eq!(result, Err(Error::Timeout));
            } else {
                assert!(result.is_ok(), "elapsed={elapsed}: {result:?}");
            }
        }
    }
}

#[tokio::test]
async fn retrying_update_and_reset_actions_keeps_their_original_absolute_deadline() {
    let mut action = update_action();
    action.kind = update::ActionKind::ObserveQuiet;
    action.deadline_us = NOW + QUIET_TIMEOUT_US;
    for command in [Command::Update(action), Command::Reset(reset_action())] {
        let root = Root::new();
        let listener = root.bind();
        let client = root.client(true);
        let clock = Clock::new();
        let server = tokio::spawn(async move {
            for issued in [NOW, NOW + 10_000_000] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                assert_eq!(request.issued_us, issued);
                assert_eq!(request.expires_us, NOW + QUIET_TIMEOUT_US);
                stream
                    .write_all(&frame(&valid_reply(&request)))
                    .await
                    .unwrap();
            }
        });
        for issued in [NOW, NOW + 10_000_000] {
            clock.set(issued);
            client
                .call("stable-step", command.clone(), clock.callback())
                .await
                .unwrap();
        }
        server.await.unwrap();
        clock.set(NOW + QUIET_TIMEOUT_US);
        assert_eq!(
            client.call("stable-step", command, clock.callback()).await,
            Err(Error::InvalidRequest)
        );
    }
}

#[tokio::test]
async fn probe_needs_fresh_voice_capability_and_preserves_privacy_inhibition() {
    for case in 0..7 {
        assert_eq!(
            changed(Command::Probe, move |reply| {
                if let Reply::Probe(p) = &mut reply.reply {
                    match case {
                        0 => p.startup.clear(),
                        1 => p.startup[0].capability = startup::Capability::Starting,
                        2 => p.startup[0].clock_id = "foreign".into(),
                        3 => p.startup[0].observed_us -= RECEIPT_AGE_US,
                        4 => p.startup.push(p.startup[0].clone()),
                        5 => p.startup[0].component = startup::Component::Body,
                        _ => p.unix_seconds = 0,
                    }
                }
            })
            .await,
            Err(Error::InvalidReply),
            "case {case}"
        );
    }
    let Reply::Probe(probe) = changed(Command::Probe, |reply| {
        if let Reply::Probe(p) = &mut reply.reply {
            p.startup[0].capability = startup::Capability::Inhibited;
        }
    })
    .await
    .unwrap() else {
        panic!("wrong reply")
    };
    assert_eq!(
        probe.voice_capability(),
        Some(startup::Capability::Inhibited)
    );
}

#[tokio::test]
async fn update_receipts_keep_exact_step_guard_and_owner_failure_is_private() {
    for case in 0..4 {
        assert_eq!(
            changed(Command::Update(update_action()), move |reply| {
                if let Reply::Update(r) = &mut reply.reply {
                    match case {
                        0 => r.step_id += 1,
                        1 => {
                            r.guard = Some(update::Guard {
                                supervisor_session: "other".into(),
                                exclusive_token: "foreign".into(),
                                expected_current: None,
                                owner: None,
                                quiesce_token: None,
                            })
                        }
                        2 => {
                            r.evidence = update::Evidence::Failed {
                                reason: "secret provider credential".into(),
                            }
                        }
                        _ => r.operation_id = "old-operation".into(),
                    }
                }
            })
            .await,
            Err(Error::InvalidReply)
        );
    }
    assert!(
        changed(Command::Update(update_action()), |reply| {
            if let Reply::Update(r) = &mut reply.reply {
                r.evidence = update::Evidence::Failed {
                    reason: "owner action failed".into(),
                };
            }
        })
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn reset_receipts_keep_exact_request_and_leave_barrier_validation_to_the_reset_machine() {
    assert_eq!(
        changed(Command::Reset(reset_action()), |reply| {
            if let Reply::Reset(r) = &mut reply.reply {
                r.request_id = "other-store".into();
            }
        })
        .await,
        Err(Error::InvalidReply)
    );
    let Reply::Reset(receipt) = changed(Command::Reset(reset_action()), |reply| {
        if let Reply::Reset(r) = &mut reply.reply {
            r.outcome = reset::Outcome::Prepared {
                writes_frozen: false,
                durable_barrier: false,
                voice_quiet_and_closed: false,
            };
        }
    })
    .await
    .unwrap() else {
        panic!("wrong reply")
    };
    assert_eq!(
        receipt.outcome,
        reset::Outcome::Prepared {
            writes_frozen: false,
            durable_barrier: false,
            voice_quiet_and_closed: false
        }
    );
}

#[tokio::test]
async fn configuration_requires_exact_binding_full_quiet_exit_cas_and_fresh_new_owner() {
    for case in 0..18 {
        assert_eq!(
            changed(configure(), move |reply| {
                if let Reply::Configure(r) = &mut reply.reply {
                    if case == 0 {
                        r.expected = None;
                        return;
                    }
                    if case == 1 {
                        r.target = generation('c');
                        return;
                    }
                    if case == 2 {
                        r.operation_id = "foreign".into();
                        return;
                    }
                    if let ConfigurationOutcome::Applied {
                        barrier,
                        selection,
                        ready,
                    } = &mut r.outcome
                    {
                        match case {
                            3 => barrier.observed_since_us += 1,
                            4 => barrier.last_heard_us = Some(NOW - 1),
                            5 => barrier.last_said_us = Some(NOW - 1),
                            6 => barrier.admission_closed = false,
                            7 => barrier.workers_gone = false,
                            8 => selection.durable = false,
                            9 => selection.exclusive_token = "foreign".into(),
                            10 => ready.owner_session = "voice-old".into(),
                            11 => ready.generation = generation('c'),
                            12 => ready.observed_us += 1,
                            13 => barrier.complete = false,
                            14 => barrier.owner_session = None,
                            15 => selection.from = None,
                            16 => barrier.last_heard_us = Some(NOW + 1),
                            _ => barrier.observed_us += 1,
                        }
                    }
                }
            })
            .await,
            Err(Error::InvalidReply),
            "case {case}"
        );
    }
    let Reply::Configure(receipt) = changed(configure(), |reply| {
        if let Reply::Configure(r) = &mut reply.reply
            && let ConfigurationOutcome::Applied { ready, .. } = &mut r.outcome
        {
            ready.capability = VoiceCapability::Inhibited;
        }
    })
    .await
    .unwrap() else {
        panic!("wrong reply")
    };
    assert!(receipt.applied());
}

#[tokio::test]
async fn durable_configuration_rollback_is_explicit_and_never_reported_as_applied() {
    for previous in [Some(generation('a')), None] {
        let command = Command::Configure {
            operation_id: "settings".into(),
            expected: previous,
            target: generation('b'),
        };
        let Reply::Configure(receipt) = changed(command, |reply| {
            if let Reply::Configure(r) = &mut reply.reply {
                r.outcome = ConfigurationOutcome::RolledBack {
                    failure: ConfigurationFailure::StartFailed,
                    barrier: barrier(NOW),
                    selection: ConfigurationSelection {
                        from: Some(r.target.clone()),
                        to: r.expected.clone(),
                        exclusive_token: "fence".into(),
                        observed_us: NOW,
                        durable: true,
                    },
                    ready: r.expected.clone().map(|generation| ConfigurationReady {
                        owner_session: "recovered-owner".into(),
                        generation,
                        observed_us: NOW,
                        capability: VoiceCapability::Inhibited,
                    }),
                };
            }
        })
        .await
        .unwrap() else {
            panic!("wrong reply")
        };
        assert!(!receipt.applied());
    }
}

#[tokio::test]
async fn rejected_or_incomplete_configuration_never_promotes_a_generation() {
    for outcome in [
        ConfigurationOutcome::Rejected {
            failure: ConfigurationFailure::QuietTimeout,
        },
        ConfigurationOutcome::Incomplete {
            failure: ConfigurationFailure::RecoveryFailed,
            selected: Some(generation('b')),
        },
    ] {
        let Reply::Configure(receipt) = changed(configure(), move |reply| {
            if let Reply::Configure(r) = &mut reply.reply {
                r.outcome = outcome;
            }
        })
        .await
        .unwrap() else {
            panic!("wrong reply")
        };
        assert!(!receipt.applied());
    }
}

#[tokio::test]
async fn configuration_rollback_must_prove_previous_selection_and_its_fresh_owner() {
    for case in 0..4 {
        assert_eq!(
            changed(configure(), move |reply| {
                if let Reply::Configure(r) = &mut reply.reply {
                    let mut selection = ConfigurationSelection {
                        from: Some(r.target.clone()),
                        to: r.expected.clone(),
                        exclusive_token: "fence".into(),
                        observed_us: NOW,
                        durable: true,
                    };
                    let mut ready = Some(ConfigurationReady {
                        owner_session: "recovered".into(),
                        generation: r.expected.clone().unwrap(),
                        observed_us: NOW,
                        capability: VoiceCapability::Inhibited,
                    });
                    match case {
                        0 => selection.from = None,
                        1 => selection.to = Some(r.target.clone()),
                        2 => ready = None,
                        _ => ready.as_mut().unwrap().generation = r.target.clone(),
                    }
                    r.outcome = ConfigurationOutcome::RolledBack {
                        failure: ConfigurationFailure::StartFailed,
                        barrier: barrier(NOW),
                        selection,
                        ready,
                    };
                }
            })
            .await,
            Err(Error::InvalidReply),
            "case {case}"
        );
    }
}

#[tokio::test]
async fn invalid_requests_and_excessive_frames_do_not_connect() {
    let root = Root::new();
    let listener = root.bind();
    for command in [
        Command::Configure {
            operation_id: "settings".into(),
            expected: None,
            target: generation('z'),
        },
        Command::Configure {
            operation_id: "settings".into(),
            expected: Some(generation('a')),
            target: generation('a'),
        },
    ] {
        assert_eq!(
            root.client(true)
                .call("request", command, Clock::new().callback())
                .await,
            Err(Error::InvalidRequest)
        );
    }
    let mut action = update_action();
    action.guard = Some(update::Guard {
        supervisor_session: "a".repeat(MAX_FRAME),
        exclusive_token: "fence".into(),
        expected_current: None,
        owner: None,
        quiesce_token: None,
    });
    assert_eq!(
        root.client(true)
            .call("request", Command::Update(action), Clock::new().callback())
            .await,
        Err(Error::Oversized)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(15), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn error_messages_are_fixed_and_contain_no_peer_text() {
    for error in [
        Error::Disabled,
        Error::Busy,
        Error::Unavailable,
        Error::UnsafeEndpoint,
        Error::InvalidRequest,
        Error::InvalidReply,
        Error::Oversized,
        Error::Timeout,
        Error::ClockChanged,
    ] {
        assert!(error.to_string().len() < 80);
        assert!(!error.to_string().contains('/'));
    }
}
