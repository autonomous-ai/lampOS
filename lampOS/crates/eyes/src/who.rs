//! Who a face is, for the people who asked the lamp to remember them.
//!
//! The face the detector found is cut out of the picture upright, from its
//! five marks, at the size OpenCV's SFace recogniser takes ([`align`]). The
//! recogniser turns it into 128 numbers, a print ([`Recogniser`]). Two
//! prints of one person point the same way: their cosine is at least
//! [`SAME_PERSON`]. [`People`] keeps the prints of those who said "remember
//! my face" in one private file on the lamp, and nobody else's: until
//! someone asks, nobody is recognised. No picture is kept, and no print
//! leaves the lamp.
use crate::picture::{Frame, HEIGHT, Picture, WIDTH};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The exact model file, from OpenCV's model zoo (Apache-2.0). Fetched
/// separately and never committed here.
pub const MODEL_FILE: &str = "face_recognition_sface_2021dec.onnx";
pub const MODEL_BYTES: u64 = 38_696_353;
pub const MODEL_SHA256: &str = "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79";
/// The recogniser's input, square.
pub const SIDE: usize = 112;
/// How many numbers a print is.
pub const PRINT: usize = 128;
/// At or over this cosine, two prints are one person: OpenCV's own
/// threshold for this model.
pub const SAME_PERSON: f32 = 0.363;
/// At or over this cosine, a face is near enough to a known person that a
/// name within one letter of theirs is taken for a mishearing and asked
/// about (#2176 row 4). On lamp-4ace on 20 Oct a person known since 16 Oct
/// was saved again under a name one letter off theirs: the new prints came
/// to at most 0.359 against their own, 0.004 under [`SAME_PERSON`].
pub const NEAR_PERSON: f32 = 0.30;
/// People kept, and prints kept of each.
pub const MOST_PEOPLE: usize = 20;
pub const MOST_PRINTS: usize = 5;
/// The longest name kept, in characters.
const MOST_NAME_CHARS: usize = 40;
/// The kept file is never read past this.
const MOST_FILE_BYTES: u64 = 256 * 1024;
/// Beside the people file: where records it could not read are kept, never
/// dropped (#1505). The folder is 0700, each file 0600, and the product
/// reset empties it, since these are face prints.
pub const SET_ASIDE: &str = "set-aside";

/// Beside `--known-file`, while speaking is judged: the passing id
/// (`humans/faces/tracked`'s `f3`) of the known person's face, one line, and
/// no file whenever there is no name. Only an id that lasts while the face
/// stays in view: no picture, print or name.
pub fn known_face_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-face");
    PathBuf::from(beside)
}

/// One observation tying the known person's name and optional memory id to
/// the eyes publisher's boot and the passing face id from that same look.
pub fn known_speaker_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-speaker");
    PathBuf::from(beside)
}

/// Legacy positive observation only: it cannot establish typed continuity.
/// Runtime uses one `KnownSpeakerPublisher` owner instead.
/// Publish one whole source observation, never fields paired across files.
/// Missing or invalid source data removes any older observation. This is
/// visual speaking evidence, not proof of a live person or their identity.
pub fn keep_known_speaker(
    known: &Path,
    name: Option<&str>,
    id: Option<&str>,
    boot: Option<&str>,
    face: Option<&str>,
) {
    if valid_speaker_fields(name, id, boot, face) {
        let _ = write_speaker(
            known,
            &serde_json::json!({"name":name, "id":id, "boot":boot, "face":face}),
        );
    } else {
        remove_speaker(known);
    }
}

/// One non-cloneable lifecycle owns typed disclosure observations. The caller
/// checks private runtime storage before publishing; legacy writers cannot
/// manufacture this publisher's nonce or continue a typed grant through absence.
pub struct KnownSpeakerPublisher {
    known: PathBuf,
    publisher: Option<String>,
    ticket_required: std::cell::Cell<bool>,
    camera_ticket: std::cell::Cell<Option<crate::camera_authority::Binding>>,
    enabled: std::cell::Cell<bool>,
    generation: std::cell::Cell<u64>,
    state: std::cell::Cell<SpeakerState>,
    boot: std::cell::RefCell<Option<String>>,
    subject: std::cell::RefCell<Option<(String, Option<String>)>>,
    /// Retained across failed recognition so the same unresolved candidate
    /// does not force another inference on every raw frame.
    observed_face: std::cell::RefCell<Option<String>>,
    permission_until_us: std::cell::Cell<Option<u64>>,
    published_until_us: std::cell::Cell<Option<u64>>,
    observed_us: std::cell::Cell<Option<u64>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeakerState {
    Seen,
    Absent,
    Checking,
    Revoked,
}

impl KnownSpeakerPublisher {
    pub fn new(known: PathBuf) -> Self {
        Self::with_random(known, |bytes| getrandom::fill(bytes).map_err(|_| ()))
    }

    fn with_random(
        known: PathBuf,
        fill: impl FnOnce(&mut [u8]) -> std::result::Result<(), ()>,
    ) -> Self {
        remove_speaker(&known);
        let mut bytes = [0; 16];
        let publisher = fill(&mut bytes)
            .ok()
            .map(|()| bytes.iter().map(|byte| format!("{byte:02x}")).collect());
        Self {
            known,
            enabled: std::cell::Cell::new(publisher.is_some()),
            publisher,
            ticket_required: std::cell::Cell::new(false),
            camera_ticket: std::cell::Cell::new(None),
            generation: std::cell::Cell::new(1),
            state: std::cell::Cell::new(SpeakerState::Revoked),
            boot: std::cell::RefCell::new(None),
            subject: std::cell::RefCell::new(None),
            observed_face: std::cell::RefCell::new(None),
            permission_until_us: std::cell::Cell::new(None),
            published_until_us: std::cell::Cell::new(None),
            observed_us: std::cell::Cell::new(None),
        }
    }

    /// Runtime source requires V3 even when the server is unavailable. The
    /// legacy constructor remains for existing standalone protocol fixtures;
    /// runtime must call this before any source publication, with no fallback.
    pub fn require_camera_ticket(&self) {
        self.ticket_required.set(true);
        self.camera_ticket.set(None);
        self.revoke();
    }

    pub fn publisher_id(&self) -> Option<[u8; 16]> {
        let text = self.publisher.as_deref()?;
        let mut bytes = [0; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
        }
        (bytes != [0; 16]).then_some(bytes)
    }

    /// Captured-frame binding, never a recognition-generation substitute.
    pub fn camera_ticket(&self, binding: Option<crate::camera_authority::Binding>) {
        let previous = self.camera_ticket.replace(binding);
        if previous.map(|b| (b.publisher, b.hold)) != binding.map(|b| (b.publisher, b.hold)) {
            self.camera_changed();
        }
    }

    /// Supplied only inside the captured-frame permission lock.
    pub fn permission_until(&self, until_us: Option<u64>) {
        self.permission_until_us.set(until_us);
    }

    /// A successful same-track raw look may renew camera authority, never
    /// identity age. False asks for a new recognition instead of reviving an
    /// expired publication. Skipped/error/changed-track looks never call this.
    pub fn refresh_authority(&self, now_us: u64) -> bool {
        if self.state.get() != SpeakerState::Seen {
            return true;
        }
        let fresh = self
            .observed_us
            .get()
            .is_some_and(|at| at > 0 && at <= now_us && now_us - at < 3_000_000)
            && self
                .published_until_us
                .get()
                .is_some_and(|until| now_us < until)
            && self
                .permission_until_us
                .get()
                .is_some_and(|until| now_us < until);
        if !fresh {
            self.revoke();
            return false;
        }
        // Release RefCell guards before write: publication failure disables
        // the source and clears these same cells.
        let boot = self.boot.borrow().clone();
        let face = self.observed_face.borrow().clone();
        let subject = self.subject.borrow().clone();
        if let (Some(boot), Some(face), Some((name, id))) =
            (boot.as_deref(), face.as_deref(), subject.as_ref())
        {
            self.write(boot, "seen", Some((name, id.as_deref(), face)));
        }
        self.enabled.get()
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn revoked(&self) -> bool {
        self.state.get() == SpeakerState::Revoked
    }

    pub fn disable(&self) {
        self.enabled.set(false);
        self.generation.set(0);
        self.state.set(SpeakerState::Revoked);
        self.subject.borrow_mut().take();
        self.observed_face.borrow_mut().take();
        remove_speaker(&self.known);
    }

    /// Invalidate before slow camera/store work. Repeated revoked-loop checks
    /// cannot admit anything and need no new generation or status record.
    pub fn revoke(&self) {
        if !self.enabled.get() {
            return;
        }
        if self.revoked() {
            // Also clear any foreign legacy write without advancing an epoch
            // in which no grant could have been admitted.
            remove_speaker(&self.known);
            return;
        }
        let Some(next) = self.generation.get().checked_add(1) else {
            self.disable();
            return;
        };
        self.generation.set(next);
        self.state.set(SpeakerState::Revoked);
        self.subject.borrow_mut().take();
        let boot = self.boot.borrow().clone();
        match boot {
            Some(boot) => {
                self.write(&boot, "revoked", None);
            }
            None => remove_speaker(&self.known),
        }
    }

    /// A different camera hold needs a fresh recognition even when the
    /// geometric track ID survives. Ordinary failed recognition keeps its
    /// cadence; a permission boundary does not keep that candidate cache.
    pub fn camera_changed(&self) {
        self.observed_face.borrow_mut().take();
        self.revoke();
    }

    /// Withhold identity before recognizing a returning or changed track.
    /// Checking grants nothing; only a completed same-person match can keep
    /// the epoch. True asks the caller to recognize now, without its cadence.
    pub fn candidate(&self, detected: bool, boot: Option<&str>, face: Option<&str>) -> bool {
        let now = lamp_ipc::monotonic_us();
        self.candidate_at(detected, boot, face, now, now)
    }

    fn candidate_at(
        &self,
        detected: bool,
        boot: Option<&str>,
        face: Option<&str>,
        frame_us: u64,
        now_us: u64,
    ) -> bool {
        if !detected || !self.enabled.get() {
            return false;
        }
        if !fresh_speaker_frame(frame_us, now_us) {
            self.revoke();
            return false;
        }
        let Some((boot, face)) = boot
            .zip(face)
            .filter(|(boot, face)| valid_boot(boot) && valid_passing_face(face))
        else {
            self.observed_face.borrow_mut().take();
            self.revoke();
            return false;
        };
        let changed_boot = self.boot.borrow().as_deref().is_some_and(|was| was != boot);
        if changed_boot {
            self.revoke();
        }
        let needs_recognition = changed_boot
            || self.state.get() == SpeakerState::Absent
            || self.observed_face.borrow().as_deref() != Some(face);
        if needs_recognition {
            *self.boot.borrow_mut() = Some(boot.to_owned());
            *self.observed_face.borrow_mut() = Some(face.to_owned());
            self.state.set(SpeakerState::Checking);
            self.observed_us.set(Some(frame_us));
            self.write(boot, "checking", None);
        }
        needs_recognition && self.enabled.get()
    }

    pub fn seen(&self, name: &str, id: Option<&str>, boot: &str, face: &str) {
        self.seen_at(name, id, boot, face, lamp_ipc::monotonic_us());
    }

    /// Completed recognition retains the original camera capture age, never
    /// the time a slow inference returned. Caller holds the frame permit lock.
    pub fn seen_frame(&self, name: &str, id: Option<&str>, boot: &str, face: &str, frame_us: u64) {
        self.seen_frame_at(name, id, boot, face, (frame_us, lamp_ipc::monotonic_us()));
    }

    fn seen_frame_at(
        &self,
        name: &str,
        id: Option<&str>,
        boot: &str,
        face: &str,
        (frame_us, now): (u64, u64),
    ) {
        if !fresh_speaker_frame(frame_us, now) {
            self.revoke();
            return;
        }
        self.seen_at(name, id, boot, face, frame_us);
    }

    fn seen_at(&self, name: &str, id: Option<&str>, boot: &str, face: &str, now_us: u64) {
        if !valid_speaker_fields(Some(name), id, Some(boot), Some(face)) {
            self.revoke();
            return;
        }
        let subject = (name.to_owned(), id.map(str::to_owned));
        let changed = self
            .subject
            .borrow()
            .as_ref()
            .is_some_and(|was| was != &subject)
            || self.boot.borrow().as_deref().is_some_and(|was| was != boot);
        if changed {
            self.revoke();
        }
        if !self.enabled.get() {
            return;
        }
        *self.boot.borrow_mut() = Some(boot.to_owned());
        *self.subject.borrow_mut() = Some(subject);
        *self.observed_face.borrow_mut() = Some(face.to_owned());
        self.state.set(SpeakerState::Seen);
        self.observed_us.set(Some(now_us));
        self.write(boot, "seen", Some((name, id, face)));
    }

    /// Only the caller's successful raw empty-face look may call this. Missing,
    /// filtered/unaligned/unmatched observations and errors must revoke instead.
    pub fn no_detection(&self, boot: &str) {
        let now = lamp_ipc::monotonic_us();
        self.no_detection_at(boot, now, now);
    }

    fn no_detection_at(&self, boot: &str, frame_us: u64, now_us: u64) {
        if !valid_boot(boot) || !fresh_speaker_frame(frame_us, now_us) {
            self.revoke();
            return;
        }
        // An unresolved visible return is not benign absence. Even a reader
        // that missed Checking must see a new epoch after it disappears.
        if self.state.get() == SpeakerState::Checking {
            self.revoke();
        }
        if self.boot.borrow().as_deref().is_some_and(|was| was != boot) {
            self.revoke();
        }
        if !self.enabled.get() {
            return;
        }
        *self.boot.borrow_mut() = Some(boot.to_owned());
        self.observed_face.borrow_mut().take();
        self.state.set(SpeakerState::Absent);
        self.observed_us.set(Some(frame_us));
        self.write(boot, "absent", None);
    }

    fn write(&self, boot: &str, state: &str, subject: Option<(&str, Option<&str>, &str)>) {
        if !self.enabled.get() {
            return;
        }
        let Some(publisher) = self.publisher.as_deref() else {
            self.disable();
            return;
        };
        let mut source = serde_json::json!({
            "state":state, "boot":boot, "publisher":publisher, "generation":self.generation.get(),
        });
        if self.ticket_required.get() {
            source["version"] = 3.into();
            match self.camera_ticket.get() {
                Some(binding) if self.publisher_id() == Some(binding.publisher) => {
                    source["camera_hold"] = binding.hold.into();
                    source["camera_began_us"] = binding.began_us.into();
                }
                _ if state == "revoked" => {}
                _ => {
                    self.state.set(SpeakerState::Revoked);
                    self.subject.borrow_mut().take();
                    remove_speaker(&self.known);
                    return;
                }
            }
        }
        if self.ticket_required.get()
            && state != "revoked"
            && self.camera_ticket.get().is_none_or(|binding| {
                self.observed_us
                    .get()
                    .is_none_or(|observed| observed < binding.began_us)
            })
        {
            self.state.set(SpeakerState::Revoked);
            self.subject.borrow_mut().take();
            remove_speaker(&self.known);
            return;
        }
        if state != "revoked"
            && let (Some(until), Some(observed)) =
                (self.permission_until_us.get(), self.observed_us.get())
        {
            let until = self
                .camera_ticket
                .get()
                .filter(|_| self.ticket_required.get())
                .map_or(until, |binding| until.min(binding.until_us));
            source["permission_until_us"] = until.into();
            source["observed_us"] = observed.into();
            self.published_until_us.set(Some(until));
        }
        if let Some((name, id, face)) = subject {
            source["name"] = name.into();
            source["id"] = serde_json::json!(id);
            source["face"] = face.into();
        }
        if !write_speaker(&self.known, &source) {
            self.disable();
        }
    }
}

impl Drop for KnownSpeakerPublisher {
    fn drop(&mut self) {
        remove_speaker(&self.known);
    }
}

fn fresh_speaker_frame(frame_us: u64, now_us: u64) -> bool {
    frame_us > 0 && frame_us <= now_us && now_us - frame_us < 3_000_000
}

fn valid_boot(boot: &str) -> bool {
    boot.len() == 16
        && boot
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_passing_face(face: &str) -> bool {
    face.strip_prefix('f').is_some_and(|digits| {
        !digits.is_empty()
            && digits.len() <= 10
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && digits.parse::<u32>().is_ok_and(|id| id > 0)
    })
}

fn valid_speaker_fields(
    name: Option<&str>,
    id: Option<&str>,
    boot: Option<&str>,
    face: Option<&str>,
) -> bool {
    name.zip(boot)
        .zip(face)
        .is_some_and(|((name, boot), face)| {
            plain_name(name).as_deref() == Some(name)
                && id.is_none_or(lamp_msgs::known::is_person_id)
                && valid_boot(boot)
                && valid_passing_face(face)
        })
}

fn speaker_temporary(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".new");
    PathBuf::from(temporary)
}

fn remove_speaker(known: &Path) {
    let path = known_speaker_of(known);
    let _ = std::fs::remove_file(speaker_temporary(&path));
    let _ = std::fs::remove_file(path);
}

fn write_speaker(known: &Path, source: &serde_json::Value) -> bool {
    let path = known_speaker_of(known);
    let temporary = speaker_temporary(&path);
    let _ = std::fs::remove_file(&temporary);
    let write = || -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, source)?;
        file.write_all(b"\n")?;
        drop(file);
        std::fs::rename(&temporary, &path)
    };
    if write().is_err() {
        remove_speaker(known);
        false
    } else {
        true
    }
}

/// The name of the one known at the lamp (`--known-file`), replaced whole,
/// or no file; and while speaking is judged, their face's passing id in
/// [`known_face_of`], which goes with the name. With
/// `LAMP_MEMORY_STABLE_ID=on` ([`People::id_of`] gives `id`), their memory
/// id on the line after the name ([`lamp_msgs::known::known_text`]);
/// without it, the name alone, as before.
pub fn keep_known(path: &Path, name: Option<&str>, id: Option<&str>, face: Option<&str>) {
    if name.is_none() {
        remove_speaker(path);
    }
    keep_known_legacy(path, name, id, face);
}

/// Update legacy files under the main loop's separate typed-source owner.
/// Other callers use `keep_known`, whose absence still clears any old source.
pub fn keep_known_legacy(path: &Path, name: Option<&str>, id: Option<&str>, face: Option<&str>) {
    let beside = known_face_of(path);
    let Some(name) = name else {
        let _ = std::fs::remove_file(&beside);
        let _ = std::fs::remove_file(path);
        return;
    };
    // The name first, then the face (the reviewer on #2008). The voice reads
    // face, name, face; between the two writes it can only see the new name
    // beside the old face, which matches nobody and narrows a line to the
    // room. The other order could pair the old name with the new face and
    // keep a newcomer's line as the one before's.
    replace_line(path, &lamp_msgs::known::known_text(name, id));
    match face {
        Some(face) => replace_line(&beside, face),
        None => {
            let _ = std::fs::remove_file(&beside);
        }
    }
}

/// The file of names Who was here may say (`lamp-skills --tellable-file`,
/// #1997): `tellable` beside the known file.
pub fn tellable_of(known: &Path) -> PathBuf {
    known.with_file_name("tellable")
}

/// The names of those who said yes to being named, one a line, replaced
/// whole (mode 0600); no file when there are none.
pub fn keep_tellable(path: &Path, names: &[String]) {
    if names.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    replace_line(path, &names.join("\n"));
}

fn replace_line(path: &Path, line: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".new");
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| writeln!(file, "{line}"));
    if written.is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}
/// How quickly a remembered person's familiarity fades (#1670): it halves
/// in fourteen days without them.
pub const FAMILIAR_HALF_LIFE_S: u64 = 14 * 86_400;
/// A person is counted once in this long at most: about once a day, with
/// no day or time of anyone's kept (the eyes know no local day).
pub const COUNTED_EVERY_S: u64 = 16 * 3_600;
/// The familiarity at which a remembered person is `regular` (about once a
/// week, kept up) and `close` (about three days a week): authored, to be
/// tuned from real scores after a week on the office lamps (#1670).
pub const REGULAR_FROM: f32 = 2.0;
pub const CLOSE_FROM: f32 = 8.0;
/// The day's counts are written at this hour of the lamp's local day, when
/// nobody is likely near (the first builder on #1763, 13:38).
pub const SCORED_AT_HOUR: i64 = 3;

/// The last 03:00 at the lamp ([`SCORED_AT_HOUR`]) at or before `now_unix_s`,
/// for a lamp `offset_s` east of UTC, in Unix seconds.
pub fn scoring_hour(now_unix_s: u64, offset_s: i64) -> u64 {
    let local = now_unix_s as i64 + offset_s;
    let since = (local - SCORED_AT_HOUR * 3_600).rem_euclid(86_400);
    (local - since - offset_s).max(0) as u64
}

/// The forgets that wait in the set-aside folder until they can be finished
/// (#1586): each name, hashed, of someone who asked to be forgotten while a
/// set-aside file could not be searched for them, and when they asked. A
/// face of theirs kept before then that comes back, read back into the
/// people file or in a set-aside file that can be searched, is dropped at
/// once; one remembered after, at their own later word, is kept (Twelve's
/// P2 on #1767). Once nothing is left that cannot be searched, the file
/// goes. Never counted or searched as people's records, and emptied by the
/// reset with the folder.
pub const PENDING_FORGETS: &str = "forgets-pending";

/// One forget that waits: the name's hash ([`name_hash`]) and when it was
/// asked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Waiting {
    hash: String,
    at_unix_s: u64,
}

/// What a forget did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forgot {
    /// The name was found and taken out of the file or a set-aside file.
    pub forgot: bool,
    /// Set-aside files that could not be searched for it: the forget waits
    /// for them ([`PENDING_FORGETS`]).
    pub unsearched: usize,
}

/// Where the five marks of a face sit in the recogniser's 112 by 112
/// picture: the eye on the left of the picture, the other eye, the nose and
/// the two corners of the mouth, in the detector's order.
const REFERENCE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// The turn, scale and shift that best carries `from` onto `to`, as
/// `(a, b, x, y)`: a point `(u, v)` goes to `(a u - b v + x, b u + a v + y)`.
fn similarity(from: &[(f32, f32); 5], to: &[(f32, f32); 5]) -> Option<(f32, f32, f32, f32)> {
    let mean = |points: &[(f32, f32); 5]| {
        let (sx, sy) = points
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
        (sx / 5.0, sy / 5.0)
    };
    let (fx, fy) = mean(from);
    let (tx, ty) = mean(to);
    let (mut along, mut across, mut spread) = (0.0f32, 0.0f32, 0.0f32);
    for (f, t) in from.iter().zip(to) {
        let (u, v) = (f.0 - fx, f.1 - fy);
        let (p, q) = (t.0 - tx, t.1 - ty);
        along += u * p + v * q;
        across += u * q - v * p;
        spread += u * u + v * v;
    }
    if spread.is_nan() || spread <= 1e-6 {
        return None;
    }
    let (a, b) = (along / spread, across / spread);
    Some((a, b, tx - (a * fx - b * fy), ty - (b * fx + a * fy)))
}

/// The face with these marks, upright and 112 by 112: red, green and blue
/// planes, each value 0 to 255, as the recogniser takes them. Outside the
/// frame is black. `None` for marks that are all one point.
pub fn align(picture: &Picture, marks: &[(f32, f32); 5]) -> Option<Vec<f32>> {
    let plane = WIDTH * HEIGHT;
    // The picture's planes are blue, green, red; the recogniser's are red,
    // green, blue.
    align_with(marks, picture.filled, |colour, u, v| {
        picture.planes[(2 - colour) * plane + v * WIDTH + u]
    })
}

/// As [`align`], but cut from the frame at its own size (`LAMP_FACE_FULL_CROP`):
/// the marks, found in the shrunk picture, are carried back to the frame, and
/// only the 112 by 112 face is read from it, so a face 30 pixels tall in the
/// picture is cut from the 120 the camera saw, not stretched from 30.
pub fn align_full(frame: &Frame, marks: &[(f32, f32); 5]) -> Option<Vec<f32>> {
    if frame.width == 0 || frame.height == 0 || frame.rgb.len() != frame.width * frame.height * 3 {
        return None;
    }
    // A picture pixel's centre, in the frame's pixels.
    let back = |p: f32| (p + 0.5) / frame.scale - 0.5;
    let marks = marks.map(|(x, y)| (back(x), back(y)));
    align_with(&marks, (frame.width, frame.height), |colour, u, v| {
        f32::from(frame.rgb[(v * frame.width + u) * 3 + colour])
    })
}

/// The 112 by 112 face from a frame `filled` across and down, read pixel by
/// pixel through `at(colour, x, y)`, colour 0 red, 1 green, 2 blue.
fn align_with(
    marks: &[(f32, f32); 5],
    filled: (usize, usize),
    at: impl Fn(usize, usize, usize) -> f32,
) -> Option<Vec<f32>> {
    let (a, b, x, y) = similarity(marks, &REFERENCE)?;
    // Back from the recogniser's picture to the frame.
    let scale = a * a + b * b;
    let (ia, ib) = (a / scale, -b / scale);
    let (filled_x, filled_y) = (filled.0 as f32, filled.1 as f32);
    let mut out = vec![0.0f32; 3 * SIDE * SIDE];
    for row in 0..SIDE {
        for column in 0..SIDE {
            let (p, q) = (column as f32 - x, row as f32 - y);
            let u = ia * p - ib * q;
            let v = ib * p + ia * q;
            if u < 0.0 || v < 0.0 || u > filled_x - 1.0 || v > filled_y - 1.0 {
                continue;
            }
            let (u0, v0) = (u.floor() as usize, v.floor() as usize);
            let (u1, v1) = ((u0 + 1).min(filled.0 - 1), (v0 + 1).min(filled.1 - 1));
            let (du, dv) = (u - u0 as f32, v - v0 as f32);
            for colour in 0..3 {
                let value = at(colour, u0, v0) * (1.0 - du) * (1.0 - dv)
                    + at(colour, u1, v0) * du * (1.0 - dv)
                    + at(colour, u0, v1) * (1.0 - du) * dv
                    + at(colour, u1, v1) * du * dv;
                out[colour * SIDE * SIDE + row * SIDE + column] = value;
            }
        }
    }
    Some(out)
}

