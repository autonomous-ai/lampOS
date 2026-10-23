//! Caller-owned face observations and immutable per-request memory scope.
//! No global registry, polling, speaker inference, or session authorization;
//! a lease is only ever narrowed to the room, never widened.
use crate::known_faces::FaceObservation;
use lamp_turn_owner::BootId;
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Only the exact value `on`: a known face lost for a few seconds keeps its
/// memory scope through the gap ([`GAP`]), while the eyes see no face at all
/// (#2176 row 5b). Off, the scope is the room's as soon as the face's 3 s
/// freshness ends, as before.
pub const GAP_ENVIRONMENT: &str = "LAMP_FACE_GAP_HOLD";

/// How long past its last fresh sighting (the known file's 3 s freshness, so
/// about 15 s after the eyes last saw it), and past the end of the lamp's
/// last answer to them, a known face's scope is held while the eyes see no
/// face at all: a hand across, a step out of the picture. A head turned away
/// is usually still a face in the eyes' look, so it holds nothing. On
/// lamp-4ace on 20 October at 12:13 a known person lost for a moment went
/// back to the room's scope mid-conversation. 12 s, not 8: on lamp-c0a4 a
/// person who turned away and asked again took 8 to 9 s from the end of the
/// lamp's answer (the first builder on #1722, 21 Oct). One window for both
/// bounds: a face last seen as the answer ends is fresh 3 s more, so the
/// answer's 12 s end first.
pub const GAP: Duration = Duration::from_secs(12);

/// [`GAP`] when [`GAP_ENVIRONMENT`] is `on`.
pub fn gap_from_environment() -> Option<Duration> {
    (std::env::var(GAP_ENVIRONMENT).ok().as_deref() == Some("on")).then_some(GAP)
}

