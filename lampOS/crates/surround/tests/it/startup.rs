use lamp_surround::startup::*;

fn spec(component: Component) -> ComponentConfig {
    ComponentConfig {
        component,
        configured: true,
        binary_available: true,
        calibration: Calibration::Present,
        missing_dependencies: vec![],
    }
}
fn monitor(parts: &[Component]) -> Monitor {
    Monitor::new(
        Config {
            enabled: true,
            components: parts.iter().copied().map(spec).collect(),
        },
        "boot",
        "clock",
        0,
    )
    .unwrap()
}
fn report(
    component: Component,
    generation: u64,
    session: &str,
    seq: u64,
    at: u64,
    capability: Capability,
) -> Receipt {
    Receipt {
        component,
        generation,
        owner_session: session.into(),
        boot_id: "boot".into(),
        clock_id: "clock".into(),
        sequence: seq,
        observed_us: at,
        capability,
    }
}
fn exit(component: Component, generation: u64, session: &str, at: u64) -> ExitProof {
    ExitProof {
        component,
        generation,
        owner_session: session.into(),
        boot_id: "boot".into(),
        clock_id: "clock".into(),
        observed_us: at,
        workers_gone: true,
        body_at_rest: true,
    }
}
fn quiet(generation: u64, session: &str, at: u64) -> QuietProof {
    QuietProof {
        generation,
        owner_session: session.into(),
        boot_id: "boot".into(),
        clock_id: "clock".into(),
        observed_us: at,
        observed_since_us: 0,
        last_heard_us: None,
        last_said_us: None,
        complete: true,
        admission_closed: true,
    }
}

#[test]
fn supervisor_is_off_unless_explicitly_enabled() {
    let config: Config =
        serde_json::from_value(serde_json::json!({"components":[spec(Component::Voice)]})).unwrap();
    assert!(!config.enabled);
    let mut m = Monitor::new(config, "boot", "clock", 0).unwrap();
    assert!(m.plan(0).unwrap().is_empty());
    assert_eq!(m.snapshot(0).unwrap()[0].phase, Phase::Disabled);
    assert_eq!(
        m.bind(Component::Voice, 1, "voice", 0),
        Err(Error::NotRequested)
    );
}

#[test]
fn body_eyes_voice_start_order_does_not_wait_for_optional_readiness_or_network() {
    let mut config = Config {
        enabled: true,
        components: vec![
            spec(Component::Memory),
            spec(Component::Voice),
            spec(Component::Body),
            spec(Component::Eyes),
        ],
    };
    config.components[1].missing_dependencies = vec![Dependency::Network, Dependency::Memory];
    let mut m = Monitor::new(config, "boot", "clock", 0).unwrap();
    assert_eq!(
        m.plan(0)
            .unwrap()
            .iter()
            .map(|a| a.component)
            .collect::<Vec<_>>(),
        [
            Component::Body,
            Component::Eyes,
            Component::Voice,
            Component::Memory
        ]
    );
    assert!(m.plan(1).unwrap().is_empty());
    m.bind(Component::Voice, 1, "voice", 1).unwrap();
    m.observe(
        1,
        report(Component::Voice, 1, "voice", 1, 1, Capability::Listening),
    )
    .unwrap();
    let status = m
        .snapshot(1)
        .unwrap()
        .into_iter()
        .find(|s| s.component == Component::Voice)
        .unwrap();
    assert!(status.healthy && status.listening);
    assert_eq!(
        status.missing_dependencies,
        [Dependency::Network, Dependency::Memory]
    );
}

#[test]
fn missing_binary_configuration_and_calibration_are_precise_and_local() {
    let mut a = spec(Component::Body);
    a.calibration = Calibration::Missing;
    let mut b = spec(Component::Eyes);
    b.binary_available = false;
    let mut c = spec(Component::Memory);
    c.configured = false;
    let mut m = Monitor::new(
        Config {
            enabled: true,
            components: vec![a, b, c, spec(Component::Voice)],
        },
        "boot",
        "clock",
        0,
    )
    .unwrap();
    assert_eq!(
        m.plan(0).unwrap(),
        [Launch {
            component: Component::Voice,
            generation: 1
        }]
    );
    let state = m.snapshot(0).unwrap();
    assert_eq!(state[0].phase, Phase::MissingCalibration);
    assert_eq!(state[1].phase, Phase::MissingBinary);
    assert_eq!(state[3].phase, Phase::MissingConfiguration);
}

