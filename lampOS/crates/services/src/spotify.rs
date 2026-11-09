//! Spotify (#5460), behind its own switch, `LAMP_SERVICES_SPOTIFY`, off
//! unless `on`: what is playing now, read by the owner's voice. The phone
//! links it with PKCE at Spotify itself and hands over only the refresh
//! token, the person's own client id and the granted scopes, through the
//! backend's personal-token route: `connector.set.spotify` with
//! `auth_type: "pat"`, the refresh token in `api_key`, `credentials.client_id`
//! and `credentials.scopes` (one space-separated string).
//!
//! The one exception to "a link is renewed through the backend": PKCE has
//! no client secret, so the lamp renews this link itself, `POST
//! https://accounts.spotify.com/api/token` with `grant_type=refresh_token`,
//! the refresh token and the client id. The access token it gets is kept in
//! this process's memory only, never in the store; a rotated refresh token
//! replaces the old one in the store, written whole (0600, root's). Only
//! `accounts.spotify.com` (that one route) and `api.spotify.com` (the one
//! read) are ever asked, exactly those hosts, over https, no redirect
//! followed. A refresh Spotify refuses (a revoked link among them) marks
//! the link expired and is said as "link it again in the app"; at `set` it
//! is a `failure` with the code [`Refusal::code`].
//!
//! Playback (#5460, "playback without a read-back"), behind a second
//! switch, `LAMP_SERVICES_PLAYBACK`, off unless `on` and only with
//! `LAMP_SERVICES_SPOTIFY=on` too: the owner's voice's `service_play` plays
//! a search result, pauses, resumes or skips on the person's active Spotify
//! device, at once, with no read-back (it is playback, not a send). It needs
//! the `user-modify-playback-state` scope among the link's; without it the
//! person is told to link Spotify again in the app. `service_write` for
//! Spotify is always refused.
use crate::catalog::Method;
use crate::net::{self, Call, Http, Reply};
use crate::store::{Entry, Store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// The service's code, the store's file name and the tools' `service`.
pub const CODE: &str = "spotify";
/// The service's name, as a person hears it.
pub const NAME: &str = "Spotify";
/// The switch for Spotify, read by `lamp-services`: off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_SERVICES_SPOTIFY";
/// Spotify's accounts host: the token route, and nothing else on it.
pub const ACCOUNTS_HOST: &str = "accounts.spotify.com";
/// Spotify's Web API host: the one read, and playback's few routes.
pub const API_HOST: &str = "api.spotify.com";
/// The switch for playback, read by `lamp-services`: off unless `on`, and
/// only with [`ENVIRONMENT`] on too.
pub const PLAYBACK_ENVIRONMENT: &str = "LAMP_SERVICES_PLAYBACK";
/// The token route, on [`ACCOUNTS_HOST`].
pub const TOKEN_PATH: &str = "/api/token";
/// The one read, on [`API_HOST`].
pub const NOW_PLAYING_PATH: &str = "/v1/me/player/currently-playing";
/// Playback's search, on [`API_HOST`]: a spoken name to one item.
pub const SEARCH_PATH: &str = "/v1/search";
/// Playback's play and resume (`PUT`), on [`API_HOST`].
pub const PLAY_PATH: &str = "/v1/me/player/play";
/// Playback's pause (`PUT`), on [`API_HOST`].
pub const PAUSE_PATH: &str = "/v1/me/player/pause";
/// Playback's next (`POST`), on [`API_HOST`].
pub const NEXT_PATH: &str = "/v1/me/player/next";
/// Playback's previous (`POST`), on [`API_HOST`].
pub const PREVIOUS_PATH: &str = "/v1/me/player/previous";
/// The scope playback needs among the link's.
pub const MODIFY_SCOPE: &str = "user-modify-playback-state";
/// The scopes the app asks for. The modify scope is for playback
/// ([`PLAYBACK_ENVIRONMENT`]).
pub const SCOPES: [&str; 3] = [
    "user-read-playback-state",
    "user-read-currently-playing",
    "user-modify-playback-state",
];
/// The longest a client id is taken: Spotify's are 32 hexadecimal digits.
pub const CLIENT_ID_MOST: usize = 64;
/// An access token is renewed once it has less than this left.
pub const ACCESS_SKEW_S: i64 = 60;
/// The longest an access token is held, whatever Spotify says.
pub const ACCESS_MOST_S: i64 = 3_600;
/// The whole read, a renewal included, under the voice's 20 s wait.
pub const WITHIN: Duration = Duration::from_secs(15);

/// What a call to Spotify is told while [`ENVIRONMENT`] is off.
pub const OFF: &str = "Spotify is off on this lamp";
/// What a refused renewal is said as, at `set` and at a read.
pub const RELINK: &str = "Spotify refused the lamp's link: link it again in the app";
/// What a renewal that could not be made is said as.
pub const UNREACHED: &str = "Spotify couldn't be reached: try again in a while";
/// What a Spotify write is told, and `service_play`, while playback is
/// off: the lamp only reads what is playing.
pub const NO_PLAYBACK: &str =
    "The lamp doesn't control Spotify's playback: it only reads what is playing";
/// What a Spotify write is told while playback is on: it is never a write.
pub const PLAY_NOT_WRITE: &str =
    "Spotify isn't written to: play, pause or skip it with service_play";
/// What playback is told when the link lacks [`MODIFY_SCOPE`].
pub const NO_PLAY_SCOPE: &str =
    "Spotify's link doesn't allow playing: link Spotify again in the app to allow playback";
/// What playback is told when Spotify has no active device (404).
pub const NO_DEVICE: &str =
    "Nothing is open to play on: open Spotify on your phone or speaker first";
/// What playback is told when the account isn't Premium (403).
pub const PREMIUM: &str = "Playing from the lamp needs Spotify Premium";
/// Playback's actions, for the tool's description and a wrong one.
pub const ACTIONS: [&str; 5] = ["play", "pause", "resume", "next", "previous"];
/// What a play may search for (`kind`), a track unless said.
pub const KINDS: [&str; 4] = ["track", "artist", "album", "playlist"];
/// The longest spoken name searched for, in bytes.
pub const QUERY_MOST: usize = 200;
/// What Spotify reads, for the tools' description and a wrong `what`.
pub const WHATS: &str = "now_playing";
/// The one item of a read with nothing playing.
pub const NOTHING_PLAYING: &str = "Nothing is playing on Spotify right now.";

/// Why a renewal did not give an access token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Spotify refused the refresh token or the client id (a revoke among
    /// them): the person links it again in the app.
    Relink,
    /// Spotify couldn't be reached, or answered in a way the lamp can't
    /// use: the link is kept as it was.
    Unreached,
}

impl Refusal {
    /// The words said, never with what Spotify answered.
    pub fn words(self) -> &'static str {
        match self {
            Self::Relink => RELINK,
            Self::Unreached => UNREACHED,
        }
    }

    /// The failure's code, said in the app's `data.code` at `set`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Relink => "relink",
            Self::Unreached => "unreached",
        }
    }
}

