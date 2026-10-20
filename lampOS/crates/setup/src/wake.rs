//! The waking face (#1681, `docs/ring-choreography.md`): from power-on until
//! the voice takes the ring nothing of the lamp shows anything, often for
//! a quarter of a minute, and a dark lamp reads as a broken one. This
//! lights the ring as a creature waking: the asleep face's warm glow at the
//! bottom, opening slowly up the ring like an eye over [`OPENING`],
//! breathing every [`BREATH`]. When the voice asks for the ring on
//! [`lamp_ring::waking::RELEASE_SOCKET`] it fades out, sends black, closes
//! the device and only then answers, so the voice's lock never meets it.
//! If no voice has asked within [`UNWELL_AFTER`], the face turns pale and
//! slow ([`lamp_ring::waking::UNWELL`]) and stays so, still waiting to be
//! asked: a voice that never started.
//!
//! Off unless `LAMP_WAKING_FACE=on` ([`SWITCH`]): then `lamp-setup wake`
//! makes no socket and exits, and the voice opens the ring as before.

use lamp_ring::{
    FrameSink,
    waking::{ANSWER, ASK_MOST, AT_ONCE_WAIT, Ask, WakingRing},
};
use std::{
    io::{self, Read, Write},
    os::unix::net::UnixListener,
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// The switch, in `lamp-waking.service`'s environment: `on` or off.
pub const SWITCH: &str = "LAMP_WAKING_FACE";
/// One frame this often.
pub const FRAME: Duration = Duration::from_millis(50);
/// Its first frame is already this much of the glow, so it is lit from the
/// start; it fades the rest of the way in over [`FADE_IN`].
pub const FIRST_LEVEL: f32 = 0.4;
pub const FADE_IN: Duration = Duration::from_millis(1_500);
/// And out over this, when the voice asks for the ring.
pub const FADE_OUT: Duration = Duration::from_millis(600);

/// How long the face takes to fade in and to fade out: [`FADE_IN`] and
/// [`FADE_OUT`] on the lamp. A test passes short ones, to see the same
/// fades in a fraction of the time (#1806).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fades {
    pub fade_in: Duration,
    pub fade_out: Duration,
}

impl Fades {
    pub const LAMP: Self = Self {
        fade_in: FADE_IN,
        fade_out: FADE_OUT,
    };
}
/// From the asleep face's glow to the whole ring.
pub const OPENING: Duration = Duration::from_secs(20);
/// One breath while it wakes, as the asleep face breathes.
pub const BREATH: Duration = Duration::from_secs(6);
/// No voice by then: the lamp is unwell.
pub const UNWELL_AFTER: Duration = Duration::from_secs(5 * 60);
/// It turns pale over this, and breathes slower, once every [`UNWELL_BREATH`].
pub const UNWELL_CHANGE: Duration = Duration::from_secs(3);
pub const UNWELL_BREATH: Duration = Duration::from_secs(10);
/// A ring that is not there yet, or failed, is tried again this often:
/// early in boot the device may not have appeared.
pub const RETRY: Duration = Duration::from_millis(200);
/// The longest an ask waits for the ring to close before it is answered
/// all the same: under the voice's own wait of 2 s, which is only for a
/// waking face that doesn't answer.
pub const CLOSE_WAIT: Duration = Duration::from_millis(1_500);
/// Asked at once, the face sends its black only this soon after reading
/// the ask; later, it closes the ring with no frame. Past
/// [`AT_ONCE_WAIT`] the physical-mute indication may already have taken the
/// ring without the lock (`lamp_ring::waking::take`), and a black from a
/// slow face would land on its light (#1681 review). The face's clock
/// starts when it reads the ask, after the indication's, so it stops at
/// half of it.
pub const AT_ONCE_BLACK: Duration = Duration::from_micros(AT_ONCE_WAIT.as_micros() as u64 / 2);

/// Whether `value`, the switch's, turns the waking face on.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What to put on the ring now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// [`WakingRing::show`] with these.
    Show {
        open: f32,
        unwell: f32,
        breath: f32,
        level: f32,
    },
    /// Faded out: black, and the ring closed.
    Dark,
}

/// The face's motion, with no clock or device of its own: given the time
/// since the part started and whether the ring is being let go, what it
/// shows. Once dark after a let-go it stays dark.
#[derive(Debug)]
pub struct Painter {
    fades: Fades,
    fade: f32,
    /// Breaths so far: the fraction is how far through this one.
    breaths: f32,
    last: Duration,
    dark: bool,
}

