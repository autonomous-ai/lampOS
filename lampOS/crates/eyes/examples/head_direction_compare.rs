//! Compare legacy marks and the isolated #8782 geometry on the same product
//! detector/crop/mesh pipeline. This is an offline producer of measurements,
//! not a scorer or a runtime sensor. No frames or paths are written to output.
//! Usage: head_direction_compare DETECTOR MESH IMAGE_ROOT IMAGE_LIST.json
//! IMAGE_LIST is a bounded JSON array of relative JPEG paths. Keep it private.
use lamp_eyes::{faces, facing, mesh, picture::Picture};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

#[path = "head_direction/geometry.rs"]
mod geometry;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_LIST_BYTES: usize = 1_048_576;
const MAX_IMAGE_BYTES: usize = 8_388_608;
const MAX_IMAGES: usize = 10_000;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum State {
    Processed,
    Measured,
    NoFace,
    TooSmall,
    OutsideFrame,
    ClippedFace,
    InvalidGeometry,
    LegacySentinel,
    TurnTooLarge,
    InputFailed,
    DecodeFailed,
    DetectorFailed,
    CropFailed,
    MeshFailed,
}

#[derive(Debug, Serialize)]
struct Angles {
    yaw_deg: Option<f32>,
    pitch_deg: Option<f32>,
    yaw_state: State,
    pitch_state: State,
}

impl Angles {
    fn unknown(state: State) -> Self {
        Self {
            yaw_deg: None,
            pitch_deg: None,
            yaw_state: state,
            pitch_state: state,
        }
    }
}

#[derive(Debug, Serialize)]
struct Sample {
    kind: &'static str,
    index: usize,
    state: State,
    face_count: usize,
    selected_box_px: Option<[f32; 4]>,
    picture_size: Option<[usize; 2]>,
    marks: Angles,
    mesh: Angles,
    decode_us: Option<u64>,
    detector_us: Option<u64>,
    crop_mesh_us: Option<u64>,
    geometry_us: Option<u64>,
    total_us: Option<u64>,
}

impl Sample {
    fn empty(index: usize, state: State) -> Self {
        Self {
            kind: "sample",
            index,
            state,
            face_count: 0,
            selected_box_px: None,
            picture_size: None,
            marks: Angles::unknown(state),
            mesh: Angles::unknown(state),
            decode_us: None,
            detector_us: None,
            crop_mesh_us: None,
            geometry_us: None,
            total_us: None,
        }
    }
}

fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn observe_stage<T, E>(
    duration: &mut Option<u64>,
    work: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let start = Instant::now();
    let result = work();
    *duration = Some(elapsed_us(start));
    result
}

fn read_bounded(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let metadata = path.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input is not a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input grew beyond its bound",
        ));
    }
    Ok(bytes)
}

fn image_list(bytes: &[u8]) -> Result<Vec<PathBuf>> {
    if bytes.len() > MAX_LIST_BYTES {
        return Err("image list exceeds its byte bound".into());
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(bytes)?;
    if paths.is_empty() || paths.len() > MAX_IMAGES {
        return Err("image list count is outside 1..=10000".into());
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
            return Err(
                "image paths must be unique relative JPEG paths without parent traversal".into(),
            );
        }
    }
    Ok(paths)
}

fn under_root(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "image is outside the input root",
        ));
    }
    Ok(path)
}

fn visibility(face: &faces::Face, picture: &Picture) -> Option<State> {
    if face.width / (picture.filled.0.max(1) as f32) < facing::LEAST_WIDTH {
        Some(State::TooSmall)
    } else if !facing::marks_seen(
        &face.marks,
        picture.filled.0 as f32,
        picture.filled.1 as f32,
    ) {
        Some(State::OutsideFrame)
    } else {
        None
    }
}

fn candidate_visibility(face: &faces::Face, picture: &Picture) -> Option<State> {
    visibility(face, picture).or_else(|| {
        (face.x < 0.0
            || face.y < 0.0
            || face.x + face.width > picture.filled.0 as f32
            || face.y + face.height > picture.filled.1 as f32)
            .then_some(State::ClippedFace)
    })
}

