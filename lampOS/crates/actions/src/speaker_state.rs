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
        // lamp-control's step and mute (the app calls beside `volume_to`):
        // the voice's own changes, with the same intents.
        "volume_step" => match args.get("step").and_then(Value::as_str) {
            Some("up") => Some(Intent::Audible),
            Some("down") => Some(Intent::Observe),
            _ => None,
        },
        "volume_muted" => match args.get("muted").and_then(Value::as_bool) {
            Some(true) => Some(Intent::Mute),
            Some(false) => Some(Intent::Audible),
            None => None,
        },
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

    /// Stop this optional publisher for the process lifetime. A partial send
    /// gets one best-effort unknown snapshot, including identity exhaustion
    /// detected before publication. Failed revocation cannot renew the last
    /// retained snapshot's original freshness deadline.
    fn fail(&mut self, why: impl Into<String>) {
        if self.failed.is_some() {
            return;
        }
        let why = why.into();
        eprintln!("lamp-actions: requested audio unavailable until restart: {why}");
        self.failed = Some(why);
        self.state.volume_raw = None;
        self.state.muted = self.logical_muted;
        let _ = self.emit();
    }

    fn advance_mute(&mut self) {
        match self.state.mute_generation.checked_add(1) {
            Some(next) => self.state.mute_generation = next,
            None => self.fail("speaker mute generation exhausted"),
        }
    }

    pub(crate) fn before_call(&mut self, intent: Intent) {
        if matches!(intent, Intent::Mute) {
            self.logical_muted = true;
            self.state.volume_raw = None;
            self.state.muted = true;
            self.advance_mute();
            // Even if the bus fails, the physical mute still gets its one
            // attempt. Only optional speaker publication stays unavailable.
            let _ = self.publish();
        }
    }

    pub(crate) fn before_restriction(&mut self) {
        self.volume_permitted = false;
        self.observe(None, false);
        // The ceiling still gets its attempt if optional publication fails.
        let _ = self.publish();
    }

    /// Consume Volume's retained evidence before a successful audible command
    /// can clear logical mute. Explicit mute already attempted its revocation.
    fn retain_observation(&mut self, observation: Option<Observation>) -> bool {
        let Some(observation) = observation else {
            return false;
        };
        if observation.sequence.is_none() || observation.sequence < self.observation_sequence {
            self.fail("speaker observation sequence exhausted or regressed");
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
                self.fail("speaker observation identity exhausted or regressed");
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
        let mut observation = sound.as_ref().map(|volume| volume.observation());
        let mut fresh = observation.is_some_and(|observation| {
            observation.sequence != Some(0)
                && (observation.sequence.is_none()
                    || observation.sequence != self.observation_sequence)
        });
        let due = self.failed.is_none()
            && poll
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
        // Retained observations still reach caps after this publisher fails;
        // only optional polling and publication stop until restart.
        let permitted = permit(observation, fresh);
        if !fresh && permitted == self.volume_permitted {
            return self.check();
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
        self.check()?;
        if let Err(error) = self.emit() {
            self.fail(format!("speaker state unavailable: {error}"));
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
    fn a_publication_failure_stops_all_publication_even_when_the_bus_recovers() {
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
        let stopped = service.publisher.totals().published;
        assert_eq!(stopped, 4, "one failed update and one unknown attempt");
        std::fs::remove_dir(&path).unwrap();
        service.before_call(Intent::Mute);
        service.after_call(Intent::Audible, true, observed(50), true, Instant::now());
        assert!(service.check().is_err());
        assert!(service.finish().is_err());
        assert_eq!(service.publisher.totals().published, stopped);
        assert_eq!(service.state.volume_raw, None);
        assert!(bus.latest(SpeakerState::TOPIC).unwrap().is_none());
    }

    #[test]
    fn mute_generation_exhaustion_never_wraps_or_authorizes_audio() {
        let home = tempfile::tempdir().unwrap();
        let bus = Bus::new(home.path()).unwrap();
        let mut service = Service::new(bus.clone(), "host".into()).unwrap();
        service.after_call(Intent::Audible, true, observed(30), true, Instant::now());
        service.state.mute_generation = u64::MAX;
        service.before_call(Intent::Mute);
        let stopped = bus.latest(SpeakerState::TOPIC).unwrap().unwrap();
        let unknown = stopped.parse::<SpeakerState>().unwrap();
        assert_eq!(unknown.volume_raw, None);
        assert!(unknown.muted);
        assert_eq!(unknown.mute_generation, u64::MAX);
        assert_eq!(service.publisher.totals().published, 2);
        service.after_call(Intent::Audible, true, observed(50), true, Instant::now());
        service.before_call(Intent::Mute);
        assert!(service.finish().is_err());
        assert!(service.check().is_err());
        assert_eq!(service.state.mute_generation, u64::MAX);
        assert_eq!(service.state.volume_raw, None);
        assert_eq!(service.publisher.totals().published, 2);
        assert_eq!(
            bus.latest(SpeakerState::TOPIC).unwrap().unwrap().payload,
            stopped.payload
        );
    }

    #[test]
    fn an_exhausted_or_regressed_observation_identity_stays_unknown() {
        for (revision, sequence) in [
            (None, Some(3)),
            (Some(0), Some(3)),
            (Some(1), None),
            (Some(1), Some(1)),
        ] {
            let home = tempfile::tempdir().unwrap();
            let bus = Bus::new(home.path()).unwrap();
            let mut service = Service::new(bus.clone(), "host".into()).unwrap();
            service.after_call(
                Intent::Audible,
                true,
                Some(Observation {
                    raw: Some(40),
                    revocation: Some(1),
                    sequence: Some(2),
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
                    sequence,
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
            assert_eq!(service.publisher.totals().published, 2);
            service.after_call(
                Intent::Audible,
                true,
                Some(Observation {
                    raw: Some(60),
                    revocation: Some(2),
                    sequence: Some(4),
                }),
                true,
                Instant::now(),
            );
            assert!(service.finish().is_err());
            assert_eq!(service.publisher.totals().published, 2);
            assert_eq!(
                bus.latest(SpeakerState::TOPIC)
                    .unwrap()
                    .unwrap()
                    .parse::<SpeakerState>()
                    .unwrap(),
                state
            );
        }
    }

    #[test]
    fn a_failed_publisher_still_passes_retained_mixer_evidence_to_caps_without_polling() {
        struct ReadOnce;
        impl Mixer for ReadOnce {
            fn get(&mut self) -> io::Result<u8> {
                Ok(30)
            }
            fn set(&mut self, _: u8) -> io::Result<()> {
                panic!("publication failure must not write the mixer")
            }
        }
        let home = tempfile::tempdir().unwrap();
        let mut service = Service::new(Bus::new(home.path()).unwrap(), "host".into()).unwrap();
        service.fail("fixture publication failure");
        let mut volume = Volume::new(ReadOnce, 100);
        let _ = volume.level();
        let observation = volume.observation();
        let now = Instant::now();
        for (offset, fresh) in [(0, true), (2, false)] {
            let mut received = false;
            assert!(
                service
                    .tick(
                        Some(&mut volume),
                        &|| now + Duration::from_secs(offset),
                        true,
                        |observed, changed| {
                            received = true;
                            let observed = observed.unwrap();
                            assert_eq!(observed.raw, Some(30));
                            assert_eq!(observed.sequence, observation.sequence);
                            assert_eq!(changed, fresh);
                            true
                        },
                    )
                    .is_err()
            );
            assert!(received, "caps still receive retained evidence");
            assert_eq!(volume.observation().sequence, observation.sequence);
        }
        assert_eq!(service.publisher.totals().published, 1);
    }
}
