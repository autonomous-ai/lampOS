use crate::support;
use lamp_surround::{
    atomic,
    config::{ProviderDocument, Secret},
    configuration::Configurations,
    network::{self, NetworkAdapter, NetworkResult},
    operator::{self, Command, ErrorCode, InputSecret, Phase, Submission},
    owner, reset, setup, startup, update,
    worker::*,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    sync::oneshot,
};

const NOW: u64 = 100_000_000;
const CLOCK: &str = "test-clock";
fn id(n: u8) -> String {
    format!("{n:032x}")
}
fn document() -> ProviderDocument {
    ProviderDocument::parse(br#"{"schema_version":1,"provider":{"endpoint":"wss://provider.example/live","model":"gemini-3.8-live","voice":"Kore","credential_file":"/unused/key"}}"#).unwrap()
}
fn provider() -> Command {
    Command::SaveProvider {
        document: document(),
        credential: InputSecret(Secret::new(b"TOP_SECRET_FIXTURE_KEY".to_vec()).unwrap()),
        instructions: Some("Private fixture instructions".into()),
    }
}

struct Host {
    _temp: tempfile::TempDir,
    now: Arc<AtomicU64>,
    shared: operator::Shared,
    config: WorkerConfig,
}
impl Host {
    fn new(enabled: bool) -> Self {
        let temp = tempfile::Builder::new()
            .prefix("wrk-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(fs::canonicalize("/tmp").unwrap())
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        Self {
            config: WorkerConfig {
                enabled,
                state_dir: root.join("worker"),
                configuration_dir: root.join("config"),
                owner_socket: Some(root.join("owner.sock")),
                network_mode: setup::SetupMode::Wifi,
                reset_stores: vec![reset::Store::Voice, reset::Store::Memory],
                ..WorkerConfig::default()
            },
            _temp: temp,
            now: Arc::new(AtomicU64::new(NOW)),
            shared: operator::Shared::new(enabled),
        }
    }
    fn clock(&self) -> operator::Clock {
        let now = self.now.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    }
    fn worker(&self, session: &str, network: Box<dyn NetworkAdapter + Send>) -> Worker {
        Worker::open(
            self.config.clone(),
            self.shared.clone(),
            self.clock(),
            CLOCK,
            session,
            network,
        )
        .unwrap()
    }
    fn bind(&self) -> UnixListener {
        let path = self.config.owner_socket.as_ref().unwrap();
        let listener = UnixListener::bind(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        listener
    }
}

struct CountAdapter {
    count: Arc<AtomicUsize>,
    inner: network::UnsupportedAdapter,
}
impl NetworkAdapter for CountAdapter {
    fn submit(&mut self, request: network::NetworkRequest) -> Result<(), network::NetworkError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.inner.submit(request)
    }
    fn poll(&mut self) -> Option<network::Completion> {
        self.inner.poll()
    }
}
fn counted() -> (Box<dyn NetworkAdapter + Send>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    (
        Box::new(CountAdapter {
            count: count.clone(),
            inner: network::UnsupportedAdapter::default(),
        }),
        count,
    )
}
async fn submit(worker: &mut Worker, n: u8, command: Command) -> Result<(), ErrorCode> {
    let (admitted, rx) = oneshot::channel();
    worker
        .run_one(Submission {
            id: id(n),
            command,
            admitted,
        })
        .await
        .unwrap();
    rx.await.unwrap()
}
fn network() -> setup::NetworkEvidence {
    setup::NetworkEvidence {
        kind: setup::LinkKind::Wifi,
        link_up: true,
        address: Some("192.0.2.7".parse().unwrap()),
        addressing: setup::Addressing::DhcpBound,
        setup_ap: false,
        cloud: setup::Reachability::Unknown,
        authentication_rejected: false,
    }
}
fn scripted(
    steps: Vec<(
        network::CommandKind,
        Result<NetworkResult, network::NetworkError>,
    )>,
) -> Box<dyn NetworkAdapter + Send> {
    Box::new(
        network::ScriptedAdapter::new(
            steps
                .into_iter()
                .map(|(expected, result)| network::ScriptStep {
                    expected,
                    observed_us: NOW,
                    result,
                })
                .collect(),
        )
        .unwrap(),
    )
}

async fn server(
    listener: UnixListener,
    count: usize,
    handler: Arc<dyn Fn(owner::RequestEnvelope) -> owner::Reply + Send + Sync>,
) {
    for _ in 0..count {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let request: owner::RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        let reply = owner::ReplyEnvelope {
            schema_version: 1,
            id: request.id.clone(),
            clock_id: request.clock_id.clone(),
            coordinator_session: request.coordinator_session.clone(),
            observed_us: NOW,
            reply: handler(request),
        };
        let mut bytes = serde_json::to_vec(&reply).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).await.unwrap();
        stream.shutdown().await.unwrap();
    }
}
fn probe(synchronized: bool, voice_at: u64) -> owner::Reply {
    owner::Reply::Probe(owner::Probe {
        unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        time_synchronized: synchronized,
        observed_us: NOW,
        runtime: setup::RuntimeState::Ready,
        startup: vec![startup::Receipt {
            component: startup::Component::Voice,
            generation: 1,
            owner_session: "voice".into(),
            boot_id: CLOCK.into(),
            clock_id: CLOCK.into(),
            sequence: 1,
            observed_us: voice_at,
            capability: startup::Capability::Inhibited,
        }],
    })
}

#[tokio::test]
async fn disabled_worker_is_read_only_and_never_touches_an_adapter() {
    let host = Host::new(false);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    assert!(!host.config.state_dir.exists());
    assert!(!host.config.configuration_dir.exists());
    assert_eq!(
        submit(&mut worker, 1, provider()).await,
        Err(ErrorCode::Disabled)
    );
    assert_eq!(
        submit(&mut worker, 2, Command::ProbeNetwork {}).await,
        Err(ErrorCode::Disabled)
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(!host.config.state_dir.exists());
    assert!(!host.shared.busy.load(Ordering::Acquire));
}

#[tokio::test]
async fn provider_generation_is_staged_and_status_never_contains_secrets_or_claims_activation() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    assert_eq!(submit(&mut worker, 1, provider()).await, Ok(()));
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Complete);
    assert!(view.candidate_configuration.is_some());
    assert!(view.current_configuration.is_none());
    assert!(view.runtime.is_none());
    let text = serde_json::to_string(&*view).unwrap();
    assert!(!text.contains("TOP_SECRET"));
    assert!(!text.contains("Private fixture"));
    drop(view);
    let journal = fs::read_to_string(host.config.state_dir.join("operations.json")).unwrap();
    assert!(!journal.contains("TOP_SECRET"));
    assert!(!journal.contains("Private fixture"));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn durable_admission_precedes_work_and_dropped_http_caller_does_not_cancel_it() {
    let host = Host::new(true);
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    let (admitted, rx) = oneshot::channel();
    drop(rx);
    worker
        .run_one(Submission {
            id: id(1),
            command: provider(),
            admitted,
        })
        .await
        .unwrap();
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(host.config.state_dir.join("operations.json")).unwrap())
            .unwrap();
    assert_eq!(journal["operation"]["phase"], "complete");
    assert!(
        host.shared
            .view
            .read()
            .unwrap()
            .candidate_configuration
            .is_some()
    );
}

