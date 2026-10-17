//! One local notice for a still-owned failed reply. No provider or device I/O.
use super::*;
use crate::options::NoticeOptions;
use lamp_audio::notice::{Approval, Missing, NoticeInfo, NoticeKind, Notices};
use lamp_audio::ownership::PlaybackCursor;
use std::sync::Arc;

/// Preloaded once, before any audio worker is started. The selected recording
/// is immutable and bounded by the notice loader; a turn holds only an Arc.
pub(super) struct FailureNotices {
    selected: std::result::Result<Arc<Recording>, Missing>,
    /// The internet cannot be reached (#830): said once an outage, by a
    /// local-only turn, with `LAMP_OFFLINE_VOICE=on`.
    offline: std::result::Result<Arc<Recording>, Missing>,
    /// It can again: said only if `offline` was.
    back_online: std::result::Result<Arc<Recording>, Missing>,
    /// The internet answers but the voice service does not, and nothing
    /// else can answer (#830, `LAMP_SERVICE_DOWN_LINE=on`): said once an
    /// outage, by a local-only turn, in place of `offline`.
    service_down: std::result::Result<Arc<Recording>, Missing>,
    language: Option<String>,
}
pub(super) struct Recording {
    /// The provisioned notice; none for a sound made here (the head tap's
    /// "hm?"), which says nothing.
    info: Option<NoticeInfo>,
    samples: Box<[i16]>,
}
#[cfg(test)]
impl Recording {
    pub(super) fn kind(&self) -> Option<NoticeKind> {
        self.info.as_ref().map(|info| info.kind)
    }
}
impl FailureNotices {
    pub(super) fn load(options: Option<&NoticeOptions>) -> Self {
        Self::load_with_approval(options, Approval::Production)
    }
    fn load_with_approval(options: Option<&NoticeOptions>, approval: Approval) -> Self {
        let Some(options) = options else {
            return Self::none();
        };
        let loaded = Notices::load(options.directory(), approval);
        let get = |kind| {
            loaded.get(kind, options.language()).map(|notice| {
                Arc::new(Recording {
                    info: Some(notice.info.clone()),
                    samples: notice.samples().into(),
                })
            })
        };
        Self {
            selected: get(NoticeKind::TurnLost),
            offline: get(NoticeKind::ProviderUnavailable),
            back_online: get(NoticeKind::BackOnline),
            service_down: get(NoticeKind::VoiceServiceUnreachable),
            language: Some(options.language().to_owned()),
        }
    }
    pub(super) fn none() -> Self {
        Self {
            selected: Err(Missing::NoDirectory),
            offline: Err(Missing::NoDirectory),
            back_online: Err(Missing::NoDirectory),
            service_down: Err(Missing::NoDirectory),
            language: None,
        }
    }
    pub(super) fn capability(&self) -> Value {
        let mut capability = match &self.selected {
            Ok(recording) => {
                json!({"configured":true,"language":self.language,"available":recording.info})
            }
            Err(reason) => {
                json!({"configured":self.language.is_some(),"language":self.language,"missing":reason})
            }
        };
        // The offline pair (#830), only where a directory is configured.
        if self.language.is_some() {
            let state = |selected: &std::result::Result<Arc<Recording>, Missing>| match selected {
                Ok(recording) => json!({"available":recording.info}),
                Err(reason) => json!({"missing":reason}),
            };
            capability["offline"] = state(&self.offline);
            capability["back_online"] = state(&self.back_online);
            capability["service_down"] = state(&self.service_down);
        }
        capability
    }
}

/// A turn taken while the lamp could not reach the internet (#830,
/// `LAMP_OFFLINE_VOICE=on`): admitted, listened to and ended as any other,
/// but its request never goes to the provider; it is failed locally from
/// the start. `speech`: what was heard was speech, not a sound let through
/// (a cough must not make the lamp apologise).
/// `back`: the turn the lamp takes for itself to say it is back online.
/// `service`: taken while the internet answers but the voice service does
/// not (`LAMP_SERVICE_DOWN_LINE=on`), so its line is the service's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocalOnly {
    pub(super) speech: bool,
    pub(super) back: bool,
    pub(super) service: bool,
}

