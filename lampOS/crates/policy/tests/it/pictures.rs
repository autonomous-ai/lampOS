//! The picture's two askings (#871, `lamp-policy --images`) against a
//! service played on this machine: one without the picture and one with it,
//! at once; only the second carries the picture and TypeSafe's header; the
//! line says both answers and the picture's size, never the picture; and
//! with no frame nothing is asked.
use lamp_policy::{
    attention::{self, Context, Room},
    decide::{IMAGES_HEADER, Jev},
    pictures::{self, Job, KEY},
    policy::Seen,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut held = Vec::new();
    let mut piece = [0u8; 8192];
    let start = loop {
        if let Some(at) = held.windows(4).position(|four| four == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    };
    let head = String::from_utf8(held[..start].to_vec()).ok()?;
    let length: usize = head
        .to_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("content-length:")?.trim().parse().ok())?;
    while held.len() < start + length {
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    }
    Some((head, String::from_utf8(held[start..].to_vec()).ok()?))
}

/// Every question answered 0.8, after `wait`, each connection on a thread
/// of its own; each request's head and body told to the test.
fn service(wait: Duration) -> (u16, mpsc::Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, requests) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let told = told.clone();
            thread::spawn(move || {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                while let Some((head, body)) = request(&mut stream) {
                    thread::sleep(wait);
                    let asked: Value = serde_json::from_str(&body).unwrap();
                    let answers: serde_json::Map<String, Value> = asked["questions"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(|name| (name.clone(), json!({"type": "noul", "noul": 0.8})))
                        .collect();
                    let answer = json!({"model": "typesafe/jev-1.14-20261014", "answers": answers})
                        .to_string();
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                            answer.len()
                        )
                        .as_bytes(),
                    );
                    if told.send((head, asked)).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, requests)
}

struct Place(PathBuf);
impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn job() -> Job {
    Job {
        context: Context {
            heard: "Hey Lamp, what is the weather like in Hanoi today?".into(),
            room: Room::Busy,
            lamp_is_speaking: false,
            lamp_last_said: None,
            seconds_since_lamp_spoke: None,
            name_heard: Some(0.97),
            like_asker: None,
        },
        // Someone at the desk, turned to the lamp, as the eyes saw.
        seen: Some(Seen {
            faces: 1,
            near: 1,
            facing: 1,
            age_ms: 40,
        }),
        sent_at_unix_ms: 1_760_541_000_000,
    }
}

/// A 320 by 240 frame, as the eyes keep one.
fn frame() -> Vec<u8> {
    let rgb = vec![120u8; 320 * 240 * 3];
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 80)
        .encode(&rgb, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    out
}

