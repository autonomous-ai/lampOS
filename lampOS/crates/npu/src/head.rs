//! A pose model's head: its raw outputs read as one person's 17 COCO
//! keypoints, in the model's frame's pixels, and a score for the person.
//! Which head a model has is its manifest's `head`, so a model is swapped
//! by the Store without a lampOS release, and a new kind of model is one
//! more [`PoseHead`].
//!
//! - `movenet`: MoveNet SinglePose (Lightning 192x192, Thunder 256x256),
//!   Apache-2.0, converted for the NPU before its decode: four maps on a
//!   G x G grid (48 for Lightning), channels last, in this order: the
//!   person's centre (1), the keypoints' heat (17), their offsets in the
//!   cell (34, `y, x` each) and their distance from the centre (34). They
//!   are read as the model's own float graph decodes them (checked against
//!   its output, `tests/fixtures/movenet/make.py`); the person's score is the
//!   keypoints' mean. The next frame is cropped around the person found
//!   ([`PoseHead::next`], the MoveNet tutorial's crop).
//! - `yolov8-pose`: YOLOv8-pose cut before its decode, as `npu/convert.sh`
//!   cuts it (box 64 / class 1 / keypoints 51 at strides 8, 16 and 32, nine
//!   outputs in that order). The most confident anchor is the person, as the
//!   reference binding decodes it; one person is answered, so no
//!   suppression of others is needed. Kept for comparison only: its weights
//!   are AGPL-3.0 and no lamp is sent them (#7415).
use crate::{
    engine::{Input, Tensor},
    image::Region,
};

/// The COCO keypoints, in their order.
pub const KEYPOINTS: [&str; 17] = [
    "nose",
    "left_eye",
    "right_eye",
    "left_ear",
    "right_ear",
    "left_shoulder",
    "right_shoulder",
    "left_elbow",
    "right_elbow",
    "left_wrist",
    "right_wrist",
    "left_hip",
    "right_hip",
    "left_knee",
    "right_knee",
    "left_ankle",
    "right_ankle",
];

/// The heads this build reads.
pub const HEADS: [&str; 2] = ["movenet", "yolov8-pose"];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Keypoint {
    pub x: f32,
    pub y: f32,
    pub score: f32,
}

/// One person found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// 0 to 1.
    pub score: f32,
    pub keypoints: [Keypoint; 17],
}

pub trait PoseHead: Send + Sync {
    /// Its manifest name, one of [`HEADS`].
    fn name(&self) -> &'static str;
    /// The person in `outputs`, for a frame of `input`: keypoints in the
    /// frame's pixels. `Err` when the outputs are not this head's.
    fn decode(&self, outputs: &[Tensor], input: Input) -> Result<Found, String>;
    /// Where to look for `found` in the next frame from the same client,
    /// its keypoints in the picture's pixels (`width` x `height`); `None`,
    /// the whole picture.
    fn next(&self, _found: &Found, _width: usize, _height: usize) -> Option<Region> {
        None
    }
}

