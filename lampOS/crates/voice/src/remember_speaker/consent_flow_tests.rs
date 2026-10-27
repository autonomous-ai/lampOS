//! Production consent admission and the actual scoped Unix request path.
//! Playback ownership itself is exercised by the coordinator's invitation tests.
use super::{
    Proof, consent, generation,
    tests::{TESTS, exchange_proof},
};
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    provider_worker::ProviderInput,
};
use serde_json::{Value, json};

const QUESTION_BOOT: [u8; 16] = [83; 16];
const QUESTION_REQUEST: u64 = 3_298_902;

fn key(request: u64) -> RequestKey {
    RequestKey {
        worker: lamp_turn_owner::BootId::new([78; 16]).unwrap(),
        session: 91,
        request,
    }
}
fn current(face: &str, words: &str, now: u64) -> Proof {
    Proof::heard(face.into(), now - 10_000, now - 1_000, words, generation()).unwrap()
}
fn offered(at: u64) -> consent::Candidate {
    consent::clear();
    let owner = RequestScope::new(key(3_298_900), ScopeSnapshot::room());
    let lease = owner.lease();
    let mut a = current("eyes/f1", "My name is Kesh.", at);
    let candidate = consent::heard(&mut a, key(3_298_900), at).unwrap();
    // The question capability survives normal A completion, not A's lease.
    drop(owner);
    assert!(!lease.is_current());
    candidate
}
fn bound(at: u64) -> consent::Candidate {
    let candidate = offered(at);
    assert!(consent::bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
    candidate
}
fn whole(at: u64) -> consent::Candidate {
    let candidate = bound(at - 1_000_000);
    consent::delivered(&candidate, QUESTION_BOOT, QUESTION_REQUEST, true, at);
    candidate
}
fn reply(now: u64, words: &str, face: &str, key: RequestKey) -> Proof {
    let mut b = current(face, words, now);
    let _ = consent::heard(&mut b, key, now);
    b
}
fn exchange(proof: Proof, key: RequestKey, args: Value) -> (Value, Vec<Value>) {
    exchange_proof(proof, key, args, true, json!({"remembered":"Kesh"}))
}
fn denied(proof: Proof, key: RequestKey) {
    let (answer, requests) = exchange(proof, key, json!({"name":"Kesh"}));
    assert!(requests.is_empty(), "unconsented request: {requests:?}");
    assert!(answer.get("error").is_some(), "{answer}");
    assert_ne!(answer["ok"], true);
}

#[test]
fn name_only_or_an_uninvited_yes_and_model_arguments_cannot_admit_a_face() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for words in ["My name is Kesh.", "Yes, my name is Kesh.", "Kesh."] {
        consent::clear();
        let now = lamp_ipc::monotonic_us();
        let b = reply(now, words, "eyes/f1", key(3_298_901));
        let (answer, requests) = exchange(
            b,
            key(3_298_901),
            json!({
                "name":"Kesh", "consent":true, "agreed":true,
                "face_consent":true, "setup_step":"done",
            }),
        );
        assert!(requests.is_empty(), "{words}: {requests:?}");
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("They have not agreed to saving their face")),
            "{words}: {answer}"
        );
    }
    consent::clear();
}

