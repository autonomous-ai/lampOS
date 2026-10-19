//! Normalized runtime events. Every timestamp keeps its clock domain; values
//! from different domains are never subtracted without an explicit mapping.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockDomain {
    /// Fake runtime simulation time. Never comparable with wall clocks.
    Virtual,
    /// Lamp host `CLOCK_MONOTONIC` microseconds (lamp-voice traces and cues).
    LampMonotonic,
    /// Runner host `CLOCK_MONOTONIC` microseconds (the Mac driving stimuli).
    RunnerMonotonic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Fake,
    /// Final `events.jsonl` written by lamp-voice.
    Trace,
    /// Live scheduling cue from lamp-voice's optional cue socket.
    Cue,
    /// A status line lamp-voice printed on stdout.
    Stdout,
    /// The runner's own evidence (player or room recorder), never Lamp's.
    Runner,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    RunStart {
        provider_kind: Option<String>,
        /// `second` when the second route alone answered
        /// (`LAMP_VOICE_ROUTE=second`); absent otherwise, as before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        voice_route: Option<String>,
    },
    ListeningReady,
    /// A speech candidate before any destructive turn replacement.
    InputCandidate {
        candidate: Value,
    },
    InputCandidateRejected {
        candidate: Value,
        reason: Option<String>,
    },
    InputAdmitted {
        prefix_first_read_us: Option<u64>,
        /// Candidate ID and admission basis, recorded since lamp-voice 9c2c82e1.
        #[serde(default)]
        candidate: Option<Value>,
        #[serde(default)]
        basis: Option<String>,
    },
    /// A requested ring cue (`--ring-channel-ceiling`); SPI/optical output unmeasured.
    RingRequested {
        phase: String,
    },
    ProviderInputStarted,
    LocalEndpoint {
        last_block_read_us: Option<u64>,
        /// Silence wait the runtime reports inside this endpoint
        /// (`includes_silence_wait_ms`); `None` when the record states none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        silence_wait_ms: Option<u64>,
    },
    /// The detector end of a submitted request that was abandoned (for
    /// example drained during a provider outage). Never a normal endpoint:
    /// it does not start answer latency.
    InputDiscardedEndpoint {
        last_block_read_us: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        silence_wait_ms: Option<u64>,
    },
    ProviderFirstAudio,
    SpeakerFirstWrite,
    SpeechRetired,
    PlaybackGap {
        phase: String,
    },
    /// A reply revoked before completion (`turn_finished` with an owner).
    /// Cues preserve the optional original outcome; the final trace also carries it.
    TurnCancelled {
        reason: Option<String>,
        provider_audio_seen: Option<bool>,
    },
    /// Normal completion (`turn_finished` without an owner).
    TurnCompleted {
        outcome: String,
        playback_gaps: u64,
    },
    ProviderInterrupted,
    ProviderTurnComplete,
    CancelledTailRetired,
    PlaybackDiscarded {
        expected: bool,
    },
    /// Provider ASR of the person's speech; session scoped, not turn correlated.
    InputTranscript {
        text: String,
        finished: bool,
    },
    OutputTranscript {
        text: String,
        finished: bool,
    },
    RuntimeFault {
        reason: String,
    },
    /// Whether the turn-taking policy would currently retain new input
    /// (`turn_taking_availability`). Recorded on change only: V1-baseline
    /// turns it off while Lamp's own playback is owed and on again after its
    /// tail; `vad-only` reports it once, available.
    InputAvailability {
        available: bool,
    },
    /// The provider link as the coordinator observed it: `ready`,
    /// `unavailable` (an outage that may recover) or `terminal`.
    ProviderLink {
        state: String,
    },
    RunEnd {
        status: Option<String>,
        error: Option<String>,
    },
    /// Runner evidence a conversation needs was lost before the next start:
    /// a stimulus the player refused or failed to deliver, or the room
    /// recorder ending early. Synthesized by the runner, never by Lamp.
    EvidenceLost {
        source: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEvent {
    #[serde(flatten)]
    pub kind: EventKind,
    pub turn: Option<u64>,
    pub at_us: Option<u64>,
    pub domain: ClockDomain,
    pub source: EventSource,
    /// Receipt time on the runner, when the event arrived live.
    #[serde(default)]
    pub received_us: Option<u64>,
    /// Immutable playback occurrence, not a turn or provider generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_sequence: Option<u64>,
    /// Invalid trace fields are retained with their attempt, never defaulted into proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_evidence_error: Option<String>,
    /// Original turn authority generation when the source records it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_generation: Option<u64>,
    /// Boot identity of the turn authority when the source records it. A
    /// present but malformed identity is kept as all zeros, never omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_boot: Option<[u8; 16]>,
    /// Provider evidence kept verbatim from a terminal or drain record:
    /// `provider_session` and `provider_outcome` (stage, reason,
    /// local_not_submitted). Bounded; never interpreted as success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_detail: Option<Value>,
}

/// Largest retained `provider_detail` (serialized bytes).
pub const MAX_PROVIDER_DETAIL_BYTES: usize = 2048;

/// The provider fields of a trace record, bounded.
fn provider_detail(record: &Value) -> Option<Value> {
    let mut detail = serde_json::Map::new();
    for key in ["provider_session", "provider_outcome"] {
        if let Some(value) = record.get(key).filter(|v| !v.is_null()) {
            detail.insert(key.into(), value.clone());
        }
    }
    if detail.is_empty() {
        return None;
    }
    let value = Value::Object(detail);
    let bytes = value.to_string().len();
    Some(if bytes > MAX_PROVIDER_DETAIL_BYTES {
        json!({"truncated": true, "bytes": bytes})
    } else {
        value
    })
}

impl RuntimeEvent {
    pub fn new(
        kind: EventKind,
        turn: Option<u64>,
        at_us: u64,
        domain: ClockDomain,
        source: EventSource,
    ) -> Self {
        Self {
            kind,
            turn,
            at_us: Some(at_us),
            domain,
            source,
            received_us: None,
            playback_sequence: None,
            completion_evidence_error: None,
            owner_generation: None,
            owner_boot: None,
            provider_detail: None,
        }
    }
    pub fn name(&self) -> &'static str {
        match &self.kind {
            EventKind::RunStart { .. } => "run_start",
            EventKind::ListeningReady => "listening_ready",
            EventKind::InputCandidate { .. } => "input_candidate",
            EventKind::InputCandidateRejected { .. } => "input_candidate_rejected",
            EventKind::InputAdmitted { .. } => "input_admitted",
            EventKind::RingRequested { .. } => "ring_requested",
            EventKind::ProviderInputStarted => "provider_input_started",
            EventKind::LocalEndpoint { .. } => "local_endpoint",
            EventKind::InputDiscardedEndpoint { .. } => "input_discarded_endpoint",
            EventKind::ProviderFirstAudio => "provider_first_audio",
            EventKind::SpeakerFirstWrite => "speaker_first_write",
            EventKind::SpeechRetired => "speech_retired",
            EventKind::PlaybackGap { .. } => "playback_gap",
            EventKind::TurnCancelled { .. } => "turn_cancelled",
            EventKind::TurnCompleted { .. } => "turn_completed",
            EventKind::ProviderInterrupted => "provider_interrupted",
            EventKind::ProviderTurnComplete => "provider_turn_complete",
            EventKind::CancelledTailRetired => "cancelled_tail_retired",
            EventKind::PlaybackDiscarded { .. } => "playback_discarded",
            EventKind::InputTranscript { .. } => "input_transcript",
            EventKind::OutputTranscript { .. } => "output_transcript",
            EventKind::RuntimeFault { .. } => "runtime_fault",
            EventKind::InputAvailability { .. } => "input_availability",
            EventKind::ProviderLink { .. } => "provider_link",
            EventKind::RunEnd { .. } => "run_end",
            EventKind::EvidenceLost { .. } => "evidence_lost",
        }
    }
}

