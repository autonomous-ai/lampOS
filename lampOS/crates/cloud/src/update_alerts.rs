//! Alerts about the lamp's own software update, to the backend's alert
//! endpoint (#1683, `docs/cloud.md`, "Software update alerts"). They go
//! behind the same three gates as health and crash reports: `LAMP_CLOUD=on`,
//! `LAMP_REPORTS=on`, and the person's yes, read at each send through the
//! reports' own reading ([`crate::reports::Reports::consent`]). Nothing is
//! sent or retried without the yes. The small file of what was told is kept
//! either way: it holds no personal data (outcomes, builds, times), and it
//! is what stops a stage seen before the yes being sent after it.
//!
//! One alert per stage of a round: `started`, `done`, `failed`, `refused`,
//! `rolled-back`, and `self-switched`, `self-confirmed` and `self-put-back` under their own names; and `not-switched`
//! for a round that began (its `started` was told) and ended with nothing
//! switched, so no start is left without an end. Each is built
//! from `update/status` (`lamp_msgs::UpdateStatus`) and a fixed template,
//! and every value in it is typed so that nothing that came in can ride
//! along: the stage is one of [`Stage`], the running build is digits and
//! hex digits and `+` as a report's build is, a reason is one of
//! [`REASONS`], the time it took is a number. The updater's reason text is
//! never on the topic, and this reads none of its records. A canary test
//! plants secrets in every input.
//!
//! `update/status` repeats the last result every minute and says it again
//! whenever lamp-update or lamp-cloud starts, and a round can restart
//! lamp-cloud itself. So the last end told, and the round whose start was
//! told, are kept in a small file of lamp-cloud's own state folder, and an
//! alert goes once however often the value is said or lamp-cloud begins.
//! The first value found with no such file is taken as already told.
//! Best effort: one try with a timeout, a failure said in the journal and
//! dropped.
use crate::account::Account;
use crate::online::Say;
use crate::reports::{self, Shared};
use lamp_msgs::{UpdateResult, UpdateStatus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// How often `update/status` is looked at.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);
/// The longest round whose time is told: a longer one is a clock that
/// moved, not a round.
const LONGEST_TOOK_S: u64 = 24 * 60 * 60;
/// The longest alert, in bytes, without the device's id.
const MOST_BYTES: usize = 512;

/// The alert's type, as the lamp's other ops alerts carry it.
pub const TYPE: &str = "ops";

/// The file of what was told, in lamp-cloud's state folder.
pub const FILE: &str = "update-alerts.json";

/// The reason names lamp-update may say in `last.reason`: the last phase a
/// round said, or `models`. A name not in this list is not told.
pub const REASONS: &[&str] = &UpdateResult::REASONS;

/// Where the alert goes: the backend's base as the account has it, with
/// `/alert` after it. Unlike the ping, a trailing `/v1` stays: the alert
/// lives beside the OpenAI-compatible routes. `None` without a base.
pub fn url(account: &Account) -> Option<String> {
    let base = account.llm_base_url.trim().trim_end_matches('/');
    (!base.is_empty()).then(|| format!("{base}/alert"))
}

/// One stage of a round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Started,
    Done,
    Failed,
    Refused,
    RolledBack,
    /// lamp-update switched itself; its new self confirms on its next
    /// round, so this is not a round's `done`.
    SelfSwitched,
    /// lamp-update's new self confirmed the switch it was started by.
    SelfConfirmed,
    /// The old lamp-update was put back because the new one did not take;
    /// it names no build, reason or start time.
    SelfPutBack,
    /// A round whose start was told ended with nothing switched; its code
    /// is the outcome's own name from [`NOT_SWITCHED`].
    NotSwitched,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::RolledBack => "rolled-back",
            Self::SelfSwitched => "self-switched",
            Self::SelfConfirmed => "self-confirmed",
            Self::SelfPutBack => "self-put-back",
            Self::NotSwitched => "not-switched",
        }
    }
}

/// The stage a round's outcome (`Outcome::kind()`, as `update/status` says
/// it) is told as. The outcomes that changed nothing, and any this build
/// does not know, are told as none.
fn told_as(outcome: &str) -> Option<Stage> {
    match outcome {
        "switched" => Some(Stage::Done),
        "self-switched" => Some(Stage::SelfSwitched),
        "self-confirmed" => Some(Stage::SelfConfirmed),
        "self-put-back" => Some(Stage::SelfPutBack),
        "refused" => Some(Stage::Refused),
        "rolled-back" => Some(Stage::RolledBack),
        "failed" => Some(Stage::Failed),
        _ => None,
    }
}

/// The outcomes that change nothing, which end a round whose start was
/// told as `not-switched`, with the name as the code.
const NOT_SWITCHED: &[&str] = &[
    "not-quiet",
    "unreachable",
    "would",
    "up-to-date",
    "nothing-published",
];

/// The fixed code of a round that switched nothing: its outcome's name
/// from [`NOT_SWITCHED`], else `other`.
fn not_switched_code(outcome: &str) -> &'static str {
    NOT_SWITCHED
        .iter()
        .copied()
        .find(|name| *name == outcome)
        .unwrap_or("other")
}

/// A build the lamp ran: its sequence and, when it is hex digits, the
/// first [`SHORT_COMMIT`] characters of its commit, as [`reports::Build`]
/// takes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version(String);

/// How much of the commit `from` carries: a manifest's commit is 40 hex
/// characters, more than [`reports::Build`] takes after the sequence.
const SHORT_COMMIT: usize = 7;

impl Version {
    fn running(build: &lamp_msgs::UpdateBuild) -> Self {
        let commit: String = build.commit.chars().take(SHORT_COMMIT).collect();
        let whole = format!("{}+{}", build.sequence, commit);
        match reports::Build::of(&whole) {
            Some(_) => Self(whole),
            None => Self(build.sequence.to_string()),
        }
    }

