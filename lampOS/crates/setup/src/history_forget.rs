//! At a reset, the lamp's whole history on the account deleted while the
//! lamp's key is still on it (#7771; the first builder's ruling on #8125,
//! 7049606477, after Backend3's answer): the reset wipes the relay key
//! (`/etc/lampOS/voice/credential`) and the person's own keys
//! (`/etc/lampOS/voice/own`), and a delete sent after a new setup goes with
//! the new key, which can't reach the old account.
//!
//! Only with `LAMP_HISTORY_FORGET=on` in the voice's unit (read from systemd,
//! [`ENVIRONMENT_ARGV`]), so the switch is the voice's one line; off, nothing
//! is read or sent, as before. Before the wipe ([`crate::reset`]'s `finish`,
//! every entry: the button, the app's reset, setup's own and its resume
//! after a power cut), the delete of the lamp's whole history,
//! [`WHOLE_LAMP_BODY`], is posted to the lamp's own cloud API with the
//! relay key, as the voice's queue sends it, and its answer read, all within
//! [`WITHIN`]: a reset is never held longer by a dead network. A 2xx is done;
//! a 401 means the key is already gone from the account, never tried again;
//! anything else is tried again after [`AGAIN`] until [`WITHIN`] has passed,
//! then said in the journal and the reset goes on. The voice's own delete at
//! its next start (the changed memory epoch) stays as the fallback, which
//! reaches the old account only if the key it then has is the old one.
//! A reset finished at boot (`resume`, after a power cut or a stop that
//! failed) runs before the lamp's Wi-Fi is up (`lamp-setup.service` is
//! ordered only after the local file systems): there the delete first waits
//! for a route and DNS to the lamp's cloud API, at most [`NETWORK_WITHIN`]
//! (Heron's P2 on #8125, the first builder's ruling). Past it the reset
//! goes on, journalled as not reaching the account: that history may stay.
//! Only the outcome is journalled, never the key.
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

/// The voice's switch.
pub const ENVIRONMENT: &str = "LAMP_HISTORY_FORGET";
/// The voice's unit, whose environment holds [`ENVIRONMENT`].
pub const VOICE_UNIT: &str = "lamp-voice.service";
/// What systemd says of the voice unit's environment, drop-ins included.
pub const ENVIRONMENT_ARGV: [&str; 6] = [
    "/usr/bin/systemctl",
    "show",
    "-p",
    "Environment",
    "--value",
    VOICE_UNIT,
];
/// The second route's settings, which name the relay base and its key.
pub const SECOND_CONFIG: &str = "/etc/lampOS/voice/second.json";
/// The longest the delete holds a reset, every try and its answer included.
pub const WITHIN: Duration = Duration::from_secs(10);
/// The wait between tries.
pub const AGAIN: Duration = Duration::from_secs(1);
/// On a reset resumed at boot, the longest the delete waits for a route and
/// DNS to the lamp's cloud API before its first try: a lamp's Wi-Fi joins
/// and gets its address and DNS within this of setup's start in the usual
/// boot, and a reset is never held longer by a network that does not come.
pub const NETWORK_WITHIN: Duration = Duration::from_secs(60);
/// The wait between looks for the network.
pub const NETWORK_AGAIN: Duration = Duration::from_secs(1);
/// The longest one name lookup is waited for.
const LOOKUP_MOST: Duration = Duration::from_secs(5);
/// The delete of the lamp's whole history, every person's and the room's
/// (the voice's `history_forget::WHOLE_LAMP_BODY`, Backend3's call).
pub const WHOLE_LAMP_BODY: &str = r#"{"all":true,"every_person":true}"#;
/// The lamp's own cloud API (the voice's `second_config::COMPANY_RELAY_HOST`).
const RELAY_HOST: &str = "campaign-api.autonomous.ai";
/// The longest key file read.
const KEY_MOST: u64 = 4_096;
/// The longest settings file read.
const CONFIG_MOST: u64 = 64 * 1024;