#[tokio::test]
async fn admission_disk_error_refuses_before_any_adapter_or_configuration_write() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    fs::create_dir(host.config.state_dir.join(".operations.json.pending")).unwrap();
    let (admitted, rx) = oneshot::channel();
    assert!(
        worker
            .run_one(Submission {
                id: id(1),
                command: provider(),
                admitted
            })
            .await
            .is_err()
    );
    assert_eq!(rx.await.unwrap(), Err(ErrorCode::Storage));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        host.shared
            .view
            .read()
            .unwrap()
            .candidate_configuration
            .is_none()
    );
    assert!(!host.shared.busy.load(Ordering::Acquire));
}

#[tokio::test]
async fn duplicate_operation_identity_is_not_reexecuted() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    assert_eq!(submit(&mut worker, 1, provider()).await, Ok(()));
    assert_eq!(
        submit(&mut worker, 1, Command::ProbeNetwork {}).await,
        Err(ErrorCode::CheckStatus)
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn restart_marks_unfinished_admission_interrupted_without_resurrecting_input() {
    let host = Host::new(true);
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    submit(&mut worker, 1, provider()).await.unwrap();
    drop(worker);
    let path = host.config.state_dir.join("operations.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["operation"]["phase"] = serde_json::json!("working");
    atomic::write(&path, &serde_json::to_vec(&value).unwrap(), 16_384).unwrap();
    let (adapter, count) = counted();
    let _worker = host.worker("two", adapter);
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Interrupted);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn configuration_applies_only_after_private_owner_selection_and_fresh_capability_proof() {
    let host = Host::new(true);
    let listener = host.bind();
    let config_root = host.config.configuration_dir.clone();
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    submit(&mut worker, 1, provider()).await.unwrap();
    let target = host
        .shared
        .view
        .read()
        .unwrap()
        .candidate_configuration
        .clone()
        .unwrap();
    let handler = Arc::new(move |request: owner::RequestEnvelope| {
        let owner::Command::Configure {
            operation_id,
            expected,
            target,
        } = request.command
        else {
            panic!("wrong command");
        };
        let _lock = atomic::DirectoryLock::acquire(&config_root).unwrap();
        Configurations::open(&config_root)
            .unwrap()
            .select_quiescent(true, &expected, &target)
            .unwrap();
        owner::Reply::Configure(owner::ConfigurationReceipt {
            operation_id,
            expected: expected.clone(),
            target: target.clone(),
            supervisor_session: "owner".into(),
            observed_us: NOW,
            outcome: owner::ConfigurationOutcome::Applied {
                barrier: owner::ConfigurationBarrier {
                    owner_session: None,
                    exclusive_token: "fence".into(),
                    observed_since_us: NOW - 45_000_000,
                    observed_us: NOW,
                    last_heard_us: None,
                    last_said_us: None,
                    complete: true,
                    admission_closed: true,
                    workers_gone: true,
                },
                selection: owner::ConfigurationSelection {
                    from: expected,
                    to: Some(target.clone()),
                    exclusive_token: "fence".into(),
                    observed_us: NOW,
                    durable: true,
                },
                ready: owner::ConfigurationReady {
                    owner_session: "voice-new".into(),
                    generation: target,
                    observed_us: NOW,
                    capability: owner::VoiceCapability::Inhibited,
                },
            },
        })
    });
    let served = tokio::spawn(server(listener, 1, handler));
    submit(
        &mut worker,
        2,
        Command::ApplyConfiguration {
            target: target.clone(),
        },
    )
    .await
    .unwrap();
    served.await.unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(view.current_configuration, Some(target));
    assert_eq!(view.voice_capability, Some(startup::Capability::Inhibited));
    assert_eq!(view.runtime_observed_us, Some(NOW));
}

#[tokio::test]
async fn transport_success_without_configuration_activation_proof_is_failure() {
    let host = Host::new(true);
    let listener = host.bind();
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    submit(&mut worker, 1, provider()).await.unwrap();
    let target = host
        .shared
        .view
        .read()
        .unwrap()
        .candidate_configuration
        .clone()
        .unwrap();
    let handler = Arc::new(|request: owner::RequestEnvelope| {
        let owner::Command::Configure {
            operation_id,
            expected,
            target,
        } = request.command
        else {
            panic!("wrong command");
        };
        owner::Reply::Configure(owner::ConfigurationReceipt {
            operation_id,
            expected,
            target,
            supervisor_session: "owner".into(),
            observed_us: NOW,
            outcome: owner::ConfigurationOutcome::Rejected {
                failure: owner::ConfigurationFailure::Refused,
            },
        })
    });
    let served = tokio::spawn(server(listener, 1, handler));
    submit(&mut worker, 2, Command::ApplyConfiguration { target })
        .await
        .unwrap();
    served.await.unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Failed);
    assert_eq!(
        view.operation.as_ref().unwrap().error,
        Some(ErrorCode::OwnerRefused)
    );
    assert!(view.current_configuration.is_none());
}

#[tokio::test]
async fn wifi_commit_requires_selected_candidate_lan_and_keeps_runtime_distinct() {
    let host = Host::new(true);
    let listener = host.bind();
    let steps = vec![
        (network::CommandKind::JoinWifi, Ok(NetworkResult::Accepted)),
        (
            network::CommandKind::Probe,
            Ok(NetworkResult::Observation(network())),
        ),
        (network::CommandKind::Commit, Ok(NetworkResult::Committed)),
    ];
    let mut worker = host.worker("one", scripted(steps));
    let served = tokio::spawn(server(
        listener,
        1,
        Arc::new(|request| {
            assert!(matches!(request.command, owner::Command::Probe));
            probe(true, NOW)
        }),
    ));
    submit(
        &mut worker,
        1,
        Command::JoinWifi {
            ssid: "fixture-network".into(),
            password: Some(InputSecret(Secret::new(b"fixture-pass".to_vec()).unwrap())),
        },
    )
    .await
    .unwrap();
    served.await.unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Complete);
    assert!(view.setup.as_ref().unwrap().ready());
    assert_eq!(view.voice_capability, Some(startup::Capability::Inhibited));
    assert!(
        !serde_json::to_string(&*view)
            .unwrap()
            .contains("fixture-pass")
    );
}

