//! The policy as a process:
//! `lamp-policy --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR ...`.
//!
//! It runs until it is killed. The standard library cannot catch a signal
//! and this crate takes in nothing that can, so SIGTERM ends it where it
//! stands. A question that was out is then not decided. A line of the log
//! is written with one call, so it is there whole or not at all. Its
//! sockets on the bus are left for the voice's next message to remove, and
//! it stops being listed as a publisher when the bus's lease runs out.
//!
//! Everything this prints is about itself: where it listens, how many,
//! how long, which choice, which kind of failure. What was heard and what
//! the lamp said are never printed (`lamp_policy::policy::Note`).

use lamp_bus::Bus;
use lamp_policy::{
    decide::{DEFAULT_DEADLINE, DEFAULT_ENDPOINT, DEFAULT_MODEL, Endpoint, Jev},
    gather::PAUSE,
    hedge::Hedged,
    hello::{self, Hello},
    moment::{self, Moment},
    pictures::{self, Pictures},
    policy::{self, Beside, Options},
    progress::{self, LocalTime, Progress},
    record::private_dir,
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

const USAGE: &str = "usage: lamp-policy --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR [--endpoint URL] [--model NAME] [--deadline-ms N] [--hedge-after-ms N] [--bus-root ABSOLUTE_DIR] [--progress] [--hello] [--utc-offset-minutes N | --zone system|NAME] [--act progress] [--act hello] [--hello-by-rule] [--hello-quiet to-the-lamp] [--rules] [--act attention] [--attention-next] [--rule-decides KIND[;KIND...]] [--gate] [--addons] [--act addon] [--images MODEL [--sight-file ABSOLUTE_PATH]]";
const HELP: &str = "The lamp's policy, in shadow mode: judges whether each thing the voice hears was said to\n\
the lamp, publishes the decision on the bus (policy/decisions) and writes it down. Nothing\n\
acts on it. See docs/policy.md.\n\
  --key-file     a file whose one line is the key for the decision model's service; never printed\n\
  --log-dir      where the decisions log is kept, one file a day. It holds what was heard, in\n\
                 words, so it is a directory only its owner can enter: made with mode 0700 if it\n\
                 is not there, refused if its group or everyone can enter it\n\
  --endpoint     where the decision model is asked (default https://openrouter.ai/api/alpha/decisions);\n\
                 plain http only to this machine, for a test\n\
  --model        the model asked for (default typesafe/jev-1.13)\n\
  --deadline-ms  how long one asking may take, from 1 to 60000 (default 3000); after it a rule decides\n\
  --hedge-after-ms  a question not answered in this many ms (1 to 60000) is asked again on a second\n\
                 connection, and the first answer is taken; each second asking is in the journal. Off\n\
                 unless set\n\
  --bus-root     the bus's directory (default LAMP_BUS_ROOT, or /run/lampos-bus)\n\
  --progress     also judge, in shadow, whether progress of the work handed over (tasks/status)\n\
                 is worth telling now, with a second connection to the same model. Off unless set\n\
  --utc-offset-minutes  the lamp's local time east of UTC, for the hour of the day the progress\n\
                 and hello questions are shown (-720 to 840); without it the hour is not known\n\
  --zone         in place of the offset: system, the lamp's own time zone (/etc/localtime, as setup\n\
                 writes it), or a zone's name. The hour is taken at each moment, across summer time\n\
                 and a zone setup changes while this runs; a zone that cannot be read ends the start\n\
  --act progress the progress decisions act: published with shadow false, so that the voice\n\
                 tells a say_now. Needs --progress. \"Is this said to the lamp?\" stays shadow\n\
  --hello        also judge whether a hello is said now, with a connection of its own to the\n\
                 same model: a skill's (with --gate). Nothing publishes voice/arrival since the\n\
                 voice's own hello was removed (#2825). Off unless set\n\
  --act hello    the hello decisions act: say_now says the hello, wait keeps it. Needs --hello\n\
  --hello-by-rule  the hello is decided by the hard rules alone (talking, twenty minutes since the\n\
                 last hello, one an hour to each person, kept rules with --rules): a pass is say_now\n\
                 at once, with no wait for the model. The judge is still asked, in shadow: its answer\n\
                 is written on a hello_judge line and counted, never acted on (the first builder on\n\
                 #4625). Needs --hello. Off unless set\n\
  --hello-quiet to-the-lamp\n\
                 the hello's 15 s quiet counts only talk to the lamp or by it: a turn the voice held\n\
                 back as not said to it (voice/turn) is taken back from the quiet, and the model\n\
                 judges a hello at the lull. Never while the voice listens, thinks or speaks. Needs\n\
                 --hello. Off unless set: all talk counts\n\
  --act attention  \"is this said to the lamp?\" is published with shadow false, so that the voice\n\
                 holds the lamp's actions on a stay_quiet (LAMP_POLICY_HOLDS_ACTIONS=on); speech\n\
                 is not held. Off unless set\n\
  --rules        the progress and hello questions, and the moment question with --gate, obey what\n\
                 the people at the lamp asked about it speaking first (memory/rules): never, not in\n\
                 the mornings, not before or from a time of day (by --utc-offset-minutes; all day\n\
                 without it), not while they talk; no model is asked then. Needs --progress or\n\
                 --hello. Off unless set\n\
  --attention-next\n\
                 also judge each attention question a second time, in shadow (#1040): in a\n\
                 conversation with the lamp, 3 for each unit of follows_lamp above 0.55 added to the\n\
                 sum, never less than today's. Written on the decision's line as next and counted in\n\
                 the diagnostics (attention_next_differs); never published, never acted on. Off\n\
                 unless set\n\
  --rule-decides KIND[;KIND...]\n\
                 decide the attention questions of these kinds by the rule at once, without\n\
                 asking the model (#705): kinds as lamp-policy-replay --agreement prints them\n\
                 (room conversation speaking name, e.g. 'busy false false >=0.99'), named only\n\
                 once the record shows the model agreeing with the rule on them. The line says\n\
                 certain; those questions' model answers are no longer logged for fitting. Off\n\
                 unless set\n\
  --gate         also answer a skill that asks (skills/ask) whether the lamp may say hello, tell\n\
                 news or start something itself, with the hello question or a moment question\n\
                 of its own: the decision names the ask. Needs --hello and --act hello\n\
  --images       also ask MODEL (typesafe/jev-1.14 takes pictures) each attention question twice at once,\n\
                 without the eyes' newest frame and with it and six questions about it, and write both\n\
                 to pictures-YYYY-MM-DD.jsonl. Shadow; the decision never waits for it. A frame is sent\n\
                 only while the eyes keep one (the camera is permitted while the microphone is) and it\n\
                 is at most 1 s old; it is never written. Off unless set\n\
  --sight-file   the eyes' frame (default /run/lampos-eyes/sight.jpg). Needs --images\n\
What is heard is sent to the decision model's service. Do not run this on real speech in the\n\
office until the owner has said so: it is for played tests.";

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    key_file: PathBuf,
    log_dir: PathBuf,
    endpoint: String,
    model: String,
    deadline: Duration,
    bus: Option<PathBuf>,
    /// The progress question beside the attention one: off unless set.
    progress: bool,
    utc_offset_minutes: Option<i32>,
    /// `system` or a zone's name, in place of the offset (#820).
    zone: Option<String>,
    /// Its decisions act (`--act progress`): off unless set.
    act_progress: bool,
    /// The hello question (`--hello`), and whether it acts (`--act hello`):
    /// off unless set.
    hello: bool,
    act_hello: bool,
    /// The hard rules alone decide the hello, the judge only in shadow
    /// (`--hello-by-rule`, #4625): off unless set.
    hello_by_rule: bool,
    /// The hello's quiet counts only talk to the lamp or by it
    /// (`--hello-quiet to-the-lamp`, #1155): off unless set.
    hello_quiet_to_the_lamp: bool,
    /// The attention decisions act (`--act attention`, #800): off unless
    /// set.
    act_attention: bool,
    /// It obeys the people's rules about speaking first (`--rules`, #813):
    /// off unless set.
    rules: bool,
    /// Skills' asks are answered (`--gate`, #939): off unless set.
    gate: bool,
    /// Add-ons' says are answered (`--addons`, #2530 part 2), and act
    /// (`--act addon`): off unless set.
    addons: bool,
    act_addon: bool,
    /// The second attention judgement, in shadow (`--attention-next`,
    /// #1040): off unless set.
    attention_next: bool,
    /// The kinds of attention decision the rule makes alone (`--rule-decides`,
    /// #705): none unless set.
    rule_decides: Vec<lamp_policy::replay::Kind>,
    /// A question unanswered this long is asked again on a second
    /// connection (`--hedge-after-ms`, #1522): off unless set.
    hedge_after: Option<Duration>,
    /// The images model asked with the eyes' frame (`--images`, #871): off
    /// unless set.
    images: Option<String>,
    sight_file: Option<PathBuf>,
}

