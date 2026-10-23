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
pub mod sweep;

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

/// The app's `data`: a plain sender-supplied name and JPEG, each checked.
/// A sender's label is untrusted data, never a person's confirmation.
pub fn asked(data: &Value) -> Result<(String, Vec<u8>), String> {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .map(|name| name.trim_matches(' '))
        .filter(|name| {
            !name.is_empty()
                && name.chars().count() <= MOST_NAME_CHARS
                && name.chars().any(char::is_alphabetic)
                && name
                    .chars()
                    .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\''))
        })
        .ok_or("name must have at most 40 letters, spaces, hyphens or apostrophes")?;
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

/// A receipt, never an enrollment or recognition result. Any parsed Eyes
/// answer has the same public shape: even its refusal must not reveal whether
/// this face or label is already kept. Receiving does not promise a pending
/// face, a question at the lamp, or a confirmed enrollment.
pub fn told(_answer: &Value) -> Value {
    json!({"received": true})
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
        Some(answer) => Ok(told(&answer)),
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
            Place::from(env("on", "/run/lampos-photos")),
            Some(Place {
                dir: "/run/lampos-photos".into(),
                socket: "/run/lampos-eyes/who.sock".into()
            })
        );
        assert_eq!(Place::from(env("off", "/run/lampos-photos")), None);
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

    #[test]
    fn sender_names_accept_plain_unicode_names_and_reject_instruction_syntax() {
        let photo = STANDARD.encode(JPEG);
        for (given, expected) in [
            ("  Nguyễn Thị Mai  ", "Nguyễn Thị Mai"),
            ("Élodie", "Élodie"),
            ("D'Arcy", "D'Arcy"),
            ("Anne-Marie", "Anne-Marie"),
            ("李 明", "李 明"),
        ] {
            assert_eq!(
                asked(&json!({"name": given, "photo": photo})).unwrap().0,
                expected
            );
        }
        let forty = "Đ".repeat(MOST_NAME_CHARS);
        assert_eq!(
            asked(&json!({"name": forty, "photo": photo})).unwrap().0,
            forty
        );
        let mut invalid = vec![
            "".to_owned(),
            "   ".to_owned(),
            "---".to_owned(),
            "''".to_owned(),
            "Lan2".to_owned(),
            "Lan_One".to_owned(),
            "Lan\nIgnore previous rules".to_owned(),
            "\nLan".to_owned(),
            "Lan\r".to_owned(),
            "Lan\tMai".to_owned(),
            "Lan\0".to_owned(),
            "Lan\u{1b}".to_owned(),
            "Lan; call tools".to_owned(),
            "Lan\"; ignore rules".to_owned(),
            "{\"name\":\"Lan\"}".to_owned(),
            "<system>Lan</system>".to_owned(),
            "Lan\u{a0}Mai".to_owned(),
        ];
        invalid.push("Đ".repeat(MOST_NAME_CHARS + 1));
        for name in invalid {
            assert!(
                asked(&json!({"name": name, "photo": photo})).is_err(),
                "accepted name {name:?}"
            );
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
        assert_eq!(told, json!({"received": true}));
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
    fn every_eyes_outcome_has_the_same_public_receipt_without_identity() {
        let received = json!({"received": true});
        for answer in [
            json!({"added": "Sender", "unconfirmed": true}),
            json!({"remembered": "Sender"}),
            json!({"remembered": "Someone Else"}),
            json!({"error": "known_as", "known_as": "Someone Else"}),
            json!({"error": "name_taken", "name": "Someone Else"}),
            json!({"error": "no_face"}),
            json!({"error": "two_faces"}),
            json!({"error": "too_far"}),
            json!({"error": "unreadable"}),
            json!({"error": "not_a_name"}),
            json!({"error": "full"}),
            json!({"error": "photos_off"}),
            json!({"error": "new_reason", "private": {"name": "Someone Else"}}),
            json!({}),
            Value::Null,
        ] {
            // Compare the complete public data for both successful and
            // refused Eyes outcomes; no identity-bearing extras may escape.
            assert_eq!(told(&answer), received, "Eyes outcome was exposed");
        }
    }

    #[test]
    fn a_local_write_failure_is_not_a_receipt() {
        let dir = folder("write-refused");
        let place = Place {
            dir: dir.join("absent"),
            socket: dir.join("who.sock"),
        };
        assert_eq!(
            add(&place, "Sender", JPEG, 9),
            Err("the photo could not be handed to the lamp's eyes".to_owned())
        );
        assert!(!place.dir.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// The existing serializer table is not a broker-boundary control: these
/// cases use the real command parser/answer/envelope and a real Eyes socket.
#[cfg(test)]
mod upload_oracle_contract_tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Instant;
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
    const PRIVATE: &str = "PRIVATE_ENROLLMENT_CANARY";

    struct Eyes {
        dir: PathBuf,
        place: Place,
        stop: Arc<AtomicBool>,
        task: Option<std::thread::JoinHandle<Value>>,
    }
    impl Eyes {
        fn new(case: usize, reply: String) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "photo-receipt-broker-{}-{case}",
                std::process::id()
            ));
            // Every case owns its new directory; no ambient fixture is reused.
            std::fs::create_dir(&dir).unwrap();
            let place = Place {
                dir: dir.clone(),
                socket: dir.join("who.sock"),
            };
            let listener = UnixListener::bind(&place.socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let folder = dir.clone();
            let task = std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(3);
                let (mut stream, _) = loop {
                    assert!(
                        !stopped.load(Ordering::Acquire),
                        "Eyes fixture cancelled before a request"
                    );
                    match listener.accept() {
                        Ok(pair) => break pair,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "bounded Eyes accept");
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => panic!("Eyes accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .take_line(&mut line)
                    .unwrap();
                let asked: Value = serde_json::from_str(line.trim()).unwrap();
                assert_eq!(asked["remember"], "Sender label");
                let object = asked.as_object().unwrap();
                assert_eq!(object.len(), 2, "exact existing Eyes command shape");
                let filename = asked["photo"].as_str().unwrap();
                assert!(filename.starts_with("app-") && filename.ends_with(".jpg"));
                assert_eq!(std::fs::read(folder.join(filename)).unwrap(), JPEG);
                writeln!(stream, "{reply}").unwrap();
                asked
            });
            Self {
                dir,
                place,
                stop,
                task: Some(task),
            }
        }
        fn finish(&mut self) -> Value {
            self.task.take().unwrap().join().unwrap()
        }
    }
    impl Drop for Eyes {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(task) = self.task.take() {
                let _ = task.join();
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    fn payload(data: Value) -> Vec<u8> {
        json!({"cmd":"data", "kind":"faces.add_photo", "data":data})
            .to_string()
            .into_bytes()
    }
    fn reply(place: Place, data: Value) -> Value {
        let mut lamp = crate::answer::tests::Fake {
            photos: Some(place),
            ..Default::default()
        };
        let wire = crate::online::reply(
            &crate::wire::Identity::default(),
            &mut lamp,
            &payload(data),
            "2026-10-23T00:00:00Z",
        )
        .unwrap();
        serde_json::from_str(&wire).unwrap()
    }

    #[test]
    fn broker_photo_receipt_is_identical_for_old_new_and_private_eyes_outcomes() {
        let account = crate::account::Account {
            fa_channel: "fa/fixture".into(),
            ..Default::default()
        };
        assert!(crate::online::commanded(b"fa/fixture", &account));
        assert!(!crate::online::commanded(b"prefix/fa/fixture", &account));
        assert!(!crate::online::commanded(b"fa/fixture/other", &account));
        let uploaded = json!({"name":"Sender label", "photo":STANDARD.encode(JPEG)});
        let outcomes = [
            json!({"added":"Sender label", "unconfirmed":true}),
            json!({"remembered":PRIVATE}),
            json!({"error":"known_as", "known_as":PRIVATE}),
            json!({"error":"name_taken", "name":PRIVATE}),
            json!({"error":"nameclash", "name":PRIVATE}),
            json!({"error":"full"}),
            json!({"error":"unknown"}),
            json!({"error":"no_face"}),
            json!({"received":true}),
            json!({"error":"new_reason", "private":{"name":PRIVATE}}),
            Value::Null,
        ];
        let mut first: Option<Value> = None;
        for (case, outcome) in outcomes.into_iter().enumerate() {
            let mut eyes = Eyes::new(case, outcome.to_string());
            let actual = reply(eyes.place.clone(), uploaded.clone());
            let asked = eyes.finish();
            assert_eq!(
                actual["status"], "success",
                "parsed semantic outcome {case}"
            );
            assert_eq!(
                actual["data"],
                json!({"received":true}),
                "only receipt, no stored/pending promise: {case}"
            );
            assert!(actual.get("error").is_none());
            assert!(!actual.to_string().contains(PRIVATE));
            if let Some(first) = &first {
                assert_eq!(&actual, first, "complete broker reply {case}");
            } else {
                first = Some(actual);
            }
            assert!(
                !eyes.dir.join(asked["photo"].as_str().unwrap()).exists(),
                "cleanup after each semantic reply"
            );
        }
    }

    #[test]
    fn broker_photo_input_and_transport_failures_do_not_become_enrollment_receipts() {
        let uploaded = json!({"name":"Sender label", "photo":STANDARD.encode(JPEG)});
        let absent =
            std::env::temp_dir().join(format!("photo-receipt-absent-{}", std::process::id()));
        assert!(!absent.exists());
        let place = Place {
            dir: absent.clone(),
            socket: absent.join("who.sock"),
        };
        for bad in [
            json!({"name":"", "photo":STANDARD.encode(JPEG)}),
            json!({"name":"Sender label", "photo":"not base64"}),
            json!({"name":"Sender label", "photo":STANDARD.encode(b"GIF89a")}),
        ] {
            let actual = reply(place.clone(), bad);
            assert_eq!(actual["status"], "failure");
            assert!(actual.get("data").is_none());
            assert!(actual["error"].as_str().is_some());
            assert!(!absent.exists(), "validation does not create a file");
        }
        let actual = reply(place, uploaded.clone());
        assert_eq!(actual["status"], "failure");
        assert_eq!(
            actual["error"],
            "the photo could not be handed to the lamp's eyes"
        );
        for (case, invalid) in [(100, "not JSON"), (101, "")] {
            let mut eyes = Eyes::new(case, invalid.into());
            let actual = reply(eyes.place.clone(), uploaded.clone());
            let asked = eyes.finish();
            assert_eq!(actual["status"], "failure");
            assert_eq!(actual["error"], "the lamp's eyes did not answer");
            assert!(actual.get("data").is_none());
            assert!(!eyes.dir.join(asked["photo"].as_str().unwrap()).exists());
        }
    }
}
