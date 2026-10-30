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
//!    are they given their names. A file that fails leaves nothing. The
//!    build's product skills are staged with the lamp's skills folder, and
//!    the build's lamp-skills must still load every skill that loads now.
//! 7. At a quiet moment (nobody heard or answered for 45 s, the voice
//!    resting) the units are written, the product skills' files placed,
//!    and the parts restarted in the candidate units' startup order.
//! 8. For a minute each restarted part must stay up with one process and
//!    not say it is worse; otherwise every part is put back in the saved
//!    old startup order, and the build is refused from then on.
//!
//! Without `apply` it stops after step 5 and says what it would do, having
//! written nothing.
use crate::{
    added, apply, companion,
    fetch::{Channel, Fetch},
    image,
    judge::{Before, UnitState, Watch, part_of},
    manifest::{self, Manifest, Model, Opened, TooOld},
    models::{self, ModelBack, Need},
    own,
    product_skills::{self, Bundle},
    quiet::{BETWEEN_TURNS, QUIET, Quiet},
    records,
    records::{
        Added, Build, Journal, LinkBack, Records, SkillBack, State, Switching, replace_link,
        sync_folder, sync_parent, write_whole,
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
/// The voice: its restart boundary must still be quiet.
pub const VOICE_UNIT: &str = "lamp-voice.service";
/// The body; its unit declarations determine its restart order.
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
    /// `LAMP_UPDATE_SELF=on`: once the parts run a build, lamp-update
    /// switches to that build's own binary too (crate::own, #1680).
    pub self_switch: bool,
    /// The folder this program runs from, as the lamp names it
    /// (`/opt/lampOS/update/<sha256>/`), when it is one: never pruned.
    pub running: Option<String>,
    /// This program's own level (`manifest::UPDATER`): a build whose
    /// `min_updater` is above it is refused, all but the newer lamp-update
    /// it carries (#1680).
    pub updater: u64,
    /// `LAMP_UPDATE_MODELS=on`: the models a build carries are fetched,
    /// checked and switched with the parts (crate::models, #1680).
    pub models: bool,
    /// `LAMP_UPDATE_APPLY=on`: the settings and tmpfiles a build carries
    /// are written at the switch by `lamp-update-apply` (crate::apply).
    pub settings: bool,
    /// With no explicit settings switch, a self-updating product installation
    /// accepts settings from a signed level-6 product carrier. Explicit off
    /// leaves this false; dry confirmation rounds use the same validation.
    pub product_settings: bool,
    /// `LAMP_UPDATE_IMAGE=on`: the image's own files a build carries are
    /// written the same way (crate::image, #1369).
    pub image: bool,
    /// This program's own bytes, from which a round refreshes the settings
    /// writer before it looks at a build (crate::writer_swap). None: the
    /// writer is never refreshed.
    pub writer_from: Option<crate::writer_swap::Running>,
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
    /// Where the round is, one of `lamp_msgs::UpdateStatus::NOW`, for
    /// `update/status` (#2194): said as each phase begins.
    pub phase: &'a mut dyn FnMut(&'static str),
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
    /// Refused as [`Outcome::Refused`] is, only because the build's models
    /// differ from the lamp's and `LAMP_UPDATE_MODELS` is not on: the
    /// manifest was read and its signature checked, and this updater is
    /// not switched to apply models. Its own switch confirms on it
    /// (crate::own); the build waits for the models switch.
    WaitsForModels(String),
    /// Without `apply`: what would be done.
    Would(Vec<String>),
    /// Installed beside the old build, or not even that, and not switched:
    /// no quiet moment came. The next round tries again.
    NotQuiet(String),
    /// Switched, and every part well.
    Switched(u64),
    /// The parts run build `sequence`, and `update/current` now names its
    /// lamp-update (`to`) instead of `from`: started in this program's
    /// place, it must confirm itself (crate::own).
    SelfSwitched {
        sequence: u64,
        from: String,
        to: String,
    },
    /// Build `sequence` needs updater `needed`, newer than this one: only
    /// the lamp-update it carries is taken, `update/current` now naming it
    /// (`to`) instead of `from`. Started in this program's place, it must
    /// confirm itself (crate::own), and then reads the build again.
    Climbs {
        sequence: u64,
        needed: u64,
        from: String,
        to: String,
    },
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

    /// The build this outcome is about, where it names one.
    pub fn sequence(&self) -> Option<u64> {
        match self {
            Self::UpToDate(sequence) | Self::Switched(sequence) => Some(*sequence),
            Self::SelfSwitched { sequence, .. }
            | Self::Climbs { sequence, .. }
            | Self::RolledBack { sequence, .. } => Some(*sequence),
            _ => None,
        }
    }

    /// A word for the log.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::NothingPublished => "nothing-published",
            Self::UpToDate(_) => "up-to-date",
            Self::Refused(_) | Self::WaitsForModels(_) => "refused",
            Self::Would(_) => "would",
            Self::NotQuiet(_) => "not-quiet",
            Self::Switched(_) => "switched",
            Self::SelfSwitched { .. } | Self::Climbs { .. } => "self-switched",
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
            Self::Refused(why) | Self::WaitsForModels(why) => {
                write!(f, "refused, nothing switched: {why}")
            }
            Self::Would(lines) => {
                f.write_str("would update (nothing written; --apply on to act):")?;
                lines.iter().try_for_each(|line| write!(f, "\n  {line}"))
            }
            Self::NotQuiet(why) => write!(f, "not switched: {why}"),
            Self::Switched(sequence) => {
                write!(f, "switched to build {sequence}; every part is well")
            }
            Self::SelfSwitched { sequence, from, to } => write!(
                f,
                "build {sequence} runs; lamp-update switches from its build {} to {}, which must confirm itself",
                &from[..from.len().min(12)],
                &to[..to.len().min(12)]
            ),
            Self::Climbs {
                sequence,
                needed,
                from,
                to,
            } => write!(
                f,
                "build {sequence} needs updater {needed}; lamp-update switches from its build {} to {}, which the build carries, and takes nothing else of it until that one has confirmed itself",
                &from[..from.len().min(12)],
                &to[..to.len().min(12)]
            ),
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
    /// A model the build carries: from the old folder when it is already
    /// there with its SHA-256 (`from`), else from the store.
    Model { model: Model, from: Option<PathBuf> },
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
    /// One of a part's templates, sockets or timers (crate::companion),
    /// not its service: its folder is its part's, and it moves nothing.
    kind: Option<companion::Kind>,
    /// A unit the lamp lacks, which the switch adds (crate::added).
    added: Option<Adding>,
}

/// What adding a unit the lamp lacks takes (crate::added).
#[derive(Clone, Debug)]
struct Adding {
    /// The links that enable it, as the lamp names them.
    wants: Vec<String>,
    /// For a part new to the lamp: its `current` link, made to name this
    /// build.
    link: Option<(String, String)>,
}

impl Adding {
    fn added(&self) -> Added {
        Added {
            wants: self.wants.clone(),
            link: self.link.as_ref().map(|(path, _)| path.clone()),
        }
    }
}

/// A `current` link a switch moves.
#[derive(Clone, Debug)]
struct Link {
    /// As the lamp names it: `/opt/lampOS/voice/current`.
    path: String,
    old: String,
    new: String,
}

/// The build's product skills, for a lamp whose lamp-skills reads install's
/// folder: the bundle, and what each of its files needs here.
#[derive(Clone, Debug)]
struct Placing {
    bundle: Bundle,
    steps: Vec<product_skills::Step>,
}

impl Placing {
    fn writes(&self) -> bool {
        product_skills::writes(&self.steps)
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
        skills_back: Vec::new(),
        seen: None,
        climb: None,
        placings: Vec::new(),
        fetching: Vec::new(),
        model_backs: Vec::new(),
        envelope: Vec::new(),
        setting_writes: Vec::new(),
        image_writes: Vec::new(),
        applying: None,
        rollback_order: None,
        held: None,
        apply_failed: false,
    };
    if !config.apply {
        let state = run.records.peek_state();
        return match run.look(&state) {
            Ok((manifest, changes, placing))
                if changes.is_empty()
                    && run.placings.is_empty()
                    && run.setting_writes.is_empty()
                    && !placing.as_ref().is_some_and(Placing::writes) =>
            {
                Outcome::UpToDate(manifest.sequence)
            }
            Ok((manifest, changes, placing)) => {
                let mut lines = describe(
                    &manifest,
                    &changes,
                    placing.as_ref(),
                    &run.placings,
                    &run.fetching,
                );
                lines.extend(
                    run.setting_writes
                        .iter()
                        .map(|path| format!("{path}: the build's, by lamp-update-apply")),
                );
                Outcome::Would(lines)
            }
            Err(outcome) => outcome,
        };
    }
    run.held = Some(match run.records.lock() {
        Ok(lock) => lock,
        Err(error) => return Outcome::Failed(error.to_string()),
    });
    (run.world.phase)("checking");
    // A switch of lamp-update's own not yet confirmed: no round acts, so
    // an updater restarted in phase 2, or the old one run by hand, never
    // begins a second round (#1680). The new one confirms before its first.
    // One that cannot be read waits the same: the updater that runs, or
    // lamp-update-back, settles it as a switch that failed.
    let waits = match own::pending(run.records.folder()) {
        Ok(Some(pending)) => Some(format!(
            "lamp-update's own switch to its build {} is not confirmed yet",
            &pending.new[..pending.new.len().min(12)]
        )),
        Ok(None) => None,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Some(format!(
            "lamp-update's own switch cannot be read ({error}) and is not confirmed"
        )),
        // Not its JSON but the disk or a permission: the file may be fine,
        // so it is never settled. This round waits, says so, and the next
        // one reads it again (the reviewer on #2075).
        Err(error) => {
            let line = format!(
                "lamp-update's own switch could not be read this time ({error}): this round waits and the next one tries again"
            );
            run.say(&line);
            run.records.log("self-unread", &line);
            Some(line)
        }
    };
    if let Some(why) = waits {
        let outcome = Outcome::Refused(why);
        run.records
            .log_round(outcome.kind(), &outcome.kept(), outcome.sequence());
        return outcome;
    }
    run.refresh_writer();
    let mut outcome = run.acting();
    if let Some(too_old) = run.climb.take()
        && let Some(climbed) = run.climb(&too_old)
    {
        outcome = climbed;
    }
    if config.self_switch
        && let Outcome::Switched(sequence) | Outcome::UpToDate(sequence) = outcome
        && let Some(switched) = run.own(sequence)
    {
        outcome = switched;
    }
    run.records
        .log_round(outcome.kind(), &outcome.kept(), outcome.sequence());
    outcome
}

struct Run<'r, 'w> {
    config: &'r Config,
    world: &'r mut World<'w>,
    records: Records,
    quiet: Quiet,
    /// The product skills' files this round's switch wrote, or may have,
    /// as they were: put back with the units.
    skills_back: Vec<SkillBack>,
    /// The manifest the round read, checked.
    seen: Option<Manifest>,
    /// A build for a newer updater whose lamp-update this round may take.
    climb: Option<TooOld>,
    /// The models at paths of their own the build switches, and every
    /// model to fetch into the store first (crate::models).
    placings: Vec<models::Placing>,
    fetching: Vec<Model>,
    /// The model paths a switch moved, as they were: put back in reverse.
    model_backs: Vec<ModelBack>,
    /// The envelope read, as it came: staged for `lamp-update-apply`.
    envelope: Vec<u8>,
    /// The settings the build writes here (crate::apply), and the build
    /// `lamp-update-apply` was asked to write them for: put back with it.
    setting_writes: Vec<String>,
    /// Those of them that are the image's own files (crate::image).
    image_writes: Vec<String>,
    applying: Option<u64>,
    /// Old startup order fixed before switching, including inactive changed
    /// units. None means an older journal must reconstruct its old graph.
    rollback_order: Option<Vec<String>>,
    /// The records' lock, held for the whole round once taken.
    held: Option<records::Lock>,
    /// This round's `apply` of the build's settings failed: a writer not yet
    /// proved is put back after the apply's own undo (crate::writer_swap).
    apply_failed: bool,
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
    /// A unit the switch added: on the way back it is stopped and removed,
    /// with what it added, never written back.
    added: Option<Added>,
}

