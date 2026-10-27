//! Caller-owned face observations and immutable per-request memory scope.
//! No global registry, polling, speaker inference, or session authorization;
//! a lease is only ever narrowed to the room, never widened. The one late
//! look is a keep's ([`ScopeLease::keeper_within`]): a line admitted as the
//! room's whose face cache was cold at its start, or went cold before its
//! end plus [`KEEPER_WAIT`] ([`FaceCache::is_cold`]: within [`COLD_FOR`] of
//! a voice start, a privacy change, a face remembered or a change in the
//! eyes' known file, with no face named since), keeps for the known face
//! the eyes name by the line's end plus [`KEEPER_WAIT`], unless another face
//! was seen speaking it (#2807); its reads stay the room's. With the eyes'
//! view it keeps for that face only if the eyes saw it in view as the line
//! began ([`ScopeLease::face_at_end_seen`]): the known file also changes
//! when a known person walks in. Without it, only a line begun cold keeps,
//! and only for a face first named within [`REFRESH_EVERY`] of its start.
use crate::known_faces::{FaceObservation, KnownMark};
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
    /// When a refresh first named this face, of its unbroken run in this
    /// generation: a line begun cold with no eyes' view keeps for it only if
    /// this is within [`REFRESH_EVERY`] of the line's start
    /// ([`ScopeLease::face_at_end_seen`]).
    named_at: Instant,
}

/// Acquired before reading the source. An invalidation retires all earlier
/// refresh work, even if its filesystem read completes afterwards.
pub struct RefreshToken {
    generation: u64,
    /// [`FaceCache::asked`] when the refresh began (or when it read a
    /// changed known file, [`FaceCache::known_file_read`]): only a refresh
    /// begun after the cache went cold ends its coldness, by naming a face.
    asked: u64,
}

/// How long the face cache stays cold after it went cold (a voice start, a
/// privacy change, a face remembered, a change in the eyes' known file)
/// while its refreshes name nobody ([`FaceCache::is_cold`]); a refresh
/// naming a face ends it sooner. A line admitted as the room's while it is
/// cold looks later for its keeper (#2807). On lamp-c0a4 on 23 October a face
/// remembered through the eyes' own socket about 1 s after a voice restart
/// said its first line 1 to 2 s later, and the known file names a face for
/// 3 s after the eyes write it ([`crate::known_faces::FRESH`]): 5 s covers
/// both with room to spare. Past it, a cache that names nobody means nobody
/// known is in view, so a room's line has no late look (El's P2 on #3108).
/// The whole window serves only with the eyes' view, where the keeper must
/// also have been in view at the line's start; without it a line begun cold
/// keeps only for a face named within [`REFRESH_EVERY`] of its start
/// ([`ScopeLease::face_at_end_seen`]).
pub const COLD_FOR: Duration = Duration::from_secs(5);

