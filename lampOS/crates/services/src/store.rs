//! The tokens' store: `/var/lib/lampos-services/<code>.json`, one file a
//! service, root's, 0600, in a 0700 folder, each written whole (beside,
//! synced, renamed). Only this program reads or writes it.
//!
//! A token is in its file and the request to its own service, nowhere else:
//! [`Entry`]'s `Debug` hides every secret, and nothing here says one in an
//! error.
use crate::catalog;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// The store's folder on a lamp.
pub const DIR: &str = "/var/lib/lampos-services";
/// The longest credential kept, in bytes.
pub const SECRET_MOST: usize = 8_192;
/// How long an OAuth token lives when the backend says nothing of it and
/// the link can be renewed (V1's `defaultOAuthTokenLifetime`): the lamp
/// then asks the backend for a new one within the hour. A link the lamp
/// can't renew keeps no such guessed end (`expires_at`).
pub const DEFAULT_LIFETIME_S: i64 = 3_600;
/// A token is refreshed once it has less than this left (V1's
/// `connectorRefreshSkew`).
pub const REFRESH_SKEW_S: i64 = 600;

/// Shown in place of a secret.
const HIDDEN: &str = "<hidden>";

/// One linked service, as V1's `ConnectorCreds` keeps it.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub connector: String,
    pub auth_type: String,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    /// Unix seconds; 0 for a credential that never ends by time: a static
    /// one, or an OAuth link given no time that the lamp can't renew.
    pub expires_at: i64,
    pub api_key: String,
    pub scopes: Vec<String>,
    pub user_email: String,
    pub client_id: String,
    /// Connector-specific extras (Gmail's `email`, a Page's `page_id`, an
    /// MCP service's `mcp_url` and `mcp_auth_header`).
    pub credentials: BTreeMap<String, String>,
    /// Refresh allowed (V1's flag; a refresh token is needed too).
    pub refresh: bool,
    pub obtained_at: i64,
    /// A random id given when the app links it, kept through a renewal:
    /// which link a made skill's write was approved under, where the link
    /// names no account (`skill_writes`). Never a token's.
    pub link_id: String,
    /// The service itself refused this link (a 401, or its own words for a
    /// dead token) while it had no end by time: kept so, and shown expired,
    /// until the app links it again (a new entry).
    pub refused: bool,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hidden = |secret: &str| if secret.is_empty() { "" } else { HIDDEN };
        f.debug_struct("Entry")
            .field("connector", &self.connector)
            .field("auth_type", &self.auth_type)
            .field("access_token", &hidden(&self.access_token))
            .field("refresh_token", &hidden(&self.refresh_token))
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .field("api_key", &hidden(&self.api_key))
            .field("scopes", &self.scopes)
            .field("user_email", &self.user_email)
            .field("client_id", &self.client_id)
            // The extras' names only: a value may be a secret.
            .field("credentials", &self.credentials.keys().collect::<Vec<_>>())
            .field("refresh", &self.refresh)
            .field("obtained_at", &self.obtained_at)
            .field("refused", &self.refused)
            .finish()
    }
}

/// V1's `MQTTConnectorSetData`: the `data` the backend's
/// `get-message?kind=connector.set.<code>` returns. No `Debug`: it holds
/// the token.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SetData {
    connector: String,
    auth_type: String,
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: i64,
    expires_at: i64,
    api_key: String,
    scopes: Vec<String>,
    user_email: String,
    client_id: String,
    credentials: BTreeMap<String, String>,
    refresh: bool,
}

/// Whether a credential is a static secret (an app password, a personal
/// key) rather than an OAuth access token (V1's `staticConnectorCredential`).
fn static_credential(auth_type: &str, access_token: &str, api_key: &str) -> bool {
    access_token.is_empty() && (!api_key.is_empty() || auth_type.trim().eq_ignore_ascii_case("pat"))
}