/// How alike two prints are: their cosine, -1 to 1.
pub fn likeness(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn unit(values: Vec<f32>) -> Option<Vec<f32>> {
    let length = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    (values.len() == PRINT && length.is_finite() && length > 1e-6)
        .then(|| values.into_iter().map(|v| v / length).collect())
}

pub struct Recogniser {
    plan: Arc<TypedRunnableModel>,
}

impl Recogniser {
    /// Verifies the exact pinned bytes before the ONNX parser sees them.
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        if file.metadata()?.len() != MODEL_BYTES {
            return Err(format!("{} has an unexpected size", path.display()).into());
        }
        let mut bytes = Vec::with_capacity(MODEL_BYTES as usize);
        file.take(MODEL_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != MODEL_BYTES {
            return Err(format!("{} changed while it was read", path.display()).into());
        }
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if digest != MODEL_SHA256 {
            return Err(format!("{} does not match the pinned SHA256", path.display()).into());
        }
        let plan = tract_onnx::onnx()
            .model_for_read(&mut bytes.as_slice())?
            .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { plan })
    }

    /// The print of an aligned face, of length one.
    pub fn print(&self, aligned: &[f32]) -> Result<Vec<f32>> {
        let input = Tensor::from_shape(&[1, 3, SIDE, SIDE], aligned)?;
        let outputs = self.plan.run(tvec!(input.into()))?;
        let values = outputs
            .first()
            .ok_or("the recogniser gave nothing")?
            .to_plain_array_view::<f32>()?
            .iter()
            .copied()
            .collect::<Vec<f32>>();
        unit(values).ok_or_else(|| "the recogniser gave no print".into())
    }
}

/// Read one release forward (#1505): a field a later build added is not
/// this build's to refuse; the next save writes this build's fields only.
///
/// **The rule for the next field** (the reviewer on #1558, as #1512's for
/// tasks): an optional field that only adds to what is known may come
/// without a bump. A field that narrows what the lamp may do with a face
/// (an expiry, "don't greet me by name"), or any new required field, bumps
/// [`Kept`]'s `version` in the same change: this build would otherwise read
/// past it and recognise more widely, and its next save would drop it for
/// good. An older build then sets the whole file aside instead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    pub prints: Vec<Vec<f32>>,
    pub added_unix_s: u64,
    /// How familiar they are (#1670, `--visits`): one number that rises by
    /// one each day they are seen and halves every fourteen without them,
    /// as of the file's `scored_at_unix_s`. No dates or days are kept, and
    /// it goes with their prints when they are forgotten. An optional field
    /// that only adds, so the file keeps its version (the rule above).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub familiar: Option<f32>,
    /// Their yes to being named to others in Who was here (#1997, the
    /// lead's yes 20 Oct): `Some(true)` only, absent for no. An optional
    /// field under the rule above: an older build reads past it and its next
    /// save drops it, which only ever narrows what is told.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tell_visits: Option<bool>,
    /// Their memory id (#2176, `LAMP_MEMORY_STABLE_ID=on`): given when they
    /// are first remembered, and to someone remembered before it was on as
    /// `person_id` of their name, so nobody's memory moves; kept through
    /// every later remembering and a rename, never another's. An optional
    /// field under the rule above: an older build reads past it, and its
    /// next save drops it, which leaves a renamed person's memory under
    /// their old name's id until it is on again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Kept {
    version: u32,
    /// When every person's familiarity was last brought to now: the eyes'
    /// own time of scoring, not anyone's visit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scored_at_unix_s: Option<u64>,
    people: Vec<Person>,
}

/// How familiar a remembered person at the lamp is (#1670).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Familiar {
    New,
    Regular,
    Close,
}

impl Familiar {
    fn of(score: f32) -> Self {
        if score >= CLOSE_FROM {
            Self::Close
        } else if score >= REGULAR_FROM {
            Self::Regular
        } else {
            Self::New
        }
    }
}

/// `score` as of `from_unix_s`, faded to `now_unix_s`.
fn faded(score: f32, from_unix_s: u64, now_unix_s: u64) -> f32 {
    let days = now_unix_s.saturating_sub(from_unix_s) as f64 / FAMILIAR_HALF_LIFE_S as f64;
    (f64::from(score) * 0.5f64.powf(days)) as f32
}

/// A name a person gives: letters, with spaces, hyphens and apostrophes
/// between them, at most forty characters. Its form is kept as said.
pub fn plain_name(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = name.chars().count();
    let fine = (1..=MOST_NAME_CHARS).contains(&count)
        && name.chars().next().is_some_and(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’'));
    fine.then_some(name)
}

pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Lower-case letters with their accents, by the letter without: the Latin
/// ones and Vietnamese's, as the voice folds a heard name
/// (`known_faces::ACCENTED`).
const ACCENTED: &[(&str, char)] = &[
    ("àáâãäåāăąảạằắẳẵặầấẩẫậ", 'a'),
    ("çćč", 'c'),
    ("ďđ", 'd'),
    ("èéêëēėęěẻẽẹềếểễệ", 'e'),
    ("ìíîïīįỉĩị", 'i'),
    ("ľłĺ", 'l'),
    ("ñńň", 'n'),
    ("òóôõöøōőơỏọồốổỗộờớởỡợ", 'o'),
    ("ŕř", 'r'),
    ("śšşș", 's'),
    ("ťţț", 't'),
    ("ùúûüūůűưủũụừứửữự", 'u'),
    ("ýÿỳỷỹỵ", 'y'),
    ("źżž", 'z'),
];

/// A name lower-cased, with its accents and tone marks taken off, combining
/// ones too: how two spellings of one name compare.
fn folded(name: &str) -> Vec<char> {
    name.trim()
        .to_lowercase()
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .map(|c| {
            ACCENTED
                .iter()
                .find(|(letters, _)| letters.contains(c))
                .map_or(c, |(_, plain)| *plain)
        })
        .collect()
}

/// Two names of three letters or more that aren't the same name but fold to
/// one, or are one letter apart (one put in, left out or changed) once
/// folded: how a name misheard once is spelt, or one written two ways.
/// Shorter names are never near, so "Al" is not asked about "Ali".
pub(crate) fn near_name(a: &str, b: &str) -> bool {
    if same_name(a, b) {
        return false;
    }
    let (a, b) = (folded(a), folded(b));
    if a.len().min(b.len()) < 3 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let start = short
        .iter()
        .zip(long.iter())
        .take_while(|(x, y)| x == y)
        .count();
    if short.len() == long.len() {
        short[start + 1..] == long[start + 1..]
    } else {
        short[start..] == long[start + 1..]
    }
}

/// An unchanged person reserved for an explicitly approved face repair.
/// Private, non-cloneable and deliberately not Debug or serializable: the
/// snapshot contains biometric prints. It is valid only in its live store,
/// before another committed save, and is consumed by one replacement attempt.
/// This is a store precondition, not proof of account or spoken consent.
pub struct RepairTarget {
    owner: Arc<AtomicU64>,
    revision: u64,
    person: Person,
}

/// Fixed outcomes contain no name, identifier, print, or filesystem path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairError {
    TargetChanged,
    InvalidPrints,
    KnownElsewhere,
    /// No rename completed: the old file and in-memory records remain.
    NotWritten,
    /// Rename completed and RAM follows that visible file, but directory
    /// durability is unconfirmed. Consume the approval, reload and report
    /// uncertainty; never retry it as if the old face were still installed.
    Indeterminate,
}

/// Before each actual I/O operation. The private hook lets tests fail these
/// boundaries without replacing the filesystem or changing ordinary saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairStep {
    Write,
    SyncFile,
    Rename,
    SyncDirectory,
}

/// The people the lamp was asked to remember, in one file of mode 0600.
pub struct People {
    path: PathBuf,
    people: Vec<Person>,
    /// When their familiarity was last brought to now ([`Kept`]).
    scored_at: Option<u64>,
    /// Who was counted, and when, since these eyes started: kept in memory
    /// only, so nothing on disk says when anyone was at the lamp. A restart
    /// may count someone twice in a day.
    counted: Vec<(String, u64)>,
    /// Who was counted since the file was last scored: added to it at the
    /// next 03:00 at the lamp ([`Self::score`]), never at a visit, so neither
    /// the file nor its time says when anyone came (the reviewer on #1763).
    /// A restart before then loses them.
    unscored: Vec<String>,
    /// With `LAMP_MEMORY_STABLE_ID=on`: each person keeps one memory id
    /// ([`Person::id`]), told beside their name in the known file.
    stable_ids: bool,
    /// Process-local store identity and save revision. Any visible write
    /// invalidates staged repairs, including a delete followed by recreation.
    /// Exhaustion disables new repairs instead of wrapping to an old revision.
    repair_revision: Arc<AtomicU64>,
}

impl People {
    /// What the file holds; none at all if there is no file yet. Read one
    /// release forward (#1505): what this build cannot read is set aside in
    /// [`SET_ASIDE`] beside it, never dropped, and the eyes start with the
    /// rest. A file it cannot read at all (too large, not JSON, a `version`
    /// above 1, no list of people) is moved there whole, and the eyes start
    /// knowing nobody. A person it cannot read (a field of the wrong type, a
    /// name or prints this build does not keep, a second of one name, a
    /// twenty-first) is written there, and the rest saved back without them,
    /// so a restart does not set them aside again. Each is said in one line,
    /// with a count and never a name. Before reading, unlink only the exact
    /// `.new` and `.repair.new` siblings left by interrupted writes; neither
    /// is a committed record. Opening requires the same exclusive writer
    /// ownership as saving. A failure to clean a temporary or set aside a
    /// record is an error, so the eyes do not start in that state.
    pub fn open(path: PathBuf) -> Result<Self> {
        discard_unfinished(&path)?;
        let mut bytes = Vec::new();
        match File::open(&path) {
            Ok(file) => {
                file.take(MOST_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    people: Vec::new(),
                    scored_at: None,
                    counted: Vec::new(),
                    unscored: Vec::new(),
                    stable_ids: lamp_msgs::known::stable_ids(),
                    repair_revision: Arc::new(AtomicU64::new(0)),
                });
            }
            Err(error) => return Err(error.into()),
        }
        let whole = |why: &str| -> Result<Self> {
            let kept = set_aside_whole(&path, why, &bytes)?;
            eprintln!(
                "lamp-eyes: {} could not be read ({why}); moved whole to {}, and the eyes start knowing nobody",
                path.display(),
                kept.display()
            );
            Ok(Self {
                path: path.clone(),
                people: Vec::new(),
                scored_at: None,
                counted: Vec::new(),
                unscored: Vec::new(),
                stable_ids: lamp_msgs::known::stable_ids(),
                repair_revision: Arc::new(AtomicU64::new(0)),
            })
        };
        if bytes.len() as u64 > MOST_FILE_BYTES {
            return whole("too large");
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return whole("not JSON");
        };
        let list = match (
            value.get("version").and_then(serde_json::Value::as_u64),
            value.get("people").and_then(serde_json::Value::as_array),
        ) {
            (Some(1), Some(list)) => list,
            (Some(version), _) if version > 1 => {
                return whole(&format!("version {version}, from a newer build"));
            }
            _ => return whole("not a list of people"),
        };
        let mut people: Vec<Person> = Vec::new();
        let mut aside = Vec::new();
        for entry in list {
            match serde_json::from_value::<Person>(entry.clone()) {
                Ok(person)
                    if valid(std::slice::from_ref(&person))
                        && people.len() < MOST_PEOPLE
                        && !people
                            .iter()
                            .any(|other| same_name(&other.name, &person.name)) =>
                {
                    people.push(person);
                }
                _ => aside.push(entry.clone()),
            }
        }
        let scored_at = value
            .get("scored_at_unix_s")
            .and_then(serde_json::Value::as_u64);
        let mut opened = Self {
            path,
            people,
            scored_at,
            counted: Vec::new(),
            unscored: Vec::new(),
            stable_ids: false,
            repair_revision: Arc::new(AtomicU64::new(0)),
        };
        if !aside.is_empty() {
            let kept = write_set_aside(
                &opened.path,
                "person",
                &serde_json::to_vec(&serde_json::json!({"version": 1, "people": aside}))?,
            )?;
            opened.save(&opened.people)?;
            eprintln!(
                "lamp-eyes: {} people's records in {} could not be read; set aside in {}, and {} people are known",
                aside.len(),
                opened.path.display(),
                kept.display(),
                opened.people.len()
            );
        }
        // A forget that waited may be finished now (#1586): never a reason
        // not to start.
        if let Err(error) = opened.finish_pending() {
            eprintln!("lamp-eyes: the forgets that wait could not be finished now: {error}");
        }
        Ok(opened.with_stable_ids(lamp_msgs::known::stable_ids()))
    }

    /// The same people, each keeping one memory id or not
    /// (`LAMP_MEMORY_STABLE_ID`, read by [`Self::open`]). On, everyone
    /// without one is given `person_id` of their name now, in memory, so
    /// their memory stays where it is; it reaches the file with its next
    /// write. Ids already held go first, in file order, so a record from an
    /// older build with no id never takes the id a later record holds; of
    /// two with one id, the later is given another. Then each without one
    /// is given `person_id` of their name, or it with the lowest free `-N`.
    #[must_use]
    pub fn with_stable_ids(mut self, on: bool) -> Self {
        self.stable_ids = on;
        if on {
            let mut taken: Vec<String> = Vec::new();
            for person in &mut self.people {
                match person.id.take() {
                    Some(id) if !taken.contains(&id) => {
                        taken.push(id.clone());
                        person.id = Some(id);
                    }
                    _ => {}
                }
            }
            for person in &mut self.people {
                if person.id.is_none() {
                    let id = fresh_id(&person.name, &taken);
                    taken.push(id.clone());
                    person.id = Some(id);
                }
            }
        }
        self
    }

    /// The memory id told beside this person's name in the known file:
    /// only with `LAMP_MEMORY_STABLE_ID=on`, else `None`, as before.
    pub fn id_of(&self, name: &str) -> Option<String> {
        if !self.stable_ids {
            return None;
        }
        self.people
            .iter()
            .find(|person| same_name(&person.name, name))
            .and_then(|person| person.id.clone())
    }

    /// Whether memory's by-name id for `name` (`person_id` of it, the voice's
    /// key without the stable id) is nobody else's: false only with
    /// `LAMP_MEMORY_STABLE_ID=on` when a record not called `name` holds it,
    /// a person renamed from `name` who kept its memory (the reviewer-lead's
    /// P2 on #2330). Asked after a forget, so the app's remove never forgets
    /// a renamed person's memory with the name they no longer have.
    pub fn name_id_free(&self, name: &str) -> bool {
        if !self.stable_ids {
            return true;
        }
        let by_name = lamp_msgs::known::person_id(name);
        !self.people.iter().any(|person| {
            !same_name(&person.name, name) && person.id.as_deref() == Some(by_name.as_str())
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.people
            .iter()
            .map(|person| person.name.clone())
            .collect()
    }

    /// Snapshot an existing canonical record without freeing its name or
    /// changing its ID. Missing legacy IDs stay missing; no ID is allocated.
    /// The controller must separately bind account approval, the fresh lamp
    /// capture and spoken consent, and enforce cancellation and expiry.
    pub fn repair_target(&self, name: &str) -> Option<RepairTarget> {
        let name = plain_name(name)?;
        let person = self
            .people
            .iter()
            .find(|person| same_name(&person.name, &name))?;
        let revision = self.repair_revision.load(Ordering::Relaxed);
        (revision != u64::MAX).then(|| RepairTarget {
            owner: self.repair_revision.clone(),
            revision,
            person: person.clone(),
        })
    }

    /// Conditionally replace only this person's prints. Their canonical name,
    /// stable memory ID, familiarity and added time stay; named-visit consent
    /// is cleared because new face prints cannot inherit the previous yes.
    /// No record is deleted or re-added, and no memory or rule store is called.
    ///
    /// This blocking store operation belongs off the capture path. The caller
    /// must exclusively own this People writer; this is not a cross-process
    /// transaction or an authorization API. Success follows file sync, atomic
    /// rename and parent-directory sync. A post-rename failure is explicitly
    /// indeterminate, with RAM already matching the newly visible file.
    pub fn replace_prints(
        &mut self,
        target: RepairTarget,
        prints: Vec<Vec<f32>>,
    ) -> std::result::Result<(), RepairError> {
        self.replace_prints_at(target, prints, |_| Ok(()))
    }

    pub(crate) fn replace_prints_at(
        &mut self,
        target: RepairTarget,
        prints: Vec<Vec<f32>>,
        mut before: impl FnMut(RepairStep) -> io::Result<()>,
    ) -> std::result::Result<(), RepairError> {
        if !Arc::ptr_eq(&target.owner, &self.repair_revision)
            || target.revision != self.repair_revision.load(Ordering::Relaxed)
        {
            return Err(RepairError::TargetChanged);
        }
        let Some(index) = self
            .people
            .iter()
            .position(|person| *person == target.person)
        else {
            return Err(RepairError::TargetChanged);
        };
        // Recognition uses dot products of unit prints. Refuse malformed or
        // scaled vectors rather than allowing scale to hide another match.
        if !(1..=MOST_PRINTS).contains(&prints.len())
            || prints.iter().any(|print| {
                if print.len() != PRINT {
                    return true;
                }
                let squared = print.iter().map(|value| value * value).sum::<f32>();
                !print.iter().all(|value| value.is_finite())
                    || !squared.is_finite()
                    || (squared - 1.0).abs() > 0.001
            })
        {
            return Err(RepairError::InvalidPrints);
        }
        // Check every other record, not only who()'s best match: a print
        // matching the target more strongly may still match somebody else.
        if self.people.iter().enumerate().any(|(other, person)| {
            other != index
                && person.prints.iter().any(|kept| {
                    prints
                        .iter()
                        .any(|print| likeness(kept, print) >= SAME_PERSON)
                })
        }) {
            return Err(RepairError::KnownElsewhere);
        }
        let mut next = self.people.clone();
        next[index].prints = prints;
        next[index].tell_visits = None;
        let text = serde_json::to_vec(&Kept {
            version: 1,
            scored_at_unix_s: self.scored_at,
            people: next.clone(),
        })
        .map_err(|_| RepairError::NotWritten)?;
        if text.len() as u64 > MOST_FILE_BYTES {
            return Err(RepairError::NotWritten);
        }
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let directory = File::open(parent).map_err(|_| RepairError::NotWritten)?;
        let (temporary, mut file) = repair_file(&self.path).map_err(|_| RepairError::NotWritten)?;
        let written = (|| -> io::Result<()> {
            before(RepairStep::Write)?;
            file.write_all(&text)?;
            before(RepairStep::SyncFile)?;
            file.sync_all()?;
            before(RepairStep::Rename)?;
            if file.metadata()?.mode() & 0o7777 != 0o600 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "repair temporary is not private",
                ));
            }
            std::fs::rename(&temporary, &self.path)
        })();
        drop(file);
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(RepairError::NotWritten);
        }
        // No fallible step or callback between the visible rename and RAM.
        self.people = next;
        self.advance_repair_revision();
        before(RepairStep::SyncDirectory)
            .and_then(|()| directory.sync_all())
            .map_err(|_| RepairError::Indeterminate)
    }

    fn advance_repair_revision(&self) {
        let _ =
            self.repair_revision
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |revision| {
                    Some(revision.saturating_add(1))
                });
    }

    /// The names of those who said yes to being named to others in Who was
    /// here (#1997).
    pub fn tellable(&self) -> Vec<String> {
        self.people
            .iter()
            .filter(|person| person.tell_visits == Some(true))
            .map(|person| person.name.clone())
            .collect()
    }

    /// Their yes (`true`) or no to being named to others, saved; whether
    /// they are someone the lamp remembers.
    pub fn tell_visits(&mut self, name: &str, yes: bool) -> Result<bool> {
        let mut next = self.people.clone();
        let Some(person) = next.iter_mut().find(|person| same_name(&person.name, name)) else {
            return Ok(false);
        };
        person.tell_visits = yes.then_some(true);
        self.save(&next)?;
        self.people = next;
        Ok(true)
    }

    /// How alike this print is to the nearest of anyone's kept prints,
    /// whether or not it reaches [`SAME_PERSON`]; `None` with nobody kept. For the
    /// journal's line on each check: a number, never a name.
    pub fn best_likeness(&self, print: &[f32]) -> Option<f32> {
        self.people
            .iter()
            .flat_map(|person| &person.prints)
            .map(|kept| likeness(kept, print))
            .reduce(f32::max)
    }

    /// How alike this print is to `name`'s own kept prints, at best; `None`
    /// when nobody by that name is kept. What a held person's hold is
    /// judged on ([`Held::keeps_print`]).
    pub fn likeness_to(&self, name: &str, print: &[f32]) -> Option<f32> {
        self.people
            .iter()
            .find(|person| same_name(&person.name, name))?
            .prints
            .iter()
            .map(|kept| likeness(kept, print))
            .reduce(f32::max)
    }

    /// Whose print this is, and how alike: the best match at or over
    /// [`SAME_PERSON`], or nobody.
    pub fn who(&self, print: &[f32]) -> Option<(String, f32)> {
        self.people
            .iter()
            .filter_map(|person| {
                person
                    .prints
                    .iter()
                    .map(|kept| likeness(kept, print))
                    .fold(None, |best: Option<f32>, value| {
                        Some(best.map_or(value, |b| b.max(value)))
                    })
                    .map(|best| (person.name.clone(), best))
            })
            .filter(|(_, best)| *best >= SAME_PERSON)
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Keeps these prints for this name, in place of any kept for it, and
    /// writes the file before it answers. Refused: a name that is not a
    /// name, no prints or too many, prints of another shape, or a twenty-first
    /// person.
    pub fn remember(
        &mut self,
        name: &str,
        prints: Vec<Vec<f32>>,
        now_unix_s: u64,
    ) -> Result<String> {
        let name = plain_name(name).ok_or("that is not a name the lamp keeps")?;
        if prints.is_empty()
            || prints.len() > MOST_PRINTS
            || prints.iter().any(|p| p.len() != PRINT)
        {
            return Err("no prints, too many, or of the wrong length".into());
        }
        self.remember_as(&name, prints, now_unix_s, None)
    }

    /// [`Self::remember`], carrying `carried`, a renamed person's memory id
    /// (#2176), to the name: else the id the name already has, else, with
    /// the stable id on, a new one ([`fresh_id`]).
    fn remember_as(
        &mut self,
        name: &str,
        prints: Vec<Vec<f32>>,
        now_unix_s: u64,
        carried: Option<String>,
    ) -> Result<String> {
        let name = name.to_owned();
        let had = self
            .people
            .iter()
            .find(|person| same_name(&person.name, &name))
            .and_then(|person| person.id.clone());
        let mut next = self.people.clone();
        next.retain(|person| !same_name(&person.name, &name));
        if next.len() >= MOST_PEOPLE {
            return Err("the lamp already knows as many people as it keeps".into());
        }
        let id = carried.or(had).or_else(|| {
            self.stable_ids.then(|| {
                let taken: Vec<String> = next.iter().filter_map(|other| other.id.clone()).collect();
                fresh_id(&name, &taken)
            })
        });
        next.push(Person {
            name: name.clone(),
            prints,
            added_unix_s: now_unix_s,
            familiar: None,
            // A remembering starts with no yes to being named, even under a
            // name that had one: the prints are whoever is at the lamp now,
            // and the yes was another face's (the reviewer's P2 on #2098).
            // The voice asks again.
            tell_visits: None,
            id,
        });
        self.save(&next)?;
        self.people = next;
        Ok(name)
    }

    /// `name`'s prints set to `prints`, everything else of them as it was
    /// (familiarity, who-was-here's yes), written before it answers.
    fn keep_prints(&mut self, name: &str, prints: Vec<Vec<f32>>) -> Result<()> {
        let mut next = self.people.clone();
        for person in next
            .iter_mut()
            .filter(|person| same_name(&person.name, name))
        {
            person.prints = prints.clone();
        }
        self.save(&next)?;
        self.people = next;
        Ok(())
    }

    /// The same, keeping one name for one face (`LAMP_NAMES=checked`, #829's
    /// names, the first builder at 13:24 on 16 October): prints that are
    /// already someone's under another name are refused with that name, so
    /// that a name misheard never sits beside the right one with the same
    /// face. With `replace`, asked after the person said yes to the new
    /// name, that person is forgotten first. Every name the prints are
    /// known by is in the refusal, "A and B", since `replace` forgets
    /// them all (the reviewer on #1057).
    ///
    /// `apart` says the person spelled this name. Without it, prints at or
    /// over [`NEAR_PERSON`] to someone known by a name one letter from this
    /// one ([`near_name`]) are refused as [`Refused::NearTo`], naming the
    /// likeliest such person: the same person, recognised less well and
    /// misheard (#2176 row 4). A face near alone, or a name near alone,
    /// changes nothing.
    ///
    /// Under a name already kept, prints are never put in place of theirs by
    /// a face that isn't theirs, whatever `apart` or `replace` say (the
    /// reviewer's P1s on #2202 and #2211: a sibling's "yes", a mishearing
    /// into exactly their name, or a spelling took a known person's face and
    /// memory). At [`SAME_PERSON`] they are kept in place, as before. Near
    /// ([`NEAR_PERSON`]) they are refused as [`Refused::NearTo`] until
    /// spelled (`apart`), then added beside theirs, the newest
    /// [`MOST_PRINTS`] kept with at least one of their own. Under that,
    /// always [`Refused::NotLikeThem`].
    pub fn remember_one(
        &mut self,
        name: &str,
        prints: Vec<Vec<f32>>,
        now_unix_s: u64,
        replace: bool,
        apart: bool,
    ) -> std::result::Result<String, Refused> {
        let kept = plain_name(name).ok_or(Refused::NotAName)?;
        // Whoever else these prints are, by the same likeness that
        // recognises a face.
        let mut known: Vec<String> = Vec::new();
        for (other, _) in prints.iter().filter_map(|print| self.who(print)) {
            if !same_name(&other, &kept) && !known.iter().any(|seen| same_name(seen, &other)) {
                known.push(other);
            }
        }
        // Each new print's likeness to a person: its best against their own.
        let each_to = |person: &Person| -> Vec<f32> {
            prints
                .iter()
                .map(|print| {
                    person
                        .prints
                        .iter()
                        .map(|held| likeness(held, print))
                        .fold(f32::MIN, f32::max)
                })
                .collect()
        };
        // The capture as a whole, judged by its median print, so one near
        // print among unrelated ones doesn't carry them (Jarvis's P1 on
        // #2235).
        let median_to = |person: &Person| {
            let mut each = each_to(person);
            each.sort_by(f32::total_cmp);
            each.get(each.len() / 2).copied().unwrap_or(f32::MIN)
        };
        let best_to = |person: &Person| each_to(person).into_iter().fold(f32::MIN, f32::max);
        // Under a name already kept, whoever asks and whatever they say yes
        // to: a face that is someone else's never takes it, and one that
        // isn't theirs never goes in place of theirs (the reviewer's P1s on
        // #2202 and #2211, Jarvis's on #2235).
        if let Some(theirs) = self
            .people
            .iter()
            .find(|person| same_name(&person.name, &kept))
        {
            if !known.is_empty() {
                return Err(Refused::NotLikeThem);
            }
            let median = median_to(theirs);
            if median < NEAR_PERSON {
                return Err(Refused::NotLikeThem);
            }
            if median < SAME_PERSON {
                // Near theirs: perhaps another person's name misheard into
                // exactly theirs, so it's spelled first, as `near_to`.
                if !apart {
                    return Err(Refused::NearTo(theirs.name.clone()));
                }
                // Spelled as theirs: only the near prints, beside theirs,
                // never all in place of them, so a full capture still leaves
                // one of their own.
                let near: Vec<Vec<f32>> = prints
                    .iter()
                    .zip(each_to(theirs))
                    .filter(|(_, alike)| *alike >= NEAR_PERSON)
                    .map(|(print, _)| print.clone())
                    .collect();
                let ours = theirs.prints.len();
                let mut beside = theirs.prints.clone();
                beside.extend(near);
                let start = beside.len().saturating_sub(MOST_PRINTS);
                let mut kept_prints = beside.split_off(start);
                if ours > 0
                    && start >= ours
                    && let (Some(first), Some(newest)) =
                        (kept_prints.first_mut(), theirs.prints.last())
                {
                    *first = newest.clone();
                }
                return self
                    .keep_prints(&kept, kept_prints)
                    .map(|()| theirs_name(&self.people, &kept))
                    .map_err(|error| Refused::Store(error.to_string()));
            }
            // Theirs as a whole: kept in place of theirs, but only the prints
            // that are themselves at `SAME_PERSON` to theirs, so up to two of
            // five that are someone else's don't go in with them, a
            // sibling's near print included (the reviewer's P1 after #2252,
            // Reviewer-lead's P3 on #2269). The median is at `SAME_PERSON`,
            // so at least three are left.
            let own: Vec<Vec<f32>> = prints
                .iter()
                .zip(each_to(theirs))
                .filter(|(_, alike)| *alike >= SAME_PERSON)
                .map(|(print, _)| print.clone())
                .collect();
            return self
                .remember(&kept, own, now_unix_s)
                .map_err(|error| Refused::Store(error.to_string()));
        } else if known.is_empty() && !apart {
            let near = self
                .people
                .iter()
                .filter(|person| near_name(&person.name, &kept))
                .map(|person| (person, best_to(person)))
                .filter(|(_, best)| *best >= NEAR_PERSON)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((person, _)) = near {
                return Err(Refused::NearTo(person.name.clone()));
            }
        }
        let mut carried = None;
        if !known.is_empty() {
            if !replace {
                return Err(Refused::KnownAs(known.join(" and ")));
            }
            // A rename (#2176): the new name keeps the memory id of the
            // likeliest name it replaces, `known`'s first, so what they
            // told the lamp stays theirs. Only with the stable id on.
            carried = self
                .stable_ids
                .then(|| {
                    known.first().and_then(|first| {
                        self.people
                            .iter()
                            .find(|person| same_name(&person.name, first))
                            .and_then(|person| person.id.clone())
                    })
                })
                .flatten();
            let mut next = self.people.clone();
            next.retain(|person| !known.iter().any(|other| same_name(other, &person.name)));
            self.save(&next)
                .map_err(|error| Refused::Store(error.to_string()))?;
            self.people = next;
        }
        self.remember_as(&kept, prints, now_unix_s, carried)
            .map_err(|error| Refused::Store(error.to_string()))
    }

    /// Forgets this name's prints, and writes the file before it answers.
    /// Whether there were any.
    ///
    /// First unlink the exact uncommitted `.new` and `.repair.new` copies,
    /// which may still contain this person. A cleanup error aborts before
    /// changing the canonical store or RAM; it can never report completion.
    ///
    /// The set-aside records too (#1505, the reviewer on #1558): that name's
    /// entries leave every set-aside file with a list of people, which is
    /// rewritten 0600 or removed when nothing is left. A set-aside file that
    /// cannot be searched (no such list) might hold the face, so the answer
    /// is then an error, never "not known".
    ///
    /// Then the forget waits for them (#1586): the name, hashed, is kept in
    /// [`PENDING_FORGETS`], and the answer says how many could not be
    /// searched beside whether the name was found where it could be.
    pub fn forget(&mut self, name: &str) -> Result<Forgot> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        self.forget_at(name, now)
    }

    /// [`Self::forget`], asked at `now_unix_s`.
    pub fn forget_at(&mut self, name: &str, now_unix_s: u64) -> Result<Forgot> {
        discard_unfinished(&self.path)?;
        let name = name.trim();
        // Their counts not yet scored go too (#1670).
        self.unscored.retain(|counted| !same_name(counted, name));
        self.counted
            .retain(|(counted, _)| !same_name(counted, name));
        let mut next = self.people.clone();
        next.retain(|person| !same_name(&person.name, name));
        let mut forgot = next.len() != self.people.len();
        if forgot {
            self.save(&next)?;
            self.people = next;
        }
        let (aside, unsearched) = forget_set_aside(&self.path, |other, _| same_name(other, name))?;
        forgot |= aside;
        if unsearched > 0 {
            // Asked again, it waits from the later word.
            let hash = name_hash(name);
            let mut waiting = pending(&self.path)?;
            waiting.retain(|wait| wait.hash != hash);
            waiting.push(Waiting {
                hash,
                at_unix_s: now_unix_s,
            });
            keep_pending(&self.path, &waiting)?;
        }
        self.finish_pending()?;
        Ok(Forgot { forgot, unsearched })
    }

    /// The forgets waiting ([`PENDING_FORGETS`]) finished where they can
    /// be: a face of theirs kept before they asked and read back into the
    /// people file is dropped, and so is one in a set-aside file that can be
    /// searched now (one with no time it was kept counts as before). A face
    /// remembered after they asked is theirs to keep. Once no set-aside file
    /// is left that cannot be searched, they are done and the file goes.
    fn finish_pending(&mut self) -> Result<()> {
        let waiting = pending(&self.path)?;
        if waiting.is_empty() {
            return Ok(());
        }
        let is_waiting = |name: &str, added: Option<u64>| {
            let hash = name_hash(name);
            waiting
                .iter()
                .any(|wait| wait.hash == hash && added.is_none_or(|added| added < wait.at_unix_s))
        };
        let next: Vec<Person> = self
            .people
            .iter()
            .filter(|person| !is_waiting(&person.name, Some(person.added_unix_s)))
            .cloned()
            .collect();
        if next.len() != self.people.len() {
            self.save(&next)?;
            eprintln!(
                "lamp-eyes: {} people asked to be forgotten were back in {}; forgotten again",
                self.people.len() - next.len(),
                self.path.display()
            );
            self.people = next;
        }
        let (_, unsearched) = forget_set_aside(&self.path, is_waiting)?;
        if unsearched == 0 {
            keep_pending(&self.path, &[])?;
            eprintln!(
                "lamp-eyes: {} forgets that waited for set-aside files are finished",
                waiting.len()
            );
        }
        Ok(())
    }

    /// A remembered person seen at the lamp now (#1670, `--visits`): once in
    /// [`COUNTED_EVERY_S`] at most, they are counted, in memory only. Nothing
    /// is written: the count reaches the file at the next UTC day boundary
    /// ([`Self::score`]). Whether they were counted. An unknown name is
    /// nothing.
    pub fn seen(&mut self, name: &str, now_unix_s: u64) -> bool {
        if !self.people.iter().any(|person| person.name == name) {
            return false;
        }
        self.counted
            .retain(|(_, at)| now_unix_s.saturating_sub(*at) < COUNTED_EVERY_S);
        if self.counted.iter().any(|(counted, _)| counted == name) {
            return false;
        }
        self.counted.push((name.to_owned(), now_unix_s));
        self.unscored.push(name.to_owned());
        true
    }

    /// Call often, whoever is at the lamp, with the lamp's offset from UTC:
    /// once 03:00 at the lamp has passed since the file was last scored,
    /// every number is faded to that 03:00, each count since is added, and
    /// the file is written with that 03:00 as its time of scoring. So the
    /// file, and the time it was written, say only that a night passed,
    /// never when anyone came. Whether it wrote.
    pub fn score(&mut self, now_unix_s: u64, offset_s: i64) -> Result<bool> {
        let day = scoring_hour(now_unix_s, offset_s);
        // Never scored: today is where scoring begins, in memory only, so
        // the first write too waits for the next day to begin.
        let Some(scored_at) = self.scored_at else {
            self.scored_at = Some(day);
            return Ok(false);
        };
        if scored_at >= day {
            return Ok(false);
        }
        let scored = self.people.iter().any(|person| person.familiar.is_some());
        if self.unscored.is_empty() && !scored {
            return Ok(false);
        }
        let from = scored_at;
        let mut next = self.people.clone();
        for person in &mut next {
            let mut score = person.familiar.map(|score| faded(score, from, day));
            let counts = self
                .unscored
                .iter()
                .filter(|name| **name == person.name)
                .count();
            if counts > 0 {
                score = Some(score.unwrap_or(0.0) + counts as f32);
            }
            person.familiar = score;
        }
        self.save_scored(&next, Some(day))?;
        self.people = next;
        self.scored_at = Some(day);
        self.unscored.clear();
        Ok(true)
    }

    /// How familiar this remembered person is now, counts not yet scored
    /// included; `None` for a name not remembered.
    pub fn familiar(&self, name: &str, now_unix_s: u64) -> Option<Familiar> {
        let person = self.people.iter().find(|person| person.name == name)?;
        let kept = person.familiar.map_or(0.0, |score| {
            faded(score, self.scored_at.unwrap_or(now_unix_s), now_unix_s)
        });
        let pending = self
            .unscored
            .iter()
            .filter(|counted| **counted == name)
            .count();
        Some(Familiar::of(kept + pending as f32))
    }

    fn save(&self, people: &[Person]) -> Result<()> {
        self.save_scored(people, self.scored_at)
    }

    fn save_scored(&self, people: &[Person], scored_at_unix_s: Option<u64>) -> Result<()> {
        let text = serde_json::to_vec(&Kept {
            version: 1,
            scored_at_unix_s,
            people: people.to_vec(),
        })?;
        let temporary = temporary_path(&self.path, ".new");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&text)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.path)?;
        self.advance_repair_revision();
        Ok(())
    }
}

