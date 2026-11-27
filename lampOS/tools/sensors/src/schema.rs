use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, String>;
pub const V2_RUBRIC: &str = "sensor-team-2026-11-27-v2";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Sensor {
    Call,
    Addressed,
    Direction,
    Head,
    SpeakingFace,
    VoiceMatch,
    BargeIn,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataOrigin {
    RealPeople,
    Generated,
    Public,
    Invented,
}

impl DataOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RealPeople => "real_people",
            Self::Generated => "generated",
            Self::Public => "public",
            Self::Invented => "invented",
        }
    }
}

impl Sensor {
    pub fn parse(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned()))
            .map_err(|_| format!("unknown sensor: {value}"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub split: Split,
    pub started_unix_s: u64,
    pub consent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_after_unix_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_origin: Option<DataOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_verified: Option<bool>,
    pub clock_id: String,
    pub sync_error_us: u64,
    pub recording_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Development,
    Holdout,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub id: String,
    pub session_id: String,
    pub speaker_id: String,
    pub source: String,
    pub start_us: u64,
    pub end_us: u64,
    pub call_end_us: Option<u64>,
    pub last_lamp_audio_end_us: Option<u64>,
    pub recipient: String,
    pub script_id: String,
    pub look_target: String,
    pub head_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shoulders_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lamp_audio_start_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_lamp_audio_end_us: Option<u64>,
    pub bearing_deg: f64,
    pub distance_mm: u32,
    pub light: String,
    pub noise: String,
    pub visible_faces: Vec<String>,
    pub subject_face_id: Option<String>,
    pub subject_visible: bool,
    pub eyes_epoch: Option<String>,
    pub overlap: bool,
    pub first_use: bool,
    pub condition: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub sensor: Sensor,
    pub trial_id: String,
    pub reference_trial_id: Option<String>,
    pub expected: String,
    pub fault: bool,
    pub opportunity_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_gold_adjudicated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_end_us: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rubric_id: Option<String>,
    pub collection_id: String,
    pub frozen_unix_s: u64,
    pub sessions: Vec<Session>,
    pub trials: Vec<Trial>,
    pub cases: Vec<Case>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub case_id: String,
    pub clock_id: String,
    pub output: String,
    pub input_end_us: u64,
    pub ready_us: Option<u64>,
    pub replay_elapsed_us: Option<u64>,
    pub first_admission_us: Option<u64>,
    pub asr_final_us: Option<u64>,
    pub associated_us: Option<u64>,
    pub frame_work_us: Option<u64>,
    pub bearing_deg: Option<f64>,
    pub framed_speaker_id: Option<String>,
    pub face_in_frame_us: Option<u64>,
    pub turns: Option<u32>,
    pub wake_count: Option<u32>,
    pub added_wait_us: u64,
    pub held_line: bool,
    pub safety_violation: bool,
    pub subject_face_id: Option<String>,
    pub eyes_epoch: Option<String>,
    pub reason: Option<String>,
    pub natural_motion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_lamp_audio_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_until_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_stopped: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub version: u32,
    pub manifest_sha256: String,
    pub sensor: Sensor,
    pub build_sha: String,
    pub producer_binary_sha256: Option<String>,
    pub source_patch_sha256: Option<String>,
    pub model_sha256: String,
    pub config_sha256: String,
    pub evidence: String,
    pub location: String,
    pub predictions: Vec<Prediction>,
}

pub fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
}

pub fn sha(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|c| c.is_ascii_hexdigit())
}

fn one_of(value: &str, allowed: &[&str]) -> bool {
    allowed.contains(&value)
}

impl Manifest {
    pub fn trial(&self, id: &str) -> &Trial {
        self.trials
            .iter()
            .find(|t| t.id == id)
            .expect("validated trial")
    }

    pub fn session(&self, id: &str) -> &Session {
        self.sessions
            .iter()
            .find(|s| s.id == id)
            .expect("validated session")
    }

