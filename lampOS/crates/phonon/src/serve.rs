//! The socket (#8754): one request line a connection, the speech back as
//! frames ([`lamp_msgs::phonon::frame`]) as the model makes it, then the
//! end. One sentence is made at a time; a connection waiting its turn
//! starts when the one before ends. The voice closing its connection stops
//! that sentence at the next piece, which is how a barge-in stops speech.
use crate::engine::Speaker;
use lamp_msgs::phonon::{END, FAIL, LINE_MOST, frame, parse_request, to_i16};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// The most connections served or waiting at once; more are closed.
pub const CONNECTIONS_MOST: usize = 4;
/// How long a request line may take to come, and a frame to be taken.
pub const STALL: Duration = Duration::from_secs(5);

/// Serves `listener` with `speaker` until it fails.
pub fn serve(listener: UnixListener, speaker: Arc<dyn Speaker>) -> std::io::Result<()> {
    let one_at_a_time = Arc::new(Mutex::new(()));
    let open = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        let connection = connection?;
        if open.fetch_add(1, Ordering::AcqRel) >= CONNECTIONS_MOST {
            open.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let (speaker, one_at_a_time, open) = (speaker.clone(), one_at_a_time.clone(), open.clone());
        std::thread::spawn(move || {
            let _ = answer(connection, speaker.as_ref(), &one_at_a_time);
            open.fetch_sub(1, Ordering::AcqRel);
        });
    }
    Ok(())
}

/// One connection: its request read, its sentence said or refused.
pub fn answer(
    mut connection: UnixStream,
    speaker: &dyn Speaker,
    one_at_a_time: &Mutex<()>,
) -> std::io::Result<()> {
    connection.set_read_timeout(Some(STALL))?;
    connection.set_write_timeout(Some(STALL))?;
    let mut line = String::new();
    BufReader::new((&connection).take(LINE_MOST as u64 + 1)).read_line(&mut line)?;
    let Ok((text, voice)) = parse_request(&line) else {
        return connection.write_all(&FAIL);
    };
    let _turn = one_at_a_time
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    let mut gone = false;
    let said = speaker.speak(&text, voice.name, &mut |pcm| {
        // At most one second a frame.
        for piece in pcm.chunks(lamp_msgs::phonon::FRAME_MOST as usize) {
            if connection.write_all(&frame(&to_i16(piece))).is_err() {
                gone = true;
                return false;
            }
        }
        true
    });
    if gone {
        return Ok(());
    }
    match said {
        Ok(()) => connection.write_all(&END),
        Err(_) => connection.write_all(&FAIL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::phonon::{Head, head, request};
    use std::sync::atomic::AtomicBool;

    /// Says `pieces` pieces of 960 samples of 0.5, noting how many it made
    /// and whether it was stopped.
    struct Counting {
        pieces: usize,
        made: AtomicUsize,
        stopped: AtomicBool,
        fail: bool,
    }

    impl Speaker for Counting {
        fn speak(
            &self,
            _text: &str,
            _voice: &str,
            out: &mut dyn FnMut(&[f32]) -> bool,
        ) -> Result<(), String> {
            if self.fail {
                return Err("no".into());
            }
            for _ in 0..self.pieces {
                self.made.fetch_add(1, Ordering::AcqRel);
                if !out(&[0.5; 960]) {
                    self.stopped.store(true, Ordering::Release);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        }
    }

    fn counting(pieces: usize, fail: bool) -> Arc<Counting> {
        Arc::new(Counting {
            pieces,
            made: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
            fail,
        })
    }

    fn served(speaker: Arc<Counting>) -> (tempfile::TempDir, std::path::PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("phonon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || serve(listener, speaker));
        (folder, path)
    }

    /// Reads frames to the end: the samples, and whether it ended or failed.
    fn read_all(connection: &mut UnixStream) -> (Vec<i16>, Head) {
        let mut samples = Vec::new();
        loop {
            let mut bytes = [0; 4];
            if connection.read_exact(&mut bytes).is_err() {
                return (samples, Head::Failed);
            }
            match head(bytes) {
                Head::Samples(n) => {
                    let mut pcm = vec![0; n * 2];
                    connection.read_exact(&mut pcm).unwrap();
                    samples.extend(
                        pcm.chunks(2)
                            .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
                    );
                }
                end => return (samples, end),
            }
        }
    }

    #[test]
    fn a_sentence_comes_back_whole_then_the_end() {
        let speaker = counting(3, false);
        let (_folder, path) = served(speaker.clone());
        let mut connection = UnixStream::connect(&path).unwrap();
        connection
            .write_all(request("Hello there.", "Freya").as_bytes())
            .unwrap();
        let (samples, end) = read_all(&mut connection);
        assert_eq!(end, Head::End);
        assert_eq!(samples.len(), 3 * 960);
        assert!(samples.iter().all(|s| *s == (0.5 * 32767.0) as i16));
    }

    #[test]
    fn a_bad_request_or_a_failed_sentence_is_a_failure() {
        let (_folder, path) = served(counting(3, false));
        for line in ["nonsense\n".to_owned(), request("Hi.", "Kore")] {
            let mut connection = UnixStream::connect(&path).unwrap();
            connection.write_all(line.as_bytes()).unwrap();
            assert_eq!(read_all(&mut connection), (vec![], Head::Failed), "{line}");
        }
        let (_folder, path) = served(counting(3, true));
        let mut connection = UnixStream::connect(&path).unwrap();
        connection
            .write_all(request("Hi.", "Freya").as_bytes())
            .unwrap();
        assert_eq!(read_all(&mut connection), (vec![], Head::Failed));
    }

    /// The barge-in: the voice closes its connection after the first frame,
    /// and the sentence stops within a few pieces, not at its end.
    #[test]
    fn closing_the_connection_stops_the_sentence() {
        let speaker = counting(400, false);
        let (_folder, path) = served(speaker.clone());
        let mut connection = UnixStream::connect(&path).unwrap();
        connection
            .write_all(request("A long answer.", "Freya").as_bytes())
            .unwrap();
        let mut bytes = [0; 4];
        connection.read_exact(&mut bytes).unwrap();
        assert_eq!(head(bytes), Head::Samples(960));
        drop(connection);
        let started = std::time::Instant::now();
        while !speaker.stopped.load(Ordering::Acquire) {
            assert!(started.elapsed() < Duration::from_secs(5), "never stopped");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(speaker.made.load(Ordering::Acquire) < 400);
    }

    /// The next sentence is served after a stopped one, on the same model.
    #[test]
    fn a_sentence_after_a_stopped_one_is_said() {
        let speaker = counting(50, false);
        let (_folder, path) = served(speaker.clone());
        let mut first = UnixStream::connect(&path).unwrap();
        first
            .write_all(request("One.", "Freya").as_bytes())
            .unwrap();
        let mut bytes = [0; 4];
        first.read_exact(&mut bytes).unwrap();
        drop(first);
        let mut second = UnixStream::connect(&path).unwrap();
        second
            .write_all(request("Two.", "Toby").as_bytes())
            .unwrap();
        let (samples, end) = read_all(&mut second);
        assert_eq!((samples.len(), end), (50 * 960, Head::End));
    }
}
