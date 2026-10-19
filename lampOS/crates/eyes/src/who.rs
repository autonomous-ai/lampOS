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
use crate::picture::{HEIGHT, Picture, WIDTH};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
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
    let (a, b, x, y) = similarity(marks, &REFERENCE)?;
    // Back from the recogniser's picture to the frame.
    let scale = a * a + b * b;
    let (ia, ib) = (a / scale, -b / scale);
    let (filled_x, filled_y) = (picture.filled.0 as f32, picture.filled.1 as f32);
    let plane = WIDTH * HEIGHT;
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
            let (u1, v1) = (
                (u0 + 1).min(picture.filled.0 - 1),
                (v0 + 1).min(picture.filled.1 - 1),
            );
            let (du, dv) = (u - u0 as f32, v - v0 as f32);
            let at = |c: usize, uu: usize, vv: usize| picture.planes[c * plane + vv * WIDTH + uu];
            // The picture's planes are blue, green, red; the recogniser's are
            // red, green, blue.
            for (to, from) in [(0usize, 2usize), (1, 1), (2, 0)] {
                let value = at(from, u0, v0) * (1.0 - du) * (1.0 - dv)
                    + at(from, u1, v0) * du * (1.0 - dv)
                    + at(from, u0, v1) * (1.0 - du) * dv
                    + at(from, u1, v1) * du * dv;
                out[to * SIDE * SIDE + row * SIDE + column] = value;
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

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
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
    /// with a count and never a name. Only a failure to set aside is an
    /// error: the eyes do not start rather than lose a record.
    pub fn open(path: PathBuf) -> Result<Self> {
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
        Ok(opened)
    }

    pub fn names(&self) -> Vec<String> {
        self.people
            .iter()
            .map(|person| person.name.clone())
            .collect()
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
        let mut next = self.people.clone();
        next.retain(|person| !same_name(&person.name, &name));
        if next.len() >= MOST_PEOPLE {
            return Err("the lamp already knows as many people as it keeps".into());
        }
        next.push(Person {
            name: name.clone(),
            prints,
            added_unix_s: now_unix_s,
            familiar: None,
        });
        self.save(&next)?;
        self.people = next;
        Ok(name)
    }

    /// The same, keeping one name for one face (`LAMP_NAMES=checked`, #829's
    /// names, the first builder at 13:24 on 16 October): prints that are
    /// already someone's under another name are refused with that name, so
    /// that a name misheard never sits beside the right one with the same
    /// face. With `replace`, asked after the person said yes to the new
    /// name, that person is forgotten first. Every name the prints are
    /// known by is in the refusal, "A and B", since `replace` forgets
    /// them all (the reviewer on #1057).
    pub fn remember_one(
        &mut self,
        name: &str,
        prints: Vec<Vec<f32>>,
        now_unix_s: u64,
        replace: bool,
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
        if !known.is_empty() {
            if !replace {
                return Err(Refused::KnownAs(known.join(" and ")));
            }
            let mut next = self.people.clone();
            next.retain(|person| !known.iter().any(|other| same_name(other, &person.name)));
            self.save(&next)
                .map_err(|error| Refused::Store(error.to_string()))?;
            self.people = next;
        }
        self.remember(&kept, prints, now_unix_s)
            .map_err(|error| Refused::Store(error.to_string()))
    }

    /// Forgets this name's prints, and writes the file before it answers.
    /// Whether there were any.
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
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(".new");
        let temporary = PathBuf::from(temporary);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&text)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

/// What the voice service asks of the eyes on their request socket: one
/// line of JSON, `{"remember":"A"}`, `{"forget":"A"}`,
/// `{"list":true}` or `{"who":true}`. With one name for one face
/// (`LAMP_NAMES=checked`), `{"remember":"A","one_face":true}`, and
/// `"replace":true` once the person said yes to the new name. Anything else
/// is nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Remember(String),
    RememberOne { name: String, replace: bool },
    Forget(String),
    List,
    Who,
}

pub fn ask_of(line: &[u8]) -> Option<Ask> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Line {
        remember: Option<String>,
        forget: Option<String>,
        list: Option<bool>,
        who: Option<bool>,
        one_face: Option<bool>,
        replace: Option<bool>,
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
        } => Some(Ask::Remember(name)),
        Line {
            remember: Some(name),
            forget: None,
            list: None,
            who: None,
            one_face: Some(true),
            replace,
        } => Some(Ask::RememberOne {
            name,
            replace: replace.unwrap_or(false),
        }),
        Line {
            remember: None,
            forget: Some(name),
            list: None,
            who: None,
            one_face: None,
            replace: None,
        } => Some(Ask::Forget(name)),
        Line {
            remember: None,
            forget: None,
            list: Some(true),
            who: None,
            one_face: None,
            replace: None,
        } => Some(Ask::List),
        Line {
            remember: None,
            forget: None,
            list: None,
            who: Some(true),
            one_face: None,
            replace: None,
        } => Some(Ask::Who),
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

