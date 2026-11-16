//! Seeing someone speak, used (`LAMP_SECOND_SPEAKING_FACE=on`, the second
//! route): whose a heard line is takes one more input, the face seen
//! speaking through it. #1917's admitted scope gives the line the person the
//! eyes know at the lamp. When the eyes (`LAMP_EYES_SPEAKING=on`) see a face
//! speaking through the line that is not that person's face, the line may be
//! someone else's, and its scope narrows to the room: nobody's memory goes to
//! a stranger, and the answer is not held up. Two faces speaking, one of them
//! theirs, narrow it too (Third on #2008: at worst their own line loses their
//! context once). With no face speaking, nothing changes. It needs
//! `LAMP_SECOND_SCOPED_CONTEXT=on`, which tells and files each turn for its
//! lease (Second on #2008); without it, it stays off.
//!
//! Nothing new is kept: the looks are passing ids (`f3`) and whether each
//! reads speaking, in memory only and dropped after [`KEPT`], and the
//! reading is used for its line and gone.
use lamp_msgs::{Faces, SpeakingState};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

pub const ENVIRONMENT: &str = "LAMP_SECOND_SPEAKING_FACE";
/// How often the eyes' faces are read, off the audio path.
pub const EVERY: Duration = Duration::from_millis(100);
/// How long a look is kept: longer than any line.
pub const KEPT: Duration = Duration::from_secs(30);
const MOST_LOOKS: usize = 600;
/// Fewer looks than this over a line is too little to say who spoke it.
pub const FEWEST_LOOKS: usize = 3;
/// About how often the eyes publish their faces (every 250 to 600 ms): with
/// nothing fresh from them, one empty look is counted this often, as one of
/// theirs would be.
pub const EYES_EVERY: Duration = Duration::from_millis(500);
/// How far before a moment a look still says a face was in view then
/// ([`SpeakingFaces::in_view_at`]): two of the eyes' frames at their slowest.
pub const IN_VIEW_BEFORE: Duration = Duration::from_millis(1200);

/// `on` exactly: a line shorter than [`SHORT_LINE_WINDOW`] is judged
/// ([`SpeakingFaces::speakers`], [`SpeakingFaces::speakers_with_boot`]) by
/// the looks from [`SHORT_LINE_WINDOW`] before its end to its end, with the
/// same half rule and [`FEWEST_LOOKS`], and a face counts only if it read
/// `yes` in a look of the line itself and `no` in none of the window: a
/// 0.5 s "yes" spans two of the eyes' looks, under the three a line needs,
/// so it was always the room's (#5522). Off: as before.
pub const SHORT_LINE_ENVIRONMENT: &str = "LAMP_VOICE_SHORT_LINE_SPEAKER";
/// The eyes' own speaking window (`lamp-eyes` `speaking::WINDOW_US`): each
/// look's `yes` already reads the mouth over the 2 s before it.
pub const SHORT_LINE_WINDOW: Duration = Duration::from_secs(2);

