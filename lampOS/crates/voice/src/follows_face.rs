//! What the caller said before the lamp knew their face follows them once it
//! does (#2176, "Remembers you"), on the second route, with
//! `LAMP_MEMORY_FOLLOWS_FACE=on` and the Days memory. Off unless set.
//!
//! "My name is Bramble" said before "remember my face" was the room's
//! (`everyone`), so a new session's "what's my name?" found nothing. With
//! the switch, when a face is saved (`remember_my_face` answered) or
//! recognised in a conversation that began the room's, the room exchanges
//! and the things kept in it that were the caller's own lines are moved to
//! that person's scope by the memory's `adopt`, on the writer's own thread
//! (`crate::remember`), never a turn's.
//!
//! The caller is whoever said the line that saved the face, or the first
//! line the eyes knew them in. Each line is kept at its verdict with its
//! voice print and the faces the eyes saw speaking it ([`Line`]); a line is
//! the caller's only when its voice is the caller's, at
//! [`crate::same_voice::SAME_FROM`] or over, or, with no print, its one
//! speaking face is the caller's, and nothing says otherwise ([`theirs`]).
//! Two faces speaking it, another voice (under
//! [`crate::same_voice::DIFFERENT_BELOW`]), another face, or nothing to
//! tell by (no print and no face, a voice between the two bars) keep it the
//! room's. Nothing moves unless the line the face was saved or known in was
//! seen spoken by one face, the one the eyes know as that person
//! ([`anchored`]).
//!
//! Prints are of a person's voice: kept in this process's memory only, at
//! most [`LINES`] and [`KEPT_FOR`], never journalled, and dropped when
//! their conversation ends or is moved ([`Lines::forget_through`]) or on
//! any forget ([`forgot`]).
use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Only the exact value `on`, with the Days memory.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_FOLLOWS_FACE";
static ON: OnceLock<bool> = OnceLock::new();

/// Whether the caller's own early lines may follow their face once known.
pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// Set it once for this process; the first setting stands. For a caller
/// that runs the Days client itself.
pub fn switch(on: bool) -> bool {
    *ON.get_or_init(|| on)
}

/// Lines kept at once: more than a conversation before a face is known.
pub const LINES: usize = 16;

/// One heard line, as the second route judged it: its voice print, if one
/// came, and the passing ids of the faces seen speaking it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub print: Option<Vec<f32>>,
    pub faces: Vec<String>,
}

/// A line is kept at most this long after it was heard, whatever came
/// after it: a line no exchange came of (a TV line let alone) is not kept
/// until [`LINES`] newer ones push it out. Longer than the 45 s a
/// conversation waits between lines, since its first lines may be older
/// than that when the face is known.
pub const KEPT_FOR: Duration = Duration::from_secs(180);

/// The last [`LINES`] lines, by request.
#[derive(Default)]
pub struct Lines {
    kept: VecDeque<(u64, Instant, Line)>,
}

impl Lines {
    /// `request`'s line, in place of any kept for it before.
    pub fn mark(&mut self, request: u64, line: Line) {
        self.mark_at(request, line, Instant::now());
    }

    /// [`Self::mark`], heard at `at`.
    pub fn mark_at(&mut self, request: u64, line: Line, at: Instant) {
        self.prune(at);
        self.kept.retain(|(of, _, _)| *of != request);
        if self.kept.len() == LINES {
            self.kept.pop_front();
        }
        self.kept.push_back((request, at, line));
    }

    /// Drop every line heard more than [`KEPT_FOR`] before `now`.
    pub fn prune(&mut self, now: Instant) {
        self.kept
            .retain(|(_, at, _)| now.saturating_duration_since(*at) <= KEPT_FOR);
    }

    /// The lines still waiting for their print.
    pub fn unprinted(&self) -> Vec<u64> {
        self.kept
            .iter()
            .filter(|(_, _, line)| line.print.is_none())
            .map(|(request, _, _)| *request)
            .collect()
    }