fn legacy(face: &faces::Face, picture: &Picture) -> Angles {
    if let Some(reason) = visibility(face, picture) {
        return Angles::unknown(reason);
    }
    let yaw = facing::yaw_deg(&face.marks);
    let sign = if face.marks[2].0 >= (face.marks[0].0 + face.marks[1].0) / 2.0 {
        1.0
    } else {
        -1.0
    };
    let pitch = facing::pitch_deg(&face.marks);
    // Preserve the production formula, but its +90 sentinel is not an
    // observed downward head angle. Pitch is a counterfactual here: its
    // product switch is off, and this example does not change that switch.
    let pitch_state = match pitch {
        Some(value) if value == facing::LOST_DEG => State::LegacySentinel,
        Some(value) if value.is_finite() => State::Measured,
        None if yaw.is_some_and(|value| value > facing::PITCH_MOST_YAW_DEG) => State::TurnTooLarge,
        _ => State::InvalidGeometry,
    };
    Angles {
        yaw_deg: yaw
            .filter(|value| value.is_finite())
            .map(|value| value * sign),
        pitch_deg: (pitch_state == State::Measured).then_some(pitch).flatten(),
        yaw_state: if yaw.is_some_and(f32::is_finite) {
            State::Measured
        } else {
            State::InvalidGeometry
        },
        pitch_state,
    }
}

fn compare(
    index: usize,
    path: &Path,
    detector: &faces::Detector,
    model: &mut mesh::Mesh,
) -> Sample {
    let started = Instant::now();
    let mut result = Sample::empty(index, State::InputFailed);
    let work = (|| {
        let bytes = read_bounded(path, MAX_IMAGE_BYTES).map_err(|_| State::InputFailed)?;
        let picture = observe_stage(&mut result.decode_us, || Picture::from_jpeg(&bytes))
            .map_err(|_| State::DecodeFailed)?;
        result.picture_size = Some([picture.filled.0, picture.filled.1]);
        let found = observe_stage(&mut result.detector_us, || detector.find(&picture))
            .map_err(|_| State::DetectorFailed)?;
        result.face_count = found.len();
        let selected = mesh::meshed_order(
            &found.iter().map(|face| face.height).collect::<Vec<_>>(),
            false,
        );
        let Some(face) = selected.first().and_then(|index| found.get(*index)) else {
            return Err(State::NoFace);
        };
        result.selected_box_px = Some([face.x, face.y, face.width, face.height]);
        result.marks = legacy(face, &picture);
        let landmarks = observe_stage(&mut result.crop_mesh_us, || {
            let crop = mesh::crop(&picture, face).ok_or(State::CropFailed)?;
            model.landmarks_of(crop).map_err(|_| State::MeshFailed)
        })?;
        let stage = Instant::now();
        // Same subject and frame, with a conservative additional candidate
        // framing check. Never fall back to marks or invent a target label.
        result.mesh = if let Some(reason) = candidate_visibility(face, &picture) {
            Angles::unknown(reason)
        } else if let Some(head) = geometry::mesh_head(&landmarks) {
            Angles {
                yaw_deg: Some(head.yaw_deg),
                pitch_deg: Some(head.pitch_deg),
                yaw_state: State::Measured,
                pitch_state: State::Measured,
            }
        } else {
            Angles::unknown(State::InvalidGeometry)
        };
        result.geometry_us = Some(elapsed_us(stage));
        Ok::<(), State>(())
    })();
    result.state = match work {
        Ok(()) => State::Processed,
        Err(reason) => {
            if result.selected_box_px.is_none() {
                result.marks = Angles::unknown(reason);
            }
            result.mesh = Angles::unknown(reason);
            reason
        }
    };
    result.total_us = Some(elapsed_us(started));
    result
}

fn json_line(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Usage,
    InputRoot,
    ImageList,
    DetectorModel,
    MeshModel,
    Output,
    Processing,
}

