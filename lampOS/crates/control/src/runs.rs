//! A skill run's door (docs/control.md, "The run socket"): `control.sock`
//! takes only a connection that holds a **one-run token**. lamp-control mints
//! it itself, when lamp-tasks asks on the admin socket for one run of one
//! skill: from the owner's yes for that skill (the grants it named, tied to
//! the sha256 of the skill's folder), never from what lamp-tasks claims. The
//! token names exactly those grants and ends with the run (`end`), at its own
//! timeout, or when lamp-control restarts: it is kept in memory only.
use crate::protocol::Grant;
use ring::digest::{SHA256, digest};
use std::collections::{BTreeSet, HashMap};

/// Where it listens unless `--run-socket` says another place: lamp-control's
/// own runtime folder, beside the admin socket (0700, the same user).
pub const SOCKET: &str = "/run/lampos-control/control.sock";
/// The longest a token lives, whatever the run asks for.
pub const LONGEST_MS: u64 = 30 * 60 * 1000;
/// How long a token lives if the mint names no time.
pub const DEFAULT_MS: u64 = 10 * 60 * 1000;
/// At most this many runs at once.
pub const MOST_RUNS: usize = 8;

/// The `client` a hello on the run socket carries, and its zero signature
/// (64 zero bytes as base64): placeholders, since the token is the way in.
pub const HELLO_CLIENT: &str = "c_0";

/// The zero signature as base64.
pub fn hello_sig() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode([0u8; 64])
}

/// What the close frame says when a run's connection ends because its run
/// did (`end`, its time, or a newer token), so an SDK can log one line.
pub const CLOSED_ENDED: &str = "the run ended";
/// And when a newer token for the same run replaced its own.
pub const CLOSED_AGAIN: &str = "the run started again";

/// What the connection is told when its token is none, wrong, ended or old.
pub const NO_TOKEN: &str = "no token for this run";

/// The owner's yes for a skill, read where it is kept: the grants it named for
/// the skill's folder as it is now, none for a yes kept with no grant (its
/// token names none). An `Err` is the words the owner's app or lamp-tasks is
/// told (no yes; the folder changed since it; the skill is not there). The
/// skills' own part defines where the yes lives.
pub trait SkillYes: Send + Sync {
    fn grants(&self, skill: &str) -> Result<BTreeSet<Grant>, String>;
    /// `skill`'s fixed write, when its token is for that write alone, run
    /// through the lamp's agent (`LAMP_SKILL_WRITE_AGENT`,
    /// `task_yes::KeptYes`): its run's service calls then say so to
    /// lamp-services, and its `service.write` is given this write
    /// ([`Run::fixed_write`]).
    fn fixed_write(&self, _skill: &str) -> Option<FixedWrite> {
        None
    }
}

/// A lamp that keeps no such yes yet: every mint is refused in words.
pub struct NoYes;

impl SkillYes for NoYes {
    fn grants(&self, _: &str) -> Result<BTreeSet<Grant>, String> {
        Err("no skill yes is kept on this lamp yet".to_owned())
    }
}

/// Binds the socket at `path`, mode 0600, in the folder lamp-control's admin
/// socket is in (0700, its own user's): never a socket anyone else made, an
/// old one of ours is removed first. The guard is the peer's uid and then the
/// token, not the file's mode.
pub fn bind(path: &std::path::Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt as _;
    if let Some(parent) = path.parent() {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// A skill name or a run id: plain characters, never a path or a space, so
/// neither can carry anything into a client id or a journal line.
pub fn plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// A token's shape: 32 bytes as unpadded base64url, 43 characters.
pub fn token_shape(text: &str) -> bool {
    text.len() == 43
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// The client id a run has: `c_` and 16 hex of its run id's digest, so one run
/// is one client however often its token is minted again.
pub fn run_client(run: &str) -> String {
    let sum = digest(&SHA256, format!("skill-run:{run}").as_bytes());
    let hex: String = sum.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("c_{hex}")
}

fn token_key(token: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    key.copy_from_slice(digest(&SHA256, token.as_bytes()).as_ref());
    key
}

/// One live run.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub run: String,
    pub skill: String,
    pub client: String,
    pub grants: BTreeSet<Grant>,
    /// Door clock microseconds after which the token is no longer one.
    pub until_us: u64,
    /// Which mint this is, so an old timeout cannot end a newer token.
    pub serial: u64,
    /// Minted for a skill's fixed write ([`SkillYes::fixed_write`]): its
    /// `service.write` goes to lamp-services with its run's id, held there
    /// to the owner's kept yes for that content, once a run. A call that
    /// names none of its own content is given this write
    /// ([`FixedWrite::covers`]), so the run's words never carry it.
    pub fixed_write: Option<FixedWrite>,
}

/// A skill's fixed write as lamp-control read it from the skill's own file
/// at the mint (`lamp_skills::service_write::fixed_write_of`): `service`,
/// `action` and the action's fields. The run's agent is never given it in
/// words (the Reviewer-lead's P2 on #6824): its `service_write` names the
/// service alone and is given this write here, which lamp-services sends
/// only as the owner's kept yes has it. Its `Debug` never shows it.
#[derive(Clone, PartialEq)]
pub struct FixedWrite(serde_json::Map<String, serde_json::Value>);

impl std::fmt::Debug for FixedWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixedWrite(..)")
    }
}