/// Whether [`SHORT_LINE_ENVIRONMENT`] is `on`, and nothing else.
pub fn short_line_from_environment() -> bool {
    std::env::var(SHORT_LINE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Where the voice reads that the eyes judge whether a face is speaking at
/// this lamp (`LAMP_EYES_SPEAKING=on`, the eyes' own switch, set in the
/// voice's unit too): a held forget's yes then counts only from the held
/// person's face seen speaking it (#2377).
pub const EYES_ENVIRONMENT: &str = "LAMP_EYES_SPEAKING";

/// Whether [`EYES_ENVIRONMENT`] is `on`, and nothing else.
pub fn eyes_from_environment() -> bool {
    std::env::var(EYES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Whether the person `person` was seen saying a line: some face read
/// speaking through it ([`SpeakingFaces::speakers`]), and every face that
/// did is theirs, the known person's face beside the eyes' known file
/// (`known`, [`known_with_face`]) while the eyes know them. No reading, a
/// stale known file or another face speaking is not seen (#2377).
pub fn seen_saying(speakers: &[String], person: &str, known: Option<(String, String)>) -> bool {
    !speakers.is_empty() && !someone_elses(speakers, person, known)
}

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Who the eyes saw saying one heard line, read at its end
/// (`LAMP_EYES_SPEAKING=on`): its request, the faces seen speaking through
/// it, and the known person with their face ([`known_with_face`]), fresh
/// as the known file was when the line was heard, not when a call ran.
/// The engine keeps each id with the eyes' boot (`boot/f3`,
/// [`SpeakingFaces::speakers_with_boot`], [`SpeakingFaces::known_with_boot`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineSeen {
    pub request: u64,
    pub speakers: Speakers,
    pub known: Option<(String, String)>,
    /// Exact audio interval and words for opt-in requested enrollment.
    pub enrollment: Option<crate::remember_speaker::Proof>,
    /// `speakers` is the known person's face only because their own voice
    /// confirmed the line (`LAMP_SAME_VOICE_CONFIRMS`,
    /// `second_provider::confirmed_by_voice_at`), no face having been seen
    /// speaking it: memory reads it as theirs, but a linked service's
    /// write, full read or the owner's enrolled print never does
    /// (`services::seen_saying_owner`).
    pub by_voice: bool,
}

/// Far more lines than run at once: an older answer still running keeps
/// its own. Each is a request number, a few passing ids and a name, so 64
/// is cheap; and the store is the process's, so tests running in parallel
/// (each with request numbers of its own) can't push another's line out
/// before its call reads it (El on #3038).
const MOST_LINES_SEEN: usize = 64;

static LINES_SEEN: Mutex<VecDeque<LineSeen>> = Mutex::new(VecDeque::new());

/// Keeps what the eyes saw saying a line, for its calls ([`line_seen`]).
pub fn saw_line(seen: LineSeen) {
    if let Ok(mut lines) = LINES_SEEN.lock() {
        keep_line(&mut lines, seen);
    }
}

fn keep_line(lines: &mut VecDeque<LineSeen>, seen: LineSeen) {
    lines.retain(|line| line.request != seen.request);
    lines.push_back(seen);
    while lines.len() > MOST_LINES_SEEN {
        lines.pop_front();
    }
}

/// Publish enrollment under the same lock used when a request is revoked.
/// Revocation marks the lease inactive before it takes this lock: either
/// the proof is refused here, or the revoker removes it after insertion.
pub fn saw_line_scoped(mut seen: LineSeen, lease: &crate::memory_scope::ScopeLease) -> bool {
    if seen.request != lease.key().request {
        return false;
    }
    let Ok(mut lines) = LINES_SEEN.lock() else {
        return false;
    };
    if !lease.is_current() {
        return false;
    }
    if seen
        .enrollment
        .as_ref()
        .is_some_and(|proof| !proof.is_current(lamp_ipc::monotonic_us()))
    {
        seen.enrollment.take();
    }
    keep_line(&mut lines, seen);
    true
}

/// What the eyes saw saying request `request`'s line, if it was read.
pub fn line_seen(request: u64) -> Option<LineSeen> {
    expire_enrollment(lamp_ipc::monotonic_us());
    LINES_SEEN
        .lock()
        .ok()?
        .iter()
        .find(|line| line.request == request)
        .cloned()
}

/// The reader also calls this during idle time, so expired enrollment
/// words and ids do not stay in the bounded line store indefinitely.
pub fn expire_enrollment(now_us: u64) {
    if let Ok(mut lines) = LINES_SEEN.lock() {
        for line in lines.iter_mut() {
            if line
                .enrollment
                .as_ref()
                .is_some_and(|proof| !proof.is_current(now_us))
            {
                line.enrollment.take();
            }
        }
    }
}

pub(crate) fn clear_enrollment(generation: crate::remember_speaker::Generation) {
    if let Ok(mut lines) = LINES_SEEN.lock() {
        for line in lines.iter_mut() {
            if line
                .enrollment
                .as_ref()
                .is_some_and(|proof| proof.generation.same_epoch(generation))
                && let Some(proof) = line.enrollment.take()
            {
                proof.cancel();
            }
        }
    }
}

/// A stopped request cannot lend its evidence to a later tool call.
pub fn cancel_enrollment(request: u64) {
    if let Ok(mut lines) = LINES_SEEN.lock()
        && let Some(line) = lines.iter_mut().find(|line| line.request == request)
        && let Some(proof) = line.enrollment.take()
    {
        proof.cancel();
    }
}

/// Who the eyes saw speaking through a line: the passing ids, none when no
/// face read speaking or there were too few looks to say.
pub type Speakers = Vec<String>;

struct Look {
    at: Instant,
    /// The eyes' boot its frame was published under; empty for none.
    boot: String,
    /// The passing ids that read speaking in this look.
    speaking: Vec<String>,
    /// Every passing id in this look, speaking or not
    /// ([`SpeakingFaces::in_view_at`]).
    tracked: Vec<String>,
    /// Of those, the ones counted as people at the lamp
    /// (`LAMP_FACES_AT_DESK_ONLY`, [`crate::known_faces::at_desk`]): every
    /// one with the switch off. Whether another person was in view
    /// ([`SpeakingFaces::alone_in_view`], [`SpeakingFaces::one_face_in_view`],
    /// [`SpeakingFaces::no_other_face_in_view`]) is read from these.
    counted: Vec<String>,
    /// The faces whose mouth read still (`no`), turned to the lamp or not
    /// ([`SpeakingFaces::alone_in_view`]).
    still: Vec<String>,
    /// Of those, the ones turned to the lamp (`facing`), for a line the lamp
    /// began itself (#2287's follow-up, [`SpeakingFaces::turned_speakers`]).
    turned: Vec<String>,
    /// The faces turned to the lamp whose mouth read still (`no`), for
    /// [`SpeakingFaces::turned_only_silent`].
    turned_silent: Vec<String>,
    /// The faces turned to the lamp whose mouth was not read (`unknown`, or
    /// not judged at all) in this look.
    turned_unread: Vec<String>,
    /// The eyes' word in this look on which lost id each face follows
    /// (`follows`, #4885): (old, new), bare ids.
    follows: Vec<(String, String)>,
}

#[derive(Default)]
struct Looks {
    kept: VecDeque<Look>,
    /// The last frame counted, by its publisher's boot and sequence.
    frame: Option<(String, u64)>,
    /// Each of the eyes' boots, newest last, with the time its first frame
    /// was read: the boot a file the eyes wrote belongs to
    /// ([`SpeakingFaces::known_with_boot`]). Instance names only, so kept
    /// past [`KEPT`] and [`SpeakingFaces::forget`].
    boots: VecDeque<(String, SystemTime)>,
}

impl Looks {
    /// The links the eyes say hold as of `at` (`boot/f1` followed by
    /// `boot/f2`, #4885), from their own looks at or before it, as a chain:
    /// a face re-found twice is `f1` to `f2` to `f3`, though the eyes say
    /// only the last hop by then (#4966). Each pair lasts until a later
    /// look ends it: its old id in view again (in the look that says it
    /// too), or its new id in view without the eyes saying it (the
    /// Reviewer-lead's P1 on #4917). A link the eyes ended is gone at once.
    /// The voice keeps a pair longer than the eyes do: they drop a link
    /// 1.5 s after the old id was lost (`lamp-eyes` bus_out.rs), it lasts
    /// here for the looks kept ([`KEPT`]) unless a look cuts it.
    fn links_at(&self, at: Instant) -> Vec<(String, String)> {
        let mut links: Vec<(String, String)> = Vec::new();
        for look in self
            .kept
            .iter()
            .filter(|look| look.at <= at && !look.boot.is_empty())
        {
            let id = |bare: &str| format!("{}/{bare}", look.boot);
            let in_view = |face: &str| look.tracked.iter().any(|seen| id(seen) == face);
            let said = look
                .follows
                .iter()
                .filter(|(old, _)| !look.tracked.contains(old))
                .map(|(old, new)| (id(old), id(new)))
                .collect::<Vec<_>>();
            links.retain(|link| !in_view(&link.0) && (!in_view(&link.1) || said.contains(link)));
            for link in said {
                if !links.contains(&link) {
                    links.push(link);
                }
            }
        }
        links
    }
}

/// `id` (`boot/f1`) as the face it is under `links` ([`Looks::links_at`]):
/// the id that follows it, and so on, else itself.
fn one_face(id: String, links: &[(String, String)]) -> String {
    let mut id = id;
    for _ in 0..links.len() {
        match links.iter().find(|(old, _)| *old == id) {
            Some((_, new)) => id.clone_from(new),
            None => break,
        }
    }
    id
}

/// Boots of the eyes remembered with their first frame's time.
const MOST_BOOTS: usize = 8;
/// The last [`KEPT`] of looks, fed by one reader and judged at each line's
/// end. Each of the eyes' frames counts once, however often it is read (the
/// reviewer on #2008), so a short line can't pass the bar on one frame.
#[derive(Default)]
pub struct SpeakingFaces {
    looks: Mutex<Looks>,
    /// The newest pair [`Self::known_with_boot`] read, name and `boot/f3`,
    /// with when it was read: [`Self::known_lately`]'s, once the known file
    /// has gone stale with the face out of view. Memory only; dropped by
    /// [`Self::forget`].
    named: Mutex<Option<(String, String, Instant)>>,
}

/// How long [`SpeakingFaces::known_lately`] keeps a pair read from the
/// known file: a conversation's going on (`thread::GOING_ON_FOR`, 45 s).
pub const NAMED_FOR: Duration = Duration::from_secs(45);

impl SpeakingFaces {
    /// One read of the eyes' `humans/faces/tracked`: their frame, by its
    /// publisher's boot and sequence, counted only if it is new; `None` when
    /// they said nothing fresh, counted as no face speaking at most once each
    /// [`EYES_EVERY`]. Each face's `follows` (#4885) is kept with its look
    /// as the eyes said it, never merged into the looks before it.
    pub fn look(&self, at: Instant, frame: Option<((&str, u64), &Faces)>) {
        self.look_at_desk(at, frame, crate::known_faces::at_desk_only());
    }

    /// [`Self::look`] with `LAMP_FACES_AT_DESK_ONLY` given: with
    /// `desk_only`, a face too small or too unsure to be someone at the desk
    /// ([`crate::known_faces::at_desk`]) is still tracked and still read
    /// speaking, but is no other person in view.
    pub fn look_at_desk(&self, at: Instant, frame: Option<((&str, u64), &Faces)>, desk_only: bool) {
        let Ok(mut looks) = self.looks.lock() else {
            return;
        };
        match frame {
            Some(((boot, seq), _))
                if looks
                    .frame
                    .as_ref()
                    .is_some_and(|(last, at)| last == boot && *at == seq) =>
            {
                return;
            }
            Some(((boot, seq), _)) => looks.frame = Some((boot.to_owned(), seq)),
            None => {
                if looks.frame.is_none()
                    && looks
                        .kept
                        .back()
                        .is_some_and(|last| at.saturating_duration_since(last.at) < EYES_EVERY)
                {
                    return;
                }
                looks.frame = None;
            }
        }
        let speaking_faces: Vec<_> = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter(|face| {
                        face.speaking
                            .as_ref()
                            .is_some_and(|speaking| speaking.state == SpeakingState::Yes)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let speaking = speaking_faces.iter().map(|face| face.id.clone()).collect();
        let tracked = frame
            .map(|(_, faces)| faces.faces.iter().map(|face| face.id.clone()).collect())
            .unwrap_or_default();
        let counted = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .zip(crate::known_faces::at_desk(&faces.faces, desk_only))
                    .filter(|(_, counted)| *counted)
                    .map(|(face, _)| face.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let still = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter(|face| {
                        face.speaking
                            .is_some_and(|speaking| speaking.state == SpeakingState::No)
                    })
                    .map(|face| face.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let turned = speaking_faces
            .iter()
            .filter(|face| face.facing == Some(true))
            .map(|face| face.id.clone())
            .collect();
        let state = |face: &&lamp_msgs::Face| face.speaking.map(|speaking| speaking.state);
        let turned_faces: Vec<_> = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter(|face| face.facing == Some(true))
                    .collect()
            })
            .unwrap_or_default();
        let turned_silent = turned_faces
            .iter()
            .filter(|face| state(face) == Some(SpeakingState::No))
            .map(|face| face.id.clone())
            .collect();
        let turned_unread = turned_faces
            .iter()
            .filter(|face| !matches!(state(face), Some(SpeakingState::Yes | SpeakingState::No)))
            .map(|face| face.id.clone())
            .collect();
        let boot = frame
            .map(|((boot, _), _)| boot.to_owned())
            .unwrap_or_default();
        if !boot.is_empty() && !looks.boots.iter().any(|(seen, _)| *seen == boot) {
            // The wall time of `at`, for comparing with a file's time.
            let first = SystemTime::now()
                .checked_sub(Instant::now().saturating_duration_since(at))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            looks.boots.push_back((boot.clone(), first));
            while looks.boots.len() > MOST_BOOTS {
                looks.boots.pop_front();
            }
        }
        let follows = frame
            .map(|(_, faces)| {
                faces
                    .faces
                    .iter()
                    .filter_map(|face| Some((face.follows.clone()?, face.id.clone())))
                    .collect()
            })
            .unwrap_or_default();
        let kept = &mut looks.kept;
        while kept
            .front()
            .is_some_and(|look| at.saturating_duration_since(look.at) > KEPT)
            || kept.len() >= MOST_LOOKS
        {
            kept.pop_front();
        }
        kept.push_back(Look {
            at,
            boot,
            speaking,
            tracked,
            counted,
            still,
            turned,
            turned_silent,
            turned_unread,
            follows,
        });
    }

    /// The faces that read speaking in at least half of the looks from
    /// `from` to `to`; a short line's with [`SHORT_LINE_ENVIRONMENT`] as
    /// [`Self::line_counted`] reads it.
    pub fn speakers(&self, from: Instant, to: Instant) -> Speakers {
        self.line_counted(from, to, short_line_from_environment(), |_, ids| {
            ids.to_vec()
        })
    }

    /// As [`Self::speakers`], each id kept with the eyes' boot it was seen
    /// under (`boot/f3`): after the eyes start again a passing id names
    /// someone else, so an id from an older boot matches none of the new.
    /// A lost id the eyes say a face follows as of `to` is that face's
    /// ([`Looks::links_at`], #4885).
    pub fn speakers_with_boot(&self, from: Instant, to: Instant) -> Speakers {
        let links = self
            .looks
            .lock()
            .map(|looks| looks.links_at(to))
            .unwrap_or_default();
        self.line_counted(from, to, short_line_from_environment(), |look, ids| {
            let mut ids: Vec<String> = ids
                .iter()
                .map(|id| one_face(format!("{}/{id}", look.boot), &links))
                .collect();
            ids.sort();
            ids.dedup();
            ids
        })
    }

    /// Whether the eyes' links as of `to` say a new id follows `face`
    /// (`boot/f1`), their link still pending ([`Looks::links_at`]).
    pub fn follow_of(&self, face: &str, to: Instant) -> bool {
        self.looks
            .lock()
            .is_ok_and(|looks| looks.links_at(to).iter().any(|(old, _)| old == face))
    }

    /// One face seen speaking this line, for requested enrollment. A brief
    /// competitor still refuses it; a majority is insufficient for saving
    /// someone's face. Unknown looks never count as positive evidence.
    pub fn enrollment_speaker(&self, from: Instant, to: Instant) -> Option<String> {
        if to < from || to.duration_since(from) > KEPT {
            return None;
        }
        let looks = self.looks.lock().ok()?;
        let during: Vec<_> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to)
            .collect();
        let mut boot = None;
        let mut speaker = None;
        let mut positive = 0;
        for look in &during {
            if !look.boot.is_empty() {
                if boot.is_some_and(|boot| boot != &look.boot) {
                    return None;
                }
                boot = Some(&look.boot);
            }
            for id in &look.speaking {
                if look.boot.is_empty() || speaker.is_some_and(|speaker| speaker != id) {
                    return None;
                }
                speaker = Some(id);
            }
            positive += usize::from(!look.speaking.is_empty());
        }
        if positive < FEWEST_LOOKS || positive * 2 < during.len() {
            return None;
        }
        Some(format!("{}/{}", boot?, speaker?))
    }

    /// Forget every look: the camera closed or the microphone was muted,
    /// and no passing id outlives that.
    pub fn forget(&self) {
        if let Ok(mut looks) = self.looks.lock() {
            *looks = Looks {
                boots: std::mem::take(&mut looks.boots),
                ..Looks::default()
            };
        }
        if let Ok(mut named) = self.named.lock() {
            *named = None;
        }
    }

    /// [`known_with_face`], the face's passing id with the eyes' boot it
    /// was written under (`boot/f3`), to compare with
    /// [`Self::speakers_with_boot`]. The eyes' `-face` file holds the bare
    /// id only, so its boot is the newest boot whose first frame was read
    /// strictly before the file was last written (the eyes renew it every
    /// two seconds), with no tolerance: a file left from before the eyes
    /// started again, or written at the new boot's first frame, names none
    /// of the new boot's faces, though it is still fresh (#3037, Fourth's
    /// P2 on #3038). In doubt it is refused: a new boot's file written just
    /// before the voice read its first frame waits for the eyes' next
    /// renewal. `None` with no boot read before it. The eyes' own
    /// `known-speaker` file (Fourth's) is to replace this. A followed id is
    /// never mapped here: only the eyes' file moves the known face to a new
    /// id, once a print of it passes their hold's bar (#4885, the
    /// Reviewer-lead's P1 and P2 on #4917).
    /// When boot `boot`'s first frame was read, for the boundary tests.
    #[cfg(test)]
    fn first_frame_of(&self, boot: &str) -> Option<SystemTime> {
        let looks = self.looks.lock().ok()?;
        looks
            .boots
            .iter()
            .find(|(seen, _)| seen == boot)
            .map(|(_, first)| *first)
    }

    pub fn known_with_boot(&self, known: &Path) -> Option<(String, String)> {
        let (name, face, written) = known_with_face_written(known)?;
        let pair = {
            let looks = self.looks.lock().ok()?;
            let (boot, _) = looks
                .boots
                .iter()
                .filter(|(_, first)| *first < written)
                .max_by_key(|(_, first)| *first)?;
            (name, format!("{boot}/{face}"))
        };
        if let Ok(mut named) = self.named.lock() {
            *named = Some((pair.0.clone(), pair.1.clone(), Instant::now()));
        }
        Some(pair)
    }

    /// [`Self::known_with_boot`], or, with the known file stale or naming
    /// nobody, the pair it last read (within [`NAMED_FOR`]) when that face
    /// left view: in some look from `since` to `to`, and not in the
    /// newest look by `to`. The eyes stop writing the known file
    /// once the face is gone and it is fresh only [`crate::known_faces::FRESH`]
    /// (3 s), so a line said just after it left, or by it before it left,
    /// read at the line's end, named no one (lamp-06c1, 5 Nov, S4: requests
    /// 190 and 191, 3 and 8 s after Olivia's face left; neither carried).
    /// A passing id is one face for as long as it is tracked, so the pair
    /// still names it. A face still in view the eyes no longer name stays
    /// unnamed: their call.
    pub fn known_lately(
        &self,
        known: &Path,
        since: Instant,
        to: Instant,
    ) -> Option<(String, String)> {
        self.known_with_boot(known).or_else(|| {
            let (name, face, read) = self.named.lock().ok()?.clone()?;
            (to.saturating_duration_since(read) <= NAMED_FOR
                && self.face_seen_between(Some(&face), since, to)
                && !self.in_last_look(&face, to))
            .then_some((name, face))
        })
    }

    /// Whether `face` (`boot/f3`) is in the eyes' newest look at or before
    /// `at`: still in view as of then. No look, not in view.
    fn in_last_look(&self, face: &str, at: Instant) -> bool {
        self.looks.lock().is_ok_and(|looks| {
            looks
                .kept
                .iter()
                .rev()
                .find(|look| look.at <= at && !look.boot.is_empty())
                .is_some_and(|look| {
                    look.tracked
                        .iter()
                        .any(|id| format!("{}/{id}", look.boot) == face)
                })
        })
    }

    /// Whether the face `face` (`boot/f3`, as [`Self::known_with_boot`]
    /// gives it) was in view at `at`: in some look from [`IN_VIEW_BEFORE`]
    /// before `at` to `at` itself. A passing id is nobody's once its face
    /// has gone, so the same id seen later means the same face stayed in
    /// view since. No look then is not in view.
    pub fn in_view_at(&self, face: &str, at: Instant) -> bool {
        let from = at.checked_sub(IN_VIEW_BEFORE).unwrap_or(at);
        self.looks.lock().is_ok_and(|looks| {
            looks
                .kept
                .iter()
                .filter(|look| look.at >= from && look.at <= at && !look.boot.is_empty())
                .any(|look| {
                    look.tracked
                        .iter()
                        .any(|id| format!("{}/{id}", look.boot) == face)
                })
        })
    }

    /// Whether the face `face` (`boot/f3`, [`Self::known_with_boot`]) was
    /// the only face in view through a line from `from` to `to`, and seen
    /// speaking in it at least once: in each of at least [`FEWEST_LOOKS`] of
    /// the eyes' own looks it was the one face tracked, its mouth read
    /// speaking (`yes`) in at least one, and still in under half. A mouth
    /// never read speaking (only `unknown`, turned away, or still) is no
    /// evidence it was theirs: a voice off camera or a television while
    /// they read their phone stays the room's (the Reviewer-lead's P1 on
    /// #4090). On lamp-3e83 (stable 8163, 28 Oct
    /// 16:30) the known person alone at the lamp had about half their lines
    /// read with no face speaking (a mouth not read often enough), so kept
    /// as the room's, and the lamp forgot their name (#3437's follow-up).
    /// Looks the eyes published nothing fresh in say nothing either way.
    pub fn alone_in_view(&self, face: &str, from: Instant, to: Instant) -> bool {
        let Ok(looks) = self.looks.lock() else {
            return false;
        };
        let during: Vec<_> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
            .collect();
        let own = |look: &Look, ids: &[String]| {
            ids.iter().any(|id| format!("{}/{id}", look.boot) == face)
        };
        let still = during.iter().filter(|look| own(look, &look.still)).count();
        during.len() >= FEWEST_LOOKS
            && during
                .iter()
                .all(|look| look.counted.len() == 1 && own(look, &look.counted))
            && during.iter().any(|look| own(look, &look.speaking))
            && still * 2 < during.len()
    }

    /// Whether the face `face` (`boot/f3`) had its mouth read speaking
    /// (`yes`) in at least one of the eyes' own looks from `from` to `to`,
    /// however many read it still: beside [`Self::one_face_in_view`], a lone
    /// face that is not silent (lamp-06c1, stock 10538, S4 request 291: read
    /// speaking in 1 of 9 looks and still in 7, so never
    /// [`Self::alone_in_view`]).
    pub fn read_speaking_in(&self, face: &str, from: Instant, to: Instant) -> bool {
        self.looks.lock().is_ok_and(|looks| {
            looks
                .kept
                .iter()
                .filter(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
                .any(|look| {
                    look.speaking
                        .iter()
                        .any(|id| format!("{}/{id}", look.boot) == face)
                })
        })
    }

    /// Whether the known face `face` (`boot/f3`, [`Self::known_with_boot`])
    /// was the one face the eyes saw through a line from `from` to `to`
    /// (`LAMP_PERSON_SCOPE_ONE_FACE`, the launch testers' item 1 on #3997):
    /// tracked in at least [`FEWEST_LOOKS`] of the eyes' own looks in it,
    /// and no look of it with any other face. A look with nobody in it is
    /// no other face. Unlike [`Self::alone_in_view`] the mouth is not read:
    /// on lamp-220c (30 Oct) the owner's face was in 17 looks and read
    /// speaking in none, so every line was kept as the room's.
    pub fn one_face_in_view(&self, face: &str, from: Instant, to: Instant) -> bool {
        let Ok(looks) = self.looks.lock() else {
            return false;
        };
        let during: Vec<_> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
            .collect();
        let own = |look: &Look, id: &String| format!("{}/{id}", look.boot) == face;
        during
            .iter()
            .all(|look| look.counted.iter().all(|id| own(look, id)))
            && during
                .iter()
                .filter(|look| look.tracked.iter().any(|id| own(look, id)))
                .count()
                >= FEWEST_LOOKS
    }

    /// Whether the eyes saw through a line from `from` to `to`, and no face
    /// but `face` (`boot/f3`, [`Self::known_with_boot`]; none: no face at
    /// all) in any look of it: at least one of their own looks fell in it,
    /// and each tracked nobody or `face` alone (#4339). A line with no look
    /// in it, the eyes stalled or the camera covered, is not: nothing then
    /// shows nobody else was there. A lost id the eyes say a face follows
    /// as of `to` is that face's ([`Looks::links_at`], #4885); a look with
    /// two faces in it never is one face.
    pub fn no_other_face_in_view(&self, face: Option<&str>, from: Instant, to: Instant) -> bool {
        let Ok(looks) = self.looks.lock() else {
            return false;
        };
        let links = looks.links_at(to);
        let mut during = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
            .peekable();
        during.peek().is_some()
            && during.all(|look| {
                look.counted.len() <= 1
                    && look.counted.iter().all(|id| {
                        face.is_some_and(|face| {
                            one_face(format!("{}/{id}", look.boot), &links) == face
                        })
                    })
            })
    }

    /// Whether the eyes made any look of their own from `from` to `to`,
    /// whoever it tracked: a line carried past silent faces
    /// (`LAMP_CONVERSATION_CARRIES_PAST_SILENT`, #4339) needs the eyes to
    /// have looked through it, so no face seen speaking it means one was
    /// watched for. The eyes stalled or the camera covered is not.
    pub fn looked_between(&self, from: Instant, to: Instant) -> bool {
        self.looks.lock().is_ok_and(|looks| {
            looks
                .kept
                .iter()
                .any(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
        })
    }

    /// Whether the face `face` (`boot/f3`, [`Self::known_with_boot`]; none:
    /// never) was tracked in any of the eyes' own looks from `since` to
    /// `to`: for a line carried with no face seen speaking it, their face in
    /// view through it or within a few seconds before it began (#4339, the
    /// Reviewer-lead's P2 on #4479). An empty view from the start, or their
    /// face gone longer, is not.
    pub fn face_seen_between(&self, face: Option<&str>, since: Instant, to: Instant) -> bool {
        let Some(face) = face else {
            return false;
        };
        self.looks.lock().is_ok_and(|looks| {
            looks
                .kept
                .iter()
                .filter(|look| look.at >= since && look.at <= to && !look.boot.is_empty())
                .any(|look| {
                    look.tracked
                        .iter()
                        .any(|id| format!("{}/{id}", look.boot) == face)
                })
        })
    }

    /// For the journal when no face was seen speaking a line: how many of
    /// the eyes' own looks it had, how many held one face only, and of
    /// `face`'s (the known one, `boot/f3`) how many read speaking, still or
    /// unread. Counts only, no id or name.
    pub fn sight_told(&self, face: Option<&str>, from: Instant, to: Instant) -> String {
        let Ok(looks) = self.looks.lock() else {
            return String::new();
        };
        let during: Vec<_> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to && !look.boot.is_empty())
            .collect();
        let own = |look: &Look, ids: &[String]| {
            face.is_some_and(|face| ids.iter().any(|id| format!("{}/{id}", look.boot) == face))
        };
        let (mut one, mut yes, mut still, mut seen, mut dropped) = (0, 0, 0, 0, 0);
        for look in &during {
            one += usize::from(look.counted.len() == 1);
            dropped += look.tracked.len().saturating_sub(look.counted.len());
            seen += usize::from(own(look, &look.tracked));
            yes += usize::from(own(look, &look.speaking));
            still += usize::from(own(look, &look.still));
        }
        // Faces not at the desk (`LAMP_FACES_AT_DESK_ONLY`), told only when
        // there were any, so the line is as before without them.
        let dropped = if dropped > 0 {
            format!("; {dropped} faces not at the desk")
        } else {
            String::new()
        };
        format!(
            "{} looks, {one} with one face; the known face in {seen}, read speaking in {yes}, still in {still}, unread in {}{dropped}",
            during.len(),
            seen.saturating_sub(yes + still)
        )
    }

    /// Whether the person the eyes know at the lamp now (`known`, its face
    /// beside it, [`Self::known_with_boot`]) is `person`, and their face was
    /// in view at `since` ([`Self::in_view_at`]): a known face named after a
    /// line began was at the lamp, unknown yet, as it began, not walking in
    /// during it (#2807, El's P2 on #3108). Blocking: reads the files.
    pub fn known_in_view_since(&self, known: &Path, person: &str, since: Instant) -> bool {
        self.known_with_boot(known).is_some_and(|(name, face)| {
            crate::known_faces::memory_person(&name) == person && self.in_view_at(&face, since)
        })
    }

    /// The faces that read speaking and turned to the lamp in at least half
    /// of the looks from `from` to `to`: someone at the lamp talking to it,
    /// not the room (#2287's follow-up).
    pub fn turned_speakers(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| look.turned.clone())
    }

    /// As [`Self::turned_speakers`], each id with the eyes' boot, as
    /// [`Self::speakers_with_boot`] gives them.
    pub fn turned_speakers_with_boot(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| {
            look.turned
                .iter()
                .map(|id| format!("{}/{id}", look.boot))
                .collect()
        })
    }

    /// Whether the only faces turned to the lamp from `from` to `to` were
    /// seen not speaking: one turned to it read still in at least half of the
    /// looks, none turned to it read speaking in any look (a real asker's
    /// mouth reads still between words: the reviewer-lead's P2 on #2546),
    /// and none turned to it went unread in any look. Someone at the lamp looking at a screen
    /// beside it while two other people talk is not talking to it (lamp-4ace,
    /// 21 Oct 14:00, `LAMP_TURNED_SPEAKING`). A mouth the eyes could not read
    /// says nothing, so with `LAMP_EYES_SPEAKING` off this is never true.
    pub fn turned_only_silent(&self, from: Instant, to: Instant) -> bool {
        let unread = self.looks.lock().map_or(true, |looks| {
            looks
                .kept
                .iter()
                .any(|look| look.at >= from && look.at <= to && !look.turned_unread.is_empty())
        });
        !unread && self.turned_not_seen_speaking(from, to)
    }

    /// Whether the faces turned to the lamp from `from` to `to` were read and
    /// none was seen speaking: one turned to it read still in at least half
    /// of the looks, and none turned to it read speaking in any look. As
    /// [`Self::turned_only_silent`], but a look whose turned mouth went
    /// unread spoils nothing, so a still face the eyes miss now and then is
    /// still read (`LAMP_TURNED_FACE_SPEAKS`: on lamp-52e6 on 24 Oct at
    /// 05:31 a still face turned to the lamp let two other voices' desk
    /// talk out). Evidence is per face: a face turned to the lamp whose mouth
    /// was never read in the window (as with `LAMP_EYES_SPEAKING` off) says
    /// nothing, so never true while one is there (the reviewer-lead's P2 on
    /// #3305: a still face beside it does not speak for it).
    pub fn turned_not_seen_speaking(&self, from: Instant, to: Instant) -> bool {
        let spoke_or_unread = self.looks.lock().map_or(true, |looks| {
            let during = || {
                looks
                    .kept
                    .iter()
                    .filter(|look| look.at >= from && look.at <= to)
            };
            during().any(|look| !look.turned.is_empty())
                || during()
                    .flat_map(|look| &look.turned_unread)
                    .any(|id| !during().any(|look| look.turned_silent.contains(id)))
        });
        !spoke_or_unread
            && !self
                .counted(from, to, |look| look.turned_silent.clone())
                .is_empty()
    }

    /// The faces `id` names among each look's speaking ones in at least
    /// half of the looks from `from` to `to` ([`Self::counted`]). With
    /// `padded` ([`SHORT_LINE_ENVIRONMENT`]) and the line shorter than
    /// [`SHORT_LINE_WINDOW`], a face is the line's only if its `yes` began
    /// in it: it read `yes` in at least half of the line's own looks and
    /// `no` in none, at least [`FEWEST_LOOKS`] looks fell in the window from
    /// [`SHORT_LINE_WINDOW`] before `to`, at least one of the eyes' own looks
    /// fell in it before `from`, and no face read `yes` in any look before
    /// `from`. The eyes score `yes` over their own sliding 2 s window, so a
    /// face that stopped talking up to 2 s ago still reads `yes`; a `yes`
    /// before the line may be anyone's carried-over earlier line, the
    /// Reviewer-lead's P2 on #5660 (a listener's 0.4 s "yes" read as the
    /// face that spoke just before), so the line is then the room's.
    fn line_counted(
        &self,
        from: Instant,
        to: Instant,
        padded: bool,
        id: impl Fn(&Look, &[String]) -> Vec<String>,
    ) -> Speakers {
        let wide = to
            .checked_sub(SHORT_LINE_WINDOW)
            .filter(|wide| padded && *wide < from);
        let Some(wide) = wide else {
            return self.counted(from, to, |look| id(look, &look.speaking));
        };
        let Ok(looks) = self.looks.lock() else {
            return Speakers::new();
        };
        let (before, line): (Vec<&Look>, Vec<&Look>) = looks
            .kept
            .iter()
            .filter(|look| look.at >= wide && look.at <= to)
            .partition(|look| look.at < from);
        if before.len() + line.len() < FEWEST_LOOKS
            || line.is_empty()
            || !before.iter().any(|look| !look.boot.is_empty())
            || before.iter().any(|look| !look.speaking.is_empty())
        {
            return Speakers::new();
        }
        let still: Vec<String> = line.iter().flat_map(|look| id(look, &look.still)).collect();
        half_of(&line, |look| id(look, &look.speaking))
            .into_iter()
            .filter(|face| !still.contains(face))
            .collect()
    }

    /// The ids `of` each look gives in at least half of the looks from
    /// `from` to `to`, and none with fewer than [`FEWEST_LOOKS`].
    fn counted(&self, from: Instant, to: Instant, of: impl Fn(&Look) -> Vec<String>) -> Speakers {
        let Ok(looks) = self.looks.lock() else {
            return Speakers::new();
        };
        let during: Vec<&Look> = looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to)
            .collect();
        if during.len() < FEWEST_LOOKS {
            return Speakers::new();
        }
        half_of(&during, of)
    }
}

/// The ids `of` each look gives in at least half of `during`.
fn half_of(during: &[&Look], of: impl Fn(&Look) -> Vec<String>) -> Speakers {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for id in during.iter().flat_map(|look| of(look)) {
        match counts.iter_mut().find(|(seen, _)| *seen == id) {
            Some((_, count)) => *count += 1,
            None => counts.push((id, 1)),
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| count * 2 >= during.len())
        .map(|(id, _)| id)
        .collect()
}

/// Reads the eyes' faces every [`EVERY`] on its own thread, until dropped.
pub struct Reader {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The reader of `humans/faces/tracked` on the bus at `root`, feeding `eyes`;
/// `None` when the bus cannot be opened.
pub fn read_from(eyes: std::sync::Arc<SpeakingFaces>, root: &Path) -> Option<Reader> {
    let bus = lamp_bus::Bus::new(root).ok()?;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::Builder::new()
        .name("second-speaking".into())
        .spawn(move || {
            while !stopped.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::sleep(EVERY);
                expire_enrollment(lamp_ipc::monotonic_us());
                let frame = bus
                    .latest(Faces::TOPIC)
                    .ok()
                    .flatten()
                    .filter(|message| !message.expired)
                    .and_then(|message| {
                        let faces = message.parse::<Faces>().ok()?;
                        Some((message.header.boot, message.header.seq, faces))
                    });
                eyes.look(
                    Instant::now(),
                    frame
                        .as_ref()
                        .map(|(boot, seq, faces)| ((boot.as_str(), *seq), faces)),
                );
            }
        })
        .ok()?;
    Some(Reader {
        stop,
        thread: Some(thread),
    })
}

/// Beside the eyes' known file: the known person's face's passing id
/// (`lamp-eyes`' `who::known_face_of`).
pub fn known_face_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-face");
    PathBuf::from(beside)
}

/// The known person's name and their face's passing id, read together:
/// the face, the name, then the face again, so a name is never paired with
/// another's face as the eyes replace both. Each fresh as the known file is.
pub fn known_with_face(known: &Path) -> Option<(String, String)> {
    known_with_face_written(known).map(|(name, face, _)| (name, face))
}

/// [`known_with_face`], with when the face's file was last written.
fn known_with_face_written(known: &Path) -> Option<(String, String, SystemTime)> {
    let beside = known_face_of(known);
    let (first, _) = face_in(&beside)?;
    let name = crate::known_faces::known_now(known)?;
    let (again, written) = face_in(&beside)?;
    (again == first).then_some((name, first, written))
}

fn face_in(path: &Path) -> Option<(String, SystemTime)> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let written = file.metadata().ok()?.modified().ok()?;
    let age = SystemTime::now().duration_since(written).ok()?;
    if age >= crate::known_faces::FRESH {
        return None;
    }
    let mut text = String::new();
    file.take(64).read_to_string(&mut text).ok()?;
    let id = text.trim();
    let digits = id.strip_prefix('f')?;
    (!digits.is_empty() && digits.len() <= 10 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| (id.to_owned(), written))
}

/// How long past a line's end the eyes' known file is waited for when a
/// follow of the known face is pending ([`known_once_decided`], #4966): the
/// eyes check a new near id at the first look at least `REFOUND_RETRY`
/// (0.5 s) after their last check (`who::recognise_due_near`), a print
/// takes 152 ms on the lamp (eyes.md), then the known file is written
/// (`keep_held`) or removed. 0.5 s, a look and the print, with margin.
pub const FOLLOW_DECIDED_WITHIN: Duration = Duration::from_secs(1);

/// The known person and face (`known`, as [`SpeakingFaces::known_with_boot`]
/// read it at the line's end `to`), once the eyes' print has decided a
/// follow of that face (#4966, the Reviewer-lead's P2 on #4967). A link is
/// position only: someone else may sit where they were. So the known face
/// is never mapped through it; when the eyes' links as of `to` still say
/// a new id follows it ([`SpeakingFaces::follow_of`]), the known file is read again every [`EVERY`] until it
/// names another face or none, at most `within` past `to`. A file moved to
/// the new id, once its print passed the eyes' hold, makes the line theirs;
/// one still on the old id at the bound, or gone, leaves it the room's.
/// With `LAMP_FACE_KNOWN_HOLD` or `LAMP_FACE_REFOUND` off no link is said,
/// and nothing waits.
pub async fn known_once_decided(
    eyes: &std::sync::Arc<SpeakingFaces>,
    known_file: &Path,
    known: Option<(String, String)>,
    to: Instant,
    within: Duration,
) -> Option<(String, String)> {
    let Some(face) = known.as_ref().map(|(_, face)| face.clone()) else {
        return known;
    };
    if !eyes.follow_of(&face, to) {
        return known;
    }
    let until = to + within;
    let mut known = known;
    while known.as_ref().is_some_and(|(_, now)| *now == face) {
        if Instant::now() >= until {
            eprintln!(
                "lamp-voice second: the eyes did not decide a follow of the known face within {within:?} of the line's end: the known file as it stands"
            );
            break;
        }
        tokio::time::sleep(EVERY.min(until.saturating_duration_since(Instant::now()))).await;
        let (eyes, file) = (eyes.clone(), known_file.to_owned());
        known = tokio::task::spawn_blocking(move || eyes.known_with_boot(&file))
            .await
            .ok()
            .flatten();
    }
    known
}

/// Beside the eyes' known file: `-unsure`, the known file's words moved
/// there (its face to `-unsure-face`) while the eyes hold the one known
/// with no print read as theirs since a gap (`lamp-eyes`'
/// `who::hold_unsure`, `LAMP_FACE_KNOWN_UNSURE`, #5738). The known file is
/// gone meanwhile: every reader of it sees nobody known.
pub fn known_unsure_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-unsure");
    PathBuf::from(beside)
}