#[tokio::test]
async fn ethernet_or_setup_ap_is_not_a_successful_wifi_candidate() {
    for wrong in [true, false] {
        let host = Host::new(true);
        let mut evidence = network();
        if wrong {
            evidence.kind = setup::LinkKind::Wired;
        } else {
            evidence.setup_ap = true;
        }
        let steps = vec![
            (network::CommandKind::JoinWifi, Ok(NetworkResult::Accepted)),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(evidence)),
            ),
            (
                network::CommandKind::Rollback,
                Ok(NetworkResult::RolledBack),
            ),
        ];
        let mut worker = host.worker("one", scripted(steps));
        submit(
            &mut worker,
            1,
            Command::JoinWifi {
                ssid: "fixture".into(),
                password: None,
            },
        )
        .await
        .unwrap();
        let view = host.shared.view.read().unwrap();
        assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Failed);
        assert!(!view.setup.as_ref().unwrap().ready());
    }
}

#[tokio::test]
async fn probe_retains_original_voice_observation_age() {
    let host = Host::new(true);
    let listener = host.bind();
    let mut worker = host.worker(
        "one",
        scripted(vec![(
            network::CommandKind::Probe,
            Ok(NetworkResult::Observation(network())),
        )]),
    );
    let served = tokio::spawn(server(
        listener,
        1,
        Arc::new(|_| probe(true, NOW - 1_000_000)),
    ));
    submit(&mut worker, 1, Command::ProbeNetwork {})
        .await
        .unwrap();
    served.await.unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.runtime_observed_us, Some(NOW - 1_000_000));
    assert!(view.at(NOW + 1_000_001).voice_capability.is_none());
}

