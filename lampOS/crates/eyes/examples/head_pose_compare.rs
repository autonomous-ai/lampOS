//! Offline, unscored 6DRepNet replay through the product JPEG decoder and YuNet.
//! No camera, bus, product switch, image, path, or subject identity in output.
//! Usage: head_pose_compare DETECTOR POSE_ONNX POSE_SHA256 IMAGE_ROOT IMAGE_LIST.json
use lamp_eyes::{
    faces,
    picture::{Frame, Picture},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Instant,
};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_LIST_BYTES: usize = 1_048_576;
const MAX_IMAGE_BYTES: usize = 8_388_608;
const MAX_MODEL_BYTES: usize = 256 * 1_024 * 1_024;
const MAX_IMAGES: usize = 10_000;
const MAX_FRAME_SIDE: usize = 4_096;
const SIDE: usize = 224;
const LEAST_WIDTH_SHARE: f32 = 0.075;
const ROTATION_TOLERANCE: f32 = 0.01;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];
const PREPROCESS: &str = "full_rgb_cross_pad_02_resize224_center_even_bilinear_v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Measured,
    NoFace,
    AmbiguousFace,
    TooSmall,
    ClippedFace,
    InvalidBox,
    InputFailed,
    DecodeFailed,
    DetectorFailed,
    PreprocessFailed,
    InferenceFailed,
    InvalidRotation,
}

impl State {
    fn is_error(self) -> bool {
        matches!(
            self,
            Self::InputFailed
                | Self::DecodeFailed
                | Self::DetectorFailed
                | Self::InvalidBox
                | Self::PreprocessFailed
                | Self::InferenceFailed
                | Self::InvalidRotation
        )
    }
}

#[derive(Debug, Serialize)]
struct Angles {
    pitch_deg: f32,
    yaw_deg: f32,
    roll_deg: f32,
    singular: bool,
}

#[derive(Debug, Serialize)]
struct Sample {
    kind: &'static str,
    index: usize,
    state: State,
    face_count: Option<usize>,
    selected_box_px: Option<[f32; 4]>,
    picture_size: Option<[usize; 2]>,
    full_size: Option<[usize; 2]>,
    frame_scale: Option<f32>,
    crop_xyxy: Option<[usize; 4]>,
    resized_size: Option<[usize; 2]>,
    center_offset: Option<[usize; 2]>,
    angles: Option<Angles>,
    // Counts actual attempted pose calls, not decoded frames or detections.
    pose_call: Option<usize>,
    decode_us: Option<u64>,
    detector_us: Option<u64>,
    preprocess_us: Option<u64>,
    inference_us: Option<u64>,
    geometry_us: Option<u64>,
    total_us: Option<u64>,
}

impl Sample {
    fn empty(index: usize) -> Self {
        Self {
            kind: "sample",
            index,
            state: State::InputFailed,
            face_count: None,
            selected_box_px: None,
            picture_size: None,
            full_size: None,
            frame_scale: None,
            crop_xyxy: None,
            resized_size: None,
            center_offset: None,
            angles: None,
            pose_call: None,
            decode_us: None,
            detector_us: None,
            preprocess_us: None,
            inference_us: None,
            geometry_us: None,
            total_us: None,
        }
    }
}

fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn observe<T, E>(
    duration: &mut Option<u64>,
    work: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let start = Instant::now();
    let result = work();
    *duration = Some(elapsed_us(start));
    result
}

fn read_bounded(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    // A named pipe must not block the offline producer, and the file opened
    // must not become a symlink between canonicalization and open.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > maximum as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unbounded file"));
    }
    let mut bytes = Vec::new();
    (&file).take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() > maximum
        || bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unstable file"));
    }
    Ok(bytes)
}

fn image_list(bytes: &[u8]) -> Result<Vec<PathBuf>> {
    if bytes.len() > MAX_LIST_BYTES {
        return Err("image list byte bound".into());
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(bytes)?;
    if paths.is_empty() || paths.len() > MAX_IMAGES {
        return Err("image list count bound".into());
    }
    let mut unique = std::collections::HashSet::new();
    for path in &paths {
        if path.as_os_str().is_empty()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || !path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg")
            })
            || !unique.insert(path.clone())
        {
            return Err("image paths must be unique relative JPEG paths".into());
        }
    }
    Ok(paths)
}

fn under_root(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "outside root"));
    }
    Ok(path)
}