impl Default for Painter {
    fn default() -> Self {
        Self {
            fades: Fades::LAMP,
            fade: FIRST_LEVEL,
            breaths: 0.0,
            last: Duration::ZERO,
            dark: false,
        }
    }
}

/// `part` of the way from `start` to `start + long`, 0 to 1.
fn share(now: Duration, start: Duration, long: Duration) -> f32 {
    (now.saturating_sub(start).as_secs_f32() / long.as_secs_f32()).clamp(0.0, 1.0)
}

impl Painter {
    /// A painter that fades in and out over `fades`.
    pub fn faded(fades: Fades) -> Self {
        Self {
            fades,
            ..Self::default()
        }
    }

    pub fn step(&mut self, now: Duration, let_go: bool) -> Paint {
        if self.dark {
            return Paint::Dark;
        }
        let elapsed = now.saturating_sub(self.last).as_secs_f32();
        self.last = now;
        let unwell = share(now, UNWELL_AFTER, UNWELL_CHANGE);
        let breath =
            BREATH.as_secs_f32() + (UNWELL_BREATH.as_secs_f32() - BREATH.as_secs_f32()) * unwell;
        self.breaths = (self.breaths + elapsed / breath).fract();
        if let_go {
            self.fade = (self.fade - elapsed / self.fades.fade_out.as_secs_f32()).max(0.0);
            if self.fade == 0.0 {
                self.dark = true;
                return Paint::Dark;
            }
        } else {
            self.fade = (self.fade + elapsed / self.fades.fade_in.as_secs_f32()).min(1.0);
        }
        // An eye opens fast at first and slowly at the end.
        let open = share(now, Duration::ZERO, OPENING);
        Paint::Show {
            open: 1.0 - (1.0 - open) * (1.0 - open),
            unwell,
            breath: 0.5 - 0.5 * (std::f32::consts::TAU * self.breaths).cos(),
            level: self.fade,
        }
    }
}

#[derive(Default)]
struct State {
    /// How the voice has asked for the ring, if it has, and when.
    asked: Option<Ask>,
    asked_at: Option<Instant>,
    /// The ring is closed and will not be opened again.
    let_go: bool,
}

/// How the waking face ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// The voice asked, and was answered once the ring was closed.
    Released,
    /// The socket failed: the ring was let go so the voice finds it free.
    SocketFailed,
}

/// Show the waking face until the voice asks for the ring on `listener`.
/// `open` opens the ring (on a lamp, `lamp_ring::linux::open_waking`); one
/// that cannot be opened or written is tried again after `retry`, and said
/// once each time it starts to fail. An ask wakes the painter at once, not
/// at its next frame: one to let go at once is answered within a frame's
/// write.
pub fn run<S, O>(
    listener: UnixListener,
    open: O,
    log: impl Fn(&str) + Send + Sync + 'static,
    retry: Duration,
) -> Ended
where
    S: FrameSink,
    O: FnMut() -> io::Result<WakingRing<S>>,
{
    run_faded(listener, open, log, retry, Fades::LAMP)
}

