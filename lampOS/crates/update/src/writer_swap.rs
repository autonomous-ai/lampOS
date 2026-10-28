//! The settings writer refreshed from the running lamp-update (#3689, item
//! 1; docs/updates.md, "The settings writer refreshed"): the round asks,
//! and `lamp-update writer-refresh`, run as root by the writer's one-shot
//! unit (crate::writer_unit), does it.
//!
//! The round holds the records' lock for its whole run, and waits while the
//! unit runs, so the unit's program cannot take that lock itself. The round
//! hands its check over instead: under its lock, it asks
//! [`crate::writer_ready::ready`] and writes a request naming the build of
//! lamp-update it found ready ([`ask`]). The unit's program acts only on a
//! fresh request naming its own bytes ([`refresh`]). Run by hand, with no
//! round at work, it takes the lock itself and asks `ready` under it.
//!
//! The copy is placed by [`crate::fixed_copy::place`], with the copy it
//! replaces kept at [`PREV`]. A new writer is proved by its own `back`: the
//! fallback's timer runs it every 2 min, and each run that ends well is
//! noted ([`note_back`]). Once one has run since the refresh, the round asks
//! the unit to remove [`PREV`] ([`Op::Proven`]). Until then no switch of
//! lamp-update's own begins, since that `back` is its only way back
//! ([`unproven`]). A new writer whose `back` fails, or whose first `apply`
//! fails (after that apply's own undo, which it made), is put back:
//! [`PREV`] renamed over it ([`Op::PutBack`]), and that build never asked
//! to be the writer again.
use crate::{
    fixed_copy::{self, Placed},
    manifest::sha256_hex,
    own::BACK,
    records::{Lock, Records, sync_parent, write_whole},
    writer_ready,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The bytes of the lamp-update that runs, from which a round refreshes the
/// writer: shared, and shown by their SHA-256 only.
#[derive(Clone)]
pub struct Running(Arc<[u8]>);

impl Running {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes.into())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Running {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Running({})", sha256_hex(&self.0))
    }
}

/// Where the copy the refresh replaces is kept.
pub const PREV: &str = "/usr/local/lib/lampos/lamp-update-back.prev";
/// The round's request, in the records.
pub const REQUEST: &str = "writer-refresh.request";
/// What the last refresh did, in the records.
pub const DONE: &str = "writer-refresh.done";
/// The fallback's unit, which runs the writer's `back`.
pub const BACK_UNIT: &str = "lamp-update-back.service";
/// The writer's last `back` that ended well, in the records.
pub const BACK_OK: &str = "writer-back.ok";
/// How long a request stands: longer than the unit's own bound (120 s), and
/// short enough that one left by a round that stopped is not acted on later.
pub const FRESH: Duration = Duration::from_secs(5 * 60);

/// The round's request: the build of lamp-update that asked, and what for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The SHA-256 of the running lamp-update's bytes.
    pub build: String,
    pub at_unix_ms: u64,
    #[serde(default)]
    pub op: Op,
}

/// What the writer's unit is asked to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    /// The running lamp-update put at the writer's path, the copy there
    /// kept at [`PREV`].
    #[default]
    Refresh,
    /// The new writer failed: [`PREV`] renamed back over it.
    PutBack,
    /// The new writer proved itself: [`PREV`] removed.
    Proven,
}

/// What a refresh did, kept in the records for the round and a person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    /// The SHA-256 of the copy now at the writer's path.
    pub build: String,
    /// The one it replaced, none when there was none or it was already this.
    #[serde(default)]
    pub before: Option<String>,
    /// The writer's folder was synced after the rename.
    pub synced: bool,
    pub at_unix_ms: u64,
    /// Its `back` has ended well since: [`PREV`] is removed.
    #[serde(default)]
    pub proven: bool,
    /// A build of lamp-update put back as the writer: never asked to be it
    /// again.
    #[serde(default)]
    pub put_back: Option<String>,
}

/// A writer's `back` that ended well, noted by that `back` itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackOk {
    /// The SHA-256 of the writer that ran.
    pub build: String,
    pub at_unix_ms: u64,
}

/// The round's side, under its lock `held`: whether the writer is to be
/// refreshed from `running`, the running lamp-update's bytes. Ok(false)
/// when the writer already holds them. Otherwise `ready` is asked, and when
/// it answers yes a request is written for the unit's program and Ok(true)
/// returned. Why not, when not.
pub fn ask(root: &Path, held: &Lock, running: &[u8], now_unix_ms: u64) -> Result<bool, String> {
    // A build put back as the writer before is not asked again.
    let build = sha256_hex(running);
    if done(held.folder()).is_some_and(|done| done.put_back.as_ref() == Some(&build)) {
        return Ok(false);
    }
    // A lamp with no writer at all (installed before #1680) is not given
    // one: that is `install`'s.
    match fs::read(on_root(root, BACK)) {
        Ok(there) if there == running => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("{BACK}: {error}")),
    }
    writer_ready::ready(root, held, running)?;
    request(held, running, Op::Refresh, now_unix_ms)?;
    Ok(true)
}

/// The round's request for `op`, under its lock `held`, from `running`.
pub fn request(held: &Lock, running: &[u8], op: Op, now_unix_ms: u64) -> Result<(), String> {
    let request = Request {
        build: sha256_hex(running),
        at_unix_ms: now_unix_ms,
        op,
    };
    let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    write_whole(&held.folder().join(REQUEST), &bytes, 0o600)
        .map_err(|error| format!("{REQUEST}: {error}"))
}

/// Whether the writer was refreshed and has not proved itself yet: its
/// [`PREV`] is still there, and no switch of lamp-update's own may lean on
/// it.
pub fn unproven(root: &Path, records: &Path) -> bool {
    done(records)
        .is_some_and(|done| !done.proven && done.put_back.is_none() && done.before.is_some())
        && fs::symlink_metadata(on_root(root, PREV)).is_ok()
}

