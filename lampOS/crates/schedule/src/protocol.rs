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

/// Who a voice line is, for whose reminders it hears and may change.
/// A reminder set by a known person is theirs alone; one set with no sure
/// person belongs to the room, including items stored before setters.
/// A request without an asker, from the app or delivery reader, sees all.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Asker {
    /// The line's memory person id (`p-` and 16 hex digits), when its
    /// reads are a person's: `None` is the room's.
    #[serde(default)]
    pub person: Option<String>,
    /// The line is the owner's own. Retained for wire compatibility; room
    /// items no longer require an owner, and owned items require their setter.
    #[serde(default)]
    pub owner: bool,
}

impl Asker {
    /// Whether this line hears of, and may change, a reminder `set_by`.
    pub fn may_see(&self, set_by: Option<&str>) -> bool {
        match set_by {
            Some(person) => self.person.as_deref() == Some(person),
            None => true,
        }
    }
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
    ///
    /// With `asker`, only the items that line may see ([`Asker::may_see`]),
    /// and `not_yet_due`, how many of those are set and not yet due.
    ListSet {
        #[serde(default)]
        after_id: u64,
        #[serde(default = "default_page_size")]
        limit: u16,
        #[serde(default)]
        pending_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asker: Option<Asker>,
    },
    Get {
        id: u64,
    },
    /// `person`: who set it, a memory person id; none when nobody is sure.
    Set {
        guard: Guard,
        text: String,
        when: When,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        person: Option<String>,
    },
    /// With `asker`, an item that line may not see is `not_found`.
    Cancel {
        guard: Guard,
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asker: Option<Asker>,
    },
    /// With `asker`, an item that line may not see is `not_found`.
    Snooze {
        guard: Guard,
        id: u64,
        delivery: u64,
        seconds: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asker: Option<Asker>,
    },
    /// Every reminder and timer `person` set, cancelled: their "forget
    /// everything about me", or the app removing them entirely.
    Forget {
        guard: Guard,
        person: String,
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
            | Self::Forget { guard, .. }
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
            Self::Forget { .. } => "forget",
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
