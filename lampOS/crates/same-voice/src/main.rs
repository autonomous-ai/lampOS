//! `lamp-same-voice pairs MODEL.onnx LIST.tsv`: each clip of LIST (lines of
//! `id<TAB>path`, 16 kHz mono 16-bit WAV) embedded, then the cosine of
//! every pair, one `a<TAB>b<TAB>cosine` line each, and on standard error
//! each clip's length and how long its embedding took. Local files only.
//!
//! `lamp-same-voice serve MODEL.onnx`: the voice's child (#829), a print for
//! each request on standard input, as [`lamp_same_voice::serve`] describes.
use lamp_same_voice::{RATE, Voice, cosine, read_wav};
use std::{path::Path, process::ExitCode, time::Instant};

const USAGE: &str =
    "usage: lamp-same-voice pairs MODEL.onnx LIST.tsv\n       lamp-same-voice serve MODEL.onnx";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args.as_slice() {
        [command, model, list] if command == "pairs" => pairs(Path::new(model), Path::new(list)),
        [command, model] if command == "serve" => serve(Path::new(model)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-same-voice: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The model loaded, then each request answered until the voice closes
/// standard input.
fn serve(model: &Path) -> Result<(), String> {
    // Before the model loads: never at the voice's real-time priority
    // (#1445). A child that cannot leave it ends, and the check is off.
    lamp_same_voice::ordinary_priority()
        .map_err(|error| format!("the ordinary priority could not be taken: {error}"))?;
    let loaded = Instant::now();
    let voice = Voice::load(model)?;
    let loaded_ms = loaded.elapsed().as_millis();
    lamp_same_voice::serve(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        loaded_ms,
        |samples| voice.embedding(samples),
    )
    .map_err(|error| error.to_string())
}

fn pairs(model: &Path, list: &Path) -> Result<(), String> {
    let loaded = Instant::now();
    let voice = Voice::load(model)?;
    eprintln!("loaded in {} ms", loaded.elapsed().as_millis());
    let text =
        std::fs::read_to_string(list).map_err(|error| format!("{}: {error}", list.display()))?;
    let mut clips = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (id, path) = line
            .split_once('\t')
            .ok_or_else(|| format!("not id<TAB>path: {line}"))?;
        let samples = read_wav(Path::new(path))?;
        let started = Instant::now();
        let embedding = voice.embedding(&samples)?;
        eprintln!(
            "{id}\t{:.2} s\t{} ms",
            samples.len() as f32 / RATE as f32,
            started.elapsed().as_millis()
        );
        clips.push((id.to_owned(), embedding));
    }
    for (i, (a, first)) in clips.iter().enumerate() {
        for (b, second) in &clips[i + 1..] {
            match cosine(first, second) {
                Some(value) => println!("{a}\t{b}\t{value:.4}"),
                None => return Err(format!("{a} and {b}: no cosine between their embeddings")),
            }
        }
    }
    Ok(())
}
