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
    /// With the person's light ([`Self::light_with`]): set before stopping,
    /// the painter leaves its last frame on the ring ([`Self::release`]).
    leave: Option<Arc<AtomicBool>>,
    /// A reminder owed, as the muted wait tells it (#1545).
    owed: Arc<Owed>,
    thread: Option<JoinHandle<()>>,
    done: Option<std::sync::mpsc::Receiver<()>>,
    /// How long stopping waits for the painter: [`STOP_DEADLINE`].
    stop_deadline: Duration,
}

/// How long the face waits before it opens the ring, and how long stopping
/// waits for the painter: [`AFTER`] and [`STOP_DEADLINE`] on a lamp, shorter
/// in a test, so that it needn't wait them out (#1806).
#[derive(Clone, Copy)]
struct Timing {
    after: Duration,
    stop_deadline: Duration,
}

impl Timing {
    const LAMP: Self = Self {
        after: AFTER,
        stop_deadline: STOP_DEADLINE,
    };
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
        Self::start_timed(open, ceiling, pulses, Timing::LAMP)
    }

    fn start_timed<S, F>(open: F, ceiling: ChannelCeiling, pulses: Pulses, timing: Timing) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        Self::spawn(timing.stop_deadline, move |stop, owed| {
            paint(open, ceiling, (stop, owed), pulses, timing.after)
        })
    }

    /// What a hand's sleep shows on lamp-4ace's ring device under
    /// `ceiling` ([`Self::sleeping_with`]).
    #[cfg(target_os = "linux")]
    pub fn sleeping(ceiling: u16, light: Option<crate::ring_face::Lit>) -> Option<Self> {
        let ceiling = ChannelCeiling::new(ceiling).ok()?;
        Some(Self::sleeping_with(
            lamp_ring::linux::open_asleep,
            ceiling,
            light,
        ))
    }

    /// What a hand's sleep shows, on whatever `open` gives: the person's
    /// own light, drawn at once, while their ring said one was on, and the
    /// asleep face otherwise, after [`AFTER`] as always (#1362).
    pub fn sleeping_with<S, F>(
        open: F,
        ceiling: ChannelCeiling,
        light: Option<crate::ring_face::Lit>,
    ) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        match light {
            Some(lit) => Self::light_with(open, ceiling, lit),
            None => Self::start_with(open, ceiling),
        }
    }

    /// [`Self::start_light`], on whatever `open` gives.
    pub fn light_with<S, F>(open: F, ceiling: ChannelCeiling, lit: crate::ring_face::Lit) -> Self
    where
        S: FrameSink + 'static,
        F: FnOnce() -> io::Result<AsleepRing<S>> + Send + 'static,
    {
        let leave = Arc::new(AtomicBool::new(false));
        let leaving = leave.clone();
        let mut asleep = Self::spawn(STOP_DEADLINE, move |stop, _| {
            paint_light(open, ceiling, lit, stop, &leaving)
        });
        asleep.leave = Some(leave);
        asleep
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
        Self::spawn(STOP_DEADLINE, move |stop, _| {
            paint_tapped(open, ceiling, shows, stop)
        })
    }

    fn spawn(
        stop_deadline: Duration,
        paint: impl FnOnce(&AtomicBool, &Owed) -> io::Result<()> + Send + 'static,
    ) -> Self {
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
            leave: None,
            owed,
            thread,
            done: Some(done),
            stop_deadline,
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

    /// Stop painting as the lamp wakes: the person's light is left on the
    /// ring, and the device closed, so it doesn't blink off before the
    /// conversation's ring draws it again (the reviewer-lead's P2 on #2549).
    /// Any other picture goes black, as [`Self::stop`].
    pub fn release(mut self) {
        if let Some(leave) = &self.leave {
            leave.store(true, Ordering::SeqCst);
        }
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
            .is_some_and(|done| done.recv_timeout(self.stop_deadline).is_ok());
        if finished || thread.is_finished() {
            let _ = thread.join();
        } else {
            eprintln!(
                "lamp-voice ring: the asleep face did not stop within {} ms (a ring write is stuck); going on without it",
                self.stop_deadline.as_millis()
            );
        }
    }
}