#[test]
fn the_completed_typed_face_question_admits_only_the_current_own_name_once() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let now = lamp_ipc::monotonic_us();
    let issued = now - 2_000_000;
    let delivered = now - 1_000_000;
    let candidate = offered(issued);
    let mut questions = consent::Questions::default();
    questions.offer(candidate.clone(), 17, issued);
    assert!(questions.ready(17, issued).is_none());
    questions.source_finished(candidate.request, true);
    assert_eq!(questions.ready(17, issued), Some(candidate.clone()));
    assert!(questions.bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
    assert!(consent::bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
    assert!(
        questions
            .finished([84; 16], QUESTION_REQUEST, true, delivered)
            .is_none()
    );
    let receipt = questions
        .finished(QUESTION_BOOT, QUESTION_REQUEST, true, delivered)
        .unwrap();
    let ProviderInput::FaceConsentDelivery {
        candidate: receipt_candidate,
        voice_boot,
        request,
        whole,
        delivered_us,
        privacy_generation,
        by_output,
        pending_only,
    } = receipt
    else {
        panic!("a typed question must issue its typed receipt")
    };
    assert_eq!(privacy_generation, 17);
    assert!(by_output.is_none());
    assert!(!pending_only);
    assert!(whole);
    consent::delivered(&receipt_candidate, voice_boot, request, whole, delivered_us);
    let b_key = key(3_298_901);
    let b = reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key);
    let again = b.clone();
    let (answer, requests) = exchange(b, b_key, json!({"name":"Kesh"}));
    assert_eq!(answer, json!({"ok":true,"remembered":"Kesh"}));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["remember_speaker_consented"]["name"], "Kesh");
    assert_eq!(requests[0]["remember_speaker_consented"]["face"], "eyes/f1");
    assert!(requests[0].get("remember").is_none());
    assert!(requests[0].get("remember_speaker").is_none());
    assert_eq!(
        requests[0]["remember_speaker_consented"]
            .as_object()
            .unwrap()
            .len(),
        6
    );
    denied(again, b_key);
    consent::clear();
}

#[test]
fn cut_unsaid_canceled_or_unbound_question_delivery_never_authorizes_a_save() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for outcome in [
        "cut",
        "unsaid",
        "cancel",
        "name-question",
        "generic-question",
        "wrong-boot",
        "wrong-turn",
    ] {
        let now = lamp_ipc::monotonic_us();
        let candidate = offered(now - 2_000_000);
        if !matches!(outcome, "name-question" | "generic-question") {
            assert!(consent::bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
        }
        match outcome {
            "cancel" => {
                consent::canceled(&candidate);
                // A late Whole callback cannot resurrect a canceled question.
                consent::delivered(
                    &candidate,
                    QUESTION_BOOT,
                    QUESTION_REQUEST,
                    true,
                    now - 1_000_000,
                );
            }
            "wrong-boot" => consent::delivered(
                &candidate,
                [84; 16],
                QUESTION_REQUEST,
                true,
                now - 1_000_000,
            ),
            "wrong-turn" => consent::delivered(
                &candidate,
                QUESTION_BOOT,
                QUESTION_REQUEST + 1,
                true,
                now - 1_000_000,
            ),
            _ => consent::delivered(
                &candidate,
                QUESTION_BOOT,
                QUESTION_REQUEST,
                !matches!(outcome, "cut" | "unsaid"),
                now - 1_000_000,
            ),
        }
        let b_key = key(3_298_901);
        denied(reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key), b_key);
    }
    consent::clear();
}

#[test]
fn an_invitation_is_bound_to_face_worker_session_and_a_later_request() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for changed in ["face", "eyes-boot", "worker", "session", "same-request"] {
        let now = lamp_ipc::monotonic_us();
        whole(now - 1_000_000);
        let mut b_key = key(3_298_901);
        let face = match changed {
            "face" => "eyes/f2",
            "eyes-boot" => "other/f1",
            _ => "eyes/f1",
        };
        match changed {
            "worker" => b_key.worker = lamp_turn_owner::BootId::new([79; 16]).unwrap(),
            "session" => b_key.session += 1,
            "same-request" => b_key.request = 3_298_900,
            _ => {}
        }
        denied(reply(now, "Yes, my name is Kesh.", face, b_key), b_key);
    }
    consent::clear();
}

