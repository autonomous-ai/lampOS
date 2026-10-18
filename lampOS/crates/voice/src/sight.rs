//! One picture from the lamp's eyes with what a person says.
//!
//! "What do you see in front of you?" was among the first things asked of
//! the lamp by people standing at it (2026-10-13), and it answered that it
//! could not see. Its eyes (`lamp-eyes --sight-file`) keep their newest
//! frame in a private file while the camera is open. As a person begins to
//! speak this offers that frame to the model, through the same
//! `Supervisor::try_image` as the camera context: inside the request's own
//! activity, never replayed. It does so at most once every thirty seconds.
//!
//! Not with everything said, because a picture is not free. On lamp-4ace
//! seven ordinary questions in each of six fresh sessions began to be
//! answered a median 1.32 s after the end of speech was decided with no
//! picture (19 replies) and 1.78 s with one in every question (20 replies).
//! Half a second on every answer is too much to pay for something asked
//! about now and then. Every thirty seconds the lamp has a recent look at
//! the room in what the model remembers, and most answers pay nothing. What
//! this gives up: something held up to the lamp less than thirty seconds
//! after its last look is not seen until the next one.
//!
//! A frame older than a second is not offered: the eyes are closed, and the
//! model is told nothing rather than something stale. The eyes open only
//! while this service permits them, so a muted lamp has no file to read.
//! Pictures offered here go to the provider as the person's voice does;
//! nothing is kept on the lamp but that one newest frame.
use lamp_gemini::{Connect, MAX_IMAGE_BYTES, RequestId, Supervisor};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

/// The provider worker's flag, and where the coordinator reads the path.
pub const FLAG: &str = "--eyes-sight";
pub const ENVIRONMENT: &str = "LAMP_EYES_SIGHT_FILE";
/// The same file used another way: no picture goes with what is said, and
/// one goes with a request that is the lamp taking a look because it was
/// asked to (`ProviderInput::Written` with `look`). Ordinary answers then
/// carry no picture and pay nothing for one.
pub const LOOK_FLAG: &str = "--eyes-look";
pub const LOOK_ENVIRONMENT: &str = "LAMP_EYES_LOOK_FILE";
/// A full look (#836), `on` only: as the lamp says it will look, the eyes
/// are asked for one larger still taken once the head has settled
/// (`lamp-eyes --full-look`), the look waits for it a little, and the model
/// is told to say what it cannot make out rather than guess. The worker is
/// told with a flag after the look's path.
pub const LOOK_FULL_ENVIRONMENT: &str = "LAMP_LOOK_FULL";
pub const LOOK_FULL_FLAG: &str = "--eyes-look-full";
/// The still's longer side for every full look, in pixels from 320 to
/// 4096 (`LAMP_LOOK_FULL_SIDE`, off unless set): the eyes make it up to
/// that side and 184 KiB (`lamp-eyes`, `{"look_full":ID,"side":N}`), for
/// writing on paper (#941). Unset, a look's still is 1024 across and 64 KiB
/// as before. One look may ask its own side instead ([`Look::side_next`]).
pub const LOOK_FULL_SIDE_ENVIRONMENT: &str = "LAMP_LOOK_FULL_SIDE";
/// The sides a look may ask for: what the eyes take.
pub const LOOK_SIDES: std::ops::RangeInclusive<u32> = 320..=4_096;

/// `LAMP_LOOK_FULL_SIDE`, when it is a side the eyes take.
pub fn side_from_environment() -> Option<u32> {
    std::env::var(LOOK_FULL_SIDE_ENVIRONMENT)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|side| LOOK_SIDES.contains(side))
}

/// A look that has not begun this long after the lamp said it would look
/// is dropped.
pub const LOOK_BEGINS_WITHIN: Duration = Duration::from_secs(15);
/// How old a full look's still may be when the look goes. It is taken while
/// the lamp is still saying it will look, and the look goes once the lamp
/// has finished saying so, up to [`LOOK_BEGINS_WITHIN`] later; the eyes keep
/// it as long. At 3 s, as first built, a look that went more than 3 s after
/// its still was made went without it (#842, lamp-4ace 03:57).
pub const FULL_FRESH: Duration = LOOK_BEGINS_WITHIN;
/// How long the look waits for the still after the lamp said it would
/// look, at most: the eyes give it up after 2.5 s.
pub const STILL_WAIT: Duration = Duration::from_millis(2_600);