/// Where the delete goes and with which key.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub url: String,
    pub key: String,
}

/// What the delete came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The switch is off: nothing was sent.
    Off,
    /// Nothing to send it with (no settings, another base, no key).
    NoTarget(&'static str),
    /// Answered 2xx, with its status.
    Done(u16),
    /// Answered 401: the key is already gone from the account.
    Gone,
    /// Not answered done within [`WITHIN`]: the last try's why, and how many.
    Failed(String, u32),
    /// A reset resumed at boot: no route or DNS to the lamp's cloud API
    /// within [`NETWORK_WITHIN`], so nothing was sent.
    NoNetwork,
}

impl Outcome {
    /// The journal's line, counts and statuses only.
    pub fn said(&self) -> String {
        match self {
            Self::Off => format!("{ENVIRONMENT} is off; the account's history is not touched"),
            Self::NoTarget(why) => format!(
                "the account's history of this lamp was not deleted before the wipe ({why}); the voice deletes it at its next start, with the key it has then"
            ),
            Self::Done(status) => format!(
                "the account's history of this lamp was deleted before its key is wiped (HTTP {status})"
            ),
            Self::Gone => "the account's history delete answered 401: the key is already gone from the account, not tried again".into(),
            Self::Failed(why, tries) => format!(
                "the account's history of this lamp was not deleted before its key is wiped ({why}, {tries} tries in {} s); the voice deletes it at its next start with the new key, which does not reach this account",
                WITHIN.as_secs()
            ),
            Self::NoNetwork => format!(
                "the account's history of this lamp was not deleted before its key is wiped: no route or DNS to the lamp's cloud API within {} s of this boot; the delete did not reach the account, and this lamp's history may stay on it",
                NETWORK_WITHIN.as_secs()
            ),
        }
    }
}

/// Whether `environment`, as systemd prints a unit's `Environment`, sets
/// [`ENVIRONMENT`] to `on`; the last setting wins.
pub fn on_in(environment: &str) -> bool {
    let mut on = false;
    for word in environment.split_whitespace() {
        let word = word.trim_matches('"');
        if let Some(value) = word
            .strip_prefix(ENVIRONMENT)
            .and_then(|rest| rest.strip_prefix('='))
        {
            on = value == "on";
        }
    }
    on
}

/// The delete's address from the route's `llm_base_url`: only the lamp's own
/// cloud API, as the voice's `net::history::url`.
pub fn url(llm_base_url: &str) -> Option<String> {
    let base = llm_base_url.trim().trim_end_matches('/');
    let host = base.strip_prefix("https://")?.split('/').next()?;
    if host != RELAY_HOST {
        return None;
    }
    let base = base.strip_suffix("/v1").unwrap_or(base);
    Some(format!("{base}/history/turns/delete"))
}

fn read_most(path: &Path, most: u64) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(most)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// The address and key the second route's settings at `config` name.
pub fn target(config: &Path) -> Result<Target, &'static str> {
    let settings: Value = read_most(config, CONFIG_MOST)
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or("no second route settings")?;
    let url = settings["llm_base_url"]
        .as_str()
        .and_then(url)
        .ok_or("the route is not the lamp's own cloud API")?;
    let file = settings["llm_credential_file"]
        .as_str()
        .map(Path::new)
        .filter(|file| file.is_absolute())
        .ok_or("no relay key named")?;
    let key = read_most(file, KEY_MOST)
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or("no relay key")?;
    Ok(Target { url, key })
}

/// Before a reset's wipe: with the switch on in `environment`, the delete
/// of the lamp's whole history to `config`'s target, each try by `post`
/// (the address, the key, the time left; the answer's status), until done,
/// a 401, or `within` has passed.
pub fn before_wipe(
    environment: &str,
    config: &Path,
    within: Duration,
    post: impl FnMut(&str, &str, Duration) -> Result<u16, String>,
) -> Outcome {
    before_wipe_after(environment, config, within, || true, post)
}