/// What one outage has said: the offline line once, and "back online" only
/// after it.
#[derive(Debug, Default)]
pub(super) struct Outage {
    pub(super) said: bool,
    /// What was said was the service's line, not the internet's: no "back
    /// online" follows it, since the internet never left.
    pub(super) service: bool,
}

impl Outage {
    /// The offline line counts as said once the speaker has taken some of
    /// it (the turn's playback has begun) or all of it retired: not when it
    /// is merely chosen. A turn cancelled before that leaves the line owed
    /// (the reviewer on #884). Called every loop, before the turn can go.
    pub(super) fn note_delivery(&mut self, reply: &Option<Reply>) {
        let Some(current) = reply.as_ref() else {
            return;
        };
        let offline_line = current.local_only.filter(|local| !local.back);
        let delivered = match &current.notice {
            NoticeDelivery::Selected { retired_at_us, .. } => {
                current.playback.is_some() || retired_at_us.is_some()
            }
            _ => false,
        };
        if let Some(local) = offline_line.filter(|_| delivered) {
            self.said = true;
            self.service = local.service;
        }
    }
}

/// "I'm back online." is owed this long after the link returns, and then
/// let go: a lamp that has answered someone since, or could not speak for
/// a minute, has shown it is back without saying so.
pub(super) const BACK_ONLINE_OWED_US: u64 = 60_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SpeechOrigin {
    Provider,
    FailureNotice,
    /// The head tap's "hm?" (`crate::head_cue`, #711): local PCM, no answer.
    HeadCue,
    /// The same "hm?" for the lamp's own empty "Sorry?" (#1417): its own
    /// name in the trace, so it isn't counted as a tap's.
    UnclearHum,
    /// A skill's own sound (`crate::say`, #938): local PCM, no answer.
    SkillSound,
}
impl SpeechOrigin {
    /// Played from a recording held here, not from the provider.
    pub(super) fn local(self) -> bool {
        self != Self::Provider
    }
    /// A turn that is only a short sound of the lamp's own, and none for the
    /// face, the head, the room or the voice's state.
    pub(super) fn unshown(self) -> bool {
        matches!(self, Self::HeadCue | Self::UnclearHum | Self::SkillSound)
    }
    /// The trace's name for what plays.
    pub(super) fn trace_name(self) -> &'static str {
        match self {
            Self::Provider => "speaker",
            Self::FailureNotice => "failure_notice",
            Self::HeadCue => "head_tap_cue",
            Self::UnclearHum => "unclear_hum",
            Self::SkillSound => "skill_sound",
        }
    }
}

