//! `lamp-station`: makes one production run's record from the lamp's
//! self-test report, the station's answers and the coverage matrix, keeps
//! it in `--out`, and prints the run's verdict (docs/production-testing.md).
use jiff::Timestamp;
use lamp_station::{
    Refused, Utc,
    inputs::{self, Answers},
    matrix::Matrix,
    record::{self, Device, Given, Occasion, Record, RunVerdict},
    store,
};
use std::{fs, path::PathBuf, process::ExitCode};

const USAGE: &str = "usage: lamp-station --matrix PATH --answers PATH [--selftest PATH]
  --device ID --revision REVISION --batch BATCH --lampos-build COMMIT
  --suite VERSION+COMMIT --station station-ID --operator op-DIGITS
  --occasion routine|retest|qualification [--retest-of RECORD] --out DIR";
const HELP: &str = "Makes the record of one production test run: every check's verdict and
measurement, the failures against their limits, and the run's verdict. The
record is written whole into --out as <run id>.json; one already there is
never written over. Nothing is sent anywhere.

Exit status: 0 the run passed, 1 it failed, 3 it is incomplete (a mandatory
check missing, skipped, interrupted or not checked), 2 no record was made
(the arguments or an input were refused, or the record could not be written).";

#[derive(Debug)]
struct Arguments {
    matrix: PathBuf,
    answers: PathBuf,
    selftest: Option<PathBuf>,
    device: Device,
    lampos_build: String,
    suite: String,
    station: String,
    operator: String,
    occasion: Occasion,
    retest_of: Option<PathBuf>,
    out: PathBuf,
}

fn parse(given: &[String]) -> Result<Option<Arguments>, String> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut given = given.iter();
    while let Some(flag) = given.next() {
        let flag = flag.as_str();
        if matches!(flag, "-h" | "--help") {
            return Ok(None);
        }
        if !FLAGS.contains(&flag) {
            return Err(format!("unknown argument {flag:?}"));
        }
        let value = given
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        if seen.iter().any(|(f, _)| *f == flag) {
            return Err(format!("{flag} is given twice"));
        }
        seen.push((flag, value));
    }
    let get = |flag: &str| seen.iter().find(|(f, _)| *f == flag).map(|(_, v)| *v);
    let need = |flag: &str| get(flag).ok_or_else(|| format!("{flag} is needed"));
    let occasion = match need("--occasion")? {
        "routine" => Occasion::Routine,
        "retest" => Occasion::Retest,
        "qualification" => Occasion::Qualification,
        other => return Err(format!("no occasion {other:?}")),
    };
    Ok(Some(Arguments {
        matrix: need("--matrix")?.into(),
        answers: need("--answers")?.into(),
        selftest: get("--selftest").map(PathBuf::from),
        device: Device {
            id: need("--device")?.into(),
            hardware_revision: need("--revision")?.into(),
            batch: need("--batch")?.into(),
        },
        lampos_build: need("--lampos-build")?.into(),
        suite: need("--suite")?.into(),
        station: need("--station")?.into(),
        operator: need("--operator")?.into(),
        occasion,
        retest_of: get("--retest-of").map(PathBuf::from),
        out: need("--out")?.into(),
    }))
}

const FLAGS: [&str; 13] = [
    "--matrix",
    "--answers",
    "--selftest",
    "--device",
    "--revision",
    "--batch",
    "--lampos-build",
    "--suite",
    "--station",
    "--operator",
    "--occasion",
    "--retest-of",
    "--out",
];

fn read(what: &str, path: &PathBuf) -> Result<String, Refused> {
    fs::read_to_string(path)
        .map_err(|error| Refused(format!("{what} {} cannot be read: {error}", path.display())))
}

