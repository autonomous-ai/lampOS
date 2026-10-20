//! Caller-owned face observations and immutable per-request memory scope.
//! No global registry, polling, speaker inference, or session authorization;
//! a lease is only ever narrowed to the room, never widened.
use crate::known_faces::FaceObservation;
use lamp_turn_owner::BootId;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Only the exact value `on`: a known face lost for a few seconds keeps its
/// memory scope through the gap ([`GAP`]), while the eyes see no face at all
/// (#2176 row 5b). Off, the scope is the room's as soon as the face's 3 s
/// freshness ends, as before.
pub const GAP_ENVIRONMENT: &str = "LAMP_FACE_GAP_HOLD";

/// How long past its last fresh sighting (the known file's 3 s freshness, so
/// about 11 s after the eyes last saw it), and past the lamp's last answer to
/// them, a known face's scope is held while the eyes see no face at all: a
/// hand across, a step out of the picture. A head turned away is usually
/// still a face in the eyes' look, so it holds nothing. On lamp-4ace on 20
/// October at 12:13 a known person lost for a moment went back to the room's
/// scope mid-conversation.
pub const GAP: Duration = Duration::from_secs(8);

/// [`GAP`] when [`GAP_ENVIRONMENT`] is `on`.
pub fn gap_from_environment() -> Option<Duration> {
    (std::env::var(GAP_ENVIRONMENT).ok().as_deref() == Some("on")).then_some(GAP)
}

/// Room is an explicit valid scope, distinct from a revoked request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeSnapshot {
    person: Option<Arc<str>>,
}
impl ScopeSnapshot {
    pub fn room() -> Self {
        Self::default()
    }
    pub fn person(&self) -> Option<&str> {
        self.person.as_deref()
    }
}

struct CachedFace {
    generation: u64,
    scope: ScopeSnapshot,
    valid_until: Instant,
    /// The end of its last fresh sighting: a held gap is measured from it,
    /// never from a hold, so it can't be renewed by holding.
    seen_until: Instant,
}

/// Acquired before reading the source. An invalidation retires all earlier
/// refresh work, even if its filesystem read completes afterwards.
pub struct RefreshToken {
    generation: u64,
}

/// The caller refreshes this slot off the audio path and owns that refresh's
/// lifetime. Snapshot and invalidation never wait on the refresh mutex.
#[derive(Default)]
pub struct FaceCache {
    generation: AtomicU64,
    face: Mutex<Option<CachedFace>>,
    /// [`GAP`], when held ([`GAP_ENVIRONMENT`]).
    gap: Option<Duration>,
    /// Whom the lamp last answered, and when ([`Self::answered`]).
    answered: Mutex<Option<(ScopeSnapshot, Instant)>>,
}
impl FaceCache {
    /// The lamp has answered a turn of `scope`'s at `at`: a gap is held only
    /// for someone known the lamp answered within it, so mid-conversation
    /// (the reviewer's P2 on #2265). Someone who has left the picture and
    /// isn't being talked with is not held for whoever speaks next.
    pub fn answered(&self, scope: &ScopeSnapshot, at: Instant) {
        if self.gap.is_some()
            && scope.person().is_some()
            && let Ok(mut answered) = self.answered.lock()
        {
            *answered = Some((scope.clone(), at));
        }
    }

    /// A cache that holds a known face's scope through a gap of up to `gap`
    /// with no face in view ([`Self::replace_seen`]); `None` holds nothing.
    pub fn holding_gaps(gap: Option<Duration>) -> Self {
        Self {
            gap,
            ..Self::default()
        }
    }

    /// Acquire before starting the source read, never after it completes.
    pub fn begin_refresh(&self) -> RefreshToken {
        RefreshToken {
            generation: self.generation.load(Ordering::Acquire),
        }
    }

