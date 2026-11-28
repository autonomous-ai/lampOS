//! Phonon, the voice that runs on the lamp (#8754, `docs/phonon.md`): each
//! sentence asked of `lamp-phonon` on its socket, its 24 kHz samples read
//! as they come, in packets of [`PACKET_SAMPLES`]. No key, no network.
//! Dropping the speech closes the connection, which stops the sentence in
//! the service: a barge-in or a stop costs it no more than one piece.
use super::{Error, Result, tts::without_tags};
use futures_util::stream::{self, BoxStream, StreamExt};
use lamp_msgs::phonon::{Head, head, request};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    io::AsyncWriteExt,
    net::UnixStream,
    sync::{Mutex, OwnedMutexGuard},
};

use super::PACKET_SAMPLES;

/// How long one read of the service's answer may stall, once its first
/// sound has come.
pub const STALL: Duration = Duration::from_secs(4);

/// How long the service has to send a sentence's first sound. Well inside
/// the voice's first-sound window (4 s), so a service busy with another
/// asker (`lamp-phonon` makes one sentence at a time, the speech cache's
/// lines too) fails the piece in time for the lamp's one voice to say it
/// (the Reviewer-lead's P2 on #8754's pull request: at 4 s, as [`STALL`],
/// it failed just as the window ran out, and the piece went unsaid). The
/// service's first sound took 158 to 475 ms on a lamp (`docs/phonon.md`).
pub const FIRST_WITHIN: Duration = Duration::from_millis(1_500);

/// The step a sentence fails at when `lamp-phonon` is not up: its socket
/// not there yet (the service still loading its model, or not started), or
/// a connection refused (a socket left by a service that stopped). Told
/// apart by [`PhononTts::not_up`] from a failure of a service that answered.
const NOT_UP: &str = "the lamp's voice's socket";

/// The service, at its socket, in the person's chosen voice.
#[derive(Clone, Debug)]
pub struct PhononTts {
    socket: PathBuf,
    voice: &'static str,
    /// One sentence at a time, as `lamp-phonon` makes them (lamp-6377, 26
    /// Nov: the voice asks a reply's pieces together, and a second piece
    /// waiting behind the first's making ran out its first-sound window and
    /// was said in the lamp's voice). A piece takes its turn before its
    /// window starts and gives it back when its sentence has been made.
    turn: Arc<Mutex<()>>,
}

/// A piece's turn at `lamp-phonon` ([`PhononTts::turn`]).
pub type Turn = OwnedMutexGuard<()>;

impl PhononTts {
    /// The service at `socket`, speaking in `voice` (a name
    /// `lamp_msgs::phonon::voice` took).
    pub fn new(socket: impl Into<PathBuf>, voice: lamp_msgs::phonon::Voice) -> Self {
        Self {
            socket: socket.into(),
            voice: voice.name,
            turn: Arc::new(Mutex::new(())),
        }
    }

    /// Whether `why` is a `lamp-phonon` that is not up ([`NOT_UP`]): no
    /// socket, or a connection refused, at once and before any request.
    /// Every other failure is of a service that answered: a refusal, no
    /// first sound within [`FIRST_WITHIN`], a stall or a break mid-answer.
    pub fn not_up(why: &Error) -> bool {
        *why == Error::Transport(NOT_UP)
    }

