//! The camera's frames for one reader at a time (`--frames-socket`, the
//! fast `camera.stream`, docs/control.md "The camera"): every frame the
//! eyes capture, as the camera made it (MJPEG), unchanged and never decoded
//! for this, so it costs the eyes almost nothing.
//!
//! The reader (lamp-control, `lampos`) connects and says one line, the size
//! it wants: `640x480` or `1280x720`. While it is connected the eyes capture
//! at that size at [`FAST_FPS`] (the camera's own MJPEG modes), and go back
//! to their own size when it goes. Each frame is written as its length (4
//! bytes, big-endian) and its JPEG, whole, by a writer thread of its own:
//! the eyes hand it each frame and never wait, and a frame that finds
//! [`QUEUED`] already waiting is dropped, never the reader (on lamp-75c9,
//! 30 Oct, 1280x720's 90 KB frames filled the socket after two, and a write
//! that could go only in part had let the reader go). A reader that goes
//! ends the writer. A second reader is refused while one is connected.
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::time::Duration;

/// The sizes a reader may ask for: the camera's own MJPEG modes at 25 fps.
pub const SIZES: [(u32, u32); 2] = [(640, 480), (1_280, 720)];
/// The capture rate while a reader is connected.
pub const FAST_FPS: u32 = 25;
/// Buffers queued while fast: a frame every 40 ms with room for a slow look.
pub const FAST_BUFFERS: u8 = 4;
/// Frames waiting for the writer at most; a frame past that is dropped.
pub const QUEUED: usize = 2;
/// How long a new reader has to say its size.
const ASK_WITHIN: Duration = Duration::from_millis(200);

/// The size a reader's line asks for, if it's one of [`SIZES`].
pub fn size_of(line: &str) -> Option<(u32, u32)> {
    let (width, height) = line.trim().split_once('x')?;
    let size = (width.parse().ok()?, height.parse().ok()?);
    SIZES.contains(&size).then_some(size)
}

/// One reader: its size, its writer's queue, and whether it is still there.
struct Reader {
    size: (u32, u32),
    frames: SyncSender<Vec<u8>>,
    alive: Arc<AtomicBool>,
}

/// The socket, and its one reader with the size it asked for.
pub struct Tap {
    listener: UnixListener,
    reader: Option<Reader>,
    /// Frames handed to the writer, and dropped for a full queue, for the
    /// reader now, for the journal.
    pub sent: u64,
    pub dropped: u64,
}