    pub fn validate(&self) -> Result<()> {
        if ![1, 2].contains(&self.version)
            || !token(&self.collection_id)
            || self.frozen_unix_s == 0
            || (self.version == 1 && self.rubric_id.is_some())
            || (self.version == 2 && self.rubric_id.as_deref() != Some(V2_RUBRIC))
        {
            return Err(
                "manifest needs version 1 or the explicit v2 rubric, collection ID and freeze time"
                    .into(),
            );
        }
        let mut sessions = BTreeSet::new();
        for s in &self.sessions {
            if !token(&s.id)
                || !sessions.insert(&s.id)
                || !token(&s.clock_id)
                || !s.consent
                || (self.version == 1
                    && (s
                        .delete_after_unix_s
                        .is_none_or(|at| at <= s.started_unix_s)
                        || s.retention.is_some()
                        || s.inventory_sha256.is_some()
                        || s.readme_sha256.is_some()
                        || s.data_origin.is_some()
                        || s.clock_verified.is_some()))
                || (self.version == 2
                    && (s.delete_after_unix_s.is_some()
                        || s.retention.as_deref() != Some("keep_all")
                        || !s.inventory_sha256.as_ref().is_some_and(|v| sha(v, 64))
                        || !s.readme_sha256.as_ref().is_some_and(|v| sha(v, 64))
                        || s.data_origin.is_none()))
                || !sha(&s.recording_sha256, 64)
            {
                return Err(format!(
                    "invalid/duplicate session, consent or retention: {}",
                    s.id
                ));
            }
            if s.split == Split::Holdout && s.started_unix_s < self.frozen_unix_s {
                return Err(format!("holdout predates freeze: {}", s.id));
            }
        }
        let mut trials = BTreeSet::new();
        let mut captures = BTreeSet::new();
        let mut development = BTreeSet::new();
        let mut holdout = BTreeSet::new();
        for t in &self.trials {
            if !token(&t.id)
                || !trials.insert(&t.id)
                || !sessions.contains(&t.session_id)
                || !token(&t.speaker_id)
                || !token(&t.script_id)
                || t.start_us >= t.end_us
                || !captures.insert((&t.session_id, t.start_us, t.end_us))
                || !t.bearing_deg.is_finite()
                || !(-180.0..=180.0).contains(&t.bearing_deg)
                || t.distance_mm == 0
                || (self.version == 1 && t.visible_faces.len() > 2)
                || (self.version == 1
                    && (t.shoulders_target.is_some()
                        || t.lamp_audio_start_us.is_some()
                        || t.planned_lamp_audio_end_us.is_some()))
                || t.shoulders_target
                    .as_deref()
                    .is_some_and(|v| !one_of(v, &["lamp", "away", "unknown"]))
                || t.lamp_audio_start_us
                    .zip(t.planned_lamp_audio_end_us)
                    .is_some_and(|(start, end)| start >= end)
                || t.lamp_audio_start_us.is_some() != t.planned_lamp_audio_end_us.is_some()
                || t.visible_faces
                    .iter()
                    .any(|s| !token(s) || s == "unknown" || s == "no_visible")
                || t.visible_faces.iter().collect::<BTreeSet<_>>().len() != t.visible_faces.len()
                || t.subject_face_id
                    .as_ref()
                    .is_some_and(|id| !t.visible_faces.contains(id))
                || (t.subject_visible != t.subject_face_id.is_some())
                || t.eyes_epoch.as_ref().is_some_and(|id| !token(id))
                || !(one_of(&t.source, &["person", "tv", "lamp"])
                    || (self.version == 2 && t.source == "noise"))
                || !one_of(
                    &t.recipient,
                    &["lamp", "person", "phone", "none", "unknown"],
                )
                || !one_of(
                    &t.look_target,
                    &["lamp", "screen", "phone", "other", "unknown"],
                )
                || !(one_of(
                    &t.head_target,
                    &["lamp", "screen", "phone", "other", "unknown"],
                ) || (self.version == 2 && t.head_target == "away"))
                || !one_of(&t.light, &["daylight", "lamplight"])
                || !one_of(&t.noise, &["quiet", "background"])
                || !one_of(&t.condition, &["normal", "slow", "disconnected"])
                || t.call_end_us
                    .is_some_and(|v| v < t.start_us || v > t.end_us)
                || t.last_lamp_audio_end_us.is_some_and(|v| v > t.start_us)
            {
                return Err(format!("invalid/duplicate trial or capture: {}", t.id));
            }
            if t.source == "person" {
                match self.session(&t.session_id).split {
                    Split::Development => {
                        development.insert(&t.speaker_id);
                    }
                    Split::Holdout => {
                        holdout.insert(&t.speaker_id);
                    }
                }
            }
        }
        if !development.is_disjoint(&holdout) {
            return Err("development and holdout speakers overlap".into());
        }
        for session in &self.sessions {
            let mut intervals: Vec<_> = self
                .trials
                .iter()
                .filter(|t| t.session_id == session.id)
                .collect();
            intervals.sort_by_key(|t| t.start_us);
            if intervals.windows(2).any(|w| w[0].end_us > w[1].start_us) {
                return Err(format!(
                    "overlapping/reused capture intervals: {}",
                    session.id
                ));
            }
        }
        let mut cases = BTreeSet::new();
        let mut per_sensor = BTreeSet::new();
        let mut used_pairs = BTreeSet::new();
        for c in &self.cases {
            if !token(&c.id)
                || !cases.insert(&c.id)
                || !trials.contains(&c.trial_id)
                || !per_sensor.insert((c.sensor, &c.trial_id))
            {
                return Err(format!("invalid/duplicate case or sensor trial: {}", c.id));
            }
            let t = self.trial(&c.trial_id);
            if c.opportunity_us
                .is_some_and(|at| at < t.start_us || at > t.end_us)
            {
                return Err(format!("decision opportunity outside capture: {}", c.id));
            }
            if (self.version == 1
                && (c.sensor == Sensor::BargeIn
                    || c.head_gold_adjudicated.is_some()
                    || c.observation_end_us.is_some()))
                || (c.sensor != Sensor::Head && c.head_gold_adjudicated.is_some())
                || (c.sensor != Sensor::BargeIn && c.observation_end_us.is_some())
            {
                return Err(format!(
                    "v2-only field or field on another sensor: {}",
                    c.id
                ));
            }
            let valid = match c.sensor {
                Sensor::Call => one_of(&c.expected, &["call", "non_call"]),
                Sensor::Addressed => {
                    one_of(&c.expected, &["lamp", "room"])
                        && t.last_lamp_audio_end_us
                            .is_some_and(|end| t.start_us - end <= 20_000_000)
                        && (c.expected == "lamp") == (t.recipient == "lamp")
                }
                Sensor::Direction => {
                    one_of(&c.expected, &["turn", "no_turn"])
                        && (c.expected != "turn" || t.call_end_us.is_some())
                }
                Sensor::Head => {
                    let truth_valid = if self.version == 1 {
                        one_of(&c.expected, &["lamp", "screen", "phone", "unknown"])
                            && c.expected == t.head_target
                    } else {
                        let head = t.head_target.as_str();
                        let shoulders = t.shoulders_target.as_deref().unwrap_or("");
                        let observed =
                            one_of(head, &["lamp", "away"]) && one_of(shoulders, &["lamp", "away"]);
                        let derived = if head == "lamp" && shoulders == "lamp" {
                            "facing"
                        } else {
                            "away"
                        };
                        one_of(head, &["lamp", "away", "unknown"])
                            && one_of(shoulders, &["lamp", "away", "unknown"])
                            && if observed {
                                c.expected == derived
                            } else if c.head_gold_adjudicated == Some(true) {
                                one_of(&c.expected, &["facing", "away"])
                            } else {
                                c.expected == "unknown" && c.fault
                            }
                    };
                    truth_valid
                        && c.opportunity_us.is_some()
                        && (c.expected == "unknown"
                            || (t.subject_visible && t.eyes_epoch.is_some()))
                }
                Sensor::SpeakingFace => {
                    c.expected == "unknown"
                        || (c.expected == "no_visible" && !t.subject_visible)
                        || (t.visible_faces.contains(&c.expected)
                            && t.subject_face_id.as_ref() == Some(&c.expected)
                            && t.eyes_epoch.is_some())
                }
                Sensor::VoiceMatch => {
                    let reference = c
                        .reference_trial_id
                        .as_ref()
                        .and_then(|id| self.trials.iter().find(|t| &t.id == id))
                        .ok_or_else(|| format!("missing pair reference: {}", c.id))?;
                    let same = reference.source == "person"
                        && t.source == "person"
                        && reference.speaker_id == t.speaker_id;
                    let a = self.session(&reference.session_id);
                    let b = self.session(&t.session_id);
                    let mut pair = [reference.id.as_str(), t.id.as_str()];
                    pair.sort_unstable();
                    one_of(&c.expected, &["same", "different"])
                        && reference.source == "person"
                        && reference.recipient == "lamp"
                        && reference.call_end_us.is_some()
                        && (c.expected == "same") == same
                        && reference.id != t.id
                        && a.split == b.split
                        && used_pairs.insert(pair)
                        && (!same
                            || (a.id != b.id
                                && a.started_unix_s.abs_diff(b.started_unix_s) >= 7200))
                }
                Sensor::BargeIn => {
                    self.version == 2
                        && one_of(
                            &c.expected,
                            &["interrupt", "backchannel", "side_talk", "noise"],
                        )
                        && t.overlap
                        && t.lamp_audio_start_us.is_some_and(|at| at < t.start_us)
                        && t.planned_lamp_audio_end_us
                            .is_some_and(|at| at > t.start_us)
                        && c.observation_end_us.is_some_and(|at| {
                            at >= t.end_us && at >= t.planned_lamp_audio_end_us.unwrap_or(u64::MAX)
                        })
                }
            };
            if !valid || (c.sensor != Sensor::VoiceMatch && c.reference_trial_id.is_some()) {
                return Err(format!("invalid truth, pair or window: {}", c.id));
            }
        }
        Ok(())
    }
}

