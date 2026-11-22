//! `apps.context` on the app socket (#7415, the first builder's yes of 19
//! Nov 16:45, point 3; `docs/apps.md`, "What an app tells Lamp's memory"):
//! an installed app with the `remember` grant gives Lamp one short live line
//! while its session runs ("set 2 of 3, 7 squats"), told to the model beside
//! the person's line and never kept.
//!
//! The door checks the ask here, by `apps.remember`'s gates and order
//! ([`crate::app_remember`]): `LAMP_STORE_APPS`, then `LAMP_APP_REMEMBER`,
//! the grant, the words (`lamp_msgs::app_fact::tidy`, at most 120
//! characters, never cut), `for_ms` (1 ms to 10 minutes), the lamp not
//! private and the rate ([`EVERY`] an app, held across its reconnects). A
//! checked line goes to the voice on `apps/context`
//! ([`lamp_msgs::app_fact::AppContext`]) and is answered `{"ok":true}` once
//! published: the line lives in the voice's RAM only, so there is no
//! memory round trip to wait for. An empty `line` clears the app's line
//! (no `for_ms` then); a clear is never refused for the privacy switch or
//! the rate, since it only takes away. The app's last connection closing
//! (it stopped, was updated or removed) publishes a clear too, so a line
//! never outlives the app's session ([`crate::app_yes::Live::context_ends`]).
use crate::app_remember::Gates;
use crate::app_yes::Live;
use crate::protocol::{Error, ErrorCode};
use lamp_msgs::app_fact::{self, AppContext};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The op, on the app socket only.
pub const OP: &str = "apps.context";
/// The least time between two lines of one app: a set's counter changes a
/// few times a minute at most, and a line is told only at the person's next
/// turn, so one a second loses nothing an app means while a loop can't fill
/// the bus. A clear is not counted.
pub const EVERY: Duration = Duration::from_secs(1);
/// What the app is told while the lamp is private.
pub const PRIVATE: &str = "the lamp is private: nothing is told now";
/// What the app is told when the voice can't be told now.
pub const UNTOLD: &str = "Lamp's voice can't be told now";

/// Whether `frame` is an `apps.context`.
pub fn is_ours(frame: &Map<String, Value>) -> bool {
    frame.get("op").and_then(Value::as_str) == Some(OP)
}

/// An `apps.context` as the door judged it.
#[derive(Debug, PartialEq)]
pub enum Ask {
    /// Answered now: the refusal to send.
    Refused(Value),
    /// To tell the voice: the frame's id, the tidied line and its time.
    Tell { re: u64, line: String, for_ms: u64 },
    /// To clear the app's line: the frame's id.
    Clear { re: u64 },
}

fn refuse(re: u64, code: ErrorCode, why: &str) -> Ask {
    Ask::Refused(crate::protocol::refused(Some(re), &Error::new(code, why)))
}

const SHAPE: &str = "apps.context takes line and for_ms: 1 to 120 characters, told 1 to 600000 ms; an empty line alone clears it";

/// The door's judgment of app `name`'s `apps.context` `frame` at `now`. A
/// frame with no id is the door's to refuse, so this is `None`. Nothing is
/// counted here: a line counts toward the rate once it is told
/// ([`Live::context_told`]). `gates.forgetting` is
/// `apps.remember`'s alone: a live line is never kept, so a pending forget
/// does not hold it.
pub fn ask(
    live: &Live,
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
        return Some(refuse(re, ErrorCode::Off, crate::app_remember::OFF));
    }
    if !gates.granted {
        return Some(refuse(
            re,
            ErrorCode::NoGrant,
            "apps.context needs the remember grant",
        ));
    }
    let known = frame
        .keys()
        .all(|key| matches!(key.as_str(), "id" | "op" | "line" | "for_ms"));
    let Some(line) = frame.get("line").and_then(Value::as_str).filter(|_| known) else {
        return Some(refuse(re, ErrorCode::BadRequest, SHAPE));
    };
    let for_ms = frame.get("for_ms");
    if line.is_empty() {
        // A clear: no time with it.
        if for_ms.is_some() {
            return Some(refuse(re, ErrorCode::BadRequest, SHAPE));
        }
        return Some(Ask::Clear { re });
    }
    let Some(for_ms) = for_ms
        .and_then(Value::as_u64)
        .filter(|ms| (1..=app_fact::CONTEXT_MOST_MS).contains(ms))
    else {
        return Some(refuse(
            re,
            ErrorCode::BadRequest,
            "for_ms: 1 to 600000 (10 minutes); the app renews it",
        ));
    };
    let line = match app_fact::tidy(line, app_fact::CONTEXT_MOST_CHARS) {
        Ok(line) => line,
        Err(app_fact::Refused::TooLong) => {
            return Some(refuse(
                re,
                ErrorCode::BadRequest,
                "line: at most 120 characters, never cut",
            ));
        }
        Err(_) => {
            return Some(refuse(
                re,
                ErrorCode::BadRequest,
                "line: nothing to tell once links, markup and quotes are left out",
            ));
        }
    };
    if gates.private {
        return Some(refuse(re, ErrorCode::Private, PRIVATE));
    }
    if let Err(wait) = live.may_tell_context(name, now) {
        let mut error = Error::new(ErrorCode::Rate, "one line a second");
        error.retry_ms = Some(wait.as_millis().max(1) as u64);
        return Some(Ask::Refused(crate::protocol::refused(Some(re), &error)));
    }
    Some(Ask::Tell { re, line, for_ms })
}