    /// `request`'s print, come after its verdict.
    pub fn printed(&mut self, request: u64, print: Vec<f32>) {
        if let Some((_, _, line)) = self.kept.iter_mut().find(|(of, _, _)| *of == request) {
            line.print.get_or_insert(print);
        }
    }

    pub fn of(&self, request: u64) -> Option<&Line> {
        self.kept
            .iter()
            .find(|(of, _, _)| *of == request)
            .map(|(_, _, line)| line)
    }

    /// Drop every line up to and with `request`: their conversation ended
    /// or was moved.
    pub fn forget_through(&mut self, request: u64) {
        self.kept.retain(|(of, _, _)| *of > request);
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

/// This process's lines, kept by the second route's rule.
pub static LINES_HEARD: Mutex<Lines> = Mutex::new(Lines {
    kept: VecDeque::new(),
});

/// With the switch, `request`'s line into [`LINES_HEARD`].
pub fn mark(request: u64, print: Option<Vec<f32>>, faces: Vec<String>) {
    if !on() {
        return;
    }
    if let Ok(mut lines) = LINES_HEARD.lock() {
        lines.mark(request, Line { print, faces });
    }
}

/// The forgets said in this process, of any kind: the writer drops a face
/// saved before the last ([`forgot`]).
pub static FORGETS: AtomicU64 = AtomicU64::new(0);

/// The eyes' known file, read for whose face is known now.
static KNOWN: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Read the eyes' known file at `known`, with its face beside it; the first
/// setting stands.
pub fn reading_known(known: std::path::PathBuf) {
    let _ = KNOWN.set(known);
}

/// The person the eyes know now, as a memory id, and their face's passing
/// id (`fN`), read together ([`crate::speaking_faces::known_with_face`]);
/// `None` without the known file, or with it stale.
pub fn known_now() -> Option<(String, String)> {
    let (name, face) = crate::speaking_faces::known_with_face(KNOWN.get()?)?;
    Some((crate::known_faces::memory_person(&name), face))
}

/// A forget was said (`forget_my_face`, `forget everything`, `forget` by
/// what it is about or by id): with the switch, every line kept is dropped
/// and the forget counted, so nothing heard or saved before it moves.
pub fn forgot() {
    if on() {
        forgot_in(&LINES_HEARD, &FORGETS);
    }
}

/// [`forgot`] on `lines`, counted in `forgets`.
pub fn forgot_in(lines: &Mutex<Lines>, forgets: &AtomicU64) {
    forgets.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut lines) = lines.lock() {
        lines.kept.clear();
    }
}

/// Whether `line`, the one a face was saved or known in, may anchor a move
/// to `to` (a memory id): exactly one face was seen speaking it, and that
/// face is the one the eyes know now as `to` (`known`: a memory id and the
/// face's passing id, [`known_now`]). Off camera, two faces, or a face not
/// known as `to` moves nothing: a voice off camera while someone else's
/// face is in view, known or just saved, never fills that face's memory.
/// The line's print matching itself proves nothing.
pub fn anchored(line: &Line, to: &str, known: Option<(String, String)>) -> Result<(), Why> {
    let face = match line.faces.as_slice() {
        [face] => face,
        [] => return Err(Why::CantTell),
        _ => return Err(Why::TwoFaces),
    };
    let Some((_, theirs)) = known.filter(|(person, _)| person == to) else {
        return Err(Why::CantTell);
    };
    // A line's faces carry the eyes' boot (`boot/f3`); the known face's
    // file has the passing id alone. Within one conversation's minutes it
    // is the same track.
    let id = face.rsplit_once('/').map_or(face.as_str(), |(_, id)| id);
    if id == theirs {
        Ok(())
    } else {
        Err(Why::CantTell)
    }
}

/// The longest a face saved or known waits for its line's print once that
/// line is written: the saving line ("remember my face") is short, and its
/// print, asked for at its end, often comes after the turn's first output
/// (#829), or never under 1.5 s ([`crate::same_voice::LEAST_SAMPLES`]).
/// The move is weighed as soon as the print is in or none is coming, at
/// the latest this long after, or sooner when the conversation ends or
/// someone else's line comes.
pub const PRINT_WAIT: Duration = Duration::from_secs(3);

/// While it waits, the print is looked for this often.
pub const PRINT_POLL: Duration = Duration::from_millis(100);

type Fill = Box<dyn Fn(u64) -> bool + Send + Sync>;
static FILL: OnceLock<Fill> = OnceLock::new();

/// The second route's `fill`: puts the prints come since into
/// [`LINES_HEARD`] and says whether the print of the line it is given is
/// still on its way. The first setting stands.
pub fn filling_prints(fill: impl Fn(u64) -> bool + Send + Sync + 'static) {
    let _ = FILL.set(Box::new(fill));
}

/// [`filling_prints`]'s, for `request`: false, nothing filled, without one.
pub fn fill_prints(request: u64) -> bool {
    FILL.get().is_some_and(|fill| fill(request))
}

/// The caller, by `anchor`, the line a face was saved or known in, already
/// [`anchored`] to `to`. With no print of its own, the caller's print is
/// taken from another of the conversation's lines, `others`, also seen
/// spoken by exactly one face, the same face track, known now as `to`:
/// the one most like the rest of them, and only when no two of them
/// disagree (a cosine under [`crate::same_voice::SAME_FROM`]). With none,
/// the caller has no print, and only unprinted lines on their one face are
/// theirs ([`theirs`]).
pub fn caller<'a>(
    anchor: &Line,
    others: impl IntoIterator<Item = &'a Line>,
    to: &str,
    known: Option<(String, String)>,
) -> Line {
    if anchor.print.is_some() {
        return anchor.clone();
    }
    // Only the same one face, the same track, known now as `to`.
    let prints: Vec<&Vec<f32>> = others
        .into_iter()
        .filter(|line| line.faces == anchor.faces && anchored(line, to, known.clone()).is_ok())
        .filter_map(|line| line.print.as_ref())
        .collect();
    let mut strongest: Option<(f32, &Vec<f32>)> = None;
    for (at, print) in prints.iter().enumerate() {
        let mut likeness = 0.0;
        for (other, against) in prints.iter().enumerate() {
            if other == at {
                continue;
            }
            match crate::same_voice::cosine(print, against) {
                // Two voices on the one face: nothing to take.
                Some(cosine) if cosine >= crate::same_voice::SAME_FROM => likeness += cosine,
                _ => return anchor.clone(),
            }
        }
        if strongest.is_none_or(|(best, _)| likeness > best) {
            strongest = Some((likeness, print));
        }
    }
    Line {
        print: strongest.map(|(_, print)| print.clone()),
        faces: anchor.faces.clone(),
    }
}

