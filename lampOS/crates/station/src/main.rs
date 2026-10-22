//! `lamp-station`: makes one production run's record from the lamp's
//! self-test report, the station's answers and the coverage matrix, keeps
//! it in `--out` (and with `--records`, in the records' place under the
//! lamp's name), and prints the run's verdict; `lamp-station records` reads
//! one lamp's records back; `lamp-station complete` says whether every
//! lamp's latest record is complete against a matrix (docs/production-testing.md).
use jiff::Timestamp;
use lamp_station::{
    Refused, Utc, complete, fleet,
    inputs::{self, Answers},
    matrix::Matrix,
    record::{self, Device, Given, Occasion, Record, RunVerdict},
    store,
};
use std::{fs, path::PathBuf, process::ExitCode};

const USAGE: &str = "usage: lamp-station --matrix PATH --answers PATH [--selftest PATH]
  --device ID --revision REVISION --batch BATCH --lampos-build COMMIT
  --suite VERSION+COMMIT --station station-ID --operator op-DIGITS
  --occasion routine|retest|qualification [--retest-of RECORD] --out DIR
  [--records PLACE]
       lamp-station records --records PLACE --device ID [--matrix PATH]
       lamp-station complete --records PLACE --matrix PATH [--device ID]...";
const HELP: &str = "Makes the record of one production test run: every check's verdict and
measurement, the failures against their limits, and the run's verdict. The
record is written whole into --out as <run id>.json; one already there is
never written over. With --records, the folder chosen for the fleet's
records, it is also kept there as <lamp>/<run id>.json, the same way.
Nothing is sent anywhere.

`lamp-station records` lists one lamp's records in that place, oldest
first: each run's verdict, matrix version and tool commit, and a file there
that is not this lamp's record with why. With --matrix, each is judged
against that matrix version: complete, or what it lacks, so a run with a
check left out never reads as passed.

Exit status: 0 the run passed, 1 it failed, 3 it is incomplete (a mandatory
check missing, skipped, interrupted or not checked), 2 no record was made
(the arguments or an input were refused, or the record could not be written,
in --out or in --records). `records`: 0 the lamp's latest run passed (and is
complete, with --matrix) and every file in its folder is its record, 1 not,
3 the lamp has none, 2 the arguments were refused or the place could not be
read.

`lamp-station complete` takes each lamp's folder in the place, or each lamp
named with --device, and judges its latest routine or retest record (by the
record's own start time) against --matrix: one line per lamp, `complete` or
`not complete` with each gap's kind and check, and the run's verdict as its
checks give it; a lamp with none is `no record`. It changes nothing. Exit
status: 0 every lamp is complete, 1 one is not, 3 there is no lamp, 2 the
arguments were refused or the place or the matrix could not be read.";

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
    records: Option<PathBuf>,
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
        records: get("--records").map(PathBuf::from),
    }))
}

const FLAGS: [&str; 14] = [
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
    "--records",
];

/// What `lamp-station records` is given.
#[derive(Debug, PartialEq)]
struct Lookup {
    place: PathBuf,
    lamp: String,
    matrix: Option<PathBuf>,
}

/// `None` for `--help`.
fn parse_lookup(given: &[String]) -> Result<Option<Lookup>, String> {
    let (mut place, mut lamp, mut matrix) = (None, None, None);
    let mut given = given.iter();
    while let Some(flag) = given.next() {
        let slot = match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--records" => &mut place,
            "--device" => &mut lamp,
            "--matrix" => &mut matrix,
            other => return Err(format!("unknown argument {other:?}")),
        };
        let value = given
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        if slot.replace(value.clone()).is_some() {
            return Err(format!("{flag} is given twice"));
        }
    }
    Ok(Some(Lookup {
        place: place.ok_or("--records is needed")?.into(),
        lamp: lamp.ok_or("--device is needed")?,
        matrix: matrix.map(PathBuf::from),
    }))
}

