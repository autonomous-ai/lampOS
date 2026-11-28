//! Caller-conditioned frame scores using the published SpeechBrain ECAPA encoder.
//! This is an offline/shadow measurement on completed PCM, including lookahead
//! inside each bounded window. The verifier's cosine cutoff is frozen at 0.25.
use crate::{
    cosine,
    ecapa::Ecapa,
    personal_vad::{MAX_FOLLOWUP, MAX_REFERENCE},
};
use std::{ops::Range, path::Path};

const FRAME: usize = 400;
const HOP: usize = 160;
const MARGIN: usize = 640;
const CONTEXT: usize = 20_480;
const STRIDE: usize = 10_240;
const MIN_ACTIVE: usize = 8;
pub const THRESHOLD: f32 = 0.25;

pub struct StrongCaller(Ecapa);
pub struct Enrollment(Vec<f32>);

struct LocalSpeaker {
    samples: Range<usize>,
    embedding: Vec<f32>,
}

pub struct Probe {
    active: Vec<bool>,
    windows: Vec<LocalSpeaker>,
}

pub struct Trace {
    pub class: &'static str,
    /// Median local speaker cosine over active frames, in [-1, 1].
    pub cosine: Option<f32>,
    /// One entry per complete 25 ms frame at a hop of 10 ms.
    pub frames: Vec<Option<f32>>,
}

struct Activity {
    active: Vec<bool>,
    trim: Range<usize>,
}

fn frame_centre(index: usize) -> usize {
    index * HOP + FRAME / 2
}

fn activity(pcm: &[f32], limit: usize) -> Option<Activity> {
    if pcm.len() > limit || pcm.iter().any(|x| !x.is_finite() || x.abs() > 1.0) {
        return None;
    }
    let powers: Vec<f32> = pcm
        .windows(FRAME)
        .step_by(HOP)
        .map(|frame| {
            let mean = frame.iter().sum::<f32>() / FRAME as f32;
            frame.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / FRAME as f32
        })
        .collect();
    if powers.len() < MIN_ACTIVE {
        return None;
    }
    let mut ordered = powers.clone();
    ordered.sort_unstable_by(f32::total_cmp);
    let percentile = |p: usize| ordered[(ordered.len() - 1) * p / 100];
    let (low, high) = (percentile(20), percentile(95));
    if high <= low * 1.585 {
        return None;
    }
    let floor = 0.0015_f32
        .powi(2)
        .max(high * 0.00631)
        .max((low * 1.995).min(high * 0.25));
    let active: Vec<_> = powers.iter().map(|p| *p > floor).collect();
    if active.iter().filter(|&&on| on).take(MIN_ACTIVE).count() < MIN_ACTIVE {
        return None;
    }
    let first = active.iter().position(|&on| on)? * HOP;
    let last = active.iter().rposition(|&on| on)? * HOP + FRAME;
    Some(Activity {
        active,
        trim: first.saturating_sub(MARGIN)..(last + MARGIN).min(pcm.len()),
    })
}

fn local_windows(trim: Range<usize>) -> Vec<Range<usize>> {
    let mut windows = Vec::new();
    let mut start = trim.start;
    loop {
        let end = (start + CONTEXT).min(trim.end);
        windows.push(start..end);
        if end == trim.end {
            return windows;
        }
        start = (start + STRIDE).min(trim.end - CONTEXT);
    }
}

impl StrongCaller {
    pub fn load(path: &Path) -> Result<Self, String> {
        Ecapa::load(path).map(Self)
    }

    pub fn enroll(&self, wake: &[f32]) -> Result<Option<Enrollment>, String> {
        let Some(activity) = activity(wake, MAX_REFERENCE) else {
            return Ok(None);
        };
        self.0.embed(&wake[activity.trim]).map(Enrollment).map(Some)
    }

    pub fn prepare_probe(&self, line: &[f32]) -> Result<Option<Probe>, String> {
        let Some(activity) = activity(line, MAX_FOLLOWUP) else {
            return Ok(None);
        };
        let mut windows = Vec::new();
        for samples in local_windows(activity.trim) {
            if activity
                .active
                .iter()
                .enumerate()
                .any(|(i, &on)| on && samples.contains(&frame_centre(i)))
            {
                windows.push(LocalSpeaker {
                    embedding: self.0.embed(&line[samples.clone()])?,
                    samples,
                });
            }
        }
        Ok(Some(Probe {
            active: activity.active,
            windows,
        }))
    }

    /// The live worker retains no speaker embeddings between requests.
    pub fn pair(&self, wake: &[f32], line: &[f32]) -> Result<Trace, String> {
        let enrollment = self.enroll(wake)?;
        let probe = if enrollment.is_some() {
            self.prepare_probe(line)?
        } else {
            None
        };
        Ok(decide(enrollment.as_ref(), probe.as_ref()))
    }
}

