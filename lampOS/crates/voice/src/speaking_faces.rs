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
    /// The original frame contains exactly one face, positively speaking.
    /// Used only by the strict two-look exception, before filtering others.
    single_positive: bool,
    /// Every passing id in this look, speaking or not
    /// ([`SpeakingFaces::in_view_at`]).
    tracked: Vec<String>,
    /// Of those, the ones turned to the lamp (`facing`), for a line the lamp
    /// began itself (#2287's follow-up, [`SpeakingFaces::turned_speakers`]).
    turned: Vec<String>,
    /// The faces turned to the lamp whose mouth read still (`no`), for
    /// [`SpeakingFaces::turned_only_silent`].
    turned_silent: Vec<String>,
    /// The faces turned to the lamp whose mouth was not read (`unknown`, or
    /// not judged at all) in this look.
    turned_unread: Vec<String>,
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

/// Boots of the eyes remembered with their first frame's time.
const MOST_BOOTS: usize = 8;
/// The last [`KEPT`] of looks, fed by one reader and judged at each line's
/// end. Each of the eyes' frames counts once, however often it is read (the
/// reviewer on #2008), so a short line can't pass the bar on one frame.
#[derive(Default)]
pub struct SpeakingFaces {
    looks: Mutex<Looks>,
}

impl SpeakingFaces {
    /// One read of the eyes' `humans/faces/tracked`: their frame, by its
    /// publisher's boot and sequence, counted only if it is new; `None` when
    /// they said nothing fresh, counted as no face speaking at most once each
    /// [`EYES_EVERY`].
    pub fn look(&self, at: Instant, frame: Option<((&str, u64), &Faces)>) {
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
                // A missed read is not a new publisher frame. Keep its
                // deduplication key so recovery cannot count it twice.
                if looks.kept.back().is_some_and(|last| {
                    last.boot.is_empty() && at.saturating_duration_since(last.at) < EYES_EVERY
                }) {
                    return;
                }
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
        let single_positive = frame.is_some_and(|((boot, _), faces)| {
            faces.faces.len() == 1
                && speaking_faces.len() == 1
                && crate::memory_scope::qualified_speaker(&format!("{boot}/{}", faces.faces[0].id))
        });
        let speaking = speaking_faces.iter().map(|face| face.id.clone()).collect();
        let tracked = frame
            .map(|(_, faces)| faces.faces.iter().map(|face| face.id.clone()).collect())
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
            single_positive,
            tracked,
            turned,
            turned_silent,
            turned_unread,
        });
    }

    /// The faces that read speaking in at least half of the looks from
    /// `from` to `to`.
    pub fn speakers(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| look.speaking.clone())
    }

    /// As [`Self::speakers`], each id kept with the eyes' boot it was seen
    /// under (`boot/f3`): after the eyes start again a passing id names
    /// someone else, so an id from an older boot matches none of the new.
    pub fn speakers_with_boot(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| {
            look.speaking
                .iter()
                .map(|id| format!("{}/{id}", look.boot))
                .collect()
        })
    }

    /// Positive evidence for sensitive per-request disclosure or consent:
    /// exactly one boot-qualified face spoke in the interval, in at least
    /// half of at least three distinct looks, or in both of exactly two
    /// looks whose original frames each contain that face alone, speaking.
    /// Another speaker in even one look refuses the proof; a majority must
    /// not hide a second person's short contribution. This does not establish
    /// liveness or identity.
    pub(crate) fn positive_speaker(&self, from: Instant, to: Instant) -> Speakers {
        let Ok(looks) = self.looks.lock() else {
            return Vec::new();
        };
        let mut speaker: Option<String> = None;
        let mut frames = 0;
        let mut spoken = 0;
        let mut single_positive = 0;
        for look in looks
            .kept
            .iter()
            .filter(|look| look.at >= from && look.at <= to)
        {
            frames += 1;
            single_positive += usize::from(look.single_positive);
            if !look.speaking.is_empty() {
                spoken += 1;
            }
            for id in &look.speaking {
                let qualified = format!("{}/{id}", look.boot);
                match &speaker {
                    Some(seen) if *seen != qualified => return Vec::new(),
                    None => speaker = Some(qualified),
                    _ => {}
                }
            }
        }
        if (frames >= FEWEST_LOOKS && spoken * 2 >= frames) || (frames == 2 && single_positive == 2)
        {
            speaker.into_iter().collect()
        } else {
            Vec::new()
        }
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
    /// `known-speaker` file (Fourth's) is to replace this.
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
        let looks = self.looks.lock().ok()?;
        let (boot, _) = looks
            .boots
            .iter()
            .filter(|(_, first)| *first < written)
            .max_by_key(|(_, first)| *first)?;
        Some((name, format!("{boot}/{face}")))
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

/// An explicit, indivisible name/identity/face observation written by Eyes.
/// Legacy `known-face` has no boot and is never upgraded by reader timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnownSpeaker {
    pub(crate) name: String,
    pub(crate) person: String,
    /// Keep the source identity even when memory is keyed by the name.
    pub(crate) id: Option<String>,
    pub(crate) face: String,
    pub(crate) epoch: Option<KnownSpeakerEpoch>,
}

