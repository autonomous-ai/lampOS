//! Offline component replay of the current mouth path; see docs/mouth-baseline-bench.md.
//! Inputs never include labels, supplied boxes, identities, or live boot identifiers.
use lamp_eyes::{
    bus_out::{self, Ids},
    faces::{self, Detector, Face},
    facing, mesh,
    picture::{self, Picture},
    speaking::{self, MouthDiagnostic, Mouths},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ROW_BYTES: usize = 8192;
const MAX_FRAMES: usize = 100_000;
const MAX_JPEG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const USAGE: &str = "mouth_baseline_bench --root ROOT --input MANIFEST --yunet MODEL --mesh MODEL [--ort-runtime LIBRARY]\nRequires LAMP_EYES_SPEAKING=on and LAMP_EYES_SPEAKING_STEADY=on.\nJSONL on stdout; fixed diagnostics on stderr. No downloads or camera access.\n";

type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Failure {
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<usize>,
}

impl Failure {
    fn new(code: &'static str) -> Self {
        Self { code, row: None }
    }

    fn at(mut self, row: usize) -> Self {
        self.row = Some(row);
        self
    }
}

#[derive(Debug)]
struct Args {
    root: PathBuf,
    input: PathBuf,
    yunet: PathBuf,
    mesh: PathBuf,
    ort_runtime: Option<PathBuf>,
}

fn arguments(args: impl IntoIterator<Item = OsString>) -> Result<Option<Args>> {
    let args: Vec<_> = args.into_iter().collect();
    if args.len() == 1 && args[0] == "--help" {
        return Ok(None);
    }
    let (mut root, mut input, mut yunet, mut mesh, mut ort_runtime) =
        (None, None, None, None, None);
    if args.len() % 2 != 0 {
        return Err(Failure::new("arguments_invalid"));
    }
    for pair in args.chunks_exact(2) {
        let slot = match pair[0].to_str() {
            Some("--root") => &mut root,
            Some("--input") => &mut input,
            Some("--yunet") => &mut yunet,
            Some("--mesh") => &mut mesh,
            Some("--ort-runtime") => &mut ort_runtime,
            _ => return Err(Failure::new("arguments_invalid")),
        };
        if slot.is_some() || pair[1].is_empty() {
            return Err(Failure::new("arguments_invalid"));
        }
        *slot = Some(PathBuf::from(&pair[1]));
    }
    Ok(Some(Args {
        root: root.ok_or(Failure::new("arguments_invalid"))?,
        input: input.ok_or(Failure::new("arguments_invalid"))?,
        yunet: yunet.ok_or(Failure::new("arguments_invalid"))?,
        mesh: mesh.ok_or(Failure::new("arguments_invalid"))?,
        ort_runtime,
    }))
}

#[derive(Clone, Debug, Serialize)]
struct Config {
    #[serde(rename = "LAMP_EYES_SPEAKING")]
    speaking: bool,
    #[serde(rename = "LAMP_EYES_SPEAKING_STEADY")]
    steady: bool,
    #[serde(rename = "LAMP_EYES_SPEAKING_ALL_FACES")]
    all_faces: bool,
    #[serde(rename = "LAMP_EYES_SPEAKING_DROP_OUTLIER")]
    drop_outlier: bool,
    #[serde(rename = "LAMP_EYES_SPEAKING_SKIP_JUMP")]
    skip_jump: bool,
    #[serde(rename = "LAMP_EYES_MOUTH_FULL_LOOK")]
    full_look: bool,
}

fn config(mut read: impl FnMut(&str) -> Option<OsString>) -> Result<Config> {
    let mut switch = |key, default| match read(key).as_deref() {
        None => Ok(default),
        Some(value) if value == "on" => Ok(true),
        Some(value) if value == "off" => Ok(false),
        _ => Err(Failure::new("switch_invalid")),
    };
    let config = Config {
        speaking: switch(speaking::ENVIRONMENT, false)?,
        steady: switch(speaking::STEADY_ENVIRONMENT, false)?,
        all_faces: switch(mesh::ALL_FACES_ENVIRONMENT, false)?,
        drop_outlier: switch(speaking::OUTLIER_ENVIRONMENT, false)?,
        skip_jump: switch(speaking::SKIP_JUMP_ENVIRONMENT, false)?,
        full_look: switch(mesh::FULL_LOOK_ENVIRONMENT, false)?,
    };
    if !config.speaking
        || !config.steady
        || config.all_faces
        || config.drop_outlier
        || config.skip_jump
    {
        return Err(Failure::new("baseline_switches_required"));
    }
    Ok(config)
}

/// Reject traversal even if canonicalization would land back inside the root.
fn path_syntax(path: &Path, relative: bool) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    !text.is_empty()
        && text.len() <= MAX_PATH_BYTES
        && !text.chars().any(char::is_control)
        && (!relative || !path.is_absolute())
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::RootDir))
}

