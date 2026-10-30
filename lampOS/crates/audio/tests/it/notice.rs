#![cfg(unix)]
//! The failure-notice directory: what loads, what is refused and why. The
//! recordings written here are tones. They are test assets, not a voice.
use lamp_audio::notice::{
    Approval, MANIFEST, MAX_FRAMES, MAX_MANIFEST_BYTES, MAX_NOTICES, MIN_FRAMES, Missing,
    NoticeKind, Notices, RATE,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "lamp-notices-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
    fn provision(&self, notices: Vec<Value>) {
        let manifest = json!({"schema_version": 1, "notices": notices});
        self.write(MANIFEST, manifest.to_string().as_bytes());
    }
    fn load(&self, accept: Approval) -> Notices {
        Notices::load(&self.0, accept)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(Cursor::new(&mut bytes), spec).unwrap();
    for &sample in samples {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
    bytes
}
fn tone(frames: u32) -> Vec<i16> {
    (0..frames)
        .map(|index| ((f64::from(index) * 0.05).sin() * 8_000.0) as i16)
        .collect()
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn entry(kind: &str, file: &str, bytes: &[u8], frames: u32) -> Value {
    json!({
        "kind": kind,
        "language": "en-US",
        "transcript": "I can't reach my voice service right now.",
        "voice": "test-tone",
        "voice_revision": "none",
        "approval": "production",
        "file": file,
        "wav_sha256": sha(bytes),
        "frames": frames,
    })
}
/// One valid notice whose manifest entry is altered by `change`.
fn with(change: impl FnOnce(&mut Value), file_bytes: Option<Vec<u8>>) -> Result<(), Missing> {
    let directory = Directory::new();
    let frames = RATE * 3 / 2;
    let bytes = file_bytes.unwrap_or_else(|| wav(RATE, 1, &tone(frames)));
    directory.write("outage.wav", &bytes);
    let mut value = entry("provider_unavailable", "outage.wav", &bytes, frames);
    change(&mut value);
    directory.provision(vec![value]);
    // The fate of that one entry, under whatever language it now names.
    let capability = directory.load(Approval::Production).capability();
    match (capability.unavailable, capability.refused.first()) {
        (Some(reason), _) => Err(reason),
        (None, Some(refused)) => Err(refused.reason),
        (None, None) => {
            assert_eq!(capability.available.len(), 1);
            Ok(())
        }
    }
}

#[test]
fn a_provisioned_notice_is_loaded_exactly_as_recorded() {
    let directory = Directory::new();
    let samples = tone(RATE * 3 / 2);
    let bytes = wav(RATE, 1, &samples);
    directory.write("outage.wav", &bytes);
    directory.provision(vec![entry(
        "provider_unavailable",
        "outage.wav",
        &bytes,
        RATE * 3 / 2,
    )]);
    let notices = directory.load(Approval::Production);
    let notice = notices
        .get(NoticeKind::ProviderUnavailable, "en-US")
        .unwrap();
    assert_eq!(notice.samples(), samples);
    assert_eq!(notice.info.frames, 36_000);
    assert_eq!(notice.info.duration_ms, 1_500);
    assert_eq!(notice.info.wav_sha256, sha(&bytes));
    let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    assert_eq!(notice.info.pcm_sha256, sha(&pcm));
    assert_eq!(
        notice.info.transcript,
        "I can't reach my voice service right now."
    );
    assert_eq!(notice.info.approval, Approval::Production);
    // Exactly this kind in exactly this language, nothing nearby.
    assert_eq!(
        notices.get(NoticeKind::TurnLost, "en-US").err(),
        Some(Missing::NotProvisioned)
    );
    assert_eq!(
        notices.get(NoticeKind::ProviderUnavailable, "en").err(),
        Some(Missing::NotProvisioned)
    );
    let capability = notices.capability();
    assert_eq!(capability.available.len(), 1);
    assert!(capability.refused.is_empty() && capability.unavailable.is_none());
}

#[test]
fn without_a_usable_manifest_every_notice_is_unavailable_and_nothing_fails() {
    let any = |notices: &Notices| notices.get(NoticeKind::ProviderUnavailable, "en-US").err();
    assert_eq!(any(&Notices::none()), Some(Missing::NoDirectory));
    let directory = Directory::new();
    assert_eq!(
        any(&Notices::load(
            &directory.path().join("absent"),
            Approval::Production
        )),
        Some(Missing::NoDirectory)
    );
    assert_eq!(
        any(&directory.load(Approval::Production)),
        Some(Missing::NoManifest)
    );
    let frames = RATE;
    let bytes = wav(RATE, 1, &tone(frames));
    directory.write("a.wav", &bytes);
    let good = || entry("provider_unavailable", "a.wav", &bytes, frames);
    let invalid = |manifest: Vec<u8>| {
        directory.write(MANIFEST, &manifest);
        let notices = directory.load(Approval::Production);
        assert_eq!(any(&notices), Some(Missing::InvalidManifest));
        assert_eq!(
            notices.capability().unavailable,
            Some(Missing::InvalidManifest)
        );
    };
    invalid(b"not json".to_vec());
    invalid(
        json!({"schema_version": 2, "notices": [good()]})
            .to_string()
            .into(),
    );
    invalid(
        json!({"schema_version": 1, "notices": [good()], "extra": true})
            .to_string()
            .into(),
    );
    // The same kind and language twice.
    invalid(
        json!({"schema_version": 1, "notices": [good(), good()]})
            .to_string()
            .into(),
    );
    // More entries than the bound.
    let many: Vec<Value> = (0..=MAX_NOTICES)
        .map(|index| {
            let mut value = good();
            value["language"] = json!(format!("x-{index}"));
            value
        })
        .collect();
    invalid(
        json!({"schema_version": 1, "notices": many})
            .to_string()
            .into(),
    );
    // Larger than the bound, however well formed.
    let mut padded = json!({"schema_version": 1, "notices": [good()]}).to_string();
    padded.push_str(&" ".repeat(MAX_MANIFEST_BYTES as usize));
    invalid(padded.into());
    // A link is not followed, wherever it points.
    directory.provision(vec![good()]);
    assert!(
        directory
            .load(Approval::Production)
            .get(NoticeKind::ProviderUnavailable, "en-US")
            .is_ok()
    );
    fs::rename(
        directory.path().join(MANIFEST),
        directory.path().join("real.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink("real.json", directory.path().join(MANIFEST)).unwrap();
    assert_eq!(
        any(&directory.load(Approval::Production)),
        Some(Missing::InvalidManifest)
    );
}

#[test]
fn a_test_asset_is_never_spoken_by_a_production_runtime() {
    let directory = Directory::new();
    let frames = RATE;
    let bytes = wav(RATE, 1, &tone(frames));
    directory.write("a.wav", &bytes);
    let mut value = entry("turn_lost", "a.wav", &bytes, frames);
    value["approval"] = json!("test");
    directory.provision(vec![value]);
    let production = directory.load(Approval::Production);
    assert_eq!(
        production.get(NoticeKind::TurnLost, "en-US").err(),
        Some(Missing::NotApproved)
    );
    assert!(production.capability().available.is_empty());
    assert!(
        directory
            .load(Approval::Test)
            .get(NoticeKind::TurnLost, "en-US")
            .is_ok()
    );
}

#[test]
fn a_faulty_asset_refuses_only_its_own_notice() {
    let directory = Directory::new();
    let frames = RATE;
    let bytes = wav(RATE, 1, &tone(frames));
    directory.write("good.wav", &bytes);
    directory.write("changed.wav", &wav(RATE, 1, &tone(frames + 1)));
    directory.provision(vec![
        entry("provider_unavailable", "good.wav", &bytes, frames),
        // The hash is of another recording.
        entry("turn_lost", "changed.wav", &bytes, frames),
        entry("service_refused", "absent.wav", &bytes, frames),
    ]);
    let notices = directory.load(Approval::Production);
    assert!(
        notices
            .get(NoticeKind::ProviderUnavailable, "en-US")
            .is_ok()
    );
    assert_eq!(
        notices.get(NoticeKind::TurnLost, "en-US").err(),
        Some(Missing::AssetHashMismatch)
    );
    assert_eq!(
        notices.get(NoticeKind::ServiceRefused, "en-US").err(),
        Some(Missing::AssetMissing)
    );
    let capability = notices.capability();
    assert_eq!(
        (capability.available.len(), capability.refused.len()),
        (1, 2)
    );
}

#[test]
fn only_complete_mono_24khz_pcm16_of_the_stated_length_is_accepted() {
    let frames = RATE * 3 / 2;
    assert_eq!(with(|_| {}, None), Ok(()));
    for bytes in [
        wav(16_000, 1, &tone(frames)),
        wav(RATE, 2, &tone(frames * 2)),
        // One frame short of what the manifest states.
        wav(RATE, 1, &tone(frames - 1)),
        wav(RATE, 1, &vec![0; frames as usize]),
    ] {
        assert_eq!(with(|_| {}, Some(bytes)), Err(Missing::AssetFormat));
    }
    // Cut off mid-file: the container no longer matches its own length.
    let mut cut = wav(RATE, 1, &tone(frames));
    cut.truncate(cut.len() - 100);
    assert_eq!(with(|_| {}, Some(cut)), Err(Missing::AssetFormat));
    // A sentence, not a click and not a reply.
    for frames in [MIN_FRAMES - 1, MAX_FRAMES + 1] {
        let bytes = wav(RATE, 1, &tone(frames));
        assert_eq!(
            with(|value| value["frames"] = json!(frames), Some(bytes)),
            Err(Missing::InvalidEntry)
        );
    }
    // The no-Wi-Fi line with the reset pinhole, 13.4 s in the lamp's voice (#3743).
    for frames in [MIN_FRAMES, 321_000, MAX_FRAMES] {
        let bytes = wav(RATE, 1, &tone(frames));
        assert_eq!(
            with(|value| value["frames"] = json!(frames), Some(bytes)),
            Ok(())
        );
    }
}

#[test]
fn an_asset_reached_through_a_link_is_refused() {
    let directory = Directory::new();
    let frames = RATE;
    let bytes = wav(RATE, 1, &tone(frames));
    directory.write("real.wav", &bytes);
    std::os::unix::fs::symlink("real.wav", directory.path().join("linked.wav")).unwrap();
    directory.provision(vec![entry(
        "provider_unavailable",
        "linked.wav",
        &bytes,
        frames,
    )]);
    assert_eq!(
        directory
            .load(Approval::Production)
            .get(NoticeKind::ProviderUnavailable, "en-US")
            .err(),
        Some(Missing::AssetUnreadable)
    );
}

#[test]
fn an_entry_cannot_name_a_path_or_carry_malformed_text() {
    for file in [
        "../outage.wav",
        "sub/outage.wav",
        ".outage.wav",
        "outage.mp3",
        "",
    ] {
        assert_eq!(
            with(|value| value["file"] = json!(file), None),
            Err(Missing::InvalidEntry),
            "{file}"
        );
    }
    let cases: [(&str, Value); 7] = [
        ("transcript", json!("")),
        ("transcript", json!("line one\nline two")),
        ("transcript", json!("x".repeat(241))),
        ("voice", json!("")),
        ("voice_revision", json!("")),
        ("language", json!("en US")),
        ("wav_sha256", json!("A".repeat(64))),
    ];
    for (field, wrong) in cases {
        assert_eq!(
            with(|value| value[field] = wrong.clone(), None),
            Err(Missing::InvalidEntry),
            "{field} {wrong}"
        );
    }
    // An unknown kind or approval is a manifest that cannot be read at all.
    assert_eq!(
        with(|value| value["kind"] = json!("greeting"), None),
        Err(Missing::InvalidManifest)
    );
    assert_eq!(
        with(|value| value["approval"] = json!("draft"), None),
        Err(Missing::InvalidManifest)
    );
}

/// #1959: the Wi-Fi helper's one recorded line is a kind of its own, loaded
/// as any other and only by its own name.
#[test]
fn the_wifi_lost_line_is_a_kind_of_its_own() {
    let directory = Directory::new();
    let samples = tone(RATE * 4);
    let bytes = wav(RATE, 1, &samples);
    directory.write("wifi-lost.en-US.wav", &bytes);
    let mut value = entry("wifi_lost", "wifi-lost.en-US.wav", &bytes, RATE * 4);
    value["transcript"] =
        "I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again.".into();
    directory.provision(vec![value]);
    let notices = directory.load(Approval::Production);
    let notice = notices.get(NoticeKind::WifiLost, "en-US").unwrap();
    assert_eq!(notice.samples(), samples);
    assert_eq!(notice.info.kind, NoticeKind::WifiLost);
    assert_eq!(
        notices.get(NoticeKind::ProviderUnavailable, "en-US").err(),
        Some(Missing::NotProvisioned)
    );
}

/// #3743: the recovery hotspot's line is a kind of its own too, beside the
/// Wi-Fi helper's, each loaded only by its own name.
#[test]
fn the_wifi_recovering_line_is_a_kind_of_its_own() {
    let directory = Directory::new();
    let lost = tone(RATE * 4);
    let lost_bytes = wav(RATE, 1, &lost);
    directory.write("wifi-lost.en-US.wav", &lost_bytes);
    let recovering: Vec<i16> = lost.iter().map(|sample| sample / 2).collect();
    let recovering_bytes = wav(RATE, 1, &recovering);
    directory.write("wifi-recovering.en-US.wav", &recovering_bytes);
    let mut value = entry(
        "wifi_recovering",
        "wifi-recovering.en-US.wav",
        &recovering_bytes,
        RATE * 4,
    );
    value["transcript"] =
        "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app.".into();
    directory.provision(vec![
        entry("wifi_lost", "wifi-lost.en-US.wav", &lost_bytes, RATE * 4),
        value,
    ]);
    let notices = directory.load(Approval::Production);
    let notice = notices.get(NoticeKind::WifiRecovering, "en-US").unwrap();
    assert_eq!(notice.samples(), recovering);
    assert_eq!(notice.info.kind, NoticeKind::WifiRecovering);
    assert_eq!(
        notices
            .get(NoticeKind::WifiLost, "en-US")
            .unwrap()
            .samples(),
        lost
    );
}

/// #3748: the hold's sleep line is a kind of its own, loaded only by its
/// own name.
#[test]
fn the_going_to_sleep_line_is_a_kind_of_its_own() {
    let directory = Directory::new();
    let samples = tone(RATE * 3);
    let bytes = wav(RATE, 1, &samples);
    directory.write("going-to-sleep.en-US.wav", &bytes);
    let mut value = entry(
        "going_to_sleep",
        "going-to-sleep.en-US.wav",
        &bytes,
        RATE * 3,
    );
    value["transcript"] = "Going to sleep. Tap my head to wake me.".into();
    directory.provision(vec![value]);
    let notices = directory.load(Approval::Production);
    let notice = notices.get(NoticeKind::GoingToSleep, "en-US").unwrap();
    assert_eq!(notice.samples(), samples);
    assert_eq!(notice.info.kind, NoticeKind::GoingToSleep);
    assert_eq!(
        notices.get(NoticeKind::WifiLost, "en-US").err(),
        Some(Missing::NotProvisioned)
    );
}
