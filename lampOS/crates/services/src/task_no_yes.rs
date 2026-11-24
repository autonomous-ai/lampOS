//! A skill's task runs on its declared grants with no folder yes, whatever
//! its source (#8475, the first builder's approved design): one switch,
//! [`ENVIRONMENT`], read alike by `lamp-control` (the mint and the check,
//! `lamp_control::task_yes`), `lamp-skills` (the hand-over asks no yes, and
//! a model's call of a skill's task runs only on the owner's named ask),
//! `lamp-services` (a run asked aloud just now sends with no per-send yes)
//! and `lamp-voice` (it tells `lamp-skills` whether a call is a named ask).
//! Off unless set: every yes as before.
//!
//! And the one reader of the voice's record of the owner's spoken asks
//! ([`SPOKEN_ASKS_FILE`], written only by the voice's
//! `told_when_asked::keep`, for a skill the heard words named in full, the
//! owner heard saying it, #7180): here, so `lamp-services` reads the very
//! record the voice writes, never another's word for it, and the voice and
//! `lamp-services` parse it with one code.
use serde_json::{Map, Value};
use std::path::Path;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_TASK_NO_YES";

thread_local! {
    /// [`on`] as a test of this thread sets it (other crates' tests too,
    /// so not only under `cfg(test)`).
    static HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    if let Some(on) = HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`on`] for this test's thread alone: tests only.
#[doc(hidden)]
pub fn set_here(on: Option<bool>) {
    HERE.with(|here| here.set(on));
}

/// How long after the owner's spoken ask it still counts as asked just
/// now: five minutes.
pub const WITHIN_MS: u64 = 5 * 60 * 1000;

/// Where the voice keeps the skills the owner asked for by voice lately,
/// each with when: in the voice's own runtime folder
/// (`RuntimeDirectory=lampos-voice`, root's, 0700; the file root's, 0600),
/// so no skill or add-on can stand for an ask. `lamp-services` (root, with
/// no capability) reads it as its owner; `lamp-skills` (lampos) can't, so
/// the voice tells it per call (#8475).
pub const SPOKEN_ASKS_FILE: &str = "/run/lampos-voice/spoken-asks.json";

/// A skill's name as asked and as kept alike: its function's `photo_booth`
/// is the skill `photo-booth`.
pub fn key(skill: &str) -> String {
    skill.trim().to_lowercase().replace('_', "-")
}

/// The asks kept in `path`: none from no file, or one that can't be read.
pub fn read(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 64 * 1024)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// Whether `path` keeps an ask of `skill` by the owner's voice within
/// [`WITHIN_MS`] before `now_ms`.
pub fn asked_within(path: &Path, skill: &str, now_ms: u64) -> bool {
    read(path)
        .get(&key(skill))
        .and_then(Value::as_u64)
        .is_some_and(|at| at <= now_ms && now_ms - at <= WITHIN_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record is read by the skill's name or its function's, within
    /// five minutes; not after, not for another skill, not one kept after
    /// now, and nothing from no file or one that isn't JSON.
    #[test]
    fn a_kept_ask_is_read_by_its_skill_within_five_minutes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spoken-asks.json");
        assert!(!asked_within(&path, "photo-booth", 1_000));
        std::fs::write(&path, r#"{"photo-booth": 1000}"#).unwrap();
        assert!(asked_within(&path, "photo_booth", 1_000 + WITHIN_MS));
        assert!(!asked_within(&path, "photo-booth", 1_001 + WITHIN_MS));
        assert!(!asked_within(&path, "desk-check", 1_000));
        assert!(!asked_within(&path, "photo-booth", 999));
        std::fs::write(&path, "not json").unwrap();
        assert!(!asked_within(&path, "photo-booth", 1_000));
    }

    /// Off unless set; a test's own thread may set it.
    #[test]
    fn the_switch_is_set_for_this_thread_alone() {
        set_here(Some(true));
        assert!(on());
        std::thread::spawn(|| {
            if std::env::var(ENVIRONMENT).is_err() {
                assert!(!on());
            }
        })
        .join()
        .unwrap();
        set_here(Some(false));
        assert!(!on());
        set_here(None);
    }
}
