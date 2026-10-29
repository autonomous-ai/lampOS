//! Byte-preserving promotion of an already signed and published build.
use lamp_update::manifest::{self, Manifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod github;
pub mod notes;
pub const REPO: &str = "autonomous-ai/lampOS";
pub const STABLE: &str = "lampos-stable";
pub const CHANNEL: &str = "lamp-voice";
pub const WORKFLOW: &str = ".github/workflows/lampos-arm64.yml";
pub type Result<T> = std::result::Result<T, String>;
const MAX_ASSETS: usize = 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn require(ok: bool, why: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(why.to_owned()) }
}
pub fn positive(text: &str) -> Result<u64> {
    require(
        !text.is_empty() && !text.starts_with('0') && text.bytes().all(|c| c.is_ascii_digit()),
        "expected a positive decimal integer",
    )?;
    let value = text
        .parse::<u64>()
        .map_err(|_| "integer exceeds u64".to_owned())?;
    require(value > 0, "integer is zero")?;
    Ok(value)
}
fn plain(name: &str) -> bool {
    name.len() <= 255
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}
fn commit(sha: &str) -> bool {
    sha.len() == 40 && manifest::is_lower_hex(sha)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Release {
    pub id: u64,
    pub tag_name: String,
    pub target_commitish: String,
    pub draft: bool,
    pub prerelease: bool,
    // GitHub permits null notes on unrelated releases. Selected source notes
    // still have to match the publisher's exact provenance in `notes`.
    #[serde(default, deserialize_with = "nullable_notes")]
    pub body: String,
}
fn nullable_notes<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    Option::<String>::deserialize(deserializer).map(Option::unwrap_or_default)
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub state: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashSize {
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub run_attempt: u64,
    pub run_number: u64,
    pub head_sha: String,
    pub head_branch: String,
    pub path: String,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub repository: Repository,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub private: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Job {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub run_id: u64,
    pub head_sha: String,
}
/// Operations are injected so every remote mutation and uncertain response can be tested.
pub trait Remote {
    fn repository(&mut self) -> Result<Repository>;
    fn releases(&mut self) -> Result<Vec<Release>>;
    fn tag(&mut self, name: &str) -> Result<Option<String>>;
    fn assets(&mut self, release: u64) -> Result<Vec<Asset>>;
    fn read(&mut self, asset: &Asset, most: u64) -> Result<Vec<u8>>;
    fn hash(&mut self, asset: &Asset, most: u64) -> Result<HashSize>;
    fn run(&mut self, run: u64, attempt: u64) -> Result<WorkflowRun>;
    fn jobs(&mut self, run: u64, attempt: u64) -> Result<Vec<Job>>;
    fn create(&mut self, tag: &str, sha: &str, notes: &str, name: &str, bytes: &[u8])
    -> Result<()>;
    fn upload(&mut self, tag: &str, name: &str, bytes: &[u8]) -> Result<()>;
    fn rename(&mut self, id: u64, name: &str) -> Result<Asset>;
    /// Deletes one release asset by its id; never asked again.
    fn delete(&mut self, id: u64) -> Result<()>;
}
#[derive(Clone)]
pub struct Context {
    pub sequence: u64,
    pub run: u64,
    pub attempt: u64,
    pub key: [u8; 32],
}
impl Context {
    pub fn environment(sequence: &str, env: &BTreeMap<String, String>) -> Result<Self> {
        for (name, expected) in [
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REPOSITORY", REPO),
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
            ("GITHUB_REF", "refs/heads/lamp-voice"),
            ("LAMPOS_STABLE_PROMOTION", "on"),
        ] {
            require(
                env.get(name).is_some_and(|v| v == expected),
                &format!("invalid {name}"),
            )?;
        }
        require(
            env.get("PROMOTE_FULL")
                .is_none_or(|v| v == "false" || v.is_empty()),
            "promotion cannot also be a full build",
        )?;
        let value = |name| {
            env.get(name)
                .map(String::as_str)
                .ok_or_else(|| format!("missing {name}"))
        };
        Ok(Self {
            sequence: positive(sequence)?,
            run: positive(value("GITHUB_RUN_ID")?)?,
            attempt: positive(value("GITHUB_RUN_ATTEMPT")?)?,
            key: manifest::read_public_key(value("LAMPOS_UPDATE_PUBLIC_KEY")?).map_err(|e| e.0)?,
        })
    }
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub outcome: &'static str,
    pub sequence: u64,
    pub commit: String,
    pub source_run: u64,
    pub source_attempt: u64,
    pub promotion_run: u64,
    pub promotion_attempt: u64,
    pub previous_asset: Option<u64>,
    pub stable_asset: u64,
    pub envelope_sha256: String,
}
struct Candidate {
    release: Release,
    bytes: Vec<u8>,
    manifest: Manifest,
    run: u64,
    attempt: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Head {
    release: Release,
    asset: Asset,
    sha256: String,
    sequence: u64,
}

fn releases(remote: &mut impl Remote) -> Result<BTreeMap<String, Release>> {
    let mut found = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for release in remote.releases()? {
        require(
            release.id > 0 && ids.insert(release.id),
            "invalid or duplicate release ID",
        )?;
        require(
            found.insert(release.tag_name.clone(), release).is_none(),
            "duplicate release tag",
        )?;
    }
    Ok(found)
}
fn assets(remote: &mut impl Remote, release: u64) -> Result<BTreeMap<String, Asset>> {
    let entries = remote.assets(release)?;
    require(entries.len() <= MAX_ASSETS, "too many release assets")?;
    let mut names = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for asset in entries {
        require(
            asset.id > 0 && ids.insert(asset.id) && plain(&asset.name) && asset.state == "uploaded",
            "invalid release asset inventory",
        )?;
        require(
            names.insert(asset.name.clone(), asset).is_none(),
            "duplicate release asset name",
        )?;
    }
    Ok(names)
}
fn read(remote: &mut impl Remote, asset: &Asset, most: u64) -> Result<Vec<u8>> {
    require(asset.size <= most, "asset exceeds its size bound")?;
    let bytes = remote.read(asset, most)?;
    require(
        bytes.len() as u64 == asset.size,
        "downloaded asset size differs",
    )?;
    Ok(bytes)
}
fn opened(bytes: &[u8], key: &[u8; 32]) -> Result<(Manifest, Vec<u8>)> {
    let (value, payload) = manifest::open(bytes, key).map_err(|e| e.0)?;
    require(value.channel == CHANNEL, "wrong signed channel")?;
    require(
        value.files_url
            == format!(
                "https://github.com/{REPO}/releases/download/lampos-lamp-voice-{}/",
                value.sequence
            ),
        "signed payload URL is not its immutable build",
    )?;
    Ok((value, payload))
}
fn notes(release: &Release, sequence: u64) -> Result<(u64, u64)> {
    let prefix = format!(
        "Built and tested from {}; ARM64 workflow run ",
        release.target_commitish
    );
    let body = release.body.strip_suffix('\n').unwrap_or(&release.body);
    let remaining = body
        .strip_prefix(&prefix)
        .ok_or("source release lacks canonical provenance")?;
    let (run, remaining) = remaining
        .split_once(", attempt ")
        .ok_or("source run is absent")?;
    let (attempt, suffix) = remaining
        .split_once(", sequence ")
        .ok_or("source attempt is absent")?;
    require(
        suffix == format!("{sequence}."),
        "source note sequence differs",
    )?;
    Ok((positive(run)?, positive(attempt)?))
}
fn candidate(remote: &mut impl Remote, context: &Context) -> Result<Candidate> {
    let tag = format!("lampos-lamp-voice-{}", context.sequence);
    let release = releases(remote)?
        .remove(&tag)
        .ok_or("selected published build is absent")?;
    require(
        !release.draft && release.prerelease && commit(&release.target_commitish),
        "source is not a published immutable build",
    )?;
    require(
        remote.tag(&tag)?.as_deref() == Some(&release.target_commitish),
        "source tag differs from its release commit",
    )?;
    let (run_id, attempt) = notes(&release, context.sequence)?;
    let run = remote.run(run_id, attempt)?;
    require(
        run.id == run_id
            && run.run_attempt == attempt
            && run.run_number == context.sequence
            && run.head_sha == release.target_commitish
            && run.head_branch == CHANNEL
            && run.path == WORKFLOW
            && run.event == "push"
            && run.status == "completed"
            && run.conclusion.as_deref() == Some("success")
            && run.repository.full_name == REPO
            && !run.repository.private,
        "source workflow attempt is not a successful lamp-voice push",
    )?;
    let jobs = remote.jobs(run_id, attempt)?;
    let good = |job: &&Job| {
        job.run_id == run_id
            && job.head_sha == release.target_commitish
            && job.status == "completed"
            && job.conclusion.as_deref() == Some("success")
    };
    let arm: Vec<_> = jobs
        .iter()
        .filter(|j| {
            j.name
                .starts_with("lampOS — build and test on Linux ARM64 (")
        })
        .collect();
    let publish: Vec<_> = jobs
        .iter()
        .filter(|j| j.name == "lampOS — sign and publish the tested lamp-voice artifact")
        .collect();
    require(
        arm.len() == 1 && arm.iter().all(good) && publish.len() == 1 && publish.iter().all(good),
        "successful source build and publication jobs are absent",
    )?;
    let inventory = assets(remote, release.id)?;
    let envelope = inventory
        .get(manifest::ENVELOPE)
        .ok_or("source envelope is absent")?;
    let bytes = read(remote, envelope, manifest::MOST_ENVELOPE_BYTES)?;
    let (value, payload) = opened(&bytes, &context.key)?;
    require(
        value.sequence == context.sequence && value.commit == release.target_commitish,
        "source signed identity differs",
    )?;
    require(
        value.models.is_empty(),
        "external model inventories are not supported for stable promotion",
    )?;
    let raw = inventory
        .get("manifest.json")
        .ok_or("source plain manifest is absent")?;
    require(
        read(remote, raw, manifest::MOST_MANIFEST_BYTES as u64)? == payload,
        "plain manifest differs from signed bytes",
    )?;
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    let declared = value
        .files
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                f.size,
                f.sha256.as_str(),
                manifest::MOST_FILE_BYTES,
            )
        })
        .chain(value.units.iter().map(|u| {
            (
                u.file_name(),
                u.size,
                u.sha256.as_str(),
                manifest::MOST_UNIT_BYTES,
            )
        }));
    for (name, size, sha256, bound) in declared {
        require(
            plain(&name) && names.insert(name.clone()),
            "duplicate or unsafe declared asset",
        )?;
        total = total.checked_add(size).ok_or("payload size overflow")?;
        require(
            total <= MAX_TOTAL_BYTES,
            "declared release payload exceeds 2 GiB",
        )?;
        let asset = inventory
            .get(&name)
            .ok_or("declared payload asset is absent")?;
        require(
            asset.size == size && size <= bound,
            "declared asset size differs",
        )?;
        let actual = remote.hash(asset, bound)?;
        require(
            actual.size == size && actual.sha256 == sha256,
            "declared asset bytes differ",
        )?;
    }
    Ok(Candidate {
        release,
        bytes,
        manifest: value,
        run: run_id,
        attempt,
    })
}
fn head(remote: &mut impl Remote, key: &[u8; 32]) -> Result<Option<Head>> {
    let Some(release) = releases(remote)?.remove(STABLE) else {
        require(
            remote.tag(STABLE)?.is_none(),
            "stable tag exists without a release; repair required",
        )?;
        return Ok(None);
    };
    require(
        !release.draft && release.prerelease,
        "stable is not a published prerelease",
    )?;
    let asset = assets(remote, release.id)?
        .remove(manifest::ENVELOPE)
        .ok_or("stable has no canonical envelope; repair required")?;
    let bytes = read(remote, &asset, manifest::MOST_ENVELOPE_BYTES)?;
    let (value, _) = opened(&bytes, key)?;
    Ok(Some(Head {
        release,
        asset,
        sha256: manifest::sha256_hex(&bytes),
        sequence: value.sequence,
    }))
}
fn rename(remote: &mut impl Remote, asset: &Asset, name: &str) -> Result<()> {
    let result = remote.rename(asset.id, name)?;
    require(
        result.id == asset.id
            && result.name == name
            && result.size == asset.size
            && result.state == "uploaded",
        "asset rename was not confirmed",
    )
}
fn restore(
    remote: &mut impl Remote,
    release: &Release,
    previous: Option<&Head>,
    staged: &Asset,
    staged_name: &str,
) -> Result<()> {
    let inventory = assets(remote, release.id)?;
    let canonical = inventory.get(manifest::ENVELOPE);
    if canonical.map(|a| a.id) != previous.map(|h| h.asset.id) {
        if let Some(canonical) = canonical {
            require(
                canonical.id == staged.id,
                "canonical changed outside this promotion",
            )?;
            rename(remote, staged, staged_name)?;
        }
        if let Some(previous) = previous {
            rename(remote, &previous.asset, manifest::ENVELOPE)?;
        }
    }
    let inventory = assets(remote, release.id)?;
    if let Some(previous) = previous {
        let restored = inventory
            .get(manifest::ENVELOPE)
            .ok_or("previous envelope was not restored")?;
        require(
            restored.id == previous.asset.id,
            "restored asset identity differs",
        )?;
        let bytes = read(remote, restored, manifest::MOST_ENVELOPE_BYTES)?;
        require(
            manifest::sha256_hex(&bytes) == previous.sha256,
            "restored envelope bytes differ",
        )?;
    } else {
        require(
            !inventory.contains_key(manifest::ENVELOPE),
            "initial canonical still exists",
        )?;
    }
    Ok(())
}
/// How many backups the stable release keeps, as the channel's publisher
/// does (#4161): GitHub allows at most 1000 assets on one release.
pub const KEEP_BACKUPS: usize = 20;
/// The most backups and stages one promotion deletes, oldest first.
pub const MOST_PRUNED: usize = 250;
/// The sequence in a backup's name, `lampos-manifest-previous-<seq>-<id>.json`,
/// or a stage's, `lampos-manifest-staged-<seq>-<run>-<attempt>.json`.
fn named_sequence(name: &str, prefix: &str, parts: usize) -> Option<u64> {
    let numbers: Vec<&str> = name
        .strip_prefix(prefix)?
        .strip_suffix(".json")?
        .split('-')
        .collect();
    (numbers.len() == parts
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0')))
    .then(|| numbers[0].parse().ok())
    .flatten()
}
/// After a verified promotion to `sequence`: the backups beyond the newest
/// [`KEEP_BACKUPS`] (this promotion's own, and any of a later sequence,
/// always kept) and the stages of older sequences are deleted, oldest first
/// and [`MOST_PRUNED`] at most. The head is never among them. How many went.
fn prune(remote: &mut impl Remote, release: u64, sequence: u64) -> Result<usize> {
    let inventory = assets(remote, release)?;
    let mut older: Vec<(u64, u64)> = inventory
        .values()
        .filter_map(|a| {
            named_sequence(&a.name, "lampos-manifest-previous-", 2)
                .filter(|seq| *seq < sequence)
                .map(|seq| (seq, a.id))
        })
        .collect();
    older.sort_unstable();
    older.truncate(older.len().saturating_sub(KEEP_BACKUPS - 1));
    let mut doomed: Vec<(u64, u64)> = inventory
        .values()
        .filter_map(|a| {
            named_sequence(&a.name, "lampos-manifest-staged-", 3)
                .filter(|seq| *seq < sequence)
                .map(|seq| (seq, a.id))
        })
        .chain(older)
        .collect();
    doomed.sort_unstable();
    doomed.truncate(MOST_PRUNED);
    for (_, id) in &doomed {
        require(
            inventory.get(manifest::ENVELOPE).map(|a| a.id) != Some(*id),
            "the head is never pruned",
        )?;
        remote.delete(*id)?;
    }
    Ok(doomed.len())
}
/// No mutation occurs until the source, all payload bytes and existing head are verified.
pub fn promote(remote: &mut impl Remote, context: &Context) -> Result<Receipt> {
    let repo = remote.repository()?;
    require(
        repo.full_name == REPO && !repo.private,
        "wrong or inaccessible public repository",
    )?;
    let candidate = candidate(remote, context)?;
    let previous = head(remote, &context.key)?;
    let hash = manifest::sha256_hex(&candidate.bytes);
    let receipt = |outcome, stable_asset| Receipt {
        outcome,
        sequence: context.sequence,
        commit: candidate.manifest.commit.clone(),
        source_run: candidate.run,
        source_attempt: candidate.attempt,
        promotion_run: context.run,
        promotion_attempt: context.attempt,
        previous_asset: previous.as_ref().map(|h| h.asset.id),
        stable_asset,
        envelope_sha256: hash.clone(),
    };
    if let Some(old) = &previous {
        require(
            context.sequence >= old.sequence,
            "selected build is older than stable",
        )?;
        if context.sequence == old.sequence {
            require(
                hash == old.sha256,
                "same sequence has different envelope bytes",
            )?;
            return Ok(receipt("unchanged", old.asset.id));
        }
    }
    let staged_name = format!(
        "lampos-manifest-staged-{}-{}-{}.json",
        context.sequence, context.run, context.attempt
    );
    let release = if let Some(old) = &previous {
        require(
            !assets(remote, old.release.id)?.contains_key(&staged_name),
            "staged asset already exists; retry with a new attempt",
        )?;
        remote.upload(STABLE, &staged_name, &candidate.bytes)?;
        old.release.clone()
    } else {
        require(
            head(remote, &context.key)?.is_none(),
            "stable appeared before bootstrap",
        )?;
        let notes = format!(
            "Promoted unchanged from {}; source ARM64 run {}, attempt {}; promotion run {}, attempt {}.\n",
            candidate.release.tag_name,
            candidate.run,
            candidate.attempt,
            context.run,
            context.attempt
        );
        remote.create(
            STABLE,
            &candidate.manifest.commit,
            &notes,
            &staged_name,
            &candidate.bytes,
        )?;
        let made = releases(remote)?
            .remove(STABLE)
            .ok_or("stable bootstrap was not confirmed")?;
        require(
            !made.draft && made.prerelease && made.target_commitish == candidate.manifest.commit,
            "stable bootstrap identity differs",
        )?;
        require(
            remote.tag(STABLE)?.as_deref() == Some(&candidate.manifest.commit),
            "stable bootstrap tag differs",
        )?;
        made
    };
    let inventory = assets(remote, release.id)?;
    let staged = inventory
        .get(&staged_name)
        .ok_or("staged envelope is absent")?
        .clone();
    require(
        read(remote, &staged, manifest::MOST_ENVELOPE_BYTES)? == candidate.bytes,
        "staged envelope bytes differ",
    )?;
    if previous.is_some() {
        require(
            head(remote, &context.key)? == previous,
            "stable changed before promotion",
        )?;
    } else {
        require(
            !inventory.contains_key(manifest::ENVELOPE),
            "initial stable unexpectedly has a head",
        )?;
    }
    let swap: Result<()> = (|| {
        if let Some(old) = &previous {
            let backup = format!(
                "lampos-manifest-previous-{}-{}.json",
                context.sequence, old.asset.id
            );
            require(!inventory.contains_key(&backup), "backup already exists")?;
            rename(remote, &old.asset, &backup)?;
        }
        rename(remote, &staged, manifest::ENVELOPE)?;
        let current = head(remote, &context.key)?.ok_or("promoted head is absent")?;
        require(
            current.release.id == release.id
                && current.asset.id == staged.id
                && current.sha256 == hash
                && current.sequence == context.sequence,
            "promoted head was not verified",
        )?;
        Ok(())
    })();
    if let Err(error) = swap {
        if let Err(recovery) = restore(remote, &release, previous.as_ref(), &staged, &staged_name) {
            return Err(format!(
                "promotion failed ({error}); restoration failed ({recovery}); inspect release {} and retained assets {}/{}",
                release.id,
                previous.as_ref().map_or(0, |h| h.asset.id),
                staged.id
            ));
        }
        return Err(format!(
            "promotion failed ({error}); previous canonical restored, or initial head left absent"
        ));
    }
    // The promotion stands whatever the cleanup does (#4161).
    match prune(remote, release.id, context.sequence) {
        Ok(0) => {}
        Ok(pruned) => eprintln!("lampos-promote: {pruned} old backups and stages deleted"),
        Err(why) => {
            eprintln!("lampos-promote: stable cleanup stopped, the promotion stands: {why}")
        }
    }
    Ok(receipt("promoted", staged.id))
}
