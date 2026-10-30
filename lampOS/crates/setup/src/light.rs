//! The setup light (#806, `docs/setup.md`): from the hotspot to the end of
//! setup the voice is not running, so without this the lamp is dark and
//! silent, and a person would think it broke. The ring breathes slowly in
//! soft white while the lamp waits for its person, a little faster in soft
//! blue while it joins the home's Wi-Fi, and slowly in amber after a join
//! that failed, until the next try. It follows the setup status, and lets
//! the ring go just before the marker is written ([`ReleasingSave`]): the
//! marker starts the lamp, and the voice must find the ring free.

use crate::serve::{Phase, Save, Status};
use lamp_ring::{
    FrameSink,
    setup::{SetupLook, SetupRing},
};
use std::{
    io,
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// One frame this often: enough for a slow breath.
pub const FRAME: Duration = Duration::from_millis(50);
/// It fades in over this, as eyes open.
pub const FADE_IN: Duration = Duration::from_millis(1_500);
/// And out over this, when the ring is let go.
pub const FADE_OUT: Duration = Duration::from_millis(600);
/// One look turns into the next over this.
pub const CHANGE: Duration = Duration::from_millis(800);
/// The least of a breath, as a share of its top: never dark, so it reads as on.
pub const BREATH_LOW: f32 = 0.35;
/// After a ring that could not be opened or written, the next try.
pub const RETRY: Duration = Duration::from_secs(5);
/// The longest letting go waits for the ring to be closed.
pub const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// Where the painter reads the time and waits out a frame: [`HostClock`] on a
/// lamp. A test's clock may run faster, so that a fade takes no time.
pub trait Clock: Send + 'static {
    fn now(&self) -> Instant;
    fn pause(&self, pause: Duration);
}

/// The host's clock, the painter's on a lamp.
pub struct HostClock;

impl Clock for HostClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn pause(&self, pause: Duration) {
        thread::sleep(pause);
    }
}

/// What the ring shows in a setup phase. `None` once setup is done: the ring
/// is the lamp's.
pub fn look(phase: Phase) -> Option<SetupLook> {
    match phase {
        Phase::Idle => Some(SetupLook::Waiting),
        Phase::Connecting => Some(SetupLook::Joining),
        Phase::Failed => Some(SetupLook::Failed),
        Phase::Connected => None,
    }
}

/// One breath, in and out: slow and calm while waiting, as the asleep face
/// breathes; faster while joining, busy but not hurried.
pub fn breath(look: SetupLook) -> Duration {
    match look {
        SetupLook::Waiting | SetupLook::Failed | SetupLook::Unplug => Duration::from_secs(6),
        SetupLook::Joining => Duration::from_secs(3),
    }
}

/// What to put on the ring now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// [`SetupRing::show`] with these.
    Show {
        from: SetupLook,
        to: SetupLook,
        share: f32,
        level: f32,
    },
    /// Faded out: black, and the ring closed.
    Dark,
}

/// The light's motion, with no clock or device of its own: given the time
/// and the look wanted, what the ring shows. The breath runs on through a
/// change of look and of pace, so nothing jumps.
#[derive(Debug, Default)]
pub struct Painter {
    to: Option<SetupLook>,
    from: Option<SetupLook>,
    changed_at: Duration,
    /// How far in (0 to 1) the light has faded.
    fade: f32,
    /// Breaths so far: the fraction is how far through this one.
    breaths: f32,
    last: Duration,
}

impl Painter {
    /// One step at `now`, since some fixed start, towards `wanted`.
    pub fn step(&mut self, now: Duration, wanted: Option<SetupLook>) -> Paint {
        let elapsed = now.saturating_sub(self.last);
        self.last = now;
        if let Some(look) = wanted
            && self.to != Some(look)
        {
            // Mid-change, it goes on from whichever colour is the nearer.
            self.from = match (self.to, self.from) {
                (Some(_), Some(from)) if self.share(now) < 0.5 => Some(from),
                (Some(to), _) => Some(to),
                (None, _) => None,
            };
            self.to = Some(look);
            self.changed_at = now;
        }
        let Some(to) = self.to else {
            return Paint::Dark;
        };
        if let Some(look) = wanted {
            self.breaths += elapsed.as_secs_f32() / breath(look).as_secs_f32();
            self.fade = (self.fade + elapsed.as_secs_f32() / FADE_IN.as_secs_f32()).min(1.0);
        } else {
            self.breaths += elapsed.as_secs_f32() / breath(to).as_secs_f32();
            self.fade = (self.fade - elapsed.as_secs_f32() / FADE_OUT.as_secs_f32()).max(0.0);
            if self.fade == 0.0 {
                // Dark: the next look fades in from nothing, with no change.
                self.to = None;
                self.from = None;
                return Paint::Dark;
            }
        }
        self.breaths = self.breaths.fract();
        let up = 0.5 - 0.5 * (std::f32::consts::TAU * self.breaths).cos();
        Paint::Show {
            from: self.from.unwrap_or(to),
            to,
            share: self.share(now),
            level: self.fade * (BREATH_LOW + (1.0 - BREATH_LOW) * up),
        }
    }