#[tokio::test]
async fn reset_failure_is_durable_and_blocks_unrelated_work_until_explicit_resume() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    submit(
        &mut worker,
        1,
        Command::Reset {
            confirmation: RESET_CONFIRMATION.into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        host.shared.view.read().unwrap().reset_phase,
        Some(reset::Phase::Incomplete)
    );
    assert_eq!(
        submit(&mut worker, 2, provider()).await,
        Err(ErrorCode::Busy)
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    submit(&mut worker, 3, Command::ResumeReset {})
        .await
        .unwrap();
    assert_eq!(
        host.shared.view.read().unwrap().reset_phase,
        Some(reset::Phase::Incomplete)
    );
}

#[tokio::test]
async fn reset_requires_literal_confirmation_and_never_resets_on_a_generic_request() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    submit(
        &mut worker,
        1,
        Command::Reset {
            confirmation: "yes".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        host.shared
            .view
            .read()
            .unwrap()
            .operation
            .as_ref()
            .unwrap()
            .error,
        Some(ErrorCode::InvalidInput)
    );
    assert!(host.shared.view.read().unwrap().reset_phase.is_none());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn active_update_on_restart_reconciles_and_never_reports_unproven_success() {
    let host = Host::new(true);
    atomic::ensure_private_dir(&host.config.state_dir).unwrap();
    let mut coordinator =
        update::Coordinator::new(true, CLOCK, "old", update::State::default()).unwrap();
    coordinator
        .begin(
            NOW,
            update::Request {
                operation_id: id(1),
                kind: update::RequestKind::Update,
                target: update::ReleaseId("a".repeat(64)),
                expected_current: None,
            },
        )
        .unwrap();
    atomic::write(
        &host.config.state_dir.join("update.json"),
        &serde_json::to_vec(coordinator.state()).unwrap(),
        65_536,
    )
    .unwrap();
    let (adapter, count) = counted();
    let mut worker = host.worker("new", adapter);
    assert_eq!(
        submit(&mut worker, 2, provider()).await,
        Err(ErrorCode::Busy)
    );
    assert!(matches!(
        host.shared.view.read().unwrap().update_outcome,
        Some(update::Outcome::RecoveryFailed { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

struct PendingAdapter {
    request: Option<network::OperationId>,
    observed: Arc<AtomicUsize>,
}
impl NetworkAdapter for PendingAdapter {
    fn submit(&mut self, request: network::NetworkRequest) -> Result<(), network::NetworkError> {
        if self.request.is_some() {
            return Err(network::NetworkError::Busy);
        }
        self.observed.fetch_add(1, Ordering::SeqCst);
        self.request = Some(request.id);
        Ok(())
    }
    fn poll(&mut self) -> Option<network::Completion> {
        None
    }
}
#[tokio::test]
async fn slow_network_does_not_hold_status_lock_and_join_has_a_fixed_deadline() {
    let host = Host::new(true);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut worker = host.worker(
        "one",
        Box::new(PendingAdapter {
            request: None,
            observed: calls.clone(),
        }),
    );
    let (admitted, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        worker
            .run_one(Submission {
                id: id(1),
                command: Command::JoinWifi {
                    ssid: "fixture".into(),
                    password: None,
                },
                admitted,
            })
            .await
    });
    assert_eq!(rx.await.unwrap(), Ok(()));
    let start = Instant::now();
    {
        let view = host.shared.view.read().unwrap();
        assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Working);
    }
    assert!(start.elapsed() < Duration::from_millis(20));
    host.now.store(NOW + 46_000_000, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        host.shared
            .view
            .read()
            .unwrap()
            .operation
            .as_ref()
            .unwrap()
            .phase,
        Phase::Failed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn worker_clock_regression_and_invalid_store_nesting_fail_closed() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    host.now.store(NOW - 1, Ordering::SeqCst);
    assert_eq!(
        submit(&mut worker, 1, Command::ProbeNetwork {}).await,
        Err(ErrorCode::Unavailable)
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let mut config = host.config.clone();
    config.configuration_dir = config.state_dir.join("nested");
    assert!(
        Worker::open(
            config,
            operator::Shared::new(false),
            host.clock(),
            CLOCK,
            "two",
            Box::new(network::UnsupportedAdapter::default())
        )
        .is_err()
    );
}

#[tokio::test]
async fn one_lifetime_worker_lock_prevents_competing_maintenance_writers() {
    let host = Host::new(true);
    let (adapter, _) = counted();
    let _worker = host.worker("one", adapter);
    assert!(
        Worker::open(
            host.config.clone(),
            operator::Shared::new(true),
            host.clock(),
            CLOCK,
            "two",
            Box::new(network::UnsupportedAdapter::default())
        )
        .is_err()
    );
}

#[tokio::test]
async fn recent_id_retention_never_exhausts_routine_probes_and_generation_survives_restart() {
    let host = Host::new(true);
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    for n in 1..=130 {
        assert_eq!(
            submit(&mut worker, n, Command::ProbeNetwork {}).await,
            Ok(())
        );
    }
    assert_eq!(count.load(Ordering::SeqCst), 130);
    let path = host.config.state_dir.join("operations.json");
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(journal["admission_generation"], 130);
    assert_eq!(
        journal["used_ids"].as_array().unwrap().len(),
        MAX_OPERATIONS
    );
    assert_eq!(
        submit(&mut worker, 130, Command::ProbeNetwork {}).await,
        Err(ErrorCode::CheckStatus)
    );
    drop(worker);
    let (adapter, _) = counted();
    let mut worker = host.worker("two", adapter);
    assert_eq!(
        submit(&mut worker, 131, Command::ProbeNetwork {}).await,
        Ok(())
    );
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(journal["admission_generation"], 131);
}

#[tokio::test]
async fn idle_read_only_refresh_observes_later_ready_and_withdraws_failed_proof_without_rejoining()
{
    let host = Host::new(true);
    let listener = host.bind();
    let calls = Arc::new(AtomicUsize::new(0));
    let owner_calls = calls.clone();
    let served = tokio::spawn(server(
        listener,
        2,
        Arc::new(move |request| {
            assert!(matches!(request.command, owner::Command::Probe));
            if owner_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                owner::Reply::Probe(owner::Probe {
                    unix_seconds: 0,
                    time_synchronized: false,
                    observed_us: NOW,
                    runtime: setup::RuntimeState::Preparing,
                    startup: vec![],
                })
            } else {
                probe(false, NOW)
            }
        }),
    ));
    let mut worker = host.worker(
        "one",
        scripted(vec![
            (network::CommandKind::JoinWifi, Ok(NetworkResult::Accepted)),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(network())),
            ),
            (network::CommandKind::Commit, Ok(NetworkResult::Committed)),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(network())),
            ),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(network())),
            ),
        ]),
    );
    submit(
        &mut worker,
        1,
        Command::JoinWifi {
            ssid: "fixture".into(),
            password: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        host.shared
            .view
            .read()
            .unwrap()
            .setup
            .as_ref()
            .unwrap()
            .phase,
        setup::Phase::PreparingRuntime
    );
    worker.refresh_idle().await.unwrap();
    served.await.unwrap();
    assert!(
        host.shared
            .view
            .read()
            .unwrap()
            .setup
            .as_ref()
            .unwrap()
            .ready()
    );
    worker.refresh_idle().await.unwrap();
    let view = host.shared.view.read().unwrap();
    let progress = view.setup.as_ref().unwrap();
    assert!(!progress.ready());
    assert!(progress.network_fresh);
    assert!(progress.network.as_ref().unwrap().lan_ready());
    assert_eq!(view.runtime, Some(setup::RuntimeState::Unknown));
    assert!(view.voice_capability.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn reset_actions_have_distinct_stable_rpc_ids_and_durable_intent_before_each_owner_effect() {
    let host = Host::new(true);
    let listener = host.bind();
    let state = host.config.state_dir.clone();
    let seen = Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let owner_seen = seen.clone();
    let served = tokio::spawn(server(
        listener,
        4,
        Arc::new(move |request| {
            let owner::Command::Reset(action) = request.command else {
                panic!("wrong command")
            };
            assert_eq!(request.id, action.request_id);
            assert!(owner_seen.lock().unwrap().insert(request.id));
            let disk: reset::State =
                serde_json::from_slice(&fs::read(state.join("reset.json")).unwrap()).unwrap();
            assert_eq!(disk.operation.unwrap().pending.unwrap(), action);
            let journal: serde_json::Value =
                serde_json::from_slice(&fs::read(state.join("operations.json")).unwrap()).unwrap();
            assert_eq!(journal["operation"]["phase"], "working");
            owner::Reply::Reset(reset::Receipt {
                operation_id: action.operation_id,
                coordinator_session: action.coordinator_session,
                clock_id: action.clock_id,
                request_id: action.request_id,
                owner_session: format!("{:?}-owner", action.store),
                observed_us: NOW,
                outcome: match action.kind {
                    reset::Kind::Prepare => reset::Outcome::Prepared {
                        writes_frozen: true,
                        durable_barrier: true,
                        voice_quiet_and_closed: true,
                    },
                    reset::Kind::Empty => reset::Outcome::Empty {
                        read_back_empty: true,
                        writes_frozen: true,
                    },
                },
            })
        }),
    ));
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    submit(
        &mut worker,
        1,
        Command::Reset {
            confirmation: RESET_CONFIRMATION.into(),
        },
    )
    .await
    .unwrap();
    served.await.unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.reset_phase, Some(reset::Phase::Complete));
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Complete);
    assert_eq!(seen.lock().unwrap().len(), 4);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn signed_release_staging_requires_fresh_owner_utc_and_apply_failure_never_switches_locally()
{
    let fixture = support::Fixture::new();
    let manifest = fixture.manifest(1);
    fixture.publish(&manifest, 1, &support::key(1), support::FUTURE);
    let mut host = Host::new(true);
    host.config.release = Some(fixture.config.clone());
    host.config.selection_dir = Some(host.config.state_dir.parent().unwrap().join("selection"));
    let listener = host.bind();
    let calls = Arc::new(AtomicUsize::new(0));
    let owner_calls = calls.clone();
    let served = tokio::spawn(server(
        listener,
        2,
        Arc::new(move |request| {
            assert!(matches!(request.command, owner::Command::Probe));
            probe(owner_calls.fetch_add(1, Ordering::SeqCst) == 1, NOW)
        }),
    ));
    let (adapter, count) = counted();
    let mut worker = host.worker("one", adapter);
    submit(
        &mut worker,
        1,
        Command::StageRelease {
            manifest_target: "release.json".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        host.shared
            .view
            .read()
            .unwrap()
            .operation
            .as_ref()
            .unwrap()
            .error,
        Some(ErrorCode::Verification)
    );
    assert!(!fixture.config.state_dir.join("initialized").exists());
    submit(
        &mut worker,
        2,
        Command::StageRelease {
            manifest_target: "release.json".into(),
        },
    )
    .await
    .unwrap();
    served.await.unwrap();
    let set = host
        .shared
        .view
        .read()
        .unwrap()
        .staged_release
        .clone()
        .unwrap();
    let verified = lamp_surround::release::recover_verified(&fixture.config, &set).unwrap();
    assert_eq!(verified.manifest(), &manifest);
    submit(
        &mut worker,
        3,
        Command::ApplyRelease {
            set_sha256: set,
            rollback: false,
        },
    )
    .await
    .unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Failed);
    assert!(matches!(
        view.update_outcome,
        Some(update::Outcome::RecoveryFailed { .. })
    ));
    let selector = lamp_surround::selection::Selector::open(
        fixture.config.clone(),
        host.config.selection_dir.clone().unwrap(),
        false,
    )
    .unwrap();
    assert!(selector.current().unwrap().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn uncertain_wifi_commit_is_check_status_and_only_reprobes_selected_lan() {
    let host = Host::new(true);
    let mut worker = host.worker(
        "one",
        scripted(vec![
            (network::CommandKind::JoinWifi, Ok(NetworkResult::Accepted)),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(network())),
            ),
            (
                network::CommandKind::Commit,
                Err(network::NetworkError::CommitUncertain),
            ),
            (
                network::CommandKind::Probe,
                Ok(NetworkResult::Observation(network())),
            ),
        ]),
    );
    submit(
        &mut worker,
        1,
        Command::JoinWifi {
            ssid: "fixture".into(),
            password: None,
        },
    )
    .await
    .unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Failed);
    assert_eq!(
        view.operation.as_ref().unwrap().error,
        Some(ErrorCode::CheckStatus)
    );
    let progress = view.setup.as_ref().unwrap();
    assert!(progress.network_fresh);
    assert!(progress.network.as_ref().unwrap().lan_ready());
    assert!(!progress.ready());
}

#[tokio::test]
async fn admitted_lifecycle_work_immediately_withdraws_listening_at_an_unchanged_clock() {
    let host = Host::new(true);
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    submit(&mut worker, 1, provider()).await.unwrap();
    let target = host
        .shared
        .view
        .read()
        .unwrap()
        .candidate_configuration
        .clone()
        .unwrap();
    {
        let mut view = host.shared.view.write().unwrap();
        view.runtime = Some(setup::RuntimeState::Ready);
        view.runtime_observed_us = Some(NOW);
        view.voice_capability = Some(startup::Capability::Listening);
    }
    let listener = host.bind();
    let (sent, received) = oneshot::channel();
    let served = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let request: owner::RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(request.command, owner::Command::Configure { .. }));
        sent.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    let (admitted, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        worker
            .run_one(Submission {
                id: id(2),
                command: Command::ApplyConfiguration { target },
                admitted,
            })
            .await
    });
    assert_eq!(rx.await.unwrap(), Ok(()));
    tokio::time::timeout(Duration::from_secs(1), received)
        .await
        .unwrap()
        .unwrap();
    let view = host.shared.view.read().unwrap().at(NOW);
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Working);
    assert_eq!(view.runtime, Some(setup::RuntimeState::Unknown));
    assert!(view.voice_capability.is_none());
    assert!(!view.setup.as_ref().unwrap().runtime_fresh);
    assert_eq!(host.now.load(Ordering::SeqCst), NOW);
    task.abort();
    served.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(served.await.unwrap_err().is_cancelled());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(host.config.state_dir.join("operations.json")).unwrap())
            .unwrap();
    assert_eq!(journal["operation"]["phase"], "working");
}