/// One line for each record found, oldest first, and whether the lamp
/// stands passed: its latest run passed (and is complete against the
/// matrix, when one is given), and every file in its folder is its record.
/// So a run that failed and was then retested and passed stands passed,
/// and a pass followed by a failed run does not (the Reviewer-lead on
/// #3054). The earlier runs are listed all the same.
fn standing(lamp: &str, found: &[store::Found], matrix: Option<&Matrix>) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut every_file_a_record = true;
    let mut latest_passed = false;
    for (path, read) in found {
        let line = match read {
            Err(why) => {
                every_file_a_record = false;
                format!("not a record of {lamp}: {}: {why}", path.display())
            }
            Ok(record) => {
                let mut line = format!(
                    "{} {} matrix v{} tool {}",
                    record.run_id,
                    verdict(record.verdict),
                    record.suite.matrix_version,
                    record.suite.tool_commit
                );
                latest_passed = record.verdict == RunVerdict::Pass;
                if let Some(matrix) = matrix {
                    let judged = complete::check(matrix, record);
                    latest_passed &= judged.complete() && judged.verdict == RunVerdict::Pass;
                    if judged.complete() {
                        line.push_str(&format!(", complete: {}", verdict(judged.verdict)));
                    } else {
                        let gaps: Vec<String> = judged.gaps.iter().map(|g| g.to_string()).collect();
                        line.push_str(&format!(", not complete: {}", gaps.join("; ")));
                    }
                }
                line
            }
        };
        lines.push(line);
    }
    (lines, every_file_a_record && latest_passed)
}

