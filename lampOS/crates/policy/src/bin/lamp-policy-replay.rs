//! The Policy's logged attention decisions asked again of other decision
//! models (#871): `lamp_policy::replay`. Run where the logs are; it prints
//! no words. Nothing on the bus, nothing acts.
use lamp_policy::{
    decide::{DEFAULT_ENDPOINT, Decider, Jev},
    replay::{self, Should},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::Duration,
};

const USAGE: &str = "usage: lamp-policy-replay --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR --days YYYY-MM-DD[,YYYY-MM-DD...] --models NAME[,NAME...] [--labels ABSOLUTE_FILE] [--only-labelled] [--endpoint URL] [--deadline-ms 10000] [--pace-ms 500] [--most N]\n       lamp-policy-replay --again ABSOLUTE_FILE --labels ABSOLUTE_FILE [--refit [--cost-ratio N]]\n       lamp-policy-replay --pictures --log-dir ABSOLUTE_DIR --days YYYY-MM-DD[,...] [--labels ABSOLUTE_FILE]\n       lamp-policy-replay --agreement --log-dir ABSOLUTE_DIR --days YYYY-MM-DD[,...] [--from YYYY-MM-DDTHH:MM:SSZ] [--until YYYY-MM-DDTHH:MM:SSZ]\n       lamp-policy-replay --probe-connections --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR --days YYYY-MM-DD[,...] --endpoint URL [--models NAME] [--deadline-ms 10000] [--rounds N]";
const HELP: &str = "Asks each attention decision in the decisions logs of the days given again, of each\n\
model given in turn (at most four), as it was asked on the day, and prints one line per\n\
decision and a summary: each model's choice, probability and time, and the kinds of what\n\
was heard (words, the lamp named, another language), never the words. --labels gives what\n\
should have happened, one JSON line each: {\"at_unix_ms\": N, \"should\": \"answer\"|\"stay_quiet\"}.\n\
--again counts an earlier run's printed lines again with --labels, and asks nothing; with\n\
--refit it also fits the weights again to each model's own answers, on half the labelled\n\
lines, and counts on the other half, 200 times, beside the day's weights on the same halves;\n\
--cost-ratio N (1 to 20, default 1) makes an answer that was not for the lamp cost N times\n\
an ignored request, in the fit and in the counts' cost.\n\
--pictures counts the lines lamp-policy --images wrote (pictures-YYYY-MM-DD.jsonl), a label\n\
taken for the line nearest it within 3 s, and asks nothing.\n\
--agreement sets each decision a model made beside what the rule would have said of the same\n\
context, counted by kind (room, conversation, the lamp speaking, the name's score), and names\n\
the kinds of at least 20 decisions with no disagreement; it asks nothing (#705). --from and\n\
--until (UTC) keep only the decisions made from and before those moments: a night held out\n\
from the days its kinds were picked on.\n\
--probe-connections takes the newest logged question a model decided, asks it seven ways,\n\
about 17 minutes and seven askings a round, and prints one line each as it is made with\n\
what it was sent on (new, kept, ahead, replaced) and how long it took: a new connection\n\
after 5 quiet minutes and at once, the same one at once, after 140 s and after 300 s, and\n\
one made ahead at once and after 5 quiet minutes (#1522). --endpoint is required: the one\n\
lamp-policy asks. --rounds (1 to 3, default 1) repeats it. Run it with the room quiet.\n\
What was heard is sent to the decision model's service again, as on the day.";

#[derive(Debug, PartialEq)]
enum Run {
    Ask(Arguments),
    /// An earlier run's lines, and the labels to count them with; and
    /// whether to fit the weights again to each model's answers.
    Again {
        lines: PathBuf,
        labels: PathBuf,
        refit: bool,
        /// What an answer not for the lamp costs, in ignored requests.
        cost: f64,
    },
    /// The picture lines of these days, counted (#871).
    Pictures {
        log: PathBuf,
        days: Vec<String>,
        labels: Option<PathBuf>,
    },
    /// One logged question asked seven ways, to time each kind of
    /// connection (#1522).
    Probe {
        key: PathBuf,
        log: PathBuf,
        days: Vec<String>,
        endpoint: String,
        model: String,
        deadline: Duration,
        rounds: u32,
    },
    /// The rule beside the model on these days' decisions (#705).
    Agreement {
        log: PathBuf,
        days: Vec<String>,
        /// Only decisions made from `from` and before `until`, in ms since
        /// 1970 (UTC).
        from: Option<u64>,
        until: Option<u64>,
    },
}

