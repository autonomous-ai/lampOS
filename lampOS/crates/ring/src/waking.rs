//! The waking face (#1681): what the ring shows from power-on until the
//! voice takes it. Nothing else of the lamp is up yet, so like the asleep
//! face and the setup light it needs no permit: [`WakingRing`] can show
//! [`waking_pixels`] and black, and nothing else. It begins as the asleep
//! face's faint warm glow at the bottom of the ring and opens slowly up it,
//! as an eye opens, breathing; a lamp whose voice never comes turns pale
//! instead ([`UNWELL`]). No channel is ever above [`PEAK_CHANNEL`].
//!
//! The writer that shows it (`lamp-setup wake`) lets the ring go when the
//! voice asks on [`RELEASE_SOCKET`]: the voice asks before every open of
//! its own ([`let_go`]), since a ring it cannot lock stays off all session.

use crate::{
    ChannelCeiling, EncodedFrame, FrameSink, PIXEL_COUNT, Rgb,
    asleep::{BOTTOM, PEAK as ASLEEP},
    encode,
};
use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

/// The brightest any channel gets: the product voice's ring ceiling, as the
/// setup light's.
pub const PEAK_CHANNEL: u8 = 24;
/// The face fully open, at the bottom pixel and the top of a breath: the
/// asleep face's warmth, twice as bright.
pub const AWAKE: Rgb = Rgb::new(22, 9, 2);
/// A lamp whose voice never came: pale and grey, over the whole ring.
pub const UNWELL: Rgb = Rgb::new(9, 9, 8);
/// The least of a breath, as a share of its top: never dark, so it reads as on.
pub const BREATH_LOW: f32 = 0.5;
/// The top of the ring is this much of the bottom once open: the glow
/// rises from where the asleep face was.
const TOP_SHARE: f32 = 0.6;
/// The half-width, in pixels from [`BOTTOM`], of the face closed (the
/// asleep face's five pixels) and open (the whole ring).
const CLOSED_HALF: f32 = 2.5;
const OPEN_HALF: f32 = PIXEL_COUNT as f32 / 2.0 + 2.0;
/// How many pixels the lit edge eases over.
const EDGE: f32 = 2.0;

/// Where the voice asks the waking face for the ring.
pub const RELEASE_SOCKET: &str = "/run/lampos-waking/release.sock";
/// The longest the voice waits for the ring to be let go: only for a
/// waking face that doesn't answer. One that does lets go in about
/// [`Ask::Fade`]'s fade, or at once.
pub const RELEASE_WAIT: Duration = Duration::from_secs(2);
/// The longest the physical-mute indication waits ([`Ask::AtOnce`]): a
/// waking face that answers has let go well within it (#1681, the first
/// builder: within 100 ms). One that hasn't by then loses the ring
/// ([`take`]).
pub const AT_ONCE_WAIT: Duration = Duration::from_millis(100);
/// What it is answered once the ring is closed.
pub const ANSWER: &[u8] = b"released\n";
/// The longest ask, in bytes.
pub const ASK_MOST: usize = 16;

/// How the voice asks for the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The face fades out first (about 600 ms): the voice's own face
    /// follows it.
    Fade,
    /// Black and closed at once, with no fade: the privacy switch closed
    /// during boot, and its indication must not wait (#1681, the first
    /// builder: within 100 ms).
    AtOnce,
}

impl Ask {
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Fade => b"release\n",
            Self::AtOnce => b"release now\n",
        }
    }

    /// The longest this ask waits for an answer.
    pub const fn wait(self) -> Duration {
        match self {
            Self::Fade => RELEASE_WAIT,
            Self::AtOnce => AT_ONCE_WAIT,
        }
    }

    /// The ask a line is, if it is one.
    pub fn parse(line: &[u8]) -> Option<Self> {
        [Self::Fade, Self::AtOnce]
            .into_iter()
            .find(|ask| ask.bytes() == line)
    }
}

fn mix(a: Rgb, b: Rgb, share: f32) -> [f32; 3] {
    let channel = |a: u8, b: u8| f32::from(a) + (f32::from(b) - f32::from(a)) * share;
    [
        channel(a.red, b.red),
        channel(a.green, b.green),
        channel(a.blue, b.blue),
    ]
}