#[test]
fn successful_exec_or_binding_alone_never_proves_readiness() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "voice", 1).unwrap();
    let s = &m.snapshot(1).unwrap()[0];
    assert_eq!(s.phase, Phase::Starting);
    assert!(!s.healthy && !s.listening);
    assert_eq!(
        m.snapshot(START_TIMEOUT_US).unwrap()[0].phase,
        Phase::StartTimedOut
    );
    assert!(m.plan(START_TIMEOUT_US).unwrap().is_empty());
}

#[test]
fn healthy_privacy_inhibit_is_never_listening_and_expires() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "voice", 0).unwrap();
    m.observe(
        1,
        report(Component::Voice, 1, "voice", 1, 1, Capability::Inhibited),
    )
    .unwrap();
    let s = &m.snapshot(1).unwrap()[0];
    assert!(s.healthy && !s.listening);
    assert_eq!(s.phase, Phase::Inhibited);
    let s = &m.snapshot(REPORT_MAX_AGE_US + 2).unwrap()[0];
    assert!(!s.healthy && !s.listening);
    assert_eq!(s.phase, Phase::Stale);
    assert!(m.plan(REPORT_MAX_AGE_US + 2).unwrap().is_empty());
}

#[test]
fn startup_and_binding_have_one_owner_and_reject_unsolicited_receipts() {
    let mut m = monitor(&[Component::Body]);
    assert!(
        !m.observe(0, report(Component::Body, 1, "a", 1, 0, Capability::Ready))
            .unwrap()
    );
    assert_eq!(m.bind(Component::Body, 1, "a", 0), Err(Error::NotRequested));
    m.plan(0).unwrap();
    m.bind(Component::Body, 1, "a", 0).unwrap();
    assert_eq!(m.bind(Component::Body, 1, "b", 0), Err(Error::AlreadyBound));
    assert!(
        !m.observe(1, report(Component::Body, 1, "b", 1, 1, Capability::Ready))
            .unwrap()
    );
    assert!(!m.snapshot(1).unwrap()[0].healthy);
}

#[test]
fn stale_future_reordered_and_other_clock_receipts_do_not_poison_sequence() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "v", 0).unwrap();
    let mut r = report(Component::Voice, 1, "v", 100, 1, Capability::Listening);
    assert!(!m.observe(0, r.clone()).unwrap());
    r.observed_us = 0;
    assert!(!m.observe(REPORT_MAX_AGE_US + 1, r.clone()).unwrap());
    r.observed_us = REPORT_MAX_AGE_US + 1;
    r.boot_id = "old".into();
    assert!(!m.observe(REPORT_MAX_AGE_US + 1, r.clone()).unwrap());
    r.boot_id = "boot".into();
    r.clock_id = "old".into();
    assert!(!m.observe(REPORT_MAX_AGE_US + 1, r).unwrap());
    let good = report(
        Component::Voice,
        1,
        "v",
        1,
        REPORT_MAX_AGE_US + 1,
        Capability::Listening,
    );
    assert!(m.observe(REPORT_MAX_AGE_US + 1, good.clone()).unwrap());
    assert!(!m.observe(REPORT_MAX_AGE_US + 1, good).unwrap());
    assert!(m.snapshot(REPORT_MAX_AGE_US + 1).unwrap()[0].listening);
    assert_eq!(m.snapshot(0), Err(Error::ClockReversed));
}

#[test]
fn late_actual_readiness_can_recover_a_timeout_without_a_second_writer() {
    let mut m = monitor(&[Component::Eyes]);
    m.plan(0).unwrap();
    m.bind(Component::Eyes, 1, "eyes", 0).unwrap();
    assert_eq!(
        m.snapshot(START_TIMEOUT_US).unwrap()[0].phase,
        Phase::StartTimedOut
    );
    assert!(
        m.observe(
            START_TIMEOUT_US + 1,
            report(
                Component::Eyes,
                1,
                "eyes",
                1,
                START_TIMEOUT_US + 1,
                Capability::Ready
            )
        )
        .unwrap()
    );
    assert!(m.snapshot(START_TIMEOUT_US + 1).unwrap()[0].healthy);
    assert!(m.plan(START_TIMEOUT_US + 1).unwrap().is_empty());
}

