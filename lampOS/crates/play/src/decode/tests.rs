use super::*;
use std::io::Cursor;

const STEREO_44K: &[u8] = include_bytes!("../../tests/fixtures/tone-44k-stereo.mp3");
const MONO_48K: &[u8] = include_bytes!("../../tests/fixtures/tone-48k-mono.mp3");

fn play(bytes: Vec<u8>, limits: Limits) -> (Outcome, Vec<i16>) {
    let mut all = Vec::new();
    let outcome = decode(Cursor::new(bytes), limits, |block| {
        assert!(block.len() <= BLOCK);
        all.extend_from_slice(block);
        Flow::Continue
    });
    (outcome, all)
}

fn crossings(samples: &[i16]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] < 0 && pair[1] >= 0)
        .count()
}

#[test]
fn a_stereo_44_1_khz_mp3_plays_as_24_khz_mono_about_a_second_long_at_its_pitch() {
    let (outcome, samples) = play(STEREO_44K.to_vec(), Limits::default());
    assert_eq!(outcome, Outcome::Done);
    // One second of tone, within the encoder's padding either side.
    assert!(
        (22_000..=27_500).contains(&samples.len()),
        "{}",
        samples.len()
    );
    let steady = &samples[3_000..samples.len() - 3_000];
    let per_second = crossings(steady) as f64 * 24_000.0 / steady.len() as f64;
    assert!((per_second - 440.0).abs() < 15.0, "{per_second} Hz");
    let peak = steady.iter().map(|s| i32::from(*s).abs()).max().unwrap();
    // The fixture's tone is quiet on purpose (about -21 dBFS peak).
    assert!(peak > 1_500 && peak < 32_767, "{peak}");
}

#[test]
fn a_mono_48_khz_mp3_plays_at_its_pitch_too() {
    let (outcome, samples) = play(MONO_48K.to_vec(), Limits::default());
    assert_eq!(outcome, Outcome::Done);
    let steady = &samples[3_000..samples.len() - 3_000];
    let per_second = crossings(steady) as f64 * 24_000.0 / steady.len() as f64;
    assert!((per_second - 1_000.0).abs() < 25.0, "{per_second} Hz");
}

#[test]
fn something_that_is_not_an_mp3_is_bad_audio_and_plays_nothing() {
    for bytes in [
        Vec::new(),
        b"<html><body>not audio</body></html>".to_vec(),
        vec![0u8; 20_000],
        (0..20_000u32).map(|n| (n * 31 % 251) as u8).collect(),
    ] {
        let (outcome, samples) = play(bytes, Limits::default());
        assert_eq!(outcome, Outcome::BadAudio);
        assert!(samples.is_empty());
    }
}

#[test]
fn a_cut_file_ends_at_the_cut_and_never_panics() {
    for keep in [100, 417, 1_000, 4_000, 6_000] {
        let (outcome, samples) = play(STEREO_44K[..keep].to_vec(), Limits::default());
        assert!(
            matches!(outcome, Outcome::Done | Outcome::BadAudio),
            "{keep}: {outcome:?}"
        );
        assert!(samples.len() < 25_000);
    }
}

#[test]
fn damaged_bytes_never_panic_and_the_outcome_is_a_known_one() {
    for at in (0..STEREO_44K.len()).step_by(97) {
        let mut bytes = STEREO_44K.to_vec();
        for byte in bytes.iter_mut().skip(at).take(40) {
            *byte ^= 0xFF;
        }
        let (outcome, _) = play(bytes, Limits::default());
        assert!(
            matches!(outcome, Outcome::Done | Outcome::BadAudio),
            "damage at {at}: {outcome:?}"
        );
    }
}

#[test]
fn the_byte_cap_stops_a_play_as_too_big() {
    let limits = Limits {
        most_bytes: 3_000,
        ..Limits::default()
    };
    let (outcome, _) = play(STEREO_44K.to_vec(), limits);
    assert_eq!(outcome, Outcome::TooBig);
}

#[test]
fn the_time_cap_stops_a_play_as_too_long() {
    // The cap counts blocks of output: 0 seconds allows none.
    let limits = Limits {
        most_seconds: 0,
        ..Limits::default()
    };
    let (outcome, _) = play(STEREO_44K.to_vec(), limits);
    assert_eq!(outcome, Outcome::TooLong);
}

#[test]
fn the_callback_can_stop_a_play_at_once() {
    let mut blocks = 0;
    let outcome = decode(Cursor::new(STEREO_44K.to_vec()), Limits::default(), |_| {
        blocks += 1;
        Flow::Stop
    });
    assert_eq!(outcome, Outcome::Stopped);
    assert_eq!(blocks, 1);
}

#[test]
fn a_source_that_stalls_never_runs_past_what_it_gave() {
    // A reader that errors partway is a bad play, not a hang or a panic.
    struct Breaks(Cursor<Vec<u8>>);
    impl Read for Breaks {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0.position() > 2_000 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "stalled"));
            }
            let end = buffer.len().min(500);
            self.0.read(&mut buffer[..end])
        }
    }
    let outcome = decode(
        Breaks(Cursor::new(STEREO_44K.to_vec())),
        Limits::default(),
        |_| Flow::Continue,
    );
    assert!(
        matches!(outcome, Outcome::BadAudio | Outcome::Done),
        "{outcome:?}"
    );
}

#[test]
fn the_outcome_words_are_fixed() {
    let words: Vec<&str> = [
        Outcome::Done,
        Outcome::Stopped,
        Outcome::TooBig,
        Outcome::TooLong,
        Outcome::BadAudio,
    ]
    .iter()
    .map(|outcome| outcome.word())
    .collect();
    assert_eq!(
        words,
        ["done", "stopped", "too_big", "too_long", "bad_audio"]
    );
}

#[test]
fn the_report_counts_what_was_read_and_what_was_played() {
    let mut handed = 0usize;
    let report = decode_report(
        Cursor::new(STEREO_44K.to_vec()),
        Limits::default(),
        |block| {
            handed += block.len();
            Flow::Continue
        },
    );
    assert_eq!(report.outcome, Outcome::Done);
    assert_eq!(report.samples, handed as u64);
    // The whole file was read (the decoder may leave a few bytes of a tag).
    assert!(
        report.bytes + 400 >= STEREO_44K.len() as u64 && report.bytes <= STEREO_44K.len() as u64
    );
    let bad = decode_report(Cursor::new(b"nothing".to_vec()), Limits::default(), |_| {
        Flow::Continue
    });
    assert_eq!((bad.outcome, bad.samples), (Outcome::BadAudio, 0));
}