fn parse(arguments: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let (mut key_file, mut log_dir, mut endpoint, mut model, mut deadline, mut bus) =
        (None, None, None, None, None, None);
    let (mut progress, mut utc_offset_minutes) = (false, None);
    let (mut hello, mut act_progress, mut act_hello) = (false, false, false);
    let mut act_attention = false;
    let mut hello_quiet = None;
    let mut rules = false;
    let mut hello_by_rule = false;
    let mut zone = None;
    let mut gate = false;
    let (mut addons, mut act_addon) = (false, false);
    let mut attention_next = false;
    let mut rule_decides: Option<Vec<lamp_policy::replay::Kind>> = None;
    let mut hedge_after = None;
    let (mut images, mut sight_file) = (None, None);
    let mut arguments = arguments;
    while let Some(option) = arguments.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        // The options without a value.
        if option == "--progress"
            || option == "--hello"
            || option == "--rules"
            || option == "--gate"
            || option == "--addons"
            || option == "--attention-next"
            || option == "--hello-by-rule"
        {
            let flag = match option.as_str() {
                "--progress" => &mut progress,
                "--hello" => &mut hello,
                "--rules" => &mut rules,
                "--attention-next" => &mut attention_next,
                "--addons" => &mut addons,
                "--hello-by-rule" => &mut hello_by_rule,
                _ => &mut gate,
            };
            if std::mem::replace(flag, true) {
                return Err(format!("{option} was given twice"));
            }
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        let absolute = |value: &str| -> Result<PathBuf, String> {
            let path = PathBuf::from(value);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| format!("{option} must be an absolute path"))
        };
        let twice = match option.as_str() {
            "--key-file" => key_file.replace(absolute(&value)?).is_some(),
            "--log-dir" => log_dir.replace(absolute(&value)?).is_some(),
            "--bus-root" => bus.replace(absolute(&value)?).is_some(),
            "--sight-file" => sight_file.replace(absolute(&value)?).is_some(),
            "--images" => {
                if value.is_empty() || value.chars().any(char::is_control) || value.starts_with('-')
                {
                    return Err("--images is the model asked with the picture".into());
                }
                images.replace(value).is_some()
            }
            "--endpoint" => {
                Endpoint::parse(&value)?;
                endpoint.replace(value).is_some()
            }
            "--model" => {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err("--model is a model's name".into());
                }
                model.replace(value).is_some()
            }
            "--hello-quiet" => match value.as_str() {
                "to-the-lamp" => hello_quiet.replace(value).is_some(),
                _ => {
                    return Err(
                        "--hello-quiet takes to-the-lamp: the quiet counts only talk to the lamp or by it"
                            .into(),
                    );
                }
            },
            // The questions that may act: each named once.
            "--act" => match value.as_str() {
                "progress" => std::mem::replace(&mut act_progress, true),
                "hello" => std::mem::replace(&mut act_hello, true),
                "attention" => std::mem::replace(&mut act_attention, true),
                "addon" => std::mem::replace(&mut act_addon, true),
                _ => {
                    return Err(
                        "--act takes progress, hello, attention or addon, the questions that may act"
                            .into(),
                    );
                }
            },
            "--utc-offset-minutes" => {
                let minutes = value
                    .parse::<i32>()
                    .ok()
                    .filter(|minutes| (-720..=840).contains(minutes))
                    .ok_or("--utc-offset-minutes is a whole number from -720 to 840")?;
                utc_offset_minutes.replace(minutes).is_some()
            }
            "--zone" => {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err("--zone is system or a time zone's name".into());
                }
                zone.replace(value).is_some()
            }
            "--rule-decides" => {
                let kinds: Option<Vec<_>> = value
                    .split(';')
                    .map(lamp_policy::replay::Kind::parse)
                    .collect();
                let kinds = kinds.filter(|kinds| !kinds.is_empty()).ok_or(
                    "--rule-decides is kinds as lamp-policy-replay --agreement prints them, separated by ';' (busy false false >=0.99)",
                )?;
                // With the lamp speaking the rule always answers, and a
                // night's record can't show talk over the lamp's reply that
                // was not for it: those lines stay the model's (Sixth on
                // #1524, the 15 Oct "Thailand" case said over the lamp).
                if kinds.iter().any(|kind| kind.lamp_is_speaking) {
                    return Err(
                        "--rule-decides takes no kind with the lamp speaking: those lines are the model's to judge".into(),
                    );
                }
                rule_decides.replace(kinds).is_some()
            }
            "--deadline-ms" => {
                let milliseconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|milliseconds| (1..=60_000).contains(milliseconds))
                    .ok_or("--deadline-ms is a whole number from 1 to 60000")?;
                deadline
                    .replace(Duration::from_millis(milliseconds))
                    .is_some()
            }
            "--hedge-after-ms" => {
                let milliseconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|milliseconds| (1..=60_000).contains(milliseconds))
                    .ok_or("--hedge-after-ms is a whole number from 1 to 60000")?;
                hedge_after
                    .replace(Duration::from_millis(milliseconds))
                    .is_some()
            }
            other => return Err(format!("unknown option {other}")),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
    }
    if act_progress && !progress {
        return Err("--act progress needs --progress".into());
    }
    if rules && !progress && !hello {
        return Err("--rules needs --progress or --hello, a question that may speak first".into());
    }
    if act_hello && !hello {
        return Err("--act hello needs --hello".into());
    }
    if hello_by_rule && !hello {
        return Err("--hello-by-rule needs --hello".into());
    }
    if hello_quiet.is_some() && !hello {
        return Err("--hello-quiet needs --hello".into());
    }
    if zone.is_some() && utc_offset_minutes.is_some() {
        return Err("--zone and --utc-offset-minutes are one or the other".into());
    }
    // A skill that is told yes says the hello itself: the hello's decisions
    // must act, so that the lamp's own last hello and its count are kept.
    if gate && !act_hello {
        return Err("--gate needs --hello and --act hello".into());
    }
    // An add-on's say is put to the moment question, which runs with the
    // gate only (#2530 part 2).
    if addons && !gate {
        return Err("--addons needs --gate".into());
    }
    if act_addon && !addons {
        return Err("--act addon needs --addons".into());
    }
    if sight_file.is_some() && images.is_none() {
        return Err("--sight-file needs --images".into());
    }
    Ok(Some(Arguments {
        key_file: key_file.ok_or("--key-file is required")?,
        log_dir: log_dir.ok_or("--log-dir is required")?,
        endpoint: endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned()),
        model: model.unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
        deadline: deadline.unwrap_or(DEFAULT_DEADLINE),
        bus,
        progress,
        utc_offset_minutes,
        zone,
        act_progress,
        hello,
        act_hello,
        hello_by_rule,
        hello_quiet_to_the_lamp: hello_quiet.is_some(),
        act_attention,
        rules,
        gate,
        addons,
        act_addon,
        attention_next,
        rule_decides: rule_decides.unwrap_or_default(),
        hedge_after,
        images,
        sight_file,
    }))
}

