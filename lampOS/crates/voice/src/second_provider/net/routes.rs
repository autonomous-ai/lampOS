//! The relay's near addresses (`LAMP_RELAY_ROUTES=on`, #2882): the relay is
//! asked at `<llm_base_url>/v1/relay/routes`, with the route's key, which
//! of its services a lamp should reach at a nearer address, and for how
//! long the answer holds. Asked in the background, never on a turn's way;
//! a turn reads only what was last kept. Only which services are near
//! reaches the journal, never an address or a key.
use super::{Error, Result, get_json, openai_base};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

/// How long an answer holds when it says nothing ([`Read::Routes`]).
pub const TTL: Duration = Duration::from_secs(600);
/// How soon a failed asking is asked again, if sooner than the answer's
/// end: a lamp whose first asking fails before its network is up takes
/// the relay's routes a minute later, not ten (El's P3 on #2919).
pub const RETRY: Duration = Duration::from_secs(60);
/// The least and the most an answer's `ttl_seconds` is taken for.
const LEAST_TTL: u64 = 60;
const MOST_TTL: u64 = 3600;

/// The near addresses the relay gave, each the same kind of value its
/// variable takes: `chat` [`crate::second_config::DEEPSEEK_BASE_URL_ENV`]'s
/// (ending `/deepseek`), `speech` [`crate::second_config::SPEECH_BASE_URL_ENV`]'s,
/// `listening` [`crate::second_config::LISTENING_BASE_URL_ENV`]'s. `None`:
/// the service's own address. The relay's `gemini` is read past (no Live
/// route here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routes {
    pub chat: Option<String>,
    pub speech: Option<String>,
    pub listening: Option<String>,
}

impl Routes {
    /// The journal's words: which services are near, never where.
    pub fn told(&self) -> String {
        let near = |route: &Option<String>| if route.is_some() { "near" } else { "normal" };
        format!(
            "routes: chat {}, speech {}, listening {}",
            near(&self.chat),
            near(&self.speech),
            near(&self.listening)
        )
    }
}

/// What one asking of the relay says.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// These routes, asked again after `ttl`; `refused` names the services
    /// whose address was not one to use, taken as `None`.
    Routes {
        routes: Routes,
        ttl: Duration,
        refused: Vec<&'static str>,
    },
    /// The routes kept stay; why, in the journal's words (a status, never
    /// the server's words).
    Keep(String),
}

/// The relay's answer read: HTTP 404 is a relay that routes nothing, so
/// every service its own; any other failure, 401 among them, keeps what is
/// kept.
pub fn read(answer: Result<Value>) -> Read {
    let answer = match answer {
        Ok(answer) => answer,
        Err(Error::Status(404)) => {
            return Read::Routes {
                routes: Routes::default(),
                ttl: TTL,
                refused: Vec::new(),
            };
        }
        Err(error) => return Read::Keep(why(&error)),
    };
    let data = &answer["data"];
    if answer["status"].as_i64() != Some(1) || !data.is_object() {
        return Read::Keep("answer".to_owned());
    }
    let mut refused = Vec::new();
    let mut route = |name: &'static str| match &data[name] {
        Value::Null => None,
        Value::String(url) if usable(url) => Some(url.clone()),
        _ => {
            refused.push(name);
            None
        }
    };
    let routes = Routes {
        chat: route("chat"),
        speech: route("speech"),
        listening: route("listening"),
    };
    let ttl = data["ttl_seconds"].as_u64().map_or(TTL, |seconds| {
        Duration::from_secs(seconds.clamp(LEAST_TTL, MOST_TTL))
    });
    Read::Routes {
        routes,
        ttl,
        refused,
    }
}

/// A near address checked as the file's are
/// ([`crate::second_config::checked_url`]); the tests' local plain server
/// too.
fn usable(url: &str) -> bool {
    crate::second_config::checked_url(url.to_owned()).is_ok()
        || (cfg!(test) && url.starts_with("http://127.0.0.1:"))
}

/// A failure in the journal's words: its status, `transport`, `timeout`,
/// or `answer`.
fn why(error: &Error) -> String {
    match error {
        Error::Status(status) => status.to_string(),
        Error::Transport(_) => "transport".to_owned(),
        Error::Timeout(_) => "timeout".to_owned(),
        Error::Url | Error::Protocol(_) => "answer".to_owned(),
    }
}

/// Whether a near service's failure sends it back to its own address for
/// the rest of the turn: no connection, a timeout, any 5xx or a 401; the
/// journal's word for it. Any other (400, 404, a protocol's) fails as it
/// would at its own address.
pub fn fallback_why(error: &Error) -> Option<String> {
    match error {
        Error::Transport(_) | Error::Timeout(_) => Some(why(error)),
        Error::Status(status) if *status >= 500 || *status == 401 => Some(why(error)),
        _ => None,
    }
}

/// The journal's line for a near service gone back to its own address.
pub fn fell_back(service: &str, why: &str) {
    eprintln!("lamp-voice second: route=fallback service={service} why={why}");
}