/// Why playback, or the access token it needs, did not happen: the words
/// said and a code the tools socket gives beside them (`code`), never with
/// what Spotify answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub words: String,
    pub code: &'static str,
}

impl Refused {
    fn new(words: impl Into<String>, code: &'static str) -> Self {
        Self {
            words: words.into(),
            code,
        }
    }
}

impl From<Refusal> for Refused {
    fn from(refusal: Refusal) -> Self {
        Self::new(refusal.words(), refusal.code())
    }
}

/// Whether `call` may go to Spotify: https, exactly one of its two hosts,
/// no user, port or fragment, and only the token route, the one read and
/// playback's search, play, pause, next and previous, each by its method.
pub fn guard(call: &Call) -> Result<(), String> {
    let (host, path) = net::split(&call.url)?;
    let host = host.to_ascii_lowercase();
    let allowed = matches!(
        (call.method, host.as_str(), path.as_str()),
        (Method::Post, ACCOUNTS_HOST, TOKEN_PATH)
            | (Method::Get, API_HOST, NOW_PLAYING_PATH | SEARCH_PATH)
            | (Method::Put, API_HOST, PLAY_PATH | PAUSE_PATH)
            | (Method::Post, API_HOST, NEXT_PATH | PREVIOUS_PATH)
    );
    if allowed {
        Ok(())
    } else if host != ACCOUNTS_HOST && host != API_HOST {
        Err(format!(
            "the lamp sends a Spotify token only to {ACCOUNTS_HOST} and {API_HOST}"
        ))
    } else {
        Err("the lamp doesn't make that call to Spotify".into())
    }
}

/// One call to Spotify: [`guard`] first, then sent. A redirect is never
/// followed: it comes back as its own status.
pub async fn send<H: Http>(http: &H, call: Call) -> Result<Reply, String> {
    guard(&call)?;
    http.send(call).await
}

/// `application/x-www-form-urlencoded` text of `pairs`.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("https://form.invalid/").expect("a fixed URL parses");
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    }
    url.query().unwrap_or_default().to_owned()
}

/// The renewal request for `refresh_token` and `client_id`: PKCE's, no
/// client secret and no `Authorization` header.
pub fn refresh_call(refresh_token: &str, client_id: &str) -> Call {
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ]);
    Call {
        method: Method::Post,
        url: format!("https://{ACCOUNTS_HOST}{TOKEN_PATH}"),
        headers: vec![
            (
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            ),
            ("accept".into(), "application/json".into()),
            ("user-agent".into(), "lampOS-services".into()),
        ],
        body: Some(body.into_bytes()),
    }
}

/// Spotify's token answer. No `Debug`: it holds the tokens.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

/// Spotify's error answer: only its `error` code is read.
#[derive(Default, Deserialize)]
#[serde(default)]
struct TokenError {
    error: String,
}

/// A renewal's result. No `Debug`: it holds the tokens.
pub struct Fresh {
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// The rotated refresh token, when Spotify gave one.
    pub refresh_token: Option<String>,
}

/// Fit for a header: printable, one line, not too long.
fn fit(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= crate::store::SECRET_MOST
        && !token.chars().any(char::is_control)
}

/// The token answer read: 200 with an access token and a lifetime is
/// [`Fresh`]; a refused grant or client (400 `invalid_grant`,
/// `invalid_client` or `unauthorized_client`, or 401) is
/// [`Refusal::Relink`]; anything else, a redirect among them,
/// [`Refusal::Unreached`].
pub fn read_refresh(reply: &Reply, now: i64) -> Result<Fresh, Refusal> {
    match reply.status {
        200 => {}
        401 => return Err(Refusal::Relink),
        400 => {
            let error: TokenError = serde_json::from_slice(&reply.body).unwrap_or_default();
            return Err(
                if ["invalid_grant", "invalid_client", "unauthorized_client"]
                    .contains(&error.error.as_str())
                {
                    Refusal::Relink
                } else {
                    Refusal::Unreached
                },
            );
        }
        _ => return Err(Refusal::Unreached),
    }
    let token: Token = serde_json::from_slice(&reply.body).map_err(|_| Refusal::Unreached)?;
    if !fit(&token.access_token) || token.expires_in <= 0 {
        return Err(Refusal::Unreached);
    }
    let refresh_token = (!token.refresh_token.is_empty()).then_some(token.refresh_token);
    if refresh_token
        .as_deref()
        .is_some_and(|rotated| !fit(rotated))
    {
        return Err(Refusal::Unreached);
    }
    Ok(Fresh {
        access_token: token.access_token,
        expires_at: now + token.expires_in.min(ACCESS_MOST_S),
        refresh_token,
    })
}

/// A client id the lamp sends: letters and digits only, not too long.
fn client_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= CLIENT_ID_MOST && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `set`'s checks of a Spotify link, after the common ones: a personal
/// token (`pat`) whose `api_key` is the refresh token, no access token, and
/// the client id. The entry is made never to reach the backend's renewal:
/// no `refresh`, no `refresh_token`, no expiry, so it is renewed here only.
pub fn check_set(entry: &mut Entry) -> Result<(), &'static str> {
    if !entry.auth_type.eq_ignore_ascii_case("pat") {
        return Err("Spotify's link is a personal token (pat)");
    }
    if !entry.access_token.is_empty() || entry.api_key.is_empty() {
        return Err("Spotify's link is its refresh token alone");
    }
    let client_id = entry
        .credentials
        .get("client_id")
        .filter(|id| !id.is_empty())
        .cloned()
        .unwrap_or_else(|| entry.client_id.clone());
    if !client_id_ok(&client_id) {
        return Err("Spotify's link needs its client id");
    }
    entry.client_id = client_id;
    if entry.scopes.is_empty() {
        entry.scopes = entry
            .credentials
            .get("scopes")
            .map(|scopes| scopes.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
    }
    entry.refresh = false;
    entry.refresh_token.clear();
    entry.expires_at = 0;
    Ok(())
}

/// The link marked expired, after Spotify refused its renewal: `list`
/// says it `expired`, and every read says to link it again until it is.
pub fn mark_relink(entry: &mut Entry, now: i64) {
    entry.expires_at = (now - 1).max(1);
}

/// The access token held in memory, and the refresh token now good.
struct Held {
    /// The store's refresh token when this was made: a link set since
    /// has another, and this is dropped.
    stored: String,
    /// The refresh token to renew with: a rotated one even if it could
    /// not be kept.
    refresh_token: String,
    access_token: String,
    expires_at: i64,
}

/// Spotify's state in this process: the access token, in memory only, one
/// renewal at a time (a rotated refresh token is good once, so two at once
/// would refuse each other).
#[derive(Default)]
pub struct Spotify {
    held: tokio::sync::Mutex<Option<Held>>,
}