#[test]
fn consent_expiry_earlier_speech_and_privacy_close_cannot_be_regranted_by_a_late_whole() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for changed in ["expiry", "earlier-speech", "privacy"] {
        let now = lamp_ipc::monotonic_us();
        let delivered = match changed {
            "expiry" => now - consent::WITHIN_US,
            "earlier-speech" => now - 5_000,
            // Keep B after an otherwise-valid question: only privacy closure
            // and the rejected late Whole callback deny this row.
            "privacy" => now - 1_000_000,
            _ => unreachable!(),
        };
        let candidate = whole(delivered);
        if changed == "privacy" {
            super::forget();
            consent::delivered(
                &candidate,
                QUESTION_BOOT,
                QUESTION_REQUEST,
                true,
                now - 1_000,
            );
        }
        let b_key = key(3_298_901);
        // In the earlier-speech row B began before the actual question ended.
        denied(reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key), b_key);
    }
    consent::clear();
}

#[test]
fn a_no_unrelated_reply_or_other_person_consumes_the_invitation_before_a_later_yes() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for (words, face) in [
        ("No, my name is Kesh.", "eyes/f1"),
        ("Yes, don't save my face. My name is Kesh.", "eyes/f1"),
        ("What time is it?", "eyes/f1"),
        ("Yes, my name is Kesh.", "eyes/f2"),
        ("Yes? My name is Kesh?", "eyes/f1"),
    ] {
        let now = lamp_ipc::monotonic_us();
        whole(now - 1_000_000);
        let first_key = key(3_298_901);
        denied(reply(now, words, face, first_key), first_key);
        let later_key = key(3_298_903);
        denied(
            reply(now, "Yes, my name is Kesh.", "eyes/f1", later_key),
            later_key,
        );
    }
    consent::clear();
}

#[test]
fn a_yes_without_its_own_name_never_borrows_the_invitations_name() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let now = lamp_ipc::monotonic_us();
    whole(now - 1_000_000);
    let b_key = key(3_298_901);
    let (answer, requests) = exchange(
        reply(now, "Yes.", "eyes/f1", b_key),
        b_key,
        json!({"name":"Kesh"}),
    );
    assert!(requests.is_empty());
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("You did not hear that name"))
    );
    consent::clear();
}

#[test]
fn reporting_setup_done_cannot_bypass_the_face_save_admission() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    consent::clear();
    let home = crate::process::SessionDirectory::create().unwrap();
    let file = home.path.join("onboarding.json");
    let (reported, handled) = crate::onboarding::run(
        &file,
        &lamp_gemini::ToolCall {
            id: "setup".into(),
            name: crate::onboarding::STEP.into(),
            args: json!({"step":"who_you_are","outcome":"done"}),
        },
    );
    assert!(handled);
    assert_eq!(reported["ok"], true);
    let now = lamp_ipc::monotonic_us();
    let b_key = key(3_298_901);
    denied(reply(now, "My name is Kesh.", "eyes/f1", b_key), b_key);
    consent::clear();
}

#[test]
fn reported_or_negated_face_request_suffixes_still_never_connect() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for words in [
        "My name is Kesh, she said remember my face.",
        "My name is Kesh and she said remember my face.",
        "My name is Kesh, don't remember my face.",
        "My name is Kesh, never save my face.",
        "My name is Kesh and do you remember my face?",
        "My name is Kesh, remember my name.",
    ] {
        consent::clear();
        let now = lamp_ipc::monotonic_us();
        let b_key = key(3_298_901);
        denied(reply(now, words, "eyes/f1", b_key), b_key);
    }
    consent::clear();
}

#[test]
fn a_proof_clone_cannot_lend_consent_to_a_different_worker_session_or_request() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for changed in ["worker", "session", "request"] {
        let now = lamp_ipc::monotonic_us();
        whole(now - 1_000_000);
        let b_key = key(3_298_901);
        let b = reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key);
        let mut other = b_key;
        match changed {
            "worker" => other.worker = lamp_turn_owner::BootId::new([79; 16]).unwrap(),
            "session" => other.session += 1,
            "request" => other.request += 1,
            _ => unreachable!(),
        }
        denied(b.clone(), other);
    }
    consent::clear();
}

