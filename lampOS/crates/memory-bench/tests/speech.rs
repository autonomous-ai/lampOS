use lamp_memory_bench::{
    hash,
    protocol::*,
    speech::{self, Audio, Transcript},
};
fn wave() -> Vec<u8> {
    let mut w = b"RIFF".to_vec();
    w.extend(40u32.to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend([1, 0, 1, 0]);
    w.extend(16000u32.to_le_bytes());
    w.extend(32000u32.to_le_bytes());
    w.extend([2, 0, 16, 0]);
    w.extend(b"data");
    w.extend(4u32.to_le_bytes());
    w.extend([0, 0, 1, 0]);
    w
}
#[test]
fn audio_is_bounded_mono_pcm_with_complete_riff_chunks() {
    let w = wave();
    assert_eq!(speech::pcm(&w).unwrap(), [0, 0, 1, 0]);
    let mut wrong = w.clone();
    wrong[22] = 2;
    assert!(speech::pcm(&wrong).is_err());
    let mut wrong = w.clone();
    wrong[24] = 1;
    assert!(speech::pcm(&wrong).is_err());
    assert!(speech::pcm(&w[..w.len() - 1]).is_err());
    let mut wrong = w;
    wrong[40] = 3;
    assert!(speech::pcm(&wrong).is_err());
}
fn fixture() -> (Event, Audio, Transcript) {
    let event = Event {
        record_id: "one".into(),
        at_unix_ms: 1,
        session_id: "s1".into(),
        kind: EventKind::Utterance,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "My notebook is blue.".into(),
        said: String::new(),
        stt_confidence: None,
        cues: Cues::default(),
    };
    let audio = Audio {
        record_id: "one".into(),
        script_hash: hash(event.heard.as_bytes()),
        wav_hash: hash(&wave()),
        pcm_hash: hash(&[0, 0, 1, 0]),
        file: "made-up.wav".into(),
        duration_ms: 1,
        voice: "fixture".into(),
        words_per_minute: 175,
    };
    let transcript = Transcript {
        record_id: "one".into(),
        audio_hash: audio.wav_hash.clone(),
        request_hash: hash(b"request"),
        response_hash: hash(b"response"),
        request_id: "recorded-test".into(),
        model_revision: None,
        text: "My note book is glue.".into(),
        confidence: None,
        mode: "recorded".into(),
    };
    (event, audio, transcript)
}
#[test]
fn frozen_observed_text_preserves_real_errors_and_unknown_confidence() {
    let (e, a, t) = fixture();
    let out = speech::observed(&[e], &[a], &[t], false).unwrap();
    assert_eq!(out[0].heard, "My note book is glue.");
    assert_eq!(out[0].stt_confidence, None);
}
#[test]
fn missing_or_changed_cache_never_falls_back_to_clean_text() {
    let (e, a, t) = fixture();
    assert!(
        speech::observed(
            std::slice::from_ref(&e),
            std::slice::from_ref(&a),
            &[],
            false
        )
        .is_err()
    );
    let mut changed = e.clone();
    changed.heard = "other script".into();
    assert!(
        speech::observed(
            &[changed],
            std::slice::from_ref(&a),
            std::slice::from_ref(&t),
            false
        )
        .is_err()
    );
    let mut bad = t.clone();
    bad.audio_hash = hash(b"other audio");
    assert!(speech::observed(&[e], &[a], &[bad], false).is_err());
}
#[test]
fn recorded_stt_cannot_qualify_a_live_result() {
    let (e, a, t) = fixture();
    assert!(
        speech::observed(
            std::slice::from_ref(&e),
            std::slice::from_ref(&a),
            std::slice::from_ref(&t),
            true
        )
        .is_err()
    );
    let mut bad = t;
    bad.confidence = Some(1.1);
    assert!(speech::observed(&[e], &[a], &[bad], false).is_err());
}
