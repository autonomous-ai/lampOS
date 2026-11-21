//! `apps.remember` on the app socket (#7415, C166, `docs/apps.md`, "What an
//! app tells Lamp's memory"): an installed app with the `remember` grant
//! tells Lamp one fact to keep for the lamp's known person, marked with the
//! app as its source ("20 Nov 16:47: squats 3×10, 6 too shallow").
//!
//! The door checks the ask here: the switch, the grant, the words
//! (`lamp_msgs::app_fact::tidy`, at most 280 characters, never cut), the lamp
//! not private and the rate ([`MOST_A_MINUTE`] an app, held across its
//! reconnects). A checked one goes to the voice on `apps/remember`
//! ([`lamp_msgs::app_fact::AppRemember`]), which picks the person
//! (`app_fact::whose`: the lamp's known person, never another, never the
//! room) and sends memory's `remember_app_fact`; how it went comes back on
//! `apps/remembered` and is the app's answer, or [`UNANSWERED`] past
//! [`WITHIN`]. An app removed (`app_forget`) has its facts forgotten
//! (`apps/forget_facts`, memory's `forget_app_facts`), whatever the switch.
//!
//! Behind `LAMP_STORE_APPS` like the whole socket, and its own switch
//! [`ENVIRONMENT`], off until the first builder's privacy read and the ROW
//! (Friday's 16:47 on #7415).
use crate::app_yes::Live;
use crate::protocol::{Error, ErrorCode};
use lamp_msgs::app_fact::{self, AppFactsForget, AppRemember, AppRemembered};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `on`, and nothing else; off unless set.
pub const ENVIRONMENT: &str = "LAMP_APP_REMEMBER";
/// The op, on the app socket only.
pub const OP: &str = "apps.remember";
/// The most facts an app may tell in any [`MINUTE`]: a session's summary is
/// one, so three is room for a retry and never a stream. Memory's own 20 a
/// day a source (`app_fact::MOST_A_DAY`) stands beside it.
pub const MOST_A_MINUTE: usize = 3;
pub const MINUTE: Duration = Duration::from_secs(60);
/// How long the door waits for the voice's word before it answers
/// [`UNANSWERED`]: under the 3 s an app such as the gym coach waits.
pub const WITHIN: Duration = Duration::from_millis(2_500);
/// What the app is told with the switch off.
pub const OFF: &str = "remembering is off on this lamp";
/// What the app is told without the grant.
pub const NO_GRANT: &str = "apps.remember needs the remember grant";
/// What the app is told while the lamp is private.
pub const PRIVATE: &str = "the lamp is private: nothing is kept now";
/// What the app is told when no word came in time, or the voice is not
/// reachable: it may not have been kept.
pub const UNANSWERED: &str = "Lamp's memory did not answer, so it may not be kept";

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Whether `frame` is an `apps.remember`.
pub fn is_ours(frame: &Map<String, Value>) -> bool {
    frame.get("op").and_then(Value::as_str) == Some(OP)
}

/// What the door knows when the ask comes.
#[derive(Clone, Copy, Debug)]
pub struct Gates {
    /// `LAMP_STORE_APPS`.
    pub apps: bool,
    /// [`ENVIRONMENT`].
    pub on: bool,
    /// The app's manifest asks `remember`.
    pub granted: bool,
    /// The privacy switch (or its lease lapsed): none counts as private.
    pub private: bool,
}

/// An `apps.remember` as the door judged it.
#[derive(Debug, PartialEq)]
pub enum Ask {
    /// Answered now: the refusal to send.
    Refused(Value),
    /// To keep: the frame's id and the tidied words.
    Keep { re: u64, text: String },
}

fn refuse(re: u64, code: ErrorCode, why: &str) -> Ask {
    Ask::Refused(crate::protocol::refused(Some(re), &Error::new(code, why)))
}

/// The door's judgment of app `name`'s `apps.remember` `frame` at `now`. A
/// frame with no id is the door's to refuse, so this is `None`. Only an ask
/// that would be sent counts toward the rate: a refused one costs nothing.
pub fn ask(
    live: &mut Live,
    name: &str,
    frame: &Map<String, Value>,
    gates: Gates,
    now: Instant,
) -> Option<Ask> {
    let re = frame.get("id").and_then(Value::as_u64)?;
    if !gates.apps {
        return Some(refuse(re, ErrorCode::Off, crate::app_socket::OFF));
    }
    if !gates.on {
        return Some(refuse(re, ErrorCode::Off, OFF));
    }
    if !gates.granted {
        return Some(refuse(re, ErrorCode::NoGrant, NO_GRANT));
    }
    let only_text = frame
        .keys()
        .all(|key| matches!(key.as_str(), "id" | "op" | "text"));
    let Some(text) = frame
        .get("text")
        .and_then(Value::as_str)
        .filter(|_| only_text)
    else {
        return Some(refuse(
            re,
            ErrorCode::BadRequest,
            "apps.remember takes text alone: the fact, 1 to 280 characters",
        ));
    };
    let text = match app_fact::tidy(text, app_fact::MOST_CHARS) {
        Ok(text) => text,
        Err(app_fact::Refused::TooLong) => {
            return Some(refuse(
                re,
                ErrorCode::BadRequest,
                "text: at most 280 characters, never cut",
            ));
        }
        Err(_) => {
            return Some(refuse(
                re,
                ErrorCode::BadRequest,
                "text: nothing to keep once links, markup and quotes are left out",
            ));
        }
    };
    if gates.private {
        return Some(refuse(re, ErrorCode::Private, PRIVATE));
    }
    if let Err(wait) = live.may_remember(name, now) {
        let mut error = Error::new(ErrorCode::Rate, "three a minute");
        error.retry_ms = Some(wait.as_millis().max(1) as u64);
        return Some(Ask::Refused(crate::protocol::refused(Some(re), &error)));
    }
    Some(Ask::Keep { re, text })
}