/// Where the relay is asked, from the route's `llm_base_url`.
pub fn url(llm_base_url: &str) -> String {
    format!("{}/relay/routes", openai_base(llm_base_url))
}

/// The routes last read, the near addresses that failed since, and the
/// bell that asks again at once.
#[derive(Default)]
pub struct Kept {
    latest: Mutex<Routes>,
    /// Each service's near address that failed on the link since the routes
    /// were last read: the service stays at its own until the next reading,
    /// so a near host that never answers costs one slow turn or utterance,
    /// not every one (El's P2s on #2919).
    held: Mutex<Vec<(&'static str, String)>>,
    again: Notify,
}

impl Kept {
    /// The routes last read; every service its own before the first.
    pub fn latest(&self) -> Routes {
        self.latest
            .lock()
            .map(|latest| latest.clone())
            .unwrap_or_default()
    }

    /// Ask the relay again now, not at the answer's end: a service host
    /// answers again after the link was down.
    pub fn ask_again(&self) {
        self.again.notify_one();
    }

    /// `service`'s near address `url` failed on the link: its own until the
    /// routes are next read.
    pub fn hold(&self, service: &'static str, url: &str) {
        if let Ok(mut held) = self.held.lock()
            && !held
                .iter()
                .any(|(at, address)| *at == service && address == url)
        {
            held.push((service, url.to_owned()));
        }
    }

    /// Whether `service`'s near address `url` failed since the routes were
    /// last read.
    pub fn held(&self, service: &'static str, url: &str) -> bool {
        self.held.lock().is_ok_and(|held| {
            held.iter()
                .any(|(at, address)| *at == service && address == url)
        })
    }

    /// A reading's routes, and every near address tried afresh.
    fn keep(&self, routes: Routes) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = routes;
        }
        if let Ok(mut held) = self.held.lock() {
            held.clear();
        }
    }

    #[cfg(test)]
    pub fn set(&self, routes: Routes) {
        self.keep(routes);
    }
}

/// When a failed asking is asked again: at the answer's end, or after
/// [`RETRY`] if that is sooner.
fn after_a_failure(ttl: Duration) -> Duration {
    ttl.min(RETRY)
}