struct IdlePendingAdapter {
    calls: usize,
    completion: Option<network::Completion>,
    pending: Arc<tokio::sync::Notify>,
}
impl NetworkAdapter for IdlePendingAdapter {
    fn submit(&mut self, request: network::NetworkRequest) -> Result<(), network::NetworkError> {
        assert!(matches!(request.command, network::NetworkCommand::Probe));
        self.calls += 1;
        if self.calls == 1 {
            self.completion = Some(network::Completion {
                id: request.id,
                observed_us: NOW,
                result: Ok(NetworkResult::Observation(network())),
            });
        } else {
            self.pending.notify_one();
        }
        Ok(())
    }
    fn poll(&mut self) -> Option<network::Completion> {
        self.completion.take()
    }
}

#[tokio::test]
async fn stalled_idle_read_only_probe_is_preempted_for_durable_admission() {
    let host = Host::new(true);
    let pending = Arc::new(tokio::sync::Notify::new());
    let mut worker = host.worker(
        "one",
        Box::new(IdlePendingAdapter {
            calls: 0,
            completion: None,
            pending: pending.clone(),
        }),
    );
    submit(&mut worker, 1, Command::ProbeNetwork {})
        .await
        .unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(worker.run(rx));
    tokio::time::timeout(Duration::from_secs(1), pending.notified())
        .await
        .unwrap();
    let (admitted, rx) = oneshot::channel();
    let start = Instant::now();
    tx.send(Submission {
        id: id(2),
        command: provider(),
        admitted,
    })
    .await
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .unwrap()
            .unwrap(),
        Ok(())
    );
    let elapsed = start.elapsed();
    println!(
        "host fixture: channel send to durable admission during stalled idle probe = {} us",
        elapsed.as_micros()
    );
    drop(tx);
    task.await.unwrap().unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Complete);
    // The blocked idle probe has not completed its timeout/withdrawal path.
    // This proves preemption without treating host fsync latency as a device
    // guarantee or making parallel test-load timing a functional assertion.
    assert!(view.setup.as_ref().unwrap().network_fresh);
}