/// The app's answer to request `re` from how it went (`AppRemembered`'s
/// `outcome`): `{"ok":true,"kept":true}`, or why not in words with the code.
pub fn answer_of(re: u64, outcome: &str) -> Value {
    let (code, why) = match outcome {
        app_fact::KEPT => return crate::protocol::ok(re, json!({"kept": true})),
        "not_known" => (
            ErrorCode::Refused,
            "Lamp knows nobody to keep it for yet (not_known)",
        ),
        "forgotten" => (
            ErrorCode::Refused,
            "the person had Lamp forget everything since then (forgotten)",
        ),
        "too_many" => (
            ErrorCode::Refused,
            "at most 20 facts an app a day (too_many)",
        ),
        "bad_time" => (ErrorCode::Refused, "its time is ahead of Lamp's (bad_time)"),
        _ => (ErrorCode::Unavailable, UNANSWERED),
    };
    crate::protocol::refused(Some(re), &Error::new(code, why))
}

/// What lamp-control tells the voice.
#[derive(Clone, Debug, PartialEq)]
pub enum Tell {
    Remember(AppRemember),
    Forget(AppFactsForget),
}

/// How [`Tell`] reaches the voice (`apps/remember`, `apps/forget_facts`):
/// whether it was sent.
pub type Sink = Arc<dyn Fn(Tell) -> bool + Send + Sync>;

