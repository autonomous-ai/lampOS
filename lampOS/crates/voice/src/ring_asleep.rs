//! The asleep face while the lamp cannot hear (#712, `LAMP_RING_ASLEEP=on`).
//!
//! The ring worker lives and dies with each active cycle, and a muted lamp
//! has none, so its ring was dark: to a person that reads as off, or broken.
//! While the service waits out a mute or a closed privacy switch, this paints
//! the ring's asleep face ([`lamp_ring::asleep`]) from a thread of its own: a
//! faint warm glow at the bottom that breathes slowly. It is black and the
//! device is closed again before the wait returns, so the next cycle's ring
//! worker is the only writer.

use lamp_ring::{
    ChannelCeiling, FrameSink,
    asleep::{AsleepRing, PEAK},
};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The switch: the asleep face is shown only with `LAMP_RING_ASLEEP=on`.
pub const SWITCH: &str = "LAMP_RING_ASLEEP";
/// How long a wait lasts before the face shows. The wait at every cycle's
/// start, which only checks the privacy switch on the way to listening, is
/// over long before this, so a lamp that is not muted never shows it.
pub const AFTER: Duration = Duration::from_millis(1_500);
/// It fades in over this, as eyes close.
pub const FADE_IN: Duration = Duration::from_millis(1_500);
/// One breath, in and out: slower than the resting face's.
pub const BREATH: Duration = Duration::from_secs(6);
/// One frame this often: enough for a slow breath.
const FRAME: Duration = Duration::from_millis(50);
/// How often a waiting painter looks at its stop flag.
const STEP: Duration = Duration::from_millis(10);
/// How long a tap on the head of a muted lamp shows the asleep face: "about
/// a second", so the tap gets an answer while nothing moves and nothing is
/// heard (#711, First on #711, 17 Oct 10:18).
pub const TAP_SHOWS: Duration = Duration::from_millis(1_000);
/// Its fade in, and its fade out, each.
pub const TAP_FADE: Duration = Duration::from_millis(250);

/// The switch for a reminder's pulse over the asleep face (#1545): only
/// with `LAMP_REMINDER_PULSE=on`, and only where the asleep face is shown.
pub const PULSE_SWITCH: &str = "LAMP_REMINDER_PULSE";

/// How a reminder owed while the microphones are off shows on the asleep
/// face (#1545): one soft arc around the ring, taking `takes`, the first
/// when it became owed and then one each `every`, and none once it has
/// been owed for `for_at_most`, so a mute that lasts the night does not
/// pulse all night (the journal and the health still say it is owed, and
/// it is said when the microphones open).
#[derive(Clone, Copy, Debug)]
pub struct Pulses {
    pub takes: Duration,
    pub every: Duration,
    pub for_at_most: Duration,
}

impl Pulses {
    /// #1494's pulse's look: 4 s around, one a minute; half an hour of them.
    pub const OWED: Self = Self {
        takes: Duration::from_secs(4),
        every: Duration::from_secs(60),
        for_at_most: Duration::from_secs(30 * 60),
    };

    /// How far around a pulse is at `now` (0 to 1), if one is showing, for
    /// a reminder owed `since`: one starting then and one each `every`
    /// after, each for `takes`, none starting `for_at_most` after it. The
    /// minute and the half-hour count from `since` whatever held the pulse
    /// back meanwhile, so one missed is not made up and no hold starts the
    /// half-hour again (Thirteen and the reviewer on #1581).
    pub fn around(&self, since: Instant, now: Instant) -> Option<f32> {
        let owed_for = now.saturating_duration_since(since);
        let into = Duration::from_nanos((owed_for.as_nanos() % self.every.as_nanos()) as u64);
        (owed_for - into < self.for_at_most && into < self.takes)
            .then(|| into.as_secs_f32() / self.takes.as_secs_f32())
    }
}

/// Whether the pulse's switch is on in this process's environment.
pub fn pulse_enabled_from_environment() -> bool {
    enabled(std::env::var(PULSE_SWITCH).ok().as_deref())
}

/// What the muted wait tells the asleep face at each look (#1545): since
/// when the reminder owed has been owed, while its pulse may show now, and
/// nothing otherwise. The painter reads it each frame.
#[derive(Debug, Default)]
pub struct Owed(Mutex<Option<Instant>>);

impl Owed {
    pub fn tell(&self, showing_since: Option<Instant>) {
        if let Ok(mut owed) = self.0.lock() {
            *owed = showing_since;
        }
    }

    /// Since when it has been owed, while its pulse may show.
    pub fn showing(&self) -> Option<Instant> {
        self.0.lock().ok().and_then(|owed| *owed)
    }
}