/// Remembering someone: their prints, taken over a few looks while they are
/// the one face at the lamp.
#[derive(Debug)]
pub struct Taking {
    pub name: String,
    /// One name for one face, and whether to replace another name for it
    /// (`Ask::RememberOne`); `None`: as asked plainly.
    pub one_face: Option<bool>,
    prints: Vec<Vec<f32>>,
    started_us: u64,
    saw_any: bool,
    saw_far: bool,
    saw_two: bool,
}

/// Why a face was not kept under a name, with one name for one face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    NotAName,
    /// The face is someone the lamp knows by this other name.
    KnownAs(String),
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
            one_face: None,
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
    !is_temporary(file) && file.file_name().is_none_or(|name| name != PENDING_FORGETS)
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
/// ([`PENDING_FORGETS`]); none when there is no such file.
fn pending(path: &Path) -> io::Result<Vec<Waiting>> {
    let file = path
        .parent()
        .unwrap_or(Path::new("/"))
        .join(SET_ASIDE)
        .join(PENDING_FORGETS);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    #[derive(Deserialize)]
    struct Kept {
        forget: Vec<Waiting>,
    }
    serde_json::from_slice::<Kept>(&bytes)
        .map(|kept| kept.forget)
        .map_err(|_| io::Error::other(format!("{} is not a list of forgets", file.display())))
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
        let Some(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .filter(|value| value.get("people").is_some_and(serde_json::Value::is_array))
        else {
            unsearched += 1;
            continue;
        };
        let list = value["people"].as_array_mut().expect("a list");
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

fn valid(people: &[Person]) -> bool {
    people.len() <= MOST_PEOPLE
        && people.iter().enumerate().all(|(index, person)| {
            plain_name(&person.name).as_deref() == Some(person.name.as_str())
                && (1..=MOST_PRINTS).contains(&person.prints.len())
                && person.prints.iter().all(|print| {
                    print.len() == PRINT && print.iter().all(|value| value.is_finite())
                })
                && !people[..index]
                    .iter()
                    .any(|other| same_name(&other.name, &person.name))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn nobody_is_known_until_asked_and_a_forgotten_face_is_gone_from_the_file() {
        let path = file("forget");
        let mut people = People::open(path.clone()).unwrap();
        assert_eq!(people.who(&print(0.3)), None);
        let kept = people
            .remember("  A  ", vec![print(0.3), print(0.31)], 1)
            .unwrap();
        assert_eq!(kept, "A");
        let (name, how) = people.who(&print(0.3)).unwrap();
        assert_eq!(name, "A");
        assert!(how > 0.99);
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
            people.remember_one("B", vec![print(0.3)], 2, false),
            Err(Refused::KnownAs("A".into()))
        );
        assert_eq!(people.names(), vec!["A".to_owned()]);
        // The same name again replaces its own prints, as before.
        assert_eq!(
            people.remember_one("a", vec![print(0.31)], 3, false),
            Ok("a".into())
        );
        // Said yes to: the new name in place of the old.
        assert_eq!(
            people.remember_one("B", vec![print(0.31)], 4, true),
            Ok("B".into())
        );
        assert_eq!(people.names(), vec!["B".to_owned()]);
        assert_eq!(people.who(&print(0.31)).unwrap().0, "B");
        // Someone else is kept beside them.
        assert_eq!(
            people.remember_one("C", vec![print(1.7)], 5, false),
            Ok("C".into())
        );
        assert_eq!(people.names(), vec!["B".to_owned(), "C".to_owned()]);
        assert_eq!(
            people.remember_one("  ", vec![print(2.9)], 6, false),
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
            people.remember_one("B", both.clone(), 3, false),
            Err(Refused::KnownAs("A and C".into()))
        );
        assert_eq!(people.remember_one("B", both, 4, true), Ok("B".into()));
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
                replace: false
            })
        );
        assert_eq!(
            ask_of(br#"{"remember":"A","one_face":true,"replace":true}"#),
            Some(Ask::RememberOne {
                name: "A".into(),
                replace: true
            })
        );
        assert_eq!(ask_of(br#"{"remember":"A","replace":true}"#), None);
        assert_eq!(ask_of(br#"{"forget":"A","one_face":true}"#), None);
        assert_eq!(ask_of(br#"{"list":true}"#), Some(Ask::List));
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
}