impl std::fmt::Debug for Spotify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Spotify { held: <hidden> }")
    }
}

impl Spotify {
    /// The access token forgotten: the link removed, every link wiped.
    pub async fn forget(&self) {
        *self.held.lock().await = None;
    }

    /// `set`'s renewal: the link proved by one refresh, its rotated refresh
    /// token put in `entry` before it is kept, the access token held.
    pub async fn link<H: Http>(
        &self,
        http: &H,
        entry: &mut Entry,
        now: i64,
    ) -> Result<(), Refusal> {
        let mut held = self.held.lock().await;
        *held = None;
        let fresh = renew(http, &entry.api_key, &entry.client_id, now).await?;
        if let Some(rotated) = fresh.refresh_token {
            entry.api_key = rotated;
        }
        *held = Some(Held {
            stored: entry.api_key.clone(),
            refresh_token: entry.api_key.clone(),
            access_token: fresh.access_token,
            expires_at: fresh.expires_at,
        });
        Ok(())
    }

    /// An access token for the kept link, `entry` (loaded under the lock):
    /// the one held while it has over [`ACCESS_SKEW_S`] left, else a
    /// renewal, its rotated refresh token kept whole in `store`. A refused
    /// renewal marks the link expired in the store.
    async fn access<H: Http>(
        &self,
        http: &H,
        store: &Store,
        now: i64,
    ) -> Result<(String, Entry), Refused> {
        let mut held = self.held.lock().await;
        let mut entry = store.load(CODE).ok_or_else(not_linked)?;
        if entry.expired(now) {
            *held = None;
            return Err(Refusal::Relink.into());
        }
        if held
            .as_ref()
            .is_some_and(|held| held.stored != entry.api_key)
        {
            *held = None;
        }
        if let Some(held) = held
            .as_ref()
            .filter(|held| now + ACCESS_SKEW_S < held.expires_at)
        {
            return Ok((held.access_token.clone(), entry));
        }
        let refresh_token = held
            .as_ref()
            .map_or_else(|| entry.api_key.clone(), |held| held.refresh_token.clone());
        let fresh = match renew(http, &refresh_token, &entry.client_id, now).await {
            Ok(fresh) => fresh,
            Err(Refusal::Relink) => {
                *held = None;
                mark_relink(&mut entry, now);
                if store.keep(&entry).is_err() {
                    eprintln!("lamp-services: spotify's refused link not marked");
                }
                eprintln!("lamp-services: spotify's link not renewed: Spotify refused it");
                return Err(Refusal::Relink.into());
            }
            Err(Refusal::Unreached) => {
                eprintln!("lamp-services: spotify's link not renewed: not reached");
                return Err(Refusal::Unreached.into());
            }
        };
        let latest = fresh.refresh_token.unwrap_or(refresh_token);
        let mut stored = entry.api_key.clone();
        if latest != entry.api_key {
            let mut next = entry.clone();
            next.api_key = latest.clone();
            next.obtained_at = now;
            if store.keep(&next).is_ok() {
                stored = latest.clone();
                entry = next;
            } else {
                // Held in memory, so the next renewal still has it.
                eprintln!("lamp-services: spotify's rotated link not kept");
            }
        }
        *held = Some(Held {
            stored,
            refresh_token: latest,
            access_token: fresh.access_token.clone(),
            expires_at: fresh.expires_at,
        });
        Ok((fresh.access_token, entry))
    }

    /// `service_read` of Spotify: `what` `now_playing` alone, within
    /// [`WITHIN`]. The answer is quoted as every read's is.
    pub async fn read<H: Http>(
        &self,
        http: &H,
        store: &Store,
        args: &Value,
    ) -> Result<Value, String> {
        let what = args.get("what").and_then(Value::as_str).map(str::trim);
        if what != Some("now_playing") {
            return Err(format!("{CODE} reads: {WHATS}"));
        }
        let read = async {
            let (access, entry) = self
                .access(http, store, crate::now_secs())
                .await
                .map_err(|refused| refused.words)?;
            let call = Call {
                method: Method::Get,
                url: format!("https://{API_HOST}{NOW_PLAYING_PATH}?additional_types=track,episode"),
                headers: vec![
                    ("accept".into(), "application/json".into()),
                    ("user-agent".into(), "lampOS-services".into()),
                    ("authorization".into(), format!("Bearer {access}")),
                ],
                body: None,
            };
            let reply = send(http, call).await?;
            match reply.status {
                200 => {}
                204 => return Ok(crate::reads::quoted(NAME, vec![json!(NOTHING_PLAYING)])),
                401 => {
                    // The access token refused early: the next read renews.
                    *self.held.lock().await = None;
                    return Err(format!(
                        "{NAME} refused the lamp's link: it may need linking again in the app"
                    ));
                }
                403 => {
                    return Err(format!(
                        "{NAME} refused the lamp's link: it may need linking again in the app"
                    ));
                }
                429 => return Err(format!("{NAME} is busy: try again in a while")),
                _ => return Err(format!("{NAME} answered with an error")),
            }
            let text = String::from_utf8_lossy(&reply.body);
            let text = net::scrub(&net::scrub(&text, &access), &entry.api_key);
            Ok(crate::reads::quoted(NAME, vec![now_playing(&text)]))
        };
        tokio::time::timeout(WITHIN, read)
            .await
            .map_err(|_| format!("{NAME} didn't answer in time"))?
    }

    /// `service_play` of Spotify, behind [`PLAYBACK_ENVIRONMENT`] (the
    /// switches are the caller's to check): `action` one of [`ACTIONS`] on
    /// the person's active device, at once and with no read-back, within
    /// [`WITHIN`]. A play searches `query` as `kind` (a track unless said)
    /// and plays the first item found; a play with no `query` resumes. A
    /// link without [`MODIFY_SCOPE`] is refused before Spotify is asked
    /// anything. The item played is quoted as a read's is.
    pub async fn play<H: Http>(
        &self,
        http: &H,
        store: &Store,
        args: &Value,
    ) -> Result<Value, Refused> {
        let asked = asked(args)?;
        let entry = store.load(CODE).ok_or_else(not_linked)?;
        if !entry.scopes.iter().any(|scope| scope == MODIFY_SCOPE) {
            return Err(Refused::new(NO_PLAY_SCOPE, "relink"));
        }
        let play = async {
            let (access, entry) = self.access(http, store, crate::now_secs()).await?;
            let unreached = |_| Refused::new(UNREACHED, "unreached");
            let (call, shown) = match &asked {
                Asked::Play { query, kind } => {
                    let reply = send(http, search_call(&access, query, kind))
                        .await
                        .map_err(unreached)?;
                    self.player_answer(&reply, false).await?;
                    let text = String::from_utf8_lossy(&reply.body);
                    let text = net::scrub(&net::scrub(&text, &access), &entry.api_key);
                    let Some(found) = found(&text, kind) else {
                        return Err(Refused::new(
                            format!("{NAME} found nothing called \"{query}\""),
                            "not_found",
                        ));
                    };
                    (play_call(&access, kind, &found.uri), Some(found.shown))
                }
                Asked::Pause => (player_call(Method::Put, PAUSE_PATH, &access), None),
                Asked::Resume => (player_call(Method::Put, PLAY_PATH, &access), None),
                Asked::Next => (player_call(Method::Post, NEXT_PATH, &access), None),
                Asked::Previous => (player_call(Method::Post, PREVIOUS_PATH, &access), None),
            };
            let reply = send(http, call).await.map_err(unreached)?;
            self.player_answer(&reply, true).await?;
            let mut done = match shown {
                Some(shown) => crate::reads::quoted(NAME, vec![shown]),
                None => json!({}),
            };
            done["done"] = json!(asked.done());
            Ok(done)
        };
        tokio::time::timeout(WITHIN, play)
            .await
            .map_err(|_| Refused::new(format!("{NAME} didn't answer in time"), "unreached"))?
    }