fn parse_hash(value: &OsString) -> Option<String> {
    let value = value.to_str()?;
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn verified_model_bytes(path: &Path, expected: &str) -> Result<Vec<u8>> {
    let bytes = read_bounded(path, MAX_MODEL_BYTES)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("pose_model_hash_mismatch".into());
    }
    Ok(bytes)
}

struct PoseModel {
    plan: Arc<TypedRunnableModel>,
    bytes: usize,
}

impl PoseModel {
    fn load(path: &Path, expected: &str) -> Result<Self> {
        // The parser sees only the bounded bytes whose caller-supplied hash
        // passed. No model download or unpinned default is allowed here.
        let bytes = verified_model_bytes(path, expected)?;
        let model = tract_onnx::onnx()
            .with_ignore_value_info(true)
            .with_ignore_output_shapes(true)
            .model_for_read(&mut bytes.as_slice())?;
        if model.input_outlets()?.len() != 1 || model.output_outlets()?.len() != 1 {
            return Err("pose_model_interface".into());
        }
        let plan = model
            .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            plan,
            bytes: bytes.len(),
        })
    }

    fn infer(&self, planes: &[f32]) -> Result<[f32; 9]> {
        let input = Tensor::from_shape(&[1, 3, SIDE, SIDE], planes)?;
        let output = self.plan.run(tvec!(input.into()))?;
        if output.len() != 1 || output[0].shape() != [1, 3, 3] {
            return Err("pose_model_output_shape".into());
        }
        let rotation = output[0].to_plain_array_view::<f32>()?;
        let values: Vec<f32> = rotation.iter().copied().collect();
        values
            .try_into()
            .map_err(|_| "pose_model_output_size".into())
    }
}

fn selected_face<'a>(
    found: &'a [faces::Face],
    picture: &Picture,
) -> std::result::Result<&'a faces::Face, State> {
    let [face] = found else {
        return Err(if found.is_empty() {
            State::NoFace
        } else {
            State::AmbiguousFace
        });
    };
    if picture.filled.0 == 0
        || picture.filled.1 == 0
        || ![face.x, face.y, face.width, face.height]
            .into_iter()
            .all(f32::is_finite)
        || face.width <= 0.0
        || face.height <= 0.0
    {
        return Err(State::InvalidBox);
    }
    if face.x < 0.0
        || face.y < 0.0
        || face.x + face.width > picture.filled.0 as f32
        || face.y + face.height > picture.filled.1 as f32
    {
        return Err(State::ClippedFace);
    }
    if face.width / (picture.filled.0 as f32) < LEAST_WIDTH_SHARE {
        return Err(State::TooSmall);
    }
    Ok(face)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Crop {
    // Exclusive right and bottom bounds, in the full decoded RGB frame.
    xyxy: [usize; 4],
    resized: [usize; 2],
    center: [usize; 2],
}

fn center_offset(size: usize) -> Option<usize> {
    let difference = size.checked_sub(SIDE)?;
    // torchvision CenterCrop uses Python round, including ties to even.
    Some(difference / 2 + usize::from(difference % 4 == 3))
}

fn crop_geometry(frame: &Frame, face: &faces::Face) -> Option<Crop> {
    if frame.width == 0
        || frame.height == 0
        || frame.width > MAX_FRAME_SIDE
        || frame.height > MAX_FRAME_SIDE
        || frame.rgb.len() != frame.width.checked_mul(frame.height)?.checked_mul(3)?
        || !frame.scale.is_finite()
        || !(0.0..=1.0).contains(&frame.scale)
        || frame.scale == 0.0
    {
        return None;
    }
    let bounds = [
        face.x / frame.scale,
        face.y / frame.scale,
        (face.x + face.width) / frame.scale,
        (face.y + face.height) / frame.scale,
    ];
    if !bounds.into_iter().all(f32::is_finite)
        || bounds[0] < 0.0
        || bounds[1] < 0.0
        || bounds[2] > frame.width as f32 + 0.001
        || bounds[3] > frame.height as f32 + 0.001
    {
        return None;
    }
    // Truncate first, like the upstream demo. Its padding is intentionally
    // cross-axis: height determines horizontal pad, width vertical pad.
    let [left, top, right, bottom] = bounds.map(|value| value.trunc() as usize);
    let width = right.checked_sub(left)?;
    let height = bottom.checked_sub(top)?;
    if width == 0 || height == 0 {
        return None;
    }
    let pad_x = height / 5;
    let pad_y = width / 5;
    let xyxy = [
        left.saturating_sub(pad_x),
        top.saturating_sub(pad_y),
        right.saturating_add(pad_x).min(frame.width),
        bottom.saturating_add(pad_y).min(frame.height),
    ];
    let width = xyxy[2].checked_sub(xyxy[0])?;
    let height = xyxy[3].checked_sub(xyxy[1])?;
    let short = width.min(height);
    if short == 0 {
        return None;
    }
    let resized = [
        width.checked_mul(SIDE)? / short,
        height.checked_mul(SIDE)? / short,
    ];
    Some(Crop {
        xyxy,
        resized,
        center: [center_offset(resized[0])?, center_offset(resized[1])?],
    })
}