    /// From a file: only what [`reports::Build`] takes.
    fn kept(text: &str) -> Option<Self> {
        reports::Build::of(text).map(|_| Self(text.trim().to_owned()))
    }
}

/// The parts of `last` that lamp-update may add (a fixed reason code, when
/// the round began), read from the value's JSON so that this works before
/// and after it does. Absent or not valid: none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    pub reason: Option<&'static str>,
    pub started_at_unix_ms: Option<u64>,
}

impl Extras {
    pub fn of(value: &serde_json::Value) -> Self {
        Self::of_known(value, REASONS)
    }

    fn of_known(value: &serde_json::Value, known: &'static [&'static str]) -> Self {
        let last = &value["last"];
        Self {
            reason: last["reason"]
                .as_str()
                .and_then(|reason| known.iter().copied().find(|name| *name == reason)),
            started_at_unix_ms: last["started_at_unix_ms"].as_u64(),
        }
    }
}

/// One alert, as typed values only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub stage: Stage,
    /// The build the lamp ran before the round, if known.
    pub from: Option<Version>,
    /// The build the round was about, if it names one.
    pub to: Option<u64>,
    /// A fixed reason code from lamp-update, if it said one.
    pub code: Option<&'static str>,
    /// Whole seconds the round took, if lamp-update said when it began.
    pub took_s: Option<u64>,
    /// When the stage was seen, wall seconds.
    pub at_s: u64,
}

impl Alert {
    /// The fixed template: a title line, then a line for each field there
    /// is.
    pub fn message(&self) -> String {
        let mut text = format!("Software update lamp - {}", self.stage.name());
        if let Some(from) = &self.from {
            text.push_str(&format!("\nfrom: {}", from.0));
        }
        if let Some(to) = self.to {
            text.push_str(&format!("\nto: {to}"));
        }
        if let Some(code) = self.code {
            text.push_str(&format!("\ncode: {code}"));
        }
        if let Some(took) = self.took_s {
            text.push_str(&format!("\ntook: {took} s"));
        }
        text
    }

    /// The request body: the alert endpoint's `device_id`, `type`,
    /// `message` and `ts`, and an `alert_id` of this stage and time.
    pub fn body(&self, device_id: &str) -> Option<String> {
        if device_id.is_empty() || device_id.len() > reports::ID_MOST {
            return None;
        }
        let message = self.message();
        if message.len() > MOST_BYTES {
            return None;
        }
        Some(
            serde_json::json!({
                "device_id": device_id,
                "type": TYPE,
                "message": message,
                "ts": self.at_s,
                "alert_id": format!("update-{}-{}", self.stage.name(), self.at_s),
            })
            .to_string(),
        )
    }
}

/// A round's end, by what makes it one: its outcome, build and time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Key(String, Option<u64>, u64);

impl Key {
    fn of(result: &UpdateResult) -> Self {
        // Only a known name is kept, so the file never holds text that came in.
        let known =
            told_as(&result.outcome).is_some() || NOT_SWITCHED.contains(&result.outcome.as_str());
        let outcome = if known {
            result.outcome.as_str()
        } else {
            "other"
        };
        Self(outcome.to_owned(), result.sequence, result.at_unix_ms)
    }
}

/// What is kept between runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Told {
    /// The last end told (or taken as told when first found).
    ended: Option<Key>,
    /// The round whose start was told: it is named by the end before it,
    /// which `update/status` says all through the round.
    started: bool,
    started_after: Option<Key>,
    /// The build it ran then.
    from: Option<String>,
    /// Set while the last end told was a refused, failed or not-switched
    /// one: its stage, build and code. The same end again is not told, nor
    /// the start of the rounds that lead to it, until a round ends some
    /// other way.
    repeating: Option<(String, Option<u64>, Option<String>)>,
}

/// The file's write, kept to run where it may block: a temporary file,
/// synced, renamed over the file.
#[derive(Debug)]
pub struct FileWrite {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl FileWrite {
    pub fn run(self) {
        let temporary = self.path.with_extension("tmp");
        let _ = fs::remove_file(&temporary);
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .and_then(|mut file| file.write_all(&self.bytes).and_then(|()| file.sync_all()))
            .and_then(|()| fs::rename(&temporary, &self.path));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }
}

/// Turns what `update/status` says, over time, into alerts. The only
/// state it keeps is [`Told`], in the file at `store`.
#[derive(Debug, Default)]
pub struct Watcher {
    store: Option<PathBuf>,
    told: Option<Told>,
}

impl Watcher {
    pub fn new(store: Option<PathBuf>) -> Self {
        let told = store
            .as_deref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Told>(&bytes).ok());
        Self { store, told }
    }

    fn write_of(&self, told: &Told) -> Option<FileWrite> {
        Some(FileWrite {
            path: self.store.clone()?,
            bytes: serde_json::to_vec(told).ok()?,
        })
    }

    /// What `status`, seen at `now_ms`, is news of. A value past its life
    /// is not given here.
    pub fn observe(&mut self, status: &UpdateStatus, extras: &Extras, now_ms: u64) -> Vec<Alert> {
        let (alerts, write) = self.observe_deferred(status, extras, now_ms);
        if let Some(write) = write {
            write.run();
        }
        alerts
    }

