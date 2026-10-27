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
    /// The Wi-Fi helper's line with no network (#1959): played only when
    /// asked through the say service ([`crate::say::Recordings`]).
    wifi_lost: std::result::Result<Arc<Recording>, Missing>,
    /// The safe shutdown's line (#3697): played only when asked through the
    /// say service, so a lamp that cannot reach its model still says it.
    going_to_sleep: std::result::Result<Arc<Recording>, Missing>,
    /// With a notices folder configured, what a failed line plays when its
    /// recording can't be loaded (#2943 part 3): the lamp's own sigh, made
    /// here, so a lamp whose files are missing never meets a question it
    /// can't answer with silence. None with no folder configured, which
    /// stays silent as before.
    fallback: Option<Arc<Recording>>,
    language: Option<String>,
}

/// How loud the fallback sigh is, as RMS over its loudest tenth of a
/// second: under the lamp's speech (about -20 dBFS). This fallback level
/// is independent of the head tap's measured default. Not yet measured
/// against a lamp's speech.
pub(super) const FALLBACK_DBFS: f64 = -26.0;
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
            wifi_lost: get(NoticeKind::WifiLost),
            going_to_sleep: get(NoticeKind::GoingToSleep),
            fallback: Some(Arc::new(Recording {
                info: None,
                samples: crate::head_cue::breath(FALLBACK_DBFS).into_boxed_slice(),
            })),
            language: Some(options.language().to_owned()),
        }
    }
    pub(super) fn none() -> Self {
        Self {
            selected: Err(Missing::NoDirectory),
            offline: Err(Missing::NoDirectory),
            back_online: Err(Missing::NoDirectory),
            service_down: Err(Missing::NoDirectory),
            wifi_lost: Err(Missing::NoDirectory),
            going_to_sleep: Err(Missing::NoDirectory),
            fallback: None,
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
            // What stands in for a missing line (#2943 part 3).
            capability["fallback"] = json!(self.fallback.as_ref().map(|_| "sigh"));
            let state = |selected: &std::result::Result<Arc<Recording>, Missing>| match selected {
                Ok(recording) => json!({"available":recording.info}),
                Err(reason) => json!({"missing":reason}),
            };
            capability["offline"] = state(&self.offline);
            capability["back_online"] = state(&self.back_online);
            capability["service_down"] = state(&self.service_down);
            capability["wifi_lost"] = state(&self.wifi_lost);
            capability["going_to_sleep"] = state(&self.going_to_sleep);
        }
        capability
    }
    /// What the say service may play by name (#1959): the samples, or the
    /// loader's reason as its snake_case name.
    pub(super) fn recordings(&self) -> crate::say::Recordings {
        let loaded = |held: &std::result::Result<Arc<Recording>, Missing>| {
            held.as_ref()
                .map(|recording| Arc::<[i16]>::from(&recording.samples[..]))
                .map_err(|missing| {
                    serde_json::to_value(missing)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "unavailable".into())
                })
        };
        crate::say::Recordings::none()
            .with("wifi_lost", loaded(&self.wifi_lost))
            .with("going_to_sleep", loaded(&self.going_to_sleep))
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
    /// The falling breath in a hand's hold on the head (`head_cue::breath`,
    /// #1362), as the lamp settles to sleep.
    SleepBreath,
    /// The "hm?" as the person wakes the lamp from a hand's sleep (#1362).
    WakeHum,
    /// A skill's own sound (`crate::say`, #938): local PCM, no answer.
    SkillSound,
    /// A person's bounded recording, after its spoken confirmation retired.
    RequestedAudio,
}
impl SpeechOrigin {
    /// Played from a recording held here, not from the provider.
    pub(super) fn local(self) -> bool {
        self != Self::Provider
    }
    /// A turn that is only a short sound of the lamp's own, and none for the
    /// face, the head, the room or the voice's state.
    pub(super) fn unshown(self) -> bool {
        matches!(
            self,
            Self::HeadCue | Self::UnclearHum | Self::SleepBreath | Self::WakeHum | Self::SkillSound
        )
    }
    /// The trace's name for what plays.
    pub(super) fn trace_name(self) -> &'static str {
        match self {
            Self::Provider => "speaker",
            Self::FailureNotice => "failure_notice",
            Self::HeadCue => "head_tap_cue",
            Self::UnclearHum => "unclear_hum",
            Self::SleepBreath => "head_sleep_breath",
            Self::WakeHum => "head_wake_cue",
            Self::SkillSound => "skill_sound",
            Self::RequestedAudio => "requested_audio",
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
        /// `FailureNotice`, `HeadCue`, `UnclearHum`, `SleepBreath`,
        /// `WakeHum` or `SkillSound`.
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
    let (selected, stands_in) = match current.local_only {
        None => (&catalog.selected, true),
        // "Back online" is never stood in for: nothing failed.
        Some(local) if local.back => (&catalog.back_online, false),
        Some(local) if local.speech && !outage.said && local.service => {
            (&catalog.service_down, true)
        }
        Some(local) if local.speech && !outage.said => (&catalog.offline, true),
        Some(_) => return Ok(()),
    };
    let selected_recording = |recording: &Arc<Recording>| NoticeDelivery::Selected {
        origin: SpeechOrigin::FailureNotice,
        recording: Arc::clone(recording),
        cursor: 0,
        attempted_at_us: None,
        retired_at_us: None,
        final_acceptance: None,
        gaps: 0,
    };
    // A line that can't be loaded, with a folder configured: the lamp's own
    // sigh in its place, and why, in the evidence (#2943 part 3).
    let mut stood_in_for = None;
    current.notice = match (selected, &catalog.fallback) {
        (Ok(recording), _) => selected_recording(recording),
        (Err(reason), Some(sigh)) if stands_in => {
            stood_in_for = Some(*reason);
            selected_recording(sigh)
        }
        (Err(reason), _) => NoticeDelivery::Skipped(*reason),
    };
    record(
        trace,
        json!({"kind":"failure_notice_selected","at_us":monotonic_us(),
        "owner":current.owner,"turn":current.owner.turn(),"notice":current.notice.evidence(),
        "fallback_for":stood_in_for,
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

    /// A folder configured that isn't there (#2943 part 3): every line
    /// missing, the lamp's own sigh to stand in.
    pub(super) fn missing_with_sigh() -> Self {
        let gone = std::env::temp_dir().join("lamp-notice-test-never-made-2943");
        Self::load(Some(
            &crate::options::NoticeOptions::new(gone, "en-US".into()).unwrap(),
        ))
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
            wifi_lost: Err(Missing::NotProvisioned),
            going_to_sleep: Err(Missing::NotProvisioned),
            // The other tests' catalog stays silent on a missing line.
            fallback: None,
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
    /// #2943 part 3: a configured folder that isn't there leaves every line
    /// missing, with the lamp's own sigh to stand in, said in its
    /// capability; with no folder configured nothing stands in, as before.
    #[test]
    fn a_missing_folder_stands_the_sigh_in_and_no_folder_stays_silent() {
        let gone = std::env::temp_dir().join("lamp-notice-test-never-made-2943");
        let options = crate::options::NoticeOptions::new(gone, "en-US".into()).unwrap();
        let missing = FailureNotices::load(Some(&options));
        assert!(matches!(missing.selected, Err(Missing::NoDirectory)));
        let sigh = missing.fallback.as_ref().unwrap();
        assert!(sigh.info.is_none());
        assert_eq!(
            &sigh.samples[..],
            &crate::head_cue::breath(FALLBACK_DBFS)[..]
        );
        assert_eq!(missing.capability()["fallback"], "sigh");
        assert_eq!(missing.capability()["missing"], "no_directory");
        let unset = FailureNotices::load(None);
        assert!(unset.fallback.is_none());
        assert!(unset.capability().get("fallback").is_none());
    }

    /// #1959: the say service is handed the Wi-Fi line's samples when it
    /// loaded, and the loader's reason when it did not.
    #[test]
    fn the_say_service_gets_the_wifi_line_or_why_not() {
        let none = FailureNotices::none().recordings();
        assert_eq!(
            none.samples("wifi_lost"),
            Err("the recording is not there: no_directory".into())
        );
        let directory = directory();
        let (options, _) = test_asset(&directory.0);
        let only_turn_lost = FailureNotices::load_with_approval(Some(&options), Approval::Test);
        assert_eq!(
            only_turn_lost.recordings().samples("wifi_lost"),
            Err("the recording is not there: not_provisioned".into())
        );
        assert_eq!(
            only_turn_lost.capability()["wifi_lost"]["missing"],
            "not_provisioned"
        );
        let mut catalog = FailureNotices::synthetic();
        catalog.wifi_lost = Ok(Arc::new(Recording {
            info: None,
            samples: vec![99_i16; 7_200].into_boxed_slice(),
        }));
        assert_eq!(
            catalog.recordings().samples("wifi_lost"),
            Ok(vec![99_i16; 7_200])
        );
        // #3697: the shutdown's line the same way, each by its own name.
        assert_eq!(
            catalog.recordings().samples("going_to_sleep"),
            Err("the recording is not there: not_provisioned".into())
        );
        catalog.going_to_sleep = Ok(Arc::new(Recording {
            info: None,
            samples: vec![7_i16; 7_200].into_boxed_slice(),
        }));
        assert_eq!(
            catalog.recordings().samples("going_to_sleep"),
            Ok(vec![7_i16; 7_200])
        );
        assert_eq!(
            only_turn_lost.capability()["going_to_sleep"]["missing"],
            "not_provisioned"
        );
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
