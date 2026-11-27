//! What a running app that holds `turns` is told of the lamp's turns (#8726,
//! `docs/apps.md`, "Lamp Doctor's grants"): the voice's record of each
//! finished turn on `voice/turn_record`, as the event `turns.finished`. The
//! record has no words, a speaker or a name (`lamp_msgs::doctor::TurnRecord`);
//! it goes only to a running app whose installed manifest holds `turns`
//! ([`crate::server::Shared::offer_turn`]). Behind `LAMP_STORE_APPS` like the
//! app socket.
use crate::server::Shared;
use lamp_msgs::doctor::TurnRecord;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// The event an app holding `turns` is told: the record's own fields, and
/// `ev`.
pub fn turn_event(record: &TurnRecord) -> Value {
    let mut event = serde_json::to_value(record).unwrap_or_else(|_| json!({}));
    if let Some(fields) = event.as_object_mut() {
        fields.insert("ev".to_owned(), json!("turns.finished"));
    }
    event
}

/// Reads `voice/turn_record` and offers each record to the apps that hold
/// `turns`.
pub fn start(bus: &lamp_bus::Bus, shared: Arc<Shared>) {
    match bus.subscribe(TurnRecord::TOPIC, "control") {
        Ok(mut subscriber) => {
            let spawned = std::thread::Builder::new()
                .name("app-turns".into())
                .spawn(move || {
                    let mut unread_said = false;
                    loop {
                        match subscriber.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => match message.parse::<TurnRecord>() {
                                Ok(record) if record.check().is_ok() => {
                                    shared.offer_turn(&record);
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    if !unread_said {
                                        unread_said = true;
                                        eprintln!(
                                            "lamp-control: voice/turn_record: a message not read, passed over: {error}"
                                        );
                                    }
                                }
                            },
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-control: voice/turn_record can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no turn record is told to an app: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no turn record is told to an app: {error}"),
    }
}
