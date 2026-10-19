//! One round of an update: learn, check, install beside, switch at a quiet
//! moment, watch, and go back if a part is not well (docs/updates.md).
//!
//! 1. A switch left under way by the last run (a stop, a power cut) is put
//!    back first.
//! 2. The channel's envelope is read. No network, or nothing published:
//!    nothing changes, and it says so.
//! 3. Its signature is checked with the lamp's own key, and it must be a
//!    newer build than the one the lamp runs, and not one put back before.
//! 4. Each part the lamp runs (a unit in `/etc/systemd/system` starting
//!    `/opt/lampOS/<folder>/<sha256>/lamp-<part>`) is compared with the
//!    build: its binary, the other binaries in its folder, and its unit as
//!    the lamp's own folder in the build carries it.
//! 5. The disk must hold a second copy of everything that changed, and
//!    some to spare, before anything is written.
//! 6. Every new folder is filled beside the old one under a temporary name,
//!    each file checked against its SHA-256, and only when all are whole
//!    are they given their names. A file that fails leaves nothing.
//! 7. At a quiet moment (nobody heard or answered for 45 s, the voice
//!    resting) the units are written and the parts restarted, the voice
//!    first.
//! 8. For a minute each restarted part must stay up with one process and
//!    not say it is worse; otherwise every part is put back, in the reverse
//!    order, and the build is refused from then on.
//!
//! Without `apply` it stops after step 5 and says what it would do, having
//! written nothing.
use crate::{
    fetch::{Channel, Fetch},
    judge::{Before, UnitState, Watch, part_of},
    manifest::{self, Manifest},
    quiet::{BETWEEN_TURNS, QUIET, Quiet},
    records,
    records::{
        Build, Journal, LinkBack, Records, State, Switching, replace_link, sync_folder,
        sync_parent, write_whole,
    },
    unit::{self, Program},
    world::{Clock, Disk, Ears, Systemd},
};
use ring::digest;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

/// This program's own unit, which it never switches.
pub const OWN_UNIT: &str = "lamp-update.service";
/// The voice: switched only at a quiet moment, and first.
pub const VOICE_UNIT: &str = "lamp-voice.service";
/// The body: switched right after the voice, before the other parts.
pub const BODY_UNIT: &str = "lamp-body.service";
/// How often the world is looked at while waiting and watching.
const LOOK_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct Config {
    /// The filesystem the lamp's paths are under: `/` on a lamp.
    pub root: PathBuf,
    /// Where the units are, as the lamp names it: `/etc/systemd/system/`.
    pub units: String,
    /// Where the records are: `/var/lib/lampos-update/`.
    pub records: String,
    /// The channel followed, and its folder: the envelope is in it.
    pub channel: String,
    pub channel_url: String,
    /// The lamp's public key, given at setup.
    pub key: [u8; 32],
    /// This lamp's name: which of the build's lamp folders is its own.
    pub lamp: String,
    /// Act. Without it, say what would be done and write nothing.
    pub apply: bool,
    /// How long to wait for a quiet moment before leaving the switch for
    /// the next round.
    pub quiet_wait: Duration,
    /// How long each switched part is watched.
    pub watch: Duration,
    /// Bytes kept free on the disk beyond the new build.
    pub keep_free: u64,
}

impl Config {
    /// A path of the lamp's, under the root.
    pub fn on(&self, path: &str) -> PathBuf {
        self.root
            .join(path.trim_start_matches('/').trim_end_matches('/'))
    }
}

pub struct World<'a> {
    pub channel: &'a mut dyn Channel,
    pub systemd: &'a mut dyn Systemd,
    pub ears: &'a mut dyn Ears,
    pub disk: &'a dyn Disk,
    pub clock: &'a dyn Clock,
    /// One line for a person: the journal.
    pub say: &'a mut dyn FnMut(&str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The channel could not be read: nothing changed.
    Unreachable(String),
    /// The channel has published no build.
    NothingPublished,
    /// The lamp runs this build, or a later one.
    UpToDate(u64),
    /// Refused before anything was installed or switched.
    Refused(String),
    /// Without `apply`: what would be done.
    Would(Vec<String>),
    /// Installed beside the old build, or not even that, and not switched:
    /// no quiet moment came. The next round tries again.
    NotQuiet(String),
    /// Switched, and every part well.
    Switched(u64),
    /// Switched, a part was not well (or the switch was cut short), and the
    /// previous build was put back. `after` is how the parts were then.
    RolledBack {
        sequence: u64,
        why: String,
        after: Result<(), String>,
        /// What the units not well said of themselves (#1505): printed with
        /// the outcome, never kept in `log.jsonl` or `state.json`.
        told: Told,
    },
    /// Something on the lamp itself went wrong.
    Failed(String),
}

/// What the units a round found not well said of themselves, each a
/// ` (…)` to follow its reason, or empty: how often systemd started it
/// again and its own last journal line. A part's line can carry what a
/// person said, so it is printed and never kept (#1505).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Told {
    /// The unit that failed on the new build.
    pub new: String,
    /// The first unit not well on the old build after the put-back.
    pub old: String,
}

impl Outcome {
    /// The outcome as `log.jsonl` keeps it: as printed, without what the
    /// parts said of themselves ([`Told`]).
    pub fn kept(&self) -> String {
        match self {
            Self::RolledBack {
                sequence,
                why,
                after,
                ..
            } => Self::RolledBack {
                sequence: *sequence,
                why: why.clone(),
                after: after.clone(),
                told: Told::default(),
            }
            .to_string(),
            other => other.to_string(),
        }
    }

