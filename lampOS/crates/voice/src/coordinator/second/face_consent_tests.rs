// Real route flush/service and worker channels, not only a routing predicate.
use super::*;
use crate::remember_speaker::{Proof, TEST_CONSENT, consent, generation};

fn face_question(current: &Reply) -> ProviderInput {
    consent::clear();
    let now = lamp_ipc::monotonic_us();
    let mut a = Proof::heard(
        "eyes/f1".into(),
        now - 2_000_000,
        now - 1_000_000,
        "My name is Kesh.",
        generation(),
    )
    .unwrap();
    let candidate = consent::heard(
        &mut a,
        crate::memory_scope::RequestKey {
            worker: BootId::new([78; 16]).unwrap(),
            session: 1,
            request: 8001,
        },
        now,
    )
    .unwrap();
    ProviderInput::FaceQuestion {
        request: current.owner.turn(),
        session: 1,
        privacy_generation: 1,
        candidate,
        voice_boot: current.owner.boot().into(),
    }
}
fn assert_only_question(peer: &mut WorkerChannels, request: u64) {
    assert!(matches!(peer.data.receive::<ProviderInput>().unwrap(),
        Some(ProviderInput::FaceQuestion{request:got,session:1,..}) if got==request));
    assert!(peer.data.receive::<ProviderInput>().unwrap().is_none());
}

#[test]
fn an_unsent_typed_face_question_goes_once_to_its_second_worker_even_when_gemini_is_ready() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    let mut bench = Bench::standing_in();
    let (_owner, mut current) = unsent_reply();
    current.cloud.movable = false;
    assert!(
        !current.cloud.same_session_only,
        "a face question is not a Gemini continuation"
    );
    let turn = current.owner.turn();
    let question = face_question(&current);
    let mut reply = Some(current);
    let mut queue = VecDeque::from([question]);
    let original_worker = bench.rig.provider.boot;
    bench.gemini_link(1, true);
    bench.service(&mut reply, &mut queue);
    bench.flush(&mut reply, &mut queue);
    assert_eq!(bench.rig.provider.boot, original_worker);
    assert!(bench.route().second_active);
    assert!(
        bench.route().hold.is_none(),
        "fixed TTS cannot enter the stand-in audio hold"
    );
    assert!(bench.route().replay.is_empty());
    assert!(bench.route().line.commands.is_empty());
    assert!(queue.is_empty());
    assert!(reply.as_ref().unwrap().cloud.start_submitted);
    assert_only_question(&mut bench.second, turn);
    assert!(
        bench
            .gemini
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    bench.flush(&mut reply, &mut queue);
    assert!(
        bench
            .second
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    consent::clear();
}

#[test]
fn a_returning_gemini_link_never_holds_moves_or_replays_a_second_workers_question() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    let mut bench = Bench::standing_in();
    let (_owner, mut current) = unsent_reply();
    current.cloud.movable = false;
    let turn = current.owner.turn();
    let question = face_question(&current);
    let mut reply = Some(current);
    let mut queue = VecDeque::from([question]);
    let original_worker = bench.rig.provider.boot;
    bench.flush(&mut reply, &mut queue);
    assert!(queue.is_empty());
    assert!(bench.route().hold.is_none());
    assert_only_question(&mut bench.second, turn);
    bench.gemini_link(1, true);
    bench.service(&mut reply, &mut queue);
    bench.flush(&mut reply, &mut queue);
    assert_eq!(bench.rig.provider.boot, original_worker);
    assert!(bench.route().second_active);
    assert!(bench.route().replay.is_empty());
    assert!(bench.route().line.commands.is_empty());
    assert!(
        bench
            .gemini
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    assert!(
        bench
            .second
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    consent::clear();
}

#[test]
fn owner_none_cancellation_after_return_to_gemini_reaches_only_the_original_second_worker() {
    let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
    let mut bench = Bench::standing_in();
    let origin = bench.rig.provider.boot;
    let (_owner, mut current) = unsent_reply();
    current.cloud.movable = false;
    let turn = current.owner.turn();
    let question_boot: [u8; 16] = current.owner.boot().into();
    consent::clear();
    let now = lamp_ipc::monotonic_us();
    let mut a = Proof::heard(
        "eyes/f1".into(),
        now - 2_000_000,
        now - 1_000_000,
        "My name is Kesh.",
        generation(),
    )
    .unwrap();
    let candidate = consent::heard(
        &mut a,
        crate::memory_scope::RequestKey {
            worker: origin,
            session: 1,
            request: 8001,
        },
        now,
    )
    .unwrap();
    let mut questions = consent::Questions::default();
    questions.offer(candidate.clone(), 1, now);
    questions.source_finished(candidate.request, true);
    assert!(questions.bind(&candidate, question_boot, turn));
    assert!(consent::bind(&candidate, question_boot, turn));
    let mut reply = Some(current);
    let mut queue = VecDeque::from([ProviderInput::FaceQuestion {
        request: turn,
        session: 1,
        privacy_generation: 1,
        candidate: candidate.clone(),
        voice_boot: question_boot,
    }]);
    bench.flush(&mut reply, &mut queue);
    assert_only_question(&mut bench.second, turn);
    // Model C's completed typed receipt here; real playback retirement and
    // owner completion are exercised in coordinator::face_consent tests.
    let whole = questions
        .finished(question_boot, turn, true, lamp_ipc::monotonic_us())
        .unwrap();
    let ProviderInput::FaceConsentDelivery { delivered_us, .. } = whole else {
        unreachable!()
    };
    consent::delivered(&candidate, question_boot, turn, true, delivered_us);
    reply = None;
    bench.gemini_link(1, true);
    bench.service(&mut reply, &mut queue);
    assert!(!bench.route().second_active);
    assert_ne!(bench.rig.provider.boot, origin);
    assert_eq!(bench.route().worker.boot, origin);
    queue.push_back(questions.cancel(lamp_ipc::monotonic_us()).unwrap());
    bench.flush(&mut reply, &mut queue);
    assert!(reply.is_none());
    assert!(queue.is_empty());
    assert!(
        bench
            .gemini
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    assert!(
        matches!(bench.second.data.receive::<ProviderInput>().unwrap(),
        Some(ProviderInput::FaceConsentDelivery { request, candidate, whole:false, .. })
            if request==turn && candidate.worker==<[u8;16]>::from(origin))
    );
    assert!(
        bench
            .second
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );
    assert!(bench.route().line.commands.is_empty());
    assert!(bench.route().replay.is_empty());
    consent::clear();
}
