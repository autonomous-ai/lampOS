//! The speaker's volume on the bus (`speaker/volume`,
//! `lamp_msgs::SpeakerVolume`): said once it is first read and after each
//! call that changed it, whoever asked (the voice, the app through
//! lamp-cloud, a paired computer through lamp-control). This part is the
//! only writer of the speaker's control, so a reader sees every change
//! without reading the mixer. Nothing is read for it: it says what the last
//! call read ([`crate::sound::Volume::heard`]).
use crate::sound::Level;
use lamp_msgs::SpeakerVolume;

/// What was said last, and where to say the next.
pub struct VolumeOut {
    publisher: lamp_bus::Publisher,
    said: Said,
}

/// The part that decides, without the bus.
#[derive(Debug, Default)]
pub struct Said(Option<SpeakerVolume>);

impl Said {
    /// What to say for `heard`: the volume, when it differs from the last
    /// said; nothing before a first read.
    pub fn news(&mut self, heard: Option<Level>) -> Option<SpeakerVolume> {
        let level = heard?;
        let volume = level.volume.min(100);
        let now = SpeakerVolume {
            volume,
            muted: volume == 0,
        };
        (self.0 != Some(now)).then(|| {
            self.0 = Some(now);
            now
        })
    }
}

impl VolumeOut {
    /// The topic declared on the bus at `root`.
    pub fn on(bus: &lamp_bus::Bus) -> std::io::Result<Self> {
        let publisher = bus.publisher_declared(
            SpeakerVolume::TOPIC,
            &SpeakerVolume::declaration(),
            SpeakerVolume::SOURCE,
        )?;
        Ok(Self {
            publisher,
            said: Said::default(),
        })
    }

    /// Say `heard` if it is news. A failed publish is tried again with the
    /// next call, since it is not kept as said.
    pub fn after(&mut self, heard: Option<Level>) {
        let before = self.said.0;
        if let Some(volume) = self.said.news(heard)
            && self
                .publisher
                .publish_json(lamp_ipc::monotonic_us(), &volume)
                .is_err()
        {
            self.said.0 = before;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(volume: u8, raw: u8) -> Option<Level> {
        Some(Level {
            volume,
            raw,
            max_volume: 100,
        })
    }

    #[test]
    fn the_volume_is_said_once_read_and_again_only_when_it_changes() {
        let mut said = Said::default();
        assert_eq!(said.news(None), None);
        assert_eq!(
            said.news(level(40, 40)),
            Some(SpeakerVolume {
                volume: 40,
                muted: false
            })
        );
        assert_eq!(said.news(level(40, 40)), None);
        assert_eq!(
            said.news(level(50, 50)),
            Some(SpeakerVolume {
                volume: 50,
                muted: false
            })
        );
        assert_eq!(
            said.news(level(0, 0)),
            Some(SpeakerVolume {
                volume: 0,
                muted: true
            })
        );
        assert!(said.news(level(0, 0)).is_none());
        // Every one said passes the message's own check.
        let mut said = Said::default();
        for volume in 0..=100 {
            said.news(level(volume, volume)).unwrap().check().unwrap();
        }
    }
}