fn make(arguments: Arguments) -> Result<Record, Refused> {
    let matrix = Matrix::read(&arguments.matrix)?;
    let answers = Answers::parse(&read("the answers", &arguments.answers)?)?;
    let selftest = match &arguments.selftest {
        Some(path) => Some(inputs::selftest(&read("the self-test report", path)?)?),
        None => None,
    };
    let earlier = match &arguments.retest_of {
        Some(path) => Some(Record::parse(&read("the earlier record", path)?)?),
        None => None,
    };
    record::make(Given {
        matrix: &matrix,
        selftest: selftest.as_ref(),
        answers: &answers,
        device: arguments.device,
        lampos_build: arguments.lampos_build,
        suite: arguments.suite,
        station: arguments.station,
        operator: arguments.operator,
        occasion: arguments.occasion,
        earlier: earlier.as_ref(),
        now: Utc(Timestamp::now()),
    })
}

/// What a person reads: each failure and each unfinished check, then the
/// run's verdict.
fn summary(record: &Record) -> String {
    let mut out = String::new();
    for failure in &record.failures {
        out.push_str(&format!(
            "fail        {:<28} {}\n",
            failure.check, failure.reason
        ));
    }
    for unfinished in &record.incomplete {
        let verdict = serde_json::to_value(unfinished.verdict)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        out.push_str(&format!(
            "incomplete  {:<28} {verdict}: {}\n",
            unfinished.check, unfinished.reason
        ));
    }
    let verdict = match record.verdict {
        RunVerdict::Pass => "pass",
        RunVerdict::Fail => "fail",
        RunVerdict::Incomplete => "incomplete",
    };
    out.push_str(&format!("run         {} {verdict}\n", record.run_id));
    out
}

fn main() -> ExitCode {
    let given: Vec<String> = std::env::args().skip(1).collect();
    let arguments = match parse(&given) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-station: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let out = arguments.out.clone();
    let record = match make(arguments) {
        Ok(record) => record,
        Err(why) => {
            eprintln!("lamp-station: refused, no record made: {why}");
            return ExitCode::from(2);
        }
    };
    match store::keep(&out, &record) {
        Ok(path) => eprintln!("lamp-station: kept {}", path.display()),
        Err(error) => {
            eprintln!("lamp-station: no record made: {error}");
            return ExitCode::from(2);
        }
    }
    print!("{}", summary(&record));
    match record.verdict {
        RunVerdict::Pass => ExitCode::SUCCESS,
        RunVerdict::Fail => ExitCode::FAILURE,
        RunVerdict::Incomplete => ExitCode::from(3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(given: &[&str]) -> Result<Option<Arguments>, String> {
        parse(&given.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    const ALL: [&str; 24] = [
        "--matrix",
        "production/matrix-v0.toml",
        "--answers",
        "answers.json",
        "--device",
        "lamp-4ace",
        "--revision",
        "draft-a",
        "--batch",
        "pilot-1",
        "--lampos-build",
        "dab3f12",
        "--suite",
        "0+dab3f12",
        "--station",
        "station-01",
        "--operator",
        "op-7",
        "--occasion",
        "routine",
        "--out",
        "records",
        "--selftest",
        "latest.json",
    ];

    #[test]
    fn every_input_is_named_and_nothing_unknown_is_taken() {
        let all = args(&ALL).unwrap().unwrap();
        assert_eq!(all.occasion, Occasion::Routine);
        assert_eq!(all.device.hardware_revision, "draft-a");
        assert_eq!(all.selftest, Some(PathBuf::from("latest.json")));
        assert!(all.retest_of.is_none());
        // The self-test and the earlier run are optional; the rest are not.
        assert!(args(&ALL[..22]).unwrap().is_some());
        for at in (0..22).step_by(2) {
            let mut fewer = ALL.to_vec();
            fewer.drain(at..at + 2);
            assert!(args(&fewer).is_err(), "{}", ALL[at]);
        }
        let mut unknown = ALL.to_vec();
        unknown.extend(["--operator-name", "x"]);
        assert!(args(&unknown).is_err());
        let mut twice = ALL.to_vec();
        twice.extend(["--device", "lamp-52e6"]);
        assert!(args(&twice).unwrap_err().contains("twice"));
        let mut later = ALL.to_vec();
        later[19] = "later";
        assert!(args(&later).is_err());
        assert!(args(&["--matrix"]).is_err());
        assert!(args(&["--help"]).unwrap().is_none());
    }
}
