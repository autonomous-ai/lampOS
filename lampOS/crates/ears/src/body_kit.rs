//! The kit that measures the body sounds' bars (#7685, `lamp-ears-body-kit`,
//! steps in docs/body-sounds-kit.md): clips of a consenting person's own
//! sounds scored patch by patch with the publisher's own frontend and model
//! ([`score_clip`]), an hour of office scores logged with no sound
//! ([`OfficeRow`]), and from both a suggested bar per class ([`suggest`]),
//! checked with the publisher's own detector ([`Detector`]).
use crate::body_sounds::{BARS, Bar, Detector};
use crate::classes::{Frontend, Model, PATCH_HOP, bounds};
use lamp_msgs::{BodyClass, OwnVoiceEvidence, SoundScores};
use std::fmt::Write;

/// The labels of the person's own body sounds, as a clip's file name starts.
pub fn class_of(label: &str) -> Option<BodyClass> {
    BodyClass::ALL
        .into_iter()
        .find(|class| class.name() == label)
}

/// The labels of the other sounds the session asks for (60 in all). Any
/// other plain label is taken as another sound too.
pub const OTHERS: [&str; 6] = [
    "speech",
    "laughter",
    "lamp_voice",
    "typing",
    "thump",
    "chair",
];

/// A label is a plain name: 1 to 24 lowercase letters, digits or `_`.
pub fn plain_label(label: &str) -> bool {
    (1..=24).contains(&label.len())
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn score_of(scores: &SoundScores, class: BodyClass) -> f32 {
    match class {
        BodyClass::Cough => scores.cough,
        BodyClass::ThroatClear => scores.throat_clear,
        BodyClass::Sneeze => scores.sneeze,
        BodyClass::Sniff => scores.sniff,
        BodyClass::Wheeze => scores.wheeze,
    }
}

/// Each patch of a clip (16 kHz mono, -1 to 1) scored as `lamp-ears` scores
/// it, every 500 ms: the patch's end in seconds and its scores. `model`
/// must score the body sounds ([`Model::with_body_sounds`]).
pub fn score_clip(
    model: &Model,
    frontend: &Frontend,
    pcm: &[f32],
) -> Result<Vec<(f32, SoundScores)>, String> {
    let mut patches = Vec::new();
    let mut first_frame = 0;
    while bounds(first_frame).end_sample <= pcm.len() {
        let window = bounds(first_frame);
        let mut features = frontend.features(pcm, 0, window)?;
        let scores = model.score(&features);
        features.fill(0.0);
        patches.push((window.end_sample as f32 / crate::RATE as f32, scores?));
        first_frame += PATCH_HOP;
    }
    Ok(patches)
}

/// A clip's scores file, beside its sound: the header, then a line a patch.
pub const CLIP_HEADER: &str = "end_s,cough,throat_clear,sneeze,sniff,wheeze";

pub fn clip_csv(patches: &[(f32, SoundScores)]) -> String {
    let mut out = format!("{CLIP_HEADER}\n");
    for (end_s, scores) in patches {
        let _ = write!(out, "{end_s:.2}");
        for class in BodyClass::ALL {
            let _ = write!(out, ",{:.4}", score_of(scores, class));
        }
        out.push('\n');
    }
    out
}

/// One scored clip: its label and each patch's five body scores.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub label: String,
    pub patches: Vec<[f32; 5]>,
}

impl Clip {
    /// Its scores file's text, its label from the file name (`cough-07`).
    pub fn parse(label: &str, text: &str) -> Result<Self, String> {
        let mut patches = Vec::new();
        for line in text
            .lines()
            .filter(|line| !line.is_empty() && *line != CLIP_HEADER)
        {
            let cells: Vec<&str> = line.split(',').collect();
            let [_, scores @ ..] = cells.as_slice() else {
                return Err(format!("{label}: not a scores line: {line}"));
            };
            let scores: Vec<f32> = scores
                .iter()
                .map(|cell| cell.parse::<f32>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("{label}: not a scores line: {line}"))?;
            patches.push(
                scores
                    .try_into()
                    .map_err(|_| format!("{label}: not five scores: {line}"))?,
            );
        }
        Ok(Self {
            label: label.to_owned(),
            patches,
        })
    }

    /// Its highest score for `class` over every patch.
    pub fn peak(&self, class: BodyClass) -> f32 {
        let index = slot(class);
        self.patches
            .iter()
            .map(|patch| patch[index])
            .fold(0.0, f32::max)
    }
}