/// The file the second voice's face cache reads for the known person
/// (#5738): the known file, or while the eyes hold them unsure and it is
/// gone, the held name beside it ([`known_unsure_of`]), so a hold that
/// moves aside and comes back is no change of who is known and the cache
/// stays warm. A line is still tied to them only once a print decides
/// ([`known_once_printed`]); every other reader reads the known file alone.
pub fn known_or_held(known: &Path) -> PathBuf {
    if !known.exists() && known_unsure(known) {
        known_unsure_of(known)
    } else {
        known.to_owned()
    }
}

/// Whether the eyes' hold is unsure now: the mark there, written within
/// [`crate::known_faces::FRESH`] as the known file is.
pub fn known_unsure(known: &Path) -> bool {
    std::fs::metadata(known_unsure_of(known))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|written| SystemTime::now().duration_since(written).ok())
        .is_some_and(|age| age < crate::known_faces::FRESH)
}

/// The known person and face (`known`, read at the line's end `to`), once
/// a print has decided an unsure hold (#5738, the Reviewer-lead's condition
/// on #5546 as the first builder widened it): while the eyes hold it
/// unsure ([`known_unsure`]: nobody near, or a near face not printed since
/// a gap), the known file is gone and the line is tied to no one; the mark
/// is read every [`EVERY`] until it goes, then the pair is read again
/// (`read`): the known file written back by a print of theirs keeps it, a
/// let-go leaves none. Still unsure `within` past `to`, the line is the
/// room's. No mark, nothing waits.
pub async fn known_once_printed<R>(
    known_file: &Path,
    known: Option<(String, String)>,
    to: Instant,
    within: Duration,
    read: R,
) -> Option<(String, String)>
where
    R: Fn() -> Option<(String, String)> + Clone + Send + 'static,
{
    let file = known_file.to_owned();
    let unsure = move || {
        let file = file.clone();
        async move {
            tokio::task::spawn_blocking(move || known_unsure(&file))
                .await
                .unwrap_or(false)
        }
    };
    if !unsure().await {
        return known;
    }
    let until = to + within;
    while unsure().await {
        if Instant::now() >= until {
            eprintln!(
                "lamp-voice second: the eyes' hold of the known face stayed unsure, no print within {within:?} of the line's end: the room's line"
            );
            return None;
        }
        tokio::time::sleep(EVERY.min(until.saturating_duration_since(Instant::now()))).await;
    }
    tokio::task::spawn_blocking(read).await.ok().flatten()
}