/// Producer lifecycle and revocation epoch, never inferred from file timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnownSpeakerEpoch {
    pub(crate) boot: String,
    pub(crate) publisher: String,
    pub(crate) generation: u64,
}

pub(crate) enum KnownSpeakerSource {
    Seen(KnownSpeaker),
    Absent(KnownSpeakerEpoch),
    /// A returning/new track is awaiting actual recognition: deny immediately.
    Checking(KnownSpeakerEpoch),
    Revoked(KnownSpeakerEpoch),
}

pub(crate) fn known_speaker_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-speaker");
    PathBuf::from(beside)
}

/// Read one bounded source file, with a conservative monotonic deadline
/// anchored before I/O. Sample wall time after the opened file's metadata:
/// an atomic publication during open is not a future-dated observation.
/// An old Eyes boot stays old even when a new publisher reuses its passing fN.
#[cfg(test)]
pub(crate) fn known_speaker(known: &Path) -> Option<KnownSpeaker> {
    match read_known_speaker_source(
        || std::fs::File::open(known_speaker_of(known)),
        SystemTime::now,
    )? {
        KnownSpeakerSource::Seen(known) => Some(known),
        KnownSpeakerSource::Absent(_)
        | KnownSpeakerSource::Checking(_)
        | KnownSpeakerSource::Revoked(_) => None,
    }
}

/// Compatibility seam for existing positive publication/clock controls.
#[cfg(test)]
fn read_known_speaker(
    open: impl FnOnce() -> std::io::Result<std::fs::File>,
    wall: impl FnOnce() -> SystemTime,
) -> Option<KnownSpeaker> {
    match read_known_speaker_source(open, wall)? {
        KnownSpeakerSource::Seen(known) => Some(known),
        KnownSpeakerSource::Absent(_)
        | KnownSpeakerSource::Checking(_)
        | KnownSpeakerSource::Revoked(_) => None,
    }
}

/// Metadata and contents belong to one bounded FD. Only explicit versioned
/// absence can continue a request; missing, old and malformed sources cannot.
#[cfg(test)]
fn read_known_speaker_source(
    open: impl FnOnce() -> std::io::Result<std::fs::File>,
    wall: impl FnOnce() -> SystemTime,
) -> Option<KnownSpeakerSource> {
    read_known_speaker_source_at(open, wall, lamp_ipc::monotonic_us, false)
}

#[cfg(test)]
enum SourceAuthority {
    Required,
    #[cfg(test)]
    LegacyCompatibility,
}

#[cfg(test)]
pub(crate) fn read_known_speaker_source_at(
    open: impl FnOnce() -> std::io::Result<std::fs::File>,
    wall: impl FnOnce() -> SystemTime,
    now_us: impl FnMut() -> u64,
    require_authority: bool,
) -> Option<KnownSpeakerSource> {
    parse_known_speaker_source(
        open,
        wall,
        now_us,
        if require_authority {
            SourceAuthority::Required
        } else {
            SourceAuthority::LegacyCompatibility
        },
    )
}