    /// The voice's name, for the journal.
    pub fn voice(&self) -> &'static str {
        self.voice
    }

    /// `text`'s speech from its first packet on, or why there is none: no
    /// service (not chosen, still fetching, stopped), a refusal, or a
    /// first sound later than [`FIRST_WITHIN`].
    pub async fn speak(&self, text: &str) -> Result<BoxStream<'static, Result<Vec<i16>>>> {
        let turn = self.turn().await;
        self.speak_in_turn(text, turn).await
    }

    /// This voice's turn: waits while another piece's sentence is being
    /// made, in the order the pieces asked. Not counted in a piece's
    /// first-sound window.
    pub async fn turn(&self) -> Turn {
        self.turn.clone().lock_owned().await
    }

    /// [`PhononTts::speak`] once `turn` is held; the turn is given back when
    /// the sentence has been made, fails or its speech is dropped.
    pub async fn speak_in_turn(
        &self,
        text: &str,
        turn: Turn,
    ) -> Result<BoxStream<'static, Result<Vec<i16>>>> {
        let text = without_tags(text);
        let mut connection = UnixStream::connect(&self.socket)
            .await
            .map_err(not_connected)?;
        connection
            .write_all(request(text.trim(), self.voice).as_bytes())
            .await
            .map_err(|_| Error::Transport("the lamp's voice's request"))?;
        let mut reader = Reader {
            connection,
            held: Vec::new(),
            ended: false,
            turn: Some(turn),
            stall: FIRST_WITHIN,
        };
        let first = reader.packet().await?;
        let rest = stream::try_unfold(reader, |mut reader| async move {
            Ok(reader.packet().await?.map(|packet| (packet, reader)))
        });
        Ok(stream::iter(first.map(Ok)).chain(rest).boxed())
    }
}

/// The answer read frame by frame, handed on in [`PACKET_SAMPLES`] packets.
struct Reader {
    connection: UnixStream,
    held: Vec<i16>,
    ended: bool,
    /// Given back at the end: the service is free for the next sentence.
    turn: Option<Turn>,
    /// [`FIRST_WITHIN`] until the first frame's head is read, then [`STALL`].
    stall: Duration,
}

impl Reader {
    /// The next packet; `None` at the end.
    async fn packet(&mut self) -> Result<Option<Vec<i16>>> {
        while !self.ended && self.held.len() < PACKET_SAMPLES {
            let mut bytes = [0; 4];
            read_within(&mut self.connection, &mut bytes, self.stall).await?;
            self.stall = STALL;
            match head(bytes) {
                Head::End => {
                    self.ended = true;
                    self.turn = None;
                }
                Head::Failed => return Err(Error::Protocol("the lamp's voice's failure")),
                Head::Samples(n) => {
                    let mut pcm = vec![0; n * 2];
                    read_within(&mut self.connection, &mut pcm, STALL).await?;
                    self.held.extend(
                        pcm.chunks_exact(2)
                            .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
                    );
                }
            }
        }
        if self.held.is_empty() {
            return Ok(None);
        }
        let take = self.held.len().min(PACKET_SAMPLES);
        Ok(Some(self.held.drain(..take).collect()))
    }
}

/// Why the service's socket took no connection: not up ([`NOT_UP`]) when
/// it is not there or refused, else the connection's own failure.
fn not_connected(why: std::io::Error) -> Error {
    match why.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            Error::Transport(NOT_UP)
        }
        _ => Error::Transport("the lamp's voice's connection"),
    }
}