#[test]
fn missing_report_cannot_be_called_exit_or_trigger_a_replacement() {
    let mut m = monitor(&[Component::Body]);
    m.plan(0).unwrap();
    m.bind(Component::Body, 1, "body", 0).unwrap();
    assert_eq!(
        m.launch_failed(Component::Body, 1, 1),
        Err(Error::NotAbsent)
    );
    let mut proof = exit(Component::Body, 1, "body", 1);
    proof.workers_gone = false;
    assert_eq!(m.exited(1, proof), Err(Error::Stale));
    assert!(m.plan(100_000_000).unwrap().is_empty());
}

#[test]
fn verified_body_exit_without_rest_cannot_restart_but_voice_is_independent() {
    let mut m = monitor(&[Component::Body, Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Body, 1, "body", 0).unwrap();
    m.bind(Component::Voice, 1, "voice", 0).unwrap();
    let mut proof = exit(Component::Body, 1, "body", 1);
    proof.body_at_rest = false;
    m.exited(1, proof).unwrap();
    assert_eq!(
        m.snapshot(RETRY_BACKOFF_US + 1).unwrap()[0].phase,
        Phase::AwaitingRest
    );
    assert!(
        m.observe(
            RETRY_BACKOFF_US + 1,
            report(
                Component::Voice,
                1,
                "voice",
                1,
                RETRY_BACKOFF_US + 1,
                Capability::Listening
            )
        )
        .unwrap()
    );
    assert!(m.snapshot(RETRY_BACKOFF_US + 1).unwrap()[1].listening);
}

#[test]
fn confirmed_failed_launches_are_bounded_and_backed_off() {
    let mut m = monitor(&[Component::Memory]);
    for generation in 1..=u64::from(MAX_STARTS) {
        let now = (generation - 1) * RETRY_BACKOFF_US;
        assert_eq!(m.plan(now).unwrap()[0].generation, generation);
        m.launch_failed(Component::Memory, generation, now).unwrap();
        assert!(m.plan(now + RETRY_BACKOFF_US - 1).unwrap().is_empty());
    }
    assert_eq!(
        m.snapshot(u64::from(MAX_STARTS) * RETRY_BACKOFF_US)
            .unwrap()[0]
            .phase,
        Phase::Exhausted
    );
}

#[test]
fn retired_sessions_and_generations_cannot_revive_on_replacement() {
    let mut m = monitor(&[Component::Eyes]);
    m.plan(0).unwrap();
    m.bind(Component::Eyes, 1, "old", 0).unwrap();
    m.exited(1, exit(Component::Eyes, 1, "old", 1)).unwrap();
    m.plan(RETRY_BACKOFF_US + 1).unwrap();
    assert_eq!(
        m.bind(Component::Eyes, 2, "old", RETRY_BACKOFF_US + 1),
        Err(Error::AlreadyBound)
    );
    m.bind(Component::Eyes, 2, "new", RETRY_BACKOFF_US + 1)
        .unwrap();
    assert!(
        !m.observe(
            RETRY_BACKOFF_US + 1,
            report(
                Component::Eyes,
                1,
                "old",
                99,
                RETRY_BACKOFF_US + 1,
                Capability::Ready
            )
        )
        .unwrap()
    );
    assert!(!m.snapshot(RETRY_BACKOFF_US + 1).unwrap()[0].healthy);
}

#[test]
fn voice_restart_requires_complete_45_seconds_and_closed_admission_before_exit() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "v", 0).unwrap();
    let mut q = quiet(1, "v", VOICE_QUIET_US);
    q.complete = false;
    assert_eq!(
        m.authorize_voice_restart(VOICE_QUIET_US, q),
        Err(Error::Stale)
    );
    let mut q = quiet(1, "v", VOICE_QUIET_US);
    q.last_said_us = Some(1);
    assert_eq!(
        m.authorize_voice_restart(VOICE_QUIET_US, q),
        Err(Error::Invalid)
    );
    let mut q = quiet(1, "v", VOICE_QUIET_US);
    q.admission_closed = false;
    assert_eq!(
        m.authorize_voice_restart(VOICE_QUIET_US, q),
        Err(Error::Stale)
    );
    m.authorize_voice_restart(VOICE_QUIET_US, quiet(1, "v", VOICE_QUIET_US))
        .unwrap();
    assert!(m.plan(VOICE_QUIET_US).unwrap().is_empty());
    m.exited(
        VOICE_QUIET_US,
        exit(Component::Voice, 1, "v", VOICE_QUIET_US),
    )
    .unwrap();
    assert_eq!(
        m.plan(VOICE_QUIET_US + RETRY_BACKOFF_US).unwrap(),
        [Launch {
            component: Component::Voice,
            generation: 2
        }]
    );
}

