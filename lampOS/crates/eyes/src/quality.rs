//! Local face measurements shared by comparison/enrolment callers (#8971).
//!
//! This module has no runtime caller, default thresholds, inference, I/O or bus
//! publication. A fit result only compares measurements with the caller's
//! explicitly supplied limits. It establishes neither presence nor identity.
//! The caller must bind the face, working picture and native frame to the same
//! capture, and check its privacy/camera epoch and freshness before use.
use crate::{
    faces::Face,
    facing,
    picture::{Frame, HEIGHT, MAX_SIDE, Picture, WIDTH, fit_scale},
};

/// At most 128 by 128 native-pixel stencils per face, with no crop allocation.
/// This is a work bound, not a recognition or blur operating threshold.
pub const BLUR_GRID_SIDE: usize = 128;

/// The exact arithmetic/scale of a blur measurement. V1's aligned 112-pixel
/// crop variance is a different metric; its limits must not be copied here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlurMetric {
    /// Variance of the cardinal one-native-pixel Laplacian of integer luma,
    /// sampled on an evenly spaced grid inside the raw native face box.
    NativeLumaLaplacianV1,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurMeasurement {
    pub metric: BlurMetric,
    pub variance: f64,
    pub samples: usize,
}

/// No defaults: the caller supplies a measured operating point. An absent blur
/// limit remains unavailable rather than quietly disabling the blur check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComparisonLimits {
    pub minimum_score: f32,
    pub minimum_native_eye_pixels: f32,
    /// Existing five-mark yaw estimate, not a qualified physical head angle.
    pub maximum_yaw_estimate_deg: f32,
    pub maximum_clipped_fraction: f64,
    pub blur: Option<BlurLimit>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurLimit {
    pub metric: BlurMetric,
    pub minimum_variance: f64,
}

/// Bounded reasons suitable for a future numeric/enum result. No words, names,
/// image pixels, model identity or person identifier are present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum QualityReason {
    LowScore,
    Small,
    Turned,
    Blurred,
    Clipped,
    Invalid,
    Unavailable,
}

