//! A physical-mute indication is deliberately narrower than a light permit.
use super::{
    ChannelCeiling, Error, FrameSink, GuardedRing, PIXEL_COUNT, Rgb, WriteReceipt, encode,
};
use lamp_turn_owner::{BootId, Error as Denial, MonoTime, Permission};

/// A trusted reader has checked the Voice publisher, bus incarnation, sequence
/// and kernel clock. The ring independently checks the controller incarnation,
/// microphone generation and original observation lifetime before each write.
/// This evidence can authorize only the fixed physical-mute indication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivacyObservation {
    pub controller_boot: BootId,
    pub microphone_generation: u64,
    pub observed_at: MonoTime,
    pub expires_at: MonoTime,
    pub permitted: bool,
    pub physical_muted: Option<bool>,
}

/// What the physical-mute indication shows, as the person chose it (#8696,
/// `docs/ring-choreography.md`, "The person's look"). A closed choice of
/// fixed frames, so no caller supplies pixels or a ceiling. The guards are
/// the same for each: a look changes only the light, never what may paint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrivacyLook {
    /// Today's breath: red 2 to 10, ceiling 10.
    #[default]
    Red,
    /// The same 4 s breath, dimmer: red 1 to 4, ceiling 4.
    Dim,
    /// Dark: one black frame when it begins, then nothing, while the
    /// evidence is still checked and kept.
    Off,
}

const FRESH_US: u64 = 50_000;
const BREATH_US: u64 = 4_000_000;
const PAINT_INTERVAL_US: u64 = 25_000;

impl<S: FrameSink> GuardedRing<S> {
    /// Paint only a dark-red physical-mute breath, with red in 2..=10 and no
    /// other channels. No caller-supplied pixels, ceiling or light permit exist
    /// on this path. A fresh, denied, ownerless controller and a matching fresh
    /// physical LOW are both required. Ordinary Allowed snapshots never qualify.
    ///
    /// Missing, unknown, open or expired evidence blanks an existing privacy
    /// indication immediately. It cannot erase a separately authorized cue.
    /// Fresh observations refresh evidence without painting more than 40 times
    /// a second. Keep calling `tick` so either lease can expire on its own.
    pub fn show_privacy_muted(
        &mut self,
        observation: Option<PrivacyObservation>,
    ) -> Result<Option<WriteReceipt>, Error> {
        self.show_privacy_look(observation, PrivacyLook::Red)
    }

    /// [`Self::show_privacy_muted`] in the person's `look` (#8696). The same
    /// evidence is checked first, unchanged, and only then is the look's
    /// frame picked: [`PrivacyLook::Red`] is today's breath,
    /// [`PrivacyLook::Dim`] the same breath at red 1 to 4, and
    /// [`PrivacyLook::Off`] one black frame as it begins and then no write,
    /// while valid evidence is still kept so a denial blanks as before and a
    /// later look paints at once. A changed look is painted at once, not
    /// after the frame interval.
    pub fn show_privacy_look(
        &mut self,
        observation: Option<PrivacyObservation>,
        look: PrivacyLook,
    ) -> Result<Option<WriteReceipt>, Error> {
        self.ensure_live()?;
        let started_at = (self.clock)();
        let denial = observation.map_or(Some(Denial::NoAuthority), |observation| {
            self.privacy_denial(observation, started_at)
        });
        if let Some(reason) = denial {
            if reason == Denial::ClockRegression {
                self.latch_fault();
                return Err(self.reject(reason));
            }
            if self.privacy.is_some() {
                return self
                    .blank(super::BlankReason::AuthorityLost(reason))
                    .map(|_| None)
                    .map_err(|failure| {
                        self.latch_fault();
                        Error {
                            cause: super::Failure::Write(failure),
                            cleanup_error: None,
                        }
                    });
            }
            return Ok(None);
        }
        if self.active.is_some() || self.resting {
            return Ok(None);
        }
        let observation = observation.expect("validated physical-mute evidence");
        let same_look = self.privacy_painted_at.is_some() && self.privacy_look == look;
        // Dark already: the evidence is kept, nothing is written.
        if same_look && look == PrivacyLook::Off {
            self.privacy = Some(observation);
            return Ok(None);
        }
        if same_look
            && self.privacy_painted_at.is_some_and(|last| {
                started_at
                    .as_micros()
                    .checked_sub(last.as_micros())
                    .is_some_and(|elapsed| elapsed < PAINT_INTERVAL_US)
            })
        {
            self.privacy = Some(observation);
            return Ok(None);
        }
        let frame = match look {
            PrivacyLook::Red => encode(&muted_pixels(started_at), ChannelCeiling(10)),
            PrivacyLook::Dim => encode(&dim_pixels(started_at), ChannelCeiling(DIM_CEILING)),
            PrivacyLook::Off => self.black.clone(),
        };
        let receipt = match self.transfer(&frame, started_at) {
            Ok(receipt) => receipt,
            Err(failure) => return Err(self.write_fault(failure)),
        };
        self.privacy = Some(observation);
        self.privacy_painted_at = Some(started_at);
        self.privacy_look = look;
        // The red is not a cue: it never fades or dims.
        self.lit = None;
        self.fading = None;
        // An observation or controller lease may expire during a kernel write.
        // A successful transfer cannot leave that now-unauthorized red latched.
        if let Some(reason) = self.privacy_denial(observation, (self.clock)()) {
            return Err(self.reject(reason));
        }
        Ok(Some(receipt))
    }

