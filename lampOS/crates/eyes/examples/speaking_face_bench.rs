//! Offline Light-ASD component bench; no camera, microphone or voice path.
//! `LAMP_SPEAKING_FACE_SENSOR=on cargo run --release -p lamp-eyes
//! --example speaking_face_bench -- ROOT CASES.jsonl MODEL.onnx ASSET.json`.
//! Labels are intentionally absent from the input and result types.
use lamp_eyes::active_speaker::{
    self, Candidate, FaceScore, MAX_FACES, ModelAsset, SIDE, features::Mfcc,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path},
    process::ExitCode,
    time::Instant,
};

type Result<T> = active_speaker::Result<T>;
const ROW_BYTES: u64 = 65_536;
const MAX_CASES: usize = 10_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaceInput {
    track_id: String,
    gray_u8: Option<Artifact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: u64,
    clock_id: String,
    eyes_epoch: String,
    input_start_us: u64,
    input_end_us: u64,
    target_frame_us: u64,
    context_frames: usize,
    audio_pcm_s16le: Option<Artifact>,
    faces: Vec<FaceInput>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

fn validate(case: &Case, model_frames: usize) -> Result<()> {
    if !identifier(&case.clock_id)
        || !identifier(&case.eyes_epoch)
        || !matches!(case.context_frames, 25 | 50)
        || case.context_frames < model_frames
        || case.input_end_us.checked_sub(case.input_start_us)
            != Some(case.context_frames as u64 * 40_000)
        || case.input_end_us.checked_sub(case.target_frame_us) != Some(40_000)
    {
        return Err("invalid case clocks, context or face count".into());
    }
    let mut seen = BTreeSet::new();
    for face in &case.faces {
        if !identifier(&face.track_id)
            || matches!(
                face.track_id.as_str(),
                "unknown" | "no_visible" | "not_applicable"
            )
            || !seen.insert(&face.track_id)
        {
            return Err("invalid or duplicate temporary face track".into());
        }
    }
    Ok(())
}

fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// A bounded, hash-checked read from the consented/local data root. No path
/// or arbitrary file contents are ever printed in an error or result.
fn artifact(root: &Path, input: &Artifact, bytes: usize) -> Result<Vec<u8>> {
    if input.path.len() > 1024
        || !relative(Path::new(&input.path))
        || input.sha256.len() != 64
        || !input
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("invalid artifact path or hash".into());
    }
    let canonical = root
        .join(&input.path)
        .canonicalize()
        .map_err(|_| "artifact is unavailable")?;
    if !canonical.starts_with(root) {
        return Err("artifact escapes the data root".into());
    }
    let file = File::open(canonical).map_err(|_| "artifact cannot be opened")?;
    if !file.metadata()?.is_file() || file.metadata()?.len() != bytes as u64 {
        return Err("artifact byte count differs".into());
    }
    let mut data = Vec::with_capacity(bytes);
    file.take(bytes as u64 + 1).read_to_end(&mut data)?;
    if data.len() != bytes || format!("{:x}", Sha256::digest(&data)) != input.sha256 {
        return Err("artifact SHA-256 differs".into());
    }
    Ok(data)
}

#[derive(Serialize)]
struct ScoredFace {
    track_id: String,
    probability: Option<f32>,
    window_probabilities: Option<Vec<f32>>,
    host_inference_us: Option<u64>,
}

#[derive(Serialize)]
struct Record {
    kind: &'static str,
    id: u64,
    clock_id: String,
    eyes_epoch: String,
    input_start_us: u64,
    input_end_us: u64,
    target_frame_us: u64,
    ready_us: Option<u64>,
    replay_elapsed_us: u64,
    host_features_us: Option<u64>,
    model_frames: usize,
    model_sha256: String,
    backend: &'static str,
    model_load_us: u64,
    output: String,
    reason: active_speaker::Reason,
    faces: Vec<ScoredFace>,
}

fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

fn score(
    root: &Path,
    case: Case,
    model: &mut Candidate,
    mfcc: &Mfcc,
    model_load_us: u64,
) -> Result<Record> {
    let start = Instant::now();
    let frames = model.asset().frames;
    validate(&case, frames)?;
    let mut scores: Vec<_> = case
        .faces
        .iter()
        .map(|face| ScoredFace {
            track_id: face.track_id.clone(),
            probability: None,
            window_probabilities: None,
            host_inference_us: None,
        })
        .collect();
    let mut host_features_us = None;
    // One face is outside the job; more than the inference cap is an explicit
    // abstention. Both remain in the output without opening their media.
    if (2..=MAX_FACES).contains(&case.faces.len())
        && let Some(audio) = &case.audio_pcm_s16le
    {
        let bytes = artifact(root, audio, case.context_frames * 640 * 2)?;
        let tail = &bytes[bytes.len() - frames * 640 * 2..];
        let pcm: Vec<i16> = tail
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let feature_start = Instant::now();
        let features = mfcc.extract(&pcm, frames)?;
        host_features_us = Some(micros(feature_start));
        for (input, output) in case.faces.iter().zip(&mut scores) {
            let Some(gray) = &input.gray_u8 else { continue };
            let bytes = artifact(root, gray, case.context_frames * SIDE * SIDE)?;
            let visual: Vec<f32> = bytes[bytes.len() - frames * SIDE * SIDE..]
                .iter()
                .copied()
                .map(f32::from)
                .collect();
            let inference_start = Instant::now();
            let probabilities = model.probabilities(&features, &visual)?;
            output.host_inference_us = Some(micros(inference_start));
            output.probability = probabilities.last().copied();
            output.window_probabilities = Some(probabilities);
        }
    }
    let choice = active_speaker::choose(
        &scores
            .iter()
            .map(|face| FaceScore {
                track_id: face.track_id.clone(),
                probability: face.probability,
            })
            .collect::<Vec<_>>(),
    );
    Ok(Record {
        kind: "component_replay_not_qualification",
        id: case.id,
        clock_id: case.clock_id,
        eyes_epoch: case.eyes_epoch,
        input_start_us: case.input_end_us - frames as u64 * 40_000,
        input_end_us: case.input_end_us,
        target_frame_us: case.target_frame_us,
        ready_us: None,
        replay_elapsed_us: micros(start),
        host_features_us,
        model_frames: frames,
        model_sha256: model.asset().sha256.clone(),
        backend: model.backend(),
        model_load_us,
        output: choice.output,
        reason: choice.reason,
        faces: scores,
    })
}