impl Run<'_, '_> {
    /// The settings writer refreshed from this lamp-update (#3689,
    /// crate::writer_swap), before the round looks at a build, so this
    /// round's settings are written by it. Asked under the round's lock
    /// `held`, and done by the writer's one-shot unit (crate::writer_unit).
    /// Only in a round that applies. What goes wrong is said, and the
    /// writer the lamp has stays.
    fn refresh_writer(&mut self) {
        let Some(running) = self.config.writer_from.clone() else {
            return;
        };
        if !self.config.apply {
            return;
        }
        let folder = self.records.folder().to_owned();
        // A writer refreshed and not proved yet is never refreshed over: its
        // `back` that failed puts it back, and one that ended well since
        // proves it. Until either, it is waited on.
        if crate::writer_swap::unproven(&self.config.root, &folder) {
            let unit = crate::writer_swap::BACK_UNIT;
            let failed = self
                .world
                .systemd
                .states(&[unit.to_owned()])
                .ok()
                .and_then(|states| states.into_iter().find(|state| state.unit == unit))
                .is_some_and(|state| state.active == "failed");
            let done = crate::writer_swap::done(&folder);
            let proved = done
                .as_ref()
                .is_some_and(|done| crate::writer_swap::proved(&folder, done));
            // A `back` that began by the second of the refresh may be the
            // copy before's, which a refresh landing mid-run leaves to fail
            // after the unit's `reset-failed`: waited past, to the next
            // `back`, never put back for. Not known, it counts (El's P3 on
            // #4178).
            // Asked only of a `failed` one.
            let before_refresh = failed
                && done.as_ref().is_some_and(|done| {
                    self.world
                        .systemd
                        .back_began_unix_ms()
                        .is_some_and(|began| began <= done.at_unix_ms)
                });
            if before_refresh {
                self.say(
                    "the settings writer's back failed in a run that began before it was refreshed: the next back judges it",
                );
            } else if failed {
                self.writer_op(crate::writer_swap::Op::PutBack, "its back failed");
            } else if proved {
                self.writer_op(crate::writer_swap::Op::Proven, "its back ended well");
            }
            return;
        }
        let build = manifest::sha256_hex(running.bytes());
        let Some(held) = self.held.as_ref() else {
            return;
        };
        let asked =
            crate::writer_swap::ask(&self.config.root, held, running.bytes(), records::unix_ms());
        let ran = match asked {
            Ok(false) => return,
            Ok(true) => crate::writer_unit::run(&self.config.root, self.world.systemd),
            Err(why) => Err(why),
        };
        if let Err(why) = ran {
            crate::writer_swap::forget(&folder);
            self.say(&format!("the settings writer is not refreshed: {why}"));
            return;
        }
        match crate::writer_swap::done(&folder) {
            Some(done) if done.build == build => self.say(&format!(
                "the settings writer is now build {}{}",
                &build[..12],
                if done.synced {
                    ""
                } else {
                    ", but its folder could not be synced"
                }
            )),
            _ => self.say(&format!(
                "{} ran, and the settings writer is not build {}",
                crate::writer_unit::UNIT,
                &build[..12]
            )),
        }
    }

    /// The writer's unit asked to put a refreshed writer back, or to take
    /// it as proved (crate::writer_swap), and what became of it said.
    fn writer_op(&mut self, op: crate::writer_swap::Op, why: &str) {
        let (Some(running), Some(held)) = (self.config.writer_from.clone(), self.held.as_ref())
        else {
            return;
        };
        let folder = self.records.folder().to_owned();
        let ran = crate::writer_swap::request(held, running.bytes(), op, records::unix_ms())
            .and_then(|()| crate::writer_unit::run(&self.config.root, self.world.systemd));
        let line = match (op, ran) {
            (crate::writer_swap::Op::PutBack, Ok(())) => {
                format!(
                    "the settings writer refreshed is put back ({why}): the copy before is the writer again"
                )
            }
            (crate::writer_swap::Op::Proven, Ok(())) => {
                format!(
                    "the settings writer refreshed has proved itself ({why}): the copy before is removed"
                )
            }
            (_, Ok(())) => return,
            (_, Err(error)) => {
                crate::writer_swap::forget(&folder);
                format!("the settings writer could not be put back or proved ({why}): {error}")
            }
        };
        self.say(&line);
    }

    fn leave_completed_setup(&mut self, back: &Back, after: &str) -> bool {
        crate::setup_lifecycle::completed(
            &self.config.root,
            &self.config.on(&self.config.units),
            &back.unit,
            &back.text,
            after,
        ) && self.world.systemd.setup_unit_properties()
            .as_deref().is_some_and(crate::setup_lifecycle::manager_matches)
            // The bounded manager query can take time; reset may have removed
            // completion meanwhile. This last observation is not a reset lock.
            && crate::setup_lifecycle::done(&self.config.root)
    }

    fn say(&mut self, line: &str) {
        (self.world.say)(line);
    }