fn rgb_at(frame: &Frame, x: usize, y: usize, channel: usize) -> f32 {
    f32::from(frame.rgb[(y * frame.width + x) * 3 + channel])
}

fn normalized_planes(frame: &Frame, crop: Crop) -> Vec<f32> {
    let [left, top, right, bottom] = crop.xyxy;
    let width = right - left;
    let height = bottom - top;
    let mut planes = vec![0.0; 3 * SIDE * SIDE];
    // Direct sampling of the centered resized crop avoids allocating its
    // possibly long intermediate image. This is Rust bilinear sampling,
    // without PIL's integer quantization or antialias filter, not PIL parity.
    for y in 0..SIDE {
        let source_y =
            (((y + crop.center[1]) as f32 + 0.5) * height as f32 / crop.resized[1] as f32 - 0.5)
                .clamp(0.0, (height - 1) as f32);
        let y0 = source_y.floor() as usize;
        let y1 = (y0 + 1).min(height - 1);
        let dy = source_y - y0 as f32;
        for x in 0..SIDE {
            let source_x =
                (((x + crop.center[0]) as f32 + 0.5) * width as f32 / crop.resized[0] as f32 - 0.5)
                    .clamp(0.0, (width - 1) as f32);
            let x0 = source_x.floor() as usize;
            let x1 = (x0 + 1).min(width - 1);
            let dx = source_x - x0 as f32;
            for channel in 0..3 {
                let upper = rgb_at(frame, left + x0, top + y0, channel) * (1.0 - dx)
                    + rgb_at(frame, left + x1, top + y0, channel) * dx;
                let lower = rgb_at(frame, left + x0, top + y1, channel) * (1.0 - dx)
                    + rgb_at(frame, left + x1, top + y1, channel) * dx;
                let rgb = upper * (1.0 - dy) + lower * dy;
                planes[channel * SIDE * SIDE + y * SIDE + x] =
                    (rgb / 255.0 - MEAN[channel]) / STD[channel];
            }
        }
    }
    planes
}

fn rotation_angles(rotation: [f32; 9]) -> Option<Angles> {
    if !rotation.into_iter().all(f32::is_finite) {
        return None;
    }
    for a in 0..3 {
        for b in 0..3 {
            let dot: f32 = (0..3)
                .map(|row| rotation[row * 3 + a] * rotation[row * 3 + b])
                .sum();
            let expected = if a == b { 1.0 } else { 0.0 };
            if !dot.is_finite() || (dot - expected).abs() > ROTATION_TOLERANCE {
                return None;
            }
        }
    }
    let r = rotation;
    let determinant = r[0] * (r[4] * r[8] - r[5] * r[7]) - r[1] * (r[3] * r[8] - r[5] * r[6])
        + r[2] * (r[3] * r[7] - r[4] * r[6]);
    if !determinant.is_finite() || (determinant - 1.0).abs() > ROTATION_TOLERANCE {
        return None;
    }
    // Upstream compute_euler_angles_from_rotation_matrices, row-major R.
    // These are raw model axes. No camera-sign or label calibration occurs.
    let sy = (r[0] * r[0] + r[3] * r[3]).sqrt();
    let singular = sy < 1e-6;
    Some(Angles {
        pitch_deg: if singular {
            (-r[5]).atan2(r[4]).to_degrees()
        } else {
            r[7].atan2(r[8]).to_degrees()
        },
        yaw_deg: (-r[6]).atan2(sy).to_degrees(),
        roll_deg: if singular {
            0.0
        } else {
            r[3].atan2(r[0]).to_degrees()
        },
        singular,
    })
}

