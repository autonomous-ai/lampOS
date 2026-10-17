//! Bounded local requests. Trusted Voice attestations use the same envelope,
//! but their authority comes from the runtime route, never a JSON field.
use serde::{Deserialize, Serialize};

pub const MAX_LINE_BYTES: usize = 8192;
pub const MAX_REPLY_BYTES: usize = 131_072;
pub const MAX_PAGE_SIZE: u16 = 8;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub store_id: String,
    pub revision: u64,
    pub request_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status {},
    /// Who work can be given to: each target's alias and engine, never its
    /// machine or project. The voice tells the model of them.
    Targets {},
    List {
        #[serde(default)]
        after_id: u64,
        #[serde(default = "page_size")]
        limit: u16,
    },
    GetTask {
        id: u64,
    },
    GetObservation {
        id: u64,
        observation_id: u64,
    },
    /// The agent's progress line and what was read with it, kept in memory
    /// with `serve --progress`: for the lamp about to say it.
    GetProgress {
        id: u64,
    },
    Prepare {
        guard: Guard,
        text: String,
        target: String,
    },
    Confirm {
        guard: Guard,
        id: u64,
        prepared_digest: String,
        session_id: String,
    },
    Cancel {
        guard: Guard,
        id: u64,
    },
    Observe {
        guard: Guard,
        id: u64,
    },
    ClaimSpeech {
        guard: Guard,
        id: u64,
        observation_id: u64,
        text: String,
        session_id: String,
    },
    AckSpeech {
        guard: Guard,
        id: u64,
        observation_id: u64,
        claim_id: u64,
        text_digest: String,
        session_id: String,
        played: bool,
    },
    /// The voice told this observation's news (#1390): the model said it in
    /// its own words, so no exact text is claimed. Its harness may then be
    /// closed (`serve --close-told`).
    Told {
        guard: Guard,
        id: u64,
        observation_id: u64,
        session_id: String,
    },
}

const fn page_size() -> u16 {
    MAX_PAGE_SIZE
}

impl Request {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_LINE_BYTES {
            return Err("too_long");
        }
        serde_json::from_slice(bytes).map_err(|_| "bad_request")
    }

    pub fn guard(&self) -> Option<&Guard> {
        match self {
            Self::Prepare { guard, .. }
            | Self::Confirm { guard, .. }
            | Self::Cancel { guard, .. }
            | Self::Observe { guard, .. }
            | Self::ClaimSpeech { guard, .. }
            | Self::AckSpeech { guard, .. }
            | Self::Told { guard, .. } => Some(guard),
            _ => None,
        }
    }

    pub fn trusted(&self) -> bool {
        matches!(
            self,
            Self::Confirm { .. }
                | Self::ClaimSpeech { .. }
                | Self::AckSpeech { .. }
                | Self::Told { .. }
        )
    }

    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Confirm { session_id, .. }
            | Self::ClaimSpeech { session_id, .. }
            | Self::AckSpeech { session_id, .. }
            | Self::Told { session_id, .. } => Some(session_id),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Status {} => "status",
            Self::Targets {} => "targets",
            Self::List { .. } => "list",
            Self::GetTask { .. } => "get_task",
            Self::GetObservation { .. } => "get_observation",
            Self::GetProgress { .. } => "get_progress",
            Self::Prepare { .. } => "prepare",
            Self::Confirm { .. } => "confirm",
            Self::Cancel { .. } => "cancel",
            Self::Observe { .. } => "observe",
            Self::ClaimSpeech { .. } => "claim_speech",
            Self::AckSpeech { .. } => "ack_speech",
            Self::Told { .. } => "told",
        }
    }
}

pub fn refusal(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"ok":false,"error":code,"message":message})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_have_no_authority_or_target_override_fields() {
        for bytes in [
            br#"{"op":"status","trusted":true}"#.as_slice(),
            br#"{"op":"get_task","id":1,"machine_id":"other"}"#,
            br#"{"op":"list","limit":1,"engine":"claude"}"#,
            br#"{"op":"targets","engine":"codex"}"#,
            br#"{"op":"get_progress","id":1,"about":"anything"}"#,
            br#"{"op":"get_progress"}"#,
        ] {
            assert_eq!(Request::parse(bytes), Err("bad_request"));
        }
        let targets = Request::parse(br#"{"op":"targets"}"#).unwrap();
        assert_eq!(targets, Request::Targets {});
        assert!(targets.guard().is_none() && !targets.trusted());
        assert_eq!(targets.name(), "targets");
        let progress = Request::parse(br#"{"op":"get_progress","id":3}"#).unwrap();
        assert_eq!(progress, Request::GetProgress { id: 3 });
        assert!(!progress.trusted() && progress.guard().is_none());
        assert_eq!(progress.name(), "get_progress");
        assert_eq!(
            Request::parse(br#"{"op":"list"}"#).unwrap(),
            Request::List {
                after_id: 0,
                limit: MAX_PAGE_SIZE
            }
        );
    }

    #[test]
    fn trusted_operations_require_explicit_fields_and_a_separate_route() {
        let request = Request::parse(br#"{"op":"confirm","guard":{"store_id":"s","revision":0,"request_id":"r"},"id":1,"prepared_digest":"d","session_id":"s"}"#).unwrap();
        assert!(request.trusted());
        assert_eq!(request.session_id(), Some("s"));
        assert_eq!(request.name(), "confirm");
        assert_eq!(
            Request::parse(br#"{"op":"confirm","id":1}"#),
            Err("bad_request")
        );
    }

    #[test]
    fn malformed_duplicate_and_oversized_input_is_refused() {
        for bytes in [
            br#"{"op":"status","op":"list"}"#.as_slice(),
            br#"{"op":"get_task","id":-1}"#,
            br#"{"op":"status"}{}"#,
            br#"{"op":"list","limit":65536}"#,
            &[0xff],
        ] {
            assert_eq!(Request::parse(bytes), Err("bad_request"));
        }
        assert_eq!(
            Request::parse(&vec![b' '; MAX_LINE_BYTES + 1]),
            Err("too_long")
        );
    }
}
