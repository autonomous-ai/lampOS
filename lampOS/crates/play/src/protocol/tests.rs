use super::*;
use std::io::Cursor;

#[test]
fn a_frame_round_trips_and_a_request_parses() {
    let mut wire = Vec::new();
    write_frame(&mut wire, PCM, &[1, 2, 3]).unwrap();
    let (kind, payload) = read_frame(&mut Cursor::new(wire), MOST_PCM).unwrap();
    assert_eq!((kind, payload), (PCM, vec![1, 2, 3]));
    let request = br#"{"op":"play","id":7,"url":"https://cdn.example.com/a.mp3","hosts":["cdn.example.com"]}"#;
    let mut wire = Vec::new();
    write_frame(&mut wire, JSON, request).unwrap();
    assert_eq!(
        read_request(&mut Cursor::new(wire)).unwrap(),
        Request::Play {
            id: 7,
            url: "https://cdn.example.com/a.mp3".into(),
            hosts: vec!["cdn.example.com".into()]
        }
    );
}

#[test]
fn a_bad_request_is_refused_not_guessed() {
    for text in [
        &br#"{"op":"play","id":7,"url":"https://x.example.com/","hosts":[],"extra":1}"#[..],
        br#"{"op":"stop","id":7}"#,
        br#"{"op":"play","id":"7","url":"u","hosts":[]}"#,
        br#"{"op":"play","id":7,"hosts":[]}"#,
        b"not json",
        b"",
    ] {
        let mut wire = Vec::new();
        write_frame(&mut wire, JSON, text).unwrap();
        assert!(
            read_request(&mut Cursor::new(wire)).is_err(),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
    // A sound frame is no request.
    let mut wire = Vec::new();
    write_frame(&mut wire, PCM, b"{}").unwrap();
    assert!(read_request(&mut Cursor::new(wire)).is_err());
}

#[test]
fn a_frame_past_its_most_or_empty_or_cut_short_is_an_error() {
    let mut wire = Vec::new();
    write_frame(&mut wire, PCM, &vec![0u8; MOST_PCM + 1]).unwrap();
    assert!(read_frame(&mut Cursor::new(wire), MOST_PCM).is_err());
    assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 0]), MOST_PCM).is_err());
    assert!(read_frame(&mut Cursor::new(vec![0, 0, 0, 9, b'P', 1]), MOST_PCM).is_err());
    assert!(read_frame(&mut Cursor::new(vec![0, 0]), MOST_PCM).is_err());
    // A huge announced length is refused before anything is allocated for it.
    assert!(read_frame(&mut Cursor::new(vec![0xFF, 0xFF, 0xFF, 0xFF]), MOST_PCM).is_err());
}

#[test]
fn the_events_hold_ids_counts_and_a_fixed_word_and_never_a_url() {
    let mut wire = Vec::new();
    write_event(&mut wire, &Event::Started { id: 3 }).unwrap();
    write_event(
        &mut wire,
        &Event::Ended {
            id: 3,
            outcome: "done",
            seconds: 12,
            bytes: 99,
        },
    )
    .unwrap();
    let mut input = Cursor::new(wire);
    let (_, first) = read_frame(&mut input, 1024).unwrap();
    let (_, second) = read_frame(&mut input, 1024).unwrap();
    assert_eq!(
        String::from_utf8(first).unwrap(),
        r#"{"ev":"started","id":3}"#
    );
    assert_eq!(
        String::from_utf8(second).unwrap(),
        r#"{"ev":"ended","id":3,"outcome":"done","seconds":12,"bytes":99}"#
    );
}