/// One attempt, retained independently of best-effort observation storage.
/// Selection happens only after prior provider PCM has actually retired.
#[derive(Default)]
pub(super) enum NoticeDelivery {
    #[default]
    Unselected,
    Skipped(Missing),
    Selected {
        /// `FailureNotice`, `HeadCue`, `UnclearHum` or `SkillSound`.
        origin: SpeechOrigin,
        recording: Arc<Recording>,
        cursor: usize,
        attempted_at_us: Option<u64>,
        retired_at_us: Option<u64>,
        final_acceptance: Option<(u64, PlaybackCursor)>,
        gaps: u64,
    },
}
impl NoticeDelivery {
    pub(super) fn origin(&self) -> SpeechOrigin {
        match self {
            Self::Selected { origin, .. } => *origin,
            _ => SpeechOrigin::Provider,
        }
    }
    /// A short sound of the lamp's own, made here, ready to play from its
    /// first sample.
    pub(super) fn sound(origin: SpeechOrigin, samples: Vec<i16>) -> Self {
        Self::Selected {
            origin,
            recording: Arc::new(Recording {
                info: None,
                samples: samples.into_boxed_slice(),
            }),
            cursor: 0,
            attempted_at_us: None,
            retired_at_us: None,
            final_acceptance: None,
            gaps: 0,
        }
    }
    /// Whether all of it was played: its final sample retired.
    pub(super) fn retired(&self) -> bool {
        matches!(
            self,
            Self::Selected {
                retired_at_us: Some(_),
                ..
            }
        )
    }
    pub(super) fn settled(&self) -> bool {
        !matches!(
            self,
            Self::Selected {
                retired_at_us: None,
                ..
            }
        )
    }
    pub(super) fn has_chunk(&self) -> bool {
        matches!(self, Self::Selected { recording, cursor, retired_at_us:None, .. } if *cursor < recording.samples.len())
    }
    /// Called only after the current owner passes the speech-permit check.
    /// Mark before transport. The caller propagates send failure into ordinary
    /// cancellation, retaining attempted evidence and never restarting this clip.
    pub(super) fn reserve_chunk(&mut self, at_us: u64) -> io::Result<(Vec<i16>, bool, bool)> {
        let Self::Selected {
            recording,
            cursor,
            attempted_at_us,
            retired_at_us: None,
            ..
        } = self
        else {
            return Err(io::Error::other(
                "notice dispatch without selected live recording",
            ));
        };
        if at_us == 0 || *cursor >= recording.samples.len() {
            return Err(io::Error::other(
                "notice exhausted or invalid dispatch time",
            ));
        }
        let first = attempted_at_us.is_none();
        if first {
            *attempted_at_us = Some(at_us);
        }
        let end = (*cursor + 240).min(recording.samples.len());
        let mut samples = recording.samples[*cursor..end].to_vec();
        samples.resize(240, 0);
        *cursor = end;
        Ok((samples, end == recording.samples.len(), first))
    }
    /// Bound to the real full final-chunk acceptance, not dispatch or queue time.
    pub(super) fn accepted_final(
        &mut self,
        sequence: u64,
        cursor: PlaybackCursor,
    ) -> io::Result<()> {
        let Self::Selected {
            recording,
            cursor: reserved,
            attempted_at_us: Some(_),
            retired_at_us: None,
            final_acceptance,
            ..
        } = self
        else {
            return Err(io::Error::other(
                "notice final acceptance without attempted recording",
            ));
        };
        if *reserved != recording.samples.len()
            || sequence == 0
            || cursor.epoch == 0
            || cursor.frame == 0
            || final_acceptance.is_some()
        {
            return Err(io::Error::other("invalid notice final acceptance"));
        }
        *final_acceptance = Some((sequence, cursor));
        Ok(())
    }
    pub(super) fn validate_retirement(
        &self,
        sequence: u64,
        cursor: PlaybackCursor,
    ) -> io::Result<()> {
        if !matches!(self, Self::Selected {
            final_acceptance: Some(accepted), retired_at_us: None, ..
        } if *accepted == (sequence, cursor))
        {
            return Err(io::Error::other(
                "notice retirement differs from actual final acceptance",
            ));
        }
        Ok(())
    }
    pub(super) fn retire(&mut self, at_us: u64) -> io::Result<()> {
        let Self::Selected {
            recording,
            cursor,
            attempted_at_us: Some(start),
            retired_at_us,
            ..
        } = self
        else {
            return Err(io::Error::other(
                "notice retirement without an attempted recording",
            ));
        };
        if *cursor != recording.samples.len() || retired_at_us.is_some() || at_us < *start {
            return Err(io::Error::other(
                "notice retirement before exact final delivery",
            ));
        }
        *retired_at_us = Some(at_us);
        Ok(())
    }
    pub(super) fn gap(&mut self) -> io::Result<()> {
        let Self::Selected { gaps, .. } = self else {
            return Err(io::Error::other("notice gap without recording"));
        };
        *gaps = gaps
            .checked_add(1)
            .ok_or_else(|| io::Error::other("notice gap counter exhausted"))?;
        Ok(())
    }
    pub(super) fn evidence(&self) -> Value {
        match self {
            Self::Unselected => Value::Null,
            Self::Skipped(reason) => json!({"state":"skipped","reason":reason}),
            Self::Selected {
                origin,
                recording,
                cursor,
                attempted_at_us,
                retired_at_us,
                gaps,
                ..
            } => {
                let state = if retired_at_us.is_some() {
                    "retired"
                } else if attempted_at_us.is_some() {
                    "attempted"
                } else {
                    "selected"
                };
                match &recording.info {
                    Some(info) => json!({
                        "state":state,
                        "kind":info.kind,"wav_sha256":info.wav_sha256,
                        "approval":info.approval,"frames":info.frames,
                        "reserved_frames":cursor,"attempted_at_us":attempted_at_us,
                        "retired_at_us":retired_at_us,"playback_gaps":gaps,
                        "answer":false,"acoustic_delivery_verified":false}),
                    None => json!({
                        "state":state,"kind":origin.trace_name(),
                        "frames":recording.samples.len(),
                        "reserved_frames":cursor,"attempted_at_us":attempted_at_us,
                        "retired_at_us":retired_at_us,"playback_gaps":gaps,
                        "answer":false,"acoustic_delivery_verified":false}),
                }
            }
        }
    }
}

