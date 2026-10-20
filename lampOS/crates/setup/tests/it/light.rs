//! The setup light (#806, `docs/setup.md`) on a fake ring: it follows the
//! setup status and lets the ring go before the marker that starts the lamp.

use lamp_ring::{EncodedFrame, FrameSink, PRIMER_BYTES, Rgb, setup::SetupRing};
use lamp_setup::{
    light::{Clock, FADE_IN, FADE_OUT, FRAME, Light, RETRY, ReleasingSave},
    serve::{Phase, Save, Status},
};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    /// A frame: the colour of its first pixel (the whole ring is one).
    Frame(Rgb),
    /// The ring closed.
    Closed,
    /// The marker written.
    Marker,
}

type Seens = Arc<Mutex<Vec<Seen>>>;

struct Sink(Seens);

/// The first pixel of a WS2812 frame: green, red, blue, a bit to a byte.
fn first_pixel(frame: &EncodedFrame) -> Rgb {
    let channel = |index: usize| {
        let start = PRIMER_BYTES + index * 8;
        frame.bytes()[start..start + 8]
            .iter()
            .fold(0u8, |value, byte| value << 1 | u8::from(*byte == 0xFC))
    };
    Rgb::new(channel(1), channel(0), channel(2))
}

impl FrameSink for Sink {
    fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
        self.0.lock().unwrap().push(Seen::Frame(first_pixel(frame)));
        Ok(())
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.0.lock().unwrap().push(Seen::Closed);
    }
}

struct Marker(Seens);