/// Whether the switch is on in this process's environment.
pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// Whether the switch's value turns the face on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// How far up a breath (0 to 1) the face is `since` it began to show.
pub fn breath_at(since: Duration) -> f32 {
    let phase = since.as_secs_f32() / BREATH.as_secs_f32();
    0.5 - 0.5 * (std::f32::consts::TAU * phase).cos()
}

/// The ceiling `since` the face began to show: rising from 0 to `ceiling`
/// over [`FADE_IN`], never above what the face itself reaches.
pub fn ceiling_at(since: Duration, ceiling: ChannelCeiling) -> ChannelCeiling {
    let top = ceiling.get().min(PEAK.red);
    let share = (since.as_secs_f32() / FADE_IN.as_secs_f32()).clamp(0.0, 1.0);
    ChannelCeiling::new(u16::from((f32::from(top) * share).round() as u8))
        .expect("a share of a valid ceiling is valid")
}

/// The ceiling `since` a tap's face began, for a face that shows `shows`
/// ([`TAP_SHOWS`] on a lamp): up over [`TAP_FADE`] (or a quarter of a
/// shorter face), held, and down over as long at its end, never above what
/// the face itself reaches.
pub fn tap_ceiling_at(since: Duration, shows: Duration, ceiling: ChannelCeiling) -> ChannelCeiling {
    let top = ceiling.get().min(PEAK.red);
    let fade = TAP_FADE.min(shows / 4).max(Duration::from_millis(1));
    let left = shows.saturating_sub(since);
    let share = (since.min(left).as_secs_f32() / fade.as_secs_f32()).clamp(0.0, 1.0);
    ChannelCeiling::new(u16::from((f32::from(top) * share).round() as u8))
        .expect("a share of a valid ceiling is valid")
}

/// The longest stopping waits for the painter. A kernel SPI write cannot be
/// interrupted (`lamp_ring::linux`); a light that is stuck must not keep a
/// healthy voice from waking, so past this the painter is left behind and
/// the next cycle's ring worker, which is optional, finds the ring busy.
pub const STOP_DEADLINE: Duration = Duration::from_secs(1);

/// The asleep face painting from its own thread until stopped. Stopping, or
/// dropping, waits up to [`STOP_DEADLINE`] for the ring to be black and the
/// device closed.
pub struct Asleep {
    stop: Arc<AtomicBool>,
    /// A reminder owed, as the muted wait tells it (#1545).
    owed: Arc<Owed>,
    thread: Option<JoinHandle<()>>,
    done: Option<std::sync::mpsc::Receiver<()>>,
}

impl Asleep {
    /// Start painting on lamp-4ace's ring device under `ceiling`.
    #[cfg(target_os = "linux")]
    pub fn start(ceiling: u16) -> Option<Self> {
        let ceiling = ChannelCeiling::new(ceiling).ok()?;
        Some(Self::start_with(lamp_ring::linux::open_asleep, ceiling))
    }

    /// Start painting on whatever `open` gives, after [`AFTER`].
    pub fn start_with<S, F>(open: F, ceiling: ChannelCeiling) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        Self::start_pulsing_with(open, ceiling, Pulses::OWED)
    }

    /// [`Self::start_with`], a reminder owed shown as `pulses` say.
    pub fn start_pulsing_with<S, F>(open: F, ceiling: ChannelCeiling, pulses: Pulses) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        Self::spawn(move |stop, owed| paint(open, ceiling, (stop, owed), pulses))
    }

    /// Where the muted wait tells the face of a reminder owed (#1545):
    /// while its pulse may show, the face pulses as [`Pulses`] say.
    pub fn owed(&self) -> Arc<Owed> {
        self.owed.clone()
    }

    /// The face for a tap on the head of a muted lamp: at once, for
    /// [`TAP_SHOWS`], then black and the device closed by itself.
    #[cfg(target_os = "linux")]
    pub fn tapped(ceiling: u16) -> Option<Self> {
        let ceiling = ChannelCeiling::new(ceiling).ok()?;
        Some(Self::tapped_with(lamp_ring::linux::open_asleep, ceiling))
    }

    /// [`Self::tapped`], on whatever `open` gives.
    pub fn tapped_with<S, F>(open: F, ceiling: ChannelCeiling) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        Self::tapped_for(open, ceiling, TAP_SHOWS)
    }

    /// [`Self::tapped_with`] for `shows` instead of [`TAP_SHOWS`]: a test's
    /// short face, so it needn't wait a whole second.
    pub fn tapped_for<S, F>(open: F, ceiling: ChannelCeiling, shows: Duration) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        Self::spawn(move |stop, _| paint_tapped(open, ceiling, shows, stop))
    }

    fn spawn(paint: impl FnOnce(&AtomicBool, &Owed) -> io::Result<()> + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let owed = Arc::new(Owed::default());
        let flag = stop.clone();
        let owing = owed.clone();
        let (finished, done) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("ring-asleep".into())
            .spawn(move || {
                if let Err(error) = paint(&flag, &owing) {
                    eprintln!("lamp-voice ring: the asleep face stopped: {error}");
                }
                let _ = finished.send(());
            })
            .ok();
        Self {
            stop,
            owed,
            thread,
            done: Some(done),
        }
    }

    /// Whether its painting is over: a tap's ends by itself.
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stop painting: the ring is black and the device closed on return.
    pub fn stop(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let Some(thread) = self.thread.take() else {
            return;
        };
        let finished = self
            .done
            .take()
            .is_some_and(|done| done.recv_timeout(STOP_DEADLINE).is_ok());
        if finished || thread.is_finished() {
            let _ = thread.join();
        } else {
            eprintln!(
                "lamp-voice ring: the asleep face did not stop within {} ms (a ring write is stuck); going on without it",
                STOP_DEADLINE.as_millis()
            );
        }
    }
}