pub fn decide(caller: Option<&Enrollment>, probe: Option<&Probe>) -> Trace {
    let (Some(caller), Some(probe)) = (caller, probe) else {
        return Trace {
            class: "unknown",
            cosine: None,
            frames: Vec::new(),
        };
    };
    let scores: Vec<_> = probe
        .windows
        .iter()
        .map(|w| (&w.samples, cosine(&caller.0, &w.embedding)))
        .collect();
    let frames: Vec<Option<f32>> = probe
        .active
        .iter()
        .enumerate()
        .map(|(i, &active)| {
            if !active {
                return None;
            }
            let centre = frame_centre(i);
            let (mut sum, mut count) = (0.0, 0u32);
            for (window, score) in &scores {
                if window.contains(&centre) {
                    sum += (*score)?;
                    count += 1;
                }
            }
            (count != 0).then(|| sum / count as f32)
        })
        .collect();
    let mut voiced: Vec<_> = frames.iter().copied().flatten().collect();
    let score = (voiced.len() >= MIN_ACTIVE).then(|| {
        voiced.sort_unstable_by(f32::total_cmp);
        (voiced[(voiced.len() - 1) / 2] + voiced[voiced.len() / 2]) * 0.5
    });
    let class = match score {
        Some(value) if value > THRESHOLD => "same",
        Some(_) => "different",
        None => "unknown",
    };
    Trace {
        class,
        cosine: score,
        frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_cosine_cutoff_is_strict_and_seven_frames_abstain() {
        let mut identity = vec![0.0; 16];
        identity[0] = 1.0;
        let caller = Enrollment(identity);
        let mut probe = Probe {
            active: vec![true; 8],
            windows: vec![LocalSpeaker {
                samples: 0..2048,
                embedding: vec![1.0; 16],
            }],
        };
        let score = decide(Some(&caller), Some(&probe));
        assert_eq!((score.cosine, score.class), (Some(0.25), "different"));
        probe.windows[0].embedding[0] = 1.001;
        assert_eq!(decide(Some(&caller), Some(&probe)).class, "same");
        probe.active[0] = false;
        assert_eq!(decide(Some(&caller), Some(&probe)).class, "unknown");
    }

    #[test]
    fn each_frame_uses_its_local_windows_and_the_actual_caller() {
        let probe = Probe {
            active: (0..20).map(|i| i != 6).collect(),
            windows: vec![
                LocalSpeaker {
                    samples: 0..frame_centre(13),
                    embedding: vec![1.0, 0.0],
                },
                LocalSpeaker {
                    samples: frame_centre(8)..frame_centre(20),
                    embedding: vec![0.0, 1.0],
                },
            ],
        };
        for (caller, first, last) in [(vec![1.0, 0.0], 1.0, 0.0), (vec![0.0, 1.0], 0.0, 1.0)] {
            let trace = decide(Some(&Enrollment(caller)), Some(&probe));
            assert_eq!(
                (
                    trace.frames[0],
                    trace.frames[6],
                    trace.frames[8],
                    trace.frames[19]
                ),
                (Some(first), None, Some(0.5), Some(last))
            );
            assert_eq!((trace.cosine, trace.class), (Some(0.5), "same"));
        }
        let zero = decide(Some(&Enrollment(vec![0.0, 0.0])), Some(&probe));
        assert_eq!((zero.cosine, zero.class), (None, "unknown"));
        assert_eq!(decide(None, Some(&probe)).class, "unknown");
    }

    #[test]
    fn bounds_noise_floor_and_short_windows() {
        let mut pcm = vec![0.0; 16_000];
        assert!(activity(&pcm, MAX_REFERENCE).is_none());
        pcm.fill(0.4);
        assert!(activity(&pcm, MAX_REFERENCE).is_none());
        for (i, sample) in pcm[6400..9600].iter_mut().enumerate() {
            *sample = if i % 20 < 10 { 0.2 } else { -0.2 };
        }
        let a = activity(&pcm, MAX_REFERENCE).unwrap();
        assert!(!a.active[5] && a.active[45]);
        assert!(a.trim.start < 6400 && a.trim.end > 9600);
        assert_eq!(local_windows(a.trim.clone()), vec![a.trim]);
        assert_eq!(
            local_windows(1000..45_000),
            vec![
                1_000..21_480,
                11_240..31_720,
                21_480..41_960,
                24_520..45_000
            ]
        );
        assert!(activity(&pcm, 2000).is_none());
        pcm[200] = f32::NAN;
        assert!(activity(&pcm, MAX_REFERENCE).is_none());
    }
}