/// One line to the journal. A journal that has gone away is no reason for
/// the policy to stop.
fn log(line: &str) {
    let _ = writeln!(io::stderr(), "lamp-policy: {line}");
}

/// What the start line says of the questions beside attention: as it was
/// with `--progress` alone, and with `--hello` after it.
fn questions(arguments: &Arguments) -> String {
    let mut said = String::new();
    if arguments.progress {
        said.push_str(" progress=true");
    }
    if arguments.hello {
        said.push_str(" hello=true");
    }
    if arguments.hello_by_rule {
        said.push_str(" hello_by_rule=true");
    }
    if arguments.hello_quiet_to_the_lamp {
        said.push_str(" hello_quiet=to-the-lamp");
    }
    if arguments.rules {
        said.push_str(" rules=true");
    }
    if arguments.gate {
        said.push_str(" gate=true");
    }
    if arguments.addons {
        said.push_str(" addons=true");
    }
    if arguments.attention_next {
        said.push_str(" attention_next=true");
    }
    if !arguments.rule_decides.is_empty() {
        said.push_str(&format!(" rule_decides={}", arguments.rule_decides.len()));
    }
    if let Some(after) = arguments.hedge_after {
        said.push_str(&format!(" hedge_after_ms={}", after.as_millis()));
    }
    let acting: Vec<&str> = [
        (arguments.act_progress, "progress"),
        (arguments.act_hello, "hello"),
        (arguments.act_attention, "attention"),
        (arguments.act_addon, "addon"),
    ]
    .into_iter()
    .filter_map(|(acts, name)| acts.then_some(name))
    .collect();
    if !acting.is_empty() {
        said.push_str(&format!(" act={}", acting.join(",")));
    }
    said
}