impl Run {
    pub fn validate(&self, manifest: &Manifest, hash: &str) -> Result<()> {
        if self.version != manifest.version
            || self.manifest_sha256 != hash
            || !sha(&self.build_sha, 40)
            || self
                .producer_binary_sha256
                .as_ref()
                .is_some_and(|v| !sha(v, 64))
            || self
                .source_patch_sha256
                .as_ref()
                .is_some_and(|v| !sha(v, 64))
            || (self.source_patch_sha256.is_some() && self.producer_binary_sha256.is_none())
            || !sha(&self.model_sha256, 64)
            || !sha(&self.config_sha256, 64)
            || !one_of(&self.evidence, &["lamp_live", "replay", "synthetic"])
            || !one_of(&self.location, &["local", "server"])
        {
            return Err("invalid run identity, evidence, location or manifest hash".into());
        }
        if self.version == 1 && self.sensor == Sensor::BargeIn {
            return Err("barge_in requires the v2 rubric".into());
        }
        let cases: BTreeMap<_, _> = manifest.cases.iter().map(|c| (&c.id, c)).collect();
        let mut ids = BTreeSet::new();
        for p in &self.predictions {
            let c = cases
                .get(&p.case_id)
                .ok_or_else(|| format!("unknown prediction ID: {}", p.case_id))?;
            let t = manifest.trial(&c.trial_id);
            if c.sensor != self.sensor
                || p.clock_id != manifest.session(&t.session_id).clock_id
                || !ids.insert(&p.case_id)
                || !token(&p.output)
                || (self.evidence == "replay" && p.ready_us.is_some())
                || (self.version == 1 && self.evidence != "replay" && p.ready_us.is_none())
                || (self.evidence != "replay" && p.replay_elapsed_us.is_some())
                || (self.evidence == "replay"
                    && (p.associated_us.is_some()
                        || p.face_in_frame_us.is_some()
                        || p.first_admission_us.is_some()
                        || p.last_lamp_audio_us.is_some()
                        || p.observed_until_us.is_some()
                        || p.playback_stopped.is_some()))
                || ((self.version == 1 || self.sensor != Sensor::BargeIn)
                    && (p.last_lamp_audio_us.is_some()
                        || p.observed_until_us.is_some()
                        || p.playback_stopped.is_some()))
                || p.last_lamp_audio_us.is_some_and(|at| {
                    at < t.start_us || p.observed_until_us.is_some_and(|until| at > until)
                })
                || p.observed_until_us.is_some_and(|at| at < t.start_us)
                || p.ready_us.is_some_and(|at| at < p.input_end_us)
                || p.first_admission_us.is_some_and(|at| at < t.end_us)
                || p.bearing_deg
                    .is_some_and(|v| !v.is_finite() || !(-180.0..=180.0).contains(&v))
                || p.asr_final_us
                    .is_some_and(|v| v < t.end_us || p.ready_us.is_some_and(|at| v > at))
                || (self.sensor == Sensor::Addressed
                    && p.asr_final_us.is_some_and(|v| p.input_end_us != v))
                || p.associated_us
                    .is_some_and(|v| v < t.end_us || p.ready_us.is_some_and(|at| v < at))
                || p.face_in_frame_us
                    .is_some_and(|v| v < t.call_end_us.unwrap_or(t.start_us))
                || p.framed_speaker_id.as_ref().is_some_and(|s| !token(s))
                || p.subject_face_id.as_ref().is_some_and(|s| !token(s))
                || p.eyes_epoch.as_ref().is_some_and(|s| !token(s))
                || p.reason.as_ref().is_some_and(|s| !token(s))
            {
                return Err(format!(
                    "invalid/duplicate prediction or timestamp: {}",
                    p.case_id
                ));
            }
        }
        Ok(())
    }
}