/// Room is an explicit valid scope, distinct from a revoked request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeSnapshot {
    person: Option<Arc<str>>,
    /// A known face's scope kept only by its held gap, not seen now
    /// (`LAMP_FACE_GAP_HOLD`): its line is answered from the conversation as
    /// it stood, never from memory's reads, and runs no held forget (the
    /// first builder's 22:10 and 01:43 on #2265).
    held: bool,
}
impl ScopeSnapshot {
    pub fn room() -> Self {
        Self::default()
    }
    pub fn person(&self) -> Option<&str> {
        self.person.as_deref()
    }
    /// Whether this scope is a known face's held only through its gap.
    pub fn held(&self) -> bool {
        self.held
    }
    /// A known person's scope, for other parts' tests.
    #[cfg(test)]
    pub fn of(name: &str) -> Self {
        Self {
            person: Some(Arc::from(crate::remember::person_id(name))),
            held: false,
        }
    }
    /// A known person's scope kept only by its held gap, for other parts'
    /// tests.
    #[cfg(test)]
    pub fn held_of(name: &str) -> Self {
        Self {
            held: true,
            ..Self::of(name)
        }
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
    /// The faces the eyes saw at the last refresh, plus one; 0 when not
    /// known. For the journal only ([`Self::gap_told`]).
    in_view: AtomicU64,
}
impl FaceCache {
    /// The lamp has answered a turn of `scope`'s at `at`: a gap is held only
    /// for someone known the lamp answered within it, so mid-conversation
    /// (the reviewer's P2 on #2265). Someone who has left the picture and
    /// isn't being talked with is not held for whoever speaks next. `at` is
    /// the answer's end (its turn complete); only a turn the lamp spoke in
    /// marks it, so a sound with no words between the answer and the next
    /// question neither starts the gap again nor ends it.
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
                person: Some(Arc::from(observation.person())),
                held: false,
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
            let (whom, at) = answered.filter(|(whom, _)| whom.person == was.scope.person)?;
            // Only a face cached in this generation: one from before a
            // privacy change is never carried into the new one.
            (faces_in_view == Some(0) && was.generation == token.generation).then(|| CachedFace {
                generation: was.generation,
                scope: ScopeSnapshot { held: true, ..whom },
                valid_until: (was.seen_until + gap).min(at + gap),
                seen_until: was.seen_until,
            })
        };
        let next = next.or_else(held);
        *face = next;
        self.in_view.store(
            faces_in_view.map_or(0, |faces| faces as u64 + 1),
            Ordering::Release,
        );
        // Invalidation racing the assignment is enforced by the entry's
        // generation at snapshot, and reported here when already observed.
        current()
    }

    /// For the journal, with a gap held ([`Self::holding_gaps`]): what a
    /// line's scope was decided on, in ms and counts, never a name. The
    /// known face's last sighting ends [`crate::known_faces`]'s freshness
    /// after the eyes' last known file; negative while it is still fresh.
    pub fn gap_told(&self, now: Instant) -> Option<String> {
        self.gap?;
        let ago = |at: Instant| match now.checked_duration_since(at) {
            Some(after) => after.as_millis() as i64,
            None => -(at.duration_since(now).as_millis() as i64),
        };
        let answered = self
            .answered
            .try_lock()
            .ok()
            .and_then(|answered| answered.as_ref().map(|(_, at)| ago(*at)))
            .map_or("never".to_owned(), |ms| format!("{ms} ms ago"));
        let in_view = match self.in_view.load(Ordering::Acquire) {
            0 => "not known".to_owned(),
            faces => (faces - 1).to_string(),
        };
        let face = self.face.try_lock().ok()?;
        let face = face.as_ref().map_or("none cached".to_owned(), |face| {
            format!(
                "{}, its sighting ends {} ms ago",
                if face.scope.held { "held" } else { "seen" },
                ago(face.seen_until)
            )
        });
        Some(format!(
            "known face {face}, the lamp's last answer to someone known {answered}, faces in view {in_view}"
        ))
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

/// A caller may use a shorter deadline, anchored to the accepted audio's end.
/// Sealing never extends it; a late transcription consumes that same window.
pub(crate) const SPEAKER_PROOF_MAX_AGE: Duration = Duration::from_secs(30);

struct SpeakerProof {
    /// A passing face under the eyes publisher's boot, not a known-person ID.
    speaker: Box<str>,
    valid_until: Instant,
}

struct RequestContext {
    key: RequestKey,
    scope: ScopeSnapshot,
    /// Set once the line is found to be someone else's
    /// ([`ScopeLease::narrow_to_room`]): the room from then on, never wider.
    narrowed: AtomicBool,
    room: ScopeSnapshot,
    active: AtomicBool,
    /// Unset until the one audio owner seals; `None` is a final refusal.
    speaker: OnceLock<Option<SpeakerProof>>,
    /// Disclosure only: 0 unproved, 1 positively matched, 2 permanently lost.
    /// This never changes speaker, scope, tool or write authority.
    private_context: AtomicU8,
    /// Initial validated v2 subject, bounded by this request's original proof.
    private_speaker: OnceLock<crate::speaking_faces::KnownSpeaker>,
}

/// Only this non-cloneable owner keeps the request authorized. Task leases
/// hold immutable data but cannot keep authorization alive after owner drop.
pub struct RequestScope {
    context: Arc<RequestContext>,
    speaker_recorder_issued: bool,
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
                speaker: OnceLock::new(),
                private_context: AtomicU8::new(0),
                private_speaker: OnceLock::new(),
            }),
            speaker_recorder_issued: false,
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

    /// Move the only writer into this request's spoken task. Read-only leases
    /// cannot issue one, and dropping it does not permit a second recorder.
    pub(crate) fn speaker_recorder(&mut self) -> Option<SpeakerRecorder> {
        if std::mem::replace(&mut self.speaker_recorder_issued, true)
            || !self.context.active.load(Ordering::Acquire)
        {
            return None;
        }
        Some(SpeakerRecorder {
            context: self.context.clone(),
        })
    }
}
impl Drop for RequestScope {
    fn drop(&mut self) {
        self.revoke();
    }
}

/// Non-cloneable, request-bound write authority. Only the spoken task that
/// accepted final audio may seal it with the actual interval's passing faces.
/// A positive identity is visual evidence of that interval, not liveness proof.
pub(crate) struct SpeakerRecorder {
    context: Arc<RequestContext>,
}

impl SpeakerRecorder {
    /// Consume the one recording attempt even for a wrong key, absent or
    /// ambiguous face, expired deadline, or revoked/held/narrowed request.
    /// `speakers_with_boot` comes from the accepted audio interval; this type
    /// does not infer a speaker, read Eyes, or authorize an operation by itself.
    pub(crate) fn seal(
        self,
        key: RequestKey,
        speakers_with_boot: &[String],
        valid_until: Instant,
    ) -> bool {
        self.seal_at(key, speakers_with_boot, valid_until, Instant::now())
    }