fn temporary_path(path: &Path, suffix: &str) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(suffix);
    PathBuf::from(temporary)
}

/// Uncommitted copies are never adopted as records. Under the exclusive
/// writer, startup and forgetting unlink these exact entries without following
/// symlinks or traversing an unexpected directory. A failure is not completion.
fn discard_unfinished(path: &Path) -> io::Result<()> {
    for suffix in [".new", ".repair.new"] {
        match std::fs::remove_file(temporary_path(path, suffix)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// One exact sibling, included in the reset plan. The exclusive writer must
/// not overwrite a stale file or symlink; startup and forgetting unlink leftovers.
fn repair_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let temporary = temporary_path(path, ".repair.new");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    Ok((temporary, file))
}

/// What the voice service asks of the eyes on their request socket: one
/// line of JSON, `{"remember":"A"}`, `{"forget":"A"}`,
/// `{"list":true}` or `{"who":true}`. With one name for one face
/// (`LAMP_NAMES=checked`), `{"remember":"A","one_face":true}`, and
/// `"replace":true` once the person said yes to the new name, or
/// `"apart":true` once they said they are not the near one. Anything else
/// is nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    RememberSpeaker(crate::remember_speaker::Request),
    Remember(String),
    RememberOne {
        name: String,
        replace: bool,
        apart: bool,
    },
    /// Add the one face in a photo in the eyes' photo folder (`--photo-dir`,
    /// [`crate::photo`]), unconfirmed until its person says yes: its file
    /// name.
    RememberPhoto {
        name: String,
        file: String,
    },
    /// Whether a face from a photo, unconfirmed, is at the lamp now, and
    /// under which name: so the voice can ask them.
    PhotoHere,
    /// The voice has asked them, in this meeting.
    PhotoAsked(String),
    /// Their yes: kept as an ordinary print. With `LAMP_PHOTO_SPEAKER=on`
    /// (#2856), `face` is the passing id, under its boot (`boot/f3`), of the
    /// face the voice heard saying it: `None` with no `face` key,
    /// `Some(None)` with `"face":null`, so the switch off can refuse any
    /// `face` key as the parser before it did (Fourth's P3 on #3257).
    PhotoYes {
        name: String,
        face: Option<Option<String>>,
    },
    /// Their no: deleted.
    PhotoNo(String),
    /// Their yes to being named to others in Who was here (#1997), taken
    /// only from the known face at the lamp now.
    VisitsYes(String),
    /// Their no: not named to others any more.
    VisitsNo(String),
    Forget(String),
    List,
    /// The names, and with `--photo-dir` the faces from photos that wait
    /// for their person's yes: only lamp-cloud asks this, for the owner's
    /// page (`face.list`, #2192). The plain list never names those.
    ListWaiting,
    Who,
}

pub fn ask_of(line: &[u8]) -> Option<Ask> {
    // A distinct operation makes older Eyes refuse the guarded request,
    // rather than silently treating an unknown proof as an ordinary save.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Speaker {
        remember_speaker: crate::remember_speaker::Request,
    }
    if let Ok(Speaker { remember_speaker }) = serde_json::from_slice(line) {
        return Some(Ask::RememberSpeaker(remember_speaker));
    }
    // A face from a photo's consent (`--photo-dir`): one of these alone.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Consent {
        photo_here: Option<bool>,
        photo_asked: Option<String>,
        photo_yes: Option<String>,
        photo_no: Option<String>,
        visits_yes: Option<String>,
        visits_no: Option<String>,
        /// Absent is `None`, `null` is `Some(None)`: only a yes may carry
        /// it, even as `null`.
        #[serde(default, deserialize_with = "present")]
        face: Option<Option<String>>,
    }
    fn present<'de, D: serde::Deserializer<'de>>(
        given: D,
    ) -> std::result::Result<Option<Option<String>>, D::Error> {
        Option::<String>::deserialize(given).map(Some)
    }
    if let Ok(consent) = serde_json::from_slice::<Consent>(line) {
        return match consent {
            Consent {
                photo_here: Some(true),
                photo_asked: None,
                photo_yes: None,
                photo_no: None,
                visits_yes: None,
                visits_no: None,
                face: None,
            } => Some(Ask::PhotoHere),
            Consent {
                photo_here: None,
                photo_asked: Some(name),
                photo_yes: None,
                photo_no: None,
                visits_yes: None,
                visits_no: None,
                face: None,
            } => Some(Ask::PhotoAsked(name)),
            Consent {
                photo_here: None,
                photo_asked: None,
                photo_yes: Some(name),
                photo_no: None,
                visits_yes: None,
                visits_no: None,
                face,
            } => Some(Ask::PhotoYes { name, face }),
            Consent {
                photo_here: None,
                photo_asked: None,
                photo_yes: None,
                photo_no: Some(name),
                visits_yes: None,
                visits_no: None,
                face: None,
            } => Some(Ask::PhotoNo(name)),
            Consent {
                photo_here: None,
                photo_asked: None,
                photo_yes: None,
                photo_no: None,
                visits_yes: Some(name),
                visits_no: None,
                face: None,
            } => Some(Ask::VisitsYes(name)),
            Consent {
                photo_here: None,
                photo_asked: None,
                photo_yes: None,
                photo_no: None,
                visits_yes: None,
                visits_no: Some(name),
                face: None,
            } => Some(Ask::VisitsNo(name)),
            _ => None,
        };
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ListWaiting {
        list: bool,
        waiting: bool,
    }
    if let Ok(ListWaiting {
        list: true,
        waiting: true,
    }) = serde_json::from_slice(line)
    {
        return Some(Ask::ListWaiting);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Line {
        remember: Option<String>,
        forget: Option<String>,
        list: Option<bool>,
        who: Option<bool>,
        one_face: Option<bool>,
        replace: Option<bool>,
        apart: Option<bool>,
        photo: Option<String>,
    }
    let line: Line = serde_json::from_slice(line).ok()?;
    match line {
        Line {
            remember: Some(name),
            forget: None,
            list: None,
            who: None,
            one_face: None,
            replace: None,
            apart: None,
            photo: None,
        } => Some(Ask::Remember(name)),
        Line {
            remember: Some(name),
            forget: None,
            list: None,
            who: None,
            one_face: Some(true),
            replace,
            apart,
            photo: None,
        } => Some(Ask::RememberOne {
            name,
            replace: replace.unwrap_or(false),
            apart: apart.unwrap_or(false),
        }),
        Line {
            remember: None,
            forget: Some(name),
            list: None,
            who: None,
            one_face: None,
            replace: None,
            apart: None,
            photo: None,
        } => Some(Ask::Forget(name)),
        Line {
            remember: None,
            forget: None,
            list: Some(true),
            who: None,
            one_face: None,
            replace: None,
            apart: None,
            photo: None,
        } => Some(Ask::List),
        Line {
            remember: None,
            forget: None,
            list: None,
            who: Some(true),
            one_face: None,
            replace: None,
            apart: None,
            photo: None,
        } => Some(Ask::Who),
        Line {
            remember: Some(name),
            forget: None,
            list: None,
            who: None,
            one_face: None,
            replace: None,
            apart: None,
            photo: Some(file),
        } => Some(Ask::RememberPhoto { name, file }),
        _ => None,
    }
}

/// A face this tall, as a share of the picture's height, is someone at the
/// lamp: people at it were 0.12 to 0.27 on lamp-4ace. Smaller faces are not
/// recognised and not remembered.
pub const NEAR_ENOUGH: f32 = 0.10;
/// How long the lamp may take to remember a face, and how many prints it
/// takes of it.
pub const TAKING_FOR_US: u64 = 4_000_000;
/// With no face in view at all this long, nobody is there to remember: the
/// lamp says so at once instead of looking for four seconds. On lamp-4ace on
/// 2026-10-15 the model waited 4.2 s for "I could not see a face".
pub const NOBODY_FOR_US: u64 = 1_500_000;

/// Whose the near face is, asked at most this often: a print takes a
/// tenth of a second or more on the lamp.
pub const RECOGNISE_EVERY: Duration = Duration::from_secs(2);

/// Off unless `LAMP_FACE_FULL_CROP=on` on lamp-eyes: faces are recognised
/// and remembered from the frame at its own size ([`align_full`]), and the
/// known person is held through a short miss ([`Held`]).
pub const FULL_CROP_ENVIRONMENT: &str = "LAMP_FACE_FULL_CROP";

pub fn full_crop_on() -> bool {
    std::env::var(FULL_CROP_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// With [`FULL_CROP_ENVIRONMENT`] on, a known person stays known through
/// checks that find nobody for this long after they last matched: one
/// missed check (they come every [`RECOGNISE_EVERY`]) keeps them, a second
/// lets them go. On lamp-4ace the same face scored either side of
/// [`SAME_PERSON`] look to look, and each miss took their name away.
pub const KNOWN_HOLD: Duration = Duration::from_secs(4);

/// Within the hold, a printed near face that matched nobody keeps the held
/// name only if it is at least this alike to that person's own prints. The
/// reviewers on #3285: the same person's flicker sat at 0.34 to 0.36 against
/// [`SAME_PERSON`] (0.363), and a stranger scores lower; 0.30 keeps that
/// flicker with some room and lets anyone further off go. To be tuned from
/// the journal's score line.
pub const HOLD_LIKENESS: f32 = 0.30;

/// When the known person last matched, to hold them through a short miss.
/// Off, nobody is ever held: each miss lets go, as before.
#[derive(Debug)]
pub struct Held {
    on: bool,
    matched_at: Option<Instant>,
    /// The near face's passing id at that match, while speaking is judged.
    matched_id: Option<String>,
}

impl Held {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            matched_at: None,
            matched_id: None,
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// A check matched someone, whose near face had this passing id.
    pub fn matched(&mut self, now: Instant, id: Option<String>) {
        self.matched_at = Some(now);
        self.matched_id = id;
    }

    /// Whether a check that found nobody keeps the one known: only within
    /// [`KNOWN_HOLD`] of their last match.
    pub fn keeps(&self, now: Instant) -> bool {
        self.on
            && self
                .matched_at
                .is_some_and(|at| now.saturating_duration_since(at) < KNOWN_HOLD)
    }

    /// Whether a check that printed a near face matching nobody keeps the
    /// one known: within the hold, only while that face is still at least
    /// [`HOLD_LIKENESS`] to the held person's prints (`likeness`, `None` if
    /// they are no longer kept) and, when the match had a passing id, has
    /// the same one. A stranger sitting down is let go at once.
    pub fn keeps_print(&self, now: Instant, likeness: Option<f32>, id: Option<&str>) -> bool {
        self.keeps(now)
            && likeness.is_some_and(|likeness| likeness >= HOLD_LIKENESS)
            && self
                .matched_id
                .as_deref()
                .is_none_or(|matched| id == Some(matched))
    }

    /// Whether a check with no print keeps the one known: within the hold,
    /// only if no near face was seen (`near`), or the near face seen but not
    /// printed (turned, too small) has the passing id the match had. Another
    /// face, or one with no id to tell it by, lets them go at once.
    pub fn keeps_unprinted(&self, now: Instant, near: bool, id: Option<&str>) -> bool {
        self.keeps(now)
            && (!near
                || self
                    .matched_id
                    .as_deref()
                    .is_some_and(|matched| id == Some(matched)))
    }

    /// Let go at once: the camera closed, or the check let them go.
    pub fn let_go(&mut self) {
        self.matched_at = None;
        self.matched_id = None;
    }
}

/// Whether this look asks whose the near face is: at every look while
/// someone is being remembered, else once `RECOGNISE_EVERY` has passed.
pub fn recognise_due(looked_for: Option<Instant>, taking: bool, now: Instant) -> bool {
    taking || looked_for.is_none_or(|at| now.saturating_duration_since(at) >= RECOGNISE_EVERY)
}

/// A detector result before any still-face filtering. A skipped or failed
/// look is not fresh evidence that the room is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecognitionObservation {
    Skipped,
    Failed,
    Empty,
    Visible,
}

impl RecognitionObservation {
    pub fn detected(ran: bool, raw_faces: usize) -> Self {
        if !ran {
            Self::Skipped
        } else if raw_faces == 0 {
            Self::Empty
        } else {
            Self::Visible
        }
    }
}

/// Main's last observed permission hold. It catches expiry/regrant even
/// when both loop samples say the camera is currently permitted.
#[derive(Default)]
pub struct DisclosureHold {
    held: Option<u64>,
}

impl DisclosureHold {
    /// Observe the hold and reject a stale frame before publishing from it.
    /// `valid` is computed under Permit's lock from capture token/time; the
    /// caller keeps that lock through its bounded local publication.
    pub fn frame(
        &mut self,
        valid: bool,
        current: Option<u64>,
        publisher: Option<&KnownSpeakerPublisher>,
        known_file: Option<&Path>,
        known: &mut Option<(String, f32, Instant)>,
        held: &mut Held,
    ) -> bool {
        let changed = self.held != current;
        self.held = current;
        if changed || current.is_none() || !valid {
            if let Some(publisher) = publisher {
                publisher.camera_changed();
            }
            *known = None;
            held.let_go();
            if let Some(path) = known_file {
                keep_known_legacy(path, None, None, None);
            }
        }
        valid && current.is_some()
    }
}

/// Inputs at the actual loop boundary, before activity or identity printing.
pub struct RecognitionLook<'a> {
    pub observation: RecognitionObservation,
    /// Original capture time and the current host monotonic time. Publication
    /// must not turn a queued empty/returning frame into a fresh observation.
    pub frame_us: u64,
    pub now_us: u64,
    pub camera_open: bool,
    pub boot: Option<&'a str>,
    pub face: Option<&'a str>,
    pub looked_for: Option<Instant>,
    pub taking: bool,
    pub now: Instant,
}

/// Withdraw a real empty look immediately, even between recognition checks.
/// The returned decision is the loop's recognition gate. Only a successful
/// raw empty look preserves an absence epoch; filtering and failures do not.
/// The caller supplies a publisher only after checking its runtime storage.
pub fn begin_recognition_look(
    publisher: Option<&KnownSpeakerPublisher>,
    known_file: Option<&Path>,
    known: &mut Option<(String, f32, Instant)>,
    held: &mut Held,
    look: RecognitionLook<'_>,
) -> bool {
    if matches!(
        look.observation,
        RecognitionObservation::Empty | RecognitionObservation::Visible
    ) && !fresh_speaker_frame(look.frame_us, look.now_us)
    {
        if let Some(publisher) = publisher {
            publisher.revoke();
        }
        *known = None;
        held.let_go();
        if let Some(path) = known_file {
            keep_known_legacy(path, None, None, None);
        }
        return false;
    }
    match look.observation {
        RecognitionObservation::Skipped => return false,
        RecognitionObservation::Failed => {
            if let Some(publisher) = publisher {
                publisher.revoke();
            }
            return false;
        }
        RecognitionObservation::Empty => {
            // Both legacy readers and the visual hold must lose the old name.
            // Otherwise an unprinted same-ID return can republish that name.
            *known = None;
            held.let_go();
            if let Some(path) = known_file {
                keep_known_legacy(path, None, None, None);
            }
            if let Some(publisher) = publisher {
                if let Some(boot) = look.boot.filter(|_| look.camera_open) {
                    publisher.no_detection_at(boot, look.frame_us, look.now_us);
                } else {
                    publisher.revoke();
                }
            }
        }
        RecognitionObservation::Visible => {
            if let Some(publisher) = publisher {
                if !look.camera_open {
                    publisher.revoke();
                } else if publisher.candidate_at(
                    true,
                    look.boot,
                    look.face,
                    look.frame_us,
                    look.now_us,
                ) {
                    // Checking has already been written without any identity.
                    return true;
                }
            }
        }
    }
    recognise_due(look.looked_for, look.taking, look.now)
}

/// After a remembering at the lamp: kept, the next look asks at once, so the
/// known file names the person within one look, not two seconds later
/// (#3108). Refused or failed, the wait stands. A face from a photo is not
/// passed here: it is named only on its person's yes.
pub fn after_remembering(looked_for: &mut Option<Instant>, kept: bool) {
    if kept {
        *looked_for = None;
    }
}

/// Remembering someone: their prints, taken over a few looks while they are
/// the one face at the lamp.
#[derive(Debug)]
pub struct Taking {
    pub name: String,
    /// Present only for speaker-bound enrollment; checked before every
    /// accepted print and once more at the final store boundary.
    pub speaker: Option<crate::remember_speaker::Guard>,
    /// One name for one face, and whether to replace another name for it
    /// (`Ask::RememberOne`); `None`: as asked plainly.
    pub one_face: Option<bool>,
    /// Asked after the person said they are not the near one
    /// ([`Refused::NearTo`]): no near-name check.
    pub apart: bool,
    prints: Vec<Vec<f32>>,
    started_us: u64,
    saw_any: bool,
    saw_far: bool,
    saw_two: bool,
}

/// The name as it is kept, for a name asked in another case.
fn theirs_name(people: &[Person], name: &str) -> String {
    people
        .iter()
        .find(|person| same_name(&person.name, name))
        .map_or_else(|| name.to_owned(), |person| person.name.clone())
}

/// Why a face was not kept under a name, with one name for one face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    NotAName,
    /// The face is someone the lamp knows by this other name.
    KnownAs(String),
    /// The face is near someone known by a name one letter from this one,
    /// perhaps the same person misheard ([`People::remember_one`]).
    NearTo(String),
    /// The name is someone's the lamp knows, and this face isn't near theirs:
    /// refused whatever is asked, and nobody is named.
    NotLikeThem,
    /// The store refused it: full, or the file could not be written.
    Store(String),
}

