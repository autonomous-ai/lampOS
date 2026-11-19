//! A pose model's head: its raw outputs read as one person's 17 COCO
//! keypoints, in the model's frame's pixels, and a score for the person.
//! Which head a model has is its manifest's `head`, so a model is swapped
//! by the Store without a lampOS release, and a new kind of model is one
//! more [`PoseHead`].
//!
//! - `movenet`: MoveNet SinglePose (Lightning 192x192, Thunder 256x256),
//!   Apache-2.0. One output of 17 x `[y, x, score]`, normalised to the
//!   frame; the person's score is the keypoints' mean.
//! - `yolov8-pose`: YOLOv8-pose cut before its decode, as `npu/convert.sh`
//!   cuts it (box 64 / class 1 / keypoints 51 at strides 8, 16 and 32, nine
//!   outputs in that order). The most confident anchor is the person, as the
//!   reference binding decodes it; one person is answered, so no
//!   suppression of others is needed. Kept for comparison only: its weights
//!   are AGPL-3.0 and no lamp is sent them (#7415).
use crate::engine::{Input, Tensor};

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

impl PoseHead for MoveNet {
    fn name(&self) -> &'static str {
        "movenet"
    }

    fn decode(&self, outputs: &[Tensor], input: Input) -> Result<Found, String> {
        let [out] = outputs else {
            return Err(format!(
                "a MoveNet model has one output, this one {}",
                outputs.len()
            ));
        };
        if out.len() != 17 * 3 {
            return Err(format!(
                "a MoveNet output is 17 x 3 numbers, this one {}",
                out.len()
            ));
        }
        let mut keypoints = [Keypoint::default(); 17];
        for (k, keypoint) in keypoints.iter_mut().enumerate() {
            let (y, x, score) = (out.at(k * 3), out.at(k * 3 + 1), out.at(k * 3 + 2));
            *keypoint = Keypoint {
                x: x.clamp(0.0, 1.0) * input.width as f32,
                y: y.clamp(0.0, 1.0) * input.height as f32,
                score: score.clamp(0.0, 1.0),
            };
        }
        let score = keypoints.iter().map(|k| k.score).sum::<f32>() / 17.0;
        Ok(Found { score, keypoints })
    }
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

    const SQUARE: Input = Input {
        width: 192,
        height: 192,
        planar: false,
    };

    /// MoveNet's output for keypoints at `(y, x)` normalised, each `score`.
    pub fn movenet_output(points: &[(f32, f32, f32); 17]) -> Tensor {
        let values: Vec<f32> = points.iter().flat_map(|(y, x, s)| [*y, *x, *s]).collect();
        Tensor::from_f32(vec![1, 1, 17, 3], &values)
    }

    #[test]
    fn movenet_reads_y_then_x_in_the_frames_pixels() {
        let mut points = [(0.5, 0.25, 0.8); 17];
        points[0] = (0.1, 0.9, 0.4);
        let found = MoveNet.decode(&[movenet_output(&points)], SQUARE).unwrap();
        assert_eq!(
            found.keypoints[0],
            Keypoint {
                x: 0.9 * 192.0,
                y: 0.1 * 192.0,
                score: 0.4
            }
        );
        assert_eq!(found.keypoints[5].x, 48.0);
        assert!((found.score - (0.4 + 16.0 * 0.8) / 17.0).abs() < 1e-6);
    }

    #[test]
    fn movenet_refuses_outputs_not_its_own() {
        let wrong = Tensor::from_f32(vec![1, 10], &[0.0; 10]);
        assert!(MoveNet.decode(&[wrong], SQUARE).is_err());
        assert!(MoveNet.decode(&[], SQUARE).is_err());
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
