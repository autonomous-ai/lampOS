//! Someone has come and sat down at the lamp, and a hello is due
//! (`voice/arrival`, `docs/speaking-first.md`, "Asking the policy"). Counts
//! and times only: no picture, no face and nobody's identity.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// A hello is due, and the asker holds it a moment for the policy to say
/// whether now is the time. The voice asked on `voice/arrival` until its own
/// hello was removed (#2825); the policy now puts a skill's hello ask
/// (Welcome back's) as one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceArrival {
    /// This arrival's number, which the policy's decision names
    /// (`Decision::arrival`): the voice's monotonic clock in microseconds
    /// when it asked, so it grows and is never 0.
    pub arrival: u64,
    /// How long nobody was at the lamp before, in whole minutes.
    pub away_min: u32,
    /// How long they have been there, in whole seconds.
    pub there_s: u32,
    /// How long since anything was said to the lamp or by it, in whole
    /// seconds.
    pub quiet_s: u32,
    /// How long since the lamp last said hello, in whole minutes, if it has
    /// since the voice started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_hello_min: Option<u32>,
}

impl VoiceArrival {
    pub const TYPE: &str = "lamp_msgs/VoiceArrival";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/arrival";

    /// Something that happened and is over: no value is kept, and none is
    /// promised at any period.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.arrival == 0 {
            return Err(Invalid("an arrival's number is positive".into()));
        }
        Ok(())
    }
}

/// The voice's optional arrival topic, apart from the first topics.
pub fn arrival_topics() -> Vec<(&'static str, Declaration)> {
    vec![(VoiceArrival::TOPIC, VoiceArrival::declaration())]
}