/// Why a line stays the room's, counted in the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Two faces or more seen speaking it.
    TwoFaces,
    /// Its voice another's: under [`crate::same_voice::DIFFERENT_BELOW`].
    OtherVoice,
    /// Its one speaking face not the caller's.
    OtherFace,
    /// Nothing to tell it by: no print, no face, a voice between the bars.
    CantTell,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TwoFaces => "two_faces",
            Self::OtherVoice => "other_voice",
            Self::OtherFace => "other_face",
            Self::CantTell => "cant_tell",
        }
    }
}

/// Whether `line` is the caller's own, the caller being whoever said
/// `caller` ([`anchored`] says whether that was the person moved to). Two
/// faces, another voice and another face each keep it the room's, whatever
/// else agrees. Then a line with a print moves only on the same voice, at
/// [`crate::same_voice::SAME_FROM`] or over: a matching face beside an
/// unclear voice (a TV while the caller laughs or eats) is not enough. A
/// line with no print moves only on the caller's one face.
pub fn theirs(line: &Line, caller: &Line) -> Result<(), Why> {
    if line.faces.len() > 1 || caller.faces.len() > 1 {
        return Err(Why::TwoFaces);
    }
    let voice = match (&line.print, &caller.print) {
        (Some(line), Some(caller)) => crate::same_voice::cosine(line, caller),
        _ => None,
    };
    if voice.is_some_and(|cosine| cosine < crate::same_voice::DIFFERENT_BELOW) {
        return Err(Why::OtherVoice);
    }
    let face = match (line.faces.first(), caller.faces.first()) {
        (Some(line), Some(caller)) => Some(line == caller),
        _ => None,
    };
    if face == Some(false) {
        return Err(Why::OtherFace);
    }
    let agrees = match &line.print {
        Some(_) => voice.is_some_and(|cosine| cosine >= crate::same_voice::SAME_FROM),
        None => face == Some(true),
    };
    if agrees { Ok(()) } else { Err(Why::CantTell) }
}