const FRESH: Duration = Duration::from_secs(1);
/// The least time between two looks, whichever requests they fall in.
const BETWEEN_LOOKS: Duration = Duration::from_secs(30);
/// The provider takes two pictures a request.
const LOOKS_A_REQUEST: u8 = 2;
/// One standing permission: the eyes' own permit is what closes them.
const GRANT: u64 = 1;

pub struct Sight {
    path: PathBuf,
    request: Option<RequestId>,
    looked_at: Option<Instant>,
    looks: u8,
    frames: u64,
}

fn usable(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 200
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

/// The provider worker's arguments for the path in the environment: none
/// when it is unset, so a worker started without it is started as before.
pub fn arguments_from_environment() -> Vec<String> {
    let named = |environment: &str, flag: &str| {
        std::env::var(environment)
            .ok()
            .filter(|path| usable(Path::new(path)))
            .map(|path| vec![flag.to_owned(), path])
            .unwrap_or_default()
    };
    let mut arguments = named(ENVIRONMENT, FLAG);
    let look = named(LOOK_ENVIRONMENT, LOOK_FLAG);
    // A full look reads its still from its own file; the eyes' working
    // frame stays where it was for whoever else reads it (#873).
    match full_look_file().filter(|_| !look.is_empty()) {
        Some(path) => arguments.extend([
            LOOK_FLAG.to_owned(),
            path.to_string_lossy().into_owned(),
            LOOK_FULL_FLAG.to_owned(),
        ]),
        None => arguments.extend(look),
    }
    arguments
}

/// The file the eyes keep a full look's still in
/// (`lamp-eyes --full-look-file`), apart from their working frame.
pub const FULL_LOOK_FILE_ENVIRONMENT: &str = "LAMP_EYES_FULL_LOOK_FILE";

/// The full look's file, when a look is a full look: `LAMP_LOOK_FULL=on`
/// and a usable `LAMP_EYES_FULL_LOOK_FILE`. Either missing: no full look.
pub fn full_look_file() -> Option<PathBuf> {
    look_full_on()
        .then(|| std::env::var_os(FULL_LOOK_FILE_ENVIRONMENT))
        .flatten()
        .map(PathBuf::from)
        .filter(|path| usable(path))
}

/// Whether a look is a full look (`LAMP_LOOK_FULL=on`).
pub fn look_full_on() -> bool {
    std::env::var(LOOK_FULL_ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Take a leading `--eyes-look-full` off a worker's flags.
pub fn take_look_full(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == LOOK_FULL_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The id of the ask a still was taken for, from the JPEG comment the eyes
/// put right after the start of the image (`lamp-eyes`, `full_look::tagged`):
/// `FF D8 FF FE`, the length, `lamp-look ID`.
pub fn still_id(jpeg: &[u8]) -> Option<u64> {
    let [0xff, 0xd8, 0xff, 0xfe, high, low, ..] = *jpeg else {
        return None;
    };
    let text = jpeg.get(6..4 + usize::from(u16::from_be_bytes([high, low])))?;
    std::str::from_utf8(text.strip_prefix(b"lamp-look ")?)
        .ok()?
        .parse()
        .ok()
}

/// The coordinator's side of a look, full or not.
#[derive(Debug)]
pub struct Look {
    /// The eyes' picture, when a look is a full look.
    full: Option<PathBuf>,
    /// The turn whose still was asked for, and the ask's id.
    asked: Option<(u64, u64)>,
    /// The next ask's id: from the clock when this began, so that a still
    /// of a service that stopped is not taken for one of the next.
    next_id: u64,
    /// The still's side for every look, and for the next one only.
    side: Option<u32>,
    side_next: Option<u32>,
}

impl Look {
    /// From `LAMP_LOOK_FULL` and the look's file, for a lamp that looks.
    pub fn from_environment() -> Self {
        Self::new(full_look_file()).with_side(side_from_environment())
    }

    /// Every look's still at this side; `None`, as before.
    pub fn with_side(mut self, side: Option<u32>) -> Self {
        self.side = side.filter(|side| LOOK_SIDES.contains(side));
        self
    }

    /// The next look only at this side: what a skill tool's look step asks
    /// for (`look = { side = N }`, #941). A side the eyes do not take is
    /// ignored. Taken by the next [`Self::ask`].
    pub fn side_next(&mut self, side: u32) {
        self.side_next = LOOK_SIDES.contains(&side).then_some(side);
    }

    pub fn new(full: Option<PathBuf>) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |since| since.as_millis() as u64);
        Self {
            full,
            asked: None,
            next_id: now_ms.max(1),
            side: None,
            side_next: None,
        }
    }

    pub fn full(&self) -> bool {
        self.full.is_some()
    }

    /// The lamp said in `turn` that it will look: the id to ask the eyes
    /// for a still with, once, and the still's side if one is asked for.
    /// Never without a full look.
    pub fn ask(&mut self, turn: u64) -> Option<(u64, Option<u32>)> {
        if self.full.is_none() || self.asked.is_some_and(|(of, _)| of == turn) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.asked = Some((turn, id));
        Some((id, self.side_next.take().or(self.side)))
    }

    /// Whether the eyes' file holds the still taken for this ask: one with
    /// its id, not one of an earlier ask written late.
    pub fn still(&self) -> bool {
        self.full
            .as_deref()
            .zip(self.asked)
            .is_some_and(|(path, (_, id))| {
                std::fs::read(path).is_ok_and(|jpeg| still_id(&jpeg) == Some(id))
            })
    }

    /// For the journal, when a full look goes without its still: the id
    /// asked for, what the file held instead and how old it was. No picture.
    pub fn missing(&self, now: SystemTime) -> String {
        let asked = self.asked.map_or(0, |(_, id)| id);
        let Some(path) = self.full.as_deref() else {
            return format!("asked for {asked}, no file");
        };
        let held = match std::fs::read(path) {
            Err(_) => return format!("asked for {asked}, the file held nothing"),
            Ok(jpeg) => match still_id(&jpeg) {
                Some(id) => format!("the still of ask {id}"),
                None => "a picture with no ask's id".to_owned(),
            },
        };
        let age = std::fs::metadata(path)
            .and_then(|about| about.modified())
            .ok()
            .and_then(|at| now.duration_since(at).ok())
            .unwrap_or_default();
        format!(
            "asked for {asked}, the file held {held}, {} ms old",
            age.as_millis()
        )
    }

    /// Whether the look may go, `waited` after the lamp said it would look:
    /// at once without a full look; with one, once the still is there or
    /// after [`STILL_WAIT`].
    pub fn due(&self, waited: Duration) -> bool {
        self.full.is_none() || waited >= STILL_WAIT || self.still()
    }

    /// What the look's turn says, and whether the eyes' picture goes with
    /// it. A full look whose still was not written since this look asked
    /// for it says the eye was shut and takes no picture: the file may
    /// still hold an earlier look's still, which is not this one's.
    pub fn turn(&self) -> (&'static str, bool) {
        match (self.full(), self.still()) {
            (false, _) => (crate::speak_up::LOOK_NOTE, true),
            (true, true) => (crate::speak_up::LOOK_FULL_NOTE, true),
            (true, false) => (crate::speak_up::NO_LOOK_NOTE, false),
        }
    }
}

/// Take a leading `--eyes-look PATH` off a worker's flags.
pub fn take_look(flags: &[String]) -> io::Result<(Option<PathBuf>, &[String])> {
    match flags {
        [flag, path, rest @ ..] if flag == LOOK_FLAG && usable(Path::new(path)) => {
            Ok((Some(path.into()), rest))
        }
        [flag, ..] if flag == LOOK_FLAG => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the eyes' picture needs a short absolute path",
        )),
        _ => Ok((None, flags)),
    }
}