/// The waking face `open` of the way (0 to 1) from the asleep face's glow to
/// the whole ring, `unwell` of the way (0 to 1) to the pale look, `breath`
/// of the way (0 to 1) up a breath, at `level` (0 to 1) of it. It is the
/// same on both sides of [`BOTTOM`], and no channel is above
/// [`PEAK_CHANNEL`].
pub fn waking_pixels(open: f32, unwell: f32, breath: f32, level: f32) -> [Rgb; PIXEL_COUNT] {
    let open = open.clamp(0.0, 1.0);
    let unwell = unwell.clamp(0.0, 1.0);
    let breath = BREATH_LOW + (1.0 - BREATH_LOW) * breath.clamp(0.0, 1.0);
    let level = level.clamp(0.0, 1.0) * breath;
    let warm = mix(ASLEEP, AWAKE, open);
    let half = CLOSED_HALF + (OPEN_HALF - CLOSED_HALF) * open;
    let mut pixels = [Rgb::BLACK; PIXEL_COUNT];
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let away = index.abs_diff(BOTTOM);
        let away = away.min(PIXEL_COUNT - away) as f32;
        let edge = ((half - away) / EDGE).clamp(0.0, 1.0);
        let edge = edge * edge * (3.0 - 2.0 * edge);
        let rise = 1.0 - (1.0 - TOP_SHARE) * away / (PIXEL_COUNT as f32 / 2.0);
        let face = [
            warm[0] * edge * rise,
            warm[1] * edge * rise,
            warm[2] * edge * rise,
        ];
        let pale = [
            f32::from(UNWELL.red),
            f32::from(UNWELL.green),
            f32::from(UNWELL.blue),
        ];
        let channel = |channel: usize| {
            let mixed = face[channel] + (pale[channel] - face[channel]) * unwell;
            (mixed * level).round().min(f32::from(PEAK_CHANNEL)) as u8
        };
        *pixel = Rgb::new(channel(0), channel(1), channel(2));
    }
    pixels
}

/// A ring writer that shows only the waking face or black. Opening it sends
/// black; [`WakingRing::shutdown`] sends black again. It holds no authority
/// and grants none: `lamp-setup wake` uses it only until the voice asks for
/// the ring, and closes it before answering.
pub struct WakingRing<S: FrameSink> {
    sink: S,
}

impl<S: FrameSink> WakingRing<S> {
    pub fn new(mut sink: S) -> io::Result<Self> {
        sink.write_frame(&EncodedFrame::black())?;
        Ok(Self { sink })
    }

    /// Show [`waking_pixels`] for these.
    pub fn show(&mut self, open: f32, unwell: f32, breath: f32, level: f32) -> io::Result<()> {
        self.sink.write_frame(&encode(
            &waking_pixels(open, unwell, breath, level),
            ChannelCeiling(PEAK_CHANNEL),
        ))
    }

    /// Black, then the writer is gone.
    pub fn shutdown(mut self) -> io::Result<()> {
        self.sink.write_frame(&EncodedFrame::black())
    }
}

/// What asking the waking face for the ring came to.
#[derive(Debug)]
pub enum LetGo {
    /// No waking face is running: the ring is as free as it was.
    NotWaking,
    /// It closed the ring and said so.
    Released,
    /// It did not answer within the wait: the open goes on, and may fail.
    NotInTime,
    /// The socket was there and failed.
    Failed(io::Error),
}

/// Ask the waking face at `socket` to let the ring go, as `ask` says, and
/// wait at most `wait` for it to close it. No socket, or nothing listening,
/// is [`LetGo::NotWaking`] at once: the waking face is off or has ended.
pub fn let_go_at(socket: &Path, ask: Ask, wait: Duration) -> LetGo {
    let until = Instant::now() + wait;
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return LetGo::NotWaking;
        }
        Err(error) => return LetGo::Failed(error),
    };
    let asked = (|| {
        stream.set_write_timeout(Some(wait.max(Duration::from_millis(1))))?;
        stream.write_all(ask.bytes())?;
        let mut answer = Vec::new();
        let mut byte = [0; 1];
        while answer.len() < ANSWER.len() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            // macOS refuses a timeout on a socket its peer has closed;
            // what is already there is still read.
            let _ = stream.set_read_timeout(Some(left));
            match stream.read(&mut byte)? {
                0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                _ => answer.push(byte[0]),
            }
        }
        if answer == ANSWER {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the waking face answered something else",
            ))
        }
    })();
    match asked {
        Ok(()) => LetGo::Released,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            LetGo::NotInTime
        }
        Err(error) => LetGo::Failed(error),
    }
}

/// [`let_go_at`] at [`RELEASE_SOCKET`] within the ask's own
/// [`Ask::wait`]: what the voice's openers call first. Anything but the
/// ring being free already, or let go, is said once on standard error
/// under `who`.
pub fn let_go(who: &str, ask: Ask) -> LetGo {
    said(who, let_go_at(Path::new(RELEASE_SOCKET), ask, ask.wait()))
}

