//! Whether anyone is at the lamp (`room/presence`, #1677,
//! `docs/presence.md`): a face the eyes saw or a voice the lamp heard
//! lately, or nobody, and for how long. One signal for every reader: the
//! light going out when nobody is there, and the news sent to the app
//! instead of into an empty room (#1673). Counts and times only: no
//! picture, no face and nobody's identity.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Someone at the lamp, or nobody for how long, as the eyes judge it.
///
/// Said only while the eyes are looking: with the camera closed (the
/// privacy switch, a mute, the voice gone) nobody can be said to be away,
/// so nothing is said and the last value runs out ([`RoomPresence::LASTS`]).
/// A reader treats no fresh value as not known, never as nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomPresence {
    /// A face was seen, a voice heard or the lamp touched within
    /// [`RoomPresence::SOMEONE_WITHIN_S`], or, in light-out's dim, the view
    /// changed (#5746).
    pub someone: bool,
    /// How long nobody has been at the lamp, in whole seconds: since the
    /// last face seen or voice heard, or, with neither since the eyes began
    /// looking, since they began. 0 while someone is there.
    pub nobody_s: u32,
    /// While someone is there, what last showed it: a face, a voice, a
    /// touch or, in light-out's dim, the view changing
    /// (#1677, the first builder's lamp check), so a run can tell a passer-by
    /// from someone at the desk. Absent while nobody is, and from an older
    /// eyes' message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<SeenBy>,
    /// While nobody is at the lamp and light-out's light is out, the view
    /// changed (#5746, the reviewer's P2 on #5765): a reader the dim missed
    /// turning a page. Light-out brings its dim back; `nobody_s` goes on.
    /// Said true for [`RoomPresence::STIRRED_FOR`] after the change; absent
    /// (false) otherwise, and from an older eyes'.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stirred: bool,
}

/// What showed someone at the lamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeenBy {
    Face,
    Voice,
    /// A touch on the lamp, or a pet on its head (#5746): someone's hand.
    Touch,
    /// The view changed while light-out's dim showed (#5746): a reader the
    /// eyes can't find turning a page. Never said outside that dim.
    Moved,
}

impl RoomPresence {
    pub const TYPE: &str = "lamp_msgs/RoomPresence";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "room/presence";
    /// Said this often while the eyes look, and at once when `someone`
    /// changes.
    pub const EVERY: Duration = Duration::from_secs(1);
    /// A value means nothing this long after it was said: three periods, so
    /// one late message never reads as not known.
    pub const LASTS: Duration = Duration::from_secs(3);
    /// A face lost or a voice gone quiet for less than this is someone who
    /// looked away or paused, not someone who left: the voice's Arrival
    /// forgives the same 20 s (`crates/voice/src/speak_up.rs`).
    pub const SOMEONE_WITHIN_S: u32 = 20;
    /// A stir is said this long, so a reader taking once a second misses
    /// none.
    pub const STIRRED_FOR: Duration = Duration::from_secs(3);

    /// The newest replaces the one before, once a second, each good for
    /// three.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    /// Someone there has been away for no time.
    pub fn check(&self) -> Result<(), Invalid> {
        if !self.someone && self.by.is_some() {
            return Err(Invalid(
                "nobody is at the lamp, and by says what showed someone".into(),
            ));
        }
        if self.someone && self.stirred {
            return Err(Invalid(
                "someone is at the lamp, and stirred says the view changed with nobody there"
                    .into(),
            ));
        }
        if self.someone && self.nobody_s != 0 {
            return Err(Invalid(format!(
                "someone is at the lamp, and nobody_s is {}: 0 while someone is",
                self.nobody_s
            )));
        }
        Ok(())
    }
}

/// The presence topic, apart from the first topics.
pub fn presence_topics() -> Vec<(&'static str, Declaration)> {
    vec![(RoomPresence::TOPIC, RoomPresence::declaration())]
}
