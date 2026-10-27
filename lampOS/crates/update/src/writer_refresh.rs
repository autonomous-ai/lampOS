//! Persisted admission for a future fixed-writer refresh (#3689).
//!
//! **Unwired:** no CLI or round calls this module. It authenticates no build,
//! copies no executable and changes no unit or setting. Its caller must first
//! verify the signed operation and actual bytes represented by these identities.
//! The caller supplies an existing private, stable directory it owns; checking
//! privileged ancestors and installing a guard are separate, unimplemented work.
//!
//! A live session holds its own nonblocking lock, never the product-record lock.
//! A process that finds an unfinished journal may only recover it. In particular,
//! a reboot cannot revive an issued first-apply grant. Qualification retains all
//! identities until product recovery closes; it does not admit another round.
use crate::{manifest, records::write_whole};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};

const VERSION: u32 = 1;
const JOURNAL: &str = "writer-refresh.json";
const LOCK: &str = "writer-refresh.lock";
/// The journal contains identities, not executable bytes or file preimages.
pub const MOST_JOURNAL_BYTES: u64 = 64 * 1024;
type Later = BTreeMap<String, serde_json::Value>;

fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn metadata(later: &Later, known: &[&str]) -> io::Result<()> {
    if later.keys().any(|key| known.contains(&key.as_str())) {
        return Err(invalid("future metadata shadows a known field"));
    }
    Ok(())
}

/// Exact bytes, including their length. This is an identity, not a verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub sha256: String,
    pub size: u64,
    #[serde(flatten)]
    later: Later,
}

impl Artifact {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: manifest::sha256_hex(bytes),
            size: bytes.len() as u64,
            later: Later::new(),
        }
    }

    fn check(&self, most: u64) -> io::Result<()> {
        if !digest(&self.sha256) || self.size == 0 || self.size > most {
            return Err(invalid("invalid or oversized artifact identity"));
        }
        metadata(&self.later, &["sha256", "size"])
    }

    fn matches(&self, other: &Self) -> bool {
        self.sha256 == other.sha256 && self.size == other.size
    }
}

/// Expected refresh identity supplied by an authenticated caller, with the
/// immutable recovery inputs it retains. Construction is not authentication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub transaction: String,
    pub sequence: u64,
    pub envelope: Artifact,
    pub guard: Artifact,
    pub writer: Artifact,
    pub previous_writer: Artifact,
    pub product_before: Artifact,
    #[serde(flatten)]
    later: Later,
}

impl Binding {
    pub fn new(
        transaction: String,
        sequence: u64,
        envelope: Artifact,
        guard: Artifact,
        writer: Artifact,
        previous_writer: Artifact,
        product_before: Artifact,
    ) -> Self {
        Self {
            transaction,
            sequence,
            envelope,
            guard,
            writer,
            previous_writer,
            product_before,
            later: Later::new(),
        }
    }

    fn check(&self) -> io::Result<()> {
        if !digest(&self.transaction) || self.sequence == 0 {
            return Err(invalid("invalid refresh transaction or sequence"));
        }
        self.envelope.check(manifest::MOST_ENVELOPE_BYTES)?;
        for artifact in [
            &self.guard,
            &self.writer,
            &self.previous_writer,
            &self.product_before,
        ] {
            artifact.check(manifest::MOST_FILE_BYTES)?;
        }
        metadata(
            &self.later,
            &[
                "transaction",
                "sequence",
                "envelope",
                "guard",
                "writer",
                "previous_writer",
                "product_before",
            ],
        )
    }

    fn matches(&self, other: &Self) -> bool {
        self.transaction == other.transaction
            && self.sequence == other.sequence
            && self.envelope.matches(&other.envelope)
            && self.guard.matches(&other.guard)
            && self.writer.matches(&other.writer)
            && self.previous_writer.matches(&other.previous_writer)
            && self.product_before.matches(&other.product_before)
    }
}

/// One exact first-apply operation. Plan and preimages are independently kept
/// bytes; this module retains their identities but does not write or read them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstApply {
    pub transaction: String,
    pub refresh: Binding,
    /// A later build may make the first real apply with this same writer.
    /// Bind its authenticated input separately from the bootstrap envelope.
    pub envelope: Artifact,
    pub sequence: u64,
    pub plan: Artifact,
    pub preimages: Artifact,
    #[serde(flatten)]
    later: Later,
}