fn canonical_root(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || !path_syntax(path, false) {
        return Err(Failure::new("root_invalid"));
    }
    let root = fs::canonicalize(path).map_err(|_| Failure::new("root_unavailable"))?;
    if !root.is_dir() {
        return Err(Failure::new("root_invalid"));
    }
    Ok(root)
}

fn confined(root: &Path, path: &Path) -> Result<PathBuf> {
    if !path_syntax(path, false) {
        return Err(Failure::new("path_invalid"));
    }
    let canonical =
        fs::canonicalize(root.join(path)).map_err(|_| Failure::new("file_unavailable"))?;
    if !canonical.starts_with(root) || canonical == root {
        return Err(Failure::new("path_outside_root"));
    }
    Ok(canonical)
}

/// O_NONBLOCK prevents FIFOs from hanging before the regular-file check.
fn open_regular(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::new("file_unavailable"))?;
    if !file
        .metadata()
        .map_err(|_| Failure::new("file_unavailable"))?
        .is_file()
    {
        return Err(Failure::new("regular_file_required"));
    }
    Ok(file)
}

fn bounded_bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = open_regular(path)?;
    let size = file
        .metadata()
        .map_err(|_| Failure::new("file_unavailable"))?
        .len();
    if size > limit {
        return Err(Failure::new("file_limit_exceeded"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::new("file_read_failed"))?;
    if bytes.len() as u64 > limit {
        return Err(Failure::new("file_limit_exceeded"));
    }
    if bytes.len() as u64 != size {
        return Err(Failure::new("file_changed"));
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: u64,
    at_us: u64,
    jpeg: PathBuf,
    reset: bool,
    head_moving: Option<bool>,
}

fn manifest(bytes: &[u8]) -> Result<Vec<Input>> {
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Failure::new("manifest_limit_exceeded"));
    }
    let mut inputs = Vec::new();
    let mut ids = BTreeSet::new();
    let mut previous = None;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let row = index + 1;
        if row > MAX_FRAMES || line.len() > MAX_ROW_BYTES {
            return Err(Failure::new("manifest_limit_exceeded").at(row));
        }
        let input: Input = serde_json::from_slice(line)
            .map_err(|_| Failure::new("manifest_row_invalid").at(row))?;
        if !path_syntax(&input.jpeg, true) {
            return Err(Failure::new("jpeg_path_invalid").at(row));
        }
        if !ids.insert(input.id) {
            return Err(Failure::new("duplicate_input_id").at(row));
        }
        if !input.reset && previous.is_none_or(|at| input.at_us <= at) {
            return Err(Failure::new("source_time_requires_reset").at(row));
        }
        previous = Some(input.at_us);
        inputs.push(input);
    }
    if inputs.is_empty() {
        return Err(Failure::new("manifest_empty"));
    }
    Ok(inputs)
}

#[derive(Serialize)]
struct Asset {
    bytes: u64,
    sha256: String,
}