/// The head named `name`.
pub fn named(name: &str) -> Option<Box<dyn PoseHead>> {
    match name {
        "movenet" => Some(Box::new(MoveNet)),
        "yolov8-pose" => Some(Box::new(Yolov8Pose)),
        _ => None,
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// MoveNet SinglePose.
pub struct MoveNet;

/// The decode's distance softening, the model's own.
const MOVENET_EPS: f32 = 1.799_804_7;
/// A keypoint scored over this is seen, for the crop.
const SEEN: f32 = 0.2;
/// The torso: shoulders and hips.
const TORSO: [usize; 4] = [5, 6, 11, 12];

impl PoseHead for MoveNet {
    fn name(&self) -> &'static str {
        "movenet"
    }

    fn decode(&self, outputs: &[Tensor], input: Input) -> Result<Found, String> {
        let [centre, heat, offset, regress] = outputs else {
            return Err(format!(
                "a MoveNet model has four outputs, this one {}",
                outputs.len()
            ));
        };
        let cells = centre.len();
        let g = (cells as f64).sqrt().round() as usize;
        if g == 0
            || g * g != cells
            || heat.len() != 17 * cells
            || offset.len() != 34 * cells
            || regress.len() != 34 * cells
        {
            return Err(format!(
                "MoveNet's outputs are G x G maps of 1, 17, 34 and 34, these {}, {}, {} and {}",
                centre.len(),
                heat.len(),
                offset.len(),
                regress.len()
            ));
        }
        let distance = |y: f32, x: f32, cy: f32, cx: f32| {
            ((y - cy) * (y - cy) + (x - cx) * (x - cx)).sqrt() + MOVENET_EPS
        };
        // The person: the centre map times MoveNet's centre weight,
        // `1 / (distance from the frame's middle + 1.8)`, as the float
        // graph's own decode does it, so of two people the one nearer the
        // middle wins (Heron's P2 1 on #7972). The first of equals, as the
        // reference's argmax.
        let middle = g as f32 / 2.0;
        let mut best = (f32::MIN, 0);
        for i in 0..cells {
            let (y, x) = ((i / g) as f32, (i % g) as f32);
            let weighed = centre.at(i) / distance(y, x, middle, middle);
            if weighed > best.0 {
                best = (weighed, i);
            }
        }
        let (cy, cx) = (best.1 / g, best.1 % g);
        let mut keypoints = [Keypoint::default(); 17];
        for (k, keypoint) in keypoints.iter_mut().enumerate() {
            // Where the centre says keypoint k is; the most heat near it.
            let at = (cy * g + cx) * 34 + 2 * k;
            let ry = cy as f32 + regress.at(at);
            let rx = cx as f32 + regress.at(at + 1);
            let mut most = (f32::MIN, 0);
            for i in 0..cells {
                let (y, x) = ((i / g) as f32, (i % g) as f32);
                let weighed = heat.at(i * 17 + k) / distance(y, x, ry, rx);
                if weighed > most.0 {
                    most = (weighed, i);
                }
            }
            let i = most.1;
            let y = ((i / g) as f32 + offset.at(i * 34 + 2 * k)) / g as f32;
            let x = ((i % g) as f32 + offset.at(i * 34 + 2 * k + 1)) / g as f32;
            *keypoint = Keypoint {
                x: x * input.width as f32,
                y: y * input.height as f32,
                score: heat.at(i * 17 + k).clamp(0.0, 1.0),
            };
        }
        let score = keypoints.iter().map(|k| k.score).sum::<f32>() / 17.0;
        Ok(Found { score, keypoints })
    }

    /// The MoveNet tutorial's `determine_crop_region`: a square on the
    /// hips' middle, wide enough for the torso and every keypoint seen;
    /// the whole picture when the torso is not seen or the square would be
    /// wider than the picture.
    fn next(&self, found: &Found, width: usize, height: usize) -> Option<Region> {
        let k = &found.keypoints;
        let seen = |i: usize| k[i].score > SEEN;
        if !((seen(11) || seen(12)) && (seen(5) || seen(6))) {
            return None;
        }
        let (cy, cx) = ((k[11].y + k[12].y) / 2.0, (k[11].x + k[12].x) / 2.0);
        let reach = |p: &Keypoint| (cy - p.y).abs().max((cx - p.x).abs());
        let torso = TORSO.iter().map(|&i| reach(&k[i])).fold(0.0, f32::max);
        let body = (0..17)
            .filter(|&i| seen(i))
            .map(|i| reach(&k[i]))
            .fold(0.0, f32::max);
        let (w, h) = (width as f32, height as f32);
        let half = (torso * 1.9)
            .max(body * 1.2)
            .min(cx.max(w - cx).max(cy).max(h - cy));
        // Past the reference: a square under a pixel is no crop.
        if !half.is_finite() || half < 1.0 || half > w.max(h) / 2.0 {
            return None;
        }
        Some(Region {
            y0: (cy - half) / h,
            x0: (cx - half) / w,
            y1: (cy + half) / h,
            x1: (cx + half) / w,
        })
    }
}

/// MoveNet's four maps on a `g` x `g` grid for a person whose keypoints
/// are `points` (`y, x, score`, normalised), centred mid-grid: what a fake
/// NPU answers.
pub fn movenet_maps(points: &[(f32, f32, f32); 17], g: usize) -> Vec<Tensor> {
    let cells = g * g;
    let (mut centre, mut heat) = (vec![0f32; cells], vec![0f32; cells * 17]);
    let (mut offset, mut regress) = (vec![0f32; cells * 34], vec![0f32; cells * 34]);
    let middle = (g / 2) * g + g / 2;
    centre[middle] = 0.9;
    for (k, (y, x, score)) in points.iter().enumerate() {
        let (gy, gx) = (y * g as f32, x * g as f32);
        let (ky, kx) = (
            gy.floor().clamp(0.0, g as f32 - 1.0),
            gx.floor().clamp(0.0, g as f32 - 1.0),
        );
        let i = ky as usize * g + kx as usize;
        heat[i * 17 + k] = *score;
        offset[i * 34 + 2 * k] = gy - ky;
        offset[i * 34 + 2 * k + 1] = gx - kx;
        regress[middle * 34 + 2 * k] = ky - (g / 2) as f32;
        regress[middle * 34 + 2 * k + 1] = kx - (g / 2) as f32;
    }
    let map = |values: &[f32], channels: usize| Tensor::from_f32(vec![1, g, g, channels], values);
    vec![
        map(&centre, 1),
        map(&heat, 17),
        map(&offset, 34),
        map(&regress, 34),
    ]
}

/// YOLOv8-pose, cut before its decode.
pub struct Yolov8Pose;

const STRIDES: [usize; 3] = [8, 16, 32];
const DFL_BINS: usize = 16;

impl PoseHead for Yolov8Pose {
    fn name(&self) -> &'static str {
        "yolov8-pose"
    }

    fn decode(&self, outputs: &[Tensor], input: Input) -> Result<Found, String> {
        if outputs.len() != 9 {
            return Err(format!(
                "a YOLOv8-pose model cut for the NPU has 9 outputs, this one {}",
                outputs.len()
            ));
        }
        // The most confident anchor over the three strides.
        let mut best: Option<(f32, usize, usize)> = None;
        for (level, stride) in STRIDES.iter().enumerate() {
            let anchors = (input.width / stride) * (input.height / stride);
            let (boxes, class, points) = (
                &outputs[3 * level],
                &outputs[3 * level + 1],
                &outputs[3 * level + 2],
            );
            if class.len() != anchors || boxes.len() != 64 * anchors || points.len() != 51 * anchors
            {
                return Err(format!(
                    "stride {stride}'s outputs are not {anchors} anchors of 64, 1 and 51"
                ));
            }
            for i in 0..anchors {
                let logit = class.at(i);
                if best.is_none_or(|(b, _, _)| logit > b) {
                    best = Some((logit, level, i));
                }
            }
        }
        let (logit, level, i) = best.ok_or("no anchors")?;
        let stride = STRIDES[level] as f32;
        let grid_w = input.width / STRIDES[level];
        let anchors = grid_w * (input.height / STRIDES[level]);
        let points = &outputs[3 * level + 2];
        let (ax, ay) = ((i % grid_w) as f32, (i / grid_w) as f32);
        let mut keypoints = [Keypoint::default(); 17];
        for (k, keypoint) in keypoints.iter_mut().enumerate() {
            let at = |c: usize| points.at((k * 3 + c) * anchors + i);
            *keypoint = Keypoint {
                x: (at(0) * 2.0 + ax) * stride,
                y: (at(1) * 2.0 + ay) * stride,
                score: sigmoid(at(2)),
            };
        }
        Ok(Found {
            score: sigmoid(logit),
            keypoints,
        })
    }
}