/// The app's answer to request `re` once its line (or clear) was told the
/// voice (`sent`), or why not.
pub fn answer_of(re: u64, sent: bool) -> Value {
    if sent {
        crate::protocol::ok(re, json!({}))
    } else {
        crate::protocol::refused(Some(re), &Error::new(ErrorCode::Unavailable, UNTOLD))
    }
}

/// How an app's line reaches the voice (`apps/context`): whether it was
/// published.
pub type Sink = Arc<dyn Fn(&AppContext) -> bool + Send + Sync>;

/// Starts the bus side: the sink that publishes an app's line or its clear.
/// With the apps on and a bus; with none, an ask is answered [`UNTOLD`].
pub fn start(bus: &lamp_bus::Bus, shared: Arc<crate::server::Shared>) {
    match bus.publisher_declared(AppContext::TOPIC, &AppContext::declaration(), "control") {
        Ok(publisher) => {
            let publisher = Mutex::new(publisher);
            shared.set_context_sink(Arc::new(move |line: &AppContext| {
                let sent = publisher
                    .lock()
                    .map_err(|_| "poisoned".to_owned())
                    .and_then(|mut publisher| {
                        publisher
                            .publish_json(lamp_ipc::monotonic_us(), line)
                            .map_err(|error| error.to_string())
                    });
                match sent {
                    Ok(_) => true,
                    Err(error) => {
                        eprintln!(
                            "lamp-control: app {}: its context line was not told to the voice: {error}",
                            line.app
                        );
                        false
                    }
                }
            }));
        }
        Err(error) => {
            eprintln!("lamp-control: no app's context line is told to the voice: {error}")
        }
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

    fn refused(ask: Option<Ask>) -> Value {
        match ask {
            Some(Ask::Refused(reply)) => reply,
            other => panic!("refused was wanted: {other:?}"),
        }
    }

    fn code(ask: Option<Ask>) -> String {
        refused(ask)["error"]["code"].as_str().unwrap().to_owned()
    }

    /// `apps.remember`'s gates in its order: off, no grant, then the words,
    /// then private. A pending forget does not
    /// hold a live line.
    #[test]
    fn refused_without_the_switch_the_grant_or_while_private() {
        let live = Live::default();
        let t0 = Instant::now();
        let line = frame(json!({"id": 4, "op": OP, "line": "set 2 of 3", "for_ms": 60_000}));
        let with = |gates: Gates, live: &Live| ask(live, "gym-coach", &line, gates, t0);
        assert_eq!(code(with(Gates { apps: false, ..ALL }, &live)), "off");
        let off = refused(with(Gates { on: false, ..ALL }, &live));
        assert_eq!(off["error"]["why"], crate::app_remember::OFF);
        assert_eq!(off["re"], 4);
        let none = refused(with(
            Gates {
                granted: false,
                ..ALL
            },
            &live,
        ));
        assert_eq!(none["error"]["code"], "no_grant");
        // Off and no grant come before the words.
        let bad = frame(json!({"id": 5, "op": OP, "line": 3}));
        assert_eq!(
            code(ask(
                &live,
                "gym-coach",
                &bad,
                Gates { on: false, ..ALL },
                t0
            )),
            "off"
        );
        let private = refused(with(
            Gates {
                private: true,
                ..ALL
            },
            &live,
        ));
        assert_eq!(private["error"]["code"], "private");
        assert_eq!(private["error"]["why"], PRIVATE);
        // A pending forget is apps.remember's.
        assert!(matches!(
            with(
                Gates {
                    forgetting: true,
                    ..ALL
                },
                &live
            ),
            Some(Ask::Tell {
                re: 4,
                for_ms: 60_000,
                ..
            })
        ));
        assert!(ask(&live, "gym-coach", &frame(json!({"op": OP})), ALL, t0).is_none());
    }

    /// The line is tidied, at most 120 and never cut; `for_ms` 1 ms to 10
    /// minutes; no other field.
    #[test]
    fn the_line_is_tidied_capped_and_timed() {
        let live = Live::default();
        let t0 = Instant::now();
        let mut at = 0;
        let mut with = |value: Value| {
            at += 2;
            ask(
                &live,
                "gym-coach",
                &frame(value),
                ALL,
                t0 + Duration::from_secs(at),
            )
        };
        assert_eq!(
            with(
                json!({"id": 1, "op": OP, "line": " set 2 of 3, \"7\" squats <b> https://x.io ", "for_ms": 90_000})
            ),
            Some(Ask::Tell {
                re: 1,
                line: "set 2 of 3, 7 squats b".into(),
                for_ms: 90_000
            })
        );
        for bad in [
            json!({"id": 2, "op": OP, "line": "ă".repeat(121), "for_ms": 1}),
            json!({"id": 3, "op": OP, "line": "`*[]` www.x.io", "for_ms": 1}),
            json!({"id": 4, "op": OP, "for_ms": 1}),
            json!({"id": 5, "op": OP, "line": 3, "for_ms": 1}),
            json!({"id": 6, "op": OP, "line": "set 2"}),
            json!({"id": 7, "op": OP, "line": "set 2", "for_ms": 0}),
            json!({"id": 8, "op": OP, "line": "set 2", "for_ms": 600_001}),
            json!({"id": 9, "op": OP, "line": "set 2", "for_ms": -1}),
            json!({"id": 10, "op": OP, "line": "set 2", "for_ms": 1.5}),
            json!({"id": 11, "op": OP, "line": "set 2", "for_ms": 1, "person": "p-0123456789abcdef"}),
            json!({"id": 12, "op": OP, "line": "", "for_ms": 1}),
        ] {
            assert_eq!(code(with(bad.clone())), "bad_request", "{bad}");
        }
        assert!(matches!(
            with(json!({"id": 13, "op": OP, "line": "ă".repeat(120), "for_ms": 600_000})),
            Some(Ask::Tell {
                for_ms: 600_000,
                ..
            })
        ));
        assert!(matches!(
            with(json!({"id": 14, "op": OP, "line": "set 2", "for_ms": 1})),
            Some(Ask::Tell { for_ms: 1, .. })
        ));
    }

    /// One line a second an app, held across reconnects, each app its own;
    /// a clear is never held by the rate or the privacy switch.
    #[test]
    fn one_a_second_and_a_clear_always_goes() {
        let mut live = Live::default();
        let t0 = Instant::now();
        let line = frame(json!({"id": 1, "op": OP, "line": "set 1", "for_ms": 5_000}));
        let clear = frame(json!({"id": 2, "op": OP, "line": ""}));
        assert!(matches!(
            ask(&live, "gym-coach", &line, ALL, t0),
            Some(Ask::Tell { .. })
        ));
        // Judged, not told: it costs nothing until it is.
        assert!(matches!(
            ask(&live, "gym-coach", &line, ALL, t0),
            Some(Ask::Tell { .. })
        ));
        live.context_told("gym-coach", Some(t0));
        live.connection("gym-coach", true);
        live.connection("gym-coach", false);
        let over = refused(ask(
            &live,
            "gym-coach",
            &line,
            ALL,
            t0 + Duration::from_millis(400),
        ));
        assert_eq!(over["error"]["code"], "rate");
        assert_eq!(over["error"]["retry_ms"], 600);
        assert!(matches!(
            ask(&live, "other", &line, ALL, t0),
            Some(Ask::Tell { .. })
        ));
        for gates in [
            ALL,
            Gates {
                private: true,
                ..ALL
            },
        ] {
            assert_eq!(
                ask(&live, "gym-coach", &clear, gates, t0),
                Some(Ask::Clear { re: 2 })
            );
        }
        assert!(matches!(
            ask(&live, "gym-coach", &line, ALL, t0 + EVERY),
            Some(Ask::Tell { .. })
        ));
        live.context_told("gym-coach", Some(t0 + EVERY));
        live.forget_app("gym-coach");
        assert!(matches!(
            ask(&live, "gym-coach", &line, ALL, t0 + EVERY),
            Some(Ask::Tell { .. })
        ));
    }

    /// `{"ok":true}` once told; `unavailable` when it could not be.
    #[test]
    fn answered_once_published() {
        assert_eq!(answer_of(7, true), json!({"re": 7, "ok": true}));
        let not = answer_of(7, false);
        assert_eq!(not["error"]["code"], "unavailable");
        assert_eq!(not["error"]["why"], UNTOLD);
    }
}
