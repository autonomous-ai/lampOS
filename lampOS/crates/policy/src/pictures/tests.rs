use super::*;
use crate::attention::Room;

/// A JPEG of `width` by `height`, red rising across and blue rising down.
pub fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut rgb = Vec::new();
    for y in 0..u32::from(height) {
        for x in 0..u32::from(width) {
            rgb.extend([(x % 256) as u8, 90, (y % 256) as u8]);
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 80)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    out
}

fn decoded_size(jpeg: &[u8]) -> (usize, usize) {
    let mut decoder = zune_jpeg::JpegDecoder::new(jpeg);
    decoder.decode().unwrap();
    decoder.dimensions().unwrap()
}

/// The eyes' rule, as the voice keeps it: no file, a stale one, a large
/// one or one that is not a JPEG is not sent; a fresh one is, with its age.
#[test]
fn a_frame_is_taken_only_as_the_voice_takes_one() {
    let dir = std::env::temp_dir().join(format!("policy-pictures-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sight.jpg");
    let now = SystemTime::now();
    assert_eq!(fresh_picture(&path, now).unwrap_err(), "no_frame");

    let frame = jpeg(320, 240);
    fs::write(&path, &frame).unwrap();
    let (taken, age) = fresh_picture(&path, SystemTime::now()).unwrap();
    assert_eq!(taken, frame);
    assert!(age <= FRESH);
    assert_eq!(
        fresh_picture(&path, SystemTime::now() + Duration::from_millis(1_500)).unwrap_err(),
        "stale"
    );

    fs::write(&path, vec![0xff; (MOST_BYTES + 1) as usize]).unwrap();
    assert_eq!(
        fresh_picture(&path, SystemTime::now()).unwrap_err(),
        "too_large"
    );
    fs::write(&path, b"not a picture").unwrap();
    assert_eq!(
        fresh_picture(&path, SystemTime::now()).unwrap_err(),
        "not_a_jpeg"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// TypeSafe takes 256 to 1,568 pixels a side and 78,400 in all: the eyes'
/// 320 by 240 is doubled; a frame that fits is sent as it is; one far too
/// small or too large is not sent.
#[test]
fn a_frame_is_made_as_large_as_the_service_takes() {
    let (sent, size) = for_jev(&jpeg(320, 240)).unwrap();
    assert_eq!(size, (640, 480));
    assert_eq!(decoded_size(&sent), (640, 480));
    let fits = jpeg(640, 480);
    assert_eq!(for_jev(&fits).unwrap(), (fits.clone(), (640, 480)));
    assert_eq!(for_jev(&jpeg(20, 20)).unwrap_err(), "too_small");
    assert_eq!(for_jev(&jpeg(1_200, 200)).unwrap_err(), "too_large");
    assert_eq!(for_jev(b"\xff\xd8 not really").unwrap_err(), "not_a_jpeg");
}

/// With a picture the state is the same text with `picture` last, holding
/// the key, and the questions are the seven and the picture's six, whether
/// anyone is in it first.
#[test]
fn the_state_with_a_picture_names_it_last_and_six_questions_are_added() {
    let context = Context {
        heard: "Hey Lamp, what time is it?".into(),
        room: Room::Quiet,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    };
    let plain = attention::state_text(&context);
    let pictured = attention::state_text_with_picture(&context, KEY);
    assert_eq!(
        pictured,
        format!("{},\"picture\":\"{KEY}\"}}", &plain[..plain.len() - 1])
    );
    let value: Value = serde_json::from_str(&pictured).unwrap();
    assert_eq!(value["picture"], KEY);
    let asked = attention::questions_with_picture();
    assert_eq!(asked.len(), 13);
    assert_eq!(asked[7].0, "picture_person");
    assert_eq!(asked[..7], attention::questions()[..]);
    let names: Vec<&str> = asked[7..].iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, attention::picture_questions().collect::<Vec<_>>());
    for (_, question) in &asked[7..] {
        let crate::decide::Question::Noul { instructions } = question else {
            panic!("{question:?}");
        };
        assert!(instructions.contains("state.picture"), "{instructions}");
        assert!(!instructions.contains(KEY));
    }
}

/// What is taken at the moment a picture is to be sent: the eyes' frame
/// then, made as large as the service takes, with its own age. Removed, as
/// the eyes remove it when the camera is closed, or stale, none is taken.
#[test]
fn a_picture_is_taken_only_while_the_eyes_keep_a_fresh_frame() {
    let dir = std::env::temp_dir().join(format!("policy-take-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sight.jpg");
    assert_eq!(take_now(&path).unwrap_err(), "no_frame");
    let frame = jpeg(320, 240);
    fs::write(&path, &frame).unwrap();
    let taken = take_now(&path).unwrap();
    assert_eq!((taken.size, taken.eyes_bytes), ((640, 480), frame.len()));
    assert!(taken.age <= FRESH);
    let old = SystemTime::now() - Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert_eq!(take_now(&path).unwrap_err(), "stale");
    fs::remove_file(&path).unwrap();
    assert_eq!(take_now(&path).unwrap_err(), "no_frame");
    fs::remove_dir_all(&dir).unwrap();
}

fn taken() -> Taken {
    Taken {
        jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
        size: (640, 480),
        eyes_bytes: 3_000,
        age: Duration::from_millis(120),
    }
}

/// What the line says of the picture, from each try and how the asking
/// ended (the reviewer on #873): sent if any of a request with a frame
/// went, whatever came after; answered or not known; never sent when none
/// went.
#[test]
fn the_picture_is_told_as_sent_if_any_try_went() {
    let withdrawn: Result<Asked, Failed> = Err(Failed::Withdrawn);
    let timed_out: Result<Asked, Failed> = Err(Failed::TimedOut);
    // No frame by the time it was to be written.
    assert_eq!(
        picture(&[], &withdrawn),
        json!({"sent": false, "why_not": "withdrawn"})
    );
    // Taken, and not a byte of its request went.
    assert_eq!(
        picture(&[(taken(), Some(Delivery::Partly(0)))], &timed_out),
        json!({"sent": false, "why_not": "not_written"})
    );
    // Written whole on a connection that then closed, and withdrawn on the
    // next: sent, not known to have arrived.
    let line = picture(&[(taken(), Some(Delivery::Uncertain))], &withdrawn);
    assert_eq!(
        (&line["sent"], &line["delivery"], &line["withdrawn"]),
        (&json!(true), &json!("uncertain"), &json!(true))
    );
    assert_eq!(
        line["tries"],
        json!([{"written": "whole", "answered": false}])
    );
    assert_eq!(
        (&line["width"], &line["eyes_bytes"]),
        (&json!(640), &json!(3_000))
    );
    // Part of one written, then another answered.
    let line = picture(
        &[
            (taken(), Some(Delivery::Partly(512))),
            (taken(), Some(Delivery::Answered)),
        ],
        &timed_out,
    );
    assert_eq!(
        (&line["sent"], &line["delivery"], &line["withdrawn"]),
        (&json!(true), &json!("answered"), &json!(false))
    );
    assert_eq!(
        line["tries"],
        json!([{"written": "part", "request_bytes_written": 512, "answered": false},
               {"written": "whole", "answered": true}])
    );
}

/// One request read whole from `stream`: its body, or `None` if the
/// connection ended first.
fn read_body(stream: &mut std::net::TcpStream) -> Option<Value> {
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
    let head = String::from_utf8_lossy(&held[..start]).to_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:")?.trim().parse().ok())?;
    while held.len() < start + length {
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    }
    serde_json::from_slice(&held[start..start + length]).ok()
}

/// The reviewer's case on #873: the request with the picture written whole
/// on a kept connection, which the service closes with no answer as the
/// camera is closed; on the new connection there is no frame to send, so
/// nothing is written there. The line says the picture was sent, not known
/// to have arrived, and the asking withdrawn after it.
#[test]
fn a_picture_written_and_unanswered_before_a_withdrawal_is_told_as_sent() {
    written_then_withdrawn("closed", |sight, _| fs::remove_file(sight).unwrap());
}

/// The reviewer on #873 (04:14): whether someone is in sight is looked at
/// again at the moment the picture would be taken. The person leaves (the
/// eyes see no face) between the first try and the next: nothing more is
/// sent, as when the camera closes.
#[test]
fn a_person_who_left_before_the_picture_is_taken_is_sent_no_picture() {
    written_then_withdrawn("left", |_, presence| presence.saw(0, Instant::now()));
}

/// A first asking answered on a kept connection; the request with the
/// picture written whole on it, then `meanwhile` (the camera closed, or the
/// person gone), and the connection closed with no answer; the new
/// connection given nothing.
fn written_then_withdrawn(tag: &str, meanwhile: fn(&Path, &Presence)) {
    use std::io::Write as _;
    let dir = std::env::temp_dir().join(format!("policy-withdrawn-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("key"), "not-a-real-key\n").unwrap();
    let sight = dir.join("sight.jpg");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, requests) = mpsc::channel::<Option<Value>>();
    let presence = Presence::default();
    presence.saw(1, Instant::now());
    let (removed, gone) = (sight.clone(), presence.clone());
    let service = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // The first asking answered, the connection kept.
        let asked = read_body(&mut first).unwrap();
        let answers: serde_json::Map<String, Value> = asked["questions"]
            .as_object()
            .unwrap()
            .keys()
            .map(|name| (name.clone(), json!({"type": "noul", "noul": 0.8})))
            .collect();
        let answer = json!({"model": "typesafe/jev-1.14-20261014", "answers": answers}).to_string();
        first
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                    answer.len()
                )
                .as_bytes(),
            )
            .unwrap();
        told.send(Some(asked)).unwrap();
        // The second, with the picture, read whole; then the camera is
        // closed or the person gone, and the connection closed.
        told.send(read_body(&mut first)).unwrap();
        meanwhile(&removed, &gone);
        drop(first);
        // The new connection: nothing comes on it.
        let (mut second, _) = listener.accept().unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        told.send(read_body(&mut second)).unwrap();
    });
    let url = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let key = dir.join("key");
    let mut with = Jev::new(&url, "typesafe/jev-1.14", &key).unwrap();
    let state = attention::state_text(&job_context());
    crate::decide::Decider::ask(
        &mut with,
        &state,
        &attention::questions(),
        Duration::from_secs(5),
    )
    .unwrap();
    // `without` asks where nothing listens, and fails at once.
    let nowhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let nowhere_url = format!(
        "http://127.0.0.1:{}/api/alpha/decisions",
        nowhere.local_addr().unwrap().port()
    );
    drop(nowhere);
    let mut without = Jev::new(&nowhere_url, "typesafe/jev-1.14", &key).unwrap();
    fs::write(&sight, jpeg(320, 240)).unwrap();
    let job = Job {
        context: job_context(),
        seen: Some(crate::policy::Seen {
            faces: 1,
            near: 1,
            facing: 1,
            age_ms: 40,
        }),
        sent_at_unix_ms: 1_760_541_000_000,
    };
    let options = Options {
        sight_file: sight.clone(),
        deadline: Duration::from_secs(5),
        log_dir: dir.clone(),
        presence,
    };
    let line = ask_both(&mut without, &mut with, &job, &options);
    service.join().unwrap();
    let first = requests.recv().unwrap().unwrap();
    assert!(first.get("assets").is_none());
    let second = requests.recv().unwrap().unwrap();
    assert!(second["assets"][KEY]["data_base64"].is_string());
    assert_eq!(requests.recv().unwrap(), None);
    assert_eq!(line["picture"]["sent"], true, "{}", line["picture"]);
    assert_eq!(line["picture"]["delivery"], "uncertain");
    assert_eq!(line["picture"]["withdrawn"], true);
    assert_eq!(
        line["picture"]["tries"],
        json!([{"written": "whole", "answered": false}])
    );
    assert!(line["picture"].get("why_not").is_none());
    assert_eq!(line["with"]["failed"], "withdrawn");
    fs::remove_dir_all(&dir).unwrap();
}

fn job_context() -> Context {
    Context {
        heard: "Hey Lamp, what time is it?".into(),
        room: Room::Quiet,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    }
}
