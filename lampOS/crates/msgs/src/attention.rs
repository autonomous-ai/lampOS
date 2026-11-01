//! Whether what a person said was said to the lamp from close to it
//! (`attention/addressed`, `docs/attention.md`). No sound, no words and no
//! picture are in it: levels, a face's size and a score.
use crate::{EARS_SILENCE_DB, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// One request the voice heard, and the signs that it was said to the lamp
/// from close to it. The voice says it once for each request it followed:
/// when the request's sound ends, or when its answer begins if that comes
/// first. Times are the lamp's monotonic clock, in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Addressed {
    /// The voice's session with its speech service, and the request in it.
    pub session: u64,
    pub request: u64,
    /// When the request's sound began and, if it had by then, ended.
    pub began_us: u64,
    pub ended_us: Option<u64>,
    /// The room microphone's highest rise over the room's quiet while the
    /// request's sound was heard, in dB. None: the ears said nothing fresh
    /// in that time, or only while the lamp was speaking.
    pub rise_db: Option<f32>,
    /// Close: `rise_db` at or over the voice's line for a voice at the desk.
    pub near: Option<bool>,
    /// How many of the eyes' looks were taken while the request's sound
    /// was heard, or just before it began.
    pub looks: u16,
    /// The tallest face turned to the lamp in those looks, as a share of
    /// the picture's height; 0 when none was turned to it. None: no looks.
    pub facing_height: Option<f32>,
    /// The share of those looks that saw a face turned to the lamp, tall
    /// enough to be someone at the desk. None: too few looks to say.
    pub facing_share: Option<f32>,
    /// That share at or over the voice's line for someone facing the lamp.
    pub facing_close: Option<bool>,
    /// The best score of the lamp's name heard as the request began or in
    /// its sound, from 0 to 1; 0 when it was not heard.
    pub name_score: f32,
    /// Said to the lamp from close to it: near, and a close face turned to it.
    pub addressed: bool,
    /// How alike this request's voice is to the voice of the person the
    /// lamp is in conversation with, the one who said the line that began
    /// it: the cosine of the two lines' voice prints, from -1 to 1
    /// (`docs/same-voice.md`, "The asker's voice"). None when either print
    /// is not known, or no conversation is going on. Left out when none, so
    /// a voice that does not say it says what it said before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub like_asker: Option<f32>,
}

impl Addressed {
    pub const TYPE: &str = "lamp_msgs/Addressed";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "attention/addressed";

