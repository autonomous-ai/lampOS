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

/// The photo folder, made the eyes' own (0700) if it isn't there; a
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

/// One face added from a photo, not yet agreed to by its person.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Unconfirmed {
    name: String,
    print: Vec<f32>,
    added_unix_s: u64,
    /// The voice has asked them, in this meeting.
    #[serde(default)]
    asked: bool,
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
        Self { path, kept }
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
        next.push(Unconfirmed {
            name: name.to_owned(),
            print,
            added_unix_s: now_unix_s,
            asked: false,
        });
        let before = std::mem::replace(&mut self.kept, next);
        self.save().inspect_err(|_| self.kept = before)
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
        self.save()?;
        Ok(Some(Taken(kept)))
    }

    /// A yes the people's store refused (full, or not written): the face is
    /// unconfirmed again as it was, its 30 days unchanged.
    pub fn put_back(&mut self, taken: Taken) -> io::Result<()> {
        self.kept
            .retain(|kept| !crate::who::same_name(&kept.name, &taken.0.name));
        self.kept.push(taken.0);
        self.save()
    }

    /// Their no, no answer, or "forget my face": the print deleted.
    pub fn let_go(&mut self, name: &str) -> io::Result<bool> {
        let Some(at) = self.find(name) else {
            return Ok(false);
        };
        self.kept.remove(at);
        self.save().map(|()| true)
    }

    /// Deletes those added [`UNCONFIRMED_FOR_S`] ago or more; how many.
    pub fn expire(&mut self, now_unix_s: u64) -> io::Result<usize> {
        let before = self.kept.len();
        self.kept
            .retain(|kept| now_unix_s.saturating_sub(kept.added_unix_s) < UNCONFIRMED_FOR_S);
        let gone = before - self.kept.len();
        if gone > 0 {
            self.save()?;
        }
        Ok(gone)
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

/// A face taken out on its person's yes: its name and print, and what to put
/// back if the people's store refuses it.
pub struct Taken(Unconfirmed);

impl Taken {
    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn print(&self) -> Vec<f32> {
        self.0.print.clone()
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