impl Save for Marker {
    fn credential(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn name(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn timezone(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn backend(&mut self, _: &lamp_setup::serve::Backend) -> io::Result<()> {
        Ok(())
    }
    fn marker(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().push(Seen::Marker);
        Ok(())
    }
}

fn white(Rgb { red, green, blue }: Rgb) -> bool {
    red >= 8 && red >= green && green >= blue && blue > 0
}
fn blue(Rgb { red, green, blue }: Rgb) -> bool {
    blue >= 8 && blue > green && green > red
}
fn amber(Rgb { red, green, blue }: Rgb) -> bool {
    red >= 8 && red > 2 * green && blue == 0
}

/// Wait up to 4 s for the last frame shown to be `wanted`.
fn shows(seen: &Seens, wanted: fn(Rgb) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(4);
    while Instant::now() < until {
        if let Some(Seen::Frame(colour)) = seen.lock().unwrap().last()
            && wanted(*colour)
        {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}

/// The painter's clock in the tests that would otherwise wait out its fades:
/// each frame's pause moves it on by exactly the frame and waits a fiftieth
/// of that. So a fade takes no time, and the painter's time is its frames,
/// however the machine stalls. A test can hold it, so that the painter waits
/// at its next frame while the test does several things.
#[derive(Clone)]
struct Fast(Arc<(Instant, Mutex<Duration>, Mutex<()>)>);

impl Fast {
    fn new() -> Self {
        Self(Arc::new((
            Instant::now(),
            Mutex::new(Duration::ZERO),
            Mutex::new(()),
        )))
    }

    /// Hold the painter at its next frame until the guard is dropped.
    fn hold(&self) -> std::sync::MutexGuard<'_, ()> {
        self.0.2.lock().unwrap()
    }

    /// How long the painter has painted, in its own time.
    fn painted(&self) -> Duration {
        *self.0.1.lock().unwrap()
    }

    /// Wait until the painter has painted `long` more of its own time.
    fn wait(&self, long: Duration) {
        let until = self.painted() + long;
        let cap = Instant::now() + Duration::from_secs(4);
        while self.painted() < until {
            assert!(Instant::now() < cap, "the painter stopped");
            thread::sleep(Duration::from_micros(200));
        }
    }
}

impl Clock for Fast {
    fn now(&self) -> Instant {
        self.0.0 + self.painted()
    }
    fn pause(&self, pause: Duration) {
        drop(self.0.2.lock().unwrap());
        *self.0.1.lock().unwrap() += pause;
        thread::sleep(pause / 50);
    }
}

fn set(status: &Mutex<Status>, phase: Phase, run: u64) {
    let mut status = status.lock().unwrap();
    status.phase = phase;
    status.run = run;
}

#[test]
fn the_light_follows_setup_and_lets_the_ring_go_before_the_marker() {
    let seen: Seens = Arc::default();
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    let opens = Arc::new(Mutex::new(0));
    let clock = Fast::new();
    let light = Light::spawn_with_clock(
        Arc::clone(&status),
        {
            let seen = Arc::clone(&seen);
            let opens = Arc::clone(&opens);
            move || {
                *opens.lock().unwrap() += 1;
                SetupRing::new(Sink(Arc::clone(&seen)))
            }
        },
        |line| panic!("nothing should be said: {line}"),
        RETRY,
        clock.clone(),
    );
    // Waiting for the person: soft white, from a black first frame.
    assert!(shows(&seen, white), "{:?}", seen.lock().unwrap().last());
    assert_eq!(seen.lock().unwrap()[0], Seen::Frame(Rgb::BLACK));
    // Joining: soft blue.
    set(&status, Phase::Connecting, 1);
    assert!(shows(&seen, blue), "{:?}", seen.lock().unwrap().last());

    // The join went well: the ring is black and closed before the marker,
    // and letting it go did not wait out `RELEASE_WAIT` (the host's clock).
    let released = Instant::now();
    let mut save = ReleasingSave {
        save: Marker(Arc::clone(&seen)),
        light: Some(&light),
    };
    save.marker().unwrap();
    assert!(released.elapsed() < Duration::from_secs(2));
    let tail: Vec<Seen> = seen.lock().unwrap().iter().rev().take(3).copied().collect();
    assert_eq!(
        tail,
        [Seen::Marker, Seen::Closed, Seen::Frame(Rgb::BLACK)],
        "newest first"
    );

    // Saving failed after all: the hotspot is back, and so is the ring, amber.
    set(&status, Phase::Failed, 1);
    assert!(shows(&seen, amber), "{:?}", seen.lock().unwrap().last());
    assert_eq!(*opens.lock().unwrap(), 2);

    // The next try joins, saves, and the lamp is set up: the ring stays the lamp's.
    set(&status, Phase::Connecting, 2);
    assert!(shows(&seen, blue), "{:?}", seen.lock().unwrap().last());
    save.marker().unwrap();
    set(&status, Phase::Connected, 2);
    let count = seen.lock().unwrap().len();
    clock.wait(Duration::from_millis(300));
    assert_eq!(
        seen.lock().unwrap().len(),
        count,
        "nothing after the marker"
    );
    assert_eq!(seen.lock().unwrap()[count - 1], Seen::Marker);
    assert_eq!(*opens.lock().unwrap(), 2);
    assert!(light.stop());
}

#[test]
fn stopping_ends_with_black_and_a_closed_ring() {
    let seen: Seens = Arc::default();
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    let light = Light::spawn(
        Arc::clone(&status),
        {
            let seen = Arc::clone(&seen);
            move || SetupRing::new(Sink(Arc::clone(&seen)))
        },
        |line| panic!("nothing should be said: {line}"),
    );
    assert!(shows(&seen, white));
    assert!(light.stop());
    let tail: Vec<Seen> = seen.lock().unwrap().iter().rev().take(2).copied().collect();
    assert_eq!(
        tail,
        [Seen::Closed, Seen::Frame(Rgb::BLACK)],
        "newest first"
    );
}

#[test]
fn a_ring_that_cannot_be_opened_is_said_once_and_holds_nothing_up() {
    let said = Arc::new(Mutex::new(Vec::<String>::new()));
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    let light = Light::spawn(
        Arc::clone(&status),
        || -> io::Result<SetupRing<Sink>> { Err(io::Error::other("no ring here")) },
        {
            let said = Arc::clone(&said);
            move |line: &str| said.lock().unwrap().push(line.to_owned())
        },
    );
    thread::sleep(Duration::from_millis(400));
    assert_eq!(
        *said.lock().unwrap(),
        ["the setup light cannot open the ring: no ring here"]
    );
    // With no ring open, letting it go waits for nothing.
    let released = Instant::now();
    assert!(light.release());
    assert!(released.elapsed() < Duration::from_millis(200));
    assert!(light.stop());
}

#[test]
fn a_ring_freed_in_its_retry_window_is_not_opened_after_the_marker() {
    // The reviewer's probe on #1325: the ring failed, so none was open and
    // letting go returned at once; the retry then came due during the fade
    // out, after the marker, and took the device the voice was starting on.
    // Here every frame is a retry, and the ring is free from the marker on.
    let seen: Seens = Arc::default();
    let said = Arc::new(Mutex::new(Vec::<String>::new()));
    let busy = Arc::new(AtomicBool::new(true));
    let opens = Arc::new(AtomicUsize::new(0));
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    set(&status, Phase::Connecting, 1);
    let clock = Fast::new();
    let light = Light::spawn_with_clock(
        Arc::clone(&status),
        {
            let (seen, busy, opens) = (Arc::clone(&seen), Arc::clone(&busy), Arc::clone(&opens));
            move || {
                if busy.load(Ordering::SeqCst) {
                    return Err(io::Error::from(io::ErrorKind::WouldBlock));
                }
                opens.fetch_add(1, Ordering::SeqCst);
                SetupRing::new(Sink(Arc::clone(&seen)))
            }
        },
        {
            let said = Arc::clone(&said);
            move |line: &str| said.lock().unwrap().push(line.to_owned())
        },
        Duration::ZERO,
        clock.clone(),
    );
    let until = Instant::now() + Duration::from_secs(4);
    while said.lock().unwrap().is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(said.lock().unwrap().len(), 1);
    // Faded in (with no ring), so letting go has a fade out to wait through.
    clock.wait(FADE_IN);
    // The painter is held from before the marker until the ring is free, so
    // all of its fade out comes after both: a ring opened in it is seen. No
    // ring is open, so letting go does not wait for the painter.
    let held = clock.hold();
    ReleasingSave {
        save: Marker(Arc::clone(&seen)),
        light: Some(&light),
    }
    .marker()
    .unwrap();
    busy.store(false, Ordering::SeqCst);
    drop(held);
    clock.wait(FADE_OUT + Duration::from_millis(300));
    assert_eq!(opens.load(Ordering::SeqCst), 0, "opened after the marker");
    assert_eq!(*seen.lock().unwrap(), [Seen::Marker]);
    // Saving failed after all: then the ring is wanted again, and opened.
    set(&status, Phase::Failed, 1);
    assert!(shows(&seen, amber), "{:?}", seen.lock().unwrap().last());
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert!(light.stop());
}

#[test]
fn the_press_after_a_failed_save_shows_blue_at_once() {
    // The page's press sets `connecting` at once and the run is counted 2 s
    // later (the reviewer on #1325): the ring must not go dark in between.
    let seen: Seens = Arc::default();
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    set(&status, Phase::Connecting, 1);
    let light = Light::spawn_with_clock(
        Arc::clone(&status),
        {
            let seen = Arc::clone(&seen);
            move || SetupRing::new(Sink(Arc::clone(&seen)))
        },
        |line| panic!("nothing should be said: {line}"),
        RETRY,
        Fast::new(),
    );
    assert!(shows(&seen, blue));
    ReleasingSave {
        save: Marker(Arc::clone(&seen)),
        light: Some(&light),
    }
    .marker()
    .unwrap();
    set(&status, Phase::Failed, 1);
    assert!(shows(&seen, amber), "{:?}", seen.lock().unwrap().last());
    // The press, with the frames shown so far counted at the same moment.
    let amber_from = {
        let shown = seen.lock().unwrap();
        set(&status, Phase::Connecting, 1);
        shown.len()
    };
    assert!(shows(&seen, blue), "{:?}", seen.lock().unwrap().last());
    // At once in the painter's own time: its frames from the press to the
    // first blue one, each a frame of its time on this clock.
    let frames = seen.lock().unwrap()[amber_from..]
        .iter()
        .position(|seen| matches!(seen, Seen::Frame(colour) if blue(*colour)))
        .unwrap()
        + 1;
    assert!(
        FRAME * u32::try_from(frames).unwrap() < Duration::from_millis(1_500),
        "blue after {frames} frames"
    );
    assert!(
        !seen.lock().unwrap()[amber_from..].contains(&Seen::Closed),
        "the ring went dark between the press and its run"
    );
    assert!(light.stop());
}

#[test]
fn with_the_switch_off_no_ring_is_opened() {
    let opened = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(Status::new("lamp-test".into())));
    let light = Light::spawn_if(
        false,
        status,
        {
            let opened = Arc::clone(&opened);
            move || {
                opened.store(true, Ordering::SeqCst);
                SetupRing::new(Sink(Arc::default()))
            }
        },
        |line| panic!("nothing should be said: {line}"),
    );
    assert!(light.is_none());
    thread::sleep(Duration::from_millis(200));
    assert!(!opened.load(Ordering::SeqCst));
}