/// When a credential expires (V1's `resolveConnectorExpiresAt`): the time
/// given, else from now by the lifetime given; with neither, never (0) for a
/// static one and for an OAuth link the lamp can't renew (`renewable`: a
/// refresh token and `refresh`), else [`DEFAULT_LIFETIME_S`] from now.
///
/// An OAuth link given no time is often one that never ends at its service
/// (a GitHub OAuth App's token, a Slack token without rotation): a guessed
/// hour, with nothing to renew it, only ended it at the lamp. Such a link
/// is kept until the service itself refuses it ([`Entry::refused`]).
pub fn expires_at(
    auth_type: &str,
    access_token: &str,
    api_key: &str,
    renewable: bool,
    at: i64,
    within: i64,
    now: i64,
) -> i64 {
    if at > 0 {
        at
    } else if within > 0 {
        now + within
    } else if static_credential(auth_type, access_token, api_key) || !renewable {
        0
    } else {
        now + DEFAULT_LIFETIME_S
    }
}

/// A secret a request can carry in a header or a login: printable, with
/// no line break or control character, and not too long.
fn fit_secret(secret: &str) -> bool {
    secret.len() <= SECRET_MOST && !secret.chars().any(char::is_control)
}

impl Entry {
    /// The entry the app's `connector.set.<code>` data makes, or why not,
    /// in words that never hold the data's values.
    pub fn from_set(code: &str, data: &Value, now: i64) -> Result<Entry, &'static str> {
        if catalog::service(code).is_none() {
            return Err("the lamp doesn't know that service");
        }
        let data: SetData = serde_json::from_value(data.clone())
            .map_err(|_| "the connector's data is not in its shape")?;
        if !data.connector.is_empty() && data.connector != code {
            return Err("the connector's data names another service");
        }
        if data.auth_type.trim().is_empty() {
            return Err("auth_type is required");
        }
        if data.access_token.is_empty() && data.api_key.is_empty() {
            return Err("the connector's data holds no credential");
        }
        if ![&data.access_token, &data.refresh_token, &data.api_key]
            .into_iter()
            .all(|secret| fit_secret(secret))
            || !data.credentials.values().all(|value| fit_secret(value))
        {
            return Err("the credential holds what a request can't carry");
        }
        let entry = Entry {
            connector: code.to_owned(),
            expires_at: expires_at(
                &data.auth_type,
                &data.access_token,
                &data.api_key,
                data.refresh && !data.refresh_token.is_empty(),
                data.expires_at,
                data.expires_in,
                now,
            ),
            auth_type: data.auth_type.trim().to_owned(),
            access_token: data.access_token,
            refresh_token: data.refresh_token,
            token_type: data.token_type,
            api_key: data.api_key,
            scopes: data.scopes,
            user_email: data.user_email,
            client_id: data.client_id,
            credentials: data.credentials,
            refresh: data.refresh,
            obtained_at: now,
            link_id: crate::writes::random_id(),
            refused: false,
        };
        // An app password needs its account; a Google sign-in's account is
        // read from Gmail itself at `set` when the data has none.
        if code == "gmail" && !entry.oauth() && entry.account().is_empty() {
            return Err("Gmail needs the account's email");
        }
        // A send-only Google sign-in is a token and its account, never an
        // app password: Gmail can't tell its address without a read scope.
        if code == "gmail-app" {
            if !entry.oauth() {
                return Err("Gmail's Google sign-in needs its access token");
            }
            if entry.account().is_empty() {
                return Err("Gmail's Google sign-in needs the account's email");
            }
        }
        // An MCP link's address and header are checked before it is kept:
        // its token goes only to the service's own MCP host.
        if let Some(service) = catalog::service(code).filter(|service| service.mcp.is_some()) {
            crate::mcp::check_set(service, &entry)?;
        }
        Ok(entry)
    }

    /// The secret sent: the access token, else the static key.
    pub fn token(&self) -> &str {
        if self.access_token.is_empty() {
            &self.api_key
        } else {
            &self.access_token
        }
    }

    /// Whether it is an OAuth link (an access token, not `pat`) rather than
    /// a static key: for Gmail, a Google sign-in rather than an app password.
    pub fn oauth(&self) -> bool {
        !self.access_token.is_empty() && !self.auth_type.eq_ignore_ascii_case("pat")
    }

    /// The account's name, as V1's `account_of` says it.
    pub fn account(&self) -> String {
        let email = self
            .credentials
            .get("email")
            .filter(|email| !email.is_empty());
        if self.auth_type == "pat"
            && let Some(email) = email
        {
            return email.clone();
        }
        if !self.user_email.is_empty() {
            return self.user_email.clone();
        }
        email.cloned().unwrap_or_default()
    }

    /// Whether it has expired by `now`, or its service refused it.
    pub fn expired(&self, now: i64) -> bool {
        self.refused || (self.expires_at != 0 && self.expires_at < now)
    }

    /// Whether its end is known only from its service: an OAuth link with
    /// no end by time, which a refusal from the service marks expired.
    pub fn end_unknown(&self) -> bool {
        self.oauth() && self.expires_at == 0
    }

    /// Whether it should be refreshed by `now` (V1's `connectorNeedsRefresh`).
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.refresh
            && !self.refresh_token.is_empty()
            && self.expires_at != 0
            && now + REFRESH_SKEW_S >= self.expires_at
    }
}