#[test]
fn voice_exit_without_quiet_and_later_retired_owner_quiet_cannot_authorize_restart() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "v", 0).unwrap();
    m.exited(
        VOICE_QUIET_US,
        exit(Component::Voice, 1, "v", VOICE_QUIET_US),
    )
    .unwrap();
    assert_eq!(
        m.authorize_voice_restart(VOICE_QUIET_US, quiet(1, "v", VOICE_QUIET_US)),
        Err(Error::Stale)
    );
    assert!(
        m.plan(VOICE_QUIET_US + RETRY_BACKOFF_US)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        m.snapshot(VOICE_QUIET_US + RETRY_BACKOFF_US).unwrap()[0].phase,
        Phase::AwaitingQuiet
    );
}

#[test]
fn resumed_readiness_revokes_a_quiet_fence_and_missing_history_is_not_quiet() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "v", 0).unwrap();
    let mut q = quiet(1, "v", VOICE_QUIET_US);
    q.observed_since_us = 1;
    assert_eq!(
        m.authorize_voice_restart(VOICE_QUIET_US, q),
        Err(Error::Invalid)
    );
    m.authorize_voice_restart(VOICE_QUIET_US, quiet(1, "v", VOICE_QUIET_US))
        .unwrap();
    m.observe(
        VOICE_QUIET_US + 1,
        report(
            Component::Voice,
            1,
            "v",
            1,
            VOICE_QUIET_US + 1,
            Capability::Listening,
        ),
    )
    .unwrap();
    m.exited(
        VOICE_QUIET_US + 2,
        exit(Component::Voice, 1, "v", VOICE_QUIET_US + 2),
    )
    .unwrap();
    assert!(
        m.plan(VOICE_QUIET_US + RETRY_BACKOFF_US + 2)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn strict_public_wire_types_reject_unknown_fields_duplicates_and_fabricated_health() {
    let mut config = Config {
        enabled: true,
        components: vec![spec(Component::Voice), spec(Component::Voice)],
    };
    assert_eq!(config.validate(), Err(Error::Invalid));
    config.components.pop();
    let mut value = serde_json::to_value(config).unwrap();
    value["guessed_pin"] = 4.into();
    assert!(serde_json::from_value::<Config>(value).is_err());
    let r = report(Component::Eyes, 1, "e", 1, 0, Capability::Listening);
    assert_eq!(r.validate(), Err(Error::Invalid));
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    let mut s = m.snapshot(0).unwrap().pop().unwrap();
    s.healthy = true;
    assert_eq!(s.validate(), Err(Error::Invalid));
    s.healthy = false;
    s.validate().unwrap();
    let mut value = serde_json::to_value(r).unwrap();
    value["secret"] = "private".into();
    assert!(serde_json::from_value::<Receipt>(value).is_err());
}

#[test]
fn runtime_reports_exact_degradation_without_claiming_a_different_capability() {
    let mut m = monitor(&[Component::Voice]);
    m.plan(0).unwrap();
    m.bind(Component::Voice, 1, "v", 0).unwrap();
    m.observe(
        1,
        report(
            Component::Voice,
            1,
            "v",
            1,
            1,
            Capability::Unavailable {
                reason: Dependency::Provider,
            },
        ),
    )
    .unwrap();
    let s = &m.snapshot(1).unwrap()[0];
    s.validate().unwrap();
    assert!(!s.healthy && !s.listening);
    assert_eq!(s.missing_dependencies, [Dependency::Provider]);
    assert_eq!(s.phase, Phase::Unavailable);
}