/// A kind is named certain with at least this many decisions and no
/// disagreement.
const AGREEMENT_LEAST: u32 = 20;

#[derive(Debug, PartialEq)]
struct Arguments {
    key: PathBuf,
    log: PathBuf,
    days: Vec<String>,
    models: Vec<String>,
    labels: Option<PathBuf>,
    only_labelled: bool,
    endpoint: String,
    deadline: Duration,
    pace: Duration,
    most: Option<usize>,
}

fn absolute(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("file and directory paths must be absolute".into())
    }
}

fn number(value: &str, least: u64, most: u64) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| (least..=most).contains(n))
        .ok_or_else(|| format!("expected a whole number from {least} to {most}"))
}

fn day(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Run>, String> {
    let mut args = args;
    let (mut key, mut log, mut days, mut models, mut labels) = (None, None, None, None, None);
    let mut again = None;
    let mut pictures = false;
    let mut agreement = false;
    let mut probe = false;
    let mut rounds = None;
    let (mut from, mut until) = (None, None);
    let mut only_labelled = false;
    let mut refit = false;
    let mut cost = None;
    let mut endpoint = DEFAULT_ENDPOINT.to_owned();
    let (mut deadline, mut pace, mut most) = (10_000, 500, None);
    let mut seen = BTreeSet::new();
    while let Some(option) = args.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        if !seen.insert(option.clone()) {
            return Err(format!("{option} is given twice"));
        }
        if option == "--only-labelled" {
            only_labelled = true;
            continue;
        }
        if option == "--pictures" {
            pictures = true;
            continue;
        }
        if option == "--agreement" {
            agreement = true;
            continue;
        }
        if option == "--probe-connections" {
            probe = true;
            continue;
        }
        if option == "--refit" {
            refit = true;
            continue;
        }
        let value = args.next().ok_or(format!("{option} needs a value"))?;
        match option.as_str() {
            "--key-file" => key = Some(absolute(&value)?),
            "--log-dir" => log = Some(absolute(&value)?),
            "--labels" => labels = Some(absolute(&value)?),
            "--again" => again = Some(absolute(&value)?),
            "--days" => {
                let listed: Vec<String> = value.split(',').map(str::to_owned).collect();
                if listed.is_empty() || !listed.iter().all(|listed| day(listed)) {
                    return Err("--days is YYYY-MM-DD, separated by commas".into());
                }
                days = Some(listed);
            }
            "--models" => {
                let listed: Vec<String> = value.split(',').map(str::to_owned).collect();
                if !(1..=4).contains(&listed.len())
                    || listed.iter().any(|model| {
                        model.is_empty() || model.chars().any(|c| c.is_control() || c == ' ')
                    })
                {
                    return Err("--models is one to four model names, separated by commas".into());
                }
                models = Some(listed);
            }
            "--from" | "--until" => {
                let at = replay::utc_ms(&value)
                    .ok_or(format!("{option} is a UTC moment, YYYY-MM-DDTHH:MM:SSZ"))?;
                if option == "--from" {
                    from = Some(at);
                } else {
                    until = Some(at);
                }
            }
            "--endpoint" => endpoint = value,
            "--deadline-ms" => deadline = number(&value, 1, 60_000)?,
            "--pace-ms" => pace = number(&value, 0, 10_000)?,
            "--most" => most = Some(number(&value, 1, 100_000)? as usize),
            "--rounds" => rounds = Some(number(&value, 1, 3)? as u32),
            "--cost-ratio" => {
                cost = Some(
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|cost| (1.0..=replay::refit::MOST_COST).contains(cost))
                        .ok_or("--cost-ratio is a number from 1 to 20")?,
                );
            }
            _ => return Err(format!("{option} is not an option")),
        }
    }
    if rounds.is_some() && !probe {
        return Err("--rounds goes with --probe-connections only".into());
    }
    if probe {
        if agreement
            || pictures
            || again.is_some()
            || labels.is_some()
            || only_labelled
            || refit
            || cost.is_some()
            || from.is_some()
            || until.is_some()
            || most.is_some()
            || seen.contains("--pace-ms")
        {
            return Err("--probe-connections takes --key-file, --log-dir, --days, --endpoint, --models, --deadline-ms and --rounds only".into());
        }
        // Only where lamp-policy itself asks: the default is not the
        // lamp's (the reviewer on #1630).
        if !seen.contains("--endpoint") {
            return Err("--probe-connections needs --endpoint, the one lamp-policy asks".into());
        }
        let model = match models {
            None => lamp_policy::decide::DEFAULT_MODEL.to_owned(),
            Some(models) if models.len() == 1 => models[0].clone(),
            Some(_) => return Err("--probe-connections asks one model".into()),
        };
        return Ok(Some(Run::Probe {
            key: key.ok_or("--key-file is required")?,
            log: log.ok_or("--log-dir is required")?,
            days: days.ok_or("--days is required")?,
            endpoint,
            model,
            deadline: Duration::from_millis(deadline),
            rounds: rounds.unwrap_or(1),
        }));
    }
    // A window means something only to `--agreement` (the reviewer on #1539).
    if !agreement && (from.is_some() || until.is_some()) {
        return Err("--from and --until go with --agreement only".into());
    }
    if agreement {
        let window = usize::from(from.is_some()) + usize::from(until.is_some());
        if seen.len() != 3 + window || pictures {
            return Err("--agreement takes --log-dir, --days, --from and --until only".into());
        }
        if let (Some(from), Some(until)) = (from, until)
            && from >= until
        {
            return Err("--from must come before --until".into());
        }
        return Ok(Some(Run::Agreement {
            log: log.ok_or("--log-dir is required")?,
            days: days.ok_or("--days is required")?,
            from,
            until,
        }));
    }
    if pictures {
        if key.is_some() || models.is_some() || again.is_some() || only_labelled || refit {
            return Err("--pictures takes --log-dir, --days and --labels only".into());
        }
        return Ok(Some(Run::Pictures {
            log: log.ok_or("--log-dir is required")?,
            days: days.ok_or("--days is required")?,
            labels,
        }));
    }
    if let Some(lines) = again {
        if seen.len() != 2 + usize::from(refit) + usize::from(cost.is_some()) {
            return Err("--again takes --labels, --refit and --cost-ratio and nothing else".into());
        }
        if cost.is_some() && !refit {
            return Err("--cost-ratio is for --refit".into());
        }
        let labels = labels.ok_or("--again needs --labels")?;
        return Ok(Some(Run::Again {
            lines,
            labels,
            refit,
            cost: cost.unwrap_or(1.0),
        }));
    }
    if refit || cost.is_some() {
        return Err("--refit and --cost-ratio are for --again".into());
    }
    if only_labelled && labels.is_none() {
        return Err("--only-labelled needs --labels".into());
    }
    Ok(Some(Run::Ask(Arguments {
        key: key.ok_or("--key-file is required")?,
        log: log.ok_or("--log-dir is required")?,
        days: days.ok_or("--days is required")?,
        models: models.ok_or("--models is required")?,
        labels,
        only_labelled,
        endpoint,
        deadline: Duration::from_millis(deadline),
        pace: Duration::from_millis(pace),
        most,
    })))
}