fn u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

/// Map one lamp-voice `events.jsonl` record. Unknown kinds return `None` and are
/// counted by the caller; they are never guessed into a known meaning.
pub fn from_trace(record: &Value) -> Option<RuntimeEvent> {
    let kind = record.get("kind")?.as_str()?;
    let turn = u64_field(record, "turn");
    let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::to_owned);
    let event = match kind {
        "run_start" => EventKind::RunStart {
            provider_kind: text("provider_kind"),
            voice_route: text("voice_route"),
        },
        "listening_ready" => EventKind::ListeningReady,
        "input_candidate" => EventKind::InputCandidate {
            candidate: record["candidate"]["id"].clone(),
        },
        "input_candidate_rejected" => EventKind::InputCandidateRejected {
            candidate: record["candidate"].clone(),
            reason: text("reason"),
        },
        "input_admitted" => EventKind::InputAdmitted {
            prefix_first_read_us: u64_field(record, "prefix_first_host_read_us"),
            candidate: record.get("candidate").filter(|c| !c.is_null()).cloned(),
            basis: text("admission_basis"),
        },
        "ring_requested" => EventKind::RingRequested {
            phase: text("phase").unwrap_or_default(),
        },
        "provider_input_started" => EventKind::ProviderInputStarted,
        "local_endpoint" => EventKind::LocalEndpoint {
            last_block_read_us: u64_field(record, "last_block_host_read_us"),
            silence_wait_ms: u64_field(record, "includes_silence_wait_ms"),
        },
        // A submitted request abandoned during a provider outage reaches its
        // detector end here, not through the normal owned endpoint.
        "input_discarded_endpoint" => EventKind::InputDiscardedEndpoint {
            last_block_read_us: u64_field(record, "last_block_host_read_us"),
            silence_wait_ms: u64_field(record, "includes_silence_wait_ms"),
        },
        "provider_first_audio" => EventKind::ProviderFirstAudio,
        "speaker_first_write" => EventKind::SpeakerFirstWrite,
        "speech_final_sample_retired" => EventKind::SpeechRetired,
        "playback_gap" => EventKind::PlaybackGap {
            phase: record
                .get("phase")
                .map(|phase| {
                    phase
                        .as_str()
                        .map_or_else(|| phase.to_string(), str::to_owned)
                })
                .unwrap_or_default(),
        },
        // The coordinator's cancellation path records the owner; completion does not.
        "turn_finished" if record.get("owner").is_some_and(Value::is_object) => {
            EventKind::TurnCancelled {
                reason: text("outcome"),
                provider_audio_seen: record.get("provider_audio_seen").and_then(Value::as_bool),
            }
        }
        "turn_finished" => EventKind::TurnCompleted {
            outcome: text("outcome").unwrap_or_default(),
            playback_gaps: u64_field(record, "playback_gaps").unwrap_or(0),
        },
        "provider_interrupted" => EventKind::ProviderInterrupted,
        "provider_turn_complete" => EventKind::ProviderTurnComplete,
        "cancelled_tail_retired" => EventKind::CancelledTailRetired,
        "playback_discarded" => EventKind::PlaybackDiscarded {
            expected: record
                .get("expected_cancellation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "transcript" => {
            let text = text("text").unwrap_or_default();
            let finished = record
                .get("finished")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if turn.is_some() {
                EventKind::OutputTranscript { text, finished }
            } else {
                EventKind::InputTranscript { text, finished }
            }
        }
        "reference_fault"
        | "speaker_rejected"
        | "worker_exit_failed"
        | "worker_forced_shutdown"
        | "capture_processing_mismatch"
        | "stop_delivery_error"
        | "fixture_cue_invalid" => EventKind::RuntimeFault {
            reason: kind.to_owned(),
        },
        "audio_diagnostics_certified" if record.get("valid") == Some(&Value::Bool(false)) => {
            EventKind::RuntimeFault {
                reason: "audio_diagnostics_invalid".into(),
            }
        }
        "turn_taking_availability" => EventKind::InputAvailability {
            available: record.get("prospective_input_available")?.as_bool()?,
        },
        "provider_ready" => EventKind::ProviderLink {
            state: "ready".into(),
        },
        "provider_unavailable" => EventKind::ProviderLink {
            state: "unavailable".into(),
        },
        "provider_terminal" => EventKind::ProviderLink {
            state: "terminal".into(),
        },
        // The coordinator's current link state, published with readiness.
        "provider_link_current" => EventKind::ProviderLink {
            state: record
                .get("state")?
                .as_str()
                .filter(|state| matches!(*state, "ready" | "unavailable" | "terminal"))?
                .into(),
        },
        "run_end" => EventKind::RunEnd {
            status: text("status"),
            error: text("error"),
        },
        _ => return None,
    };
    // Ring requests name their turn only inside the owner.
    let turn = turn.or_else(|| record["owner"]["turn"].as_u64());
    Some(RuntimeEvent {
        kind: event,
        turn,
        at_us: u64_field(record, "at_us"),
        domain: ClockDomain::LampMonotonic,
        source: EventSource::Trace,
        received_us: None,
        // Keep an explicitly malformed identity invalid rather than treating it as legacy omission.
        playback_sequence: record
            .get("playback_sequence")
            .map(|v| v.as_u64().unwrap_or(0)),
        completion_evidence_error: (kind == "turn_finished"
            && !record.get("owner").is_some_and(Value::is_object)
            && (record
                .get("outcome")
                .and_then(Value::as_str)
                .is_none_or(|s| s.is_empty())
                || record
                    .get("playback_gaps")
                    .and_then(Value::as_u64)
                    .is_none()
                || record
                    .get("generation")
                    .is_some_and(|g| g.as_u64().is_none_or(|n| n == 0))))
        .then(|| "malformed or missing whole-turn completion fields".into()),
        owner_generation: record
            .get("owner")
            .and_then(|owner| owner.get("generation"))
            .or_else(|| record.get("generation"))
            .map(|v| v.as_u64().unwrap_or(0)),
        owner_boot: record
            .get("owner")
            .and_then(|owner| owner.get("boot"))
            .map(|boot| serde_json::from_value(boot.clone()).unwrap_or([0; 16])),
        provider_detail: matches!(kind, "turn_finished" | "input_discarded_endpoint")
            .then(|| provider_detail(record))
            .flatten(),
    })
}