/// Whether a line admitted for `person` may be someone else's: a face
/// spoke through it that is not the face of the person the eyes know now,
/// or the eyes now know someone other than `person`. No face speaking
/// leaves the line as it was admitted.
pub fn someone_elses(speakers: &[String], person: &str, known: Option<(String, String)>) -> bool {
    let theirs = known
        .filter(|(name, _)| crate::known_faces::memory_person(name) == person)
        .map(|(_, face)| face);
    speakers
        .iter()
        .any(|id| theirs.as_deref() != Some(id.as_str()))
}

/// Whether a line admitted for `person`, or whose late look named them, is
/// not theirs to be told, kept or forgotten for: the one rule every person
/// scope taken from a line goes through (the line's own admission and a
/// late look's keeper, `second_provider`). With `seen_only`
/// (`LAMP_PERSON_SCOPE_SEEN=on`), unless their own face alone was seen
/// speaking it ([`seen_saying`]): no face seen speaking is not theirs
/// (reviewer-three's P1 and P2 on #3435). Without it, only when another
/// face was ([`someone_elses`]), as before. A line narrowed so whose face
/// was alone in view is still named for the reply ([`named_alone`]).
pub fn not_theirs(
    speakers: &[String],
    person: &str,
    known: Option<(String, String)>,
    seen_only: bool,
) -> bool {
    if seen_only {
        !seen_saying(speakers, person, known)
    } else {
        someone_elses(speakers, person, known)
    }
}

/// Whether a line admitted for `person` and narrowed to the room by
/// [`not_theirs`] for want of a face seen speaking it is still *named* for
/// its reply: no face was seen speaking it, and the known face, theirs, was
/// `alone` in view through it ([`SpeakingFaces::alone_in_view`]). Only the
/// reply may be named, and by this rule alone only through the owner's
/// typed name (`worker::known_person_for`, the readers' HOLDs on #4158): a
/// known face not the owner's is told its name (`known_faces::note`) only
/// when [`named_one_face`] holds too (lamp-06c1, build 10343, request 42).
/// Their memory, the thread and every keep, recall and forget stay the
/// room's (reviewer-three's P2 on #4090: a partner off camera while the
/// owner nods). On lamp-3e83
/// (stable 8163, 28 Oct 16:30) the owner alone at the lamp had about half
/// their lines kept as the room's, and the lamp lost their name.
pub fn named_alone(
    speakers: &[String],
    person: &str,
    known: Option<&(String, String)>,
    alone: bool,
) -> bool {
    speakers.is_empty()
        && alone
        && known.is_some_and(|(name, _)| crate::known_faces::memory_person(name) == person)
}

/// [`named_alone`] without the mouth read (`LAMP_SECOND_NAME_ONE_FACE`, the
/// memory owner's call of 5 Nov: "the 'talking' detection is not
/// trustable"): no face was seen speaking it, the known face, theirs, was
/// the `one_face` in view through it ([`SpeakingFaces::one_face_in_view`],
/// the same count the one-face keep and memory's hits fallback use), and the
/// one-face rule heard no `another_voice` in the conversation. On lamp-06c1
/// (5 Nov 11:12, request 6) the test face alone in 11 of 11 looks was read
/// still in 6 and speaking in 5, so [`SpeakingFaces::alone_in_view`] failed
/// and the lamp had no name for her. Only the reply is told their name; any
/// face seen speaking it, a second face in any look, or another voice names
/// nobody.
pub fn named_one_face(
    speakers: &[String],
    person: &str,
    known: Option<&(String, String)>,
    one_face: bool,
    another_voice: bool,
) -> bool {
    !another_voice && named_alone(speakers, person, known, one_face)
}

