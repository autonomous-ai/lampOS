//! Paired, local-only replay. Both methods see identical PCM and the same wake.
//! The extra unguarded CAM++ series diagnoses what the length guard conceals.
use lamp_same_voice::{
    Voice, cosine,
    personal_vad::{Model, decide},
    read_wav,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Component, Path},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
// The whole-line comparator's shipped input bounds at simple base 0dbbfc95bb,
// voice/src/same_voice.rs (LEAST_SAMPLES, ASKED_AT, DIFFERENT_BELOW, SAME_FROM).
const PRINT_MIN: usize = 24_000;
const PRINT_MAX: usize = 28_800;

fn prediction_file(dir: &Path, name: &str, hash: &str) -> Result<BufWriter<File>> {
    let mut out = BufWriter::new(File::create_new(dir.join(name))?);
    writeln!(
        out,
        "# pairs_sha256={hash}\ncase_id\toutput\tscore\treplay_elapsed_us\tphase"
    )?;
    Ok(out)
}

fn prediction(
    out: &mut impl Write,
    id: &str,
    class: &str,
    score: Option<f32>,
    duration: Option<u128>,
    count: &mut usize,
) -> Result<()> {
    let phase = match duration {
        Some(_) if *count == 0 => "first",
        Some(_) => "warm",
        None => "unmeasured",
    };
    *count += usize::from(duration.is_some());
    writeln!(
        out,
        "{id}\t{class}\t{}\t{}\t{phase}",
        score.map_or_else(String::new, |x| format!("{x:.7}")),
        duration.map_or_else(String::new, |x| x.to_string())
    )?;
    Ok(())
}

/// All resolved audio paths must be beneath the private capture root.
fn clip(root: &Path, name: &str) -> Result<Option<Vec<f32>>> {
    let rel = Path::new(name);
    if name.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("expected a relative clip path within the corpus".into());
    }
    let source = match fs::canonicalize(root.join(rel)) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !source.starts_with(root) {
        return Err("clip escapes capture root".into());
    }
    Ok(Some(read_wav(&source)?))
}

fn file_digest(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn cam_class(score: Option<f32>) -> &'static str {
    match score {
        Some(x) if x >= 0.70 => "same",
        Some(x) if x < 0.30 => "different",
        _ => "unknown",
    }
}