/// [`run`], fading in and out over `fades` rather than the lamp's.
pub fn run_faded<S, O>(
    listener: UnixListener,
    mut open: O,
    log: impl Fn(&str) + Send + Sync + 'static,
    retry: Duration,
    fades: Fades,
) -> Ended
where
    S: FrameSink,
    O: FnMut() -> io::Result<WakingRing<S>>,
{
    let log: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(log);
    let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
    let answering = thread::spawn({
        let state = Arc::clone(&state);
        let log = Arc::clone(&log);
        move || answer(&listener, &state, |line| log(line))
    });
    let (held, changed) = &*state;
    let start = Instant::now();
    let mut painter = Painter::faded(fades);
    let mut ring: Option<WakingRing<S>> = None;
    let mut retry_at: Option<Instant> = None;
    let mut failing = false;
    // Asked at once too long ago: nothing more may be written.
    let late = || {
        let state = held.lock().unwrap_or_else(PoisonError::into_inner);
        state.asked == Some(Ask::AtOnce)
            && state
                .asked_at
                .is_some_and(|at| at.elapsed() >= AT_ONCE_BLACK)
    };
    loop {
        let said = held.lock().unwrap_or_else(PoisonError::into_inner).asked;
        // A socket that failed lets the ring go too: nobody could ask.
        let asked = said.or(answering.is_finished().then_some(Ask::Fade));
        let paint = match asked {
            Some(Ask::AtOnce) => Paint::Dark,
            _ => painter.step(start.elapsed(), asked.is_some()),
        };
        match paint {
            Paint::Show {
                open: wide,
                unwell,
                breath,
                level,
            } if ring.is_some() || asked.is_none() => {
                // An ask since this frame was chosen: the loop takes it,
                // before anything is opened or written.
                if held.lock().unwrap_or_else(PoisonError::into_inner).asked != said {
                    continue;
                }
                if ring.is_none() && retry_at.is_none_or(|at| Instant::now() >= at) {
                    match open() {
                        Ok(opened) => {
                            ring = Some(opened);
                            retry_at = None;
                            if failing {
                                log("the waking face has the ring again");
                                failing = false;
                            }
                        }
                        Err(error) => {
                            if !failing {
                                log(&format!("the waking face cannot open the ring: {error}"));
                                failing = true;
                            }
                            retry_at = Some(Instant::now() + retry);
                        }
                    }
                }
                if let Some(open_ring) = ring.as_mut()
                    && let Err(error) = open_ring.show(wide, unwell, breath, level)
                {
                    if !failing {
                        log(&format!("the waking face cannot write the ring: {error}"));
                        failing = true;
                    }
                    ring = None;
                    retry_at = Some(Instant::now() + retry);
                }
            }
            // Dark, asked at once, or asked with no ring open: nothing to fade.
            _ => {
                if late() {
                    // Closed now, with no black: the ring may be the mute
                    // light's.
                    let _ = ring.take();
                } else if let Some(open_ring) = ring.take()
                    && let Err(error) = open_ring.shutdown()
                {
                    log(&format!(
                        "the waking face's last black frame failed: {error}"
                    ));
                }
                held.lock().unwrap_or_else(PoisonError::into_inner).let_go = true;
                changed.notify_all();
                break;
            }
        }
        // The frame's wait, cut short by an ask.
        let waiting = held.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = changed
            .wait_timeout_while(waiting, FRAME, |state| state.asked == said)
            .unwrap_or_else(PoisonError::into_inner);
    }
    match answering.join() {
        Ok(true) => Ended::Released,
        _ => Ended::SocketFailed,
    }
}

/// Read one ask, a line of at most [`ASK_MOST`] bytes.
fn read_ask(stream: &mut impl Read) -> Option<Ask> {
    let mut line = Vec::new();
    let mut byte = [0; 1];
    while line.len() < ASK_MOST {
        match stream.read(&mut byte) {
            Ok(1) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    return Ask::parse(&line);
                }
            }
            _ => return None,
        }
    }
    None
}