    /// A word for the log.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::NothingPublished => "nothing-published",
            Self::UpToDate(_) => "up-to-date",
            Self::Refused(_) => "refused",
            Self::Would(_) => "would",
            Self::NotQuiet(_) => "not-quiet",
            Self::Switched(_) => "switched",
            Self::RolledBack { .. } => "rolled-back",
            Self::Failed(_) => "failed",
        }
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "nothing changed: {why}"),
            Self::NothingPublished => {
                f.write_str("nothing changed: the channel has published no build")
            }
            Self::UpToDate(sequence) => write!(f, "up to date: build {sequence}"),
            Self::Refused(why) => write!(f, "refused, nothing switched: {why}"),
            Self::Would(lines) => {
                f.write_str("would update (nothing written; --apply on to act):")?;
                lines.iter().try_for_each(|line| write!(f, "\n  {line}"))
            }
            Self::NotQuiet(why) => write!(f, "not switched: {why}"),
            Self::Switched(sequence) => {
                write!(f, "switched to build {sequence}; every part is well")
            }
            Self::RolledBack {
                sequence,
                why,
                after,
                told,
            } => {
                write!(f, "build {sequence} put back: {why}{}; ", told.new)?;
                match after {
                    Ok(()) => f.write_str("the previous build is well"),
                    Err(why) => write!(
                        f,
                        "the previous build is not well either: {why}{}",
                        told.old
                    ),
                }
            }
            Self::Failed(why) => write!(f, "failed: {why}"),
        }
    }
}

/// A unit of a part on this lamp.
#[derive(Clone, Debug)]
struct Installed {
    unit: String,
    text: String,
    program: Program,
}