/// The journal's words for who was seen speaking a line admitted as a known
/// person's, given whether it stayed `theirs` ([`not_theirs`]): the passing
/// face ids only, never a name or the line's words. `None` when no face was
/// seen speaking it and it stayed theirs (the switch off): nothing to tell.
/// With `known_face` the known face as read for the line (`boot/f3`), or
/// none: a face seen speaking that is that face, kept as the room's, was
/// the known face while the eyes' known file named someone else, not
/// another face (lamp-29e7, 28 Oct 18:54:27, request 35: the file changed
/// just before).
pub fn seen_speaking_note(
    speakers: &[String],
    theirs: bool,
    known_face: Option<&str>,
) -> Option<String> {
    match (speakers, theirs) {
        ([id], false) if known_face == Some(id.as_str()) => Some(format!(
            "seen speaking it: the known face (id {id}), but the eyes' known file named someone else when read"
        )),
        ([id], false) if known_face.is_none() => Some(format!(
            "seen speaking it: a face (id {id}), with no known face read for this boot of the eyes"
        )),
        ([], true) => None,
        ([], false) => Some("no face was seen speaking it (LAMP_PERSON_SCOPE_SEEN)".to_owned()),
        ([id], true) => Some(format!(
            "seen speaking it: the known person's face (id {id})"
        )),
        ([id], false) => Some(format!(
            "seen speaking it: a face not the known person's (id {id})"
        )),
        (ids, _) => Some(format!(
            "seen speaking it: {} faces (ids {})",
            ids.len(),
            ids.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{Face, Speaking};

    /// #4339: no face but theirs (or none) in any of the eyes' looks
    /// through a line, and at least one look there; another face in one, or
    /// no look at all, is not.
    /// The Reviewer-lead's P2 on #4479: a line is carried unseen only with
    /// their face in view through it or within
    /// [`crate::second_provider::CARRY_UNSEEN_WITHIN`] before it began: 3 s
    /// after it left, carried; 8 s after, the room's; an empty view from the
    /// start, the room's.
    #[test]
    fn an_unseen_line_carries_only_soon_after_their_face_was_in_view() {
        let within = crate::second_provider::CARRY_UNSEEN_WITHIN;
        let face = Face {
            id: "f1".to_owned(),
            xmin: 0.1,
            ymin: 0.1,
            xmax: 0.3,
            ymax: 0.4,
            confidence: 1.0,
            yaw: None,
            facing: None,
            speaking: None,
            follows: None,
        };
        let start = Instant::now();
        let eyes = SpeakingFaces::default();
        eyes.look(start, Some((("b", 1), &Faces { faces: vec![face] })));
        let left = start + Duration::from_millis(500);
        eyes.look(left, Some((("b", 2), &Faces { faces: vec![] })));
        let seen = |begun: Instant| {
            eyes.face_seen_between(Some("b/f1"), begun - within, begun + Duration::from_secs(1))
        };
        assert!(seen(left + Duration::from_secs(3)), "3 s after: carried");
        assert!(
            !seen(left + Duration::from_secs(8)),
            "8 s after: the room's"
        );
        assert!(seen(start), "still in view through the line");
        assert!(
            !eyes.face_seen_between(None, start - within, left),
            "no face"
        );
        let empty = SpeakingFaces::default();
        empty.look(start, Some((("b", 1), &Faces { faces: vec![] })));
        assert!(
            !empty.face_seen_between(Some("b/f1"), start - within, start + Duration::from_secs(1)),
            "an empty view from the start: the room's"
        );
    }

    #[test]
    fn no_other_face_in_view_needs_a_look_and_no_face_but_theirs() {
        let face = |id: &str| Face {
            id: id.to_owned(),
            xmin: 0.1,
            ymin: 0.1,
            xmax: 0.3,
            ymax: 0.4,
            confidence: 1.0,
            yaw: None,
            facing: None,
            speaking: Some(Speaking {
                state: SpeakingState::No,
                score: 0.9,
                from: None,
            }),
            follows: None,
        };
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        assert!(
            !eyes.no_other_face_in_view(Some("b/f1"), from, Instant::now()),
            "no look"
        );
        eyes.look(Instant::now(), Some((("b", 1), &Faces { faces: vec![] })));
        eyes.look(
            Instant::now(),
            Some((
                ("b", 2),
                &Faces {
                    faces: vec![face("f1")],
                },
            )),
        );
        let to = Instant::now();
        assert!(eyes.no_other_face_in_view(Some("b/f1"), from, to));
        assert!(
            !eyes.no_other_face_in_view(None, from, to),
            "f1 is someone's face"
        );
        assert!(!eyes.no_other_face_in_view(Some("b/f2"), from, to));
        let later = Instant::now();
        eyes.look(Instant::now(), Some((("b", 3), &Faces { faces: vec![] })));
        assert!(
            eyes.no_other_face_in_view(None, later, Instant::now()),
            "nobody in view"
        );
        eyes.look(
            Instant::now(),
            Some((
                ("b", 4),
                &Faces {
                    faces: vec![face("f1"), face("f2")],
                },
            )),
        );
        assert!(
            !eyes.no_other_face_in_view(Some("b/f1"), from, Instant::now()),
            "f2 too"
        );
        // Looked through, whoever was tracked; not before any look.
        assert!(
            eyes.looked_between(from, Instant::now()),
            "two faces looked at"
        );
        let after = Instant::now();
        assert!(!eyes.looked_between(after, Instant::now()), "no look since");
    }

    /// `LAMP_PERSON_SCOPE_ONE_FACE` (the launch testers' item 1 on #3997):
    /// the known face in at least `FEWEST_LOOKS` looks and no other face in
    /// any, its mouth never read speaking; empty looks between are no other
    /// face. One look with a second face, an unknown face alone, too few
    /// looks, or another boot's `f1`, and it is not.
    /// The a0ae name-forget report (2 Nov): with `LAMP_FACES_AT_DESK_ONLY`,
    /// a tiny, unsure face that comes and goes beside the known face is no
    /// other person, so the one-face keep and the carry's no other face
    /// act; a second person at the desk still stops both, and off, the tiny
    /// face stops them as before. A tiny face seen speaking still counts as
    /// speaking.
    #[test]
    fn faces_not_at_the_desk_are_no_other_face() {
        let face = |id: &str, width: f32, confidence: f32| Face {
            id: id.to_owned(),
            xmin: 0.3,
            ymin: 0.2,
            xmax: 0.3 + width,
            ymax: 0.2 + width * 4.0 / 3.0,
            confidence,
            yaw: None,
            facing: None,
            speaking: Some(Speaking {
                state: SpeakingState::No,
                score: 0.9,
                from: None,
            }),
            follows: None,
        };
        let looks = |frames: &[&[(&str, f32, f32)]], desk_only: bool| {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            for (seq, look) in frames.iter().enumerate() {
                let faces = Faces {
                    faces: look
                        .iter()
                        .map(|(id, width, confidence)| face(id, *width, *confidence))
                        .collect(),
                };
                eyes.look_at_desk(
                    Instant::now(),
                    Some((("b", seq as u64 + 1), &faces)),
                    desk_only,
                );
            }
            (eyes, from, Instant::now())
        };
        let a0ae: &[&[(&str, f32, f32)]] = &[
            &[("f802", 0.15, 0.88)],
            &[("f802", 0.13, 0.80), ("f816", 0.06, 0.67)],
            &[
                ("f802", 0.12, 0.87),
                ("f816", 0.04, 0.78),
                ("f817", 0.02, 0.61),
            ],
            &[("f802", 0.15, 0.88), ("f818", 0.06, 0.67)],
            &[("f802", 0.14, 0.86)],
        ];
        let (eyes, from, to) = looks(a0ae, true);
        assert!(eyes.one_face_in_view("b/f802", from, to), "one face kept");
        assert!(
            eyes.no_other_face_in_view(Some("b/f802"), from, to),
            "no other face for the carry"
        );
        assert!(
            eyes.sight_told(Some("b/f802"), from, to)
                .contains("5 looks, 5 with one face")
                && eyes
                    .sight_told(Some("b/f802"), from, to)
                    .ends_with("; 4 faces not at the desk")
        );
        // The tiny faces are still tracked.
        assert!(eyes.in_view_at("b/f816", to));
        let (eyes, from, to) = looks(a0ae, false);
        assert!(!eyes.one_face_in_view("b/f802", from, to), "off: as before");
        assert!(!eyes.no_other_face_in_view(Some("b/f802"), from, to));
        // A second person at the desk is another face.
        let two: &[&[(&str, f32, f32)]] = &[
            &[("f802", 0.14, 0.85), ("f9", 0.12, 0.82)],
            &[("f802", 0.14, 0.85), ("f9", 0.12, 0.82)],
            &[("f802", 0.14, 0.85), ("f9", 0.12, 0.82)],
        ];
        let (eyes, from, to) = looks(two, true);
        assert!(!eyes.one_face_in_view("b/f802", from, to));
        assert!(!eyes.no_other_face_in_view(Some("b/f802"), from, to));
    }

    #[test]
    fn one_face_in_view_needs_their_face_in_enough_looks_and_no_other_in_any() {
        let face = |id: &str| Face {
            id: id.to_owned(),
            xmin: 0.1,
            ymin: 0.1,
            xmax: 0.3,
            ymax: 0.4,
            confidence: 1.0,
            yaw: None,
            facing: None,
            speaking: Some(Speaking {
                state: SpeakingState::No,
                score: 0.9,
                from: None,
            }),
            follows: None,
        };
        let looks = |frames: &[&[&str]]| {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            for (seq, ids) in frames.iter().enumerate() {
                let faces = Faces {
                    faces: ids.iter().map(|id| face(id)).collect(),
                };
                eyes.look(Instant::now(), Some((("b", seq as u64 + 1), &faces)));
            }
            (eyes, from, Instant::now())
        };
        let alone = |frames: &[&[&str]]| {
            let (eyes, from, to) = looks(frames);
            eyes.one_face_in_view("b/f1", from, to)
        };
        assert!(
            alone(&[&["f1"], &["f1"], &[], &["f1"]]),
            "theirs, never speaking"
        );
        assert!(
            !alone(&[&["f1"], &["f1"], &["f1", "f2"], &["f1"]]),
            "a second face in one look"
        );
        assert!(!alone(&[&["f2"], &["f2"], &["f2"]]), "an unknown face");
        assert!(!alone(&[&["f1"], &["f1"]]), "too few looks");
        assert!(!alone(&[&[], &[], &[]]), "no face");
        let (eyes, from, to) = looks(&[&["f1"], &["f1"], &["f1"]]);
        assert!(
            !eyes.one_face_in_view("other-boot/f1", from, to),
            "another boot's f1"
        );
    }

    #[test]
    fn with_person_scope_seen_a_line_no_face_was_seen_speaking_is_not_theirs() {
        let lan = crate::known_faces::memory_person("A");
        let known = || Some(("A".to_owned(), "f1".to_owned()));
        let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        // On: only their own face seen speaking keeps it theirs.
        assert!(not_theirs(&ids(&[]), &lan, known(), true));
        assert!(!not_theirs(&ids(&["f1"]), &lan, known(), true));
        assert!(not_theirs(&ids(&["f2"]), &lan, known(), true));
        assert!(not_theirs(&ids(&["f1", "f2"]), &lan, known(), true));
        assert!(not_theirs(&ids(&["f1"]), &lan, None, true));
        // Off: exactly `someone_elses`.
        for speakers in [&[][..], &["f1"], &["f2"], &["f1", "f2"]] {
            for pair in [known(), None] {
                assert_eq!(
                    not_theirs(&ids(speakers), &lan, pair.clone(), false),
                    someone_elses(&ids(speakers), &lan, pair),
                );
            }
        }
    }

    #[test]
    fn seen_speaking_note_names_face_ids_only() {
        let name = "Alex";
        let known = || Some((name.to_owned(), "b/f1".to_owned()));
        let lan = crate::known_faces::memory_person(name);
        let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        let note = |speakers: &[&str]| {
            let speakers = ids(speakers);
            seen_speaking_note(
                &speakers,
                !not_theirs(&speakers, &lan, known(), true),
                Some("b/f1"),
            )
        };
        assert_eq!(
            note(&["b/f1"]).as_deref(),
            Some("seen speaking it: the known person's face (id b/f1)")
        );
        assert_eq!(
            note(&["b/f2"]).as_deref(),
            Some("seen speaking it: a face not the known person's (id b/f2)")
        );
        assert_eq!(
            note(&["b/f1", "b/f2"]).as_deref(),
            Some("seen speaking it: 2 faces (ids b/f1, b/f2)")
        );
        assert_eq!(
            note(&[]).as_deref(),
            Some("no face was seen speaking it (LAMP_PERSON_SCOPE_SEEN)")
        );
        // Off, no face seen leaves it theirs: nothing to tell.
        assert_eq!(seen_speaking_note(&[], true, None), None);
        // lamp-29e7's request 35: the known face itself, kept as the room's
        // because the known file named someone else as it was read; and a
        // face with no known face read at all.
        assert_eq!(
            seen_speaking_note(&ids(&["b/f1"]), false, Some("b/f1")).as_deref(),
            Some(
                "seen speaking it: the known face (id b/f1), but the eyes' known file named someone else when read"
            )
        );
        assert_eq!(
            seen_speaking_note(&ids(&["b/f2"]), false, None).as_deref(),
            Some(
                "seen speaking it: a face (id b/f2), with no known face read for this boot of the eyes"
            )
        );
        for speakers in [&[][..], &["b/f1"], &["b/f2"], &["b/f1", "b/f2"]] {
            let note = note(speakers).unwrap_or_default();
            assert!(!note.contains(name) && !note.contains(&lan), "{note}");
        }
    }

    fn faces(speaking: &[(&str, SpeakingState)]) -> Faces {
        Faces {
            faces: speaking
                .iter()
                .map(|(id, state)| Face {
                    id: (*id).to_owned(),
                    xmin: 0.2,
                    ymin: 0.2,
                    xmax: 0.4,
                    ymax: 0.5,
                    confidence: 1.0,
                    yaw: None,
                    facing: None,
                    speaking: Some(Speaking {
                        state: *state,
                        score: 0.9,
                        from: None,
                    }),
                    follows: None,
                })
                .collect(),
        }
    }

    fn seen(looks: &[&[(&str, SpeakingState)]]) -> (SpeakingFaces, Instant, Instant) {
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        for (index, look) in looks.iter().enumerate() {
            eyes.look(
                from + EVERY * index as u32,
                Some((("b", index as u64), &faces(look))),
            );
        }
        (eyes, from, from + EVERY * looks.len() as u32)
    }

    use SpeakingState::{No, Unknown, Yes};

    /// #4885: a line spanning a new passing id the eyes say follows the old
    /// (`f2` follows `f1`, with their hold and re-found check on) is one
    /// face's: no other face in view, and their face alone seen speaking it.
    /// Without the link it is two faces, narrowed to the room as before; and
    /// a look with both ids in it is never one face.
    #[test]
    fn a_line_across_a_followed_id_is_one_faces() {
        let lan = crate::known_faces::memory_person("A");
        let known = Some(("A".to_owned(), "b/f2".to_owned()));
        let line = |link: bool| {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            for index in 0..4u32 {
                let mut look = faces(&[(if index < 2 { "f1" } else { "f2" }, Yes)]);
                if link && index >= 2 {
                    look.faces[0].follows = Some("f1".to_owned());
                }
                eyes.look(from + EVERY * index, Some((("b", u64::from(index)), &look)));
            }
            let to = from + EVERY * 4;
            (
                eyes.no_other_face_in_view(Some("b/f2"), from, to),
                eyes.speakers_with_boot(from, to),
            )
        };
        let (alone, speakers) = line(true);
        assert!(alone);
        assert_eq!(speakers, ["b/f2"]);
        assert!(!not_theirs(&speakers, &lan, known.clone(), true));
        let (alone, speakers) = line(false);
        assert!(!alone);
        assert_eq!(speakers, ["b/f1", "b/f2"]);
        assert!(not_theirs(&speakers, &lan, known.clone(), true));
        // The old id in the same look as the new: two faces.
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        eyes.look(from, Some((("b", 0), &faces(&[("f1", Yes)]))));
        let mut both = faces(&[("f1", Yes), ("f2", Yes)]);
        both.faces[1].follows = Some("f1".to_owned());
        eyes.look(from + EVERY, Some((("b", 1), &both)));
        assert!(!eyes.no_other_face_in_view(Some("b/f2"), from, from + EVERY * 2));
    }

    /// The looks of a line on `f15` re-found twice: `f15` speaking, then
    /// `f16` following `f15`, a look with no face, `f17` following `f16`,
    /// and `last` after them.
    fn re_found_twice(last: &[(&str, SpeakingState)]) -> (SpeakingFaces, Instant, Instant) {
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        let mut first = faces(&[("f16", Yes)]);
        first.faces[0].follows = Some("f15".to_owned());
        let mut second = faces(&[("f17", Yes)]);
        second.faces[0].follows = Some("f16".to_owned());
        let looks = [
            faces(&[("f15", Yes)]),
            faces(&[("f15", Yes)]),
            faces(&[("f15", Yes)]),
            faces(&[("f15", Yes)]),
            first,
            faces(&[]),
            faces(last),
            second,
        ];
        for (index, look) in looks.iter().enumerate() {
            eyes.look(
                from + EVERY * index as u32,
                Some((("b", index as u64), look)),
            );
        }
        (eyes, from, from + EVERY * looks.len() as u32)
    }

    /// #4966 (lamp-52e6, try 7): one face re-found twice through a line,
    /// `f15` to `f16` to `f17`, the eyes saying only the last hop in the
    /// last look; their known file moved on to `f17`. The line is theirs:
    /// each hop the eyes said is kept until they end it.
    #[test]
    fn a_line_on_a_face_re_found_twice_is_the_known_persons() {
        let lan = crate::known_faces::memory_person("A");
        let known = Some(("A".to_owned(), "b/f17".to_owned()));
        let (eyes, from, to) = re_found_twice(&[]);
        let speakers = eyes.speakers_with_boot(from, to);
        assert_eq!(speakers, ["b/f17"]);
        assert!(!not_theirs(&speakers, &lan, known, true));
        assert!(eyes.no_other_face_in_view(Some("b/f17"), from, to));
    }

    /// #4966: the same chain broken by the old id seen again (`f15` back
    /// between the hops): `f15` is not `f17`, and the line is the room's.
    #[test]
    fn a_chain_the_eyes_broke_does_not_map() {
        let lan = crate::known_faces::memory_person("A");
        let known = Some(("A".to_owned(), "b/f17".to_owned()));
        let (eyes, from, to) = re_found_twice(&[("f15", Yes)]);
        let speakers = eyes.speakers_with_boot(from, to);
        assert!(speakers.contains(&"b/f15".to_owned()), "{speakers:?}");
        assert!(not_theirs(&speakers, &lan, known, true));
        assert!(!eyes.no_other_face_in_view(Some("b/f17"), from, to));
    }

    /// #4966: a known file in a fresh folder naming A on `f1`, and the
    /// eyes' looks through a line: `f1` speaking (with `on_f1`), then `f2`
    /// speaking, which with `link` the eyes say follows `f1`. Gives the
    /// line's end.
    fn follow_seen(
        tag: &str,
        link: bool,
        on_f1: bool,
    ) -> (std::sync::Arc<SpeakingFaces>, PathBuf, Instant, Instant) {
        let dir = std::env::temp_dir().join(format!(
            "speaking-faces-decided-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let eyes = std::sync::Arc::new(SpeakingFaces::default());
        // The line ends now, as the voice reads it.
        let from = Instant::now() - EVERY * 4;
        for index in 0..4u32 {
            let mut look = faces(&[(if on_f1 && index < 2 { "f1" } else { "f2" }, Yes)]);
            if link && index >= 2 {
                look.faces[0].follows = Some("f1".to_owned());
            }
            eyes.look(from + EVERY * index, Some((("b", u64::from(index)), &look)));
        }
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        (eyes, known, from, from + EVERY * 4)
    }

    /// The line's known pair once the eyes decided, and whether the line
    /// is theirs to `not_theirs`.
    async fn decided(
        eyes: &std::sync::Arc<SpeakingFaces>,
        known: &Path,
        from: Instant,
        to: Instant,
        within: Duration,
    ) -> (Option<String>, bool) {
        let pair = eyes.known_with_boot(known);
        let pair = known_once_decided(eyes, known, pair, to, within).await;
        let face = pair.as_ref().map(|(_, face)| face.clone());
        let theirs = !not_theirs(
            &eyes.speakers_with_boot(from, to),
            &crate::known_faces::memory_person("Lan"),
            pair,
            true,
        );
        (face, theirs)
    }

    /// #4966, the Reviewer-lead's P2 on #4967: A (known, on `f1`) gets up
    /// and B sits in the same spot as `f2`, which the eyes say follows
    /// `f1`; B's print never passes, so the known file stays on `f1`. B's
    /// line spoken on `f2` alone, or one begun on `f1`, is the room's. The
    /// line is the room's: not told or kept as A's, so its reply reads none
    /// of A's memory. The link alone never makes it A's.
    #[tokio::test]
    async fn a_follower_the_eyes_never_printed_as_theirs_leaves_the_line_the_rooms() {
        let (eyes, known, from, to) = follow_seen("never", true, false);
        let within = Duration::from_millis(300);
        let (face, theirs) = decided(&eyes, &known, from, to, within).await;
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(!theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        let (eyes, known, from, to) = follow_seen("never-f1", true, true);
        let (face, theirs) = decided(&eyes, &known, from, to, within).await;
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(!theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        // The eyes let them go (no print, or under 0.30): the file goes.
        let (eyes, known, from, to) = follow_seen("let-go", true, true);
        let file = known.clone();
        let gone = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            std::fs::remove_file(known_face_of(&file)).unwrap();
        });
        let (face, theirs) = decided(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        gone.join().unwrap();
        assert_eq!(face, None);
        assert!(!theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #4966: A re-found, a line spoken on `f1` and carried on across the
    /// re-find on `f2` (Fifth's case on #4967). Their print of `f2` passes
    /// the eyes' hold, which rewrites the known face to `f2` within the
    /// bound: the line is A's.
    #[tokio::test]
    async fn a_follower_the_eyes_printed_as_theirs_makes_the_line_theirs() {
        let (eyes, known, from, to) = follow_seen("printed", true, true);
        let file = known.clone();
        let carried = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            std::fs::write(known_face_of(&file), "f2\n").unwrap();
        });
        let (face, theirs) = decided(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        carried.join().unwrap();
        assert_eq!(face.as_deref(), Some("b/f2"));
        assert!(theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// The known face `f1` seen speaking through four looks, the line
    /// ending now, and the eyes' known file naming them on `f1`.
    fn held_seen(tag: &str) -> (std::sync::Arc<SpeakingFaces>, PathBuf, Instant, Instant) {
        let dir = std::env::temp_dir().join(format!(
            "speaking-faces-unsure-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let eyes = std::sync::Arc::new(SpeakingFaces::default());
        let from = Instant::now() - EVERY * 4;
        for index in 0..4u32 {
            let look = faces(&[("f1", Yes)]);
            eyes.look(from + EVERY * index, Some((("b", u64::from(index)), &look)));
        }
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        (eyes, known, from, from + EVERY * 4)
    }

    /// The line's known pair once an unsure hold is decided
    /// ([`known_once_printed`]), and whether the line is theirs.
    async fn printed_pair(
        eyes: &std::sync::Arc<SpeakingFaces>,
        known: &Path,
        from: Instant,
        to: Instant,
        within: Duration,
    ) -> (Option<String>, bool) {
        let pair = eyes.known_with_boot(known);
        let read = {
            let (eyes, file) = (eyes.clone(), known.to_owned());
            move || eyes.known_with_boot(&file)
        };
        let pair = known_once_printed(known, pair, to, within, read).await;
        let face = pair.as_ref().map(|(_, face)| face.clone());
        let theirs = !not_theirs(
            &eyes.speakers_with_boot(from, to),
            &crate::known_faces::memory_person("Lan"),
            pair,
            true,
        );
        (face, theirs)
    }

    /// The eyes' unsure hold as `who::hold_unsure` leaves it: the known file
    /// and its face moved to `-unsure` and `-unsure-face`.
    fn hold_unsure(known: &Path) {
        let unsure = known_unsure_of(known);
        std::fs::rename(known, &unsure).unwrap();
        std::fs::rename(known_face_of(known), known_face_of(&unsure)).unwrap();
    }

    /// The eyes' print kept them: the known file written back, then the
    /// hold's files removed (`who::keep_known`).
    fn print_keeps(known: &Path) {
        std::fs::write(known, "Lan\n").unwrap();
        std::fs::write(known_face_of(known), "f1\n").unwrap();
        let unsure = known_unsure_of(known);
        std::fs::remove_file(known_face_of(&unsure)).unwrap();
        std::fs::remove_file(unsure).unwrap();
    }

    /// #5738: with no unsure hold nothing waits, and a line spoken by the
    /// known face stays theirs.
    #[tokio::test]
    async fn a_sure_hold_ties_the_line_at_once() {
        let (eyes, known, from, to) = held_seen("none");
        let (face, theirs) = printed_pair(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(theirs);
        assert!(Instant::now().saturating_duration_since(to) < Duration::from_millis(300));
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #5738, the Reviewer-lead's P2 on #5743: while the eyes hold A unsure,
    /// a reader of the known file (the first voice's name note, memory's
    /// person, the second voice's face cache) sees nobody known.
    #[test]
    fn an_unsure_hold_names_nobody_to_a_reader_of_the_known_file() {
        let (_eyes, known, ..) = held_seen("reader");
        hold_unsure(&known);
        assert!(known_unsure(&known));
        assert_eq!(crate::known_faces::known_now(&known), None);
        assert_eq!(known_with_face(&known), None);
        // The second voice's face cache alone reads the held name, so the
        // hold moving aside and back is no change there (V2's point on #5743).
        let held = known_or_held(&known);
        assert_eq!(held, known_unsure_of(&known));
        assert_eq!(crate::known_faces::known_now(&held).as_deref(), Some("Lan"));
        assert_eq!(crate::known_faces::KnownMark::read(&held), {
            std::fs::write(known.with_file_name("probe"), "Lan\n").unwrap();
            crate::known_faces::KnownMark::read(&known.with_file_name("probe"))
        });
        print_keeps(&known);
        assert_eq!(known_or_held(&known), known);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #5738, the Reviewer-lead's condition on #5546: the eyes hold A
    /// unsure (their face near after a gap, not printed yet). A's line
    /// waits; the next print reads A and the known file is back: the line
    /// is A's. The print reads someone else and the eyes let go: the room's.
    #[tokio::test]
    async fn an_unsure_hold_waits_for_the_print_that_decides_it() {
        let (eyes, known, from, to) = held_seen("theirs");
        hold_unsure(&known);
        let file = known.clone();
        let printed = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            print_keeps(&file);
        });
        let (face, theirs) = printed_pair(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        printed.join().unwrap();
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        let (eyes, known, from, to) = held_seen("let-go");
        hold_unsure(&known);
        let file = known.clone();
        let let_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let unsure = known_unsure_of(&file);
            std::fs::remove_file(known_face_of(&unsure)).unwrap();
            std::fs::remove_file(unsure).unwrap();
        });
        let (face, theirs) = printed_pair(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        let_go.join().unwrap();
        assert_eq!(face, None);
        assert!(!theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #5738: no print at the bound (nobody near, or a face turned away):
    /// the wait ends there and the line is the room's.
    #[tokio::test]
    async fn an_unsure_hold_undecided_at_the_bound_leaves_the_line_the_rooms() {
        let (eyes, known, from, to) = held_seen("bound");
        hold_unsure(&known);
        let within = Duration::from_millis(400);
        let (face, theirs) = printed_pair(&eyes, &known, from, to, within).await;
        let past = Instant::now().saturating_duration_since(to);
        assert!(
            past >= within && past < within + Duration::from_millis(300),
            "{past:?}"
        );
        assert_eq!(face, None);
        assert!(!theirs);
        // A mark the eyes stopped writing is no hold of theirs.
        let stale = std::fs::File::options()
            .write(true)
            .open(known_unsure_of(&known))
            .unwrap();
        stale
            .set_modified(SystemTime::now() - crate::known_faces::FRESH)
            .unwrap();
        assert!(!known_unsure(&known));
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #4966: the eyes undecided at the bound: the wait ends there, and the
    /// line is the room's.
    #[tokio::test]
    async fn an_undecided_follow_waits_no_longer_than_the_bound() {
        let (eyes, known, from, to) = follow_seen("bound", true, true);
        let within = Duration::from_millis(400);
        let (face, theirs) = decided(&eyes, &known, from, to, within).await;
        let past = Instant::now().saturating_duration_since(to);
        assert!(
            past >= within && past < within + Duration::from_millis(300),
            "{past:?}"
        );
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(!theirs);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #4966: with `LAMP_FACE_KNOWN_HOLD` or `LAMP_FACE_REFOUND` off no
    /// `follows` is said: nothing waits, the known face is as read, and a
    /// line spoken on a new id is the room's as before. A link only in an
    /// older look, or whose old id is in the last look, waits for nothing.
    #[tokio::test]
    async fn with_no_follow_pending_nothing_waits() {
        let (eyes, known, from, to) = follow_seen("off", false, true);
        let started = Instant::now();
        let (face, theirs) = decided(&eyes, &known, from, to, FOLLOW_DECIDED_WITHIN).await;
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(face.as_deref(), Some("b/f1"));
        assert!(!theirs);
        let mut older = faces(&[("f2", Yes)]);
        older.faces[0].follows = Some("f1".to_owned());
        eyes.look(to, Some((("b", 4), &older)));
        eyes.look(to + EVERY, Some((("b", 5), &faces(&[("f2", Yes)]))));
        assert!(eyes.follow_of("b/f1", to));
        assert!(!eyes.follow_of("b/f1", to + EVERY));
        let mut both = faces(&[("f2", Yes), ("f1", No)]);
        both.faces[0].follows = Some("f1".to_owned());
        eyes.look(to + EVERY * 2, Some((("b", 6), &both)));
        assert!(!eyes.follow_of("b/f1", to + EVERY * 2));
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// lamp-3e83 (stable 8163, 28 Oct 16:30): the known person alone at the
    /// lamp, their mouth not read speaking often enough, had about half
    /// their lines kept as the room's, and the lamp lost their name. Their
    /// face the one in view through the line, read speaking at least once
    /// and still in under half, names the line for its reply; its scope stays
    /// the room's (reviewer-three's P2 on #4090: a partner off camera while
    /// the owner nods). Another face in any look, too few looks, their face
    /// read still or never read speaking, another person known, or a face
    /// seen speaking it names nothing.
    #[test]
    fn a_line_no_face_was_seen_speaking_is_named_when_their_face_alone_was_in_view() {
        let lan = crate::known_faces::memory_person("A");
        let known = Some(("A".to_owned(), "b/f1".to_owned()));
        // Unread or read speaking now and then, never the half: no speaker.
        let (eyes, from, to) = seen(&[
            &[("f1", Unknown)],
            &[("f1", Yes)],
            &[("f1", Unknown)],
            &[("f1", Unknown)],
        ]);
        let speakers = eyes.speakers_with_boot(from, to);
        assert!(speakers.is_empty());
        let alone = eyes.alone_in_view("b/f1", from, to);
        assert!(alone);
        // The room's scope, as before, but named for the reply.
        assert!(not_theirs(&speakers, &lan, known.clone(), true));
        assert!(named_alone(&speakers, &lan, known.as_ref(), alone));
        // Not alone, someone else known now, or nobody: not named.
        assert!(!named_alone(&speakers, &lan, known.as_ref(), false));
        let other = Some(("B".to_owned(), "b/f1".to_owned()));
        assert!(!named_alone(&speakers, &lan, other.as_ref(), alone));
        assert!(!named_alone(&speakers, &lan, None, alone));
        // A face seen speaking it: decided by `not_theirs`, never named here.
        assert!(!named_alone(
            &["b/f2".to_owned()],
            &lan,
            known.as_ref(),
            true
        ));
        // A second face in one look: not alone.
        let (eyes, from, to) = seen(&[
            &[("f1", Unknown)],
            &[("f1", Unknown), ("f2", Unknown)],
            &[("f1", Unknown)],
        ]);
        assert!(!eyes.alone_in_view("b/f1", from, to));
        // Too few looks.
        let (eyes, from, to) = seen(&[&[("f1", Unknown)], &[("f1", Unknown)]]);
        assert!(!eyes.alone_in_view("b/f1", from, to));
        // The owner silent, their mouth never read speaking, only unread,
        // while another voice speaks off camera or a television plays: the
        // room's (the Reviewer-lead's P1 on #4090).
        let (eyes, from, to) = seen(&[&[("f1", Unknown)][..]; 4]);
        assert!(eyes.speakers_with_boot(from, to).is_empty());
        assert!(!eyes.alone_in_view("b/f1", from, to));
        // Their mouth read still through most of it: a voice off camera.
        let (eyes, from, to) = seen(&[
            &[("f1", No)],
            &[("f1", No)],
            &[("f1", Unknown)],
            &[("f1", No)],
        ]);
        assert!(!eyes.alone_in_view("b/f1", from, to));
        // Another face alone, or theirs from another boot: not theirs.
        let (eyes, from, to) = seen(&[&[("f2", Unknown)][..]; 4]);
        assert!(!eyes.alone_in_view("b/f1", from, to));
        assert!(!eyes.alone_in_view("a/f2", from, to));
        // Nobody in view at all.
        let (eyes, from, to) = seen(&[&[][..]; 4]);
        assert!(!eyes.alone_in_view("b/f1", from, to));
    }

    /// lamp-06c1 (5 Nov 11:12, request 6, `LAMP_SECOND_NAME_ONE_FACE`): the
    /// test face alone in 11 of 11 looks, read speaking in 5 and still in 6,
    /// so [`SpeakingFaces::alone_in_view`] failed and the lamp had no name for
    /// her. [`named_one_face`] reads only the one-face count: their face the
    /// one in view, mouth never read speaking, no other voice: named. A
    /// second face in a look, another voice, someone else known, or a face
    /// seen speaking it that isn't theirs: not named.
    #[test]
    fn a_lone_known_face_is_named_by_the_one_face_count_whatever_its_mouth() {
        let lan = crate::known_faces::memory_person("A");
        let known = Some(("A".to_owned(), "b/f1".to_owned()));
        // As on lamp-06c1: read still in 6 of 11, speaking in 5, no speaker.
        let mut looks: Vec<&[(&str, SpeakingState)]> = vec![&[("f1", No)]; 6];
        looks.extend(vec![&[("f1", Yes)][..]; 5]);
        let (eyes, from, to) = seen(&looks);
        assert!(
            !eyes.alone_in_view("b/f1", from, to),
            "the mouth rule fails"
        );
        assert!(eyes.speakers_with_boot(from, to).is_empty());
        let one = eyes.one_face_in_view("b/f1", from, to);
        assert!(one);
        let speakers: Vec<String> = Vec::new();
        assert!(named_one_face(&speakers, &lan, known.as_ref(), one, false));
        // Never read speaking at all, only still: still named.
        let (eyes, from, to) = seen(&[&[("f1", No)][..]; 4]);
        assert!(eyes.speakers_with_boot(from, to).is_empty());
        let one = eyes.one_face_in_view("b/f1", from, to);
        assert!(one);
        assert!(named_one_face(&speakers, &lan, known.as_ref(), one, false));
        // Another voice heard in the conversation: not named.
        assert!(!named_one_face(&speakers, &lan, known.as_ref(), one, true));
        // lamp-06c1 (build 10343, request 42): alone in 12 of 12 looks, read
        // speaking in 2, still in 4, unread in 6. The mouth rule holds too,
        // and the one-face count still names her.
        let mut looks: Vec<&[(&str, SpeakingState)]> = vec![&[("f1", Yes)]; 2];
        looks.extend(vec![&[("f1", No)][..]; 4]);
        looks.extend(vec![&[("f1", Unknown)][..]; 6]);
        let (eyes, from, to) = seen(&looks);
        assert!(eyes.speakers_with_boot(from, to).is_empty());
        assert!(eyes.alone_in_view("b/f1", from, to), "the mouth rule holds");
        assert!(named_alone(&speakers, &lan, known.as_ref(), true));
        let one = eyes.one_face_in_view("b/f1", from, to);
        assert!(one);
        assert!(named_one_face(&speakers, &lan, known.as_ref(), one, false));
        // Someone else known now, or nobody: not named.
        let other = Some(("B".to_owned(), "b/f1".to_owned()));
        assert!(!named_one_face(&speakers, &lan, other.as_ref(), one, false));
        assert!(!named_one_face(&speakers, &lan, None, one, false));
        // A face seen speaking it that isn't theirs: not named.
        assert!(!named_one_face(
            &["b/f2".to_owned()],
            &lan,
            known.as_ref(),
            one,
            false
        ));
        // A second face in one look: not the one face, not named.
        let (eyes, from, to) = seen(&[
            &[("f1", No)],
            &[("f1", No), ("f2", No)],
            &[("f1", No)],
            &[("f1", No)],
        ]);
        let one = eyes.one_face_in_view("b/f1", from, to);
        assert!(!one);
        assert!(!named_one_face(&speakers, &lan, known.as_ref(), one, false));
    }

    /// lamp-29e7 (28 Oct 18:48): 18 of 20 lines read with no face
    /// speaking, and the journal could not say why. Counts only: looks,
    /// looks with one face, and the known face's reads.
    #[test]
    fn the_sight_of_a_line_no_face_was_seen_speaking_is_told_in_counts() {
        let (eyes, from, to) = seen(&[
            &[("f1", No)],
            &[("f1", No), ("f2", Unknown)],
            &[("f1", Unknown)],
            &[("f1", Yes)],
        ]);
        assert_eq!(
            eyes.sight_told(Some("b/f1"), from, to),
            "4 looks, 3 with one face; the known face in 4, read speaking in 1, still in 2, unread in 1"
        );
        assert_eq!(
            eyes.sight_told(None, from, to),
            "4 looks, 3 with one face; the known face in 0, read speaking in 0, still in 0, unread in 0"
        );
    }

    #[test]
    fn remember_speaker_rejects_a_competitor_seen_once() {
        let (eyes, from, to) = seen(&[
            &[("f1", Yes)],
            &[("f1", Yes)],
            &[("f1", Yes), ("f2", Yes)],
            &[("f1", Yes)],
            &[("f1", Yes)],
        ]);
        assert_eq!(eyes.speakers_with_boot(from, to), ["b/f1"]);
        assert_eq!(eyes.enrollment_speaker(from, to), None);
    }

    #[test]
    fn remember_speaker_requires_positive_evidence_in_one_boot_and_the_whole_window() {
        let (eyes, from, to) = seen(&[
            &[("f1", Yes)],
            &[("f1", Yes)],
            &[("f1", Yes)],
            &[("f1", No)],
        ]);
        assert_eq!(eyes.enrollment_speaker(from, to), Some("b/f1".into()));
        for looks in [
            vec![&[("f1", Unknown)][..]; 4],
            vec![&[("f1", No)][..]; 4],
            vec![&[("f1", Yes)][..]; 2],
            vec![&[][..]; 4],
        ] {
            let (eyes, from, to) = seen(&looks);
            assert_eq!(eyes.enrollment_speaker(from, to), None);
        }
        assert_eq!(eyes.enrollment_speaker(from, from + KEPT + EVERY), None);
        eyes.look(to, Some((("restarted", 1), &faces(&[("f1", No)]))));
        assert_eq!(eyes.enrollment_speaker(from, to), None);
        eyes.forget();
        assert_eq!(eyes.enrollment_speaker(from, to), None);
    }

    /// #2287's follow-up: of the faces speaking, only those turned to the
    /// lamp in at least half the looks are its own person's; one turned away,
    /// or not said, is the room's. The speakers are as before.
    #[test]
    fn a_face_speaking_turned_to_the_lamp_is_told_apart_from_the_room() {
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        for index in 0..4u32 {
            let mut seen = faces(&[("f1", Yes), ("f2", Yes), ("f3", Yes)]);
            seen.faces[0].facing = Some(true);
            seen.faces[1].facing = Some(false);
            // f3 is turned to the lamp in one look of four only.
            seen.faces[2].facing = (index == 0).then_some(true);
            eyes.look(from + EVERY * index, Some((("b", u64::from(index)), &seen)));
        }
        let to = from + EVERY * 4;
        assert_eq!(eyes.turned_speakers(from, to), ["f1"]);
        assert_eq!(eyes.speakers(from, to), ["f1", "f2", "f3"]);
        // Turned to the lamp but not speaking is no one's line.
        let quiet = SpeakingFaces::default();
        for index in 0..3u32 {
            let mut seen = faces(&[("f1", No)]);
            seen.faces[0].facing = Some(true);
            quiet.look(from + EVERY * index, Some((("b", u64::from(index)), &seen)));
        }
        assert!(quiet.turned_speakers(from, from + EVERY * 3).is_empty());
    }

    /// A face in a look: id, its mouth, and whether it is turned to the lamp.
    type TurnedFace<'a> = (&'a str, Option<SpeakingState>, bool);

    fn turned_seen(looks: &[&[TurnedFace]]) -> (SpeakingFaces, Instant, Instant) {
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        for (index, look) in looks.iter().enumerate() {
            let mut seen = faces(&[]);
            for (id, state, turned) in look.iter() {
                let mut face = faces(&[(id, No)]).faces.remove(0);
                face.speaking = state.map(|state| Speaking {
                    state,
                    score: 0.0,
                    from: None,
                });
                face.facing = turned.then_some(true);
                seen.faces.push(face);
            }
            eyes.look(
                from + EVERY * index as u32,
                Some((("b", index as u64), &seen)),
            );
        }
        (eyes, from, from + EVERY * looks.len() as u32)
    }

    /// `LAMP_TURNED_FACE_SPEAKS`, per face (the reviewer-lead's P2 on
    /// #3305): a second face turned to the lamp whose mouth is never read
    /// keeps the sign though the first is read still; a face read still
    /// with a few unread looks among them is still not speaking.
    #[test]
    fn a_turned_face_never_read_is_not_spoken_for_by_a_still_one() {
        let not_seen = |looks: &[&[TurnedFace]]| {
            let (eyes, from, to) = turned_seen(looks);
            eyes.turned_not_seen_speaking(from, to)
        };
        let unknown: &[TurnedFace] = &[("f1", Some(No), true), ("f2", Some(Unknown), true)];
        assert!(!not_seen(&[unknown; 4]), "f2 unknown");
        let absent: &[TurnedFace] = &[("f1", Some(No), true), ("f2", None, true)];
        assert!(!not_seen(&[absent; 4]), "f2 with no speaking read");
        let still: &[TurnedFace] = &[("f1", Some(No), true)];
        let missed: &[TurnedFace] = &[("f1", Some(Unknown), true)];
        assert!(
            not_seen(&[still, missed, still, still, missed, still]),
            "f1 with a few unread looks"
        );
        assert!(!not_seen(&[missed; 4]), "f1 never read");
    }

    /// `LAMP_TURNED_SPEAKING` (lamp-4ace, 21 Oct 14:00): the faces turned to
    /// the lamp are all silent only when one read still in half the looks,
    /// none turned to it spoke, and none turned to it went unread.
    #[test]
    fn only_silent_faces_turned_to_the_lamp_are_told_apart() {
        let only_silent = |looks: &[&[TurnedFace]]| {
            let (eyes, from, to) = turned_seen(looks);
            eyes.turned_only_silent(from, to)
        };
        let at_the_desk: &[TurnedFace] = &[("f1", Some(No), true)];
        assert!(only_silent(&[at_the_desk; 4]), "someone at the desk, still");
        // Two other people speaking, turned away: still only the silent one.
        let talking: &[TurnedFace] = &[("f1", Some(No), true), ("f2", Some(Yes), false)];
        assert!(only_silent(&[talking; 4]));
        // The one turned to it speaks: someone talking to the lamp.
        let speaking: &[TurnedFace] = &[("f1", Some(No), true), ("f2", Some(Yes), true)];
        assert!(!only_silent(&[speaking; 4]));
        // A real asker: speaking in 2 looks of 5, still between words in 3.
        let between: &[TurnedFace] = &[("f1", Some(No), true)];
        let word: &[TurnedFace] = &[("f1", Some(Yes), true)];
        assert!(!only_silent(&[between, word, between, word, between]));
        let alone: &[TurnedFace] = &[("f1", Some(Yes), true)];
        assert!(!only_silent(&[alone; 4]));
        // A mouth not read says nothing, in any look.
        let unknown: &[TurnedFace] = &[("f1", Some(Unknown), true)];
        assert!(!only_silent(&[unknown; 4]));
        let not_judged: &[TurnedFace] = &[("f1", None, true)];
        assert!(!only_silent(&[not_judged; 4]));
        assert!(!only_silent(&[
            at_the_desk,
            at_the_desk,
            at_the_desk,
            &[("f1", Some(No), true), ("f3", Some(Unknown), true)],
        ]));
        // Nobody turned to the lamp, or too few looks: nothing to tell.
        let turned_away: &[TurnedFace] = &[("f1", Some(No), false)];
        assert!(!only_silent(&[turned_away; 4]));
        assert!(!only_silent(&[at_the_desk; 2]));
    }

    /// A face speaking in at least half of the line's looks spoke through
    /// it; one under half did not; two faces doing so are several.
    #[test]
    fn the_face_that_spoke_through_the_line_is_the_one_seen_speaking() {
        let (eyes, from, to) = seen(&[
            &[("f1", Yes), ("f2", No)],
            &[("f1", Yes), ("f2", Yes)],
            &[("f1", No), ("f2", No)],
            &[("f1", Yes), ("f2", Unknown)],
        ]);
        assert_eq!(eyes.speakers(from, to), ["f1"]);
        let (eyes, from, to) = seen(&[
            &[("f1", Yes), ("f2", Yes)],
            &[("f1", Yes), ("f2", Yes)],
            &[("f1", No), ("f2", No)],
        ]);
        assert_eq!(eyes.speakers(from, to), ["f1", "f2"]);
        let (eyes, from, to) = seen(&[&[("f1", No)], &[("f1", Unknown)], &[]]);
        assert!(eyes.speakers(from, to).is_empty());
        // Two looks are too few to say.
        let (eyes, from, to) = seen(&[&[("f1", Yes)], &[("f1", Yes)]]);
        assert!(eyes.speakers(from, to).is_empty());
    }

    /// Only the line's own looks count, the eyes saying nothing counts as
    /// nobody speaking, and a look older than [`KEPT`] is gone.
    #[test]
    fn only_the_lines_own_looks_count_and_old_ones_go() {
        let eyes = SpeakingFaces::default();
        let start = Instant::now();
        let looked = |index: u32| start + EYES_EVERY * index;
        for index in 0..4u32 {
            eyes.look(
                looked(index),
                Some((("b", index.into()), &faces(&[("f1", Yes)]))),
            );
        }
        for index in 10..14u32 {
            eyes.look(looked(index), None);
        }
        assert!(eyes.speakers(looked(10), looked(14)).is_empty());
        assert_eq!(eyes.speakers(start, looked(4)), ["f1"]);
        eyes.look(start + KEPT + EYES_EVERY * 5, None);
        assert!(eyes.speakers(start, looked(4)).is_empty());
    }

    /// #2305's review: an id is kept with the eyes' boot, so the same
    /// passing id after the eyes start again is another; and a privacy
    /// change forgets every look.
    #[test]
    fn ids_keep_their_boot_and_a_privacy_change_forgets_them() {
        let eyes = SpeakingFaces::default();
        let start = Instant::now();
        for index in 0..3u32 {
            eyes.look(
                start + EVERY * index,
                Some((("a", index.into()), &faces(&[("f1", Yes)]))),
            );
        }
        for index in 3..6u32 {
            eyes.look(
                start + EVERY * index,
                Some((("b", index.into()), &faces(&[("f1", Yes)]))),
            );
        }
        let (first, second) = (start + EVERY * 2, start + EVERY * 3);
        assert_eq!(eyes.speakers_with_boot(start, first), ["a/f1"]);
        assert_eq!(eyes.speakers_with_boot(second, start + EVERY * 5), ["b/f1"]);
        assert_eq!(eyes.speakers(start, first), ["f1"]);
        eyes.forget();
        assert!(eyes.speakers(start, start + EVERY * 5).is_empty());
    }

    /// The reviewer on #2008: the eyes' one frame, read six times, is one
    /// look, too few to say who spoke; their silence read every 100 ms is
    /// one look each 500 ms. Three frames of the same face are enough.
    #[test]
    fn each_of_the_eyes_frames_counts_once() {
        let eyes = SpeakingFaces::default();
        let start = Instant::now();
        let speaking = faces(&[("f2", Yes)]);
        for read in 0..6u32 {
            eyes.look(start + EVERY * read, Some((("b", 7), &speaking)));
        }
        assert!(eyes.speakers(start, start + EVERY * 6).is_empty());
        let quiet = start + EVERY * 10;
        for read in 0..10u32 {
            eyes.look(quiet + EVERY * read, None);
        }
        assert!(eyes.speakers(quiet, quiet + EVERY * 4).is_empty());
        assert!(eyes.speakers(quiet, quiet + EVERY * 9).is_empty());
        let later = start + EVERY * 30;
        for frame in 8..11u64 {
            for read in 0..3u32 {
                let at = later + EYES_EVERY * (frame - 8) as u32 + EVERY * read;
                eyes.look(at, Some((("b", frame), &speaking)));
            }
        }
        assert_eq!(eyes.speakers(later, later + EYES_EVERY * 3), ["f2"]);
    }

    /// The rows of the rule: the known face alone speaking keeps the line
    /// theirs; another face, theirs and another together, an unknown known
    /// face, or the eyes now knowing someone else narrows it; nobody leaves
    /// it.
    #[test]
    fn a_line_is_someone_elses_when_any_other_face_spoke_it() {
        let lan = crate::remember::person_id("Lan");
        let known = || Some(("Lan".to_owned(), "f1".to_owned()));
        let ids = |ids: &[&str]| -> Vec<String> { ids.iter().map(|id| (*id).to_owned()).collect() };
        assert!(!someone_elses(&ids(&["f1"]), &lan, known()));
        assert!(someone_elses(&ids(&["f2"]), &lan, known()));
        assert!(someone_elses(&ids(&["f1", "f2"]), &lan, known()));
        assert!(someone_elses(&ids(&["f1"]), &lan, None));
        assert!(someone_elses(
            &ids(&["f1"]),
            &lan,
            Some(("Minh".to_owned(), "f1".to_owned()))
        ));
        assert!(!someone_elses(&ids(&[]), &lan, None));
        assert!(!someone_elses(&ids(&[]), &lan, known()));
    }

    /// The face's file is read with the name, only while fresh and only as
    /// a passing id; a missing face file leaves no pair.
    #[test]
    fn the_known_face_is_read_with_the_name_and_only_as_an_id() {
        let dir = std::env::temp_dir().join(format!("speaking-faces-known-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        std::fs::write(&known, "Lan\n").unwrap();
        assert_eq!(known_with_face(&known), None);
        std::fs::write(known_face_of(&known), "f12\n").unwrap();
        assert_eq!(
            known_with_face(&known),
            Some(("Lan".to_owned(), "f12".to_owned()))
        );
        for wrong in ["12", "f", "fx1", "f12345678901", "Lan"] {
            std::fs::write(known_face_of(&known), wrong).unwrap();
            assert_eq!(known_with_face(&known), None, "{wrong}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #4885, the Reviewer-lead's P1 on #4917: A is known on `f1`; for one
    /// look A goes undetected and a stranger shows beside them as `f2`,
    /// which the eyes say follows `f1`; A comes back on `f1`, which ends the
    /// link. The known file's `f1` is A's, never mapped to B's `f2`, and a
    /// line over those looks is not one face. Only the eyes' own file moves
    /// the known face to a new id, once its print passes the hold's bar.
    #[test]
    fn a_link_the_eyes_ended_never_maps_the_known_face_to_the_follower() {
        let dir =
            std::env::temp_dir().join(format!("speaking-faces-follow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let now = Instant::now();
        let eyes = SpeakingFaces::default();
        let from = now - Duration::from_secs(1);
        eyes.look(from, Some((("b", 0), &faces(&[("f1", Yes)]))));
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        let read = || eyes.known_with_boot(&known).map(|(_, face)| face);
        assert_eq!(read().as_deref(), Some("b/f1"));
        let mut stranger = faces(&[("f2", Yes)]);
        stranger.faces[0].follows = Some("f1".to_owned());
        eyes.look(from + EVERY, Some((("b", 1), &stranger)));
        // The link is said, but the eyes' file still names f1: so is A.
        assert_eq!(read().as_deref(), Some("b/f1"));
        // A back on f1 beside B: the eyes end the link.
        eyes.look(
            from + EVERY * 2,
            Some((("b", 2), &faces(&[("f1", Yes), ("f2", No)]))),
        );
        assert_eq!(read().as_deref(), Some("b/f1"));
        let to = from + EVERY * 3;
        assert!(!eyes.no_other_face_in_view(Some("b/f2"), from, to));
        assert!(!eyes.no_other_face_in_view(Some("b/f1"), from, to));
        assert_eq!(eyes.speakers_with_boot(from, to), ["b/f1"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// lamp-06c1, 5 Nov, S4: the known file goes stale once the face has
    /// left; the pair last read still names it for a line within the
    /// window after it left. Not once the face is out of the window, not
    /// for a face still in view the eyes stopped naming, not past
    /// [`NAMED_FOR`], and not after a privacy change ([`SpeakingFaces::forget`]).
    #[test]
    fn the_face_last_named_names_it_only_just_after_it_left() {
        let dir =
            std::env::temp_dir().join(format!("speaking-faces-lately-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let eyes = SpeakingFaces::default();
        let now = Instant::now();
        let at = |seconds: f32| now - Duration::from_secs_f32(seconds);
        eyes.look(at(6.0), Some((("b", 0), &faces(&[("f1", Yes)]))));
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        let lan = Some(("Lan".to_owned(), "b/f1".to_owned()));
        assert_eq!(eyes.known_with_boot(&known), lan);
        // In view until 3 s ago, then gone; the eyes stop naming it.
        eyes.look(at(3.0), Some((("b", 1), &faces(&[("f1", No)]))));
        eyes.look(at(2.5), Some((("b", 2), &faces(&[]))));
        std::fs::remove_file(&known).unwrap();
        let to = Instant::now();
        assert_eq!(eyes.known_with_boot(&known), None);
        assert_eq!(eyes.known_lately(&known, at(5.0), to), lan, "left 3 s ago");
        assert_eq!(
            eyes.known_lately(&known, at(2.0), to),
            None,
            "out of the window"
        );
        // Still in view, unnamed: the eyes' call.
        eyes.look(at(0.1), Some((("b", 3), &faces(&[("f1", No)]))));
        assert_eq!(eyes.known_lately(&known, at(5.0), to), None, "in view");
        eyes.look(Instant::now(), Some((("b", 4), &faces(&[]))));
        let later = Instant::now();
        assert_eq!(eyes.known_lately(&known, at(5.0), later), lan, "gone again");
        assert_eq!(
            eyes.known_lately(&known, at(5.0), later + NAMED_FOR),
            None,
            "read too long ago"
        );
        eyes.forget();
        assert_eq!(eyes.known_lately(&known, at(5.0), later), None, "forgotten");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Thirteen's P3 on #5448: the pair [`SpeakingFaces::known_lately`]
    /// keeps once the known file is stale also feeds [`not_theirs`] and
    /// [`named_alone`]. Her face gone, nobody seen speaking: the line is not
    /// hers and not named (not alone in view). Her own passing id seen
    /// speaking it before she left: hers, one tracked id being one face.
    /// Another face seen speaking it: not hers.
    #[test]
    fn the_face_last_named_feeds_not_theirs_and_named_alone_as_a_fresh_pair() {
        let dir =
            std::env::temp_dir().join(format!("speaking-faces-stale-pair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let eyes = SpeakingFaces::default();
        let now = Instant::now();
        let at = |seconds: f32| now - Duration::from_secs_f32(seconds);
        eyes.look(at(6.0), Some((("b", 0), &faces(&[("f1", No)]))));
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        assert!(eyes.known_with_boot(&known).is_some());
        let lan = crate::known_faces::memory_person("Lan");
        // Seen speaking, then gone; the eyes stop naming her.
        for (seq, ago) in [(1, 3.0), (2, 2.9), (3, 2.8)] {
            eyes.look(at(ago), Some((("b", seq), &faces(&[("f1", Yes)]))));
        }
        for (seq, ago) in [(4, 2.0), (5, 1.0), (6, 0.5), (7, 0.1)] {
            eyes.look(at(ago), Some((("b", seq), &faces(&[]))));
        }
        std::fs::remove_file(&known).unwrap();
        let to = Instant::now();
        // A line begun after she left: nobody seen speaking it.
        let (from, since) = (at(1.5), at(6.5));
        let pair = eyes.known_lately(&known, since, to);
        assert_eq!(pair, Some(("Lan".to_owned(), "b/f1".to_owned())));
        let speakers = eyes.speakers_with_boot(from, to);
        assert!(speakers.is_empty());
        assert!(not_theirs(&speakers, &lan, pair.clone(), true), "not hers");
        let alone = eyes.alone_in_view("b/f1", from, to);
        assert!(!alone, "not in view");
        assert!(!named_alone(&speakers, &lan, pair.as_ref(), alone));
        // A line she began before she left, her id seen speaking it: hers.
        let speakers = eyes.speakers_with_boot(at(3.05), at(2.75));
        assert_eq!(speakers, vec!["b/f1".to_owned()]);
        assert!(!not_theirs(&speakers, &lan, pair.clone(), true), "hers");
        // Another face seen speaking it: not hers.
        assert!(not_theirs(&["b/f2".to_owned()], &lan, pair, true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #3037: the known face's id is compared with its boot. The eyes'
    /// `-face` file holds the bare id, so it takes the boot whose first frame
    /// was read before it was written: a file still fresh from before the
    /// eyes started again names none of the new boot's faces, though their
    /// ids are the same, until the new eyes write it again.
    #[test]
    fn the_known_face_takes_the_boot_it_was_written_under() {
        let dir = std::env::temp_dir().join(format!("speaking-faces-boot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        std::fs::write(&known, "Lan\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        let eyes = SpeakingFaces::default();
        // No boot read before the file was written: no pair.
        assert_eq!(eyes.known_with_boot(&known), None);
        let now = Instant::now();
        let ago = |seconds: f32| now - Duration::from_secs_f32(seconds);
        for index in 0..3u32 {
            eyes.look(
                ago(2.8) + EVERY * index,
                Some((("a", index.into()), &faces(&[("f1", Yes)]))),
            );
        }
        for index in 0..3u32 {
            eyes.look(
                ago(1.0) + EVERY * index,
                Some((("b", index.into()), &faces(&[("f1", Yes)]))),
            );
        }
        let written = |seconds: f32| {
            std::fs::File::options()
                .write(true)
                .open(known_face_of(&known))
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs_f32(seconds))
                .unwrap();
        };
        let lan = crate::known_faces::memory_person("Lan");
        let said = eyes.speakers_with_boot(ago(1.0), now);
        assert_eq!(said, ["b/f1"]);
        // Written under the eyes before they started again: Lan's `f1`
        // was another face, so the new `f1` saying it is not hers.
        written(1.6);
        let before = eyes.known_with_boot(&known);
        assert_eq!(before, Some(("Lan".to_owned(), "a/f1".to_owned())));
        assert!(!seen_saying(&said, &lan, before));
        // Written again by the new eyes: hers.
        written(0.4);
        let again = eyes.known_with_boot(&known);
        assert_eq!(again, Some(("Lan".to_owned(), "b/f1".to_owned())));
        assert!(seen_saying(&said, &lan, again));
        // A privacy change forgets the looks, not which boot came when.
        eyes.forget();
        assert_eq!(
            eyes.known_with_boot(&known),
            Some(("Lan".to_owned(), "b/f1".to_owned()))
        );
        // Fourth's P2 on #3038: no tolerance. A file written 250, 100 or
        // 1 ms before the new boot's first frame, or at it, is the old
        // eyes' and never the new face's; 1 ms after, it is the new boot's.
        let first = eyes.first_frame_of("b").unwrap();
        let ms = Duration::from_millis;
        let at = |time: SystemTime| {
            std::fs::File::options()
                .write(true)
                .open(known_face_of(&known))
                .unwrap()
                .set_modified(time)
                .unwrap();
            eyes.known_with_boot(&known)
        };
        for before in [ms(250), ms(100), ms(1), Duration::ZERO] {
            let bound = at(first - before);
            assert_eq!(
                bound,
                Some(("Lan".to_owned(), "a/f1".to_owned())),
                "{before:?}"
            );
            assert!(!seen_saying(&said, &lan, bound), "{before:?}");
        }
        let bound = at(first + ms(1));
        assert_eq!(bound, Some(("Lan".to_owned(), "b/f1".to_owned())));
        assert!(seen_saying(&said, &lan, bound));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #2807 on lamp-c0a4: a line armed only by the eyes' known file
    /// changing mid-line keeps for the known face only if that face's
    /// passing id was in view as the line began. Lan's `f1` tracked (not
    /// speaking, unknown yet) from before the line: in view. El's walk-in on
    /// #3108: `f2` first tracked mid-line is not; nor is a face of another
    /// boot, another name in the known file, or a moment with no look within
    /// [`IN_VIEW_BEFORE`] before it. An empty look (nothing fresh) shows
    /// nobody.
    #[test]
    fn a_known_face_was_in_view_at_a_lines_start_only_if_its_id_was_tracked_then() {
        let dir = std::env::temp_dir().join(format!("speaking-faces-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let known = dir.join("known");
        let now = Instant::now();
        let ago = |seconds: f32| now - Duration::from_secs_f32(seconds);
        let eyes = SpeakingFaces::default();
        let mut frame = 0u64;
        // `f1` from 3 s ago; `f2` joins 0.8 s ago.
        for step in 0..15u32 {
            let at = ago(3.0) + Duration::from_millis(200) * step;
            let mut seen = vec![("f1", No)];
            if at >= ago(0.8) {
                seen.push(("f2", No));
            }
            frame += 1;
            eyes.look(at, Some((("b", frame), &faces(&seen))));
        }
        let begun = ago(1.5);
        let lan = crate::known_faces::memory_person("Lan");
        let named = |name: &str, face: &str| {
            std::fs::write(&known, format!("{name}\n")).unwrap();
            std::fs::write(known_face_of(&known), format!("{face}\n")).unwrap();
        };
        named("Lan", "f1");
        assert!(eyes.in_view_at("b/f1", begun));
        assert!(
            eyes.known_in_view_since(&known, &lan, begun),
            "the lamp's case"
        );
        named("Lan", "f2");
        assert!(!eyes.in_view_at("b/f2", begun));
        assert!(!eyes.known_in_view_since(&known, &lan, begun), "walked in");
        assert!(eyes.known_in_view_since(&known, &lan, ago(0.1)));
        named("Ann", "f1");
        assert!(
            !eyes.known_in_view_since(&known, &lan, begun),
            "another name"
        );
        assert!(!eyes.in_view_at("a/f1", begun), "another boot");
        assert!(!eyes.in_view_at("b/f1", ago(4.5)), "no look then");
        let quiet = now + IN_VIEW_BEFORE + EYES_EVERY;
        eyes.look(quiet, None);
        assert!(!eyes.in_view_at("b/f1", quiet), "an empty look");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #5522: a 0.4 s "yes" spans two of the eyes' looks, under
    /// [`FEWEST_LOOKS`], so it is nobody's. With the short-line switch a face
    /// whose `yes` began in the line is its speaker. A face read `no` in
    /// the line, or any face reading `yes` before it (carried over from
    /// its own earlier talk by the eyes' 2 s window: the Reviewer-lead's P2
    /// on #5660), leaves it the room's; off is as before.
    #[test]
    fn a_short_line_is_judged_over_the_eyes_window_with_the_switch() {
        use SpeakingState::{No, Unknown, Yes};
        let face = |id: &str, state: SpeakingState| Face {
            id: id.to_owned(),
            xmin: 0.1,
            ymin: 0.1,
            xmax: 0.3,
            ymax: 0.4,
            confidence: 1.0,
            yaw: None,
            facing: Some(true),
            speaking: Some(Speaking {
                state,
                score: 0.03,
                from: None,
            }),
            follows: None,
        };
        let start = Instant::now();
        // Looks every 250 ms from 0 to 2 s; the line is its last 0.4 s.
        let seen = |states_at: &dyn Fn(u64) -> Vec<(&'static str, SpeakingState)>| {
            let eyes = SpeakingFaces::default();
            for (seq, ms) in (0..=2000u64).step_by(250).enumerate() {
                let faces = Faces {
                    faces: states_at(ms)
                        .into_iter()
                        .map(|(id, state)| face(id, state))
                        .collect(),
                };
                eyes.look(
                    start + Duration::from_millis(ms),
                    Some((("b", seq as u64 + 1), &faces)),
                );
            }
            eyes
        };
        let (from, to) = (
            start + Duration::from_millis(1600),
            start + Duration::from_millis(2000),
        );
        let ids = |_: &Look, ids: &[String]| ids.to_vec();
        let on = |eyes: &SpeakingFaces| eyes.line_counted(from, to, true, ids);
        let in_line = |ms: u64| ms >= 1600;

        // A listener still before the line says it: its `yes` begins in it.
        let onset = seen(&|ms| vec![("f1", if in_line(ms) { Yes } else { No })]);
        assert_eq!(
            onset.speakers(from, to),
            Speakers::new(),
            "off: two looks, the room's"
        );
        assert_eq!(
            onset.line_counted(from, to, false, ids),
            Speakers::new(),
            "off: as before"
        );
        assert_eq!(on(&onset), vec!["f1".to_owned()], "on: theirs");

        let still = seen(&|_| vec![("f1", No)]);
        assert_eq!(on(&still), Speakers::new(), "a still face: the room's");

        let still_in_line = seen(&|ms| {
            vec![(
                "f1",
                if ms == 1750 {
                    No
                } else if in_line(ms) {
                    Yes
                } else {
                    No
                },
            )]
        });
        assert_eq!(on(&still_in_line), Speakers::new(), "read `no` in the line");

        let before_only = seen(&|ms| vec![("f1", if in_line(ms) { Unknown } else { Yes })]);
        assert_eq!(
            on(&before_only),
            Speakers::new(),
            "never `yes` in the line itself"
        );

        // The Reviewer-lead's P2 on #5660: A talks until 1.5 s and the eyes'
        // window carries its `yes` through the line; listener B, read still,
        // is too new in its own window to read anything but `unknown`.
        let carried = seen(&|ms| vec![("f1", Yes), ("f2", if in_line(ms) { Unknown } else { No })]);
        assert_eq!(
            on(&carried),
            Speakers::new(),
            "A's carried `yes`: the room's"
        );
        // Nobody else in view: an off-camera voice after A's talk is not A's.
        let alone = seen(&|_| vec![("f1", Yes)]);
        assert_eq!(
            on(&alone),
            Speakers::new(),
            "A alone, still `yes`: the room's"
        );
        // The cost: B's own onset with A's `yes` before it is the room's too.
        let both = seen(&|ms| vec![("f1", Yes), ("f2", if in_line(ms) { Yes } else { No })]);
        assert_eq!(on(&both), Speakers::new(), "another face's `yes` before");

        // Exactly the window long: judged on its own looks, as before.
        let edge = start;
        assert_eq!(to.duration_since(edge), SHORT_LINE_WINDOW);
        assert_eq!(
            alone.line_counted(edge, to, true, ids),
            vec!["f1".to_owned()],
            "a 2 s line is not padded"
        );
        assert_eq!(
            alone.line_counted(edge, to, true, ids),
            alone.line_counted(edge, to, false, ids)
        );
    }
}