fn pictures(log: &Path, days: &[String], labels: Option<&PathBuf>) -> io::Result<()> {
    let labels = match labels {
        Some(path) => replay::labels(&std::fs::read_to_string(path)?).map_err(io::Error::other)?,
        None => BTreeMap::new(),
    };
    let lines = replay::read_pictures(log, days)?;
    println!("{}", replay::pictures_summary(&lines, &labels));
    Ok(())
}

fn agreement(log: &Path, days: &[String], from: Option<u64>, until: Option<u64>) -> io::Result<()> {
    let (mut logged, unread) = replay::read(log, days)?;
    // Said first, so an empty table from a folder or day with no file reads
    // apart from a window with no decisions (Fourth on #1539).
    let read = logged.len();
    replay::within(&mut logged, from, until);
    println!(
        "attention decisions read from the days given: {read}; within the window: {}",
        logged.len()
    );
    println!(
        "{}",
        replay::agreement_summary(&replay::agreement(&logged), AGREEMENT_LEAST)
    );
    println!("lines that could not be read as a decision: {unread}");
    Ok(())
}

fn again(lines: &PathBuf, labels: &PathBuf, refit: bool, cost: f64) -> io::Result<()> {
    let labels = replay::labels(&std::fs::read_to_string(labels)?).map_err(io::Error::other)?;
    let (lines, models) =
        replay::relabelled(&std::fs::read_to_string(lines)?, &labels).map_err(io::Error::other)?;
    println!("{}", replay::summary(&lines, &models));
    if refit {
        println!("{}", replay::refit::summary_costing(&lines, &models, cost));
    }
    Ok(())
}