fn cam_score(model: &Voice, wake: &[f32], reply: &[f32]) -> Option<f32> {
    let first = model.embedding(&wake[..wake.len().min(PRINT_MAX)]).ok()?;
    let second = model.embedding(&reply[..reply.len().min(PRINT_MAX)]).ok()?;
    cosine(&first, &second)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [manifest, cam_path, frame_dir, output] = args.as_slice() else {
        return Err("usage: caller_frame_bench PAIRS.tsv CAMPLUS.onnx FRAME_MODEL_DIRECTORY NEW_RUN_DIRECTORY".into());
    };
    let manifest = fs::canonicalize(manifest)?;
    let root = manifest.parent().ok_or("missing corpus root")?;
    let text = fs::read_to_string(&manifest)?;
    let mut rows = text.lines();
    if rows.next() != Some("case_id\treference\tfollowup\tlabel\twords\tcondition") {
        return Err("unexpected pairs.tsv header".into());
    }
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let cam_hash = file_digest(Path::new(cam_path))?;
    if cam_hash != "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b" {
        return Err("unexpected product CAM++ SHA-256".into());
    }
    let frame_hash = file_digest(&Path::new(frame_dir).join("caller-frame.f32"))?;
    let binary_hash = file_digest(&std::env::current_exe()?)?;
    let dir = Path::new(output);
    fs::create_dir(dir)?;
    fs::write(
        dir.join("provenance.tsv"),
        format!(
            "input\tsha256\npairs.tsv\t{hash}\ncam++\t{cam_hash}\npersonal-vad\t{frame_hash}\nproducer\t{binary_hash}\n"
        ),
    )?;
    fs::write(
        dir.join("README.md"),
        "Local paired replay on 16 kHz microphone-format PCM. The case list binds each output by SHA-256. Timings use a host monotonic clock. No lamp readiness or policy outcome is inferred. A COMPLETE file marks a finished run.\n",
    )?;
    let mut whole = prediction_file(dir, "whole.tsv", &hash)?;
    let mut forced = prediction_file(dir, "unguarded-cam.tsv", &hash)?;
    let mut frame = prediction_file(dir, "frame.tsv", &hash)?;
    let mut trace = BufWriter::new(File::create_new(dir.join("posteriors.tsv"))?);
    writeln!(trace, "case_id\tframe_end_ms\ttarget\tsilence\tother")?;
    let mut stages = BufWriter::new(File::create_new(dir.join("stages.tsv"))?);
    writeln!(
        stages,
        "case_id\twake_samples\tline_samples\tenrollment_us\tfollowup_us"
    )?;
    let clock = Instant::now();
    let cam = Voice::load(Path::new(cam_path))?;
    let cam_load = clock.elapsed().as_micros();
    let clock = Instant::now();
    let model = Model::load(Path::new(frame_dir))?;
    fs::write(
        dir.join("model-load.tsv"),
        format!(
            "model\tcold_host_load_us\ncam++\t{cam_load}\npersonal-vad\t{}\n",
            clock.elapsed().as_micros()
        ),
    )?;
    let mut ids = HashSet::new();
    let (mut n_whole, mut n_forced, mut n_frame) = (0, 0, 0);
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        let [id, wake_path, reply_path, label, words, condition] = fields.as_slice() else {
            return Err("expected six manifest fields".into());
        };
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || !ids.insert(*id)
            || !matches!(*label, "same" | "different")
            || !matches!(*condition, "quiet" | "noisy")
            || words.parse::<usize>()? == 0
        {
            return Err("invalid or duplicate labelled case".into());
        }
        let wake = clip(root, wake_path)?;
        let line = clip(root, reply_path)?;
        let (Some(wake), Some(line)) = (wake, line) else {
            for (out, count) in [
                (&mut whole, &mut n_whole),
                (&mut forced, &mut n_forced),
                (&mut frame, &mut n_frame),
            ] {
                prediction(out, id, "unknown", None, None, count)?;
            }
            continue;
        };
        // Recompute both inputs per case, as the stateless live frame child does.
        let clock = Instant::now();
        let caller = model.enroll(&wake);
        let enrollment = clock.elapsed().as_micros();
        let followup = Instant::now();
        let frames = caller.map_or_else(Vec::new, |c| model.posteriors(&c, &line));
        let verdict = decide(&frames);
        let followup_us = followup.elapsed().as_micros();
        let total_us = clock.elapsed().as_micros();
        prediction(
            &mut frame,
            id,
            verdict.class,
            verdict.target_fraction,
            Some(total_us),
            &mut n_frame,
        )?;
        writeln!(
            stages,
            "{id}\t{}\t{}\t{enrollment}\t{followup_us}",
            wake.len(),
            line.len()
        )?;
        for (n, [t, s, o]) in frames.iter().enumerate() {
            writeln!(trace, "{id}\t{}\t{t:.8}\t{s:.8}\t{o:.8}", 25 + 10 * n)?;
        }
        let clock = Instant::now();
        let score = cam_score(&cam, &wake, &line);
        let micros = clock.elapsed().as_micros();
        prediction(
            &mut forced,
            id,
            cam_class(score),
            score,
            Some(micros),
            &mut n_forced,
        )?;
        if wake.len() >= PRINT_MIN && line.len() >= PRINT_MIN {
            // Exactly the same truncated tensors and invocation as the diagnostic.
            prediction(
                &mut whole,
                id,
                cam_class(score),
                score,
                Some(micros),
                &mut n_whole,
            )?;
        } else {
            prediction(&mut whole, id, "unknown", None, None, &mut n_whole)?;
        }
    }
    if ids.is_empty() {
        return Err("empty case manifest".into());
    }
    for out in [&mut whole, &mut frame, &mut forced, &mut trace, &mut stages] {
        out.flush()?;
    }
    fs::write(
        dir.join("COMPLETE"),
        format!("{} paired cases\n", ids.len()),
    )?;
    Ok(())
}
