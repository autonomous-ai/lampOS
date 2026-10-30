//! The lamp's timers and reminders for the app's Lamp 2 page (#2195, Mobile
//! on #1396), off unless `LAMP_APP_SCHEDULE=on`.
//!
//! lamp-schedule keeps them (`/run/lampos-schedule/call.sock`, the parts'
//! own user only). `schedule.list` reads them a page at a time, and
//! `schedule.cancel` cancels one, through the same client the voice's
//! `list_reminders` and `cancel_reminder` go through
//! (`lamp_actions::schedule::Adapter`): it reads the store's identity
//! before a change, checks every page, and its failures are fixed sentences
//! that never carry a reminder's words. The reminders are the household's,
//! as anyone at the lamp can list or cancel them by voice; with the switch
//! the owner's app can too, and their words pass through the broker to it.
use lamp_actions::schedule::Adapter;
use serde_json::{Value, json};
use std::path::Path;

/// The switch, in lamp-cloud's unit: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_APP_SCHEDULE";
/// Where lamp-schedule is asked, as for lamp-actions; its own default when
/// unset.
pub const SOCKET_ENVIRONMENT: &str = "LAMP_SCHEDULE_SOCKET";

/// The client, with the switch on and a usable socket path; none otherwise,
/// a bad path said once.
pub fn from_environment() -> Option<Adapter> {
    let switch = std::env::var_os(ENVIRONMENT);
    let socket = std::env::var_os(SOCKET_ENVIRONMENT);
    match Adapter::configured(switch.as_deref(), socket.as_deref().map(Path::new)) {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!(
                "lamp-cloud: {ENVIRONMENT}=on but the schedule's socket can't be used: {error}"
            );
            None
        }
    }
}

/// `schedule.list`: `{"after_id"?: n}`, a page of at most eight, each as the
/// lamp keeps it (`id`, `kind`, `text`, `state`, `local`, `due_unix_ms`,
/// `repeat`, `snoozed`, `late`), with `next_after_id` for the next page and
/// `complete` when there is none.
pub fn list(adapter: &Adapter, data: &Value) -> Result<Option<Value>, String> {
    let after_id = match data.get("after_id") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().ok_or("after_id must be a whole number")?,
    };
    let page = answered(adapter.call_plain("list_reminders", &json!({"after_id": after_id})))?;
    Ok(Some(json!({
        "items": page["items"],
        "next_after_id": page["next_after_id"],
        "complete": page["complete"],
    })))
}

/// `schedule.cancel`: `{"id": n}`, the timer or the whole reminder series
/// cancelled: `{"cancelled": true, "id": n}`, or why not, in the person's
/// words (no longer active, changed meanwhile: list again, or not confirmed:
/// it may have happened).
pub fn cancel(adapter: &Adapter, data: &Value) -> Result<Option<Value>, String> {
    let id = data
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or("id must be a whole number above 0")?;
    answered(adapter.call("cancel_reminder", &json!({"id": id})))?;
    Ok(Some(json!({"cancelled": true, "id": id})))
}

/// The client's envelope: its result, or its fixed sentence.
fn answered(reply: Value) -> Result<Value, String> {
    if reply["ok"] == true {
        Ok(reply["result"].clone())
    } else {
        Err(reply["error"]
            .as_str()
            .unwrap_or("the reminder store could not be used just now")
            .to_owned())
    }
}