/// Why a face could not be remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTaken {
    NoFace,
    TooFar,
    TwoFaces,
}

impl NotTaken {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoFace => "no_face",
            Self::TooFar => "too_far",
            Self::TwoFaces => "two_faces",
        }
    }
}

impl Taking {
    pub fn new(name: String, now_us: u64) -> Self {
        Self {
            name,
            speaker: None,
            one_face: None,
            apart: false,
            prints: Vec::new(),
            started_us: now_us,
            saw_any: false,
            saw_far: false,
            saw_two: false,
        }
    }

    /// One look: the heights of the faces in it, as shares of the
    /// picture's, and the print of the largest when there is one near.
    /// A print counts only when that face is the one face at the lamp.
    pub fn look(&mut self, heights: &[f32], print: Option<Vec<f32>>) {
        let near = heights
            .iter()
            .filter(|height| **height >= NEAR_ENOUGH)
            .count();
        self.saw_any |= !heights.is_empty();
        self.saw_far |= !heights.is_empty() && near == 0;
        self.saw_two |= near > 1;
        if near == 1
            && let Some(print) = print
            && self.prints.len() < MOST_PRINTS
        {
            self.prints.push(print);
        }
    }

    /// The real camera path: the optional speaker guard sees every fresh
    /// capture before the legacy sample collector can accept its print.
    pub fn look_seen(
        &mut self,
        heights: &[f32],
        print: Option<Vec<f32>>,
        face: Option<&str>,
        frame_us: u64,
        now_us: u64,
        hold: Option<u64>,
    ) {
        let accepts = self.speaker.as_mut().is_none_or(|guard| {
            guard.looked(
                face,
                print.as_deref(),
                heights.iter().filter(|h| **h >= NEAR_ENOUGH).count() == 1,
                frame_us,
                now_us,
                hold,
            )
        });
        if accepts {
            self.look(heights, print);
        }
    }

    /// The prints, once there are enough.
    pub fn taken(&mut self) -> Option<Vec<Vec<f32>>> {
        (self.prints.len() >= MOST_PRINTS).then(|| std::mem::take(&mut self.prints))
    }

    /// Why not, once the time is up without enough prints.
    pub fn given_up(&self, now_us: u64) -> Option<NotTaken> {
        let why = if self.saw_two {
            NotTaken::TwoFaces
        } else if self.saw_far {
            NotTaken::TooFar
        } else {
            NotTaken::NoFace
        };
        let looked_for = now_us.saturating_sub(self.started_us);
        let enough = self.prints.len() >= MOST_PRINTS;
        let nobody = !self.saw_any && looked_for >= NOBODY_FOR_US;
        (!enough && (looked_for >= TAKING_FOR_US || nobody)).then_some(why)
    }
}

/// The set-aside folder beside the people file at `path`: made 0700, or
/// set so if it is there; a link or anything else that is no folder is
/// refused, never followed.
fn set_aside_folder(path: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let folder = path.parent().unwrap_or(Path::new("/")).join(SET_ASIDE);
    match std::fs::symlink_metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(_) => {
            return Err(io::Error::other(format!(
                "{} is not a folder",
                folder.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new().mode(0o700).create(&folder)?;
        }
        Err(error) => return Err(error),
    }
    Ok(folder)
}

/// The name a set-aside record is kept under: `people-<why>-<sha256 of its
/// bytes, 16 hex>.json`. The same bytes set aside twice (a crash, or a
/// failed save back, before the rest were saved) are one file, so nothing
/// is counted twice.
fn set_aside_name(folder: &Path, why: &str, bytes: &[u8]) -> PathBuf {
    let why: String = why
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let digest = Sha256::digest(bytes);
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    folder.join(format!("people-{why}-{hex}.json"))
}

/// A rewrite's temporary file in the set-aside folder (`….json.new`), which a
/// crash may leave: never counted or searched, so it neither doubles the
/// count nor makes every later `forget` an error.
fn is_temporary(file: &Path) -> bool {
    file.extension().is_some_and(|extension| extension == "new")
}

/// Whether `file` in the set-aside folder is people's records: not a
/// rewrite's temporary file, nor the forgets that wait ([`PENDING_FORGETS`]).
fn is_a_record(file: &Path) -> bool {
    !is_temporary(file)
        && file
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.starts_with(PENDING_FORGETS))
}

/// A name as the forgets that wait keep it: a plain SHA-256 of its lower
/// case, not the name. It hides the name from a glance at the file, not from
/// someone who tries the household's few names, and it is kept only beside
/// the set-aside files, which hold names in plain text anyway (Twelve's P3
/// on #1767).
fn name_hash(name: &str) -> String {
    let digest = Sha256::digest(name.trim().to_lowercase().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The forgets that wait beside the people file at `path`
/// ([`PENDING_FORGETS`]); none when there is no such file. One from a
/// later build reads past what it doesn't know (#1680). One that can't be
/// read at all is set aside beside it, kept as it is with its mode
/// (`forgets-pending.unreadable-<unix s>`), with a line that counts only:
/// every later `forget` would otherwise fail (#2052's audit). A set-aside
/// list that can be read again, by the build that wrote it, is merged back.
/// The reset empties the folder with it.
fn pending(path: &Path) -> io::Result<Vec<Waiting>> {
    let folder = path.parent().unwrap_or(Path::new("/")).join(SET_ASIDE);
    let file = folder.join(PENDING_FORGETS);
    let mut waiting = match std::fs::read(&file) {
        Ok(bytes) => match read_waits(&bytes) {
            Some(waiting) => waiting,
            None => {
                set_pending_aside(&file, bytes.len())?;
                Vec::new()
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    // A list set aside before that can be read now (the build that wrote
    // it, back after a step back): its waits come back, so a face read back
    // from a set-aside file is still forgotten (the reviewer on #2070). One
    // still unreadable stays, with no line each start.
    let mut merged = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name
                .to_str()
                .is_some_and(|name| name.starts_with(&format!("{PENDING_FORGETS}.unreadable-")))
            {
                continue;
            }
            let Some(waits) = std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| read_waits(&bytes))
            else {
                continue;
            };
            for wait in waits {
                // Asked twice, it waits from the later word.
                match waiting.iter_mut().find(|kept| kept.hash == wait.hash) {
                    Some(kept) => kept.at_unix_s = kept.at_unix_s.max(wait.at_unix_s),
                    None => waiting.push(wait),
                }
            }
            merged.push(entry.path());
        }
    }
    if !merged.is_empty() {
        keep_pending(path, &waiting)?;
        for file in &merged {
            std::fs::remove_file(file)?;
        }
        eprintln!(
            "lamp-eyes: {} set-aside lists of the forgets that wait could be read now; their forgets wait again",
            merged.len()
        );
    }
    Ok(waiting)
}

/// The forgets that wait in a file's bytes, past any field this build
/// doesn't know; `None` when they can't be read.
fn read_waits(bytes: &[u8]) -> Option<Vec<Waiting>> {
    #[derive(Deserialize)]
    struct Kept {
        forget: Vec<Waiting>,
    }
    serde_json::from_slice::<Kept>(bytes)
        .ok()
        .map(|kept| kept.forget)
}

/// The forgets file this build can't read, set aside beside it as it is,
/// with its mode, and said with a line that counts only.
fn set_pending_aside(file: &Path, bytes: usize) -> io::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut aside = file.with_file_name(format!("{PENDING_FORGETS}.unreadable-{now}"));
    let mut n = 0;
    while aside.exists() {
        n += 1;
        aside = file.with_file_name(format!("{PENDING_FORGETS}.unreadable-{now}-{n}"));
    }
    std::fs::rename(file, &aside)?;
    eprintln!(
        "lamp-eyes: the forgets that wait could not be read ({bytes} bytes); set aside as {}, and forgetting goes on",
        aside.display()
    );
    Ok(())
}

/// The forgets that wait written whole, 0600, or the file removed when
/// there are none.
fn keep_pending(path: &Path, waiting: &[Waiting]) -> io::Result<()> {
    let folder = set_aside_folder(path)?;
    let file = folder.join(PENDING_FORGETS);
    if waiting.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => sync_folder(&folder),
        };
    }
    let temporary = folder.join(format!("{PENDING_FORGETS}.new"));
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?;
    out.write_all(&serde_json::to_vec(
        &serde_json::json!({ "forget": waiting }),
    )?)?;
    out.sync_all()?;
    std::fs::rename(&temporary, &file)?;
    sync_folder(&folder)
}

/// The folder synced, so what was set aside is there before the people
/// file is changed.
fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

/// The whole people file moved into the set-aside folder, as it was, 0600:
/// linked there and then unlinked, so nothing of that name is replaced. The
/// same bytes already set aside are not kept twice.
fn set_aside_whole(path: &Path, why: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let folder = set_aside_folder(path)?;
    let kept = set_aside_name(&folder, why, bytes);
    match std::fs::hard_link(path, &kept) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o600))?;
    sync_folder(&folder)?;
    std::fs::remove_file(path)?;
    Ok(kept)
}

/// `bytes` written whole into the set-aside folder, 0600, synced, and the
/// folder synced; nothing replaced, and the same bytes kept once.
fn write_set_aside(path: &Path, why: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let folder = set_aside_folder(path)?;
    let kept = set_aside_name(&folder, why, bytes);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&kept)
    {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    sync_folder(&folder)?;
    Ok(kept)
}

/// Whether a set-aside file's JSON holds nothing that could be a person:
/// `null`, a yes or no, a number, or an empty object or list (#2850). A
/// string, or an object or list with anything in it, might.
fn holds_nothing(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => true,
        serde_json::Value::String(_) => false,
        serde_json::Value::Array(list) => list.is_empty(),
        serde_json::Value::Object(fields) => fields.is_empty(),
    }
}

/// The entries that `matches`, by their name and when they were kept (none
/// when they say no time), taken out of every set-aside file with a list of
/// people: the file rewritten 0600, or removed once nothing is left.
/// Whether any were, and how many set-aside files could not be searched.
fn forget_set_aside(
    path: &Path,
    matches: impl Fn(&str, Option<u64>) -> bool,
) -> io::Result<(bool, usize)> {
    let folder = path.parent().unwrap_or(Path::new("/")).join(SET_ASIDE);
    match std::fs::symlink_metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(io::Error::other(format!(
                "{} is not a folder",
                folder.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((false, 0)),
        Err(error) => return Err(error),
    }
    let entries = std::fs::read_dir(&folder)?;
    let (mut forgot, mut unsearched) = (false, 0);
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() || !is_a_record(&entry.path()) {
            continue;
        }
        let file = entry.path();
        let mut bytes = Vec::new();
        File::open(&file)?
            .take(MOST_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            unsearched += 1;
            continue;
        };
        // JSON that holds nothing at all (`{}`, `[]`, a number) holds
        // nobody: searched, so it never keeps every forget waiting (#2850:
        // a 2-byte `people-not-a-list-of-people-….json` did on lamp-c0a4).
        // Anything else with no list of people might be a newer build's
        // shape, and still waits (#1586).
        if holds_nothing(&value) {
            continue;
        }
        let Some(list) = value
            .get_mut("people")
            .and_then(serde_json::Value::as_array_mut)
        else {
            unsearched += 1;
            continue;
        };
        // A record whose name is not a plain string cannot be told apart
        // by name: it might be this person (the reviewer on #1558).
        if list
            .iter()
            .any(|person| !person.get("name").is_some_and(serde_json::Value::is_string))
        {
            unsearched += 1;
        }
        let before = list.len();
        list.retain(|person| {
            !person
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| {
                    matches(
                        name,
                        person
                            .get("added_unix_s")
                            .and_then(serde_json::Value::as_u64),
                    )
                })
        });
        if list.len() == before {
            continue;
        }
        forgot = true;
        if list.is_empty() {
            std::fs::remove_file(&file)?;
        } else {
            let mut temporary = file.clone().into_os_string();
            temporary.push(".new");
            let temporary = PathBuf::from(temporary);
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temporary)?;
            out.write_all(&serde_json::to_vec(&value)?)?;
            out.sync_all()?;
            std::fs::rename(&temporary, &file)?;
        }
        sync_folder(&folder)?;
    }
    Ok((forgot, unsearched))
}

/// How many people's records are in the set-aside folder beside the people
/// file at `path`: each file's list, or one for a file that is no list.
/// What the eyes' health says while it is more than none (#1505).
pub fn set_aside_count(path: &Path) -> usize {
    let folder = path.parent().unwrap_or(Path::new("/")).join(SET_ASIDE);
    match std::fs::symlink_metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {}
        // Something that is no folder where the folder should be: said, as
        // one, never followed.
        Ok(_) => return 1,
        Err(_) => return 0,
    }
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| is_a_record(&entry.path()))
        .map(|entry| {
            let mut bytes = Vec::new();
            let read = File::open(entry.path())
                .and_then(|file| file.take(MOST_FILE_BYTES + 1).read_to_end(&mut bytes));
            read.ok()
                .and_then(|_| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .get("people")
                        .and_then(|list| list.as_array().map(Vec::len))
                })
                .unwrap_or(1)
        })
        .sum()
}

/// A new memory id for `name` (#2176): `person_id` of it, the voice's id for
/// the name before the stable id, unless another already has that (a new
/// "Minh" after the one before was renamed), then the same with the lowest
/// free `-N`.
fn fresh_id(name: &str, taken: &[String]) -> String {
    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
    lamp_msgs::known::unique_id(&lamp_msgs::known::person_id(name), &taken)
}