impl FixedWrite {
    /// The write `content`, an object naming its `service`; none otherwise.
    pub fn of(content: &serde_json::Value) -> Option<Self> {
        let content = content.as_object()?;
        content.get("service")?.as_str()?;
        Some(Self(content.clone()))
    }

    /// The service the write names.
    pub fn service(&self) -> &str {
        self.0
            .get("service")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }

    /// The write's fields, its service named beside them.
    pub fn args(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut args = self.0.clone();
        args.remove("service");
        args
    }

    /// Whether a run's `service.write` `args` name no content of their
    /// own: each field they give is this write's, as it is (none, or its
    /// `action`). Then it is given this write; any other `args` go to
    /// lamp-services as asked, which refuses a content the owner didn't
    /// say yes to.
    pub fn covers(&self, args: &serde_json::Map<String, serde_json::Value>) -> bool {
        args.iter()
            .all(|(key, value)| key != "service" && self.0.get(key) == Some(value))
    }
}

/// The live runs, by the digest of their token (the token itself is not kept).
#[derive(Default)]
pub struct Runs {
    by_token: HashMap<[u8; 32], Run>,
    serial: u64,
}

impl Runs {
    /// A run of `skill` named `run` with `grants`, until `now_us + for_ms`
    /// (never past [`LONGEST_MS`]); the token it holds and the run. A second
    /// mint for a live run replaces the first token.
    pub fn mint(
        &mut self,
        token: &str,
        run: &str,
        skill: &str,
        grants: BTreeSet<Grant>,
        for_ms: Option<u64>,
        now_us: u64,
    ) -> Result<Run, String> {
        if !self.by_token.values().any(|live| live.run == run) && self.by_token.len() >= MOST_RUNS {
            return Err(format!("{MOST_RUNS} runs are going: one must end first"));
        }
        self.by_token.retain(|_, live| live.run != run);
        self.serial += 1;
        let for_ms = for_ms.unwrap_or(DEFAULT_MS).clamp(1, LONGEST_MS);
        let live = Run {
            run: run.to_owned(),
            skill: skill.to_owned(),
            client: run_client(run),
            grants,
            until_us: now_us.saturating_add(for_ms.saturating_mul(1000)),
            serial: self.serial,
            fixed_write: None,
        };
        self.by_token.insert(token_key(token), live.clone());
        Ok(live)
    }

    /// Marks live run `run` as a skill's fixed write's, `write`
    /// ([`Run::fixed_write`]).
    pub fn mark_fixed_write(&mut self, run: &str, write: &FixedWrite) {
        for live in self.by_token.values_mut() {
            if live.run == run {
                live.fixed_write = Some(write.clone());
            }
        }
    }

    /// The run `token` holds now, if it is one.
    pub fn of(&self, token: &str, now_us: u64) -> Option<&Run> {
        self.by_token
            .get(&token_key(token))
            .filter(|live| now_us < live.until_us)
    }

    /// The run whose client is `client`, if it is live at `now_us`.
    pub fn of_client(&self, client: &str, now_us: u64) -> Option<&Run> {
        self.by_token
            .values()
            .find(|live| live.client == client && now_us < live.until_us)
    }