/// Where a file of a new folder comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    /// The build: fetched and checked.
    Build(manifest::File),
    /// The old folder: a model or a setting beside the binary, which a
    /// build does not carry. Linked, or copied.
    Kept(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Want {
    name: String,
    source: Source,
    size: u64,
}

/// One unit that changes.
#[derive(Clone, Debug)]
struct Change {
    unit: String,
    part: String,
    before: String,
    after: String,
    old_folder: String,
    new_folder: String,
    /// The new folder is to be made, with these files.
    make: Option<Vec<Want>>,
    was_active: bool,
    /// A part started through its `current` link: the link, and the build
    /// it names before and after. Its unit is written only if it changes.
    link: Option<Link>,
}

/// A `current` link a switch moves.
#[derive(Clone, Debug)]
struct Link {
    /// As the lamp names it: `/opt/lampOS/voice/current`.
    path: String,
    old: String,
    new: String,
}

/// The order of a switch: the voice first, at the quiet moment itself;
/// then the body; then the rest by name. Going back is the reverse.
fn order(unit: &str) -> (u8, &str) {
    match unit {
        VOICE_UNIT => (0, unit),
        BODY_UNIT => (1, unit),
        _ => (2, unit),
    }
}

/// One round. What goes wrong in the world is the outcome, never a panic.
pub fn round(config: &Config, world: &mut World) -> Outcome {
    let records = Records::new(config.on(&config.records));
    let quiet = Quiet::new(world.clock.now_ms());
    let mut run = Run {
        config,
        world,
        records,
        quiet,
    };
    if !config.apply {
        let state = run.records.peek_state();
        return match run.look(&state) {
            Ok((manifest, changes)) if changes.is_empty() => Outcome::UpToDate(manifest.sequence),
            Ok((manifest, changes)) => Outcome::Would(describe(&manifest, &changes)),
            Err(outcome) => outcome,
        };
    }
    let _lock = match run.records.lock() {
        Ok(lock) => lock,
        Err(error) => return Outcome::Failed(error.to_string()),
    };
    let outcome = run.acting();
    run.records.log(outcome.kind(), &outcome.kept());
    outcome
}

struct Run<'r, 'w> {
    config: &'r Config,
    world: &'r mut World<'w>,
    records: Records,
    quiet: Quiet,
}

/// A unit as it is to be put back.
struct Back {
    unit: String,
    part: String,
    text: String,
    was_active: bool,
    /// It was restarted on the new build, or may have been: it is
    /// restarted again on the old one. One never restarted is not.
    restarted: bool,
    before: Before,
    /// The `current` link, and the build it named before the switch.
    link: Option<LinkBack>,
}

impl Run<'_, '_> {
    fn say(&mut self, line: &str) {
        (self.world.say)(line);
    }

    fn acting(&mut self) -> Outcome {
        match self.records.journal() {
            Ok(Some(journal)) => return self.recover(journal),
            Ok(None) => {}
            Err(error) => return Outcome::Failed(format!("the journal cannot be read: {error}")),
        }
        let mut state = match self.records.state(self.world.say) {
            Ok(state) => state,
            Err(error) => return Outcome::Failed(format!("the state cannot be read: {error}")),
        };
        let (manifest, mut changes) = match self.look(&state) {
            Ok(found) => found,
            Err(outcome) => return outcome,
        };
        let build = Build {
            sequence: manifest.sequence,
            commit: manifest.commit.clone(),
        };
        if changes.is_empty() {
            // Nothing of this build is this lamp's to change: it runs it.
            state.installed = Some(build);
            return match self.records.save_state(&state) {
                Ok(()) => Outcome::UpToDate(manifest.sequence),
                Err(error) => Outcome::Failed(format!("the state cannot be written: {error}")),
            };
        }
        let units: Vec<String> = changes.iter().map(|change| change.unit.clone()).collect();
        match self.world.systemd.states(&units) {
            Ok(states) => {
                for change in &mut changes {
                    change.was_active = states
                        .iter()
                        .any(|state| state.unit == change.unit && state.running());
                }
            }
            Err(why) => return Outcome::Failed(format!("systemd cannot be asked: {why}")),
        }
        if let Err(outcome) = self.install(&mut state, &manifest, &changes) {
            return outcome;
        }
        if let Err(outcome) = self.skills_still_load(&manifest, &changes) {
            return outcome;
        }
        self.switch(&mut state, build, changes)
    }

    /// Steps 2 to 5: what the newest build would change here, or why
    /// nothing will.
    fn look(&mut self, state: &State) -> Result<(Manifest, Vec<Change>), Outcome> {
        let url = format!("{}{}", self.config.channel_url, manifest::ENVELOPE);
        let mut envelope = Vec::new();
        match self
            .world
            .channel
            .fetch(&url, manifest::MOST_ENVELOPE_BYTES, &mut envelope)
        {
            Ok(_) => {}
            Err(Fetch::Missing) => return Err(Outcome::NothingPublished),
            Err(error) => return Err(Outcome::Unreachable(error.to_string())),
        }
        let (manifest, _) = manifest::open(&envelope, &self.config.key)
            .map_err(|why| Outcome::Refused(why.to_string()))?;
        if manifest.channel != self.config.channel {
            return Err(Outcome::Refused(format!(
                "the build is of channel {}, and this lamp follows {}",
                manifest.channel, self.config.channel
            )));
        }
        if let Some(installed) = &state.installed {
            if manifest.sequence == installed.sequence {
                return Err(Outcome::UpToDate(manifest.sequence));
            }
            if manifest.sequence < installed.sequence {
                return Err(Outcome::Refused(format!(
                    "build {} is older than build {}, which the lamp runs",
                    manifest.sequence, installed.sequence
                )));
            }
        }
        if let Some(refusal) = state.refusal(manifest.sequence) {
            return Err(Outcome::Refused(format!(
                "build {} was put back on this lamp ({}); waiting for a newer one",
                manifest.sequence, refusal.why
            )));
        }
        let installed = installed_units(self.config)
            .map_err(|error| Outcome::Failed(format!("the units cannot be read: {error}")))?;
        let carried = self.carried_units(&manifest, &installed)?;
        let changes =
            plan(self.config, &manifest, &installed, &carried).map_err(Outcome::Refused)?;
        self.room(&changes)?;
        Ok((manifest, changes))
    }

    /// The build's units for this lamp's parts, fetched and checked. Small:
    /// read even when nothing is applied, to show what would change.
    fn carried_units(
        &mut self,
        manifest: &Manifest,
        installed: &[Installed],
    ) -> Result<BTreeMap<String, String>, Outcome> {
        let mut carried = BTreeMap::new();
        for unit in &manifest.units {
            if unit.lamp != self.config.lamp || !installed.iter().any(|part| part.unit == unit.unit)
            {
                continue;
            }
            let url = format!("{}{}", manifest.files_url, unit.file_name());
            let mut bytes = Vec::new();
            match self.world.channel.fetch(&url, unit.size, &mut bytes) {
                Ok(_) => {}
                Err(Fetch::Unreachable(why)) => return Err(Outcome::Unreachable(why)),
                Err(error) => {
                    return Err(Outcome::Refused(format!("{}: {error}", unit.file_name())));
                }
            }
            if manifest::sha256_hex(&bytes) != unit.sha256 {
                return Err(Outcome::Refused(format!(
                    "{} does not match its SHA-256",
                    unit.file_name()
                )));
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| Outcome::Refused(format!("{} is not text", unit.file_name())))?;
            carried.insert(unit.unit.clone(), text);
        }
        Ok(carried)
    }

    /// Step 5: room for a second copy of everything that changed, before
    /// anything is written. Counted whole, as if no file could be linked.
    fn room(&self, changes: &[Change]) -> Result<(), Outcome> {
        let needed: u64 = changes
            .iter()
            .flat_map(|change| change.make.iter().flatten())
            .map(|want| want.size)
            .sum();
        if needed == 0 {
            return Ok(());
        }
        let free = self
            .world
            .disk
            .free(&self.config.on(unit::INSTALLED))
            .map_err(|error| {
                Outcome::Failed(format!("the disk's free room cannot be read: {error}"))
            })?;
        if free < needed.saturating_add(self.config.keep_free) {
            return Err(Outcome::Refused(format!(
                "the disk has {} MB free; the new build needs {} MB beside the old one, and {} MB are kept free",
                free / 1_000_000,
                needed.div_ceil(1_000_000),
                self.config.keep_free / 1_000_000
            )));
        }
        Ok(())
    }

    /// Step 6: every new folder filled and checked, then named. All or none.
    fn install(
        &mut self,
        state: &mut State,
        manifest: &Manifest,
        changes: &[Change],
    ) -> Result<(), Outcome> {
        let making: Vec<(&Change, &Vec<Want>)> = changes
            .iter()
            .filter_map(|change| change.make.as_ref().map(|wants| (change, wants)))
            .collect();
        let mut filled = Vec::new();
        let mut result = Ok(());
        for (change, wants) in &making {
            let temporary = filling(&self.config.on(&change.new_folder));
            filled.push(temporary.clone());
            result = self.fill(manifest, &temporary, wants);
            if result.is_err() {
                break;
            }
        }
        if let Err(why) = result {
            for temporary in &filled {
                let _ = fs::remove_dir_all(temporary);
            }
            return Err(why);
        }
        for (change, _) in &making {
            let final_path = self.config.on(&change.new_folder);
            let named = fs::rename(filling(&final_path), &final_path)
                .and_then(|()| sync_parent(&final_path));
            if let Err(error) = named {
                return Err(Outcome::Failed(format!(
                    "{} cannot be named: {error}",
                    final_path.display()
                )));
            }
            if !state.made.contains(&change.new_folder) {
                state.made.push(change.new_folder.clone());
            }
            self.say(&format!(
                "installed {} beside {}",
                change.new_folder, change.old_folder
            ));
        }
        self.records
            .save_state(state)
            .map_err(|error| Outcome::Failed(format!("the state cannot be written: {error}")))
    }

    /// One new folder, under its temporary name.
    fn fill(&mut self, manifest: &Manifest, folder: &Path, wants: &[Want]) -> Result<(), Outcome> {
        let failed = |error: io::Error| Outcome::Failed(format!("{}: {error}", folder.display()));
        if folder.exists() {
            // Left by a run that stopped: this program's, by its name.
            fs::remove_dir_all(folder).map_err(failed)?;
        }
        fs::DirBuilder::new()
            .mode(0o755)
            .create(folder)
            .map_err(failed)?;
        for want in wants {
            let path = folder.join(&want.name);
            match &want.source {
                Source::Build(file) => {
                    let url = format!("{}{}", manifest.files_url, file.name);
                    let mut out = fs::File::create(&path).map_err(failed)?;
                    let mut hashing = Hashing {
                        file: &mut out,
                        context: digest::Context::new(&digest::SHA256),
                    };
                    let fetched = self.world.channel.fetch(&url, file.size, &mut hashing);
                    let sha = manifest::hex(hashing.context.finish().as_ref());
                    match fetched {
                        Ok(size) if size == file.size && sha == file.sha256 => {}
                        Ok(size) => {
                            return Err(Outcome::Refused(format!(
                                "{} does not match its SHA-256 ({size} bytes, {}...; the build says {} bytes, {}...)",
                                file.name,
                                &sha[..16],
                                file.size,
                                &file.sha256[..16]
                            )));
                        }
                        Err(Fetch::Unreachable(why)) => return Err(Outcome::Unreachable(why)),
                        Err(error) => {
                            return Err(Outcome::Refused(format!("{}: {error}", file.name)));
                        }
                    }
                    out.set_permissions(fs::Permissions::from_mode(0o755))
                        .and_then(|()| out.sync_all())
                        .map_err(failed)?;
                }
                Source::Kept(from) => {
                    // The same disk: a link costs nothing. Otherwise a copy.
                    if fs::hard_link(from, &path).is_err() {
                        fs::copy(from, &path).map_err(failed)?;
                        fs::File::open(&path)
                            .and_then(|file| file.sync_all())
                            .map_err(failed)?;
                    }
                }
            }
        }
        sync_folder(folder).map_err(failed)
    }

    /// Step 6b: the skills the lamp's folder holds still load under the
    /// build's lamp-skills (the first builder on #1722). A round places no
    /// skill, so a lamp-skills with checks its folders fail (#1698's
    /// `[permissions]`, on folders from before them) would run with every
    /// skill refused, up and well. The build's own `lamp-skills list` and
    /// the running one's are read on the unit's `--dir`: a skill that loads
    /// now and would not under the build refuses the build, naming each,
    /// with nothing switched. One both refuse holds nothing back.
    fn skills_still_load(
        &mut self,
        manifest: &Manifest,
        changes: &[Change],
    ) -> Result<(), Outcome> {
        let Some(change) = changes
            .iter()
            .find(|change| change.part == SKILLS_PART && change.new_folder != change.old_folder)
        else {
            return Ok(());
        };
        let dir = skills_dir(&change.after);
        let folder = self.config.on(&dir);
        if !folder.is_dir() {
            return Ok(());
        }
        let binary = |build: &str| self.config.on(build).join(SKILLS_BINARY);
        let after = match list_skills(&binary(&change.new_folder), &folder) {
            Ok(Some(refused)) => refused,
            // Older than `list`, so older than its checks (both came with
            // #1698): it refuses nothing for its permissions.
            Ok(None) => return Ok(()),
            Err(why) => {
                return Err(Outcome::Failed(format!(
                    "build {}'s lamp-skills cannot list {dir}: {why}; nothing is switched",
                    manifest.sequence
                )));
            }
        };
        // The running one's: one that cannot say (older than `list`, or
        // gone) loaded every folder.
        let before = list_skills(&binary(&change.old_folder), &folder)
            .ok()
            .flatten()
            .unwrap_or_default();
        let stopping: Vec<String> = after
            .iter()
            .filter(|(name, _)| !before.contains_key(*name))
            .map(|(name, why)| format!("{name} ({why})"))
            .collect();
        if stopping.is_empty() {
            return Ok(());
        }
        Err(Outcome::Refused(format!(
            "build {}'s lamp-skills would not load these skills of {dir}, which load now: {}; \
             nothing is switched until they load under it (`lamp-update install` places the build's own)",
            manifest.sequence,
            stopping.join("; ")
        )))
    }

    /// Feeds what the voice says in the next `wait` to the quiet gate.
    fn listen(&mut self, wait: Duration) {
        for heard in self.world.ears.listen(wait) {
            let now = self.world.clock.now_ms();
            self.quiet.hear(heard, now);
        }
    }

    /// Whether the voice runs here: one that does not hears nobody. Not
    /// known counts as running.
    fn voice_running(&mut self) -> bool {
        self.world
            .systemd
            .states(&[VOICE_UNIT.to_owned()])
            .map(|states| {
                states.iter().any(|state| {
                    state.unit == VOICE_UNIT
                        && !matches!(state.active.as_str(), "inactive" | "failed" | "")
                })
            })
            .unwrap_or(true)
    }

    /// Waits for `long` of quiet, at most `within`. Ok, or why not. With
    /// `unseen_is_quiet`, a voice whose state cannot be seen is not waited
    /// for: that is for putting back a build, not for switching to one.
    fn wait_quiet(
        &mut self,
        long: Duration,
        within: Duration,
        unseen_is_quiet: bool,
    ) -> Result<(), String> {
        let until = self.world.clock.now_ms() + within.as_millis() as u64;
        let mut told = false;
        loop {
            if !self.voice_running() {
                return Ok(());
            }
            let now = self.world.clock.now_ms();
            if unseen_is_quiet && !self.quiet.seen(now) {
                return Ok(());
            }
            let why = match self.quiet.for_at_least(long, now) {
                Ok(()) => return Ok(()),
                Err(why) => why,
            };
            if now >= until {
                return Err(format!("no quiet moment in {} s: {why}", within.as_secs()));
            }
            if !told {
                self.say(&format!("waiting for a quiet moment: {why}"));
                told = true;
            }
            self.listen(LOOK_EVERY);
        }
    }

    /// Writes each unit's text, and reloads.
    fn write_units<'t>(
        &mut self,
        units: impl Iterator<Item = (&'t str, &'t str)>,
    ) -> Result<(), String> {
        for (unit, text) in units {
            let path = self.config.on(&self.config.units).join(unit);
            write_whole(&path, text.as_bytes(), 0o644)
                .map_err(|error| format!("{unit} cannot be written: {error}"))?;
        }
        self.world.systemd.reload()
    }

    /// Points each `current` link at its build, replaced whole.
    fn set_links<'t>(
        &mut self,
        links: impl Iterator<Item = (&'t str, &'t str)>,
    ) -> Result<(), String> {
        for (path, build) in links {
            replace_link(&self.config.on(path), build)
                .map_err(|error| format!("{path} cannot be pointed at {build}: {error}"))?;
        }
        Ok(())
    }

    /// Every unit as it was, then every moved link back to the build it
    /// named. A linked unit the switch did not change is written again with
    /// the same bytes.
    fn put_units_and_links_back(&mut self, backs: &[Back]) -> Result<(), String> {
        self.write_units(
            backs
                .iter()
                .map(|back| (back.unit.as_str(), back.text.as_str())),
        )?;
        self.set_links(backs.iter().filter_map(|back| {
            let link = back.link.as_ref()?;
            Some((link.path.as_str(), link.build.as_str()))
        }))
    }

    /// Steps 7 and 8.
    fn switch(&mut self, state: &mut State, build: Build, changes: Vec<Change>) -> Outcome {
        if let Err(why) = self.wait_quiet(QUIET, self.config.quiet_wait, false) {
            return Outcome::NotQuiet(why);
        }
        // How each part says it is, just before: what it is held to after.
        let health = self.world.ears.health();
        let mut backs: Vec<Back> = changes
            .iter()
            .map(|change| Back {
                unit: change.unit.clone(),
                part: change.part.clone(),
                text: change.before.clone(),
                was_active: change.was_active,
                restarted: false,
                before: Before::of(&change.part, health.as_ref()),
                link: change.link.as_ref().map(|link| LinkBack {
                    path: link.path.clone(),
                    build: link.old.clone(),
                }),
            })
            .collect();
        // The units as they were, and each moved link with the build it
        // named: the way back by hand (docs/updates.md).
        let mut previous: Vec<(String, String)> = backs
            .iter()
            .map(|back| (back.unit.clone(), back.text.clone()))
            .collect();
        previous.extend(backs.iter().filter_map(|back| {
            let link = back.link.as_ref()?;
            Some((
                format!("{}{}", back.unit, records::LINK_SUFFIX),
                format!("{} {}\n", link.path, link.build),
            ))
        }));
        let journal = Journal {
            build: build.clone(),
            units: backs
                .iter()
                .map(|back| Switching {
                    unit: back.unit.clone(),
                    was_active: back.was_active,
                    link: back.link.clone(),
                })
                .collect(),
        };
        if let Err(error) = self
            .records
            .keep_previous(&previous)
            .and_then(|()| self.records.save_journal(&journal))
        {
            return Outcome::Failed(format!("the journal cannot be written: {error}"));
        }
        // A linked part's unit is written only when the build changes it;
        // its link is what moves.
        let written = self
            .write_units(
                changes
                    .iter()
                    .filter(|change| change.link.is_none() || change.after != change.before)
                    .map(|change| (change.unit.as_str(), change.after.as_str())),
            )
            .and_then(|()| {
                self.set_links(changes.iter().filter_map(|change| {
                    let link = change.link.as_ref()?;
                    Some((link.path.as_str(), link.new.as_str()))
                }))
            });
        if let Err(why) = written {
            return self.put_back(state, &build, &backs, None, why, String::new());
        }
        // Writing and reloading took a moment: still quiet, or nothing is
        // started and the old units go back.
        self.listen(Duration::ZERO);
        let now = self.world.clock.now_ms();
        if self.voice_running()
            && let Err(why) = self.quiet.for_at_least(QUIET, now)
        {
            let restored = self.put_units_and_links_back(&backs);
            if let Err(error) = restored.and_then(|()| {
                self.records
                    .clear_journal()
                    .map_err(|error| error.to_string())
            }) {
                return Outcome::Failed(format!("the units could not be put back: {error}"));
            }
            return Outcome::NotQuiet(format!("someone spoke as the switch began: {why}"));
        }
        self.say(&format!("switching to build {}", build.sequence));
        let mut watches = Vec::new();
        for at in 0..backs.len() {
            if !backs[at].was_active {
                continue;
            }
            backs[at].restarted = true;
            let back = &backs[at];
            if let Err(why) = self.world.systemd.restart(&back.unit) {
                let unit = back.unit.clone();
                let told = self.told(&unit);
                return self.put_back(state, &build, &backs, Some(&unit), why, told);
            }
            watches.push(Watch::new(&back.unit, &back.part, back.before.clone()));
        }
        if let Err((unit, why, judged)) = self.watch(&mut watches) {
            let told = if judged {
                self.told(&unit)
            } else {
                String::new()
            };
            return self.put_back(state, &build, &backs, Some(&unit), why, told);
        }
        state.previous = state.installed.replace(build.clone());
        if let Err(error) = self
            .records
            .save_state(state)
            .and_then(|()| self.records.clear_journal())
        {
            return Outcome::Failed(format!(
                "switched, and the state cannot be written: {error}"
            ));
        }
        self.prune(state);
        Outcome::Switched(build.sequence)
    }

    /// Watches the restarted parts for the configured time. The unit and
    /// why, for the first that is not well.
    /// The third is whether the part was found not well: false when systemd
    /// itself could not be asked, and no part was looked at.
    fn watch(&mut self, watches: &mut [Watch]) -> Result<(), (String, String, bool)> {
        if watches.is_empty() {
            return Ok(());
        }
        let units: Vec<String> = watches.iter().map(|watch| watch.unit.clone()).collect();
        let until = self.world.clock.now_ms() + self.config.watch.as_millis() as u64;
        loop {
            let states = self.world.systemd.states(&units).map_err(|why| {
                (
                    units[0].clone(),
                    format!("systemd cannot be asked: {why}"),
                    false,
                )
            })?;
            let done = self.world.clock.now_ms() >= until;
            let health = if done { self.world.ears.health() } else { None };
            for watch in watches.iter_mut() {
                let state = states
                    .iter()
                    .find(|state| state.unit == watch.unit)
                    .cloned()
                    .unwrap_or_else(|| UnitState {
                        unit: watch.unit.clone(),
                        ..UnitState::default()
                    });
                let judged = if done {
                    watch.finish(&state, health.as_ref())
                } else {
                    watch.look(&state)
                };
                judged.map_err(|why| (watch.unit.clone(), why, true))?;
            }
            if done {
                return Ok(());
            }
            self.listen(LOOK_EVERY);
        }
    }

    /// Every changed unit back as it was, then the build refused from then
    /// on.
    fn put_back(
        &mut self,
        state: &mut State,
        build: &Build,
        backs: &[Back],
        failed: Option<&str>,
        why: String,
        // What the failing unit made of the new build ([`Run::told`]), read
        // by the caller before anything is put back.
        told: String,
    ) -> Outcome {
        self.say(&format!(
            "putting build {} back: {why}{told}",
            build.sequence
        ));
        let (after, old) = self.restore(backs, failed);
        state.refuse(build, &why);
        if let Err(error) = self
            .records
            .save_state(state)
            .and_then(|()| self.records.clear_journal())
        {
            return Outcome::Failed(format!(
                "put back, and the state cannot be written: {error}"
            ));
        }
        self.prune(state);
        Outcome::RolledBack {
            sequence: build.sequence,
            why,
            after,
            told: Told { new: told, old },
        }
    }

    /// What a unit not well said of itself, for an outcome and never for the
    /// records (a part's line can carry what a person said): how often
    /// systemd started it again since this round restarted it (`NRestarts`,
    /// which a restart asked for flushes), and its last own journal line.
    /// Empty when there is neither.
    fn told(&mut self, unit: &str) -> String {
        let restarts = self
            .world
            .systemd
            .states(&[unit.to_owned()])
            .ok()
            .and_then(|states| states.into_iter().find(|state| state.unit == unit))
            .map_or(0, |state| state.restarts);
        let line = self.world.systemd.last_line(unit);
        let mut parts = Vec::new();
        match restarts {
            0 => {}
            1 => parts.push("systemd started it again once".to_owned()),
            n => parts.push(format!("systemd started it again {n} times")),
        }
        if let Some(line) = line {
            parts.push(format!("its last line: \"{line}\""));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join("; "))
        }
    }

    /// The units written back, then those restarted on the new build
    /// restarted again, in the reverse order of a switch: the voice last,
    /// and between two turns unless the voice itself is what failed. Then
    /// watched. Every unit is tried, whatever happens to one.
    /// With what the first unit found not well said of itself ([`Told`]).
    fn restore(&mut self, backs: &[Back], failed: Option<&str>) -> (Result<(), String>, String) {
        if let Err(why) = self.put_units_and_links_back(backs) {
            return (Err(why), String::new());
        }
        let mut again: Vec<&Back> = backs.iter().filter(|back| back.restarted).collect();
        again.sort_by(|a, b| order(&b.unit).cmp(&order(&a.unit)));
        let mut watches = Vec::new();
        let mut troubles = Vec::new();
        for back in again {
            if back.unit == VOICE_UNIT && failed != Some(VOICE_UNIT) {
                // A voice that is well is not cut off mid-sentence.
                let within = self.config.quiet_wait;
                if let Err(why) = self.wait_quiet(BETWEEN_TURNS, within, true) {
                    self.say(&format!("the voice is put back all the same: {why}"));
                }
            }
            match self.world.systemd.restart(&back.unit) {
                Ok(()) => watches.push(Watch::new(&back.unit, &back.part, back.before.clone())),
                Err(why) => troubles.push(why),
            }
        }
        let mut told = String::new();
        if let Err((unit, why, judged)) = self.watch(&mut watches) {
            if judged {
                told = self.told(&unit);
            }
            troubles.push(why);
        }
        if troubles.is_empty() {
            (Ok(()), told)
        } else {
            (Err(troubles.join("; ")), told)
        }
    }

    /// Step 1: a switch the last run did not finish is put back.
    fn recover(&mut self, journal: Journal) -> Outcome {
        let why =
            "the switch was cut short (lamp-update stopped, or the lamp lost power)".to_owned();
        self.say(&format!(
            "putting build {} back: {why}",
            journal.build.sequence
        ));
        let mut backs = Vec::new();
        for switching in &journal.units {
            let text = match self.records.previous(&switching.unit) {
                Ok(text) => text,
                Err(error) => {
                    return Outcome::Failed(format!(
                        "{} as it was cannot be read: {error}",
                        switching.unit
                    ));
                }
            };
            // A unit still as it was, with its link as it was, was never
            // switched, nor restarted.
            let now = fs::read_to_string(self.config.on(&self.config.units).join(&switching.unit));
            let link_as_was = switching.link.as_ref().is_none_or(|link| {
                fs::read_link(self.config.on(&link.path))
                    .is_ok_and(|now| now == Path::new(&link.build))
            });
            if now.is_ok_and(|now| now == text) && link_as_was {
                continue;
            }
            backs.push(Back {
                unit: switching.unit.clone(),
                part: switching
                    .unit
                    .trim_start_matches("lamp-")
                    .trim_end_matches(".service")
                    .to_owned(),
                text,
                was_active: switching.was_active,
                restarted: switching.was_active,
                before: Before::default(),
                link: switching.link.clone(),
            });
        }
        let mut state = match self.records.state(self.world.say) {
            Ok(state) => state,
            Err(error) => return Outcome::Failed(format!("the state cannot be read: {error}")),
        };
        self.put_back(&mut state, &journal.build, &backs, None, why, String::new())
    }

    /// Removes the folders this program made that no unit names now, nor
    /// any unit kept from before the last switch: the lamp keeps its build
    /// and the one before. Its own folder, which its unit starts it from,
    /// stays too. A folder it did not make is never removed.
    fn prune(&mut self, state: &mut State) {
        // Named now: by a unit, or by a part's current link.
        let mut named: BTreeSet<String> = installed_units(self.config)
            .unwrap_or_default()
            .into_iter()
            .map(|part| part.program.folder())
            .collect();
        // Named by this program's own unit, which no round switches: the
        // folder install linked `update/current` to is what it starts from
        // at the next boot (#1680).
        let own = fs::read_to_string(self.config.on(&self.config.units).join(OWN_UNIT));
        if let Some(program) = own.ok().and_then(|text| {
            unit::program(&text).or_else(|| unit::linked_program(&text, &self.config.root))
        }) {
            named.insert(program.folder());
        }
        // Named before the last switch: by a unit kept, or a link kept.
        named.extend(
            self.records
                .previous_units()
                .iter()
                .filter_map(|text| unit::program(text))
                .map(|program| program.folder()),
        );
        named.extend(
            self.records
                .previous_links()
                .into_iter()
                .filter_map(|link| {
                    let builds = link.path.strip_suffix(unit::CURRENT)?;
                    Some(unit::folder(builds, &link.build))
                }),
        );
        let mut left = Vec::new();
        for folder in std::mem::take(&mut state.made) {
            // Only a build's folder under /opt/lampOS, whatever the state
            // file says.
            if named.contains(&folder) || !unit::is_build_folder(&folder) {
                left.push(folder);
                continue;
            }
            match fs::remove_dir_all(self.config.on(&folder)) {
                Ok(()) => self.say(&format!("removed {folder}, which no unit names")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    self.say(&format!("{folder} cannot be removed: {error}"));
                    left.push(folder);
                }
            }
        }
        state.made = left;
        if let Err(error) = self.records.save_state(state) {
            self.say(&format!("the state cannot be written: {error}"));
        }
    }
}