/// Wait for the voice's ask, have the ring let go, and answer once it is
/// closed, or after [`CLOSE_WAIT`]. Whether the voice was answered.
fn answer(listener: &UnixListener, state: &(Mutex<State>, Condvar), log: impl Fn(&str)) -> bool {
    let (held, changed) = state;
    loop {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) => {
                log(&format!("the waking face's socket failed: {error}"));
                return false;
            }
        };
        let _ = stream.set_read_timeout(Some(CLOSE_WAIT));
        let Some(ask) = read_ask(&mut stream) else {
            // Not the voice's ask: the ring stays.
            continue;
        };
        let mut state = held.lock().unwrap_or_else(PoisonError::into_inner);
        state.asked = Some(ask);
        state.asked_at = Some(Instant::now());
        changed.notify_all();
        let closed = changed
            .wait_timeout_while(state, CLOSE_WAIT, |state| !state.let_go)
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .let_go;
        if !closed {
            log("the waking face did not close the ring in time");
        }
        if let Err(error) = stream.write_all(ANSWER) {
            log(&format!(
                "the waking face could not answer the voice: {error}"
            ));
        }
        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_ring::{PIXEL_COUNT, Rgb, waking::waking_pixels};

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn pixels(paint: Paint) -> [Rgb; PIXEL_COUNT] {
        match paint {
            Paint::Show {
                open,
                unwell,
                breath,
                level,
            } => waking_pixels(open, unwell, breath, level),
            Paint::Dark => [Rgb::BLACK; PIXEL_COUNT],
        }
    }

    #[test]
    fn only_on_turns_it_on() {
        assert!(switched_on(Some("on")));
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!switched_on(off), "{off:?}");
        }
    }

    #[test]
    fn its_first_frame_is_already_lit() {
        let mut painter = Painter::default();
        let first = pixels(painter.step(ms(0), false));
        assert!(first.iter().any(|pixel| *pixel != Rgb::BLACK), "{first:?}");
    }

    #[test]
    fn it_fades_in_and_opens_up_the_ring_over_the_opening() {
        let mut painter = Painter::default();
        let mut levels = Vec::new();
        let mut opens = Vec::new();
        for at in (0..=25_000).step_by(50) {
            let Paint::Show { open, level, .. } = painter.step(ms(at), false) else {
                panic!("dark at {at}");
            };
            levels.push(level);
            opens.push(open);
        }
        assert_eq!(levels[0], FIRST_LEVEL);
        assert!(levels.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(levels[40], 1.0, "in by 2 s");
        assert!(opens.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(opens[100] > 0.3 && opens[100] < 0.6, "{}", opens[100]);
        assert_eq!(opens[400], 1.0, "open at 20 s");
    }

    #[test]
    fn it_breathes_every_six_seconds_then_every_ten_once_unwell() {
        let tops = |from: u64, to: u64| {
            let mut painter = Painter::default();
            let mut tops = 0;
            let mut was = 0.0;
            let mut rising = true;
            for at in (0..=to).step_by(50) {
                let Paint::Show { breath, .. } = painter.step(ms(at), false) else {
                    panic!("dark");
                };
                if at > from && rising && breath < was {
                    tops += 1;
                }
                rising = breath >= was;
                was = breath;
            }
            tops
        };
        assert_eq!(tops(0, 60_000), 10);
        let unwell = UNWELL_AFTER.as_millis() as u64 + 3_000;
        assert_eq!(tops(unwell, unwell + 60_000), 6);
    }

    #[test]
    fn with_no_voice_for_five_minutes_it_turns_pale() {
        let mut painter = Painter::default();
        let mut at = 0;
        while at < UNWELL_AFTER.as_millis() as u64 {
            let Paint::Show { unwell, .. } = painter.step(ms(at), false) else {
                panic!("dark");
            };
            assert_eq!(unwell, 0.0, "at {at}");
            at += 1_000;
        }
        let Paint::Show { unwell, .. } = painter.step(UNWELL_AFTER + ms(1_500), false) else {
            panic!("dark");
        };
        assert!((unwell - 0.5).abs() < 0.01);
        let Paint::Show { unwell, .. } = painter.step(UNWELL_AFTER + UNWELL_CHANGE, false) else {
            panic!("dark");
        };
        assert_eq!(unwell, 1.0);
    }

    #[test]
    fn a_painter_fades_over_the_fades_it_is_given_and_the_lamps_by_default() {
        assert_eq!(
            Fades::LAMP,
            Fades {
                fade_in: FADE_IN,
                fade_out: FADE_OUT
            }
        );
        let mut painter = Painter::faded(Fades {
            fade_in: ms(100),
            fade_out: ms(100),
        });
        // From its first level to full in the rest of 100 ms.
        let Paint::Show { level, .. } = painter.step(ms(60), false) else {
            panic!("dark");
        };
        assert_eq!(level, 1.0);
        let Paint::Show { level, .. } = painter.step(ms(110), true) else {
            panic!("dark");
        };
        assert!((level - 0.5).abs() < 0.01, "{level}");
        assert!(matches!(painter.step(ms(160), true), Paint::Dark));
        // The lamp's own is still fading in then.
        let Paint::Show { level, .. } = Painter::default().step(ms(60), false) else {
            panic!("dark");
        };
        assert!(level < 1.0, "{level}");
    }

    #[test]
    fn let_go_it_fades_out_within_the_fade_and_stays_dark() {
        let mut painter = Painter::default();
        for at in (0..=10_000).step_by(50) {
            painter.step(ms(at), false);
        }
        let mut at = 10_000;
        let mut levels = Vec::new();
        loop {
            at += 50;
            match painter.step(ms(at), true) {
                Paint::Dark => break,
                Paint::Show { level, .. } => levels.push(level),
            }
        }
        assert!(at - 10_000 <= FADE_OUT.as_millis() as u64 + 50, "{at}");
        assert!(levels.windows(2).all(|pair| pair[1] < pair[0]));
        assert_eq!(painter.step(ms(at + 50), false), Paint::Dark);
    }
}
