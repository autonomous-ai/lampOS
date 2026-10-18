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
    os::unix::fs::OpenOptionsExt,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub name: String,
    pub prints: Vec<Vec<f32>>,
    pub added_unix_s: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    version: u32,
    people: Vec<Person>,
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
}

impl People {
    /// What the file holds; none at all if there is no file yet. A file
    /// that is too large or not of this shape is an error, never a guess.
    pub fn open(path: PathBuf) -> Result<Self> {
        let people = match File::open(&path) {
            Ok(file) => {
                let mut text = String::new();
                file.take(MOST_FILE_BYTES + 1).read_to_string(&mut text)?;
                if text.len() as u64 > MOST_FILE_BYTES {
                    return Err(format!("{} is too large", path.display()).into());
                }
                let kept: Kept = serde_json::from_str(&text)?;
                if kept.version != 1 || !valid(&kept.people) {
                    return Err(format!("{} is not a list of people", path.display()).into());
                }
                kept.people
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, people })
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
    pub fn forget(&mut self, name: &str) -> Result<bool> {
        let mut next = self.people.clone();
        next.retain(|person| !same_name(&person.name, name.trim()));
        if next.len() == self.people.len() {
            return Ok(false);
        }
        self.save(&next)?;
        self.people = next;
        Ok(true)
    }

    fn save(&self, people: &[Person]) -> Result<()> {
        let text = serde_json::to_vec(&Kept {
            version: 1,
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
    /// build doesn't know at its top and in a person. The same file without
    /// them opens; with them, lamp-eyes refuses it today and doesn't start.
    /// `el/people-forward` makes it read: this test then stops panicking, and
    /// becomes a plain test in that pull request (Tests).
    #[test]
    #[should_panic(expected = "unknown field")]
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
        assert!(people.forget("A").unwrap());
        assert!(!people.forget("A").unwrap());
        assert!(People::open(path).unwrap().names().is_empty());
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
    fn names_prints_and_people_are_bounded_and_a_bad_file_is_refused() {
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
        std::fs::write(
            &path,
            b"{\"version\":1,\"people\":[{\"name\":\"x;y\",\"prints\":[],\"added_unix_s\":1}]}",
        )
        .unwrap();
        assert!(People::open(path.clone()).is_err());
        std::fs::write(&path, vec![b' '; MOST_FILE_BYTES as usize + 1]).unwrap();
        assert!(People::open(path).is_err());
    }
}