    /// Spotify's answer to a search or a player call, as said: 2xx is done;
    /// 401 forgets the held token (the next call renews); 403 is Premium
    /// needed, a scope missing, or refused; 404 on a player call is no
    /// active device; 429 busy.
    async fn player_answer(&self, reply: &Reply, player: bool) -> Result<(), Refused> {
        let error: SpotifyError = serde_json::from_slice(&reply.body).unwrap_or_default();
        match reply.status {
            200..=204 => Ok(()),
            401 => {
                *self.held.lock().await = None;
                Err(Refused::new(
                    format!("{NAME} refused the lamp's link: it may need linking again in the app"),
                    "relink",
                ))
            }
            403 if error.error.reason == "PREMIUM_REQUIRED" => {
                Err(Refused::new(PREMIUM, "premium"))
            }
            403 if error.error.message.to_ascii_lowercase().contains("scope") => {
                Err(Refused::new(NO_PLAY_SCOPE, "relink"))
            }
            403 => Err(Refused::new(
                format!("{NAME} wouldn't do that just now"),
                "refused",
            )),
            404 if player => Err(Refused::new(NO_DEVICE, "no_device")),
            429 => Err(Refused::new(
                format!("{NAME} is busy: try again in a while"),
                "busy",
            )),
            _ => Err(Refused::new(
                format!("{NAME} answered with an error"),
                "error",
            )),
        }
    }
}

/// What the voice's model is told when Spotify isn't linked.
fn not_linked() -> Refused {
    Refused::new(format!("{NAME} {}", crate::serve::NOT_LINKED), "not_linked")
}

/// Spotify's Web API error answer: only its reason and message are read,
/// and neither is said.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SpotifyError {
    error: SpotifyErrorBody,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SpotifyErrorBody {
    message: String,
    reason: String,
}

/// One playback action, as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Play { query: String, kind: &'static str },
    Pause,
    Resume,
    Next,
    Previous,
}

impl Asked {
    /// What the answer's `done` says was done.
    fn done(&self) -> &'static str {
        match self {
            Self::Play { .. } => "playing",
            Self::Pause => "paused",
            Self::Resume => "resumed",
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
}

/// `service_play`'s arguments read: `action`, and for a play `query` and
/// `kind`. A play with no query resumes.
fn asked(args: &Value) -> Result<Asked, Refused> {
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
    };
    let wrong = || {
        Refused::new(
            format!(
                "{CODE} plays: {} (play takes query and kind: {})",
                ACTIONS.join(", "),
                KINDS.join(", ")
            ),
            "invalid",
        )
    };
    Ok(match text("action") {
        "play" => {
            let query = text("query");
            if query.is_empty() {
                return Ok(Asked::Resume);
            }
            if query.len() > QUERY_MOST || query.chars().any(char::is_control) {
                return Err(Refused::new(
                    "Say a shorter name of what to play",
                    "invalid",
                ));
            }
            let kind = match text("kind") {
                "" => "track",
                kind => KINDS
                    .iter()
                    .copied()
                    .find(|known| *known == kind)
                    .ok_or_else(wrong)?,
            };
            Asked::Play {
                query: query.to_owned(),
                kind,
            }
        }
        "pause" => Asked::Pause,
        "resume" => Asked::Resume,
        "next" => Asked::Next,
        "previous" => Asked::Previous,
        _ => return Err(wrong()),
    })
}

/// The headers of a Web API call with `access` as the bearer.
fn api_headers(access: &str) -> Vec<(String, String)> {
    vec![
        ("accept".into(), "application/json".into()),
        ("user-agent".into(), "lampOS-services".into()),
        ("authorization".into(), format!("Bearer {access}")),
    ]
}

/// The search for `query` as `kind`: a few items, playable for the
/// person's own market.
fn search_call(access: &str, query: &str, kind: &str) -> Call {
    let query = form(&[
        ("q", query),
        ("type", kind),
        ("limit", "5"),
        ("market", "from_token"),
    ]);
    Call {
        method: Method::Get,
        url: format!("https://{API_HOST}{SEARCH_PATH}?{query}"),
        headers: api_headers(access),
        body: None,
    }
}

/// A player call with an empty body, said as `Content-Length: 0`: Spotify
/// answers a `PUT` or `POST` without one 411, and the client sends none
/// for an empty body.
fn player_call(method: Method, path: &str, access: &str) -> Call {
    let mut headers = api_headers(access);
    headers.push(("content-length".into(), "0".into()));
    Call {
        method,
        url: format!("https://{API_HOST}{path}"),
        headers,
        body: None,
    }
}

/// Playing `uri`: a track by its URI, an artist, album or playlist as the
/// context.
fn play_call(access: &str, kind: &str, uri: &str) -> Call {
    let body = if kind == "track" {
        json!({ "uris": [uri] })
    } else {
        json!({ "context_uri": uri })
    };
    let mut headers = api_headers(access);
    headers.push(("content-type".into(), "application/json".into()));
    Call {
        method: Method::Put,
        url: format!("https://{API_HOST}{PLAY_PATH}"),
        headers,
        body: Some(body.to_string().into_bytes()),
    }
}

/// The item a search found: its URI and what is said of it.
struct Found {
    uri: String,
    shown: Value,
}