fn said(who: &str, outcome: LetGo) -> LetGo {
    match &outcome {
        LetGo::NotWaking | LetGo::Released => {}
        LetGo::NotInTime => eprintln!("{who}: the waking face did not let the ring go in time"),
        LetGo::Failed(error) => eprintln!("{who}: asking the waking face for the ring: {error}"),
    }
    outcome
}

/// How [`take`] has the ring device opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    /// With its lock, as every opener does.
    Locked,
    /// Without it: the lock is held by someone else.
    Unlocked,
}

/// The physical-mute indication's open: it never depends on another
/// process (#1681 review). The waking face is asked to let go at once and
/// waited for at most [`AT_ONCE_WAIT`]; then the ring is opened with its
/// lock, and if the lock is still held, it is opened without it: the
/// indication takes the device. A waking face that is alive but stuck
/// still holds the lock, and once its write returns it may land the frame
/// it was stuck on; then, seeing the ask, it closes the ring with no black
/// of its own (`lamp-setup`'s `AT_ONCE_BLACK`), and the indication paints
/// again every 25 ms. Only this opener takes the device: the voice's other
/// owners are kept apart by the supervisor, so a lock held at its open is
/// the waking face's. A stall in the kernel's SPI transfer itself would
/// hold every writer of the device alike; that is not the face's to give.
pub fn take<T>(open: impl FnMut(Lock) -> io::Result<T>) -> io::Result<T> {
    take_at(Path::new(RELEASE_SOCKET), open)
}