    /// One for each request; each is over when it has been sent.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.ended_us.is_some_and(|ended| ended < self.began_us) {
            return Err(Invalid("ended_us is before began_us".into()));
        }
        let most = -EARS_SILENCE_DB;
        if let Some(rise) = self.rise_db
            && !(rise.is_finite() && (-most..=most).contains(&rise))
        {
            return Err(Invalid(format!(
                "rise_db is {rise}: dB from {} to {most}",
                -most
            )));
        }
        if self.rise_db.is_none() && self.near.is_some() {
            return Err(Invalid("near is said with no rise_db".into()));
        }
        for (name, share) in [
            ("facing_height", self.facing_height),
            ("facing_share", self.facing_share),
        ] {
            if let Some(share) = share
                && !(share.is_finite() && (0.0..=1.0).contains(&share))
            {
                return Err(Invalid(format!("{name} is {share}: from 0 to 1")));
            }
        }
        if (self.looks == 0) != self.facing_height.is_none() {
            return Err(Invalid(
                "facing_height is said exactly when there were looks".into(),
            ));
        }
        if self.facing_share.is_some() && self.looks < 2 {
            return Err(Invalid(
                "facing_share is said of fewer than two looks".into(),
            ));
        }
        if self.facing_share.is_none() && self.facing_close.is_some() {
            return Err(Invalid("facing_close is said with no facing_share".into()));
        }
        if !(self.name_score.is_finite() && (0.0..=1.0).contains(&self.name_score)) {
            return Err(Invalid(format!(
                "name_score is {}: from 0 to 1",
                self.name_score
            )));
        }
        if let Some(like) = self.like_asker
            && !(like.is_finite() && (-1.0..=1.0).contains(&like))
        {
            return Err(Invalid(format!("like_asker is {like}: from -1 to 1")));
        }
        if self.addressed != (self.near == Some(true) && self.facing_close == Some(true)) {
            return Err(Invalid(
                "addressed is near and a close face turned to the lamp, no more and no less".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said() -> Addressed {
        Addressed {
            session: 3,
            request: 41,
            began_us: 10_000_000,
            ended_us: Some(12_400_000),
            rise_db: Some(21.5),
            near: Some(true),
            looks: 8,
            facing_height: Some(0.22),
            facing_share: Some(0.75),
            facing_close: Some(true),
            name_score: 0.0,
            addressed: true,
            like_asker: None,
        }
    }

    #[test]
    fn a_request_said_close_and_facing_is_numbers_only() {
        let text = serde_json::to_string(&said()).unwrap();
        assert_eq!(
            text,
            "{\"session\":3,\"request\":41,\"began_us\":10000000,\"ended_us\":12400000,\
             \"rise_db\":21.5,\"near\":true,\"looks\":8,\"facing_height\":0.22,\
             \"facing_share\":0.75,\"facing_close\":true,\"name_score\":0.0,\"addressed\":true}"
        );
        let back: Addressed = serde_json::from_str(&text).unwrap();
        assert_eq!(back, said());
        assert!(back.check().is_ok());
        assert_eq!(Addressed::declaration().kind, Kind::Event);
    }

    #[test]
    fn like_asker_is_said_only_when_known_and_an_older_message_reads_as_none() {
        // A message from before the field, as an older voice sends it.
        let older = "{\"session\":3,\"request\":41,\"began_us\":10000000,\"ended_us\":12400000,\
             \"rise_db\":21.5,\"near\":true,\"looks\":8,\"facing_height\":0.22,\
             \"facing_share\":0.75,\"facing_close\":true,\"name_score\":0.0,\"addressed\":true}";
        let read: Addressed = serde_json::from_str(older).unwrap();
        assert_eq!(read, said());
        assert!(read.check().is_ok());
        // One with it, back as it was sent.
        let alike = Addressed {
            like_asker: Some(0.82),
            ..said()
        };
        let text = serde_json::to_string(&alike).unwrap();
        assert!(
            text.ends_with(",\"addressed\":true,\"like_asker\":0.82}"),
            "{text}"
        );
        let back: Addressed = serde_json::from_str(&text).unwrap();
        assert_eq!(back, alike);
        assert!(back.check().is_ok());
        for edge in [-1.0, 0.0, 1.0] {
            let at_edge = Addressed {
                like_asker: Some(edge),
                ..said()
            };
            assert!(at_edge.check().is_ok(), "{edge}");
        }
    }

    #[test]
    fn unknown_signs_are_none_and_never_addressed() {
        let unknown = Addressed {
            rise_db: None,
            near: None,
            looks: 0,
            facing_height: None,
            facing_share: None,
            facing_close: None,
            addressed: false,
            ..said()
        };
        assert!(unknown.check().is_ok());
        assert!(
            Addressed {
                addressed: true,
                ..unknown
            }
            .check()
            .is_err()
        );
    }

    #[test]
    fn what_does_not_add_up_is_refused() {
        let wrong = [
            Addressed {
                ended_us: Some(9_000_000),
                ..said()
            },
            Addressed {
                rise_db: Some(f32::NAN),
                ..said()
            },
            Addressed {
                rise_db: Some(130.0),
                ..said()
            },
            Addressed {
                rise_db: None,
                ..said()
            },
            Addressed {
                facing_height: Some(1.5),
                ..said()
            },
            Addressed {
                facing_height: None,
                ..said()
            },
            Addressed { looks: 0, ..said() },
            Addressed { looks: 1, ..said() },
            Addressed {
                facing_share: Some(-0.1),
                ..said()
            },
            Addressed {
                facing_share: None,
                ..said()
            },
            Addressed {
                name_score: 1.2,
                ..said()
            },
            Addressed {
                near: Some(false),
                ..said()
            },
            Addressed {
                addressed: false,
                ..said()
            },
            Addressed {
                like_asker: Some(f32::NAN),
                ..said()
            },
            Addressed {
                like_asker: Some(f32::INFINITY),
                ..said()
            },
            Addressed {
                like_asker: Some(1.01),
                ..said()
            },
            Addressed {
                like_asker: Some(-1.5),
                ..said()
            },
        ];
        for message in wrong {
            assert!(message.check().is_err(), "{message:?}");
        }
    }
}