/// What a person reads before applying: each unit, what changes, and the
/// lines of its unit file that would change.
fn describe(manifest: &Manifest, changes: &[Change]) -> Vec<String> {
    let mut lines = vec![format!(
        "build {} of {} (commit {})",
        manifest.sequence,
        manifest.channel,
        &manifest.commit[..12]
    )];
    for change in changes {
        let size: u64 = change
            .make
            .iter()
            .flatten()
            .filter(|want| matches!(want.source, Source::Build(_)))
            .map(|want| want.size)
            .sum();
        let folder = if change.old_folder == change.new_folder {
            "same folder, the unit changes".to_owned()
        } else if change.make.is_some() {
            format!(
                "{} -> {} (fetch {:.1} MB)",
                change.old_folder,
                change.new_folder,
                size as f64 / 1e6
            )
        } else {
            format!(
                "{} -> {} (already there)",
                change.old_folder, change.new_folder
            )
        };
        lines.push(format!("{}: {folder}", change.unit));
        for line in unit::differences(
            &change.before.replace(&change.old_folder, "<build>/"),
            &change.after.replace(&change.new_folder, "<build>/"),
        ) {
            lines.push(format!("    {line}"));
        }
    }
    lines.push("switched at a quiet moment: nobody heard or answered for 45 s".into());
    lines
}

