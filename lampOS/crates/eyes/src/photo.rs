//! A face remembered from a photo (`lamp-eyes --photo-dir`, the first builder
//! on #1722, 19 Oct 21:51): the app sends a picture of the person, something
//! on the lamp puts it in the eyes' photo folder, and the eyes are asked
//! `{"remember":"NAME","photo":"FILE"}` on their `who` socket.
//!
//! The file is read once and removed whatever follows ([`take`]): no
//! picture is kept. Its one face ([`one_face`]) is cut out upright and
//! printed as recognition does (`who::align`, `Recogniser::print`), and only
//! the print is kept, with the name, as a face remembered at the lamp is.
use crate::faces::Face;
use std::{
    fs::File,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
};

/// The photo folder, made (0700) if it isn't there; a
/// folder already there is refused unless it is a folder, not a link, and
/// no one else's to read (#1968: lamp-cloud, the same user, writes into
/// it).
pub fn ready(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::symlink_metadata(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new().mode(0o700).create(dir)
        }
        Err(error) => Err(error),
        Ok(meta) if !meta.file_type().is_dir() => {
            Err(io::Error::other("the photo folder is not a folder"))
        }
        Ok(meta) if meta.permissions().mode() & 0o077 != 0 => Err(io::Error::other(
            "the photo folder is open to others; it must be 0700",
        )),
        Ok(_) => Ok(()),
    }
}

/// The largest photo read.
pub const MOST_BYTES: u64 = 8 << 20;
/// A second face this share of the first's height or more makes the photo
/// two people's.
pub const SECOND_FACE: f32 = 0.5;

/// Why a photo gave no face to remember, in the eyes' own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTaken {
    /// No file, a link, not a plain file, too large or not a picture.
    Unreadable,
    NoFace,
    /// The one face is under `who::NEAR_ENOUGH` of the picture's height.
    TooFar,
    TwoFaces,
}

impl NotTaken {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::NoFace => "no_face",
            Self::TooFar => "too_far",
            Self::TwoFaces => "two_faces",
        }
    }
}