    fn seal_at(
        self,
        key: RequestKey,
        speakers_with_boot: &[String],
        valid_until: Instant,
        now: Instant,
    ) -> bool {
        let permitted = || {
            self.context.active.load(Ordering::Acquire)
                && !self.context.narrowed.load(Ordering::Acquire)
                && !self.context.scope.held()
        };
        let proof = match speakers_with_boot {
            [speaker]
                if key == self.context.key
                    && permitted()
                    && valid_until > now
                    && valid_until.duration_since(now) <= SPEAKER_PROOF_MAX_AGE
                    && qualified_speaker(speaker) =>
            {
                Some(SpeakerProof {
                    speaker: speaker.clone().into_boxed_str(),
                    valid_until,
                })
            }
            _ => None,
        };
        let accepted = proof.is_some();
        self.context.speaker.set(proof).is_ok() && accepted && permitted()
    }
}

/// Match the bus's 16 lowercase hex boot and the passing face reader's fN
/// contract. Keep the whole qualified ID: a new boot's f1 is a different face.
pub(crate) fn qualified_speaker(speaker: &str) -> bool {
    if !(19..=28).contains(&speaker.len()) {
        return false;
    }
    let Some((boot, face)) = speaker.split_once('/') else {
        return false;
    };
    let Some(digits) = face.strip_prefix('f') else {
        return false;
    };
    boot.len() == 16
        && boot
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (1..=10).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<u32>().is_ok_and(|id| id > 0)
}

