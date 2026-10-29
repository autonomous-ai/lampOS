//! The stand a person asked for by voice (`body/stand_asked`, #4135): a
//! share of the lamp's own factory stand, taken in place of
//! `--stand-share`. Every share lies on the way the body already rises, from
//! `--folded-arm` to `--stand`, so the tipping guard applies to it as to
//! rising. Read from the topic's kept value at most once a second: a person
//! asked it, and a second is soon enough. Off unless [`ENVIRONMENT`] is `on`.

use lamp_bus::Bus;
use lamp_msgs::StandAsked;
use std::time::{Duration, Instant};

/// The switch, in the body's environment. `on` only.
pub const ENVIRONMENT: &str = "LAMP_BODY_POSE_ASK";

const LOOK_EVERY: Duration = Duration::from_secs(1);

/// The body's end of `body/stand_asked`.
pub struct StandIn {
    bus: Bus,
    looked: Option<Instant>,
    share: Option<f32>,
}

impl StandIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            share: None,
        }
    }

    /// The share asked now, 0.01 to 1.0, if any. Never waits longer than a
    /// file read.
    pub fn share(&mut self) -> Option<f32> {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            self.share = asked(&self.bus);
        }
        self.share
    }
}

/// What the kept value asks; nothing kept, unreadable or out of range asks
/// nothing, and the lamp stands as made.
fn asked(bus: &Bus) -> Option<f32> {
    bus.latest(StandAsked::TOPIC)
        .ok()
        .flatten()
        .and_then(|message| message.parse::<StandAsked>().ok())
        .filter(|asked| asked.check().is_ok())
        .map(|asked| f32::from(asked.share) / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kept_share_is_asked_until_the_next_and_an_out_of_range_one_is_not() {
        let root = std::env::temp_dir().join(format!("lamp-body-stand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut stand = StandIn::on(Bus::new(&root).unwrap());
        assert_eq!(stand.share(), None);
        let mut out = bus
            .publisher_declared(StandAsked::TOPIC, &StandAsked::declaration(), "skills")
            .unwrap();
        out.publish_json(1, &StandAsked { share: 60 }).unwrap();
        stand.looked = None;
        assert_eq!(stand.share(), Some(0.6));
        out.publish_json(2, &StandAsked { share: 101 }).unwrap();
        stand.looked = None;
        assert_eq!(stand.share(), None);
        out.publish_json(3, &StandAsked { share: 0 }).unwrap();
        stand.looked = None;
        assert_eq!(stand.share(), None);
        out.publish_json(4, &StandAsked { share: 30 }).unwrap();
        stand.looked = None;
        assert_eq!(stand.share(), Some(0.3));
        let _ = std::fs::remove_dir_all(&root);
    }
}