    /// Whether the physical-mute indication holds now, in any look: the
    /// last evidence passed every guard and nothing has blanked it since.
    /// Dark ([`PrivacyLook::Off`]) holds too, with no frame being written,
    /// so the lamp can still say the microphone is off at the switch.
    pub fn privacy_shown(&self) -> bool {
        self.privacy.is_some()
    }

    pub(super) fn privacy_denial(
        &self,
        observation: PrivacyObservation,
        now: MonoTime,
    ) -> Option<Denial> {
        if self.guard.is_faulted() {
            return Some(Denial::Faulted);
        }
        let Some(state) = self.guard.state() else {
            return Some(Denial::NoAuthority);
        };
        if state.boot() != observation.controller_boot {
            Some(Denial::WrongBoot)
        } else if self.privacy_painted_at.is_some_and(|last| now < last) {
            Some(Denial::ClockRegression)
        } else if state.issued_at() > now {
            Some(Denial::FutureState)
        } else if now >= state.expires_at() {
            Some(Denial::ExpiredState)
        } else if state.microphone_permission() != Permission::Denied
            || state.camera_permission() != Permission::Denied
            || state.owner().is_some()
            || observation.permitted
            || observation.physical_muted != Some(true)
        {
            Some(Denial::PrivacyClosed)
        } else if state.microphone_generation() != observation.microphone_generation {
            Some(Denial::StateRegression)
        } else if observation.observed_at.as_micros() == 0
            || observation
                .expires_at
                .as_micros()
                .checked_sub(observation.observed_at.as_micros())
                .is_none_or(|lease| lease == 0 || lease > FRESH_US)
        {
            Some(Denial::InvalidLease)
        } else if observation.observed_at > now {
            Some(Denial::FutureObservation)
        } else if now >= observation.expires_at {
            Some(Denial::ExpiredState)
        } else {
            None
        }
    }
}

fn muted_pixels(at: MonoTime) -> [Rgb; PIXEL_COUNT] {
    let phase = (at.as_micros() % BREATH_US) as f64 / BREATH_US as f64;
    let red = (2.0 + 4.0 * (1.0 - (phase * std::f64::consts::TAU).cos())).round() as u8;
    [Rgb::new(red.min(10), 0, 0); PIXEL_COUNT]
}

/// The dimmer look's highest red (#8696): the same breath under half of
/// red's 10. Named in `docs/ring-choreography.md`.
pub const DIM_CEILING: u8 = 4;

/// The dimmer look: the same 4 s cosine breath as [`muted_pixels`], red 1
/// to [`DIM_CEILING`], so it never goes dark at the trough.
fn dim_pixels(at: MonoTime) -> [Rgb; PIXEL_COUNT] {
    let phase = (at.as_micros() % BREATH_US) as f64 / BREATH_US as f64;
    let red = (1.0 + 1.5 * (1.0 - (phase * std::f64::consts::TAU).cos())).round() as u8;
    [Rgb::new(red.clamp(1, DIM_CEILING), 0, 0); PIXEL_COUNT]
}
