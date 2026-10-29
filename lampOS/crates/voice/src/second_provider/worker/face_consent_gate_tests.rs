//! The real worker intake boundary, including normal C retirement and privacy.
use super::{Engine, Gate, ProviderInput, Told};
use crate::{
    memory_scope::RequestKey,
    remember_speaker::{Proof, TEST_CONSENT, consent, generation},
    second_provider::{Listener, Services, net},
};
use futures_util::{future::BoxFuture, stream::BoxStream};
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
};
use std::sync::Arc;

struct Silent;
impl Services for Silent {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        Box::pin(std::future::pending())
    }
    fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
        Box::pin(futures_util::stream::pending())
    }
    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        Box::pin(futures_util::stream::pending())
    }
}
fn key(request: u64) -> RequestKey {
    RequestKey {
        worker: BootId::new([78; 16]).unwrap(),
        session: 91,
        request,
    }
}
fn tick() -> u64 {
    lamp_ipc::monotonic_us()
}
fn install(owner: &mut Controller, gate: &mut Gate, engine: &mut Engine) {
    let now = tick();
    gate.install(
        now,
        owner.snapshot(MonoTime::from_micros(now)).unwrap(),
        engine,
    )
    .unwrap();
}
fn invitation() -> (Controller, Gate, Engine, ProviderInput) {
    consent::clear();
    let now = tick();
    let at = MonoTime::from_micros(now);
    let boot = BootId::new([87; 16]).unwrap();
    let mut owner = Controller::new(boot, at);
    owner
        .set_microphone_permission(at, Permission::Allowed)
        .unwrap();
    let until = MonoTime::from_micros(now + 500_000);
    owner
        .set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    let c = owner.admit(at, AdmittedInput::NewTurn).unwrap();
    owner.input_ended(at, c).unwrap();
    let mut gate = Gate::new(boot, now);
    let mut engine = Engine::new(
        Arc::new(Silent),
        Told::default,
        Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
    )
    .remembering_speaker(true, None);
    install(&mut owner, &mut gate, &mut engine);
    let mut a = Proof::heard(
        "eyes/f1".into(),
        now - 2_000_000,
        now - 1_000_000,
        "My name is Kesh.",
        generation(),
    )
    .unwrap();
    let candidate = consent::heard(&mut a, key(8001), now).unwrap();
    assert!(consent::bind(&candidate, c.boot().into(), c.turn()));
    let privacy = owner
        .snapshot(MonoTime::from_micros(tick()))
        .unwrap()
        .microphone_generation();
    owner
        .complete_turn(MonoTime::from_micros(tick()), c)
        .unwrap();
    install(&mut owner, &mut gate, &mut engine);
    assert!(gate.guard.state().unwrap().owner().is_none());
    assert!(engine.begun_last.is_none());
    let receipt = ProviderInput::FaceConsentDelivery {
        request: c.turn(),
        privacy_generation: privacy,
        candidate,
        voice_boot: c.boot().into(),
        delivered_us: tick() - 1_000,
        whole: true,
        by_output: None,
        pending_only: false,
    };
    (owner, gate, engine, receipt)
}
fn b_admitted() -> bool {
    let now = tick();
    let mut b = Proof::heard(
        "eyes/f1".into(),
        now - 10,
        now,
        "Yes, my name is Kesh.",
        generation(),
    )
    .unwrap();
    let _ = consent::heard(&mut b, key(8002), now);
    let admitted = consent::take(&b, key(8002));
    assert!(
        !consent::take(&b, key(8002)),
        "a worker receipt never supplies reusable input authority"
    );
    admitted
}

#[test]
fn a_current_epoch_whole_receipt_survives_c_retirement_without_granting_input() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    let (_, gate, mut engine, receipt) = invitation();
    let retired = gate.retired_through;
    assert!(gate.admits(tick(), &receipt).unwrap());
    engine.submit(receipt);
    assert_eq!(gate.retired_through, retired);
    assert!(gate.guard.state().unwrap().owner().is_none());
    assert!(engine.begun_last.is_none());
    assert!(b_admitted());
    consent::clear();
}

#[test]
fn closed_or_reopened_privacy_drops_old_metadata_instead_of_faulting_or_granting() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    for reopened in [false, true] {
        for whole in [false, true] {
            let (mut owner, mut gate, mut engine, mut receipt) = invitation();
            let ProviderInput::FaceConsentDelivery {
                whole: delivered, ..
            } = &mut receipt
            else {
                unreachable!()
            };
            *delivered = whole;
            owner
                .set_microphone_permission(MonoTime::from_micros(tick()), Permission::Denied)
                .unwrap();
            install(&mut owner, &mut gate, &mut engine);
            if reopened {
                let at = MonoTime::from_micros(tick());
                owner
                    .set_microphone_permission(at, Permission::Allowed)
                    .unwrap();
                let until = MonoTime::from_micros(tick() + 500_000);
                owner
                    .set_capture(at, CaptureState::RetainingUntil(until))
                    .unwrap();
                owner
                    .set_admission(at, AdmissionState::OpenUntil(until))
                    .unwrap();
                install(&mut owner, &mut gate, &mut engine);
            }
            assert!(
                !gate.admits(tick(), &receipt).unwrap(),
                "closed/reopened={reopened}, whole={whole}"
            );
            assert!(gate.guard.state().unwrap().owner().is_none());
            assert!(engine.begun_last.is_none());
            assert!(!b_admitted());
        }
    }
    consent::clear();
}

#[test]
fn current_metadata_still_needs_the_exact_bound_question_boot_and_turn() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    for wrong in ["boot", "turn"] {
        let (_, gate, mut engine, mut receipt) = invitation();
        let ProviderInput::FaceConsentDelivery {
            voice_boot,
            request,
            ..
        } = &mut receipt
        else {
            unreachable!()
        };
        if wrong == "boot" {
            *voice_boot = [88; 16];
        } else {
            *request += 1;
        }
        assert!(gate.admits(tick(), &receipt).unwrap());
        engine.submit(receipt);
        assert!(engine.begun_last.is_none());
        assert!(gate.guard.state().unwrap().owner().is_none());
        assert!(!b_admitted(), "{wrong}");
    }
    consent::clear();
}

#[test]
fn a_current_epoch_cancel_receipt_revokes_the_question_without_starting_input() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    let (_, gate, mut engine, mut receipt) = invitation();
    let ProviderInput::FaceConsentDelivery { whole, .. } = &mut receipt else {
        unreachable!()
    };
    *whole = false;
    assert!(gate.admits(tick(), &receipt).unwrap());
    engine.submit(receipt);
    assert!(gate.guard.state().unwrap().owner().is_none());
    assert!(engine.begun_last.is_none());
    assert!(!b_admitted());
    consent::clear();
}