fn run(setting: Option<&str>, args: &[OsString], output: &mut impl Write) -> Result<()> {
    if !active_speaker::enabled(setting) {
        writeln!(
            output,
            "{{\"kind\":\"component_replay_not_qualification\",\"enabled\":false}}"
        )?;
        return Ok(());
    }
    if !matches!(args.len(), 4 | 5) {
        return Err(
            "usage: ROOT CASES.jsonl MODEL.onnx ASSET.json [PINNED_RUNTIME] (explicit switch required)".into(),
        );
    }
    let root = Path::new(&args[0])
        .canonicalize()
        .map_err(|_| "data root is unavailable")?;
    let asset_file = File::open(&args[3]).map_err(|_| "model specification cannot be opened")?;
    if asset_file.metadata()?.len() > 4096 {
        return Err("model specification exceeds its bound".into());
    }
    let asset: ModelAsset =
        serde_json::from_reader(asset_file).map_err(|_| "invalid model specification JSON")?;
    let model_start = Instant::now();
    let mut model = if let Some(runtime) = args.get(4) {
        Candidate::load_with_runtime(setting, Path::new(&args[2]), asset, Path::new(runtime))?
    } else {
        Candidate::load(setting, Path::new(&args[2]), asset)?
    }
    .ok_or("candidate is off")?;
    let model_load_us = micros(model_start);
    let mfcc = Mfcc::default();
    let file = File::open(&args[1]).map_err(|_| "case manifest cannot be opened")?;
    let mut input = BufReader::new(file);
    let mut row = Vec::new();
    let mut ids = BTreeSet::new();
    for index in 0..=MAX_CASES {
        row.clear();
        (&mut input)
            .take(ROW_BYTES + 1)
            .read_until(b'\n', &mut row)?;
        if row.is_empty() {
            if ids.is_empty() {
                return Err("case manifest is empty".into());
            }
            return Ok(());
        }
        if index == MAX_CASES || row.len() as u64 > ROW_BYTES {
            return Err("case manifest exceeds its bound".into());
        }
        let case: Case = serde_json::from_slice(&row)
            .map_err(|_| format!("case row {}: invalid JSON", index + 1))?;
        if !ids.insert(case.id) {
            return Err("duplicate case id".into());
        }
        let result = score(&root, case, &mut model, &mfcc, model_load_us)?;
        serde_json::to_writer(&mut *output, &result)?;
        writeln!(output)?;
        output.flush()?;
    }
    unreachable!("the extra row fails the bound")
}

fn main() -> ExitCode {
    let setting = std::env::var(active_speaker::ENVIRONMENT).ok();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match run(setting.as_deref(), &args, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("speaking-face bench: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case() -> Case {
        Case {
            id: 1,
            clock_id: "public-source-1".into(),
            eyes_epoch: "track-set-1".into(),
            input_start_us: 0,
            input_end_us: 2_000_000,
            target_frame_us: 1_960_000,
            context_frames: 50,
            audio_pcm_s16le: None,
            faces: vec![
                FaceInput {
                    track_id: "track1".into(),
                    gray_u8: None,
                },
                FaceInput {
                    track_id: "track2".into(),
                    gray_u8: None,
                },
            ],
        }
    }

    #[test]
    fn too_many_visible_faces_remain_valid_cases_and_abstain() {
        let mut input = case();
        input.faces = (0..=MAX_FACES)
            .map(|i| FaceInput {
                track_id: format!("face{i}"),
                gray_u8: None,
            })
            .collect();
        assert!(validate(&input, 50).is_ok());
        let scores: Vec<_> = input
            .faces
            .iter()
            .map(|f| FaceScore {
                track_id: f.track_id.clone(),
                probability: None,
            })
            .collect();
        assert_eq!(
            active_speaker::choose(&scores).reason,
            active_speaker::Reason::TooManyFaces
        );
    }

    #[test]
    fn off_reads_no_arguments_model_manifest_or_media() {
        let mut output = Vec::new();
        run(None, &["/does/not/exist".into()], &mut output).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("\"enabled\":false")
        );
    }

    #[test]
    fn source_windows_and_temporary_ids_are_validated() {
        validate(&case(), 25).unwrap();
        validate(&case(), 50).unwrap();
        let mut bad = case();
        bad.input_end_us = 3_000_000;
        assert!(validate(&bad, 25).is_err());
        let mut bad = case();
        bad.faces[1].track_id = "track1".into();
        assert!(validate(&bad, 25).is_err());
        let mut bad = case();
        bad.clock_id.clear();
        assert!(validate(&bad, 25).is_err());
    }

    #[test]
    fn private_paths_are_relative_and_labels_are_refused() {
        assert!(relative(Path::new("session/face.gray")));
        for path in ["", "/tmp/face.gray", "../face.gray", "session/../face.gray"] {
            assert!(!relative(Path::new(path)));
        }
        let input = r#"{"id":1,"expected":"track1","private_words":"secret"}"#;
        assert!(serde_json::from_str::<Case>(input).is_err());
    }
}