/// The caller refreshes this slot off the audio path and owns that refresh's
/// lifetime. Snapshot and invalidation never wait on the refresh mutex.
#[derive(Default)]
pub struct FaceCache {
    camera_client: OnceLock<Arc<crate::camera_authority::Client>>,
    generation: AtomicU64,
    face: Mutex<Option<CachedFace>>,
    /// [`GAP`], when held ([`GAP_ENVIRONMENT`]).
    gap: Option<Duration>,
    /// Whom the lamp last answered, and when ([`Self::answered`]).
    answered: Mutex<Option<(ScopeSnapshot, Instant)>>,
    /// The faces the eyes saw at the last refresh, plus one; 0 when not
    /// known. For the journal only ([`Self::gap_told`]).
    in_view: AtomicU64,
    /// How many times the cache went cold: at start (0), and once more at
    /// each [`Self::went_cold`] (a privacy change, a face remembered, a
    /// change in the eyes' known file).
    asked: AtomicU64,
    /// The highest [`Self::asked`] a completed refresh naming a face began
    /// after, plus one: a face has been named since the cache last went cold
    /// when this is past `asked` ([`Self::is_cold`]).
    named_after: AtomicU64,
    /// When the cache last went cold; `None` until first stamped, cold.
    cold_at: Mutex<Option<Instant>>,
    /// The eyes' known file as the last refresh read it; `None` before the
    /// first ([`Self::known_file_read`]).
    known_mark: Mutex<Option<KnownMark>>,
}
impl FaceCache {
    /// Startup only: one bounded private-disclosure client, never on capture.
    pub(crate) fn set_camera_client(&self, client: Arc<crate::camera_authority::Client>) -> bool {
        self.camera_client.set(client).is_ok()
    }

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
            asked: self.asked.load(Ordering::Acquire),
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
        // Nothing known in the new generation until the next refresh.
        self.went_cold(Instant::now());
    }

    /// A face was just remembered by the voice's own tool: the eyes' known
    /// file may name it at any moment (#2807). A face remembered any other
    /// way is seen by the refresh ([`Self::known_file_read`]). Never waits.
    pub fn face_remembered(&self) {
        self.went_cold(Instant::now());
    }

    /// The cache goes cold at `at` (the voice's start, a privacy change, a
    /// face remembered, a change in the eyes' known file): until a refresh
    /// begun after now names a face, or [`COLD_FOR`] past `at`
    /// ([`Self::is_cold`]). Returns the new cold count. Never waits on a
    /// refresh's read.
    pub fn went_cold(&self, at: Instant) -> u64 {
        let asked = self.asked.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        if let Ok(mut cold_at) = self.cold_at.lock() {
            *cold_at = Some(cold_at.map_or(at, |was| was.max(at)));
        }
        asked
    }

    /// How many times the cache has gone cold so far: a line notes it at its
    /// start, and any rise by its end means it went cold during the line
    /// ([`ScopeLease::face_at_end`]).
    fn cold_count(&self) -> u64 {
        self.asked.load(Ordering::Acquire)
    }

    /// For tests: the cache last went cold at `at`, earlier than any
    /// [`Self::went_cold`] since (a privacy change stamps now).
    #[cfg(test)]
    pub(crate) fn cold_since(&self, at: Instant) {
        *self.cold_at.lock().unwrap() = Some(at);
    }

    /// A refresh read the eyes' known file as `mark` ([`KnownMark::read`]),
    /// at `at`, after `token` began: a change from the last refresh's mark,
    /// whoever wrote it, makes the cache cold (#2807: a face remembered
    /// through the eyes' own socket after a voice restart). The refresh read
    /// the change, so it may end that coldness by naming a face. The first
    /// read only sets the mark. Returns whether it changed.
    pub fn known_file_read(&self, token: &mut RefreshToken, mark: KnownMark, at: Instant) -> bool {
        let Ok(mut last) = self.known_mark.lock() else {
            return false;
        };
        let changed = last.as_ref().is_some_and(|last| *last != mark);
        *last = Some(mark);
        drop(last);
        if changed {
            token.asked = token.asked.max(self.went_cold(at));
        }
        changed
    }

    /// Whether the cache is cold at `now`: it went cold (its start, a
    /// privacy change, a face remembered, a change in the eyes' known file)
    /// within [`COLD_FOR`], and no refresh begun since has named a face. A
    /// refresh naming nobody inside that window does not end it. A line
    /// begun while cold, or during which the cache went cold, is the only
    /// one a late look is armed for ([`RequestScope::looking_later`]): past
    /// it, with no change during the line, a cache that names nobody means
    /// nobody known is in view (El's P2 on #3108). A cache never
    /// stamped ([`Self::went_cold`]) is cold until a face is named.
    pub fn is_cold(&self, now: Instant) -> bool {
        if self.named_after.load(Ordering::Acquire) > self.asked.load(Ordering::Acquire) {
            return false;
        }
        match self.cold_at.lock() {
            Ok(cold_at) => cold_at.is_none_or(|at| now < at + COLD_FOR),
            Err(_) => false,
        }
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
        let Ok(mut face) = self.face.lock() else {
            return false;
        };
        if !current() {
            return false;
        }
        let named_at = Instant::now();
        let next = observation.map(|observation| {
            let person: Arc<str> = Arc::from(observation.person());
            // The same face named on, unbroken in this generation, keeps
            // when it was first named.
            let named_at = face
                .as_ref()
                .filter(|was| {
                    was.generation == token.generation
                        && !was.scope.held
                        && was.scope.person.as_deref() == Some(&*person)
                })
                .map_or(named_at, |was| was.named_at);
            CachedFace {
                generation: token.generation,
                scope: ScopeSnapshot {
                    person: Some(person),
                    held: false,
                },
                valid_until: observation.valid_until(),
                seen_until: observation.valid_until(),
                named_at,
            }
        });
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
                named_at: was.named_at,
            })
        };
        let named = next.is_some();
        let next = next.or_else(held);
        *face = next;
        self.in_view.store(
            faces_in_view.map_or(0, |faces| faces as u64 + 1),
            Ordering::Release,
        );
        // Invalidation racing the assignment is enforced by the entry's
        // generation at snapshot, and reported here when already observed.
        let answered = current();
        // Only a fresh face named ends coldness ([`Self::is_cold`]); nobody,
        // or a face held only by its gap, does not.
        if answered && named {
            self.named_after
                .fetch_max(token.asked.saturating_add(1), Ordering::AcqRel);
        }
        answered
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

    /// The privacy generation now: a late look ([`RequestScope::looking_later`])
    /// reads the cache only while it is the one its line was admitted in.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn snapshot(&self, now: Instant) -> ScopeSnapshot {
        self.snapshot_locking(now, false)
    }

    /// [`Self::snapshot`], waiting for the cache's lock when `wait` (a late
    /// look's last read, so a refresh holding it then is not the room's).
    fn snapshot_locking(&self, now: Instant, wait: bool) -> ScopeSnapshot {
        self.named_locking(now, wait)
            .map(|(scope, _)| scope)
            .unwrap_or_default()
    }

    /// [`Self::snapshot_locking`] with when the cached face was first named
    /// ([`CachedFace::named_at`]); `None` for the room's.
    fn named_locking(&self, now: Instant, wait: bool) -> Option<(ScopeSnapshot, Instant)> {
        let generation = self.generation.load(Ordering::Acquire);
        if generation == u64::MAX {
            return None;
        }
        let face = if wait {
            self.face.lock().ok()
        } else {
            self.face.try_lock().ok()
        };
        let named = face.and_then(|face| {
            face.as_ref()
                .filter(|face| face.generation == generation && now < face.valid_until)
                .map(|face| (face.scope.clone(), face.named_at))
        });
        named.filter(|_| generation == self.generation.load(Ordering::Acquire))
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
    /// Disclosure only: 0 unproved, 1 matched, 2 permanently lost, 3 checking.
    /// This never changes speaker, scope, tool or write authority.
    private_context: AtomicU8,
    /// A composition must be retried if any clone observed Checking while it
    /// was built, even when another clone already resumed the same grant.
    private_context_checks: AtomicU64,
    /// Initial validated typed subject, bounded by this request's original proof.
    private_speaker: OnceLock<crate::speaking_faces::KnownSpeaker>,
    camera_client: Option<Arc<crate::camera_authority::Client>>,
    camera_binding: OnceLock<crate::camera_authority::RequestBinding>,
    /// A line admitted as the room's with the camera allowed: its late look
    /// (#2807), armed only if the face cache was cold at its start or went
    /// cold before its end plus [`KEEPER_WAIT`] ([`LateLook::armed`]).
    late: Option<LateLook>,
    /// The face cache a line with the camera allowed was admitted from, told
    /// when a face is remembered in it ([`ScopeLease::face_remembered`]).
    cache: Option<Arc<FaceCache>>,
}

/// A keep's late look (#2807): the face cache and its generation when the
/// line was admitted, when that was, whether it was cold then and how many
/// times it had gone cold, and the keeper decided at the line's end
/// ([`ScopeLease::record_keeper`]), once.
struct LateLook {
    cache: Arc<FaceCache>,
    generation: u64,
    started: Instant,
    cold_at_start: bool,
    cold_count_at_start: u64,
    keeper: OnceLock<Option<String>>,
}
impl LateLook {
    /// Armed: the cache was cold at the line's start, or has gone cold
    /// since (#2807 on lamp-c0a4 at 05:13: the eyes' known file changed
    /// mid-line, after a warm start). A warm start with no cold event since
    /// arms nothing (El's P2 on #3108). Armed only since, its keeper must
    /// also have been in view as the line began
    /// ([`ScopeLease::face_at_end_seen`]).
    fn armed(&self) -> bool {
        self.cold_at_start || self.cache.cold_count() > self.cold_count_at_start
    }
}

/// Whether the eyes had a person's face in view at a time
/// (`speaking_faces::known_in_view_since`), when their view is there.
pub type InView<'a> = &'a dyn Fn(&str, Instant) -> bool;

