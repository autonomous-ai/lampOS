//! A skill tool's look (#941): the voice takes the full look for the turn
//! itself, asked in the call's answer, with no spoken phrase. Fixture eyes:
//! a datagram socket standing for the eyes' permit socket, and a still
//! written to the full look's file as the eyes write it.
use super::*;
use std::os::unix::net::UnixDatagram;

/// A still as the eyes write it (`lamp_eyes::full_look::tagged`).
fn still_of(id: u64) -> Vec<u8> {
    let text = format!("lamp-look {id}");
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xfe, 0, text.len() as u8 + 2];
    jpeg.extend_from_slice(text.as_bytes());
    jpeg.extend_from_slice(&[0xff, 0xdb, 0, 2, 0xff, 0xd9]);
    jpeg
}

struct Eyes {
    directory: std::path::PathBuf,
    socket: UnixDatagram,
    link: crate::body_link::EyesLink,
    still: std::path::PathBuf,
}

impl Eyes {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-tool-look-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let permit = directory.join("permit.sock");
        let socket = UnixDatagram::bind(&permit).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self {
            link: crate::body_link::EyesLink::to(permit).unwrap(),
            socket,
            still: directory.join("full-look.jpg"),
            directory,
        }
    }

    /// Every ask the eyes were sent, by id.
    fn asks(&self) -> Vec<u64> {
        let mut got = [0u8; 64];
        let mut asks = Vec::new();
        while let Ok(length) = self.socket.recv(&mut got) {
            let ask: Value = serde_json::from_slice(&got[..length]).unwrap();
            asks.push(ask["look_full"].as_u64().unwrap());
        }
        asks
    }
}

impl Drop for Eyes {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn a_tool_s_look_asks_the_eyes_once_with_no_phrase_and_its_still_goes_with_the_look() {
    let mut eyes = Eyes::new("still");
    let mut look = crate::sight::Look::new(Some(eyes.still.clone()));
    let mut wanted = None;
    let mut opening = None;
    let mut trace = SessionState::default();
    let turn = 4;
    // The skill's answer reached the model: the look is the turn's, and the
    // eyes are asked, with nothing said yet.
    record_tool_look(look.full(), turn, Some(turn), None, &mut wanted, &mut trace).unwrap();
    let (of, asked_us) = wanted.unwrap();
    assert_eq!(of, turn);
    ask_for_still(&mut look, Some(&mut eyes.link), turn, 1, &mut trace).unwrap();
    // The model then says it will look, as it is told to: nothing more is
    // asked, and the look's wait still counts from the tool.
    record_look_intent(
        true,
        Some(turn),
        "Let me take a look.",
        None,
        None,
        &mut opening,
        &mut wanted,
        &mut trace,
    )
    .unwrap();
    record_tool_look(look.full(), turn, Some(turn), None, &mut wanted, &mut trace).unwrap();
    assert_eq!(wanted, Some((turn, asked_us)));
    ask_for_still(&mut look, Some(&mut eyes.link), turn, 2, &mut trace).unwrap();
    let asks = eyes.asks();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(!look.due(Duration::from_millis(500)));
    // The eyes write its still: the look goes with it and the note.
    std::fs::write(&eyes.still, still_of(asks[0])).unwrap();
    assert!(look.due(Duration::from_millis(500)));
    assert_eq!(look.turn(), (crate::speak_up::LOOK_FULL_NOTE, true));
}

#[test]
fn a_tool_s_look_with_no_still_tells_the_model_it_could_not_see() {
    let mut eyes = Eyes::new("none");
    let mut look = crate::sight::Look::new(Some(eyes.still.clone()));
    let mut wanted = None;
    let mut trace = SessionState::default();
    record_tool_look(look.full(), 6, Some(6), None, &mut wanted, &mut trace).unwrap();
    ask_for_still(&mut look, Some(&mut eyes.link), 6, 1, &mut trace).unwrap();
    assert_eq!(eyes.asks().len(), 1);
    assert!(!look.due(crate::sight::STILL_WAIT - Duration::from_millis(1)));
    assert!(look.due(crate::sight::STILL_WAIT));
    let (note, with_picture) = look.turn();
    assert!(!with_picture);
    assert!(note.contains("could not see"), "{note}");
    assert!(note.contains("never guess"), "{note}");
}

#[test]
fn a_tool_s_look_is_taken_only_with_a_full_look_and_only_for_the_turn_in_hand() {
    let mut trace = SessionState::default();
    let mut wanted = None;
    // `LAMP_LOOK_FULL` off: the model's phrase is the look, as before.
    record_tool_look(false, 3, Some(3), None, &mut wanted, &mut trace).unwrap();
    assert_eq!(trace.len(), 0);
    // A turn already over, or a reminder's: dropped, and each says so
    // (the first builder's 23:16 P3 on #1146).
    record_tool_look(true, 3, Some(4), None, &mut wanted, &mut trace).unwrap();
    record_tool_look(true, 3, None, None, &mut wanted, &mut trace).unwrap();
    record_tool_look(true, 3, Some(3), Some(3), &mut wanted, &mut trace).unwrap();
    assert!(wanted.is_none());
    assert_eq!(trace.len(), 3);
    record_tool_look(true, 3, Some(3), Some(2), &mut wanted, &mut trace).unwrap();
    assert_eq!(wanted.map(|(turn, _)| turn), Some(3));
    assert_eq!(trace.len(), 4);
}