#[cfg(test)]
fn parse_known_speaker_source(
    open: impl FnOnce() -> std::io::Result<std::fs::File>,
    wall: impl FnOnce() -> SystemTime,
    mut now_us: impl FnMut() -> u64,
    authority_policy: SourceAuthority,
) -> Option<KnownSpeakerSource> {
    let require_authority = matches!(authority_policy, SourceAuthority::Required);
    use std::io::Read;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Legacy {
        name: String,
        id: Option<String>,
        boot: String,
        face: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "state", rename_all = "lowercase", deny_unknown_fields)]
    enum Published {
        Seen {
            name: String,
            id: Option<String>,
            boot: String,
            face: String,
            publisher: String,
            generation: u64,
            #[serde(default)]
            permission_until_us: Option<u64>,
            #[serde(default)]
            observed_us: Option<u64>,
        },
        Absent {
            boot: String,
            publisher: String,
            generation: u64,
            #[serde(default)]
            permission_until_us: Option<u64>,
            #[serde(default)]
            observed_us: Option<u64>,
        },
        Checking {
            boot: String,
            publisher: String,
            generation: u64,
            #[serde(default)]
            permission_until_us: Option<u64>,
            #[serde(default)]
            observed_us: Option<u64>,
        },
        Revoked {
            boot: String,
            publisher: String,
            generation: u64,
            #[serde(default)]
            permission_until_us: Option<u64>,
            #[serde(default)]
            observed_us: Option<u64>,
        },
    }
    impl Published {
        fn authority(&self) -> Option<(u64, u64)> {
            let (until, observed) = match self {
                Self::Seen {
                    permission_until_us,
                    observed_us,
                    ..
                }
                | Self::Absent {
                    permission_until_us,
                    observed_us,
                    ..
                }
                | Self::Checking {
                    permission_until_us,
                    observed_us,
                    ..
                }
                | Self::Revoked {
                    permission_until_us,
                    observed_us,
                    ..
                } => (*permission_until_us, *observed_us),
            };
            Some((until?, observed?))
        }
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Source {
        Legacy(Legacy),
        Published(Published),
    }
    let began = Instant::now();
    let file = open().ok()?;
    let modified = file.metadata().ok()?.modified().ok()?;
    let age = wall().duration_since(modified).ok()?;
    let remaining = crate::known_faces::FRESH.checked_sub(age)?;
    let expires = began.checked_add(remaining)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    let source: Source = serde_json::from_slice(&bytes).ok()?;
    let authority = match &source {
        Source::Published(published) => published.authority(),
        Source::Legacy(_) => None,
    };
    let revoked = matches!(&source, Source::Published(Published::Revoked { .. }));
    if require_authority && !revoked && !source_authority_current(authority, now_us()) {
        return None;
    }
    let epoch = |boot: String, publisher: String, generation: u64| {
        (crate::memory_scope::qualified_speaker(&format!("{boot}/f1"))
            && publisher.len() == 32
            && publisher
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && generation != 0)
            .then_some(KnownSpeakerEpoch {
                boot,
                publisher,
                generation,
            })
    };
    let seen = |name: String,
                id: Option<String>,
                boot: String,
                face: String,
                epoch: Option<KnownSpeakerEpoch>| {
        let face = format!("{boot}/{face}");
        (crate::memory_scope::qualified_speaker(&face)
            && !name.is_empty()
            && name.trim() == name
            && name.chars().count() <= 40
            && !name.chars().any(char::is_control)
            && id.as_deref().is_none_or(lamp_msgs::known::is_person_id))
        .then(|| {
            KnownSpeakerSource::Seen(KnownSpeaker {
                person: crate::known_faces::person_of(
                    &name,
                    id.as_deref(),
                    lamp_msgs::known::stable_ids(),
                ),
                name,
                id,
                face,
                epoch,
            })
        })
    };
    let source = match source {
        Source::Legacy(Legacy {
            name,
            id,
            boot,
            face,
        }) => seen(name, id, boot, face, None)?,
        Source::Published(Published::Seen {
            name,
            id,
            boot,
            face,
            publisher,
            generation,
            ..
        }) => {
            let epoch = epoch(boot.clone(), publisher, generation)?;
            seen(name, id, boot, face, Some(epoch))?
        }
        Source::Published(Published::Absent {
            boot,
            publisher,
            generation,
            ..
        }) => KnownSpeakerSource::Absent(epoch(boot, publisher, generation)?),
        Source::Published(Published::Checking {
            boot,
            publisher,
            generation,
            ..
        }) => KnownSpeakerSource::Checking(epoch(boot, publisher, generation)?),
        Source::Published(Published::Revoked {
            boot,
            publisher,
            generation,
            ..
        }) => KnownSpeakerSource::Revoked(epoch(boot, publisher, generation)?),
    };
    (Instant::now() < expires
        && (!require_authority || revoked || source_authority_current(authority, now_us())))
    .then_some(source)
}