    /// Ends run `run`: its record, whether it was live.
    pub fn end(&mut self, run: &str) -> Option<Run> {
        let key = self
            .by_token
            .iter()
            .find_map(|(key, live)| (live.run == run).then_some(*key))?;
        self.by_token.remove(&key)
    }

    /// Ends the run only if it is still mint number `serial` (its own timeout
    /// came, and a newer token for the same run is not its to end).
    pub fn end_if(&mut self, run: &str, serial: u64) -> Option<Run> {
        let key = self
            .by_token
            .iter()
            .find_map(|(key, live)| (live.run == run && live.serial == serial).then_some(*key))?;
        self.by_token.remove(&key)
    }

    /// Every run whose time is past: removed, and returned.
    pub fn expire(&mut self, now_us: u64) -> Vec<Run> {
        let gone: Vec<[u8; 32]> = self
            .by_token
            .iter()
            .filter(|(_, live)| now_us >= live.until_us)
            .map(|(key, _)| *key)
            .collect();
        gone.iter()
            .filter_map(|key| self.by_token.remove(key))
            .collect()
    }

    /// The live runs now.
    pub fn live(&self, now_us: u64) -> Vec<&Run> {
        self.by_token
            .values()
            .filter(|live| now_us < live.until_us)
            .collect()
    }
}

/// A new token: 32 random bytes as unpadded base64url.
pub fn new_token(random: &dyn ring::rand::SecureRandom) -> Option<String> {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    random.fill(&mut bytes).ok()?;
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light() -> BTreeSet<Grant> {
        [Grant::Light].into()
    }

    #[test]
    fn a_token_is_one_runs_and_ends_with_it() {
        let mut runs = Runs::default();
        let token = new_token(&ring::rand::SystemRandom::new()).unwrap();
        assert!(token_shape(&token), "{token}");
        let live = runs.mint(&token, "r1", "booth", light(), None, 0).unwrap();
        assert_eq!(live.grants, light());
        assert_eq!(runs.of(&token, 1).map(|run| run.run.as_str()), Some("r1"));
        assert!(runs.of("x".repeat(43).as_str(), 1).is_none());
        // Past its time it is no token; ended it is gone.
        assert!(runs.of(&token, DEFAULT_MS * 1000).is_none());
        assert!(runs.end("r1").is_some());
        assert!(runs.of(&token, 1).is_none());
        assert!(runs.end("r1").is_none());
    }

    #[test]
    fn a_second_mint_replaces_the_first_and_an_old_timeout_cannot_end_the_new() {
        let mut runs = Runs::default();
        let first = runs
            .mint("a".repeat(43).as_str(), "r", "s", light(), None, 0)
            .unwrap();
        let second = runs
            .mint("b".repeat(43).as_str(), "r", "s", light(), None, 0)
            .unwrap();
        assert!(runs.of("a".repeat(43).as_str(), 1).is_none());
        assert!(runs.of("b".repeat(43).as_str(), 1).is_some());
        assert!(runs.end_if("r", first.serial).is_none());
        assert!(runs.end_if("r", second.serial).is_some());
    }

    #[test]
    fn the_time_is_capped_and_the_runs_are_few() {
        let mut runs = Runs::default();
        let live = runs
            .mint(
                "a".repeat(43).as_str(),
                "r",
                "s",
                light(),
                Some(u64::MAX),
                0,
            )
            .unwrap();
        assert_eq!(live.until_us, LONGEST_MS * 1000);
        for n in 0..MOST_RUNS - 1 {
            runs.mint(
                &format!("{n:0>43}"),
                &format!("r{n}"),
                "s",
                light(),
                None,
                0,
            )
            .unwrap();
        }
        assert!(
            runs.mint(&"z".repeat(43), "full", "s", light(), None, 0)
                .is_err()
        );
        // A live run may still be minted again when the table is full.
        assert!(
            runs.mint(&"y".repeat(43), "r", "s", light(), None, 0)
                .is_ok()
        );
    }

    #[test]
    fn names_are_plain_and_a_run_has_one_client() {
        assert!(plain("photo-booth") && plain("run_1.2"));
        assert!(!plain("") && !plain("a b") && !plain("../x") && !plain(&"a".repeat(65)));
        assert_eq!(run_client("r1"), run_client("r1"));
        assert_ne!(run_client("r1"), run_client("r2"));
        assert!(run_client("r1").starts_with("c_") && run_client("r1").len() == 18);
    }
}