impl QualityReason {
    const ALL: [Self; 7] = [
        Self::LowScore,
        Self::Small,
        Self::Turned,
        Self::Blurred,
        Self::Clipped,
        Self::Invalid,
        Self::Unavailable,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::LowScore => "low_score",
            Self::Small => "small",
            Self::Turned => "turned",
            Self::Blurred => "blurred",
            Self::Clipped => "clipped",
            Self::Invalid => "invalid",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QualityDecision {
    pub fit: bool,
    reasons: u8,
}

impl QualityDecision {
    pub fn has(self, reason: QualityReason) -> bool {
        self.reasons & (1 << reason as u8) != 0
    }

    pub fn reasons(self) -> impl Iterator<Item = QualityReason> {
        QualityReason::ALL
            .into_iter()
            .filter(move |reason| self.has(*reason))
    }

    fn reject(&mut self, reason: QualityReason) {
        self.fit = false;
        self.reasons |= 1 << reason as u8;
    }

    fn require(&mut self, measured: Option<bool>, reason: QualityReason) {
        match measured {
            Some(true) => {}
            Some(false) => self.reject(reason),
            None => self.reject(QualityReason::Unavailable),
        }
    }
}

/// Measurements refer to the original detection, never a clamped/enlarged box.
/// Native detail is unavailable when no consistent native frame was provided.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceQuality {
    pub score: Option<f32>,
    /// x, y, width, height in the working picture's pixels.
    pub raw_box: Option<[f32; 4]>,
    pub height_fraction: Option<f32>,
    pub clipped_fraction: Option<f64>,
    pub working_eye_pixels: Option<f32>,
    pub native_eye_pixels: Option<f32>,
    pub yaw_estimate_abs_deg: Option<f32>,
    pub blur: Option<BlurMeasurement>,
    invalid: bool,
}

impl FaceQuality {
    pub fn fit_to_compare(&self, limits: &ComparisonLimits) -> QualityDecision {
        let mut decision = QualityDecision {
            fit: true,
            reasons: 0,
        };
        let valid_limits = (0.0..=1.0).contains(&limits.minimum_score)
            && limits.minimum_native_eye_pixels.is_finite()
            && limits.minimum_native_eye_pixels > 0.0
            && (0.0..=90.0).contains(&limits.maximum_yaw_estimate_deg)
            && (0.0..=1.0).contains(&limits.maximum_clipped_fraction)
            && limits.blur.is_none_or(|limit| {
                limit.minimum_variance.is_finite() && limit.minimum_variance >= 0.0
            });
        let valid_measurements = self.score.is_none_or(|v| (0.0..=1.0).contains(&v))
            && self
                .native_eye_pixels
                .is_none_or(|v| v.is_finite() && v > 0.0)
            && self
                .yaw_estimate_abs_deg
                .is_none_or(|v| (0.0..=90.0).contains(&v))
            && self
                .clipped_fraction
                .is_none_or(|v| (0.0..=1.0).contains(&v))
            && self.blur.is_none_or(|v| {
                v.variance.is_finite()
                    && v.variance >= 0.0
                    && (2..=BLUR_GRID_SIDE * BLUR_GRID_SIDE).contains(&v.samples)
            });
        if !valid_limits || !valid_measurements || self.invalid {
            decision.reject(QualityReason::Invalid);
        }
        decision.require(
            self.score
                .map(|v| v.is_finite() && v >= limits.minimum_score),
            QualityReason::LowScore,
        );
        decision.require(
            self.native_eye_pixels
                .map(|v| v.is_finite() && v >= limits.minimum_native_eye_pixels),
            QualityReason::Small,
        );
        decision.require(
            self.yaw_estimate_abs_deg
                .map(|v| v.is_finite() && v <= limits.maximum_yaw_estimate_deg),
            QualityReason::Turned,
        );
        decision.require(
            self.clipped_fraction
                .map(|v| v.is_finite() && v <= limits.maximum_clipped_fraction),
            QualityReason::Clipped,
        );
        decision.require(
            self.blur.zip(limits.blur).map(|(value, limit)| {
                value.metric == limit.metric
                    && value.samples > 0
                    && value.variance.is_finite()
                    && value.variance >= limit.minimum_variance
            }),
            QualityReason::Blurred,
        );
        decision
    }
}

/// Bounded arithmetic on one already-decoded capture. Layout consistency is
/// checked; matching pixels/capture epochs remain the caller's responsibility.
pub fn measure(face: &Face, picture: &Picture, native_frame: Option<&Frame>) -> FaceQuality {
    let mut quality = FaceQuality {
        score: None,
        raw_box: None,
        height_fraction: None,
        clipped_fraction: None,
        working_eye_pixels: None,
        native_eye_pixels: None,
        yaw_estimate_abs_deg: None,
        blur: None,
        invalid: true,
    };
    let (width, height) = picture.filled;
    if !(1..=WIDTH).contains(&width)
        || !(1..=HEIGHT).contains(&height)
        || picture.planes.len() != 3 * WIDTH * HEIGHT
        || !(0.0..=1.0).contains(&face.score)
        || [face.x, face.y, face.width, face.height]
            .into_iter()
            .any(|v| !v.is_finite())
        || !(0.0..=WIDTH as f32).contains(&face.width)
        || !(0.0..=HEIGHT as f32).contains(&face.height)
        || face.width == 0.0
        || face.height == 0.0
    {
        return quality;
    }
    let (x, y, across, down) = (
        f64::from(face.x),
        f64::from(face.y),
        f64::from(face.width),
        f64::from(face.height),
    );
    let visible_width = ((x + across).min(width as f64) - x.max(0.0)).max(0.0);
    let visible_height = ((y + down).min(height as f64) - y.max(0.0)).max(0.0);
    quality.score = Some(face.score);
    quality.raw_box = Some([face.x, face.y, face.width, face.height]);
    quality.height_fraction = Some(face.height / height as f32);
    quality.clipped_fraction =
        Some((1.0 - visible_width * visible_height / (across * down)).clamp(0.0, 1.0));
    // V1's alignment guard: every landmark must be inside both the raw box
    // and the filled image. Padding and invented off-frame landmarks are not
    // observed features, even if a fitted box appears plausible.
    let marks_valid = face.marks.iter().all(|&(mx, my)| {
        mx.is_finite()
            && my.is_finite()
            && f64::from(mx) >= x.max(0.0)
            && f64::from(mx) <= (x + across).min(width as f64)
            && f64::from(my) >= y.max(0.0)
            && f64::from(my) <= (y + down).min(height as f64)
    });
    if marks_valid {
        let pixels = (face.marks[0].0 - face.marks[1].0).hypot(face.marks[0].1 - face.marks[1].1);
        if pixels.is_finite() && pixels > 0.0 {
            quality.working_eye_pixels = Some(pixels);
            quality.yaw_estimate_abs_deg = facing::yaw_deg(&face.marks);
        }
    }
    quality.invalid = !marks_valid || quality.yaw_estimate_abs_deg.is_none();
    if let Some(frame) = native_frame {
        if native_matches(frame, picture) {
            quality.native_eye_pixels = quality.working_eye_pixels.map(|v| v / frame.scale);
            quality.blur = blur(face, frame);
        } else {
            quality.invalid = true;
        }
    }
    quality
}

fn native_matches(frame: &Frame, picture: &Picture) -> bool {
    if !(1..=MAX_SIDE).contains(&frame.width) || !(1..=MAX_SIDE).contains(&frame.height) {
        return false;
    }
    let scale = fit_scale(frame.width, frame.height);
    // Frame::scale is produced by this exact helper at decode; a mismatched
    // scale cannot manufacture native detail from an enlarged working crop.
    frame.scale.to_bits() == scale.to_bits()
        && frame.rgb.len() == frame.width * frame.height * 3
        && picture.filled
            == (
                ((frame.width as f32 * scale) as usize).clamp(1, WIDTH),
                ((frame.height as f32 * scale) as usize).clamp(1, HEIGHT),
            )
}

fn blur(face: &Face, frame: &Frame) -> Option<BlurMeasurement> {
    let scale = f64::from(frame.scale);
    let left = (f64::from(face.x) / scale)
        .ceil()
        .clamp(0.0, frame.width as f64) as usize;
    let top = (f64::from(face.y) / scale)
        .ceil()
        .clamp(0.0, frame.height as f64) as usize;
    let right = ((f64::from(face.x) + f64::from(face.width)) / scale)
        .floor()
        .clamp(0.0, frame.width as f64) as usize;
    let bottom = ((f64::from(face.y) + f64::from(face.height)) / scale)
        .floor()
        .clamp(0.0, frame.height as f64) as usize;
    let across = right.checked_sub(left)?.checked_sub(2)?;
    let down = bottom.checked_sub(top)?.checked_sub(2)?;
    if across == 0 || down == 0 {
        return None;
    }
    let nx = across.min(BLUR_GRID_SIDE);
    let ny = down.min(BLUR_GRID_SIDE);
    if nx * ny < 2 {
        return None;
    }
    let luma = |x: usize, y: usize| -> f64 {
        let at = (y * frame.width + x) * 3;
        // Fixed 8-bit luma definition; coefficients sum to 256. No resize,
        // interpolation or floating pixel representation is introduced.
        f64::from(
            (77 * u32::from(frame.rgb[at])
                + 150 * u32::from(frame.rgb[at + 1])
                + 29 * u32::from(frame.rgb[at + 2]))
                >> 8,
        )
    };
    let (mut sum, mut squared) = (0.0, 0.0);
    for iy in 0..ny {
        let y = top + 1 + (2 * iy + 1) * down / (2 * ny);
        for ix in 0..nx {
            let x = left + 1 + (2 * ix + 1) * across / (2 * nx);
            let lap = luma(x - 1, y) + luma(x + 1, y) + luma(x, y - 1) + luma(x, y + 1)
                - 4.0 * luma(x, y);
            sum += lap;
            squared += lap * lap;
        }
    }
    let samples = nx * ny;
    let mean = sum / samples as f64;
    Some(BlurMeasurement {
        metric: BlurMetric::NativeLumaLaplacianV1,
        variance: (squared / samples as f64 - mean * mean).max(0.0),
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, checker: bool) -> Frame {
        let mut rgb = vec![0; width * height * 3];
        for y in 0..height {
            for x in 0..width {
                let value = if checker && (x + y) % 2 == 0 { 255 } else { 0 };
                rgb[(y * width + x) * 3..(y * width + x) * 3 + 3].fill(value);
            }
        }
        Frame {
            rgb,
            width,
            height,
            scale: fit_scale(width, height),
        }
    }

    fn picture(frame: &Frame) -> Picture {
        Picture::from_rgb(&frame.rgb, frame.width, frame.height).unwrap()
    }

    fn face() -> Face {
        Face {
            score: 0.9,
            x: 40.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
            marks: [
                (70.0, 80.0),
                (110.0, 80.0),
                (90.0, 100.0),
                (75.0, 130.0),
                (105.0, 130.0),
            ],
        }
    }

    // Authored arithmetic controls only; these numbers are not product limits.
    fn limits() -> ComparisonLimits {
        ComparisonLimits {
            minimum_score: 0.8,
            minimum_native_eye_pixels: 30.0,
            maximum_yaw_estimate_deg: 30.0,
            maximum_clipped_fraction: 0.0,
            blur: Some(BlurLimit {
                metric: BlurMetric::NativeLumaLaplacianV1,
                minimum_variance: 0.0,
            }),
        }
    }

    #[test]
    fn known_measurements_compare_without_creating_a_person_or_identity() {
        let native = frame(320, 240, false);
        let quality = measure(&face(), &picture(&native), Some(&native));
        assert_eq!(quality.working_eye_pixels, Some(40.0));
        assert_eq!(quality.native_eye_pixels, Some(40.0));
        assert_eq!(quality.clipped_fraction, Some(0.0));
        assert_eq!(quality.yaw_estimate_abs_deg, Some(0.0));
        assert_eq!(quality.blur.unwrap().variance, 0.0);
        let decision = quality.fit_to_compare(&limits());
        assert!(decision.fit);
        assert_eq!(decision.reasons().count(), 0);
    }

    #[test]
    fn missing_native_pixels_or_unmeasured_blur_limit_is_unavailable() {
        let native = frame(320, 240, false);
        let working = picture(&native);
        let quality = measure(&face(), &working, None);
        assert_eq!(quality.working_eye_pixels, Some(40.0));
        assert_eq!(quality.native_eye_pixels, None);
        assert_eq!(quality.blur, None);
        let decision = quality.fit_to_compare(&limits());
        assert!(!decision.fit);
        assert_eq!(
            decision.reasons().collect::<Vec<_>>(),
            [QualityReason::Unavailable]
        );
        let mut unmeasured = limits();
        unmeasured.blur = None;
        let known = measure(&face(), &working, Some(&native));
        assert!(
            known
                .fit_to_compare(&unmeasured)
                .has(QualityReason::Unavailable)
        );
    }

    #[test]
    fn native_eye_detail_comes_from_actual_frame_scale_not_enlargement() {
        let small = frame(320, 240, false);
        let large = frame(1280, 960, false);
        let q_small = measure(&face(), &picture(&small), Some(&small));
        let q_large = measure(&face(), &picture(&large), Some(&large));
        assert_eq!(q_small.working_eye_pixels, q_large.working_eye_pixels);
        assert_eq!(q_small.native_eye_pixels, Some(40.0));
        assert_eq!(q_large.native_eye_pixels, Some(160.0));
        let mut operating = limits();
        operating.minimum_native_eye_pixels = 80.0;
        assert!(q_small.fit_to_compare(&operating).has(QualityReason::Small));
        assert!(q_large.fit_to_compare(&operating).fit);
        let mut invented = frame(320, 240, false);
        invented.scale = 0.25;
        let bad = measure(&face(), &picture(&small), Some(&invented));
        assert_eq!(bad.native_eye_pixels, None);
        assert!(bad.fit_to_compare(&operating).has(QualityReason::Invalid));
    }

    #[test]
    fn clipping_uses_filled_image_instead_of_detector_padding() {
        let native = frame(320, 240, false);
        let mut edge = face();
        edge.y = 170.0;
        edge.height = 80.0;
        edge.marks = [
            (70.0, 190.0),
            (110.0, 190.0),
            (90.0, 210.0),
            (75.0, 230.0),
            (105.0, 230.0),
        ];
        let quality = measure(&edge, &picture(&native), Some(&native));
        assert_eq!(quality.clipped_fraction, Some(0.125));
        assert_eq!(quality.raw_box, Some([40.0, 170.0, 100.0, 80.0]));
        assert!(
            quality
                .fit_to_compare(&limits())
                .has(QualityReason::Clipped)
        );
        let mut exact = limits();
        exact.maximum_clipped_fraction = 0.125;
        assert!(quality.fit_to_compare(&exact).fit);
    }

    #[test]
    fn score_size_turn_and_blur_have_separate_reasons() {
        let native = frame(320, 240, false);
        let mut uncertain = face();
        uncertain.score = 0.7;
        uncertain.marks[2].0 = 110.0;
        let mut operating = limits();
        operating.minimum_native_eye_pixels = 50.0;
        operating.blur.as_mut().unwrap().minimum_variance = 1.0;
        let decision =
            measure(&uncertain, &picture(&native), Some(&native)).fit_to_compare(&operating);
        assert!(!decision.fit);
        assert_eq!(
            decision.reasons().collect::<Vec<_>>(),
            [
                QualityReason::LowScore,
                QualityReason::Small,
                QualityReason::Turned,
                QualityReason::Blurred,
            ]
        );
    }

    #[test]
    fn coincident_nonfinite_and_off_box_landmarks_are_invalid() {
        let native = frame(320, 240, false);
        let working = picture(&native);
        for bad in [0, 1, 2, 3] {
            let mut detected = face();
            match bad {
                0 => detected.marks[1] = detected.marks[0],
                1 => detected.marks[2].0 = f32::NAN,
                2 => detected.marks[4].0 = 200.0,
                _ => detected.marks[0].1 = 245.0,
            }
            let result = measure(&detected, &working, Some(&native)).fit_to_compare(&limits());
            assert!(!result.fit);
            assert!(result.has(QualityReason::Invalid));
        }
    }

    #[test]
    fn malformed_boxes_pictures_and_native_buffers_fail_closed() {
        let native = frame(320, 240, false);
        let working = picture(&native);
        for bad in [f32::NAN, f32::INFINITY, 0.0, -1.0, f32::MAX] {
            let mut detected = face();
            detected.width = bad;
            assert!(
                measure(&detected, &working, Some(&native))
                    .fit_to_compare(&limits())
                    .has(QualityReason::Invalid)
            );
        }
        for size in [(0, 240), (321, 240), (320, 257)] {
            let mut wrong = working.clone();
            wrong.filled = size;
            assert!(
                !measure(&face(), &wrong, Some(&native))
                    .fit_to_compare(&limits())
                    .fit
            );
        }
        let mut broken = working.clone();
        broken.planes.pop();
        assert!(
            !measure(&face(), &broken, Some(&native))
                .fit_to_compare(&limits())
                .fit
        );
        let mut wrong_native = frame(320, 240, false);
        wrong_native.rgb.pop();
        assert!(
            !measure(&face(), &working, Some(&wrong_native))
                .fit_to_compare(&limits())
                .fit
        );
        wrong_native.width = usize::MAX;
        assert!(
            !measure(&face(), &working, Some(&wrong_native))
                .fit_to_compare(&limits())
                .fit
        );
        let other_shape = frame(1280, 720, false);
        assert!(
            measure(&face(), &working, Some(&other_shape))
                .fit_to_compare(&limits())
                .has(QualityReason::Invalid)
        );
    }

    #[test]
    fn malformed_operating_limits_never_pass() {
        let native = frame(320, 240, false);
        let quality = measure(&face(), &picture(&native), Some(&native));
        for bad in 0..10 {
            let mut operating = limits();
            match bad {
                0 => operating.minimum_score = f32::NAN,
                1 => operating.minimum_score = -1.0,
                2 => operating.minimum_native_eye_pixels = 0.0,
                3 => operating.minimum_native_eye_pixels = f32::INFINITY,
                4 => operating.maximum_yaw_estimate_deg = f32::NAN,
                5 => operating.maximum_yaw_estimate_deg = 91.0,
                6 => operating.maximum_clipped_fraction = f64::NAN,
                7 => operating.maximum_clipped_fraction = -0.01,
                8 => operating.blur.as_mut().unwrap().minimum_variance = f64::INFINITY,
                _ => operating.blur.as_mut().unwrap().minimum_variance = -1.0,
            }
            let decision = quality.fit_to_compare(&operating);
            assert!(!decision.fit);
            assert!(decision.has(QualityReason::Invalid));
        }
    }

    #[test]
    fn native_laplacian_has_known_flat_and_checkerboard_values() {
        let flat = frame(6, 6, false);
        let checker = frame(6, 6, true);
        let crop = Face {
            x: 0.0,
            y: 0.0,
            width: 6.0,
            height: 6.0,
            ..face()
        };
        let smooth = blur(&crop, &flat).unwrap();
        let sharp = blur(&crop, &checker).unwrap();
        assert_eq!(smooth.samples, 16);
        assert_eq!(smooth.variance, 0.0);
        assert_eq!(sharp.samples, 16);
        assert_eq!(sharp.variance, 1_040_400.0);
    }

    #[test]
    fn blur_has_a_fixed_work_bound_and_no_padding_or_tiny_crop_fallback() {
        let native = frame(1280, 960, false);
        let full = Face {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
            ..face()
        };
        assert_eq!(
            blur(&full, &native).unwrap().samples,
            BLUR_GRID_SIDE * BLUR_GRID_SIDE
        );
        let tiny = Face {
            width: 0.5,
            height: 0.5,
            ..full
        };
        assert_eq!(blur(&tiny, &native), None);
        let outside = Face {
            x: -100.0,
            y: -100.0,
            width: 20.0,
            height: 20.0,
            ..full
        };
        assert_eq!(blur(&outside, &native), None);
    }

    #[test]
    fn exact_measurement_boundaries_are_inclusive() {
        let native = frame(320, 240, false);
        let mut detected = face();
        detected.score = 0.8;
        let quality = measure(&detected, &picture(&native), Some(&native));
        let mut operating = limits();
        operating.minimum_native_eye_pixels = 40.0;
        operating.maximum_yaw_estimate_deg = 0.0;
        assert!(quality.fit_to_compare(&operating).fit);
        operating.minimum_native_eye_pixels = f32::from_bits(40.0f32.to_bits() + 1);
        assert!(quality.fit_to_compare(&operating).has(QualityReason::Small));
    }

    #[test]
    fn invalid_public_measurements_cannot_silently_pass() {
        let native = frame(320, 240, false);
        for kind in 0..5 {
            let mut quality = measure(&face(), &picture(&native), Some(&native));
            match kind {
                0 => quality.score = Some(2.0),
                1 => quality.yaw_estimate_abs_deg = Some(-20.0),
                2 => quality.clipped_fraction = Some(-1.0),
                3 => quality.blur.as_mut().unwrap().samples = 1,
                _ => quality.blur.as_mut().unwrap().variance = f64::NAN,
            }
            let decision = quality.fit_to_compare(&limits());
            assert!(!decision.fit);
            assert!(decision.has(QualityReason::Invalid));
        }
        let tiny = Face {
            x: 0.0,
            y: 0.0,
            width: 3.0,
            height: 3.0,
            ..face()
        };
        assert_eq!(blur(&tiny, &native), None);
    }
}