/// How long past a line's end, at most, a line admitted as the room's waits
/// for the eyes to name a known face as its keeper (#2807): one refresh of
/// the face cache (200 ms) and a little. A face first named later is never
/// the keeper. A keep also waits at most this long for that decision.
pub const KEEPER_WAIT: Duration = Duration::from_millis(300);
/// How often the second voice refreshes the face cache from the eyes' known
/// file. Without the eyes' view, a line begun while the cache is cold keeps
/// for a known face only if it was named within this of the line's start
/// ([`ScopeLease::face_at_end_seen`]): a keep going to the wrong person's
/// memory is the worse failure (the first builder on #3108, 06:50).
pub const REFRESH_EVERY: Duration = Duration::from_millis(200);
/// How often that wait looks at the cache.
const KEEPER_POLL: Duration = Duration::from_millis(50);

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
                private_context_checks: AtomicU64::new(0),
                private_speaker: OnceLock::new(),
                camera_client: None,
                camera_binding: OnceLock::new(),
                late: None,
                cache: None,
            }),
            speaker_recorder_issued: false,
        }
    }
    /// Same ordinary admission, carrying only the startup disclosure client.
    /// This must not arm late-look/cache keeper authority when that mode is off.
    pub(crate) fn in_cache(key: RequestKey, scope: ScopeSnapshot, cache: &FaceCache) -> Self {
        let mut request = Self::new(key, scope);
        Arc::get_mut(&mut request.context)
            .expect("new request has one context owner")
            .camera_client = cache.camera_client.get().cloned();
        request
    }
    /// A line admitted while the camera was allowed. Only one admitted as
    /// the room's while `cache` is cold ([`FaceCache::is_cold`]: within
    /// [`COLD_FOR`] of a voice restart, a privacy change, a face remembered
    /// or a change in the eyes' known file, with no face named since), or
    /// during which `cache` goes cold before its end plus [`KEEPER_WAIT`]
    /// (decided at the end, [`ScopeLease::face_at_end`]), looks again for
    /// its keeps ([`ScopeLease::keeper_within`]): a warm cache that named
    /// nobody and saw no change through the line means nobody known was in
    /// view, and a known face first cached during it never takes the keep of
    /// a line someone off camera said (El's P2 on #3108). A person's or a
    /// held scope takes no late look, nor does anything once `cache`'s
    /// generation moves on (a privacy change).
    pub fn looking_later(key: RequestKey, scope: ScopeSnapshot, cache: Arc<FaceCache>) -> Self {
        let late = (scope.person.is_none() && !scope.held).then(|| {
            let cache = cache.clone();
            let generation = cache.generation();
            // The count first: a cold event between the two reads is seen
            // by one of them.
            let cold_count_at_start = cache.cold_count();
            let started = Instant::now();
            let cold_at_start = cache.is_cold(started);
            LateLook {
                cache,
                generation,
                started,
                cold_at_start,
                cold_count_at_start,
                keeper: OnceLock::new(),
            }
        });
        Self {
            context: Arc::new(RequestContext {
                key,
                scope,
                narrowed: AtomicBool::new(false),
                room: ScopeSnapshot::room(),
                active: AtomicBool::new(true),
                speaker: OnceLock::new(),
                private_context: AtomicU8::new(0),
                private_context_checks: AtomicU64::new(0),
                private_speaker: OnceLock::new(),
                camera_client: cache.camera_client.get().cloned(),
                camera_binding: OnceLock::new(),
                late,
                cache: Some(cache),
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
        crate::speaking_faces::cancel_enrollment(self.context.key.request);
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
    pub(crate) fn camera_client(&self) -> Option<&Arc<crate::camera_authority::Client>> {
        self.context.camera_client.as_ref()
    }
    pub(crate) fn camera_binding(&self) -> Option<&crate::camera_authority::RequestBinding> {
        self.context.camera_binding.get()
    }
    pub(crate) fn pin_camera_binding(
        &self,
        binding: crate::camera_authority::RequestBinding,
    ) -> &crate::camera_authority::RequestBinding {
        self.context.camera_binding.get_or_init(|| binding)
    }
    pub(crate) fn speaker_deadline(&self) -> Option<Instant> {
        self.speaker()?;
        Some(self.context.speaker.get()?.as_ref()?.valid_until)
    }
    /// Initial ticket acquisition is unresolved disclosure, never readiness.
    /// Increment the same composition counter before publishing Pending.
    pub(crate) fn pending_camera_context(&self) -> bool {
        if self
            .context
            .private_context_checks
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .is_err()
        {
            self.private_context_permitted(false);
            return false;
        }
        self.context
            .private_context
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, 0 | 1 | 3).then_some(3)
            })
            .is_ok()
    }

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
                    (0 | 3, true) => 1,
                    (1 | 3, false) => 2,
                    _ => state,
                })
            })
            .expect("disclosure transition always updates");
        proved && previous != 2
    }

    /// Retire disclosure before an admitted ordinary face mutation runs.
    /// Only its later fixed public receipt may use the separate request guard.
    /// Only a still-proved private request can transfer; an observed loss or
    /// unresolved/unproved state wins instead. Private disclosure stays Lost.
    pub(crate) fn retire_private_for_face_receipt(&self) -> bool {
        self.context
            .private_context
            .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn private_context_lost(&self) -> bool {
        self.context.private_context.load(Ordering::Acquire) == 2
    }

    /// A fresh matching Checking publication can suspend only an already
    /// validated typed grant. It grants no disclosure and never resets the audio
    /// proof's deadline; a concurrent terminal loss stays terminal.
    pub(crate) fn pause_private_context(&self) -> bool {
        if self.context.private_speaker.get().is_none() {
            return false;
        }
        // Publish the observation before Pending can be visible. A failed
        // pause may cause a conservative rebuild, never a missed observation.
        if self
            .context
            .private_context_checks
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |seen| {
                seen.checked_add(1)
            })
            .is_err()
        {
            self.private_context_permitted(false);
            return false;
        }
        self.context
            .private_context
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                matches!(state, 1 | 3).then_some(3)
            })
            .is_ok()
    }

    pub(crate) fn private_context_checks(&self) -> u64 {
        self.context.private_context_checks.load(Ordering::Acquire)
    }

    pub(crate) fn private_context_waiting(&self) -> bool {
        self.context.private_context.load(Ordering::Acquire) == 3
    }

    /// Only the actual private-source matcher may pin a validated typed subject.
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

    /// A face was remembered in this line: the face cache it was admitted
    /// from has not answered for it yet ([`FaceCache::face_remembered`]).
    pub fn face_remembered(&self) {
        if let Some(cache) = &self.context.cache {
            cache.face_remembered();
        }
    }

    /// A line admitted as the room's with the camera allowed
    /// ([`RequestScope::looking_later`]): whether its late look is armed is
    /// decided at its end ([`Self::face_at_end`], [`Self::record_keeper`]).
    pub fn may_look_later(&self) -> bool {
        self.context.late.is_some()
    }

    /// [`Self::may_look_later`], armed as of now: the face cache was cold at
    /// the line's start, or has gone cold since.
    pub fn looks_later(&self) -> bool {
        self.context.late.as_ref().is_some_and(LateLook::armed)
    }

    /// Still a late look that may name a keeper: current, not narrowed, and
    /// the cache's privacy generation the one the line was admitted in.
    fn late_open(&self) -> Option<&LateLook> {
        let late = self.context.late.as_ref()?;
        (self.is_current()
            && !self.context.narrowed.load(Ordering::Acquire)
            && late.cache.generation() == late.generation)
            .then_some(late)
    }

    /// The known face the face cache names as of the line's end (`ended`,
    /// when its audio ended; started then, not once its words are back),
    /// waiting until `ended` + [`KEEPER_WAIT`]
    /// at most (#2807). Blocking. Armed only if the cache was cold at the
    /// line's start or went cold by then ([`LateLook::armed`]): on lamp-c0a4
    /// the eyes' known file changed mid-line, after a warm start. `None` for
    /// a line not looking later, one never armed by then, a person's or
    /// narrowed line, a revoked one, a privacy change since admission, a
    /// face held only by its gap, or nobody known by then: a face first
    /// named past that bound is never the keeper, so one walking in while
    /// the lamp answers never takes the keep. The caller still checks who
    /// was seen speaking the line before recording it. Without the eyes'
    /// view ([`Self::face_at_end_seen`]), a line armed only by going cold
    /// during it names nobody, and one begun cold names only a face first
    /// named within [`REFRESH_EVERY`] of its start.
    pub fn face_at_end(&self, ended: Instant) -> Option<String> {
        self.face_at_end_seen(ended, None)
    }

    /// [`Self::face_at_end`], where `in_view(person, started)` says whether
    /// `person`'s face was in view as the line was admitted (`started`):
    /// the eyes' passing id beside their known file, tracked then
    /// (`LAMP_EYES_SPEAKING=on`, [`crate::speaking_faces::SpeakingFaces::known_in_view_since`]).
    /// With the eyes' view, an armed line, whether begun cold or gone cold
    /// during it (the eyes' known file also changes when a known person
    /// walks in), keeps only for a face in view at its start: on lamp-c0a4
    /// A's face was in view, unknown, before the line and named ~2 s into
    /// it, and after a restart A's face is tracked before their first line;
    /// a known person walking in while someone off camera speaks never
    /// takes the keep (El's and reviewer-three's P2s on #3108). `None` for
    /// `in_view` (the eyes' speaking off): a line begun cold keeps only for
    /// a face first named within [`REFRESH_EVERY`] of its start, and one
    /// armed only during it keeps for nobody (the first builder's 06:50 on
    /// #3108: the wrong person's keep is the worse failure).
    pub fn face_at_end_seen(&self, ended: Instant, in_view: Option<InView<'_>>) -> Option<String> {
        let until = ended + KEEPER_WAIT;
        loop {
            let late = self.late_open()?;
            let now = Instant::now();
            let last = now >= until;
            // The last read waits for the cache's lock: a refresh holding
            // it then does not make the line the room's (Eighth's P3).
            let named = late.cache.named_locking(now, last);
            if late.armed()
                && let Some((seen, named_at)) = &named
                && !seen.held
                && let Some(person) = seen.person()
                && now <= until
                && match in_view {
                    // With the eyes' view: in view at the line's start,
                    // whether it began cold or went cold during it.
                    Some(in_view) => in_view(person, late.started),
                    // Without it: begun cold, and named within one refresh
                    // of its start.
                    None => late.cold_at_start && *named_at <= late.started + REFRESH_EVERY,
                }
            {
                return Some(person.to_owned());
            }
            if last {
                return None;
            }
            std::thread::sleep((until - now).min(KEEPER_POLL));
        }
    }

    /// The keeper decided at the line's end: [`Self::face_at_end`], left as
    /// `None` when another face was seen speaking the line. Only the first
    /// record counts; a line not looking later records nothing.
    pub fn record_keeper(&self, keeper: Option<String>) {
        if let Some(late) = self.context.late.as_ref() {
            let _ = late.keeper.set(keeper);
        }
    }

    /// Whose a keep in this line is (#2807): the keeper recorded at the
    /// line's end ([`Self::record_keeper`]), waiting up to `wait` for that
    /// record, but only for a line admitted as the room's whose face cache
    /// was cold at its start or went cold before its end plus
    /// [`KEEPER_WAIT`] ([`RequestScope::looking_later`]): after a voice
    /// restart, or a face just remembered, the first line was admitted
    /// before the eyes named them and its keep went to the room. `None`,
    /// and the keep is the lease's own (the room's), for a person's line, a
    /// line never armed (a warm cache, no cold event through it), one whose
    /// named face the eyes did not see in view as it began, or, without the
    /// eyes' view, one armed only during it or whose face was first named
    /// past [`REFRESH_EVERY`] from its start ([`Self::face_at_end_seen`]), a revoked one, a privacy change since admission, a face held only by
    /// its gap, another face seen speaking the line, nobody known by its end
    /// plus [`KEEPER_WAIT`], or no record by then. Only a keep reads this;
    /// every read stays the room's.
    pub fn keeper_within(&self, wait: Duration) -> Option<String> {
        self.context.late.as_ref()?;
        let until = Instant::now() + wait;
        loop {
            let late = self.late_open()?;
            if let Some(keeper) = late.keeper.get() {
                return keeper.clone();
            }
            let left = until.checked_duration_since(Instant::now())?;
            if left.is_zero() {
                return None;
            }
            std::thread::sleep(left.min(KEEPER_POLL));
        }
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
        cached_named(cache, name, until, Instant::now());
    }
    /// [`cached`], first named at `named_at`: a line's keeper without the
    /// eyes' view must be named within [`REFRESH_EVERY`] of the line's start,
    /// so a test naming one "at the start" takes that start from the line
    /// ([`line_start`]), not the real clock a loaded runner may stall past.
    fn cached_named(cache: &FaceCache, name: &str, until: Instant, named_at: Instant) {
        *cache.face.lock().unwrap() = Some(CachedFace {
            generation: cache.generation.load(Ordering::Acquire),
            scope: ScopeSnapshot {
                person: Some(Arc::from(crate::remember::person_id(name))),
                held: false,
            },
            valid_until: until,
            seen_until: until,
            named_at,
        });
    }
    /// When `lease`'s line began, as the line itself took it
    /// ([`RequestScope::looking_later`]).
    fn line_start(lease: &ScopeLease) -> Instant {
        lease
            .context
            .late
            .as_ref()
            .expect("a line looking later")
            .started
    }
    /// The face a refresh just named was first named at `at`, the line's
    /// start ([`line_start`]): the same naming however long the runner
    /// stalled before the refresh ran.
    fn first_named_at(cache: &FaceCache, at: Instant) {
        cache
            .face
            .lock()
            .unwrap()
            .as_mut()
            .expect("a face cached")
            .named_at = at;
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

    /// The late keeper chooses only where a Room line keeps something. It
    /// cannot manufacture the separate, single-use audio proof or turn that
    /// original Room admission into authority to disclose a person's context.
    #[test]
    fn looking_later_keeps_one_recorder_and_never_upgrades_room_disclosure() {
        let mut directory = crate::process::SessionDirectory::create().unwrap();
        directory.path = directory.path.canonicalize().unwrap();
        let known = directory.path.join("known");
        let name = "Fixture owner";
        let person = crate::remember::person_id(name);
        std::fs::write(&known, name).unwrap();
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&known),
            serde_json::json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"seen", "name":name, "id":null,
                "boot":"0123456789abcdef", "face":"f1",
                "publisher":"0123456789abcdef0123456789abcdef", "generation":1,
                "observed_us":lamp_ipc::monotonic_us(),
                "permission_until_us":lamp_ipc::monotonic_us().checked_add(1_500_000).unwrap()})
            .to_string(),
        )
        .unwrap();
        let name_told = |lease: &ScopeLease| {
            crate::second_provider::worker::known_name_told(true, Some(&known), Some(lease))
        };
        let fixture = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let cache = fixture.cache();
        // Cache only the hold FD; every tested Room/private request below
        // still seals and matches its own original authority independently.
        let mut warm = RequestScope::in_cache(key(66), ScopeSnapshot::of(name), &cache);
        let warm_key = warm.lease().key();
        assert!(warm.speaker_recorder().unwrap().seal(
            warm_key,
            &speakers(&[SPEAKER]),
            Instant::now() + Duration::from_secs(5)
        ));
        assert_eq!(
            fixture.ready(&warm.lease()),
            crate::second_provider::PermissionState::Ready
        );
        let mut room = RequestScope::looking_later(key(61), ScopeSnapshot::room(), cache.clone());
        let lease = room.lease();
        let admitted = lease.admitted_scope().unwrap().clone();
        let started = room.context.late.as_ref().unwrap().started;
        let recorder = room.speaker_recorder().unwrap();
        assert!(room.speaker_recorder().is_none());
        assert!(name_told(&lease).is_empty(), "no audio proof yet");
        assert!(lease.speaker().is_none());
        cached(&cache, name, Instant::now() + Duration::from_secs(5));
        // The keeper's caller supplies visibility, not positive speech. Pin
        // this fixture to the exact admitted subject and original line start.
        let in_view = |seen: &str, at: Instant| seen == person && at == started;
        lease.record_keeper(lease.face_at_end_seen(Instant::now(), Some(&in_view)));
        assert_eq!(lease.keeper_within(Duration::ZERO), Some(person.clone()));
        assert_eq!(lease.admitted_scope(), Some(&admitted));
        assert_eq!(lease.scope(), Some(&ScopeSnapshot::room()));
        assert!(
            lease.speaker().is_none(),
            "a selected keeper is not a speaker"
        );
        assert!(lease.private_speaker().is_none());
        assert!(!lease.private_context_lost());
        assert!(name_told(&lease).is_empty());
        let now = Instant::now();
        assert!(recorder.seal_at(
            key(61),
            &speakers(&[SPEAKER]),
            now + Duration::from_secs(5),
            now
        ));
        assert_eq!(lease.speaker_at(now), Some(SPEAKER));
        assert!(room.speaker_recorder().is_none());
        assert!(
            name_told(&lease).is_empty(),
            "positive audio cannot upgrade the original Room admission"
        );
        assert_eq!(lease.admitted_scope(), Some(&admitted));
        assert!(
            lease.private_speaker().is_none(),
            "no private grant was pinned"
        );

        // The same constructor also admits an already known person. Only
        // its actual sole-positive seal, not visibility or the late keeper,
        // makes the real private-source matcher accept that request.
        for (index, ids, positive) in [
            (62, speakers(&[]), false),
            (63, speakers(&[SPEAKER, OTHER_BOOT_SPEAKER]), false),
            (64, speakers(&[SPEAKER]), true),
        ] {
            let private = ScopeSnapshot::of(name);
            let mut owner = RequestScope::looking_later(key(index), private.clone(), cache.clone());
            let lease = owner.lease();
            assert!(
                !lease.may_look_later(),
                "a personal admission takes no late look"
            );
            assert!(name_told(&lease).is_empty());
            let recorder = owner.speaker_recorder().unwrap();
            assert!(owner.speaker_recorder().is_none());
            let now = Instant::now();
            assert_eq!(
                recorder.seal_at(key(index), &ids, now + Duration::from_secs(5), now),
                positive
            );
            assert_eq!(lease.speaker_at(now).is_some(), positive);
            assert_eq!(!name_told(&lease).is_empty(), positive);
            assert_eq!(lease.private_speaker().is_some(), positive);
            assert_eq!(lease.admitted_scope(), Some(&private));
            assert_eq!(lease.scope(), Some(&private));
            assert!(owner.speaker_recorder().is_none());
        }
        drop(warm);
        drop(room);
        drop(cache);
        drop(fixture);
        std::fs::remove_dir_all(&directory.path).unwrap();
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

    #[test]
    fn checking_can_only_pause_a_pinned_grant_and_never_wins_over_loss() {
        for _ in 0..32 {
            let owner = disclosure_owner(47);
            let lease = owner.lease();
            assert!(!lease.pause_private_context(), "unproved cannot wait");
            assert!(lease.private_context_permitted(true));
            assert!(!lease.pause_private_context(), "legacy proof cannot wait");
            lease.pin_private_speaker(crate::speaking_faces::KnownSpeaker {
                name: "Fixture owner".into(),
                person: crate::remember::person_id("Fixture owner"),
                id: None,
                face: SPEAKER.into(),
                epoch: Some(crate::speaking_faces::KnownSpeakerEpoch {
                    boot: "0123456789abcdef".into(),
                    publisher: "0123456789abcdef0123456789abcdef".into(),
                    generation: 1,
                }),
            });
            assert!(lease.pause_private_context());
            assert!(lease.clone().private_context_waiting());
            assert!(!lease.private_context_lost());
            assert!(lease.private_context_permitted(true));
            assert!(!lease.private_context_waiting());
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|threads| {
                let waiting = threads.spawn(|| {
                    barrier.wait();
                    lease.pause_private_context()
                });
                let losing = threads.spawn(|| {
                    barrier.wait();
                    lease.private_context_permitted(false)
                });
                let _ = waiting.join().unwrap();
                assert!(!losing.join().unwrap());
            });
            assert!(lease.private_context_lost());
            assert!(!lease.private_context_waiting());
            assert!(!lease.pause_private_context());
            assert!(!lease.private_context_permitted(true));
            assert!(lease.is_current());
            assert_eq!(lease.speaker(), Some(SPEAKER));
        }
    }

    /// #2807: right after a voice restart, a remembered face's first line
    /// was admitted before the eyes' first answer, as the room's. Its keep
    /// looks again: the known face named by then, or within the wait, is
    /// the keeper. Its reads stay the room's.
    #[test]
    fn a_keep_in_a_line_admitted_before_the_eyes_answered_goes_to_their_face() {
        let cache = Arc::new(FaceCache::default());
        let owner =
            RequestScope::looking_later(key(1), cache.snapshot(Instant::now()), cache.clone());
        let lease = owner.lease();
        assert_eq!(lease.scope(), Some(&ScopeSnapshot::room()));
        cached_named(
            &cache,
            "Ann",
            Instant::now() + Duration::from_secs(3),
            line_start(&lease),
        );
        ended(&lease);
        let ann = crate::remember::person_id("Ann");
        assert_eq!(lease.keeper_within(Duration::ZERO), Some(ann.clone()));
        assert_eq!(
            lease.scope(),
            Some(&ScopeSnapshot::room()),
            "reads stay the room's"
        );

        // The eyes answer during the wait.
        let cache = Arc::new(FaceCache::default());
        let owner = RequestScope::looking_later(key(2), ScopeSnapshot::room(), cache.clone());
        let lease = owner.lease();
        let later = {
            let cache = cache.clone();
            let start = line_start(&lease);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                // Named 60 ms in, within one refresh of the start, however
                // late a loaded runner gets here.
                cached_named(
                    &cache,
                    "Ann",
                    Instant::now() + Duration::from_secs(3),
                    start + Duration::from_millis(60),
                );
            })
        };
        ended(&lease);
        assert_eq!(lease.keeper_within(KEEPER_WAIT), Some(ann));
        later.join().unwrap();
    }

    /// The line's end, as the second voice decides it: the known face named
    /// by then plus [`KEEPER_WAIT`], recorded for the keep.
    fn ended(lease: &ScopeLease) {
        lease.record_keeper(lease.face_at_end(Instant::now()));
    }

    /// Eighth's P2 on #3108: the keeper is decided as of the line's end, not
    /// at the keep. A known face first named past the line's end plus
    /// [`KEEPER_WAIT`] (walking in while the lamp answers) never takes it.
    #[test]
    fn a_face_first_named_after_the_lines_end_and_the_wait_is_not_the_keeper() {
        // Nobody by the line's end and the wait; the face comes at the keep.
        let cache = Arc::new(FaceCache::default());
        let owner = RequestScope::looking_later(key(1), ScopeSnapshot::room(), cache.clone());
        let lease = owner.lease();
        let started = Instant::now();
        ended(&lease);
        assert!(started.elapsed() >= KEEPER_WAIT, "waited the bound");
        assert!(
            started.elapsed() < KEEPER_WAIT + Duration::from_millis(300),
            "bounded"
        );
        cached(&cache, "Ann", Instant::now() + Duration::from_secs(3));
        assert_eq!(lease.keeper_within(KEEPER_WAIT), None);

        // Decided late: a face cached now, past a line that ended earlier
        // than the bound, is not taken as the line's.
        let cache = Arc::new(FaceCache::default());
        let owner = RequestScope::looking_later(key(2), ScopeSnapshot::room(), cache.clone());
        let lease = owner.lease();
        cached(&cache, "Ann", Instant::now() + Duration::from_secs(3));
        let line_end = Instant::now() - KEEPER_WAIT - Duration::from_millis(100);
        assert_eq!(lease.face_at_end(line_end), None);

        // Recorded once: a later record does not replace the line's keeper.
        lease.record_keeper(None);
        lease.record_keeper(Some(crate::remember::person_id("Ann")));
        assert_eq!(lease.keeper_within(Duration::ZERO), None);
    }

    /// Nobody known by the end of the wait, a line narrowed to the room, a
    /// privacy change since admission, a revoked line, a face held only by
    /// its gap, a person's line or a plain room lease: no keeper, and the
    /// keep is the lease's own.
    #[test]
    fn a_keep_looks_again_only_for_a_room_line_admitted_with_nobody_known_yet() {
        let started = Instant::now();
        let cache = Arc::new(FaceCache::default());
        let nobody = RequestScope::looking_later(key(1), ScopeSnapshot::room(), cache.clone());
        assert_eq!(
            nobody.lease().keeper_within(Duration::from_millis(120)),
            None
        );
        assert!(started.elapsed() < Duration::from_millis(600), "bounded");

        let narrowed = RequestScope::looking_later(key(2), ScopeSnapshot::room(), cache.clone());
        narrowed.lease().narrow_to_room();
        let changed = RequestScope::looking_later(key(3), ScopeSnapshot::room(), cache.clone());
        let revoked = RequestScope::looking_later(key(4), ScopeSnapshot::room(), cache.clone());
        revoked.revoke();
        let held = RequestScope::looking_later(
            key(5),
            ScopeSnapshot {
                person: Some(Arc::from("p-0123456789abcdef")),
                held: true,
            },
            cache.clone(),
        );
        let theirs = RequestScope::looking_later(key(6), ScopeSnapshot::of("Bob"), cache.clone());
        let plain = RequestScope::new(key(7), ScopeSnapshot::room());
        cached_named(
            &cache,
            "Ann",
            Instant::now() + Duration::from_secs(3),
            line_start(&changed.lease()),
        );
        for (what, owner) in [
            ("narrowed", &narrowed),
            ("revoked", &revoked),
            ("held", &held),
            ("a person's", &theirs),
            ("plain", &plain),
        ] {
            ended(&owner.lease());
            assert_eq!(owner.lease().keeper_within(Duration::ZERO), None, "{what}");
        }
        ended(&changed.lease());
        assert!(changed.lease().keeper_within(Duration::ZERO).is_some());
        // A privacy change since admission: never, whatever is cached after.
        cache.invalidate();
        cached(&cache, "Ann", Instant::now() + Duration::from_secs(3));
        assert_eq!(changed.lease().keeper_within(Duration::ZERO), None);
        let after = RequestScope::looking_later(key(9), ScopeSnapshot::room(), cache.clone());
        ended(&after.lease());
        assert!(after.lease().keeper_within(Duration::ZERO).is_some());

        // A face held only by its gap names no keeper either.
        let cache = Arc::new(FaceCache::default());
        let line = RequestScope::looking_later(key(8), ScopeSnapshot::room(), cache.clone());
        *cache.face.lock().unwrap() = Some(CachedFace {
            generation: cache.generation(),
            scope: ScopeSnapshot {
                person: Some(Arc::from("p-0123456789abcdef")),
                held: true,
            },
            valid_until: Instant::now() + Duration::from_secs(3),
            seen_until: Instant::now(),
            named_at: Instant::now(),
        });
        ended(&line.lease());
        assert_eq!(line.lease().keeper_within(Duration::ZERO), None);
    }

    /// A fresh observation of `name`, read as the refresh reads the eyes'
    /// known file.
    fn observation(name: &str) -> FaceObservation {
        let path = std::env::temp_dir().join(format!(
            "observation-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, name).unwrap();
        let observation = FaceObservation::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        observation
    }

    /// El's P2 on #3108 and the #2807 run on lamp-c0a4: a late look is armed
    /// only for a line begun while the face cache is cold: within
    /// [`COLD_FOR`] of its going cold (start, a privacy change, a face
    /// remembered, a change in the eyes' known file), with no face named by
    /// a refresh begun since. A refresh naming nobody inside the window does
    /// not end it; one naming a face does; past the window a cache that
    /// named nobody arms none, so a known face first cached during a line
    /// someone off camera said never takes its keep.
    #[test]
    fn a_late_look_is_armed_only_while_the_cache_is_cold() {
        let armed = |cache: &Arc<FaceCache>, request: u8| {
            RequestScope::looking_later(key(request), ScopeSnapshot::room(), cache.clone())
                .lease()
                .looks_later()
        };
        let long_ago = || {
            Instant::now()
                .checked_sub(COLD_FOR + Duration::from_secs(1))
                .unwrap()
        };
        let cache = Arc::new(FaceCache::default());
        assert!(armed(&cache, 1), "cold before it is stamped");
        cache.went_cold(Instant::now());
        assert!(cache.replace(cache.begin_refresh(), None));
        assert!(
            armed(&cache, 2),
            "naming nobody inside the window: still cold"
        );
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        assert!(!cache.is_cold(Instant::now()), "a face named ends it");
        assert!(!armed(&cache, 3));

        // The window passing with nobody named ends it too (El's case).
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago());
        assert!(cache.replace(cache.begin_refresh(), None));
        assert!(
            !armed(&cache, 4),
            "warm: long after any change, nobody named"
        );
        assert!(cache.is_cold(Instant::now() - COLD_FOR - Duration::from_secs(1)));

        // A face remembered by the voice's tool: cold again, and a refresh
        // begun before it naming a face does not end it.
        let early = cache.begin_refresh();
        let line = RequestScope::looking_later(key(5), ScopeSnapshot::room(), cache.clone());
        assert!(!line.lease().looks_later());
        line.lease().face_remembered();
        cache.replace(early, Some(observation("Ann")));
        assert!(armed(&cache, 6), "cold after a face remembered");

        // A privacy change: cold for the new generation.
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago());
        assert!(!armed(&cache, 7));
        cache.invalidate();
        assert!(armed(&cache, 8), "cold after a privacy change");

        // A plain scope (camera not allowed) remembers into no cache.
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago());
        RequestScope::new(key(9), ScopeSnapshot::room())
            .lease()
            .face_remembered();
        assert!(!armed(&cache, 10));
    }

    /// The #2807 run on lamp-c0a4 at 05:13 on 23 October: the cache was warm
    /// as the line began, A's face in view (unknown); the eyes' known file
    /// changed mid-line (cold), and A was named after, before the line's end
    /// plus [`KEEPER_WAIT`]: A is the keeper, decided at the end. A cold
    /// event within the wait past the end counts too. Armed only mid-line,
    /// A's face first in view during the line (El's walk-in) or no eyes'
    /// view at all keeps for nobody. Cold at the start, it keeps for A with
    /// the eyes' view only if A was in view at the start, and without it
    /// only if A was named within one refresh of the start. A warm line with no cold event through it arms nothing, A cached
    /// mid-line or not (El's P2 on #3108).
    #[test]
    fn a_cache_gone_cold_during_the_line_arms_its_late_look_at_the_end() {
        let ann = crate::remember::person_id("Ann");
        let warm = || {
            let cache = Arc::new(FaceCache::default());
            cache.went_cold(
                Instant::now()
                    .checked_sub(COLD_FOR + Duration::from_secs(1))
                    .unwrap(),
            );
            assert!(cache.replace(cache.begin_refresh(), None));
            assert!(!cache.is_cold(Instant::now()));
            cache
        };

        // The eyes' view: whether `person`'s face was in view as the line
        // began, asked with when it was admitted, which must not be later
        // than `begun`.
        let view = |in_view: bool, begun: Instant| {
            let ann = ann.clone();
            move |person: &str, since: Instant| {
                assert!(since <= begun, "asked as of the line's start");
                in_view && person == ann
            }
        };
        let seen_end = |lease: &ScopeLease, eyes: Option<InView<'_>>| {
            lease.record_keeper(lease.face_at_end_seen(Instant::now(), eyes));
        };

        // Cold mid-line, A in view (unknown) since before the line, named
        // after the cold event: A.
        let cache = warm();
        let line = RequestScope::looking_later(key(1), ScopeSnapshot::room(), cache.clone());
        let begun = Instant::now();
        assert!(line.lease().may_look_later());
        assert!(!line.lease().looks_later(), "warm at its start");
        cache.went_cold(Instant::now());
        assert!(line.lease().looks_later(), "armed by the cold event");
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        assert!(!cache.is_cold(Instant::now()), "warm again by the end");
        seen_end(&line.lease(), Some(&view(true, begun)));
        assert_eq!(
            line.lease().keeper_within(Duration::ZERO),
            Some(ann.clone())
        );

        // Cold, and A named, only within the wait past the end, A in view
        // since the start: A.
        let cache = warm();
        let line = RequestScope::looking_later(key(2), ScopeSnapshot::room(), cache.clone());
        let begun = Instant::now();
        let later = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                cache.went_cold(Instant::now());
                assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
            })
        };
        seen_end(&line.lease(), Some(&view(true, begun)));
        later.join().unwrap();
        assert_eq!(
            line.lease().keeper_within(Duration::ZERO),
            Some(ann.clone())
        );

        // El's walk-in: cold mid-line because A walked in, A's face first in
        // view mid-line, nobody seen speaking: nobody.
        let cache = warm();
        let line = RequestScope::looking_later(key(5), ScopeSnapshot::room(), cache.clone());
        let begun = Instant::now();
        cache.went_cold(Instant::now());
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        seen_end(&line.lease(), Some(&view(false, begun)));
        assert_eq!(line.lease().keeper_within(Duration::ZERO), None);

        // The eyes' speaking off: a cold event mid-line arms nobody.
        let cache = warm();
        let line = RequestScope::looking_later(key(6), ScopeSnapshot::room(), cache.clone());
        cache.went_cold(Instant::now());
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        seen_end(&line.lease(), None);
        assert_eq!(line.lease().keeper_within(Duration::ZERO), None);

        // Cold at its start: A with the eyes' view and A in view at the
        // start, or without the view and A named within one refresh of the
        // start; with the view and A first in view mid-line, nobody
        // (reviewer-three's P2 on #3108).
        for (index, eyes, keeper) in [
            (0, None, Some(ann.clone())),
            (1, Some(true), Some(ann.clone())),
            (2, Some(false), None),
        ] {
            let cache = warm();
            cache.went_cold(Instant::now());
            let line =
                RequestScope::looking_later(key(8 + index), ScopeSnapshot::room(), cache.clone());
            let begun = Instant::now();
            assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
            // Named at the start, as the line took it.
            first_named_at(&cache, line_start(&line.lease()));
            let view = eyes.map(|in_view| view(in_view, begun));
            seen_end(
                &line.lease(),
                view.as_ref()
                    .map(|view| view as &dyn Fn(&str, Instant) -> bool),
            );
            assert_eq!(
                line.lease().keeper_within(Duration::ZERO),
                keeper,
                "{eyes:?}"
            );
        }

        // Warm, no cold event through the line, A cached mid-line: nobody.
        let cache = warm();
        let line = RequestScope::looking_later(key(3), ScopeSnapshot::room(), cache.clone());
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        ended(&line.lease());
        assert!(!line.lease().looks_later());
        assert_eq!(line.lease().keeper_within(Duration::ZERO), None);

        // A cold event past the end and the wait arms nothing.
        let cache = warm();
        let line = RequestScope::looking_later(key(4), ScopeSnapshot::room(), cache.clone());
        ended(&line.lease());
        cache.went_cold(Instant::now());
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        assert_eq!(line.lease().keeper_within(Duration::ZERO), None);
    }

    /// Reviewer-three's P2 on #3108 and the first builder's 06:50 ruling: a
    /// line begun while the cache is cold (here a privacy change, an unmute)
    /// by someone off camera, while known A walks in and is named mid-line,
    /// never keeps for A. With the eyes' view, A must have been in view at
    /// the line's start; without it, A must have been named within one
    /// refresh ([`REFRESH_EVERY`]) of the start, not anywhere in
    /// [`COLD_FOR`].
    #[test]
    fn a_line_begun_cold_keeps_only_for_a_face_seen_or_named_at_its_start() {
        let ann = crate::remember::person_id("Ann");
        let in_view_at_start = |at_start: bool| move |_: &str, _: Instant| -> bool { at_start };
        let past_one_refresh = || std::thread::sleep(REFRESH_EVERY + Duration::from_millis(50));

        // The probe: invalidate, a line begun cold, A cached mid-line, no
        // eyes' view: nobody. With the view and A not in view at the start:
        // nobody either.
        let cache = Arc::new(FaceCache::default());
        cache.invalidate();
        let line = RequestScope::looking_later(key(1), ScopeSnapshot::room(), cache.clone());
        assert!(line.lease().looks_later(), "begun cold");
        past_one_refresh();
        cached(&cache, "Ann", Instant::now() + Duration::from_secs(3));
        assert_eq!(line.lease().face_at_end_seen(Instant::now(), None), None);
        let not_seen = in_view_at_start(false);
        assert_eq!(
            line.lease()
                .face_at_end_seen(Instant::now(), Some(&not_seen)),
            None
        );
        // With the view and A in view at the start: A, however late named.
        let seen = in_view_at_start(true);
        assert_eq!(
            line.lease().face_at_end_seen(Instant::now(), Some(&seen)),
            Some(ann.clone())
        );

        // No view, A named within one refresh of the start: A.
        let cache = Arc::new(FaceCache::default());
        cache.invalidate();
        let line = RequestScope::looking_later(key(2), ScopeSnapshot::room(), cache.clone());
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        // Named at the start, as the line took it.
        first_named_at(&cache, line_start(&line.lease()));
        past_one_refresh();
        // Named on by later refreshes: still first named at the start.
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        assert_eq!(
            line.lease().face_at_end_seen(Instant::now(), None),
            Some(ann.clone())
        );

        // No view, A first named past one refresh of the start: nobody.
        let cache = Arc::new(FaceCache::default());
        cache.invalidate();
        let line = RequestScope::looking_later(key(3), ScopeSnapshot::room(), cache.clone());
        past_one_refresh();
        assert!(cache.replace(cache.begin_refresh(), Some(observation("Ann"))));
        assert_eq!(line.lease().face_at_end_seen(Instant::now(), None), None);
    }

    /// The #2807 run on lamp-c0a4 at `39a68c91b`: a face remembered through
    /// the eyes' own socket, not the voice's tool, changes the eyes' known
    /// file; the refresh that reads the change makes the cache cold,
    /// whoever wrote it. The first read only sets the mark; the known
    /// file's mtime alone (rewritten each look) is no change; the eyes'
    /// names file rewritten (a face remembered) is one.
    #[test]
    fn a_change_in_the_eyes_known_file_makes_the_cache_cold() {
        let directory = std::env::temp_dir().join(format!(
            "known-mark-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let known = directory.join("known");
        let tellable = directory.join("tellable");
        let _ = std::fs::remove_file(&known);
        let _ = std::fs::remove_file(&tellable);
        let long_ago = Instant::now()
            .checked_sub(COLD_FOR + Duration::from_secs(1))
            .unwrap();
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago);
        let read = |cache: &FaceCache| {
            let mut token = cache.begin_refresh();
            let changed = cache.known_file_read(
                &mut token,
                crate::known_faces::KnownMark::read(&known),
                Instant::now(),
            );
            assert!(cache.replace(token, None));
            changed
        };
        assert!(!read(&cache), "the first read sets the mark");
        assert!(!read(&cache), "unchanged");
        assert!(!cache.is_cold(Instant::now()), "warm, nobody named");

        // Another writer names A: cold, though this refresh named nobody.
        std::fs::write(&known, "A\n").unwrap();
        assert!(read(&cache));
        assert!(cache.is_cold(Instant::now()));
        // The same words rewritten later: no change.
        std::fs::write(&known, "A\n").unwrap();
        assert!(!read(&cache));

        // A refresh that read the change and names A ends it at once.
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago);
        cache.known_file_read(
            &mut cache.begin_refresh(),
            crate::known_faces::KnownMark::read(&known),
            Instant::now(),
        );
        std::fs::remove_file(&known).unwrap();
        let mut token_b = cache.begin_refresh();
        assert!(cache.known_file_read(
            &mut token_b,
            crate::known_faces::KnownMark::read(&known),
            Instant::now(),
        ));
        assert!(cache.replace(token_b, Some(observation("A"))));
        assert!(
            !cache.is_cold(Instant::now()),
            "named by the refresh that saw it"
        );

        // The eyes' names file rewritten after a face remembered: a change.
        let cache = Arc::new(FaceCache::default());
        cache.went_cold(long_ago);
        std::fs::write(&tellable, "A\n").unwrap();
        assert!(!read(&cache));
        let file = std::fs::File::options()
            .write(true)
            .open(&tellable)
            .unwrap();
        file.set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        assert!(read(&cache));
        assert!(cache.is_cold(Instant::now()));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
