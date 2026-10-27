//! Real credited IPC: private authority must survive dequeue and every retry.
use super::*;
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    second_provider::{Listener, PrivateContext, Services, net},
};
use futures_util::{
    StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller, Permission};

struct Quiet;
impl Services for Quiet {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        panic!("no capture in the IPC fixture")
    }
    fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
        stream::empty().boxed()
    }
    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        panic!("no synthesis in the IPC fixture")
    }
}

#[tokio::test]
async fn private_output_credit_stall_rechecks_proof_without_changing_microphone_owner() {
    const NOW: u64 = 10_000_000;
    const SPEAKER: &str = "0123456789abcdef/f7";
    for case in ["expired-private", "positive-private", "public-room"] {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let boot = BootId::new([37; 16]).unwrap();
        let worker_boot = BootId::new([38; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, true).unwrap();
        parent.connect(&directory.path, "second", false).unwrap();
        worker.connect(&directory.path, "second", true).unwrap();
        let at = MonoTime::from_micros(NOW);
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_camera_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(NOW + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let request = owner.admit(at, AdmittedInput::NewTurn).unwrap().turn();
        let before = owner.snapshot(at).unwrap();
        let mut engine = Engine::new(
            Arc::new(Quiet),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no tools in the IPC fixture")),
        )
        .guarding_private_context(Arc::new(|lease| (lease.speaker() == Some(SPEAKER)).into()));
        let mut scope = RequestScope::new(
            RequestKey {
                worker: worker_boot,
                session: 1,
                request,
            },
            if case == "public-room" {
                ScopeSnapshot::room()
            } else {
                ScopeSnapshot::of("Fixture owner")
            },
        );
        // Exercise the real request-owned proof deadline. Only the captured
        // source identity is supplied by the fixture, not the expiry predicate.
        let deadline = Instant::now()
            + if case == "expired-private" {
                Duration::from_millis(250)
            } else {
                Duration::from_secs(5)
            };
        if case != "public-room" {
            let key = scope.lease().key();
            assert!(
                scope
                    .speaker_recorder()
                    .unwrap()
                    .seal(key, &[SPEAKER.into()], deadline)
            );
            assert_eq!(scope.lease().speaker(), Some(SPEAKER));
        }
        let guard =
            PrivateContext::new(&engine.shared, Some(&scope.lease()), case != "public-room");
        let mut gate = Gate::new(boot, NOW);
        gate.install(NOW, before, &mut engine).unwrap();
        // Spend both initial credits through the actual worker/parent sockets.
        for _ in 0..2 {
            let public = ProviderOutput::GenerationComplete { request }.into();
            assert!(
                gate.send_or_discard(&engine, NOW, &public, |event| worker
                    .data
                    .send_with(event, || NOW))
                    .unwrap()
            );
            assert!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| NOW)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(!gate.flow.can_send());
        assert!(
            engine
                .shared
                .emit_private(
                    &guard,
                    ProviderOutput::Transcript {
                        request: Some(request),
                        text: "fixture response".into(),
                        finished: false
                    }
                )
                .await
        );
        let pending = next_output(&mut engine).unwrap();
        let mut blocked_attempts = 0;
        assert!(
            !gate
                .send_or_discard(&engine, NOW, &pending, |event| {
                    blocked_attempts += 1;
                    worker.data.send_with(event, || NOW)
                })
                .unwrap()
        );
        assert_eq!(blocked_attempts, 0, "no credit means no IPC attempt");
        assert!(
            parent
                .data
                .receive_with::<ProviderOutput>(|| NOW)
                .unwrap()
                .is_none()
        );
        // The same pending entry crosses an actual scheduling boundary without
        // credit. The public control authority and microphone generation stay.
        tokio::task::yield_now().await;
        if case == "expired-private" {
            tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
            assert_eq!(scope.lease().speaker(), None);
        }
        let retry = NOW + 1_000;
        gate.fresh(retry).unwrap();
        gate.may_send(retry, &pending.event).unwrap();
        let after = gate.guard.state().unwrap();
        assert_eq!(before.owner(), after.owner());
        assert_eq!(
            before.microphone_generation(),
            after.microphone_generation()
        );
        assert!(scope.lease().is_current());
        gate.flow.grant(3).unwrap();
        let mut attempts = 0;
        assert!(
            gate.send_or_discard(&engine, retry, &pending, |event| {
                attempts += 1;
                worker.data.send_with(event, || retry)
            })
            .unwrap()
        );
        let delivered = case != "expired-private";
        assert_eq!(
            attempts,
            usize::from(delivered),
            "expired proof causes no IPC attempt"
        );
        assert_eq!(
            matches!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| retry)
                    .unwrap(),
                Some(ProviderOutput::Transcript { .. })
            ),
            delivered,
            "{case}"
        );
        assert_eq!(
            gate.flow.can_send(),
            !delivered,
            "discard spends no credit: {case}"
        );
        // A discarded private event cannot hold up the public terminal event.
        gate.flow.grant(4).unwrap();
        let done = ProviderOutput::TurnComplete {
            request,
            idle: true,
        }
        .into();
        assert!(
            gate.send_or_discard(&engine, retry, &done, |event| worker
                .data
                .send_with(event, || retry))
                .unwrap()
        );
        assert!(
            matches!(parent.data.receive_with::<ProviderOutput>(||retry).unwrap(),Some(ProviderOutput::TurnComplete {request:got,..}) if got==request)
        );
    }
}