/// The store's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store(pub PathBuf);

impl Store {
    fn folder(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.0)?;
        fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700))
    }

    /// A known service's file; `None` for any other code, so a code is never
    /// a path of its own.
    pub fn path(&self, code: &str) -> Option<PathBuf> {
        catalog::service(code).map(|service| self.0.join(format!("{}.json", service.code)))
    }

    /// Keep an entry whole: written beside, 0600, synced, then renamed over.
    pub fn keep(&self, entry: &Entry) -> io::Result<()> {
        let path = self
            .path(&entry.connector)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown service"))?;
        self.folder()?;
        let beside = self.0.join(format!(".{}.json.new", entry.connector));
        let text = serde_json::to_vec(entry).map_err(io::Error::other)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&beside)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&text)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&beside, &path)?;
        sync(&self.0)
    }

    /// The entry kept for `code`, if one is there and reads.
    pub fn load(&self, code: &str) -> Option<Entry> {
        let entry: Entry = serde_json::from_slice(&fs::read(self.path(code)?).ok()?).ok()?;
        (entry.connector == code).then_some(entry)
    }

    /// Remove `code`'s file; one not there is removed already.
    pub fn remove(&self, code: &str) -> io::Result<()> {
        let path = self
            .path(code)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown service"))?;
        let _ = fs::remove_file(self.0.join(format!(".{code}.json.new")));
        match fs::remove_file(path) {
            Ok(()) => sync(&self.0),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Remove everything in the folder (unpairing, a reset), the folder
    /// itself kept for its unit.
    pub fn wipe(&self) -> io::Result<()> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        sync(&self.0)
    }

    /// `[{"code","account","expired"}]` for each linked service, in the
    /// catalog's order. Never a secret.
    pub fn list(&self, now: i64) -> Vec<Value> {
        catalog::SERVICES
            .iter()
            .filter_map(|service| self.load(service.code))
            .map(|entry| {
                json!({
                    "code": entry.connector,
                    "account": entry.account(),
                    "expired": entry.refused
                        || (entry.expired(now) && entry.refresh_token.is_empty()),
                })
            })
            .collect()
    }
}

