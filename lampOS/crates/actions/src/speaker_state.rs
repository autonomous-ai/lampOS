//! Opt-in speaker observations from the existing, sole mixer owner.
use crate::sound::{Mixer, Observation, Volume};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{ClockStamp, SpeakerState};
use serde_json::Value;
use std::{
    ffi::OsStr,
    io,
    path::Path,
    time::{Duration, Instant},
};

pub const ENVIRONMENT: &str = "LAMP_REQUESTED_AUDIO";
pub const REFRESH: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
pub(crate) enum Intent {
    Mute,
    Audible,
    Observe,
}

/// Match the existing executive's accepted argument shapes. This is not a new
/// private command and does not widen or narrow the public Actions interface.
pub(crate) fn intent(name: &str, args: &Value) -> Option<Intent> {
    let nothing = args.as_object().is_none_or(serde_json::Map::is_empty) || args.is_null();
    match name {
        "volume_mute" if nothing => Some(Intent::Mute),
        "volume_up" | "volume_unmute" if nothing => Some(Intent::Audible),
        "volume_down" if nothing => Some(Intent::Observe),
        "volume_level" => Some(Intent::Observe),
        "volume_to" => args
            .get("volume")
            .and_then(Value::as_u64)
            .filter(|raw| *raw <= 100)
            .map(|raw| {
                if raw == 0 {
                    Intent::Mute
                } else {
                    Intent::Audible
                }
            }),
        _ => None,
    }
}

pub struct Service {
    bus: Bus,
    publisher: Publisher,
    state: SpeakerState,
    logical_muted: bool,
    observation_revocation: u64,
    observation_sequence: Option<u64>,
    volume_permitted: bool,
    observed_at: Option<Instant>,
    failed: Option<String>,
}

impl Service {
    /// Construction declares the topic but does not publish a state or read a
    /// mixer. Serving publishes unknown only after acquiring the Actions socket.
    pub fn new(bus: Bus, clock_id: String) -> io::Result<Self> {
        let stamp = ClockStamp {
            clock_id,
            at_us: 0,
            sequence: 1,
        };
        stamp.check().map_err(io::Error::other)?;
        let publisher = bus.publisher_declared(
            SpeakerState::TOPIC,
            &SpeakerState::declaration(),
            SpeakerState::SOURCE,
        )?;
        let state = SpeakerState {
            stamp,
            session_id: publisher.boot().to_owned(),
            mute_generation: 0,
            volume_raw: None,
            muted: false,
        };
        Ok(Self {
            bus,
            publisher,
            state,
            logical_muted: false,
            observation_revocation: 0,
            observation_sequence: None,
            volume_permitted: true,
            observed_at: None,
            failed: None,
        })
    }

