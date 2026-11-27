use lamp_sensor_score::{
    digest,
    schema::{Manifest, Run, Sensor},
    score, voice,
};
use serde::de::DeserializeOwned;
use std::{env, fs, io::Write, path::Path, process::ExitCode};

fn read<T: DeserializeOwned>(path: &str) -> Result<(T, String), String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("read input: {e}"))?
        .len();
    if size > 16 * 1024 * 1024 {
        return Err("input exceeds 16 MiB".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("read input: {e}"))?;
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("parse input: {e}"))?;
    Ok((value, digest(&bytes)))
}

fn output(path: &str, mut value: serde_json::Value, inputs: &[(&str, &str)]) -> Result<(), String> {
    let binary =
        fs::read(env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    value["scorer_binary_sha256"] = digest(&binary).into();
    value["input_sha256"] = inputs
        .iter()
        .map(|(name, hash)| {
            (
                (*name).to_owned(),
                serde_json::Value::String((*hash).to_owned()),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    let dir = Path::new(path);
    fs::create_dir(dir).map_err(|e| format!("create NEW output directory: {e}"))?;
    let rows = value.get("rows").and_then(|v| v.as_array());
    if let Some(rows) = rows {
        let mut file = fs::File::create(dir.join("trials.tsv")).map_err(|e| e.to_string())?;
        writeln!(file, "case_id\ttrial_id\tsession_id\tspeaker_id\texpected\toutput\tcorrect\tavailable\tlatency_us").map_err(|e| e.to_string())?;
        for row in rows {
            let fields: Vec<_> = [
                "case_id",
                "trial_id",
                "session_id",
                "speaker_id",
                "expected",
                "output",
                "correct",
                "available",
                "latency_us",
            ]
            .iter()
            .map(|key| {
                row[*key]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| row[*key].to_string())
            })
            .collect();
            writeln!(file, "{}", fields.join("\t")).map_err(|e| e.to_string())?;
        }
    }
    fs::write(
        dir.join("report.json"),
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write report: {e}"))?;
    Ok(())
}

fn execute(args: &[String]) -> Result<bool, String> {
    match args {
        [command, manifest] if command == "validate" => {
            let (m, hash): (Manifest, _) = read(manifest)?;
            m.validate()?;
            println!("manifest_sha256={hash} trials={} cases={}; schema validation is not qualification", m.trials.len(), m.cases.len());
            Ok(true)
        }
        [command, sensor, manifest, run, out] if command == "score" => {
            let (m, m_hash): (Manifest, _) = read(manifest)?;
            let (r, r_hash): (Run, _) = read(run)?;
            if r.sensor != Sensor::parse(sensor)? { return Err("command and run sensor differ".into()); }
            let report = score::score(&m, &r, &m_hash)?;
            let passed = report.qualified;
            output(out, serde_json::to_value(report).map_err(|e| e.to_string())?, &[("manifest", &m_hash), ("run", &r_hash)])?;
            println!("{}: {out}/report.json", if passed { "QUALIFIED" } else { "NOT QUALIFIED" });
            Ok(passed)
        }
        [command, comparison, out] if command == "compare" => {
            let (c, hash): (voice::Comparison, _) = read(comparison)?;
            let report = voice::compare(&c)?;
            let passed = report.passed;
            output(out, serde_json::to_value(report).map_err(|e| e.to_string())?, &[("comparison", &hash)])?;
            println!("{}: {out}/report.json", if passed { "NO REGRESSION" } else { "HOLD" });
            Ok(passed)
        }
        _ => Err("usage: lamp-sensor-score validate MANIFEST | score SENSOR MANIFEST RUN NEW_OUT_DIR | compare COMPARISON NEW_OUT_DIR".into()),
    }
}

fn main() -> ExitCode {
    match execute(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("sensor-score: {error}");
            ExitCode::from(2)
        }
    }
}