impl Failure {
    fn code(&self) -> &'static str {
        match self {
            Self::Usage => "usage: head_direction_compare DETECTOR MESH IMAGE_ROOT IMAGE_LIST.json",
            Self::InputRoot => "input_root_failed",
            Self::ImageList => "image_list_failed",
            Self::DetectorModel => "detector_model_failed",
            Self::MeshModel => "mesh_model_failed",
            Self::Output => "output_failed",
            Self::Processing => "processing_failed: inspect processing_errors",
        }
    }
}

fn run(args: &[OsString], output: &mut impl Write) -> std::result::Result<(), Failure> {
    let [_, detector_path, mesh_path, input_root, list_path] = args else {
        return Err(Failure::Usage);
    };
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
    let detector =
        faces::Detector::load(Path::new(detector_path)).map_err(|_| Failure::DetectorModel)?;
    let mut model = mesh::Mesh::load(Path::new(mesh_path)).map_err(|_| Failure::MeshModel)?;
    let load_us = elapsed_us(loaded);
    json_line(
        output,
        &serde_json::json!({
            "kind":"metadata", "schema":"lamp-head-direction-comparison/1",
            "images":paths.len(), "list_sha256":format!("{:x}", Sha256::digest(&list_bytes)),
            "detector_sha256":faces::MODEL_SHA256, "mesh_sha256":mesh::MODEL_SHA256,
            "candidate":"mesh_head_3a9ae0692_checked", "crop":"product_picture_rotated_1_5",
            "subject_selection":"product_largest_height", "coordinates":"degrees_camera_right_down",
            "runtime":"tract", "legacy_pitch":"counterfactual_product_switch_off",
            "timing_boundary":"file_read_start_to_result", "capture_age_ms":null, "load_us":load_us,
            "scoring":"none"
        }),
    )
    .map_err(|_| Failure::Output)?;
    let mut errors = 0usize;
    for (index, relative) in paths.iter().enumerate() {
        let sample = match under_root(&root, relative) {
            Ok(path) => compare(index, &path, &detector, &mut model),
            Err(_) => Sample::empty(index, State::InputFailed),
        };
        if matches!(
            sample.state,
            State::InputFailed
                | State::DecodeFailed
                | State::DetectorFailed
                | State::CropFailed
                | State::MeshFailed
        ) {
            errors += 1;
        }
        json_line(output, &sample).map_err(|_| Failure::Output)?;
    }
    json_line(
        output,
        &serde_json::json!({"kind":"complete","samples":paths.len(),"processing_errors":errors}),
    )
    .map_err(|_| Failure::Output)?;
    output.flush().map_err(|_| Failure::Output)?;
    if errors > 0 {
        return Err(Failure::Processing);
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut output = io::BufWriter::new(io::stdout().lock());
    match run(&std::env::args_os().collect::<Vec<_>>(), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("head_direction_compare: {}", failure.code());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempted_failure_keeps_duration_while_unattempted_stages_are_null() {
        let mut duration = None;
        let result: std::result::Result<(), State> = observe_stage(&mut duration, || {
            std::thread::sleep(std::time::Duration::from_millis(1));
            Err(State::DecodeFailed)
        });
        assert_eq!(result, Err(State::DecodeFailed));
        assert!(duration.is_some_and(|value| value >= 1_000));
        let sample = Sample::empty(0, State::InputFailed);
        let value = serde_json::to_value(sample).unwrap();
        for field in [
            "decode_us",
            "detector_us",
            "crop_mesh_us",
            "geometry_us",
            "total_us",
        ] {
            assert_eq!(value[field], serde_json::Value::Null);
        }
    }

    #[test]
    fn clipped_face_is_unknown_even_with_visible_eye_and_nose_marks() {
        let picture = Picture {
            planes: Vec::new(),
            filled: (320, 240),
        };
        let mut face = faces::Face {
            score: 0.99,
            x: 20.0,
            y: -10.0,
            width: 120.0,
            height: 120.0,
            marks: [
                (40.0, 40.0),
                (100.0, 40.0),
                (70.0, 70.0),
                (50.0, 90.0),
                (90.0, 90.0),
            ],
        };
        assert_eq!(visibility(&face, &picture), None);
        assert_eq!(
            candidate_visibility(&face, &picture),
            Some(State::ClippedFace)
        );
        assert_eq!(legacy(&face, &picture).yaw_state, State::Measured);
        face.y = 10.0;
        assert_eq!(candidate_visibility(&face, &picture), None);
        face.y = 200.0;
        assert_eq!(
            candidate_visibility(&face, &picture),
            Some(State::ClippedFace)
        );
    }

    #[test]
    fn startup_failures_do_not_repeat_private_paths_or_json_values() {
        let canary = "PRIVATE_PATH_MUST_NOT_APPEAR";
        let root = std::env::temp_dir().join(format!(
            "head-direction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let list = root.join("input.json");
        let private_model = root.join(canary);
        let args = [
            OsString::from("head_direction_compare"),
            private_model.as_os_str().to_owned(),
            private_model.as_os_str().to_owned(),
            root.as_os_str().to_owned(),
            list.as_os_str().to_owned(),
        ];
        std::fs::write(&list, format!(r#"{{"private":"{canary}"}}"#)).unwrap();
        let mut output = Vec::new();
        let failure = run(&args, &mut output).unwrap_err();
        assert_eq!(failure, Failure::ImageList);
        assert_eq!(failure.code(), "image_list_failed");
        assert!(output.is_empty());
        std::fs::write(&list, r#"["frame.jpg"]"#).unwrap();
        let failure = run(&args, &mut output).unwrap_err();
        assert_eq!(failure, Failure::DetectorModel);
        assert!(!failure.code().contains(canary));
        assert!(output.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn image_list_preserves_order_and_rejects_duplicates_and_escapes() {
        assert_eq!(
            image_list(br#"["b.jpg","folder/a.jpeg"]"#).unwrap(),
            [PathBuf::from("b.jpg"), PathBuf::from("folder/a.jpeg")]
        );
        for invalid in [
            r#"[]"#,
            r#"["a.jpg","a.jpg"]"#,
            r#"["../a.jpg"]"#,
            r#"["/a.jpg"]"#,
            r#"["a.txt"]"#,
            r#"[""]"#,
            r#"[3]"#,
        ] {
            assert!(image_list(invalid.as_bytes()).is_err(), "{invalid}");
        }
        assert!(image_list(&vec![b' '; MAX_LIST_BYTES + 1]).is_err());
    }

    #[test]
    fn unread_legacy_pitch_stays_unknown_in_the_measurement_output() {
        let picture = Picture {
            planes: Vec::new(),
            filled: (320, 240),
        };
        let face = faces::Face {
            score: 0.9,
            x: 20.0,
            y: 20.0,
            width: 120.0,
            height: 120.0,
            marks: [
                (40.0, 50.0),
                (100.0, 50.0),
                (70.0, 100.0),
                (50.0, 20.0),
                (90.0, 20.0),
            ],
        };
        let result = legacy(&face, &picture);
        assert_eq!(result.pitch_state, State::LegacySentinel);
        assert_eq!(result.pitch_deg, None);
        assert_eq!(result.yaw_deg, Some(0.0));
    }

    #[test]
    fn small_and_unframed_faces_do_not_become_away_measurements() {
        let picture = Picture {
            planes: Vec::new(),
            filled: (320, 240),
        };
        let mut face = faces::Face {
            score: 0.99,
            x: 20.0,
            y: 20.0,
            width: 20.0,
            height: 20.0,
            marks: [(20.0, 20.0); 5],
        };
        assert_eq!(legacy(&face, &picture).yaw_state, State::TooSmall);
        face.width = 100.0;
        face.marks[2].0 = -1.0;
        assert_eq!(legacy(&face, &picture).yaw_state, State::OutsideFrame);
    }

    #[test]
    fn failed_rows_remain_explicit_and_do_not_export_paths_or_landmarks() {
        let mut output = Vec::new();
        json_line(&mut output, &Sample::empty(7, State::DecodeFailed)).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["index"], 7);
        assert_eq!(value["marks"]["yaw_deg"], serde_json::Value::Null);
        assert_eq!(value["mesh"]["yaw_state"], "decode_failed");
        assert!(value.get("path").is_none());
        assert!(value.get("landmarks").is_none());
    }
}
