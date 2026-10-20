//! A face from a photo the app sends (`faces.add_photo`, the first builder on
//! #1968, 22:19): lamp-cloud's side. The photo comes over the lamp's own
//! account channel (`fa_channel`, which only the lamp's signed-in account
//! may publish to), is written into the eyes' photo folder (mode 0600, a new
//! file), and the eyes are asked `{"remember":NAME,"photo":FILE}`. The eyes
//! read it once and remove it, and keep its face **unconfirmed** until its
//! person says yes at the lamp (`docs/eyes.md`, "A face from a photo"). If
//! the eyes never take it, lamp-cloud removes it: the photo is never kept.
//!
//! Off unless `LAMP_PHOTO_FACES=on`, with the eyes' folder
//! (`LAMP_EYES_PHOTO_DIR`) and request socket (`LAMP_EYES_WHO_SOCKET`).
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::OpenOptionsExt, net::UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

pub const ENVIRONMENT: &str = "LAMP_PHOTO_FACES";
pub const DIR_ENVIRONMENT: &str = "LAMP_EYES_PHOTO_DIR";
pub const SOCKET_ENVIRONMENT: &str = "LAMP_EYES_WHO_SOCKET";
/// The largest photo taken, as the eyes take it.
pub const MOST_BYTES: usize = 8 << 20;
/// A name as the eyes keep one, at most.
pub const MOST_NAME_CHARS: usize = 40;
/// The eyes decode, find and print the face, between two of the camera's
/// looks: a few hundred ms on a lamp. Bounded short, since the answer is
/// waited for on the runtime that keeps the broker's connection alive (the
/// reviewer on #1979, 22:56). A photo the eyes take after it is still kept
/// unconfirmed, and the person is asked at the lamp as for any other.
pub const DEADLINE: Duration = Duration::from_secs(2);

/// Where the photo goes and whom to ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub dir: PathBuf,
    pub socket: PathBuf,
}

impl Place {
    /// From the environment: `None` unless the switch is on and both paths
    /// are absolute.
    pub fn from_environment() -> Option<Self> {
        Self::from(|name| std::env::var(name).ok())
    }

    pub fn from(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        (var(ENVIRONMENT).as_deref() == Some("on")).then_some(())?;
        let absolute = |name| {
            var(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        Some(Self {
            dir: absolute(DIR_ENVIRONMENT)?,
            socket: absolute(SOCKET_ENVIRONMENT)?,
        })
    }
}

/// The app's `data`: `name` and `photo` (a JPEG, base64), each checked.
pub fn asked(data: &Value) -> Result<(String, Vec<u8>), String> {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= MOST_NAME_CHARS)
        .ok_or("name must be a name of at most 40 characters")?;
    let photo = data
        .get("photo")
        .and_then(Value::as_str)
        .filter(|photo| photo.len() <= MOST_BYTES.div_ceil(3) * 4 + 4)
        .ok_or("photo must be a JPEG of at most 8 MiB, base64")?;
    let bytes = STANDARD
        .decode(photo)
        .map_err(|_| "photo must be a JPEG of at most 8 MiB, base64".to_owned())?;
    if bytes.len() > MOST_BYTES || !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err("photo must be a JPEG of at most 8 MiB, base64".to_owned());
    }
    Ok((name.to_owned(), bytes))
}

/// The person's words for the eyes' reasons.
pub fn told(answer: &Value) -> Result<Value, String> {
    if let Some(name) = answer["added"].as_str() {
        return Ok(json!({"added": name, "unconfirmed": true}));
    }
    if let Some(name) = answer["remembered"].as_str() {
        return Ok(json!({"remembered": name}));
    }
    Err(match answer["error"].as_str() {
        Some("no_face") => "no face could be found in that photo".to_owned(),
        Some("two_faces") => "that photo has more than one person in it".to_owned(),
        Some("too_far") => "the face in that photo is too small".to_owned(),
        Some("unreadable") => "that photo could not be read".to_owned(),
        Some("known_as") => "the lamp already knows that face by another name".to_owned(),
        Some("not_a_name") => "that name can't be kept".to_owned(),
        Some("full") => "the lamp can't keep another face".to_owned(),
        Some("photos_off") => "the lamp's eyes don't take photos".to_owned(),
        _ => "the lamp's eyes could not take that photo".to_owned(),
    })
}

/// Writes the photo as a new file in the folder (0600), asks the eyes, and
/// removes the file if it is still there: answered or not, nothing of it is
/// kept by lamp-cloud. `stamp` names the file.
pub fn add(place: &Place, name: &str, photo: &[u8], stamp: u64) -> Result<Value, String> {
    let file = format!("app-{stamp}.jpg");
    let path = place.dir.join(&file);
    let written = (|| -> std::io::Result<()> {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        out.write_all(photo)?;
        out.sync_all()
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&path);
        return Err("the photo could not be handed to the lamp's eyes".to_owned());
    }
    let answer = exchange(
        &place.socket,
        &json!({"remember": name, "photo": file}),
        DEADLINE,
    );
    // The eyes remove it once read; if they never read it, it goes here.
    let _ = std::fs::remove_file(&path);
    match answer {
        Some(answer) => told(&answer),
        None => Err("the lamp's eyes did not answer".to_owned()),
    }
}

