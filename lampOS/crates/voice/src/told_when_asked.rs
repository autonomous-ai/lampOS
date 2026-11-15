//! A run the owner asked for by voice keeps its word in quiet hours (#7180,
//! the first builder's ruling at 03:24 on 16 Nov). On lamp-9adf a run now
//! asked at night was answered "Running … now", and then nothing: its
//! agent's answer waited behind the read gate's quiet hours, and was
//! skipped an hour later. Quiet hours stop the lamp speaking up unasked;
//! an answer the owner asked for just then is not unasked. Off unless
//! [`ENVIRONMENT`] is `on`, in `lamp-skills` and `lamp-voice` alike.
//!
//! Only a run started by the owner's own spoken ask: in `lamp-skills`, a
//! run now (`made_run`, the voice's alone, which asked whose line it was);
//! in `lamp-voice`, a skill the heard words named, the owner heard saying
//! it ([`keep`]). A trigger, the app or a schedule keeps the quiet as
//! before. Only within [`WITHIN_MS`]: an answer ready later than that
//! after the ask waits, or is skipped, as before ([`lifted_until`]), and
//! one ready in time is told only until [`WITHIN_MS`] after it was ready,
//! so a wait for the owner never speaks at one in the morning unasked.
//! Every other part of the gate stays: the owner there and alone, no voice
//! for a minute, not private or paused, the lamp's one read turn, and the
//! night's volume.
use serde_json::{Map, Value};
use std::path::Path;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_TOLD_WHEN_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// How long after the owner's spoken ask its run's answer, or its task's
/// first yes asked, still lifts the quiet: five minutes.
pub const WITHIN_MS: u64 = 5 * 60 * 1000;

/// Where the voice keeps the skills the owner asked for by voice lately,
/// each with when ([`keep`]), for the say service's task ask
/// ([`crate::skill_task_yes::asked_aloud_here`]): in the voice's own
/// runtime folder (`RuntimeDirectory=lampos-voice`, 0700), so no skill or
/// add-on can stand for an ask.
pub const SPOKEN_ASKS_FILE: &str = "/run/lampos-voice/spoken-asks.json";

/// A skill's name as asked and as kept alike: its function's `photo_booth`
/// is the skill `photo-booth`.
fn key(skill: &str) -> String {
    skill.trim().to_lowercase().replace('_', "-")
}

/// Until when a told answer ready at `ready_ms`, of a run the owner asked
/// for by voice at `asked_ms`, is told in quiet hours: [`WITHIN_MS`] after
/// it was ready, when it was ready within [`WITHIN_MS`] of the ask; none
/// later, or for a clock that went back.
pub fn lifted_until(asked_ms: u64, ready_ms: u64) -> Option<u64> {
    (ready_ms >= asked_ms && ready_ms - asked_ms <= WITHIN_MS)
        .then(|| ready_ms.saturating_add(WITHIN_MS))
}

/// Whether the quiet holds at `now_ms`: the lamp's quiet hours or the
/// skill's own (`quiet`), unless lifted until `lifted` ([`lifted_until`]).
pub fn quiet_holds(quiet: bool, lifted: Option<u64>, now_ms: u64) -> bool {
    quiet && lifted.is_none_or(|until| now_ms > until)
}

/// The owner's spoken ask of `skill` at `at_ms` kept in `path`, beside
/// those within [`WITHIN_MS`] (older ones let go). Written whole, for the
/// voice only, under one lock, so two named calls kept at once lose
/// neither.
pub fn keep(path: &Path, skill: &str, at_ms: u64) -> Result<(), String> {
    static KEEPING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = KEEPING.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut asks = read(path);
    asks.retain(|_, at| {
        at.as_u64()
            .is_some_and(|at| at <= at_ms && at_ms - at <= WITHIN_MS)
    });
    asks.insert(key(skill), Value::from(at_ms));
    crate::thread::write_file(path, &Value::Object(asks).to_string())
        .map_err(|error| format!("the owner's spoken asks: {error}"))
}