/// The eyes' newest frame and its age, if they are open: a file written in
/// the last second, of a size the provider takes.
pub fn fresh(path: &Path, now: SystemTime) -> Result<(Vec<u8>, Duration), &'static str> {
    fresh_within(path, now, FRESH)
}

/// The same, written in the last `within`.
pub fn fresh_within(
    path: &Path,
    now: SystemTime,
    within: Duration,
) -> Result<(Vec<u8>, Duration), &'static str> {
    fresh_up_to(path, now, within, MAX_IMAGE_BYTES)
}

/// The same, of at most `most` bytes: a full look's still may be one asked
/// at a side of its own, up to `lamp_gemini::MAX_LOOK_IMAGE_BYTES` (#941).
pub fn fresh_up_to(
    path: &Path,
    now: SystemTime,
    within: Duration,
    most: usize,
) -> Result<(Vec<u8>, Duration), &'static str> {
    let about = std::fs::metadata(path).map_err(|_| "the eyes have no picture")?;
    let age = about
        .modified()
        .ok()
        .and_then(|at| now.duration_since(at).ok())
        .unwrap_or_default();
    if age > within {
        return Err("the eyes' picture is old");
    }
    if about.len() > most as u64 {
        return Err("the eyes' picture is too large");
    }
    let jpeg = std::fs::read(path).map_err(|_| "the eyes have no picture")?;
    Ok((jpeg, age))
}