/// The DFL box of anchor `i` (`x0, y0, x1, y1` in the frame's pixels), as
/// the reference decodes it: not in the answer, kept for the comparison
/// against the reference.
pub fn yolov8_box(outputs: &[Tensor], input: Input, level: usize, i: usize) -> [f32; 4] {
    let stride = STRIDES[level];
    let grid_w = input.width / stride;
    let anchors = grid_w * (input.height / stride);
    let boxes = &outputs[3 * level];
    let mut d = [0f32; 4];
    for (side, distance) in d.iter_mut().enumerate() {
        let bins: Vec<f32> = (0..DFL_BINS)
            .map(|b| boxes.at((side * DFL_BINS + b) * anchors + i))
            .collect();
        let most = bins.iter().copied().fold(f32::MIN, f32::max);
        let e: Vec<f32> = bins.iter().map(|v| (v - most).exp()).collect();
        let sum: f32 = e.iter().sum();
        *distance = e.iter().enumerate().map(|(b, v)| b as f32 * v).sum::<f32>() / sum;
    }
    let (ax, ay) = ((i % grid_w) as f32 + 0.5, (i / grid_w) as f32 + 0.5);
    let s = stride as f32;
    [
        (ax - d[0]) * s,
        (ay - d[1]) * s,
        (ax + d[2]) * s,
        (ay + d[3]) * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Format, Quant};

    const SQUARE: Input = Input {
        width: 192,
        height: 192,
        planar: false,
    };

    #[derive(serde::Deserialize)]
    struct Map {
        channels: usize,
        scale: f32,
        zero: i32,
        fill: i8,
        /// Cell-and-channel index, value; every other number is `fill`.
        at: Vec<i32>,
    }

    #[derive(serde::Deserialize)]
    struct Heads {
        maps: Vec<Map>,
        keypoints: Vec<[f32; 3]>,
    }

    /// The NPU's int8 maps of two frames (the converter's simulation of
    /// the lamp's model) and the keypoints MoveNet's own decode
    /// reads from them, by `tests/fixtures/movenet/make.py`.
    #[test]
    fn movenet_reads_the_npus_maps_as_the_reference_does() {
        let runs: Vec<Heads> =
            serde_json::from_str(include_str!("../tests/fixtures/movenet/heads.json")).unwrap();
        assert_eq!(runs.len(), 2);
        for run in runs {
            let outputs: Vec<Tensor> = run
                .maps
                .iter()
                .map(|m| {
                    let mut data = vec![m.fill as u8; 48 * 48 * m.channels];
                    for pair in m.at.chunks(2) {
                        data[pair[0] as usize] = pair[1] as i8 as u8;
                    }
                    Tensor {
                        dims: vec![1, 48, 48, m.channels],
                        format: Format::I8,
                        quant: Quant::Affine {
                            scale: m.scale,
                            zero: m.zero,
                        },
                        data,
                    }
                })
                .collect();
            let found = MoveNet.decode(&outputs, SQUARE).unwrap();
            for (k, [y, x, score]) in run.keypoints.iter().enumerate() {
                let ours = found.keypoints[k];
                assert!(
                    (ours.y / 192.0 - y).abs() < 1e-4
                        && (ours.x / 192.0 - x).abs() < 1e-4
                        && (ours.score - score).abs() < 1e-4,
                    "keypoint {k}: {ours:?}, the reference {y} {x} {score}"
                );
            }
        }
    }

    /// The same two frames' maps as the int16 MoveNet gives them: dynamic
    /// fixed point, fl 15, 15, 10 and 8 (its `nbg_meta.json`), made from
    /// the int8 maps' values. The decode reads them through their
    /// quantisation to the same keypoints.
    #[test]
    fn movenet_reads_int16_fixed_point_maps() {
        let runs: Vec<Heads> =
            serde_json::from_str(include_str!("../tests/fixtures/movenet/heads.json")).unwrap();
        for run in runs {
            let outputs: Vec<Tensor> = run
                .maps
                .iter()
                .zip([15, 15, 10, 8])
                .map(|(m, pos)| {
                    let mut q = vec![i32::from(m.fill); 48 * 48 * m.channels];
                    for pair in m.at.chunks(2) {
                        q[pair[0] as usize] = pair[1];
                    }
                    let data = q
                        .iter()
                        .flat_map(|&q| {
                            let real = (q - m.zero) as f32 * m.scale;
                            let fixed = (real * 2f32.powi(pos)).round();
                            (fixed.clamp(-32768.0, 32767.0) as i16).to_le_bytes()
                        })
                        .collect();
                    Tensor {
                        dims: vec![1, 48, 48, m.channels],
                        format: Format::I16,
                        quant: Quant::Dfp { pos },
                        data,
                    }
                })
                .collect();
            let found = MoveNet.decode(&outputs, SQUARE).unwrap();
            for (k, [y, x, score]) in run.keypoints.iter().enumerate() {
                let ours = found.keypoints[k];
                assert!(
                    (ours.y / 192.0 - y).abs() < 2e-3
                        && (ours.x / 192.0 - x).abs() < 2e-3
                        && (ours.score - score).abs() < 1e-3,
                    "keypoint {k}: {ours:?}, the reference {y} {x} {score}"
                );
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct Track {
        width: usize,
        height: usize,
        /// `x, y, score` in the picture's pixels.
        keypoints: Vec<[f32; 3]>,
        next: [f32; 4],
    }

    /// Keypoints found on two frames, and the crop the reference
    /// (`mn.crop_region`) takes next; a frame without its shoulders seen
    /// is looked at whole again.
    #[test]
    fn movenet_crops_the_next_frame_as_the_reference_does() {
        let tracks: Vec<Track> =
            serde_json::from_str(include_str!("../tests/fixtures/movenet/track.json")).unwrap();
        for track in &tracks {
            let mut keypoints = [Keypoint::default(); 17];
            for (k, [x, y, score]) in track.keypoints.iter().enumerate() {
                keypoints[k] = Keypoint {
                    x: *x,
                    y: *y,
                    score: *score,
                };
            }
            let found = Found {
                score: 0.5,
                keypoints,
            };
            let whole = Region::whole(track.width, track.height, SQUARE);
            let region = MoveNet
                .next(&found, track.width, track.height)
                .unwrap_or(whole);
            let ours = [region.y0, region.x0, region.y1, region.x1];
            for (a, b) in ours.iter().zip(track.next) {
                assert!(
                    (a - b).abs() < 1e-4,
                    "{ours:?}, the reference {:?}",
                    track.next
                );
            }
        }
        let last = tracks.last().unwrap();
        assert!(
            MoveNet
                .next(
                    &Found {
                        score: 0.5,
                        keypoints: [Keypoint::default(); 17]
                    },
                    last.width,
                    last.height
                )
                .is_none()
        );
    }

    #[test]
    fn movenet_maps_decode_to_their_points() {
        let mut points = [(0.5, 0.25, 0.8); 17];
        points[0] = (0.1, 0.9, 0.4);
        let found = MoveNet.decode(&movenet_maps(&points, 48), SQUARE).unwrap();
        let nose = found.keypoints[0];
        assert!((nose.x - 0.9 * 192.0).abs() < 1e-3 && (nose.y - 0.1 * 192.0).abs() < 1e-3);
        assert!((nose.score - 0.4).abs() < 1e-6);
        assert!((found.keypoints[5].x - 48.0).abs() < 1e-3);
        assert!((found.score - (0.4 + 16.0 * 0.8) / 17.0).abs() < 1e-6);
    }

    /// Two people: a fainter centre in the middle and a stronger one far
    /// off it. MoveNet's weight, `1 / (distance + 1.8)`, takes the one in
    /// the middle (0.6 / 1.8 against 0.9 / 21.8).
    #[test]
    fn movenet_takes_the_person_nearest_the_middle() {
        let points = [(0.5, 0.5, 0.8); 17];
        let mut maps = movenet_maps(&points, 48);
        let mut centre = vec![0f32; 48 * 48];
        centre[24 * 48 + 24] = 0.6;
        centre[4 * 48 + 4] = 0.9;
        maps[0] = Tensor::from_f32(vec![1, 48, 48, 1], &centre);
        // The far person's own keypoints, all at its centre, hotter.
        let mut heat: Vec<f32> = (0..maps[1].len()).map(|i| maps[1].at(i)).collect();
        for k in 0..17 {
            heat[(4 * 48 + 4) * 17 + k] = 0.95;
        }
        maps[1] = Tensor::from_f32(vec![1, 48, 48, 17], &heat);
        let found = MoveNet.decode(&maps, SQUARE).unwrap();
        let nose = found.keypoints[0];
        assert!(
            (nose.x - 96.0).abs() < 1e-3 && (nose.y - 96.0).abs() < 1e-3,
            "{nose:?}"
        );
    }

    #[test]
    fn movenet_refuses_outputs_not_its_own() {
        let wrong = Tensor::from_f32(vec![1, 10], &[0.0; 10]);
        assert!(MoveNet.decode(&[wrong], SQUARE).is_err());
        assert!(MoveNet.decode(&[], SQUARE).is_err());
        let mut maps = movenet_maps(&[(0.5, 0.5, 0.5); 17], 48);
        maps.swap(1, 2);
        assert!(MoveNet.decode(&maps, SQUARE).is_err());
    }

    /// Nine YOLOv8-pose maps for a 64x32 frame, all quiet but anchor `i`
    /// of stride 16, whose keypoint offsets are `(0.5, 0.25)` and logits 3.
    fn yolo_outputs(i: usize) -> Vec<Tensor> {
        let input = Input {
            width: 64,
            height: 32,
            planar: false,
        };
        let mut outputs = Vec::new();
        for (level, stride) in STRIDES.iter().enumerate() {
            let anchors = (input.width / stride) * (input.height / stride);
            let boxes = vec![0f32; 64 * anchors];
            let mut class = vec![-9f32; anchors];
            let mut points = vec![0f32; 51 * anchors];
            if level == 1 {
                class[i] = 2.0;
                for k in 0..17 {
                    points[(k * 3) * anchors + i] = 0.5;
                    points[(k * 3 + 1) * anchors + i] = 0.25;
                    points[(k * 3 + 2) * anchors + i] = 3.0;
                }
            }
            outputs.push(Tensor::from_f32(vec![1, 64, anchors], &boxes));
            outputs.push(Tensor::from_f32(vec![1, 1, anchors], &class));
            outputs.push(Tensor::from_f32(vec![1, 51, anchors], &points));
        }
        outputs
    }

    #[test]
    fn yolov8_pose_takes_the_most_confident_anchor() {
        let input = Input {
            width: 64,
            height: 32,
            planar: false,
        };
        // Stride 16: a 4x2 grid; anchor 5 is column 1, row 1.
        let outputs = yolo_outputs(5);
        let found = Yolov8Pose.decode(&outputs, input).unwrap();
        assert!((found.score - sigmoid(2.0)).abs() < 1e-6);
        let k = found.keypoints[0];
        assert_eq!(
            (k.x, k.y),
            ((0.5 * 2.0 + 1.0) * 16.0, (0.25 * 2.0 + 1.0) * 16.0)
        );
        assert!((k.score - sigmoid(3.0)).abs() < 1e-6);
        // All-zero DFL bins: each side the bins' mean, 7.5 strides out.
        let b = yolov8_box(&outputs, input, 1, 5);
        assert_eq!(
            b,
            [
                (1.5 - 7.5) * 16.0,
                (1.5 - 7.5) * 16.0,
                (1.5 + 7.5) * 16.0,
                (1.5 + 7.5) * 16.0
            ]
        );
        assert!(Yolov8Pose.decode(&outputs[..8], input).is_err());
    }

    #[test]
    fn every_head_is_named() {
        for name in HEADS {
            assert_eq!(named(name).unwrap().name(), name);
        }
        assert!(named("rtmpose").is_none());
    }
}