/// The asks kept in `path`: none from no file, or one that can't be read.
fn read(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 64 * 1024)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// How long the say service's ask of a task's first yes, in quiet hours,
/// waits for the owner's spoken ask of its skill to be kept ([`keep`]):
/// the voice judges whose line it was on a thread of its own, never
/// holding the named call for it, so the ask may come first.
pub const ASKED_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// [`asked_within`], looked at again every 100 ms for up to `wait`: on the
/// say service's own thread, never a turn's.
pub fn asked_within_waiting(
    path: &Path,
    skill: &str,
    now_ms: u64,
    wait: std::time::Duration,
) -> bool {
    let began = std::time::Instant::now();
    loop {
        if asked_within(path, skill, now_ms) {
            return true;
        }
        if began.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
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

    /// An answer ready within five minutes of the ask is told until five
    /// minutes after it was ready; one ready later, or before its ask by a
    /// clock gone back, is not lifted.
    #[test]
    fn an_answer_ready_within_five_minutes_of_the_ask_lifts_the_quiet() {
        assert_eq!(lifted_until(1_000, 61_000), Some(61_000 + WITHIN_MS));
        assert_eq!(
            lifted_until(1_000, 1_000 + WITHIN_MS),
            Some(1_000 + 2 * WITHIN_MS)
        );
        assert_eq!(lifted_until(1_000, 1_001 + WITHIN_MS), None);
        assert_eq!(lifted_until(61_000, 1_000), None);
    }

    /// The quiet holds unless lifted, and only until the lift's end; no
    /// quiet holds nothing whatever the lift.
    #[test]
    fn the_quiet_holds_unless_lifted_until_its_end() {
        assert!(quiet_holds(true, None, 5));
        assert!(!quiet_holds(true, Some(10), 5));
        assert!(!quiet_holds(true, Some(10), 10));
        assert!(quiet_holds(true, Some(10), 11));
        assert!(!quiet_holds(false, None, 5));
    }

    /// An ask kept is found within five minutes, by its skill's name or its
    /// function's; not after, not for another skill, and nothing from no
    /// file or one that can't be read.
    #[test]
    fn a_spoken_ask_is_found_within_five_minutes_by_its_skill() {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-told-asked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spoken-asks.json");
        assert!(!asked_within(&path, "photo-booth", 1_000));
        keep(&path, "photo_booth", 1_000).unwrap();
        keep(&path, "read-mail", 2_000).unwrap();
        assert!(asked_within(&path, "photo-booth", 1_000 + WITHIN_MS));
        assert!(!asked_within(&path, "photo-booth", 1_001 + WITHIN_MS));
        assert!(asked_within(&path, "read_mail", 3_000));
        assert!(!asked_within(&path, "desk-check", 3_000));
        // An ask kept later lets the stale one go.
        keep(&path, "desk-check", 10_000 + WITHIN_MS).unwrap();
        let kept: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(kept.as_object().unwrap().len(), 1, "{kept}");
        std::fs::write(&path, "not json").unwrap();
        assert!(!asked_within(&path, "desk-check", 10_000 + WITHIN_MS));
        keep(&path, "photo-booth", 20_000).unwrap();
        assert!(asked_within(&path, "photo-booth", 20_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two asks kept at once, from two threads, are both kept.
    #[test]
    fn two_asks_kept_at_once_are_both_kept() {
        let dir = std::env::temp_dir().join(format!("lamp-voice-told-both-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spoken-asks.json");
        for round in 0..20u64 {
            let _ = std::fs::remove_file(&path);
            let (a, b) = (path.clone(), path.clone());
            let one = std::thread::spawn(move || keep(&a, "photo-booth", 1_000 + round).unwrap());
            let two = std::thread::spawn(move || keep(&b, "desk-check", 1_000 + round).unwrap());
            one.join().unwrap();
            two.join().unwrap();
            assert!(asked_within(&path, "photo-booth", 1_000 + round));
            assert!(asked_within(&path, "desk-check", 1_000 + round));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The say service's ask waits a little for the spoken ask the voice
    /// keeps off the turn's thread: one kept 300 ms later is found; none,
    /// it gives up once the wait is over.
    #[test]
    fn a_spoken_ask_kept_a_little_later_is_waited_for() {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-told-waited-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spoken-asks.json");
        let kept = path.clone();
        let keeper = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            keep(&kept, "photo-booth", 5_000).unwrap();
        });
        let wait = std::time::Duration::from_secs(2);
        assert!(asked_within_waiting(&path, "photo-booth", 5_000, wait));
        keeper.join().unwrap();
        let began = std::time::Instant::now();
        let short = std::time::Duration::from_millis(300);
        assert!(!asked_within_waiting(&path, "desk-check", 5_000, short));
        assert!(began.elapsed() >= short);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