fn pinned_asset(path: &Path, size: u64, digest: &str, code: &'static str) -> Result<Asset> {
    let bytes = bounded_bytes(path, size).map_err(|_| Failure::new(code))?;
    let actual = sha256(&bytes);
    if bytes.len() as u64 != size || actual != digest {
        return Err(Failure::new(code));
    }
    Ok(Asset {
        bytes: size,
        sha256: actual,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MeshStatus {
    NotSelected,
    CropUnavailable,
    MouthUnavailable,
    Read,
}

#[derive(Debug, Serialize)]
struct FaceRecord {
    detection_index: usize,
    temporary_id: u32,
    box_px: [f32; 4],
    box_normalized: [f32; 4],
    detection_score: f32,
    mesh_status: MeshStatus,
    /// None only for the empty clipped box that bus_out::message also skips.
    decision: Option<MouthDiagnostic>,
    skipped_reason: Option<&'static str>,
}

fn new_mouths() -> Result<Mouths> {
    speaking::from_environment().ok_or(Failure::new("baseline_switches_required"))
}

/// Only the bus adapter is repeated: the tracker, geometry and judge are production.
/// The test below compares this call sequence against bus_out::message, face by face.
fn judge_faces(
    ids: &mut Ids,
    mouths: &mut Mouths,
    found: &[Face],
    opens: &[Option<f32>],
    statuses: &[MeshStatus],
    filled: (usize, usize),
    at_us: u64,
) -> Vec<FaceRecord> {
    let boxes: Vec<_> = found
        .iter()
        .map(|face| bus_out::edges(face, filled))
        .collect();
    let numbers = ids.of(&boxes, at_us);
    let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
    found
        .iter()
        .zip(boxes)
        .zip(numbers)
        .enumerate()
        .map(|(index, ((face, edges), id))| {
            let visible = edges[0] < edges[2] && edges[1] < edges[3];
            let decision = visible.then(|| {
                let measured = face.width / width >= facing::LEAST_WIDTH
                    && facing::marks_seen(&face.marks, width, height);
                let turn = measured.then(|| facing::yaw_deg(&face.marks)).flatten();
                let (_, mut detail) =
                    mouths.judge_with_diagnostics(id, &face.marks, turn, opens[index], at_us);
                // There is no capture-to-live-look age in component replay. The
                // zero passed to the diagnostic-only setter is never exported.
                detail.frame_age_ms = None;
                detail
            });
            FaceRecord {
                detection_index: index,
                temporary_id: id,
                box_px: [face.x, face.y, face.width, face.height],
                box_normalized: edges,
                detection_score: face.score,
                mesh_status: statuses[index],
                decision,
                skipped_reason: (!visible).then_some("empty_clipped_box"),
            }
        })
        .collect()
}

#[derive(Default, Serialize)]
struct Stages {
    read_and_hash_us: u64,
    decode_us: u64,
    detector_us: u64,
    mesh_us: u64,
    judge_us: u64,
}

#[derive(Serialize)]
struct FrameRecord {
    kind: &'static str,
    row: usize,
    id: u64,
    segment: usize,
    at_us: u64,
    reset: bool,
    head_moving: Option<bool>,
    oracle_stationary_lamp: bool,
    jpeg: Asset,
    frame_size: [usize; 2],
    detector_filled: [usize; 2],
    mesh_scale: f32,
    selected_mesh_index: Option<usize>,
    outcome: &'static str,
    faces: Vec<FaceRecord>,
    host_started_us: u64,
    host_completed_us: u64,
    host_elapsed_us: u64,
    stages: Stages,
}

fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

struct Replay {
    ids: Ids,
    mouths: Mouths,
    segment: usize,
}

impl Replay {
    fn new() -> Result<Self> {
        Ok(Self {
            ids: Ids::default(),
            mouths: new_mouths()?,
            segment: 0,
        })
    }

    fn begin(&mut self, input: &Input) -> Result<()> {
        if input.reset {
            self.ids = Ids::default();
            self.mouths = new_mouths()?;
            self.segment += 1;
        }
        self.mouths.set_head_moved(input.head_moving, 0);
        Ok(())
    }

    fn frame(
        &mut self,
        input: &Input,
        root: &Path,
        detector: &Detector,
        mesh: &mut mesh::Mesh,
        full_look: bool,
        run_start: Instant,
    ) -> Result<FrameRecord> {
        let host_started_us = elapsed_us(run_start);
        self.begin(input)?;
        let mut stages = Stages::default();
        let stage = Instant::now();
        // Re-resolve for every frame, including repeated filenames.
        let path = confined(root, &input.jpeg)?;
        let bytes = bounded_bytes(&path, MAX_JPEG_BYTES)?;
        let jpeg = Asset {
            bytes: bytes.len() as u64,
            sha256: sha256(&bytes),
        };
        stages.read_and_hash_us = elapsed_us(stage);
        let stage = Instant::now();
        let (picture, full) =
            Picture::from_jpeg_full(&bytes).map_err(|_| Failure::new("jpeg_decode_failed"))?;
        stages.decode_us = elapsed_us(stage);
        let stage = Instant::now();
        let found = detector
            .find(&picture)
            .map_err(|_| Failure::new("detector_inference_failed"))?;
        stages.detector_us = elapsed_us(stage);
        let stage = Instant::now();
        let mut opens = vec![None; found.len()];
        let mut statuses = vec![MeshStatus::NotSelected; found.len()];
        let heights: Vec<_> = found.iter().map(|face| face.height).collect();
        let selected = mesh::meshed_order(&heights, false).first().copied();
        if let Some(index) = selected {
            let crop = if full_look {
                mesh::crop_full(&full, &found[index])
            } else {
                mesh::crop(&picture, &found[index])
            };
            statuses[index] = match crop {
                None => MeshStatus::CropUnavailable,
                Some(crop) => {
                    let points = mesh
                        .landmarks_of(crop)
                        .map_err(|_| Failure::new("mesh_inference_failed"))?;
                    if points.len() != mesh::LANDMARKS * 3
                        || points.iter().any(|point| !point.is_finite())
                    {
                        return Err(Failure::new("mesh_output_invalid"));
                    }
                    opens[index] = mesh::mouth_open(&points);
                    if opens[index].is_some() {
                        MeshStatus::Read
                    } else {
                        MeshStatus::MouthUnavailable
                    }
                }
            };
        }
        stages.mesh_us = elapsed_us(stage);
        let stage = Instant::now();
        let mesh_scale = if full_look { 1.0 / full.scale } else { 1.0 };
        self.mouths.set_mesh_scale(mesh_scale);
        let faces = judge_faces(
            &mut self.ids,
            &mut self.mouths,
            &found,
            &opens,
            &statuses,
            picture.filled,
            input.at_us,
        );
        stages.judge_us = elapsed_us(stage);
        let host_completed_us = elapsed_us(run_start);
        Ok(FrameRecord {
            kind: "mouth_baseline_frame",
            row: 0,
            id: input.id,
            segment: self.segment,
            at_us: input.at_us,
            reset: input.reset,
            head_moving: input.head_moving,
            oracle_stationary_lamp: input.head_moving == Some(false),
            jpeg,
            frame_size: [full.width, full.height],
            detector_filled: [picture.filled.0, picture.filled.1],
            mesh_scale,
            selected_mesh_index: selected,
            outcome: match selected {
                None => "no_detection",
                Some(index) if opens[index].is_none() => "mesh_missing",
                Some(_) => "observed",
            },
            faces,
            host_started_us,
            host_completed_us,
            host_elapsed_us: host_completed_us - host_started_us,
            stages,
        })
    }
}

fn json_line(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(|_| Failure::new("output_failed"))?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|_| Failure::new("output_failed"))
}

fn run(args: Args, output: &mut impl Write) -> Result<()> {
    let started = Instant::now();
    let config = config(|key| std::env::var_os(key))?;
    let root = canonical_root(&args.root)?;
    let manifest_path = confined(&root, &args.input)?;
    let bytes = bounded_bytes(&manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest_hash = sha256(&bytes);
    let inputs = manifest(&bytes)?;
    // Check every path before inference; bytes and symlinks are checked again on use.
    for (index, input) in inputs.iter().enumerate() {
        let path = confined(&root, &input.jpeg).map_err(|error| error.at(index + 1))?;
        let file = open_regular(&path).map_err(|error| error.at(index + 1))?;
        if file
            .metadata()
            .map_err(|_| Failure::new("file_unavailable").at(index + 1))?
            .len()
            > MAX_JPEG_BYTES
        {
            return Err(Failure::new("file_limit_exceeded").at(index + 1));
        }
    }
    let yunet_path = confined(&root, &args.yunet)?;
    let mesh_path = confined(&root, &args.mesh)?;
    let yunet_asset = pinned_asset(
        &yunet_path,
        faces::MODEL_BYTES,
        faces::MODEL_SHA256,
        "yunet_pin_failed",
    )?;
    let mesh_asset = pinned_asset(
        &mesh_path,
        mesh::MODEL_BYTES,
        mesh::MODEL_SHA256,
        "mesh_pin_failed",
    )?;
    let runtime_path = args
        .ort_runtime
        .as_ref()
        .map(|path| confined(&root, path))
        .transpose()?;
    let runtime = match &runtime_path {
        Some(path) => {
            let pin =
                lamp_turn_end::ONNX_RUNTIME.ok_or(Failure::new("runtime_target_unsupported"))?;
            let asset = pinned_asset(path, pin.bytes, pin.sha256, "runtime_pin_failed")?;
            Some(serde_json::json!({"version": pin.version, "asset": asset}))
        }
        None => None,
    };
    let detector = Detector::load(&yunet_path).map_err(|_| Failure::new("yunet_load_failed"))?;
    let mut mesh = match &runtime_path {
        Some(runtime) => mesh::Mesh::load_with_runtime(&mesh_path, runtime),
        None => mesh::Mesh::load(&mesh_path),
    }
    .map_err(|_| Failure::new("mesh_load_failed"))?;
    let source_hashes = serde_json::json!({
        "runner": sha256(include_bytes!("mouth_baseline_bench.rs")),
        "faces": sha256(include_bytes!("../src/faces.rs")),
        "picture": sha256(include_bytes!("../src/picture.rs")),
        "mesh": sha256(include_bytes!("../src/mesh.rs")),
        "facing": sha256(include_bytes!("../src/facing.rs")),
        "bus_out": sha256(include_bytes!("../src/bus_out.rs")),
        "speaking": sha256(include_bytes!("../src/speaking.rs")),
    });
    json_line(
        output,
        &serde_json::json!({
            "kind": "mouth_baseline_run", "schema": 1,
            "evidence": "component_replay", "source_time_basis": "manifest_at_us",
            "host_time_basis": "elapsed_since_process_run_start",
            "baseline": if config.full_look { "current_judge_full_frame_variant" } else { "current_production" },
            "config": config, "yunet": yunet_asset, "mesh": mesh_asset,
            "mesh_engine": if runtime.is_some() { "onnx_runtime" } else { "tract" },
            "runtime": runtime, "source_sha256": source_hashes,
            "manifest_sha256": manifest_hash, "manifest_bytes": bytes.len(),
            "input_frames": inputs.len(), "load_and_validation_elapsed_us": elapsed_us(started),
            "detector_size": [picture::WIDTH, picture::HEIGHT],
            "host_os": std::env::consts::OS, "host_arch": std::env::consts::ARCH,
            "limits": {"manifest_bytes": MAX_MANIFEST_BYTES, "row_bytes": MAX_ROW_BYTES,
                "frames": MAX_FRAMES, "jpeg_bytes": MAX_JPEG_BYTES},
            "oracle_stationary_lamp_frames": inputs.iter().filter(|input| input.head_moving == Some(false)).count(),
            "head_unknown_frames": inputs.iter().filter(|input| input.head_moving.is_none()).count(),
            "head_moving_frames": inputs.iter().filter(|input| input.head_moving == Some(true)).count(),
        }),
    )?;
    let mut replay = Replay::new()?;
    let (mut emitted, mut no_detection, mut mesh_missing) = (0, 0, 0);
    for (index, input) in inputs.iter().enumerate() {
        let mut record = replay
            .frame(
                input,
                &root,
                &detector,
                &mut mesh,
                config.full_look,
                started,
            )
            .map_err(|error| error.at(index + 1))?;
        record.row = index + 1;
        no_detection += usize::from(record.outcome == "no_detection");
        mesh_missing += usize::from(record.outcome == "mesh_missing");
        json_line(output, &record)?;
        emitted += 1;
    }
    json_line(
        output,
        &serde_json::json!({
            "kind": "mouth_baseline_complete", "complete": true,
            "input_frames": inputs.len(), "emitted_frames": emitted,
            "no_detection_frames": no_detection, "mesh_missing_frames": mesh_missing,
            "segments": replay.segment, "host_elapsed_us": elapsed_us(started),
        }),
    )
}

fn main() -> ExitCode {
    // A dependency panic must not print decoder bytes, paths or model errors.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| -> Result<()> {
        match arguments(std::env::args_os().skip(1))? {
            Some(args) => run(args, &mut io::stdout().lock()),
            None => io::stdout()
                .lock()
                .write_all(USAGE.as_bytes())
                .map_err(|_| Failure::new("output_failed")),
        }
    })
    .unwrap_or_else(|_| Err(Failure::new("internal_panic")));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = json_line(
                &mut io::stderr().lock(),
                &serde_json::json!({
                    "kind": "mouth_baseline_error", "error": error,
                }),
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::SpeakingState;
    use std::{
        os::unix::fs::symlink,
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn row(id: u64, at_us: u64, reset: bool, head_moving: Option<bool>) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": id, "at_us": at_us, "jpeg": "frames/one.jpg",
            "reset": reset, "head_moving": head_moving,
        }))
        .unwrap()
    }

    fn lines(rows: &[Vec<u8>]) -> Vec<u8> {
        rows.iter()
            .flat_map(|row| row.iter().copied().chain([b'\n']))
            .collect()
    }

    #[test]
    fn arguments_require_exact_unique_flags_and_never_echo_values() {
        let good = [
            "--root",
            "/root",
            "--input",
            "input.jsonl",
            "--yunet",
            "yu.onnx",
            "--mesh",
            "mesh.onnx",
        ];
        assert!(arguments(good.map(OsString::from)).unwrap().is_some());
        assert!(arguments([OsString::from("--help")]).unwrap().is_none());
        for args in [
            vec![],
            vec!["--input", "private value"],
            [good.as_slice(), &["--mesh", "private value"]].concat(),
            [good.as_slice(), &["--extra", "private value"]].concat(),
            [good.as_slice(), &["--ort-runtime"]].concat(),
        ] {
            let error = arguments(args.into_iter().map(OsString::from)).unwrap_err();
            assert_eq!(error.code, "arguments_invalid");
            assert!(
                !serde_json::to_string(&error)
                    .unwrap()
                    .contains("private value")
            );
        }
    }

    #[test]
    fn switches_are_explicit_and_incompatible_variants_are_refused() {
        let read = |key: &str| match key {
            speaking::ENVIRONMENT | speaking::STEADY_ENVIRONMENT => Some(OsString::from("on")),
            _ => None,
        };
        let baseline = config(read).unwrap();
        assert!(baseline.speaking && baseline.steady);
        assert!(!baseline.full_look && !baseline.all_faces);
        assert_eq!(
            config(|_| None).unwrap_err().code,
            "baseline_switches_required"
        );
        for key in [
            mesh::ALL_FACES_ENVIRONMENT,
            speaking::OUTLIER_ENVIRONMENT,
            speaking::SKIP_JUMP_ENVIRONMENT,
        ] {
            assert_eq!(
                config(|name| if name == key {
                    Some("on".into())
                } else {
                    read(name)
                })
                .unwrap_err()
                .code,
                "baseline_switches_required"
            );
        }
        assert_eq!(
            config(|_| Some("ON".into())).unwrap_err().code,
            "switch_invalid"
        );
        assert!(
            config(|key| if key == mesh::FULL_LOOK_ENVIRONMENT {
                Some("on".into())
            } else {
                read(key)
            })
            .unwrap()
            .full_look
        );
    }

    #[test]
    fn strict_manifest_does_not_accept_labels_strings_or_invent_head_state() {
        let unknown = manifest(&row(1, 0, true, None)).unwrap();
        assert_eq!(unknown[0].head_moving, None);
        let omitted = br#"{"id":2,"at_us":0,"jpeg":"one.jpg","reset":true}"#;
        assert_eq!(manifest(omitted).unwrap()[0].head_moving, None);
        let still = manifest(&row(1, 0, true, Some(false))).unwrap();
        assert_eq!(still[0].head_moving, Some(false));
        for bad in [
            br#"{"id":"private value","at_us":0,"jpeg":"one.jpg","reset":true}"#.as_slice(),
            br#"{"id":1,"at_us":0,"jpeg":"one.jpg","reset":true,"gold_speaking":true}"#,
            br#"{"id":1,"at_us":0,"jpeg":"one.jpg"}"#,
            br#"{"id":1,"at_us":0,"jpeg":"one.jpg","reset":true,"head_moving":"false"}"#,
            br#"{"id":1,"at_us":1.5,"jpeg":"one.jpg","reset":true}"#,
            br#"{"id":-1,"at_us":0,"jpeg":"one.jpg","reset":true}"#,
            br#"{"id":1,"at_us":0,"jpeg":"one.jpg","reset":true,"id":2}"#,
            b"\n",
            b"private value",
            b"\xff",
        ] {
            let error = manifest(bad).unwrap_err();
            assert_eq!(error.code, "manifest_row_invalid");
            assert!(
                !serde_json::to_string(&error)
                    .unwrap()
                    .contains("private value")
            );
        }
        assert_eq!(manifest(&[]).unwrap_err().code, "manifest_empty");
        assert_eq!(
            manifest(&vec![b' '; MAX_ROW_BYTES + 1]).unwrap_err().code,
            "manifest_limit_exceeded"
        );
    }

    #[test]
    fn source_time_and_id_validation_preserves_gaps_and_requires_resets() {
        for times in [[10, 10], [10, 9]] {
            let error = manifest(&lines(&[
                row(1, times[0], true, None),
                row(2, times[1], false, None),
            ]))
            .unwrap_err();
            assert_eq!(error, Failure::new("source_time_requires_reset").at(2));
        }
        assert_eq!(
            manifest(&row(1, 0, false, None)).unwrap_err().code,
            "source_time_requires_reset"
        );
        let input = manifest(&lines(&[
            row(1, 0, true, None),
            row(2, u64::MAX, false, None),
            row(3, 0, true, None),
        ]))
        .unwrap();
        assert_eq!(input[1].at_us, u64::MAX);
        assert_eq!(input[2].at_us, 0);
        assert_eq!(
            manifest(&lines(&[row(1, 0, true, None), row(1, 1, true, None)]))
                .unwrap_err()
                .code,
            "duplicate_input_id"
        );
    }

    // Test artifacts are retained, with hashes, rather than removed by a tempdir guard.
    fn test_directory() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "mouth-baseline-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn keep_fixture(root: &Path, name: &str, bytes: &[u8]) {
        fs::write(root.join(name), bytes).unwrap();
        let mut hashes = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("SHA256SUMS"))
            .unwrap();
        writeln!(hashes, "{}  {}", sha256(bytes), name).unwrap();
    }

    #[test]
    fn paths_are_confined_regular_bounded_and_symlinks_cannot_escape() {
        let fixture = test_directory();
        let root = fixture.join("root");
        fs::create_dir(&root).unwrap();
        keep_fixture(&fixture, "outside.jpg", b"outside");
        keep_fixture(&root, "inside.jpg", b"inside");
        symlink("../outside.jpg", root.join("escape.jpg")).unwrap();
        symlink("inside.jpg", root.join("alias.jpg")).unwrap();
        assert_eq!(
            confined(&root, Path::new("escape.jpg")).unwrap_err().code,
            "path_outside_root"
        );
        assert_eq!(
            confined(&root, &fixture.join("outside.jpg"))
                .unwrap_err()
                .code,
            "path_outside_root"
        );
        assert_eq!(
            confined(&root, Path::new("alias.jpg")).unwrap(),
            root.join("inside.jpg")
        );
        assert_eq!(
            confined(&root, Path::new("../root/inside.jpg"))
                .unwrap_err()
                .code,
            "path_invalid"
        );
        assert_eq!(
            bounded_bytes(&root, 10).unwrap_err().code,
            "regular_file_required"
        );
        assert_eq!(
            bounded_bytes(&root.join("inside.jpg"), 6).unwrap(),
            b"inside"
        );
        assert_eq!(
            bounded_bytes(&root.join("inside.jpg"), 5).unwrap_err().code,
            "file_limit_exceeded"
        );
        assert!(open_regular(&root.join("alias.jpg")).is_err());
        for path in [
            "../one.jpg",
            "/one.jpg",
            "a/../../one.jpg",
            "",
            "one\n.jpg",
            "./one.jpg",
        ] {
            assert!(!path_syntax(Path::new(path), true));
        }
        assert!(path_syntax(Path::new("frames/one.jpg"), true));
        assert_eq!(
            canonical_root(Path::new("relative")).unwrap_err().code,
            "root_invalid"
        );
        assert_eq!(
            pinned_asset(&root.join("inside.jpg"), 6, &sha256(b"inside"), "bad_pin")
                .unwrap()
                .sha256,
            sha256(b"inside")
        );
        assert_eq!(
            pinned_asset(&root.join("inside.jpg"), 6, &sha256(b"other!"), "bad_pin")
                .err()
                .unwrap()
                .code,
            "bad_pin"
        );
    }

    /// Environment mutation is process-local and happens before Rust starts any
    /// threads. Tests never use unsafe set_var or race another test's switches.
    fn child_with_baseline(test: &str) -> bool {
        if std::env::var("MOUTH_BASELINE_TEST_CHILD").as_deref() == Ok(test) {
            return true;
        }
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env("MOUTH_BASELINE_TEST_CHILD", test)
            .env(speaking::ENVIRONMENT, "on")
            .env(speaking::STEADY_ENVIRONMENT, "on")
            .env(speaking::OUTLIER_ENVIRONMENT, "off")
            .env(speaking::SKIP_JUMP_ENVIRONMENT, "off")
            .env(mesh::ALL_FACES_ENVIRONMENT, "off")
            .env(mesh::FULL_LOOK_ENVIRONMENT, "off")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        false
    }

    fn face(x: f32, height: f32) -> Face {
        Face {
            score: 0.95,
            x,
            y: 20.0,
            width: 80.0,
            height,
            marks: [
                (x + 20.0, 40.0),
                (x + 60.0, 40.0),
                (x + 40.0, 65.0),
                (x + 25.0, 85.0),
                (x + 55.0, 85.0),
            ],
        }
    }

    #[test]
    fn production_adapter_parity_all_faces_missing_mesh_and_geometry_gates() {
        if !child_with_baseline(
            "tests::production_adapter_parity_all_faces_missing_mesh_and_geometry_gates",
        ) {
            return;
        }
        let mut ids = Ids::default();
        let mut mouths = new_mouths().unwrap();
        let mut production = Ids::default();
        production.mouths = Some(new_mouths().unwrap());
        for step in 0..30 {
            let time = step * 250_000;
            let mut faces = vec![face(40.0, 100.0), face(170.0, 90.0)];
            if step == 18 {
                faces.clear();
            }
            if step == 19 {
                faces[0].marks[0].0 = -1.0;
            }
            if step == 20 {
                faces[0].width = 10.0;
            }
            if step == 21 {
                faces[0].marks[1] = faces[0].marks[0];
            }
            if step == 22 {
                faces[0].x = -90.0;
            }
            let opens: Vec<_> = (0..faces.len())
                .map(|index| {
                    (index == 0 && step != 12).then_some(if step % 2 == 0 { 0.02 } else { 0.07 })
                })
                .collect();
            let status: Vec<_> = opens
                .iter()
                .map(|open| {
                    if open.is_some() {
                        MeshStatus::Read
                    } else {
                        MeshStatus::NotSelected
                    }
                })
                .collect();
            let head = match step {
                7 => None,
                8 => Some(true),
                _ => Some(false),
            };
            mouths.set_head_moved(head, 0);
            mouths.set_mesh_scale(4.0);
            let production_mouths = production.mouths.as_mut().unwrap();
            production_mouths.set_head_moved(head, 0);
            production_mouths.set_mesh_scale(4.0);
            let actual = judge_faces(
                &mut ids,
                &mut mouths,
                &faces,
                &opens,
                &status,
                (320, 240),
                time,
            );
            let expected = bus_out::message(&faces, &opens, (320, 240), &mut production, time);
            let judged: Vec<_> = actual
                .iter()
                .filter(|face| face.decision.is_some())
                .collect();
            assert_eq!(judged.len(), expected.faces.len());
            for (ours, theirs) in judged.into_iter().zip(&expected.faces) {
                assert_eq!(format!("f{}", ours.temporary_id), theirs.id);
                assert_eq!(
                    ours.decision.as_ref().unwrap().speaking,
                    theirs.speaking.unwrap()
                );
                assert_eq!(
                    ours.box_normalized,
                    [theirs.xmin, theirs.ymin, theirs.xmax, theirs.ymax]
                );
                assert_eq!(
                    ours.decision.as_ref().unwrap().turn_deg,
                    theirs.yaw.map(|radians| radians.abs().to_degrees())
                );
            }
        }
    }

    #[test]
    fn production_unknown_head_never_silently_becomes_still() {
        if !child_with_baseline("tests::production_unknown_head_never_silently_becomes_still") {
            return;
        }
        let face = face(40.0, 100.0);
        for head in [None, Some(true), Some(false)] {
            let mut mouths = new_mouths().unwrap();
            let mut last = None;
            for step in 0..8 {
                mouths.set_head_moved(head, 0);
                last = Some(
                    mouths
                        .judge_with_diagnostics(
                            1,
                            &face.marks,
                            Some(0.0),
                            Some(if step % 2 == 0 { 0.02 } else { 0.08 }),
                            step * 250_000,
                        )
                        .1,
                );
            }
            let last = last.unwrap();
            if head == Some(false) {
                assert_eq!(last.speaking.state, SpeakingState::Yes);
                assert_eq!(last.reason, speaking::Reason::Mesh);
            } else {
                assert_eq!(last.speaking.state, SpeakingState::Unknown);
                assert_eq!(last.reason, speaking::Reason::FewSteadyPairs);
            }
        }
    }

    #[test]
    fn production_tracking_reset_and_time_gap_start_new_windows() {
        if !child_with_baseline("tests::production_tracking_reset_and_time_gap_start_new_windows") {
            return;
        }
        let mut replay = Replay::new().unwrap();
        let faces = [face(40.0, 100.0), face(170.0, 100.0)];
        assert_eq!(mesh::meshed_order(&[100.0, 100.0], false), vec![0]);
        let mut last = Vec::new();
        for step in 0..6 {
            let input = manifest(&row(step, step * 250_000, true, Some(false)))
                .unwrap()
                .remove(0);
            let input = Input {
                reset: step == 0,
                ..input
            };
            replay.begin(&input).unwrap();
            last = judge_faces(
                &mut replay.ids,
                &mut replay.mouths,
                &faces,
                &[Some(0.05), None],
                &[MeshStatus::Read, MeshStatus::NotSelected],
                (320, 240),
                input.at_us,
            );
        }
        assert_eq!(
            last[0].decision.as_ref().unwrap().speaking.state,
            SpeakingState::No
        );
        assert_eq!(last[0].decision.as_ref().unwrap().window_looks, Some(6));
        let after_gap = judge_faces(
            &mut replay.ids,
            &mut replay.mouths,
            &faces,
            &[None, None],
            &[MeshStatus::MouthUnavailable, MeshStatus::NotSelected],
            (320, 240),
            10_000_000,
        );
        assert_ne!(after_gap[0].temporary_id, last[0].temporary_id);
        assert_eq!(
            after_gap[0].decision.as_ref().unwrap().window_looks,
            Some(1)
        );
        replay
            .begin(&manifest(&row(9, 0, true, None)).unwrap().remove(0))
            .unwrap();
        let reset = judge_faces(
            &mut replay.ids,
            &mut replay.mouths,
            &faces,
            &[None, None],
            &[MeshStatus::MouthUnavailable, MeshStatus::NotSelected],
            (320, 240),
            0,
        );
        assert_eq!(replay.segment, 2);
        assert_eq!(reset[0].temporary_id, 1);
        assert_eq!(reset[0].decision.as_ref().unwrap().window_looks, Some(1));
        assert!(reset[0].decision.as_ref().unwrap().window_reset);
        assert_eq!(
            reset[0].decision.as_ref().unwrap().head,
            Some(speaking::HeadAtFrame::Unknown)
        );
        assert!(
            !serde_json::to_string(&reset[0])
                .unwrap()
                .contains("frame_age_ms")
        );
        assert!(
            judge_faces(
                &mut replay.ids,
                &mut replay.mouths,
                &[],
                &[],
                &[],
                (320, 240),
                1
            )
            .is_empty()
        );
    }
}