/// [`take`], asking at `socket`.
pub fn take_at<T>(socket: &Path, mut open: impl FnMut(Lock) -> io::Result<T>) -> io::Result<T> {
    let who = "lamp-voice privacy ring";
    said(who, let_go_at(socket, Ask::AtOnce, Ask::AtOnce.wait()));
    match open(Lock::Locked) {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            eprintln!("{who}: the ring is still locked; taking it without the lock");
            open(Lock::Unlocked)
        }
        opened => opened,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread};

    fn lit(pixels: &[Rgb; PIXEL_COUNT]) -> usize {
        pixels.iter().filter(|pixel| **pixel != Rgb::BLACK).count()
    }

    #[test]
    fn every_picture_is_capped_and_the_same_on_both_sides_of_the_bottom() {
        for open in [0.0, 0.1, 0.4, 0.7, 1.0] {
            for unwell in [0.0, 0.5, 1.0] {
                for breath in [0.0, 0.5, 1.0] {
                    let pixels = waking_pixels(open, unwell, breath, 1.0);
                    for (index, pixel) in pixels.iter().enumerate() {
                        assert!(pixel.red.max(pixel.green).max(pixel.blue) <= PEAK_CHANNEL);
                        let mirror = (2 * BOTTOM + PIXEL_COUNT - index) % PIXEL_COUNT;
                        assert_eq!(*pixel, pixels[mirror], "{open} {unwell} {index}");
                    }
                }
            }
        }
        assert!(
            waking_pixels(1.0, 1.0, 1.0, 0.0)
                .iter()
                .all(|p| *p == Rgb::BLACK)
        );
    }

    #[test]
    fn closed_it_is_the_asleep_glow_and_it_opens_up_the_ring_warm() {
        let closed = waking_pixels(0.0, 0.0, 1.0, 1.0);
        assert_eq!(lit(&closed), 5, "{closed:?}");
        assert_eq!(closed[BOTTOM], ASLEEP);
        let mut was = 0;
        for step in 0..=10 {
            let pixels = waking_pixels(step as f32 / 10.0, 0.0, 1.0, 1.0);
            let now = lit(&pixels);
            assert!(now >= was, "it only opens");
            was = now;
            for pixel in pixels {
                assert!(
                    pixel.red >= pixel.green && pixel.green >= pixel.blue,
                    "warm"
                );
            }
        }
        let open = waking_pixels(1.0, 0.0, 1.0, 1.0);
        assert_eq!(lit(&open), PIXEL_COUNT);
        assert_eq!(open[BOTTOM], AWAKE);
        assert!(open[0].red < open[BOTTOM].red, "the bottom is brightest");
    }

    #[test]
    fn a_breath_never_goes_dark_and_unwell_is_pale_over_the_whole_ring() {
        let low = waking_pixels(0.0, 0.0, 0.0, 1.0)[BOTTOM].red;
        let high = waking_pixels(0.0, 0.0, 1.0, 1.0)[BOTTOM].red;
        assert!(low > 0 && low < high, "{low} {high}");
        let pale = waking_pixels(1.0, 1.0, 1.0, 1.0);
        assert!(pale.iter().all(|pixel| *pixel == UNWELL));
        let Rgb { red, green, blue } = UNWELL;
        assert!(red - blue <= 1 && red == green, "grey, not a colour");
    }

    struct Frames(Vec<EncodedFrame>);
    impl FrameSink for &mut Frames {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            self.0.push(frame.clone());
            Ok(())
        }
    }

    #[test]
    fn the_writer_begins_and_ends_black() {
        let mut frames = Frames(Vec::new());
        let mut ring = WakingRing::new(&mut frames).unwrap();
        ring.show(0.3, 0.0, 0.5, 1.0).unwrap();
        ring.shutdown().unwrap();
        assert_eq!(frames.0.len(), 3);
        assert_eq!(frames.0[0], EncodedFrame::black());
        assert_eq!(
            frames.0[1],
            encode(&waking_pixels(0.3, 0.0, 0.5, 1.0), ChannelCeiling(24))
        );
        assert_eq!(frames.0[2], EncodedFrame::black());
    }

    #[test]
    fn only_the_voices_own_ask_waits_the_long_fallback() {
        assert_eq!(Ask::Fade.wait(), RELEASE_WAIT);
        assert!(Ask::AtOnce.wait() <= Duration::from_millis(100));
    }

    #[test]
    fn the_privacy_open_locks_when_it_can_and_takes_it_when_it_cannot() {
        let path = socket("take");
        let mut tried = Vec::new();
        let opened = take_at(&path, |lock| {
            tried.push(lock);
            Ok(lock)
        });
        assert_eq!(opened.unwrap(), Lock::Locked);
        assert_eq!(tried, [Lock::Locked]);
        tried.clear();
        let opened = take_at(&path, |lock| {
            tried.push(lock);
            match lock {
                Lock::Locked => Err(io::Error::from(io::ErrorKind::WouldBlock)),
                Lock::Unlocked => Ok(lock),
            }
        });
        assert_eq!(opened.unwrap(), Lock::Unlocked);
        assert_eq!(tried, [Lock::Locked, Lock::Unlocked]);
        // Any other failure is the open's, and is not retried unlocked.
        tried.clear();
        let opened = take_at(&path, |lock| {
            tried.push(lock);
            Err::<Lock, _>(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(opened.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(tried, [Lock::Locked]);
    }

    #[test]
    fn each_ask_is_one_short_line_and_nothing_else_is_one() {
        for ask in [Ask::Fade, Ask::AtOnce] {
            assert!(ask.bytes().len() <= ASK_MOST && ask.bytes().ends_with(b"\n"));
            assert_eq!(Ask::parse(ask.bytes()), Some(ask));
        }
        assert_eq!(Ask::parse(b"release"), None);
        assert_eq!(Ask::parse(b"released\n"), None);
    }

    fn socket(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("lamp-waking-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn with_no_waking_face_the_voice_goes_on_at_once() {
        let path = socket("none");
        let started = Instant::now();
        assert!(matches!(
            let_go_at(&path, Ask::Fade, RELEASE_WAIT),
            LetGo::NotWaking
        ));
        // A socket left behind with nobody listening is the same.
        drop(UnixListener::bind(&path).unwrap());
        assert!(matches!(
            let_go_at(&path, Ask::AtOnce, RELEASE_WAIT),
            LetGo::NotWaking
        ));
        assert!(started.elapsed() < Duration::from_millis(100));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_voice_waits_for_the_answer_and_no_longer_than_its_wait() {
        let path = socket("answer");
        let listener = UnixListener::bind(&path).unwrap();
        let answering = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = [0; 8];
            stream.read_exact(&mut asked).unwrap();
            assert_eq!(Ask::parse(&asked), Some(Ask::Fade));
            thread::sleep(Duration::from_millis(100));
            stream.write_all(ANSWER).unwrap();
            // A second, which never answers.
            let (stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(600));
            drop(stream);
        });
        let started = Instant::now();
        assert!(matches!(
            let_go_at(&path, Ask::Fade, RELEASE_WAIT),
            LetGo::Released
        ));
        assert!(started.elapsed() >= Duration::from_millis(100));
        let started = Instant::now();
        let outcome = let_go_at(&path, Ask::AtOnce, Duration::from_millis(300));
        assert!(matches!(outcome, LetGo::NotInTime), "{outcome:?}");
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(300) && waited < Duration::from_millis(550),
            "{waited:?}"
        );
        answering.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
