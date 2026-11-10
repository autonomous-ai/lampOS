//! What a person does in front of the lamp, made by hand, so that the
//! examples that react can run on the virtual lamp: the air, a hand, words
//! heard. Each is a file in the lamp's folder that `lamp-sim air|hand|hear`
//! writes and the running lamp takes (and removes) within 200 ms, then says
//! on the bus as the part that senses it would. No model runs.
use lamp_bus::{Bus, Kind, Publisher};
use lamp_msgs::{
    AirFeel, AirState, OwnVoiceEvidence, SoundClasses, SoundScores, SoundWindow, Speech,
};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The sound classes lamp-control reads from `ears/classes`.
pub const SOUND_CLASSES: [&str; 4] = ["music", "laughter", "door", "footsteps"];
/// A sound set by hand is said this often, and for this long.
const SOUND_EVERY: Duration = Duration::from_millis(500);
const SOUND_FOR: Duration = Duration::from_millis(1500);

/// The ears' message with `class` scored 0.9 (`heard`) or nothing heard.
fn sound_scores(class: &str, heard: bool, now_us: u64) -> SoundClasses {
    let at = |name: &str| if heard && name == class { 0.9 } else { 0.0 };
    SoundClasses::Scored {
        model_id: "lamp-sim".to_owned(),
        window: SoundWindow {
            start_us: now_us.saturating_sub(960_000),
            end_us: now_us,
            completed_us: now_us,
            capture_epoch: 1,
        },
        scores: SoundScores {
            speech: 0.0,
            music: at("music"),
            laughter: at("laughter"),
            door: at("door"),
            footsteps: at("footsteps"),
        },
        own_voice: OwnVoiceEvidence::Clear,
    }
}

/// The air is said this often while a reading is set, as the sensor does.
const AIR_EVERY: Duration = Duration::from_secs(5);
/// A hand wave: frames a second and how long.
const HAND_FRAMES_A_SECOND: u64 = 10;
const WAVE_FRAMES: u64 = 15;

pub struct Stimuli {
    dir: PathBuf,
    heard: Publisher,
    hands: Publisher,
    sounds: Publisher,
    sound_on: Option<(String, Instant)>,
    sound_said: Option<Instant>,
    air: Publisher,
    air_set: Option<AirState>,
    air_said: Option<Instant>,
    wave: Option<(u64, Instant)>,
}

impl Stimuli {
    pub fn on(bus: &Bus, dir: &Path) -> io::Result<Stimuli> {
        Ok(Stimuli {
            dir: dir.to_owned(),
            heard: bus.publisher(Speech::TOPICS[0], Kind::Event, "voice")?,
            hands: bus.publisher("humans/hands/tracked", Kind::Event, "eyes")?,
            sounds: bus.publisher_declared(
                SoundClasses::TOPIC,
                &SoundClasses::declaration(),
                "ears",
            )?,
            sound_on: None,
            sound_said: None,
            air: bus.publisher_declared(AirState::TOPIC, &AirState::declaration(), "air")?,
            air_set: None,
            air_said: None,
            wave: None,
        })
    }

