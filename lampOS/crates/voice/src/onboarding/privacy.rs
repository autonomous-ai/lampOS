//! Step-specific physical privacy evidence, never microphone authority.
//!
//! The caller supplies receipts already accepted by Controls' existing fence.
//! Close observation is memory-only; persist only after revocation and verified
//! teardown. Opening still requires the ordinary privacy gate and input readiness.
use lamp_msgs::PrivacySwitch;
use lamp_turn_owner::BootId;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::io::Read;

pub(crate) const ATTEMPT_US: u64 = 300_000_000;
const MAX_BYTES: usize = 4096;
const VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Epoch {
    /// Device boot, not a voice-cycle boot: a new voice cycle is expected.
    pub device_boot: String,
    pub clock_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Acquisition {
    pub controls_session: String,
    pub physical: PrivacySwitch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Invitation {
    pub voice_boot: BootId,
    pub turn: u64,
    /// The agreed attributable delivery boundary, not model text or admission.
    pub delivered_at_us: u64,
}

/// The eyes' receipt this close waits for (#1802): asked with `token` at
/// `asked_unix_ms`, only when eyes that give receipts were there at the
/// close. Absent: the step proves the microphones only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraClose {
    pub token: u64,
    pub asked_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Armed,
    CloseObserved,
    Closed,
    Done,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenOutcome {
    WaitingForReadiness,
    Done,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Invalidation {
    SoftwareMute,
    SourceRestart,
    Redo,
    #[cfg(test)]
    Reset,
    FenceFailure,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    attempt: BootId,
    epoch: Epoch,
    invitation: Invitation,
    baseline: Acquisition,
    /// Present only after cleanup was verified; unverified close is never stored.
    closed: Option<Acquisition>,
    /// Only with a close, and only when the eyes were asked for a receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<CameraClose>,
}

pub(crate) struct Attempt {
    stored: Stored,
    phase: Phase,
    pending_close: Option<Acquisition>,
    latest_sequence: u64,
    latest_at_us: u64,
}

/// Owned candidate attached to the physical mute lifecycle token. The outer
/// service may persist it only after the rig's verified cleanup has returned.
#[derive(Clone)]
pub(crate) struct Checkpoint {
    // None is an explicit discard operation, not absence of a lifecycle action.
    attempt: Option<std::sync::Arc<std::sync::Mutex<Attempt>>>,
    path: PathBuf,
}

impl std::fmt::Debug for Checkpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivacyCheckpoint")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckpointOutcome {
    Closed,
    Discarded,
}

impl Checkpoint {
    pub(crate) fn new(attempt: Attempt, path: PathBuf) -> Self {
        Self {
            attempt: Some(std::sync::Arc::new(std::sync::Mutex::new(attempt))),
            path,
        }
    }

    pub(crate) fn discard(path: PathBuf) -> Self {
        Self {
            attempt: None,
            path,
        }
    }

    pub(crate) fn is_discard(&self) -> bool {
        self.attempt.is_none()
    }
    pub(crate) fn matches_path(&self, path: &Path) -> bool {
        self.path == path
    }
    pub(crate) fn closed_bytes(&self) -> io::Result<Option<Vec<u8>>> {
        let Some(attempt) = &self.attempt else {
            return Ok(None);
        };
        let attempt = attempt
            .lock()
            .map_err(|_| io::Error::other("privacy checkpoint lock poisoned"))?;
        if attempt.phase != Phase::Closed {
            return Err(invalid("privacy checkpoint not committed"));
        }
        let bytes = serde_json::to_vec(&attempt.stored).map_err(io::Error::other)?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("privacy checkpoint exceeds metadata bound"));
        }
        Ok(Some(bytes))
    }

    /// The receipt token the eyes are to be asked for, if this close waits
    /// for one.
    pub(crate) fn camera(&self) -> Option<CameraClose> {
        self.attempt
            .as_ref()?
            .lock()
            .ok()
            .and_then(|attempt| attempt.camera())
    }

    /// This close also waits for the eyes' receipt (#1802).
    pub(crate) fn expect_camera(&self, camera: CameraClose) {
        if let Some(attempt) = &self.attempt
            && let Ok(mut attempt) = attempt.lock()
        {
            attempt.expect_camera(camera);
        }
    }

    pub(crate) fn persist_after_cleanup(&self) -> io::Result<CheckpointOutcome> {
        match &self.attempt {
            Some(attempt) => {
                attempt
                    .lock()
                    .map_err(|_| io::Error::other("privacy checkpoint lock poisoned"))?
                    .persist_closed_after_cleanup(&self.path, true)?;
                Ok(CheckpointOutcome::Closed)
            }
            None => {
                discard_checkpoint(&self.path)?;
                Ok(CheckpointOutcome::Discarded)
            }
        }
    }
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn identity(epoch: &Epoch, value: &Acquisition) -> bool {
    name(&epoch.device_boot)
        && name(&epoch.clock_id)
        && name(&value.controls_session)
        && value.physical.check().is_ok()
        && value.physical.fault.is_none()
        && value.physical.muted.is_some()
        && value.physical.stamp.clock_id == epoch.clock_id
}
fn same_source(left: &Acquisition, right: &Acquisition) -> bool {
    left.controls_session == right.controls_session
        && left.physical.reader_session_id == right.physical.reader_session_id
        && left.physical.stamp.clock_id == right.physical.stamp.clock_id
}

impl Attempt {
    pub(crate) fn arm(
        epoch: Epoch,
        invitation: Invitation,
        baseline: Acquisition,
        now_us: u64,
    ) -> io::Result<Self> {
        if !identity(&epoch, &baseline)
            || !baseline.physical.fresh_at(&epoch.clock_id, now_us)
            || baseline.physical.muted != Some(false)
            || invitation.turn == 0
            || invitation.delivered_at_us > now_us
            || invitation.delivered_at_us.checked_add(ATTEMPT_US).is_none()
        {
            return Err(invalid(
                "privacy attempt needs delivered invitation and fresh open baseline",
            ));
        }
        let latest_sequence = baseline.physical.stamp.sequence;
        let latest_at_us = baseline.physical.stamp.at_us;
        Ok(Self {
            stored: Stored {
                version: VERSION,
                attempt: crate::process::new_boot()?,
                epoch,
                invitation,
                baseline,
                closed: None,
                camera: None,
            },
            phase: Phase::Armed,
            pending_close: None,
            latest_sequence,
            latest_at_us,
        })
    }

    pub(crate) fn id(&self) -> BootId {
        self.stored.attempt
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn delivered_at_us(&self) -> u64 {
        self.stored.invitation.delivered_at_us
    }
    pub(crate) fn camera(&self) -> Option<CameraClose> {
        self.stored.camera
    }

    /// The close just observed also waits for the eyes' receipt; written
    /// with the close, never alone.
    pub(crate) fn expect_camera(&mut self, camera: CameraClose) {
        if self.phase == Phase::CloseObserved {
            self.stored.camera = Some(camera);
        }
    }

    /// Persisted acquisition watermark for recovery. The runner tracks any
    /// subsequent open observations it feeds while waiting for readiness.
    pub(crate) fn last_acquisition(&self) -> &Acquisition {
        self.stored.closed.as_ref().unwrap_or(&self.stored.baseline)
    }

    /// Explicit software mute, source discontinuity, redo and reset never prove
    /// a physical close. The owner calls erase separately at a safe I/O boundary.
    pub(crate) fn invalidate(&mut self, _why: Invalidation) {
        self.phase = Phase::Invalid;
        self.pending_close = None;
    }

    fn reject<T>(&mut self, why: &'static str) -> io::Result<T> {
        self.phase = Phase::Invalid;
        self.pending_close = None;
        Err(invalid(why))
    }

    fn fresh_next(&self, value: &Acquisition, now_us: u64) -> bool {
        identity(&self.stored.epoch, value)
            && same_source(&self.stored.baseline, value)
            && value.physical.fresh_at(&self.stored.epoch.clock_id, now_us)
            && value.physical.stamp.sequence > self.latest_sequence
            && value.physical.stamp.at_us >= self.latest_at_us
    }

    /// No filesystem work: return to revocation/teardown before persisting.
    pub(crate) fn observe_close(&mut self, value: Acquisition, now_us: u64) -> io::Result<()> {
        if self.phase != Phase::Armed
            || !self.fresh_next(&value, now_us)
            || value.physical.muted != Some(true)
            || value.physical.stamp.at_us <= self.stored.invitation.delivered_at_us
        {
            return self
                .reject("privacy close is stale, duplicate, wrong-source or before invitation");
        }
        self.latest_sequence = value.physical.stamp.sequence;
        self.latest_at_us = value.physical.stamp.at_us;
        self.pending_close = Some(value);
        self.phase = Phase::CloseObserved;
        Ok(())
    }

    /// Optional armed checkpoint; never serializes an observed unverified close.
    #[cfg(test)]
    pub(crate) fn persist_armed(&mut self, path: &Path) -> io::Result<()> {
        if self.phase != Phase::Armed {
            return Err(invalid("privacy attempt is not armed"));
        }
        if let Err(error) = save(path, &self.stored) {
            self.phase = Phase::Invalid;
            return Err(error);
        }
        Ok(())
    }

    /// Only the service's verified-cleanup boundary may call this with true.
    /// A failed write never promotes the in-memory attempt to Closed. An
    /// error after rename can leave a truthful verified-close checkpoint on
    /// disk; it still cannot complete without a fresh open and readiness.
    pub(crate) fn persist_closed_after_cleanup(
        &mut self,
        path: &Path,
        cleanup_verified: bool,
    ) -> io::Result<()> {
        if !cleanup_verified || self.phase != Phase::CloseObserved {
            return self.reject("privacy close requires verified cleanup before persistence");
        }
        let mut next = self.stored.clone();
        next.closed = self.pending_close.clone();
        if let Err(error) = save(path, &next) {
            self.phase = Phase::Invalid;
            self.pending_close = None;
            return Err(error);
        }
        self.stored = next;
        self.pending_close = None;
        self.phase = Phase::Closed;
        Ok(())
    }

    /// Only an open physical observation evaluates expiry; waiting closed never
    /// expires into an authority change. `ready` attests the existing stable-open
    /// gate AND listening readiness. A new voice boot is fine; a new source is not.
    pub(crate) fn observe_open(
        &mut self,
        value: Acquisition,
        now_us: u64,
        ready: bool,
    ) -> io::Result<OpenOutcome> {
        if self.phase != Phase::Closed
            || !self.fresh_next(&value, now_us)
            || value.physical.muted != Some(false)
        {
            return self
                .reject("privacy reopen lacks a fresh same-source open after verified close");
        }
        self.latest_sequence = value.physical.stamp.sequence;
        self.latest_at_us = value.physical.stamp.at_us;
        if now_us.saturating_sub(self.stored.invitation.delivered_at_us) >= ATTEMPT_US {
            self.phase = Phase::Invalid;
            return Ok(OpenOutcome::Expired);
        }
        if !ready {
            return Ok(OpenOutcome::WaitingForReadiness);
        }
        self.phase = Phase::Done;
        Ok(OpenOutcome::Done)
    }

    /// A bounded metadata checkpoint, not a grant to reopen. Armed checkpoints
    /// recover as Armed; only a stored verified close recovers as Closed.
    #[cfg(test)]
    pub(crate) fn load(path: &Path, epoch: &Epoch, now_us: u64) -> io::Result<Option<Self>> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(invalid("privacy checkpoint path must be an absolute file"));
        }
        let file = match rustix::fs::openat(
            rustix::fs::CWD,
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(io::Error::from)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("privacy checkpoint exceeds metadata bound"));
        }
        Self::from_bytes(&bytes, epoch, now_us).map(Some)
    }

    pub(crate) fn from_bytes(bytes: &[u8], epoch: &Epoch, now_us: u64) -> io::Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("privacy checkpoint exceeds metadata bound"));
        }
        let stored: Stored =
            serde_json::from_slice(bytes).map_err(|_| invalid("invalid privacy checkpoint"))?;
        if stored.version != VERSION
            || &stored.epoch != epoch
            || !identity(epoch, &stored.baseline)
            || stored.baseline.physical.muted != Some(false)
            || stored.invitation.turn == 0
            || stored.invitation.delivered_at_us > now_us
            || stored
                .invitation
                .delivered_at_us
                .checked_add(ATTEMPT_US)
                .is_none()
            || stored.baseline.physical.stamp.at_us > now_us
        {
            return Err(invalid("privacy checkpoint epoch or invitation is invalid"));
        }
        if stored.camera.is_some() && stored.closed.is_none() {
            return Err(invalid("privacy checkpoint camera receipt without a close"));
        }
        if let Some(close) = &stored.closed
            && (!identity(epoch, close)
                || !same_source(&stored.baseline, close)
                || close.physical.muted != Some(true)
                || close.physical.stamp.sequence <= stored.baseline.physical.stamp.sequence
                || close.physical.stamp.at_us < stored.baseline.physical.stamp.at_us
                || close.physical.stamp.at_us <= stored.invitation.delivered_at_us
                || close.physical.stamp.at_us > now_us)
        {
            return Err(invalid("privacy checkpoint close is invalid"));
        }
        let last = stored.closed.as_ref().unwrap_or(&stored.baseline);
        let latest_sequence = last.physical.stamp.sequence;
        let latest_at_us = last.physical.stamp.at_us;
        let phase = if stored.closed.is_some() {
            Phase::Closed
        } else {
            Phase::Armed
        };
        Ok(Self {
            stored,
            phase,
            pending_close: None,
            latest_sequence,
            latest_at_us,
        })
    }

    /// Redo/reset must erase pending evidence, including a failed temporary write.
    /// The caller must also reset its own progress/acknowledgment state, and
    /// must not resume an attempt after an erase error. After Done, persist
    /// progress keyed by id before erasing, so a restart cannot acknowledge twice.
    #[cfg(test)]
    pub(crate) fn erase(&mut self, path: &Path) -> io::Result<()> {
        self.phase = Phase::Invalid;
        self.pending_close = None;
        let next = temporary(path, self.id())?;
        remove(path)?;
        remove(&next)?;
        sync_parent(path)
    }
}