#[test]
fn completed_consent_does_not_replace_the_current_proofs_freshness_or_cancellation() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for changed in ["canceled", "stale"] {
        let now = lamp_ipc::monotonic_us();
        whole(now - 1_000_000);
        let b_key = key(3_298_901);
        let mut b = reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key);
        if changed == "canceled" {
            b.cancel();
        } else {
            b.ended_us = now - super::FRESH_US - 1;
            b.began_us = b.ended_us - 1;
        }
        denied(b, b_key);
    }
    consent::clear();
}

#[test]
fn late_transcription_never_restamps_the_original_face_evidence_deadline() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    consent::clear();
    let now = lamp_ipc::monotonic_us();
    let mut a = current("eyes/f1", "My name is Kesh.", now - 5_000_000);
    let original_end = a.ended_us;
    let candidate = consent::heard(&mut a, key(3_298_900), now).unwrap();
    assert_eq!(candidate.issued_us, original_end);
    assert!(candidate.fresh(original_end + super::FRESH_US - 1));
    assert!(!candidate.fresh(original_end + super::FRESH_US));
    assert!(!candidate.fresh(original_end - 1));
    consent::clear();
}

#[test]
fn delayed_interposed_output_cancels_minted_b_but_bs_own_output_does_not() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    for changed in ["own", "request", "worker", "session"] {
        let unrelated = changed != "own";
        let now = lamp_ipc::monotonic_us();
        let candidate = whole(now - 1_000_000);
        let b_key = key(3_298_901);
        let b = reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key);
        // Simulate the cancellation metadata arriving after the worker has
        // already admitted B's raw line. Only an unrelated D revokes it.
        let mut output = consent::OutputKey::from_key(b_key);
        match changed {
            "request" => output.request += 1,
            "worker" => output.worker = [79; 16],
            "session" => output.session += 1,
            "own" => {}
            _ => unreachable!(),
        }
        consent::delivered_from(
            &candidate,
            QUESTION_BOOT,
            QUESTION_REQUEST,
            false,
            now,
            Some(output),
        );
        let (answer, requests) = exchange(b, b_key, json!({"name":"Kesh"}));
        if unrelated {
            assert!(requests.is_empty());
            assert!(answer.get("error").is_some());
        } else {
            assert_eq!(requests.len(), 1);
            assert_eq!(answer, json!({"ok":true,"remembered":"Kesh"}));
        }
    }
    // The tool may already have taken the one-shot token. Cancellation
    // must still revoke the proof checked immediately before/during I/O.
    let now = lamp_ipc::monotonic_us();
    let candidate = whole(now - 1_000_000);
    let b_key = key(3_298_901);
    let b = reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key);
    assert!(consent::take(&b, b_key));
    assert!(b.is_current(lamp_ipc::monotonic_us()));
    consent::delivered_from(
        &candidate,
        QUESTION_BOOT,
        QUESTION_REQUEST,
        false,
        now,
        Some(consent::OutputKey::from_key(key(b_key.request + 1))),
    );
    assert!(!b.is_current(lamp_ipc::monotonic_us()));
    denied(b, b_key);
    consent::clear();
}

#[test]
fn a_name_first_offers_one_question_but_explicit_requests_and_admitted_b_never_offer_another() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    consent::clear();
    let now = lamp_ipc::monotonic_us();
    let mut a = current("eyes/f1", "My name is Kesh.", now);
    assert!(consent::heard(&mut a, key(3_298_900), now).is_some());
    for words in [
        "Please remember my face. My name is Kesh.",
        "My name is Kesh, remember my face.",
    ] {
        consent::clear();
        let now = lamp_ipc::monotonic_us();
        let mut direct = current("eyes/f1", words, now);
        assert!(
            consent::heard(&mut direct, key(3_298_900), now).is_none(),
            "{words}"
        );
    }
    let now = lamp_ipc::monotonic_us();
    whole(now - 1_000_000);
    let b_key = key(3_298_901);
    let mut b = current("eyes/f1", "Yes, my name is Kesh.", now);
    assert!(
        consent::heard(&mut b, b_key, now).is_none(),
        "admitted B must not queue C2"
    );
    assert!(consent::take(&b, b_key));
    assert!(!consent::take(&b, b_key));
    consent::clear();
}