fn slot(class: BodyClass) -> usize {
    BodyClass::ALL
        .iter()
        .position(|&one| one == class)
        .expect("every class is in ALL")
}

/// One patch of the office hour: scores only, no sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OfficeRow {
    pub end_us: u64,
    pub own: bool,
    pub scores: [f32; 5],
}

pub const OFFICE_HEADER: &str = "end_us,own_voice,cough,throat_clear,sneeze,sniff,wheeze";

impl OfficeRow {
    pub fn of(end_us: u64, own_voice: OwnVoiceEvidence, scores: &SoundScores) -> Self {
        Self {
            end_us,
            own: own_voice == OwnVoiceEvidence::Present,
            scores: BodyClass::ALL.map(|class| score_of(scores, class)),
        }
    }

    pub fn csv(&self) -> String {
        let scores = self.scores.map(|score| format!("{score:.4}")).join(",");
        format!(
            "{},{},{scores}",
            self.end_us,
            if self.own { "present" } else { "unknown" }
        )
    }

    pub fn parse(line: &str) -> Result<Option<Self>, String> {
        if line.is_empty() || line == OFFICE_HEADER {
            return Ok(None);
        }
        let said = || format!("not an office line: {line}");
        let cells: Vec<&str> = line.split(',').collect();
        let [end_us, own, scores @ ..] = cells.as_slice() else {
            return Err(said());
        };
        let scores: Vec<f32> = scores
            .iter()
            .map(|cell| cell.parse::<f32>())
            .collect::<Result<_, _>>()
            .map_err(|_| said())?;
        Ok(Some(Self {
            end_us: end_us.parse().map_err(|_| said())?,
            own: *own == "present",
            scores: scores.try_into().map_err(|_| said())?,
        }))
    }
}

/// How often the publisher's own detector would tell `class` over the
/// office hour at `bar`, and the hour's length in seconds (first patch's end
/// to last's).
pub fn office_alarms(rows: &[OfficeRow], class: BodyClass, bar: Bar) -> (usize, f64) {
    let mut bars = BARS;
    for (one, set) in &mut bars {
        *set = (*one == class).then_some(bar);
    }
    let mut detector = Detector::with_bars(bars);
    let mut told = 0;
    for row in rows {
        let mut scores = SoundScores::default();
        scores.cough = row.scores[0];
        scores.throat_clear = row.scores[1];
        scores.sneeze = row.scores[2];
        scores.sniff = row.scores[3];
        scores.wheeze = row.scores[4];
        let own = if row.own {
            OwnVoiceEvidence::Present
        } else {
            OwnVoiceEvidence::Unknown
        };
        told += detector.feed(&scores, own, row.end_us).len();
    }
    let seconds = match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => last.end_us.saturating_sub(first.end_us) as f64 / 1e6,
        _ => 0.0,
    };
    (told, seconds)
}

/// The bar suggested for one class, and how it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub class: BodyClass,
    /// None: no clip of the class, or none of the others to set it against.
    pub bar: Option<Bar>,
    /// The person's own clips of it, and those at or over the bar.
    pub own: usize,
    pub hits: usize,
    /// Clips of the other body sounds over it (a sneeze taken for a cough).
    pub cross: usize,
    /// The other sounds' clips over it: none, by its choice.
    pub others: usize,
    /// Over the office hour: times told, and the hour's seconds.
    pub office: Option<(usize, f64)>,
}

impl Suggestion {
    pub fn recall(&self) -> f64 {
        if self.own == 0 {
            0.0
        } else {
            self.hits as f64 / self.own as f64
        }
    }

    /// The share of clips over the bar that are the class's own.
    pub fn precision(&self) -> f64 {
        let over = self.hits + self.cross + self.others;
        if over == 0 {
            0.0
        } else {
            self.hits as f64 / over as f64
        }
    }

    pub fn alarms_an_hour(&self) -> Option<f64> {
        self.office
            .map(|(told, seconds)| told as f64 * 3600.0 / seconds.max(1.0))
    }

    /// The published rule (docs/ears.md, "Body sounds"): at least 9 of 10
    /// of its own, and under one false alarm an hour over the office hour.
    pub fn publishable(&self) -> bool {
        self.bar.is_some()
            && self.recall() >= 0.9
            && self.alarms_an_hour().is_some_and(|rate| rate < 1.0)
    }
}