    /// As [`Watcher::observe`], with the file's write handed back for the
    /// caller to run where it may block.
    pub fn observe_deferred(
        &mut self,
        status: &UpdateStatus,
        extras: &Extras,
        now_ms: u64,
    ) -> (Vec<Alert>, Option<FileWrite>) {
        let current = status.last.as_ref().map(Key::of);
        let Some(told) = self.told.clone() else {
            // Nothing kept: what is said now was told, or was not to be.
            let told = Told {
                ended: current,
                ..Told::default()
            };
            let write = self.write_of(&told);
            self.told = Some(told);
            return (Vec::new(), write);
        };
        let mut told = told;
        let mut alerts = Vec::new();
        if status.now != "idle" {
            // `checking` opens every round, most of which find nothing to
            // do: a round is started when it goes on to fetch.
            if status.now != "checking" && !(told.started && told.started_after == current) {
                told.started = true;
                told.started_after = current;
                told.from = status
                    .running
                    .as_ref()
                    .map(|build| Version::running(build).0);
                // The start is still kept, but not told, while the same end
                // repeats: its end would not be told either.
                if told.repeating.is_none() {
                    alerts.push(Alert {
                        stage: Stage::Started,
                        from: told.from.as_deref().and_then(Version::kept),
                        to: None,
                        code: None,
                        took_s: None,
                        at_s: now_ms / 1000,
                    });
                }
            }
        } else if told.ended != current {
            // The end before this one names the round, if its start was told.
            let began = told.started && told.started_after == told.ended;
            told.ended = current.clone();
            if let Some(result) = &status.last
                && let Some(stage) =
                    told_as(&result.outcome).or_else(|| began.then_some(Stage::NotSwitched))
            {
                // The start told is of this round only if it was said after
                // the end before this one.
                let from = told.from.take().and_then(|text| Version::kept(&text));
                let code = if stage == Stage::NotSwitched {
                    Some(not_switched_code(&result.outcome))
                } else {
                    extras.reason
                };
                let triple = matches!(stage, Stage::Refused | Stage::Failed | Stage::NotSwitched)
                    .then(|| {
                        (
                            stage.name().to_owned(),
                            result.sequence,
                            code.map(str::to_owned),
                        )
                    });
                let again = triple.is_some() && told.repeating == triple;
                told.repeating = triple;
                if !again {
                    alerts.push(Alert {
                        stage,
                        from,
                        to: result.sequence,
                        code,
                        took_s: extras
                            .started_at_unix_ms
                            .and_then(|began| result.at_unix_ms.checked_sub(began))
                            .map(|ms| ms / 1000)
                            .filter(|seconds| *seconds <= LONGEST_TOOK_S),
                        at_s: now_ms / 1000,
                    });
                }
            }
        }
        let mut write = None;
        if self.told.as_ref() != Some(&told) {
            write = self.write_of(&told);
            self.told = Some(told);
        }
        (alerts, write)
    }
}

/// One try at the alert endpoint: why it did not go, or nothing. The key
/// is only in the request's header, and the reply is not read.
pub async fn post(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    body: String,
) -> Result<(), String> {
    let response = client
        .post(url)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "no answer in time".to_owned()
            } else {
                "no connection".to_owned()
            }
        })?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("status {}", response.status().as_u16()))
    }
}

/// Sends alerts: the watcher, the HTTPS client and the last failure said.
pub struct Alerts {
    watcher: Watcher,
    client: Option<reqwest::Client>,
    failing: Option<String>,
}

impl Alerts {
    pub fn new(store: Option<PathBuf>) -> Self {
        Self {
            watcher: Watcher::new(store),
            client: crate::beat::client().ok(),
            failing: None,
        }
    }

    /// Looks at one value of `update/status` (none if nothing is kept or
    /// it is stale) and sends what it is news of, if the person's yes
    /// stands now. A stage seen while it does not is not kept for later.
    /// Returns how many went.
    pub async fn step(
        &mut self,
        shared: &Shared,
        status: Option<(&UpdateStatus, &Extras)>,
        account: Option<&Account>,
        key: &str,
        now_ms: u64,
        say: &mut impl Say,
    ) -> usize {
        let Some((status, extras)) = status else {
            return 0;
        };
        let (alerts, write) = self.watcher.observe_deferred(status, extras, now_ms);
        if let Some(write) = write {
            // Off the runtime's worker: a slow flash sync stalls nothing else.
            let _ = tokio::task::spawn_blocking(move || write.run()).await;
        }
        let mut sent = 0;
        for alert in alerts {
            // Read at each send.
            if !shared.with(|reports| reports.consent().share) {
                continue;
            }
            let (Some(account), Some(client)) = (account, &self.client) else {
                continue;
            };
            let (Some(url), Some(body)) = (url(account), alert.body(&account.device_id)) else {
                continue;
            };
            match post(client, &url, key, body).await {
                Ok(()) => {
                    sent += 1;
                    if self.failing.take().is_some() {
                        say.line("the backend's alert endpoint answers again");
                    }
                }
                Err(why) => {
                    if self.failing.as_deref() != Some(why.as_str()) {
                        say.line(&format!("an update alert was dropped: {why}"));
                        self.failing = Some(why);
                    }
                }
            }
        }
        sent
    }
}