/// How many lines stayed the room's, and why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stayed {
    pub two_faces: usize,
    pub other_voice: usize,
    pub other_face: usize,
    pub cant_tell: usize,
}

impl Stayed {
    pub fn count(&mut self, why: Why) {
        match why {
            Why::TwoFaces => self.two_faces += 1,
            Why::OtherVoice => self.other_voice += 1,
            Why::OtherFace => self.other_face += 1,
            Why::CantTell => self.cant_tell += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.two_faces + self.other_voice + self.other_face + self.cant_tell
    }
}

impl std::fmt::Display for Stayed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "stayed={} (two_faces={} other_voice={} other_face={} cant_tell={})",
            self.total(),
            self.two_faces,
            self.other_voice,
            self.other_face,
            self.cant_tell
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(print: Option<[f32; 2]>, faces: &[&str]) -> Line {
        Line {
            print: print.map(|print| print.to_vec()),
            faces: faces.iter().map(|face| (*face).to_owned()).collect(),
        }
    }

    const CALLER: [f32; 2] = [1.0, 0.0];
    /// 0.80 against the caller's: over 0.70.
    const SAME: [f32; 2] = [0.8, 0.6];
    /// 0.50 against the caller's: between the bars.
    const BETWEEN: [f32; 2] = [0.5, 0.866];
    /// 0.31 and 0.69 against the caller's: a TV's voice, between the bars.
    const TV_LOW: [f32; 2] = [0.31, 0.950_6];
    const TV_HIGH: [f32; 2] = [0.69, 0.723_8];
    /// 0.10 against the caller's: under 0.30.
    const OTHER: [f32; 2] = [0.1, 0.995];

    #[test]
    fn a_line_is_the_callers_by_their_voice_or_their_one_face_and_nothing_against() {
        let caller = line(Some(CALLER), &["f1"]);
        assert_eq!(theirs(&line(Some(SAME), &[]), &caller), Ok(()));
        assert_eq!(theirs(&line(None, &["f1"]), &caller), Ok(()));
        assert_eq!(theirs(&line(Some(SAME), &["f1"]), &caller), Ok(()));
    }

    /// The reviewer-lead's P2 on #2673: a line with a print moves only when
    /// its voice agrees; the caller's face beside an unclear voice is not
    /// enough. A TV's voice at 0.30 to 0.70 while the caller's lips move
    /// (laughing, eating) stays the room's, and so does a printed line when
    /// the caller has no print to agree with.
    #[test]
    fn a_printed_line_moves_only_when_its_voice_agrees_whatever_the_face() {
        let caller = line(Some(CALLER), &["f1"]);
        for tv in [BETWEEN, TV_LOW, TV_HIGH] {
            assert_eq!(
                theirs(&line(Some(tv), &["f1"]), &caller),
                Err(Why::CantTell),
                "{tv:?}"
            );
        }
        let unprinted = line(None, &["f1"]);
        assert_eq!(
            theirs(&line(Some(SAME), &["f1"]), &unprinted),
            Err(Why::CantTell)
        );
        // With no print, the caller's one face alone may move it.
        assert_eq!(theirs(&line(None, &["f1"]), &unprinted), Ok(()));
    }