impl Sight {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        if !usable(&path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the eyes' picture needs a short absolute path",
            ));
        }
        Ok(Self {
            path,
            request: None,
            looked_at: None,
            looks: 0,
            frames: 0,
        })
    }

    /// Take a leading `--eyes-sight PATH` off a worker's flags.
    pub fn take(flags: &[String]) -> io::Result<(Option<Self>, &[String])> {
        match flags {
            [flag, path, rest @ ..] if flag == FLAG => Ok((Some(Self::new(path.into())?), rest)),
            [flag] if flag == FLAG => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the eyes' picture needs a path",
            )),
            _ => Ok((None, flags)),
        }
    }

    /// Whether to look now, with this request's input open: when the last
    /// look, in whichever request, was thirty seconds ago or more, and this
    /// request has not had its two.
    fn due(&mut self, request: RequestId, now: Instant) -> bool {
        if self.request != Some(request) {
            self.request = Some(request);
            self.looks = 0;
        }
        let due = self.looks < LOOKS_A_REQUEST
            && self
                .looked_at
                .is_none_or(|at| now.saturating_duration_since(at) >= BETWEEN_LOOKS);
        if due {
            self.looks += 1;
            self.looked_at = Some(now);
        }
        due
    }

    /// The newest frame and its age, if the eyes are open.
    fn picture(&self, now: SystemTime) -> Result<(Vec<u8>, Duration), &'static str> {
        fresh(&self.path, now)
    }

    /// Call every tick with the request whose input is open, if any.
    /// The request a picture went with, if one did.
    pub fn offer<C: Connect>(
        &mut self,
        request: Option<RequestId>,
        provider: &Supervisor<C>,
    ) -> Option<RequestId> {
        let request = request?;
        let now = Instant::now();
        if !self.due(request, now) {
            return None;
        }
        let outcome = self.picture(SystemTime::now()).and_then(|(jpeg, age)| {
            self.frames += 1;
            let captured = now.checked_sub(age).unwrap_or(now);
            provider
                .try_image(request, self.frames, GRANT, captured, &jpeg)
                .map(|_| (jpeg.len(), age))
                .map_err(|_| "the provider did not take it")
        });
        match outcome {
            Ok((bytes, age)) => {
                eprintln!(
                    "lamp-voice provider: sight: a picture of {bytes} bytes, {} ms old, went with request {} (look {})",
                    age.as_millis(),
                    request.get(),
                    self.looks
                );
                Some(request)
            }
            Err(why) => {
                eprintln!(
                    "lamp-voice provider: sight: no picture with request {} (look {}): {why}",
                    request.get(),
                    self.looks
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(number: u64) -> RequestId {
        RequestId::new(number).unwrap()
    }

    #[test]
    fn the_path_travels_as_one_leading_flag_or_not_at_all() {
        let flags = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        let given = flags(&[FLAG, "/run/lampos-eyes/sight.jpg", "--camera-context", "x"]);
        let (sight, rest) = Sight::take(&given).unwrap();
        assert_eq!(
            sight.unwrap().path,
            PathBuf::from("/run/lampos-eyes/sight.jpg")
        );
        assert_eq!(rest, &given[2..]);
        let none = flags(&["--camera-context", "x"]);
        let (sight, rest) = Sight::take(&none).unwrap();
        assert!(sight.is_none());
        assert_eq!(rest, &none[..]);
        assert!(Sight::take(&flags(&[FLAG])).is_err());
        // The look flag is its own, after it.
        let both = flags(&[LOOK_FLAG, "/run/lampos-eyes/sight.jpg", "--camera-context"]);
        let (look, rest) = take_look(&both).unwrap();
        assert_eq!(look, Some(PathBuf::from("/run/lampos-eyes/sight.jpg")));
        assert_eq!(rest, &both[2..]);
        assert_eq!(take_look(&none).unwrap(), (None, &none[..]));
        assert!(take_look(&flags(&[LOOK_FLAG])).is_err());
        assert!(take_look(&flags(&[LOOK_FLAG, "sight.jpg"])).is_err());
        assert!(Sight::take(&flags(&[FLAG, "sight.jpg"])).is_err());
        assert!(Sight::take(&flags(&[FLAG, "/run/../etc/passwd"])).is_err());
    }

    #[test]
    fn it_looks_as_someone_begins_and_then_no_more_than_every_thirty_seconds() {
        let mut sight = Sight::new("/run/x.jpg".into()).unwrap();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        // The first thing said gets a look as it begins, and no second one
        // however it goes on in the next half minute.
        assert!(sight.due(request(5), at(0)));
        assert!(!sight.due(request(5), at(2)));
        assert!(!sight.due(request(5), at(29_999)));
        // A conversation: the questions that follow get none until thirty
        // seconds after that look, and then the next one does.
        assert!(!sight.due(request(6), at(9_000)));
        assert!(!sight.due(request(7), at(18_000)));
        assert!(!sight.due(request(8), at(27_000)));
        assert!(sight.due(request(9), at(36_000)));
        assert!(!sight.due(request(10), at(45_000)));
        // Someone who speaks for a very long time gets a second, no third.
        assert!(sight.due(request(11), at(100_000)));
        assert!(sight.due(request(11), at(130_000)));
        assert!(!sight.due(request(11), at(170_000)));
        assert_eq!(sight.looks, 2);
    }

    #[test]
    fn only_a_fresh_picture_of_a_size_the_provider_takes_is_read() {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-sight-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sight.jpg");
        let sight = Sight::new(path.clone()).unwrap();
        let now = SystemTime::now();
        assert_eq!(sight.picture(now), Err("the eyes have no picture"));
        std::fs::write(&path, b"frame").unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        let (jpeg, age) = sight.picture(written + Duration::from_millis(300)).unwrap();
        assert_eq!(
            (jpeg.as_slice(), age),
            (&b"frame"[..], Duration::from_millis(300))
        );
        // A clock a moment behind the file's is no age, not an error.
        assert_eq!(
            sight.picture(written - Duration::from_millis(5)).unwrap().1,
            Duration::ZERO
        );
        assert_eq!(
            sight.picture(written + Duration::from_millis(1_001)),
            Err("the eyes' picture is old")
        );
        std::fs::write(&path, vec![0u8; MAX_IMAGE_BYTES + 1]).unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            sight.picture(written),
            Err("the eyes' picture is too large")
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn without_a_full_look_a_look_goes_at_once_with_the_old_note_and_asks_nothing() {
        let mut look = Look::new(None);
        assert!(!look.full());
        assert_eq!(look.ask(7), None);
        assert!(look.due(Duration::ZERO));
        assert!(!look.still());
        // The picture goes, with the note as it was.
        assert_eq!(look.turn(), (crate::speak_up::LOOK_NOTE, true));
        // And the worker is given no flag, so it reads as before.
        let flags = vec!["--camera-context".to_owned()];
        assert_eq!(take_look_full(&flags), (false, &flags[..]));
    }

    /// A still as the eyes write it (`lamp_eyes::full_look::tagged`).
    fn still_of(id: u64) -> Vec<u8> {
        let text = format!("lamp-look {id}");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xfe, 0, text.len() as u8 + 2];
        jpeg.extend_from_slice(text.as_bytes());
        jpeg.extend_from_slice(&[0xff, 0xdb, 0, 2, 0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn a_still_s_id_is_read_from_its_comment_and_nothing_else_has_one() {
        assert_eq!(still_id(&still_of(42)), Some(42));
        assert_eq!(still_id(&still_of(u64::MAX)), Some(u64::MAX));
        // A camera frame, a cut-off still, another comment.
        assert_eq!(still_id(&[0xff, 0xd8, 0xff, 0xdb, 0, 2]), None);
        assert_eq!(still_id(&still_of(42)[..10]), None);
        assert_eq!(still_id(b"\xff\xd8\xff\xfe\x00\x07hello"), None);
        assert_eq!(still_id(b""), None);
    }

    #[test]
    fn a_full_look_asks_once_and_waits_for_the_still_with_its_own_id() {
        let directory = scratch("full-look");
        let path = directory.join("sight.jpg");
        // A working frame, or an earlier look's still, is not the still.
        std::fs::write(&path, b"\xff\xd8\xff\xdb\x00\x02").unwrap();
        let mut look = Look::new(Some(path.clone()));
        assert!(look.full());
        let id = look.ask(7).unwrap().0;
        assert_eq!(look.ask(7), None);
        assert!(!look.still());
        assert!(!look.due(Duration::from_millis(900)));
        std::fs::write(&path, still_of(id - 1)).unwrap();
        assert!(!look.still());
        // The eyes write its still: the look goes, with it and the note
        // that says not to guess.
        std::fs::write(&path, still_of(id)).unwrap();
        assert!(look.still());
        assert!(look.due(Duration::from_millis(900)));
        let (note, with_picture) = look.turn();
        assert!(with_picture);
        assert_eq!(note, crate::speak_up::LOOK_FULL_NOTE);
        assert!(note.contains("hold it a bit closer"));
        assert!(note.contains("never guess"));
        assert!(!crate::speak_up::LOOK_NOTE.contains("closer"));
        // The next look asks again, with another id.
        let next = look.ask(9).unwrap().0;
        assert_ne!(next, id);
        assert!(!look.still());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_look_asks_no_side_unless_one_is_set_and_one_look_may_ask_its_own() {
        let path = PathBuf::from("/run/lampos-eyes/full-look.jpg");
        // Unset: no side, as before.
        let mut look = Look::new(Some(path.clone()));
        assert_eq!(look.ask(1).unwrap().1, None);
        // One look at its own side, the next as before.
        look.side_next(3_264);
        assert_eq!(look.ask(2).unwrap().1, Some(3_264));
        assert_eq!(look.ask(3).unwrap().1, None);
        // A side the eyes do not take is none.
        look.side_next(100);
        assert_eq!(look.ask(4).unwrap().1, None);
        // Every look at a side, one look at another.
        let mut look = Look::new(Some(path)).with_side(Some(2_048));
        assert_eq!(look.ask(1).unwrap().1, Some(2_048));
        look.side_next(3_264);
        assert_eq!(look.ask(2).unwrap().1, Some(3_264));
        assert_eq!(look.ask(3).unwrap().1, Some(2_048));
        assert_eq!(
            Look::new(None).with_side(Some(5_000)).side,
            None,
            "outside 320 to 4096"
        );
    }

    #[test]
    fn a_sided_still_is_read_up_to_the_look_bound_and_a_frame_only_to_the_old_one() {
        let directory = scratch("sided-still");
        let path = directory.join("full.jpg");
        std::fs::write(&path, vec![0u8; MAX_IMAGE_BYTES + 1]).unwrap();
        let now = SystemTime::now();
        assert!(fresh_within(&path, now, FULL_FRESH).is_err());
        let most = lamp_gemini::MAX_LOOK_IMAGE_BYTES;
        assert!(fresh_up_to(&path, now, FULL_FRESH, most).is_ok());
        std::fs::write(&path, vec![0u8; most + 1]).unwrap();
        assert!(fresh_up_to(&path, now, FULL_FRESH, most).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_full_look_without_its_own_still_says_the_eye_was_shut_and_takes_no_picture() {
        let directory = scratch("full-look-none");
        let path = directory.join("sight.jpg");
        // No file at all.
        let mut look = Look::new(Some(path.clone()));
        let first = look.ask(3).unwrap().0;
        assert!(!look.due(STILL_WAIT - Duration::from_millis(1)));
        assert!(look.due(STILL_WAIT));
        assert_eq!(look.turn(), (crate::speak_up::NO_LOOK_NOTE, false));
        // Reviewer's case on #842: an earlier look's still is in the file
        // (the eyes keep one 3 s) and the worker would take it at the wait's
        // end; this look's capture fails. No picture goes.
        std::fs::write(&path, still_of(first)).unwrap();
        assert!(look.ask(4).is_some());
        assert!(fresh_within(&path, SystemTime::now(), FULL_FRESH).is_ok());
        assert!(look.due(STILL_WAIT));
        assert_eq!(look.turn(), (crate::speak_up::NO_LOOK_NOTE, false));
        // And the overlapping case: the earlier, cancelled look's still is
        // written after this look asked, so later than the ask by any
        // clock. Its id is not this one's.
        std::fs::remove_file(&path).unwrap();
        let cancelled = look.ask(5).unwrap().0;
        let asked = look.ask(6).unwrap().0;
        std::fs::write(&path, still_of(cancelled)).unwrap();
        assert!(!look.still());
        assert!(!look.due(Duration::from_millis(900)));
        assert_eq!(look.turn(), (crate::speak_up::NO_LOOK_NOTE, false));
        std::fs::write(&path, still_of(asked)).unwrap();
        assert_eq!(look.turn(), (crate::speak_up::LOOK_FULL_NOTE, true));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_full_look_without_its_still_says_in_one_line_what_the_file_held() {
        let directory = scratch("full-look-missing");
        let path = directory.join("look.jpg");
        let mut look = Look::new(Some(path.clone()));
        let id = look.ask(3).unwrap().0;
        let now = SystemTime::now();
        assert_eq!(
            look.missing(now),
            format!("asked for {id}, the file held nothing")
        );
        std::fs::write(&path, still_of(id - 1)).unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            look.missing(written + Duration::from_millis(3_200)),
            format!(
                "asked for {id}, the file held the still of ask {}, 3200 ms old",
                id - 1
            )
        );
        std::fs::write(&path, b"\xff\xd8\xff\xdb\x00\x02").unwrap();
        let line = look.missing(SystemTime::now());
        assert!(line.contains("a picture with no ask's id"), "{line}");
        // The look's whole wait fits the still's life on the lamp.
        assert!(FULL_FRESH >= LOOK_BEGINS_WITHIN);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_full_look_s_still_may_be_older_than_a_working_frame() {
        let directory = scratch("full-fresh");
        let path = directory.join("sight.jpg");
        std::fs::write(&path, b"still").unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        let later = written + Duration::from_millis(1_800);
        assert_eq!(fresh(&path, later), Err("the eyes' picture is old"));
        assert_eq!(
            fresh_within(&path, later, FULL_FRESH).unwrap().0,
            b"still".to_vec()
        );
        // #842: a look that begins more than 3 s after its still was made,
        // the lamp still saying it would look, still takes it.
        assert!(fresh_within(&path, written + Duration::from_millis(5_100), FULL_FRESH).is_ok());
        assert!(
            fresh_within(
                &path,
                written + FULL_FRESH + Duration::from_millis(1),
                FULL_FRESH
            )
            .is_err()
        );
        let flags = vec![LOOK_FULL_FLAG.to_owned(), "--camera-context".to_owned()];
        assert_eq!(take_look_full(&flags), (true, &flags[1..]));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