/// The lowest bar, in steps of 0.01, over every other sound's peak for the
/// class: no false hit on the 60 others, as much of the person's own as that
/// leaves. Its refractory is the most patches (500 ms each) any own clip
/// holds over 70% of the bar (the detector's re-arm), at least a second.
pub fn suggest(clips: &[Clip], office: Option<&[OfficeRow]>) -> Vec<Suggestion> {
    BodyClass::ALL
        .into_iter()
        .map(|class| {
            let own: Vec<&Clip> = clips
                .iter()
                .filter(|clip| class_of(&clip.label) == Some(class))
                .collect();
            let others: Vec<&Clip> = clips
                .iter()
                .filter(|clip| class_of(&clip.label).is_none())
                .collect();
            let cross: Vec<&Clip> = clips
                .iter()
                .filter(|clip| class_of(&clip.label).is_some_and(|other| other != class))
                .collect();
            let bar = (!own.is_empty() && !others.is_empty()).then(|| {
                let highest = others
                    .iter()
                    .map(|clip| clip.peak(class))
                    .fold(0.0, f32::max);
                let bar = ((highest * 100.0).floor() + 1.0).min(100.0) / 100.0;
                let index = slot(class);
                let longest = own
                    .iter()
                    .map(|clip| {
                        clip.patches
                            .iter()
                            .filter(|patch| patch[index] >= bar * 0.7)
                            .count()
                    })
                    .max()
                    .unwrap_or(0);
                Bar {
                    bar,
                    refractory_us: (longest as u64 * 500_000).max(1_000_000),
                }
            });
            let over = |clips: &[&Clip]| {
                bar.map_or(0, |bar| {
                    clips
                        .iter()
                        .filter(|clip| clip.peak(class) >= bar.bar)
                        .count()
                })
            };
            Suggestion {
                class,
                bar,
                own: own.len(),
                hits: over(&own),
                cross: over(&cross),
                others: over(&others),
                office: bar.and_then(|bar| office.map(|rows| office_alarms(rows, class, bar))),
            }
        })
        .collect()
}

/// The suggestions as a table, then `body_sounds::BARS` as it would read.
pub fn report(clips: &[Clip], suggestions: &[Suggestion]) -> String {
    let others = clips
        .iter()
        .filter(|clip| class_of(&clip.label).is_none())
        .count();
    let mut out = format!(
        "{} clips: {} of the person's body sounds, {others} other sounds\n",
        clips.len(),
        clips.len() - others
    );
    out.push_str(
        "class         bar   refr_s  recall        precision  cross  others  office/h  publish\n",
    );
    for one in suggestions {
        let bar = one
            .bar
            .map_or("-".to_owned(), |bar| format!("{:.2}", bar.bar));
        let refractory = one.bar.map_or("-".to_owned(), |bar| {
            format!("{:.1}", bar.refractory_us as f64 / 1e6)
        });
        let office = one
            .alarms_an_hour()
            .map_or("-".to_owned(), |rate| format!("{rate:.2}"));
        let _ = writeln!(
            out,
            "{:<13} {bar:<5} {refractory:<7} {:<13} {:<10.2} {:<6} {:<7} {office:<9} {}",
            one.class.name(),
            format!("{}/{} {:.2}", one.hits, one.own, one.recall()),
            one.precision(),
            one.cross,
            one.others,
            if one.publishable() { "yes" } else { "no" }
        );
    }
    out.push_str("\npub const BARS: [(BodyClass, Option<Bar>); 5] = [\n");
    for one in suggestions {
        let variant = match one.class {
            BodyClass::Cough => "Cough",
            BodyClass::ThroatClear => "ThroatClear",
            BodyClass::Sneeze => "Sneeze",
            BodyClass::Sniff => "Sniff",
            BodyClass::Wheeze => "Wheeze",
        };
        match one.bar.filter(|_| one.publishable()) {
            Some(bar) => {
                let _ = writeln!(
                    out,
                    "    (BodyClass::{variant}, Some(Bar {{ bar: {:.2}, refractory_us: {} }})),",
                    bar.bar, bar.refractory_us
                );
            }
            None => {
                let _ = writeln!(out, "    (BodyClass::{variant}, None),");
            }
        }
    }
    out.push_str("];\n");
    out
}

#[cfg(test)]
mod tests;