/// No owner is minted here. A completed, cancelled or never-owned request
/// cannot acquire this recording. Selection does not grant a speaker permit.
#[cfg(test)]
pub(super) fn select_failure_notice(
    catalog: &FailureNotices,
    reply: &mut Option<Reply>,
    pending: &Option<PendingSpeaker>,
    trace: &mut SessionState,
) -> Result<()> {
    select_failure_notice_in(catalog, reply, pending, &mut Outage::default(), trace)
}

/// The same within one outage (#830), which says the offline line once.
pub(super) fn select_failure_notice_in(
    catalog: &FailureNotices,
    reply: &mut Option<Reply>,
    pending: &Option<PendingSpeaker>,
    outage: &mut Outage,
    trace: &mut SessionState,
) -> Result<()> {
    let Some(current) = reply.as_mut() else {
        return Ok(());
    };
    // An autonomous reminder that cannot be delivered stays pending silently.
    // A local failure recording cannot stand in for the intended reminder.
    if current.reminder.is_some() {
        return Ok(());
    }
    if !matches!(current.notice, NoticeDelivery::Unselected)
        || !matches!(current.cloud.outcome, Some(RequestOutcome::Failed { .. }))
        || !current.local_input_ended
        || !current.provider_idle
        || !current.pcm.is_empty()
        || current.playback.is_some()
        || current.final_chunk.is_some()
        || pending.is_some()
    {
        return Ok(());
    }
    // A local-only turn says the offline line, once an outage and only for
    // speech; any other try gets no words, only the ring's offline breath.
    let selected = match current.local_only {
        None => &catalog.selected,
        Some(local) if local.back => &catalog.back_online,
        Some(local) if local.speech && !outage.said && local.service => &catalog.service_down,
        Some(local) if local.speech && !outage.said => &catalog.offline,
        Some(_) => return Ok(()),
    };
    current.notice = match selected {
        Ok(recording) => NoticeDelivery::Selected {
            origin: SpeechOrigin::FailureNotice,
            recording: Arc::clone(recording),
            cursor: 0,
            attempted_at_us: None,
            retired_at_us: None,
            final_acceptance: None,
            gaps: 0,
        },
        Err(reason) => NoticeDelivery::Skipped(*reason),
    };
    record(
        trace,
        json!({"kind":"failure_notice_selected","at_us":monotonic_us(),
        "owner":current.owner,"turn":current.owner.turn(),"notice":current.notice.evidence(),
        "provider_outcome":current.cloud.outcome,"output_authority_granted":false}),
    )
}

#[cfg(test)]
impl FailureNotices {
    /// The synthetic catalog with an offline line too (#830).
    pub(super) fn synthetic_offline() -> Self {
        let mut catalog = Self::synthetic();
        let Ok(turn_lost) = &catalog.selected else {
            unreachable!("the synthetic catalog has its recording");
        };
        let recording = |kind, sample| {
            let mut info = turn_lost.info.clone();
            if let Some(info) = info.as_mut() {
                info.kind = kind;
            }
            Ok(Arc::new(Recording {
                info,
                samples: vec![sample; 7_200].into_boxed_slice(),
            }))
        };
        catalog.offline = recording(NoticeKind::ProviderUnavailable, 123);
        catalog.back_online = recording(NoticeKind::BackOnline, 77);
        catalog.service_down = recording(NoticeKind::VoiceServiceUnreachable, 55);
        catalog
    }