impl FirstApply {
    pub fn new(
        transaction: String,
        refresh: Binding,
        envelope: Artifact,
        sequence: u64,
        plan: Artifact,
        preimages: Artifact,
    ) -> Self {
        Self {
            transaction,
            refresh,
            envelope,
            sequence,
            plan,
            preimages,
            later: Later::new(),
        }
    }

    fn check(&self, binding: &Binding) -> io::Result<()> {
        self.refresh.check()?;
        if !digest(&self.transaction)
            || self.transaction == binding.transaction
            || !self.refresh.matches(binding)
            || self.sequence < binding.sequence
            || (self.sequence == binding.sequence && !self.envelope.matches(&binding.envelope))
        {
            return Err(invalid("first apply does not match this refresh"));
        }
        self.envelope.check(manifest::MOST_ENVELOPE_BYTES)?;
        self.plan.check(manifest::MOST_FILE_BYTES)?;
        self.preimages.check(manifest::MOST_FILE_BYTES)?;
        metadata(
            &self.later,
            &[
                "transaction",
                "refresh",
                "envelope",
                "sequence",
                "plan",
                "preimages",
            ],
        )
    }

    fn matches(&self, other: &Self) -> bool {
        self.transaction == other.transaction
            && self.refresh.matches(&other.refresh)
            && self.envelope.matches(&other.envelope)
            && self.sequence == other.sequence
            && self.plan.matches(&other.plan)
            && self.preimages.matches(&other.preimages)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Armed,
    Published,
    Probed,
    Granted,
    Applying,
    Qualified,
    Closed,
    Restoring,
    Restored,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    version: u32,
    operation: Operation,
    binding: Binding,
    phase: Phase,
    restore_from: Option<Phase>,
    grant: Option<FirstApply>,
    qualification: Option<Artifact>,
    product_closed: Option<Artifact>,
    #[serde(flatten)]
    later: Later,
}

impl Journal {
    fn check(&self, expected: &Binding) -> io::Result<()> {
        if self.version != VERSION {
            return Err(invalid("unsupported writer-refresh journal version"));
        }
        self.binding.check()?;
        expected.check()?;
        if !self.binding.matches(expected) {
            return Err(invalid("writer-refresh identity does not match"));
        }
        if let Some(grant) = &self.grant {
            grant.check(&self.binding)?;
        }
        for receipt in [&self.qualification, &self.product_closed]
            .into_iter()
            .flatten()
        {
            receipt.check(MOST_JOURNAL_BYTES)?;
        }
        if !matches!(self.phase, Phase::Restoring | Phase::Restored) && self.restore_from.is_some()
        {
            return Err(invalid("unexpected writer-refresh restoration provenance"));
        }
        let shape = match self.phase {
            Phase::Prepared | Phase::Armed | Phase::Published | Phase::Probed => {
                self.grant.is_none()
                    && self.qualification.is_none()
                    && self.product_closed.is_none()
            }
            Phase::Granted | Phase::Applying => {
                self.grant.is_some()
                    && self.qualification.is_none()
                    && self.product_closed.is_none()
            }
            Phase::Qualified => {
                self.grant.is_some()
                    && self.qualification.is_some()
                    && self.product_closed.is_none()
            }
            Phase::Closed => {
                self.grant.is_some()
                    && self.qualification.is_some()
                    && self.product_closed.is_some()
            }
            Phase::Restoring | Phase::Restored => {
                let grant_required = match self.restore_from {
                    Some(Phase::Prepared | Phase::Armed | Phase::Published | Phase::Probed) => {
                        false
                    }
                    Some(Phase::Granted | Phase::Applying) => true,
                    _ => return Err(invalid("missing or invalid restoration provenance")),
                };
                self.grant.is_some() == grant_required
                    && self.qualification.is_none()
                    && self.product_closed.is_none()
            }
        };
        if !shape {
            return Err(invalid("inconsistent writer-refresh journal phase"));
        }
        metadata(
            &self.later,
            &[
                "version",
                "operation",
                "binding",
                "phase",
                "restore_from",
                "grant",
                "qualification",
                "product_closed",
            ],
        )
    }

    fn ordinary(&self) -> Option<Artifact> {
        match self.phase {
            Phase::Closed => Some(self.binding.writer.clone()),
            Phase::Restored => Some(self.binding.previous_writer.clone()),
            _ => None,
        }
    }
}

/// Admission is separate from executing or restoring any external effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Ordinary {
        writer: Artifact,
    },
    /// Only the live session that completed its probe can issue the grant.
    AwaitingFirstApply,
    OnlyFirstApply(Box<FirstApply>),
    RecoveryNeeded,
    /// Retain the qualified writer and both sets of preimages for the product
    /// recovery. Reverting to the old writer here would lose new-path undo.
    ProductRecoveryNeeded,
}

