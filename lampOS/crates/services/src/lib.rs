//! `lamp-services`: the person's linked services on the lamp (Queue #4710).
//! V1's token connectors (Gmail, Google Calendar and Drive, Figma, GitHub,
//! Linear, Asana, Ahrefs, a Facebook Page) and its MCP ones (Slack,
//! monday.com, HubSpot, Amplitude) and Notion's, through `mcp.rs`, their
//! tokens kept by this one root process in `/var/lib/lampos-services`, never
//! in a log, the journal, the bus, the conversation or the model.
//!
//! Two sockets: the admin one, which only `lamp-cloud.service` may ask
//! (`set`, `remove`, `wipe`, `list`), and a tools folder for the voice
//! (`tools.json` and `call.sock`), which only `lamp-voice.service` may call
//! (and, with `LAMP_SKILL_READS=on`, `lamp-skills.service`, for reads alone).
//! Reads go through an allow-list per service; writes are behind their own
//! switch (`LAMP_SERVICES_WRITE=on`) and two phases: a full read-back first,
//! then the one send it names. Off unless `LAMP_SERVICES=on` (or `--enable`).
//! With `LAMP_SKILL_WRITES=on` too, `lamp-skills.service` may make a made
//! skill's one fixed write, approved at install ([`skill_writes`]).
pub mod catalog;
pub mod mail;
pub mod mcp;
pub mod net;
pub mod reads;
pub mod refresh;
pub mod serve;
pub mod skill_writes;
pub mod spoken;
pub mod store;
pub mod tools;
pub mod writes;

/// The switch for the linked services, on the voice, lamp-cloud and this
/// program: each is off unless `on` (this program also with `--enable`).
pub const ENVIRONMENT: &str = "LAMP_SERVICES";
/// The switch for writes, read by this program: off unless `on`.
pub const WRITE_ENVIRONMENT: &str = "LAMP_SERVICES_WRITE";
/// The switch for a made skill's reads (#5288), read by this program,
/// `lamp-skills`, its manage helper and the voice: off unless `on`. On,
/// `lamp-skills.service` may call the tools' socket too, for `service_read`
/// alone.
pub const SKILL_READS_ENVIRONMENT: &str = "LAMP_SKILL_READS";
/// The switch for changing and cancelling a Google Calendar event by its
/// id (`update_event`, `delete_event`), read by this program: off unless
/// `on`, and only with [`WRITE_ENVIRONMENT`] on too.
pub const CALENDAR_EDITS_ENVIRONMENT: &str = "LAMP_CALENDAR_EDITS";

/// Whether a switch is on, as `var` gives it.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// Seconds since the epoch, now.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}