#[tokio::test]
async fn failed_manual_rollback_recovers_original_but_is_not_a_successful_requested_change() {
    let fixture = support::Fixture::new();
    fixture.publish(&fixture.manifest(1), 1, &support::key(1), support::FUTURE);
    let target = fixture.stage().await.unwrap();
    fixture.publish(&fixture.manifest(2), 2, &support::key(1), support::FUTURE);
    let original = fixture.stage().await.unwrap();
    let mut host = Host::new(true);
    host.config.release = Some(fixture.config.clone());
    let selection_dir = host.config.state_dir.parent().unwrap().join("selection");
    host.config.selection_dir = Some(selection_dir.clone());
    let selector = lamp_surround::selection::Selector::open(
        fixture.config.clone(),
        selection_dir.clone(),
        true,
    )
    .unwrap();
    selector.select(None, &original).unwrap();
    let listener = host.bind();
    let release_config = fixture.config.clone();
    let original_id = update::ReleaseId(original.set_sha256().into());
    let target_id = update::ReleaseId(target.set_sha256().into());
    let server_original = original_id.clone();
    let server_target = target_id.clone();
    let worker_state = host.config.state_dir.clone();
    let served = tokio::spawn(async move {
        let mut inventory = update::Inventory {
            current: Some(server_original),
            supervisor_session: "supervisor".into(),
            exclusive_token: "fence".into(),
            voice: None,
            body: None,
            eyes: None,
            absent_workers_gone: true,
            body_safe_when_absent: true,
        };
        let mut starts = 0;
        let mut failed = false;
        for _ in 0..32 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).await.unwrap();
            let request: owner::RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
            let owner::Command::Update(action) = &request.command else {
                panic!("unexpected owner request")
            };
            let state: update::State =
                serde_json::from_slice(&fs::read(worker_state.join("update.json")).unwrap())
                    .unwrap();
            assert_eq!(
                state.operation.as_ref().unwrap().pending.as_ref(),
                Some(action)
            );
            let done = action.kind == update::ActionKind::ReleaseOwnership;
            let evidence = match &action.kind {
                update::ActionKind::Inspect { .. } => update::Evidence::Inspected {
                    inventory: inventory.clone(),
                },
                update::ActionKind::SwitchSet { to } => {
                    let selector = lamp_surround::selection::Selector::open(
                        release_config.clone(),
                        selection_dir.clone(),
                        true,
                    )
                    .unwrap();
                    let current = selector.current().unwrap();
                    let candidate = lamp_surround::release::recover_verified(
                        &release_config,
                        &to.as_ref().unwrap().0,
                    )
                    .unwrap();
                    selector.select(current.as_ref(), &candidate).unwrap();
                    inventory.current = to.clone();
                    update::Evidence::Switched {
                        current: to.clone(),
                    }
                }
                update::ActionKind::StartVoice
                    if inventory.current.as_ref() == Some(&server_target) =>
                {
                    failed = true;
                    update::Evidence::Failed {
                        reason: "owner action failed".into(),
                    }
                }
                update::ActionKind::StartBody
                | update::ActionKind::StartEyes
                | update::ActionKind::StartVoice => {
                    starts += 1;
                    let (part, slot) = match action.kind {
                        update::ActionKind::StartBody => {
                            inventory.body_safe_when_absent = false;
                            (update::Part::Body, &mut inventory.body)
                        }
                        update::ActionKind::StartEyes => (update::Part::Eyes, &mut inventory.eyes),
                        _ => (update::Part::Voice, &mut inventory.voice),
                    };
                    let owner = update::Owner {
                        session: format!("owner-{starts}"),
                        release: inventory.current.clone().unwrap(),
                    };
                    *slot = Some(owner.clone());
                    update::Evidence::Started { part, owner }
                }
                update::ActionKind::RestBody => update::Evidence::BodyRested {
                    owner_session: inventory.body.as_ref().unwrap().session.clone(),
                    token: "rest".into(),
                    at_rest: true,
                    new_goals_blocked: true,
                },
                update::ActionKind::StopBody | update::ActionKind::StopEyes => {
                    let (part, slot) = if action.kind == update::ActionKind::StopBody {
                        inventory.body_safe_when_absent = true;
                        (update::Part::Body, &mut inventory.body)
                    } else {
                        (update::Part::Eyes, &mut inventory.eyes)
                    };
                    update::Evidence::Stopped {
                        part,
                        owner_session: slot.take().unwrap().session,
                        quiesce_token: action.guard.as_ref().unwrap().quiesce_token.clone(),
                        all_workers_gone: true,
                    }
                }
                update::ActionKind::VerifyReady => {
                    let capability = |owner: &Option<update::Owner>| update::Capability {
                        owner: owner.clone().unwrap(),
                        observed_us: NOW,
                        health: update::Health::Ready,
                    };
                    update::Evidence::Ready {
                        body: capability(&inventory.body),
                        eyes: capability(&inventory.eyes),
                        voice: capability(&inventory.voice),
                    }
                }
                update::ActionKind::ReleaseOwnership => update::Evidence::Released,
                other => panic!("unexpected action: {other:?}"),
            };
            let receipt = update::Receipt {
                operation_id: action.operation_id.clone(),
                step_id: action.step_id,
                clock_id: action.clock_id.clone(),
                coordinator_session: action.coordinator_session.clone(),
                observed_us: NOW,
                guard: action.guard.clone(),
                evidence,
            };
            let reply = owner::ReplyEnvelope {
                schema_version: 1,
                id: request.id,
                clock_id: request.clock_id,
                coordinator_session: request.coordinator_session,
                observed_us: NOW,
                reply: owner::Reply::Update(receipt),
            };
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).await.unwrap();
            stream.shutdown().await.unwrap();
            if done {
                assert!(failed);
                return;
            }
        }
        panic!("worker did not finish bounded rollback recovery");
    });
    let (adapter, _) = counted();
    let mut worker = host.worker("one", adapter);
    submit(
        &mut worker,
        1,
        Command::ApplyRelease {
            set_sha256: target_id.0,
            rollback: true,
        },
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), served)
        .await
        .unwrap()
        .unwrap();
    let view = host.shared.view.read().unwrap();
    assert_eq!(
        view.update_outcome,
        Some(update::Outcome::RolledBack {
            release: original_id
        })
    );
    assert_eq!(view.operation.as_ref().unwrap().phase, Phase::Failed);
    assert_eq!(
        view.operation.as_ref().unwrap().error,
        Some(ErrorCode::OwnerRefused)
    );
    assert_eq!(selector.current_release().unwrap(), Some(original));
}