/// The validated stage whose effects may need undoing. A Restoring journal
/// retains this original stage; it is not treated as a new writer-only plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryOrigin {
    Prepared,
    Armed,
    Published,
    Probed,
    Granted,
    Applying,
}

/// A read-only view while the recovery session holds its guard lock. These
/// references identify retained inputs; they authenticate no signature or
/// bytes and are not permission to execute an external effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryInputs<'a> {
    RestoreWriter {
        binding: &'a Binding,
        origin: RecoveryOrigin,
        restoration_started: bool,
    },
    RestoreApply {
        binding: &'a Binding,
        first_apply: &'a FirstApply,
        origin: RecoveryOrigin,
        restoration_started: bool,
    },
    /// Keep this qualified writer for product recovery: the older writer
    /// cannot necessarily interpret the first apply's new-path preimages.
    RecoverProduct {
        binding: &'a Binding,
        first_apply: &'a FirstApply,
        qualification: &'a Artifact,
    },
    /// Terminal state has no refresh restoration action. Receipts and inputs
    /// remain available without reparsing the journal or authorizing pruning.
    Ordinary {
        binding: &'a Binding,
        writer: &'a Artifact,
        first_apply: Option<&'a FirstApply>,
        qualification: Option<&'a Artifact>,
        product_closed: Option<&'a Artifact>,
    },
}

/// An already existing private folder. No default system path is provided.
pub struct Store {
    folder: PathBuf,
    #[cfg(test)]
    fail_after_write: std::cell::Cell<bool>,
}

impl Store {
    pub fn open(folder: PathBuf) -> io::Result<Self> {
        let meta = fs::symlink_metadata(&folder)?;
        if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid("writer-refresh store is not a private directory"));
        }
        Ok(Self {
            folder,
            #[cfg(test)]
            fail_after_write: std::cell::Cell::new(false),
        })
    }

    fn lock(&self) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(self.folder.join(LOCK))?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid("writer-refresh lock is not a private regular file"));
        }
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(io::Error::from)?;
        Ok(file)
    }

    fn read(&self, expected: &Binding) -> io::Result<Journal> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(self.folder.join(JOURNAL))?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid(
                "writer-refresh journal is not a private regular file",
            ));
        }
        if meta.len() > MOST_JOURNAL_BYTES {
            return Err(invalid("writer-refresh journal is too large"));
        }
        let mut bytes = Vec::new();
        file.take(MOST_JOURNAL_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MOST_JOURNAL_BYTES {
            return Err(invalid("writer-refresh journal grew too large"));
        }
        // Serde's full error may echo an unknown enum value or personal bytes.
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(&format!(
                "writer-refresh journal cannot be read at line {} column {}",
                error.line(),
                error.column()
            ))
        })?;
        journal.check(expected)?;
        Ok(journal)
    }

    fn save(&self, journal: &Journal) -> io::Result<()> {
        journal.check(&journal.binding)?;
        let bytes = serde_json::to_vec(journal).map_err(io::Error::other)?;
        if bytes.len() as u64 > MOST_JOURNAL_BYTES {
            return Err(invalid("writer-refresh journal is too large"));
        }
        write_whole(&self.folder.join(JOURNAL), &bytes, 0o600)?;
        // Test visible new bytes with a failed acknowledgment. A real parent
        // fsync can fail after rename; a failed acknowledgment after completed
        // write_whole is the same conservative case for the live controller.
        #[cfg(test)]
        if self.fail_after_write.replace(false) {
            return Err(io::Error::other(
                "injected failure after journal publication",
            ));
        }
        Ok(())
    }

    /// Creates one transaction. Existing journals, including terminal receipts,
    /// are retained; archiving/starting a later refresh is not implemented here.
    pub fn prepare(&self, binding: Binding) -> io::Result<Session<'_>> {
        let lock = self.lock()?;
        match fs::symlink_metadata(self.folder.join(JOURNAL)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => return Err(invalid("writer-refresh journal already exists")),
        }
        let journal = Journal {
            version: VERSION,
            operation: Operation::Refresh,
            binding,
            phase: Phase::Prepared,
            restore_from: None,
            grant: None,
            qualification: None,
            product_closed: None,
            later: Later::new(),
        };
        self.save(&journal)?;
        Ok(Session {
            store: self,
            _lock: lock,
            journal,
            recovery: false,
            poisoned: false,
        })
    }

    /// Loading cannot revive a live first-apply grant. Missing or invalid data
    /// is an error; the bytes are never deleted, set aside or replaced.
    pub fn inspect(&self, expected: &Binding) -> io::Result<Admission> {
        let _lock = self.lock()?;
        let journal = self.read(expected)?;
        Ok(resting_admission(&journal))
    }

    pub fn recover(&self, expected: &Binding) -> io::Result<Session<'_>> {
        let lock = self.lock()?;
        let journal = self.read(expected)?;
        Ok(Session {
            store: self,
            _lock: lock,
            journal,
            recovery: true,
            poisoned: false,
        })
    }
}