async fn read_within(connection: &mut UnixStream, bytes: &mut [u8], stall: Duration) -> Result<()> {
    match tokio::time::timeout(stall, connection.read_exact(bytes)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) => Err(Error::Transport("the lamp's voice's answer")),
        Err(_) => Err(Error::Timeout("the lamp's voice's answer")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::phonon::{END, FAIL, frame, parse_request};
    use tokio::{io::AsyncBufReadExt, net::UnixListener};

    /// A service at a socket of its own that answers one connection with
    /// `frames` then `tail`, and the request it read.
    async fn service(
        frames: Vec<Vec<i16>>,
        tail: Option<[u8; 4]>,
    ) -> (Folder, PathBuf, tokio::task::JoinHandle<(String, bool)>) {
        let folder = Folder::new();
        let path = folder.0.join("p.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let task = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            let mut connection = tokio::io::BufReader::new(connection);
            let mut line = String::new();
            connection.read_line(&mut line).await.unwrap();
            let mut connection = connection.into_inner();
            let mut closed = false;
            for samples in frames {
                if connection.write_all(&frame(&samples)).await.is_err() {
                    closed = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            if let Some(tail) = tail {
                closed |= connection.write_all(&tail).await.is_err();
            }
            (line, closed)
        });
        (folder, path, task)
    }

    /// A folder of the test's own, short enough for a socket's path, removed
    /// at the end.
    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ph{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn freya() -> lamp_msgs::phonon::Voice {
        lamp_msgs::phonon::voice("Freya").unwrap()
    }

    /// The sentence is asked without its speech tags, in the chosen voice,
    /// and its frames come back as packets of 960, the last one short.
    #[tokio::test]
    async fn a_sentence_is_asked_bare_and_comes_back_in_packets() {
        let (_folder, path, task) =
            service(vec![vec![7; 700], vec![8; 700], vec![9; 100]], Some(END)).await;
        let tts = PhononTts::new(&path, freya());
        let packets: Vec<Vec<i16>> = tts
            .speak("[warmly] Hello there.")
            .await
            .unwrap()
            .map(|packet| packet.unwrap())
            .collect()
            .await;
        assert_eq!(packets.iter().map(Vec::len).collect::<Vec<_>>(), [960, 540]);
        assert_eq!(packets[0][699], 7);
        assert_eq!(packets[0][700], 8);
        let (line, _) = task.await.unwrap();
        let (text, voice) = parse_request(&line).unwrap();
        assert_eq!((text.as_str(), voice.name), ("Hello there.", "Freya"));
    }

    /// No service, or one that fails before its first sound, is an error
    /// before any speech, so the lamp's own voice can say the sentence.
    /// No socket, or a connection refused, is a service not up; a failure
    /// it answered with is not.
    #[tokio::test]
    async fn no_service_or_a_failure_is_an_error_before_any_speech() {
        let folder = Folder::new();
        let tts = PhononTts::new(folder.0.join("none.sock"), freya());
        let none = tts.speak("Hello.").await.err().unwrap();
        assert!(PhononTts::not_up(&none), "{none}");
        let stale = folder.0.join("stale.sock");
        drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
        let refused = PhononTts::new(&stale, freya())
            .speak("Hello.")
            .await
            .err()
            .unwrap();
        assert!(PhononTts::not_up(&refused), "{refused}");
        let (_folder, path, _task) = service(vec![], Some(FAIL)).await;
        let tts = PhononTts::new(&path, freya());
        let failed = tts.speak("Hello.").await.err().unwrap();
        assert!(matches!(failed, Error::Protocol(_)));
        assert!(!PhononTts::not_up(&failed));
    }

    /// A service that takes the request and sends no sound (busy with
    /// another asker's sentence) is a timeout within [`FIRST_WITHIN`], well
    /// before the voice's 4 s first-sound window runs out.
    #[tokio::test]
    async fn a_service_with_no_first_sound_is_a_timeout_well_inside_the_window() {
        let folder = Folder::new();
        let path = folder.0.join("p.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let _held = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
            drop(connection);
        });
        let tts = PhononTts::new(&path, freya());
        let started = std::time::Instant::now();
        assert!(matches!(
            tts.speak("Hello.").await.err(),
            Some(Error::Timeout(_))
        ));
        let took = started.elapsed();
        assert!(
            took >= FIRST_WITHIN && took < Duration::from_millis(2_500),
            "{took:?}"
        );
    }

    /// Dropping the speech after its first packet (a barge-in) closes the
    /// connection, and the service's next frame finds it closed.
    #[tokio::test]
    async fn dropping_the_speech_closes_the_connection() {
        let (_folder, path, task) = service(vec![vec![1; 960]; 400], Some(END)).await;
        let tts = PhononTts::new(&path, freya());
        let mut speech = tts.speak("A long one.").await.unwrap();
        assert_eq!(speech.next().await.unwrap().unwrap().len(), 960);
        drop(speech);
        let (_, closed) = task.await.unwrap();
        assert!(closed, "the service wrote to the end");
    }
}