/// Every unit of a part installed by build, but this program's own.
fn installed_units(config: &Config) -> io::Result<Vec<Installed>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(config.on(&config.units))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // A linked unit lives somewhere else, which is not this program's
        // to change.
        if !manifest::is_unit_name(&name) || name == OWN_UNIT || !entry.file_type()?.is_file() {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        let program = unit::program(&text).or_else(|| unit::linked_program(&text, &config.root));
        if let Some(program) = program {
            found.push(Installed {
                unit: name,
                text,
                program,
            });
        }
    }
    found.sort_by(|a, b| a.unit.cmp(&b.unit));
    Ok(found)
}

/// The SHA-256 of a file on the lamp.
pub(crate) fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut context = digest::Context::new(&digest::SHA256);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(manifest::hex(context.finish().as_ref()));
        }
        context.update(&buffer[..read]);
    }
}

/// Whether a folder already holds what is wanted: the build's files with
/// their hashes, and a file of each kept name.
fn holds(folder: &Path, wants: &[Want]) -> bool {
    wants.iter().all(|want| {
        let path = folder.join(&want.name);
        match &want.source {
            Source::Build(file) => file_sha256(&path).is_ok_and(|sha| sha == file.sha256),
            Source::Kept(_) => path.is_file(),
        }
    })
}