impl Tap {
    /// Listens at `path` (a stale socket is replaced), for its owner only.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            reader: None,
            sent: 0,
            dropped: 0,
        })
    }

    /// The size the reader asked for, while one is connected.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.reader.as_ref().map(|reader| reader.size)
    }

    /// Takes a new reader, if one is waiting: what it said, once. A second
    /// while one is connected, or one asking a size there isn't, is closed.
    pub fn accept(&mut self) -> Option<String> {
        let (stream, _) = self.listener.accept().ok()?;
        if self.reader.is_some() {
            return Some("refused a second frames reader".to_owned());
        }
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(ASK_WITHIN));
        let mut line = String::new();
        let asked = BufReader::new(&stream)
            .read_line(&mut line)
            .ok()
            .and(size_of(&line));
        let Some(size) = asked else {
            return Some(format!("refused a frames reader asking {:?}", line.trim()));
        };
        // Written whole on its own thread, blocking only that thread.
        let (frames, queued) = mpsc::sync_channel::<Vec<u8>>(QUEUED);
        let alive = Arc::new(AtomicBool::new(true));
        let writing = alive.clone();
        let mut stream = stream;
        let started = std::thread::Builder::new()
            .name("frames".into())
            .spawn(move || {
                for frame in queued {
                    let length = u32::try_from(frame.len()).unwrap_or(u32::MAX).to_be_bytes();
                    if stream
                        .write_all(&length)
                        .and_then(|()| stream.write_all(&frame))
                        .is_err()
                    {
                        break;
                    }
                }
                writing.store(false, Ordering::SeqCst);
            });
        if started.is_err() {
            return Some("refused a frames reader: no writer thread".to_owned());
        }
        self.reader = Some(Reader {
            size,
            frames,
            alive,
        });
        (self.sent, self.dropped) = (0, 0);
        Some(format!("a frames reader at {} by {}", size.0, size.1))
    }

    /// One frame for the reader, if any: handed to its writer, never waited
    /// for, dropped if [`QUEUED`] are already waiting. Says when the reader
    /// has gone.
    pub fn send(&mut self, jpeg: &[u8]) -> Option<String> {
        let reader = self.reader.as_ref()?;
        if reader.alive.load(Ordering::SeqCst) {
            match reader.frames.try_send(jpeg.to_vec()) {
                Ok(()) => {
                    self.sent += 1;
                    return None;
                }
                Err(TrySendError::Full(_)) => {
                    self.dropped += 1;
                    return None;
                }
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
        self.reader = None;
        Some(format!(
            "the frames reader went, after {} frames ({} dropped for a full queue)",
            self.sent, self.dropped
        ))
    }

    /// The reader, gone or not keeping up: it is let go, and its writer ends.
    pub fn end(&mut self) {
        self.reader = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    fn socket() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eyes-tap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("frames.sock")
    }

    #[test]
    fn only_the_cameras_own_25_fps_sizes() {
        assert_eq!(size_of("640x480\n"), Some((640, 480)));
        assert_eq!(size_of("1280x720"), Some((1_280, 720)));
        assert_eq!(size_of("1280x960"), None);
        assert_eq!(size_of("big"), None);
    }

    #[test]
    fn a_reader_gets_each_frame_whole_and_a_second_is_refused() {
        let path = socket();
        let mut tap = Tap::bind(&path).unwrap();
        assert_eq!(tap.size(), None);
        let mut reader = UnixStream::connect(&path).unwrap();
        reader.write_all(b"640x480\n").unwrap();
        assert_eq!(
            tap.accept().as_deref(),
            Some("a frames reader at 640 by 480")
        );
        assert_eq!(tap.size(), Some((640, 480)));
        let _second = UnixStream::connect(&path).unwrap();
        assert_eq!(
            tap.accept().as_deref(),
            Some("refused a second frames reader")
        );
        assert_eq!(tap.send(&[0xff, 0xd8, 1, 0xff, 0xd9]), None);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(tap.send(&[0xff, 0xd8, 2, 2, 0xff, 0xd9]), None);
        let mut got = [0u8; 4 + 5 + 4 + 6];
        reader.read_exact(&mut got).unwrap();
        assert_eq!(&got[..4], &5u32.to_be_bytes());
        assert_eq!(&got[4..9], &[0xff, 0xd8, 1, 0xff, 0xd9]);
        assert_eq!(&got[9..13], &6u32.to_be_bytes());
        assert_eq!(tap.sent, 2);
    }

    #[test]
    fn a_slow_reader_loses_frames_never_its_stream_and_the_eyes_never_wait() {
        let path = socket();
        let mut tap = Tap::bind(&path).unwrap();
        let mut reader = UnixStream::connect(&path).unwrap();
        reader.write_all(b"1280x720\n").unwrap();
        tap.accept().unwrap();
        // 30 frames of 90 KB as fast as they come, nobody reading yet.
        let frame = vec![7u8; 90_000];
        let start = std::time::Instant::now();
        for _ in 0..30 {
            assert_eq!(tap.send(&frame), None, "the reader is kept");
        }
        assert!(start.elapsed() < Duration::from_millis(200), "never waited");
        assert!(tap.dropped > 0);
        // What did come is whole: each a length and that many bytes.
        let mut length = [0u8; 4];
        reader.read_exact(&mut length).unwrap();
        assert_eq!(u32::from_be_bytes(length), 90_000);
        let mut body = vec![0u8; 90_000];
        reader.read_exact(&mut body).unwrap();
        assert!(body.iter().all(|byte| *byte == 7));
        assert_eq!(tap.size(), Some((1_280, 720)));
    }

    #[test]
    fn a_reader_that_goes_or_asks_a_size_there_isnt_is_let_go() {
        let path = socket();
        let mut tap = Tap::bind(&path).unwrap();
        let mut wrong = UnixStream::connect(&path).unwrap();
        wrong.write_all(b"1280x960\n").unwrap();
        assert!(tap.accept().unwrap().starts_with("refused a frames reader"));
        assert_eq!(tap.size(), None);
        let mut reader = UnixStream::connect(&path).unwrap();
        reader.write_all(b"1280x720\n").unwrap();
        tap.accept().unwrap();
        drop(reader);
        // Written into a closed reader: let go, and said.
        let mut said = None;
        for _ in 0..50 {
            said = said.or(tap.send(&[0xff, 0xd8, 0xff, 0xd9]));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(said.unwrap().starts_with("the frames reader went"));
        assert_eq!(tap.size(), None);
    }
}