fn compare(
    index: usize,
    root: &Path,
    relative: &Path,
    detector: &faces::Detector,
    model: &PoseModel,
    pose_calls: &mut usize,
) -> Sample {
    let started = Instant::now();
    let mut sample = Sample::empty(index);
    let result = (|| {
        let path = under_root(root, relative).map_err(|_| State::InputFailed)?;
        let bytes = read_bounded(&path, MAX_IMAGE_BYTES).map_err(|_| State::InputFailed)?;
        let (picture, frame) = observe(&mut sample.decode_us, || Picture::from_jpeg_full(&bytes))
            .map_err(|_| State::DecodeFailed)?;
        sample.picture_size = Some([picture.filled.0, picture.filled.1]);
        sample.full_size = Some([frame.width, frame.height]);
        sample.frame_scale = Some(frame.scale);
        let found = observe(&mut sample.detector_us, || detector.find(&picture))
            .map_err(|_| State::DetectorFailed)?;
        sample.face_count = Some(found.len());
        let face = selected_face(&found, &picture)?;
        sample.selected_box_px = Some([face.x, face.y, face.width, face.height]);
        let (crop, planes) = observe(&mut sample.preprocess_us, || {
            let crop = crop_geometry(&frame, face).ok_or(State::PreprocessFailed)?;
            Ok::<_, State>((crop, normalized_planes(&frame, crop)))
        })?;
        sample.crop_xyxy = Some(crop.xyxy);
        sample.resized_size = Some(crop.resized);
        sample.center_offset = Some(crop.center);
        sample.pose_call = Some(*pose_calls);
        *pose_calls += 1;
        let rotation = observe(&mut sample.inference_us, || model.infer(&planes))
            .map_err(|_| State::InferenceFailed)?;
        sample.angles = Some(observe(&mut sample.geometry_us, || {
            rotation_angles(rotation).ok_or(State::InvalidRotation)
        })?);
        Ok::<_, State>(())
    })();
    sample.state = result.err().unwrap_or(State::Measured);
    // Includes root containment, file read, decode, detector and all pose
    // work. Stops before this row's JSON serialization and output flushing.
    sample.total_us = Some(elapsed_us(started));
    sample
}