fn run(arguments: &Arguments) -> io::Result<()> {
    let labels: BTreeMap<u64, Should> = match &arguments.labels {
        Some(path) => replay::labels(&std::fs::read_to_string(path)?).map_err(io::Error::other)?,
        None => BTreeMap::new(),
    };
    let mut judges: Vec<Jev> = arguments
        .models
        .iter()
        .map(|model| Jev::new(&arguments.endpoint, model, &arguments.key))
        .collect::<io::Result<_>>()?;
    let (logged, unread) = replay::read(&arguments.log, &arguments.days)?;
    let chosen: Vec<_> = logged
        .into_iter()
        .filter(|logged| !arguments.only_labelled || labels.contains_key(&logged.at_unix_ms))
        .take(arguments.most.unwrap_or(usize::MAX))
        .collect();
    eprintln!(
        "lamp-policy-replay: {} decisions to ask of {} ({} lines not read)",
        chosen.len(),
        arguments.models.join(", "),
        unread
    );
    let mut lines = Vec::new();
    let mut stdout = io::stdout().lock();
    let mut first = true;
    for (index, logged) in chosen.iter().enumerate() {
        let mut replayed = Vec::new();
        // The same question to each model, one after the other.
        for (model, judge) in arguments.models.iter().zip(judges.iter_mut()) {
            if !first {
                thread::sleep(arguments.pace);
            }
            first = false;
            let judge: &mut dyn Decider = judge;
            replayed.push((
                model.clone(),
                replay::ask(judge, logged, arguments.deadline),
            ));
        }
        let line = replay::line(logged, labels.get(&logged.at_unix_ms).copied(), &replayed);
        writeln!(stdout, "{line}")?;
        lines.push(line);
        if (index + 1) % 20 == 0 {
            eprintln!("lamp-policy-replay: {} of {}", index + 1, chosen.len());
        }
    }
    writeln!(stdout, "{}", replay::summary(&lines, &arguments.models))?;
    stdout.flush()
}