pub(crate) fn exchange(socket: &Path, line: &Value, deadline: Duration) -> Option<Value> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(deadline)).ok()?;
    stream.set_write_timeout(Some(deadline)).ok()?;
    writeln!(stream, "{line}").ok()?;
    let mut answer = String::new();
    BufReader::new(stream.try_clone().ok()?)
        .take_line(&mut answer)
        .ok()?;
    serde_json::from_str(answer.trim()).ok()
}

trait TakeLine {
    fn take_line(self, line: &mut String) -> std::io::Result<usize>;
}

impl<R: std::io::Read> TakeLine for BufReader<R> {
    fn take_line(self, line: &mut String) -> std::io::Result<usize> {
        let mut bounded = BufReader::new(std::io::Read::take(self, 4096));
        bounded.read_line(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cloud-photos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn it_is_off_unless_switched_on_with_both_places() {
        let env = |on: &'static str, dir: &'static str| {
            move |name: &str| match name {
                ENVIRONMENT => Some(on.to_owned()),
                DIR_ENVIRONMENT => Some(dir.to_owned()),
                SOCKET_ENVIRONMENT => Some("/run/lampos-eyes/who.sock".to_owned()),
                _ => None,
            }
        };
        assert_eq!(
            Place::from(env("on", "/run/lampos-eyes/photos")),
            Some(Place {
                dir: "/run/lampos-eyes/photos".into(),
                socket: "/run/lampos-eyes/who.sock".into()
            })
        );
        assert_eq!(Place::from(env("off", "/run/lampos-eyes/photos")), None);
        assert_eq!(Place::from(env("on", "photos")), None);
        assert_eq!(Place::from(|_| None), None);
    }

    #[test]
    fn the_apps_photo_is_a_name_and_a_jpeg_checked() {
        let photo = STANDARD.encode(JPEG);
        let (name, bytes) = asked(&json!({"name": " Lan ", "photo": photo})).unwrap();
        assert_eq!((name.as_str(), bytes.as_slice()), ("Lan", JPEG));
        for wrong in [
            json!({"photo": photo}),
            json!({"name": "", "photo": photo}),
            json!({"name": "x".repeat(41), "photo": photo}),
            json!({"name": "Lan"}),
            json!({"name": "Lan", "photo": "not base64!"}),
            json!({"name": "Lan", "photo": STANDARD.encode(b"GIF89a")}),
            json!({"name": "Lan", "photo": STANDARD.encode(vec![0xFF; MOST_BYTES + 1])}),
        ] {
            assert!(asked(&wrong).is_err(), "{wrong}");
        }
    }

    /// The photo is written as a new 0600 file, the eyes are asked for it
    /// by name, and it is gone afterwards whether they answered or not.
    #[test]
    fn the_photo_goes_to_the_eyes_and_is_never_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = folder("add");
        let socket = dir.join("who.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let seen = dir.clone();
        let eyes = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let asked: Value = serde_json::from_str(line.trim()).unwrap();
            let file = seen.join(asked["photo"].as_str().unwrap());
            // The eyes find it there, the person's own, and take it.
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            let bytes = std::fs::read(&file).unwrap();
            std::fs::remove_file(&file).unwrap();
            writeln!(
                &stream,
                "{}",
                json!({"added": asked["remember"], "unconfirmed": true})
            )
            .unwrap();
            (asked, mode, bytes)
        });
        let place = Place {
            dir: dir.clone(),
            socket: socket.clone(),
        };
        let told = add(&place, "Lan", JPEG, 7).unwrap();
        assert_eq!(told, json!({"added": "Lan", "unconfirmed": true}));
        let (asked, mode, bytes) = eyes.join().unwrap();
        assert_eq!(asked, json!({"remember": "Lan", "photo": "app-7.jpg"}));
        assert_eq!((mode, bytes.as_slice()), (0o600, JPEG));
        // With nobody answering, the photo still goes.
        std::fs::remove_file(&socket).unwrap();
        assert_eq!(
            add(&place, "Lan", JPEG, 8),
            Err("the lamp's eyes did not answer".to_owned())
        );
        assert!(!dir.join("app-8.jpg").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_eyes_reasons_are_said_plainly() {
        assert_eq!(
            told(&json!({"remembered": "Lan"})).unwrap(),
            json!({"remembered": "Lan"})
        );
        assert_eq!(
            told(&json!({"error": "two_faces"})).unwrap_err(),
            "that photo has more than one person in it"
        );
        assert!(told(&json!({"error": "new_reason"})).is_err());
    }
}