    fn share(&self, now: Duration) -> f32 {
        if self.from.is_none() {
            return 1.0;
        }
        (now.saturating_sub(self.changed_at).as_secs_f32() / CHANGE.as_secs_f32()).min(1.0)
    }
}

#[derive(Default)]
struct State {
    /// The join ended well and the marker is next: the ring is let go while
    /// setup is still `connecting`. Cleared as soon as it is not, so a save
    /// that failed shows amber, and the next press shows blue at once.
    released: bool,
    stop: bool,
    /// No ring is open.
    closed: bool,
    /// The painter has ended.
    ended: bool,
}

/// The setup light's painter, running on a thread of its own.
pub struct Light {
    state: Arc<(Mutex<State>, Condvar)>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Light {
    /// Start painting what `status` says. `open` opens the ring (on a lamp,
    /// `lamp_ring::linux::open_setup`); a ring that cannot be opened or
    /// written is tried again after [`RETRY`], and said once each time it
    /// starts to fail.
    pub fn spawn<S, O, L>(status: Arc<Mutex<Status>>, open: O, log: L) -> Self
    where
        S: FrameSink + Send + 'static,
        O: FnMut() -> io::Result<SetupRing<S>> + Send + 'static,
        L: Fn(&str) + Send + Sync + 'static,
    {
        Self::spawn_with_retry(status, open, log, RETRY)
    }

    /// [`Light::spawn`] only when `on`, setup's `light` switch: off, no
    /// thread, and the ring is never opened.
    pub fn spawn_if<S, O, L>(on: bool, status: Arc<Mutex<Status>>, open: O, log: L) -> Option<Self>
    where
        S: FrameSink + Send + 'static,
        O: FnMut() -> io::Result<SetupRing<S>> + Send + 'static,
        L: Fn(&str) + Send + Sync + 'static,
    {
        on.then(|| Self::spawn(status, open, log))
    }

    /// [`Light::spawn`], trying a failed ring again after `retry`.
    pub fn spawn_with_retry<S, O, L>(
        status: Arc<Mutex<Status>>,
        open: O,
        log: L,
        retry: Duration,
    ) -> Self
    where
        S: FrameSink + Send + 'static,
        O: FnMut() -> io::Result<SetupRing<S>> + Send + 'static,
        L: Fn(&str) + Send + Sync + 'static,
    {
        Self::spawn_with_clock(status, open, log, retry, HostClock)
    }

    /// [`Light::spawn_with_retry`], painting on `clock` instead of the
    /// host's: for a test that runs the fades fast.
    pub fn spawn_with_clock<S, O, L, C>(
        status: Arc<Mutex<Status>>,
        open: O,
        log: L,
        retry: Duration,
        clock: C,
    ) -> Self
    where
        S: FrameSink + Send + 'static,
        O: FnMut() -> io::Result<SetupRing<S>> + Send + 'static,
        L: Fn(&str) + Send + Sync + 'static,
        C: Clock,
    {
        let log: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(log);
        let state = Arc::new((
            Mutex::new(State {
                closed: true,
                ..State::default()
            }),
            Condvar::new(),
        ));
        thread::spawn({
            let state = Arc::clone(&state);
            let status = Arc::clone(&status);
            let log = Arc::clone(&log);
            move || paint(&state, &status, open, |line| log(line), retry, &clock)
        });
        Self { state, log }
    }

    /// Let the ring go before the marker: fade out, black, and close it. It
    /// waits at most [`RELEASE_WAIT`] (said if that was not enough); whether
    /// the ring was closed in time. A run that then fails shows the failure.
    /// Call it only while setup is `connecting`, as the marker is.
    pub fn release(&self) -> bool {
        // A painter that died must not hold up the marker: its lock is
        // taken as it was left.
        let (state, changed) = &*self.state;
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        state.released = true;
        let closed = changed
            .wait_timeout_while(state, RELEASE_WAIT, |state| !state.closed)
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .closed;
        if !closed {
            (self.log)("the setup light did not let the ring go in time");
        }
        closed
    }

    /// Fade out, black, and end. Waits at most [`RELEASE_WAIT`].
    pub fn stop(&self) -> bool {
        let (state, changed) = &*self.state;
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        state.stop = true;
        changed
            .wait_timeout_while(state, RELEASE_WAIT, |state| !state.ended)
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .ended
    }
}

fn paint<S: FrameSink>(
    state: &(Mutex<State>, Condvar),
    status: &Mutex<Status>,
    mut open: impl FnMut() -> io::Result<SetupRing<S>>,
    log: impl Fn(&str),
    retry: Duration,
    clock: &impl Clock,
) {
    let (state, changed) = state;
    let start = clock.now();
    let mut painter = Painter::default();
    let mut ring: Option<SetupRing<S>> = None;
    let mut retry_at: Option<Instant> = None;
    let mut failing = false;
    loop {
        // The whole step under the lock, so a ring let go is never opened
        // again by a step that began before.
        let mut held = state.lock().unwrap_or_else(PoisonError::into_inner);
        let phase = status.lock().unwrap_or_else(PoisonError::into_inner).phase;
        if phase != Phase::Connecting {
            held.released = false;
        }
        let let_go = held.stop || held.released;
        let wanted = if let_go { None } else { look(phase) };
        match painter.step(clock.now().saturating_duration_since(start), wanted) {
            Paint::Show {
                from,
                to,
                share,
                level,
            } => {
                // Opened only while a look is wanted: never while fading out
                // after a release, when the marker may already be written.
                if ring.is_none() && wanted.is_some() && retry_at.is_none_or(|at| clock.now() >= at)
                {
                    match open() {
                        Ok(opened) => {
                            ring = Some(opened);
                            retry_at = None;
                            if failing {
                                log("the setup light has the ring again");
                                failing = false;
                            }
                        }
                        Err(error) => {
                            if !failing {
                                log(&format!("the setup light cannot open the ring: {error}"));
                                failing = true;
                            }
                            retry_at = Some(clock.now() + retry);
                        }
                    }
                }
                if let Some(open_ring) = ring.as_mut()
                    && let Err(error) = open_ring.show(from, to, share, level)
                {
                    if !failing {
                        log(&format!("the setup light cannot write the ring: {error}"));
                        failing = true;
                    }
                    ring = None;
                    retry_at = Some(clock.now() + retry);
                }
            }
            Paint::Dark => {
                if let Some(open_ring) = ring.take()
                    && let Err(error) = open_ring.shutdown()
                {
                    log(&format!(
                        "the setup light's last black frame failed: {error}"
                    ));
                }
            }
        }
        held.closed = ring.is_none();
        if held.stop && held.closed {
            held.ended = true;
            changed.notify_all();
            return;
        }
        changed.notify_all();
        drop(held);
        clock.pause(FRAME);
    }
}

/// A [`Save`] that lets the setup light's ring go just before the marker,
/// since the marker starts the lamp. Without a light it is `save` alone.
pub struct ReleasingSave<'a, S> {
    pub save: S,
    pub light: Option<&'a Light>,
}