fn sync(folder: &Path) -> io::Result<()> {
    fs::File::open(folder)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn gmail_data() -> Value {
        json!({
            "connector": "gmail",
            "auth_type": "pat",
            "api_key": "abcd efgh ijkl mnop",
            "credentials": {"email": "a@b.c"},
            "scopes": ["mail"]
        })
    }

    #[test]
    fn the_store_is_root_only_and_written_whole() {
        let root = tempfile::tempdir().unwrap();
        let store = Store(root.path().join("lampos-services"));
        let entry = Entry::from_set("gmail", &gmail_data(), 100).unwrap();
        store.keep(&entry).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&store.0), 0o700);
        assert_eq!(mode(&store.0.join("gmail.json")), 0o600);
        assert!(!store.0.join(".gmail.json.new").exists());
        assert_eq!(store.load("gmail"), Some(entry.clone()));
        // A file made wider before is narrowed at the next keep.
        fs::set_permissions(&store.0, fs::Permissions::from_mode(0o755)).unwrap();
        store.keep(&entry).unwrap();
        assert_eq!(mode(&store.0), 0o700);
        // Gmail's app password never expires.
        assert_eq!(entry.expires_at, 0);
        assert_eq!(entry.account(), "a@b.c");
        assert_eq!(
            store.list(100),
            vec![json!({"code":"gmail","account":"a@b.c","expired":false})]
        );
        // A code is never a path.
        assert!(store.path("../etc/passwd").is_none());
        assert!(store.load("../gmail").is_none());
    }

    #[test]
    fn an_oauth_link_given_no_end_and_no_refresh_token_never_ends_by_time() {
        let root = tempfile::tempdir().unwrap();
        let store = Store(root.path().join("s"));
        let now = 1_000;
        let two_hours = now + 2 * 3_600;
        // A GitHub OAuth App token, given no time: kept past the old
        // guessed hour, and not marked to be renewed.
        let github = json!({"auth_type":"oauth","access_token":"gho_x"});
        let entry = Entry::from_set("github", &github, now).unwrap();
        assert_eq!(entry.expires_at, 0);
        assert!(entry.end_unknown() && !entry.refused);
        assert!(!entry.expired(two_hours));
        assert!(!entry.needs_refresh(two_hours));
        store.keep(&entry).unwrap();
        assert_eq!(store.list(two_hours)[0]["expired"], false);
        // A refresh token the lamp isn't allowed to use renews nothing
        // either.
        let unused = json!({"auth_type":"oauth","access_token":"t","refresh_token":"r"});
        assert_eq!(
            Entry::from_set("linear", &unused, now).unwrap().expires_at,
            0
        );
        // Marked refused, it is expired whatever its time, and listed so.
        let mut refused = entry.clone();
        refused.refused = true;
        assert!(refused.expired(now));
        store.keep(&refused).unwrap();
        assert_eq!(store.list(now)[0]["expired"], true);
        // Linked again, a new entry, the mark gone.
        store
            .keep(&Entry::from_set("github", &github, now).unwrap())
            .unwrap();
        assert_eq!(store.list(two_hours)[0]["expired"], false);
        // A link given its time still ends at it.
        let timed = json!({"auth_type":"oauth","access_token":"gho_x","expires_in":3600});
        let entry = Entry::from_set("github", &timed, now).unwrap();
        assert_eq!(entry.expires_at, now + 3_600);
        assert!(!entry.end_unknown());
        assert!(!entry.expired(now + 3_600) && entry.expired(now + 3_601));
        let at = json!({"auth_type":"oauth","access_token":"gho_x","expires_at":5_000});
        assert_eq!(
            Entry::from_set("github", &at, now).unwrap().expires_at,
            5_000
        );
        // One the lamp renews keeps the guessed hour, so it is renewed
        // before it.
        let renewable =
            json!({"auth_type":"oauth","access_token":"t","refresh_token":"r","refresh":true});
        let entry = Entry::from_set("notion", &renewable, now).unwrap();
        assert_eq!(entry.expires_at, now + DEFAULT_LIFETIME_S);
        assert!(!entry.end_unknown());
        assert!(!entry.needs_refresh(now) && entry.needs_refresh(now + 3_000));
        // A static key still never ends.
        let key = json!({"auth_type":"api_key","api_key":"k"});
        let entry = Entry::from_set("linear", &key, now).unwrap();
        assert_eq!(entry.expires_at, 0);
        assert!(!entry.end_unknown() && !entry.expired(two_hours));
    }

    #[test]
    fn remove_and_wipe_delete_the_files() {
        let root = tempfile::tempdir().unwrap();
        let store = Store(root.path().join("s"));
        store
            .keep(&Entry::from_set("gmail", &gmail_data(), 1).unwrap())
            .unwrap();
        let github = json!({"auth_type":"oauth","access_token":"gho_x","expires_in":3600});
        store
            .keep(&Entry::from_set("github", &github, 1).unwrap())
            .unwrap();
        store.remove("gmail").unwrap();
        assert!(!store.0.join("gmail.json").exists());
        assert!(store.0.join("github.json").exists());
        store.remove("gmail").unwrap();
        fs::write(store.0.join(".linear.json.new"), "x").unwrap();
        store.wipe().unwrap();
        assert_eq!(fs::read_dir(&store.0).unwrap().count(), 0);
        assert!(store.0.is_dir());
        assert!(store.list(1).is_empty());
        Store(root.path().join("never")).wipe().unwrap();
    }

    #[test]
    fn debug_never_shows_a_secret() {
        let data = json!({
            "auth_type": "oauth",
            "access_token": "planted-access-4710",
            "refresh_token": "planted-refresh-4710",
            "api_key": "planted-key-4710",
            "credentials": {"email": "a@b.c", "password": "planted-extra-4710"},
            "refresh": true,
            "expires_in": 60
        });
        let entry = Entry::from_set("notion", &data, 10).unwrap();
        let shown = format!("{entry:?} {entry:#?}");
        assert!(!shown.contains("planted"), "{shown}");
        assert!(shown.contains("<hidden>"));
        assert_eq!(entry.expires_at, 70);
        assert!(entry.needs_refresh(10));
        // A refusal never repeats the data.
        let wrong = json!({"auth_type": 5, "access_token": "planted-access-4710"});
        let why = Entry::from_set("notion", &wrong, 0).unwrap_err();
        assert!(!why.contains("planted"));
        let other = json!({"connector":"github","auth_type":"oauth","access_token":"planted"});
        assert!(Entry::from_set("notion", &other, 0).is_err());
        let broken = json!({"auth_type":"oauth","access_token":"planted\r\nX: y"});
        assert!(Entry::from_set("notion", &broken, 0).is_err());
        let no_email = json!({"auth_type":"pat","api_key":"k"});
        assert!(Entry::from_set("gmail", &no_email, 0).is_err());
        // A Google sign-in's account is read from Gmail at `set`.
        let signed_in = json!({"auth_type":"oauth","access_token":"t","expires_in":60});
        assert!(Entry::from_set("gmail", &signed_in, 0).unwrap().oauth());
        assert!(!Entry::from_set("gmail", &gmail_data(), 0).unwrap().oauth());
        assert!(Entry::from_set("nothing", &gmail_data(), 0).is_err());
        // A send-only sign-in: its token and its account, or nothing kept.
        let app = json!({"auth_type":"oauth","access_token":"t","user_email":"me@home.test",
                         "refresh_token":"r","expires_in":3599,"refresh":true,"client_id":"c"});
        let kept = Entry::from_set("gmail-app", &app, 0).unwrap();
        assert!(kept.oauth() && kept.refresh);
        assert_eq!(kept.account(), "me@home.test");
        let by_credentials =
            json!({"auth_type":"oauth","access_token":"t","credentials":{"email":"me@home.test"}});
        assert_eq!(
            Entry::from_set("gmail-app", &by_credentials, 0)
                .unwrap()
                .account(),
            "me@home.test"
        );
        for refused in [
            json!({"auth_type":"oauth","access_token":"t"}),
            json!({"auth_type":"pat","api_key":"pw","credentials":{"email":"me@home.test"}}),
        ] {
            assert!(
                Entry::from_set("gmail-app", &refused, 0).is_err(),
                "{refused}"
            );
        }
    }
}