    /// The reviewer-lead's P1 on #2673: the line a face was saved or known
    /// in anchors the move only when exactly one face was seen speaking it
    /// and that face is the one the eyes know as `to` now. Its print
    /// matching itself proves nothing.
    #[test]
    fn the_anchor_is_spoken_by_the_one_face_known_as_whose_it_becomes() {
        let known = |name: &str, face: &str| Some((name.to_owned(), face.to_owned()));
        let spoken = line(Some(CALLER), &["b1/f1"]);
        assert_eq!(anchored(&spoken, "p-a", known("p-a", "f1")), Ok(()));
        // Off camera: no face seen speaking it, whatever its print.
        assert_eq!(
            anchored(&line(Some(CALLER), &[]), "p-a", known("p-a", "f1")),
            Err(Why::CantTell)
        );
        assert_eq!(
            anchored(
                &line(Some(CALLER), &["b1/f1", "b1/f2"]),
                "p-a",
                known("p-a", "f1")
            ),
            Err(Why::TwoFaces)
        );
        // Another face known as them, someone else known, nobody known.
        for known in [known("p-a", "f2"), known("p-b", "f1"), None] {
            assert_eq!(
                anchored(&spoken, "p-a", known.clone()),
                Err(Why::CantTell),
                "{known:?}"
            );
        }
        // An id of another boot's face is not this one.
        assert_eq!(
            anchored(&line(None, &["b1/f12"]), "p-a", known("p-a", "f1")),
            Err(Why::CantTell)
        );
    }

    /// The reviewer-lead's P2 on #2673 (b): a saving line with no print
    /// takes the caller's print from another line of the conversation seen
    /// spoken by the same one face, known as `to`, the one most like the
    /// rest; never from a line on another face, two faces or none, nor when
    /// two such lines disagree.
    #[test]
    fn an_unprinted_anchor_takes_the_print_of_a_line_on_the_same_one_face() {
        let known = || Some(("p-a".to_owned(), "f1".to_owned()));
        let anchor = line(None, &["b1/f1"]);
        let on_it = line(Some(SAME), &["b1/f1"]);
        let taken = caller(&anchor, [&on_it], "p-a", known());
        assert_eq!(taken, line(Some(SAME), &["b1/f1"]));
        // The strongest: of three on that face, the one most like the rest.
        let (a, b, c) = (
            line(Some([1.0, 0.0]), &["b1/f1"]),
            line(Some([0.95, 0.312]), &["b1/f1"]),
            line(Some([0.8, 0.6]), &["b1/f1"]),
        );
        assert_eq!(caller(&anchor, [&a, &b, &c], "p-a", known()).print, b.print);
        // A printed anchor is the caller as it is.
        let printed = line(Some(CALLER), &["b1/f1"]);
        assert_eq!(caller(&printed, [&on_it], "p-a", known()), printed);
        for others in [
            vec![line(Some(SAME), &["b1/f2"])],
            vec![line(Some(SAME), &["b1/f1", "b1/f2"])],
            vec![line(Some(SAME), &[])],
            // Another boot's track, the same passing id: another face.
            vec![line(Some(SAME), &["b0/f1"])],
            vec![line(None, &["b1/f1"])],
            // Two voices on the one face: nothing to take.
            vec![
                line(Some(CALLER), &["b1/f1"]),
                line(Some(OTHER), &["b1/f1"]),
            ],
            vec![
                line(Some(CALLER), &["b1/f1"]),
                line(Some(TV_HIGH), &["b1/f1"]),
            ],
        ] {
            assert_eq!(
                caller(&anchor, &others, "p-a", known()),
                anchor,
                "{others:?}"
            );
        }
        // The face known now as someone else, or nobody known.
        for known in [Some(("p-b".to_owned(), "f1".to_owned())), None] {
            assert_eq!(caller(&anchor, [&on_it], "p-a", known), anchor);
        }
    }