    /// One look, a few times a second.
    pub fn step(&mut self) -> io::Result<()> {
        let now_us = lamp_ipc::monotonic_us();
        if let Some(text) = take(&self.dir.join("hear")) {
            let words = Speech {
                text: text.trim().to_owned(),
                finished: true,
                locale: None,
                request: None,
            };
            self.heard.publish_json(now_us, &words)?;
        }
        if let Some(class) = take(&self.dir.join("sound")) {
            let class = class.trim().to_owned();
            if SOUND_CLASSES.contains(&class.as_str()) {
                self.sound_on = Some((class, Instant::now()));
                self.sound_said = None;
            } else {
                // A file written by hand: it would score every class zero, silently.
                eprintln!(
                    "lamp-sim: sound {class:?} is not one of {SOUND_CLASSES:?}; nothing heard"
                );
            }
        }
        // The class is heard for 1.5 s (scored 0.9, said every 500 ms as the ears
        // do), then said gone, so that the next one is news again.
        if let Some((class, since)) = self.sound_on.clone()
            && self.sound_said.is_none_or(|at| at.elapsed() >= SOUND_EVERY)
        {
            let over = since.elapsed() >= SOUND_FOR;
            self.sounds
                .publish_json(now_us, &sound_scores(&class, !over, now_us))?;
            self.sound_said = Some(Instant::now());
            if over {
                self.sound_on = None;
            }
        }
        if take(&self.dir.join("hand")).is_some() {
            self.wave = Some((0, Instant::now()));
        }
        if let Some((frame, started)) = self.wave {
            let due = started + Duration::from_millis(frame * 1_000 / HAND_FRAMES_A_SECOND);
            if Instant::now() >= due {
                // The palm sweeps left and right across the picture, open.
                let x = 0.5 + 0.2 * (frame as f64 * 0.9).sin();
                self.hands.publish_json(
                    now_us,
                    &json!({"x": x, "y": 0.5, "size": 0.2, "open": true, "confidence": 0.95}),
                )?;
                self.wave = (frame + 1 < WAVE_FRAMES).then_some((frame + 1, started));
            }
        }
        if let Some(text) = take(&self.dir.join("air")) {
            self.air_set = air_of(&text);
            self.air_said = None;
        }
        if let Some(air) = self.air_set
            && self.air_said.is_none_or(|at| at.elapsed() >= AIR_EVERY)
        {
            self.air.publish_json(now_us, &air)?;
            self.air_said = Some(Instant::now());
        }
        Ok(())
    }
}

/// The file's text, and the file removed; none if there is none.
fn take(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(text)
}

/// A reading from `key=value` words (`pm2_5=41 co2_ppm=1350`); the rest of
/// the sensor is unknown. None for nothing readable.
pub fn air_of(text: &str) -> Option<AirState> {
    let mut values = serde_json::Map::new();
    for word in text.split_whitespace() {
        let (key, value) = word.split_once('=')?;
        values.insert(key.to_owned(), json!(value.parse::<f64>().ok()?));
    }
    let number = |key: &str| values.get(key).and_then(Value::as_f64);
    let co2_ppm = number("co2_ppm").map(|v| v.clamp(0.0, 40_000.0) as u16);
    let air = AirState {
        pm1_0: number("pm1_0").map(|v| v as f32),
        pm2_5: number("pm2_5").map(|v| v as f32),
        pm4_0: number("pm4_0").map(|v| v as f32),
        pm10: number("pm10").map(|v| v as f32),
        humidity: number("humidity").map(|v| v as f32),
        temperature: number("temperature").map(|v| v as f32),
        co2_ppm,
        feel: AirFeel::of_co2(co2_ppm),
    };
    air.check().ok()?;
    (!values.is_empty()).then_some(air)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_set_by_hand_is_a_valid_message_that_scores_only_its_class() {
        for class in SOUND_CLASSES {
            let heard = sound_scores(class, true, 5_000_000);
            assert!(heard.check().is_ok(), "{class}");
            let json = serde_json::to_value(&heard).unwrap();
            for other in SOUND_CLASSES {
                let score = json["scores"][other].as_f64().unwrap();
                assert_eq!(score > 0.5, other == class, "{class} / {other}");
            }
            let gone = serde_json::to_value(sound_scores(class, false, 6_000_000)).unwrap();
            assert_eq!(gone["scores"][class].as_f64(), Some(0.0));
        }
    }

    #[test]
    fn a_reading_is_words_and_its_feel_follows_the_carbon_dioxide() {
        let air = air_of("pm2_5=41 co2_ppm=1350").unwrap();
        assert_eq!(air.pm2_5, Some(41.0));
        assert_eq!(air.co2_ppm, Some(1350));
        assert_eq!(air.feel, AirFeel::Stuffy);
        assert!(air.humidity.is_none());
    }

    #[test]
    fn nonsense_or_nothing_is_no_reading() {
        assert!(air_of("").is_none());
        assert!(air_of("co2_ppm=lots").is_none());
        assert!(air_of("humidity=900").is_none());
    }
}