/// Whether `name` is a plain file name in the photo folder: no folder of
/// its own, not hidden, at most 128 bytes.
pub fn plain_file(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The photo's bytes, read once from `dir`, and the file removed whatever
/// they are: no picture is kept. A link, anything but a plain file and a
/// file over [`MOST_BYTES`] are refused, and removed too.
pub fn take(dir: &Path, name: &str) -> Result<Vec<u8>, NotTaken> {
    if !plain_file(name) {
        return Err(NotTaken::Unreadable);
    }
    let path = dir.join(name);
    let read = (|| -> io::Result<Vec<u8>> {
        // The entry itself, not what a link names; then the file opened is
        // checked to be that same entry. The folder is the eyes' own (0700).
        let entry = std::fs::symlink_metadata(&path)?;
        if !entry.file_type().is_file() || entry.len() > MOST_BYTES {
            return Err(io::Error::other("not a photo"));
        }
        let file = File::open(&path)?;
        let meta = file.metadata()?;
        if meta.dev() != entry.dev() || meta.ino() != entry.ino() || meta.len() > MOST_BYTES {
            return Err(io::Error::other("not a photo"));
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        File::take(file, MOST_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MOST_BYTES {
            return Err(io::Error::other("not a photo"));
        }
        Ok(bytes)
    })();
    // Gone whatever it was, a link included (the link, not what it names);
    // a folder is left.
    let _ = std::fs::remove_file(&path);
    read.map_err(|_| NotTaken::Unreadable)
}

/// The photo's one face: the largest, if it is at least `near` of the
/// picture's height (`tall` pixels) and no other face is [`SECOND_FACE`] of
/// its height or more.
pub fn one_face(faces: &[Face], tall: f32, near: f32) -> Result<&Face, NotTaken> {
    let largest = faces
        .iter()
        .max_by(|a, b| a.height.total_cmp(&b.height))
        .ok_or(NotTaken::NoFace)?;
    let others = faces
        .iter()
        .filter(|face| !std::ptr::eq(*face, largest))
        .any(|face| face.height >= largest.height * SECOND_FACE);
    if others {
        return Err(NotTaken::TwoFaces);
    }
    if largest.height / tall.max(1.0) < near {
        return Err(NotTaken::TooFar);
    }
    Ok(largest)
}

/// The print of the photo's one face, from its JPEG bytes: decoded at the
/// detector's size as a camera frame is, its faces found, the one face cut
/// out upright and printed as recognition does.
pub fn print_of(
    bytes: &[u8],
    detector: &crate::faces::Detector,
    recogniser: &crate::who::Recogniser,
) -> Result<Vec<f32>, NotTaken> {
    let picture = crate::picture::Picture::from_jpeg(bytes).map_err(|_| NotTaken::Unreadable)?;
    let faces = detector.find(&picture).map_err(|_| NotTaken::Unreadable)?;
    let face = one_face(&faces, picture.filled.1 as f32, crate::who::NEAR_ENOUGH)?;
    let aligned = crate::who::align(&picture, &face.marks).ok_or(NotTaken::NoFace)?;
    recogniser.print(&aligned).map_err(|_| NotTaken::NoFace)
}

/// An unconfirmed face is deleted this long after it was added from a
/// photo, if its person never said yes at the lamp (the first builder on
/// #1968, 22:12).
pub const UNCONFIRMED_FOR_S: u64 = 30 * 86_400;
/// The most unconfirmed faces kept at once.
pub const MOST_UNCONFIRMED: usize = 20;
/// A meeting is over once an asked face hasn't been seen this long: its
/// person gave no answer, and the face is let go.
pub const MEETING_GAP_US: u64 = 20_000_000;
/// The prints of an unconfirmed face seen at the lamp kept beside its photo's,
/// the newest: with the photo's, the most a person keeps (#2854). A selfie is
/// one view, the phone's; seen only by it, the face was named only where it
/// looked most like the selfie, near the middle (lamp-0c89, 21 Oct, #1968).
pub const SEEN_PRINTS: usize = crate::who::MOST_PRINTS - 1;

/// One face added from a photo, not yet agreed to by its person.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Unconfirmed {
    name: String,
    print: Vec<f32>,
    added_unix_s: u64,
    /// The voice has asked them, in this meeting.
    #[serde(default)]
    asked: bool,
    /// This record's number in memory, never in the file: a face added
    /// again under the name is another record, so a yes waiting for the
    /// last one keeps nothing ([`PendingYes`], #2856).
    #[serde(skip)]
    record: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UnconfirmedFile {
    version: u32,
    unconfirmed: Vec<Unconfirmed>,
}

/// The faces added from photos and not yet agreed to, in their own private
/// file beside the people's (mode 0600), never in it: an older build reads
/// the people's file only, so it never takes one of these as someone who
/// asked. None is ever named, counted, greeted or used for "who is this?";
/// the eyes say only that one is at the lamp, so the voice can ask.
pub struct Unconfirmeds {
    path: std::path::PathBuf,
    kept: Vec<Unconfirmed>,
    /// Each face's prints as the lamp saw it, the newest [`SEEN_PRINTS`]: in
    /// memory only, never in the file, kept with its photo's on a yes and
    /// gone with it otherwise.
    seen: Vec<(String, Vec<Vec<f32>>)>,
    /// The last record's number given ([`Unconfirmed::record`]).
    records: u64,
}

impl Unconfirmeds {
    /// Beside the people's file `people`: `<people>.from-photos`.
    pub fn beside(people: &Path) -> std::path::PathBuf {
        let mut path = people.as_os_str().to_owned();
        path.push(".from-photos");
        path.into()
    }

    /// The kept file, or none; a file that can't be read is said and left
    /// as it is, and nothing is unconfirmed until the next one added
    /// replaces it.
    pub fn open(path: std::path::PathBuf) -> Self {
        let kept = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<UnconfirmedFile>(&bytes) {
                Ok(file) if file.version == 1 => file.unconfirmed,
                // Unconfirmed prints out of the 30 days' reach are no one's
                // to keep: the file goes (the reviewer on #1968, 22:28).
                _ => {
                    let _ = std::fs::remove_file(&path);
                    eprintln!(
                        "lamp-eyes: the faces from photos could not be read; the file is removed and none is kept"
                    );
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };
        let mut faces = Self {
            path,
            kept,
            seen: Vec::new(),
            records: 0,
        };
        for at in 0..faces.kept.len() {
            faces.kept[at].record = faces.next_record();
        }
        faces
    }

    fn next_record(&mut self) -> u64 {
        self.records += 1;
        self.records
    }

    /// The record kept for `name` now, if any: another after it is let go
    /// and added again ([`Unconfirmed::record`]).
    pub fn record_of(&self, name: &str) -> Option<u64> {
        self.find(name).map(|at| self.kept[at].record)
    }

    fn save(&self) -> io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let text = serde_json::to_vec(&UnconfirmedFile {
            version: 1,
            unconfirmed: self.kept.clone(),
        })?;
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(".new");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&text)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.path)
    }

    /// Keeps `print` for `name`, unconfirmed, in place of any for that name.
    pub fn add(&mut self, name: &str, print: Vec<f32>, now_unix_s: u64) -> io::Result<()> {
        let mut next = self.kept.clone();
        next.retain(|kept| !crate::who::same_name(&kept.name, name));
        if next.len() >= MOST_UNCONFIRMED {
            return Err(io::Error::other("too many faces from photos"));
        }
        let record = self.next_record();
        next.push(Unconfirmed {
            name: name.to_owned(),
            print,
            added_unix_s: now_unix_s,
            asked: false,
            record,
        });
        let before = std::mem::replace(&mut self.kept, next);
        self.save().inspect_err(|_| self.kept = before)?;
        // A new face under the name: nothing the lamp saw of the last.
        self.take_seen(name);
        Ok(())
    }

    /// `print` of the unconfirmed face `name`, as the lamp saw it, kept in
    /// memory with the newest [`SEEN_PRINTS`] of it. Only for a face that is
    /// `name`'s by [`Self::here`] and nobody known's.
    pub fn saw(&mut self, name: &str, print: Vec<f32>) {
        if self.find(name).is_none() {
            return;
        }
        let at = match self
            .seen
            .iter()
            .position(|(seen, _)| crate::who::same_name(seen, name))
        {
            Some(at) => at,
            None => {
                self.seen.push((name.to_owned(), Vec::new()));
                self.seen.len() - 1
            }
        };
        let prints = &mut self.seen[at].1;
        prints.push(print);
        let over = prints.len().saturating_sub(SEEN_PRINTS);
        prints.drain(..over);
    }

    fn take_seen(&mut self, name: &str) -> Vec<Vec<f32>> {
        match self
            .seen
            .iter()
            .position(|(seen, _)| crate::who::same_name(seen, name))
        {
            Some(at) => self.seen.remove(at).1,
            None => Vec::new(),
        }
    }

    /// The unconfirmed face this print is, if any, by the likeness that
    /// recognises a face.
    pub fn here(&self, print: &[f32]) -> Option<String> {
        self.kept
            .iter()
            .map(|kept| (kept, crate::who::likeness(&kept.print, print)))
            .filter(|(_, likeness)| *likeness >= crate::who::SAME_PERSON)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(kept, _)| kept.name.clone())
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.kept
            .iter()
            .position(|kept| crate::who::same_name(&kept.name, name))
    }

    /// The voice has asked them; `false` if there is no such face.
    pub fn asked(&mut self, name: &str) -> io::Result<bool> {
        let Some(at) = self.find(name) else {
            return Ok(false);
        };
        self.kept[at].asked = true;
        self.save().map(|()| true)
    }

    /// Whether `name` was asked, in this meeting.
    pub fn was_asked(&self, name: &str) -> bool {
        self.find(name).is_some_and(|at| self.kept[at].asked)
    }

    /// The names asked, whose meeting may end with no answer.
    pub fn asked_names(&self) -> Vec<String> {
        self.kept
            .iter()
            .filter(|kept| kept.asked)
            .map(|kept| kept.name.clone())
            .collect()
    }

    /// Their yes: the print, taken out to be kept as an ordinary one. Put
    /// back with [`Self::put_back`] if the people's store refuses it.
    pub fn confirmed(&mut self, name: &str) -> io::Result<Option<Taken>> {
        let Some(at) = self.find(name) else {
            return Ok(None);
        };
        let kept = self.kept.remove(at);
        if let Err(error) = self.save() {
            self.kept.insert(at, kept);
            return Err(error);
        }
        let seen = self.take_seen(name);
        Ok(Some(Taken(kept, seen)))
    }

    /// A yes the people's store refused (full, or not written): the face is
    /// unconfirmed again as it was, its 30 days unchanged.
    pub fn put_back(&mut self, taken: Taken) -> io::Result<()> {
        self.kept
            .retain(|kept| !crate::who::same_name(&kept.name, &taken.0.name));
        if !taken.1.is_empty() {
            self.seen.push((taken.0.name.clone(), taken.1));
        }
        self.kept.push(taken.0);
        self.save()
    }

    /// Their no, no answer, or "forget my face": the print deleted.
    pub fn let_go(&mut self, name: &str) -> io::Result<bool> {
        let Some(at) = self.find(name) else {
            return Ok(false);
        };
        self.kept.remove(at);
        self.take_seen(name);
        self.save().map(|()| true)
    }

    /// Deletes those added [`UNCONFIRMED_FOR_S`] ago or more; how many.
    pub fn expire(&mut self, now_unix_s: u64) -> io::Result<usize> {
        let before = self.kept.len();
        self.kept
            .retain(|kept| now_unix_s.saturating_sub(kept.added_unix_s) < UNCONFIRMED_FOR_S);
        let gone = before - self.kept.len();
        let kept = &self.kept;
        self.seen.retain(|(name, _)| {
            kept.iter()
                .any(|face| crate::who::same_name(&face.name, name))
        });
        if gone > 0 {
            self.save()?;
        }
        Ok(gone)
    }

    /// The names that wait for their person's yes: only for the lamp's
    /// owner's own page (`face.list`, #2192), who sent them. Never said at
    /// the lamp, counted or greeted.
    pub fn names(&self) -> Vec<String> {
        self.kept.iter().map(|kept| kept.name.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

/// A face taken out on its person's yes: its name and prints, and what to put
/// back if the people's store refuses it.
pub struct Taken(Unconfirmed, Vec<Vec<f32>>);

impl Taken {
    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn print(&self) -> Vec<f32> {
        self.0.print.clone()
    }

    /// What is kept on the yes: the photo's print, then the newest prints
    /// the lamp took of the face (#2854).
    pub fn prints(&self) -> Vec<Vec<f32>> {
        std::iter::once(self.0.print.clone())
            .chain(self.1.iter().cloned())
            .collect()
    }
}

/// Whether the eyes take an answer for `name`: only while that face is the
/// one at the lamp, seen within `fresh` (the reviewer on #1968, 22:28). A
/// yes for a face elsewhere would remember someone who never agreed.
pub fn is_here(
    here: Option<(&str, std::time::Duration)>,
    name: &str,
    fresh: std::time::Duration,
) -> bool {
    here.is_some_and(|(here, ago)| ago < fresh && crate::who::same_name(here, name))
}

/// Off unless `LAMP_PHOTO_SPEAKER=on` on lamp-eyes (#2856): `photo_here`
/// names the passing id of the face that matched the photo, and a yes is
/// kept only with that id ([`is_that_face`]). Off, both answer as before.
pub const SPEAKER_ENVIRONMENT: &str = "LAMP_PHOTO_SPEAKER";

pub fn speaker_from_environment() -> bool {
    std::env::var(SPEAKER_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Whether a yes's `face` is the one that matched the photo here (`here`,
/// `boot/f3`, `None` while speaking isn't judged): the same id under the
/// same boot, or the yes is not kept. It closes the gap between the voice's
/// `photo_here` and its yes, in which another face could have come.
pub fn is_that_face(here: Option<&str>, face: Option<&str>) -> bool {
    here.is_some() && here == face
}

/// With [`SPEAKER_ENVIRONMENT`] on, a yes that passes is not kept at once:
/// the eyes force one look after it reached them and keep it only on that
/// look (#2856, reviewer-three's P2: a newcomer within 1.5 s of the photo
/// face takes its passing id, `bus_out::Ids::of`). No such look within this
/// long, the yes is `not_that_face`: it bounds the yes's answer at about a
/// second, inside the voice's 6 s (`known_faces::DEADLINE`).
pub const YES_LOOK_WITHIN: std::time::Duration = std::time::Duration::from_millis(1000);

/// Whether a look's near face is the photo's `name` by its print: `matched`
/// is the unconfirmed face its print matched (`Unconfirmeds::here`), `None`
/// for no match or someone who asked. With [`SPEAKER_ENVIRONMENT`] on, a
/// print that isn't the photo's drops `photo_here`, so a passing id taken
/// over by another face is not left naming the photo's.
pub fn print_is_photos(name: &str, matched: Option<&str>) -> bool {
    matched.is_some_and(|matched| crate::who::same_name(matched, name))
}

/// The yes's answer from the look forced after it: kept only if that look's
/// near face printed as the photo's `name` and has the yes's passing id
/// (`face_now`, `yes_face`, both `boot/f3`). Anything else, no face or no
/// print among them, is `not_that_face`, and nothing is kept.
pub fn yes_after_look(
    name: &str,
    matched: Option<&str>,
    face_now: Option<&str>,
    yes_face: Option<&str>,
) -> bool {
    print_is_photos(name, matched) && is_that_face(face_now, yes_face)
}

/// With [`SPEAKER_ENVIRONMENT`] on, the one yes that passed and waits for a
/// look forced after it (#2856). It pins what it answers: the unconfirmed
/// record (`Unconfirmeds::record_of`), the camera permission's hold
/// (`permit::Permit::held`) and when it reached the eyes. The loop asks it
/// at every turn ([`Self::checked`]) and at each look ([`Self::looked`]).
pub struct PendingYes<C> {
    name: String,
    face: Option<String>,
    caller: C,
    record: u64,
    hold: u64,
    arrived_us: u64,
}

/// What a waiting yes ends as: kept, with its name and caller, or not, with
/// the caller and the error to answer.
#[derive(Debug, PartialEq)]
pub enum YesEnded<C> {
    Keep(String, C),
    Refuse(C, &'static str),
}

/// A waiting yes's caller, the voice's connection: whether it has closed
/// it, so the yes was withdrawn (Fourth's 17:01 on #2856).
pub trait Caller {
    fn closed(&self) -> bool;
}

impl Caller for std::os::unix::net::UnixStream {
    /// A read that doesn't wait: the connection's end (nothing more will
    /// come) or an error is closed; nothing to read yet, or a byte, is open.
    /// Its one line was read already and only the answer is left to write,
    /// so a stray byte this takes is lost to no one. The stream is left
    /// blocking, as it was accepted.
    fn closed(&self) -> bool {
        if self.set_nonblocking(true).is_err() {
            return true;
        }
        let read = (&mut &*self).read(&mut [0u8]);
        let _ = self.set_nonblocking(false);
        match read {
            Ok(0) => true,
            Ok(_) => false,
            Err(error) => !matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ),
        }
    }
}

impl<C: Caller> YesEnded<C> {
    /// The last fence before a yes is kept: a caller that closed meanwhile
    /// makes it `peer_closed`, and nothing is kept.
    pub fn fenced(self) -> Self {
        match self {
            YesEnded::Keep(_, caller) if caller.closed() => YesEnded::Refuse(caller, "peer_closed"),
            ended => ended,
        }
    }
}

/// A frame's capture time for a waiting yes ([`YesLook::frame_us`]): the
/// driver's time in the shared monotonic clock, or, without it, 0, before
/// any yes, so such a frame never answers one, which then runs out (not
/// dated by its read, which a frame buffered before the yes would pass,
/// #3257).
pub fn frame_us(timestamp: Option<lamp_camera::DriverTimestamp>) -> u64 {
    timestamp
        .filter(|at| at.domain == lamp_camera::TimestampDomain::HostMonotonic)
        .map_or(0, |at| at.micros)
}

/// A look's part in a waiting yes: when its frame was captured, the
/// permission's hold and the record of the yes's name now, and the near
/// face's match and passing id ([`yes_after_look`]).
pub struct YesLook<'a> {
    pub frame_us: u64,
    pub now_us: u64,
    pub hold: Option<u64>,
    pub record: Option<u64>,
    pub matched: Option<&'a str>,
    pub face: Option<&'a str>,
}

impl<C: Caller> PendingYes<C> {
    pub fn new(
        name: String,
        face: Option<String>,
        caller: C,
        record: u64,
        hold: u64,
        arrived_us: u64,
    ) -> Self {
        Self {
            name,
            face,
            caller,
            record,
            hold,
            arrived_us,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every turn: a caller that closed its connection withdrew the yes,
    /// `peer_closed` ([`Caller`]); past [`YES_LOOK_WITHIN`] it is
    /// `not_that_face`; under another hold, or none, the camera was shut
    /// meanwhile and it is `eyes_shut`, for good, though the camera opens
    /// again. Else it waits.
    pub fn checked(self, now_us: u64, hold: Option<u64>) -> Result<YesEnded<C>, Self> {
        if self.caller.closed() {
            Ok(YesEnded::Refuse(self.caller, "peer_closed"))
        } else if now_us >= self.arrived_us + YES_LOOK_WITHIN.as_micros() as u64 {
            Ok(YesEnded::Refuse(self.caller, "not_that_face"))
        } else if hold != Some(self.hold) {
            Ok(YesEnded::Refuse(self.caller, "eyes_shut"))
        } else {
            Err(self)
        }
    }

    /// At a look, as it ends, so nothing between the frame and the keep is
    /// unchecked: first [`Self::checked`]; a frame not captured strictly
    /// after the yes reached the eyes, the same microsecond included, leaves
    /// it waiting; another record under its name (added again, let
    /// go, forgotten or a no) is `not_unconfirmed`; and it is kept only if
    /// the near face is the photo's and the one that said it
    /// ([`yes_after_look`]).
    pub fn looked(self, look: &YesLook) -> Result<YesEnded<C>, Self> {
        let pending = match self.checked(look.now_us, look.hold) {
            Ok(ended) => return Ok(ended),
            Err(pending) => pending,
        };
        if look.frame_us <= pending.arrived_us {
            Err(pending)
        } else if look.record != Some(pending.record) {
            Ok(YesEnded::Refuse(pending.caller, "not_unconfirmed"))
        } else if yes_after_look(
            &pending.name,
            look.matched,
            look.face,
            pending.face.as_deref(),
        ) {
            Ok(YesEnded::Keep(pending.name, pending.caller))
        } else {
            Ok(YesEnded::Refuse(pending.caller, "not_that_face"))
        }
    }
}

/// The eyes' answer to `{"photo_here":true}`: the face from a photo seen at
/// the lamp within `fresh` (`here`: its name and how long ago), and whether
/// that face was asked about, its own mark. A face let go and added again
/// under the same name is a new face, not asked yet (Mobile on #1968).
/// With `speaker` ([`SPEAKER_ENVIRONMENT`]), also that face's passing id
/// (`face`), or `null`.
pub fn here_answer(
    unconfirmed: &Unconfirmeds,
    here: Option<(&str, std::time::Duration)>,
    face: Option<&str>,
    speaker: bool,
    fresh: std::time::Duration,
) -> serde_json::Value {
    let name = here.filter(|(_, ago)| *ago < fresh).map(|(name, _)| name);
    let mut answer = serde_json::json!({
        "photo_here": name,
        "asked": name.map(|name| unconfirmed.was_asked(name)),
    });
    if speaker {
        answer["face"] = serde_json::json!(name.and(face));
    }
    answer
}

/// The asked names whose meeting is over: not seen for [`MEETING_GAP_US`]
/// (`seen` gives each asked name's last sighting), so their person gave no
/// answer and the face is let go.
pub fn unanswered(asked: &[String], seen: &[(String, u64)], now_us: u64) -> Vec<String> {
    asked
        .iter()
        .filter(|name| {
            seen.iter()
                .find(|(seen_name, _)| crate::who::same_name(seen_name, name))
                .is_none_or(|(_, at)| now_us.saturating_sub(*at) >= MEETING_GAP_US)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(height: f32) -> Face {
        Face {
            score: 0.9,
            x: 10.0,
            y: 10.0,
            width: height * 0.8,
            height,
            marks: [(0.0, 0.0); 5],
        }
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eyes-photo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_photo_is_read_once_and_removed_whatever_it_is() {
        let dir = folder("take");
        std::fs::write(dir.join("one.jpg"), b"picture").unwrap();
        assert_eq!(take(&dir, "one.jpg").unwrap(), b"picture");
        assert!(!dir.join("one.jpg").exists());
        // Gone, so a second ask finds nothing.
        assert_eq!(take(&dir, "one.jpg"), Err(NotTaken::Unreadable));
        // Too large: refused, and removed.
        let big = std::fs::File::create(dir.join("big.jpg")).unwrap();
        big.set_len(MOST_BYTES + 1).unwrap();
        assert_eq!(take(&dir, "big.jpg"), Err(NotTaken::Unreadable));
        assert!(!dir.join("big.jpg").exists());
        // A link is not followed: refused, the link removed, what it names kept.
        let outside = folder("take-outside").join("kept.jpg");
        std::fs::write(&outside, b"not ours").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link.jpg")).unwrap();
        assert_eq!(take(&dir, "link.jpg"), Err(NotTaken::Unreadable));
        assert!(std::fs::symlink_metadata(dir.join("link.jpg")).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"not ours");
        // A folder is not a photo, and is left.
        std::fs::create_dir(dir.join("sub")).unwrap();
        assert_eq!(take(&dir, "sub"), Err(NotTaken::Unreadable));
        std::fs::remove_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_photo_folder_is_made_the_eyes_own_or_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = folder("ready");
        let photos = dir.join("photos");
        ready(&photos).unwrap();
        assert_eq!(
            std::fs::metadata(&photos).unwrap().permissions().mode() & 0o777,
            0o700
        );
        ready(&photos).unwrap();
        std::fs::set_permissions(&photos, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ready(&photos).is_err());
        std::fs::write(dir.join("file"), b"").unwrap();
        assert!(ready(&dir.join("file")).is_err());
        std::os::unix::fs::symlink(&dir, dir.join("link")).unwrap();
        assert!(ready(&dir.join("link")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_a_plain_file_name_is_taken() {
        for good in ["photo.jpg", "a-b_c.1.jpeg"] {
            assert!(plain_file(good), "{good}");
        }
        for bad in [
            "",
            "../people.json",
            "a/b.jpg",
            ".hidden",
            "with space.jpg",
            &"x".repeat(129),
        ] {
            assert!(!plain_file(bad), "{bad}");
        }
        let dir = folder("names");
        assert_eq!(take(&dir, "../x.jpg"), Err(NotTaken::Unreadable));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// With the pinned detector and recogniser in `LAMP_EYES_MODELS`: a
    /// picture of no face is `no_face`, bytes that are no JPEG `unreadable`,
    /// and with `LAMP_FACE_PHOTO` (a JPEG of one face, never committed) a
    /// print of length one.
    #[test]
    fn a_photo_is_printed_with_the_pinned_models_when_present() {
        let Some(models) = std::env::var_os("LAMP_EYES_MODELS") else {
            return;
        };
        let models = std::path::PathBuf::from(models);
        let detector =
            crate::faces::Detector::load(&models.join(crate::faces::MODEL_FILE)).unwrap();
        let recogniser =
            crate::who::Recogniser::load(&models.join(crate::who::MODEL_FILE)).unwrap();
        let mut grey = Vec::new();
        jpeg_encoder::Encoder::new(&mut grey, 80)
            .encode(
                &[128u8; 320 * 240 * 3],
                320,
                240,
                jpeg_encoder::ColorType::Rgb,
            )
            .unwrap();
        assert_eq!(
            print_of(&grey, &detector, &recogniser),
            Err(NotTaken::NoFace)
        );
        assert_eq!(
            print_of(b"not a picture", &detector, &recogniser),
            Err(NotTaken::Unreadable)
        );
        if let Some(photo) = std::env::var_os("LAMP_FACE_PHOTO") {
            let print = print_of(&std::fs::read(photo).unwrap(), &detector, &recogniser).unwrap();
            let length: f32 = print.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 1e-3);
        }
    }

    fn unit(n: usize) -> Vec<f32> {
        let mut print = vec![0.0f32; crate::who::PRINT];
        print[n] = 1.0;
        print
    }

    /// The first builder on #1968, 22:12: a face from a photo is kept
    /// unconfirmed in its own file, found only as "one is here", taken out
    /// on a yes, deleted on a no, after a meeting with no answer, or after
    /// 30 days, and never written into the people's file.
    #[test]
    fn a_face_from_a_photo_is_unconfirmed_until_its_person_says_yes() {
        let dir = folder("unconfirmed");
        let people = dir.join("people.json");
        let path = Unconfirmeds::beside(&people);
        assert!(path.ends_with("people.json.from-photos"));
        let mut faces = Unconfirmeds::open(path.clone());
        assert!(faces.is_empty());
        faces.add("Lan", unit(0), 1_000).unwrap();
        faces.add("Minh", unit(1), 1_000).unwrap();
        // Kept in its own file, 0600, never the people's.
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!people.exists());
        // Found as the face here, by likeness, and nobody else's.
        assert_eq!(faces.here(&unit(0)).as_deref(), Some("Lan"));
        assert_eq!(faces.here(&unit(2)), None);
        // Read back as written.
        let mut faces = Unconfirmeds::open(path.clone());
        assert_eq!(faces.len(), 2);
        // Named, for the owner's page only (`face.list`, #2192).
        assert_eq!(faces.names(), ["Lan", "Minh"]);
        // Yes: taken out, to be kept as an ordinary print.
        let taken = faces.confirmed("lan").unwrap().unwrap();
        assert_eq!((taken.name(), taken.print()), ("Lan", unit(0)));
        assert_eq!(faces.here(&unit(0)), None);
        // A yes the people's store refused: unconfirmed again, as it was.
        faces.put_back(taken).unwrap();
        assert_eq!(faces.here(&unit(0)).as_deref(), Some("Lan"));
        faces.confirmed("Lan").unwrap().unwrap();
        // No: deleted.
        assert!(faces.let_go("Minh").unwrap());
        assert!(faces.is_empty());
        assert!(!faces.let_go("Minh").unwrap());
        assert!(faces.confirmed("Minh").unwrap().is_none());
        // Asked, and the meeting over with no answer: let go.
        faces.add("Hoa", unit(3), 1_000).unwrap();
        assert!(!faces.was_asked("Hoa"));
        assert!(faces.asked("Hoa").unwrap());
        assert!(faces.was_asked("hoa"));
        assert!(!faces.asked("Nobody").unwrap());
        assert_eq!(faces.asked_names(), ["Hoa"]);
        // What the voice is told: the face here, and whether it was asked.
        let fresh = std::time::Duration::from_secs(5);
        let near = Some(("Hoa", std::time::Duration::from_secs(1)));
        assert_eq!(
            here_answer(&faces, near, None, false, fresh),
            serde_json::json!({"photo_here": "Hoa", "asked": true})
        );
        assert_eq!(
            here_answer(&faces, Some(("Hoa", fresh)), None, false, fresh),
            serde_json::json!({"photo_here": null, "asked": null})
        );
        // Let go and added again under the name: a new face, not asked.
        assert!(faces.let_go("Hoa").unwrap());
        faces.add("Hoa", unit(3), 1_000).unwrap();
        assert_eq!(
            here_answer(&faces, near, None, false, fresh),
            serde_json::json!({"photo_here": "Hoa", "asked": false})
        );
        assert!(faces.asked("Hoa").unwrap());
        let seen = [("Hoa".to_owned(), 5_000_000u64)];
        assert!(unanswered(&faces.asked_names(), &seen, 5_000_000 + MEETING_GAP_US - 1).is_empty());
        assert_eq!(
            unanswered(&faces.asked_names(), &seen, 5_000_000 + MEETING_GAP_US),
            ["Hoa"]
        );
        assert_eq!(unanswered(&faces.asked_names(), &[], 0), ["Hoa"]);
        // Thirty days after it was added, never confirmed: deleted.
        assert_eq!(faces.expire(1_000 + UNCONFIRMED_FOR_S - 1).unwrap(), 0);
        assert_eq!(faces.expire(1_000 + UNCONFIRMED_FOR_S).unwrap(), 1);
        assert!(Unconfirmeds::open(path.clone()).is_empty());
        // A file that can't be read is removed, not kept out of reach.
        std::fs::write(&path, b"not json").unwrap();
        assert!(Unconfirmeds::open(path.clone()).is_empty());
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn mixed(parts: &[(usize, f32)]) -> Vec<f32> {
        let mut print = vec![0.0f32; crate::who::PRINT];
        for (n, share) in parts {
            print[*n] = *share;
        }
        let length = print.iter().map(|v| v * v).sum::<f32>().sqrt();
        print.iter().map(|v| v / length).collect()
    }

    /// #2854: the prints the lamp took of an unconfirmed face are kept in
    /// memory only, the newest four, and go with its photo's on a yes, or
    /// with it on a no, a new photo under the name or its 30 days.
    #[test]
    fn the_lamps_own_prints_of_a_face_from_a_photo_go_with_it() {
        let dir = folder("seen");
        let path = Unconfirmeds::beside(&dir.join("people.json"));
        let mut faces = Unconfirmeds::open(path.clone());
        faces.add("Lan", unit(0), 1_000).unwrap();
        // Nothing kept for a face that isn't one of these.
        faces.saw("Minh", unit(9));
        for n in 1..=6 {
            faces.saw("Lan", unit(n));
        }
        // Never written: read back, the photo's print alone.
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(!file.contains("seen"));
        let mut again = Unconfirmeds::open(path.clone());
        assert_eq!(again.confirmed("Lan").unwrap().unwrap().prints(), [unit(0)]);
        // The yes: the photo's, then the newest four as the lamp saw them.
        let mut faces = Unconfirmeds::open(path.clone());
        faces.add("Lan", unit(0), 1_000).unwrap();
        for n in 1..=6 {
            faces.saw("lan", unit(n));
        }
        let taken = faces.confirmed("Lan").unwrap().unwrap();
        assert_eq!(
            taken.prints(),
            [unit(0), unit(3), unit(4), unit(5), unit(6)]
        );
        assert_eq!(taken.prints().len(), crate::who::MOST_PRINTS);
        // Refused by the people's store: put back with them.
        faces.put_back(taken).unwrap();
        assert_eq!(faces.confirmed("Lan").unwrap().unwrap().prints().len(), 5);
        // A no, a new photo under the name, or 30 days: they go too.
        for end in ["no", "new photo", "expired"] {
            faces.add("Lan", unit(0), 1_000).unwrap();
            faces.saw("Lan", unit(7));
            match end {
                "no" => assert!(faces.let_go("Lan").unwrap()),
                "new photo" => {}
                _ => assert_eq!(faces.expire(1_000 + UNCONFIRMED_FOR_S).unwrap(), 1),
            }
            // Gone at once on a no or the 30 days, not only at the next add.
            assert_eq!(faces.seen.is_empty(), end != "new photo", "{end}");
            faces.add("Lan", unit(0), 1_000).unwrap();
            assert_eq!(
                faces.confirmed("Lan").unwrap().unwrap().prints(),
                [unit(0)],
                "{end}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #2854, lamp-0c89 on 21 Oct: a face from a photo was named only near
    /// the middle of the view. The selfie is one view; at the side the face
    /// is turned and seen through the lens's edge, unlike it, but like the
    /// lamp's own views of the same face. Kept with those on the yes, it is
    /// named there too; with the photo's print alone, it isn't.
    #[test]
    fn a_face_from_a_photo_is_named_at_the_side_as_at_the_middle() {
        let selfie = unit(0);
        // At the middle: like the selfie, so found as the face here.
        let middle = mixed(&[(0, 0.8), (1, 0.6)]);
        // At the side: unlike the selfie, like the lamp's middle view.
        let side = mixed(&[(0, 0.2), (1, 0.6), (2, 0.77)]);
        assert!(crate::who::likeness(&selfie, &middle) >= crate::who::SAME_PERSON);
        assert!(crate::who::likeness(&selfie, &side) < crate::who::SAME_PERSON);
        // Before #2854 nothing the lamp saw was kept: as if it saw nothing.
        for (seen_kept, named_at_side) in [(false, false), (true, true)] {
            let dir = folder(if seen_kept { "side-seen" } else { "side-photo" });
            let mut people = crate::who::People::open(dir.join("people.json")).unwrap();
            let mut faces = Unconfirmeds::open(Unconfirmeds::beside(&dir.join("people.json")));
            faces.add("Lan", selfie.clone(), 1_000).unwrap();
            let here = faces.here(&middle).unwrap();
            if seen_kept {
                faces.saw(&here, middle.clone());
            }
            let taken = faces.confirmed("Lan").unwrap().unwrap();
            people
                .remember_one(taken.name(), taken.prints(), 1_000, false, true)
                .unwrap();
            assert_eq!(
                people.who(&middle).map(|(name, _)| name).as_deref(),
                Some("Lan")
            );
            assert_eq!(
                people.who(&side).map(|(name, _)| name).as_deref(),
                named_at_side.then_some("Lan"),
                "kept with the lamp's prints: {seen_kept}"
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// An answer is taken only for the face at the lamp now.
    #[test]
    fn an_answer_is_taken_only_while_that_face_is_here() {
        let fresh = std::time::Duration::from_secs(5);
        let ago = std::time::Duration::from_secs;
        assert!(is_here(Some(("Lan", ago(1))), "lan", fresh));
        assert!(!is_here(Some(("Lan", ago(5))), "Lan", fresh));
        assert!(!is_here(Some(("Minh", ago(1))), "Lan", fresh));
        assert!(!is_here(None, "Lan", fresh));
    }

    /// With `LAMP_PHOTO_SPEAKER=on` (#2856) a yes is kept only with the
    /// passing id, under its boot, of the face that matched the photo.
    #[test]
    fn a_yes_is_kept_only_from_the_face_that_matched_the_photo() {
        assert!(is_that_face(Some("b1/f3"), Some("b1/f3")));
        // Another face, the same id from an older boot, none, or none judged.
        assert!(!is_that_face(Some("b1/f3"), Some("b1/f4")));
        assert!(!is_that_face(Some("b1/f3"), Some("b0/f3")));
        assert!(!is_that_face(Some("b1/f3"), None));
        assert!(!is_that_face(None, None));
        assert!(!is_that_face(None, Some("b1/f3")));
    }

    /// Reviewer-three's P2 on #2856: another face leaning into the photo
    /// face's place within 1.5 s takes its passing id, so the yes passes
    /// `is_that_face`; the look forced after it is kept only on the
    /// photo's print.
    #[test]
    fn a_yes_from_a_face_that_took_the_photo_faces_id_is_not_kept() {
        // The same id, a print that isn't the photo's: not kept.
        assert!(!yes_after_look("Lan", None, Some("b1/f1"), Some("b1/f1")));
        assert!(!yes_after_look(
            "Lan",
            Some("Hoa"),
            Some("b1/f1"),
            Some("b1/f1")
        ));
        // Nobody near, or no id judged, at that look: not kept.
        assert!(!yes_after_look("Lan", None, None, Some("b1/f1")));
        assert!(!yes_after_look("Lan", Some("Lan"), None, Some("b1/f1")));
        // The photo's print, but another face's id: not kept.
        assert!(!yes_after_look(
            "Lan",
            Some("Lan"),
            Some("b1/f2"),
            Some("b1/f1")
        ));
        // The photo's print on the face that said yes: kept.
        assert!(yes_after_look(
            "Lan",
            Some("lan"),
            Some("b1/f1"),
            Some("b1/f1")
        ));
    }

    /// The waiting yes's caller, a number here, that never closes; its
    /// arrival at 1 s.
    const ARRIVED_US: u64 = 1_000_000;

    impl Caller for u32 {
        fn closed(&self) -> bool {
            false
        }
    }

    fn yes_on(
        caller: std::os::unix::net::UnixStream,
    ) -> PendingYes<std::os::unix::net::UnixStream> {
        PendingYes::new(
            "Mirel".into(),
            Some("b1/f1".into()),
            caller,
            1,
            3,
            ARRIVED_US,
        )
    }

    fn why<C>(ended: Result<YesEnded<C>, PendingYes<C>>) -> Option<&'static str> {
        match ended {
            Ok(YesEnded::Refuse(_, why)) => Some(why),
            Ok(YesEnded::Keep(..)) => Some("kept"),
            Err(_) => None,
        }
    }

    /// Fourth's barrier on #2856 (17:01): the yes is accepted, the voice
    /// closes its connection before the forced look, then a matching frame
    /// comes: nothing is kept. The same at a turn without a look, and at
    /// the last fence before the keep. An open caller still keeps.
    #[test]
    fn a_yes_whose_voice_closed_its_connection_keeps_nothing() {
        use std::os::unix::net::UnixStream;
        let (ours, voice) = UnixStream::pair().unwrap();
        let waiting = yes_on(ours);
        drop(voice);
        assert_eq!(
            why(waiting.looked(&look(ARRIVED_US + 60_000, ARRIVED_US + 120_000))),
            Some("peer_closed")
        );

        let (ours, voice) = UnixStream::pair().unwrap();
        let waiting = yes_on(ours);
        let Err(waiting) = waiting.checked(ARRIVED_US + 60_000, Some(3)) else {
            panic!("an open caller's yes ended");
        };
        drop(voice);
        assert_eq!(
            why(waiting.checked(ARRIVED_US + 60_000, Some(3))),
            Some("peer_closed")
        );

        // Kept by the look, closed before the fence: refused there.
        let (ours, voice) = UnixStream::pair().unwrap();
        let ended = yes_on(ours).looked(&look(ARRIVED_US + 60_000, ARRIVED_US + 120_000));
        let Ok(ended @ YesEnded::Keep(..)) = ended else {
            panic!("an open caller's matching look was not kept");
        };
        drop(voice);
        assert!(matches!(ended.fenced(), YesEnded::Refuse(_, "peer_closed")));

        // Open to the end: kept.
        let (ours, _voice) = UnixStream::pair().unwrap();
        let ended = yes_on(ours).looked(&look(ARRIVED_US + 60_000, ARRIVED_US + 120_000));
        assert!(matches!(
            ended.ok().map(YesEnded::fenced),
            Some(YesEnded::Keep(..))
        ));
    }

    fn pending(record: u64) -> PendingYes<u32> {
        PendingYes::new(
            "Mirel".into(),
            Some("b1/f1".into()),
            7,
            record,
            3,
            ARRIVED_US,
        )
    }

    /// A look at `frame_us`, ending at `now_us`, under hold 3 with record 1,
    /// whose near face is the photo's and the one that said yes.
    fn look(frame_us: u64, now_us: u64) -> YesLook<'static> {
        YesLook {
            frame_us,
            now_us,
            hold: Some(3),
            record: Some(1),
            matched: Some("Mirel"),
            face: Some("b1/f1"),
        }
    }

    /// Fourth's first P2 on #2856: a frame buffered before the yes, though
    /// it matches, leaves it waiting; no newer frame, and its deadline ends
    /// it; a newer frame that isn't the photo's ends it too.
    #[test]
    fn a_yes_is_looked_at_only_on_a_frame_captured_after_it() {
        let Err(waiting) = pending(1).looked(&look(ARRIVED_US - 40_000, ARRIVED_US + 50_000))
        else {
            panic!("a frame from before the yes was looked at");
        };
        let Err(waiting) = waiting.checked(ARRIVED_US + 999_999, Some(3)) else {
            panic!("ended before its deadline");
        };
        assert_eq!(
            waiting.checked(ARRIVED_US + 1_000_000, Some(3)).ok(),
            Some(YesEnded::Refuse(7, "not_that_face"))
        );
        let Err(waiting) = pending(1).looked(&look(ARRIVED_US - 40_000, ARRIVED_US + 50_000))
        else {
            panic!("a frame from before the yes was looked at");
        };
        let other = YesLook {
            matched: None,
            ..look(ARRIVED_US + 60_000, ARRIVED_US + 120_000)
        };
        assert_eq!(
            waiting.looked(&other).ok(),
            Some(YesEnded::Refuse(7, "not_that_face"))
        );
        // Strictly after it (Fourth's P3 on #3257): a frame of the yes's
        // own microsecond leaves it waiting; one a microsecond later, the
        // photo's face that said it, is kept.
        let Err(waiting) = pending(1).looked(&look(ARRIVED_US, ARRIVED_US + 120_000)) else {
            panic!("a frame of the yes's own microsecond was looked at");
        };
        assert_eq!(
            waiting
                .looked(&look(ARRIVED_US + 1, ARRIVED_US + 120_000))
                .ok(),
            Some(YesEnded::Keep("Mirel".into(), 7))
        );
    }

    /// Fourth's P3 on #3257: the conversion the loop uses. A frame without a
    /// timestamp, or with one in another clock, is dated 0 and never
    /// answers a yes; a known monotonic time is kept as it is, so an old
    /// one leaves the yes waiting and a new one answers it.
    #[test]
    fn a_frame_is_dated_only_by_the_drivers_monotonic_time() {
        use lamp_camera::{DriverTimestamp, TimestampDomain, TimestampPoint};
        let at = |micros, domain| {
            Some(DriverTimestamp {
                micros,
                domain,
                point: TimestampPoint::EndOfFrame,
            })
        };
        assert_eq!(frame_us(None), 0);
        assert_eq!(
            frame_us(at(ARRIVED_US + 60_000, TimestampDomain::Unknown)),
            0
        );
        assert_eq!(
            frame_us(at(ARRIVED_US + 60_000, TimestampDomain::Realtime)),
            0
        );
        let old = frame_us(at(ARRIVED_US - 40_000, TimestampDomain::HostMonotonic));
        let new = frame_us(at(ARRIVED_US + 60_000, TimestampDomain::HostMonotonic));
        assert_eq!((old, new), (ARRIVED_US - 40_000, ARRIVED_US + 60_000));
        for undated in [
            frame_us(None),
            frame_us(at(ARRIVED_US + 60_000, TimestampDomain::Unknown)),
            old,
        ] {
            assert!(
                pending(1)
                    .looked(&look(undated, ARRIVED_US + 120_000))
                    .is_err()
            );
        }
        assert_eq!(
            pending(1).looked(&look(new, ARRIVED_US + 120_000)).ok(),
            Some(YesEnded::Keep("Mirel".into(), 7))
        );
    }

    /// Fourth's second P2 on #2856: the deadline and the camera's hold are
    /// checked as it is kept. A look that ends late, a camera shut during it,
    /// or shut and open again, keeps nothing; shut between looks, it ends at
    /// the next turn, and a hold given again doesn't bring it back.
    #[test]
    fn a_yes_ends_past_its_deadline_or_once_the_camera_is_shut() {
        assert_eq!(
            pending(1)
                .looked(&look(ARRIVED_US + 900_000, ARRIVED_US + 1_000_000))
                .ok(),
            Some(YesEnded::Refuse(7, "not_that_face"))
        );
        for hold in [None, Some(4)] {
            let shut = YesLook {
                hold,
                ..look(ARRIVED_US + 60_000, ARRIVED_US + 120_000)
            };
            assert_eq!(
                pending(1).looked(&shut).ok(),
                Some(YesEnded::Refuse(7, "eyes_shut"))
            );
            assert_eq!(
                pending(1).checked(ARRIVED_US + 60_000, hold).ok(),
                Some(YesEnded::Refuse(7, "eyes_shut"))
            );
        }
        assert!(pending(1).checked(ARRIVED_US + 60_000, Some(3)).is_err());
    }

    /// Fourth's third P2 on #2856: a face added again under the name before
    /// the forced look, let go or forgotten, is another record (or none),
    /// and the yes waiting for the last keeps nothing.
    #[test]
    fn a_yes_for_a_face_added_again_under_its_name_is_not_kept() {
        let dir = folder("pending-yes");
        let mut faces = Unconfirmeds::open(dir.join("people.json.from-photos"));
        faces.add("Mirel", unit(3), 1_000).unwrap();
        assert!(faces.asked("Mirel").unwrap());
        let record = faces.record_of("Mirel").unwrap();
        let later = |faces: &Unconfirmeds| YesLook {
            record: faces.record_of("Mirel"),
            ..look(ARRIVED_US + 60_000, ARRIVED_US + 120_000)
        };
        assert_eq!(
            pending(record).looked(&later(&faces)).ok(),
            Some(YesEnded::Keep("Mirel".into(), 7))
        );
        // The same name, from another photo, in the same batch.
        faces.add("mirel", unit(4), 1_001).unwrap();
        assert_ne!(faces.record_of("Mirel"), Some(record));
        assert_eq!(
            pending(record).looked(&later(&faces)).ok(),
            Some(YesEnded::Refuse(7, "not_unconfirmed"))
        );
        // Let go (a no, or forgotten): no record.
        assert!(faces.let_go("Mirel").unwrap());
        assert_eq!(
            pending(record).looked(&later(&faces)).ok(),
            Some(YesEnded::Refuse(7, "not_unconfirmed"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A look whose near print isn't the photo's drops `photo_here`.
    #[test]
    fn a_print_that_is_not_the_photos_drops_it_from_here() {
        assert!(print_is_photos("Lan", Some("Lan")));
        assert!(!print_is_photos("Lan", None));
        assert!(!print_is_photos("Lan", Some("Hoa")));
    }

    /// `photo_here` names that face with the switch on, and is today's
    /// answer, without `face`, with it off.
    #[test]
    fn the_face_here_is_named_only_with_the_switch_on() {
        let dir = folder("speaker");
        let path = dir.join("people.json.from-photos");
        let mut faces = Unconfirmeds::open(path);
        faces.add("Hoa", unit(3), 1_000).unwrap();
        let fresh = std::time::Duration::from_secs(5);
        let near = Some(("Hoa", std::time::Duration::from_secs(1)));
        assert_eq!(
            here_answer(&faces, near, Some("b1/f3"), false, fresh),
            serde_json::json!({"photo_here": "Hoa", "asked": false})
        );
        assert_eq!(
            here_answer(&faces, near, Some("b1/f3"), true, fresh),
            serde_json::json!({"photo_here": "Hoa", "asked": false, "face": "b1/f3"})
        );
        // Speaking not judged: no face to name.
        assert_eq!(
            here_answer(&faces, near, None, true, fresh),
            serde_json::json!({"photo_here": "Hoa", "asked": false, "face": null})
        );
        // Nobody fresh here: no face either.
        assert_eq!(
            here_answer(&faces, Some(("Hoa", fresh)), Some("b1/f3"), true, fresh),
            serde_json::json!({"photo_here": null, "asked": null, "face": null})
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_photos_one_face_is_taken_or_the_reason_said() {
        let tall = 256.0;
        assert_eq!(one_face(&[], tall, 0.10), Err(NotTaken::NoFace));
        assert_eq!(one_face(&[face(60.0)], tall, 0.10).unwrap().height, 60.0);
        // Two people: a second face half the first's height or more.
        assert_eq!(
            one_face(&[face(60.0), face(30.0)], tall, 0.10),
            Err(NotTaken::TwoFaces)
        );
        // A face far behind them is no second person.
        assert_eq!(
            one_face(&[face(29.0), face(60.0)], tall, 0.10)
                .unwrap()
                .height,
            60.0
        );
        // Too small to print well.
        assert_eq!(one_face(&[face(20.0)], tall, 0.10), Err(NotTaken::TooFar));
        assert_eq!(NotTaken::TwoFaces.code(), "two_faces");
    }
}