/// A Spotify URI of `kind`: `spotify:<kind>:` and an id of letters and
/// digits.
fn uri_ok(uri: &str, kind: &str) -> bool {
    uri.strip_prefix("spotify:")
        .and_then(|rest| rest.strip_prefix(kind))
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|id| {
            !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

/// The first item of `kind` in a search's answer with a good URI, made
/// small: its type, title, and artists, or a playlist's owner. Nothing
/// else of Spotify's answer is kept.
fn found(body: &str, kind: &str) -> Option<Found> {
    let answer: Value = serde_json::from_str(body).ok()?;
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|text| !text.is_empty())
    };
    let item = answer
        .get(format!("{kind}s"))?
        .get("items")?
        .as_array()?
        .iter()
        .find(|item| text(item, "uri").is_some_and(|uri| uri_ok(&uri, kind)))?;
    let mut shown = serde_json::Map::new();
    shown.insert("type".into(), json!(kind));
    if let Some(title) = text(item, "name") {
        shown.insert("title".into(), json!(title));
    }
    let artists: Vec<String> = item
        .get("artists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| text(artist, "name"))
        .take(5)
        .collect();
    if !artists.is_empty() {
        shown.insert("artists".into(), json!(artists.join(", ")));
    }
    if let Some(owner) = item
        .get("owner")
        .and_then(|owner| text(owner, "display_name"))
    {
        shown.insert("by".into(), json!(owner));
    }
    Some(Found {
        uri: text(item, "uri")?,
        shown: Value::Object(shown),
    })
}

/// One renewal: the call, guarded, and its answer read.
async fn renew<H: Http>(
    http: &H,
    refresh_token: &str,
    client_id: &str,
    now: i64,
) -> Result<Fresh, Refusal> {
    let reply = send(http, refresh_call(refresh_token, client_id))
        .await
        .map_err(|_| Refusal::Unreached)?;
    read_refresh(&reply, now)
}

/// What is playing, made small: whether it plays, what it is (a track or
/// an episode), its title, its artists, album or show. Nothing else of
/// Spotify's answer (the device, links, ids) is kept.
pub fn now_playing(body: &str) -> Value {
    let Ok(answer) = serde_json::from_str::<Value>(body) else {
        return json!(NOTHING_PLAYING);
    };
    let Some(item) = answer.get("item").filter(|item| item.is_object()) else {
        return json!(NOTHING_PLAYING);
    };
    let text = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|text| !text.is_empty())
    };
    let mut shown = serde_json::Map::new();
    shown.insert(
        "playing".into(),
        json!(
            answer
                .get("is_playing")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
    );
    if let Some(kind) = text(item, "type") {
        shown.insert("type".into(), json!(kind));
    }
    if let Some(title) = text(item, "name") {
        shown.insert("title".into(), json!(title));
    }
    let artists: Vec<String> = item
        .get("artists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| text(artist, "name"))
        .take(5)
        .collect();
    if !artists.is_empty() {
        shown.insert("artists".into(), json!(artists.join(", ")));
    }
    if let Some(album) = item.get("album").and_then(|album| text(album, "name")) {
        shown.insert("album".into(), json!(album));
    }
    if let Some(show) = item.get("show").and_then(|show| text(show, "name")) {
        shown.insert("show".into(), json!(show));
    }
    Value::Object(shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// One request as the fake server saw it.
    #[derive(Clone, Debug)]
    struct Seen {
        line: String,
        headers: String,
        body: String,
    }

    /// A local HTTP server, on 127.0.0.1 only, answering each request with
    /// the next of its scripted answers and keeping what it saw.
    struct FakeSpotify {
        port: u16,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeSpotify {
        fn start(answers: Vec<String>) -> FakeSpotify {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let kept = seen.clone();
            std::thread::spawn(move || {
                for answer in answers {
                    let Ok((stream, _)) = listener.accept() else {
                        return;
                    };
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let mut headers = String::new();
                    let mut length = 0usize;
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header.trim().is_empty() {
                            break;
                        }
                        let lower = header.to_ascii_lowercase();
                        if let Some(value) = lower.strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap();
                        }
                        headers.push_str(&lower);
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    kept.lock().unwrap().push(Seen {
                        line: line.trim().to_owned(),
                        headers,
                        body: String::from_utf8(body).unwrap(),
                    });
                    let mut stream = stream;
                    stream.write_all(answer.as_bytes()).unwrap();
                }
            });
            FakeSpotify { port, seen }
        }

        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    /// An answer: its status line, then JSON or nothing.
    fn answer(status: &str, body: &str, extra: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
            body.len()
        )
    }

    /// The product's client (no redirect, its timeouts), sending each
    /// Spotify URL to the fake server over plain HTTP: the guard has seen
    /// the https URL first, in [`send`].
    struct Local(u16);

    impl Http for Local {
        async fn send(&self, call: Call) -> Result<Reply, String> {
            let url = reqwest::Url::parse(&call.url).unwrap();
            let mut local = call;
            local.url = format!(
                "http://127.0.0.1:{}{}{}",
                self.0,
                url.path(),
                url.query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default()
            );
            net::send_on(&net::client(false)?, local).await
        }
    }

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn link(refresh_token: &str) -> Entry {
        let data = json!({
            "connector": "spotify",
            "auth_type": "pat",
            "api_key": refresh_token,
            "credentials": {
                "client_id": "0123456789abcdef0123456789abcdef",
                "scopes": "user-read-playback-state user-read-currently-playing user-modify-playback-state"
            }
        });
        Entry::from_set(CODE, &data, 100).unwrap()
    }

    const PLAYING: &str = r#"{"is_playing":true,"progress_ms":1000,"device":{"name":"Desk","id":"dev-1"},"item":{"type":"track","name":"Clair de Lune","artists":[{"name":"Debussy"}],"album":{"name":"Suite"},"uri":"spotify:track:x"}}"#;

    #[test]
    fn a_link_is_its_refresh_token_and_client_id_and_never_the_backends() {
        let entry = link("rt-planted-1");
        assert_eq!(entry.api_key, "rt-planted-1");
        assert_eq!(entry.client_id, "0123456789abcdef0123456789abcdef");
        assert_eq!(entry.scopes, SCOPES);
        assert!(entry.access_token.is_empty() && entry.refresh_token.is_empty());
        assert!(!entry.refresh && entry.expires_at == 0);
        assert!(!entry.needs_refresh(i64::MAX / 2) && !entry.expired(i64::MAX / 2));
        assert!(!format!("{entry:?}").contains("planted"));
        for refused in [
            json!({"auth_type":"oauth","access_token":"a","credentials":{"client_id":"c"}}),
            json!({"auth_type":"pat","api_key":"r"}),
            json!({"auth_type":"pat","api_key":"r","credentials":{"client_id":"c/../x"}}),
            json!({"auth_type":"pat","api_key":"r","access_token":"a","credentials":{"client_id":"c"}}),
        ] {
            assert!(Entry::from_set(CODE, &refused, 0).is_err(), "{refused}");
        }
        // A link given a lifetime and a refresh flag still never reaches
        // the backend's renewal.
        let timed = json!({"auth_type":"pat","api_key":"r","refresh_token":"r","refresh":true,
                           "expires_in":10,"client_id":"abc123"});
        let timed = Entry::from_set(CODE, &timed, 0).unwrap();
        assert!(!timed.needs_refresh(100) && timed.refresh_token.is_empty());
        assert_eq!(timed.client_id, "abc123");
    }

    #[test]
    fn only_the_two_hosts_and_their_two_routes_are_called() {
        let fake = FakeSpotify::start(vec![]);
        let call = |method, url: &str| Call {
            method,
            url: url.into(),
            headers: vec![],
            body: None,
        };
        assert!(
            guard(&call(
                Method::Post,
                "https://accounts.spotify.com/api/token"
            ))
            .is_ok()
        );
        assert!(
            guard(&call(
                Method::Get,
                "https://api.spotify.com/v1/me/player/currently-playing?additional_types=track"
            ))
            .is_ok()
        );
        for (method, url) in [
            (Method::Post, "http://accounts.spotify.com/api/token"),
            (
                Method::Post,
                "https://accounts.spotify.com.evil.test/api/token",
            ),
            (Method::Post, "https://evil.test/api/token"),
            (Method::Post, "https://x.accounts.spotify.com/api/token"),
            (Method::Post, "https://accounts.spotify.com:8443/api/token"),
            (Method::Post, "https://me@accounts.spotify.com/api/token"),
            (Method::Get, "https://accounts.spotify.com/api/token"),
            (Method::Post, "https://accounts.spotify.com/authorize"),
            (
                Method::Get,
                "https://spotify.com/v1/me/player/currently-playing",
            ),
            (
                Method::Get,
                "https://evil.api.spotify.com/v1/me/player/currently-playing",
            ),
            (Method::Get, "https://api.spotify.com/v1/me/player"),
            (Method::Patch, "https://api.spotify.com/v1/me/player/play"),
            (Method::Post, "https://api.spotify.com/v1/me/player/play"),
            (Method::Put, "https://api.spotify.com/v1/me/player/next"),
            (Method::Get, "https://api.spotify.com/v1/me/player/pause"),
            (Method::Put, "https://api.spotify.com/v1/me/player/volume"),
            (Method::Post, "https://api.spotify.com/v1/me/player/queue"),
            (Method::Put, "https://api.spotify.com/v1/me/player"),
            (Method::Post, "https://api.spotify.com/v1/search"),
            (Method::Get, "https://evil.test/v1/search?q=x"),
            (
                Method::Get,
                "https://api.spotify.com/v1/me/player/currently-playing#x",
            ),
        ] {
            let refused = run(send(&Local(fake.port), call(method, url)));
            assert!(refused.is_err(), "{method:?} {url}");
        }
        // Playback's routes, each by its own method alone.
        for (method, url) in [
            (
                Method::Get,
                "https://api.spotify.com/v1/search?q=x&type=track",
            ),
            (Method::Put, "https://api.spotify.com/v1/me/player/play"),
            (Method::Put, "https://api.spotify.com/v1/me/player/pause"),
            (Method::Post, "https://api.spotify.com/v1/me/player/next"),
            (
                Method::Post,
                "https://api.spotify.com/v1/me/player/previous",
            ),
        ] {
            assert!(guard(&call(method, url)).is_ok(), "{url}");
        }
        // Nothing reached the server.
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn a_renewal_is_pkces_with_no_secret_and_a_rotation_is_kept_whole() {
        let fake = FakeSpotify::start(vec![
            answer(
                "200 OK",
                r#"{"access_token":"acc-planted-1","token_type":"Bearer","expires_in":3600,"refresh_token":"rt-planted-2","scope":"user-read-currently-playing"}"#,
                "",
            ),
            answer("200 OK", PLAYING, ""),
            answer(
                "200 OK",
                r#"{"access_token":"acc-planted-2","token_type":"Bearer","expires_in":3600}"#,
                "",
            ),
            answer("204 No Content", "", ""),
        ]);
        let http = Local(fake.port);
        let dir = tempfile::tempdir().unwrap();
        let store = Store(dir.path().join("s"));
        let spotify = Spotify::default();
        let mut entry = link("rt-planted-1");
        run(spotify.link(&http, &mut entry, crate::now_secs())).unwrap();
        // The rotated refresh token is the one kept.
        assert_eq!(entry.api_key, "rt-planted-2");
        store.keep(&entry).unwrap();
        let seen = fake.seen();
        assert_eq!(seen[0].line, "POST /api/token HTTP/1.1");
        assert!(
            seen[0]
                .headers
                .contains("content-type: application/x-www-form-urlencoded")
        );
        assert!(
            !seen[0].headers.contains("authorization"),
            "{}",
            seen[0].headers
        );
        assert_eq!(
            seen[0].body,
            "grant_type=refresh_token&refresh_token=rt-planted-1&client_id=0123456789abcdef0123456789abcdef"
        );
        // A read uses the access token held: no second renewal.
        let read = run(spotify.read(&http, &store, &json!({"what": "now_playing"}))).unwrap();
        let seen = fake.seen();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[1]
                .line
                .starts_with("GET /v1/me/player/currently-playing?")
        );
        assert!(
            seen[1]
                .headers
                .contains("authorization: bearer acc-planted-1")
        );
        assert_eq!(read["source"], "Spotify");
        assert_eq!(
            read["quoted"][0],
            json!({"playing": true, "type": "track", "title": "Clair de Lune",
                   "artists": "Debussy", "album": "Suite"})
        );
        // The access token is never kept; the file is root's alone.
        let file = std::fs::read_to_string(store.0.join("spotify.json")).unwrap();
        assert!(!file.contains("acc-planted") && file.contains("rt-planted-2"));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.0.join("spotify.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!store.0.join(".spotify.json.new").exists());
        // A restart holds nothing: the kept, rotated token renews.
        let restarted = Spotify::default();
        let read = run(restarted.read(&http, &store, &json!({"what": "now_playing"}))).unwrap();
        assert_eq!(read["quoted"][0], NOTHING_PLAYING);
        let seen = fake.seen();
        assert!(
            seen[2].body.contains("refresh_token=rt-planted-2"),
            "{}",
            seen[2].body
        );
        assert!(
            seen[3]
                .headers
                .contains("authorization: bearer acc-planted-2")
        );
        // No rotation that time: the kept token is unchanged.
        assert_eq!(store.load(CODE).unwrap().api_key, "rt-planted-2");
        // Only now_playing is read.
        let other = run(restarted.read(&http, &store, &json!({"what": "get", "path": "/v1/me"})));
        assert_eq!(other.unwrap_err(), "spotify reads: now_playing");
        assert_eq!(fake.seen().len(), 4);
    }

    #[test]
    fn a_revoked_link_says_link_it_again_and_is_marked_expired() {
        let fake = FakeSpotify::start(vec![
            answer(
                "400 Bad Request",
                r#"{"error":"invalid_grant","error_description":"Refresh token revoked"}"#,
                "",
            ),
            answer("401 Unauthorized", r#"{"error":"invalid_client"}"#, ""),
            answer("400 Bad Request", r#"{"error":"invalid_grant"}"#, ""),
        ]);
        let http = Local(fake.port);
        let spotify = Spotify::default();
        // At set: refused, with its code.
        let mut entry = link("rt-planted-1");
        assert_eq!(
            run(spotify.link(&http, &mut entry, crate::now_secs())).err(),
            Some(Refusal::Relink)
        );
        assert_eq!(Refusal::Relink.code(), "relink");
        assert_eq!(
            run(spotify.link(&http, &mut entry, crate::now_secs())).err(),
            Some(Refusal::Relink)
        );
        // At a read: said as "link it again", and the link marked.
        let dir = tempfile::tempdir().unwrap();
        let store = Store(dir.path().join("s"));
        store.keep(&link("rt-planted-1")).unwrap();
        let read = run(spotify.read(&http, &store, &json!({"what": "now_playing"})));
        assert_eq!(read.unwrap_err(), RELINK);
        assert_eq!(
            store.list(crate::now_secs()),
            vec![json!({"code": "spotify", "account": "", "expired": true})]
        );
        // Marked, it is not asked of Spotify again until linked again.
        let again = run(spotify.read(&http, &store, &json!({"what": "now_playing"})));
        assert_eq!(again.unwrap_err(), RELINK);
        assert_eq!(fake.seen().len(), 3);
    }

    #[test]
    fn a_redirect_is_never_followed() {
        let fake = FakeSpotify::start(vec![
            answer("302 Found", "", "Location: https://evil.test/api/token\r\n"),
            answer(
                "307 Temporary Redirect",
                "",
                "Location: /v1/me/player/currently-playing\r\n",
            ),
        ]);
        let http = Local(fake.port);
        let spotify = Spotify::default();
        let mut entry = link("rt-planted-1");
        assert_eq!(
            run(spotify.link(&http, &mut entry, crate::now_secs())).err(),
            Some(Refusal::Unreached)
        );
        // Not followed: one request, and the second answer still waiting.
        assert_eq!(fake.seen().len(), 1);
        let reply = run(send(&http, refresh_call("rt-planted-1", "abc"))).unwrap();
        assert_eq!(reply.status, 307);
        assert_eq!(fake.seen().len(), 2);
        // An unreached renewal keeps the link as it was.
        assert_eq!(entry.api_key, "rt-planted-1");
    }

    #[test]
    fn the_token_answer_is_read_without_trusting_it() {
        let reply = |status, body: &str| Reply {
            status,
            body: body.as_bytes().to_vec(),
            ..Reply::default()
        };
        let fresh = read_refresh(
            &reply(200, r#"{"access_token":"a","expires_in":999999}"#),
            10,
        )
        .ok()
        .unwrap();
        assert_eq!(fresh.expires_at, 10 + ACCESS_MOST_S);
        assert!(fresh.refresh_token.is_none());
        for (status, body, refusal) in [
            (200, r#"{"access_token":"a"}"#, Refusal::Unreached),
            (
                200,
                r#"{"access_token":"","expires_in":5}"#,
                Refusal::Unreached,
            ),
            (
                200,
                r#"{"access_token":"a\nb","expires_in":5}"#,
                Refusal::Unreached,
            ),
            (
                200,
                r#"{"access_token":"a","expires_in":5,"refresh_token":"r\r\nx"}"#,
                Refusal::Unreached,
            ),
            (200, "not json", Refusal::Unreached),
            (400, r#"{"error":"invalid_request"}"#, Refusal::Unreached),
            (400, r#"{"error":"unauthorized_client"}"#, Refusal::Relink),
            (401, "", Refusal::Relink),
            (500, "", Refusal::Unreached),
            (302, "", Refusal::Unreached),
        ] {
            assert_eq!(
                read_refresh(&reply(status, body), 0).err(),
                Some(refusal),
                "{status} {body}"
            );
        }
        // An episode by its show; an ad or a private session as nothing.
        assert_eq!(
            now_playing(
                r#"{"is_playing":false,"item":{"type":"episode","name":"E1","show":{"name":"Talk"}}}"#
            ),
            json!({"playing": false, "type": "episode", "title": "E1", "show": "Talk"})
        );
        assert_eq!(
            now_playing(r#"{"is_playing":true,"item":null,"currently_playing_type":"ad"}"#),
            json!(NOTHING_PLAYING)
        );
    }

    const TOKEN: &str =
        r#"{"access_token":"acc-planted-1","token_type":"Bearer","expires_in":3600}"#;
    const TRACKS: &str = r#"{"tracks":{"items":[{"name":"Clair de Lune","uri":"spotify:track:4u7EnebtmKWzUH433cf5Qv","artists":[{"name":"Debussy"}],"album":{"name":"Suite"},"id":"4u7"}]}}"#;
    /// Spotify's search may give a playlist as null: the first good one is
    /// taken.
    const PLAYLISTS: &str = r#"{"playlists":{"items":[null,{"name":"Focus","uri":"spotify:playlist:37i9dQZF1DX","owner":{"display_name":"Spotify"}}]}}"#;

    fn stored(dir: &std::path::Path, scopes: &str) -> Store {
        let store = Store(dir.join("s"));
        let data = json!({
            "connector": "spotify",
            "auth_type": "pat",
            "api_key": "rt-planted-1",
            "credentials": {"client_id": "0123456789abcdef0123456789abcdef", "scopes": scopes}
        });
        store
            .keep(&Entry::from_set(CODE, &data, 100).unwrap())
            .unwrap();
        store
    }

    const ALL_SCOPES: &str =
        "user-read-playback-state user-read-currently-playing user-modify-playback-state";

    /// A play searches the spoken name and plays the first item on the
    /// active device (a track by its URI, a playlist as the context); pause,
    /// resume, next and previous are one call each, the access token held.
    #[test]
    fn playback_plays_a_search_result_and_pauses_resumes_and_skips() {
        let done = || answer("204 No Content", "", "");
        let fake = FakeSpotify::start(vec![
            answer("200 OK", TOKEN, ""),
            answer("200 OK", TRACKS, ""),
            done(),
            answer("200 OK", PLAYLISTS, ""),
            answer("202 Accepted", "", ""),
            done(),
            done(),
            done(),
            done(),
            done(),
        ]);
        let http = Local(fake.port);
        let dir = tempfile::tempdir().unwrap();
        let store = stored(dir.path(), ALL_SCOPES);
        let spotify = Spotify::default();
        let played = run(spotify.play(
            &http,
            &store,
            &json!({"action": "play", "query": "clair de lune"}),
        ))
        .unwrap();
        assert_eq!(played["done"], "playing");
        assert_eq!(played["source"], "Spotify");
        assert_eq!(
            played["quoted"][0],
            json!({"type": "track", "title": "Clair de Lune", "artists": "Debussy"})
        );
        let seen = fake.seen();
        assert_eq!(seen[0].line, "POST /api/token HTTP/1.1");
        assert!(
            seen[1].line.starts_with(
                "GET /v1/search?q=clair+de+lune&type=track&limit=5&market=from_token "
            ),
            "{}",
            seen[1].line
        );
        assert!(
            seen[1]
                .headers
                .contains("authorization: bearer acc-planted-1")
        );
        assert_eq!(seen[2].line, "PUT /v1/me/player/play HTTP/1.1");
        assert_eq!(
            serde_json::from_str::<Value>(&seen[2].body).unwrap(),
            json!({"uris": ["spotify:track:4u7EnebtmKWzUH433cf5Qv"]})
        );
        assert!(
            seen[2]
                .headers
                .contains("authorization: bearer acc-planted-1")
        );
        // A playlist, by name: its context, the null item passed over.
        let played = run(spotify.play(
            &http,
            &store,
            &json!({"action": "play", "query": "focus", "kind": "playlist"}),
        ))
        .unwrap();
        assert_eq!(
            played["quoted"][0],
            json!({"type": "playlist", "title": "Focus", "by": "Spotify"})
        );
        let seen = fake.seen();
        assert!(seen[3].line.contains("type=playlist"));
        assert_eq!(
            serde_json::from_str::<Value>(&seen[4].body).unwrap(),
            json!({"context_uri": "spotify:playlist:37i9dQZF1DX"})
        );
        // Pause, resume (and a play with nothing named), next, previous.
        for (action, line, said) in [
            ("pause", "PUT /v1/me/player/pause HTTP/1.1", "paused"),
            ("resume", "PUT /v1/me/player/play HTTP/1.1", "resumed"),
            ("play", "PUT /v1/me/player/play HTTP/1.1", "resumed"),
            ("next", "POST /v1/me/player/next HTTP/1.1", "next"),
            (
                "previous",
                "POST /v1/me/player/previous HTTP/1.1",
                "previous",
            ),
        ] {
            let done = run(spotify.play(&http, &store, &json!({"action": action}))).unwrap();
            assert_eq!(done, json!({"done": said}), "{action}");
            let last = fake.seen().last().unwrap().clone();
            assert_eq!(last.line, line, "{action}");
            assert!(last.headers.contains("content-length: 0"), "{action}");
            assert!(last.body.is_empty(), "{action}");
        }
        // One renewal for all of it, and no token ever kept or said.
        assert_eq!(fake.seen().len(), 10);
        let file = std::fs::read_to_string(store.0.join("spotify.json")).unwrap();
        assert!(!file.contains("acc-planted"));
    }

    /// Spotify's refusals said plainly, with their codes: no active device
    /// (404), Premium needed (403), the access token refused (401, then
    /// renewed), and nothing found.
    #[test]
    fn playback_says_no_device_premium_relink_and_nothing_found() {
        let fake = FakeSpotify::start(vec![
            answer("200 OK", TOKEN, ""),
            answer(
                "404 Not Found",
                r#"{"error":{"status":404,"message":"Player command failed: No active device found","reason":"NO_ACTIVE_DEVICE"}}"#,
                "",
            ),
            answer(
                "403 Forbidden",
                r#"{"error":{"status":403,"message":"Player command failed: Premium required","reason":"PREMIUM_REQUIRED"}}"#,
                "",
            ),
            answer(
                "401 Unauthorized",
                r#"{"error":{"status":401,"message":"The access token expired"}}"#,
                "",
            ),
            answer(
                "200 OK",
                r#"{"access_token":"acc-planted-2","token_type":"Bearer","expires_in":3600}"#,
                "",
            ),
            answer("200 OK", r#"{"tracks":{"items":[]}}"#, ""),
            answer(
                "200 OK",
                r#"{"artists":{"items":[{"name":"X","uri":"https://evil.test/x"}]}}"#,
                "",
            ),
            answer(
                "403 Forbidden",
                r#"{"error":{"status":403,"message":"Insufficient client scope"}}"#,
                "",
            ),
            answer("429 Too Many Requests", "", ""),
        ]);
        let http = Local(fake.port);
        let dir = tempfile::tempdir().unwrap();
        let store = stored(dir.path(), ALL_SCOPES);
        let spotify = Spotify::default();
        let refused = |args: Value| run(spotify.play(&http, &store, &args)).unwrap_err();
        assert_eq!(
            refused(json!({"action": "pause"})),
            Refused::new(NO_DEVICE, "no_device")
        );
        assert!(NO_DEVICE.contains("open Spotify on your phone or speaker first"));
        assert_eq!(
            refused(json!({"action": "next"})),
            Refused::new(PREMIUM, "premium")
        );
        let expired = refused(json!({"action": "resume"}));
        assert_eq!(expired.code, "relink");
        // The held token was forgotten: the next call renews.
        let nothing = refused(json!({"action": "play", "query": "zzqx"}));
        assert_eq!(
            nothing,
            Refused::new("Spotify found nothing called \"zzqx\"", "not_found")
        );
        let seen = fake.seen();
        assert_eq!(seen[4].line, "POST /api/token HTTP/1.1");
        assert!(
            seen[5]
                .headers
                .contains("authorization: bearer acc-planted-2")
        );
        // An item whose URI isn't Spotify's is never played.
        let odd = refused(json!({"action": "play", "query": "x", "kind": "artist"}));
        assert_eq!(odd.code, "not_found");
        assert_eq!(fake.seen().len(), 7);
        assert_eq!(
            refused(json!({"action": "previous"})),
            Refused::new(NO_PLAY_SCOPE, "relink")
        );
        assert_eq!(refused(json!({"action": "pause"})).code, "busy");
        assert_eq!(fake.seen().len(), 9);
        // No answer's words are ever said, nor a token.
        for words in [expired.words, odd.words] {
            assert!(!words.contains("expired") && !words.contains("planted"));
        }
    }

    /// A link without the modify scope, a wrong action or kind, and no
    /// link at all: refused before Spotify is asked anything.
    #[test]
    fn playback_without_its_scope_or_a_good_ask_calls_nothing() {
        let fake = FakeSpotify::start(vec![]);
        let http = Local(fake.port);
        let dir = tempfile::tempdir().unwrap();
        let spotify = Spotify::default();
        let unlinked = Store(dir.path().join("none"));
        let none = run(spotify.play(&http, &unlinked, &json!({"action": "pause"}))).unwrap_err();
        assert_eq!(none.code, "not_linked");
        assert!(none.words.contains("Connectors"));
        let store = stored(
            dir.path(),
            "user-read-playback-state user-read-currently-playing",
        );
        let scope = run(spotify.play(&http, &store, &json!({"action": "pause"}))).unwrap_err();
        assert_eq!(scope, Refused::new(NO_PLAY_SCOPE, "relink"));
        assert!(NO_PLAY_SCOPE.contains("link Spotify again in the app"));
        for args in [
            json!({"action": "volume", "level": 100}),
            json!({}),
            json!({"action": "play", "query": "x", "kind": "episode"}),
            json!({"action": "play", "query": "x".repeat(QUERY_MOST + 1)}),
            json!({"action": "play", "query": "a\nb"}),
        ] {
            let refused = run(spotify.play(&http, &store, &args)).unwrap_err();
            assert_eq!(refused.code, "invalid", "{args}");
        }
        assert!(fake.seen().is_empty());
    }
}