fn valid(people: &[Person]) -> bool {
    people.len() <= MOST_PEOPLE
        && people.iter().enumerate().all(|(index, person)| {
            plain_name(&person.name).as_deref() == Some(person.name.as_str())
                && (1..=MOST_PRINTS).contains(&person.prints.len())
                && person.prints.iter().all(|print| {
                    print.len() == PRINT && print.iter().all(|value| value.is_finite())
                })
                && person
                    .id
                    .as_deref()
                    .is_none_or(lamp_msgs::known::is_person_id)
                && !people[..index]
                    .iter()
                    .any(|other| same_name(&other.name, &person.name))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remember_kept_is_recognised_at_the_next_look_and_then_every_two_seconds() {
        let start = Instant::now();
        // The last look while remembering, then the reply.
        let mut looked_for = Some(start);
        after_remembering(&mut looked_for, true);
        let next = start + Duration::from_millis(350);
        assert!(recognise_due(looked_for, false, next));
        // That look asks once; the two seconds stand again after it.
        looked_for = Some(next);
        assert!(!recognise_due(
            looked_for,
            false,
            next + Duration::from_millis(350)
        ));
        assert!(recognise_due(looked_for, false, next + RECOGNISE_EVERY));
    }

    #[test]
    fn a_remember_refused_waits_the_two_seconds() {
        let start = Instant::now();
        let mut looked_for = Some(start);
        after_remembering(&mut looked_for, false);
        assert!(!recognise_due(
            looked_for,
            false,
            start + Duration::from_millis(350)
        ));
        assert!(recognise_due(looked_for, false, start + RECOGNISE_EVERY));
        // While remembering, every look asks.
        assert!(recognise_due(
            looked_for,
            true,
            start + Duration::from_millis(350)
        ));
    }

    /// A picture whose red is the column, green the row, and blue zero.
    fn gradient() -> Picture {
        let mut rgb = Vec::new();
        for y in 0..240u32 {
            for x in 0..320u32 {
                rgb.extend([(x % 256) as u8, (y % 256) as u8, 0]);
            }
        }
        Picture::from_rgb(&rgb, 320, 240).unwrap()
    }

    #[test]
    fn marks_where_the_reference_has_them_cut_out_the_corner_as_it_is() {
        let aligned = align(&gradient(), &REFERENCE).unwrap();
        for (row, column) in [(0, 0), (10, 90), (60, 56), (111, 111)] {
            let at = row * SIDE + column;
            assert!(
                (aligned[at] - column as f32).abs() < 0.01,
                "red at {row},{column}"
            );
            assert!(
                (aligned[SIDE * SIDE + at] - row as f32).abs() < 0.01,
                "green"
            );
            assert_eq!(aligned[2 * SIDE * SIDE + at], 0.0, "blue");
        }
    }

    #[test]
    fn a_face_twice_the_size_and_further_on_is_brought_to_the_reference() {
        // The same face, twice as large and 40 across, 20 down.
        let marks = REFERENCE.map(|(x, y)| (2.0 * x + 40.0, 2.0 * y + 20.0));
        let aligned = align(&gradient(), &marks).unwrap();
        for (row, column) in [(0usize, 0usize), (30, 70), (100, 100)] {
            let at = row * SIDE + column;
            let (x, y) = (2.0 * column as f32 + 40.0, 2.0 * row as f32 + 20.0);
            if x <= 319.0 && y <= 239.0 {
                assert!(
                    (aligned[at] - x).abs() < 0.05,
                    "red at {row},{column}: {}",
                    aligned[at]
                );
                assert!((aligned[SIDE * SIDE + at] - y).abs() < 0.05, "green");
            }
        }
        // A face turned a quarter round is turned back: the eyes level.
        let turned = REFERENCE.map(|(x, y)| (200.0 - y, x));
        let (a, b, _, _) = similarity(&turned, &REFERENCE).unwrap();
        assert!(a.abs() < 1e-4 && (b + 1.0).abs() < 1e-4, "{a} {b}");
        // Marks that are one point are no face.
        assert!(align(&gradient(), &[(5.0, 5.0); 5]).is_none());
    }

    /// A 1280 by 960 frame, as `--full-look 1280x960` gives, with its
    /// picture, as the look decodes them.
    fn full_frame(rgb_at: impl Fn(usize, usize) -> [u8; 3]) -> (Picture, Frame) {
        let (width, height) = (1_280, 960);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.extend(rgb_at(x, y));
            }
        }
        let picture = Picture::from_rgb(&rgb, width, height).unwrap();
        let scale = picture.filled.0 as f32 / width as f32;
        (
            picture,
            Frame {
                rgb,
                width,
                height,
                scale,
            },
        )
    }

    #[test]
    fn a_full_crop_reads_the_frame_where_the_shrunk_picture_has_the_face() {
        // Red rises across the frame and green down it, one step a 6 pixels.
        let (picture, frame) = full_frame(|x, y| [(x / 6) as u8, (y / 6) as u8, 0]);
        assert_eq!(picture.filled, (320, 240));
        // A face 28 pixels across in the picture: 112 in the frame.
        let marks = REFERENCE.map(|(x, y)| (x / 4.0 + 100.0, y / 4.0 + 80.0));
        let aligned = align_full(&frame, &marks).unwrap();
        for (row, column) in [(0usize, 0usize), (30, 70), (111, 111)] {
            let at = row * SIDE + column;
            // A picture pixel's centre, carried back to the frame.
            let (x, y) = (column as f32 + 401.5, row as f32 + 321.5);
            assert!((aligned[at] - x / 6.0).abs() < 1.0, "red at {row},{column}");
            assert!((aligned[SIDE * SIDE + at] - y / 6.0).abs() < 1.0, "green");
            assert_eq!(aligned[2 * SIDE * SIDE + at], 0.0, "blue");
        }
        // Not a frame: no face.
        let broken = Frame {
            rgb: vec![0; 10],
            ..frame
        };
        assert!(align_full(&broken, &marks).is_none());
    }

    #[test]
    fn a_full_crop_keeps_the_detail_the_shrunk_picture_drops() {
        // Stripes one pixel wide: every fourth column, which is all the
        // shrunk picture keeps, is the same, so its face is flat.
        let (picture, frame) = full_frame(|x, _| [if x % 2 == 0 { 0 } else { 255 }; 3]);
        // Placed so the face's pixels fall on the frame's, not between two.
        let marks = REFERENCE.map(|(x, y)| (x / 4.0 + 100.125, y / 4.0 + 80.0));
        let spread = |aligned: Vec<f32>| {
            let red = &aligned[..SIDE * SIDE];
            red.iter().copied().fold(f32::MIN, f32::max)
                - red.iter().copied().fold(f32::MAX, f32::min)
        };
        assert_eq!(spread(align(&picture, &marks).unwrap()), 0.0);
        assert!(spread(align_full(&frame, &marks).unwrap()) > 200.0);
    }

    #[test]
    fn a_held_person_stays_through_one_miss_and_goes_after_the_hold() {
        let start = Instant::now();
        let mut held = Held::new(true);
        // Nobody matched yet: nobody to hold.
        assert!(!held.keeps(start));
        held.matched(start, None);
        // The next check, two seconds on, finds nobody: still them.
        assert!(held.keeps(start + RECOGNISE_EVERY));
        // The check after that finds nobody: let go.
        assert!(!held.keeps(start + 2 * RECOGNISE_EVERY));
        assert!(!held.keeps(start + KNOWN_HOLD));
        // A match renews the hold.
        held.matched(start + 2 * RECOGNISE_EVERY, None);
        assert!(held.keeps(start + 3 * RECOGNISE_EVERY));
    }

    #[test]
    fn a_printed_face_keeps_the_held_name_only_if_it_still_looks_like_them() {
        let start = Instant::now();
        let then = start + RECOGNISE_EVERY;
        let mut held = Held::new(true);
        held.matched(start, Some("f3".to_owned()));
        // (a) No usable print: held.
        assert!(held.keeps(then));
        // (b) A stranger's print, far from the held person: let go, no name.
        assert!(!held.keeps_print(then, Some(0.12), Some("f3")));
        assert!(!held.keeps_print(then, Some(HOLD_LIKENESS - 0.001), Some("f3")));
        // The held person no longer kept: let go.
        assert!(!held.keeps_print(then, None, Some("f3")));
        // (c) Close to the held person, the same face: held.
        assert!(held.keeps_print(then, Some(0.35), Some("f3")));
        assert!(held.keeps_print(then, Some(HOLD_LIKENESS), Some("f3")));
        // (d) Another passing id, or none where the match had one: let go.
        assert!(!held.keeps_print(then, Some(0.35), Some("f4")));
        assert!(!held.keeps_print(then, Some(0.35), None));
        // (e) Past the hold: let go, however alike.
        assert!(!held.keeps_print(start + KNOWN_HOLD, Some(0.35), Some("f3")));
        // A match with no passing id (speaking not judged): likeness alone.
        held.matched(start, None);
        assert!(held.keeps_print(then, Some(0.35), Some("f9")));
        assert!(!held.keeps_print(then, Some(0.12), None));
    }

    #[test]
    fn an_unprinted_near_face_keeps_the_held_name_only_with_the_same_id() {
        let start = Instant::now();
        let then = start + RECOGNISE_EVERY;
        let mut held = Held::new(true);
        held.matched(start, Some("f3".to_owned()));
        // A near face seen but not printed, another passing id: no name.
        assert!(!held.keeps_unprinted(then, true, Some("f4")));
        assert!(!held.keeps_unprinted(then, true, None));
        // The same passing id: held.
        assert!(held.keeps_unprinted(then, true, Some("f3")));
        // No near face: held until the bound.
        assert!(held.keeps_unprinted(then, false, None));
        assert!(!held.keeps_unprinted(start + KNOWN_HOLD, false, None));
        assert!(!held.keeps_unprinted(start + KNOWN_HOLD, true, Some("f3")));
        // A match with no passing id: an unprinted near face lets go.
        held.matched(start, None);
        assert!(!held.keeps_unprinted(then, true, None));
        assert!(held.keeps_unprinted(then, false, None));
    }

    #[test]
    fn a_camera_closed_lets_a_held_person_go_at_once() {
        let start = Instant::now();
        let mut held = Held::new(true);
        held.matched(start, None);
        held.let_go();
        assert!(!held.keeps(start));
        assert!(!held.keeps(start + Duration::from_millis(1)));
    }

    #[test]
    fn without_the_switch_nobody_is_held() {
        let start = Instant::now();
        let mut held = Held::new(false);
        held.matched(start, None);
        assert!(!held.on());
        assert!(!held.keeps(start));
        assert!(!held.keeps(start + RECOGNISE_EVERY));
    }

    fn print(seed: f32) -> Vec<f32> {
        unit(
            (0..PRINT)
                .map(|i| ((i as f32 + 1.0) * seed).sin())
                .collect(),
        )
        .unwrap()
    }

    fn file(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("lamp-eyes-who-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("people.json")
    }

    fn repair_print(axis: usize) -> Vec<f32> {
        let mut print = vec![0.0; PRINT];
        print[axis] = 1.0;
        print
    }

    fn repair_people(label: &str) -> People {
        let mut people = People::open(file(label)).unwrap().with_stable_ids(true);
        people
            .remember("Fixture Original", vec![repair_print(0)], 101)
            .unwrap();
        people
            .remember_one("Fixture Target", vec![repair_print(0)], 202, true, false)
            .unwrap();
        people
            .remember("Fixture Other", vec![repair_print(1)], 303)
            .unwrap();
        people.people[0].familiar = Some(4.5);
        people.people[0].tell_visits = Some(true);
        people.people[1].tell_visits = Some(true);
        people.scored_at = Some(404);
        people.counted = vec![("Fixture Target".into(), 505)];
        people.unscored = vec!["Fixture Target".into()];
        people.save(&people.people).unwrap();
        people
    }

    #[test]
    fn repair_reserves_the_whole_record_then_replaces_only_prints_and_visit_consent() {
        use std::os::unix::fs::PermissionsExt;
        let mut people = repair_people("repair-preserves");
        let before = people.people.clone();
        let bytes = std::fs::read(&people.path).unwrap();
        let counted = people.counted.clone();
        let unscored = people.unscored.clone();
        let target = people.repair_target("fixture target").unwrap();
        assert_eq!(people.people, before, "staging removes no record");
        assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        assert_eq!(people.who(&repair_print(0)).unwrap().0, "Fixture Target");
        assert_eq!(
            people.remember_one("Fixture Target", vec![repair_print(2)], 606, true, true),
            Err(Refused::NotLikeThem),
            "the reserved name cannot be taken by ordinary remembering"
        );
        assert_eq!(
            people.id_of("Fixture Target"),
            Some(lamp_msgs::known::person_id("Fixture Original")),
            "the repair starts from a renamed person's original memory ID"
        );
        let untouched = people.path.with_file_name("unrelated-private-state");
        std::fs::write(&untouched, b"synthetic memory and rules marker").unwrap();
        let prints = vec![repair_print(2), repair_print(3)];
        people.replace_prints(target, prints.clone()).unwrap();
        let mut expected = before;
        expected[0].prints = prints;
        expected[0].tell_visits = None;
        assert_eq!(people.people, expected);
        assert_eq!(people.scored_at, Some(404));
        assert_eq!(people.counted, counted);
        assert_eq!(people.unscored, unscored);
        assert_eq!(people.tellable(), ["Fixture Other"]);
        assert_eq!(
            std::fs::read(&untouched).unwrap(),
            b"synthetic memory and rules marker"
        );
        let reopened = People::open(people.path.clone())
            .unwrap()
            .with_stable_ids(true);
        assert_eq!(reopened.people, expected);
        assert_eq!(reopened.scored_at, people.scored_at);
        assert_eq!(
            std::fs::metadata(&people.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn repair_never_allocates_an_id_for_a_legacy_record() {
        let mut people = People::open(file("repair-no-id"))
            .unwrap()
            .with_stable_ids(false);
        people
            .remember("Fixture Target", vec![repair_print(0)], 101)
            .unwrap();
        let target = people.repair_target("Fixture Target").unwrap();
        people
            .replace_prints(target, vec![repair_print(2)])
            .unwrap();
        assert_eq!(people.people[0].id, None);
        let kept: Kept = serde_json::from_slice(&std::fs::read(&people.path).unwrap()).unwrap();
        assert_eq!(kept.people[0].id, None);
        assert_eq!(kept.people[0].name, "Fixture Target");
        assert_eq!(kept.people[0].added_unix_s, 101);
    }

    #[test]
    fn repair_targets_are_missing_or_bound_to_their_original_live_store() {
        let people = repair_people("repair-target-owner");
        assert!(people.repair_target("Fixture Missing").is_none());
        assert!(people.repair_target("not/a/name").is_none());
        let target = people.repair_target("Fixture Target").unwrap();
        let mut reopened = People::open(people.path.clone())
            .unwrap()
            .with_stable_ids(true);
        let bytes = std::fs::read(&people.path).unwrap();
        assert_eq!(
            reopened.replace_prints(target, vec![repair_print(2)]),
            Err(RepairError::TargetChanged)
        );
        assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        let target = people.repair_target("Fixture Target").unwrap();
        let mut elsewhere = repair_people("repair-target-elsewhere");
        assert_eq!(
            elsewhere.replace_prints(target, vec![repair_print(2)]),
            Err(RepairError::TargetChanged)
        );
    }

    #[test]
    fn repair_refuses_every_changed_target_field() {
        for field in ["name", "id", "prints", "added", "familiar", "visits"] {
            let mut people = repair_people(&format!("repair-changed-{field}"));
            let target = people.repair_target("Fixture Target").unwrap();
            let person = &mut people.people[0];
            match field {
                "name" => person.name = "Fixture Renamed".into(),
                "id" => person.id = Some(lamp_msgs::known::person_id("Fixture Different")),
                "prints" => person.prints = vec![repair_print(4)],
                "added" => person.added_unix_s += 1,
                "familiar" => person.familiar = Some(5.0),
                "visits" => person.tell_visits = None,
                _ => unreachable!(),
            }
            // Exercise the snapshot comparison itself, independently of the
            // save revision: a future in-memory metadata change must refuse too.
            let before = people.people.clone();
            let bytes = std::fs::read(&people.path).unwrap();
            assert_eq!(
                people.replace_prints(target, vec![repair_print(2)]),
                Err(RepairError::TargetChanged),
                "{field}"
            );
            assert_eq!(people.people, before);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        }
    }

    #[test]
    fn repair_refuses_deleted_renamed_and_deleted_then_recreated_targets() {
        for change in ["deleted", "renamed", "recreated"] {
            let mut people = repair_people(&format!("repair-{change}"));
            let target = people.repair_target("Fixture Target").unwrap();
            let original = people.people.clone();
            if change == "renamed" {
                people
                    .remember_one("Fixture Renamed", vec![repair_print(0)], 606, true, false)
                    .unwrap();
            } else {
                people.forget_at("Fixture Target", 606).unwrap();
                if change == "recreated" {
                    people.people = original;
                    people.save(&people.people).unwrap();
                }
            }
            let before = people.people.clone();
            let bytes = std::fs::read(&people.path).unwrap();
            assert_eq!(
                people.replace_prints(target, vec![repair_print(2)]),
                Err(RepairError::TargetChanged),
                "{change}"
            );
            assert_eq!(people.people, before);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        }
    }

    #[test]
    fn repair_cas_rejects_a_second_snapshot_and_an_intervening_save_even_if_restored() {
        let mut people = repair_people("repair-cas");
        let first = people.repair_target("Fixture Target").unwrap();
        let second = people.repair_target("Fixture Target").unwrap();
        people.replace_prints(first, vec![repair_print(2)]).unwrap();
        let bytes = std::fs::read(&people.path).unwrap();
        assert_eq!(
            people.replace_prints(second, vec![repair_print(3)]),
            Err(RepairError::TargetChanged)
        );
        assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        let target = people.repair_target("Fixture Target").unwrap();
        let before = people.people.clone();
        people.tell_visits("Fixture Target", true).unwrap();
        people.tell_visits("Fixture Target", false).unwrap();
        assert_eq!(people.people, before);
        assert_eq!(
            people.replace_prints(target, vec![repair_print(4)]),
            Err(RepairError::TargetChanged),
            "a change and restoration cannot revive a staged repair"
        );
    }

    #[test]
    fn repair_rejects_invalid_embeddings_without_changing_memory_or_disk() {
        let mut nan = repair_print(2);
        nan[3] = f32::NAN;
        let mut infinite = repair_print(2);
        infinite[3] = f32::INFINITY;
        let mut negative_infinite = repair_print(2);
        negative_infinite[3] = f32::NEG_INFINITY;
        let mut scaled = repair_print(2);
        scaled[2] = 0.1;
        let mut people = repair_people("repair-invalid");
        let before = people.people.clone();
        let bytes = std::fs::read(&people.path).unwrap();
        for prints in [
            vec![],
            vec![repair_print(2); MOST_PRINTS + 1],
            vec![vec![1.0; PRINT - 1]],
            vec![vec![1.0; PRINT + 1]],
            vec![nan],
            vec![infinite],
            vec![negative_infinite],
            vec![vec![0.0; PRINT]],
            vec![vec![f32::MAX; PRINT]],
            vec![scaled],
        ] {
            let target = people.repair_target("Fixture Target").unwrap();
            assert_eq!(
                people.replace_prints(target, prints),
                Err(RepairError::InvalidPrints)
            );
            assert_eq!(people.people, before);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        }
    }

    #[test]
    fn repair_rejects_any_print_matching_another_person_even_when_the_target_matches_better() {
        let mut people = repair_people("repair-other-person");
        let before = people.people.clone();
        let bytes = std::fs::read(&people.path).unwrap();
        let mut shared = vec![0.0; PRINT];
        shared[0] = 0.8;
        shared[1] = 0.6;
        assert_eq!(people.who(&shared).unwrap().0, "Fixture Target");
        for prints in [
            vec![repair_print(1)],
            vec![repair_print(2), repair_print(1)],
            vec![shared],
        ] {
            let target = people.repair_target("Fixture Target").unwrap();
            assert_eq!(
                people.replace_prints(target, prints),
                Err(RepairError::KnownElsewhere)
            );
            assert_eq!(people.people, before);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        }
    }

    #[test]
    fn repair_write_faults_leave_ram_matching_the_visible_old_or_new_file() {
        for step in [
            RepairStep::Write,
            RepairStep::SyncFile,
            RepairStep::Rename,
            RepairStep::SyncDirectory,
        ] {
            let mut people = repair_people(&format!("repair-fault-{step:?}"));
            let target = people.repair_target("Fixture Target").unwrap();
            let mut expected = people.people.clone();
            let changed = step == RepairStep::SyncDirectory;
            let prints = vec![repair_print(2)];
            if changed {
                expected[0].prints = prints.clone();
                expected[0].tell_visits = None;
            }
            let result = people.replace_prints_at(target, prints, |at| {
                if at == step {
                    Err(io::Error::other("injected repair boundary"))
                } else {
                    Ok(())
                }
            });
            assert_eq!(
                result,
                Err(if changed {
                    RepairError::Indeterminate
                } else {
                    RepairError::NotWritten
                })
            );
            assert_eq!(people.people, expected, "{step:?}");
            assert_eq!(
                std::fs::symlink_metadata(people.path.with_file_name("people.json.repair.new"))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::NotFound,
                "the exact repair temporary is gone before startup cleanup"
            );
            let reopened = People::open(people.path.clone())
                .unwrap()
                .with_stable_ids(true);
            assert_eq!(reopened.people, expected, "{step:?}");
            assert_eq!(
                reopened.id_of("Fixture Target"),
                Some(lamp_msgs::known::person_id("Fixture Original"))
            );
        }
    }

    #[test]
    fn repair_syncs_the_file_before_rename_and_the_directory_before_success() {
        let mut people = repair_people("repair-durable-order");
        let target = people.repair_target("Fixture Target").unwrap();
        let path = people.path.clone();
        let before = people.people.clone();
        let mut expected = before.clone();
        expected[0].prints = vec![repair_print(2)];
        expected[0].tell_visits = None;
        let mut reached = Vec::new();
        people
            .replace_prints_at(target, vec![repair_print(2)], |step| {
                reached.push(step);
                let visible: Kept = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                assert_eq!(
                    visible.people,
                    if step == RepairStep::SyncDirectory {
                        expected.clone()
                    } else {
                        before.clone()
                    }
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(
            reached,
            [
                RepairStep::Write,
                RepairStep::SyncFile,
                RepairStep::Rename,
                RepairStep::SyncDirectory
            ]
        );
        assert_eq!(people.people, expected);
    }

    #[test]
    fn open_discards_partial_and_complete_uncommitted_people_files_only() {
        for partial in [true, false] {
            let people = repair_people(&format!("repair-open-leftovers-{partial}"));
            let bytes = std::fs::read(&people.path).unwrap();
            let expected = people.people.clone();
            let mut staged = expected.clone();
            staged[0].prints = vec![repair_print(2)];
            let staged = serde_json::to_vec(&Kept {
                version: 1,
                scored_at_unix_s: people.scored_at,
                people: staged,
            })
            .unwrap();
            let unfinished = if partial {
                &staged[..staged.len() / 2]
            } else {
                &staged[..]
            };
            let directory = people.path.parent().unwrap();
            for name in ["people.json.new", "people.json.repair.new"] {
                std::fs::write(directory.join(name), unfinished).unwrap();
            }
            let untouched = directory.join("people.json.repair.new.keep");
            std::fs::write(&untouched, b"unrelated synthetic setting").unwrap();
            let reopened = People::open(people.path.clone()).unwrap();
            assert_eq!(reopened.people, expected);
            assert_eq!(reopened.scored_at, people.scored_at);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
            for name in ["people.json.new", "people.json.repair.new"] {
                assert!(std::fs::symlink_metadata(directory.join(name)).is_err());
            }
            assert_eq!(
                std::fs::read(untouched).unwrap(),
                b"unrelated synthetic setting"
            );
        }
    }

    #[test]
    fn forget_removes_complete_uncommitted_people_copies_without_reopening() {
        let mut people = repair_people("forget-uncommitted-copies");
        let bytes = std::fs::read(&people.path).unwrap();
        let other = people.people[1].clone();
        let other_id = people.id_of("Fixture Other");
        let scored_at = people.scored_at;
        let directory = people.path.parent().unwrap().to_owned();
        for name in ["people.json.new", "people.json.repair.new"] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join(name))
                .unwrap();
            file.write_all(&bytes).unwrap();
            let copy: Kept =
                serde_json::from_slice(&std::fs::read(directory.join(name)).unwrap()).unwrap();
            assert_eq!(copy.people, people.people);
        }
        let neighbor = directory.join("people.json.repair.new.keep");
        std::fs::write(&neighbor, b"unrelated synthetic setting").unwrap();

        assert_eq!(
            people.forget_at("Fixture Target", 606).unwrap(),
            Forgot {
                forgot: true,
                unsearched: 0
            }
        );
        // Check the actual forget result before open can clean anything up.
        for name in ["people.json.new", "people.json.repair.new"] {
            assert_eq!(
                std::fs::symlink_metadata(directory.join(name))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::NotFound
            );
        }
        let canonical: Kept =
            serde_json::from_slice(&std::fs::read(&people.path).unwrap()).unwrap();
        assert_eq!(canonical.people.as_slice(), std::slice::from_ref(&other));
        assert_eq!(people.people, [other]);
        assert_eq!(canonical.scored_at_unix_s, scored_at);
        assert_eq!(people.id_of("Fixture Other"), other_id);
        assert!(people.who(&repair_print(0)).is_none());
        assert_eq!(
            std::fs::read(neighbor).unwrap(),
            b"unrelated synthetic setting"
        );
    }

    #[test]
    fn forget_refuses_unremovable_temporary_before_mutating_the_store() {
        for name in ["people.json.new", "people.json.repair.new"] {
            let mut people = repair_people(&format!("forget-unremovable-{name}"));
            let original = people.people.clone();
            let bytes = std::fs::read(&people.path).unwrap();
            let counted = people.counted.clone();
            let unscored = people.unscored.clone();
            let unexpected = people.path.with_file_name(name);
            std::fs::create_dir(&unexpected).unwrap();
            let retained = unexpected.join("uncommitted-people");
            std::fs::write(&retained, &bytes).unwrap();
            let neighbor = people.path.with_file_name("people.json.repair.new.keep");
            std::fs::write(&neighbor, b"unrelated synthetic setting").unwrap();

            assert!(people.forget_at("Fixture Target", 606).is_err());
            assert_eq!(people.people, original);
            assert_eq!(people.counted, counted);
            assert_eq!(people.unscored, unscored);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
            assert_eq!(std::fs::read(retained).unwrap(), bytes);
            assert_eq!(
                std::fs::read(neighbor).unwrap(),
                b"unrelated synthetic setting"
            );
        }
    }

    #[test]
    fn forget_unlinks_temporary_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;
        let mut people = repair_people("forget-uncommitted-links");
        let other = people.people[1].clone();
        let directory = people.path.parent().unwrap().to_owned();
        let neighbor = directory.join("unrelated");
        std::fs::write(&neighbor, b"unrelated synthetic setting").unwrap();
        for name in ["people.json.new", "people.json.repair.new"] {
            symlink(&neighbor, directory.join(name)).unwrap();
        }

        assert!(people.forget_at("Fixture Target", 606).unwrap().forgot);
        for name in ["people.json.new", "people.json.repair.new"] {
            assert_eq!(
                std::fs::symlink_metadata(directory.join(name))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::NotFound
            );
        }
        assert_eq!(people.people, [other]);
        assert_eq!(
            std::fs::read(neighbor).unwrap(),
            b"unrelated synthetic setting"
        );
    }

    #[test]
    fn open_discards_uncommitted_files_even_without_a_canonical_store() {
        let people = repair_people("repair-open-no-canonical");
        let bytes = std::fs::read(&people.path).unwrap();
        let directory = people.path.parent().unwrap();
        for name in ["people.json.new", "people.json.repair.new"] {
            std::fs::write(directory.join(name), &bytes).unwrap();
        }
        std::fs::remove_file(&people.path).unwrap();
        let reopened = People::open(people.path.clone()).unwrap();
        assert!(reopened.people.is_empty());
        assert!(!people.path.exists());
        for name in ["people.json.new", "people.json.repair.new"] {
            assert!(std::fs::symlink_metadata(directory.join(name)).is_err());
        }
    }

    #[test]
    fn open_unlinks_temporary_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;
        for kind in ["file", "directory", "missing"] {
            let people = repair_people(&format!("repair-open-link-{kind}"));
            let directory = people.path.parent().unwrap();
            let destination = directory.join("unrelated");
            let retained = if kind == "directory" {
                std::fs::create_dir(&destination).unwrap();
                destination.join("keep")
            } else {
                destination.clone()
            };
            if kind != "missing" {
                std::fs::write(&retained, b"unrelated synthetic setting").unwrap();
            }
            for name in ["people.json.new", "people.json.repair.new"] {
                symlink(&destination, directory.join(name)).unwrap();
            }
            let reopened = People::open(people.path.clone()).unwrap();
            assert_eq!(reopened.people, people.people);
            for name in ["people.json.new", "people.json.repair.new"] {
                assert!(std::fs::symlink_metadata(directory.join(name)).is_err());
            }
            if kind == "missing" {
                assert!(!destination.exists());
            } else {
                assert_eq!(
                    std::fs::read(retained).unwrap(),
                    b"unrelated synthetic setting"
                );
            }
        }
    }

    #[test]
    fn open_refuses_a_temporary_entry_it_cannot_unlink() {
        for name in ["people.json.new", "people.json.repair.new"] {
            for canonical in [true, false] {
                let people = repair_people(&format!("repair-open-refused-{name}-{canonical}"));
                let bytes = std::fs::read(&people.path).unwrap();
                if !canonical {
                    std::fs::remove_file(&people.path).unwrap();
                }
                // A directory is not an unfinished file. Do not broaden
                // startup cleanup to recursively delete its contents.
                let unexpected = people.path.with_file_name(name);
                std::fs::create_dir(&unexpected).unwrap();
                std::fs::write(unexpected.join("keep"), b"unexpected synthetic entry").unwrap();
                assert!(People::open(people.path.clone()).is_err());
                if canonical {
                    assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
                } else {
                    assert!(!people.path.exists());
                }
                assert_eq!(
                    std::fs::read(unexpected.join("keep")).unwrap(),
                    b"unexpected synthetic entry"
                );
            }
        }
    }

    #[test]
    fn repair_refuses_an_occupied_fixed_temporary_without_overwriting_it() {
        use std::os::unix::fs::symlink;
        for kind in ["file", "link", "directory"] {
            let mut people = repair_people(&format!("repair-occupied-{kind}"));
            let expected = people.people.clone();
            let bytes = std::fs::read(&people.path).unwrap();
            let temporary = people.path.with_file_name("people.json.repair.new");
            let retained = match kind {
                "file" => temporary.clone(),
                "link" => {
                    let target = people.path.with_file_name("unrelated");
                    symlink(&target, &temporary).unwrap();
                    target
                }
                "directory" => {
                    std::fs::create_dir(&temporary).unwrap();
                    temporary.join("keep")
                }
                _ => unreachable!(),
            };
            std::fs::write(&retained, b"uncommitted synthetic prints").unwrap();
            let target = people.repair_target("Fixture Target").unwrap();
            let mut attempted_write = false;
            assert_eq!(
                people.replace_prints_at(target, vec![repair_print(2)], |_| {
                    attempted_write = true;
                    Ok(())
                }),
                Err(RepairError::NotWritten)
            );
            assert!(!attempted_write);
            assert_eq!(people.people, expected);
            assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
            assert_eq!(
                std::fs::read(retained).unwrap(),
                b"uncommitted synthetic prints"
            );
            assert!(std::fs::symlink_metadata(temporary).is_ok());
        }
    }

    #[test]
    fn repair_refuses_a_temporary_mode_changed_before_rename() {
        use std::os::unix::fs::PermissionsExt;
        let mut people = repair_people("repair-mode-before-rename");
        let expected = people.people.clone();
        let bytes = std::fs::read(&people.path).unwrap();
        let temporary = people.path.with_file_name("people.json.repair.new");
        let target = people.repair_target("Fixture Target").unwrap();
        let mut checked = false;
        assert_eq!(
            people.replace_prints_at(target, vec![repair_print(2)], |step| {
                if step == RepairStep::Rename {
                    assert_eq!(
                        std::fs::metadata(&temporary)?.permissions().mode() & 0o7777,
                        0o600
                    );
                    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o644))?;
                    checked = true;
                }
                Ok(())
            }),
            Err(RepairError::NotWritten)
        );
        assert!(
            checked,
            "the fixed temporary was made private before the hook"
        );
        assert_eq!(people.people, expected);
        assert_eq!(std::fs::read(&people.path).unwrap(), bytes);
        assert!(std::fs::symlink_metadata(temporary).is_err());
    }

    /// #1997, the lead's yes: a remembered person's yes to being named to
    /// others is kept with their face and only theirs; a no takes it back;
    /// a new remembering under the same name starts without it; a forget
    /// takes it with them; the `tellable` file holds the yes names, 0600, and is gone with
    /// none. An older build's file, with no field, reads as no.
    #[test]
    fn a_yes_to_being_named_is_kept_with_the_face_and_goes_with_a_no_or_a_forget() {
        use std::os::unix::fs::PermissionsExt;
        let path = file("tell");
        let mut people = People::open(path.clone()).unwrap();
        people.remember("Alpha", vec![print(0.1)], 1).unwrap();
        people.remember("Beta", vec![print(0.7)], 1).unwrap();
        assert!(people.tellable().is_empty(), "nobody said yes yet");
        assert!(people.tell_visits("Alpha", true).unwrap());
        assert!(
            !people.tell_visits("Gamma", true).unwrap(),
            "not remembered"
        );
        assert_eq!(people.tellable(), ["Alpha"]);
        // Kept on disk, and only for Alpha.
        let reopened = People::open(path.clone()).unwrap();
        assert_eq!(reopened.tellable(), ["Alpha"]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("tell_visits").count(), 1, "{text}");
        // Someone else remembered under the same name (other prints): the
        // first Alpha's yes isn't theirs (the reviewer's P2 on #2098), nor is
        // it kept for the same face asked again: the voice asks again.
        people.remember("Alpha", vec![print(0.9)], 2).unwrap();
        assert!(people.tellable().is_empty(), "a new remembering has no yes");
        people.tell_visits("Alpha", true).unwrap();
        people.remember("Alpha", vec![print(0.9)], 3).unwrap();
        assert!(
            people.tellable().is_empty(),
            "nor the same face asked again"
        );
        people.tell_visits("Alpha", true).unwrap();
        // The file beside the known one.
        let known = path.with_file_name("known");
        let tellable = tellable_of(&known);
        assert_eq!(tellable, path.with_file_name("tellable"));
        keep_tellable(&tellable, &people.tellable());
        assert_eq!(std::fs::read_to_string(&tellable).unwrap(), "Alpha\n");
        assert_eq!(
            std::fs::metadata(&tellable).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // A no takes it back, and the file goes with the last name.
        assert!(people.tell_visits("Alpha", false).unwrap());
        assert!(people.tellable().is_empty());
        keep_tellable(&tellable, &people.tellable());
        assert!(!tellable.exists());
        // A forget takes it with them.
        people.tell_visits("Beta", true).unwrap();
        people.forget("Beta").unwrap();
        assert!(people.tellable().is_empty());
        // An older build's record, with no field: no.
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"people":[{{"name":"Delta","prints":[{:?}],"added_unix_s":1}}]}}"#,
                print(0.3)
            ),
        )
        .unwrap();
        assert!(People::open(path.clone()).unwrap().tellable().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_yes_or_no_to_being_named_is_asked_alone() {
        assert_eq!(
            ask_of(br#"{"visits_yes":"A"}"#),
            Some(Ask::VisitsYes("A".into()))
        );
        assert_eq!(
            ask_of(br#"{"visits_no":"A"}"#),
            Some(Ask::VisitsNo("A".into()))
        );
        assert_eq!(ask_of(br#"{"visits_yes":"A","visits_no":"A"}"#), None);
        assert_eq!(ask_of(br#"{"visits_yes":"A","photo_yes":"A"}"#), None);
    }

    /// The known name, and while speaking is judged its face's passing id
    /// beside it: both 0600, the face gone with no face, both gone with no
    /// name.
    #[test]
    fn the_known_face_is_kept_beside_the_known_name_and_goes_with_it() {
        use std::os::unix::fs::PermissionsExt;
        let known = file("known-face").with_file_name("known");
        let beside = known_face_of(&known);
        assert_eq!(beside.file_name().unwrap(), "known-face");
        keep_known(&known, Some("Lan"), None, Some("f3"));
        assert_eq!(std::fs::read_to_string(&known).unwrap(), "Lan\n");
        assert_eq!(std::fs::read_to_string(&beside).unwrap(), "f3\n");
        for path in [&known, &beside] {
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Speaking not judged: the name only, and no older face beside it.
        keep_known(&known, Some("Minh"), None, None);
        assert_eq!(std::fs::read_to_string(&known).unwrap(), "Minh\n");
        assert!(!beside.exists());
        keep_known(&known, Some("Minh"), None, Some("f4"));
        keep_known(&known, None, None, Some("f4"));
        assert!(!known.exists() && !beside.exists());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn raw_look_permission_refresh_keeps_identity_age_and_never_revives_expiry() {
        let known = file("permission-refresh").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.permission_until(Some(2_500_000));
        source.seen_at("Alpha", None, "0123456789abcdef", "f1", 1_000_000);
        let original = typed_source(&known);
        for now in [1_500_000u64, 2_000_000, 2_500_000, 3_000_000, 3_500_000] {
            source.permission_until(now.checked_add(1_500_000));
            assert!(source.refresh_authority(now));
            let refreshed = typed_source(&known);
            assert_eq!(refreshed["observed_us"], original["observed_us"]);
            assert_eq!(refreshed["generation"], original["generation"]);
            assert_eq!(refreshed["name"], original["name"]);
            assert_eq!(refreshed["permission_until_us"], now + 1_500_000);
        }
        source.permission_until(Some(5_500_000));
        assert!(
            !source.refresh_authority(4_000_000),
            "identity expires at equality"
        );
        assert_eq!(typed_source(&known)["state"], "revoked");
        source.seen_at("Alpha", None, "0123456789abcdef", "f1", 6_000_000);
        source.permission_until(Some(8_000_000));
        assert!(
            !source.refresh_authority(6_000_001),
            "a new permit cannot revive expired publication"
        );
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn failed_authority_refresh_disables_without_borrowing_through_write() {
        let known = file("permission-refresh-write-failure").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.permission_until(Some(2_500_000));
        source.seen_at("Alpha", None, "0123456789abcdef", "f1", 1_000_000);
        let path = known_speaker_of(&known);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        source.permission_until(Some(3_000_000));
        assert!(!source.refresh_authority(1_500_000));
        assert!(!source.enabled());
        assert!(source.subject.borrow().is_none());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn slow_recognition_keeps_capture_age_even_while_permission_is_renewed() {
        let known = file("permission-slow-recognition").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.permission_until(Some(5_500_000));
        source.seen_frame_at(
            "Alpha",
            None,
            "0123456789abcdef",
            "f1",
            (1_000_000, 3_999_999),
        );
        assert_eq!(typed_source(&known)["observed_us"], 1_000_000);
        source.seen_frame_at(
            "Alpha",
            None,
            "0123456789abcdef",
            "f1",
            (1_000_000, 4_000_000),
        );
        assert_eq!(typed_source(&known)["state"], "revoked");
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn stale_empty_and_checking_captures_revoke_before_publication() {
        const BOOT: &str = "0123456789abcdef";
        for observation in [
            RecognitionObservation::Empty,
            RecognitionObservation::Visible,
        ] {
            for frame_us in [0, 4_000_001, 1_000_000, 999_999] {
                let known =
                    file(&format!("stale-loop-{observation:?}-{frame_us}")).with_file_name("known");
                let source = KnownSpeakerPublisher::new(known.clone());
                source.permission_until(Some(5_500_000));
                source.seen_at("Alpha", None, BOOT, "f1", 1_000_000);
                let generation = typed_source(&known)["generation"].as_u64().unwrap();
                keep_known_legacy(&known, Some("Alpha"), None, Some("f1"));
                let now = Instant::now();
                let mut visual = Some(("Alpha".to_owned(), 1.0, now));
                let mut held = Held::new(true);
                held.matched(now, Some("f1".into()));
                assert!(!begin_recognition_look(
                    Some(&source),
                    Some(&known),
                    &mut visual,
                    &mut held,
                    RecognitionLook {
                        observation,
                        frame_us,
                        now_us: 4_000_000,
                        camera_open: true,
                        boot: Some(BOOT),
                        face: Some("f2"),
                        looked_for: Some(now),
                        taking: false,
                        now,
                    },
                ));
                let revoked = typed_source(&known);
                assert_eq!(revoked["state"], "revoked");
                assert!(revoked["generation"].as_u64().unwrap() > generation);
                assert!(revoked.get("name").is_none());
                assert!(visual.is_none() && !held.keeps(now));
                assert!(!known.exists() && !known_face_of(&known).exists());
                drop(source);
                std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
            }
        }
    }

    #[test]
    fn fresh_empty_and_checking_captures_keep_original_age_and_epoch() {
        const BOOT: &str = "0123456789abcdef";
        for observation in [
            RecognitionObservation::Empty,
            RecognitionObservation::Visible,
        ] {
            let known = file(&format!("fresh-loop-{observation:?}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            source.permission_until(Some(5_500_000));
            source.seen_at("Alpha", None, BOOT, "f1", 1_000_000);
            let generation = typed_source(&known)["generation"].clone();
            let now = Instant::now();
            let mut visual = Some(("Alpha".to_owned(), 1.0, now));
            let mut held = Held::new(true);
            held.matched(now, Some("f1".into()));
            let recognise = begin_recognition_look(
                Some(&source),
                Some(&known),
                &mut visual,
                &mut held,
                RecognitionLook {
                    observation,
                    frame_us: 1_000_001,
                    now_us: 4_000_000,
                    camera_open: true,
                    boot: Some(BOOT),
                    face: Some("f2"),
                    looked_for: Some(now),
                    taking: false,
                    now,
                },
            );
            assert_eq!(recognise, observation == RecognitionObservation::Visible);
            let published = typed_source(&known);
            assert_eq!(
                published["state"],
                if recognise { "checking" } else { "absent" }
            );
            assert_eq!(published["observed_us"], 1_000_001);
            assert_eq!(published["permission_until_us"], 5_500_000);
            assert_eq!(published["generation"], generation);
            for field in ["name", "id", "face"] {
                assert!(published.get(field).is_none());
            }
            drop(source);
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    fn typed_source(known: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(known_speaker_of(known)).unwrap()).unwrap()
    }

    #[test]
    fn a_short_raw_empty_look_withdraws_identity_before_a_same_id_return() {
        const BOOT: &str = "0123456789abcdef";
        for (returned, hold_on) in [("Beta", true), ("Alpha", true), ("Beta", false)] {
            let known =
                file(&format!("raw-empty-return-{returned}-{hold_on}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            let start = Instant::now();
            let mut ids = crate::bus_out::Ids::default();
            let bounds = [[0.2, 0.2, 0.6, 0.8]];
            let first = ids.of(&bounds, 1_000_000);
            let face = format!("f{}", first[0]);
            source.seen("Alpha", None, BOOT, &face);
            keep_known_legacy(&known, Some("Alpha"), None, Some(&face));
            let initial = typed_source(&known);
            let mut visual = Some(("Alpha".to_owned(), 1.0, start));
            let mut held = Held::new(hold_on);
            held.matched(start, Some(face.clone()));

            // The real tracker intentionally retains its ID through this gap.
            let empty = ids.of(&[], 1_250_000);
            assert!(empty.is_empty());
            let recognise = begin_recognition_look(
                Some(&source),
                Some(&known),
                &mut visual,
                &mut held,
                RecognitionLook {
                    frame_us: 1_000_000,
                    now_us: 1_000_000,
                    observation: RecognitionObservation::detected(true, empty.len()),
                    camera_open: true,
                    boot: Some(BOOT),
                    face: None,
                    looked_for: Some(start),
                    taking: false,
                    now: start + Duration::from_millis(250),
                },
            );
            assert!(!recognise, "absence must withdraw before the print is due");
            assert!(visual.is_none() && !held.keeps(start + Duration::from_millis(250)));
            assert!(!known.exists() && !known_face_of(&known).exists());
            let absent = typed_source(&known);
            assert_eq!(absent["state"], "absent");
            assert_eq!(absent["generation"], initial["generation"]);
            for field in ["name", "id", "face"] {
                assert!(absent.get(field).is_none());
            }

            let next = ids.of(&bounds, 1_500_000);
            assert_eq!(
                next, first,
                "reproduce the same passing ID, not a new track"
            );
            let returning_face = format!("f{}", next[0]);
            let recognise = begin_recognition_look(
                Some(&source),
                Some(&known),
                &mut visual,
                &mut held,
                RecognitionLook {
                    frame_us: 1_000_000,
                    now_us: 1_000_000,
                    observation: RecognitionObservation::detected(true, next.len()),
                    camera_open: true,
                    boot: Some(BOOT),
                    face: Some(&returning_face),
                    looked_for: Some(start),
                    taking: false,
                    now: start + Duration::from_millis(500),
                },
            );
            assert!(
                recognise,
                "a same-ID return forces recognition before two seconds"
            );
            let checking = typed_source(&known);
            assert_eq!(checking["state"], "checking");
            for field in ["name", "id", "face"] {
                assert!(
                    checking.get(field).is_none(),
                    "no identity before inference"
                );
            }
            assert!(!known.exists() && !known_face_of(&known).exists());
            assert!(visual.is_none() && !held.keeps_unprinted(start, true, Some(&face)));

            // A completed same-person match may resume the old epoch; a
            // different match must start a new one. No tracker identity is
            // used as the recognition result.
            source.seen(returned, None, BOOT, &returning_face);
            let seen = typed_source(&known);
            assert_eq!(seen["state"], "seen");
            assert_eq!(seen["name"], returned);
            assert_eq!(
                seen["generation"].as_u64().unwrap(),
                initial["generation"].as_u64().unwrap() + u64::from(returned != "Alpha")
            );
            drop(source);
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn a_continuous_visible_identity_keeps_the_existing_recognition_cadence() {
        const BOOT: &str = "0123456789abcdef";
        let known = file("continuous-recognition-cadence").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        let start = Instant::now();
        source.seen("Alpha", None, BOOT, "f1");
        keep_known_legacy(&known, Some("Alpha"), None, Some("f1"));
        let initial = typed_source(&known);
        let mut visual = Some(("Alpha".to_owned(), 1.0, start));
        let mut held = Held::new(true);
        held.matched(start, Some("f1".into()));
        for (after, due) in [(Duration::from_millis(250), false), (RECOGNISE_EVERY, true)] {
            assert_eq!(
                begin_recognition_look(
                    Some(&source),
                    Some(&known),
                    &mut visual,
                    &mut held,
                    RecognitionLook {
                        frame_us: 1_000_000,
                        now_us: 1_000_000,
                        observation: RecognitionObservation::detected(true, 1),
                        camera_open: true,
                        boot: Some(BOOT),
                        face: Some("f1"),
                        looked_for: Some(start),
                        taking: false,
                        now: start + after,
                    },
                ),
                due,
            );
            assert_eq!(typed_source(&known), initial);
            assert!(known.exists() && known_face_of(&known).exists());
            assert!(visual.is_some());
        }
        drop(source);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn skipped_failed_filtered_and_closed_looks_are_not_benign_absence() {
        const BOOT: &str = "0123456789abcdef";
        for (label, observation, camera_open, expected) in [
            (
                "skipped",
                RecognitionObservation::detected(false, 0),
                true,
                "seen",
            ),
            ("failed", RecognitionObservation::Failed, true, "revoked"),
            (
                "filtered",
                RecognitionObservation::detected(true, 1),
                true,
                "revoked",
            ),
            (
                "closed",
                RecognitionObservation::detected(true, 0),
                false,
                "revoked",
            ),
        ] {
            let known = file(&format!("non-absence-loop-{label}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            let start = Instant::now();
            source.seen("Alpha", None, BOOT, "f1");
            let initial = typed_source(&known);
            let mut visual = Some(("Alpha".to_owned(), 1.0, start));
            let mut held = Held::new(true);
            held.matched(start, Some("f1".into()));
            assert!(!begin_recognition_look(
                Some(&source),
                Some(&known),
                &mut visual,
                &mut held,
                RecognitionLook {
                    frame_us: 1_000_000,
                    now_us: 1_000_000,
                    observation,
                    camera_open,
                    boot: Some(BOOT),
                    face: None,
                    looked_for: Some(start),
                    taking: false,
                    now: start + Duration::from_millis(250),
                },
            ));
            let actual = typed_source(&known);
            assert_eq!(actual["state"], expected);
            if expected == "seen" {
                assert_eq!(actual, initial, "a skipped detector refreshes nothing");
            } else {
                assert!(actual.get("name").is_none());
                assert!(actual["generation"].as_u64() > initial["generation"].as_u64());
            }
            drop(source);
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn typed_no_detection_keeps_only_the_same_publisher_generation() {
        use std::os::unix::fs::PermissionsExt;
        let known = file("typed-absence").with_file_name("known");
        keep_known(&known, Some("Alpha"), None, Some("f3"));
        let source = KnownSpeakerPublisher::new(known.clone());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        let seen = typed_source(&known);
        assert_eq!(seen["state"], "seen");
        assert_eq!(seen["generation"], 1);
        assert_eq!(seen["name"], "Alpha");
        assert_eq!(seen["publisher"].as_str().unwrap().len(), 32);
        assert!(
            seen["publisher"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        source.no_detection("0123456789abcdef");
        let absent = typed_source(&known);
        assert_eq!(
            absent,
            serde_json::json!({
                "state":"absent", "boot":"0123456789abcdef",
                "publisher":seen["publisher"], "generation":1
            })
        );
        let source_bytes = std::fs::read(known_speaker_of(&known)).unwrap();
        let modified = std::fs::metadata(known_speaker_of(&known))
            .unwrap()
            .modified()
            .unwrap();
        assert!(!source.candidate(false, None, None));
        assert_eq!(
            std::fs::read(known_speaker_of(&known)).unwrap(),
            source_bytes
        );
        assert_eq!(
            std::fs::metadata(known_speaker_of(&known))
                .unwrap()
                .modified()
                .unwrap(),
            modified,
            "a skipped inference neither refreshes nor revokes typed status"
        );
        // Main alone owns typed source: clearing only legacy files preserves
        // this explicit status. Other legacy callers still fail closed.
        keep_known_legacy(&known, None, None, None);
        assert!(!known.exists());
        assert_eq!(typed_source(&known), absent);
        assert_eq!(
            std::fs::metadata(known_speaker_of(&known))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        keep_known(&known, None, None, None);
        assert!(!known_speaker_of(&known).exists());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn hard_source_invalidation_advances_before_any_same_subject_can_return() {
        let known = file("typed-revocation").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        let publisher = typed_source(&known)["publisher"].clone();
        source.revoke();
        let revoked = typed_source(&known);
        assert_eq!(
            revoked,
            serde_json::json!({
                "state":"revoked", "boot":"0123456789abcdef", "publisher":publisher, "generation":2
            })
        );
        source.revoke();
        assert!(
            !known_speaker_of(&known).exists(),
            "repeated hard clear also removes a foreign/legacy source"
        );
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        assert_eq!(typed_source(&known)["generation"], 2);
        source.no_detection("0123456789abcdef");
        source.revoke();
        assert_eq!(
            typed_source(&known)["generation"],
            3,
            "absence never hides a hard invalidation"
        );
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        assert_eq!(typed_source(&known)["generation"], 3);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn returning_candidate_is_checking_before_recognition_without_identity() {
        for (suffix, absent, face) in [
            ("same-id", true, "f3"),
            ("new-id", true, "f4"),
            ("changed-visible", false, "f4"),
        ] {
            let known = file(&format!("returning-checking-{suffix}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            let seen = typed_source(&known);
            assert!(
                !source.candidate(true, Some("0123456789abcdef"), Some("f3")),
                "a continuously seen unchanged track keeps the ordinary cadence"
            );
            assert_eq!(typed_source(&known), seen);
            if absent {
                source.no_detection("0123456789abcdef");
            }
            // This is the boundary before the caller runs any recognizer.
            assert!(
                source.candidate(true, Some("0123456789abcdef"), Some(face)),
                "a return must bypass even a just-completed recognition cadence"
            );
            assert_eq!(
                typed_source(&known),
                serde_json::json!({
                    "state":"checking", "boot":"0123456789abcdef",
                    "publisher":seen["publisher"], "generation":seen["generation"]
                }),
                "an unresolved returning candidate exposes no identity or absence grant"
            );
            source.seen("Alpha", None, "0123456789abcdef", face);
            let returned = typed_source(&known);
            assert_eq!(returned["state"], "seen");
            assert_eq!(returned["face"], face);
            assert_eq!(returned["publisher"], seen["publisher"]);
            assert_eq!(returned["generation"], seen["generation"]);
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn returning_unmatched_candidate_does_not_force_recognition_every_frame() {
        let known = file("returning-unmatched-cadence").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f3")));
        let checking = typed_source(&known);
        assert!(!source.candidate(true, Some("0123456789abcdef"), Some("f3")));
        assert_eq!(typed_source(&known), checking);
        source.revoke(); // The forced recognizer found no known person.
        let revoked = typed_source(&known);
        for _ in 0..8 {
            assert!(
                !source.candidate(true, Some("0123456789abcdef"), Some("f3")),
                "unmatched, unaligned and error outcomes retain the normal retry cadence"
            );
            assert_eq!(typed_source(&known), revoked);
        }
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f4")));
        source.revoke();
        source.no_detection("0123456789abcdef");
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f4")));
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn returning_unresolved_checking_cannot_restore_absence_in_the_old_epoch() {
        let known = file("returning-unresolved-absence").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        let original = typed_source(&known);
        source.no_detection("0123456789abcdef");
        assert_eq!(typed_source(&known)["generation"], original["generation"]);
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f4")));
        source.no_detection("0123456789abcdef");
        assert_eq!(
            typed_source(&known),
            serde_json::json!({
                "state":"absent", "boot":"0123456789abcdef",
                "publisher":original["publisher"],
                "generation":original["generation"].as_u64().unwrap() + 1
            }),
            "disappearing before a positive match cannot restore the old grant"
        );
        source.seen("Alpha", None, "0123456789abcdef", "f4");
        assert_ne!(typed_source(&known)["generation"], original["generation"]);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn returning_same_person_new_track_preserves_epoch_only_after_positive_match() {
        let known = file("returning-positive-id").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        let id = lamp_msgs::known::person_id("Alpha");
        source.seen("Alpha", Some(&id), "0123456789abcdef", "f3");
        let original = typed_source(&known);
        source.no_detection("0123456789abcdef");
        source.candidate(true, Some("0123456789abcdef"), Some("f4"));
        source.seen("Alpha", Some(&id), "0123456789abcdef", "f4");
        let returned = typed_source(&known);
        assert_eq!(returned["generation"], original["generation"]);
        assert_eq!(returned["publisher"], original["publisher"]);
        assert_eq!(returned["face"], "f4");
        assert_eq!(returned["id"], id);

        let other_id = lamp_msgs::known::person_id("Beta");
        source.seen("Alpha", Some(&other_id), "0123456789abcdef", "f4");
        assert_eq!(
            typed_source(&known)["generation"],
            original["generation"].as_u64().unwrap() + 1,
            "a display name does not conceal a changed stable person id"
        );
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn returning_other_or_unmatched_candidate_hard_revokes_the_retained_epoch() {
        for outcome in ["other", "unmatched", "privacy", "error"] {
            let known = file(&format!("returning-revoke-{outcome}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            let generation = typed_source(&known)["generation"].as_u64().unwrap();
            source.no_detection("0123456789abcdef");
            source.candidate(true, Some("0123456789abcdef"), Some("f4"));
            assert_eq!(typed_source(&known)["state"], "checking");
            if outcome == "other" {
                source.seen("Beta", None, "0123456789abcdef", "f4");
                assert_eq!(typed_source(&known)["state"], "seen");
            } else {
                // Main uses this same hard clear for failed recognition,
                // privacy/forget and inference errors, never no_detection.
                source.revoke();
                assert_eq!(typed_source(&known)["state"], "revoked");
            }
            assert_eq!(typed_source(&known)["generation"], generation + 1);
            source.seen("Alpha", None, "0123456789abcdef", "f4");
            assert!(
                typed_source(&known)["generation"].as_u64().unwrap() > generation,
                "a later positive match cannot restore the old epoch"
            );
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn another_candidate_person_or_publisher_boot_revokes_typed_continuity() {
        let known = file("typed-candidate").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        source.no_detection("0123456789abcdef");
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f3")));
        assert_eq!(typed_source(&known)["state"], "checking");
        assert!(source.candidate(true, Some("0123456789abcdef"), Some("f4")));
        assert_eq!(typed_source(&known)["state"], "checking");
        assert_eq!(typed_source(&known)["generation"], 1);
        source.seen("Beta", None, "0123456789abcdef", "f4");
        assert_eq!(typed_source(&known)["generation"], 2);
        source.seen("Alpha", None, "0123456789abcdef", "f4");
        assert_eq!(
            typed_source(&known)["generation"],
            3,
            "same geometry is not person authority"
        );
        source.no_detection("fedcba9876543210");
        assert_eq!(typed_source(&known)["generation"], 4);
        assert_eq!(typed_source(&known)["boot"], "fedcba9876543210");
        source.seen("Alpha", None, "fedcba9876543210", "f4");
        assert_eq!(typed_source(&known)["generation"], 4);
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_visible_candidate_and_unmatched_source_never_become_absence() {
        let known = file("typed-visible-uncertain").with_file_name("known");
        let source = KnownSpeakerPublisher::new(known.clone());
        for missing in [
            (None, Some("f3")),
            (Some("0123456789abcdef"), None),
            (Some("not-a-boot"), Some("f3")),
            (Some("0123456789abcdef"), Some("f0")),
            (Some("0123456789abcdef"), Some("f4294967296")),
        ] {
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            assert!(!source.candidate(true, missing.0, missing.1));
            assert_eq!(typed_source(&known)["state"], "revoked");
        }
        for invalid in ["", "Alpha\nBeta"] {
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            let generation = typed_source(&known)["generation"].as_u64().unwrap();
            source.seen(invalid, None, "0123456789abcdef", "f3");
            assert_eq!(typed_source(&known)["state"], "revoked");
            assert_eq!(typed_source(&known)["generation"], generation + 1);
        }
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        source.disable();
        assert!(
            !known_speaker_of(&known).exists(),
            "a failed producer cannot publish benign status"
        );
        source.no_detection("0123456789abcdef");
        assert!(!known_speaker_of(&known).exists());
        assert!(!source.candidate(true, Some("0123456789abcdef"), Some("f4")));
        assert!(!known_speaker_of(&known).exists());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn typed_source_entropy_failure_and_generation_overflow_disable_without_revival() {
        let known = file("typed-disabled").with_file_name("known");
        keep_known_speaker(
            &known,
            Some("Alpha"),
            None,
            Some("0123456789abcdef"),
            Some("f3"),
        );
        let no_entropy = KnownSpeakerPublisher::with_random(known.clone(), |_| Err(()));
        assert!(!known_speaker_of(&known).exists());
        no_entropy.seen("Alpha", None, "0123456789abcdef", "f3");
        no_entropy.no_detection("0123456789abcdef");
        assert!(!known_speaker_of(&known).exists());
        drop(no_entropy);
        let source = KnownSpeakerPublisher::new(known.clone());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        source.generation.set(u64::MAX);
        source.revoke();
        assert!(!known_speaker_of(&known).exists());
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        assert!(!known_speaker_of(&known).exists());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn typed_source_write_failure_cannot_restore_the_previous_generation() {
        for state in ["absent", "checking"] {
            let known = file(&format!("typed-write-failure-{state}")).with_file_name("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            let temporary = speaker_temporary(&known_speaker_of(&known));
            std::fs::create_dir(&temporary).unwrap();
            if state == "absent" {
                source.no_detection("0123456789abcdef");
            } else {
                assert!(!source.candidate(true, Some("0123456789abcdef"), Some("f4")));
            }
            assert!(!known_speaker_of(&known).exists());
            std::fs::remove_dir(&temporary).unwrap();
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            assert!(
                !known_speaker_of(&known).exists(),
                "repairing storage must not revive a failed publisher"
            );
            std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn typed_source_new_lifecycle_uses_new_entropy_and_never_follows_old_links() {
        use std::os::unix::fs::symlink;
        let known = file("typed-lifecycle").with_file_name("known");
        let old = {
            let source = KnownSpeakerPublisher::new(known.clone());
            source.seen("Alpha", None, "0123456789abcdef", "f3");
            typed_source(&known)["publisher"].clone()
        };
        assert!(!known_speaker_of(&known).exists());
        let outside = known.with_file_name("outside");
        std::fs::write(&outside, b"sentinel").unwrap();
        symlink(&outside, known_speaker_of(&known)).unwrap();
        let source = KnownSpeakerPublisher::new(known.clone());
        assert!(!known_speaker_of(&known).exists());
        symlink(&outside, speaker_temporary(&known_speaker_of(&known))).unwrap();
        source.seen("Alpha", None, "0123456789abcdef", "f3");
        assert_ne!(typed_source(&known)["publisher"], old);
        assert_eq!(typed_source(&known)["generation"], 1);
        assert_eq!(std::fs::read(outside).unwrap(), b"sentinel");
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_known_speaker_is_one_private_source_observation_and_keeps_legacy_files() {
        use std::os::unix::fs::PermissionsExt;
        let known = file("known-speaker-source").with_file_name("known.custom");
        let path = known_speaker_of(&known);
        assert_eq!(path.file_name().unwrap(), "known.custom-speaker");
        let id = lamp_msgs::known::person_id("Alpha");
        keep_known(&known, Some("Alpha"), Some(&id), Some("f3"));
        let legacy = std::fs::read(&known).unwrap();
        let legacy_face = std::fs::read(known_face_of(&known)).unwrap();
        keep_known_speaker(
            &known,
            Some("Alpha"),
            Some(&id),
            Some("0123456789abcdef"),
            Some("f3"),
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
            serde_json::json!({"name": "Alpha", "id": id, "boot": "0123456789abcdef", "face": "f3"})
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Replacement is a whole new observation, including an explicit null id.
        keep_known_speaker(
            &known,
            Some("Beta"),
            None,
            Some("fedcba9876543210"),
            Some("f4"),
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),
            serde_json::json!({"name": "Beta", "id": null, "boot": "fedcba9876543210", "face": "f4"})
        );
        assert_eq!(std::fs::read(&known).unwrap(), legacy);
        assert_eq!(std::fs::read(known_face_of(&known)).unwrap(), legacy_face);
        assert!(!path.with_file_name("known.custom-speaker.new").exists());
        keep_known(&known, None, None, None);
        assert!(
            !path.exists(),
            "clearing the known name retained its speaker proof"
        );
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_interrupted_known_speaker_write_never_reuses_a_temporary_or_old_proof() {
        let known = file("known-speaker-write-failure").with_file_name("known");
        let path = known_speaker_of(&known);
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".new");
        let temporary = PathBuf::from(temporary);
        keep_known_speaker(
            &known,
            Some("Alpha"),
            None,
            Some("0123456789abcdef"),
            Some("f3"),
        );
        assert!(path.exists());
        // A leftover regular temporary is replaced, never read as a source.
        std::fs::write(&temporary, b"interrupted").unwrap();
        keep_known_speaker(
            &known,
            Some("Beta"),
            None,
            Some("fedcba9876543210"),
            Some("f4"),
        );
        let source: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(source["name"], "Beta");
        assert!(!temporary.exists());
        // A temporary that cannot be removed/opened fails closed.
        std::fs::create_dir(&temporary).unwrap();
        keep_known_speaker(
            &known,
            Some("Alpha"),
            None,
            Some("0123456789abcdef"),
            Some("f3"),
        );
        assert!(!path.exists());
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_known_speaker_is_removed_for_missing_or_invalid_source_fields() {
        let known = file("known-speaker-invalid").with_file_name("known");
        let path = known_speaker_of(&known);
        let valid = (Some("Alpha"), None, Some("0123456789abcdef"), Some("f3"));
        for fields in [
            (None, valid.1, valid.2, valid.3),
            (valid.0, valid.1, None, valid.3),
            (valid.0, valid.1, valid.2, None),
            (Some(""), valid.1, valid.2, valid.3),
            (Some("Alpha\nBeta"), valid.1, valid.2, valid.3),
            (valid.0, Some("not-an-id"), valid.2, valid.3),
            (valid.0, valid.1, Some("0123456789abcde"), valid.3),
            (valid.0, valid.1, Some("0123456789abcdeg"), valid.3),
            (valid.0, valid.1, Some("0123456789ABCDEF"), valid.3),
            (valid.0, valid.1, valid.2, Some("f")),
            (valid.0, valid.1, valid.2, Some("f0")),
            (valid.0, valid.1, valid.2, Some("f-3")),
            (valid.0, valid.1, valid.2, Some("f3\n")),
            (valid.0, valid.1, valid.2, Some("f4294967296")),
        ] {
            keep_known_speaker(&known, valid.0, valid.1, valid.2, valid.3);
            assert!(path.exists());
            keep_known_speaker(&known, fields.0, fields.1, fields.2, fields.3);
            assert!(!path.exists(), "invalid source observation survived");
        }
        std::fs::remove_dir_all(known.parent().unwrap()).unwrap();
    }

    /// #1505: the people file as a later build writes it, with a field this
    /// build doesn't know at its top and in a person, opens with everyone
    /// in it (Tests' forward-read test, made plain by `el/people-forward`).
    #[test]
    fn a_file_from_a_later_build_is_read() {
        let path = file("later");
        let mut people = People::open(path.clone()).unwrap();
        people.remember("A", vec![print(0.3)], 1).unwrap();
        assert_eq!(
            People::open(path.clone()).unwrap().names(),
            vec!["A".to_owned()]
        );
        let mut later: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        later["a_field_from_a_later_build"] = serde_json::json!(true);
        later["people"][0]["a_field_from_a_later_build"] = serde_json::json!(true);
        std::fs::write(&path, serde_json::to_vec(&later).unwrap()).unwrap();
        assert_eq!(People::open(path).unwrap().names(), vec!["A".to_owned()]);
    }

    /// The people files kept as fixtures, one for each change of the file's
    /// shape, each written by its day's build ([`write_a_people_fixture`]).
    /// Every later build opens each with everyone in it, their faces known,
    /// their yes to being named, their memory id and, where the file was
    /// scored, how familiar each is and when that was, so an update never
    /// leaves a lamp's known people behind or makes them strangers again
    /// (the build row "Updates cover the whole lamp, V3 to V5").
    #[test]
    fn each_kept_people_file_opens_with_everyone_in_it() {
        use std::os::unix::fs::PermissionsExt;
        let kept: [(&str, &str); 2] = [
            (
                "2026-10-21",
                include_str!("../tests/fixtures/stores/people-2026-10-21.json"),
            ),
            // The same day's shape with a night scored, which the first lacks.
            (
                "2026-10-21-scored",
                include_str!("../tests/fixtures/stores/people-2026-10-21-scored.json"),
            ),
        ];
        for (day, text) in kept {
            let path = file(&format!("kept-{day}"));
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            // With memory's stable ids on, as `LAMP_MEMORY_STABLE_ID=on` has it.
            let people = People::open(path)
                .unwrap_or_else(|error| panic!("{day}: {error:?}"))
                .with_stable_ids(true);
            assert_eq!(people.names(), ["A", "B"], "{day}");
            let who = |seed: f32| people.who(&print(seed)).map(|(name, _)| name);
            assert_eq!(
                (who(0.3), who(1.7)),
                (Some("A".into()), Some("B".into())),
                "{day}"
            );
            assert_eq!(people.tellable(), ["A"], "{day}");
            assert_eq!(
                people.id_of("A").as_deref(),
                Some("p-ca978112ca1bbdca"),
                "{day}"
            );
            // Familiarity as kept, where the file has it.
            let saved: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(
                people.scored_at,
                saved["scored_at_unix_s"].as_u64(),
                "{day}"
            );
            for person in &people.people {
                let kept = saved["people"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|kept| kept["name"] == person.name.as_str())
                    .unwrap();
                assert_eq!(
                    person.familiar.map(f64::from),
                    kept["familiar"].as_f64(),
                    "{day}: {}",
                    person.name
                );
            }
        }
    }

    /// Writes this build's people file, with two people (one with two
    /// prints), one's yes to being named to others, their memory ids and a
    /// day of familiarity scored, to the path in `LAMP_EYES_PEOPLE_FIXTURE`.
    /// Run by hand when the file's shape changes, keeping the ones kept.
    #[test]
    #[ignore = "writes a people fixture: run by hand with LAMP_EYES_PEOPLE_FIXTURE set"]
    fn write_a_people_fixture() {
        let target =
            std::env::var_os("LAMP_EYES_PEOPLE_FIXTURE").expect("LAMP_EYES_PEOPLE_FIXTURE");
        let path = file("fixture");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        people
            .remember("A", vec![print(0.3), print(0.31)], 1_760_000_000)
            .unwrap();
        people
            .remember("B", vec![print(1.7)], 1_760_000_100)
            .unwrap();
        assert!(people.tell_visits("A", true).unwrap());
        people.seen("A", 1_760_086_400);
        // The first scoring only begins the day; the next night's writes.
        people.score(1_760_090_000, 0).unwrap();
        assert!(people.score(1_760_090_000 + 86_400, 0).unwrap());
        std::fs::copy(&path, target).unwrap();
    }

    #[test]
    fn nobody_is_known_until_asked_and_a_forgotten_face_is_gone_from_the_file() {
        let path = file("forget");
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.who(&print(0.3)), None);
        assert_eq!(people.best_likeness(&print(0.3)), None);
        let kept = people
            .remember("  A  ", vec![print(0.3), print(0.31)], 1)
            .unwrap();
        assert_eq!(kept, "A");
        let (name, how) = people.who(&print(0.3)).unwrap();
        assert_eq!(name, "A");
        assert!(how > 0.99);
        // The journal's number: the best likeness, matched or not.
        assert_eq!(people.best_likeness(&print(0.3)), Some(how));
        let far = people.best_likeness(&print(1.7)).unwrap();
        assert!(far < SAME_PERSON, "{far}");
        // Someone else is nobody.
        assert_eq!(people.who(&print(1.7)), None);
        // Kept across a restart, private.
        let again = People::open(path.clone()).unwrap();
        assert_eq!(again.names(), vec!["A".to_owned()]);
        let mode = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );
        // Asked again, the new prints replace the old.
        people.remember("a", vec![print(1.7)], 2).unwrap();
        assert_eq!(people.names(), vec!["a".to_owned()]);
        assert_eq!(people.who(&print(0.3)), None);
        assert!(people.forget("A").unwrap().forgot);
        assert!(!people.forget("A").unwrap().forgot);
        assert!(People::open(path).unwrap().names().is_empty());
    }

    /// #1670 (`--visits`): a remembered person is counted at most once in
    /// 16 hours, in memory; nothing is written at a visit. Once a UTC day
    /// has begun, the counts reach the file, scored as of that day's start,
    /// so neither the file nor its time of scoring says when anyone came
    /// (the reviewer on #1763). The number rises by one a counted day and
    /// halves in fourteen days without them, and the level follows it,
    /// counts not yet written included. Forgetting them takes the number
    /// and their counts.
    #[test]
    fn a_remembered_person_grows_familiar_once_a_day_and_fades_and_is_forgotten() {
        const DAY: u64 = 86_400;
        let path = file("familiar");
        let mut people = People::open(path.clone()).unwrap();
        people.remember("Ann", vec![print(0.3)], 1).unwrap();
        people.remember("Bo", vec![print(1.7)], 1).unwrap();
        let before = std::fs::read(&path).unwrap();
        // A lamp at UTC+7. Visits at 19:00 local; 03:00 local is `night`.
        const OFFSET: i64 = 7 * 3_600;
        let t = 1_000 * DAY + DAY / 2;
        let night = scoring_hour(t, OFFSET);
        assert_eq!((night as i64 + OFFSET) % DAY as i64, 3 * 3_600);
        let visit = |n: u64| t + n * DAY;
        let scoring = |n: u64| night + n * DAY + 60;
        // Never seen is new; someone not remembered is nothing.
        assert_eq!(people.familiar("Ann", t), Some(Familiar::New));
        assert_eq!(people.familiar("Cy", t), None);
        assert!(!people.seen("Cy", t));
        // Counted once, not again within 16 hours, and nothing written.
        assert!(people.seen("Ann", visit(0)));
        assert!(!people.seen("Ann", visit(0) + COUNTED_EVERY_S - 1));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Never scored: scoring begins at the last 03:00, with no write; and
        // none before the next 03:00.
        assert!(!people.score(t + 60, OFFSET).unwrap());
        assert!(!people.score(night + DAY - 60, OFFSET).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Past 03:00: written once, scored as of that 03:00.
        assert!(people.score(scoring(1), OFFSET).unwrap());
        let text: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(text["scored_at_unix_s"], night + DAY);
        assert!(!people.score(scoring(1) + 60, OFFSET).unwrap());
        // Seen on two more days: regular, before the third is written.
        assert!(people.seen("Ann", visit(1)));
        assert_eq!(people.familiar("Ann", visit(1)), Some(Familiar::New));
        people.score(scoring(2), OFFSET).unwrap();
        assert!(people.seen("Ann", visit(2)));
        assert_eq!(people.familiar("Ann", visit(2)), Some(Familiar::Regular));
        // Every day for two weeks: close.
        for day in 3..=16 {
            people.score(scoring(day), OFFSET).unwrap();
            people.seen("Ann", visit(day));
        }
        people.score(scoring(17), OFFSET).unwrap();
        let after = visit(17);
        assert_eq!(people.familiar("Ann", after), Some(Familiar::Close));
        // Read back from the file, the same; its time of scoring is a
        // 03:00 at the lamp.
        let kept = People::open(path.clone()).unwrap();
        assert_eq!(kept.familiar("Ann", after), Some(Familiar::Close));
        assert_eq!(
            (kept.scored_at.unwrap() as i64 + OFFSET) % DAY as i64,
            3 * 3_600
        );
        // Away: halved in fourteen days, and so on, down to new.
        let score = |people: &People, at: u64| {
            let person = people.people.iter().find(|p| p.name == "Ann").unwrap();
            faded(person.familiar.unwrap(), people.scored_at.unwrap(), at)
        };
        let now = score(&kept, after);
        assert!((score(&kept, after + FAMILIAR_HALF_LIFE_S) - now / 2.0).abs() < 1e-4);
        assert_eq!(
            kept.familiar("Ann", after + 8 * FAMILIAR_HALF_LIFE_S),
            Some(Familiar::New)
        );
        // Only one number a person and one time of scoring are written.
        let text: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut keys: Vec<&str> = text
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["people", "scored_at_unix_s", "version"]);
        for person in text["people"].as_array().unwrap() {
            let mut keys: Vec<&str> = person
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            let never_seen = ["added_unix_s", "name", "prints"];
            let seen = ["added_unix_s", "familiar", "name", "prints"];
            assert!(keys == never_seen || keys == seen, "{keys:?}");
        }
        assert!(
            text["people"][1].get("familiar").is_none(),
            "Bo was never seen"
        );
        // Forgotten: the number goes with the prints, and a count not yet
        // written goes too.
        let mut people = People::open(path.clone()).unwrap();
        assert!(people.seen("Ann", after));
        assert!(people.forget("Ann").unwrap().forgot);
        assert!(!people.score(after + DAY, OFFSET).unwrap());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("Ann"));
        assert_eq!(people.familiar("Ann", after), None);
    }

    /// A print whose cosine to `print(0.3)` is `cosine`.
    fn at(cosine: f32) -> Vec<f32> {
        let base = print(0.3);
        let other = print(1.7);
        let along: f32 = base.iter().zip(&other).map(|(a, b)| a * b).sum();
        let across = unit(
            base.iter()
                .zip(&other)
                .map(|(a, b)| b - along * a)
                .collect(),
        )
        .unwrap();
        let sine = (1.0 - cosine * cosine).sqrt();
        unit(
            base.iter()
                .zip(&across)
                .map(|(a, b)| cosine * a + sine * b)
                .collect(),
        )
        .unwrap()
    }

    /// #2176 row 4, lamp-4ace 20 Oct: someone known was saved again under a
    /// name one letter off theirs, the new prints at most 0.359 to their
    /// own. A face over [`NEAR_PERSON`] with a name one letter off is
    /// refused with the near name, and `apart` (spelled) keeps it apart. A
    /// known name never takes a face that isn't theirs (the reviewer's P1s
    /// on #2202 and #2211): near, it's spelled first and then added beside
    /// theirs, a full capture still leaving one of their own; far, it's
    /// refused whatever is asked. Made-up names only.
    #[test]
    fn a_near_face_with_a_near_name_is_asked_about_not_kept_beside_it() {
        assert!((likeness(&at(0.33), &print(0.3)) - 0.33).abs() < 1e-4);
        assert!(likeness(&print(2.9), &print(0.3)).abs() < 0.1);
        assert!(likeness(&print(2.9), &at(0.33)).abs() < 0.1);
        // Each case against a store knowing only "Ossa", by print(0.3).
        let known = |case: &str| {
            let mut people = People::open(file(case)).unwrap();
            people.remember("Ossa", vec![print(0.3)], 1).unwrap();
            people
        };
        let whose = |people: &People, print: &[f32]| people.who(print).map(|(name, _)| name);
        let capture = || vec![at(0.33); MOST_PRINTS];
        let mut people = known("near-name");
        assert_eq!(
            people.remember_one("Ossi", vec![at(0.2), at(0.33)], 2, false, false),
            Err(Refused::NearTo("Ossa".into()))
        );
        assert_eq!(people.names().len(), 1, "nothing kept when asked about");
        for (name, prints, why) in [
            ("Ossb", vec![at(0.2)], "a far face"),
            ("Minh", vec![at(0.33)], "a far name"),
        ] {
            let mut people = known(&format!("near-name-{name}"));
            assert_eq!(
                people.remember_one(name, prints, 2, false, false),
                Ok(name.to_owned()),
                "{why}"
            );
        }
        // Exactly their name with a near face: spelled first, then beside
        // theirs, and a full capture still leaves one of their own.
        let mut same = known("near-name-same");
        assert_eq!(
            same.remember_one("ossa", capture(), 2, false, false),
            Err(Refused::NearTo("Ossa".into()))
        );
        assert_eq!(
            same.remember_one("ossa", capture(), 3, false, true),
            Ok("Ossa".into())
        );
        assert_eq!(same.people[0].prints.len(), MOST_PRINTS);
        assert_eq!(
            whose(&same, &print(0.3)),
            Some("Ossa".into()),
            "their own still"
        );
        // Their name with a face not near theirs: refused, `replace` or
        // `apart` too, and they are still recognised.
        let mut other = known("near-name-other");
        for (replace, apart) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                other.remember_one("Ossa", vec![print(2.9)], 2, replace, apart),
                Err(Refused::NotLikeThem),
                "replace {replace}, apart {apart}"
            );
        }
        assert_eq!(whose(&other, &print(0.3)), Some("Ossa".into()));
        // Two known, names one letter apart: a capture near the first's face
        // and far from the second's, spelled as the second's name, takes
        // nothing of the second's (the reviewer's third path on #2211).
        let mut two = known("near-name-two");
        two.remember("Ossi", vec![print(2.9)], 1).unwrap();
        // Ossi's own face isn't near, spelled or not, so it's refused.
        assert_eq!(
            two.remember_one("Ossi", vec![at(0.33)], 2, false, false),
            Err(Refused::NotLikeThem)
        );
        assert_eq!(
            two.remember_one("Ossi", vec![at(0.33)], 3, false, true),
            Err(Refused::NotLikeThem)
        );
        assert_eq!(whose(&two, &print(2.9)), Some("Ossi".into()));
        // Jarvis's P1 on #2235: one near print among four unrelated ones,
        // spelled as their name, is not near as a whole, and nothing of it
        // is kept; four near and one unrelated adds only the near four.
        let unrelated = || vec![print(0.5), print(0.7), print(0.9), print(1.1)];
        let mut mixed = known("near-name-mixed");
        let mut few = vec![at(0.33)];
        few.extend(unrelated());
        assert_eq!(
            mixed.remember_one("Ossa", few, 2, false, true),
            Err(Refused::NotLikeThem)
        );
        assert_eq!(mixed.people[0].prints.len(), 1);
        let mut most = vec![at(0.33); 4];
        most.push(print(2.9));
        assert_eq!(
            mixed.remember_one("Ossa", most, 3, false, true),
            Ok("Ossa".into())
        );
        assert_eq!(
            whose(&mixed, &print(2.9)),
            None,
            "the unrelated print isn't theirs"
        );
        assert_eq!(whose(&mixed, &print(0.3)), Some("Ossa".into()));
        // The reviewer's P1 after #2252: theirs as a whole (three of five at
        // their face) keeps only the three, so neither an unrelated face nor
        // a sibling's near print (Reviewer-lead's P3 on #2269) goes in with
        // theirs.
        let mut whole = known("near-name-whole");
        let mut capture_of_three = vec![at(0.9); 3];
        capture_of_three.push(at(0.33));
        capture_of_three.push(print(0.5));
        assert_eq!(
            whole.remember_one("Ossa", capture_of_three, 2, false, false),
            Ok("Ossa".into())
        );
        assert_eq!(whole.people[0].prints.len(), 3, "only their own three");
        assert_eq!(whose(&whole, &print(0.5)), None);
        assert_eq!(whose(&whole, &at(0.9)), Some("Ossa".into()));
        // Jarvis's new P1: a face known under another name never takes a
        // known name, `replace` or `apart` or not, even when it's near theirs
        // (siblings), and both stay as they were.
        let mut renamed = known("near-name-renamed");
        renamed.remember("Bex", vec![at(0.33)], 1).unwrap();
        for (replace, apart) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                renamed.remember_one("Ossa", vec![at(0.33)], 2, replace, apart),
                Err(Refused::NotLikeThem),
                "replace {replace}, apart {apart}"
            );
        }
        assert_eq!(
            renamed
                .people
                .iter()
                .find(|p| p.name == "Ossa")
                .unwrap()
                .prints
                .len(),
            1
        );
        assert_eq!(whose(&renamed, &print(0.3)), Some("Ossa".into()));
        assert_eq!(whose(&renamed, &at(0.33)), Some("Bex".into()));
        // A new name spelled after the question is kept apart.
        let mut apart = known("near-name-apart");
        assert_eq!(
            apart.remember_one("Ossi", vec![at(0.33)], 2, false, true),
            Ok("Ossi".into())
        );
        assert_eq!(apart.names().len(), 2, "both kept, as asked");
    }

    #[test]
    fn names_one_letter_apart_are_near_and_short_or_same_names_are_not() {
        for (a, b) in [
            ("Ossa", "Ossai"),
            ("Ossai", "ossa"),
            ("Minh", "Linh"),
            ("Anna", "Ana"),
            ("Hoa", "Hoà"),
        ] {
            assert!(near_name(a, b), "{a} {b}");
        }
        for (a, b) in [
            ("Ossa", "OSSA"),
            ("Al", "Ali"),
            ("Ossa", "Osman"),
            ("Minh", "Mike"),
            ("Bo", "Bob"),
        ] {
            assert!(!near_name(a, b), "{a} {b}");
        }
    }

    /// The first builder's 13:24: a face known as "A" is asked to be kept
    /// as "B". With one name for one face it is refused with the name it
    /// is known by, and kept as "B" only with `replace`, "A" gone. Another
    /// person's face, or the same name again, is kept as before.
    #[test]
    fn one_face_keeps_one_name_and_another_is_taken_only_in_its_place() {
        let mut people = People::open(file("one-face")).unwrap();
        people
            .remember("A", vec![print(0.3), print(0.31)], 1)
            .unwrap();
        assert_eq!(
            people.remember_one("B", vec![print(0.3)], 2, false, false),
            Err(Refused::KnownAs("A".into()))
        );
        assert_eq!(people.names(), vec!["A".to_owned()]);
        // The same name again replaces its own prints, as before.
        assert_eq!(
            people.remember_one("a", vec![print(0.31)], 3, false, false),
            Ok("a".into())
        );
        // Said yes to: the new name in place of the old.
        assert_eq!(
            people.remember_one("B", vec![print(0.31)], 4, true, false),
            Ok("B".into())
        );
        assert_eq!(people.names(), vec!["B".to_owned()]);
        assert_eq!(people.who(&print(0.31)).unwrap().0, "B");
        // Someone else is kept beside them.
        assert_eq!(
            people.remember_one("C", vec![print(1.7)], 5, false, false),
            Ok("C".into())
        );
        assert_eq!(people.names(), vec!["B".to_owned(), "C".to_owned()]);
        assert_eq!(
            people.remember_one("  ", vec![print(2.9)], 6, false, false),
            Err(Refused::NotAName)
        );
        // Without the check, a second name sits beside the first, as today.
        people.remember("Jay", vec![print(0.31)], 7).unwrap();
        assert_eq!(people.names().len(), 3);
    }

    /// The reviewer on #1057: prints known by two names are refused with
    /// both, and `replace`, which forgets both, keeps only the new one.
    #[test]
    fn a_face_known_by_two_names_is_refused_with_both_and_replaced_for_both() {
        let mut people = People::open(file("two-names")).unwrap();
        people.remember("A", vec![print(0.3)], 1).unwrap();
        people.remember("C", vec![print(1.7)], 2).unwrap();
        let both = vec![print(0.3), print(1.7)];
        assert_eq!(
            people.remember_one("B", both.clone(), 3, false, false),
            Err(Refused::KnownAs("A and C".into()))
        );
        assert_eq!(
            people.remember_one("B", both, 4, true, false),
            Ok("B".into())
        );
        assert_eq!(people.names(), vec!["B".to_owned()]);
    }

    #[test]
    fn the_voice_asks_in_one_of_four_lines_and_anything_else_is_nothing() {
        assert_eq!(
            ask_of(br#"{"remember":"A"}"#),
            Some(Ask::Remember("A".into()))
        );
        assert_eq!(ask_of(br#"{"forget":"A"}"#), Some(Ask::Forget("A".into())));
        assert_eq!(
            ask_of(br#"{"remember":"A","one_face":true}"#),
            Some(Ask::RememberOne {
                name: "A".into(),
                replace: false,
                apart: false
            })
        );
        assert_eq!(
            ask_of(br#"{"remember":"A","one_face":true,"replace":true}"#),
            Some(Ask::RememberOne {
                name: "A".into(),
                replace: true,
                apart: false
            })
        );
        assert_eq!(
            ask_of(br#"{"remember":"A","one_face":true,"apart":true}"#),
            Some(Ask::RememberOne {
                name: "A".into(),
                replace: false,
                apart: true
            })
        );
        assert_eq!(ask_of(br#"{"remember":"A","apart":true}"#), None);
        assert_eq!(ask_of(br#"{"remember":"A","replace":true}"#), None);
        assert_eq!(ask_of(br#"{"forget":"A","one_face":true}"#), None);
        // From a photo (`--photo-dir`): a name and the photo's file.
        assert_eq!(
            ask_of(br#"{"remember":"A","photo":"a.jpg"}"#),
            Some(Ask::RememberPhoto {
                name: "A".into(),
                file: "a.jpg".into(),
            })
        );
        // A photo never replaces anyone: it has no say for them.
        assert_eq!(
            ask_of(br#"{"remember":"A","photo":"a.jpg","replace":true}"#),
            None
        );
        // Its person's consent, one ask at a time.
        assert_eq!(ask_of(br#"{"photo_here":true}"#), Some(Ask::PhotoHere));
        assert_eq!(
            ask_of(br#"{"photo_asked":"A"}"#),
            Some(Ask::PhotoAsked("A".into()))
        );
        assert_eq!(
            ask_of(br#"{"photo_yes":"A"}"#),
            Some(Ask::PhotoYes {
                name: "A".into(),
                face: None
            })
        );
        // The face the voice heard saying it (`LAMP_PHOTO_SPEAKER=on`, #2856):
        // only beside a yes.
        assert_eq!(
            ask_of(br#"{"photo_yes":"A","face":"b0/f3"}"#),
            Some(Ask::PhotoYes {
                name: "A".into(),
                face: Some(Some("b0/f3".into()))
            })
        );
        // `null` is a `face` key too, not its absence (Fourth's P3 on
        // #3257): told apart for the yes, and refused beside anything else.
        assert_eq!(
            ask_of(br#"{"photo_yes":"A","face":null}"#),
            Some(Ask::PhotoYes {
                name: "A".into(),
                face: Some(None)
            })
        );
        assert_eq!(ask_of(br#"{"photo_here":true,"face":null}"#), None);
        assert_eq!(ask_of(br#"{"photo_no":"A","face":null}"#), None);
        assert_eq!(ask_of(br#"{"photo_no":"A","face":"b0/f3"}"#), None);
        assert_eq!(ask_of(br#"{"photo_here":true,"face":"b0/f3"}"#), None);
        assert_eq!(ask_of(br#"{"photo_asked":"A","face":"b0/f3"}"#), None);
        assert_eq!(
            ask_of(br#"{"photo_no":"A"}"#),
            Some(Ask::PhotoNo("A".into()))
        );
        assert_eq!(ask_of(br#"{"photo_yes":"A","photo_no":"A"}"#), None);
        assert_eq!(ask_of(br#"{"photo_here":false}"#), None);
        assert_eq!(ask_of(br#"{"photo":"a.jpg"}"#), None);
        assert_eq!(
            ask_of(br#"{"remember":"A","photo":"a.jpg","one_face":true}"#),
            None
        );
        assert_eq!(ask_of(br#"{"forget":"A","photo":"a.jpg"}"#), None);
        assert_eq!(ask_of(br#"{"list":true}"#), Some(Ask::List));
        assert_eq!(
            ask_of(br#"{"list":true,"waiting":true}"#),
            Some(Ask::ListWaiting)
        );
        assert_eq!(ask_of(br#"{"list":true,"waiting":false}"#), None);
        assert_eq!(ask_of(br#"{"waiting":true}"#), None);
        assert_eq!(ask_of(br#"{"who":true}"#), Some(Ask::Who));
        for nothing in [
            &br#"{"who":false}"#[..],
            br#"{"remember":"A","forget":"A"}"#,
            br#"{"remember":"A","extra":1}"#,
            br#"{}"#,
            b"who",
        ] {
            assert_eq!(ask_of(nothing), None);
        }
    }

    #[test]
    fn a_face_is_remembered_only_as_the_one_face_near_and_otherwise_says_why() {
        let mut taking = Taking::new("A".into(), 0);
        // Nobody there: said after a second and a half, not four.
        taking.look(&[], None);
        assert_eq!(taking.given_up(NOBODY_FOR_US - 1), None);
        assert_eq!(taking.given_up(NOBODY_FOR_US), Some(NotTaken::NoFace));
        // Then someone across the room, then two at the lamp: the full four
        // seconds to come closer or step aside.
        taking.look(&[0.05], None);
        assert_eq!(taking.given_up(NOBODY_FOR_US), None);
        assert_eq!(taking.given_up(TAKING_FOR_US), Some(NotTaken::TooFar));
        taking.look(&[0.2, 0.15], Some(print(0.3)));
        assert_eq!(taking.given_up(TAKING_FOR_US), Some(NotTaken::TwoFaces));
        assert!(taking.taken().is_none());
        // One person at the lamp, five looks: taken.
        let mut taking = Taking::new("A".into(), 0);
        for n in 0..MOST_PRINTS {
            assert!(taking.taken().is_none());
            taking.look(&[0.2, 0.04], Some(print(0.3 + n as f32 * 0.001)));
        }
        assert_eq!(taking.given_up(1), None);
        assert_eq!(taking.taken().unwrap().len(), MOST_PRINTS);
    }

    #[test]
    fn names_prints_and_people_are_bounded_and_a_bad_file_is_set_aside() {
        let path = file("bounds");
        let mut people = People::open(path.clone()).unwrap();
        for bad in ["", "  ", "1A", "rm -rf /", "A;", &"a".repeat(41)] {
            assert!(
                people.remember(bad, vec![print(0.3)], 1).is_err(),
                "{bad:?}"
            );
        }
        assert!(people.remember("B", vec![], 1).is_err());
        assert!(
            people
                .remember("B", vec![print(0.3); MOST_PRINTS + 1], 1)
                .is_err()
        );
        assert!(people.remember("B", vec![vec![0.5; PRINT - 1]], 1).is_err());
        assert_eq!(plain_name("Nguyễn Thị B").as_deref(), Some("Nguyễn Thị B"));
        assert_eq!(plain_name("O’Brien").as_deref(), Some("O’Brien"));
        for n in 0..MOST_PEOPLE {
            let name = format!("Person {}", char::from(b'A' + n as u8));
            people
                .remember(&name, vec![print(0.1 + n as f32)], 1)
                .unwrap();
        }
        assert!(people.remember("One More", vec![print(0.05)], 1).is_err());
        // Someone already known may be asked again even when full.
        people.remember("person a", vec![print(0.1)], 2).unwrap();
        assert_eq!(
            People::open(path.clone()).unwrap().names().len(),
            MOST_PEOPLE
        );
        // A person this build does not keep is set aside, never dropped
        // (#1505), and a file too large is moved there whole.
        std::fs::write(
            &path,
            b"{\"version\":1,\"people\":[{\"name\":\"x;y\",\"prints\":[],\"added_unix_s\":1}]}",
        )
        .unwrap();
        assert!(People::open(path.clone()).unwrap().names().is_empty());
        assert_eq!(set_aside_count(&path), 1);
        std::fs::write(&path, vec![b' '; MOST_FILE_BYTES as usize + 1]).unwrap();
        assert!(People::open(path.clone()).unwrap().names().is_empty());
        assert!(!path.exists(), "moved whole, not copied");
        assert_eq!(set_aside_count(&path), 2);
    }

    /// The set-aside folder's files, as names and modes.
    fn set_aside_files(path: &Path) -> Vec<(String, u32)> {
        use std::os::unix::fs::PermissionsExt;
        let folder = path.parent().unwrap().join(SET_ASIDE);
        let mut found: Vec<(String, u32)> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| {
                        let mode = entry.metadata().unwrap().permissions().mode() & 0o777;
                        (entry.file_name().to_string_lossy().into_owned(), mode)
                    })
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    /// #1505, harm 3: a later build's field, on the file and on a person,
    /// is no reason to refuse: everyone loads, nothing is set aside, and the
    /// next save writes this build's fields only.
    #[test]
    fn a_later_field_reads_one_release_forward() {
        let path = file("forward");
        let one = serde_json::to_string(&print(0.2)).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"later":true,"people":[{{"name":"Ann","prints":[{one}],"added_unix_s":1,"seen_unix_s":9}}]}}"#
            ),
        )
        .unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.names(), ["Ann"]);
        assert_eq!(set_aside_count(&path), 0);
        assert!(set_aside_files(&path).is_empty());
        people.remember("Bo", vec![print(0.7)], 2).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            !saved.contains("seen_unix_s") && !saved.contains("later"),
            "{saved}"
        );
    }

    /// One person this build cannot read is set aside, in a 0600 file in a
    /// 0700 folder; the others load and are saved back without them, so a
    /// restart sets nothing aside again.
    #[test]
    fn a_person_it_cannot_read_is_set_aside_and_the_rest_load() {
        let path = file("one-aside");
        let good = serde_json::to_string(&print(0.2)).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"people":[{{"name":"Ann","prints":[{good}],"added_unix_s":1}},{{"name":"Cy","prints":"a newer shape","added_unix_s":1}}]}}"#
            ),
        )
        .unwrap();
        let people = People::open(path.clone()).unwrap();
        assert_eq!(people.names(), ["Ann"]);
        assert_eq!(set_aside_count(&path), 1);
        let files = set_aside_files(&path);
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(files[0].0.starts_with("people-person-"), "{files:?}");
        assert_eq!(files[0].1, 0o600);
        {
            use std::os::unix::fs::PermissionsExt;
            let folder = path.parent().unwrap().join(SET_ASIDE);
            assert_eq!(
                std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let kept = std::fs::read_to_string(folder.join(&files[0].0)).unwrap();
            assert!(
                kept.contains("Cy") && kept.contains("a newer shape"),
                "{kept}"
            );
        }
        assert!(!std::fs::read_to_string(&path).unwrap().contains("Cy"));
        // Opened again: nothing more set aside.
        assert_eq!(People::open(path.clone()).unwrap().names(), ["Ann"]);
        assert_eq!(set_aside_files(&path).len(), 1);
    }

    /// A file it cannot read at all, a newer build's `version` or not JSON,
    /// is moved whole, and the eyes start knowing nobody.
    #[test]
    fn a_whole_file_it_cannot_read_is_moved_and_the_eyes_start() {
        for (name, text, starts) in [
            ("newer", r#"{"version":2,"people":[]}"#, "people-version-2"),
            ("broken", "not json at all", "people-not-JSON-"),
        ] {
            let path = file(name);
            std::fs::write(&path, text).unwrap();
            let people = People::open(path.clone()).unwrap();
            assert!(people.names().is_empty(), "{name}");
            assert!(!path.exists(), "{name}");
            let files = set_aside_files(&path);
            assert_eq!(files.len(), 1, "{name}: {files:?}");
            assert!(files[0].0.starts_with(starts), "{name}: {files:?}");
            let kept = path.parent().unwrap().join(SET_ASIDE).join(&files[0].0);
            assert_eq!(std::fs::read_to_string(kept).unwrap(), text, "{name}");
        }
    }

    /// "Forget my face" reaches what was set aside (the reviewer on #1558):
    /// a person set aside alone, and one in a whole file moved aside, are
    /// forgotten there too, and no set-aside file holds them after.
    #[test]
    fn forgetting_a_name_reaches_its_set_aside_records() {
        let path = file("forget-aside");
        let good = serde_json::to_string(&print(0.2)).unwrap();
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"people":[{{"name":"Ann","prints":[{good}],"added_unix_s":1}},{{"name":"Cy","prints":"a newer shape","added_unix_s":1}},{{"name":"Di","prints":"a newer shape","added_unix_s":1}}]}}"#
            ),
        )
        .unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(set_aside_count(&path), 2);
        assert!(
            people.forget("cy").unwrap().forgot,
            "set aside, and forgotten there"
        );
        assert_eq!(set_aside_count(&path), 1);
        let folder = path.parent().unwrap().join(SET_ASIDE);
        for (name, _) in set_aside_files(&path) {
            let kept = std::fs::read_to_string(folder.join(name)).unwrap();
            assert!(!kept.contains("\"Cy\""), "{kept}");
        }
        // The last one in a file: the file goes.
        assert!(people.forget("Di").unwrap().forgot);
        assert!(set_aside_files(&path).is_empty());
        // A whole file moved aside, from a newer build.
        let path = file("forget-whole");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":2,"people":[{{"name":"Ed","prints":[{good}],"added_unix_s":1}}]}}"#
            ),
        )
        .unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert!(people.names().is_empty());
        assert!(people.forget("Ed").unwrap().forgot);
        assert!(set_aside_files(&path).is_empty());
        assert!(
            !people.forget("Ed").unwrap().forgot,
            "not known anywhere now"
        );
    }

    /// #1586: a set-aside file that cannot be searched might hold the face.
    /// The forget says so (never "not known", never a fault) and waits: a
    /// face of theirs read back is dropped at once, and once the file can be
    /// searched it is forgotten there and nothing waits any more.
    #[test]
    fn a_forget_that_cannot_reach_a_set_aside_file_waits_and_is_finished_when_it_can() {
        let path = file("forget-waits");
        let good = serde_json::to_string(&print(0.2)).unwrap();
        let folder = path.parent().unwrap().join(SET_ASIDE);
        std::fs::create_dir(&folder).unwrap();
        let unreadable = folder.join("people-whole-0000.json");
        std::fs::write(&unreadable, "not json").unwrap();
        let fay = format!(
            r#"{{"version":1,"people":[{{"name":"Fay","prints":[{good}],"added_unix_s":1}}]}}"#
        );
        std::fs::write(&path, &fay).unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.names(), ["Fay"]);
        assert_eq!(
            people.forget("Fay").unwrap(),
            Forgot {
                forgot: true,
                unsearched: 1
            }
        );
        assert!(people.names().is_empty());
        // It waits, hashed: the name itself is never kept for it, and it is
        // not counted as anyone's record.
        let waiting = std::fs::read_to_string(folder.join(PENDING_FORGETS)).unwrap();
        assert!(!waiting.to_lowercase().contains("fay"), "{waiting}");
        assert_eq!(set_aside_count(&path), 1);
        // Read back into the file (by a later build, or by hand): forgotten
        // again at once.
        std::fs::write(&path, &fay).unwrap();
        let people = People::open(path.clone()).unwrap();
        assert!(people.names().is_empty());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("Fay"));
        assert!(folder.join(PENDING_FORGETS).exists(), "still waits");
        // The set-aside file can be searched now, and holds her: forgotten
        // there, and nothing waits.
        std::fs::write(&unreadable, &fay).unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert!(!unreadable.exists(), "her record was all that was in it");
        assert!(!folder.join(PENDING_FORGETS).exists());
        assert_eq!(set_aside_count(&path), 0);
        assert_eq!(
            people.forget("Fay").unwrap(),
            Forgot {
                forgot: false,
                unsearched: 0
            }
        );

        // A set-aside person whose name is no plain string might be anyone,
        // so it cannot be searched either (the reviewer on #1558).
        let path = file("forget-nameless");
        std::fs::write(
            &path,
            r#"{"version":1,"people":[{"name":{"given":"Cy"},"prints":[],"added_unix_s":1}]}"#,
        )
        .unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(set_aside_count(&path), 1);
        assert_eq!(
            people.forget("Cy").unwrap(),
            Forgot {
                forgot: false,
                unsearched: 1
            }
        );
        assert_eq!(
            set_aside_count(&path),
            1,
            "kept: nothing can be told about it"
        );
    }

    /// #2850 (lamp-c0a4, 22 October): a 2-byte set-aside
    /// `people-not-a-list-of-people-….json`, `{}`, made every forget wait.
    /// JSON that holds nothing (`{}`, `[]`, a number, `null`) holds nobody,
    /// so it is searched and a forget finishes; one that can't be read, or
    /// an object with something in it but no list of people (a newer build's
    /// shape), still keeps it waiting (#1586).
    #[test]
    fn a_set_aside_file_that_holds_nothing_never_keeps_a_forget_waiting() {
        let good = serde_json::to_string(&print(0.2)).unwrap();
        let fay = format!(
            r#"{{"version":1,"people":[{{"name":"Fay","prints":[{good}],"added_unix_s":1}}]}}"#
        );
        for (name, text, unsearched) in [
            ("empty-object", "{}", 0),
            ("empty-list", "[]", 0),
            ("number", "7", 0),
            ("null", "null", 0),
            ("garbled", "{\"peo", 1),
            ("newer-shape", r#"{"version":3,"faces":{"Fay":[1]}}"#, 1),
            ("a-name", r#""Fay""#, 1),
        ] {
            let path = file(&format!("aside-holds-{name}"));
            let folder = path.parent().unwrap().join(SET_ASIDE);
            std::fs::create_dir(&folder).unwrap();
            let aside = folder.join("people-not-a-list-of-people-0000.json");
            std::fs::write(&aside, text).unwrap();
            std::fs::write(&path, &fay).unwrap();
            let mut people = People::open(path.clone()).unwrap();
            assert_eq!(
                people.forget("Fay").unwrap(),
                Forgot {
                    forgot: true,
                    unsearched
                },
                "{name}"
            );
            assert_eq!(
                folder.join(PENDING_FORGETS).exists(),
                unsearched > 0,
                "{name}: a forget waits only on a file that might hold her"
            );
            assert!(aside.exists(), "{name}: never removed");
        }
    }

    /// #1680's rule for the forgets that wait (#2052's audit): a file from a
    /// later build, with fields this one doesn't know, is read and its waits
    /// kept; one that can't be read is set aside, kept as it was, never
    /// counted as anyone's records, and a forget goes on and waits afresh.
    /// Before, every forget failed while it couldn't be read.
    #[test]
    fn the_forgets_that_wait_never_stop_a_forget() {
        let good = serde_json::to_string(&print(0.2)).unwrap();
        let fay = format!(
            r#"{{"version":1,"people":[{{"name":"Fay","prints":[{good}],"added_unix_s":1}}]}}"#
        );
        let setup = |name: &str, waits: &str| {
            let path = file(name);
            let folder = path.parent().unwrap().join(SET_ASIDE);
            std::fs::create_dir(&folder).unwrap();
            // One set-aside file nothing can search, so a forget must wait.
            std::fs::write(folder.join("people-whole-0000.json"), "not json").unwrap();
            std::fs::write(folder.join(PENDING_FORGETS), waits).unwrap();
            std::fs::write(&path, &fay).unwrap();
            (path, folder)
        };
        let waits_for = |folder: &Path| -> Vec<String> {
            let text = std::fs::read_to_string(folder.join(PENDING_FORGETS)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["forget"]
                .as_array()
                .unwrap()
                .iter()
                .map(|wait| wait["hash"].as_str().unwrap().to_owned())
                .collect()
        };
        let waited = Forgot {
            forgot: true,
            unsearched: 1,
        };

        // From a later build: a field at its top and in a wait.
        let later = format!(
            r#"{{"forget":[{{"hash":"{}","at_unix_s":5,"why":"asked"}}],"kind":2}}"#,
            name_hash("Gil")
        );
        let (path, folder) = setup("pending-later", &later);
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.forget("Fay").unwrap(), waited);
        assert_eq!(waits_for(&folder), [name_hash("Gil"), name_hash("Fay")]);

        // Not readable at all: set aside as it was, and the forget waits.
        let (path, folder) = setup("pending-unreadable", "{\"forget\": [tor");
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.forget("Fay").unwrap(), waited);
        assert!(people.names().is_empty());
        assert_eq!(waits_for(&folder), [name_hash("Fay")]);
        let kept: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("forgets-pending.unreadable-"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            std::fs::read_to_string(folder.join(&kept[0])).unwrap(),
            "{\"forget\": [tor"
        );
        // Never anyone's records: still the one set-aside file.
        assert_eq!(set_aside_count(&path), 1);

        // The reviewer on #2070: the build that wrote it, back after a step
        // back, reads it again. At its next start its waits are merged back,
        // the later word kept for a name in both, and the copy goes.
        std::fs::write(
            folder.join(&kept[0]),
            format!(
                r#"{{"forget":[{{"hash":"{}","at_unix_s":7}},{{"hash":"{}","at_unix_s":1}}]}}"#,
                name_hash("Gil"),
                name_hash("Fay")
            ),
        )
        .unwrap();
        let fay_asked = {
            let text = std::fs::read_to_string(folder.join(PENDING_FORGETS)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["forget"][0]["at_unix_s"].as_u64().unwrap()
        };
        People::open(path.clone()).unwrap();
        assert_eq!(waits_for(&folder), [name_hash("Fay"), name_hash("Gil")]);
        let text = std::fs::read_to_string(folder.join(PENDING_FORGETS)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["forget"][0]["at_unix_s"].as_u64(), Some(fay_asked));
        assert!(!folder.join(&kept[0]).exists());
        assert_eq!(set_aside_count(&path), 1);
    }

    /// Twelve's P2 on #1767: a waiting forget covers only the faces kept
    /// before it was asked. Someone remembered again after is theirs to
    /// keep, across a start and a forget of someone else, while their old
    /// record still goes once its set-aside file can be searched.
    #[test]
    fn someone_remembered_after_a_waiting_forget_is_kept() {
        let path = file("forget-then-remember");
        let folder = path.parent().unwrap().join(SET_ASIDE);
        std::fs::create_dir(&folder).unwrap();
        let unreadable = folder.join("people-whole-0000.json");
        std::fs::write(&unreadable, "not json").unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.forget_at("Fay", 100).unwrap().unsearched, 1);
        people
            .remember("Fay", vec![print(0.7)], 200)
            .expect("remembered again");
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.names(), ["Fay"], "her later word stands");
        people.forget_at("Gus", 300).unwrap();
        assert_eq!(People::open(path.clone()).unwrap().names(), ["Fay"]);
        // Her old record, kept before she asked, still goes when it can be
        // searched; her new one stays.
        let good = serde_json::to_string(&print(0.2)).unwrap();
        std::fs::write(
            &unreadable,
            format!(
                r#"{{"version":1,"people":[{{"name":"Fay","prints":[{good}],"added_unix_s":50}},{{"name":"Fay","prints":[{good}]}}]}}"#
            ),
        )
        .unwrap();
        let people = People::open(path.clone()).unwrap();
        assert!(
            !unreadable.exists(),
            "both old records gone, the timeless one too"
        );
        assert_eq!(people.names(), ["Fay"]);
        assert!(!folder.join(PENDING_FORGETS).exists());
    }

    /// A temporary file a crash left in `set-aside/` is neither counted nor
    /// searched; and a linked `set-aside` is not followed by `forget` or the
    /// count (the reviewer on #1558).
    #[test]
    fn a_left_temporary_file_and_a_linked_folder_are_left_alone() {
        let path = file("aside-temporary");
        let folder = path.parent().unwrap().join(SET_ASIDE);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("people-person-0000.json.new"), "cut sho").unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(set_aside_count(&path), 0);
        assert!(!people.forget("Hal").unwrap().forgot);

        let path = file("aside-linked");
        let elsewhere = path.parent().unwrap().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(
            elsewhere.join("people-person-1111.json"),
            r#"{"version":1,"people":[{"name":"Ivy","prints":"x","added_unix_s":1}]}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&elsewhere, path.parent().unwrap().join(SET_ASIDE)).unwrap();
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(set_aside_count(&path), 1, "said as one, not followed");
        assert!(people.forget("Ivy").is_err());
        assert!(elsewhere.join("people-person-1111.json").exists());
    }

    /// The same record set aside again (a crash, or a failed save back,
    /// before the rest were saved) is one file, counted once; and a
    /// set-aside that is a link is refused, never followed.
    #[test]
    fn the_same_record_is_set_aside_once_and_a_linked_folder_is_refused() {
        let path = file("aside-once");
        let text =
            r#"{"version":1,"people":[{"name":"Gus","prints":"a newer shape","added_unix_s":1}]}"#;
        std::fs::write(&path, text).unwrap();
        People::open(path.clone()).unwrap();
        // As if the save back never happened.
        std::fs::write(&path, text).unwrap();
        People::open(path.clone()).unwrap();
        assert_eq!(set_aside_files(&path).len(), 1);
        assert_eq!(set_aside_count(&path), 1);

        let path = file("aside-link");
        let elsewhere = path.parent().unwrap().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, path.parent().unwrap().join(SET_ASIDE)).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert!(People::open(path.clone()).is_err());
        assert!(path.exists(), "nothing moved through the link");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    /// #2176 row 3: with `LAMP_MEMORY_STABLE_ID=on`, a person keeps one
    /// memory id from their first remembering, through a later one and a
    /// rename (A to B carries A's id), and the known file has it on the line
    /// after the name.
    #[test]
    fn a_rename_keeps_the_memory_id_and_the_known_file_tells_it() {
        use lamp_msgs::known::person_id;
        let path = file("stable-rename");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        people.remember("Minh", vec![print(0.3)], 1).unwrap();
        assert_eq!(people.id_of("Minh"), Some(person_id("Minh")));
        // Remembered again: the same id.
        assert_eq!(
            people.remember_one("minh", vec![print(0.31)], 2, false, false),
            Ok("minh".into())
        );
        assert_eq!(people.id_of("Minh"), Some(person_id("Minh")));
        // Renamed, said yes to: Sam keeps Minh's id, in the file too.
        assert_eq!(
            people.remember_one("Sam", vec![print(0.31)], 3, true, false),
            Ok("Sam".into())
        );
        assert_eq!(people.names(), vec!["Sam".to_owned()]);
        assert_eq!(people.id_of("Sam"), Some(person_id("Minh")));
        let reopened = People::open(path.clone()).unwrap().with_stable_ids(true);
        assert_eq!(reopened.id_of("Sam"), Some(person_id("Minh")));
        // The known file: the name, then the id.
        let known = path.with_file_name("known");
        let id = people.id_of("Sam");
        keep_known(&known, Some("Sam"), id.as_deref(), None);
        assert_eq!(
            std::fs::read_to_string(&known).unwrap(),
            format!("Sam\n{}\n", person_id("Minh"))
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Nobody's memory moves: a record from before the stable id, with no
    /// id, is given exactly `person_id` of its name, written with its next
    /// write; until then the file is as it was.
    #[test]
    fn an_old_record_with_no_id_keeps_its_memory_key() {
        let path = file("stable-old");
        let old = format!(
            r#"{{"version":1,"people":[{{"name":"Lan","prints":[{:?}],"added_unix_s":1}}]}}"#,
            print(0.3)
        );
        std::fs::write(&path, &old).unwrap();
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        assert_eq!(
            people.id_of("Lan"),
            Some(lamp_msgs::known::person_id("Lan"))
        );
        assert_eq!(people.id_of("Lan").unwrap(), "p-094a367b026246fb");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old, "only read");
        people.tell_visits("Lan", true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""id":"p-094a367b026246fb""#), "{text}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Two people never share one id: a new "Minh" after the first was
    /// renamed Sam (who keeps Minh's id) is given Minh's id with `-2`; and
    /// of two records with one id in the file, the later is given another.
    #[test]
    fn two_people_never_share_one_memory_id() {
        use lamp_msgs::known::person_id;
        let path = file("stable-two");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        people.remember("Minh", vec![print(0.3)], 1).unwrap();
        people
            .remember_one("Sam", vec![print(0.3)], 2, true, false)
            .unwrap();
        // Prints apart from Sam's: someone new.
        assert_eq!(
            people.remember_one("Minh", vec![print(1.7)], 3, false, true),
            Ok("Minh".into())
        );
        let minh = person_id("Minh");
        assert_eq!(people.id_of("Sam"), Some(minh.clone()));
        assert_eq!(people.id_of("Minh"), Some(format!("{minh}-2")));
        // A file where two share one: the later is given another.
        let shared = format!(
            r#"{{"version":1,"people":[{{"name":"Sam","prints":[{:?}],"added_unix_s":1,"id":"{minh}"}},{{"name":"Minh","prints":[{:?}],"added_unix_s":1,"id":"{minh}"}}]}}"#,
            print(0.3),
            print(1.7)
        );
        std::fs::write(&path, shared).unwrap();
        let people = People::open(path.clone()).unwrap().with_stable_ids(true);
        assert_eq!(people.id_of("Sam"), Some(minh.clone()));
        assert_eq!(people.id_of("Minh"), Some(format!("{minh}-2")));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Ids already held go first: a file an older eyes build saved, a new
    /// "Minh" with no id before Sam (renamed from Minh, holding Minh's id).
    /// Sam keeps the id, so Sam's memory stays; the new Minh gets `-2`.
    #[test]
    fn a_held_id_is_kept_before_an_earlier_record_with_none_is_given_one() {
        use lamp_msgs::known::person_id;
        let path = file("stable-order");
        let minh = person_id("Minh");
        let older = format!(
            r#"{{"version":1,"people":[{{"name":"Minh","prints":[{:?}],"added_unix_s":1}},{{"name":"Sam","prints":[{:?}],"added_unix_s":1,"id":"{minh}"}}]}}"#,
            print(1.7),
            print(0.3)
        );
        std::fs::write(&path, older).unwrap();
        let people = People::open(path.clone()).unwrap().with_stable_ids(true);
        assert_eq!(people.id_of("Sam"), Some(minh.clone()));
        assert_eq!(people.id_of("Minh"), Some(format!("{minh}-2")));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// The reviewer-lead's chain on #2330: "Lan" renamed "Mai" keeps `p-` of
    /// "Lan"; then "Minh" renamed "Lan" keeps `p-` of "Minh". Forgetting
    /// "Lan" says her by-name id is not free (Mai holds it), so the app's
    /// remove leaves Mai's memory. One person never renamed, forgotten, is
    /// free; and off, the name is always the only key, so free.
    #[test]
    fn a_forget_says_whether_the_names_memory_id_is_someone_elses() {
        use lamp_msgs::known::person_id;
        let path = file("stable-chain");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        people.remember("Lan", vec![print(0.3)], 1).unwrap();
        people
            .remember_one("Mai", vec![print(0.3)], 2, true, false)
            .unwrap();
        people
            .remember_one("Minh", vec![print(1.7)], 3, false, true)
            .unwrap();
        people
            .remember_one("Lan", vec![print(1.7)], 4, true, false)
            .unwrap();
        assert_eq!(people.id_of("Mai"), Some(person_id("Lan")));
        assert_eq!(people.id_of("Lan"), Some(person_id("Minh")));
        people.forget_at("Lan", 5).unwrap();
        assert!(!people.name_id_free("Lan"), "Mai holds Lan's by-name id");
        assert_eq!(people.id_of("Mai"), Some(person_id("Lan")));
        // Mai herself, forgotten: her name's id is nobody's.
        people.forget_at("Mai", 6).unwrap();
        assert!(people.name_id_free("Mai"));
        assert!(people.name_id_free("Lan"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        // One person, never renamed: free once forgotten.
        let path = file("stable-plain");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(true);
        people.remember("Lan", vec![print(0.3)], 1).unwrap();
        people.forget_at("Lan", 2).unwrap();
        assert!(people.name_id_free("Lan"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        // Off: free, whatever ids an earlier run left in the file.
        let path = file("stable-chain-off");
        let held = format!(
            r#"{{"version":1,"people":[{{"name":"Mai","prints":[{:?}],"added_unix_s":1,"id":"{}"}}]}}"#,
            print(0.3),
            person_id("Lan")
        );
        std::fs::write(&path, held).unwrap();
        let people = People::open(path.clone()).unwrap().with_stable_ids(false);
        assert!(people.name_id_free("Lan"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Off (the default), nothing changes: no id in the people file or the
    /// known file, byte for byte as before, and a rename carries nothing.
    #[test]
    fn without_the_stable_id_the_files_are_as_before() {
        let path = file("stable-off");
        let mut people = People::open(path.clone()).unwrap().with_stable_ids(false);
        people.remember("Minh", vec![print(0.3)], 1).unwrap();
        people
            .remember_one("Sam", vec![print(0.3)], 2, true, false)
            .unwrap();
        assert_eq!(people.id_of("Sam"), None);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"id\""), "{text}");
        let kept: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<&String> = kept["people"][0].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["added_unix_s", "name", "prints"]);
        assert_eq!(kept["people"].as_array().unwrap().len(), 1);
        let known = path.with_file_name("known");
        keep_known(&known, Some("Sam"), people.id_of("Sam").as_deref(), None);
        assert_eq!(std::fs::read_to_string(&known).unwrap(), "Sam\n");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