/// [`before_wipe`], once `network` says the lamp's cloud API can be reached
/// (a reset resumed at boot, [`network_within`]); when it says not, nothing
/// is sent ([`Outcome::NoNetwork`]). Asked only with the switch on and a
/// target, so no other reset waits.
pub fn before_wipe_after(
    environment: &str,
    config: &Path,
    within: Duration,
    network: impl FnOnce() -> bool,
    post: impl FnMut(&str, &str, Duration) -> Result<u16, String>,
) -> Outcome {
    if !on_in(environment) {
        return Outcome::Off;
    }
    match target(config) {
        Ok(target) if network() => send_within(&target, within, post),
        Ok(_) => Outcome::NoNetwork,
        Err(why) => Outcome::NoTarget(why),
    }
}

/// Whether `up` (given the time left) says the network is there before
/// `within` has passed, looked at every `again`, time read by `elapsed`
/// and waited by `sleep`.
pub fn network_within(
    within: Duration,
    again: Duration,
    mut up: impl FnMut(Duration) -> bool,
    mut sleep: impl FnMut(Duration),
    elapsed: impl Fn() -> Duration,
) -> bool {
    loop {
        let Some(left) = within.checked_sub(elapsed()).filter(|left| !left.is_zero()) else {
            return false;
        };
        if up(left) {
            return true;
        }
        let left = within.saturating_sub(elapsed());
        if left.is_zero() {
            return false;
        }
        sleep(again.min(left));
    }
}

/// One real look: the lamp's cloud API's name resolved, so the lamp has a
/// route and DNS, within `left` and at most [`LOOKUP_MOST`]. The lookup
/// runs on a thread of its own, left to end by itself when it is slower.
pub fn relay_reached(left: Duration) -> bool {
    use std::net::ToSocketAddrs;
    let (tell, told) = std::sync::mpsc::channel();
    let looked = std::thread::Builder::new()
        .name("history-dns".into())
        .spawn(move || {
            let found = (RELAY_HOST, 443)
                .to_socket_addrs()
                .is_ok_and(|mut found| found.next().is_some());
            let _ = tell.send(found);
        });
    looked.is_ok() && told.recv_timeout(left.min(LOOKUP_MOST)).unwrap_or(false)
}

/// The delete to `target`, each try by `post`, until done, a 401, or
/// `within` has passed.
pub fn send_within(
    target: &Target,
    within: Duration,
    mut post: impl FnMut(&str, &str, Duration) -> Result<u16, String>,
) -> Outcome {
    let until = Instant::now() + within;
    let mut tries = 0;
    loop {
        let Some(left) = until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
        else {
            return Outcome::Failed("no time left".into(), tries);
        };
        tries += 1;
        let why = match post(&target.url, &target.key, left) {
            Ok(status) if (200..300).contains(&status) => return Outcome::Done(status),
            Ok(401) => return Outcome::Gone,
            Ok(status) => format!("HTTP {status}"),
            Err(why) => why,
        };
        if until.saturating_duration_since(Instant::now()) < AGAIN {
            return Outcome::Failed(why, tries);
        }
        std::thread::sleep(AGAIN);
    }
}

/// One real try: [`WHOLE_LAMP_BODY`] posted to `url` with `key` in
/// `x-api-key`, connection and answer within `within`, no redirect.
pub fn post(url: &str, key: &str, within: Duration) -> Result<u16, String> {
    post_with(url, key, within, true)
}