/// Lists one lamp's records and says whether it stands passed.
fn lookup(given: &[String]) -> ExitCode {
    let lookup = match parse_lookup(given) {
        Ok(Some(lookup)) => lookup,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-station records: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let matrix = match &lookup.matrix {
        Some(path) => match Matrix::read(path) {
            Ok(matrix) => Some(matrix),
            Err(why) => {
                eprintln!("lamp-station records: {why}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let found = match store::lamp_records(&lookup.place, &lookup.lamp) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("lamp-station records: {error}");
            return ExitCode::from(2);
        }
    };
    if found.is_empty() {
        println!(
            "no records of {} in {}",
            lookup.lamp,
            lookup.place.display()
        );
        return ExitCode::from(3);
    }
    let (lines, passed) = standing(&lookup.lamp, &found, matrix.as_ref());
    for line in lines {
        println!("{line}");
    }
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// What `lamp-station complete` is given.
#[derive(Debug, PartialEq)]
struct Pilot {
    place: PathBuf,
    matrix: PathBuf,
    /// The lamps named; every lamp's folder in the place when none.
    lamps: Vec<String>,
}

/// `None` for `--help`.
fn parse_complete(given: &[String]) -> Result<Option<Pilot>, String> {
    let (mut place, mut matrix, mut lamps) = (None, None, Vec::new());
    let mut given = given.iter();
    while let Some(flag) = given.next() {
        if matches!(flag.as_str(), "-h" | "--help") {
            return Ok(None);
        }
        let mut value = || given.next().ok_or_else(|| format!("{flag} needs a value"));
        let slot = match flag.as_str() {
            "--records" => &mut place,
            "--matrix" => &mut matrix,
            "--device" => {
                let lamp = value()?.clone();
                if lamps.contains(&lamp) {
                    return Err(format!("--device {lamp} is given twice"));
                }
                lamps.push(lamp);
                continue;
            }
            other => return Err(format!("unknown argument {other:?}")),
        };
        if slot.replace(value()?.clone()).is_some() {
            return Err(format!("{flag} is given twice"));
        }
    }
    Ok(Some(Pilot {
        place: place.ok_or("--records is needed")?.into(),
        matrix: matrix.ok_or("--matrix is needed")?.into(),
        lamps,
    }))
}

/// Says, one line per lamp, whether each lamp's latest record is complete
/// against the matrix. Reads only.
fn pilot(given: &[String]) -> ExitCode {
    let refused = |why: String| {
        eprintln!("lamp-station complete: {why}");
        ExitCode::from(2)
    };
    let pilot = match parse_complete(given) {
        Ok(Some(pilot)) => pilot,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => return refused(format!("{why}\n{USAGE}")),
    };
    let matrix = match Matrix::read(&pilot.matrix) {
        Ok(matrix) => matrix,
        Err(why) => return refused(why.to_string()),
    };
    let (names, others) = if pilot.lamps.is_empty() {
        match fleet::lamps_in(&pilot.place) {
            Ok(found) => found,
            Err(error) => {
                return refused(format!("{}: {error}", pilot.place.display()));
            }
        }
    } else {
        (pilot.lamps, Vec::new())
    };
    let mut lamps = Vec::new();
    for name in &names {
        match fleet::judge(&pilot.place, name, &matrix) {
            Ok(lamp) => lamps.push(lamp),
            Err(error) => return refused(format!("{name}: {error}")),
        }
    }
    let (lines, status) = fleet::summary(&matrix, &lamps, &others);
    for line in lines {
        println!("{line}");
    }
    ExitCode::from(status)
}

fn verdict(verdict: RunVerdict) -> &'static str {
    match verdict {
        RunVerdict::Pass => "pass",
        RunVerdict::Fail => "fail",
        RunVerdict::Incomplete => "incomplete",
    }
}

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
    out.push_str(&format!(
        "run         {} {}\n",
        record.run_id,
        verdict(record.verdict)
    ));
    out
}

fn main() -> ExitCode {
    let given: Vec<String> = std::env::args().skip(1).collect();
    if given.first().is_some_and(|first| first == "records") {
        return lookup(&given[1..]);
    }
    if given.first().is_some_and(|first| first == "complete") {
        return pilot(&given[1..]);
    }
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
    let place = arguments.records.clone();
    let record = match make(arguments) {
        Ok(record) => record,
        Err(why) => {
            eprintln!("lamp-station: refused, no record made: {why}");
            return ExitCode::from(2);
        }
    };
    let kept = match store::keep(&out, &record) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("lamp-station: no record made: {error}");
            return ExitCode::from(2);
        }
    };
    eprintln!("lamp-station: kept {}", kept.display());
    if let Some(place) = place {
        match store::keep_for_lamp(&place, &record) {
            Ok(path) => eprintln!("lamp-station: kept {}", path.display()),
            Err(error) => {
                // The run can't be made again (--out keeps it), so say how
                // its record reaches the place by hand, unless it's there.
                let by_hand = if error.kind() == std::io::ErrorKind::AlreadyExists {
                    String::new()
                } else {
                    format!(
                        "; once the place can take it, copy {} into its lamp's folder there",
                        kept.display()
                    )
                };
                eprintln!(
                    "lamp-station: kept in --out, but not in the records' place: {error}{by_hand}"
                );
                return ExitCode::from(2);
            }
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

    /// The records' place is off unless given (#2794).
    #[test]
    fn the_records_place_is_taken_only_when_given() {
        assert!(args(&ALL).unwrap().unwrap().records.is_none());
        let mut with = ALL.to_vec();
        with.extend(["--records", "/srv/station-records"]);
        assert_eq!(
            args(&with).unwrap().unwrap().records,
            Some(PathBuf::from("/srv/station-records"))
        );
        with.extend(["--records", "elsewhere"]);
        assert!(args(&with).unwrap_err().contains("twice"));
    }

    #[test]
    fn records_needs_its_place_and_a_lamp() {
        let lookup =
            |given: &[&str]| parse_lookup(&given.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            lookup(&["--records", "place", "--device", "lamp-4ace"])
                .unwrap()
                .unwrap(),
            Lookup {
                place: "place".into(),
                lamp: "lamp-4ace".into(),
                matrix: None
            }
        );
        let full = lookup(&[
            "--device",
            "lamp-4ace",
            "--matrix",
            "m.toml",
            "--records",
            "place",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(full.matrix, Some(PathBuf::from("m.toml")));
        assert!(lookup(&["--help"]).unwrap().is_none());
        assert!(lookup(&["--records", "place", "-h"]).unwrap().is_none());
        assert!(lookup(&["--device", "lamp-4ace"]).is_err());
        assert!(lookup(&["--records", "place"]).is_err());
        assert!(lookup(&["--records", "place", "--device", "a", "--device", "b"]).is_err());
        assert!(lookup(&["--records", "place", "--device", "a", "--out", "x"]).is_err());
        assert!(lookup(&["--records"]).is_err());
    }

    #[test]
    fn complete_needs_its_place_and_a_matrix_and_takes_lamps() {
        let pilot = |given: &[&str]| {
            parse_complete(&given.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(
            pilot(&["--records", "place", "--matrix", "m.toml"])
                .unwrap()
                .unwrap(),
            Pilot {
                place: "place".into(),
                matrix: "m.toml".into(),
                lamps: Vec::new()
            }
        );
        let named = pilot(&[
            "--device",
            "lamp-4ace",
            "--records",
            "place",
            "--device",
            "lamp-52e6",
            "--matrix",
            "m.toml",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(named.lamps, ["lamp-4ace", "lamp-52e6"]);
        assert!(pilot(&["--help"]).unwrap().is_none());
        assert!(pilot(&["--records", "place"]).is_err());
        assert!(pilot(&["--matrix", "m.toml"]).is_err());
        assert!(
            pilot(&[
                "--records",
                "p",
                "--matrix",
                "m",
                "--device",
                "a",
                "--device",
                "a"
            ])
            .is_err()
        );
        assert!(pilot(&["--records", "p", "--records", "q", "--matrix", "m"]).is_err());
        assert!(pilot(&["--records", "p", "--matrix", "m", "--out", "x"]).is_err());
        assert!(pilot(&["--records", "p", "--matrix", "m", "--device"]).is_err());
    }

    /// The Reviewer-lead on #3054: a lamp stands by its latest run. A
    /// failed run then a passed retest stands passed; a pass then a failed
    /// run doesn't; a file in its folder that isn't its record never stands
    /// passed.
    #[test]
    fn a_lamp_stands_by_its_latest_run() {
        let record = |run: &str, verdict: RunVerdict| {
            let record = Record {
                format: record::FORMAT,
                run_id: run.into(),
                occasion: Occasion::Routine,
                verdict,
                started: "2026-10-19T08:00:00Z".parse().unwrap(),
                ended: "2026-10-19T08:30:00Z".parse().unwrap(),
                device: Device {
                    id: "lamp-4ace".into(),
                    hardware_revision: "draft-a".into(),
                    batch: "pilot-1".into(),
                },
                lampos_build: "dab3f12".into(),
                suite: record::Suite {
                    matrix_version: "0".into(),
                    matrix_sha256: "0".repeat(64),
                    matrix_approved: false,
                    tool_commit: "dab3f12".into(),
                },
                station: "station-01".into(),
                operator: "op-7".into(),
                retest_of: None,
                repairs: Vec::new(),
                selftest: None,
                checks: Vec::new(),
                failures: Vec::new(),
                incomplete: Vec::new(),
            };
            (PathBuf::from(format!("{run}.json")), Ok(record))
        };
        let failed = record("20261019T080000Z-station-01-lamp-4ace", RunVerdict::Fail);
        let retested = record("20261019T090000Z-station-01-lamp-4ace", RunVerdict::Pass);
        let (lines, passed) = standing("lamp-4ace", &[failed.clone(), retested.clone()], None);
        assert!(passed);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(" fail "), "{lines:?}");
        assert!(!standing("lamp-4ace", &[retested.clone(), failed.clone()], None).1);
        let stray: store::Found = (PathBuf::from("x.json"), Err(Refused("half".into())));
        assert!(!standing("lamp-4ace", &[failed, retested, stray], None).1);
    }
}