impl<S: Save> Save for ReleasingSave<'_, S> {
    fn credential(&mut self, key: &str) -> io::Result<()> {
        self.save.credential(key)
    }
    fn name(&mut self, name: &str) -> io::Result<()> {
        self.save.name(name)
    }
    fn timezone(&mut self, zone: &str) -> io::Result<()> {
        self.save.timezone(zone)
    }
    fn language(&mut self, code: &str) -> io::Result<()> {
        self.save.language(code)
    }
    fn backend(&mut self, link: &crate::serve::Backend) -> io::Result<()> {
        self.save.backend(link)
    }
    fn reports(&mut self, share: bool) -> io::Result<()> {
        self.save.reports(share)
    }
    fn own_keys_on(&self) -> bool {
        self.save.own_keys_on()
    }
    fn own_key(&mut self, given: &crate::keys::Given) -> crate::keys::Verdict {
        self.save.own_key(given)
    }
    fn keeps_settings(&self) -> bool {
        self.save.keeps_settings()
    }
    fn marker(&mut self) -> io::Result<()> {
        if let Some(light) = self.light {
            light.release();
        }
        self.save.marker()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn level(paint: Paint) -> f32 {
        match paint {
            Paint::Show { level, .. } => level,
            Paint::Dark => 0.0,
        }
    }

    #[test]
    fn each_phase_has_its_look_and_a_done_lamp_has_none() {
        assert_eq!(look(Phase::Idle), Some(SetupLook::Waiting));
        assert_eq!(look(Phase::Connecting), Some(SetupLook::Joining));
        assert_eq!(look(Phase::Failed), Some(SetupLook::Failed));
        assert_eq!(look(Phase::Connected), None);
        assert!(breath(SetupLook::Joining) < breath(SetupLook::Waiting));
    }

    #[test]
    fn it_fades_in_then_breathes_and_never_goes_dark() {
        let mut painter = Painter::default();
        assert_eq!(
            painter.step(ms(0), Some(SetupLook::Waiting)),
            Paint::Show {
                from: SetupLook::Waiting,
                to: SetupLook::Waiting,
                share: 1.0,
                level: 0.0,
            }
        );
        let mut at = 0;
        let mut levels = Vec::new();
        while at < 20_000 {
            at += 50;
            levels.push(level(painter.step(ms(at), Some(SetupLook::Waiting))));
        }
        // Fading in, under the breath's top.
        assert!(
            levels[..29]
                .windows(2)
                .all(|pair| pair[0] <= pair[1] + 0.01)
        );
        // Then between the breath's low and its top, reaching both.
        let after: Vec<f32> = levels[60..].to_vec();
        assert!(
            after
                .iter()
                .all(|level| (BREATH_LOW - 0.001..=1.0).contains(level))
        );
        assert!(after.iter().any(|level| *level > 0.99));
        assert!(after.iter().any(|level| *level < BREATH_LOW + 0.01));
    }

    #[test]
    fn a_new_look_turns_into_the_next_over_the_change() {
        let mut painter = Painter::default();
        for at in (0..=3_000).step_by(50) {
            painter.step(ms(at), Some(SetupLook::Waiting));
        }
        let Paint::Show {
            from, to, share, ..
        } = painter.step(ms(3_050), Some(SetupLook::Joining))
        else {
            panic!("dark");
        };
        assert_eq!((from, to), (SetupLook::Waiting, SetupLook::Joining));
        assert!(share < 0.1);
        let Paint::Show { share, .. } = painter.step(ms(3_450), Some(SetupLook::Joining)) else {
            panic!("dark");
        };
        assert!((share - 0.5).abs() < 0.01);
        let Paint::Show { share, .. } = painter.step(ms(3_900), Some(SetupLook::Joining)) else {
            panic!("dark");
        };
        assert_eq!(share, 1.0);
    }

    #[test]
    fn let_go_it_fades_out_then_is_dark_and_comes_back_from_nothing() {
        let mut painter = Painter::default();
        for at in (0..=3_000).step_by(50) {
            painter.step(ms(at), Some(SetupLook::Joining));
        }
        let mut at = 3_000;
        let mut faded = Vec::new();
        loop {
            at += 50;
            match painter.step(ms(at), None) {
                Paint::Dark => break,
                paint => faded.push(level(paint)),
            }
        }
        assert!(at - 3_000 <= FADE_OUT.as_millis() as u64 + 50);
        assert!(faded.windows(2).all(|pair| pair[1] < pair[0] + 0.05));
        assert_eq!(painter.step(ms(at + 50), None), Paint::Dark);
        // A failure after it was let go: amber, fading in from dark, no change.
        let Paint::Show {
            from,
            to,
            share,
            level,
            ..
        } = painter.step(ms(at + 100), Some(SetupLook::Failed))
        else {
            panic!("dark");
        };
        assert_eq!(
            (from, to, share),
            (SetupLook::Failed, SetupLook::Failed, 1.0)
        );
        assert!(level < 0.05);
    }

    #[test]
    fn joining_breathes_twice_as_fast_as_waiting() {
        let tops = |look| {
            let mut painter = Painter::default();
            let mut tops = 0;
            let mut was = 0.0;
            let mut rising = true;
            for at in (0..=30_000).step_by(50) {
                let now = level(painter.step(ms(at), Some(look)));
                if rising && now < was && at > 1_000 {
                    tops += 1;
                }
                rising = now >= was;
                was = now;
            }
            tops
        };
        assert_eq!(tops(SetupLook::Waiting), 5);
        assert_eq!(tops(SetupLook::Joining), 10);
    }
}