/// Wall milliseconds now.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The driver: looks at `update/status` every [`LOOK_EVERY`], forever. The
/// account is the newest one the ping kept; `key` is the ping's bearer;
/// `state` is lamp-cloud's state folder.
pub async fn run(
    shared: Arc<Shared>,
    bus: Option<lamp_bus::Bus>,
    accounts: watch::Receiver<Option<Account>>,
    key: String,
    state: Option<PathBuf>,
    say: &mut impl Say,
) {
    let Some(bus) = bus else {
        return std::future::pending().await;
    };
    let mut alerts = Alerts::new(state.map(|folder| folder.join(FILE)));
    let mut tick = tokio::time::interval(LOOK_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let kept = bus
            .latest(UpdateStatus::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired);
        let status = kept.as_ref().and_then(|message| {
            let status = message.parse::<UpdateStatus>().ok()?;
            let extras = message
                .parse::<serde_json::Value>()
                .map(|value| Extras::of(&value))
                .unwrap_or_default();
            UpdateStatus::NOW
                .contains(&status.now.as_str())
                .then_some((status, extras))
        });
        let account = accounts.borrow().clone();
        alerts
            .step(
                &shared,
                status.as_ref().map(|(status, extras)| (status, extras)),
                account.as_ref(),
                &key,
                now_ms(),
                say,
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::UpdateBuild;
    use serde_json::json;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const T0: u64 = 1_792_296_000_000;

    type Last<'a> = Option<(&'a str, Option<u64>, u64)>;

    fn status(now: &str, running: u64, last: Last) -> UpdateStatus {
        UpdateStatus {
            running: Some(UpdateBuild {
                sequence: running,
                commit: "4a39f04".into(),
            }),
            now: now.into(),
            last: last.map(|(outcome, sequence, at_unix_ms)| UpdateResult {
                outcome: outcome.into(),
                sequence,
                at_unix_ms,
                reason: None,
                started_at_unix_ms: None,
            }),
        }
    }

    fn folder(name: &str) -> PathBuf {
        let root = PathBuf::from(format!("/tmp/lcu-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn none() -> Extras {
        Extras::default()
    }

    #[test]
    fn a_manifests_whole_commit_is_cut_to_seven_so_from_keeps_it() {
        let build = UpdateBuild {
            sequence: 3309,
            commit: "bc3fccb546a22baaef180fcd57b3291a44e0e4c1".into(),
        };
        assert_eq!(Version::running(&build).0, "3309+bc3fccb");
        let not_hex = UpdateBuild {
            sequence: 3309,
            commit: "main".into(),
        };
        assert_eq!(Version::running(&not_hex).0, "3309");
    }

    #[test]
    fn the_url_is_the_base_with_alert_after_it_and_its_v1_kept() {
        let account = |base: &str| Account {
            device_id: "d-42".into(),
            llm_base_url: base.into(),
            ..Account::default()
        };
        assert_eq!(
            url(&account("https://api.test/v1/")).as_deref(),
            Some("https://api.test/v1/alert")
        );
        assert_eq!(
            url(&account("https://api.test")).as_deref(),
            Some("https://api.test/alert")
        );
        assert_eq!(url(&account(" ")), None);
    }

    #[test]
    fn each_stage_has_its_message_from_the_fixed_template() {
        let alert = |stage, from: Option<&str>, to, code, took_s| Alert {
            stage,
            from: from.map(|from| Version(from.into())),
            to,
            code,
            took_s,
            at_s: 5,
        };
        assert_eq!(
            alert(Stage::Started, Some("3309+4a39f04"), None, None, None).message(),
            "Software update lamp - started\nfrom: 3309+4a39f04"
        );
        assert_eq!(
            alert(
                Stage::Done,
                Some("3309+4a39f04"),
                Some(3310),
                None,
                Some(95)
            )
            .message(),
            "Software update lamp - done\nfrom: 3309+4a39f04\nto: 3310\ntook: 95 s"
        );
        assert_eq!(
            alert(Stage::Refused, None, None, Some("models"), None).message(),
            "Software update lamp - refused\ncode: models"
        );
        assert_eq!(
            alert(Stage::SelfSwitched, None, Some(3310), None, None).message(),
            "Software update lamp - self-switched\nto: 3310"
        );
        for (outcome, want) in [
            ("switched", Some(Stage::Done)),
            ("self-switched", Some(Stage::SelfSwitched)),
            ("self-confirmed", Some(Stage::SelfConfirmed)),
            ("self-put-back", Some(Stage::SelfPutBack)),
            ("refused", Some(Stage::Refused)),
            ("rolled-back", Some(Stage::RolledBack)),
            ("failed", Some(Stage::Failed)),
            ("up-to-date", None),
            ("unreachable", None),
            ("nothing-published", None),
            ("not-quiet", None),
            ("would", None),
            ("something else", None),
        ] {
            assert_eq!(told_as(outcome), want, "{outcome}");
        }
        let body: serde_json::Value = serde_json::from_str(
            &alert(Stage::Failed, None, Some(7), None, None)
                .body("d-42")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["device_id"], "d-42");
        assert_eq!(body["type"], "ops");
        assert_eq!(body["ts"], 5);
        assert_eq!(body["alert_id"], "update-failed-5");
        assert_eq!(body["message"], "Software update lamp - failed\nto: 7");
        assert_eq!(alert(Stage::Failed, None, None, None, None).body(""), None);
    }

    #[test]
    fn a_reason_and_a_start_time_are_read_only_when_valid_and_absent_changes_nothing() {
        const KNOWN: &[&str] = &["models", "signature"];
        let read = |value| Extras::of_known(&value, KNOWN);
        assert_eq!(read(json!({"last": {"outcome": "refused"}})), none());
        assert_eq!(read(json!({"last": null})), none());
        assert_eq!(
            read(json!({"last": {"reason": "models", "started_at_unix_ms": 5}})),
            Extras {
                reason: Some("models"),
                started_at_unix_ms: Some(5)
            }
        );
        // Text that is not on the list, or a time that is not a number.
        assert_eq!(
            read(json!({"last": {"reason": "/etc/lampOS/x Alice", "started_at_unix_ms": "5"}})),
            none()
        );
        // The names lamp-update says, each passing as the code; others do not.
        for name in REASONS {
            let read = Extras::of(&json!({"last": {"reason": name}}));
            assert_eq!(read.reason, Some(*name));
            let alert = Alert {
                stage: Stage::Refused,
                from: None,
                to: None,
                code: read.reason,
                took_s: None,
                at_s: 1,
            };
            assert!(alert.message().ends_with(&format!("code: {name}")));
        }
        assert_eq!(REASONS.len(), 6);
        for other in ["signature", "Fetching", "/etc/x", ""] {
            assert_eq!(Extras::of(&json!({"last": {"reason": other}})), none());
        }
    }

    /// `took` is the end minus the round's `checking`, in whole seconds,
    /// and never negative or absurd.
    #[test]
    fn the_time_it_took_is_whole_seconds_from_checking_and_never_negative() {
        let took = |began: u64, at: u64| {
            let mut watcher = Watcher::new(None);
            let old = Some(("up-to-date", Some(1), 1));
            watcher.observe(&status("idle", 1, old), &none(), T0);
            watcher.observe(&status("fetching", 1, old), &none(), T0 + 1);
            let extras = Extras {
                reason: None,
                started_at_unix_ms: Some(began),
            };
            let ended = Some(("switched", Some(2), at));
            watcher.observe(&status("idle", 2, ended), &extras, T0 + 2)[0].took_s
        };
        assert_eq!(took(T0, T0 + 95_900), Some(95));
        assert_eq!(took(T0, T0), Some(0));
        assert_eq!(took(T0 + 5_000, T0), None);
        assert_eq!(took(T0, T0 + 90_000_000), None);
    }

    /// The old lamp-update put back is always told, with no build, reason
    /// or time needed, once, and it breaks a repeating streak.
    #[test]
    fn a_self_put_back_is_always_told_once_and_breaks_a_streak() {
        let mut watcher = Watcher::new(None);
        let old = Some(("up-to-date", Some(8599), T0 - 1));
        watcher.observe(&status("idle", 8599, old), &none(), T0);
        watcher.observe(&status("fetching", 8599, old), &none(), T0 + 1_000);
        let refused = Some(("refused", None, T0 + 5_000));
        assert_eq!(
            watcher
                .observe(&status("idle", 8599, refused), &none(), T0 + 6_000)
                .len(),
            1
        );
        // No round began on this run (the helper acts on its own), and the
        // value has no build.
        let back = Some(("self-put-back", None, T0 + 100_000));
        let told = watcher.observe(&status("idle", 8599, back), &none(), T0 + 101_000);
        assert_eq!((told.len(), told[0].stage), (1, Stage::SelfPutBack));
        assert_eq!(
            (told[0].to, told[0].code, told[0].took_s),
            (None, None, None)
        );
        assert_eq!(told[0].message(), "Software update lamp - self-put-back");
        for minute in 1..4 {
            let again = status("idle", 8599, back);
            assert!(
                watcher
                    .observe(&again, &none(), T0 + 101_000 + minute * 60_000)
                    .is_empty()
            );
        }
        // The refusal after it is told again.
        watcher.observe(&status("fetching", 8599, back), &none(), T0 + 400_000);
        let refused = Some(("refused", None, T0 + 405_000));
        assert_eq!(
            watcher
                .observe(&status("idle", 8599, refused), &none(), T0 + 406_000)
                .len(),
            1
        );
    }

    /// The confirmation of lamp-update's own switch is its own stage, told
    /// once however often it is said, and it breaks a repeating streak.
    #[test]
    fn a_self_confirmation_is_told_once_and_breaks_a_streak() {
        let mut watcher = Watcher::new(None);
        let old = Some(("up-to-date", Some(8599), T0 - 1));
        watcher.observe(&status("idle", 8599, old), &none(), T0);
        watcher.observe(&status("fetching", 8599, old), &none(), T0 + 1_000);
        let refused = Some(("refused", None, T0 + 5_000));
        assert_eq!(
            watcher
                .observe(&status("idle", 8599, refused), &none(), T0 + 6_000)
                .len(),
            1
        );
        // The next refusal would be held; the confirmation is told, and
        // the refusal after it is told again.
        let confirmed = Some(("self-confirmed", Some(8600), T0 + 100_000));
        let told = watcher.observe(&status("idle", 8599, confirmed), &none(), T0 + 101_000);
        assert_eq!((told.len(), told[0].stage), (1, Stage::SelfConfirmed));
        for minute in 1..4 {
            let again = status("idle", 8599, confirmed);
            assert!(
                watcher
                    .observe(&again, &none(), T0 + 101_000 + minute * 60_000)
                    .is_empty()
            );
        }
        watcher.observe(&status("fetching", 8599, confirmed), &none(), T0 + 400_000);
        let refused = Some(("refused", None, T0 + 405_000));
        let told = watcher.observe(&status("idle", 8599, refused), &none(), T0 + 406_000);
        assert_eq!(told.len(), 1);
    }

    #[test]
    fn a_round_is_told_once_per_stage_across_the_minute_repeats() {
        let mut watcher = Watcher::new(None);
        let old = Some(("up-to-date", Some(3309), T0 - 600_000));
        // Start-up: the result found is told as nothing, however often said.
        assert!(
            watcher
                .observe(&status("idle", 3309, old), &none(), T0)
                .is_empty()
        );
        assert!(
            watcher
                .observe(&status("idle", 3309, old), &none(), T0 + 60_000)
                .is_empty()
        );
        // A round that finds nothing.
        assert!(
            watcher
                .observe(&status("checking", 3309, old), &none(), T0 + 120_000)
                .is_empty()
        );
        let nothing = Some(("up-to-date", Some(3309), T0 + 121_000));
        assert!(
            watcher
                .observe(&status("idle", 3309, nothing), &none(), T0 + 122_000)
                .is_empty()
        );
        // A round that updates.
        assert!(
            watcher
                .observe(&status("checking", 3309, nothing), &none(), T0 + 200_000)
                .is_empty()
        );
        let started = watcher.observe(&status("fetching", 3309, nothing), &none(), T0 + 201_000);
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].stage, Stage::Started);
        assert_eq!(started[0].from, Some(Version("3309+4a39f04".into())));
        for (now, at) in [
            ("fetching", 230_000),
            ("switching", 260_000),
            ("watching", 290_000),
        ] {
            assert!(
                watcher
                    .observe(&status(now, 3309, nothing), &none(), T0 + at)
                    .is_empty(),
                "{now}"
            );
        }
        let ended = Some(("switched", Some(3310), T0 + 320_000));
        let extras = Extras {
            reason: None,
            started_at_unix_ms: Some(T0 + 200_000),
        };
        let done = watcher.observe(&status("idle", 3310, ended), &extras, T0 + 321_000);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].stage, Stage::Done);
        assert_eq!(done[0].from, Some(Version("3309+4a39f04".into())));
        assert_eq!(done[0].to, Some(3310));
        assert_eq!(done[0].took_s, Some(120));
        for minute in 1..5 {
            assert!(
                watcher
                    .observe(
                        &status("idle", 3310, ended),
                        &extras,
                        T0 + 321_000 + minute * 60_000
                    )
                    .is_empty()
            );
        }
        // A later round's refusal is a new end (its time differs) and is told;
        // said again, it is not.
        let refused = Some(("refused", None, T0 + 900_000));
        assert_eq!(
            watcher
                .observe(&status("fetching", 3310, ended), &none(), T0 + 890_000)
                .len(),
            1
        );
        let told = watcher.observe(&status("idle", 3310, refused), &none(), T0 + 901_000);
        assert_eq!((told.len(), told[0].stage), (1, Stage::Refused));
        assert_eq!(told[0].took_s, None);
        assert!(
            watcher
                .observe(&status("idle", 3310, refused), &none(), T0 + 961_000)
                .is_empty()
        );
    }

    /// A round that began and switched nothing is ended, so no start is left
    /// alone; one that never began sends nothing; an unknown name is `other`.
    #[test]
    fn a_started_round_that_switches_nothing_ends_as_not_switched() {
        let mut watcher = Watcher::new(None);
        let old = Some(("up-to-date", Some(3309), T0 - 1_000));
        assert!(
            watcher
                .observe(&status("idle", 3309, old), &none(), T0)
                .is_empty()
        );
        // A round with no start told ends as up-to-date: nothing.
        let quiet = Some(("up-to-date", Some(3309), T0 + 10_000));
        assert!(
            watcher
                .observe(&status("checking", 3309, old), &none(), T0 + 5_000)
                .is_empty()
        );
        assert!(
            watcher
                .observe(&status("idle", 3309, quiet), &none(), T0 + 11_000)
                .is_empty()
        );
        let mut stages = Vec::new();
        let mut last = quiet;
        for (i, outcome) in ["not-quiet", "switched"].into_iter().enumerate() {
            let at = T0 + 100_000 * (i as u64 + 1);
            stages.extend(watcher.observe(&status("fetching", 3309, last), &none(), at));
            last = Some((outcome, Some(3310), at + 50_000));
            stages.extend(watcher.observe(&status("idle", 3309, last), &none(), at + 51_000));
        }
        let told: Vec<_> = stages.iter().map(|a| (a.stage, a.code)).collect();
        assert_eq!(
            told,
            [
                (Stage::Started, None),
                (Stage::NotSwitched, Some("not-quiet")),
                // The next start is held while that end could repeat.
                (Stage::Done, None),
            ]
        );
        for name in [
            "not-quiet",
            "unreachable",
            "would",
            "up-to-date",
            "nothing-published",
        ] {
            assert_eq!(not_switched_code(name), name);
        }
        assert_eq!(not_switched_code("Alice said s3cret"), "other");
        assert_eq!(
            stages[1].message(),
            "Software update lamp - not-switched\nfrom: 3309+4a39f04\nto: 3310\ncode: not-quiet"
        );
        // An unknown name after a started round is told as other.
        watcher.observe(&status("fetching", 3310, last), &none(), T0 + 500_000);
        let odd = Some(("Alice said s3cret", None, T0 + 510_000));
        let told = watcher.observe(&status("idle", 3310, odd), &none(), T0 + 511_000);
        assert_eq!(told[0].code, Some("other"));
    }

    /// #4490's P3: a round that found lamp-update's lock taken (a settings
    /// change restarting a part under it) ends `not-quiet` before any phase,
    /// so no start was told and nothing is alerted. Ended `failed`, as it
    /// was before, it was told as a failed update.
    #[test]
    fn a_round_refused_by_the_lock_alerts_nothing() {
        let dir = folder("locked");
        let mut watcher = Watcher::new(Some(dir.join(FILE)));
        let old = Some(("up-to-date", Some(8599), T0 - 1));
        watcher.observe(&status("idle", 8599, old), &none(), T0);
        let locked = Some(("not-quiet", None, T0 + 10_000));
        let told = watcher.observe(&status("idle", 8599, locked), &none(), T0 + 11_000);
        assert!(told.is_empty(), "{told:?}");
        let failed = Some(("failed", None, T0 + 20_000));
        let told = watcher.observe(&status("idle", 8599, failed), &none(), T0 + 21_000);
        assert_eq!(
            told.into_iter().map(|a| a.stage).collect::<Vec<_>>(),
            [Stage::Failed]
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// A lamp that keeps refusing, or is talked to through every update
    /// window, tells it once, not every round; and a restart mid-streak
    /// does not tell it again.
    #[test]
    fn the_same_refusal_or_not_switched_end_is_told_once_until_another_end() {
        let stages =
            |watcher: &mut Watcher, outcome: &'static str, round: u64, last: Last<'static>| {
                let at = T0 + round * 1_000_000;
                let mut told = watcher.observe(&status("fetching", 8599, last), &none(), at);
                let ended = Some((outcome, Some(8600), at + 50_000));
                told.extend(watcher.observe(&status("idle", 8599, ended), &none(), at + 51_000));
                (told.into_iter().map(|a| a.stage).collect::<Vec<_>>(), ended)
            };
        let dir = folder("streak");
        let store = dir.join(FILE);
        let mut watcher = Watcher::new(Some(store.clone()));
        let old = Some(("up-to-date", Some(8599), T0 - 1));
        watcher.observe(&status("idle", 8599, old), &none(), T0);
        let (first, mut last) = stages(&mut watcher, "refused", 1, old);
        assert_eq!(first, [Stage::Started, Stage::Refused]);
        // A restart in the middle of the streak.
        let mut watcher = Watcher::new(Some(store.clone()));
        for round in 2..4 {
            let (told, ended) = stages(&mut watcher, "refused", round, last);
            assert!(told.is_empty(), "round {round}: {told:?}");
            last = ended;
        }
        // Another end breaks it, and is told whole; the build coming
        // after it is told again.
        let (told, ended) = stages(&mut watcher, "switched", 4, last);
        assert_eq!(told, [Stage::Done]);
        last = ended;
        let (told, ended) = stages(&mut watcher, "refused", 5, last);
        assert_eq!(told, [Stage::Started, Stage::Refused]);
        last = ended;
        // A different triple (another build) is told even in a streak.
        watcher.observe(&status("fetching", 8599, last), &none(), T0 + 6_000_000);
        let other = Some(("refused", Some(8601), T0 + 6_050_000));
        let told = watcher.observe(&status("idle", 8599, other), &none(), T0 + 6_051_000);
        assert_eq!(told.len(), 1);
        last = other;
        // The not-switched streak: told through the window each round, once.
        let (told, ended) = stages(&mut watcher, "not-quiet", 7, last);
        assert_eq!(told, [Stage::NotSwitched]);
        last = ended;
        let (told, ended) = stages(&mut watcher, "not-quiet", 8, last);
        assert!(told.is_empty(), "{told:?}");
        last = ended;
        let (told, _) = stages(&mut watcher, "switched", 9, last);
        assert_eq!(told, [Stage::Done]);
        // A file from before this field loads: no streak.
        fs::write(
            &store,
            r#"{"ended":["switched",8600,5],"started":false,"started_after":null,"from":null}"#,
        )
        .unwrap();
        let mut old_file = Watcher::new(Some(store));
        let (told, _) = stages(
            &mut old_file,
            "refused",
            10,
            Some(("switched", Some(8600), 5)),
        );
        assert_eq!(told, [Stage::Started, Stage::Refused]);
    }

    /// A round restarts lamp-cloud when it changed, and a reboot begins it
    /// again: the file joins the start and the end, and tells neither twice.
    #[test]
    fn a_restart_between_the_start_and_the_end_keeps_both_once() {
        let dir = folder("restart");
        let store = dir.join(FILE);
        let old = Some(("up-to-date", Some(3309), T0 - 1_000));
        let mut first = Watcher::new(Some(store.clone()));
        assert!(
            first
                .observe(&status("idle", 3309, old), &none(), T0)
                .is_empty()
        );
        assert_eq!(
            first
                .observe(&status("fetching", 3309, old), &none(), T0 + 1_000)
                .len(),
            1
        );
        // lamp-cloud is restarted by the round: a new run, the same file.
        let mut second = Watcher::new(Some(store.clone()));
        assert!(
            second
                .observe(&status("switching", 3309, old), &none(), T0 + 30_000)
                .is_empty()
        );
        let ended = Some(("switched", Some(3310), T0 + 60_000));
        let told = second.observe(&status("idle", 3310, ended), &none(), T0 + 61_000);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].stage, Stage::Done);
        assert_eq!(told[0].from, Some(Version("3309+4a39f04".into())));
        // A reboot, and lamp-update says the same end again: nothing.
        let mut third = Watcher::new(Some(store.clone()));
        assert!(
            third
                .observe(&status("idle", 3310, ended), &none(), T0 + 600_000)
                .is_empty()
        );
        assert!(
            third
                .observe(&status("idle", 3310, ended), &none(), T0 + 660_000)
                .is_empty()
        );
        // The file is the lamp-cloud's own, 0600, and holds only keys and a build.
        let mode =
            std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&store).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o600);
        let kept = fs::read_to_string(&store).unwrap();
        assert!(!kept.contains("Alice"), "{kept}");
        // An end that came while lamp-cloud was down is told when it is back.
        let after = Some(("failed", None, T0 + 900_000));
        let mut fourth = Watcher::new(Some(store));
        let told = fourth.observe(&status("idle", 3310, after), &none(), T0 + 901_000);
        assert_eq!((told.len(), told[0].stage), (1, Stage::Failed));
        // A file that cannot be read is no file: the value found is taken as told.
        fs::write(dir.join("bad.json"), "nonsense").unwrap();
        let mut bad = Watcher::new(Some(dir.join("bad.json")));
        assert!(
            bad.observe(&status("idle", 3310, after), &none(), T0)
                .is_empty()
        );
    }

    fn consent(places: &reports::Places, yes: bool) {
        let share = if yes { "true" } else { "false" };
        std::fs::write(
            &places.setup,
            format!(r#"{{"share":{share},"asked":1,"at":"2026-10-19T04:00:00Z"}}"#),
        )
        .unwrap();
    }

    /// A backend on a free local port that keeps each request it gets.
    async fn backend() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|n| n.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
                    )
                    .await;
            }
        });
        (base, requests)
    }

    fn account(base: &str) -> Account {
        Account {
            device_id: "d-42".into(),
            llm_base_url: base.into(),
            ..Account::default()
        }
    }

    /// The request is the alert endpoint's, with the key as bearer, and a
    /// closed gate sends nothing and keeps nothing for later.
    #[tokio::test]
    async fn an_alert_goes_to_the_alert_path_with_the_key_and_only_with_the_yes() {
        let places = reports::tests::places("alerts-gate");
        consent(&places, false);
        let shared = Shared::new(reports::Reports::open(places.clone()));
        let (base, requests) = backend().await;
        let account = account(&base);
        let mut alerts = Alerts::new(None);
        let mut say = |_: &str| {};
        let old = Some(("up-to-date", Some(3309), T0 - 1));
        let idle = status("idle", 3309, old);
        let fetching = status("fetching", 3309, old);
        let mut step = async |alerts: &mut Alerts, s: &UpdateStatus, at: u64| {
            alerts
                .step(
                    &shared,
                    Some((s, &none())),
                    Some(&account),
                    "lob_key",
                    at,
                    &mut say,
                )
                .await
        };

        assert_eq!(step(&mut alerts, &idle, T0).await, 0);
        assert_eq!(step(&mut alerts, &fetching, T0 + 1_000).await, 0);
        assert!(requests.lock().unwrap().is_empty());

        consent(&places, true);
        let ended = status("idle", 3310, Some(("switched", Some(3310), T0 + 40_000)));
        assert_eq!(step(&mut alerts, &ended, T0 + 41_000).await, 1);
        let seen = requests.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert!(request.starts_with("POST /v1/alert HTTP/1.1"), "{request}");
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer lob_key")
        );
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["type"], "ops");
        assert_eq!(body["device_id"], "d-42");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .starts_with("Software update lamp - done")
        );

        // A no again: read at each send.
        consent(&places, false);
        let rolled = status("idle", 3310, Some(("rolled-back", Some(3311), T0 + 90_000)));
        assert_eq!(step(&mut alerts, &rolled, T0 + 91_000).await, 0);
        assert_eq!(requests.lock().unwrap().len(), 1);
        // No account or no value: nothing.
        let mut say = |_: &str| {};
        assert_eq!(
            alerts
                .step(&shared, Some((&rolled, &none())), None, "k", T0, &mut say)
                .await,
            0
        );
        assert_eq!(
            alerts
                .step(&shared, None, Some(&account), "k", T0, &mut say)
                .await,
            0
        );
    }

    /// A backend that is down drops the alert once: said, never retried.
    #[tokio::test]
    async fn an_alert_the_backend_cannot_take_is_said_and_dropped() {
        let places = reports::tests::places("alerts-down");
        consent(&places, true);
        let shared = Shared::new(reports::Reports::open(places));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        drop(listener);
        let account = account(&base);
        let mut alerts = Alerts::new(None);
        let mut lines = Vec::new();
        let mut say = |line: &str| lines.push(line.to_owned());
        let idle = status("idle", 3309, None);
        alerts
            .step(
                &shared,
                Some((&idle, &none())),
                Some(&account),
                "k",
                T0,
                &mut say,
            )
            .await;
        let fetching = status("fetching", 3309, None);
        let sent = alerts
            .step(
                &shared,
                Some((&fetching, &none())),
                Some(&account),
                "k",
                T0 + 1_000,
                &mut say,
            )
            .await;
        assert_eq!(sent, 0);
        assert_eq!(lines, ["an update alert was dropped: no connection"]);
    }

    /// The canary: secrets planted in every input reach no alert.
    #[tokio::test]
    async fn no_secret_planted_in_any_input_reaches_any_alert() {
        let places = reports::tests::places("alerts-canary");
        consent(&places, true);
        let shared = Shared::new(reports::Reports::open(places));
        let (base, requests) = backend().await;
        let account = account(&base);
        let dir = folder("canary");
        let mut alerts = Alerts::new(Some(dir.join(FILE)));
        let mut say = |_: &str| {};
        let secret = "Alice said HomeWiFi-5G at 192.168.1.42 lamp-7f72 Asia/Ho_Chi_Minh s3cret remind me journal line PRIVATE_FAILED_UNIT";
        let planted = |now: &str, last: Last| UpdateStatus {
            running: Some(UpdateBuild {
                sequence: 3309,
                commit: format!("deadbeefcafe-Alice {secret}"),
            }),
            now: now.into(),
            last: last.map(|(outcome, sequence, at_unix_ms)| UpdateResult {
                outcome: outcome.into(),
                sequence,
                at_unix_ms,
                reason: None,
                started_at_unix_ms: None,
            }),
        };
        // Reason and start time as lamp-update may one day say them, with
        // the planted text where a name or a number goes.
        let hostile =
            Extras::of(&json!({"last": {"reason": secret, "started_at_unix_ms": secret}}));
        let mut at = T0;
        let mut look = async |alerts: &mut Alerts, s: UpdateStatus| {
            at += 1_000;
            alerts
                .step(
                    &shared,
                    Some((&s, &hostile)),
                    Some(&account),
                    "k",
                    at,
                    &mut say,
                )
                .await;
        };
        let old = Some(("up-to-date", Some(3309), T0 - 1));
        look(&mut alerts, planted("idle", old)).await;
        // Outcomes that are not the round's own names are no alert at all.
        for text in [secret, "refused Alice", "switched s3cret"] {
            look(&mut alerts, planted("idle", Some((text, None, T0)))).await;
        }
        for (now, last) in [
            ("fetching", old),
            ("idle", Some(("refused", None, T0 + 10_000))),
            ("fetching", Some(("refused", None, T0 + 10_000))),
            ("idle", Some(("rolled-back", Some(3310), T0 + 20_000))),
            ("fetching", Some(("rolled-back", Some(3310), T0 + 20_000))),
            ("idle", Some(("switched", Some(3311), T0 + 30_000))),
            ("idle", Some(("failed", None, T0 + 40_000))),
            ("idle", Some(("self-switched", Some(3312), T0 + 50_000))),
            ("fetching", Some(("self-switched", Some(3312), T0 + 50_000))),
            ("idle", Some(("not-quiet", None, T0 + 60_000))),
            ("fetching", Some(("not-quiet", None, T0 + 60_000))),
            ("idle", Some((secret, None, T0 + 70_000))),
        ] {
            look(&mut alerts, planted(now, last)).await;
        }
        let seen = requests.lock().unwrap().clone();
        assert!(seen.len() >= 9, "{}", seen.len());
        for request in &seen {
            let body = request.split("\r\n\r\n").nth(1).unwrap();
            for secret in [
                "Alice",
                "HomeWiFi-5G",
                "192.168.1.42",
                "lamp-7f72",
                "Asia/Ho_Chi_Minh",
                "s3cret",
                "remind me",
                "deadbeefcafe-Alice",
                "journal line",
                "PRIVATE_FAILED_UNIT",
            ] {
                assert!(!body.contains(secret), "{secret} in {body}");
            }
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(keys, ["alert_id", "device_id", "message", "ts", "type"]);
            let message = value["message"].as_str().unwrap();
            assert!(
                message
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || " +\n:-".contains(c)),
                "{message}"
            );
            assert!(message.starts_with("Software update lamp - "));
        }
        let kept = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(
            !kept.contains("Alice") && !kept.contains("s3cret"),
            "{kept}"
        );
    }
}