impl Drop for Asleep {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Wait [`AFTER`]; unless stopped by then, open the ring and paint the
/// breathing face until stopped, then black. While a reminder `owed` may
/// show, one soft arc goes around it as `pulses` say (#1545).
fn paint<S: FrameSink>(
    open: impl FnOnce() -> io::Result<AsleepRing<S>>,
    ceiling: ChannelCeiling,
    (stop, owed): (&AtomicBool, &Owed),
    pulses: Pulses,
) -> io::Result<()> {
    if !wait(AFTER, stop) {
        return Ok(());
    }
    let mut ring = open()?;
    let began = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let since = began.elapsed();
        let (breath, under) = (breath_at(since), ceiling_at(since, ceiling));
        let around = owed
            .showing()
            .and_then(|since| pulses.around(since, Instant::now()));
        let shown = match around {
            Some(around) => ring.show_owed(breath, around, under),
            None => ring.show(breath, under),
        };
        if let Err(error) = shown {
            let _ = ring.shutdown();
            return Err(error);
        }
        wait(FRAME, stop);
    }
    ring.shutdown()
}

/// A tap's face at once, faded in and out over `shows`, then black, unless
/// stopped first.
fn paint_tapped<S: FrameSink>(
    open: impl FnOnce() -> io::Result<AsleepRing<S>>,
    ceiling: ChannelCeiling,
    shows: Duration,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut ring = open()?;
    let began = Instant::now();
    while !stop.load(Ordering::SeqCst) && began.elapsed() < shows {
        // At the top of a breath: a second is a sixth of one.
        if let Err(error) = ring.show(1.0, tap_ceiling_at(began.elapsed(), shows, ceiling)) {
            let _ = ring.shutdown();
            return Err(error);
        }
        wait(FRAME, stop);
    }
    ring.shutdown()
}

/// The asleep face for about a second at each tap on the head while the
/// lamp waits out a mute (#711): one at a time, and none for the tap that
/// lets it hear again, since its ring worker is about to start.
pub struct TapFace<F: FnMut() -> Option<Asleep>> {
    start: F,
    showing: Option<Asleep>,
}

impl<F: FnMut() -> Option<Asleep>> TapFace<F> {
    pub fn new(start: F) -> Self {
        Self {
            start,
            showing: None,
        }
    }

    /// One look of the muted wait: whether a tap on the head was taken in
    /// it, and whether the lamp may hear again. True when a face began.
    pub fn poll(&mut self, tapped: bool, allowed: bool) -> bool {
        if self.showing.as_ref().is_some_and(Asleep::finished) {
            self.showing = None;
        }
        if tapped && !allowed && self.showing.is_none() {
            self.showing = (self.start)();
            return self.showing.is_some();
        }
        false
    }

    /// Black and the device closed, before the next cycle's ring worker.
    pub fn stop(self) {
        if let Some(showing) = self.showing {
            showing.stop();
        }
    }
}