    pub(super) fn synthetic() -> Self {
        // Test-only PCM. No manifest or production approval is created.
        let samples = vec![321_i16; 7_200].into_boxed_slice();
        Self {
            selected: Ok(Arc::new(Recording {
                info: Some(NoticeInfo {
                    kind: NoticeKind::TurnLost,
                    language: "en-US".into(),
                    transcript: "Synthetic PCM for tests; not a voice asset".into(),
                    voice: "test".into(),
                    voice_revision: "test".into(),
                    approval: Approval::Test,
                    wav_sha256: "test-only-no-wav".into(),
                    pcm_sha256: "test-only".into(),
                    frames: 7_200,
                    duration_ms: 300,
                }),
                samples,
            })),
            offline: Err(Missing::NotProvisioned),
            back_online: Err(Missing::NotProvisioned),
            service_down: Err(Missing::NotProvisioned),
            language: Some("en-US".into()),
        }
    }
}

#[cfg(test)]
mod loading_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn directory() -> Directory {
        let path = std::env::temp_dir().join(format!(
            "lamp-notice-test-{}",
            boot_text(new_boot().unwrap())
        ));
        fs::create_dir(&path).unwrap();
        Directory(path)
    }
    fn test_asset(directory: &Path) -> (crate::options::NoticeOptions, PathBuf) {
        let wav = directory.join("lost.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 24_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..7_200 {
            writer.write_sample(321_i16).unwrap();
        }
        writer.finalize().unwrap();
        let hash = format!("{:x}", Sha256::digest(fs::read(&wav).unwrap()));
        let manifest = json!({"schema_version":1,"notices":[{"kind":"turn_lost","language":"en-US",
            "transcript":"Synthetic PCM, not a voice asset","voice":"test","voice_revision":"test",
            "approval":"test","file":"lost.wav","wav_sha256":hash,"frames":7200}]});
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        (
            crate::options::NoticeOptions::new(directory.into(), "en-US".into()).unwrap(),
            wav,
        )
    }
    #[test]
    fn production_never_accepts_test_pcm_or_unknown_language() {
        let directory = directory();
        let (options, _) = test_asset(&directory.0);
        let catalog = FailureNotices::load(Some(&options));
        assert_eq!(catalog.capability()["missing"], "not_approved");
        let test_only = FailureNotices::load_with_approval(Some(&options), Approval::Test);
        assert_eq!(test_only.capability()["available"]["approval"], "test");
        let other = NoticeOptions::new(directory.0.clone(), "en".into()).unwrap();
        assert_eq!(
            FailureNotices::load_with_approval(Some(&other), Approval::Test).capability()["missing"],
            "not_provisioned"
        );
    }
    #[test]
    fn preloaded_notice_is_immutable_and_missing_corrupt_assets_have_bounded_reasons() {
        let directory = directory();
        let (options, wav) = test_asset(&directory.0);
        let loaded = FailureNotices::load_with_approval(Some(&options), Approval::Test);
        fs::write(&wav, b"changed after preload").unwrap();
        let catalog = FailureNotices::load_with_approval(Some(&options), Approval::Test);
        assert_eq!(catalog.capability()["missing"], "asset_hash_mismatch");
        match &loaded.selected {
            Ok(recording) => assert_eq!(&*recording.samples, &vec![321; 7200]),
            Err(reason) => panic!("test load refused {reason:?}"),
        }
        fs::remove_file(&wav).unwrap();
        assert_eq!(
            FailureNotices::load_with_approval(Some(&options), Approval::Test).capability()["missing"],
            "asset_missing"
        );
        fs::write(directory.0.join("manifest.json"), b"{}").unwrap();
        assert_eq!(
            FailureNotices::load(Some(&options)).capability()["missing"],
            "invalid_manifest"
        );
    }
}
