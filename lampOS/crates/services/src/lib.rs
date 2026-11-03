//! `lamp-services`: the person's linked services on the lamp (Queue #4710).
//! V1's token connectors (Gmail, Google Calendar and Drive, Notion, Figma,
//! GitHub, Linear, Asana, Ahrefs, a Facebook Page) and its MCP ones (Slack,
//! monday.com, HubSpot, Amplitude, through `mcp.rs`), their tokens kept by
//! this one root process in `/var/lib/lampos-services`, never in a log, the
//! journal, the bus, the conversation or the model.
//!
//! Two sockets: the admin one, which only `lamp-cloud.service` may ask
//! (`set`, `remove`, `wipe`, `list`), and a tools folder for the voice
//! (`tools.json` and `call.sock`), which only `lamp-voice.service` may call.
//! Reads go through an allow-list per service; writes are behind their own
//! switch (`LAMP_SERVICES_WRITE=on`) and two phases: a full read-back first,
//! then the one send it names. Off unless started with `--enable`.
pub mod catalog;
pub mod mail;
pub mod mcp;
pub mod net;
pub mod reads;
pub mod refresh;
pub mod serve;
pub mod store;
pub mod tools;
pub mod writes;

/// The switch for reads, on the voice and lamp-cloud (this program is
/// closed by `--enable` and its unit instead).
pub const ENVIRONMENT: &str = "LAMP_SERVICES";
/// The switch for writes, read by this program: off unless `on`.
pub const WRITE_ENVIRONMENT: &str = "LAMP_SERVICES_WRITE";

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
