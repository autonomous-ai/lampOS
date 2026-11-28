//! Replay the production caller model on sealed, label-free inputs. All PCM stays local.
use lamp_same_voice::strong_caller::{Enrollment, Probe, StrongCaller, Trace, decide};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    error::Error,
    fs::{self, File},
    io::{BufWriter, Cursor, Write},
    path::{Component, Path},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Table = BufWriter<File>;

struct Case {
    id: String,
    reference: String,
    probe: String,
    time_fresh: bool,
}

struct Measurement {
    trace: Trace,
    total_us: u128,
    phase: &'static str,
}

fn byte_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn file_hash(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn digest_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn rows<'a>(text: &'a str, hash: &str, header: &str) -> Result<impl Iterator<Item = &'a str>> {
    let mut rows = text.lines();
    if rows.next() != Some(format!("# pairs_sha256={hash}").as_str()) || rows.next() != Some(header)
    {
        return Err("unexpected plan binding or header".into());
    }
    Ok(rows)
}

fn cases(text: &str, hash: &str) -> Result<Vec<Case>> {
    let mut seen = HashSet::new();
    let mut cases = Vec::new();
    for row in rows(
        text,
        hash,
        "case_id\treference_sha256\tfollowup_sha256\ttime_fresh",
    )? {
        let columns: Vec<_> = row.split('\t').collect();
        let [id, reference, probe, time] = columns.as_slice() else {
            return Err("invalid planned row".into());
        };
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            || !seen.insert(*id)
            || !digest_id(reference)
            || !digest_id(probe)
            || !matches!(*time, "0" | "1")
        {
            return Err("invalid or duplicate planned case".into());
        }
        cases.push(Case {
            id: (*id).into(),
            reference: (*reference).into(),
            probe: (*probe).into(),
            time_fresh: *time == "1",
        });
    }
    if cases.first().is_none_or(|c| !c.time_fresh) {
        return Err("the first real case must have a fresh measurement".into());
    }
    Ok(cases)
}

fn load_audio(corpus: &Path, assets: &str, pairs_hash: &str) -> Result<HashMap<String, Vec<f32>>> {
    let mut audio = HashMap::new();
    for row in rows(assets, pairs_hash, "wav_sha256\trelative_path")? {
        let (hash, rel) = row.split_once('\t').ok_or("invalid audio asset row")?;
        let path = Path::new(rel);
        if !digest_id(hash)
            || rel.is_empty()
            || !path.components().all(|p| matches!(p, Component::Normal(_)))
        {
            return Err("invalid private audio path or digest".into());
        }
        let source = fs::canonicalize(corpus.join(path))?;
        if !source.starts_with(corpus) {
            return Err("audio outside its sealed corpus".into());
        }
        let bytes = fs::read(source)?;
        if byte_hash(&bytes) != hash {
            return Err("model-input WAV digest changed".into());
        }
        let mut wav = hound::WavReader::new(Cursor::new(bytes))?;
        let spec = wav.spec();
        if spec.channels != 1
            || spec.sample_rate != 16_000
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err("expected mono 16 kHz signed PCM16 WAV".into());
        }
        let samples = wav
            .samples::<i16>()
            .map(|v| v.map(|s| f32::from(s) / 32768.0))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if audio.insert(hash.to_string(), samples).is_some() {
            return Err("duplicate canonical WAV".into());
        }
    }
    Ok(audio)
}

fn table(root: &Path, name: &str, header: &str) -> Result<Table> {
    let mut output = BufWriter::new(File::create_new(root.join(name))?);
    writeln!(output, "{header}")?;
    Ok(output)
}

fn optional(n: Option<f32>) -> String {
    n.map(|x| x.to_string()).unwrap_or_default()
}

fn compare_trace(a: &Trace, b: &Trace) -> bool {
    a.class == b.class && a.cosine == b.cosine && a.frames == b.frames
}