fn resting_admission(journal: &Journal) -> Admission {
    if let Some(writer) = journal.ordinary() {
        Admission::Ordinary { writer }
    } else if journal.phase == Phase::Qualified {
        Admission::ProductRecoveryNeeded
    } else {
        Admission::RecoveryNeeded
    }
}

/// The guard lock stays held for this live controller. No product lock is
/// acquired or inherited here. Dropping it leaves durable recovery required.
pub struct Session<'a> {
    store: &'a Store,
    _lock: File,
    journal: Journal,
    recovery: bool,
    poisoned: bool,
}

/// A durable grant already consumed by its live session. It cannot be created
/// by deserializing a caller receipt or regained after a restart.
pub struct Permit {
    grant: FirstApply,
}

impl Session<'_> {
    /// Expose only Store-validated retained inputs after a recovery reload.
    /// Live or poisoned sessions must first drop their lock and reload; this
    /// accessor neither reparses JSON nor advances or saves a journal phase.
    pub fn recovery_inputs(&self) -> io::Result<RecoveryInputs<'_>> {
        if !self.recovery || self.poisoned {
            return Err(invalid(
                "recovery inputs require a reloaded recovery session",
            ));
        }
        let journal = &self.journal;
        let grant = || {
            journal
                .grant
                .as_ref()
                .ok_or_else(|| invalid("recovery input is missing its first-apply identity"))
        };
        match journal.phase {
            Phase::Qualified => {
                return Ok(RecoveryInputs::RecoverProduct {
                    binding: &journal.binding,
                    first_apply: grant()?,
                    qualification: journal.qualification.as_ref().ok_or_else(|| {
                        invalid("product recovery input is missing its qualification identity")
                    })?,
                });
            }
            Phase::Closed | Phase::Restored => {
                let writer = if journal.phase == Phase::Closed {
                    &journal.binding.writer
                } else {
                    &journal.binding.previous_writer
                };
                return Ok(RecoveryInputs::Ordinary {
                    binding: &journal.binding,
                    writer,
                    first_apply: journal.grant.as_ref(),
                    qualification: journal.qualification.as_ref(),
                    product_closed: journal.product_closed.as_ref(),
                });
            }
            _ => {}
        }
        let restoration_started = journal.phase == Phase::Restoring;
        let source = if restoration_started {
            journal
                .restore_from
                .ok_or_else(|| invalid("recovery input is missing its restoration origin"))?
        } else {
            journal.phase
        };
        let origin = match source {
            Phase::Prepared => RecoveryOrigin::Prepared,
            Phase::Armed => RecoveryOrigin::Armed,
            Phase::Published => RecoveryOrigin::Published,
            Phase::Probed => RecoveryOrigin::Probed,
            Phase::Granted => RecoveryOrigin::Granted,
            Phase::Applying => RecoveryOrigin::Applying,
            _ => {
                return Err(invalid(
                    "recovery input has an unsupported restoration origin",
                ));
            }
        };
        if matches!(source, Phase::Granted | Phase::Applying) {
            Ok(RecoveryInputs::RestoreApply {
                binding: &journal.binding,
                first_apply: grant()?,
                origin,
                restoration_started,
            })
        } else {
            Ok(RecoveryInputs::RestoreWriter {
                binding: &journal.binding,
                origin,
                restoration_started,
            })
        }
    }

    pub fn admission(&self) -> Admission {
        if self.poisoned {
            return Admission::RecoveryNeeded;
        }
        if !self.recovery {
            match self.journal.phase {
                Phase::Probed => return Admission::AwaitingFirstApply,
                Phase::Granted => {
                    return Admission::OnlyFirstApply(Box::new(
                        self.journal.grant.clone().unwrap(),
                    ));
                }
                _ => {}
            }
        }
        resting_admission(&self.journal)
    }

    fn update(&mut self, journal: Journal) -> io::Result<()> {
        if self.poisoned {
            return Err(invalid(
                "writer-refresh persistence failed; reload for recovery",
            ));
        }
        if let Err(error) = self.store.save(&journal) {
            // The rename may be visible even if the following sync failed.
            // Never act on the stale in-memory phase after any save error.
            self.poisoned = true;
            return Err(error);
        }
        self.journal = journal;
        Ok(())
    }

    fn advance(&mut self, before: Phase, after: Phase) -> io::Result<()> {
        if self.recovery || self.journal.phase != before {
            return Err(invalid("writer-refresh phase cannot advance"));
        }
        let mut next = self.journal.clone();
        next.phase = after;
        self.update(next)
    }

    /// These acknowledgments record the caller's verified external results;
    /// they do not perform or establish the effects themselves.
    pub fn armed(&mut self) -> io::Result<()> {
        self.advance(Phase::Prepared, Phase::Armed)
    }

    pub fn published(&mut self) -> io::Result<()> {
        self.advance(Phase::Armed, Phase::Published)
    }

    pub fn probed(&mut self) -> io::Result<()> {
        self.advance(Phase::Published, Phase::Probed)
    }

    pub fn grant(&mut self, grant: FirstApply) -> io::Result<()> {
        if self.recovery || self.journal.phase != Phase::Probed {
            return Err(invalid("first apply cannot be granted in this phase"));
        }
        grant.check(&self.journal.binding)?;
        let mut next = self.journal.clone();
        next.grant = Some(grant);
        next.phase = Phase::Granted;
        self.update(next)
    }

    /// Consume once, durably before returning permission to the live caller.
    pub fn claim(&mut self, grant: &FirstApply) -> io::Result<Permit> {
        if self.recovery
            || self.journal.phase != Phase::Granted
            || !self
                .journal
                .grant
                .as_ref()
                .is_some_and(|saved| saved.matches(grant))
        {
            return Err(invalid(
                "first-apply grant is stale, mismatched or consumed",
            ));
        }
        self.advance(Phase::Granted, Phase::Applying)?;
        Ok(Permit {
            grant: self.journal.grant.clone().unwrap(),
        })
    }

    pub fn qualify(&mut self, permit: &Permit, receipt: Artifact) -> io::Result<()> {
        if self.recovery
            || self.journal.phase != Phase::Applying
            || !self
                .journal
                .grant
                .as_ref()
                .is_some_and(|saved| saved.matches(&permit.grant))
        {
            return Err(invalid(
                "writer qualification does not match the live apply",
            ));
        }
        let mut next = self.journal.clone();
        next.qualification = Some(receipt);
        next.phase = Phase::Qualified;
        self.update(next)
    }

    /// The caller has durably closed or recovered the bound product journal.
    /// A loaded Qualified journal may close only this product recovery; it may
    /// not resume the candidate or restore a writer too old for its preimages.
    pub fn product_closed(&mut self, grant: &FirstApply, receipt: Artifact) -> io::Result<()> {
        if self.journal.phase != Phase::Qualified
            || !self
                .journal
                .grant
                .as_ref()
                .is_some_and(|saved| saved.matches(grant))
        {
            return Err(invalid(
                "product closure does not match the qualified apply",
            ));
        }
        let mut next = self.journal.clone();
        next.product_closed = Some(receipt);
        next.phase = Phase::Closed;
        self.update(next)
    }

    pub fn restoring(&mut self) -> io::Result<()> {
        if self.poisoned {
            return Err(invalid(
                "writer-refresh persistence failed; reload for recovery",
            ));
        }
        if self.journal.phase == Phase::Restoring {
            return Ok(());
        }
        if matches!(
            self.journal.phase,
            Phase::Qualified | Phase::Closed | Phase::Restored
        ) {
            return Err(invalid(
                "this writer cannot be reverted by refresh recovery",
            ));
        }
        let mut next = self.journal.clone();
        next.restore_from = Some(next.phase);
        next.phase = Phase::Restoring;
        self.update(next)
    }

    /// Only after the caller restored partial settings/bookkeeping, then the
    /// previous writer, and verified its serviceability. No files are removed.
    pub fn restored(&mut self) -> io::Result<()> {
        if self.journal.phase != Phase::Restoring {
            return Err(invalid("writer restoration was not in progress"));
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Restored;
        self.update(next)
    }
}

#[cfg(test)]
mod tests;