/// The name of a folder whose first binary did not change and whose other
/// binaries did: the SHA-256 of their list, as `SHA256SUMS` writes it.
/// Still 64 hex characters, so still a build to the next round.
fn list_sha256(wants: &[Want]) -> String {
    let mut lines: Vec<String> = wants
        .iter()
        .filter_map(|want| match &want.source {
            Source::Build(file) => Some(format!("{}  {}\n", file.sha256, file.name)),
            Source::Kept(_) => None,
        })
        .collect();
    lines.sort();
    manifest::sha256_hex(lines.concat().as_bytes())
}

/// What goes into a part's new folder: what is beside its binary now,
/// each from the build if the build carries it, else from the old folder;
/// and the binary. A folder that is not there is installed afresh (V1's
/// lesson: a missing installation is a reinstall, not an error).
fn wants_of(
    config: &Config,
    files: &BTreeMap<&str, &manifest::File>,
    program: &Program,
) -> Result<Vec<Want>, String> {
    let old_folder = program.folder();
    let mut names = BTreeSet::from([program.binary.clone()]);
    if let Ok(entries) = fs::read_dir(config.on(&old_folder)) {
        for entry in entries {
            let entry = entry.map_err(|error| format!("{old_folder}: {error}"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry
                .file_type()
                .map_err(|error| format!("{old_folder}{name}: {error}"))?;
            if !kind.is_file() {
                return Err(format!(
                    "{old_folder}{name} is not a file, and only files are carried to a new build"
                ));
            }
            names.insert(name);
        }
    }
    names
        .into_iter()
        .map(|name| match files.get(name.as_str()) {
            Some(file) => Ok(Want {
                name,
                source: Source::Build((*file).clone()),
                size: file.size,
            }),
            None => {
                let path = config.on(&old_folder).join(&name);
                let size = fs::metadata(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .len();
                Ok(Want {
                    name,
                    source: Source::Kept(path),
                    size,
                })
            }
        })
        .collect()
}

/// Step 4: what changes, unit by unit, in the order of a switch.
fn plan(
    config: &Config,
    manifest: &Manifest,
    installed: &[Installed],
    carried: &BTreeMap<String, String>,
) -> Result<Vec<Change>, String> {
    let files: BTreeMap<&str, &manifest::File> = manifest
        .files
        .iter()
        .map(|file| (file.name.as_str(), file))
        .collect();
    let mut changes: Vec<Change> = Vec::new();
    for part in installed {
        let Some(binary) = files.get(part.program.binary.as_str()) else {
            continue;
        };
        let base = carried.get(&part.unit).unwrap_or(&part.text);
        let target = if part.program.linked {
            // A product part stays started through its link (docs/install.md).
            match unit::linked(base) {
                Some((builds, binary)) if builds == part.program.builds => Program {
                    builds,
                    build: part.program.build.clone(),
                    binary,
                    linked: true,
                },
                _ => {
                    return Err(format!(
                        "the build's {} does not start {} through its current link",
                        part.unit, part.program.builds
                    ));
                }
            }
        } else {
            unit::program(base).ok_or_else(|| {
                format!(
                    "the build's {} does not start a part from /opt/lampOS/<folder>/<sha256>/",
                    part.unit
                )
            })?
        };
        if target.binary != part.program.binary {
            return Err(format!(
                "the build's {} starts {}, and the lamp's starts {}",
                part.unit, target.binary, part.program.binary
            ));
        }
        let wants = wants_of(config, &files, &part.program)?;
        // The folder is named by its binary's SHA-256, unless a folder of
        // that name holds other versions of the binaries beside it.
        let mut name = binary.sha256.clone();
        let mut there = config.on(&unit::folder(&target.builds, &name));
        if there.exists() && !holds(&there, &wants) {
            name = list_sha256(&wants);
            there = config.on(&unit::folder(&target.builds, &name));
            if there.exists() && !holds(&there, &wants) {
                return Err(format!(
                    "{} holds other files than the build's",
                    there.display()
                ));
            }
        }
        let new_folder = unit::folder(&target.builds, &name);
        // A linked unit names no build: it is only ever the build's own text.
        let after = if target.linked {
            base.clone()
        } else {
            unit::with_build(base, &target, &name)
        };
        let link = target.linked.then(|| Link {
            path: unit::current(&target.builds),
            old: part.program.build.clone(),
            new: name.clone(),
        });
        let old_folder = part.program.folder();
        if new_folder == old_folder && unit::same(&after, &part.text) {
            continue;
        }
        // Two units of one folder (a part started two ways) make it once.
        let make = match changes.iter().find(|other| other.new_folder == new_folder) {
            Some(other) if other.make.is_some() && other.make.as_ref() != Some(&wants) => {
                return Err(format!("two units want {new_folder} with other files"));
            }
            Some(_) => None,
            None => (!there.exists()).then_some(wants),
        };
        changes.push(Change {
            unit: part.unit.clone(),
            part: part_of(&part.program.binary).to_owned(),
            before: part.text.clone(),
            after,
            old_folder,
            new_folder,
            make,
            was_active: false,
            link,
        });
    }
    changes.sort_by(|a, b| order(&a.unit).cmp(&order(&b.unit)));
    Ok(changes)
}

/// The skills part, and its binary.
const SKILLS_PART: &str = "skills";
const SKILLS_BINARY: &str = "lamp-skills";
/// Where lamp-skills reads its skills when its unit gives no `--dir`.
const SKILLS_DIR: &str = "/var/lib/lampOS/skills";
/// How long `lamp-skills list` may take: killed after, and then it cannot say.
const LIST_WITHIN: Duration = Duration::from_secs(30);

/// The folder a lamp-skills unit reads: its `ExecStart=`'s `--dir`.
fn skills_dir(unit: &str) -> String {
    unit.lines()
        .find_map(|line| line.trim().strip_prefix("ExecStart="))
        .and_then(|exec| {
            let mut words = exec.split_whitespace();
            words.find(|word| *word == "--dir")?;
            words.next().map(str::to_owned)
        })
        .unwrap_or_else(|| SKILLS_DIR.to_owned())
}

/// The skills `binary list --dir folder` says are not loaded, by name, with
/// why; `None` for a lamp-skills with no `list` (a wrong option, status 2).
fn list_skills(binary: &Path, folder: &Path) -> Result<Option<BTreeMap<String, String>>, String> {
    let mut child = std::process::Command::new(binary)
        .arg("list")
        .arg("--dir")
        .arg(folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", binary.display()))?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let err = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < LIST_WITHIN => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it did not end within {} s", LIST_WITHIN.as_secs()));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    match status.code() {
        Some(0) => Ok(Some(not_loaded(&out))),
        Some(2) => Ok(None),
        _ => Err(err
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map_or_else(|| status.to_string(), |line| line.trim().to_owned())),
    }
}

/// The rows of `lamp-skills list` whose last column is `not loaded: WHY`.
fn not_loaded(listed: &str) -> BTreeMap<String, String> {
    const NOT_LOADED: &str = "not loaded: ";
    listed
        .lines()
        .filter_map(|line| {
            let at = line.find(NOT_LOADED)?;
            let name = line.split_whitespace().next()?;
            Some((
                name.to_owned(),
                line[at + NOT_LOADED.len()..].trim().to_owned(),
            ))
        })
        .collect()
}

/// What is written into a file being fetched, hashed on the way.
pub(crate) struct Hashing<'a> {
    pub(crate) file: &'a mut fs::File,
    pub(crate) context: digest::Context,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.context.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// The temporary name of a folder being filled.
pub(crate) fn filling(final_path: &Path) -> PathBuf {
    let name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    final_path.with_file_name(format!(".{name}.lamp-update"))
}
