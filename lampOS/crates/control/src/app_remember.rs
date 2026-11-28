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
//! [`WITHIN`]. An app removed (`app_forget`) has its facts forgotten on
//! memory's forget socket, held as a pending mark until memory says it was
//! done ([`crate::app_facts_forget`]), whatever the switch; while one is
//! pending, the app's new facts wait ([`FORGETTING`]).
//!
//! Behind `LAMP_STORE_APPS` like the whole socket, and its own switch
//! [`ENVIRONMENT`], on in the product's `lamp-control.service` by the first
//! builder's DEFAULT-ON on the switch-on row (Friday's 16:47 on #7415).
use crate::app_yes::Live;
use crate::protocol::{Error, ErrorCode};
use lamp_msgs::app_fact::{self, AppRemember, AppRemembered};
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
/// What the app is told while its facts from before a remove are still
/// being forgotten (a reinstall's new facts would go with them).
pub const FORGETTING: &str = "Lamp is still forgetting this app's earlier facts; try again later";
/// What the app is told when memory did not keep it for the person: one
/// word for `not_known` and `forgotten` alike, so an app learns nothing of
/// the person's own forgets (Friday's P3 on #8203). The journal keeps the
/// exact code.
pub const NOT_KEPT: &str = "Lamp did not keep it";
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
    /// A forget of the app's facts from a remove is still pending.
    pub forgetting: bool,
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
    if gates.forgetting {
        return Some(refuse(re, ErrorCode::Unavailable, FORGETTING));
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
        "not_known" | "forgotten" => (ErrorCode::Refused, NOT_KEPT),
        "too_many" => (
            ErrorCode::Refused,
            "at most 20 facts an app a day (too_many)",
        ),
        "bad_time" => (ErrorCode::Refused, "its time is ahead of Lamp's (bad_time)"),
        _ => (ErrorCode::Unavailable, UNANSWERED),
    };
    crate::protocol::refused(Some(re), &Error::new(code, why))
}

/// How an app's fact reaches the voice (`apps/remember`): whether it may
/// have reached it. `false` only when nothing was sent; then the app is
/// answered [`UNANSWERED`] at once. Otherwise the voice's word within
/// [`WITHIN`] is the answer, and no word is [`UNANSWERED`].
pub type Sink = Arc<dyn Fn(&AppRemember) -> bool + Send + Sync>;

/// Whether a publish that returned `error` may still have reached the
/// voice. The bus says a trouble with the topic (such as more than 64
/// subscribers, #8272) after it sent to the subscribers it links, and the
/// count of those is lost with the error, so only a message the bus
/// refuses before any send (`InvalidInput`: too long or badly labelled)
/// surely reached nobody.
pub fn may_have_reached(error: &std::io::Error) -> bool {
    error.kind() != std::io::ErrorKind::InvalidInput
}

/// Starts the bus side: the sink that publishes an app's fact, and the
/// thread that hands each `apps/remembered` to the ask that waits for it.
/// With the apps on and a bus; each part that can't start is said and the
/// rest goes on (with no sink, an ask is answered [`UNANSWERED`]).
pub fn start(bus: &lamp_bus::Bus, shared: Arc<crate::server::Shared>) {
    match bus.publisher_declared(AppRemember::TOPIC, &AppRemember::declaration(), "control") {
        Ok(remember) => {
            let remember = Mutex::new(remember);
            shared.set_remember_sink(Arc::new(move |fact: &AppRemember| {
                let Ok(mut publisher) = remember.lock() else {
                    eprintln!("lamp-control: an app's fact was not told to the voice: poisoned");
                    return false;
                };
                match publisher.publish_json(lamp_ipc::monotonic_us(), fact) {
                    Ok(_) => true,
                    // The voice may have it all the same: its word decides.
                    Err(error) if may_have_reached(&error) => {
                        eprintln!(
                            "lamp-control: an app's fact was told to the voice with trouble, \
                             its word decides: {error}"
                        );
                        true
                    }
                    Err(error) => {
                        eprintln!("lamp-control: an app's fact was not told to the voice: {error}");
                        false
                    }
                }
            }));
        }
        Err(error) => eprintln!("lamp-control: no app's fact is told to the voice: {error}"),
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
        forgetting: false,
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
        let waits = with(
            Gates {
                forgetting: true,
                ..ALL
            },
            &mut live,
        );
        let Some(Ask::Refused(waits)) = waits else {
            panic!()
        };
        assert_eq!(waits["error"]["code"], "unavailable");
        assert_eq!(waits["error"]["why"], FORGETTING);
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

    /// Only a message the bus refused before any send reached nobody.
    #[test]
    fn a_publish_with_trouble_may_still_have_reached_the_voice() {
        use std::io::{Error, ErrorKind};
        assert!(may_have_reached(&Error::other(
            "more than 64 subscribers (65 found): the rest get nothing"
        )));
        assert!(may_have_reached(&Error::from(ErrorKind::PermissionDenied)));
        assert!(!may_have_reached(&Error::from(ErrorKind::InvalidInput)));
    }

    #[test]
    fn how_it_went_is_the_apps_answer_in_words() {
        assert_eq!(
            answer_of(3, app_fact::KEPT),
            json!({"re": 3, "ok": true, "kept": true})
        );
        // One word for not_known and forgotten: nothing of the person's forgets.
        for code in ["not_known", "forgotten"] {
            let refused = answer_of(3, code);
            assert_eq!(refused["ok"], false);
            assert_eq!(refused["error"]["code"], "refused");
            assert_eq!(refused["error"]["why"], NOT_KEPT);
        }
        assert_eq!(answer_of(3, "too_many")["error"]["code"], "refused");
        for other in [app_fact::UNAVAILABLE, "unknown_op", "storage"] {
            assert_eq!(answer_of(3, other)["error"]["code"], "unavailable");
            assert_eq!(answer_of(3, other)["error"]["why"], UNANSWERED);
        }
    }
}