    /// Revoke cached observations without waiting for I/O or a held mutex.
    /// This makes old data unusable; it does not promise immediate erasure.
    pub fn invalidate(&self) {
        let _ = self
            .generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                Some(generation.saturating_add(1))
            });
    }

    /// Only the off-path refresher calls replace. A token begun before a
    /// privacy change can never restore its observation after invalidation.
    pub fn replace(&self, token: RefreshToken, observation: Option<FaceObservation>) -> bool {
        self.replace_seen(token, observation, None)
    }

    /// [`Self::replace`], told how many faces the eyes see now
    /// (`humans/faces/tracked`; `None` when that isn't known). With no fresh
    /// known face, a holding cache ([`Self::holding_gaps`]) keeps the known
    /// face it had through a gap of up to its gap past that face's last
    /// fresh sighting, but only when the eyes looked and saw no face at all:
    /// any face in view may be someone else's, and nothing known is nothing
    /// held. A different known face replaces it at once, as before.
    pub fn replace_seen(
        &self,
        token: RefreshToken,
        observation: Option<FaceObservation>,
        faces_in_view: Option<usize>,
    ) -> bool {
        let current = || {
            token.generation != u64::MAX
                && token.generation == self.generation.load(Ordering::Acquire)
        };
        if !current() {
            return false;
        }
        let next = observation.map(|observation| CachedFace {
            generation: token.generation,
            scope: ScopeSnapshot {
                person: Some(Arc::from(crate::remember::person_id(observation.name()))),
            },
            valid_until: observation.valid_until(),
            seen_until: observation.valid_until(),
        });
        let Ok(mut face) = self.face.lock() else {
            return false;
        };
        if !current() {
            return false;
        }
        // Up to the gap past the face's last sighting and past the lamp's
        // last answer to them, whichever ends first: never someone the lamp
        // isn't talking with.
        let answered = self
            .answered
            .lock()
            .ok()
            .and_then(|answered| answered.clone());
        let held = || {
            let gap = self.gap?;
            let was = face.as_ref()?;
            let (whom, at) = answered.filter(|(whom, _)| *whom == was.scope)?;
            // Only a face cached in this generation: one from before a
            // privacy change is never carried into the new one.
            (faces_in_view == Some(0) && was.generation == token.generation).then(|| CachedFace {
                generation: was.generation,
                scope: whom,
                valid_until: (was.seen_until + gap).min(at + gap),
                seen_until: was.seen_until,
            })
        };
        let next = next.or_else(held);
        *face = next;
        // Invalidation racing the assignment is enforced by the entry's
        // generation at snapshot, and reported here when already observed.
        current()
    }

    pub fn snapshot(&self, now: Instant) -> ScopeSnapshot {
        let generation = self.generation.load(Ordering::Acquire);
        if generation == u64::MAX {
            return ScopeSnapshot::room();
        }
        let snapshot = self
            .face
            .try_lock()
            .ok()
            .and_then(|face| {
                face.as_ref()
                    .filter(|face| face.generation == generation && now < face.valid_until)
                    .map(|face| face.scope.clone())
            })
            .unwrap_or_default();
        if generation == self.generation.load(Ordering::Acquire) {
            snapshot
        } else {
            ScopeSnapshot::room()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestKey {
    pub worker: BootId,
    pub session: u64,
    pub request: u64,
}

struct RequestContext {
    key: RequestKey,
    scope: ScopeSnapshot,
    /// Set once the line is found to be someone else's
    /// ([`ScopeLease::narrow_to_room`]): the room from then on, never wider.
    narrowed: AtomicBool,
    room: ScopeSnapshot,
    active: AtomicBool,
}

/// Only this non-cloneable owner keeps the request authorized. Task leases
/// hold immutable data but cannot keep authorization alive after owner drop.
pub struct RequestScope {
    context: Arc<RequestContext>,
}
impl RequestScope {
    pub fn new(key: RequestKey, scope: ScopeSnapshot) -> Self {
        Self {
            context: Arc::new(RequestContext {
                key,
                scope,
                narrowed: AtomicBool::new(false),
                room: ScopeSnapshot::room(),
                active: AtomicBool::new(true),
            }),
        }
    }
    pub fn lease(&self) -> ScopeLease {
        ScopeLease {
            context: self.context.clone(),
        }
    }
    pub fn revoke(&self) {
        self.context.active.store(false, Ordering::Release);
    }
}
impl Drop for RequestScope {
    fn drop(&mut self) {
        self.revoke();
    }
}

#[derive(Clone)]
pub struct ScopeLease {
    context: Arc<RequestContext>,
}
impl ScopeLease {
    pub fn key(&self) -> RequestKey {
        self.context.key
    }
    pub fn is_current(&self) -> bool {
        self.context.active.load(Ordering::Acquire)
    }
    /// None means revoked, NOT room. A borrowed snapshot is not an ongoing
    /// grant: recheck is_current at each RPC/output boundary after slow work.
    pub fn scope(&self) -> Option<&ScopeSnapshot> {
        self.is_current().then(|| {
            if self.context.narrowed.load(Ordering::Acquire) {
                &self.context.room
            } else {
                &self.context.scope
            }
        })
    }
    /// The line turned out to be someone else's (a face seen speaking
    /// through it that is not the known person's, `crate::speaking_faces`):
    /// every later boundary reads the room. One way only; nothing widens a
    /// scope again.
    pub fn narrow_to_room(&self) {
        self.context.narrowed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn key(worker: u8) -> RequestKey {
        RequestKey {
            worker: BootId::new([worker; 16]).unwrap(),
            session: 1,
            request: 7,
        }
    }
    fn cached(cache: &FaceCache, name: &str, until: Instant) {
        *cache.face.lock().unwrap() = Some(CachedFace {
            generation: cache.generation.load(Ordering::Acquire),
            scope: ScopeSnapshot {
                person: Some(Arc::from(crate::remember::person_id(name))),
            },
            valid_until: until,
            seen_until: until,
        });
    }
    #[test]
    fn cache_uses_latest_face_never_sticky_session_owner_and_expires() {
        let now = Instant::now();
        let cache = FaceCache::default();
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
        cached(&cache, "A", now + Duration::from_secs(1));
        let first = cache.snapshot(now);
        cached(&cache, "B", now + Duration::from_secs(1));
        assert_eq!(
            first.person(),
            Some(crate::remember::person_id("A").as_str())
        );
        assert_eq!(
            cache.snapshot(now).person(),
            Some(crate::remember::person_id("B").as_str())
        );
        assert_eq!(
            cache.snapshot(now + Duration::from_secs(1)),
            ScopeSnapshot::room()
        );
        cache.invalidate();
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
    }
    #[test]
    fn refreshed_observation_is_hashed_off_path_and_uses_its_original_expiry() {
        let path = std::env::temp_dir().join(format!("memory-scope-face-{}", std::process::id()));
        std::fs::write(&path, "  Example Name\n").unwrap();
        let cache = FaceCache::default();
        let token = cache.begin_refresh();
        let observation = FaceObservation::read(&path).unwrap();
        let until = observation.valid_until();
        assert!(cache.replace(token, Some(observation)));
        assert_eq!(
            cache.snapshot(Instant::now()).person(),
            Some(crate::remember::person_id("Example Name").as_str())
        );
        assert_eq!(cache.snapshot(until), ScopeSnapshot::room());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn contention_is_unknown_instead_of_waiting_or_reusing_an_old_person() {
        let cache = FaceCache::default();
        let now = Instant::now();
        cached(&cache, "A", now + Duration::from_secs(1));
        let held = cache.face.lock().unwrap();
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
        drop(held);
        assert!(cache.snapshot(now).person().is_some());
    }
    #[test]
    fn invalidation_never_waits_for_a_refresh_lock_and_retires_inflight_reads() {
        let cache = FaceCache::default();
        let now = Instant::now();
        cached(&cache, "A", now + Duration::from_secs(1));
        let old = cache.begin_refresh();
        let locked = cache.face.lock().unwrap();
        cache.invalidate();
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
        drop(locked);
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
        // A stale missing-file result must not clear a newer observation.
        cached(&cache, "B", now + Duration::from_secs(1));
        assert!(!cache.replace(old, None));
        assert_eq!(
            cache.snapshot(now).person(),
            Some(crate::remember::person_id("B").as_str())
        );
        let path =
            std::env::temp_dir().join(format!("memory-scope-stale-read-{}", std::process::id()));
        std::fs::write(&path, "A").unwrap();
        let old = cache.begin_refresh();
        let observation = FaceObservation::read(&path).unwrap();
        cache.invalidate();
        assert!(!cache.replace(old, Some(observation)));
        assert_eq!(cache.snapshot(Instant::now()), ScopeSnapshot::room());
        let new = cache.begin_refresh();
        assert!(cache.replace(new, FaceObservation::read(&path)));
        assert!(cache.snapshot(Instant::now()).person().is_some());
        std::fs::remove_file(path).unwrap();
        cache.generation.store(u64::MAX, Ordering::Release);
        cache.invalidate();
        assert!(!cache.replace(cache.begin_refresh(), None));
        assert_eq!(cache.snapshot(now), ScopeSnapshot::room());
    }

    /// A line found to be someone else's narrows every lease of it to the
    /// room, those already handed out too, and nothing widens it again; a
    /// revoked one stays revoked.
    #[test]
    fn a_narrowed_lease_reads_the_room_at_every_later_boundary() {
        let person = ScopeSnapshot {
            person: Some(Arc::from("p-0123456789abcdef")),
        };
        let owner = RequestScope::new(key(1), person.clone());
        let handed = owner.lease();
        assert_eq!(handed.scope(), Some(&person));
        owner.lease().narrow_to_room();
        assert_eq!(handed.scope(), Some(&ScopeSnapshot::room()));
        assert_eq!(owner.lease().scope(), Some(&ScopeSnapshot::room()));
        handed.narrow_to_room();
        assert_eq!(handed.scope(), Some(&ScopeSnapshot::room()));
        drop(owner);
        assert!(handed.scope().is_none());
    }

    #[test]
    fn owner_drop_revokes_every_task_lease_without_revoking_a_new_request() {
        let owner = RequestScope::new(key(1), ScopeSnapshot::room());
        let first = owner.lease();
        let task = first.clone();
        assert!(task.scope().is_some_and(|scope| scope.person().is_none()));
        assert_eq!(task.key(), key(1));
        drop(owner);
        assert!(!first.is_current());
        assert!(task.scope().is_none());
        let next = RequestScope::new(key(2), ScopeSnapshot::room());
        assert!(next.lease().is_current());
        assert_ne!(task.key(), next.lease().key());
        next.revoke();
        assert!(next.lease().scope().is_none());
    }
    #[test]
    fn revocation_after_borrow_requires_a_fresh_boundary_check() {
        let owner = RequestScope::new(key(1), ScopeSnapshot::room());
        let task = owner.lease();
        let snapshot = task.scope().unwrap();
        owner.revoke();
        assert_eq!(snapshot.person(), None);
        assert!(!task.is_current());
        assert!(task.scope().is_none());
    }

    /// The known face as the eyes' file gives it, and its last fresh moment.
    fn seen(cache: &FaceCache, name: &str, faces_in_view: Option<usize>) -> Instant {
        // One file a call: the tests run at once.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "memory-scope-gap-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, name).unwrap();
        let observation = FaceObservation::read(&path).unwrap();
        let until = observation.valid_until();
        assert!(cache.replace_seen(cache.begin_refresh(), Some(observation), faces_in_view));
        std::fs::remove_file(path).unwrap();
        until
    }

    fn lost(cache: &FaceCache, faces_in_view: Option<usize>) {
        assert!(cache.replace_seen(cache.begin_refresh(), None, faces_in_view));
    }

    /// Whose a request admitted at `at` is, as the voice's memory calls read
    /// it (`ScopeLease::scope`, which `remember::run_scoped` sends with).
    fn whose(cache: &FaceCache, at: Instant) -> Option<String> {
        RequestScope::new(key(1), cache.snapshot(at))
            .lease()
            .scope()
            .and_then(ScopeSnapshot::person)
            .map(str::to_owned)
    }

    /// The lamp answered `name` at `at`.
    fn talked_with(cache: &FaceCache, name: &str, at: Instant) {
        let scope = ScopeSnapshot {
            person: Some(Arc::from(crate::remember::person_id(name))),
        };
        cache.answered(&scope, at);
    }

    /// #2176 row 5b, lamp-4ace's 12:13 case: someone known, mid-conversation,
    /// lost for a moment with no face in view, then the same line goes on.
    /// Their memory scope holds through the gap, up to `GAP` past their last
    /// fresh sighting and never renewed by holding; the room's after it.
    #[test]
    fn a_known_face_lost_with_no_face_in_view_keeps_its_scope_through_the_gap() {
        let cache = FaceCache::holding_gaps(Some(GAP));
        let known = Some(crate::remember::person_id("Example Name"));
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until);
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, until + Duration::from_secs(4)), known);
        // Held again: still measured from the last sighting.
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, until + GAP - Duration::from_millis(1)), known);
        assert_eq!(whose(&cache, until + GAP), None);
        // Seen again: theirs, fresh.
        let until = seen(&cache, "Example Name", Some(1));
        assert_eq!(whose(&cache, until - Duration::from_millis(1)), known);
    }

    /// Any face in view may be someone else's, and not knowing is not
    /// knowing that nobody is there: then nothing is held. Off, nothing is.
    /// A different known face is theirs at once, and a privacy change
    /// revokes a held face as it does a fresh one.
    #[test]
    fn a_face_in_view_or_no_look_or_the_switch_off_holds_nothing() {
        for (gap, in_view) in [(Some(GAP), Some(1)), (Some(GAP), None), (None, Some(0))] {
            let cache = FaceCache::holding_gaps(gap);
            let until = seen(&cache, "Example Name", Some(1));
            talked_with(&cache, "Example Name", until);
            lost(&cache, in_view);
            assert_eq!(whose(&cache, until), None, "{gap:?} {in_view:?}");
        }
        let cache = FaceCache::holding_gaps(Some(GAP));
        seen(&cache, "Example Name", Some(1));
        let other = seen(&cache, "Another Name", Some(1));
        assert_eq!(
            whose(&cache, other - Duration::from_millis(1)),
            Some(crate::remember::person_id("Another Name"))
        );
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until);
        lost(&cache, Some(0));
        cache.invalidate();
        assert_eq!(whose(&cache, until + Duration::from_secs(1)), None);
        // Nor does a hold after the change bring back a face from before it.
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until);
        cache.invalidate();
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, until + Duration::from_secs(1)), None);
    }

    /// The reviewer's P2 on #2265: someone known who gets up and goes, and
    /// someone else out of the picture speaking 5 s later. The gap is held
    /// only for someone the lamp answered within it: never answered, answered
    /// long ago, or answered as someone else, that line is the room's; and a
    /// hold ends the gap past the lamp's last answer to them, if that comes
    /// first.
    #[test]
    fn a_gap_is_held_only_for_someone_the_lamp_was_just_talking_with() {
        let known = Some(crate::remember::person_id("Example Name"));
        let five_on = |until: Instant| until + Duration::from_secs(5);
        let cache = FaceCache::holding_gaps(Some(GAP));
        let until = seen(&cache, "Example Name", Some(1));
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, five_on(until)), None, "never answered");
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until - Duration::from_secs(20));
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, five_on(until)), None, "answered long ago");
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Another Name", until);
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, five_on(until)), None, "answered someone else");
        // Answered 6 s before the last sighting: held 2 s past it, not 8.
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until - Duration::from_secs(6));
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, until + Duration::from_secs(1)), known);
        assert_eq!(whose(&cache, until + Duration::from_secs(2)), None);
    }
}