    /// Inexact or absent switches do not inspect the root or the host clock.
    pub fn configured(switch: Option<&OsStr>, root: Option<&Path>) -> io::Result<Option<Self>> {
        if switch != Some(OsStr::new("on")) {
            return Ok(None);
        }
        let root = root.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "LAMP_REQUESTED_AUDIO=on requires --bus-root",
            )
        })?;
        let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Self::new(Bus::new(root)?, clock_id.trim().to_owned()).map(Some)
    }

    pub fn from_environment(root: Option<&Path>) -> io::Result<Option<Self>> {
        Self::configured(std::env::var_os(ENVIRONMENT).as_deref(), root)
    }

    pub(crate) fn start(&mut self) -> io::Result<()> {
        self.publish()
    }

    pub(crate) fn check(&self) -> io::Result<()> {
        match &self.failed {
            Some(why) => Err(io::Error::other(why.clone())),
            None => Ok(()),
        }
    }

    fn advance_mute(&mut self) {
        match self.state.mute_generation.checked_add(1) {
            Some(next) => self.state.mute_generation = next,
            None => {
                self.failed
                    .get_or_insert_with(|| "speaker mute generation exhausted".into());
                self.state.volume_raw = None;
            }
        }
    }

    pub(crate) fn before_call(&mut self, intent: Intent) {
        if matches!(intent, Intent::Mute) {
            self.advance_mute();
            self.logical_muted = true;
            self.state.volume_raw = None;
            self.state.muted = true;
            // Even if the bus fails, the physical mute still gets its one
            // attempt. The serving loop then fails with readiness removed.
            let _ = self.publish();
        }
    }

    pub(crate) fn before_restriction(&mut self) {
        self.volume_permitted = false;
        self.observe(None, false);
        // The ceiling still gets its attempt if publication fails. That
        // failure is sticky and serving then removes readiness.
        let _ = self.publish();
    }

    /// Consume Volume's retained evidence before a successful audible command
    /// can clear logical mute. Explicit mute already published its revocation.
    fn retain_observation(&mut self, observation: Option<Observation>) -> bool {
        let Some(observation) = observation else {
            return false;
        };
        if observation.sequence.is_none() || observation.sequence < self.observation_sequence {
            self.failed.get_or_insert_with(|| {
                "speaker observation sequence exhausted or regressed".into()
            });
            self.state.volume_raw = None;
            return true;
        }
        self.observation_sequence = observation.sequence;
        match observation.revocation {
            Some(revision) if revision >= self.observation_revocation => {
                let changed = revision != self.observation_revocation;
                self.observation_revocation = revision;
                if changed && !self.logical_muted {
                    self.advance_mute();
                }
                changed
            }
            _ => {
                self.failed.get_or_insert_with(|| {
                    "speaker observation identity exhausted or regressed".into()
                });
                self.state.volume_raw = None;
                true
            }
        }
    }

    fn observe(&mut self, raw: Option<u8>, retained: bool) {
        let raw = raw.filter(|raw| *raw <= 100);
        // A failed observation can be followed immediately by a successful
        // command. Latest may coalesce the unknown state, so its revocation
        // must remain in the counter. Explicit mute already advanced it.
        if raw.is_none() && self.state.volume_raw.is_some() && !self.logical_muted && !retained {
            self.advance_mute();
        }
        // A rounded app target or an independently observed zero also retires
        // old audio. It must not disappear between two Latest observations.
        if raw == Some(0) && self.state.volume_raw != Some(0) && !self.logical_muted && !retained {
            self.advance_mute();
        }
        self.state.volume_raw = if self.failed.is_none() { raw } else { None };
        self.state.muted = self.logical_muted || self.state.volume_raw == Some(0);
    }

    pub(crate) fn after_call(
        &mut self,
        intent: Intent,
        succeeded: bool,
        observation: Option<Observation>,
        volume_permitted: bool,
        at: Instant,
    ) {
        let retained = self.retain_observation(observation);
        let raw = observation.and_then(|observation| observation.raw);
        if succeeded
            && matches!(intent, Intent::Audible)
            && raw.is_some_and(|raw| raw > 0 && raw <= 100)
        {
            self.logical_muted = false;
        }
        self.volume_permitted = volume_permitted;
        self.observe(
            if succeeded && volume_permitted {
                raw
            } else {
                None
            },
            retained,
        );
        self.observed_at = Some(at);
        let _ = self.publish();
    }

    pub(crate) fn tick<M: Mixer>(
        &mut self,
        sound: Option<&mut Volume<M>>,
        now: &dyn Fn() -> Instant,
        poll: bool,
        mut permit: impl FnMut(Option<Observation>, bool) -> bool,
    ) -> io::Result<()> {
        self.check()?;
        let mut observation = sound.as_ref().map(|volume| volume.observation());
        let mut fresh = observation.is_some_and(|observation| {
            observation.sequence != Some(0)
                && (observation.sequence.is_none()
                    || observation.sequence != self.observation_sequence)
        });
        let due = poll
            && self
                .observed_at
                .is_none_or(|at| now().saturating_duration_since(at) >= REFRESH);
        if !fresh && due {
            observation = sound.map(|volume| {
                let _ = volume.level();
                volume.observation()
            });
            fresh = true;
        }
        // Cap writes already read back. Consume their new evidence now, rather
        // than doing a second read or waiting for this service's next heartbeat.
        // The callback may confirm a volume cap from this exact observation;
        // neither cap publication nor an unrelated light failure gates sound.
        let permitted = permit(observation, fresh);
        if !fresh && permitted == self.volume_permitted {
            return Ok(());
        }
        self.volume_permitted = permitted;
        let retained = self.retain_observation(observation);
        let raw = if permitted {
            observation.and_then(|observation| observation.raw)
        } else {
            None
        };
        self.observe(raw, retained);
        // Time the next read from completion, so a slow mixer cannot create a
        // burst of catch-up work. The loop never polls the mixer every 50 ms.
        if fresh {
            self.observed_at = Some(now());
        }
        self.publish()
    }

    pub(crate) fn finish(&mut self) -> io::Result<()> {
        self.state.volume_raw = None;
        self.state.muted = self.logical_muted;
        self.publish()
    }

    fn emit(&mut self) -> io::Result<()> {
        self.state.stamp.at_us = lamp_ipc::monotonic_us();
        self.state.stamp.sequence = self
            .publisher
            .totals()
            .published
            .checked_add(1)
            .ok_or_else(|| io::Error::other("speaker state sequence exhausted"))?;
        self.state.check().map_err(io::Error::other)?;
        self.publisher
            .publish_json(self.state.stamp.at_us, &self.state)?;
        // Bus suppresses repeated root errors. A successful call alone does
        // not establish that a new subscriber will read this exact snapshot.
        let retained = self.bus.latest(SpeakerState::TOPIC)?;
        let retained = retained.filter(|message| {
            message.header.boot == self.state.session_id
                && message.header.seq == self.state.stamp.sequence
                && message.header.at_us == self.state.stamp.at_us
                && message
                    .parse::<SpeakerState>()
                    .is_ok_and(|state| state == self.state)
        });
        if retained.is_none() {
            return Err(io::Error::other("speaker state snapshot was not retained"));
        }
        Ok(())
    }

    fn publish(&mut self) -> io::Result<()> {
        if self.failed.is_some() {
            self.state.volume_raw = None;
        }
        if let Err(error) = self.emit() {
            self.failed
                .get_or_insert_with(|| format!("speaker state unavailable: {error}"));
            self.state.volume_raw = None;
            self.state.muted = self.logical_muted;
            // Best effort revocation after partial transport failure; never
            // publish an audible observation again in this service session.
            let _ = self.emit();
        }
        self.check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(raw: u8) -> Option<Observation> {
        Some(Observation {
            raw: Some(raw),
            revocation: Some(0),
            sequence: Some(1),
        })
    }

    #[test]
    fn only_exact_on_inspects_configuration() {
        let unusable = Path::new("relative");
        for switch in [
            None,
            Some(OsStr::new("off")),
            Some(OsStr::new("ON")),
            Some(OsStr::new("true")),
        ] {
            assert!(
                Service::configured(switch, Some(unusable))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(Service::configured(Some(OsStr::new("on")), None).is_err());
    }

    #[test]
    fn only_executable_volume_arguments_signal_an_intent() {
        for args in [serde_json::json!({}), Value::Null, serde_json::json!([])] {
            assert!(matches!(intent("volume_mute", &args), Some(Intent::Mute)));
        }
        assert!(intent("volume_mute", &serde_json::json!({"by":1})).is_none());
        for args in [
            serde_json::json!({"volume":-1}),
            serde_json::json!({"volume":101}),
            serde_json::json!({"volume":0.0}),
            serde_json::json!({}),
        ] {
            assert!(intent("volume_to", &args).is_none());
        }
        assert!(matches!(
            intent("volume_to", &serde_json::json!({"volume":0})),
            Some(Intent::Mute)
        ));
        assert!(matches!(
            intent("volume_to", &serde_json::json!({"volume":40})),
            Some(Intent::Audible)
        ));
    }

    #[test]
    fn a_publication_failure_stays_unknown_even_when_the_bus_recovers() {
        let home = tempfile::tempdir().unwrap();
        let bus = Bus::new(home.path()).unwrap();
        let mut service = Service::new(Bus::new(home.path()).unwrap(), "host".into()).unwrap();
        service.start().unwrap();
        service.after_call(Intent::Audible, true, observed(30), true, Instant::now());
        let path = home.path().join(SpeakerState::TOPIC).join("latest");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        service.after_call(Intent::Audible, true, observed(40), true, Instant::now());
        assert!(service.check().is_err());
        std::fs::remove_dir(&path).unwrap();
        service.after_call(Intent::Audible, true, observed(50), true, Instant::now());
        assert!(service.check().is_err());
        let state = bus
            .latest(SpeakerState::TOPIC)
            .unwrap()
            .unwrap()
            .parse::<SpeakerState>()
            .unwrap();
        assert_eq!(state.volume_raw, None);
    }

    #[test]
    fn mute_generation_exhaustion_never_wraps_or_authorizes_audio() {
        let home = tempfile::tempdir().unwrap();
        let mut service = Service::new(Bus::new(home.path()).unwrap(), "host".into()).unwrap();
        service.state.mute_generation = u64::MAX;
        service.before_call(Intent::Mute);
        service.after_call(Intent::Audible, true, observed(50), true, Instant::now());
        assert!(service.check().is_err());
        assert_eq!(service.state.mute_generation, u64::MAX);
        assert_eq!(service.state.volume_raw, None);
    }

    #[test]
    fn an_exhausted_or_regressed_observation_identity_stays_unknown() {
        for revision in [None, Some(0)] {
            let home = tempfile::tempdir().unwrap();
            let bus = Bus::new(home.path()).unwrap();
            let mut service = Service::new(bus.clone(), "host".into()).unwrap();
            service.after_call(
                Intent::Audible,
                true,
                Some(Observation {
                    raw: Some(40),
                    revocation: Some(1),
                    sequence: Some(1),
                }),
                true,
                Instant::now(),
            );
            service.after_call(
                Intent::Audible,
                true,
                Some(Observation {
                    raw: Some(50),
                    revocation: revision,
                    sequence: Some(2),
                }),
                true,
                Instant::now(),
            );
            assert!(service.check().is_err());
            let state = bus
                .latest(SpeakerState::TOPIC)
                .unwrap()
                .unwrap()
                .parse::<SpeakerState>()
                .unwrap();
            assert_eq!(state.volume_raw, None);
        }
    }
}