fn validate_trace(t: &Trace) -> Result<()> {
    if !matches!(t.class, "same" | "different" | "unknown")
        || (t.class != "unknown" && t.cosine.is_none())
        || t.cosine
            .iter()
            .chain(t.frames.iter().flatten())
            .any(|x| !x.is_finite() || !(-1.0..=1.0).contains(x))
    {
        return Err("invalid native classification or cosine".into());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [plan, corpus, model, mode, out] = args.as_slice() else {
        return Err("usage: strong_caller_bench PLAN CORPUS MODEL_FILE all|sample NEW_RUN".into());
    };
    let all = match mode.to_str() {
        Some("all") => true,
        Some("sample") => false,
        _ => return Err("unrecognized timing mode".into()),
    };
    let plan = Path::new(plan);
    let out = Path::new(out);
    let model = Path::new(model);
    let pairs_hash = file_hash(&plan.join("pairs.tsv"))?;
    let case_text = fs::read_to_string(plan.join("cases.tsv"))?;
    let asset_text = fs::read_to_string(plan.join("assets.tsv"))?;
    let cases = cases(&case_text, &pairs_hash)?;
    let audio = load_audio(&fs::canonicalize(corpus)?, &asset_text, &pairs_hash)?;
    let unique: HashSet<_> = cases
        .iter()
        .flat_map(|c| [&c.reference, &c.probe])
        .collect();
    if unique.len() != audio.len() || unique.iter().any(|id| !audio.contains_key(*id)) {
        return Err("case list and audio plan differ".into());
    }

    fs::create_dir(out)?;
    let hashes = format!(
        "{{\"pairs_sha256\":\"{pairs_hash}\",\"cases_sha256\":\"{}\",\"assets_sha256\":\"{}\",\"model_sha256\":\"{}\",\"producer_binary_sha256\":\"{}\"}}\n",
        byte_hash(case_text.as_bytes()),
        byte_hash(asset_text.as_bytes()),
        file_hash(model)?,
        file_hash(&std::env::current_exe()?)?
    );
    fs::write(out.join("inputs.json"), hashes)?;
    let mut predictions = table(
        out,
        "predictions.tsv",
        &format!("# pairs_sha256={pairs_hash}\ncase_id\toutput\tscore\treplay_elapsed_us\tphase"),
    )?;
    let mut trace = table(
        out,
        "frames.tsv",
        "case_id\tstart_sample\tend_sample_exclusive\tcosine",
    )?;
    let mut feature_times = table(
        out,
        "feature-times.tsv",
        "role\twav_sha256\texecution\tavailable\thost_us",
    )?;
    let mut pair_times = table(
        out,
        "fresh-times.tsv",
        "case_id\tphase\tenroll_us\tprobe_us\tdecide_us\ttotal_us",
    )?;
    let mut state = table(
        out,
        "status.tsv",
        "case_id\tenrollment_available\tprobe_available\tactive_frames\ttotal_frames\tcached_compare_us\tfresh_verified",
    )?;

    let load = Instant::now();
    let engine = StrongCaller::load(model)?;
    let load_us = load.elapsed().as_micros();
    fs::write(
        out.join("load.tsv"),
        format!("host_model_load_us\n{load_us}\n"),
    )?;
    let mut enrollments: HashMap<&str, Option<Enrollment>> = HashMap::new();
    let mut probes: HashMap<&str, Option<Probe>> = HashMap::new();
    let mut measured = HashMap::new();
    for case in cases.iter().filter(|c| all || c.time_fresh) {
        let start = Instant::now();
        let enrollment = engine.enroll(&audio[&case.reference])?;
        let enrolled = Instant::now();
        let attempted = enrollment.is_some();
        let probe = if attempted {
            engine.prepare_probe(&audio[&case.probe])?
        } else {
            None
        };
        let prepared = Instant::now();
        let verdict = decide(enrollment.as_ref(), probe.as_ref());
        let finished = Instant::now();
        validate_trace(&verdict)?;
        let enrollment_us = (enrolled - start).as_micros();
        let probe_us = if attempted {
            (prepared - enrolled).as_micros().to_string()
        } else {
            String::new()
        };
        let decide_us = (finished - prepared).as_micros();
        let total_us = (finished - start).as_micros();
        let phase = if measured.is_empty() { "first" } else { "warm" };
        writeln!(
            pair_times,
            "{}\t{phase}\t{enrollment_us}\t{probe_us}\t{decide_us}\t{total_us}",
            case.id
        )?;
        writeln!(
            feature_times,
            "enrollment\t{}\tfresh\t{}\t{enrollment_us}",
            case.reference,
            enrollment.is_some()
        )?;
        writeln!(
            feature_times,
            "probe\t{}\t{}\t{}\t{probe_us}",
            case.probe,
            if attempted {
                "fresh"
            } else {
                "skipped_no_enrollment"
            },
            probe.is_some()
        )?;
        enrollments.entry(&case.reference).or_insert(enrollment);
        if attempted {
            probes.entry(&case.probe).or_insert(probe);
        }
        measured.insert(
            case.id.as_str(),
            Measurement {
                trace: verdict,
                total_us,
                phase,
            },
        );
    }

    for case in &cases {
        if let Entry::Vacant(slot) = enrollments.entry(&case.reference) {
            let start = Instant::now();
            let enrollment = engine.enroll(&audio[&case.reference])?;
            let us = start.elapsed().as_micros();
            writeln!(
                feature_times,
                "enrollment\t{}\tcache_fill\t{}\t{us}",
                case.reference,
                enrollment.is_some()
            )?;
            slot.insert(enrollment);
        }
        let enrollment = enrollments[case.reference.as_str()].as_ref();
        if enrollment.is_some()
            && let Entry::Vacant(slot) = probes.entry(&case.probe)
        {
            let start = Instant::now();
            let probe = engine.prepare_probe(&audio[&case.probe])?;
            let us = start.elapsed().as_micros();
            writeln!(
                feature_times,
                "probe\t{}\tcache_fill\t{}\t{us}",
                case.probe,
                probe.is_some()
            )?;
            slot.insert(probe);
        }
        let probe = enrollment.and_then(|_| probes.get(case.probe.as_str())?.as_ref());
        let start = Instant::now();
        let verdict = decide(enrollment, probe);
        let compare_us = start.elapsed().as_micros();
        validate_trace(&verdict)?;
        let (duration, phase) = match measured.get(case.id.as_str()) {
            Some(m) if compare_trace(&verdict, &m.trace) => (m.total_us.to_string(), m.phase),
            Some(_) => return Err("full fresh and cached candidate results differ".into()),
            None => (String::new(), "unmeasured"),
        };
        writeln!(
            predictions,
            "{}\t{}\t{}\t{duration}\t{phase}",
            case.id,
            verdict.class,
            optional(verdict.cosine)
        )?;
        let active = verdict.frames.iter().flatten().count();
        writeln!(
            state,
            "{}\t{}\t{}\t{active}\t{}\t{compare_us}\t{}",
            case.id,
            enrollment.is_some(),
            probe.is_some(),
            verdict.frames.len(),
            phase != "unmeasured"
        )?;
        for (i, cosine) in verdict.frames.iter().enumerate() {
            let start_sample = i * 160;
            writeln!(
                trace,
                "{}\t{start_sample}\t{}\t{}",
                case.id,
                start_sample + 400,
                optional(*cosine)
            )?;
        }
    }
    for t in [
        &mut predictions,
        &mut trace,
        &mut feature_times,
        &mut pair_times,
        &mut state,
    ] {
        t.flush()?;
    }
    fs::write(
        out.join("COMPLETE"),
        format!(
            "{} ordered pairs; {} fully uncached measurements\n",
            cases.len(),
            measured.len()
        ),
    )?;
    Ok(())
}
