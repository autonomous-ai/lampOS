//! Caller-owned face observations and immutable per-request memory scope.
//! No global registry, polling, speaker inference, or session authorization.
use crate::known_faces::FaceObservation;
use lamp_turn_owner::BootId;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

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
}
impl FaceCache {
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
        });
        let Ok(mut face) = self.face.lock() else {
            return false;
        };
        if !current() {
            return false;
        }
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
        self.is_current().then_some(&self.context.scope)
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
}
