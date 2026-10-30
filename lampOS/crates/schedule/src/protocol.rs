//! One bounded JSON request and response per connection. Mutation guards belong
//! to the local caller, never to the language model.
use crate::time::When;
use serde::{Deserialize, Serialize};

pub const MAX_LINE_BYTES: usize = 8192;
pub const MAX_REPLY_BYTES: usize = 8192;
pub const DEFAULT_PAGE_SIZE: u16 = 8;
pub const MAX_PAGE_SIZE: u16 = 8;

/// A compare-and-set guard prevents a timed-out or reordered mutation from
/// being performed twice. Reusing the exact most recent mutation returns its
/// original durable receipt; otherwise the caller must read the current state.
// Saved exactly in the delivery ledger's Ack: a new field needs a `delivery::SCHEMA`
// bump, or an older build refuses the whole ledger instead of setting it aside.
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
    List {
        #[serde(default)]
        after_id: u64,
        #[serde(default = "default_page_size")]
        limit: u16,
        #[serde(default)]
        pending_only: bool,
    },
    /// [`Request::List`] with each item's set time, `set_local` and
    /// `set_day` (the person's call, 30 Oct: "when did I set that
    /// reminder?"). A request of its own, so `List`'s items stay exactly
    /// what the delivery ledger reads strictly; an older build refuses it as
    /// `bad_request`, and the caller lists without it.
    ListSet {
        #[serde(default)]
        after_id: u64,
        #[serde(default = "default_page_size")]
        limit: u16,
        #[serde(default)]
        pending_only: bool,
    },
    Get {
        id: u64,
    },
    Set {
        guard: Guard,
        text: String,
        when: When,
    },
    Cancel {
        guard: Guard,
        id: u64,
    },
    Snooze {
        guard: Guard,
        id: u64,
        delivery: u64,
        seconds: u64,
    },
    // Saved exactly in the delivery ledger: a new field needs a `delivery::SCHEMA`
    // bump, or an older build refuses the whole ledger instead of setting it aside.
    Ack {
        guard: Guard,
        id: u64,
        delivery: u64,
    },
}

const fn default_page_size() -> u16 {
    DEFAULT_PAGE_SIZE
}

impl Request {
    pub fn guard(&self) -> Option<&Guard> {
        match self {
            Self::Set { guard, .. }
            | Self::Cancel { guard, .. }
            | Self::Snooze { guard, .. }
            | Self::Ack { guard, .. } => Some(guard),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Status {} => "status",
            Self::List { .. } => "list",
            Self::ListSet { .. } => "list_set",
            Self::Get { .. } => "get",
            Self::Set { .. } => "set",
            Self::Cancel { .. } => "cancel",
            Self::Snooze { .. } => "snooze",
            Self::Ack { .. } => "ack",
        }
    }

    pub fn parse(line: &[u8]) -> Result<Self, &'static str> {
        if line.len() > MAX_LINE_BYTES {
            return Err("too_long");
        }
        serde_json::from_slice(line).map_err(|_| "bad_request")
    }
}

pub fn refusal(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": code, "message": message})
}