#[test]
fn the_same_question_is_asked_without_the_picture_and_with_it_and_the_picture_is_never_written() {
    let place =
        Place(std::env::temp_dir().join(format!("policy-pictures-program-{}", std::process::id())));
    let _ = fs::remove_dir_all(&place.0);
    fs::create_dir_all(&place.0).unwrap();
    fs::write(place.0.join("key"), "not-a-real-key\n").unwrap();
    let sight = place.0.join("sight.jpg");
    let (port, requests) = service(Duration::ZERO);
    let url = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let key = place.0.join("key");
    let mut without = Jev::new(&url, "typesafe/jev-1.14", &key).unwrap();
    let mut with = Jev::new(&url, "typesafe/jev-1.14", &key).unwrap();
    let options = pictures::Options {
        sight_file: sight.clone(),
        deadline: Duration::from_secs(5),
        log_dir: place.0.clone(),
        presence: pictures::Presence::default(),
    };
    let presence = options.presence.clone();

    // Nobody in sight now, the eyes saying nothing, or their word too old:
    // nothing is asked, frame or not (the first builder on #871). It is what
    // the eyes see now that counts, not the utterance's own snapshot
    // (`job().seen`, a face): a question that waited in the queue while the
    // person left sends nothing (the reviewer on #873).
    fs::write(&sight, frame()).unwrap();
    let stale = Instant::now()
        .checked_sub(pictures::FACES_FRESH + Duration::from_millis(500))
        .unwrap();
    for (eyes, why) in [
        (Some((0, Instant::now())), "no_face"),
        (None, "eyes_unknown"),
        (Some((1, stale)), "eyes_unknown"),
    ] {
        match eyes {
            Some((faces, at)) => presence.saw(faces, at),
            None => presence.gone(),
        }
        let line = pictures::ask_both(&mut without, &mut with, &job(), &options);
        assert_eq!(line["picture"], json!({"sent": false, "why_not": why}));
        assert!(line["without"].is_null() && line["with"].is_null());
        // The utterance's snapshot is still written down, for the record.
        assert_eq!(line["seen"]["faces"], 1);
    }
    assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());
    fs::remove_file(&sight).unwrap();
    presence.saw(1, Instant::now());

    // No frame: the camera is not permitted. Nothing is asked.
    let none = pictures::ask_both(&mut without, &mut with, &job(), &options);
    assert_eq!(
        none["picture"],
        json!({"sent": false, "why_not": "no_frame"})
    );
    assert!(none["without"].is_null() && none["with"].is_null());
    assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());

    let kept = frame();
    fs::write(&sight, &kept).unwrap();
    let line = pictures::ask_both(&mut without, &mut with, &job(), &options);
    let mut asked: Vec<(String, Value)> = requests.try_iter().collect();
    assert_eq!(asked.len(), 2);
    asked.sort_by_key(|(_, body)| body.get("assets").is_some());
    let (plain_head, plain) = &asked[0];
    let (pictured_head, pictured) = &asked[1];
    // Without: the day's question, as the decision itself asks it.
    assert!(!plain_head.contains(IMAGES_HEADER));
    assert_eq!(
        plain["state"],
        serde_json::from_str::<Value>(&attention::state_text(&job().context)).unwrap()
    );
    assert_eq!(plain["questions"].as_object().unwrap().len(), 7);
    // With: the header, the picture under its key, and thirteen questions.
    assert!(
        pictured_head
            .split("\r\n")
            .any(|line| line == IMAGES_HEADER)
    );
    assert_eq!(pictured["state"]["picture"], KEY);
    assert_eq!(pictured["questions"].as_object().unwrap().len(), 13);
    assert_eq!(pictured["assets"][KEY]["format"], "jpg");
    let base64 = pictured["assets"][KEY]["data_base64"].as_str().unwrap();

    // The line: both answers, the picture's size and age, never the picture.
    assert_eq!(line["about"], "attention_picture");
    assert_eq!(line["picture"]["sent"], true);
    assert_eq!(
        (&line["picture"]["width"], &line["picture"]["height"]),
        (&json!(640), &json!(480))
    );
    assert_eq!(line["picture"]["eyes_bytes"], kept.len());
    assert_eq!(line["without"]["answers"].as_object().unwrap().len(), 7);
    assert_eq!(line["with"]["answers"].as_object().unwrap().len(), 13);
    assert_eq!(line["with"]["answers"]["picture_faces_lamp"], 0.8);
    assert!(line["with"]["choice"].is_string() && line["without"]["choice"].is_string());
    assert!(line["with"].get("failed").is_none(), "{}", line["with"]);
    let text = line.to_string();
    assert!(!text.contains(&base64[..40]));
}

/// Started as the program starts it, the thread writes one line a question
/// to `pictures-YYYY-MM-DD.jsonl`, and passes over a question that comes
/// while it is still asking.
#[test]
fn the_thread_writes_its_lines_and_passes_over_what_comes_while_it_asks() {
    let place =
        Place(std::env::temp_dir().join(format!("policy-pictures-thread-{}", std::process::id())));
    let _ = fs::remove_dir_all(&place.0);
    fs::create_dir_all(&place.0).unwrap();
    fs::write(place.0.join("key"), "not-a-real-key\n").unwrap();
    // Slow enough that the thread is surely still asking when the second
    // question comes.
    let (port, _requests) = service(Duration::from_millis(300));
    let url = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let key = place.0.join("key");
    let started = pictures::Pictures::start(
        Jev::new(&url, "typesafe/jev-1.14", &key).unwrap(),
        Jev::new(&url, "typesafe/jev-1.14", &key).unwrap(),
        pictures::Options {
            sight_file: place.0.join("sight.jpg"),
            deadline: Duration::from_secs(5),
            log_dir: place.0.clone(),
            presence: {
                let someone = pictures::Presence::default();
                someone.saw(1, Instant::now());
                someone
            },
        },
    )
    .unwrap();
    fs::write(place.0.join("sight.jpg"), frame()).unwrap();
    // The thread is waiting for work: the first is taken.
    let mut taken = false;
    for _ in 0..50 {
        if started.offer(job()) {
            taken = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(taken);
    // While it asks, another is passed over.
    assert!(!started.offer(job()));
    let log = place.0.join("pictures-2025-10-15.jsonl");
    for _ in 0..200 {
        if fs::read_to_string(&log).is_ok_and(|text| text.lines().count() == 1) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let text = fs::read_to_string(&log).unwrap();
    let line: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(line["picture"]["sent"], true);
    assert!(started.alive());
}
