//! `lamp-services`: the person's linked services on the lamp (Queue #4710).
//! V1's token connectors (Gmail, Google Calendar and Drive, Figma, GitHub,
//! Linear, Asana, Ahrefs, a Facebook Page) and its MCP ones (Slack,
//! monday.com, HubSpot, Amplitude) and Notion's, through `mcp.rs`, their
//! tokens kept by this one root process in `/var/lib/lampos-services`, never
//! in a log, the journal, the bus, the conversation or the model.
//!
//! Two sockets: the admin one, which only `lamp-cloud.service` may ask
//! (`set`, `set_local`, `remove`, `wipe`, `list`), and a tools folder for the voice
//! (`tools.json` and `call.sock`), which only `lamp-voice.service` may call
//! (and, with `LAMP_SKILL_READS=on`, `lamp-skills.service`, for reads alone).
//! Reads go through an allow-list per service; writes are behind their own
//! switch (`LAMP_SERVICES_WRITE=on`) and two phases: a full read-back first,
//! then the one send it names. Off unless `LAMP_SERVICES=on` (or `--enable`).
//! With `LAMP_SKILL_WRITES=on` too, `lamp-skills.service` may make a made
//! skill's one fixed write, approved at install ([`skill_writes`]). With
//! `LAMP_SKILL_TASKS=on`, `lamp-control.service` may read, or prepare a
//! write, for a skill's run under its grants (#6432). With
//! `LAMP_SKILL_SECRETS=on`, an uploaded skill's keys given in the app are
//! kept here too, and `lamp-tasks.service` may ask for a run's
//! ([`skill_secrets`]). With `LAMP_APPLE_HEALTH=on`, the phone app's
//! Apple Health summaries are kept here too, with no token, and read by
//! the voice for the owner's line as any linked service ([`health`]).
pub mod catalog;
pub mod health;
pub mod mail;
pub mod mcp;
pub mod mcp_server;
pub mod net;
pub mod reads;
pub mod refresh;
pub mod run_calls;
pub mod serve;
pub mod skill_secrets;
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
/// The switch for a skill's run on the lamp's Hermes (#6432), read by this
/// program, lamp-control and lamp-tasks: off unless `on`. On,
/// `lamp-control.service` may call the tools' socket too, for a run's
/// `service_read` and a `service_write` that is only prepared and held for
/// the person's spoken yes ([`serve::Services::run_tool`]).
pub const SKILL_TASKS_ENVIRONMENT: &str = "LAMP_SKILL_TASKS";
/// The switch for a live spoken ask of a linked service handed to the
/// lamp's agent (the owner's ruling, 17 Nov: connector work goes to
/// Hermes, never the second route's own model), read by lamp-voice,
/// lamp-tasks and lamp-control: off unless `on`, and only with
/// `LAMP_SERVICES`, `LAMP_AGENT_SERVICES` and `LAMP_SKILL_TASKS` on. Nothing
/// of this program's changes with it: the agent's run calls the tools'
/// socket through lamp-control as any skill run's does, under
/// [`VOICE_ASK_SKILL`].
pub const SERVICES_TO_AGENT_ENVIRONMENT: &str = "LAMP_SERVICES_TO_AGENT";
/// The skill name a live ask's run carries (lamp-control's token, this
/// program's run calls and the skills' write bounds): no skill folder is
/// ever named so, and [`skill_writes::skill_name_ok`] takes it.
pub const VOICE_ASK_SKILL: &str = "voice-ask";
/// The switch for changing and cancelling a Google Calendar event by its
/// id (`update_event`, `delete_event`), read by this program: off unless
/// `on`, and only with [`WRITE_ENVIRONMENT`] on too.
pub const CALENDAR_EDITS_ENVIRONMENT: &str = "LAMP_CALENDAR_EDITS";
/// The switch for a Google Calendar read searching by the person's words
/// over a bounded window, so an event from an earlier conversation can be
/// found to move or cancel (tracker row C132), read by this program: off
/// unless `on`, and only with [`CALENDAR_EDITS_ENVIRONMENT`] on too.
pub const CALENDAR_EVENT_SEARCH_ENVIRONMENT: &str = "LAMP_CALENDAR_EVENT_SEARCH";
/// The switch for changing and deleting a saved Gmail draft by its id
/// (`update_draft`, `delete_draft`), read by this program: off unless
/// `on`, and only with [`WRITE_ENVIRONMENT`] on too.
pub const DRAFT_EDITS_ENVIRONMENT: &str = "LAMP_GMAIL_DRAFT_EDITS";

/// The switch for the lamp's agent's connectors (Hermes's MCP server
/// `lamp-services mcp`, [`mcp_server`]), read by lamp-tasks, which adds
/// the server to Hermes's `mcp_servers` with it on and takes it out with it
/// off: off unless `on`. The server opens no door: a skill's run reaches
/// the services only through lamp-control with its one-run token's
/// `<code>-read` / `<code>-write` grants, as [`SKILL_TASKS_ENVIRONMENT`]
/// has it.
pub const AGENT_SERVICES_ENVIRONMENT: &str = "LAMP_AGENT_SERVICES";

/// The switch for the voice's tools naming only the linked services
/// (#6218), read by this program: off unless `on`, when every service the
/// lamp can link is named, linked or not.
pub const LINKED_TOOLS_ENVIRONMENT: &str = "LAMP_SERVICES_LINKED_TOOLS";

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