    #[test]
    fn two_faces_another_voice_or_face_or_nothing_to_tell_keep_a_line_the_rooms() {
        let caller = line(Some(CALLER), &["f1"]);
        for (said, why) in [
            (line(Some(SAME), &["f1", "f2"]), Why::TwoFaces),
            (line(Some(OTHER), &["f1"]), Why::OtherVoice),
            (line(Some(OTHER), &[]), Why::OtherVoice),
            (line(Some(SAME), &["f2"]), Why::OtherFace),
            (line(None, &["f2"]), Why::OtherFace),
            (line(Some(BETWEEN), &[]), Why::CantTell),
            // The caller's face and a voice between the bars: the voice
            // doesn't agree, so the face alone doesn't tell.
            (line(Some(BETWEEN), &["f1"]), Why::CantTell),
            // Too short to print, no face seen speaking it.
            (line(None, &[]), Why::CantTell),
        ] {
            assert_eq!(theirs(&said, &caller), Err(why), "{said:?}");
        }
        // A caller with two faces, or nothing to tell them by, moves nothing.
        assert_eq!(
            theirs(&caller, &line(Some(CALLER), &["f1", "f2"])),
            Err(Why::TwoFaces)
        );
        let unknown = line(None, &[]);
        assert_eq!(theirs(&unknown, &unknown), Err(Why::CantTell));
    }

    #[test]
    fn lines_are_bounded_filled_late_and_forgotten_through_a_request() {
        let mut lines = Lines::default();
        for request in 1..=(LINES as u64 + 2) {
            lines.mark(request, line(None, &[]));
        }
        assert!(lines.of(2).is_none() && lines.of(3).is_some());
        lines.printed(5, CALLER.to_vec());
        assert_eq!(lines.of(5).unwrap().print.as_deref(), Some(&CALLER[..]));
        assert!(!lines.unprinted().contains(&5));
        lines.forget_through(LINES as u64);
        assert_eq!(lines.unprinted(), [LINES as u64 + 1, LINES as u64 + 2]);
        lines.forget_through(u64::MAX);
        assert!(lines.is_empty());
    }

    /// The reviewer-lead's P3 on #2673: a line with no exchange (a TV line
    /// let alone) is not kept past [`KEPT_FOR`], whatever came after it.
    #[test]
    fn a_line_is_kept_no_longer_than_kept_for() {
        let mut lines = Lines::default();
        let then = Instant::now();
        lines.mark_at(1, line(Some(CALLER), &[]), then);
        lines.mark_at(2, line(Some(CALLER), &[]), then + KEPT_FOR / 2);
        lines.prune(then + KEPT_FOR);
        assert!(lines.of(1).is_some(), "dropped at the bound, not before");
        lines.prune(then + KEPT_FOR + Duration::from_millis(1));
        assert!(lines.of(1).is_none() && lines.of(2).is_some());
        lines.mark_at(3, line(None, &[]), then + KEPT_FOR * 2);
        assert!(lines.of(2).is_none() && lines.of(3).is_some());
    }

    /// The reviewer-lead's P2 on #2673: any forget drops every line kept
    /// and counts one, for the writer to drop a face saved before it.
    #[test]
    fn a_forget_drops_every_line_and_is_counted() {
        let (lines, forgets) = (Mutex::new(Lines::default()), AtomicU64::new(0));
        lines.lock().unwrap().mark(1, line(Some(CALLER), &["f1"]));
        forgot_in(&lines, &forgets);
        assert!(lines.lock().unwrap().is_empty());
        assert_eq!(forgets.load(Ordering::Acquire), 1);
    }
}