/// `--probe-connections` (#1522): the newest logged question a model
/// decided, asked seven ways; a line each as it is made.
fn probe(
    key: &Path,
    log: &Path,
    days: &[String],
    endpoint: &str,
    model: &str,
    deadline: Duration,
    rounds: u32,
) -> io::Result<()> {
    let mut judge = Jev::new(endpoint, model, key)?;
    let (logged, _) = replay::read(log, days)?;
    let question = replay::newest_asked(&logged).ok_or_else(|| {
        io::Error::other("no attention decision a model made in those days' logs")
    })?;
    eprintln!(
        "lamp-policy-replay: {} asked seven ways, {rounds} round(s), about {} minutes",
        question.id,
        rounds * 18
    );
    let mut written = Ok(());
    replay::probe_connections(
        &mut judge,
        question,
        deadline,
        rounds,
        &mut |wait| {
            if !wait.is_zero() {
                eprintln!("lamp-policy-replay: nothing asked for {} s", wait.as_secs());
                thread::sleep(wait);
            }
        },
        &mut |line| {
            if written.is_ok() {
                let mut stdout = io::stdout().lock();
                written = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
            }
        },
    );
    written
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(None) => {
            println!("{USAGE}\n{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Some(chosen)) => match match &chosen {
            Run::Ask(arguments) => run(arguments),
            Run::Probe {
                key,
                log,
                days,
                endpoint,
                model,
                deadline,
                rounds,
            } => probe(key, log, days, endpoint, model, *deadline, *rounds),
            Run::Again {
                lines,
                labels,
                refit,
                cost,
            } => again(lines, labels, *refit, *cost),
            Run::Pictures { log, days, labels } => pictures(log, days, labels.as_ref()),
            Run::Agreement {
                log,
                days,
                from,
                until,
            } => agreement(log, days, *from, *until),
        } {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-policy-replay: {error}");
                ExitCode::FAILURE
            }
        },
        Err(why) => {
            eprintln!("lamp-policy-replay: {why}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(line: &str) -> Result<Option<Arguments>, String> {
        match parse(line.split_whitespace().map(str::to_owned))? {
            Some(Run::Ask(arguments)) => Ok(Some(arguments)),
            Some(
                Run::Again { .. }
                | Run::Pictures { .. }
                | Run::Agreement { .. }
                | Run::Probe { .. },
            ) => Err("another run".into()),
            None => Ok(None),
        }
    }

    /// #1522: the probe takes the key, the logs and the days, one model at
    /// most (lamp-policy's by default), and nothing that is another run's.
    #[test]
    fn the_connection_probe_takes_its_own_options_only() {
        let probe = |line: &str| parse(line.split_whitespace().map(str::to_owned));
        let read = probe("--probe-connections --key-file /k --log-dir /d --days 2026-10-18 --endpoint https://campaign-api.autonomous.ai/x --rounds 2").unwrap();
        // Never without the endpoint lamp-policy asks.
        assert!(probe("--probe-connections --key-file /k --log-dir /d --days 2026-10-18").is_err());
        match read {
            Some(Run::Probe {
                model,
                rounds,
                endpoint,
                deadline,
                ..
            }) => {
                assert_eq!(model, lamp_policy::decide::DEFAULT_MODEL);
                assert_eq!(rounds, 2);
                assert_eq!(endpoint, "https://campaign-api.autonomous.ai/x");
                assert_eq!(deadline, Duration::from_secs(10));
            }
            other => panic!("{other:?}"),
        }
        for refused in [
            "--probe-connections --key-file /k --log-dir /d --days 2026-10-18 --endpoint https://e/x --models a,b",
            "--probe-connections --key-file /k --log-dir /d --days 2026-10-18 --endpoint https://e/x --rounds 4",
            "--probe-connections --key-file /k --log-dir /d --days 2026-10-18 --endpoint https://e/x --most 3",
            "--probe-connections --key-file /k --log-dir /d --days 2026-10-18 --endpoint https://e/x --agreement",
            "--probe-connections --log-dir /d --days 2026-10-18 --endpoint https://e/x",
            "--key-file /k --log-dir /d --days 2026-10-18 --models m --rounds 2",
        ] {
            assert!(probe(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn the_arguments_name_the_logs_the_days_and_the_models() {
        let arguments = of("--key-file /k --log-dir /var/lib/lampos-policy --days 2026-10-14,2026-10-15 --models typesafe/jev-1.13,typesafe/jev-1.14")
            .unwrap()
            .unwrap();
        assert_eq!(arguments.days, ["2026-10-14", "2026-10-15"]);
        assert_eq!(arguments.models, ["typesafe/jev-1.13", "typesafe/jev-1.14"]);
        assert_eq!(arguments.endpoint, DEFAULT_ENDPOINT);
        assert_eq!(arguments.deadline, Duration::from_secs(10));
        assert_eq!(arguments.pace, Duration::from_millis(500));
        assert_eq!((arguments.most, arguments.only_labelled), (None, false));
        let labelled = of("--key-file /k --log-dir /d --days 2026-10-15 --models m --labels /l --only-labelled --pace-ms 0 --most 40")
            .unwrap()
            .unwrap();
        assert!(labelled.only_labelled);
        assert_eq!(labelled.most, Some(40));
        assert_eq!(labelled.pace, Duration::ZERO);
        assert_eq!(of("--help").unwrap(), None);
        assert_eq!(
            parse(
                "--again /r --labels /l"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Again {
                lines: "/r".into(),
                labels: "/l".into(),
                refit: false,
                cost: 1.0
            })
        );
        assert_eq!(
            parse(
                "--again /r --refit --labels /l"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Again {
                lines: "/r".into(),
                labels: "/l".into(),
                refit: true,
                cost: 1.0
            })
        );
        assert_eq!(
            parse(
                "--again /r --refit --cost-ratio 3 --labels /l"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Again {
                lines: "/r".into(),
                labels: "/l".into(),
                refit: true,
                cost: 3.0
            })
        );
        assert_eq!(
            parse(
                "--pictures --log-dir /d --days 2026-10-15"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Pictures {
                log: "/d".into(),
                days: vec!["2026-10-15".into()],
                labels: None
            })
        );
        assert_eq!(
            parse(
                "--agreement --log-dir /d --days 2026-10-17,2026-10-18"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Agreement {
                log: "/d".into(),
                days: vec!["2026-10-17".into(), "2026-10-18".into()],
                from: None,
                until: None,
            })
        );
        assert_eq!(
            parse(
                "--agreement --log-dir /d --days 2026-10-18,2026-10-19 --from 2026-10-18T11:00:00Z --until 2026-10-19T00:00:00Z"
                    .split_whitespace()
                    .map(str::to_owned)
            )
            .unwrap(),
            Some(Run::Agreement {
                log: "/d".into(),
                days: vec!["2026-10-18".into(), "2026-10-19".into()],
                from: Some(1_792_321_200_000),
                until: Some(1_792_368_000_000),
            })
        );
        for refused in [
            "--agreement --log-dir /d",
            "--pictures --log-dir /d --days 2026-10-15 --from 2026-10-18T11:00:00Z",
            "--again /r --labels /l --until 2026-10-18T11:00:00Z",
            "--agreement --log-dir /d --days 2026-10-18 --from 2026-10-18T11:00:00",
            "--agreement --log-dir /d --days 2026-10-18 --from 2026-10-18 --until 2026-10-19T00:00:00Z",
            "--agreement --log-dir /d --days 2026-10-18 --from 2026-10-19T00:00:00Z --until 2026-10-18T11:00:00Z",
            "--agreement --log-dir /d --days 2026-10-18 --from 2026-10-18T11:00:00Z --from 2026-10-18T12:00:00Z",
            "--agreement --log-dir /d --days 2026-10-15 --models m",
            "--agreement --pictures --log-dir /d --days 2026-10-15",
            "--again /r",
            "--again /r --labels /l --days 2026-10-15",
            "--pictures --log-dir /d --days 2026-10-15 --models m",
            "--pictures --days 2026-10-15",
            "--pictures --log-dir /d --days 2026-10-15 --refit",
            "--again /r --labels /l --refit --refit",
            "--again /r --labels /l --cost-ratio 3",
            "--again /r --labels /l --refit --cost-ratio 0.5",
            "--again /r --labels /l --refit --cost-ratio 21",
            "--again /r --labels /l --refit --cost-ratio three",
            "--key-file /k --log-dir /d --days 2026-10-15 --models m --refit",
        ] {
            assert!(
                parse(refused.split_whitespace().map(str::to_owned)).is_err(),
                "{refused}"
            );
        }
        for refused in [
            "--log-dir /d --days 2026-10-15 --models m",
            "--key-file k --log-dir /d --days 2026-10-15 --models m",
            "--key-file /k --log-dir /d --days 15-10-2026 --models m",
            "--key-file /k --log-dir /d --days 2026-10-15 --models a,b,c,d,e",
            "--key-file /k --log-dir /d --days 2026-10-15 --models a,,b",
            "--key-file /k --log-dir /d --days 2026-10-15 --models m --only-labelled",
            "--key-file /k --log-dir /d --days 2026-10-15 --models m --deadline-ms 0",
            "--key-file /k --log-dir /d --days 2026-10-15 --models m --most 1 --most 2",
            "--key-file /k --log-dir /d --days 2026-10-15 --models m --other x",
        ] {
            assert!(of(refused).is_err(), "{refused}");
        }
    }
}