/// Parse a lamp-voice trace file's text, keeping a count of unmapped records.
pub fn parse_trace(text: &str) -> Result<(Vec<RuntimeEvent>, usize)> {
    let mut events = Vec::new();
    let mut unmapped = 0;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if index >= 25_000 {
            return Err(invalid("trace exceeds the 20,000-event runtime bound"));
        }
        let record: Value = serde_json::from_str(line)?;
        match from_trace(&record) {
            Some(event) => events.push(event),
            None => unmapped += 1,
        }
    }
    Ok((events, unmapped))
}

/// lamp-voice scheduling datagram (`fixture_provider::CueSink`, schema 1).
/// These timestamps are software events, never microphone/speaker onsets.
pub const CUE_FRESH_US: u64 = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cue {
    pub schema: u8,
    pub sequence: u64,
    pub kind: String,
    pub boot: [u8; 16],
    pub turn: Option<u64>,
    pub generation: Option<u64>,
    pub capture_epoch: Option<u64>,
    pub reference_epoch_context: Option<u64>,
    pub event_us: u64,
    pub sent_us: u64,
    pub expires_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_gaps: Option<u64>,
    /// `input_availability` only: the policy's prospective input availability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// `provider_link` only: `ready`, `unavailable` or `terminal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

pub fn parse_cue(bytes: &[u8]) -> Result<Cue> {
    if bytes.len() > 512 {
        return Err(invalid("cue exceeds 512 bytes"));
    }
    let cue: Cue = serde_json::from_slice(bytes)?;
    let owned = match cue.kind.as_str() {
        // Unowned readiness evidence proposed for lamp-voice's cue sink: the
        // runner needs it to wait for the next turn by event, not by delay.
        "listening_ready" | "run_end" | "input_availability" | "provider_link"
        | "input_candidate" => false,
        "input_admitted"
        | "local_endpoint"
        | "speaker_first_write"
        | "speech_retired"
        | "cancelled"
        | "turn_completed" => true,
        _ => return Err(invalid("unknown cue kind")),
    };
    if cue.schema != 1
        || cue.sequence == 0
        || cue.boot == [0; 16]
        || cue.sent_us < cue.event_us
        || cue.sent_us >= cue.expires_us
        || cue.event_us.checked_add(CUE_FRESH_US) != Some(cue.expires_us)
        || (owned && (cue.turn.is_none_or(|v| v == 0) || cue.generation.is_none_or(|v| v == 0)))
        || (!owned && (cue.turn.is_some() || cue.generation.is_some()))
        || (cue.kind == "turn_completed"
            && (cue.playback_gaps.is_none()
                || !cue.outcome.as_deref().is_some_and(completion_outcome)))
        || (cue.kind == "turn_completed"
            && !completion_gaps_match(
                cue.outcome.as_deref().unwrap_or(""),
                cue.playback_gaps.unwrap_or(0),
            ))
        || (cue.kind != "turn_completed" && (cue.outcome.is_some() || cue.playback_gaps.is_some()))
        || cue.playback_sequence.is_some_and(|n| {
            n == 0 || !matches!(cue.kind.as_str(), "speaker_first_write" | "speech_retired")
        })
        || cue.capture_epoch == Some(0)
        || cue.reference_epoch_context == Some(0)
        || cue
            .reason
            .as_ref()
            .is_some_and(|s| cue.kind != "cancelled" || s.is_empty())
        || (cue.kind == "input_availability") != cue.available.is_some()
        || (cue.kind == "provider_link")
            != cue
                .link
                .as_deref()
                .is_some_and(|l| matches!(l, "ready" | "unavailable" | "terminal"))
        || (cue.kind != "provider_link" && cue.link.is_some())
    {
        return Err(invalid("unsupported or inconsistent cue identity/times"));
    }
    Ok(cue)
}

pub(crate) fn completion_gaps_match(outcome: &str, gaps: u64) -> bool {
    match outcome {
        "audio_written_unscored" | "no_audio_answer" => gaps == 0,
        "audio_written_with_playback_gaps_unscored" => gaps > 0,
        // A terminal failure may follow a partial answer with or without gaps.
        PROVIDER_FAILED => true,
        _ => false,
    }
}

/// Terminal provider failure (MAIN-009): a failed answer, before audio or
/// after a partial answer whose played audio was retired. Provisional name
/// until the coordinator publishes its frozen cue schema.
pub const PROVIDER_FAILED: &str = "provider_failed";

pub(crate) fn completion_outcome(value: &str) -> bool {
    matches!(
        value,
        "no_audio_answer"
            | "audio_written_unscored"
            | "audio_written_with_playback_gaps_unscored"
            | PROVIDER_FAILED
    )
}

impl Cue {
    pub fn event(&self) -> Option<RuntimeEvent> {
        let kind = match self.kind.as_str() {
            "listening_ready" => EventKind::ListeningReady,
            "input_admitted" => EventKind::InputAdmitted {
                prefix_first_read_us: None,
                candidate: None,
                basis: None,
            },
            "local_endpoint" => EventKind::LocalEndpoint {
                last_block_read_us: None,
                silence_wait_ms: None,
            },
            "speaker_first_write" => EventKind::SpeakerFirstWrite,
            "speech_retired" => EventKind::SpeechRetired,
            "cancelled" => EventKind::TurnCancelled {
                reason: self.reason.clone(),
                provider_audio_seen: None,
            },
            "turn_completed" => EventKind::TurnCompleted {
                outcome: self.outcome.clone()?,
                playback_gaps: self.playback_gaps?,
            },
            "run_end" => EventKind::RunEnd {
                status: None,
                error: None,
            },
            "input_availability" => EventKind::InputAvailability {
                available: self.available?,
            },
            "provider_link" => EventKind::ProviderLink {
                state: self.link.clone()?,
            },
            // The cue carries no candidate identity; the final trace does.
            "input_candidate" => EventKind::InputCandidate {
                candidate: Value::Null,
            },
            _ => return None,
        };
        Some(RuntimeEvent {
            kind,
            turn: self.turn,
            at_us: Some(self.event_us),
            domain: ClockDomain::LampMonotonic,
            source: EventSource::Cue,
            received_us: None,
            playback_sequence: self.playback_sequence,
            completion_evidence_error: None,
            owner_generation: self.generation,
            owner_boot: self.turn.map(|_| self.boot),
            provider_detail: None,
        })
    }
}

/// Serialize an event in lamp-voice's trace vocabulary. Used by the fake
/// lamp-voice process for loopback tests of the physical orchestration path.
pub fn to_trace(event: &RuntimeEvent, owner: Option<Value>) -> Value {
    let at = event.at_us.unwrap_or(0);
    let mut record = match &event.kind {
        EventKind::RunStart {
            provider_kind,
            voice_route,
        } => {
            let mut record =
                json!({"kind":"run_start","provider_kind":provider_kind,"acoustic_score":null});
            if let Some(route) = voice_route {
                record["voice_route"] = json!(route);
            }
            record
        }
        EventKind::ListeningReady => json!({"kind":"listening_ready"}),
        EventKind::InputCandidate { candidate } => {
            json!({"kind":"input_candidate","candidate":{"id":candidate}})
        }
        EventKind::InputCandidateRejected { candidate, reason } => {
            json!({"kind":"input_candidate_rejected","candidate":candidate,"reason":reason})
        }
        EventKind::InputAdmitted {
            prefix_first_read_us,
            candidate,
            basis,
        } => {
            json!({"kind":"input_admitted","owner":owner,"prefix_first_host_read_us":prefix_first_read_us,
                "candidate":candidate,"admission_basis":basis})
        }
        EventKind::RingRequested { phase } => {
            json!({"kind":"ring_requested","owner":owner,"phase":phase})
        }
        EventKind::ProviderInputStarted => {
            json!({"kind":"provider_input_started","waiting_for_barrier":false})
        }
        // The fake states its own detector's wait; no default is invented.
        EventKind::LocalEndpoint {
            last_block_read_us,
            silence_wait_ms,
        } => {
            json!({"kind":"local_endpoint","owner":owner,
            "last_block_host_read_us":last_block_read_us,"includes_silence_wait_ms":silence_wait_ms,"acoustic_speech_end":null})
        }
        EventKind::InputDiscardedEndpoint {
            last_block_read_us,
            silence_wait_ms,
        } => {
            json!({"kind":"input_discarded_endpoint","owner":owner,
            "last_block_host_read_us":last_block_read_us,"includes_silence_wait_ms":silence_wait_ms,"acoustic_speech_end":null})
        }
        EventKind::ProviderFirstAudio => json!({"kind":"provider_first_audio"}),
        EventKind::SpeakerFirstWrite => json!({"kind":"speaker_first_write","owner":owner}),
        EventKind::SpeechRetired => json!({"kind":"speech_final_sample_retired","owner":owner}),
        EventKind::PlaybackGap { phase } => json!({"kind":"playback_gap","phase":phase}),
        EventKind::TurnCancelled {
            reason,
            provider_audio_seen,
        } => json!({"kind":"turn_finished","owner":owner.unwrap_or_else(|| json!({})),
            "outcome":reason,"provider_audio_seen":provider_audio_seen,"playback_gaps":0}),
        EventKind::TurnCompleted {
            outcome,
            playback_gaps,
        } => {
            json!({"kind":"turn_finished","generation":owner.as_ref().and_then(|o| o["generation"].as_u64()),
            "outcome":outcome,"playback_gaps":playback_gaps})
        }
        EventKind::ProviderInterrupted => json!({"kind":"provider_interrupted"}),
        EventKind::ProviderTurnComplete => json!({"kind":"provider_turn_complete","idle":true}),
        EventKind::CancelledTailRetired => json!({"kind":"cancelled_tail_retired","owner":owner}),
        EventKind::PlaybackDiscarded { expected } => {
            json!({"kind":"playback_discarded","owner":owner,"expected_cancellation":expected})
        }
        EventKind::InputTranscript { text, finished } => {
            json!({"kind":"transcript","text":text,"finished":finished})
        }
        EventKind::OutputTranscript { text, finished } => {
            json!({"kind":"transcript","text":text,"finished":finished})
        }
        EventKind::RuntimeFault { reason } => json!({"kind":reason}),
        EventKind::InputAvailability { available } => json!({"kind":"turn_taking_availability",
            "prospective_input_available":available,"hard_capture_authority_unchanged":true}),
        EventKind::ProviderLink { state } => json!({"kind":format!("provider_{state}")}),
        EventKind::RunEnd { status, error } => {
            json!({"kind":"run_end","status":status,"error":error})
        }
        EventKind::EvidenceLost { source, reason } => {
            json!({"kind":"runner_evidence_lost","source":source,"reason":reason})
        }
    };
    if let Some(sequence) = event.playback_sequence {
        record["playback_sequence"] = json!(sequence);
    }
    record["at_us"] = json!(at);
    record["turn"] = event.turn.map_or(Value::Null, |turn| json!(turn));
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records copied field-for-field from the `json!` calls in
    /// `crates/voice/src/coordinator.rs` at source 64529dee.
    const COORDINATOR_RECORDS: &str = r#"{"kind":"run_start","at_us":10,"scope":"directed_voice_only","seconds":30,"acoustic_score":null,"audio_diagnostics_requested":true,"provider_kind":"one_cached_reply","fixture":null,"cue_requested":true}
{"kind":"listening_ready","at_us":20,"scope":"explicit directed session; addressee inference absent"}
{"kind":"input_admitted","owner":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":1,"generation":2},"turn":1,"at_us":30,"authority_issued_at_us":29,"prefix_first_host_read_us":5}
{"kind":"local_endpoint","turn":1,"at_us":40,"last_block_host_read_us":39,"includes_silence_wait_ms":600,"acoustic_speech_end":null}
{"kind":"speaker_first_write","turn":1,"owner":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":1,"generation":2},"at_us":50,"boundary":"ALSA accepted; acoustic onset unmeasured"}
{"kind":"turn_finished","owner":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":1,"generation":2},"turn":1,"at_us":60,"outcome":"user_interrupted","provider_audio_seen":true,"playback_gaps":0}
{"kind":"turn_finished","turn":2,"at_us":70,"outcome":"audio_written_with_playback_gaps_unscored","playback_gaps":1}
{"kind":"transcript","turn":null,"text":"How are you doing today?","finished":true,"at_us":80,"input_correlation":"unreliable; session scoped"}
{"kind":"transcript","turn":2,"text":"I'm doing well","finished":false,"at_us":81,"input_correlation":"output lineage"}
{"kind":"reference_fault","worker":"speaker","details":{}}
{"kind":"session_boot","at_us":1,"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]}
{"kind":"run_end","at_us":90,"status":"completed_unscored","error":null,"cue_valid":true}"#;

    /// Records from the candidate/ring vocabulary at source 23bde487.
    const CANDIDATE_RECORDS: &str = r#"{"kind":"input_candidate","candidate":{"id":{"controller":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"serial":4},"trigger_sequence":40,"trigger_read_at_us":95,"first_read_at_us":40,"decision_deadline_us":295,"displaced_owner":null},"at_us":100}
{"kind":"input_admitted","owner":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":2,"generation":3},"turn":2,"at_us":101,"authority_issued_at_us":100,"prefix_first_host_read_us":40,"candidate":{"controller":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"serial":4},"admission_basis":"directed_session_vad_only"}
{"kind":"input_candidate_rejected","candidate":{"controller":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"serial":5},"reason":"deadline","at_us":120}
{"kind":"ring_requested","at_us":130,"owner":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":2,"generation":3},"phase":"listening","ceiling":24,"permit_expires_at_us":230}"#;

    #[test]
    fn readiness_records_map_from_the_trace() {
        let text = r#"{"kind":"turn_taking_availability","at_us":10,"policy":"v1_baseline","prospective_input_available":false,"hard_capture_authority_unchanged":true}
{"kind":"provider_ready","at_us":11,"at_us_upper":11,"at_us_role":"host_native","coordinator_observed_at_us":12,"link":{},"setup_us":5}
{"kind":"provider_unavailable","at_us":13,"link":{}}
{"kind":"provider_terminal","at_us":14,"link":{}}
{"kind":"turn_taking_availability","at_us":15,"prospective_input_available":true}
{"kind":"turn_taking_availability","at_us":16}"#;
        let (events, unmapped) = parse_trace(text).unwrap();
        // A record without the availability value is not guessed.
        assert_eq!(unmapped, 1);
        let kinds: Vec<_> = events.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                EventKind::InputAvailability { available: false },
                EventKind::ProviderLink {
                    state: "ready".into()
                },
                EventKind::ProviderLink {
                    state: "unavailable".into()
                },
                EventKind::ProviderLink {
                    state: "terminal".into()
                },
                EventKind::InputAvailability { available: true },
            ]
        );
        assert_eq!(events[0].at_us, Some(10));
        assert_eq!(events[1].at_us, Some(11));
        for event in &events {
            let round = from_trace(&to_trace(event, None)).unwrap();
            assert_eq!(round.kind, event.kind);
        }
    }

    #[test]
    fn proposed_readiness_cues_parse_strictly_and_carry_no_owner() {
        let boot = [7_u8; 16];
        let base = json!({"schema":1,"sequence":3,"boot":boot,"turn":null,"generation":null,
            "capture_epoch":1,"reference_epoch_context":1,"event_us":100,"sent_us":101,"expires_us":100_100});
        let cue = |extra: Value| {
            let mut value = base.clone();
            for (key, field) in extra.as_object().unwrap() {
                value[key] = field.clone();
            }
            parse_cue(&serde_json::to_vec(&value).unwrap())
        };
        let available = cue(json!({"kind":"input_availability","available":true})).unwrap();
        assert_eq!(
            available.event().unwrap().kind,
            EventKind::InputAvailability { available: true }
        );
        let link = cue(json!({"kind":"provider_link","link":"terminal"})).unwrap();
        assert_eq!(
            link.event().unwrap().kind,
            EventKind::ProviderLink {
                state: "terminal".into()
            }
        );
        let candidate = cue(json!({"kind":"input_candidate"})).unwrap();
        assert!(matches!(
            candidate.event().unwrap().kind,
            EventKind::InputCandidate { .. }
        ));
        for bad in [
            json!({"kind":"input_availability"}),
            json!({"kind":"input_availability","available":true,"turn":1,"generation":1}),
            json!({"kind":"provider_link"}),
            json!({"kind":"provider_link","link":"maybe"}),
            json!({"kind":"listening_ready","available":true}),
            json!({"kind":"listening_ready","link":"ready"}),
            json!({"kind":"input_candidate","turn":2,"generation":2}),
        ] {
            assert!(cue(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn candidate_and_ring_vocabulary_maps_with_turns_and_ids() {
        let (events, unmapped) = parse_trace(CANDIDATE_RECORDS).unwrap();
        assert_eq!(unmapped, 0);
        let controller = [1_u8; 16];
        let id = json!({"controller": controller, "serial": 4});
        assert_eq!(
            events[0].kind,
            EventKind::InputCandidate {
                candidate: id.clone()
            }
        );
        assert_eq!(
            events[1].kind,
            EventKind::InputAdmitted {
                prefix_first_read_us: Some(40),
                candidate: Some(id),
                basis: Some("directed_session_vad_only".into())
            }
        );
        assert!(
            matches!(&events[2].kind, EventKind::InputCandidateRejected { reason: Some(r), .. } if r == "deadline")
        );
        assert_eq!(
            events[3].kind,
            EventKind::RingRequested {
                phase: "listening".into()
            }
        );
        assert_eq!(
            events[3].turn,
            Some(2),
            "ring requests take the owner's turn"
        );
    }

    #[test]
    fn coordinator_vocabulary_maps_without_guessing() {
        let (events, unmapped) = parse_trace(COORDINATOR_RECORDS).unwrap();
        assert_eq!(unmapped, 1, "session_boot is not a scored event");
        let names: Vec<_> = events.iter().map(RuntimeEvent::name).collect();
        assert_eq!(
            names,
            [
                "run_start",
                "listening_ready",
                "input_admitted",
                "local_endpoint",
                "speaker_first_write",
                "turn_cancelled",
                "turn_completed",
                "input_transcript",
                "output_transcript",
                "runtime_fault",
                "run_end"
            ]
        );
        assert_eq!(
            events[5].kind,
            EventKind::TurnCancelled {
                reason: Some("user_interrupted".into()),
                provider_audio_seen: Some(true)
            }
        );
        assert_eq!(
            events[2].kind,
            EventKind::InputAdmitted {
                prefix_first_read_us: Some(5),
                candidate: None,
                basis: None
            }
        );
        assert_eq!(
            events[9].at_us, None,
            "a fault without a timestamp keeps none"
        );
        assert!(
            events
                .iter()
                .all(|e| e.domain == ClockDomain::LampMonotonic)
        );
    }

    #[test]
    fn fake_serialization_round_trips_through_the_trace_parser() {
        let boot = [2_u8; 16];
        let owner = json!({"boot": boot, "turn": 3, "generation": 4});
        for kind in [
            EventKind::InputAdmitted {
                prefix_first_read_us: Some(7),
                candidate: Some(json!({"serial": 1})),
                basis: Some("directed_session_vad_only".into()),
            },
            EventKind::InputCandidate {
                candidate: json!({"serial": 1}),
            },
            EventKind::RingRequested {
                phase: "speaking".into(),
            },
            EventKind::SpeakerFirstWrite,
            EventKind::SpeechRetired,
            EventKind::TurnCancelled {
                reason: Some("user_interrupted".into()),
                provider_audio_seen: Some(true),
            },
            EventKind::TurnCompleted {
                outcome: "audio_written_unscored".into(),
                playback_gaps: 0,
            },
            EventKind::OutputTranscript {
                text: "hi".into(),
                finished: true,
            },
            EventKind::RunEnd {
                status: Some("failed".into()),
                error: Some("x".into()),
            },
        ] {
            let mut event = RuntimeEvent::new(
                kind,
                Some(3),
                99,
                ClockDomain::LampMonotonic,
                EventSource::Trace,
            );
            // Only events whose wire vocabulary retains authority can round-trip it.
            if matches!(
                event.kind,
                EventKind::InputAdmitted { .. }
                    | EventKind::RingRequested { .. }
                    | EventKind::SpeakerFirstWrite
                    | EventKind::SpeechRetired
                    | EventKind::TurnCancelled { .. }
                    | EventKind::TurnCompleted { .. }
            ) {
                event.owner_generation = Some(4);
                // Completion records carry a generation but no owner boot.
                if !matches!(event.kind, EventKind::TurnCompleted { .. }) {
                    event.owner_boot = Some(boot);
                }
            }
            let parsed = from_trace(&to_trace(&event, Some(owner.clone()))).unwrap();
            assert_eq!(parsed, event);
        }
    }

    #[test]
    fn cues_require_schema_one_and_consistent_times() {
        let good = br#"{"schema":1,"sequence":2,"kind":"speaker_first_write","boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"turn":9,"generation":11,"capture_epoch":3,"reference_epoch_context":8,"event_us":100,"sent_us":101,"expires_us":100100}"#;
        let cue = parse_cue(good).unwrap();
        let event = cue.event().unwrap();
        assert_eq!(event.kind, EventKind::SpeakerFirstWrite);
        assert_eq!((event.turn, event.at_us), (Some(9), Some(100)));
        let stale = br#"{"schema":1,"sequence":2,"kind":"run_end","event_us":100,"sent_us":99,"expires_us":100100}"#;
        assert!(parse_cue(stale).is_err());
        let future = br#"{"schema":2,"sequence":2,"kind":"run_end","event_us":100,"sent_us":100,"expires_us":100100}"#;
        assert!(parse_cue(future).is_err());
    }
    #[test]
    fn directed_cues_preserve_owner_epochs_and_cancel_reason() {
        for (kind, name) in [
            ("input_admitted", "input_admitted"),
            ("local_endpoint", "local_endpoint"),
            ("cancelled", "turn_cancelled"),
        ] {
            let value = json!({"schema":1,"sequence":1,"kind":kind,"boot":vec![7;16],
                "turn":1,"generation":2,"capture_epoch":3,"reference_epoch_context":4,
                "event_us":10,"sent_us":12,"expires_us":100010,
                "reason":if kind == "cancelled" { Some("user_interrupted") } else { None }});
            let cue = parse_cue(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(
                (
                    cue.boot,
                    cue.turn,
                    cue.generation,
                    cue.capture_epoch,
                    cue.reference_epoch_context
                ),
                ([7; 16], Some(1), Some(2), Some(3), Some(4))
            );
            assert_eq!(cue.event().unwrap().name(), name);
            if kind == "cancelled" {
                assert!(
                    matches!(cue.event().unwrap().kind, EventKind::TurnCancelled { reason: Some(reason), .. } if reason == "user_interrupted")
                );
            }
            if kind == "cancelled" {
                let mut legacy = value.clone();
                legacy.as_object_mut().unwrap().remove("reason");
                assert!(matches!(
                    parse_cue(&serde_json::to_vec(&legacy).unwrap())
                        .unwrap()
                        .event()
                        .unwrap()
                        .kind,
                    EventKind::TurnCancelled { reason: None, .. }
                ));
            }
            for (field, bad) in [
                ("schema", json!(257)),
                ("sequence", json!(0)),
                ("boot", json!(vec![0; 16])),
                ("turn", Value::Null),
                ("generation", json!(0)),
                ("expires_us", json!(100011)),
                ("sent_us", json!(100010)),
                ("kind", json!("invented")),
            ] {
                let mut invalid = value.clone();
                invalid[field] = bad;
                assert!(
                    parse_cue(&serde_json::to_vec(&invalid).unwrap()).is_err(),
                    "{invalid}"
                );
            }
        }
    }
    #[test]
    fn completion_cues_require_all_terminal_fields_and_original_generation() {
        let good = json!({"schema":1,"sequence":8,"kind":"turn_completed","boot":vec![7;16],
            "turn":1,"generation":2,"event_us":100,"sent_us":101,"expires_us":100100,
            "outcome":"audio_written_unscored","playback_gaps":0});
        let parsed = parse_cue(&serde_json::to_vec(&good).unwrap()).unwrap();
        assert!(matches!(
            parsed.event().unwrap().kind,
            EventKind::TurnCompleted {
                playback_gaps: 0,
                ..
            }
        ));
        for field in ["outcome", "playback_gaps", "generation"] {
            let mut bad = good.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(
                parse_cue(&serde_json::to_vec(&bad).unwrap()).is_err(),
                "{field}"
            );
        }
        for (field, value) in [
            ("outcome", json!("invented")),
            ("playback_gaps", json!(-1)),
            ("generation", json!(0)),
            ("playback_sequence", json!(2)),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(
                parse_cue(&serde_json::to_vec(&bad).unwrap()).is_err(),
                "{bad}"
            );
        }
    }
}