/// Whether the writer refreshed as `done` has had a `back` end well since.
pub fn proved(records: &Path, done: &Done) -> bool {
    back_ok(records).is_some_and(|ok| ok.build == done.build && ok.at_unix_ms > done.at_unix_ms)
}

/// Noted by the writer's own `back` when it ends well: its own bytes'
/// SHA-256 and the time.
pub fn note_back(records: &Path, own: &[u8], now_unix_ms: u64) -> io::Result<()> {
    let ok = BackOk {
        build: sha256_hex(own),
        at_unix_ms: now_unix_ms,
    };
    write_whole(
        &records.join(BACK_OK),
        &serde_json::to_vec(&ok).map_err(io::Error::other)?,
        0o600,
    )
}

fn back_ok(records: &Path) -> Option<BackOk> {
    serde_json::from_slice(&fs::read(records.join(BACK_OK)).ok()?).ok()
}

/// The unit's side, `lamp-update writer-refresh`: the running lamp-update's
/// bytes `running` placed at the writer's path under `root`, the copy there
/// kept at [`PREV`]. With the records' lock held by another (the round that
/// started the unit), only on a fresh request naming these bytes; with the
/// lock free (by hand), only once `ready` says yes under it. The request
/// is removed either way, and what was done is kept as [`DONE`].
pub fn refresh(
    root: &Path,
    records: &Path,
    running: &[u8],
    now_unix_ms: u64,
    probe: &mut dyn FnMut(&Path) -> Result<String, String>,
) -> Result<Done, String> {
    let held = Records::new(records.to_owned()).lock();
    // By hand, with no round at work, only a refresh.
    let op = match &held {
        Ok(lock) => writer_ready::ready(root, lock, running).map(|()| Op::Refresh),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            requested(records, running, now_unix_ms)
        }
        Err(error) => Err(format!("the records' lock: {error}")),
    };
    let done = op.and_then(|op| match op {
        Op::Refresh => fixed_copy::place(
            &on_root(root, BACK),
            running,
            Some(&on_root(root, PREV)),
            probe,
        )
        .map(|placed| {
            let (before, synced) = match placed {
                Placed::Already => (None, true),
                Placed::Replaced { before, synced } => (before, synced),
            };
            Done {
                build: sha256_hex(running),
                before,
                synced,
                at_unix_ms: now_unix_ms,
                proven: false,
                put_back: None,
            }
        }),
        Op::PutBack => put_back(root, now_unix_ms),
        Op::Proven => prove(root, records),
    });
    match fs::remove_file(records.join(REQUEST)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(format!("{REQUEST}: {error}"));
        }
        _ => {}
    }
    let done = done?;
    let bytes = serde_json::to_vec(&done).map_err(|error| error.to_string())?;
    write_whole(&records.join(DONE), &bytes, 0o600).map_err(|error| format!("{DONE}: {error}"))?;
    Ok(done)
}

/// [`PREV`] renamed back over the writer, which failed: the copy that ran
/// before is the writer again, and the failed one's build is kept as put
/// back.
fn put_back(root: &Path, now_unix_ms: u64) -> Result<Done, String> {
    let (writer, prev) = (on_root(root, BACK), on_root(root, PREV));
    let read = |path: &Path| fs::read(path).map_err(|error| format!("{}: {error}", path.display()));
    let (failed, kept) = (read(&writer)?, read(&prev)?);
    fs::rename(&prev, &writer).map_err(|error| format!("{}: {error}", writer.display()))?;
    Ok(Done {
        build: sha256_hex(&kept),
        before: Some(sha256_hex(&failed)),
        synced: sync_parent(&writer).is_ok(),
        at_unix_ms: now_unix_ms,
        proven: true,
        put_back: Some(sha256_hex(&failed)),
    })
}

/// [`PREV`] removed: the writer refreshed has proved itself.
fn prove(root: &Path, records: &Path) -> Result<Done, String> {
    let mut done = done(records).ok_or_else(|| format!("{DONE}: no refresh to prove"))?;
    let prev = on_root(root, PREV);
    match fs::remove_file(&prev) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(format!("{}: {error}", prev.display()));
        }
        _ => {}
    }
    done.synced = sync_parent(&prev).is_ok();
    done.proven = true;
    Ok(done)
}

/// The round's request taken back, when its unit could not run it.
pub fn forget(records: &Path) {
    let _ = fs::remove_file(records.join(REQUEST));
}

/// What the last refresh did, if one is kept and can be read.
pub fn done(records: &Path) -> Option<Done> {
    serde_json::from_slice(&fs::read(records.join(DONE)).ok()?).ok()
}

/// What a fresh request in `records` naming `running`'s bytes asks, or
/// why there is none.
fn requested(records: &Path, running: &[u8], now_unix_ms: u64) -> Result<Op, String> {
    let path = records.join(REQUEST);
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "another lamp-update holds the records' lock and asked nothing ({REQUEST}: {error})"
        )
    })?;
    let request: Request = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{REQUEST} cannot be read: {error}"))?;
    if request.build != sha256_hex(running) {
        return Err(format!(
            "{REQUEST} names build {}, and this lamp-update is another",
            request.build.get(..12).unwrap_or(&request.build)
        ));
    }
    let age = now_unix_ms.saturating_sub(request.at_unix_ms);
    if request.at_unix_ms > now_unix_ms || age > FRESH.as_millis() as u64 {
        return Err(format!(
            "{REQUEST} is not fresh: asked {} s ago",
            age / 1000
        ));
    }
    Ok(request.op)
}

fn on_root(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests;