fn post_with(url: &str, key: &str, within: Duration, https_only: bool) -> Result<u16, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "no runtime".to_owned())?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::keys::tls().ok_or("no TLS")?)
            .https_only(https_only)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(within)
            .timeout(within)
            .build()
            .map_err(|_| "no HTTPS client".to_owned())?;
        let request = client
            .post(url)
            .header("x-api-key", key)
            .header("content-type", "application/json")
            .body(WHOLE_LAMP_BODY)
            .send();
        // The error is dropped whole: reqwest's names the URL.
        match tokio::time::timeout(within, request).await {
            Err(_) => Err("no answer in time".to_owned()),
            Ok(Err(_)) => Err("no connection".to_owned()),
            Ok(Ok(response)) => Ok(response.status().as_u16()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("lamp-setup-history-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        /// Settings naming the lamp's cloud API and a key file beside them.
        fn config(&self) -> std::path::PathBuf {
            let key = self.0.join("credential");
            std::fs::write(&key, "relay-secret\n").unwrap();
            let config = self.0.join("second.json");
            std::fs::write(
                &config,
                serde_json::json!({
                    "llm_base_url": "https://campaign-api.autonomous.ai/api/v1/ai",
                    "llm_credential_file": key,
                })
                .to_string(),
            )
            .unwrap();
            config
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const ON: &str = "LAMP_VOICE_ROUTE=second LAMP_HISTORY_FORGET=on LAMP_HISTORY_SEND=on";

    #[test]
    fn the_switch_is_read_from_the_voices_environment() {
        assert!(on_in(ON));
        assert!(on_in("\"LAMP_HISTORY_FORGET=on\""));
        assert!(!on_in("LAMP_HISTORY_FORGET=on LAMP_HISTORY_FORGET=off"));
        assert!(!on_in("LAMP_HISTORY_FORGET_X=on"));
        assert!(!on_in(""));
    }

    #[test]
    fn only_the_lamps_own_cloud_api_with_the_relay_key() {
        let scratch = Scratch::new("target");
        assert_eq!(
            target(&scratch.config()),
            Ok(Target {
                url: "https://campaign-api.autonomous.ai/api/v1/ai/history/turns/delete".into(),
                key: "relay-secret".into(),
            })
        );
        assert_eq!(
            url("https://campaign-api.autonomous.ai/api/v1/ai/v1/"),
            Some("https://campaign-api.autonomous.ai/api/v1/ai/history/turns/delete".into())
        );
        assert_eq!(url("https://api.deepseek.com"), None);
        assert_eq!(url("http://campaign-api.autonomous.ai/api/v1/ai"), None);
        assert_eq!(
            target(&scratch.0.join("missing.json")),
            Err("no second route settings")
        );
    }

    /// Off, nothing is read or sent: the reset is as it was.
    #[test]
    fn off_nothing_is_sent() {
        let scratch = Scratch::new("off");
        let outcome = before_wipe(
            "LAMP_HISTORY_SEND=on",
            &scratch.config(),
            WITHIN,
            |_, _, _| panic!("nothing is sent with the switch off"),
        );
        assert_eq!(outcome, Outcome::Off);
    }

    /// The first builder's ruling: a 401 is the key already gone from the
    /// account, never tried again.
    #[test]
    fn a_401_is_not_tried_again() {
        let scratch = Scratch::new("401");
        let tries = RefCell::new(Vec::new());
        let outcome = before_wipe(ON, &scratch.config(), WITHIN, |url, key, _| {
            tries.borrow_mut().push((url.to_owned(), key.to_owned()));
            Ok(401)
        });
        assert_eq!(outcome, Outcome::Gone);
        assert_eq!(
            *tries.borrow(),
            [(
                "https://campaign-api.autonomous.ai/api/v1/ai/history/turns/delete".to_owned(),
                "relay-secret".to_owned()
            )]
        );
    }

    /// A 5xx is tried again within the bound; a 2xx after it is done.
    #[test]
    fn a_failed_try_is_tried_again_within_the_bound() {
        let scratch = Scratch::new("again");
        let mut answers = vec![Ok(200), Err("no connection".to_owned()), Ok(503)];
        let outcome = before_wipe(ON, &scratch.config(), WITHIN, |_, _, _| {
            answers.pop().unwrap()
        });
        assert_eq!(outcome, Outcome::Done(200));
        assert!(answers.is_empty());
    }

    /// A time the test moves: `sleep` adds to it.
    fn clock() -> std::rc::Rc<std::cell::Cell<Duration>> {
        std::rc::Rc::default()
    }

    /// Heron's P2 on #8125: a reset resumed at boot whose network comes up
    /// 20 s in waits for it, then deletes.
    #[test]
    fn a_resumed_reset_waits_for_the_network_then_deletes() {
        let scratch = Scratch::new("network-later");
        let now = clock();
        let (looks, sleeps) = (now.clone(), now.clone());
        let outcome = before_wipe_after(
            ON,
            &scratch.config(),
            WITHIN,
            || {
                network_within(
                    NETWORK_WITHIN,
                    NETWORK_AGAIN,
                    |_| looks.get() >= Duration::from_secs(20),
                    |wait| sleeps.set(sleeps.get() + wait),
                    || now.get(),
                )
            },
            |_, _, _| Ok(200),
        );
        assert_eq!(outcome, Outcome::Done(200));
        assert_eq!(now.get(), Duration::from_secs(20));
    }

    /// No network for the whole bound: nothing is sent, the reset goes on
    /// after [`NETWORK_WITHIN`], and the journal says the delete did not
    /// reach the account.
    #[test]
    fn a_resumed_reset_with_no_network_goes_on_after_the_bound() {
        let scratch = Scratch::new("network-never");
        let now = clock();
        let sleeps = now.clone();
        let outcome = before_wipe_after(
            ON,
            &scratch.config(),
            WITHIN,
            || {
                network_within(
                    NETWORK_WITHIN,
                    NETWORK_AGAIN,
                    |_| false,
                    |wait| sleeps.set(sleeps.get() + wait),
                    || now.get(),
                )
            },
            |_, _, _| panic!("nothing is sent with no network"),
        );
        assert_eq!(outcome, Outcome::NoNetwork);
        assert_eq!(now.get(), NETWORK_WITHIN);
        assert!(
            outcome
                .said()
                .contains("the delete did not reach the account"),
            "{}",
            outcome.said()
        );
        // Off, no network is waited for.
        let outcome = before_wipe_after(
            "LAMP_HISTORY_SEND=on",
            &scratch.config(),
            WITHIN,
            || panic!("no wait with the switch off"),
            |_, _, _| panic!("nothing is sent with the switch off"),
        );
        assert_eq!(outcome, Outcome::Off);
    }

    /// A dead network holds the reset only the bound, with a real request
    /// to a server that takes the connection and never answers.
    #[test]
    fn a_dead_network_waits_only_the_bound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let held = std::thread::spawn(move || {
            let kept: Vec<_> = listener.incoming().take(4).collect();
            std::thread::sleep(Duration::from_secs(5));
            drop(kept);
        });
        let target = Target {
            url: format!("http://{address}/history/turns/delete"),
            key: "k".into(),
        };
        let within = Duration::from_millis(1_500);
        let started = Instant::now();
        let outcome = send_within(&target, within, |url, key, left| {
            post_with(url, key, left, false)
        });
        let took = started.elapsed();
        assert!(matches!(outcome, Outcome::Failed(_, _)), "{outcome:?}");
        assert!(took < within + Duration::from_millis(500), "{took:?}");
        // And `before_wipe` itself, its tries never answered.
        let scratch = Scratch::new("dead");
        let started = Instant::now();
        let outcome = before_wipe(ON, &scratch.config(), within, |_, _, left| {
            std::thread::sleep(left);
            Err("no answer in time".to_owned())
        });
        assert_eq!(outcome, Outcome::Failed("no answer in time".into(), 1));
        assert!(started.elapsed() < within + Duration::from_millis(300));
        drop(held);
    }
}