#[test]
fn question_expiry_preserves_consent_already_bound_to_the_timely_current_reply() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let now = lamp_ipc::monotonic_us();
    let candidate = offered(now - 15_000_000);
    let mut questions = consent::Questions::default();
    questions.offer(candidate.clone(), 17, now - 15_000_000);
    questions.source_finished(candidate.request, true);
    assert!(questions.bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
    assert!(consent::bind(&candidate, QUESTION_BOOT, QUESTION_REQUEST));
    let delivered = now - consent::WITHIN_US - 1_000_000;
    let whole = questions
        .finished(QUESTION_BOOT, QUESTION_REQUEST, true, delivered)
        .unwrap();
    let ProviderInput::FaceConsentDelivery {
        candidate,
        voice_boot,
        request,
        whole,
        delivered_us,
        ..
    } = whole
    else {
        unreachable!()
    };
    consent::delivered(&candidate, voice_boot, request, whole, delivered_us);
    // B really was admitted inside the original reply window; its own
    // evidence remains fresh at expiry. No original timestamp is restamped.
    let b_at = delivered + 1_000_000;
    let b_key = key(3_298_901);
    let mut b = current("eyes/f1", "Yes, my name is Kesh.", b_at);
    assert!(consent::heard(&mut b, b_key, b_at).is_none());
    assert!(b.is_current(now));
    let expired = questions.unsettled(None, 17, now).unwrap();
    let ProviderInput::FaceConsentDelivery {
        candidate,
        voice_boot,
        request,
        whole,
        pending_only,
        by_output,
        ..
    } = expired
    else {
        unreachable!()
    };
    assert!(!whole);
    assert!(pending_only);
    assert!(by_output.is_none());
    consent::expired(&candidate, voice_boot, request);
    assert!(b.is_current(lamp_ipc::monotonic_us()));
    let (answer, requests) = exchange(b, b_key, json!({"name":"Kesh"}));
    assert_eq!(answer, json!({"ok":true,"remembered":"Kesh"}));
    assert_eq!(requests.len(), 1);
    consent::clear();
}

/// Since #3739 the eyes refuse a consented face near someone kept under
/// another name (`near_to`). That call used up the one-shot consent, so the
/// model asks for the explicit request with the name spelled in the same
/// sentence, which carries its own consent; a spelling alone sends nothing.
#[test]
fn a_near_face_refusal_asks_for_a_spelled_face_request_and_a_spelling_alone_sends_nothing() {
    let _serial = TESTS.lock().unwrap_or_else(|p| p.into_inner());
    let now = lamp_ipc::monotonic_us();
    whole(now - 1_000_000);
    let b_key = key(3_298_901);
    let (answer, requests) = exchange_proof(
        reply(now, "Yes, my name is Kesh.", "eyes/f1", b_key),
        b_key,
        json!({"name":"Kesh"}),
        true,
        json!({"error":"near_to","near_to":"Ossa"}),
    );
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["remember_speaker_consented"]["apart"], false);
    let told = answer["error"].as_str().unwrap();
    assert!(
        told.contains("Remember my face, my name is") && told.contains("spell"),
        "{told}"
    );
    assert!(!told.contains("Ossa"), "{told}");
    let c_key = key(3_298_902);
    let (answer, requests) = exchange(
        reply(now, "K E S H.", "eyes/f1", c_key),
        c_key,
        json!({"name":"Kesh","apart":true}),
    );
    assert!(requests.is_empty(), "{requests:?}");
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("They have not agreed to saving their face")),
        "{answer}"
    );
    let d_key = key(3_298_903);
    let (answer, requests) = exchange(
        reply(
            now,
            "Remember my face, my name is Kesh, K E S H.",
            "eyes/f1",
            d_key,
        ),
        d_key,
        json!({"name":"Kesh","apart":true}),
    );
    assert_eq!(answer, json!({"ok":true,"remembered":"Kesh"}));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["remember_speaker_consented"]["apart"], true);
    consent::clear();
}