/// Starts the bus side: the sink that publishes [`Tell`], and the thread
/// that hands each `apps/remembered` to the ask that waits for it. With the
/// apps on and a bus; each part that can't start is said and the rest goes
/// on (with no sink, an ask is answered [`UNANSWERED`] and a remove forgets
/// nothing of memory's).
pub fn start(bus: &lamp_bus::Bus, shared: Arc<crate::server::Shared>) {
    let remember =
        bus.publisher_declared(AppRemember::TOPIC, &AppRemember::declaration(), "control");
    let forget = bus.publisher_declared(
        AppFactsForget::TOPIC,
        &AppFactsForget::declaration(),
        "control",
    );
    match (remember, forget) {
        (Ok(remember), Ok(forget)) => {
            let (remember, forget) = (Mutex::new(remember), Mutex::new(forget));
            shared.set_remember_sink(Arc::new(move |tell: Tell| {
                let sent = match &tell {
                    Tell::Remember(fact) => remember
                        .lock()
                        .map_err(|_| "poisoned".to_owned())
                        .and_then(|mut publisher| {
                            publisher
                                .publish_json(lamp_ipc::monotonic_us(), fact)
                                .map_err(|error| error.to_string())
                        }),
                    Tell::Forget(gone) => forget
                        .lock()
                        .map_err(|_| "poisoned".to_owned())
                        .and_then(|mut publisher| {
                            publisher
                                .publish_json(lamp_ipc::monotonic_us(), gone)
                                .map_err(|error| error.to_string())
                        }),
                };
                match sent {
                    Ok(_) => true,
                    Err(error) => {
                        eprintln!("lamp-control: an app's fact was not told to the voice: {error}");
                        false
                    }
                }
            }));
        }
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("lamp-control: no app's fact is told to the voice: {error}");
        }
    }
    match bus.subscribe(AppRemembered::TOPIC, "control") {
        Ok(mut subscriber) => {
            let spawned = std::thread::Builder::new()
                .name("app-remembered".into())
                .spawn(move || {
                    loop {
                        match subscriber.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => {
                                if let Ok(went) = message.parse::<AppRemembered>()
                                    && went.check().is_ok()
                                {
                                    shared.remembered(&went);
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-control: apps/remembered can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no app's fact is answered: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no app's fact is answered: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    const ALL: Gates = Gates {
        apps: true,
        on: true,
        granted: true,
        private: false,
    };

    fn code(ask: Option<Ask>) -> String {
        match ask {
            Some(Ask::Refused(reply)) => reply["error"]["code"].as_str().unwrap().to_owned(),
            other => panic!("refused was wanted: {other:?}"),
        }
    }

    /// Off, no grant, private: refused in words, nothing kept or counted.
    #[test]
    fn refused_without_the_switch_the_grant_or_while_private() {
        let mut live = Live::default();
        let t0 = Instant::now();
        let fact = frame(json!({"id": 4, "op": OP, "text": "squats 3x10"}));
        let with = |gates: Gates, live: &mut Live| ask(live, "gym-coach", &fact, gates, t0);
        assert_eq!(code(with(Gates { apps: false, ..ALL }, &mut live)), "off");
        let off = with(Gates { on: false, ..ALL }, &mut live);
        let Some(Ask::Refused(off)) = off else {
            panic!()
        };
        assert_eq!(off["error"]["why"], OFF);
        assert_eq!(off["re"], 4);
        let none = with(
            Gates {
                granted: false,
                ..ALL
            },
            &mut live,
        );
        let Some(Ask::Refused(none)) = none else {
            panic!()
        };
        assert_eq!(none["error"]["code"], "no_grant");
        assert_eq!(none["error"]["why"], NO_GRANT);
        assert_eq!(
            code(with(
                Gates {
                    private: true,
                    ..ALL
                },
                &mut live
            )),
            "private"
        );
        // None of those counted: three still go in the minute.
        for _ in 0..MOST_A_MINUTE {
            assert!(matches!(with(ALL, &mut live), Some(Ask::Keep { .. })));
        }
        assert!(ask(&mut live, "gym-coach", &frame(json!({"op": OP})), ALL, t0).is_none());
    }

    /// The words are the app_fact rules': tidied, at most 280, never cut,
    /// and nothing but `text`.
    #[test]
    fn the_words_are_tidied_capped_and_alone() {
        let mut live = Live::default();
        let t0 = Instant::now();
        let mut with = |value: Value| ask(&mut live, "gym-coach", &frame(value), ALL, t0);
        assert_eq!(
            with(
                json!({"id": 1, "op": OP, "text": " 20 Nov 16:47: squats 3×10, \"6\" too shallow <b> https://x.io "})
            ),
            Some(Ask::Keep {
                re: 1,
                text: "20 Nov 16:47: squats 3×10, 6 too shallow b".into()
            })
        );
        assert_eq!(
            code(with(json!({"id": 2, "op": OP, "text": "ă".repeat(281)}))),
            "bad_request"
        );
        assert_eq!(
            code(with(json!({"id": 3, "op": OP, "text": "`*[]` www.x.io"}))),
            "bad_request"
        );
        assert_eq!(code(with(json!({"id": 4, "op": OP}))), "bad_request");
        assert_eq!(
            code(with(json!({"id": 5, "op": OP, "text": 3}))),
            "bad_request"
        );
        assert_eq!(
            code(with(
                json!({"id": 6, "op": OP, "text": "squats", "person": "p-0123456789abcdef"})
            )),
            "bad_request",
            "the app never names whose it is"
        );
        assert!(matches!(
            with(json!({"id": 7, "op": OP, "text": "ă".repeat(280)})),
            Some(Ask::Keep { .. })
        ));
    }

    /// Three a minute an app, held across its reconnects, each app its own;
    /// only a remove forgets it.
    #[test]
    fn three_a_minute_an_app_across_reconnects() {
        let mut live = Live::default();
        let t0 = Instant::now();
        let fact = frame(json!({"id": 1, "op": OP, "text": "squats"}));
        for at in 0..MOST_A_MINUTE as u64 {
            let now = t0 + Duration::from_secs(at);
            assert!(matches!(
                ask(&mut live, "gym-coach", &fact, ALL, now),
                Some(Ask::Keep { .. })
            ));
        }
        live.connection("gym-coach", true);
        live.connection("gym-coach", false);
        let over = ask(
            &mut live,
            "gym-coach",
            &fact,
            ALL,
            t0 + Duration::from_secs(10),
        );
        let Some(Ask::Refused(over)) = over else {
            panic!()
        };
        assert_eq!(over["error"]["code"], "rate");
        assert_eq!(over["error"]["retry_ms"], 50_000);
        assert!(matches!(
            ask(&mut live, "other", &fact, ALL, t0),
            Some(Ask::Keep { .. })
        ));
        // The first leaves the minute: one more goes.
        assert!(matches!(
            ask(&mut live, "gym-coach", &fact, ALL, t0 + MINUTE),
            Some(Ask::Keep { .. })
        ));
        live.forget_app("gym-coach");
        assert!(matches!(
            ask(&mut live, "gym-coach", &fact, ALL, t0 + MINUTE),
            Some(Ask::Keep { .. })
        ));
    }

    #[test]
    fn how_it_went_is_the_apps_answer_in_words() {
        assert_eq!(
            answer_of(3, app_fact::KEPT),
            json!({"re": 3, "ok": true, "kept": true})
        );
        let refused = answer_of(3, "not_known");
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"]["code"], "refused");
        assert!(
            refused["error"]["why"]
                .as_str()
                .unwrap()
                .ends_with("(not_known)")
        );
        assert_eq!(answer_of(3, "too_many")["error"]["code"], "refused");
        for other in [app_fact::UNAVAILABLE, "unknown_op", "storage"] {
            assert_eq!(answer_of(3, other)["error"]["code"], "unavailable");
            assert_eq!(answer_of(3, other)["error"]["why"], UNANSWERED);
        }
    }
}