/// Asks the relay at `url` at once, then at each answer's end
/// ([`Read::Routes`]'s `ttl`) or when [`Kept::ask_again`] rings, and keeps
/// what it says in `kept`. The journal says the routes when they change and
/// a failure once a streak, never an address. Runs for as long as the
/// worker's runtime does.
pub async fn task(url: String, authorization: String, kept: Arc<Kept>) {
    let mut ttl = TTL;
    let mut told: Option<Routes> = None;
    let mut failing = false;
    let mut refused_before: Vec<&'static str> = Vec::new();
    loop {
        let next = match read(get_json(&url, &authorization).await) {
            Read::Routes {
                routes,
                ttl: holds,
                refused,
            } => {
                for service in &refused {
                    if !refused_before.contains(service) {
                        eprintln!(
                            "lamp-voice second: the relay's {service} route is not an address to use; {service} stays at its own"
                        );
                    }
                }
                refused_before = refused;
                ttl = holds;
                kept.keep(routes.clone());
                if std::mem::replace(&mut failing, false) || told.as_ref() != Some(&routes) {
                    eprintln!("lamp-voice second: {}", routes.told());
                    told = Some(routes);
                }
                ttl
            }
            Read::Keep(why) => {
                let next = after_a_failure(ttl);
                if !std::mem::replace(&mut failing, true) {
                    eprintln!(
                        "lamp-voice second: the relay's routes were not read ({why}); those kept stay, asked again in {} s",
                        next.as_secs()
                    );
                }
                next
            }
        };
        tokio::select! {
            () = tokio::time::sleep(next) => {}
            () = kept.again.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn routes(answer: Value) -> (Routes, Duration, Vec<&'static str>) {
        match read(Ok(answer)) {
            Read::Routes {
                routes,
                ttl,
                refused,
            } => (routes, ttl, refused),
            Read::Keep(why) => panic!("kept: {why}"),
        }
    }

    const NEAR: &str = "https://near.example.invalid/api/v1/ai";

    /// #2882: every route null is every service at its own address; each
    /// one set is that one near; `gemini` is read past.
    #[test]
    fn each_route_set_is_near_and_null_is_normal() {
        let all_null = json!({"status": 1, "data": {"chat": null, "speech": null,
            "listening": null, "gemini": null, "ttl_seconds": 600}});
        assert_eq!(routes(all_null), (Routes::default(), TTL, Vec::new()));
        for name in ["chat", "speech", "listening"] {
            let mut data = json!({"chat": null, "speech": null, "listening": null,
                "gemini": NEAR, "ttl_seconds": 600});
            data[name] = json!(format!("{NEAR}/{name}"));
            let (got, _, refused) = routes(json!({"status": 1, "data": data}));
            assert!(refused.is_empty());
            let near = Some(format!("{NEAR}/{name}"));
            let want = match name {
                "chat" => Routes {
                    chat: near,
                    ..Routes::default()
                },
                "speech" => Routes {
                    speech: near,
                    ..Routes::default()
                },
                _ => Routes {
                    listening: near,
                    ..Routes::default()
                },
            };
            assert_eq!(got, want, "{name}");
        }
        // Absent is null too.
        assert_eq!(
            routes(json!({"status": 1, "data": {}})).0,
            Routes::default()
        );
    }

    /// 404 is a relay that routes nothing; 401 and any other failure, or an
    /// answer not in the relay's form, keep what is kept.
    #[test]
    fn not_found_is_normal_and_any_other_failure_keeps() {
        assert_eq!(
            read(Err(Error::Status(404))),
            Read::Routes {
                routes: Routes::default(),
                ttl: TTL,
                refused: Vec::new()
            }
        );
        assert_eq!(read(Err(Error::Status(401))), Read::Keep("401".into()));
        assert_eq!(read(Err(Error::Status(503))), Read::Keep("503".into()));
        assert_eq!(
            read(Err(Error::Transport("request"))),
            Read::Keep("transport".into())
        );
        assert_eq!(
            read(Err(Error::Timeout("request"))),
            Read::Keep("timeout".into())
        );
        assert_eq!(
            read(Ok(json!({"status": 0, "data": null}))),
            Read::Keep("answer".into())
        );
        assert_eq!(
            read(Ok(json!({"status": 1, "data": "x"}))),
            Read::Keep("answer".into())
        );
    }

    /// A route not an address to use is null for that service alone, and
    /// named; the others stand.
    #[test]
    fn a_bad_address_is_null_for_its_service_alone() {
        let long = format!("https://near.example.invalid/{}", "a".repeat(2048));
        for bad in [
            json!("http://plain.example.invalid"),
            json!("https://near.example.invalid/?key=1"),
            json!("https://near.example.invalid/#x"),
            json!("https://user@near.example.invalid"),
            json!("https:///no-host"),
            json!(long),
            json!(7),
        ] {
            let (got, _, refused) = routes(json!({"status": 1, "data": {
                "chat": bad, "speech": NEAR, "listening": null}}));
            assert_eq!(
                got,
                Routes {
                    speech: Some(NEAR.into()),
                    ..Routes::default()
                },
                "{bad}"
            );
            assert_eq!(refused, ["chat"]);
        }
    }

    /// `ttl_seconds` is taken between a minute and an hour; absent or not a
    /// count, ten minutes.
    #[test]
    fn the_answers_time_is_clamped() {
        let ttl = |value: Value| routes(json!({"status": 1, "data": {"ttl_seconds": value}})).1;
        assert_eq!(ttl(json!(5)), Duration::from_secs(60));
        assert_eq!(ttl(json!(120)), Duration::from_secs(120));
        assert_eq!(ttl(json!(86400)), Duration::from_secs(3600));
        assert_eq!(ttl(json!(-1)), TTL);
        assert_eq!(ttl(json!("600")), TTL);
        assert_eq!(ttl(Value::Null), TTL);
    }

    /// Back to its own address: no connection, a timeout, a 5xx or a 401;
    /// never a 400, a 404 or a protocol's failure.
    #[test]
    fn only_the_links_failures_and_a_refused_key_fall_back() {
        assert_eq!(
            fallback_why(&Error::Transport("request")).as_deref(),
            Some("transport")
        );
        assert_eq!(
            fallback_why(&Error::Timeout("request")).as_deref(),
            Some("timeout")
        );
        assert_eq!(fallback_why(&Error::Status(503)).as_deref(), Some("503"));
        assert_eq!(fallback_why(&Error::Status(500)).as_deref(), Some("500"));
        assert_eq!(fallback_why(&Error::Status(401)).as_deref(), Some("401"));
        for kept in [
            Error::Status(400),
            Error::Status(403),
            Error::Status(404),
            Error::Status(429),
            Error::Protocol("answer"),
            Error::Url,
        ] {
            assert_eq!(fallback_why(&kept), None, "{kept:?}");
        }
    }

    /// El's P3 and P2s on #2919: a failed asking is asked again within a
    /// minute; a near address that failed is held until the routes are next
    /// read, for its service alone, and every one is tried afresh then.
    #[test]
    fn a_failure_is_asked_again_soon_and_a_failed_address_held_until_read() {
        assert_eq!(after_a_failure(TTL), RETRY);
        assert_eq!(after_a_failure(Duration::from_secs(60)), RETRY);
        let kept = Kept::default();
        kept.hold("chat", NEAR);
        assert!(kept.held("chat", NEAR));
        assert!(!kept.held("speech", NEAR));
        assert!(!kept.held("chat", "https://other.example.invalid/ai"));
        kept.set(Routes::default());
        assert!(!kept.held("chat", NEAR));
    }

    /// The journal names which are near, never an address.
    #[test]
    fn the_journal_names_no_address() {
        let told = Routes {
            chat: Some(NEAR.into()),
            ..Routes::default()
        }
        .told();
        assert_eq!(told, "routes: chat near, speech normal, listening normal");
        assert_eq!(
            url("https://campaign-api.autonomous.ai/api/v1/ai"),
            "https://campaign-api.autonomous.ai/api/v1/ai/v1/relay/routes"
        );
    }
}