/// Both deadlines are checked at the final reader boundary. Publication mtime
/// may advance for camera renewals; original identity age must never advance.
#[cfg(test)]
fn source_authority_current(authority: Option<(u64, u64)>, now_us: u64) -> bool {
    authority.is_some_and(|(until, observed)| {
        observed > 0
            && observed <= now_us
            && Duration::from_micros(now_us - observed) < crate::known_faces::FRESH
            && now_us < until
            && until - now_us <= 1_500_000
    })
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
/// face was ([`someone_elses`]), as before.
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

/// The journal's words for who was seen speaking a line admitted as a known
/// person's, given whether it stayed `theirs` ([`not_theirs`]): the passing
/// face ids only, never a name or the line's words. `None` when no face was
/// seen speaking it and it stayed theirs (the switch off): nothing to tell.
pub fn seen_speaking_note(speakers: &[String], theirs: bool) -> Option<String> {
    match (speakers, theirs) {
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
            seen_speaking_note(&speakers, !not_theirs(&speakers, &lan, known(), true))
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
        assert_eq!(seen_speaking_note(&[], true), None);
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

    #[test]
    fn explicit_known_speaker_is_atomic_bounded_fresh_and_never_guesses_a_boot() {
        let dir = std::env::temp_dir().join(format!("known-speaker-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let known = dir.join("known");
        let source = known_speaker_of(&known);
        assert_eq!(source.file_name().unwrap(), "known-speaker");
        std::fs::write(&known, "Example").unwrap();
        std::fs::write(known_face_of(&known), "f1").unwrap();
        assert!(known_speaker(&known).is_none());
        let good = serde_json::json!({"name":"Example", "id":null,
            "boot":"0123456789abcdef", "face":"f1"});
        std::fs::write(&source, good.to_string()).unwrap();
        let read = known_speaker(&known).unwrap();
        assert_eq!(read.name, "Example");
        assert_eq!(read.person, lamp_msgs::known::person_id("Example"));
        assert_eq!(read.face, "0123456789abcdef/f1");
        // The atomic source owns its name; separate old legacy files cannot
        // mix that new face with another name or freshly guess its boot.
        std::fs::write(&known, "Another").unwrap();
        assert_eq!(known_speaker(&known).unwrap().name, "Example");
        for (field, bad) in [
            ("name", serde_json::json!("")),
            ("name", serde_json::json!(" Example")),
            ("name", serde_json::json!("Example\nInjected")),
            ("name", serde_json::json!("x".repeat(41))),
            ("id", serde_json::json!("not-a-person-id")),
            ("boot", serde_json::json!("")),
            ("boot", serde_json::json!("0123456789abcdeF")),
            ("face", serde_json::json!("f")),
            ("face", serde_json::json!("f1/another")),
            ("face", serde_json::json!("f12345678901")),
        ] {
            let mut bad_source = good.clone();
            bad_source[field] = bad;
            std::fs::write(&source, bad_source.to_string()).unwrap();
            assert!(known_speaker(&known).is_none(), "bad {field}");
        }
        for text in ["f1".to_owned(), "{}".to_owned(), "x".repeat(4097)] {
            std::fs::write(&source, text).unwrap();
            assert!(known_speaker(&known).is_none());
        }
        for modified in [
            SystemTime::now() - Duration::from_secs(4),
            SystemTime::now() + Duration::from_secs(60),
        ] {
            std::fs::write(&source, good.to_string()).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&source)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
            assert!(known_speaker(&known).is_none());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_publication_between_reader_entry_and_open_is_fresh_not_future() {
        use std::{cell::Cell, fs::File};
        let directory =
            std::env::temp_dir().join(format!("known-speaker-publication-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).unwrap();
        let source = known_speaker_of(&directory.join("known"));
        let temporary = directory.join("known-speaker.new");
        let before = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let published_at = before + Duration::from_secs(1);
        let published = Cell::new(false);
        let write = |path: &Path, face: &str, modified| {
            std::fs::write(
                path,
                serde_json::json!({"name":"Synthetic owner", "id":null,
                "boot":"0123456789abcdef", "face":face})
                .to_string(),
            )
            .unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
        };
        write(&source, "f1", before - Duration::from_secs(4));
        let read = read_known_speaker(
            || {
                // Eyes replaces a complete file after reader entry but before
                // open resolves its FD, exactly the real publisher's rename.
                write(&temporary, "f2", published_at);
                std::fs::rename(&temporary, &source)?;
                published.set(true);
                File::open(&source)
            },
            || {
                if published.get() {
                    published_at
                } else {
                    before
                }
            },
        )
        .expect("a publication during open must not look future-dated");
        assert_eq!(read.face, "0123456789abcdef/f2");
        assert_eq!(read.person, lamp_msgs::known::person_id("Synthetic owner"));
        // The ordering fix adds no future tolerance and extends no stale age.
        assert!(read_known_speaker(|| File::open(&source), || before).is_none());
        assert!(
            read_known_speaker(
                || File::open(&source),
                || published_at + crate::known_faces::FRESH
            )
            .is_none()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn positive_speaker_never_hides_a_minority_speaker_or_an_eyes_restart() {
        let (eyes, from, to) = seen(&[
            &[("f1", Yes)],
            &[("f1", Yes)],
            &[("f1", Yes)],
            &[("f2", Yes)],
        ]);
        assert_eq!(eyes.speakers_with_boot(from, to), ["b/f1"]);
        assert!(eyes.positive_speaker(from, to).is_empty());
        let (eyes, from, to) = seen(&[&[("f1", Yes)], &[("f1", Yes)], &[("f1", No)]]);
        assert_eq!(eyes.positive_speaker(from, to), ["b/f1"]);
        eyes.look(to, Some((("new-boot", 0), &faces(&[("f1", Yes)]))));
        assert!(eyes.positive_speaker(from, to).is_empty());
        let (eyes, from, to) = seen(&[&[("f1", Yes)], &[("f1", Yes)]]);
        assert!(eyes.positive_speaker(from, to).is_empty());
        let (eyes, from, to) = seen(&[&[("f1", No)], &[("f1", Unknown)], &[("f1", No)]]);
        assert!(eyes.positive_speaker(from, to).is_empty());
    }

    /// Two observations suffice only when both original frames contain one
    /// positive face. Legacy narrowing still requires its three-look bar.
    #[test]
    fn two_short_looks_require_the_same_single_positive_face() {
        const BOOT: &str = "0123456789abcdef";
        for (case, second, same_frame, expected) in [
            ("same", vec![("f1", Yes)], false, true),
            ("one frame read twice", vec![("f1", Yes)], true, false),
            ("still", vec![("f1", No)], false, false),
            ("unknown", vec![("f1", Unknown)], false, false),
            ("none", vec![], false, false),
            ("other", vec![("f2", Yes)], false, false),
            ("two speakers", vec![("f1", Yes), ("f2", Yes)], false, false),
            (
                "unknown neighbour",
                vec![("f1", Yes), ("f2", Unknown)],
                false,
                false,
            ),
            (
                "silent neighbour",
                vec![("f1", Yes), ("f2", No)],
                false,
                false,
            ),
            (
                "duplicate face",
                vec![("f1", Yes), ("f1", Yes)],
                false,
                false,
            ),
        ] {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            eyes.look(from, Some(((BOOT, 1), &faces(&[("f1", Yes)]))));
            eyes.look(
                from + EVERY,
                Some(((BOOT, if same_frame { 1 } else { 2 }), &faces(&second))),
            );
            let to = from + EVERY * 2;
            assert_eq!(
                eyes.positive_speaker(from, to),
                if expected {
                    vec![format!("{BOOT}/f1")]
                } else {
                    Vec::new()
                },
                "{case}"
            );
            assert!(
                eyes.speakers(from, to).is_empty(),
                "legacy bar unchanged: {case}"
            );
        }
        for (boot, id) in [("bad", "f1"), (BOOT, "bad")] {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            for seq in 0..2 {
                eyes.look(
                    from + EVERY * seq,
                    Some(((boot, u64::from(seq)), &faces(&[(id, Yes)]))),
                );
            }
            assert!(eyes.positive_speaker(from, from + EVERY * 2).is_empty());
        }
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        eyes.look(from, Some(((BOOT, 1), &faces(&[("f1", Yes)]))));
        eyes.look(
            from + EVERY,
            Some((("fedcba9876543210", 2), &faces(&[("f1", Yes)]))),
        );
        assert!(eyes.positive_speaker(from, from + EVERY * 2).is_empty());
        eyes.forget();
        assert!(eyes.positive_speaker(from, from + EVERY * 2).is_empty());
    }

    /// A transient bus error becomes None in the reader. Reading the same
    /// still-fresh publisher frame afterward cannot create another look.
    #[test]
    fn missing_reads_do_not_recount_a_published_speaking_frame() {
        const BOOT: &str = "0123456789abcdef";
        for reads in [3, 7] {
            let eyes = SpeakingFaces::default();
            let from = Instant::now();
            let speaking = faces(&[("f1", Yes)]);
            for read in 0..reads {
                eyes.look(
                    from + EVERY * read,
                    (read % 2 == 0).then_some(((BOOT, 1), &speaking)),
                );
            }
            let to = from + EVERY * reads;
            assert!(eyes.positive_speaker(from, to).is_empty());
            assert!(eyes.enrollment_speaker(from, to).is_none());
            assert!(eyes.speakers(from, to).is_empty());
            let looks = eyes.looks.lock().unwrap();
            assert_eq!(
                looks
                    .kept
                    .iter()
                    .filter(|look| !look.boot.is_empty())
                    .count(),
                1
            );
        }
    }

    #[test]
    fn recovery_after_a_missing_read_counts_a_new_frame() {
        const BOOT: &str = "0123456789abcdef";
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        let speaking = faces(&[("f1", Yes)]);
        eyes.look(from, Some(((BOOT, 1), &speaking)));
        eyes.look(from + EVERY, None);
        eyes.look(from + EVERY * 2, Some(((BOOT, 1), &speaking)));
        assert!(eyes.positive_speaker(from, from + EVERY * 3).is_empty());
        eyes.look(from + EVERY * 3, Some(((BOOT, 2), &speaking)));
        assert_eq!(
            eyes.positive_speaker(from, from + EVERY * 4),
            [format!("{BOOT}/f1")]
        );
        assert!(eyes.enrollment_speaker(from, from + EVERY * 4).is_none());
    }

    #[test]
    fn repeated_missing_reads_keep_the_original_empty_look_rate() {
        let eyes = SpeakingFaces::default();
        let from = Instant::now();
        for read in 0..10 {
            eyes.look(from + EVERY * read, None);
        }
        let looks = eyes.looks.lock().unwrap();
        assert_eq!(looks.kept.len(), 2);
        assert!(looks.kept.iter().all(|look| look.boot.is_empty()));
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
}
