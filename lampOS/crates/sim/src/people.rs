//! The eyes' stand-in for who is in view (`humans/faces/tracked`). No face
//! model runs in v1: the count is the number in `DIR/people` (a viewer's
//! button, a test, or a person editing it), one if the file is absent, said
//! several times a second as the eyes do, so that the room counts as seen.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{Face, Faces};
use std::{io, path::Path};

pub struct People {
    faces: Publisher,
    file: std::path::PathBuf,
}

impl People {
    pub fn on(bus: &Bus, dir: &Path) -> io::Result<People> {
        Ok(People {
            faces: bus.publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")?,
            file: dir.join("people"),
        })
    }

    /// The count now, from the file.
    pub fn count(&self) -> usize {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(1)
            .min(8)
    }

    /// Says who is in view, once.
    pub fn say(&mut self) -> io::Result<()> {
        let faces = (0..self.count())
            .map(|k| Face {
                id: format!("sim{k}"),
                xmin: 0.3,
                ymin: 0.2,
                xmax: 0.7,
                ymax: 0.8,
                confidence: 0.99,
                yaw: Some(0.0),
                facing: Some(true),
                speaking: None,
                follows: None,
            })
            .collect();
        self.faces
            .publish_json(lamp_ipc::monotonic_us(), &Faces { faces })?;
        Ok(())
    }
}