/// Sleep `length` in short steps; false if stopped meanwhile.
fn wait(length: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + length;
    while Instant::now() < until {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(STEP.min(until.saturating_duration_since(Instant::now())));
    }
    !stop.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_ring::EncodedFrame;

    #[derive(Clone, Default)]
    struct Frames(Arc<Mutex<Vec<EncodedFrame>>>);
    impl FrameSink for Frames {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            self.0.lock().unwrap().push(frame.clone());
            Ok(())
        }
    }

    fn ceiling(value: u16) -> ChannelCeiling {
        ChannelCeiling::new(value).unwrap()
    }

    #[test]
    fn a_wait_stopped_before_the_delay_never_opens_the_ring() {
        let opened = Arc::new(AtomicBool::new(false));
        let flag = opened.clone();
        let asleep = Asleep::start_with(
            move || {
                flag.store(true, Ordering::SeqCst);
                AsleepRing::new(Frames::default())
            },
            ceiling(48),
        );
        std::thread::sleep(Duration::from_millis(100));
        asleep.stop();
        assert!(!opened.load(Ordering::SeqCst));
    }

    #[test]
    fn a_long_wait_paints_and_ends_black_with_the_painter_gone() {
        let frames = Frames::default();
        let sink = frames.clone();
        let asleep = Asleep::start_with(move || AsleepRing::new(sink), ceiling(48));
        std::thread::sleep(AFTER + Duration::from_millis(400));
        asleep.stop();
        let frames = frames.0.lock().unwrap();
        // Black on opening, some faces, black on closing; nothing after.
        assert!(frames.len() >= 4, "{}", frames.len());
        assert_eq!(frames[0], EncodedFrame::black());
        assert_eq!(*frames.last().unwrap(), EncodedFrame::black());
        assert!(
            frames[1..frames.len() - 1]
                .iter()
                .any(|frame| *frame != EncodedFrame::black())
        );
    }

    /// A sink whose writes never return once the face has begun: a stuck
    /// SPI write.
    struct Stuck(Arc<AtomicBool>);
    impl FrameSink for Stuck {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            if *frame != EncodedFrame::black() {
                self.0.store(true, Ordering::SeqCst);
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            Ok(())
        }
    }

    #[test]
    fn a_stuck_ring_write_never_keeps_the_wait_from_returning() {
        let stuck = Arc::new(AtomicBool::new(false));
        let flag = stuck.clone();
        let asleep = Asleep::start_with(move || AsleepRing::new(Stuck(flag)), ceiling(48));
        while !stuck.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let began = Instant::now();
        asleep.stop();
        let took = began.elapsed();
        assert!(
            took >= STOP_DEADLINE && took < STOP_DEADLINE * 2,
            "{took:?}"
        );
    }

    #[test]
    fn it_fades_in_and_breathes_slowly() {
        assert_eq!(ceiling_at(Duration::ZERO, ceiling(48)).get(), 0);
        assert_eq!(ceiling_at(FADE_IN, ceiling(48)).get(), PEAK.red);
        assert_eq!(ceiling_at(FADE_IN * 3, ceiling(6)).get(), 6);
        assert!(breath_at(Duration::ZERO) < 0.01);
        assert!(breath_at(BREATH / 2) > 0.99);
        // A 50 ms frame never moves a breath by more than a few hundredths.
        let mut since = Duration::ZERO;
        while since < BREATH {
            let next = since + FRAME;
            assert!((breath_at(next) - breath_at(since)).abs() < 0.03);
            since = next;
        }
    }

    /// #711: a tap on a muted lamp's head shows the asleep face at once,
    /// up over a quarter of a second, held, down over the last quarter, and
    /// then black and the device closed by itself, within its second.
    #[test]
    fn a_taps_face_shows_at_once_fades_in_and_out_and_closes_by_itself() {
        let top = ceiling(48);
        assert_eq!(tap_ceiling_at(Duration::ZERO, TAP_SHOWS, top).get(), 0);
        assert_eq!(
            tap_ceiling_at(TAP_SHOWS / 2, TAP_SHOWS, top).get(),
            PEAK.red
        );
        assert_eq!(tap_ceiling_at(TAP_SHOWS, TAP_SHOWS, top).get(), 0);
        let quarter = tap_ceiling_at(TAP_FADE / 2, TAP_SHOWS, top).get();
        assert!(0 < quarter && quarter < PEAK.red, "{quarter}");
        // The painter, on a face a test makes short: it ends by itself.
        let shows = Duration::from_millis(200);
        let frames = Frames::default();
        let sink = frames.clone();
        let started = Instant::now();
        let face = Asleep::tapped_for(move || AsleepRing::new(sink), top, shows);
        while !face.finished() {
            assert!(started.elapsed() < shows + STOP_DEADLINE, "never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(started.elapsed() >= shows);
        face.stop();
        let frames = frames.0.lock().unwrap();
        assert_eq!(frames[0], EncodedFrame::black());
        assert_eq!(*frames.last().unwrap(), EncodedFrame::black());
        assert!(
            frames[1..frames.len() - 1]
                .iter()
                .any(|frame| *frame != EncodedFrame::black())
        );
    }

    /// #711: one face at a time; none for a tap that lets the lamp hear
    /// again, since its ring worker is about to start; and stopping it is
    /// black at once.
    #[test]
    fn a_tap_face_comes_one_at_a_time_and_never_for_a_tap_that_wakes_it() {
        let frames = Frames::default();
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (sink, counted) = (frames.clone(), starts.clone());
        let mut face = TapFace::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let sink = sink.clone();
            Some(Asleep::tapped_for(
                move || AsleepRing::new(sink),
                ceiling(48),
                Duration::from_millis(100),
            ))
        });
        assert!(!face.poll(false, false), "no tap, no face");
        assert!(!face.poll(true, true), "the tap that wakes it");
        assert!(face.poll(true, false));
        assert!(!face.poll(true, false), "one at a time");
        std::thread::sleep(Duration::from_millis(300));
        assert!(face.poll(true, false), "the next tap, once it ended");
        face.stop();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            *frames.0.lock().unwrap().last().unwrap(),
            EncodedFrame::black()
        );
    }

    #[test]
    fn the_switch_is_off_unless_set_to_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    /// #1545: the first pulse when it became owed (its due time), then one
    /// on each minute from then, each 4 s around, none starting half an
    /// hour on; one missed is not made up, the next comes on the minute.
    #[test]
    fn an_owed_reminder_pulses_from_when_it_was_owed_each_minute_for_half_an_hour() {
        let pulses = Pulses::OWED;
        let since = Instant::now();
        let at = |seconds: u64| since + Duration::from_secs(seconds);
        let around = |seconds: u64| pulses.around(since, at(seconds));
        assert_eq!(around(0), Some(0.0), "at once");
        assert_eq!(around(2), Some(0.5));
        assert_eq!(around(4), None, "4 s around");
        assert_eq!(around(59), None);
        assert_eq!(around(60), Some(0.0), "a minute on");
        assert_eq!(around(61), Some(0.25));
        // Looked at again only at 90 s, after a hold: none until 120 s.
        assert_eq!(around(90), None, "not made up");
        assert_eq!(around(120), Some(0.0));
        // The last starts at 29 minutes and goes all the way around.
        assert_eq!(around(29 * 60), Some(0.0));
        assert_eq!(around(29 * 60 + 3), Some(0.75));
        assert_eq!(around(30 * 60), None, "half an hour owed");
        assert_eq!(around(30 * 60 + 1), None);
        assert_eq!(around(45 * 60), None);
        // A look from just before `since` is the first pulse's start.
        assert_eq!(pulses.around(at(1), since), Some(0.0));
    }

    /// Whether a frame lights a pixel the asleep face never does: a pulse.
    fn pulsing(frame: &EncodedFrame) -> bool {
        (0..lamp_ring::PIXEL_COUNT)
            .filter(|index| index.abs_diff(lamp_ring::asleep::BOTTOM) > 2)
            .any(|index| {
                let offset = lamp_ring::PRIMER_BYTES + index * 24;
                frame.bytes()[offset..offset + 24].contains(&0xFC)
            })
    }

    /// The painter: no pulse while nothing is owed, and one going around
    /// once the wait says a reminder is owed.
    #[test]
    fn the_asleep_face_pulses_only_while_a_reminder_is_owed() {
        let frames = Frames::default();
        let sink = frames.clone();
        let pulses = Pulses {
            takes: Duration::from_secs(2),
            every: Duration::from_secs(60),
            for_at_most: Duration::from_secs(60),
        };
        let asleep = Asleep::start_pulsing_with(move || AsleepRing::new(sink), ceiling(48), pulses);
        std::thread::sleep(AFTER + Duration::from_millis(300));
        let before = frames.0.lock().unwrap().len();
        assert!(before > 2, "{before}");
        asleep.owed().tell(Some(Instant::now()));
        std::thread::sleep(Duration::from_millis(400));
        let owed = frames.0.lock().unwrap().len();
        asleep.owed().tell(None);
        std::thread::sleep(Duration::from_millis(400));
        asleep.stop();
        let frames = frames.0.lock().unwrap();
        assert!(
            !frames[..before].iter().any(pulsing),
            "nothing owed, no pulse"
        );
        assert!(frames[before..owed].iter().any(pulsing), "owed: a pulse");
        // A frame or two may be on its way as it is told.
        assert!(
            !frames[owed + 2..].iter().any(pulsing),
            "no longer owed or held: none"
        );
        assert_eq!(*frames.last().unwrap(), EncodedFrame::black());
    }
}