fn run(arguments: &Arguments) -> io::Result<()> {
    // The key first: without it no model can be asked, and that is to be
    // found out before anything is subscribed to.
    let jev = Jev::new(&arguments.endpoint, &arguments.model, &arguments.key_file)?;
    // Then the zone: one that cannot be read ends the start, never a silent
    // UTC (#820).
    let local = match (&arguments.zone, arguments.utc_offset_minutes) {
        (Some(zone), _) => Some(Arc::new(LocalTime::zone(zone)?)),
        (None, Some(minutes)) => Some(Arc::new(LocalTime::Offset(minutes))),
        (None, None) => None,
    };
    let bus = match &arguments.bus {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    log(&format!(
        "started endpoint={} model={} deadline_ms={} pause_ms={} log_dir={} bus_root={} shadow={}{}{}{}",
        arguments.endpoint,
        arguments.model,
        arguments.deadline.as_millis(),
        PAUSE.as_millis(),
        arguments.log_dir.display(),
        bus.root().display(),
        policy::SHADOW && !arguments.act_attention,
        questions(arguments),
        arguments
            .zone
            .as_ref()
            .map_or_else(String::new, |zone| format!(" zone={zone}")),
        arguments
            .images
            .as_ref()
            .map_or_else(String::new, |model| format!(" images={model}")),
    ));
    let options = Options {
        log_dir: arguments.log_dir.clone(),
        deadline: arguments.deadline,
        pause: PAUSE,
        act_attention: arguments.act_attention,
        report_every: policy::REPORT_EVERY,
    };
    // Nothing sets the flag: the process runs until it is killed.
    let stop = AtomicBool::new(false);
    // The picture's two askings, each on its own connection to the images
    // model, on a thread of their own (#871).
    let pictures = match &arguments.images {
        Some(model) => Some(Pictures::start(
            Jev::new(&arguments.endpoint, model, &arguments.key_file)?,
            Jev::new(&arguments.endpoint, model, &arguments.key_file)?,
            pictures::Options {
                sight_file: arguments
                    .sight_file
                    .clone()
                    .unwrap_or_else(|| pictures::SIGHT_FILE.into()),
                deadline: arguments.deadline.max(Duration::from_secs(10)),
                log_dir: arguments.log_dir.clone(),
                presence: pictures::Presence::default(),
            },
        )?),
        None => None,
    };
    // Each question that may speak first a judge of its own, on its own
    // connection: no question waits for another.
    let judge = || Jev::new(&arguments.endpoint, &arguments.model, &arguments.key_file);
    let progress = if arguments.progress {
        let options = progress::Options {
            act: arguments.act_progress,
            rules: arguments.rules,
            ..progress::Options::new(arguments.deadline, local.clone())
        };
        Some(Progress::start(judge()?, options)?)
    } else {
        None
    };
    let hello = if arguments.hello {
        let options = hello::Options {
            act: arguments.act_hello,
            gate: arguments.gate,
            addons: arguments.addons,
            rules: arguments.rules,
            quiet_to_the_lamp: arguments.hello_quiet_to_the_lamp,
            by_rule: arguments.hello_by_rule,
            ..hello::Options::new(arguments.deadline, local.clone())
        };
        let hello = Hello::start(judge()?, options)?;
        // Answering skills, the moment question too, with a judge of its
        // own (#939).
        Some(if arguments.gate {
            let options = moment::Options {
                act_addon: arguments.act_addon,
                ..moment::Options::new(arguments.deadline, local)
            };
            hello.with_moment(Moment::start(judge()?, options)?)
        } else {
            hello
        })
    } else {
        None
    };
    let beside = Beside {
        progress,
        hello,
        pictures,
        attention_next: arguments.attention_next,
        rule_decides: arguments.rule_decides.clone(),
        timing: policy::Timing::LAMP,
    };
    let say = &mut |note: policy::Note| log(&note.to_string());
    match arguments.hedge_after {
        // A second judge with its own connection, asked only when the
        // first is slow (#1522).
        Some(after) => {
            let second = Jev::new(&arguments.endpoint, &arguments.model, &arguments.key_file)?;
            let hedged = Hedged::new([jev, second], after, Arc::new(|line: &str| log(line)))?;
            policy::run_beside(&bus, hedged, &options, &stop, say, beside)
        }
        None => policy::run_beside(&bus, jev, &options, &stop, say, beside),
    }
    .map(|_| ())
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let arguments = match parse(std::env::args().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-policy: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Before anything else is made: a directory others can enter is the
    // caller's mistake, like a wrong option.
    if let Err(error) = private_dir(&arguments.log_dir) {
        eprintln!("lamp-policy: {error}");
        return ExitCode::from(2);
    }
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-policy: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Result<Option<Arguments>, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn the_options_are_read_and_the_rest_have_their_defaults() {
        let all = of("--key-file /etc/lampos/decisions.key --log-dir /var/lib/lampos/decisions --endpoint http://127.0.0.1:8080/decisions --model typesafe/jev-2 --deadline-ms 1500 --bus-root /run/lampos-bus")
            .unwrap()
            .unwrap();
        assert_eq!(
            all,
            Arguments {
                key_file: "/etc/lampos/decisions.key".into(),
                log_dir: "/var/lib/lampos/decisions".into(),
                endpoint: "http://127.0.0.1:8080/decisions".into(),
                model: "typesafe/jev-2".into(),
                deadline: Duration::from_millis(1_500),
                bus: Some("/run/lampos-bus".into()),
                progress: false,
                utc_offset_minutes: None,
                zone: None,
                act_progress: false,
                hello: false,
                act_hello: false,
                hello_by_rule: false,
                hello_quiet_to_the_lamp: false,
                act_attention: false,
                rules: false,
                gate: false,
                addons: false,
                act_addon: false,
                attention_next: false,
                rule_decides: Vec::new(),
                hedge_after: None,
                images: None,
                sight_file: None,
            }
        );
        let least = of("--log-dir /d --key-file /k").unwrap().unwrap();
        assert_eq!(
            least,
            Arguments {
                key_file: "/k".into(),
                log_dir: "/d".into(),
                endpoint: "https://openrouter.ai/api/alpha/decisions".into(),
                model: "typesafe/jev-1.13".into(),
                deadline: Duration::from_millis(3_000),
                bus: None,
                progress: false,
                utc_offset_minutes: None,
                zone: None,
                act_progress: false,
                hello: false,
                act_hello: false,
                hello_by_rule: false,
                hello_quiet_to_the_lamp: false,
                act_attention: false,
                rules: false,
                gate: false,
                addons: false,
                act_addon: false,
                attention_next: false,
                rule_decides: Vec::new(),
                hedge_after: None,
                images: None,
                sight_file: None,
            }
        );
        // The progress question, and the lamp's local time for it.
        let progress = of("--log-dir /d --progress --key-file /k --utc-offset-minutes 420")
            .unwrap()
            .unwrap();
        assert!(progress.progress && !progress.act_progress);
        assert_eq!(progress.utc_offset_minutes, Some(420));
        // Or its zone (#820).
        let zoned = of("--log-dir /d --progress --key-file /k --zone system")
            .unwrap()
            .unwrap();
        assert_eq!(
            (zoned.zone.as_deref(), zoned.utc_offset_minutes),
            (Some("system"), None)
        );
        // The picture's askings (#871), with the eyes' frame where it is
        // unless another is named.
        let pictured = of("--log-dir /d --key-file /k --images typesafe/jev-1.14")
            .unwrap()
            .unwrap();
        assert_eq!(
            (pictured.images.as_deref(), pictured.sight_file.as_deref()),
            (Some("typesafe/jev-1.14"), None)
        );
        let elsewhere = of("--log-dir /d --key-file /k --images m --sight-file /tmp/sight.jpg")
            .unwrap()
            .unwrap();
        assert_eq!(
            elsewhere.sight_file.as_deref(),
            Some(std::path::Path::new("/tmp/sight.jpg"))
        );
        // Beside the hello question (#883), as on lamp-4ace.
        let greeting = of("--log-dir /d --key-file /k --hello --act hello --images m")
            .unwrap()
            .unwrap();
        assert!(greeting.hello && greeting.act_hello && greeting.images.is_some());
        assert_eq!(questions(&greeting), " hello=true act=hello");
        assert!(!greeting.hello_quiet_to_the_lamp);
        // The hello's quiet counting only talk to the lamp (#1155).
        let quieter =
            of("--log-dir /d --key-file /k --hello --act hello --hello-quiet to-the-lamp")
                .unwrap()
                .unwrap();
        assert!(quieter.hello_quiet_to_the_lamp);
        assert_eq!(
            questions(&quieter),
            " hello=true hello_quiet=to-the-lamp act=hello"
        );
        // And acting on it.
        let acting = of("--log-dir /d --progress --act progress --key-file /k")
            .unwrap()
            .unwrap();
        assert!(acting.progress && acting.act_progress && !acting.act_attention);
        assert_eq!(questions(&acting), " progress=true act=progress");
        // "Is this said to the lamp?" acting (#800), on its own, and beside
        // progress acting.
        let attending = of("--log-dir /d --key-file /k --act attention")
            .unwrap()
            .unwrap();
        assert!(attending.act_attention && !attending.progress && !attending.act_progress);
        assert_eq!(questions(&attending), " act=attention");
        let both = of("--log-dir /d --key-file /k --progress --act attention --act progress")
            .unwrap()
            .unwrap();
        assert_eq!(questions(&both), " progress=true act=progress,attention");
        // And obeying the people's rules about speaking first (#813).
        let ruled = of("--log-dir /d --progress --rules --key-file /k")
            .unwrap()
            .unwrap();
        assert!(ruled.progress && ruled.rules && !ruled.act_progress);
        assert_eq!(questions(&ruled), " progress=true rules=true");
        // The second attention judgement, in shadow (#1040).
        let next = of("--log-dir /d --key-file /k --attention-next")
            .unwrap()
            .unwrap();
        assert!(next.attention_next && !next.act_attention);
        assert_eq!(questions(&next), " attention_next=true");
        // The hello question obeys them alone too, and with the gate the
        // moment question.
        let greeting = of("--log-dir /d --hello --rules --key-file /k")
            .unwrap()
            .unwrap();
        assert!(greeting.hello && greeting.rules && !greeting.progress);
        assert_eq!(questions(&greeting), " hello=true rules=true");
        let all = of("--log-dir /d --hello --act hello --gate --rules --key-file /k")
            .unwrap()
            .unwrap();
        assert_eq!(
            questions(&all),
            " hello=true rules=true gate=true act=hello"
        );
        // The hello question, and both acting.
        let both = of("--log-dir /d --hello --progress --act hello --act progress --key-file /k")
            .unwrap()
            .unwrap();
        assert!(both.hello && both.act_hello && both.progress && both.act_progress);
        assert_eq!(
            questions(&both),
            " progress=true hello=true act=progress,hello"
        );
        // Answering skills' asks (#939), with the hello acting.
        let gating = of("--log-dir /d --hello --act hello --gate --key-file /k")
            .unwrap()
            .unwrap();
        assert!(gating.gate && gating.hello && gating.act_hello);
        assert_eq!(questions(&gating), " hello=true gate=true act=hello");
        // The hello by the hard rules alone, the judge in shadow (#4625),
        // as the product units run it.
        let by_rule = of(
            "--log-dir /d --zone system --hello --act hello --gate --hello-by-rule --key-file /k",
        )
        .unwrap()
        .unwrap();
        assert!(by_rule.hello_by_rule && by_rule.hello && by_rule.act_hello && by_rule.gate);
        assert_eq!(
            questions(&by_rule),
            " hello=true hello_by_rule=true gate=true act=hello"
        );
        assert!(!gating.hello_by_rule);
        let only = of("--log-dir /d --key-file /k --progress")
            .unwrap()
            .unwrap();
        assert_eq!(questions(&only), " progress=true");
        assert_eq!(
            questions(&of("--log-dir /d --key-file /k").unwrap().unwrap()),
            ""
        );
        assert_eq!(
            of("--log-dir /d --key-file /k --utc-offset-minutes -300")
                .unwrap()
                .unwrap()
                .utc_offset_minutes,
            Some(-300)
        );
        assert_eq!(of("--help").unwrap(), None);
        assert_eq!(of("-h").unwrap(), None);
    }

    #[test]
    fn a_second_asking_is_set_from_1_to_60000_ms_and_said_at_the_start() {
        let with = |more: &[&str]| {
            let mut words: Vec<String> = ["--key-file", "/k", "--log-dir", "/d"]
                .map(str::to_owned)
                .into();
            words.extend(more.iter().map(|word| (*word).to_owned()));
            parse(words.into_iter())
        };
        let read = with(&["--hedge-after-ms", "550"]).unwrap().unwrap();
        assert_eq!(read.hedge_after, Some(Duration::from_millis(550)));
        assert_eq!(questions(&read), " hedge_after_ms=550");
        assert_eq!(with(&[]).unwrap().unwrap().hedge_after, None);
        for refused in [
            &["--hedge-after-ms", "0"][..],
            &["--hedge-after-ms", "60001"],
            &["--hedge-after-ms", "soon"],
            &["--hedge-after-ms", "550", "--hedge-after-ms", "550"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// `--rule-decides` (#705): kinds as `--agreement` prints them, said on
    /// the start line by count; refused when malformed, given twice, or with
    /// the lamp speaking (Sixth on #1524).
    #[test]
    fn the_rule_decides_kinds_are_read_and_a_speaking_kind_is_refused() {
        let with = |kinds: &[&str]| {
            let mut words: Vec<String> = ["--key-file", "/k", "--log-dir", "/d"]
                .map(str::to_owned)
                .into();
            for kind in kinds {
                words.push("--rule-decides".into());
                words.push((*kind).to_owned());
            }
            parse(words.into_iter())
        };
        let read = with(&["busy false false >=0.99;quiet false false none"])
            .unwrap()
            .unwrap();
        assert_eq!(read.rule_decides.len(), 2);
        assert_eq!(read.rule_decides[0].to_string(), "busy false false >=0.99");
        assert_eq!(questions(&read), " rule_decides=2");
        assert!(with(&[]).unwrap().unwrap().rule_decides.is_empty());
        assert_eq!(questions(&with(&[]).unwrap().unwrap()), "");
        for refused in [
            &["busy false true none"][..],
            &["busy false false >=0.99;busy true true none"],
            &["busy false false 0.99"],
            &["busy false false >=0.99;"],
            &[""],
            &["busy false false >=0.99", "busy false false >=0.99"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// #2530 part 2: an add-on's say is answered with the gate, and acts
    /// only with `--act addon`.
    #[test]
    fn add_ons_are_answered_with_the_gate_and_act_when_asked() {
        let gated = "--key-file /k --log-dir /d --hello --act hello --gate";
        let answered = of(&format!("{gated} --addons")).unwrap().unwrap();
        assert!(answered.addons && !answered.act_addon);
        let acting = of(&format!("{gated} --addons --act addon"))
            .unwrap()
            .unwrap();
        assert!(acting.addons && acting.act_addon);
        assert!(questions(&acting).contains(" addons=true"));
    }

    #[test]
    fn anything_else_is_refused_with_the_reason() {
        let paths = "--key-file /k --log-dir /d";
        assert!(of(paths).is_ok());
        for refused in [
            "",
            "--key-file /k",
            "--log-dir /d",
            "--key-file key --log-dir /d",
            "--key-file /k --log-dir decisions",
            "--key-file /k --log-dir /d --bus-root relative",
            "--key-file /k --log-dir /d --endpoint openrouter.ai/api",
            "--key-file /k --log-dir /d --endpoint ftp://openrouter.ai/api",
            "--key-file /k --log-dir /d --endpoint https://",
            "--key-file /k --log-dir /d --endpoint http://openrouter.ai/api/alpha/decisions",
            "--key-file /k --log-dir /d --endpoint https://host:0/",
            "--key-file /k --log-dir /d --deadline-ms 0",
            "--key-file /k --log-dir /d --deadline-ms 60001",
            "--key-file /k --log-dir /d --deadline-ms soon",
            "--key-file /k --log-dir /d --deadline-ms 1.5",
            "--key-file /k --log-dir /d --key-file /other",
            "--key-file /k --log-dir /d --deadline-ms 5 --deadline-ms 5",
            "--key-file /k --log-dir /d --shadow off",
            "--key-file /k --log-dir /d extra",
            "--key-file /k --log-dir",
            "--key-file /k --log-dir /d --progress --progress",
            "--key-file /k --log-dir /d --act progress",
            "--key-file /k --log-dir /d --act attention --act attention",
            "--key-file /k --log-dir /d --act hello",
            "--key-file /k --log-dir /d --rules",
            "--key-file /k --log-dir /d --progress --rules --rules",
            "--key-file /k --log-dir /d --progress --act hello",
            "--key-file /k --log-dir /d --hello --hello",
            "--key-file /k --log-dir /d --hello-quiet to-the-lamp",
            "--key-file /k --log-dir /d --hello --hello-quiet",
            "--key-file /k --log-dir /d --hello --hello-quiet all",
            "--key-file /k --log-dir /d --hello --hello-quiet to-the-lamp --hello-quiet to-the-lamp",
            "--key-file /k --log-dir /d --hello --act hello --act hello",
            "--key-file /k --log-dir /d --hello-by-rule",
            "--key-file /k --log-dir /d --act hello --hello-by-rule",
            "--key-file /k --log-dir /d --hello --hello-by-rule --hello-by-rule",
            "--key-file /k --log-dir /d --gate",
            "--key-file /k --log-dir /d --hello --gate",
            "--key-file /k --log-dir /d --hello --act hello --gate --gate",
            "--key-file /k --log-dir /d --addons",
            "--key-file /k --log-dir /d --hello --act hello --addons",
            "--key-file /k --log-dir /d --hello --act hello --gate --act addon",
            "--key-file /k --log-dir /d --hello --act hello --gate --addons --addons",
            "--key-file /k --log-dir /d --attention-next --attention-next",
            "--key-file /k --log-dir /d --progress --act progress --act progress",
            "--key-file /k --log-dir /d --progress --act",
            "--key-file /k --log-dir /d --utc-offset-minutes 841",
            "--key-file /k --log-dir /d --utc-offset-minutes -721",
            "--key-file /k --log-dir /d --utc-offset-minutes 7h",
            "--key-file /k --log-dir /d --utc-offset-minutes 1 --utc-offset-minutes 1",
            "--key-file /k --log-dir /d --zone",
            "--key-file /k --log-dir /d --zone system --zone system",
            "--key-file /k --log-dir /d --zone system --utc-offset-minutes 420",
            "--key-file /k --log-dir /d --utc-offset-minutes 0 --zone UTC",
            "--key-file /k --log-dir /d --sight-file /s",
            "--key-file /k --log-dir /d --images m --sight-file s",
            "--key-file /k --log-dir /d --images --progress",
            "--key-file /k --log-dir /d --images m --images m",
        ] {
            assert!(of(refused).is_err(), "{refused:?}");
        }
        assert_eq!(
            of("--key-file /k --log-dir /d --shadow off").unwrap_err(),
            "unknown option --shadow"
        );
        assert_eq!(
            of("--key-file /k --log-dir /d --hello-by-rule").unwrap_err(),
            "--hello-by-rule needs --hello"
        );
        assert!(of("--key-file /k --log-dir /d --deadline-ms 1").is_ok());
        assert!(of("--key-file /k --log-dir /d --deadline-ms 60000").is_ok());
    }
}