impl Drop for Asleep {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Wait `after` ([`AFTER`] on a lamp); unless stopped by then, open the ring and paint the
/// breathing face until stopped, then black. While a reminder `owed` may
/// show, one soft arc goes around it as `pulses` say (#1545).
fn paint<S: FrameSink>(
    open: impl FnOnce() -> io::Result<AsleepRing<S>>,
    ceiling: ChannelCeiling,
    (stop, owed): (&AtomicBool, &Owed),
    pulses: Pulses,
    after: Duration,
) -> io::Result<()> {
    if !wait(after, stop) {
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

/// A hand's sleep with the person's own light on (#1362, the first builder
/// on #1362, 21 Oct 15:1x): the light stays theirs. The conversation's ring
/// worker writes black as the mute stops it; this draws their light back at
/// once, as their ring showed it at rest ([`crate::ring_face::Face`]), with
/// one slow dip of a step as it settles: up from the black to a level lower
/// by [`LIGHT_DIP`]'s first, held to its second, back up by its third. Then
/// it is their light, and nothing more.
pub const LIGHT_DIP: [Duration; 3] = [
    Duration::from_millis(600),
    Duration::from_millis(2_000),
    Duration::from_millis(4_000),
];

/// How much of the person's light shows `since` its painter began, with
/// `lower` the share one level down ([`crate::ring_face::Lit::step_down_share`]).
pub fn dip_share(since: Duration, lower: f32) -> f32 {
    let ease = |from: Duration, to: Duration| {
        let share = (since.saturating_sub(from).as_secs_f32()
            / to.saturating_sub(from).as_secs_f32())
        .clamp(0.0, 1.0);
        0.5 - 0.5 * (std::f32::consts::PI * share).cos()
    };
    let [rise, hold, back] = LIGHT_DIP;
    if since < rise {
        lower * ease(Duration::ZERO, rise)
    } else if since < hold {
        lower
    } else {
        lower + (1.0 - lower) * ease(hold, back)
    }
}

/// The person's light `since` its painter began, at `now_us`: their ring's
/// resting frame under `ceiling`, by [`dip_share`].
pub fn light_pixels(
    face: &mut crate::ring_face::Face,
    lit: crate::ring_face::Lit,
    since: Duration,
    now_us: u64,
    ceiling: ChannelCeiling,
) -> [lamp_ring::Rgb; lamp_ring::PIXEL_COUNT] {
    let share = dip_share(since, lit.step_down_share());
    face.render(crate::ring_face::Mood::Resting, 0, now_us, ceiling.get())
        .map(|pixel| {
            let scale = |channel: u8| (f32::from(channel) * share).round() as u8;
            lamp_ring::Rgb::new(scale(pixel.red), scale(pixel.green), scale(pixel.blue))
        })
}

/// The person's own light while a hand's sleep keeps the lamp muted, if
/// their ring said one was on (`light/state`, kept on the bus by the ring's
/// light, `crate::ring_light::SOURCE`): the last light the ring showed,
/// however old, since the ring stopped saying it at the mute. Off, never
/// said, said by another part, or not a light the ring can draw: none, and
/// the sleep shows the asleep face.
pub fn persons_light(bus: &lamp_bus::Bus) -> Option<crate::ring_face::Lit> {
    let message = bus.latest(lamp_msgs::Light::TOPICS[1]).ok().flatten()?;
    if message.header.source != crate::ring_light::SOURCE {
        return None;
    }
    let light = message.parse::<lamp_msgs::Light>().ok()?;
    light
        .on
        .then(|| crate::ring_face::Lit::of(true, &light.colour, light.level))
        .flatten()
}

/// The person's light drawn at once ([`light_pixels`]) until stopped, then
/// black, or left on the ring when told to `leave` (a wake).
fn paint_light<S: FrameSink>(
    open: impl FnOnce() -> io::Result<AsleepRing<S>>,
    ceiling: ChannelCeiling,
    lit: crate::ring_face::Lit,
    stop: &AtomicBool,
    leave: &AtomicBool,
) -> io::Result<()> {
    let mut ring = open()?;
    let mut face = crate::ring_face::Face::new();
    face.light(lit);
    let began = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let pixels = light_pixels(
            &mut face,
            lit,
            began.elapsed(),
            lamp_ipc::monotonic_us(),
            ceiling,
        );
        if let Err(error) = ring.show_light(&pixels, ceiling) {
            let _ = ring.shutdown();
            return Err(error);
        }
        wait(FRAME, stop);
    }
    if leave.load(Ordering::SeqCst) {
        ring.leave();
        return Ok(());
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

/// The asleep face while a hand held on the head keeps the lamp asleep
/// (#1362), where the face is not already shown for the whole wait: begun
/// at the first look of the muted wait that finds the head's sleep, after
/// [`AFTER`] and fading in as the face always does, and stopped when the
/// sleep ends. With the Controls privacy route's slide (`yields`), the
/// slide closing stops it at once, black and the device closed, before the
/// wait starts the slide's own indication, so the ring keeps one writer
/// (#795); that indication keeps the device until the wait ends, so the
/// face does not come back in the same wait.
pub struct SleepFace<F: FnMut() -> Option<Asleep>> {
    start: F,
    showing: Option<Asleep>,
    yields: bool,
    gave_way: bool,
}

impl<F: FnMut() -> Option<Asleep>> SleepFace<F> {
    pub fn new(start: F, yields: bool) -> Self {
        Self {
            start,
            showing: None,
            yields,
            gave_way: false,
        }
    }

    /// One look of the muted wait: whether the head's sleep keeps it
    /// muted, and whether the lamp may hear (`opens`). True when a face
    /// began. The sleep ended by a wake that opens it leaves the person's
    /// light on for the conversation's ring ([`Asleep::release`]).
    pub fn poll(&mut self, asleep: bool, opens: bool) -> bool {
        if !asleep {
            if let Some(showing) = self.showing.take() {
                if opens {
                    showing.release();
                } else {
                    showing.stop();
                }
            }
            return false;
        }
        if self.showing.is_none() && !self.gave_way {
            self.showing = (self.start)();
            return self.showing.is_some();
        }
        false
    }

    /// What the wait read of the slide, as it changes: closed, with
    /// `yields`, the face stops now and not again in this wait.
    pub fn slide(&mut self, state: crate::privacy::PrivacyState) {
        if self.yields && state == crate::privacy::PrivacyState::Muted {
            self.gave_way = true;
            if let Some(showing) = self.showing.take() {
                showing.stop();
            }
        }
    }

    /// Whether the face is begun and not stopped.
    pub fn showing(&self) -> bool {
        self.showing.is_some()
    }

    /// Black and the device closed, before the next cycle's ring worker.
    pub fn stop(self) {
        if let Some(showing) = self.showing {
            showing.stop();
        }
    }

    /// As the wait ends on a wake: the person's light left on, any other
    /// picture black, and the device closed ([`Asleep::release`]).
    pub fn release(self) {
        if let Some(showing) = self.showing {
            showing.release();
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

    /// The face's wait and stop in a test: short, so they aren't waited out.
    const QUICK: Timing = Timing {
        after: Duration::from_millis(150),
        stop_deadline: Duration::from_millis(250),
    };

    #[test]
    fn a_long_wait_paints_and_ends_black_with_the_painter_gone() {
        let frames = Frames::default();
        let sink = frames.clone();
        let asleep = Asleep::start_timed(
            move || AsleepRing::new(sink),
            ceiling(48),
            Pulses::OWED,
            QUICK,
        );
        std::thread::sleep(QUICK.after + Duration::from_millis(400));
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
        let asleep = Asleep::start_timed(
            move || AsleepRing::new(Stuck(flag)),
            ceiling(48),
            Pulses::OWED,
            QUICK,
        );
        while !stuck.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let began = Instant::now();
        asleep.stop();
        let took = began.elapsed();
        assert!(
            took >= QUICK.stop_deadline && took < QUICK.stop_deadline * 2,
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
        let asleep = Asleep::start_timed(move || AsleepRing::new(sink), ceiling(48), pulses, QUICK);
        std::thread::sleep(QUICK.after + Duration::from_millis(300));
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

    /// A ring that says whether it is open: set when the face opens it,
    /// cleared when the face closes it.
    struct Open(Arc<AtomicBool>, Frames);
    impl FrameSink for Open {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            self.1.write_frame(frame)
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }

    /// #1362: a hand's sleep's face begins once while the sleep lasts and
    /// stops when it ends. With the privacy slide's route, the slide closing
    /// leaves the ring black and closed by the time it is told, before the
    /// slide's indication may open it, and the face does not come back in
    /// that wait. Off the route, a closed slide leaves it as it is.
    #[test]
    fn a_hands_sleep_shows_the_face_and_gives_the_ring_to_a_closing_slide() {
        use crate::privacy::PrivacyState;
        let open = Arc::new(AtomicBool::new(false));
        let frames = Frames::default();
        let starts = std::cell::Cell::new(0);
        let start = || {
            starts.set(starts.get() + 1);
            let (flag, sink) = (open.clone(), frames.clone());
            Some(Asleep::start_timed(
                move || {
                    flag.store(true, Ordering::SeqCst);
                    AsleepRing::new(Open(flag, sink))
                },
                ceiling(48),
                Pulses::OWED,
                QUICK,
            ))
        };
        let mut face = SleepFace::new(start, true);
        assert!(!face.poll(false, false), "awake: nothing");
        assert!(face.poll(true, false), "asleep: begun");
        assert!(!face.poll(true, false), "once");
        let until = Instant::now() + Duration::from_secs(2);
        while !open.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(open.load(Ordering::SeqCst), "the face opened the ring");
        face.slide(PrivacyState::Allowed);
        assert!(face.showing(), "an open slide changes nothing");
        face.slide(PrivacyState::Muted);
        assert!(!open.load(Ordering::SeqCst), "closed when told");
        assert_eq!(
            *frames.0.lock().unwrap().last().unwrap(),
            EncodedFrame::black()
        );
        assert!(!face.showing());
        assert!(!face.poll(true, false), "not again in this wait");
        face.slide(PrivacyState::Allowed);
        assert!(!face.poll(true, false), "nor once the slide opens");
        assert_eq!(starts.get(), 1);
        face.stop();

        let starts = std::cell::Cell::new(0);
        let mut off_route = SleepFace::new(
            || {
                starts.set(starts.get() + 1);
                Some(Asleep::start_timed(
                    || AsleepRing::new(Frames::default()),
                    ceiling(48),
                    Pulses::OWED,
                    QUICK,
                ))
            },
            false,
        );
        assert!(off_route.poll(true, false));
        off_route.slide(PrivacyState::Muted);
        assert!(
            off_route.showing(),
            "off the route the face is the muted look"
        );
        assert!(!off_route.poll(false, false));
        assert!(!off_route.showing(), "awake again: stopped");
        assert!(off_route.poll(true, false), "asleep again: begun again");
        assert_eq!(starts.get(), 2);
        off_route.stop();
    }

    /// #1362: with the person's light on, the light comes back at once from
    /// the mute's black, a level lower, held, and back up to their light by
    /// four seconds; then it is their light and nothing more.
    #[test]
    fn the_persons_light_comes_back_with_one_slow_dip() {
        let lower = 0.65;
        let at = |ms| dip_share(Duration::from_millis(ms), lower);
        assert_eq!(at(0), 0.0);
        assert!(at(300) > 0.0 && at(300) < lower);
        assert_eq!(at(600), lower);
        assert_eq!(at(1_999), lower);
        assert!(at(3_000) > lower && at(3_000) < 1.0);
        assert_eq!(at(4_000), 1.0);
        assert_eq!(at(60_000), 1.0);
        // Never more than their light, never dark once it has come back.
        let mut since = 0;
        while since <= 5_000 {
            let share = at(since);
            assert!((0.0..=1.0).contains(&share), "{since}");
            if since >= 600 {
                assert!(share >= lower, "{since}");
            }
            since += 50;
        }
    }

    /// #1362: what the sleep draws once the dip is over is the person's
    /// light at its colour and level, exactly as their ring drew it at rest;
    /// in the dip, a level lower.
    #[test]
    fn the_persons_light_is_drawn_at_its_colour_and_level() {
        use crate::ring_face::{Face, Lit, Mood};
        let lit = Lit::of(true, "blue", 4).unwrap();
        let ceiling = ceiling(48);
        let (mut drawn, mut theirs) = (Face::new(), Face::new());
        drawn.light(lit);
        theirs.light(lit);
        let mut now = 1_000_000;
        let mut last = (
            [lamp_ring::Rgb::BLACK; lamp_ring::PIXEL_COUNT],
            [lamp_ring::Rgb::BLACK; lamp_ring::PIXEL_COUNT],
        );
        for step in 0..120 {
            now += 50_000;
            let since = Duration::from_millis(50 * (step + 1));
            let pixels = light_pixels(&mut drawn, lit, since, now, ceiling);
            let rest = theirs.render(Mood::Resting, 0, now, ceiling.get());
            if since == Duration::from_millis(1_500) {
                let share = lit.step_down_share();
                for (pixel, full) in pixels.iter().zip(rest.iter()) {
                    assert_eq!(pixel.blue, (f32::from(full.blue) * share).round() as u8);
                }
            }
            last = (pixels, rest);
        }
        // Six seconds in: their light, blue, as their ring draws it.
        assert_eq!(last.0, last.1);
        assert!(last.0.iter().any(|pixel| pixel.blue > 0));
        assert!(
            last.0
                .iter()
                .all(|pixel| pixel.blue >= pixel.red && pixel.blue >= pixel.green)
        );
    }

    /// #1362: the person's light is read from what the ring's light said,
    /// however old: on, it is the light to draw; off, never said, said by
    /// another part or not a light there is, none (the asleep face).
    #[test]
    fn the_persons_light_is_what_the_rings_light_said_on() {
        let root = std::env::temp_dir().join(format!("lamp-voice-light-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        assert_eq!(persons_light(&bus), None, "never said");
        let said = |on, colour: &str, level| lamp_msgs::Light {
            on,
            colour: colour.into(),
            level,
        };
        let mut ring = bus
            .publisher_declared(
                lamp_msgs::Light::TOPICS[1],
                &lamp_msgs::Light::declaration(),
                crate::ring_light::SOURCE,
            )
            .unwrap();
        ring.publish_json(1, &said(true, "white", 4)).unwrap();
        assert_eq!(
            persons_light(&bus),
            crate::ring_face::Lit::of(true, "white", 4)
        );
        ring.publish_json(2, &said(false, "white", 4)).unwrap();
        assert_eq!(persons_light(&bus), None, "off");
        drop(ring);
        let other = lamp_bus::Bus::new(&root).unwrap();
        let mut actions = other
            .publisher_declared(
                lamp_msgs::Light::TOPICS[1],
                &lamp_msgs::Light::declaration(),
                "actions",
            )
            .unwrap();
        actions.publish_json(1, &said(true, "red", 5)).unwrap();
        assert_eq!(persons_light(&bus), None, "another part's");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #1362: with the person's light, the sleep draws it at once, from the
    /// writer's black; with none, the asleep face waits its 1.5 s as always,
    /// so nothing is drawn yet. Both end black.
    #[test]
    fn a_hands_sleep_draws_the_persons_light_or_the_asleep_face() {
        let frames = Frames::default();
        let sink = frames.clone();
        let lit = crate::ring_face::Lit::of(true, "green", 3).unwrap();
        let light = Asleep::sleeping_with(move || AsleepRing::new(sink), ceiling(48), Some(lit));
        std::thread::sleep(Duration::from_millis(300));
        light.stop();
        let drawn = frames.0.lock().unwrap().clone();
        assert!(drawn.len() >= 3, "{}", drawn.len());
        assert_eq!(drawn[0], EncodedFrame::black());
        assert_eq!(*drawn.last().unwrap(), EncodedFrame::black());
        assert!(
            drawn[1..drawn.len() - 1]
                .iter()
                .any(|frame| *frame != EncodedFrame::black())
        );
        let frames = Frames::default();
        let sink = frames.clone();
        let face = Asleep::sleeping_with(move || AsleepRing::new(sink), ceiling(48), None);
        std::thread::sleep(Duration::from_millis(300));
        face.stop();
        assert!(frames.0.lock().unwrap().is_empty(), "the face waits AFTER");
    }

    /// The reviewer-lead's P2 on #2549: as the lamp wakes, the person's
    /// light is left on the ring for the conversation's ring to draw again,
    /// not blacked for its start; stopped any other way, it ends black.
    #[test]
    fn a_wake_leaves_the_persons_light_on_and_anything_else_ends_black() {
        let lit = crate::ring_face::Lit::of(true, "warm", 3).unwrap();
        let painted = |release: bool| {
            let frames = Frames::default();
            let sink = frames.clone();
            let light = Asleep::light_with(move || AsleepRing::new(sink), ceiling(48), lit);
            std::thread::sleep(Duration::from_millis(300));
            if release {
                light.release();
            } else {
                light.stop();
            }
            frames.0.lock().unwrap().clone()
        };
        let released = painted(true);
        assert!(released.len() >= 2, "{}", released.len());
        assert_ne!(*released.last().unwrap(), EncodedFrame::black(), "left on");
        let stopped = painted(false);
        assert_eq!(*stopped.last().unwrap(), EncodedFrame::black());
        // Through the sleep's face: a wake that opens it leaves the light on;
        // a sleep that ends without opening it ends black.
        for (opens, black) in [(true, false), (false, true)] {
            let frames = Frames::default();
            let sink = Arc::new(Mutex::new(Some(frames.clone())));
            let mut face = SleepFace::new(
                || {
                    let sink = sink.lock().unwrap().take()?;
                    Some(Asleep::light_with(
                        move || AsleepRing::new(sink),
                        ceiling(48),
                        lit,
                    ))
                },
                true,
            );
            assert!(face.poll(true, false));
            std::thread::sleep(Duration::from_millis(300));
            assert!(!face.poll(false, opens));
            let last = frames.0.lock().unwrap().last().cloned().unwrap();
            assert_eq!(last == EncodedFrame::black(), black, "opens {opens}");
        }
        // The asleep face itself goes black on a wake too.
        let frames = Frames::default();
        let sink = frames.clone();
        let face = Asleep::start_timed(
            move || AsleepRing::new(sink),
            ceiling(48),
            Pulses::OWED,
            QUICK,
        );
        std::thread::sleep(QUICK.after + Duration::from_millis(200));
        face.release();
        assert_eq!(
            *frames.0.lock().unwrap().last().unwrap(),
            EncodedFrame::black()
        );
    }
}