#[derive(Clone)]
pub struct ScopeLease {
    context: Arc<RequestContext>,
}
impl ScopeLease {
    pub fn key(&self) -> RequestKey {
        self.context.key
    }
    /// Record actual private-source validation, never model arguments. Initial
    /// unproved input stays neutral; observed loss after proof cannot revive.
    pub(crate) fn private_context_permitted(&self, proved: bool) -> bool {
        let previous = self
            .context
            .private_context
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                Some(match (state, proved) {
                    (0, true) => 1,
                    (1, false) => 2,
                    _ => state,
                })
            })
            .expect("disclosure transition always updates");
        proved && previous != 2
    }

    pub(crate) fn private_context_lost(&self) -> bool {
        self.context.private_context.load(Ordering::Acquire) == 2
    }

    /// Only the actual private-source matcher may pin a validated v2 subject.
    /// No file I/O occurs under initialization; clones share this one grant.
    pub(crate) fn pin_private_speaker(
        &self,
        known: crate::speaking_faces::KnownSpeaker,
    ) -> &crate::speaking_faces::KnownSpeaker {
        self.context.private_speaker.get_or_init(|| known)
    }

    pub(crate) fn private_speaker(&self) -> Option<&crate::speaking_faces::KnownSpeaker> {
        self.context.private_speaker.get()
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
    /// The immutable original admission, distinct from a private request
    /// narrowed to room. Read-only disclosure checks must not treat narrowing
    /// as permission to disclose the room instead. Revocation grants nothing.
    pub(crate) fn admitted_scope(&self) -> Option<&ScopeSnapshot> {
        self.is_current().then_some(&self.context.scope)
    }
    /// The accepted audio's one passing face, including for a pending photo
    /// subject whose memory scope is still the room. A held or narrowed scope
    /// never qualifies. Like `scope`, a borrow is not ongoing authority: call
    /// again at the final boundary after any slow work, and compare the whole ID.
    pub(crate) fn speaker(&self) -> Option<&str> {
        self.speaker_at(Instant::now())
    }

    fn speaker_at(&self, now: Instant) -> Option<&str> {
        let proof = self.context.speaker.get()?.as_ref()?;
        (now < proof.valid_until
            && self.is_current()
            && !self.context.narrowed.load(Ordering::Acquire)
            && !self.context.scope.held())
        .then_some(proof.speaker.as_ref())
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
                held: false,
            },
            valid_until: until,
            seen_until: until,
        });
    }
    #[test]
    fn original_admission_distinguishes_room_from_narrowed_private_and_revocation() {
        let person = RequestScope::new(key(1), ScopeSnapshot::of("Synthetic owner"));
        let room = RequestScope::new(key(2), ScopeSnapshot::room());
        let private = person.lease();
        private.narrow_to_room();
        assert_eq!(private.scope(), room.lease().scope());
        assert!(private.admitted_scope().unwrap().person().is_some());
        assert_eq!(room.lease().admitted_scope(), Some(&ScopeSnapshot::room()));
        person.revoke();
        room.revoke();
        assert!(private.admitted_scope().is_none());
        assert!(room.lease().admitted_scope().is_none());
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
            held: false,
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

    const SPEAKER: &str = "0123456789abcdef/f1";
    const OTHER_BOOT_SPEAKER: &str = "fedcba9876543210/f1";

    fn speakers(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn speaker_recorder_is_issued_once_and_seals_only_its_request() {
        let now = Instant::now();
        let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
        let lease = owner.lease();
        assert_eq!(lease.speaker_at(now), None);
        let recorder = owner.speaker_recorder().unwrap();
        assert!(owner.speaker_recorder().is_none());
        // Moving the non-cloneable recorder to the spoken task does not move
        // the owner: its leases still share exactly this request's context.
        assert!(
            std::thread::spawn(move || {
                recorder.seal_at(
                    key(1),
                    &speakers(&[SPEAKER]),
                    now + Duration::from_secs(5),
                    now,
                )
            })
            .join()
            .unwrap()
        );
        assert_eq!(lease.speaker_at(now), Some(SPEAKER));
        assert_eq!(owner.lease().speaker_at(now), Some(SPEAKER));
        assert!(owner.speaker_recorder().is_none());
        // Defence in depth: even an internal duplicate cannot overwrite the
        // seal. Production cannot construct one outside this module.
        assert!(
            !SpeakerRecorder {
                context: owner.context.clone()
            }
            .seal_at(
                key(1),
                &speakers(&[OTHER_BOOT_SPEAKER]),
                now + Duration::from_secs(5),
                now,
            )
        );
        assert_eq!(lease.speaker_at(now), Some(SPEAKER));

        let mut abandoned = RequestScope::new(key(2), ScopeSnapshot::room());
        drop(abandoned.speaker_recorder().unwrap());
        assert!(abandoned.speaker_recorder().is_none());
        assert_eq!(abandoned.lease().speaker_at(now), None);
    }

    #[test]
    fn speaker_proof_rejects_wrong_worker_session_and_request_without_transfer() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(5);
        for wrong in [
            key(2),
            RequestKey {
                session: 2,
                ..key(1)
            },
            RequestKey {
                request: 8,
                ..key(1)
            },
        ] {
            let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
            assert!(!owner.speaker_recorder().unwrap().seal_at(
                wrong,
                &speakers(&[SPEAKER]),
                deadline,
                now,
            ));
            assert!(owner.speaker_recorder().is_none());
            assert_eq!(owner.lease().speaker_at(now), None);
            assert!(matches!(owner.context.speaker.get(), Some(None)));
            assert!(
                !SpeakerRecorder {
                    context: owner.context.clone()
                }
                .seal_at(key(1), &speakers(&[SPEAKER]), deadline, now,)
            );
        }
        let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
        assert!(owner.speaker_recorder().unwrap().seal_at(
            key(1),
            &speakers(&[SPEAKER]),
            deadline,
            now,
        ));
        // Even identical numeric keys are not a shared registry or transferable
        // proof. A new owner gets a new context and an unset seal.
        for other_key in [
            key(1),
            key(2),
            RequestKey {
                session: 2,
                ..key(1)
            },
            RequestKey {
                request: 8,
                ..key(1)
            },
        ] {
            let other = RequestScope::new(other_key, ScopeSnapshot::room());
            assert_eq!(other.lease().speaker_at(now), None);
            assert_eq!(owner.lease().speaker_at(now), Some(SPEAKER));
        }
    }

    #[test]
    fn speaker_proof_requires_exactly_one_well_formed_boot_qualified_face() {
        let now = Instant::now();
        let invalid = [
            vec![],
            vec![""],
            vec!["f1"],
            vec!["/f1"],
            vec!["boot/f1"],
            vec!["0123456789ABCDEF/f1"],
            vec!["0123456789abcdef/f"],
            vec!["0123456789abcdef/f12345678901"],
            vec!["0123456789abcdef/f-1"],
            vec!["0123456789abcdef/f0"],
            vec!["0123456789abcdef/f4294967296"],
            vec!["0123456789abcdef/f1/extra"],
            vec!["0123456789abcdef/p-0123456789abcdef"],
            vec!["0123456789abcdef/f1\n"],
            vec!["0123456789abcdef/f１"],
            vec![SPEAKER, SPEAKER],
            vec![SPEAKER, "0123456789abcdef/f2"],
            vec![SPEAKER, OTHER_BOOT_SPEAKER],
        ];
        for ids in invalid {
            let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
            assert!(
                !owner.speaker_recorder().unwrap().seal_at(
                    key(1),
                    &speakers(&ids),
                    now + Duration::from_secs(5),
                    now,
                ),
                "accepted {ids:?}"
            );
            assert_eq!(owner.lease().speaker_at(now), None);
            assert!(matches!(owner.context.speaker.get(), Some(None)));
        }
    }

    #[test]
    fn room_speaker_proof_keeps_old_and_new_boots_distinct_for_pending_photos() {
        let now = Instant::now();
        for id in [SPEAKER, OTHER_BOOT_SPEAKER] {
            let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
            assert!(owner.speaker_recorder().unwrap().seal_at(
                key(1),
                &speakers(&[id]),
                now + Duration::from_secs(5),
                now,
            ));
            let lease = owner.lease();
            assert_eq!(lease.scope().and_then(ScopeSnapshot::person), None);
            assert_eq!(lease.speaker_at(now), Some(id));
            assert_ne!(lease.speaker_at(now), Some("f1"));
            assert_ne!(
                lease.speaker_at(now),
                Some(if id == SPEAKER {
                    OTHER_BOOT_SPEAKER
                } else {
                    SPEAKER
                })
            );
        }
    }

    #[test]
    fn speaker_expiry_stays_anchored_to_audio_end_and_has_strict_bounds() {
        let ended = Instant::now();
        let sealed = ended + Duration::from_secs(5);
        let deadline = ended + SPEAKER_PROOF_MAX_AGE;
        let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
        assert!(owner.speaker_recorder().unwrap().seal_at(
            key(1),
            &speakers(&[SPEAKER]),
            deadline,
            sealed,
        ));
        let lease = owner.lease();
        assert_eq!(
            lease.speaker_at(deadline - Duration::from_nanos(1)),
            Some(SPEAKER)
        );
        assert_eq!(lease.speaker_at(deadline), None);
        assert_eq!(lease.speaker_at(deadline + Duration::from_secs(1)), None);
        for invalid in [
            sealed - Duration::from_nanos(1),
            sealed,
            sealed + SPEAKER_PROOF_MAX_AGE + Duration::from_nanos(1),
        ] {
            let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
            assert!(!owner.speaker_recorder().unwrap().seal_at(
                key(1),
                &speakers(&[SPEAKER]),
                invalid,
                sealed,
            ));
            assert_eq!(owner.lease().speaker_at(sealed), None);
        }
    }

    #[test]
    fn held_narrowed_and_revoked_requests_never_expose_speaker_proof() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(5);
        let mut held = RequestScope::new(
            key(1),
            ScopeSnapshot {
                person: Some(Arc::from("p-0123456789abcdef")),
                held: true,
            },
        );
        assert!(!held.speaker_recorder().unwrap().seal_at(
            key(1),
            &speakers(&[SPEAKER]),
            deadline,
            now,
        ));
        assert_eq!(held.lease().speaker_at(now), None);

        for revoke in [false, true] {
            for before_seal in [false, true] {
                let mut owner = RequestScope::new(key(1), ScopeSnapshot::of("Synthetic"));
                let lease = owner.lease();
                let recorder = owner.speaker_recorder().unwrap();
                let invalidate = || {
                    if revoke {
                        owner.revoke();
                    } else {
                        lease.narrow_to_room();
                    }
                };
                if before_seal {
                    invalidate();
                }
                assert_eq!(
                    recorder.seal_at(key(1), &speakers(&[SPEAKER]), deadline, now),
                    !before_seal
                );
                if !before_seal {
                    assert_eq!(lease.speaker_at(now), Some(SPEAKER));
                    invalidate();
                }
                assert_eq!(lease.speaker_at(now), None);
                assert_eq!(lease.clone().speaker_at(now), None);
            }
        }
        let mut revoked = RequestScope::new(key(1), ScopeSnapshot::room());
        revoked.revoke();
        assert!(revoked.speaker_recorder().is_none());
    }

    #[test]
    fn dropping_the_request_owner_revokes_unsealed_and_sealed_speaker_proofs() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(5);
        let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
        let lease = owner.lease();
        let recorder = owner.speaker_recorder().unwrap();
        drop(owner);
        assert!(!recorder.seal_at(key(1), &speakers(&[SPEAKER]), deadline, now));
        assert_eq!(lease.speaker_at(now), None);

        let mut owner = RequestScope::new(key(1), ScopeSnapshot::room());
        let lease = owner.lease();
        assert!(owner.speaker_recorder().unwrap().seal_at(
            key(1),
            &speakers(&[SPEAKER]),
            deadline,
            now,
        ));
        assert_eq!(lease.speaker_at(now), Some(SPEAKER));
        drop(owner);
        assert_eq!(lease.speaker_at(now), None);
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
            held: false,
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

    /// The first builder on #1722 (21 Oct 14:1x): lamp-c0a4's timeline,
    /// replayed. The known person is answered and looks away as the answer
    /// ends; the eyes' known file, last renewed then, stays fresh up to 3 s
    /// and then goes, with no face in view; the next line, from out of the
    /// picture, is decided 4 to 6 s after the answer. It is held, so it is
    /// told only ([`ScopeSnapshot::held`]); off, it is the room's.
    #[test]
    fn lamp_c0a4s_timeline_a_line_decided_4_to_6_s_after_the_answer_is_held() {
        let known = Some(crate::remember::person_id("Example Name"));
        for gap in [Some(GAP), None] {
            let cache = FaceCache::holding_gaps(gap);
            let until = seen(&cache, "Example Name", Some(1));
            let answer_ends = until - crate::known_faces::FRESH;
            talked_with(&cache, "Example Name", answer_ends);
            lost(&cache, Some(0));
            for decided in [4, 5, 6] {
                let at = answer_ends + Duration::from_secs(decided);
                let scope = cache.snapshot(at);
                if gap.is_some() {
                    assert!(scope.held(), "{decided} s");
                    assert_eq!(whose(&cache, at), known, "{decided} s");
                } else {
                    assert!(!scope.held());
                    assert_eq!(whose(&cache, at), None, "off: {decided} s");
                }
            }
            // The journal's account of it: ms and counts, never a name.
            let told = cache.gap_told(answer_ends + Duration::from_secs(5));
            match gap {
                Some(_) => {
                    let told = told.unwrap();
                    assert!(
                        told.starts_with("known face held, its sighting ends 2"),
                        "{told}"
                    );
                    assert!(told.ends_with("ms ago, faces in view 0"), "{told}");
                    assert!(
                        told.contains("last answer to someone known 5000 ms ago"),
                        "{told}"
                    );
                    assert!(!told.contains("Example"), "{told}");
                }
                None => assert_eq!(told, None),
            }
        }
    }

    /// The first builder on #1722 (21 Oct): on lamp-c0a4 a person who
    /// turned away and asked again took 8 to 9 s from the end of the lamp's
    /// answer, and the turn-away's sound came first as a request of no words.
    /// That request, about 1.5 s after the answer, is admitted held but ends
    /// unanswered, so it marks no one: the gap neither starts again nor ends
    /// with it. The question admitted 9 s after the answer's end is still
    /// held and theirs; off, the room's; 12 s on, the room's.
    #[test]
    fn lamp_c0a4s_question_9_s_after_the_answer_is_held_past_a_sound_of_no_words() {
        let known = Some(crate::remember::person_id("Example Name"));
        for gap in [Some(GAP), None] {
            let cache = FaceCache::holding_gaps(gap);
            let until = seen(&cache, "Example Name", Some(1));
            let answer_ends = until - crate::known_faces::FRESH;
            talked_with(&cache, "Example Name", answer_ends);
            lost(&cache, Some(0));
            // The sound with no words: admitted, never answered.
            let sound = answer_ends + Duration::from_millis(1_500);
            assert_eq!(cache.snapshot(sound).held(), gap.is_some());
            // The eyes go on looking, and still see no face.
            lost(&cache, Some(0));
            let question = answer_ends + Duration::from_secs(9);
            let scope = cache.snapshot(question);
            if gap.is_some() {
                assert!(scope.held(), "9 s after the answer's end");
                assert_eq!(whose(&cache, question), known);
                let past = answer_ends + Duration::from_secs(12);
                assert_eq!(whose(&cache, past - Duration::from_millis(1)), known);
                assert!(!cache.snapshot(past).held());
                assert_eq!(whose(&cache, past), None, "12 s on");
            } else {
                assert!(!scope.held());
                assert_eq!(whose(&cache, question), None, "off");
            }
        }
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
        // Answered 10 s before the last sighting: held 2 s past it, not 12.
        let until = seen(&cache, "Example Name", Some(1));
        talked_with(&cache, "Example Name", until - Duration::from_secs(10));
        lost(&cache, Some(0));
        assert_eq!(whose(&cache, until + Duration::from_secs(1)), known);
        assert_eq!(whose(&cache, until + Duration::from_secs(2)), None);
    }

    fn disclosure_owner(request: u8) -> RequestScope {
        let mut owner = RequestScope::new(key(request), ScopeSnapshot::of("Fixture owner"));
        let now = Instant::now();
        assert!(owner.speaker_recorder().unwrap().seal_at(
            key(request),
            &speakers(&[SPEAKER]),
            now + Duration::from_secs(5),
            now,
        ));
        owner
    }

    #[test]
    fn an_unproved_private_read_check_does_not_poison_a_later_actual_proof() {
        let owner = disclosure_owner(41);
        let lease = owner.lease();
        assert!(!lease.private_context_permitted(false));
        assert!(!lease.private_context_lost());
        assert!(!lease.clone().private_context_permitted(false));
        assert!(lease.private_context_permitted(true));
        assert!(!lease.private_context_lost());
    }

    #[test]
    fn private_read_loss_is_shared_by_clones_and_cannot_be_reproved() {
        let owner = disclosure_owner(42);
        let first = owner.lease();
        let second = first.clone();
        assert!(first.private_context_permitted(true));
        assert!(!second.private_context_permitted(false));
        assert!(first.private_context_lost());
        assert!(second.private_context_lost());
        assert!(!first.private_context_permitted(true));
        assert!(!owner.lease().private_context_permitted(true));
    }

    #[test]
    fn a_new_request_has_fresh_private_read_state_after_the_previous_lost_it() {
        let old = disclosure_owner(43);
        assert!(old.lease().private_context_permitted(true));
        assert!(!old.lease().private_context_permitted(false));
        let next = disclosure_owner(44);
        assert_ne!(old.lease().key(), next.lease().key());
        assert!(!next.lease().private_context_lost());
        assert!(next.lease().private_context_permitted(true));
        assert!(!old.lease().private_context_permitted(true));
    }

    #[test]
    fn private_read_loss_does_not_revoke_narrow_or_destroy_the_original_speaker() {
        let owner = disclosure_owner(45);
        let lease = owner.lease();
        let before = lease.scope().unwrap().clone();
        let admitted = lease.admitted_scope().unwrap().clone();
        assert!(lease.private_context_permitted(true));
        assert!(!lease.private_context_permitted(false));
        assert!(lease.private_context_lost());
        assert!(lease.is_current());
        assert_eq!(lease.scope(), Some(&before));
        assert_eq!(lease.admitted_scope(), Some(&admitted));
        assert_eq!(lease.speaker(), Some(SPEAKER));
        assert!(!lease.scope().unwrap().held());
        // Existing non-read authority stays independently revocable.
        owner.revoke();
        assert!(!lease.is_current());
        assert!(lease.scope().is_none());
        assert!(lease.speaker().is_none());
    }

    #[test]
    fn concurrent_true_and_false_after_private_proof_always_finish_lost() {
        for _ in 0..32 {
            let owner = disclosure_owner(46);
            let lease = owner.lease();
            assert!(lease.private_context_permitted(true));
            let gate = std::sync::Barrier::new(2);
            std::thread::scope(|threads| {
                let yes = &lease;
                let no = lease.clone();
                let gate = &gate;
                let proving = threads.spawn(move || {
                    gate.wait();
                    yes.private_context_permitted(true)
                });
                let losing = threads.spawn(move || {
                    gate.wait();
                    no.private_context_permitted(false)
                });
                let _ = proving.join().unwrap();
                assert!(!losing.join().unwrap());
            });
            assert!(lease.private_context_lost());
            assert!(!lease.private_context_permitted(true));
            assert!(lease.is_current());
            assert_eq!(lease.speaker(), Some(SPEAKER));
        }
    }
}
