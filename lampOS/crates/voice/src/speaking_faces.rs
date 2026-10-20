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

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Who the eyes saw speaking through a line: the passing ids, none when no
/// face read speaking or there were too few looks to say.
pub type Speakers = Vec<String>;

struct Look {
    at: Instant,
    /// The passing ids that read speaking in this look.
    speaking: Vec<String>,
    /// Of those, the ones turned to the lamp (`facing`), for a line the lamp
    /// began itself (#2287's follow-up, [`SpeakingFaces::turned_speakers`]).
    turned: Vec<String>,
}

#[derive(Default)]
struct Looks {
    kept: VecDeque<Look>,
    /// The last frame counted, by its publisher's boot and sequence.
    frame: Option<(String, u64)>,
}

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
        let turned = speaking_faces
            .iter()
            .filter(|face| face.facing == Some(true))
            .map(|face| face.id.clone())
            .collect();
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
            speaking,
            turned,
        });
    }

    /// The faces that read speaking in at least half of the looks from
    /// `from` to `to`.
    pub fn speakers(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| &look.speaking)
    }

    /// The faces that read speaking and turned to the lamp in at least half
    /// of the looks from `from` to `to`: someone at the lamp talking to it,
    /// not the room (#2287's follow-up).
    pub fn turned_speakers(&self, from: Instant, to: Instant) -> Speakers {
        self.counted(from, to, |look| &look.turned)
    }

    /// The ids `of` each look gives in at least half of the looks from
    /// `from` to `to`, and none with fewer than [`FEWEST_LOOKS`].
    fn counted(&self, from: Instant, to: Instant, of: impl Fn(&Look) -> &Vec<String>) -> Speakers {
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
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for id in during.iter().flat_map(|look| of(look)) {
            match counts.iter_mut().find(|(seen, _)| *seen == id.as_str()) {
                Some((_, count)) => *count += 1,
                None => counts.push((id, 1)),
            }
        }
        counts
            .into_iter()
            .filter(|(_, count)| count * 2 >= during.len())
            .map(|(id, _)| id.to_owned())
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
    let beside = known_face_of(known);
    let first = face_in(&beside)?;
    let name = crate::known_faces::known_now(known)?;
    (face_in(&beside)? == first).then_some((name, first))
}

fn face_in(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let age = SystemTime::now()
        .duration_since(file.metadata().ok()?.modified().ok()?)
        .ok()?;
    if age >= crate::known_faces::FRESH {
        return None;
    }
    let mut text = String::new();
    file.take(64).read_to_string(&mut text).ok()?;
    let id = text.trim();
    let digits = id.strip_prefix('f')?;
    (!digits.is_empty() && digits.len() <= 10 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.to_owned())
}

/// Whether a line admitted for `person` may be someone else's: a face
/// spoke through it that is not the face of the person the eyes know now,
/// or the eyes now know someone other than `person`. No face speaking
/// leaves the line as it was admitted.
pub fn someone_elses(speakers: &[String], person: &str, known: Option<(String, String)>) -> bool {
    let theirs = known
        .filter(|(name, _)| crate::remember::person_id(name) == person)
        .map(|(_, face)| face);
    speakers
        .iter()
        .any(|id| theirs.as_deref() != Some(id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{Face, Speaking};

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
}