    fn acting(&mut self) -> Outcome {
        match self.records.journal() {
            Ok(Some(journal)) => return self.recover(journal),
            Ok(None) => {}
            // A journal it cannot read is set aside, said, and the round
            // goes on from the units as they are (#1505): updates must not
            // stop for good because of their own file.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                match self.records.set_aside("journal.json") {
                    Ok(aside) => {
                        let line = format!(
                            "the journal could not be read ({error}): kept as {}, and the units are taken as they are",
                            aside.display()
                        );
                        self.say(&line);
                        self.records.log("journal-set-aside", &line);
                    }
                    Err(error) => {
                        return Outcome::Failed(format!(
                            "the journal cannot be set aside: {error}"
                        ));
                    }
                }
            }
            Err(error) => return Outcome::Failed(format!("the journal cannot be read: {error}")),
        }
        let mut state = match self.records.state(self.world.say) {
            Ok(state) => state,
            Err(error) => return Outcome::Failed(format!("the state cannot be read: {error}")),
        };
        let (manifest, mut changes, placing) = match self.look(&state) {
            Ok(found) => found,
            Err(outcome) => return outcome,
        };
        let build = Build {
            sequence: manifest.sequence,
            commit: manifest.commit.clone(),
        };
        let writes = placing.as_ref().is_some_and(Placing::writes);
        if writes
            && let Err(error) = product_skills::writable(&self.config.root, product_skills::FOLDER)
        {
            // Not the build's fault: it is not refused, and the next round
            // tries again.
            return Outcome::Failed(format!(
                "{} cannot be written ({error}): lamp-update.service needs it in ReadWritePaths; nothing is switched",
                product_skills::FOLDER
            ));
        }
        if changes.is_empty()
            && !writes
            && self.placings.is_empty()
            && self.setting_writes.is_empty()
        {
            // Nothing of this build is this lamp's to change: it runs it.
            if let Some(placing) = &placing {
                record_skills(&mut state, &placing.steps);
            }
            state.installed = Some(build);
            return match self.records.save_state(&state) {
                Ok(()) => Outcome::UpToDate(manifest.sequence),
                Err(error) => Outcome::Failed(format!("the state cannot be written: {error}")),
            };
        }
        // The setup walk under way: nothing fetched or switched this round,
        // at most `walk::MOST_HELD` rounds on a lamp (#3689, crate::walk).
        if crate::walk::under_way(self.config)
            && let Some(held) = crate::walk::hold(self.records.folder())
        {
            return Outcome::NotQuiet(crate::walk::held(held));
        }
        // A template is never active itself, and never restarted: only its
        // instances run.
        let units: Vec<String> = changes
            .iter()
            .filter(|change| change.kind != Some(companion::Kind::Template))
            .map(|change| change.unit.clone())
            .collect();
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
        (self.world.phase)("fetching");
        if let Err(outcome) = self.install(&mut state, &manifest, &changes) {
            return outcome;
        }
        if let Err(outcome) = self.skills_still_load(&manifest, &changes, placing.as_ref()) {
            return outcome;
        }
        (self.world.phase)("switching");
        if changes.is_empty() && self.placings.is_empty() && self.setting_writes.is_empty() {
            // Only the product skills change: lamp-skills reloads its
            // folder by itself, so nothing is restarted.
            return self.place_alone(&mut state, build, placing);
        }
        self.switch(&mut state, build, changes, placing)
    }

    /// Each core skill's file put back over what the lamp did to it, said
    /// once it is written, never before (the reviewer on #2585).
    fn say_put_back(&mut self, steps: &[product_skills::Step]) {
        for step in steps {
            if let product_skills::Act::PutBack(why) = step.act {
                self.say(&format!(
                    "put back {}: {why}; a core skill's files are the build's",
                    step.path
                ));
            }
        }
    }

    /// The product skills of a build that changes no part here: written
    /// (each file as it was put back if one cannot be), recorded, and the
    /// build is the lamp's.
    fn place_alone(
        &mut self,
        state: &mut State,
        build: Build,
        placing: Option<Placing>,
    ) -> Outcome {
        let Some(placing) = placing else {
            return Outcome::UpToDate(build.sequence);
        };
        let steps = match self.plan_skills(state, &placing) {
            Ok(steps) => steps,
            Err(why) => return Outcome::Failed(why),
        };
        let backs = match product_skills::backs(&self.config.root, &steps) {
            Ok(backs) => backs,
            Err(error) => return Outcome::Failed(format!("the product skills: {error}")),
        };
        if let Err(why) = product_skills::write(&self.config.root, &steps) {
            let root = self.config.root.clone();
            let restored = product_skills::restore(&root, &backs, self.world.say);
            return Outcome::Failed(match restored {
                Ok(()) => format!("{why}; the product skills were put back as they were"),
                Err(also) => format!("{why}; and putting them back: {also}"),
            });
        }
        self.say_put_back(&steps);
        self.say(&format!(
            "placed {} files of the product skills",
            backs.len()
        ));
        record_skills(state, &steps);
        state.installed = Some(build.clone());
        match self.records.save_state(state) {
            Ok(()) => Outcome::UpToDate(build.sequence),
            Err(error) => Outcome::Failed(format!("the state cannot be written: {error}")),
        }
    }

    /// What each product skill's file needs now, as the state records them:
    /// planned again at the switch, since the lamp may have changed one
    /// while the round waited for a quiet moment.
    fn plan_skills(
        &self,
        state: &State,
        placing: &Placing,
    ) -> Result<Vec<product_skills::Step>, String> {
        product_skills::plan(
            &self.config.root,
            &placing.bundle,
            &state.settings,
            &state.settings_writing,
        )
        .map_err(|error| format!("the product skills cannot be read: {error}"))
    }

    /// Steps 2 to 5: what the newest build would change here, or why
    /// nothing will.
    fn look(&mut self, state: &State) -> Result<(Manifest, Vec<Change>, Option<Placing>), Outcome> {
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
        let manifest =
            match manifest::open_or_too_old(&envelope, &self.config.key, self.config.updater)
                .map_err(|why| Outcome::Refused(why.to_string()))?
            {
                Opened::Manifest(manifest, _) => manifest,
                Opened::TooOld(too_old) => return Err(self.too_old(state, too_old)),
            };
        if manifest.channel != self.config.channel {
            return Err(Outcome::Refused(format!(
                "the build is of channel {}, and this lamp follows {}",
                manifest.channel, self.config.channel
            )));
        }
        self.seen = Some(manifest.clone());
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
        let companions = installed_companions(self.config)
            .map_err(|error| Outcome::Failed(format!("the units cannot be read: {error}")))?;
        let carried = self.carried_units(&manifest, &installed, &companions)?;
        let adding = self.adding_units(&manifest, &installed, &companions)?;
        let taken = self.models_of(&manifest, state, &installed)?;
        let future_units: BTreeMap<String, String> = carried
            .iter()
            .filter(|(name, _)| {
                installed.iter().any(|part| {
                    &part.unit == *name
                        && manifest
                            .files
                            .iter()
                            .any(|file| file.name == part.program.binary)
                })
            })
            .map(|(name, text)| (name.clone(), text.clone()))
            .collect();
        self.settings_of(&manifest, state, envelope, &future_units)?;
        // A part that reads a model or a setting the switch changes is
        // restarted with it. Not for the reset plan: the reset reads it
        // again each time the button is held (#806).
        let touched: Vec<String> = self
            .placings
            .iter()
            .map(|placing| placing.model.path.clone())
            .chain(
                self.setting_writes
                    .iter()
                    .filter(|path| *path != manifest::RESET_PLAN)
                    .cloned(),
            )
            .collect();
        // And a part an image file is read by without its unit naming it:
        // the voice, by /etc/asound.conf. It is switched, as every switch
        // of the voice, only at a quiet moment.
        let forced: Vec<String> = self
            .image_writes
            .iter()
            .filter_map(|path| image::restarts(path))
            .map(str::to_owned)
            .collect();
        let (changes, rollback_order) = plan(
            self.config,
            &manifest,
            &installed,
            &companions,
            &carried,
            &adding,
            &taken,
            &touched,
            &forced,
        )
        .map_err(Outcome::Refused)?;
        if manifest.settings.is_empty() {
            crate::head_touch::verify(
                &self.config.root,
                &self.config.on(&self.config.units),
                &future_units,
                &[],
            )
            .map_err(Outcome::Refused)?;
        }
        if self
            .setting_writes
            .iter()
            .any(|path| path == crate::head_touch::PINS)
            && crate::head_touch::active_pins(
                &self.config.root,
                &self.config.on(&self.config.units),
                &future_units,
            )
            .map_err(Outcome::Refused)?
            .as_deref()
                == Some(crate::head_touch::PINS)
            && !changes.iter().any(|change| {
                change.unit == "lamp-controls.service"
                    && models::reads(&change.after, crate::head_touch::PINS)
            })
        {
            return Err(Outcome::Refused(
                "head pins need their Controls reader included in the switch".into(),
            ));
        }
        self.rollback_order = Some(rollback_order);
        self.fetching = to_fetch(self.config, &changes, &self.placings);
        self.room(&changes)?;
        let placing = self.product_skills(state, &manifest, &installed, &carried)?;
        Ok((manifest, changes, placing))
    }

    /// The build's product skills (#2014's P2), fetched and checked, and
    /// what each file needs here: for a lamp whose lamp-skills reads
    /// install's folder, from a build that carries them. None otherwise:
    /// a lamp without lamp-skills, one whose skills folder is its own
    /// (lamp-4ace's), or a build from before them.
    fn product_skills(
        &mut self,
        state: &State,
        manifest: &Manifest,
        installed: &[Installed],
        carried: &BTreeMap<String, String>,
    ) -> Result<Option<Placing>, Outcome> {
        let Some(part) = installed
            .iter()
            .find(|part| part.program.binary == SKILLS_BINARY)
        else {
            return Ok(None);
        };
        let unit = carried.get(&part.unit).unwrap_or(&part.text);
        if skills_dir(unit) != product_skills::FOLDER {
            return Ok(None);
        }
        let Some(file) = manifest
            .files
            .iter()
            .find(|file| file.name == product_skills::BUNDLE)
        else {
            return Ok(None);
        };
        if file.size > product_skills::MOST_BYTES {
            return Err(Outcome::Refused(format!(
                "{} is {} bytes, and at most {}",
                file.name,
                file.size,
                product_skills::MOST_BYTES
            )));
        }
        let url = format!("{}{}", manifest.files_url, file.name);
        let mut bytes = Vec::new();
        match self.world.channel.fetch(&url, file.size, &mut bytes) {
            Ok(_) => {}
            Err(Fetch::Unreachable(why)) => return Err(Outcome::Unreachable(why)),
            Err(error) => return Err(Outcome::Refused(format!("{}: {error}", file.name))),
        }
        if bytes.len() as u64 != file.size || manifest::sha256_hex(&bytes) != file.sha256 {
            return Err(Outcome::Refused(format!(
                "{} does not match its SHA-256",
                file.name
            )));
        }
        let mut bundle = product_skills::parse(&bytes).map_err(Outcome::Refused)?;
        // A core skill turned on on this lamp stays on (the first builder
        // on #2585, 22:24), kept in its record only by a round that writes.
        let paths: Vec<String> = bundle
            .files
            .iter()
            .map(|file| format!("{}/{}", product_skills::FOLDER, file.path))
            .collect();
        product_skills::keep_turned_on(
            &self.config.root,
            paths
                .iter()
                .map(String::as_str)
                .zip(bundle.files.iter_mut().map(|file| &mut file.text)),
            &state.settings,
            &state.settings_writing,
            self.config.apply,
            self.world.say,
        )
        .map_err(|error| Outcome::Failed(format!("the product skills cannot be read: {error}")))?;
        let steps = product_skills::plan(
            &self.config.root,
            &bundle,
            &state.settings,
            &state.settings_writing,
        )
        .map_err(|error| Outcome::Failed(format!("the product skills cannot be read: {error}")))?;
        Ok(Some(Placing { bundle, steps }))
    }

    /// The models of the build this round takes, and those at paths of
    /// their own it switches ([`Run::placings`]). With the switch off, none;
    /// and a build whose models are not the lamp's is refused, since its
    /// parts may need them.
    fn models_of(
        &mut self,
        manifest: &Manifest,
        state: &State,
        installed: &[Installed],
    ) -> Result<Vec<Model>, Outcome> {
        self.placings.clear();
        self.fetching.clear();
        if manifest.models.is_empty() {
            return Ok(Vec::new());
        }
        let builds: Vec<&str> = installed
            .iter()
            .map(|part| part.program.builds.as_str())
            .collect();
        if !self.config.models {
            // What a lamp's own file is follows the rule of the switch on:
            // kept, and said. Any other model that differs waits for it.
            let mut differs = None;
            for model in &manifest.models {
                let at = if models::in_a_part(&model.path) {
                    builds.iter().find_map(|builds| {
                        let name = models::name_in(model, builds)?;
                        let part = installed
                            .iter()
                            .find(|part| part.program.builds == *builds)?;
                        Some(self.config.on(&part.program.folder()).join(name))
                    })
                } else {
                    match models::need(self.config, model, state.settings.get(&model.path)) {
                        Need::LampsOwn => {
                            self.say(&format!("kept {}: it is not the pinned model", model.path));
                            None
                        }
                        // Not there at all: ignored, as before the field.
                        Need::Switch(placing) if placing.back.sha256.is_none() => None,
                        Need::There | Need::Switch(_) => Some(self.config.on(&model.path)),
                    }
                };
                if differs.is_none()
                    && at
                        .and_then(|at| file_sha256(&at).ok())
                        .is_some_and(|sha| sha != model.sha256)
                {
                    differs = Some(model);
                }
            }
            if let Some(model) = differs {
                return Err(Outcome::WaitsForModels(format!(
                    "build {} carries another {} than this lamp's, and {}=on is not set",
                    manifest.sequence,
                    model.path,
                    models::SWITCH
                )));
            }
            return Ok(Vec::new());
        }
        let mut taken = Vec::new();
        for model in &manifest.models {
            if models::in_a_part(&model.path) {
                taken.push(model.clone());
                continue;
            }
            match models::need(self.config, model, state.settings.get(&model.path)) {
                Need::There => {}
                Need::Switch(placing) => self.placings.push(placing),
                Need::LampsOwn => {
                    self.say(&format!("kept {}: it is not the pinned model", model.path));
                }
            }
        }
        Ok(taken)
    }

    /// Product auto-apply is scoped to the managed updater's current link;
    /// it neither rewrites its unit nor changes the fixed fallback contract.
    fn product_settings_of(&self, manifest: &Manifest) -> bool {
        if !self.config.product_settings
            || !self.config.self_switch
            || manifest.min_updater.unwrap_or(1) < manifest::PRODUCT_UNITS_UPDATER
            || !manifest
                .units
                .iter()
                .any(|unit| unit.lamp == manifest::PRODUCT_LAMP)
        {
            return false;
        }
        self.managed_updater()
    }

    /// This lamp-update's own unit is a regular file that starts it through
    /// `update/current`: an updater install manages.
    fn managed_updater(&self) -> bool {
        let path = self.config.on(&self.config.units).join(OWN_UNIT);
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
            return false;
        }
        fs::read_to_string(path)
            .ok()
            .and_then(|text| unit::linked_program(&text, &self.config.root))
            .is_some_and(|program| program.builds == own::FOLDER && program.binary == "lamp-update")
    }

    /// The product's units this lamp lacks, which the build asks a round to
    /// add (crate::added): those `install` puts on a lamp, fetched and
    /// checked. Only on a lamp installed as the product, every part of it
    /// started through its `current` link by a managed updater, and only
    /// from a build of [`manifest::ADDS_UPDATER`]: a lamp of its own units
    /// (lamp-4ace's) is never given the product's. One that runs as a user
    /// or a group the lamp lacks is not added, and that is said.
    fn adding_units(
        &mut self,
        manifest: &Manifest,
        installed: &[Installed],
        companions: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, Outcome> {
        let mut adding = BTreeMap::new();
        if manifest.min_updater.unwrap_or(1) < manifest::ADDS_UPDATER
            || installed.is_empty()
            || !installed.iter().all(|part| part.program.linked)
            || !self.managed_updater()
        {
            return Ok(adding);
        }
        for unit in &manifest.units {
            let name = unit.on_lamp();
            if unit.lamp != manifest::PRODUCT_LAMP
                || name == OWN_UNIT
                || !crate::install::PRODUCT
                    .iter()
                    .any(|(product, _)| *product == name)
                || installed.iter().any(|part| part.unit == name)
                || companions.contains_key(&name)
            {
                continue;
            }
            // Not there at all: a link (a masked unit) or anything else
            // there is the lamp's, and left.
            match fs::symlink_metadata(self.config.on(&self.config.units).join(&name)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => continue,
            }
            let text = self.fetch_unit(manifest, unit)?;
            let missing = added::missing(&self.config.root, &text);
            if !missing.is_empty() {
                self.say(&format!(
                    "{name} is not added: it runs as {}, which this lamp lacks; lamp-update install makes them",
                    missing.join(", ")
                ));
                continue;
            }
            adding.insert(name, text);
        }
        Ok(adding)
    }

    /// The settings the build writes on this lamp ([`Run::setting_writes`]):
    /// each still the build's before, or never there. One the lamp changed
    /// is kept, and said. With the switch off none is written, and that is
    /// said: a round writes no setting, as before the field.
    fn settings_of(
        &mut self,
        manifest: &Manifest,
        state: &State,
        envelope: Vec<u8>,
        future_units: &BTreeMap<String, String>,
    ) -> Result<(), Outcome> {
        self.setting_writes.clear();
        self.image_writes.clear();
        let mut writes = Vec::new();
        let settings = self.config.settings || self.product_settings_of(manifest);
        if !manifest.settings.is_empty() || manifest.reset_plan.is_some() {
            let mut steps = apply::plan(
                &self.config.root,
                &manifest.settings,
                &state.settings,
                &state.settings_writing,
                &BTreeMap::new(),
            )
            .map_err(|error| Outcome::Failed(format!("the settings cannot be read: {error}")))?;
            if settings {
                crate::head_touch::project(
                    &self.config.root,
                    &self.config.on(&self.config.units),
                    future_units,
                    &manifest.settings,
                    &mut steps,
                )
                .map_err(Outcome::Refused)?;
            }
            crate::head_touch::verify(
                &self.config.root,
                &self.config.on(&self.config.units),
                future_units,
                if settings { &steps } else { &[] },
            )
            .map_err(Outcome::Refused)?;
            // Setup's reset plan, over any plan the lamp has (#806).
            if let Some(plan) = &manifest.reset_plan {
                steps.extend(
                    apply::reset_plan_step(&self.config.root, plan, &state.settings).map_err(
                        |error| Outcome::Failed(format!("the reset plan cannot be read: {error}")),
                    )?,
                );
            }
            let these = apply::writes(&steps);
            if !settings {
                if !these.is_empty() {
                    self.say(&format!(
                        "build {}'s settings are not written ({}): {}=on is not set",
                        manifest.sequence,
                        these.join(", "),
                        apply::SWITCH
                    ));
                }
            } else {
                self.say_kept(&steps);
                writes.extend(these);
            }
        }
        // The image's files only with the switch on: off, neither the
        // hardware profile nor any of them is read. On, one that can't be
        // read is said, and the rest of the build goes on without them.
        if !manifest.image.is_empty() && !self.config.image {
            self.say(&format!(
                "build {}'s image files are not written: {}=on is not set",
                manifest.sequence,
                image::SWITCH
            ));
        } else if !manifest.image.is_empty() {
            let steps = match image::variant(&self.config.root) {
                Ok(Some(variant)) => {
                    let (files, shipped) = image::for_variant(&manifest.image, &variant);
                    apply::plan(
                        &self.config.root,
                        &files,
                        &state.settings,
                        &BTreeMap::new(),
                        &shipped,
                    )
                    .unwrap_or_else(|error| {
                        self.say(&format!(
                            "the image's files are not written: they cannot be read: {error}"
                        ));
                        Vec::new()
                    })
                }
                Ok(None) => {
                    self.say(&format!(
                        "the image's files are not written: {} names no variant",
                        image::PROFILE
                    ));
                    Vec::new()
                }
                Err(error) => {
                    self.say(&format!(
                        "the image's files are not written: {} cannot be read: {error}",
                        image::PROFILE
                    ));
                    Vec::new()
                }
            };
            let these = apply::writes(&steps);
            self.say_kept(&steps);
            self.image_writes = these.clone();
            writes.extend(these);
        }
        if !writes.is_empty() {
            self.setting_writes = writes;
            self.envelope = envelope;
        }
        Ok(())
    }

    fn say_kept(&mut self, steps: &[apply::Step]) {
        for step in steps {
            if let apply::Act::Keep(why) = &step.act {
                self.say(&format!(
                    "kept {}: {why}; the build's is not written",
                    step.setting.path
                ));
            }
        }
    }

    /// The image's files `lamp-update-apply` was asked for and did not
    /// record: a `lamp-update-back` older than them ignores them. Said, and
    /// the rest of the build goes on. Those paths, returned.
    fn image_not_written(
        &mut self,
        manifest_image: &[manifest::ImageFile],
        state: &State,
    ) -> Vec<String> {
        let missing: Vec<String> = self
            .image_writes
            .iter()
            .filter(|path| {
                let new = manifest_image
                    .iter()
                    .filter(|file| &&file.path == path)
                    .map(|file| &file.sha256)
                    .collect::<Vec<_>>();
                !state
                    .settings
                    .get(path.as_str())
                    .is_some_and(|sha| new.contains(&sha))
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.say(&format!(
                "the image's files were not written ({}): this lamp's lamp-update-back is older than them, so install again",
                missing.join(", ")
            ));
        }
        missing
    }

    /// Said when the build's reset plan was to be written and was not: a
    /// lamp-update-back from before the field passes it over (#806). The
    /// rest of the build goes on.
    fn reset_plan_not_written(&mut self, state: &State) {
        let plan = self
            .seen
            .as_ref()
            .and_then(|seen| seen.reset_plan.as_ref())
            .and_then(|plan| plan.sha256.clone());
        if self
            .setting_writes
            .iter()
            .any(|path| path == manifest::RESET_PLAN)
            && state.settings.get(manifest::RESET_PLAN) != plan.as_ref()
        {
            self.say(&format!(
                "{} was not written: this lamp's lamp-update-back is older than it, so install again",
                manifest::RESET_PLAN
            ));
        }
    }

    /// The units of `changes` restarted only for an image file that was
    /// not written (`missing`): their unit, folder and link the same, and
    /// no written model, setting or image file they read. Not restarted.
    fn only_for(&self, changes: &[Change], missing: &[String]) -> Vec<String> {
        let written: Vec<&String> = self
            .placings
            .iter()
            .map(|placing| &placing.model.path)
            .chain(self.setting_writes.iter())
            .filter(|path| !missing.contains(path))
            .collect();
        changes
            .iter()
            .filter(|change| {
                change.before == change.after
                    && change.old_folder == change.new_folder
                    && change.make.is_none()
                    && change.link.as_ref().is_none_or(|link| link.old == link.new)
                    && missing
                        .iter()
                        .any(|path| image::restarts(path) == Some(change.unit.as_str()))
                    && !written.iter().any(|path| {
                        models::reads(&change.before, path)
                            || image::restarts(path) == Some(change.unit.as_str())
                    })
            })
            .map(|change| change.unit.clone())
            .collect()
    }

    /// Asks `lamp-update-apply` to apply the build's settings, or put them
    /// back, and takes what it did into the state.
    fn ask_apply(&mut self, op: apply::Op, sequence: u64) -> Result<(), String> {
        let records = self.records.folder().to_owned();
        let envelope = (op == apply::Op::Apply).then_some(self.envelope.as_slice());
        let image = op == apply::Op::Apply && !self.image_writes.is_empty();
        apply::stage(&records, op, sequence, envelope, image)
            .map_err(|error| format!("the settings cannot be staged: {error}"))?;
        self.world
            .systemd
            .restart(apply::UNIT)
            .map_err(|why| format!("{}: {why}", apply::UNIT))?;
        let mut state = self
            .records
            .state(self.world.say)
            .map_err(|error| format!("the state cannot be read: {error}"))?;
        let done = apply::merge_for(&records, &mut state, op, sequence)
            .map_err(|error| format!("what {} did cannot be read: {error}", apply::UNIT))?;
        if !done {
            return Err(format!("{} did nothing (is it installed?)", apply::UNIT));
        }
        self.records
            .save_state(&state)
            .and_then(|()| apply::finish(&records))
            .map_err(|error| format!("the state cannot be written: {error}"))
    }

    /// The settings the state now records, as `lamp-update-apply` left them:
    /// taken into the round's own copy of the state.
    fn settings_now(&mut self, state: &mut State) {
        let now = self.records.peek_state();
        state.settings = now.settings;
    }

    /// The build's units for this lamp's parts, fetched and checked. Small:
    /// read even when nothing is applied, to show what would change.
    fn carried_units(
        &mut self,
        manifest: &Manifest,
        installed: &[Installed],
        companions: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, Outcome> {
        let mut carried = BTreeMap::new();
        // Read host entries first so compatibility, not manifest order, decides
        // whether a linked product template is still needed.
        for unit in manifest
            .units
            .iter()
            .filter(|unit| unit.lamp != manifest::PRODUCT_LAMP)
            .chain(
                manifest
                    .units
                    .iter()
                    .filter(|unit| unit.lamp == manifest::PRODUCT_LAMP),
            )
        {
            // A template is named without its `@` in a manifest.
            let name = unit.on_lamp();
            if companion::kind(&name).is_some() {
                // Only the product's (crate::manifest refuses any other's
                // too), and only where the lamp has it, belonging to a part
                // it has.
                if unit.lamp != manifest::PRODUCT_LAMP {
                    continue;
                }
                let Some(ours) = owner(installed, companions, &name) else {
                    continue;
                };
                let text = self.fetch_unit(manifest, unit)?;
                let mut built = companions.clone();
                built.insert(name.clone(), text.clone());
                let before = &companions[&name];
                if companion::starts(&name, &text) != companion::starts(&name, before)
                    || owner(installed, &built, &name).map(|part| &part.unit) != Some(&ours.unit)
                {
                    return Err(Outcome::Refused(format!(
                        "the product's {} does not start {} through {}current as the lamp's does",
                        name, ours.program.binary, ours.program.builds
                    )));
                }
                carried.insert(name, text);
                continue;
            }
            let Some(part) = installed.iter().find(|part| part.unit == unit.unit) else {
                continue;
            };
            let product = unit.lamp == manifest::PRODUCT_LAMP;
            if product {
                // A carried host template wins; plan still validates it.
                // Pinned installations never acquire a product current link.
                if !part.program.linked || carried.contains_key(&unit.unit) {
                    continue;
                }
            } else if unit.lamp != self.config.lamp {
                continue;
            }
            let text = self.fetch_unit(manifest, unit)?;
            if !product
                && part.program.linked
                && unit::program(&text).is_some()
                && manifest
                    .units
                    .iter()
                    .any(|other| other.lamp == manifest::PRODUCT_LAMP && other.unit == unit.unit)
            {
                self.say(&format!(
                    "skipping pinned {} for {}: the installed part uses {}current",
                    unit.unit, unit.lamp, part.program.builds
                ));
                continue;
            }
            if product
                && unit::linked(&text)
                    != Some((part.program.builds.clone(), part.program.binary.clone()))
            {
                return Err(Outcome::Refused(format!(
                    "the product's {} does not start {} through {}current",
                    unit.unit, part.program.binary, part.program.builds
                )));
            }
            carried.insert(unit.unit.clone(), text);
        }
        Ok(carried)
    }

    /// One unit of the build, fetched and checked against its SHA-256.
    fn fetch_unit(
        &mut self,
        manifest: &Manifest,
        unit: &manifest::Unit,
    ) -> Result<String, Outcome> {
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
        String::from_utf8(bytes)
            .map_err(|_| Outcome::Refused(format!("{} is not text", unit.file_name())))
    }

    /// Step 5: room for a second copy of everything that changed, before
    /// anything is written. Counted whole, as if no file could be linked.
    /// The store's new entries are counted too, before any is fetched.
    fn room(&self, changes: &[Change]) -> Result<(), Outcome> {
        let stored: u64 = self.fetching.iter().map(Model::fetch_bytes).sum();
        let needed: u64 = changes
            .iter()
            .flat_map(|change| change.make.iter().flatten())
            .map(|want| want.size)
            .sum::<u64>()
            + stored;
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
                "the disk has {} MB free; the new build needs {} MB beside the old one{}, and {} MB are kept free",
                free / 1_000_000,
                needed.div_ceil(1_000_000),
                if stored > 0 {
                    format!(" ({} MB of it models)", stored.div_ceil(1_000_000))
                } else {
                    String::new()
                },
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
        self.store()?;
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

    /// Every model the build needs and the store lacks, fetched and
    /// checked. All or none: one that fails leaves none of this round's.
    fn store(&mut self) -> Result<(), Outcome> {
        if self.fetching.is_empty() {
            return Ok(());
        }
        models::make_store(self.config)
            .map_err(|error| Outcome::Failed(format!("{}: {error}", manifest::STORE)))?;
        let mut fetched = Vec::new();
        for model in self.fetching.clone() {
            let at = self.config.on(&models::store_path(&model.sha256));
            match crate::install::fetch_model(self.world.channel, &model, &at) {
                Ok(()) => {
                    self.say(&format!(
                        "fetched {} into {}",
                        model.path,
                        models::store_path(&model.sha256)
                    ));
                    fetched.push(at);
                }
                Err(why) => {
                    for at in &fetched {
                        let _ = fs::remove_file(at);
                    }
                    // Made for this round and empty: gone too.
                    let _ = fs::remove_dir(self.config.on(manifest::STORE));
                    return Err(match why {
                        Fetch::Unreachable(why) => Outcome::Unreachable(why),
                        why => Outcome::Refused(format!("{}: {why}", model.path)),
                    });
                }
            }
        }
        Ok(())
    }

    /// One new folder, under its temporary name.
    fn fill(&mut self, manifest: &Manifest, folder: &Path, wants: &[Want]) -> Result<(), Outcome> {
        let failed = |error: io::Error| Outcome::Failed(format!("{}: {error}", folder.display()));
        if folder.exists() {
            // Left by a run that stopped: this program's, by its name.
            fs::remove_dir_all(folder).map_err(failed)?;
        }
        // A part new to the lamp has no folder of its builds yet.
        if let Some(builds) = folder.parent() {
            fs::DirBuilder::new()
                .mode(0o755)
                .recursive(true)
                .create(builds)
                .map_err(failed)?;
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
                Source::Model { model, from } => {
                    let from = from
                        .clone()
                        .unwrap_or_else(|| self.config.on(&models::store_path(&model.sha256)));
                    if fs::hard_link(&from, &path).is_err() {
                        fs::copy(&from, &path).map_err(failed)?;
                        fs::File::open(&path)
                            .and_then(|file| file.sync_all())
                            .map_err(failed)?;
                    }
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
    /// build's lamp-skills (the first builder on #1722), the folder judged
    /// as the round leaves it: a copy of it with the build's product skills
    /// placed, staged in the records (#2014's P2). A lamp-skills with
    /// checks its folders fail (#1698's `[permissions]`, on a lamp's own
    /// folder from before them) would run with every such skill refused, up
    /// and well. The build's own `lamp-skills list` on the staged folder
    /// and the running one's on the lamp's are read: a skill that loads now
    /// and would not after refuses the build, naming each, with nothing
    /// switched. One both refuse holds nothing back. Run when lamp-skills
    /// changes folder, or when product skills are written.
    fn skills_still_load(
        &mut self,
        manifest: &Manifest,
        changes: &[Change],
        placing: Option<&Placing>,
    ) -> Result<(), Outcome> {
        let placing = placing.filter(|placing| placing.writes());
        let (old_folder, new_folder, unit) = match changes
            .iter()
            .find(|change| change.part == SKILLS_PART && change.new_folder != change.old_folder)
        {
            Some(change) => (
                change.old_folder.clone(),
                change.new_folder.clone(),
                change.after.clone(),
            ),
            None if placing.is_some() => {
                let installed = installed_units(self.config).map_err(|error| {
                    Outcome::Failed(format!("the units cannot be read: {error}"))
                })?;
                let Some(part) = installed
                    .into_iter()
                    .find(|part| part.program.binary == SKILLS_BINARY)
                else {
                    return Ok(());
                };
                (part.program.folder(), part.program.folder(), part.text)
            }
            None => return Ok(()),
        };
        let dir = skills_dir(&unit);
        let folder = self.config.on(&dir);
        let staged = self.records.folder().join(STAGED);
        let judged = match placing {
            Some(placing) => {
                product_skills::stage(&self.config.root, &dir, &placing.steps, &staged).map_err(
                    |error| {
                        Outcome::Failed(format!("the product skills cannot be staged: {error}"))
                    },
                )?;
                staged.clone()
            }
            None if folder.is_dir() => folder.clone(),
            None => return Ok(()),
        };
        let judged_result =
            self.skills_judged(manifest, &dir, &folder, &judged, &old_folder, &new_folder);
        if placing.is_some() {
            let _ = fs::remove_dir_all(&staged);
        }
        judged_result
    }

    /// The build's lamp-skills on `judged` against the running one's on
    /// `folder` ([`Run::skills_still_load`]).
    fn skills_judged(
        &self,
        manifest: &Manifest,
        dir: &str,
        folder: &Path,
        judged: &Path,
        old_folder: &str,
        new_folder: &str,
    ) -> Result<(), Outcome> {
        let binary = |build: &str| self.config.on(build).join(SKILLS_BINARY);
        let after = match list_skills(&binary(new_folder), judged) {
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
        let before = list_skills(&binary(old_folder), folder)
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
             nothing is switched until they load under it (a round places the build's product \
             skills, but never over a file of the lamp's own)",
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
    /// for: that is for putting back a build, not for switching to one. With
    /// `walk`, a setup walk under way at any look ends it as
    /// [`crate::walk::UNDER_WAY`], quiet or not: the switch's own wait.
    fn wait_quiet(
        &mut self,
        long: Duration,
        within: Duration,
        unseen_is_quiet: bool,
        walk: bool,
    ) -> Result<(), String> {
        let until = self.world.clock.now_ms() + within.as_millis() as u64;
        let mut told = false;
        // Whether the bus has been listened to in this wait. Before the
        // first listen a round's gate has heard nothing, so its reason is
        // always that the voice says nothing: that line is said only after
        // a look, with the real reason (lamp-3e83 on 2026-10-28 logged
        // "says nothing" for 22 minutes of someone talking to the lamp).
        let mut looked = false;
        loop {
            if !self.voice_running() {
                return Ok(());
            }
            if walk && crate::walk::under_way(self.config) {
                return Err(crate::walk::UNDER_WAY.to_owned());
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
            if looked && !told {
                self.say(&format!("waiting for a quiet moment: {why}"));
                told = true;
            }
            self.listen(LOOK_EVERY);
            looked = true;
        }
    }

    /// Writes each unit's text, and reloads.
    /// `install::SECOND` at `install::SECOND_PATH`, as `install` writes its
    /// private settings (`install::settings_with_mode`, 0600, the same
    /// records): written when missing or still install's own and not the
    /// build's, and a lamp's own kept. Only when the build's voice unit
    /// names that file.
    fn place_second(&mut self, state: &mut State, changes: &[Change]) -> Result<(), String> {
        let named = changes.iter().any(|change| {
            change.unit == VOICE_UNIT
                && unit::environment(&change.after, crate::install::SECOND_CONFIG_ENV).as_deref()
                    == Some(crate::install::SECOND_PATH)
        });
        if !named {
            return Ok(());
        }
        let install = crate::install::Install {
            root: self.config.root.clone(),
            channel: self.config.channel.clone(),
            channel_url: self.config.channel_url.clone(),
            key: self.config.key,
            records: self.config.records.clone(),
            setup: false,
            models: Vec::new(),
        };
        let mut changed = Vec::new();
        let mut said = Vec::new();
        let records = &self.records;
        crate::install::settings_with_mode(
            &install,
            &[(crate::install::SECOND_PATH, crate::install::SECOND)],
            0o600,
            state,
            &mut changed,
            &mut |line| said.push(line.to_owned()),
            &mut |state| {
                records
                    .save_state(state)
                    .map_err(|error| format!("the records cannot be written: {error}"))
            },
        )
        .map_err(|why| {
            format!(
                "{} cannot be placed, so no unit is written: {why}",
                crate::install::SECOND_PATH
            )
        })?;
        for line in changed.iter().chain(&said) {
            self.say(line);
        }
        Ok(())
    }

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
    /// named, then every model path in reverse. A linked unit the switch did
    /// not change is written again with the same bytes. One that fails does
    /// not stop the others: each is tried, and what failed is said.
    fn put_units_and_links_back(&mut self, backs: &[Back]) -> Result<(), String> {
        let mut troubles = Vec::new();
        // An added unit is stopped and removed before the reload, so systemd
        // forgets it with the rest put back.
        self.remove_added(backs, &mut troubles);
        if let Err(why) = self.write_units(
            backs
                .iter()
                .filter(|back| back.added.is_none())
                .map(|back| (back.unit.as_str(), back.text.as_str())),
        ) {
            troubles.push(why);
        }
        if let Err(why) = self.set_links(backs.iter().filter_map(|back| {
            let link = back.link.as_ref()?;
            Some((link.path.as_str(), link.build.as_str()))
        })) {
            troubles.push(why);
        }
        for back in self.model_backs.clone().iter().rev() {
            if let Err(error) = models::put_back(self.config, back) {
                troubles.push(format!("{} cannot be put back: {error}", back.path));
            }
        }
        if let Some(sequence) = self.applying {
            let records = self.records.folder().to_owned();
            if apply::wrote(&records, sequence) {
                if let Err(why) = self.ask_apply(apply::Op::Back, sequence) {
                    troubles.push(format!("the settings cannot be put back: {why}"));
                }
            } else {
                // No back.json of this build: lamp-update-apply wrote
                // nothing (it never ran, or failed before its first write),
                // so there is nothing to put back, and nothing to ask of a
                // unit that may not run at all.
                self.say(&format!(
                    "the settings need no putting back: {} wrote nothing for build {sequence}",
                    apply::UNIT
                ));
                if let Err(error) = apply::finish(&records) {
                    troubles.push(format!("the settings' request cannot be removed: {error}"));
                }
            }
            // Undone above by the writer that made the apply, if it wrote
            // at all; only then is a new writer whose first apply failed
            // put back itself, whether or not it wrote before failing.
            if self.apply_failed && crate::writer_swap::unproven(&self.config.root, &records) {
                self.writer_op(crate::writer_swap::Op::PutBack, "its first apply failed");
            }
        }
        if troubles.is_empty() {
            Ok(())
        } else {
            Err(troubles.join("; "))
        }
    }

    /// Each model at a path of its own pointed at its new model.
    fn switch_models(&mut self) -> Result<(), String> {
        for placing in self.placings.clone() {
            models::switch(self.config, &placing).map_err(|error| {
                format!(
                    "{} cannot be pointed at its model: {error}",
                    placing.model.path
                )
            })?;
        }
        Ok(())
    }

    fn write_changes(&mut self, changes: &[Change]) -> Result<(), String> {
        // An added unit's enabling links, before the reload reads them.
        self.enable_added(changes)
            .and_then(|()| {
                self.write_units(
                    changes
                        .iter()
                        .filter(|change| change.link.is_none() || change.after != change.before)
                        .map(|change| (change.unit.as_str(), change.after.as_str())),
                )
            })
            .and_then(|()| {
                self.set_links(changes.iter().filter_map(|change| {
                    let link = change.link.as_ref()?;
                    Some((link.path.as_str(), link.new.as_str()))
                }))
            })
            .and_then(|()| {
                // A part new to the lamp: its `current` link, made.
                self.set_links(changes.iter().filter_map(|change| {
                    let (path, build) = change.added.as_ref()?.link.as_ref()?;
                    Some((path.as_str(), build.as_str()))
                }))
            })
            .and_then(|()| self.switch_models())
    }

    /// Each added unit's links, as `systemctl enable` writes them: to the
    /// unit's own file, in a folder made if it is not there.
    fn enable_added(&mut self, changes: &[Change]) -> Result<(), String> {
        for change in changes {
            for link in change.added.iter().flat_map(|adding| &adding.wants) {
                let path = self.config.on(link);
                let to = Path::new(added::UNITS).join(&change.unit);
                let made = path
                    .parent()
                    .map_or(Ok(()), fs::create_dir_all)
                    .and_then(|()| match fs::read_link(&path) {
                        Ok(now) if now == to => Ok(()),
                        _ => replace_link(&path, &to.to_string_lossy()),
                    });
                made.map_err(|error| format!("{link} cannot be made: {error}"))?;
            }
        }
        Ok(())
    }

    /// Each added unit stopped, then its links, its file and the `current`
    /// link it made removed. One that fails does not stop the others.
    fn remove_added(&mut self, backs: &[Back], troubles: &mut Vec<String>) {
        for back in backs.iter().rev() {
            let Some(added) = &back.added else {
                continue;
            };
            if let Err(why) = self.world.systemd.stop(&back.unit) {
                troubles.push(format!("{} cannot be stopped: {why}", back.unit));
            }
            let unit = format!("{}/{}", added::UNITS, back.unit);
            for path in added.wants.iter().chain([&unit]).chain(added.link.as_ref()) {
                match fs::remove_file(self.config.on(path)) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => {
                        troubles.push(format!("{path} cannot be removed: {error}"));
                    }
                    _ => {}
                }
            }
            // A `.wants` folder left empty goes too, as `systemctl disable`
            // leaves none behind; one with other links stays.
            for link in &added.wants {
                if let Some(folder) = self.config.on(link).parent() {
                    let _ = fs::remove_dir(folder);
                }
            }
        }
    }

    /// Steps 7 and 8.
    fn switch(
        &mut self,
        state: &mut State,
        build: Build,
        changes: Vec<Change>,
        placing: Option<Placing>,
    ) -> Outcome {
        // A walk begun while this round fetched or waited holds it too, at
        // any look of the wait, before anything is written (crate::walk).
        // A count that can't be kept holds nothing: the wait goes on as
        // before, with the walk no longer looked at.
        let walk = !crate::walk::spent(self.records.folder());
        let waited = match self.wait_quiet(QUIET, self.config.quiet_wait, false, walk) {
            Err(why) if walk && why == crate::walk::UNDER_WAY => {
                if let Some(held) = crate::walk::hold(self.records.folder()) {
                    return Outcome::NotQuiet(crate::walk::held(held));
                }
                self.wait_quiet(QUIET, self.config.quiet_wait, false, false)
            }
            waited => waited,
        };
        if let Err(why) = waited {
            return Outcome::NotQuiet(why);
        }
        // The product skills' files as they are now, and as they were: kept
        // in the journal before anything is written.
        let steps = match placing
            .as_ref()
            .map(|placing| self.plan_skills(state, placing))
        {
            Some(Ok(steps)) => steps,
            Some(Err(why)) => return Outcome::Failed(why),
            None => Vec::new(),
        };
        let skills = match product_skills::backs(&self.config.root, &steps) {
            Ok(backs) => backs,
            Err(error) => return Outcome::Failed(format!("the product skills: {error}")),
        };
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
                added: change.added.as_ref().map(Adding::added),
            })
            .collect();
        // The second route's services, when the build's voice unit names
        // them, placed as install places them before any unit is written
        // (#1940, the reviewer's P1 and the first builder's 00:11): a voice
        // unit that asks for `second.json` with none there would not start,
        // and the round would put everything back and refuse the build. A
        // round that cannot place it writes nothing.
        if let Err(why) = self.place_second(state, &changes) {
            return Outcome::Failed(why);
        }
        // The units as they were, and each moved link with the build it
        // named: the way back by hand (docs/updates.md).
        // An added unit was not there: it has no text to go back to.
        let mut previous: Vec<(String, String)> = backs
            .iter()
            .filter(|back| back.added.is_none())
            .map(|back| (back.unit.clone(), back.text.clone()))
            .collect();
        previous.extend(backs.iter().filter_map(|back| {
            let link = back.link.as_ref()?;
            Some((
                format!("{}{}", back.unit, records::LINK_SUFFIX),
                format!("{} {}\n", link.path, link.build),
            ))
        }));
        self.model_backs = self
            .placings
            .iter()
            .map(|placing| placing.back.clone())
            .collect();
        let journal = Journal {
            build: build.clone(),
            rollback_order: self.rollback_order.clone(),
            units: backs
                .iter()
                .map(|back| Switching {
                    unit: back.unit.clone(),
                    was_active: back.was_active,
                    link: back.link.clone(),
                    added: back.added.clone(),
                })
                .collect(),
            models: self.model_backs.clone(),
            settings: !self.setting_writes.is_empty(),
            skills: skills.clone(),
        };
        if let Err(error) = self
            .records
            .keep_previous(&previous)
            .and_then(|()| self.records.save_journal(&journal))
        {
            return Outcome::Failed(format!("the journal cannot be written: {error}"));
        }
        // Head pins depend on the candidate units; other settings retain
        // their established apply-before-units ordering.
        let head_pins = changes.iter().any(|change| {
            change.unit == "lamp-controls.service"
                && models::reads(&change.after, crate::head_touch::PINS)
        }) && self
            .setting_writes
            .iter()
            .any(|path| path == crate::head_touch::PINS);
        if head_pins && let Err(why) = self.write_changes(&changes) {
            return self.put_back(state, &build, &backs, None, why, String::new());
        }
        // Read the candidate units now installed on disk, before restarting
        // any reader. A failure restores both units and settings from the journal.
        let mut unrestarted = Vec::new();
        if !self.setting_writes.is_empty() {
            self.applying = Some(build.sequence);
            let applied = self.ask_apply(apply::Op::Apply, build.sequence);
            self.settings_now(state);
            if let Err(why) = applied {
                self.apply_failed = true;
                return self.put_back(state, &build, &backs, None, why, String::new());
            }
            self.reset_plan_not_written(state);
            if !self.image_writes.is_empty() {
                let image = self
                    .seen
                    .as_ref()
                    .map(|seen| seen.image.clone())
                    .unwrap_or_default();
                let missing = self.image_not_written(&image, state);
                unrestarted = self.only_for(&changes, &missing);
            }
        }
        if !head_pins && let Err(why) = self.write_changes(&changes) {
            return self.put_back(state, &build, &backs, None, why, String::new());
        }
        if let Err(why) = crate::head_touch::verify(
            &self.config.root,
            &self.config.on(&self.config.units),
            &BTreeMap::new(),
            &[],
        ) {
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
            self.settings_now(state);
            if let Err(error) = restored.and_then(|()| {
                self.records
                    .clear_journal()
                    .map_err(|error| error.to_string())
            }) {
                return Outcome::Failed(format!("the units could not be put back: {error}"));
            }
            return Outcome::NotQuiet(format!("someone spoke as the switch began: {why}"));
        }
        // The build's product skills, before any part is restarted: the new
        // lamp-skills starts on them, and a put-back restores them.
        if !skills.is_empty() {
            self.skills_back = skills;
            if let Err(why) = product_skills::write(&self.config.root, &steps) {
                return self.put_back(state, &build, &backs, None, why, String::new());
            }
            self.say_put_back(&steps);
            self.say(&format!(
                "placed {} files of the product skills",
                self.skills_back.len()
            ));
        }
        self.say(&format!("switching to build {}", build.sequence));
        // The voice is stopped first when a part it reads restarts before it:
        // a clean stop, with Controls still up, so no build's voice arms its
        // restart guard. Restarted after its prerequisites, it starts the new
        // voice. The old voice's own release can't help a lamp updating from
        // a build without it (the 7071 lamps, #3689; 06c1 on 8474), and a
        // climb makes this updater the one that runs their round.
        let voice_at = backs.iter().position(|back| {
            back.unit == VOICE_UNIT && back.was_active && !unrestarted.contains(&back.unit)
        });
        let voice_stopped = match voice_at {
            Some(voice_at)
                if backs[..voice_at]
                    .iter()
                    .any(|back| back.was_active && !unrestarted.contains(&back.unit)) =>
            {
                self.listen(Duration::ZERO);
                if let Err(why) = self.wait_quiet(QUIET, self.config.quiet_wait, false, false) {
                    return self.put_back(state, &build, &backs, None, why, String::new());
                }
                backs[voice_at].restarted = true;
                if let Err(why) = self.world.systemd.stop(VOICE_UNIT) {
                    let told = self.told(VOICE_UNIT);
                    return self.put_back(state, &build, &backs, Some(VOICE_UNIT), why, told);
                }
                self.say("the voice is stopped before the parts it reads restart");
                true
            }
            _ => false,
        };
        let mut watches = Vec::new();
        for at in 0..backs.len() {
            if !backs[at].was_active || unrestarted.contains(&backs[at].unit) {
                continue;
            }
            let back = &backs[at];
            if changes
                .iter()
                .find(|change| change.unit == back.unit)
                .is_some_and(|change| self.leave_completed_setup(back, &change.after))
            {
                self.say("setup is complete; its phone grace period is not restarted");
                continue;
            }
            if backs[at].unit == VOICE_UNIT && !voice_stopped {
                // A prerequisite's restart can take time. Drain speech
                // queued during it and judge quiet at Voice's own boundary,
                // before marking it restarted or stopping its old process.
                // A voice stopped first was judged before its stop.
                self.listen(Duration::ZERO);
                if let Err(why) = self.wait_quiet(QUIET, self.config.quiet_wait, false, false) {
                    return self.put_back(state, &build, &backs, None, why, String::new());
                }
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
        (self.world.phase)("watching");
        if let Err((unit, why, judged)) = self.watch(&mut watches) {
            let told = if judged {
                self.told(&unit)
            } else {
                String::new()
            };
            return self.put_back(state, &build, &backs, Some(&unit), why, told);
        }
        record_skills(state, &steps);
        state.previous = state.installed.replace(build.clone());
        if !self.placings.is_empty() {
            let mut before = state.models.clone();
            for placing in &self.placings {
                match &placing.back.sha256 {
                    Some(sha) => before.insert(placing.model.path.clone(), sha.clone()),
                    None => before.remove(&placing.model.path),
                };
                state
                    .models
                    .insert(placing.model.path.clone(), placing.model.sha256.clone());
            }
            state.models_before = before;
        }
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
    /// itself still could not be asked at the watch's end.
    fn watch(&mut self, watches: &mut [Watch]) -> Result<(), (String, String, bool)> {
        if watches.is_empty() {
            return Ok(());
        }
        let units: Vec<String> = watches.iter().map(|watch| watch.unit.clone()).collect();
        let until = self.world.clock.now_ms() + self.config.watch.as_millis() as u64;
        loop {
            // A query that fails is not known yet, not a part not well: it
            // is asked again until the watch's end, and only one that still
            // fails then puts the build back.
            let asked = self.world.systemd.states(&units);
            let done = self.world.clock.now_ms() >= until;
            let states = match asked {
                Ok(states) => states,
                Err(_) if !done => {
                    self.listen(LOOK_EVERY);
                    continue;
                }
                Err(why) => {
                    return Err((
                        units[0].clone(),
                        format!("systemd cannot be asked: {why}"),
                        false,
                    ));
                }
            };
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
        let (after, old, files_back) = self.restore(backs, failed);
        self.settings_now(state);
        if !files_back {
            // Not all is as it was: the journal stays, for the next start
            // to try again, and nothing is pruned that a path still names.
            return Outcome::RolledBack {
                sequence: build.sequence,
                why,
                after,
                told: Told { new: told, old },
            };
        }
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
        // A part new to the lamp, taken away: the folder of its builds goes
        // too once pruned empty, as `install` leaves none (jarvis's P3 on
        // #4065). One with anything left in it stays.
        for link in backs
            .iter()
            .filter_map(|back| back.added.as_ref()?.link.as_ref())
        {
            if let Some(builds) = self.config.on(link).parent() {
                let _ = fs::remove_dir(builds);
            }
        }
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
    /// restarted again in the saved old units' startup dependency order.
    /// Voice waits for two turns' gap unless it is what failed. Every unit
    /// is tried, whatever happens to one, and parts are restarted even when
    /// a file could not be put back. An invalid ordering is refused before
    /// any restoration write, retaining the journal and its recovery prefix.
    /// With what the first unit found not well said of itself ([`Told`]),
    /// and whether every file is as it was.
    fn restore(
        &mut self,
        backs: &[Back],
        failed: Option<&str>,
    ) -> (Result<(), String>, String, bool) {
        let again = match self.restore_order(backs) {
            Ok(again) => again,
            Err(why) => return (Err(why), String::new(), false),
        };
        // The product skills' files first, before lamp-skills is started
        // again on the old build.
        let skills = std::mem::take(&mut self.skills_back);
        let skills = product_skills::restore(&self.config.root, &skills, self.world.say);
        let mut troubles = Vec::new();
        let files_back = match self.put_units_and_links_back(backs) {
            Ok(()) => true,
            Err(why) => {
                troubles.push(why);
                false
            }
        };
        troubles.extend(skills.err());
        let mut watches = Vec::new();
        for back in again {
            // Recovery's journal truthfully retains was_active, even when
            // Setup completed while fetching. Recheck its restored product
            // lifecycle here as well as at the forward restart boundary.
            if files_back && self.leave_completed_setup(back, &back.text) {
                self.say("setup is complete; restoring its files needs no restart");
                continue;
            }
            if back.unit == VOICE_UNIT && failed != Some(VOICE_UNIT) {
                // A voice that is well is not cut off mid-sentence.
                self.listen(Duration::ZERO);
                let within = self.config.quiet_wait;
                if let Err(why) = self.wait_quiet(BETWEEN_TURNS, within, true, false) {
                    self.say(&format!("the voice is put back all the same: {why}"));
                }
            }
            match self.world.systemd.restart(&back.unit) {
                Ok(()) => watches.push(Watch::new(&back.unit, &back.part, back.before.clone())),
                Err(why) => troubles.push(why),
            }
        }
        let mut told = String::new();
        (self.world.phase)("restoring");
        if let Err((unit, why, judged)) = self.watch(&mut watches) {
            if judged {
                told = self.told(&unit);
            }
            troubles.push(why);
        }
        if troubles.is_empty() {
            (Ok(()), told, files_back)
        } else {
            (Err(troubles.join("; ")), told, files_back)
        }
    }

    /// Use the order validated and saved before switching. Only older
    /// journals reconstruct the old graph from saved and installed units.
    /// Already-restored and inactive members do not gain a restart merely
    /// by being named in the full saved order.
    fn restore_order<'a>(&self, backs: &'a [Back]) -> Result<Vec<&'a Back>, String> {
        if let Some(order) = &self.rollback_order {
            if backs.iter().any(|back| !order.contains(&back.unit)) {
                return Err("the saved rollback order omits a unit to restore".into());
            }
            return Ok(order
                .iter()
                .filter_map(|name| {
                    backs
                        .iter()
                        .find(|back| back.restarted && back.added.is_none() && back.unit == *name)
                })
                .collect());
        }
        let mut texts: BTreeMap<String, String> = installed_units(self.config)
            .map_err(|error| format!("the restoration's unit order cannot be read: {error}"))?
            .into_iter()
            .map(|part| (part.unit, part.text))
            .collect();
        for back in backs {
            texts.insert(back.unit.clone(), back.text.clone());
        }
        let order = unit::start_order(
            texts
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str())),
        )
        .map_err(|why| format!("the restoration's unit order is refused: {why}"))?;
        Ok(order
            .iter()
            .filter_map(|name| {
                backs
                    .iter()
                    .find(|back| back.restarted && back.unit == *name)
            })
            .collect())
    }

    /// Step 1: a switch the last run did not finish is put back.
    fn recover(&mut self, journal: Journal) -> Outcome {
        // Records validates this against all journal members before any
        // already-restored prefix is excluded below. Legacy journals stay
        // byte-for-byte intact while their graph is reconstructed.
        self.rollback_order = journal.rollback_order.clone();
        let why =
            "the switch was cut short (lamp-update stopped, or the lamp lost power)".to_owned();
        self.say(&format!(
            "putting build {} back: {why}",
            journal.build.sequence
        ));
        self.model_backs = journal.models.clone();
        self.applying = journal.settings.then_some(journal.build.sequence);
        let mut backs = Vec::new();
        for switching in &journal.units {
            // A unit the switch added is put back by removing it, with what
            // it added; one none of which is there was never added.
            if let Some(added) = &switching.added {
                let unit = format!("{}/{}", added::UNITS, switching.unit);
                let there = |path: &String| fs::symlink_metadata(self.config.on(path)).is_ok();
                if !(there(&unit) || added.wants.iter().any(there) || added.link.iter().any(there))
                {
                    continue;
                }
                backs.push(Back {
                    unit: switching.unit.clone(),
                    part: String::new(),
                    text: String::new(),
                    was_active: false,
                    restarted: false,
                    before: Before::default(),
                    link: None,
                    added: Some(added.clone()),
                });
                continue;
            }
            let text = match self.records.previous(&switching.unit) {
                Ok(text) => text,
                Err(error) => {
                    return Outcome::Failed(format!(
                        "{} as it was cannot be read: {error}",
                        switching.unit
                    ));
                }
            };
            // A unit still as it was, with its link as it was and every
            // model it reads as it was, was never switched, nor restarted.
            let now = fs::read_to_string(self.config.on(&self.config.units).join(&switching.unit));
            let link_as_was = switching.link.as_ref().is_none_or(|link| {
                fs::read_link(self.config.on(&link.path))
                    .is_ok_and(|now| now == Path::new(&link.build))
            });
            let models_as_were = journal
                .models
                .iter()
                .filter(|back| models::reads(&text, &back.path))
                .all(|back| models::is_back(self.config, back));
            // Controls may have read the candidate pins after a reboot even
            // when its binary and unit never changed. Reload restored pins.
            let head_pins_to_restore = journal.settings
                && models::reads(&text, crate::head_touch::PINS)
                && apply::wrote(self.records.folder(), journal.build.sequence);
            if now.is_ok_and(|now| now == text)
                && link_as_was
                && models_as_were
                && !head_pins_to_restore
            {
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
                added: None,
            });
        }
        self.skills_back = journal.skills;
        let mut state = match self.records.state(self.world.say) {
            Ok(state) => state,
            Err(error) => return Outcome::Failed(format!("the state cannot be read: {error}")),
        };
        self.put_back(&mut state, &journal.build, &backs, None, why, String::new())
    }

    /// A signed build for a newer updater than this one: refused, with
    /// nothing fetched or written. With `LAMP_UPDATE_SELF=on`, when it
    /// carries a lamp-update of this channel's, newer than the build the
    /// lamp runs and not put back before, that one alone is taken: then
    /// `check` says so and a round with `--apply on` switches to it.
    fn too_old(&mut self, state: &State, too_old: TooOld) -> Outcome {
        let why = too_old.why();
        let Some(carried) = too_old.carried.as_ref() else {
            return Outcome::Refused(why);
        };
        let ours = too_old.channel.as_deref() == Some(self.config.channel.as_str());
        let newer = too_old.sequence.is_some_and(|sequence| {
            state
                .installed
                .as_ref()
                .is_none_or(|installed| sequence > installed.sequence)
        });
        if !self.config.self_switch
            || !ours
            || !newer
            || own::refused_for_good(self.records.folder(), &carried.sha256)
        {
            return Outcome::Refused(why);
        }
        if !self.config.apply {
            return Outcome::Would(vec![format!(
                "{}: lamp-update first, its build {}, which the build carries",
                why.split(';').next().unwrap_or_default(),
                &carried.sha256[..12]
            )]);
        }
        self.climb = Some(too_old);
        Outcome::Refused(why)
    }

    /// Phase 1 of lamp-update's own switch for a build that needs a newer
    /// updater: the lamp-update it carries and nothing else. None when it
    /// is not switched (said), and the refusal stands.
    fn climb(&mut self, too_old: &TooOld) -> Option<Outcome> {
        let (Some(sequence), Some(files_url), Some(carried)) = (
            too_old.sequence,
            too_old.files_url.clone(),
            too_old.carried.clone(),
        ) else {
            return None;
        };
        // Only what own_switch reads of a build: its sequence, where its
        // files are, and the lamp-update it carries.
        let only_the_updater = Manifest {
            schema: manifest::SCHEMA,
            channel: self.config.channel.clone(),
            sequence,
            commit: "0".repeat(40),
            files_url,
            files: vec![carried],
            units: Vec::new(),
            min_updater: None,
            models: Vec::new(),
            settings: Vec::new(),
            image: Vec::new(),
            reset_plan: None,
        };
        match self.own_switch(&only_the_updater) {
            Ok(Some(Outcome::SelfSwitched { from, to, .. })) => Some(Outcome::Climbs {
                sequence,
                needed: too_old.needed,
                from,
                to,
            }),
            Ok(_) => None,
            Err(why) => {
                self.say(&format!("lamp-update's own build is not switched: {why}"));
                None
            }
        }
    }

    /// Phase 1 of lamp-update's own switch (crate::own), once the parts run
    /// build `sequence`: its lamp-update installed beside, `self.json`
    /// written, then `update/current` moved. None when lamp-update already
    /// runs that binary, the build has none, it was put back before, or
    /// `update/current` is not install's link. What goes wrong is said, and
    /// the parts' outcome stands.
    fn own(&mut self, sequence: u64) -> Option<Outcome> {
        let manifest = self.seen.take().filter(|seen| seen.sequence == sequence)?;
        match self.own_switch(&manifest) {
            Ok(switched) => switched,
            Err(why) => {
                self.say(&format!("lamp-update's own build is not switched: {why}"));
                None
            }
        }
    }

    fn own_switch(&mut self, manifest: &Manifest) -> Result<Option<Outcome>, String> {
        let Some(file) = manifest.files.iter().find(|file| file.name == own::BINARY) else {
            return Ok(None);
        };
        let link = self.config.on(own::CURRENT);
        let Ok(old) = fs::read_link(&link) else {
            return Ok(None);
        };
        let old = old.to_string_lossy().into_owned();
        if !unit::is_build_folder(&unit::folder(own::FOLDER, &old))
            || file_sha256(&link.join(own::BINARY)).is_ok_and(|sha| sha == file.sha256)
            || own::refused_for_good(self.records.folder(), &file.sha256)
        {
            return Ok(None);
        }
        own::recovery_ready(&self.config.root, self.world.systemd)?;
        let new_folder = unit::folder(own::FOLDER, &file.sha256);
        let there = self.config.on(&new_folder);
        let wants = vec![Want {
            name: own::BINARY.to_owned(),
            source: Source::Build(file.clone()),
            size: file.size,
        }];
        if !holds(&there, &wants) {
            if there.exists() {
                return Err(format!("{new_folder} holds other files than the build's"));
            }
            let free = self
                .world
                .disk
                .free(&self.config.on(unit::INSTALLED))
                .map_err(|error| format!("the disk's free room cannot be read: {error}"))?;
            if free < file.size.saturating_add(self.config.keep_free) {
                return Err(format!(
                    "the disk has {} MB free, and {} MB are kept free",
                    free / 1_000_000,
                    self.config.keep_free / 1_000_000
                ));
            }
            let temporary = filling(&there);
            if let Err(outcome) = self.fill(manifest, &temporary, &wants) {
                let _ = fs::remove_dir_all(&temporary);
                return Err(outcome.to_string());
            }
            fs::rename(&temporary, &there)
                .and_then(|()| sync_parent(&there))
                .map_err(|error| format!("{new_folder} cannot be named: {error}"))?;
            let mut state = self
                .records
                .state(self.world.say)
                .map_err(|error| format!("the state cannot be read: {error}"))?;
            if !state.made.contains(&new_folder) {
                state.made.push(new_folder.clone());
            }
            self.records
                .save_state(&state)
                .map_err(|error| format!("the state cannot be written: {error}"))?;
            self.say(&format!(
                "installed {new_folder} beside {}",
                unit::folder(own::FOLDER, &old)
            ));
        }
        // The writer's `back` is a switch's only way back: one refreshed and
        // not proved yet is not leaned on (crate::writer_swap).
        if crate::writer_swap::unproven(&self.config.root, self.records.folder()) {
            self.say("lamp-update's own switch waits: the settings writer was refreshed and has not proved itself yet");
            return Ok(None);
        }
        // One put back across a reboot is tried once more (#2810), said.
        if own::retry(self.records.folder(), &file.sha256)
            .map_err(|error| format!("self-refused cannot be written: {error}"))?
        {
            self.say(&format!(
                "lamp-update's build {} was put back after a reboot came in its minutes: it is tried once more, and a second put-back is final",
                &file.sha256[..12]
            ));
        }
        own::note_began(self.records.folder(), &self.config.root, &file.sha256)
            .map_err(|error| format!("self-began cannot be written: {error}"))?;
        own::begin(self.records.folder(), &old, &file.sha256)
            .map_err(|error| format!("self.json cannot be written: {error}"))?;
        replace_link(&link, &file.sha256).map_err(|error| {
            format!(
                "{} cannot be pointed at {}: {error}",
                own::CURRENT,
                file.sha256
            )
        })?;
        self.say(&format!(
            "lamp-update switches to its build {}; it must confirm itself within {} min",
            &file.sha256[..12],
            own::DEADLINE.as_secs() / 60
        ));
        Ok(Some(Outcome::SelfSwitched {
            sequence: manifest.sequence,
            from: old,
            to: file.sha256.clone(),
        }))
    }

    /// Removes the folders this program made that no unit names now, nor
    /// any unit kept from before the last switch: the lamp keeps its build
    /// and the one before. Its own folder, which its unit starts it from,
    /// stays too. A folder it did not make is never removed.
    fn prune(&mut self, state: &mut State) {
        // Named now: by a unit, or by a part's current link.
        let installed = match installed_units(self.config) {
            Ok(installed) => installed,
            Err(error) => {
                self.say(&format!(
                    "build folders are kept because the installed units cannot be read: {error}"
                ));
                return;
            }
        };
        let mut named: BTreeSet<String> = installed
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
        // The folder this program runs from: a round started by hand has no
        // unit of its own to name it, and after phase 1 of its own switch
        // `update/current` names another while it still runs (#1680).
        if let Some(folder) = &self.config.running
            && folder.starts_with(own::FOLDER)
        {
            named.insert(folder.clone());
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
        // And whatever a model path links to now: state.json set aside as
        // unreadable names nothing, and a live link is never left dangling.
        let mut kept: BTreeSet<String> = state
            .models
            .values()
            .chain(state.models_before.values())
            .cloned()
            .collect();
        kept.extend(models::linked_now(self.config));
        for removed in models::prune(self.config, &kept) {
            match removed {
                Ok(path) => self.say(&format!("removed {path}, which no kept build names")),
                Err(why) => self.say(&why),
            }
        }
        if let Err(error) = self.records.save_state(state) {
            self.say(&format!("the state cannot be written: {error}"));
        }
    }
}

/// What a person reads before applying: each unit, what changes, and the
/// lines of its unit file that would change.
fn describe(
    manifest: &Manifest,
    changes: &[Change],
    placing: Option<&Placing>,
    placings: &[models::Placing],
    fetching: &[Model],
) -> Vec<String> {
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
            .filter(|want| {
                matches!(
                    want.source,
                    Source::Build(_) | Source::Model { from: None, .. }
                )
            })
            .map(|want| want.size)
            .sum();
        if let Some(adding) = &change.added {
            let by: Vec<&str> = adding
                .wants
                .iter()
                .filter_map(|link| added::target(link))
                .collect();
            let enabled = if by.is_empty() {
                "not enabled".to_owned()
            } else {
                format!("enabled by {}", by.join(", "))
            };
            let made = match (&adding.link, &change.make) {
                (Some(_), Some(_)) => format!(
                    ", {} made (fetch {:.1} MB)",
                    change.new_folder,
                    size as f64 / 1e6
                ),
                (Some(_), None) => format!(", {} (already there)", change.new_folder),
                _ => String::new(),
            };
            lines.push(format!("{}: added, {enabled}{made}", change.unit));
            continue;
        }
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
    for step in placing
        .map(|placing| placing.steps.as_slice())
        .unwrap_or_default()
    {
        match step.act {
            product_skills::Act::Write => {
                lines.push(format!("product skills: write {}", step.path))
            }
            product_skills::Act::PutBack(why) => {
                lines.push(format!("product skills: put back {} ({why})", step.path));
            }
            product_skills::Act::Record => {}
        }
    }
    for placing in placings {
        lines.push(format!(
            "{}: -> {} (model)",
            placing.model.path,
            models::store_path(&placing.model.sha256)
        ));
    }
    for model in fetching {
        lines.push(format!(
            "fetch {} ({:.1} MB) into the store",
            model.path,
            model.fetch_bytes() as f64 / 1e6
        ));
    }
    lines.push("switched at a quiet moment: nobody heard or answered for 45 s, or the voice muted (asleep or for privacy)".into());
    lines
}

/// Each product skill's file written or found already the build's, as
/// install records its own: the build's before at the next round.
fn record_skills(state: &mut State, steps: &[product_skills::Step]) {
    for step in steps {
        state
            .settings
            .insert(step.path.clone(), step.sha256.clone());
    }
}

/// Every unit of a part installed by build, but this program's own.
fn installed_units(config: &Config) -> io::Result<Vec<Installed>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(config.on(&config.units))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // A linked unit lives somewhere else, which is not this program's
        // to change. Neither is a unit of lamp-update's own: its service, and
        // the writer's one-shot (crate::writer_unit), which starts
        // `update/current` too and must never move that link as a part's.
        if !manifest::is_unit_name(&name)
            || name == OWN_UNIT
            || name == crate::writer_unit::UNIT
            || !entry.file_type()?.is_file()
        {
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

/// A part's templates, sockets and timers on this lamp (crate::companion):
/// each regular file of those names, by name. A linked one lives somewhere
/// else, which is not this program's to change.
fn installed_companions(config: &Config) -> io::Result<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(config.on(&config.units))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if companion::kind(&name).is_none() || !entry.file_type()?.is_file() {
            continue;
        }
        found.insert(name, fs::read_to_string(entry.path())?);
    }
    Ok(found)
}

/// The installed part one of a part's other units on this lamp belongs to:
/// started through its `current` link, as that unit starts it. None for
/// one that starts no part (the keeper's) or a part not installed here.
fn owner<'i>(
    installed: &'i [Installed],
    companions: &BTreeMap<String, String>,
    name: &str,
) -> Option<&'i Installed> {
    let text = companions.get(name)?;
    let lamps = |unit: &str| {
        companions.get(unit).map(String::as_str).or_else(|| {
            installed
                .iter()
                .find(|part| part.unit == unit)
                .map(|part| part.text.as_str())
        })
    };
    let (builds, binary) = companion::part(name, text, lamps)?;
    installed.iter().find(|part| {
        part.program.linked && part.program.builds == builds && part.program.binary == binary
    })
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
            Source::Model { model, .. } => file_sha256(&path).is_ok_and(|sha| sha == model.sha256),
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
            Source::Model { model, .. } => Some(format!("{}  {}\n", model.sha256, want.name)),
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
    models: &BTreeMap<&str, &Model>,
    program: &Program,
) -> Result<Vec<Want>, String> {
    let old_folder = program.folder();
    let mut names = BTreeSet::from([program.binary.clone()]);
    names.extend(models.keys().map(|name| (*name).to_owned()));
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
        .map(
            |name| match (files.get(name.as_str()), models.get(name.as_str())) {
                (None, Some(model)) => {
                    let path = config.on(&old_folder).join(&name);
                    let from = file_sha256(&path)
                        .is_ok_and(|sha| sha == model.sha256)
                        .then_some(path);
                    Ok(Want {
                        name,
                        source: Source::Model {
                            model: (*model).clone(),
                            from,
                        },
                        size: model.bytes,
                    })
                }
                (Some(file), _) => Ok(Want {
                    name,
                    source: Source::Build((*file).clone()),
                    size: file.size,
                }),
                (None, None) => {
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
            },
        )
        .collect()
}

/// Step 4: what changes, unit by unit, in the order of a switch.
#[allow(clippy::too_many_arguments)]
fn plan(
    config: &Config,
    manifest: &Manifest,
    installed: &[Installed],
    companions: &BTreeMap<String, String>,
    carried: &BTreeMap<String, String>,
    adding: &BTreeMap<String, String>,
    models: &[Model],
    touched: &[String],
    forced: &[String],
) -> Result<(Vec<Change>, Vec<String>), String> {
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
        let ours: BTreeMap<&str, &Model> = models
            .iter()
            .filter_map(|model| Some((models::name_in(model, &part.program.builds)?, model)))
            .collect();
        let wants = wants_of(config, &files, &ours, &part.program)?;
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
        // A part that reads a model a switch moves is restarted with it.
        let reads_moved = touched.iter().any(|path| models::reads(&part.text, path))
            || forced.contains(&part.unit);
        if new_folder == old_folder && unit::same(&after, &part.text) && !reads_moved {
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
            kind: None,
            added: None,
        });
    }
    // A part's templates, sockets and timers the build changes, written
    // with the rest. Their part's folder is named, and none is made.
    for (name, before) in companions {
        let (Some(after), Some(owner)) = (carried.get(name), owner(installed, companions, name))
        else {
            continue;
        };
        if unit::same(after, before) {
            continue;
        }
        let folder = owner.program.folder();
        changes.push(Change {
            unit: name.clone(),
            part: part_of(&owner.program.binary).to_owned(),
            before: before.clone(),
            after: after.clone(),
            old_folder: folder.clone(),
            new_folder: folder,
            make: None,
            was_active: false,
            link: None,
            kind: companion::kind(name),
            added: None,
        });
    }
    add_units(
        config,
        installed,
        companions,
        carried,
        adding,
        &files,
        models,
        &mut changes,
    )?;
    // Refuse a graph with no safe order before installing or switching
    // anything. The old graph must also be usable if this build is put back.
    let mut texts: BTreeMap<&str, &str> = installed
        .iter()
        .map(|part| (part.unit.as_str(), part.text.as_str()))
        .chain(
            companions
                .iter()
                .filter(|(name, _)| owner(installed, companions, name).is_some())
                .map(|(name, text)| (name.as_str(), text.as_str())),
        )
        .collect();
    let rollback_order = unit::start_order(texts.iter().map(|(name, text)| (*name, *text)))
        .map_err(|why| format!("the rollback unit order is refused: {why}"))?;
    // Project only after ordering the full old graph: unchanged nodes can
    // connect two changed units. Inactive members stay for crash recovery.
    // An added unit is not in the old graph: it is named last, and on the
    // way back stopped and removed, never started again.
    let rollback_order = rollback_order
        .into_iter()
        .filter(|name| changes.iter().any(|change| change.unit == *name))
        .chain(
            changes
                .iter()
                .filter(|change| change.added.is_some())
                .map(|change| change.unit.clone()),
        )
        .collect();
    for change in &changes {
        texts.insert(&change.unit, &change.after);
    }
    let order = unit::start_order(texts.iter().map(|(name, text)| (*name, *text)))?;
    let positions: BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(position, name)| (name.as_str(), position))
        .collect();
    changes.sort_by_key(|change| positions[change.unit.as_str()]);
    Ok((changes, rollback_order))
}

/// The product's units the lamp lacks (`adding`, crate::added), as changes
/// that add them: a part's own service first, so that its other units find
/// it. A part already installed here keeps its folder and link; one new to
/// the lamp is installed afresh in the folder of its binary's SHA-256, its
/// `current` link made to name it. An added unit whose part is neither
/// installed nor added, or whose part's binary the build lacks, is not
/// added. Each is enabled as `install` enables it, and only then.
#[allow(clippy::too_many_arguments)]
fn add_units(
    config: &Config,
    installed: &[Installed],
    companions: &BTreeMap<String, String>,
    carried: &BTreeMap<String, String>,
    adding: &BTreeMap<String, String>,
    files: &BTreeMap<&str, &manifest::File>,
    models: &[Model],
    changes: &mut Vec<Change>,
) -> Result<(), String> {
    let enabled = crate::install::enabled();
    // Only the links not there yet are the switch's to make and to take
    // away: one already there (an old enable) is left as it is, both ways.
    let wants = |name: &str, text: &str| -> Result<Vec<String>, String> {
        if !enabled.iter().any(|unit| unit == name) {
            return Ok(Vec::new());
        }
        Ok(added::wants(name, text)?
            .into_iter()
            .filter(|link| fs::symlink_metadata(config.on(link)).is_err())
            .collect())
    };
    let services = adding
        .iter()
        .filter(|(name, _)| companion::kind(name).is_none());
    for (name, text) in services {
        let Some((builds, binary)) = unit::linked(text) else {
            continue;
        };
        let Some(file) = files.get(binary.as_str()) else {
            continue;
        };
        let wants = wants(name, text)?;
        let installed_part = installed.iter().find(|part| {
            part.program.linked && part.program.builds == builds && part.program.binary == binary
        });
        let link_path = unit::current(&builds);
        let added_part = changes.iter().find(|change| {
            change
                .added
                .as_ref()
                .and_then(|adding| adding.link.as_ref())
                .is_some_and(|(path, _)| *path == link_path)
        });
        let (folder, make, link) = match (installed_part, added_part) {
            (Some(part), _) => (part.program.folder(), None, None),
            (None, Some(other)) => (other.new_folder.clone(), None, None),
            (None, None) => {
                if fs::symlink_metadata(config.on(&link_path)).is_ok() {
                    return Err(format!(
                        "{link_path} is there, and no unit of this lamp starts through it"
                    ));
                }
                let program = Program {
                    builds: builds.clone(),
                    build: file.sha256.clone(),
                    binary: binary.clone(),
                    linked: true,
                };
                let ours: BTreeMap<&str, &Model> = models
                    .iter()
                    .filter_map(|model| Some((models::name_in(model, &builds)?, model)))
                    .collect();
                let wants = wants_of(config, files, &ours, &program)?;
                let folder = program.folder();
                let there = config.on(&folder);
                if there.exists() && !holds(&there, &wants) {
                    return Err(format!(
                        "{} holds other files than the build's",
                        there.display()
                    ));
                }
                let make = (!there.exists()).then_some(wants);
                (folder, make, Some((link_path, file.sha256.clone())))
            }
        };
        changes.push(Change {
            unit: name.clone(),
            part: part_of(&binary).to_owned(),
            before: String::new(),
            after: text.clone(),
            old_folder: folder.clone(),
            new_folder: folder,
            make,
            was_active: false,
            link: None,
            kind: None,
            added: Some(Adding { wants, link }),
        });
    }
    let others = adding
        .iter()
        .filter(|(name, _)| companion::kind(name).is_some());
    for (name, text) in others {
        // The unit it starts: added too, carried, or the lamp's own.
        let lookup = |unit: &str| {
            adding
                .get(unit)
                .or_else(|| carried.get(unit))
                .or_else(|| companions.get(unit))
                .map(String::as_str)
                .or_else(|| {
                    installed
                        .iter()
                        .find(|part| part.unit == unit)
                        .map(|part| part.text.as_str())
                })
        };
        let Some((builds, binary)) = companion::part(name, text, lookup) else {
            continue;
        };
        let folder = installed
            .iter()
            .find(|part| {
                part.program.linked
                    && part.program.builds == builds
                    && part.program.binary == binary
            })
            .map(|part| part.program.folder())
            .or_else(|| {
                changes
                    .iter()
                    .find(|change| {
                        change.added.is_some()
                            && change.kind.is_none()
                            && unit::linked(&change.after) == Some((builds.clone(), binary.clone()))
                    })
                    .map(|change| change.new_folder.clone())
            });
        let Some(folder) = folder else {
            continue;
        };
        changes.push(Change {
            unit: name.clone(),
            part: part_of(&binary).to_owned(),
            before: String::new(),
            after: text.clone(),
            old_folder: folder.clone(),
            new_folder: folder,
            make: None,
            was_active: false,
            link: None,
            kind: companion::kind(name),
            added: Some(Adding {
                wants: wants(name, text)?,
                link: None,
            }),
        });
    }
    Ok(())
}

/// The skills part, and its binary.
const SKILLS_PART: &str = "skills";
const SKILLS_BINARY: &str = "lamp-skills";
/// Where lamp-skills reads its skills when its unit gives no `--dir`.
const SKILLS_DIR: &str = "/var/lib/lampOS/skills";
/// The skills folder as a round would leave it, judged before anything is
/// written: in the records, and removed after.
const STAGED: &str = "skills-staged";
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

/// Every model a round needs in the store and it lacks, once each: those
/// for paths of their own, and those for a part's new folder not already
/// in its old one.
fn to_fetch(config: &Config, changes: &[Change], placings: &[models::Placing]) -> Vec<Model> {
    let mut seen = BTreeSet::new();
    let folder_models = changes
        .iter()
        .flat_map(|change| change.make.iter().flatten())
        .filter_map(|want| match &want.source {
            Source::Model { model, from: None } => Some(model),
            _ => None,
        });
    placings
        .iter()
        .map(|placing| &placing.model)
        .chain(folder_models)
        .filter(|model| !models::stored(config, &model.sha256))
        .filter(|model| seen.insert(model.sha256.clone()))
        .cloned()
        .collect()
}
