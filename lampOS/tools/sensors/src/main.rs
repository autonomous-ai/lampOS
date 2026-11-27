use lamp_sensor_score::{
    archive, digest,
    schema::{Manifest, Run, Sensor},
    score, voice, whole,
};
use serde::de::DeserializeOwned;
use std::{env, fs, path::Path, process::ExitCode};

fn read<T: DeserializeOwned>(path: &str) -> Result<(T, String), String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("read input: {e}"))?
        .len();
    if size > 16 * 1024 * 1024 {
        return Err("input exceeds 16 MiB".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("read input: {e}"))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("input exceeds 16 MiB".into());
    }
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("parse input: {e}"))?;
    Ok((value, digest(&bytes)))
}

fn provenance(
    mut value: serde_json::Value,
    inputs: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
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
    Ok(value)
}

fn output(path: &str, value: serde_json::Value, inputs: &[(&str, &str)]) -> Result<(), String> {
    let value = provenance(value, inputs)?;
    let dir = Path::new(path);
    archive::private_dir(dir)?;
    let mut sums = String::new();
    let rows = value.get("rows").and_then(|v| v.as_array());
    if let Some(rows) = rows {
        let keys = if value.get("purpose").is_some() {
            vec![
                "case_id",
                "leg",
                "mode",
                "correct",
                "missing",
                "unknown",
                "timing_unknown",
                "safety_violation",
                "latency_us",
            ]
        } else if value["version"] == 2 {
            vec![
                "case_id",
                "trial_id",
                "session_id",
                "speaker_id",
                "split",
                "data_origin",
                "applicable",
                "reference_trial_id",
                "expected",
                "output",
                "correct",
                "available",
                "latency_us",
                "stop_latency_us",
            ]
        } else {
            vec![
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
        };
        let mut tsv = format!("{}\n", keys.join("\t"));
        for row in rows {
            let fields: Vec<_> = keys
                .iter()
                .map(|key| {
                    row[*key]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| row[*key].to_string())
                })
                .collect();
            tsv.push_str(&fields.join("\t"));
            tsv.push('\n');
        }
        archive::write_new(&dir.join("trials.tsv"), tsv.as_bytes())?;
        sums.push_str(&format!("{}  trials.tsv\n", digest(tsv.as_bytes())));
    }
    let report = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    archive::write_new(&dir.join("report.json"), &report)?;
    sums.push_str(&format!("{}  report.json\n", digest(&report)));
    let readme = b"# Private sensor score output\n\nRetention: keep_all. Retain every run, including failures and earlier versions.\nNever delete or overwrite these files. Keep real people's evidence, labels,\ntrial rows and paths local. Publish aggregate counts, limits and hashes only.\n\nSHA256SUMS hashes every other output file. report.json records exact input\nand scorer executable hashes. Keep those original inputs and their local\nraw inventories too. A passing tool report does not authorize a switch-on,\nmerge or train; the lead and reviewer still verify provenance and scope.\n";
    archive::write_new(&dir.join("README.md"), readme)?;
    sums.push_str(&format!("{}  README.md\n", digest(readme)));
    archive::write_new(&dir.join("SHA256SUMS"), sums.as_bytes())?;
    Ok(())
}

fn prerequisite(
    c: &whole::Comparison,
    manifest: &str,
    run: &str,
    path: &str,
) -> Result<Option<String>, String> {
    if c.purpose == whole::Purpose::BaselineCharacterization {
        if [manifest, run, path] != ["-", "-", "-"] {
            return Err(
                "C/D characterization uses '- - -' in place of a sensor prerequisite".into(),
            );
        }
        return Ok(None);
    }
    let p = c
        .prerequisite
        .as_ref()
        .ok_or("missing prerequisite metadata")?;
    let (r, hash): (serde_json::Value, _) = read(path)?;
    let (m, m_hash): (Manifest, _) = read(manifest)?;
    let (run, run_hash): (Run, _) = read(run)?;
    let computed = score::score(&m, &run, &m_hash)?;
    let expected = provenance(
        serde_json::to_value(computed).map_err(|e| e.to_string())?,
        &[("manifest", &m_hash), ("run", &run_hash)],
    )?;
    if hash != p.report_sha256
        || r["version"] != 2
        || r["rubric_id"] != whole::RUBRIC
        || r["sensor"].as_str() != c.added_sensor.as_deref()
        || r["qualified"] != true
        || r["evidence"] != "lamp_live"
        || r != expected
        || r["model_sha256"].as_str() != Some(p.model_sha256.as_str())
        || r["config_sha256"].as_str() != Some(p.config_sha256.as_str())
    {
        return Err("prerequisite must reproduce exactly from its original manifest/run with this scorer, qualify v2 on-lamp, and bind the added component's model/config".into());
    }
    Ok(Some(hash))
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
        [command, comparison, sensor_manifest, sensor_run, sensor_report, out] if command == "compare-whole" => {
            let (c, hash): (whole::Comparison, _) = read(comparison)?;
            let binary = fs::read(env::current_exe().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if c.scorer_binary_sha256 != digest(&binary) { return Err("whole comparison declares a different scorer executable; freeze this binary before collection".into()); }
            let p_hash = prerequisite(&c, sensor_manifest, sensor_run, sensor_report)?;
            let report = whole::compare(&c)?;
            let passed = report.passed;
            let mut inputs = vec![("comparison", hash.as_str())];
            if let Some(p) = &p_hash { inputs.push(("sensor_report", p.as_str())); }
            output(out, serde_json::to_value(report).map_err(|e| e.to_string())?, &inputs)?;
            println!("{}: {out}/report.json", if passed { "NUMERIC GATE PASSED; POLICY HOLD REMAINS" } else { "HOLD / CHARACTERIZATION ONLY" });
            Ok(passed)
        }
        [command, cases] if command == "whole-cases-hash" => {
            let (cases, _): (Vec<whole::Case>, _) = read(cases)?;
            println!("{}", whole::cases_hash(&cases)?);
            Ok(true)
        }
        [command, root, out] if command == "archive" => {
            let (inventory, readme) = archive::create(Path::new(root), Path::new(out))?;
            println!("inventory_sha256={inventory}\nreadme_sha256={readme}\nPRIVATE LOCAL INVENTORY; originals remain in place");
            Ok(true)
        }
        [command, root, inventory_path] if command == "verify-archive" => {
            let (inventory, hash): (archive::Inventory, _) = read(inventory_path)?;
            let readme = Path::new(inventory_path).with_file_name("README.md");
            archive::verify(Path::new(root), &inventory, &fs::read(readme).map_err(|e| e.to_string())?)?;
            println!("VERIFIED inventory_sha256={hash}; all files and README match");
            Ok(true)
        }
        _ => Err("usage: lamp-sensor-score validate MANIFEST | score SENSOR MANIFEST RUN NEW_OUT_DIR | compare COMPARISON NEW_OUT_DIR | compare-whole COMPARISON SENSOR_MANIFEST SENSOR_RUN SENSOR_REPORT NEW_OUT_DIR (use - - - for C/D) | whole-cases-hash CASES | archive RAW_ROOT NEW_OUT_DIR | verify-archive RAW_ROOT INVENTORY".into()),
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