fn json_line(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn synthetic_tensor(probe: &str) -> Vec<f32> {
    let pixels = SIDE * SIDE;
    match probe {
        "zeros" => vec![0.0; 3 * pixels],
        "ones" => vec![1.0; 3 * pixels],
        "gradient" => (0..3 * pixels)
            .map(|at| {
                // Match numpy.linspace(-2, 2, 224*224, dtype=float32),
                // followed by concatenating [grid, -grid, grid / 2].
                let grid = (-2.0 + (at % pixels) as f64 * (4.0 / (pixels - 1) as f64)) as f32;
                match at / pixels {
                    0 => grid,
                    1 => -grid,
                    _ => grid / 2.0,
                }
            })
            .collect(),
        _ => unreachable!("only the fixed synthetic probes are accepted"),
    }
}

fn synthetic_smoke(
    pose_path: &Path,
    pose_hash: &str,
    output: &mut impl Write,
) -> std::result::Result<(), Failure> {
    let started = Instant::now();
    let model = PoseModel::load(pose_path, pose_hash).map_err(|_| Failure::PoseModel)?;
    let pose_load_us = elapsed_us(started);
    json_line(
        output,
        &serde_json::json!({
            "kind":"metadata", "schema":"lamp-head-pose-synthetic/1",
            "pose_sha256":pose_hash, "pose_bytes":model.bytes,
            "runtime":"tract-onnx-0.23.8", "input_shape":[1,3,SIDE,SIDE],
            "output_shape":[1,3,3], "pose_load_us":pose_load_us,
            "synthetic_only":true, "images_read":0, "implicit_warmups":0,
            "input":"already_normalized_float32_nchw",
            "probes":["zeros","ones","gradient"]
        }),
    )
    .map_err(|_| Failure::Output)?;
    let mut errors = 0usize;
    for (index, probe) in ["zeros", "ones", "gradient"].into_iter().enumerate() {
        let input = synthetic_tensor(probe);
        let input_hash = format!(
            "{:x}",
            Sha256::digest(
                input
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<_>>()
            )
        );
        let started = Instant::now();
        let rotation = model.infer(&input).ok();
        let inference_us = elapsed_us(started);
        let angles = rotation.and_then(rotation_angles);
        let state = if rotation.is_none() {
            State::InferenceFailed
        } else if angles.is_none() {
            State::InvalidRotation
        } else {
            State::Measured
        };
        errors += usize::from(state.is_error());
        json_line(
            output,
            &serde_json::json!({
                "kind":"synthetic_sample", "index":index, "probe":probe,
                "input_le_f32_sha256":input_hash, "state":state,
                "rotation":rotation.filter(|values| values.iter().all(|value| value.is_finite())),
                "angles":angles, "inference_us":inference_us
            }),
        )
        .map_err(|_| Failure::Output)?;
    }
    json_line(
        output,
        &serde_json::json!({
            "kind":"complete", "samples":3, "processing_errors":errors
        }),
    )
    .map_err(|_| Failure::Output)?;
    output.flush().map_err(|_| Failure::Output)?;
    if errors > 0 {
        Err(Failure::Processing)
    } else {
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Usage,
    InputRoot,
    ImageList,
    DetectorModel,
    PoseModel,
    Output,
    Processing,
}

impl Failure {
    fn code(&self) -> &'static str {
        match self {
            Self::Usage => {
                "usage: head_pose_compare DETECTOR POSE_ONNX POSE_SHA256 IMAGE_ROOT IMAGE_LIST.json; or head_pose_compare --synthetic-smoke POSE_ONNX POSE_SHA256"
            }
            Self::InputRoot => "input_root_failed",
            Self::ImageList => "image_list_failed",
            Self::DetectorModel => "detector_model_failed",
            Self::PoseModel => "pose_model_failed",
            Self::Output => "output_failed",
            Self::Processing => "processing_failed: inspect processing_errors",
        }
    }
}

fn run(args: &[OsString], output: &mut impl Write) -> std::result::Result<(), Failure> {
    if let [_, mode, pose_path, pose_hash] = args
        && mode == "--synthetic-smoke"
    {
        let pose_hash = parse_hash(pose_hash).ok_or(Failure::Usage)?;
        return synthetic_smoke(Path::new(pose_path), &pose_hash, output);
    }
    let [
        _,
        detector_path,
        pose_path,
        pose_hash,
        input_root,
        list_path,
    ] = args
    else {
        return Err(Failure::Usage);
    };
    let pose_hash = parse_hash(pose_hash).ok_or(Failure::Usage)?;
    let root = Path::new(input_root)
        .canonicalize()
        .map_err(|_| Failure::InputRoot)?;
    if !root.is_dir() {
        return Err(Failure::InputRoot);
    }
    let list_bytes =
        read_bounded(Path::new(list_path), MAX_LIST_BYTES).map_err(|_| Failure::ImageList)?;
    let paths = image_list(&list_bytes).map_err(|_| Failure::ImageList)?;
    let loaded = Instant::now();
    let stage = Instant::now();
    let detector =
        faces::Detector::load(Path::new(detector_path)).map_err(|_| Failure::DetectorModel)?;
    let detector_load_us = elapsed_us(stage);
    let stage = Instant::now();
    let model =
        PoseModel::load(Path::new(pose_path), &pose_hash).map_err(|_| Failure::PoseModel)?;
    let pose_load_us = elapsed_us(stage);
    let load_us = elapsed_us(loaded);
    json_line(output, &serde_json::json!({
        "kind":"metadata", "schema":"lamp-head-pose-comparison/1",
        "images":paths.len(), "list_sha256":format!("{:x}", Sha256::digest(&list_bytes)),
        "detector_sha256":faces::MODEL_SHA256, "detector_bytes":faces::MODEL_BYTES,
        "pose_sha256":pose_hash, "pose_bytes":model.bytes,
        "runtime":"tract-onnx-0.23.8", "preprocess":PREPROCESS,
        "input_shape":[1,3,SIDE,SIDE], "output_shape":[1,3,3],
        "input":"float32_rgb_nchw_imagenet", "interpolation":"rust_bilinear_pixel_centers_no_antialias_not_pil_bitwise",
        "subject_selection":"exactly_one_full_box", "least_width_share":LEAST_WIDTH_SHARE,
        "coordinates":"raw_6drepnet_euler_pitch_yaw_roll_degrees", "scoring":"none",
        "rotation_tolerance":ROTATION_TOLERANCE,
        "timing_boundary":"input_read_start_to_result_before_json_including_path_resolution",
        "first_image_index":0, "first_pose_call":0, "implicit_warmups":0,
        "capture_age_ms":null, "detector_load_us":detector_load_us,
        "pose_load_us":pose_load_us, "load_us":load_us
    })).map_err(|_| Failure::Output)?;
    let mut errors = 0usize;
    let mut unknown = 0usize;
    let mut pose_calls = 0usize;
    for (index, relative) in paths.iter().enumerate() {
        let sample = compare(index, &root, relative, &detector, &model, &mut pose_calls);
        errors += usize::from(sample.state.is_error());
        unknown += usize::from(sample.state != State::Measured);
        json_line(output, &sample).map_err(|_| Failure::Output)?;
    }
    json_line(
        output,
        &serde_json::json!({
            "kind":"complete", "samples":paths.len(), "pose_calls":pose_calls,
            "unknown":unknown, "processing_errors":errors
        }),
    )
    .map_err(|_| Failure::Output)?;
    output.flush().map_err(|_| Failure::Output)?;
    if errors > 0 {
        Err(Failure::Processing)
    } else {
        Ok(())
    }
}

fn main() -> ExitCode {
    let mut output = io::BufWriter::new(io::stdout().lock());
    match run(&std::env::args_os().collect::<Vec<_>>(), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            // Never print underlying parser, I/O or inference errors: they
            // may contain a private path or a model's arbitrary text.
            eprintln!("head_pose_compare: {}", failure.code());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, scale: f32) -> Frame {
        Frame {
            rgb: vec![0; width * height * 3],
            width,
            height,
            scale,
        }
    }

    fn face(x: f32, y: f32, width: f32, height: f32) -> faces::Face {
        faces::Face {
            score: 0.99,
            x,
            y,
            width,
            height,
            marks: [(0.0, 0.0); 5],
        }
    }

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
    }

    #[test]
    fn conservative_subject_selection_keeps_unknowns_out_of_inference() {
        let picture = Picture {
            planes: Vec::new(),
            filled: (320, 240),
        };
        let good = face(30.0, 40.0, 24.0, 45.0);
        assert_eq!(selected_face(&[], &picture), Err(State::NoFace));
        assert_eq!(
            selected_face(&[good, good], &picture),
            Err(State::AmbiguousFace)
        );
        assert_eq!(
            selected_face(&[face(0.0, 0.0, 23.99, 45.0)], &picture),
            Err(State::TooSmall)
        );
        assert_eq!(
            selected_face(&[face(-0.01, 0.0, 30.0, 45.0)], &picture),
            Err(State::ClippedFace)
        );
        assert_eq!(
            selected_face(&[face(300.0, 0.0, 30.0, 45.0)], &picture),
            Err(State::ClippedFace)
        );
        assert_eq!(
            selected_face(&[face(0.0, 220.0, 30.0, 45.0)], &picture),
            Err(State::ClippedFace)
        );
        assert_eq!(
            selected_face(&[face(0.0, 0.0, f32::NAN, 45.0)], &picture),
            Err(State::InvalidBox)
        );
        assert_eq!(selected_face(&[good], &picture), Ok(&good));
    }

    #[test]
    fn crop_maps_exact_frame_scale_truncates_then_cross_pads() {
        let full = frame(1_000, 750, 0.32);
        let crop = crop_geometry(&full, &face(32.32, 32.32, 64.16, 32.16)).unwrap();
        // Truncated full box: [101,101,301,201]. Pad x=20, y=40.
        assert_eq!(crop.xyxy, [81, 61, 321, 241]);
        assert_eq!(crop.resized, [298, 224]);
        assert_eq!(crop.center, [37, 0]);
        let clipped_pad = crop_geometry(&full, &face(0.0, 0.0, 64.0, 32.0)).unwrap();
        assert_eq!(clipped_pad.xyxy, [0, 0, 220, 140]);
    }

    #[test]
    fn crop_geometry_refuses_bad_frames_and_collapsed_boxes() {
        let mut full = frame(320, 240, 1.0);
        assert!(crop_geometry(&full, &face(0.0, 0.0, 0.5, 40.0)).is_none());
        assert!(crop_geometry(&full, &face(f32::NAN, 0.0, 40.0, 40.0)).is_none());
        assert!(crop_geometry(&full, &face(310.0, 0.0, 40.0, 40.0)).is_none());
        full.scale = 0.0;
        assert!(crop_geometry(&full, &face(0.0, 0.0, 40.0, 40.0)).is_none());
        full.scale = 1.0;
        full.rgb.pop();
        assert!(crop_geometry(&full, &face(0.0, 0.0, 40.0, 40.0)).is_none());
    }

    #[test]
    fn center_crop_matches_python_ties_to_even_for_both_parities() {
        assert_eq!(center_offset(223), None);
        for (extra, expected) in [(0, 0), (1, 0), (2, 1), (3, 2), (5, 2), (7, 4)] {
            assert_eq!(center_offset(SIDE + extra), Some(expected));
        }
    }

    #[test]
    fn preprocessing_preserves_rgb_planes_and_imagenet_normalization() {
        let mut full = frame(224, 224, 1.0);
        for rgb in full.rgb.chunks_exact_mut(3) {
            rgb.copy_from_slice(&[255, 127, 0]);
        }
        let crop = Crop {
            xyxy: [0, 0, 224, 224],
            resized: [224, 224],
            center: [0, 0],
        };
        let planes = normalized_planes(&full, crop);
        assert_eq!(planes.len(), 3 * SIDE * SIDE);
        for channel in 0..3 {
            let expected = ([1.0, 127.0 / 255.0, 0.0][channel] - MEAN[channel]) / STD[channel];
            for offset in [0, SIDE * SIDE / 2, SIDE * SIDE - 1] {
                close(planes[channel * SIDE * SIDE + offset], expected);
            }
        }
    }

    #[test]
    fn bilinear_sampler_uses_pixel_centers_and_clamps_crop_edges() {
        let mut full = frame(2, 2, 1.0);
        full.rgb = vec![0, 0, 0, 100, 0, 0, 0, 100, 0, 100, 100, 0];
        let crop = Crop {
            xyxy: [0, 0, 2, 2],
            resized: [224, 224],
            center: [0, 0],
        };
        let planes = normalized_planes(&full, crop);
        let unnormalize = |channel: usize, at: usize| {
            (planes[channel * SIDE * SIDE + at] * STD[channel] + MEAN[channel]) * 255.0
        };
        close(unnormalize(0, 0), 0.0);
        close(unnormalize(0, SIDE - 1), 100.0);
        close(unnormalize(1, (SIDE - 1) * SIDE), 100.0);
        close(unnormalize(0, 111), 100.0 * (111.5 * 2.0 / 224.0 - 0.5));
        close(
            unnormalize(1, 111 * SIDE),
            100.0 * (111.5 * 2.0 / 224.0 - 0.5),
        );
    }

    fn rotation(pitch: f32, yaw: f32, roll: f32) -> [f32; 9] {
        let (sx, cx) = pitch.to_radians().sin_cos();
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sz, cz) = roll.to_radians().sin_cos();
        // Rz(roll) * Ry(yaw) * Rx(pitch).
        [
            cy * cz,
            sx * sy * cz - cx * sz,
            cx * sy * cz + sx * sz,
            cy * sz,
            sx * sy * sz + cx * cz,
            cx * sy * sz - sx * cz,
            -sy,
            sx * cy,
            cx * cy,
        ]
    }

    #[test]
    fn euler_matches_upstream_axis_signs_and_retains_large_pitch() {
        for (pitch, yaw, roll) in [(0.0, 0.0, 0.0), (30.0, -25.0, 10.0), (-135.0, 40.0, -45.0)] {
            let angles = rotation_angles(rotation(pitch, yaw, roll)).unwrap();
            assert!(!angles.singular);
            close(angles.pitch_deg, pitch);
            close(angles.yaw_deg, yaw);
            close(angles.roll_deg, roll);
        }
    }

    #[test]
    fn singular_euler_uses_upstream_branch_and_zero_roll() {
        for yaw in [-90.0, 90.0] {
            let angles = rotation_angles(rotation(25.0, yaw, 0.0)).unwrap();
            assert!(angles.singular);
            close(angles.pitch_deg, 25.0);
            close(angles.yaw_deg, yaw);
            close(angles.roll_deg, 0.0);
        }
    }

    #[test]
    fn invalid_rotations_never_become_angles() {
        for bad in [
            [0.0; 9],
            [f32::NAN; 9],
            [f32::INFINITY; 9],
            [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            [1.0, 0.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        ] {
            assert!(rotation_angles(bad).is_none());
        }
    }

    #[test]
    fn list_bounds_and_paths_are_checked_before_model_loading() {
        for invalid in [
            r#"[]"#,
            r#"["/private/canary.jpg"]"#,
            r#"["../canary.jpg"]"#,
            r#"["nested/../../canary.jpg"]"#,
            r#"["canary.png"]"#,
            r#"["a.jpg","a.jpg"]"#,
        ] {
            assert!(image_list(invalid.as_bytes()).is_err());
        }
        assert!(image_list(&vec![b' '; MAX_LIST_BYTES + 1]).is_err());
        let many = serde_json::to_vec(
            &(0..=MAX_IMAGES)
                .map(|i| format!("{i}.jpg"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(image_list(&many).is_err());
        assert_eq!(
            image_list(br#"["nested/a.JPG","b.jpeg"]"#).unwrap(),
            [PathBuf::from("nested/a.JPG"), PathBuf::from("b.jpeg")]
        );
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("head-pose-unit-{}-{nonce}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bounded_files_and_model_digest_fail_before_any_onnx_parse() {
        let dir = Scratch::new();
        let path = dir.0.join("invalid-model.onnx");
        std::fs::write(&path, b"not an onnx model").unwrap();
        assert!(read_bounded(&path, 3).is_err());
        assert!(read_bounded(&dir.0, MAX_MODEL_BYTES).is_err());
        let wrong = "0".repeat(64);
        let error = PoseModel::load(&path, &wrong).err().unwrap();
        assert_eq!(error.to_string(), "pose_model_hash_mismatch");
        let correct = format!("{:x}", Sha256::digest(b"not an onnx model"));
        assert_eq!(
            verified_model_bytes(&path, &correct).unwrap(),
            b"not an onnx model"
        );
        assert!(parse_hash(&OsString::from("private-canary")).is_none());
        assert_eq!(
            parse_hash(&OsString::from("A".repeat(64))),
            Some("a".repeat(64))
        );
    }

    #[cfg(unix)]
    #[test]
    fn input_symlink_cannot_escape_canonical_root() {
        let dir = Scratch::new();
        let root = dir.0.join("root");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.0.join("outside.jpg");
        std::fs::write(&outside, b"synthetic").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape.jpg")).unwrap();
        assert!(under_root(&root.canonicalize().unwrap(), Path::new("escape.jpg")).is_err());
        assert!(read_bounded(&root.join("escape.jpg"), 100).is_err());
    }

    #[test]
    fn failed_stages_keep_elapsed_time_without_filling_unattempted_stages() {
        let mut duration = None;
        let result: std::result::Result<(), State> =
            observe(&mut duration, || Err(State::DecodeFailed));
        assert_eq!(result, Err(State::DecodeFailed));
        assert!(duration.is_some());
        let sample = Sample::empty(9);
        assert!(sample.face_count.is_none());
        assert!(sample.pose_call.is_none());
        assert!(sample.angles.is_none());
        assert!(sample.inference_us.is_none());
        assert!(State::InferenceFailed.is_error());
        assert!(!State::NoFace.is_error());
    }

    #[test]
    fn synthetic_probes_are_fixed_normalized_tensors_without_images() {
        assert!(synthetic_tensor("zeros").iter().all(|value| *value == 0.0));
        assert!(synthetic_tensor("ones").iter().all(|value| *value == 1.0));
        let gradient = synthetic_tensor("gradient");
        let pixels = SIDE * SIDE;
        assert_eq!(gradient.len(), 3 * pixels);
        close(gradient[0], -2.0);
        close(gradient[pixels - 1], 2.0);
        for at in [0, 103, pixels / 2, pixels - 1] {
            close(gradient[pixels + at], -gradient[at]);
            close(gradient[2 * pixels + at], gradient[at] / 2.0);
        }
        // Independently generated with NumPy 1.26.4, float32 little endian.
        for (probe, expected) in [
            (
                "zeros",
                "ad8c5f4d30e45b2a50f7f4d1f347f64d613c6fbcd92c54ca746ab077f402d8cc",
            ),
            (
                "ones",
                "fb446ff86f61c7dfadb0cc01a6ad2d5923a13392df002e8e869c9ee50ba0aafa",
            ),
            (
                "gradient",
                "82ef2544f152099149b643d19b630116dac6b581755d361b861d0e0d05237c49",
            ),
        ] {
            let bytes: Vec<u8> = synthetic_tensor(probe)
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected);
        }
    }

    #[test]
    fn startup_failure_and_sample_json_do_not_echo_private_strings() {
        let canary = "PRIVATE_PATH_AND_WORDS_CANARY";
        let args = [
            "head_pose_compare",
            canary,
            canary,
            &"0".repeat(64),
            canary,
            canary,
        ]
        .map(OsString::from);
        let mut output = Vec::new();
        let failure = run(&args, &mut output).unwrap_err();
        assert_eq!(failure, Failure::InputRoot);
        assert!(output.is_empty());
        assert!(!failure.code().contains(canary));
        let sample = serde_json::to_string(&Sample::empty(0)).unwrap();
        assert!(!sample.contains(canary));
        assert!(!sample.contains("path"));
        assert!(!sample.contains("transcript"));
    }
}