/// Durable removal of the single recovery checkpoint. A new service and a
/// verified non-physical cycle boundary use this; invitation budget is separate.
/// A failure must disable privacy onboarding for that service, never privacy itself.
pub(crate) fn discard_checkpoint(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid("privacy checkpoint path must be an absolute file"));
    }
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Start-up only: a crash mid-write can leave `<checkpoint>.next-<attempt>`
/// beside the checkpoint; no write is in flight before initialization.
pub(crate) fn discard_temporaries(path: &Path) -> io::Result<()> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(invalid("privacy checkpoint path must be an absolute file"));
    };
    let mut prefix = name.to_owned();
    prefix.push(".next-");
    let prefix = prefix.as_encoded_bytes();
    let entries = match fs::read_dir(parent) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().as_encoded_bytes().starts_with(prefix) {
            remove(&entry.path())?;
        }
    }
    Ok(())
}

fn temporary(path: &Path, attempt: BootId) -> io::Result<PathBuf> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid("privacy checkpoint path must be an absolute file"));
    }
    let mut value = path.as_os_str().to_owned();
    value.push(format!(".next-{}", crate::process::boot_text(attempt)));
    Ok(PathBuf::from(value))
}
fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
fn sync_parent(path: &Path) -> io::Result<()> {
    fs::File::open(
        path.parent()
            .ok_or_else(|| invalid("privacy checkpoint has no parent"))?,
    )?
    .sync_all()
}
fn save(path: &Path, stored: &Stored) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let next = temporary(path, stored.attempt)?;
    let bytes = serde_json::to_vec(stored).map_err(io::Error::other)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("privacy checkpoint exceeds metadata bound"));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&next, path)?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = remove(&next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::ClockStamp;
    const T: u64 = 1_000_000;
    fn epoch() -> Epoch {
        Epoch {
            device_boot: "device-boot".into(),
            clock_id: "device-clock".into(),
        }
    }
    fn sample(sequence: u64, at_us: u64, muted: bool) -> Acquisition {
        Acquisition {
            controls_session: "controls-session".into(),
            physical: PrivacySwitch {
                stamp: ClockStamp {
                    clock_id: epoch().clock_id,
                    at_us,
                    sequence,
                },
                reader_session_id: "reader".into(),
                muted: Some(muted),
                fault: None,
            },
        }
    }
    fn armed() -> Attempt {
        Attempt::arm(
            epoch(),
            Invitation {
                voice_boot: BootId::new([1; 16]).unwrap(),
                turn: 7,
                delivered_at_us: T,
            },
            sample(10, T, false),
            T,
        )
        .unwrap()
    }
    struct Store(PathBuf);
    impl Store {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "onboarding-privacy-{}",
                crate::process::boot_text(crate::process::new_boot().unwrap())
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> PathBuf {
            self.0.join("pending.json")
        }
    }
    impl Drop for Store {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn closed(store: &Store) -> Attempt {
        let mut attempt = armed();
        attempt.persist_armed(&store.file()).unwrap();
        attempt
            .observe_close(sample(11, T + 10_000, true), T + 10_000)
            .unwrap();
        attempt
            .persist_closed_after_cleanup(&store.file(), true)
            .unwrap();
        attempt
    }

    #[test]
    fn physical_close_survives_voice_cycle_and_requires_fresh_open_and_readiness() {
        let store = Store::new();
        let original = closed(&store);
        let id = original.id();
        drop(original);
        let mut restored = Attempt::load(&store.file(), &epoch(), T + 20_000)
            .unwrap()
            .unwrap();
        assert_eq!(restored.id(), id);
        assert_eq!(restored.phase(), Phase::Closed);
        assert_eq!(
            restored
                .observe_open(sample(12, T + 20_000, false), T + 20_000, false)
                .unwrap(),
            OpenOutcome::WaitingForReadiness
        );
        assert_eq!(restored.phase(), Phase::Closed);
        assert_eq!(
            restored
                .observe_open(sample(13, T + 90_000, false), T + 90_000, true)
                .unwrap(),
            OpenOutcome::Done
        );
        assert_eq!(restored.phase(), Phase::Done);
        assert!(
            restored
                .observe_open(sample(14, T + 95_000, false), T + 95_000, true)
                .is_err()
        );
    }

    #[test]
    fn a_camera_receipt_is_kept_only_with_its_close_and_survives_a_voice_cycle() {
        let store = Store::new();
        let mut attempt = armed();
        let camera = CameraClose {
            token: 42,
            asked_unix_ms: 1_760_000_000_000,
        };
        // Before a close nothing is kept.
        attempt.expect_camera(camera);
        assert_eq!(attempt.camera(), None);
        attempt
            .observe_close(sample(11, T + 10_000, true), T + 10_000)
            .unwrap();
        attempt.expect_camera(camera);
        attempt
            .persist_closed_after_cleanup(&store.file(), true)
            .unwrap();
        let restored = Attempt::load(&store.file(), &epoch(), T + 20_000)
            .unwrap()
            .unwrap();
        assert_eq!(restored.camera(), Some(camera));
        // A record from before #1802 has none and still loads.
        let old = closed(&Store::new());
        assert_eq!(old.camera(), None);
        // A receipt without a close is not a record this code writes.
        let mut forged: serde_json::Value =
            serde_json::from_slice(&fs::read(store.file()).unwrap()).unwrap();
        forged["closed"] = serde_json::Value::Null;
        fs::write(store.file(), serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(Attempt::load(&store.file(), &epoch(), T + 20_000).is_err());
    }

    #[test]
    fn close_observation_never_writes_or_recovers_as_verified_after_a_crash() {
        let store = Store::new();
        let mut attempt = armed();
        attempt.persist_armed(&store.file()).unwrap();
        let before = fs::read(store.file()).unwrap();
        attempt
            .observe_close(sample(11, T + 10_000, true), T + 10_000)
            .unwrap();
        assert_eq!(attempt.phase(), Phase::CloseObserved);
        assert_eq!(fs::read(store.file()).unwrap(), before);
        drop(attempt);
        let mut restored = Attempt::load(&store.file(), &epoch(), T + 20_000)
            .unwrap()
            .unwrap();
        assert_eq!(restored.phase(), Phase::Armed);
        assert!(
            restored
                .observe_open(sample(12, T + 20_000, false), T + 20_000, true)
                .is_err()
        );
    }

    #[test]
    fn failed_cleanup_and_persistence_cannot_promote_closed() {
        let store = Store::new();
        let mut attempt = armed();
        attempt
            .observe_close(sample(11, T + 10_000, true), T + 10_000)
            .unwrap();
        assert!(
            attempt
                .persist_closed_after_cleanup(&store.file(), false)
                .is_err()
        );
        assert_eq!(attempt.phase(), Phase::Invalid);
        assert!(!store.file().exists());
        let mut attempt = armed();
        attempt.persist_armed(&store.file()).unwrap();
        let before = fs::read(store.file()).unwrap();
        attempt
            .observe_close(sample(11, T + 10_000, true), T + 10_000)
            .unwrap();
        let next = temporary(&store.file(), attempt.id()).unwrap();
        fs::write(&next, b"existing temporary file").unwrap();
        assert!(
            attempt
                .persist_closed_after_cleanup(&store.file(), true)
                .is_err()
        );
        assert_eq!(attempt.phase(), Phase::Invalid);
        assert_eq!(fs::read(store.file()).unwrap(), before);
        assert_eq!(fs::read(&next).unwrap(), b"existing temporary file");
        assert_eq!(
            Attempt::load(&store.file(), &epoch(), T + 20_000)
                .unwrap()
                .unwrap()
                .phase(),
            Phase::Armed
        );
        assert!(
            attempt
                .observe_open(sample(12, T + 20_000, false), T + 20_000, true)
                .is_err()
        );
    }

    #[test]
    fn duplicate_stale_regressed_and_restarted_physical_sources_invalidate() {
        for case in 0..7 {
            let mut attempt = armed();
            let mut close = sample(11, T + 10_000, true);
            let mut now = T + 10_000;
            match case {
                0 => close.physical.stamp.sequence = 10,
                1 => now += lamp_msgs::PRIVACY_SWITCH_TTL_US,
                2 => close.physical.stamp.at_us = T - 1,
                3 => close.controls_session = "replacement".into(),
                4 => close.physical.reader_session_id = "replacement".into(),
                5 => close.physical.stamp.clock_id = "replacement".into(),
                _ => close.physical.muted = Some(false),
            }
            assert!(attempt.observe_close(close, now).is_err(), "case {case}");
            assert_eq!(attempt.phase(), Phase::Invalid);
        }
        let store = Store::new();
        let mut attempt = closed(&store);
        assert_eq!(
            attempt
                .observe_open(sample(12, T + 20_000, false), T + 20_000, false)
                .unwrap(),
            OpenOutcome::WaitingForReadiness
        );
        assert!(
            attempt
                .observe_open(sample(12, T + 20_000, false), T + 20_001, true)
                .is_err()
        );
    }

    #[test]
    fn fresh_close_before_delivery_and_replaced_source_on_open_are_not_proof() {
        let mut attempt = Attempt::arm(
            epoch(),
            Invitation {
                voice_boot: BootId::new([1; 16]).unwrap(),
                turn: 7,
                delivered_at_us: T,
            },
            sample(10, T - 20_000, false),
            T,
        )
        .unwrap();
        assert!(
            attempt
                .observe_close(sample(11, T - 10_000, true), T)
                .is_err()
        );
        let store = Store::new();
        let mut attempt = closed(&store);
        let mut opened = sample(12, T + 20_000, false);
        opened.physical.reader_session_id = "new-reader".into();
        assert!(attempt.observe_open(opened, T + 20_000, true).is_err());
        assert_eq!(attempt.phase(), Phase::Invalid);
    }

    #[test]
    fn expiry_is_only_evaluated_on_open_and_never_turns_into_done() {
        let store = Store::new();
        let original = closed(&store);
        drop(original);
        let late = T + ATTEMPT_US;
        let mut attempt = Attempt::load(&store.file(), &epoch(), late)
            .unwrap()
            .unwrap();
        assert_eq!(
            attempt.phase(),
            Phase::Closed,
            "time alone does not reopen or complete"
        );
        assert_eq!(
            attempt
                .observe_open(sample(12, late, false), late, true)
                .unwrap(),
            OpenOutcome::Expired
        );
        assert_eq!(attempt.phase(), Phase::Invalid);
    }

    #[test]
    fn software_mute_redo_reset_and_source_restart_erase_attempt_evidence() {
        for why in [
            Invalidation::SoftwareMute,
            Invalidation::Redo,
            Invalidation::Reset,
            Invalidation::SourceRestart,
            Invalidation::FenceFailure,
        ] {
            let store = Store::new();
            let mut attempt = closed(&store);
            let next = temporary(&store.file(), attempt.id()).unwrap();
            fs::write(&next, b"interrupted temporary write").unwrap();
            attempt.invalidate(why);
            assert!(
                attempt
                    .observe_open(sample(12, T + 20_000, false), T + 20_000, true)
                    .is_err()
            );
            attempt.erase(&store.file()).unwrap();
            assert!(!next.exists());
            assert!(
                Attempt::load(&store.file(), &epoch(), T + 20_000)
                    .unwrap()
                    .is_none()
            );
            attempt.erase(&store.file()).unwrap();
        }
    }

    #[test]
    fn recovery_rejects_device_reboot_malformed_and_oversized_records() {
        let store = Store::new();
        let _ = closed(&store);
        let mut reboot = epoch();
        reboot.device_boot = "new-boot".into();
        assert!(Attempt::load(&store.file(), &reboot, T + 20_000).is_err());
        fs::write(store.file(), b"{}").unwrap();
        assert!(Attempt::load(&store.file(), &epoch(), T + 20_000).is_err());
        fs::write(store.file(), vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(Attempt::load(&store.file(), &epoch(), T + 20_000).is_err());
    }

    #[test]
    fn attempts_are_unique_and_checkpoint_permissions_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let store = Store::new();
        let first = closed(&store);
        assert_ne!(first.id(), armed().id());
        assert_eq!(
            fs::metadata(store.file()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::metadata(store.file()).unwrap().len() <= MAX_BYTES as u64);
    }
}
