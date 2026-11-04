//! `lamp-bus`: the bus from a shell. See `docs/bus.md`.
//!
//! Everything here goes through the library the lamp's processes use, so a
//! `tap` is an ordinary subscriber and can be as slow as it likes without a
//! publisher noticing.
use lamp_bus::logging::{Log, LogLevel, keep::Kept};
use lamp_bus::{Bus, Declaration, Header, Kind, Message, Notice, Publisher, Subscriber};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    str::FromStr,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "\
usage: lamp-bus [--root PATH] COMMAND ...

  topics
      The topics under the root, how many listen, whether a value is kept
      and the type each was declared to carry.
  info TOPIC
      What the topic was declared to carry, who publishes it now, who
      listens, and how old its kept value is. Status 1 if nobody has
      published, subscribed to or declared it.
  drops [TOPIC]
      Each subscriber of each topic, or of TOPIC, with how many messages the
      live publishers could not give it (its queue was full) since each
      found it, as their entries said at their last refresh. Status 1 if TOPIC
      is not on the bus.
  hz TOPIC [--seconds S]
      Listen for S seconds (5) and print how many messages came, how often,
      the mean and the largest gap between two, their mean age on arrival,
      and how many were missed or had expired. Status 1 if none came.
  tap TOPIC [--name NAME] [--count N] [--seconds S]
      Subscribe and print each message: its header, then its payload if that
      is JSON, else <N bytes>. A value the topic keeps is printed too, once
      nothing else is waiting. Ends after N messages or S seconds, and with
      status 1 if N was asked for and fewer came.
  logs [--part NAME] [--level LEVEL] [--count N] [--seconds S]
      Follow validated operational logs, optionally one part, at LEVEL or
      higher (info). Levels: debug, info, warn, error, fatal. Count only
      matching logs. No history or persistent storage is implied.
  keep-logs DIR [--level LEVEL] [--file-kb N] [--files N] [--seconds S]
      Keep validated logs at LEVEL or higher (info) on disk, one JSON line
      each with its bus identity, in DIR/logs.NNNNNNNN.jsonl: a new file at
      each start, files of at most N KiB (2048, 64 to 65536), only the
      newest N files (10, 2 to 100). DIR must exist; nothing else in it is
      touched. Lines that could not be written are counted in the next.
      Synced at most once a second. Until killed, or for S seconds.
  latest TOPIC
      Print the topic's current value, expired or not. Status 1 if it keeps
      none.
  pub TOPIC latest|event JSON [--at-us N] [--type NAME]
      Publish one JSON message as lamp_bus_cli. With --type, as a publisher
      that declares the topic to carry NAME: status 1 if the topic is
      declared as another type or kind.
  record DIR TOPIC... [--seconds S] [--max-mb N]
      Append every message of the topics to DIR/<topic with / as _>.jsonl
      until killed or for S seconds, a topic's declaration before its
      messages. A file stops growing at N MiB (64).
  replay FILE [--speed X]
      Publish a recording again as replay, X times as fast (0: at once),
      each topic declared as the recording says it was.

The root is --root, else $LAMP_BUS_ROOT, else /run/lampos-bus.
";

/// Every option takes one value.
const OPTIONS: [&str; 12] = [
    "root", "name", "count", "seconds", "max-mb", "speed", "at-us", "type", "part", "level",
    "file-kb", "files",
];

/// A wait is cut into pieces this long: an empty one is when a subscriber
/// looks at the topic's kept value.
const TICK: Duration = Duration::from_millis(200);

enum Failure {
    /// The command line was wrong: status 2, with the usage.
    Usage(String),
    /// It was right and did not work: status 1.
    Failed(String),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        // What the library refuses as input is a name or a size the caller
        // typed.
        if error.kind() == io::ErrorKind::InvalidInput {
            Self::Usage(error.to_string())
        } else {
            Self::Failed(error.to_string())
        }
    }
}

struct Arguments {
    words: Vec<String>,
    options: Vec<(String, String)>,
    help: bool,
}

impl Arguments {
    fn read(given: impl Iterator<Item = String>) -> Result<Self, Failure> {
        let mut arguments = Self {
            words: Vec::new(),
            options: Vec::new(),
            help: false,
        };
        let mut given = given;
        while let Some(word) = given.next() {
            if matches!(word.as_str(), "--help" | "-h") {
                arguments.help = true;
            } else if let Some(option) = word.strip_prefix("--") {
                if !OPTIONS.contains(&option) {
                    return Err(Failure::Usage(format!("unknown option {word}")));
                }
                if arguments.options.iter().any(|(name, _)| name == option) {
                    return Err(Failure::Usage(format!("{word} is given twice")));
                }
                let value = given
                    .next()
                    .ok_or_else(|| Failure::Usage(format!("{word} needs a value")))?;
                arguments.options.push((option.into(), value));
            } else {
                arguments.words.push(word);
            }
        }
        Ok(arguments)
    }

    /// Take an option out, read as a `T`.
    fn option<T: FromStr>(&mut self, name: &str, what: &str) -> Result<Option<T>, Failure> {
        let Some(at) = self.options.iter().position(|(option, _)| option == name) else {
            return Ok(None);
        };
        let (_, value) = self.options.remove(at);
        match value.parse() {
            Ok(value) => Ok(Some(value)),
            Err(_) => Err(Failure::Usage(format!("--{name} {value:?} is not {what}"))),
        }
    }

    /// A number of seconds, or of anything else that cannot be negative.
    fn amount(&mut self, name: &str) -> Result<Option<f64>, Failure> {
        match self.option::<f64>(name, "a number")? {
            Some(amount) if !(0.0..=1e9).contains(&amount) => Err(Failure::Usage(format!(
                "--{name} {amount} is not between 0 and 1e9"
            ))),
            amount => Ok(amount),
        }
    }

    /// The words after the command, once every option it knows has been
    /// taken out: what is left over does not belong to it.
    fn words(self, least: usize, most: usize) -> Result<Vec<String>, Failure> {
        if let Some((option, _)) = self.options.first() {
            return Err(Failure::Usage(format!(
                "--{option} does not go with {}",
                self.words[0]
            )));
        }
        let given = self.words.len() - 1;
        if given < least || given > most {
            return Err(Failure::Usage(format!(
                "{} takes {} and was given {given}",
                self.words[0],
                match (least, most) {
                    (0, 0) => "no arguments".to_string(),
                    _ if least == most => format!("{least} arguments"),
                    _ => format!("{least} arguments or more"),
                }
            )));
        }
        Ok(self.words.into_iter().skip(1).collect())
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(status) => status,
        Err(Failure::Usage(why)) => {
            eprintln!("lamp-bus: {why}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Failed(why)) => {
            eprintln!("lamp-bus: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run(given: impl Iterator<Item = String>) -> Result<ExitCode, Failure> {
    let mut arguments = Arguments::read(given)?;
    if arguments.help {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let bus = match arguments.option::<PathBuf>("root", "a path")? {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let Some(command) = arguments.words.first().cloned() else {
        return Err(Failure::Usage("no command".into()));
    };
    match command.as_str() {
        "topics" => {
            arguments.words(0, 0)?;
            topics(&bus)
        }
        "info" => {
            let words = arguments.words(1, 1)?;
            info(&bus, &words[0])
        }
        "drops" => {
            let words = arguments.words(0, 1)?;
            drops(&bus, words.first().map(String::as_str))
        }
        "hz" => {
            let seconds = arguments.amount("seconds")?.unwrap_or(5.0);
            let words = arguments.words(1, 1)?;
            hz(&bus, &words[0], Duration::from_secs_f64(seconds))
        }
        "tap" => {
            let name = arguments.option::<String>("name", "a name")?;
            let count = arguments.option::<u64>("count", "a whole number")?;
            let seconds = arguments.amount("seconds")?;
            let words = arguments.words(1, 1)?;
            let subscriber = bus.subscribe(&words[0], name.as_deref().unwrap_or("tap"))?;
            tap(subscriber, count, seconds.map(Duration::from_secs_f64))
        }
        "logs" => {
            let part = arguments.option::<String>("part", "a part name")?;
            let level = arguments
                .option::<LogLevel>("level", "debug, info, warn, error or fatal")?
                .unwrap_or(LogLevel::Info);
            let count = arguments.option::<u64>("count", "a whole number")?;
            let seconds = arguments.amount("seconds")?;
            arguments.words(0, 0)?;
            if let Some(part) = &part {
                Log::check_part(part)?;
            }
            let subscriber = bus.subscribe_typed(Log::TOPIC, "logs", Log::TYPE)?;
            logs(
                subscriber,
                part.as_deref(),
                level,
                count,
                seconds.map(Duration::from_secs_f64),
            )
        }
        "keep-logs" => {
            let level = arguments
                .option::<LogLevel>("level", "debug, info, warn, error or fatal")?
                .unwrap_or(LogLevel::Info);
            let file_kb = arguments
                .option::<u64>("file-kb", "a whole number")?
                .unwrap_or(2_048);
            let files = arguments
                .option::<usize>("files", "a whole number")?
                .unwrap_or(10);
            let seconds = arguments.amount("seconds")?;
            let words = arguments.words(1, 1)?;
            if !(64..=65_536).contains(&file_kb) {
                return Err(Failure::Usage("--file-kb is from 64 to 65536".into()));
            }
            if !(2..=100).contains(&files) {
                return Err(Failure::Usage("--files is from 2 to 100".into()));
            }
            let kept = Kept::open(Path::new(&words[0]), file_kb * 1_024, files)?;
            let subscriber = bus.subscribe_typed(Log::TOPIC, "keep_logs", Log::TYPE)?;
            keep_logs(
                subscriber,
                kept,
                level,
                seconds.map(Duration::from_secs_f64),
            )
        }
        "latest" => {
            let words = arguments.words(1, 1)?;
            match bus.latest(&words[0])? {
                Some(message) => {
                    say(&line(&message));
                    if message.expired {
                        eprintln!(
                            "lamp-bus: {}: the value expired {} ms ago",
                            words[0],
                            age_ms(&message.header)
                                .saturating_sub(message.header.expires_ms.unwrap_or(0))
                        );
                    }
                    Ok(ExitCode::SUCCESS)
                }
                None => Err(Failure::Failed(format!("{} keeps no value", words[0]))),
            }
        }
        "pub" => {
            let at_us = arguments.option::<u64>("at-us", "a whole number")?;
            let type_name = arguments.option::<String>("type", "a type's name")?;
            let words = arguments.words(3, 3)?;
            let kind = match words[1].as_str() {
                "latest" => Kind::Latest,
                "event" => Kind::Event,
                other => {
                    return Err(Failure::Usage(format!(
                        "a topic's kind is latest or event, not {other}"
                    )));
                }
            };
            let value: serde_json::Value = serde_json::from_str(&words[2])
                .map_err(|error| Failure::Usage(format!("{:?} is not JSON: {error}", words[2])))?;
            let mut publisher = match type_name {
                None => bus.publisher(&words[0], kind, "lamp_bus_cli")?,
                Some(type_name) => {
                    // A topic that already carries this type keeps what its
                    // own publisher promised of it: this one only joins.
                    let declaration = match bus.declaration(&words[0]) {
                        Ok(Some(there)) if there.type_name == type_name && there.kind == kind => {
                            there
                        }
                        _ => Declaration::new(&type_name, 1, kind),
                    };
                    bus.publisher_declared(&words[0], &declaration, "lamp_bus_cli")?
                }
            };
            // This process has no clock it shares with anyone: 0 says so.
            let outcome = publisher.publish_json(at_us.unwrap_or(0), &value)?;
            say(&format!(
                "delivered {} dropped {} removed {}",
                outcome.delivered, outcome.dropped, outcome.removed
            ));
            Ok(ExitCode::SUCCESS)
        }
        "record" => {
            let seconds = arguments.amount("seconds")?;
            let megabytes = arguments.amount("max-mb")?.unwrap_or(64.0);
            let words = arguments.words(2, usize::MAX)?;
            record(
                &bus,
                Path::new(&words[0]),
                &words[1..],
                seconds.map(Duration::from_secs_f64),
                megabytes,
            )
        }
        "replay" => {
            let speed = arguments.amount("speed")?.unwrap_or(1.0);
            let words = arguments.words(1, 1)?;
            replay(&bus, Path::new(&words[0]), speed)
        }
        other => Err(Failure::Usage(format!("unknown command {other}"))),
    }
}

/// One line on standard output, at once. False when nobody reads it any
/// more, which is how a `tap` piped into `head` ends.
fn say(line: &str) -> bool {
    let mut out = io::stdout().lock();
    writeln!(out, "{line}").and_then(|()| out.flush()).is_ok()
}

/// A message on one line: what it says of itself, then what it carries.
fn line(message: &Message) -> String {
    let header = serde_json::to_string(&message.header).unwrap_or_default();
    match message.json() {
        // Printed from the value, so that a payload with line ends in it
        // is still one line.
        Ok(value) => format!("{header} {value}"),
        Err(_) => format!("{header} <{} bytes>", message.payload.len()),
    }
}

/// How long to wait in one go, or `None` once the time is up.
fn tick(until: Option<Instant>) -> Option<Duration> {
    match until {
        None => Some(TICK),
        Some(until) => {
            let left = until.saturating_duration_since(Instant::now());
            (!left.is_zero()).then(|| left.min(TICK))
        }
    }
}

/// How long ago a message was sent, by this process's wall clock.
fn age_ms(header: &Header) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    now.saturating_sub(header.at_unix_ms)
}

/// Say on standard error what has changed about a subscriber's topic.
fn announce(subscriber: &mut Subscriber) {
    for notice in subscriber.notices() {
        let what = match notice {
            Notice::SourceGone => "no publisher is alive and nothing has come for a lease",
            Notice::SourceBack => "a publisher is there again",
            Notice::Late => "nothing for more than twice the declared period",
            Notice::OnTime => "messages are coming again",
            _ => continue,
        };
        eprintln!("lamp-bus: {}: {what}", subscriber.topic());
    }
}

fn topics(bus: &Bus) -> Result<ExitCode, Failure> {
    for topic in bus.topics()? {
        let stale = match topic.stale {
            0 => String::new(),
            stale => format!(" stale={stale}"),
        };
        let declared = match bus.declaration(&topic.name) {
            Ok(Some(declaration)) => format!(" type={}", declaration.type_name),
            _ => String::new(),
        };
        say(&format!(
            "{} subscribers={}{stale} latest={}{declared}",
            topic.name,
            topic.subscribers,
            if topic.latest { "yes" } else { "no" }
        ));
    }
    Ok(ExitCode::SUCCESS)
}

/// A declaration as `key=value` words.
fn declared(declaration: &Declaration) -> String {
    let mut words = format!(
        "type={} version={} kind={}",
        declaration.type_name,
        declaration.version,
        match declaration.kind {
            Kind::Latest => "latest",
            Kind::Event => "event",
        }
    );
    if let Some(period) = declaration.period_ms {
        words.push_str(&format!(" period_ms={period}"));
    }
    if let Some(lifespan) = declaration.lifespan_ms {
        words.push_str(&format!(" lifespan_ms={lifespan}"));
    }
    words
}

fn info(bus: &Bus, topic: &str) -> Result<ExitCode, Failure> {
    let declaration = bus.declaration(topic);
    let publishers = bus.publishers(topic)?;
    let subscribers = bus.subscribers(topic)?;
    let latest = bus.latest(topic);
    if !bus.root().join(topic).is_dir() {
        return Err(Failure::Failed(format!(
            "{topic} is not on the bus: nobody has published, subscribed to or declared it"
        )));
    }
    say(&format!("topic {topic}"));
    say(&match declaration {
        Ok(Some(declaration)) => format!("declaration {}", declared(&declaration)),
        Ok(None) => "declaration none".into(),
        Err(error) => format!("declaration unreadable: {error}"),
    });
    if publishers.is_empty() {
        say("publishers none");
    }
    for publisher in publishers {
        say(&format!(
            "publisher {} pid={} boot={} alive_ms={}",
            publisher.name,
            publisher.pid,
            publisher.boot,
            publisher.age.as_millis()
        ));
    }
    if subscribers.is_empty() {
        say("subscribers none");
    }
    for subscriber in subscribers {
        say(&format!(
            "subscriber {} {}",
            subscriber.socket,
            if subscriber.listening {
                "listening"
            } else {
                "stale"
            }
        ));
    }
    say(&match latest {
        Ok(Some(message)) => format!(
            "latest source={} seq={} age_ms={} expired={}",
            message.header.source,
            message.header.seq,
            age_ms(&message.header),
            if message.expired { "yes" } else { "no" }
        ),
        Ok(None) => "latest none".into(),
        Err(error) => format!("latest unreadable: {error}"),
    });
    Ok(ExitCode::SUCCESS)
}

/// One line a subscriber: `<topic> <subscriber> dropped=<n>`, the sum over
/// the topic's live publishers. A subscriber that has gone is listed while
/// a publisher's entry still names it.
fn drops(bus: &Bus, topic: Option<&str>) -> Result<ExitCode, Failure> {
    let topics = match topic {
        Some(topic) => {
            // A name the bus refuses is the caller's to fix: status 2.
            bus.subscribers(topic)?;
            if !bus.root().join(topic).is_dir() {
                return Err(Failure::Failed(format!(
                    "{topic} is not on the bus: nobody has published, subscribed to or declared it"
                )));
            }
            vec![topic.to_string()]
        }
        None => bus.topics()?.into_iter().map(|topic| topic.name).collect(),
    };
    for topic in topics {
        let mut totals: BTreeMap<String, u64> = bus
            .subscribers(&topic)?
            .into_iter()
            .map(|subscriber| (subscriber.socket, 0))
            .collect();
        for dropped in bus.drops(&topic)? {
            *totals.entry(dropped.subscriber).or_default() += dropped.dropped;
        }
        for (subscriber, dropped) in totals {
            say(&format!("{topic} {subscriber} dropped={dropped}"));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn hz(bus: &Bus, topic: &str, lasting: Duration) -> Result<ExitCode, Failure> {
    let mut subscriber = bus.subscribe(topic, "hz")?;
    // The value the topic keeps was sent before this listened: taken here,
    // it is not counted as a message that came.
    let _ = subscriber.latest();
    let started = Instant::now();
    let until = Some(started + lasting);
    while let Some(wait) = tick(until) {
        subscriber.recv(wait)?;
        announce(&mut subscriber);
    }
    let seconds = started.elapsed().as_secs_f64();
    let statistics = subscriber.statistics();
    let milliseconds = |time: Option<Duration>| match time {
        Some(time) => format!("{:.1}", time.as_secs_f64() * 1e3),
        None => "-".into(),
    };
    // A rate is one over the time between two messages, so it takes two.
    let rate = match statistics.mean_interval {
        Some(mean) if !mean.is_zero() => format!("{:.2}", 1.0 / mean.as_secs_f64()),
        _ => "-".into(),
    };
    say(&format!(
        "{topic} messages={} seconds={seconds:.2} rate_hz={rate} gap_mean_ms={} gap_max_ms={} \
         age_mean_ms={} missed={} expired={}",
        statistics.received,
        milliseconds(statistics.mean_interval),
        milliseconds(statistics.largest_interval),
        milliseconds(statistics.mean_age),
        statistics.gaps,
        statistics.expired
    ));
    Ok(if statistics.received == 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn tap(
    mut subscriber: Subscriber,
    count: Option<u64>,
    lasting: Option<Duration>,
) -> Result<ExitCode, Failure> {
    let until = lasting.map(|lasting| Instant::now() + lasting);
    let mut printed = 0;
    while count.is_none_or(|count| printed < count) {
        let Some(wait) = tick(until) else {
            break;
        };
        let received = subscriber.recv(wait)?;
        announce(&mut subscriber);
        let Some(message) = received else {
            continue;
        };
        if message.publisher_restarted {
            eprintln!("lamp-bus: {} began again", message.header.source);
        }
        if message.gap > 0 {
            eprintln!(
                "lamp-bus: missed {} from {} before seq {}",
                message.gap, message.header.source, message.header.seq
            );
        }
        if !say(&line(&message)) {
            break;
        }
        printed += 1;
    }
    Ok(if count.is_some_and(|count| printed < count) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn logs(
    mut subscriber: Subscriber,
    part: Option<&str>,
    level: LogLevel,
    count: Option<u64>,
    lasting: Option<Duration>,
) -> Result<ExitCode, Failure> {
    let until = lasting.map(|lasting| Instant::now() + lasting);
    let mut printed = 0;
    let mut invalid = 0;
    while count.is_none_or(|count| printed < count) {
        let Some(wait) = tick(until) else {
            break;
        };
        let Some(message) = subscriber.recv(wait)? else {
            continue;
        };
        let log = match Log::from_message(&message) {
            Ok(log) => log,
            Err(_) => {
                invalid += 1;
                continue;
            }
        };
        if part.is_some_and(|part| log.part != part) || log.level < level {
            continue;
        }
        // Debug-escaped text stays on one terminal line, including ESC and C1
        // controls. The part/code/header identity has already been validated.
        if !say(&format!(
            "{} {} {} [{}] {:?} attempt={} boot={} bus_seq={} bus_gap={} restarted={}",
            log.created_unix_ms,
            log.level.as_str(),
            log.part,
            log.code,
            log.message,
            log.attempt,
            message.header.boot,
            message.header.seq,
            message.gap,
            message.publisher_restarted,
        )) {
            break;
        }
        printed += 1;
    }
    let rejected = invalid + subscriber.malformed() + subscriber.wrong_type();
    if rejected > 0 {
        eprintln!("lamp-bus: logs: passed over {rejected} malformed or non-log messages");
    }
    Ok(if count.is_some_and(|count| printed < count) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Keep the logs on disk until killed or for `lasting`. A write that fails
/// is said once until one works again; what it lost is counted in the files.
fn keep_logs(
    mut subscriber: Subscriber,
    mut kept: Kept,
    level: LogLevel,
    lasting: Option<Duration>,
) -> Result<ExitCode, Failure> {
    let until = lasting.map(|lasting| Instant::now() + lasting);
    eprintln!(
        "lamp-bus: keep-logs: keeping {} and above in {}",
        level.as_str(),
        kept.path().display()
    );
    let mut invalid = 0;
    let mut failing = false;
    let said = |result: io::Result<()>, failing: &mut bool| match result {
        Ok(()) if *failing => {
            eprintln!("lamp-bus: keep-logs: writing again");
            *failing = false;
        }
        Err(error) if !*failing => {
            eprintln!("lamp-bus: keep-logs: cannot write, counting what is lost: {error}");
            *failing = true;
        }
        _ => {}
    };
    while let Some(wait) = tick(until) {
        if let Err(error) = kept.sync_if_due(Instant::now()) {
            said(Err(error), &mut failing);
        }
        let Some(message) = subscriber.recv(wait)? else {
            continue;
        };
        let Ok(log) = Log::from_message(&message) else {
            invalid += 1;
            continue;
        };
        if log.level < level {
            continue;
        }
        let result = kept.keep(&log, &message, Instant::now());
        said(result, &mut failing);
    }
    if let Err(error) = kept.sync() {
        eprintln!("lamp-bus: keep-logs: the last lines may not be on disk: {error}");
    }
    let rejected = invalid + subscriber.malformed() + subscriber.wrong_type();
    eprintln!(
        "lamp-bus: keep-logs: kept {}, could not write {}, passed over {rejected} malformed or non-log messages",
        kept.lines, kept.lost
    );
    Ok(ExitCode::SUCCESS)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 with padding, added to `text`.
fn base64(bytes: &[u8], text: &mut Vec<u8>) {
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .fold(0u32, |group, &byte| group << 8 | u32::from(byte))
            << (8 * (3 - chunk.len()));
        for place in 0..4 {
            text.push(if place <= chunk.len() {
                ALPHABET[(group >> (18 - 6 * place) & 63) as usize]
            } else {
                b'='
            });
        }
    }
}

/// The bytes of padded base64, or `None` if it is not that.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    for (index, chunk) in text.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|&&byte| byte == b'=').count();
        // Padding ends the text and never makes a whole group.
        if padding > 2 || (padding > 0 && (index + 1) * 4 != text.len()) {
            return None;
        }
        let mut group = 0u32;
        for &byte in &chunk[..4 - padding] {
            group = group << 6 | ALPHABET.iter().position(|&letter| letter == byte)? as u32;
        }
        group <<= 6 * padding;
        bytes.extend_from_slice(&group.to_be_bytes()[1..4 - padding]);
    }
    Some(bytes)
}

/// A topic's declaration as a line of a recording.
fn recorded_declaration(
    topic: &str,
    declaration: &Declaration,
    line: &mut Vec<u8>,
) -> io::Result<()> {
    line.clear();
    line.extend_from_slice(b"{\"topic\":");
    serde_json::to_writer(&mut *line, topic)?;
    line.extend_from_slice(b",\"declaration\":");
    serde_json::to_writer(&mut *line, declaration)?;
    line.extend_from_slice(b"}\n");
    Ok(())
}

/// One message as a line of a recording.
fn recorded(message: &Message, line: &mut Vec<u8>) -> io::Result<()> {
    line.clear();
    line.extend_from_slice(b"{\"header\":");
    serde_json::to_writer(&mut *line, &message.header)?;
    line.extend_from_slice(b",\"payload_b64\":\"");
    base64(&message.payload, line);
    line.extend_from_slice(b"\"}\n");
    Ok(())
}

/// One topic being recorded.
struct Recording {
    subscriber: Subscriber,
    path: PathBuf,
    file: fs::File,
    bytes: u64,
}

/// How a topic's recording ended.
struct Recorded {
    topic: String,
    path: PathBuf,
    messages: u64,
    missed: u64,
    full: bool,
    failed: Option<String>,
}

impl Recording {
    /// Append `line` unless the file would grow past `most` bytes: false
    /// when it would.
    fn append(&mut self, line: &[u8], most: u64) -> io::Result<bool> {
        if self.bytes + line.len() as u64 > most {
            return Ok(false);
        }
        self.file.write_all(line)?;
        self.bytes += line.len() as u64;
        Ok(true)
    }

    /// Write the topic's declaration if it is not the one last written: at
    /// the start, and again before the first message of a publisher that
    /// declared the topic after this began or declared it anew.
    fn declare(
        &mut self,
        written: &mut Option<Declaration>,
        line: &mut Vec<u8>,
        most: u64,
    ) -> io::Result<bool> {
        let Some(declaration) = self.subscriber.declaration().cloned() else {
            return Ok(true);
        };
        if written.as_ref() == Some(&declaration) {
            return Ok(true);
        }
        recorded_declaration(self.subscriber.topic(), &declaration, line)?;
        let fits = self.append(line, most)?;
        *written = Some(declaration);
        Ok(fits)
    }

    /// Append each message as it comes. A line goes to the file in one
    /// write and nothing is held back, so the file is whole lines whenever
    /// this process is killed.
    fn run(mut self, until: Option<Instant>, most: u64, megabytes: f64) -> Recorded {
        let mut ended = Recorded {
            topic: self.subscriber.topic().into(),
            path: self.path.clone(),
            messages: 0,
            missed: 0,
            full: false,
            failed: None,
        };
        let mut line = Vec::new();
        let mut declared = None;
        if let Err(error) = self.declare(&mut declared, &mut line, most) {
            ended.failed = Some(error.to_string());
            return ended;
        }
        while let Some(wait) = tick(until) {
            let message = match self.subscriber.recv(wait) {
                Ok(Some(message)) => message,
                Ok(None) => continue,
                Err(error) => {
                    ended.failed = Some(error.to_string());
                    break;
                }
            };
            ended.missed += message.gap;
            let written = self
                .declare(&mut declared, &mut line, most)
                .and_then(|fits| {
                    if !fits {
                        return Ok(false);
                    }
                    recorded(&message, &mut line)?;
                    self.append(&line, most)
                });
            match written {
                Ok(true) => ended.messages += 1,
                Ok(false) => {
                    eprintln!(
                        "lamp-bus: record: {} has reached {megabytes} MiB: no more of {} is recorded",
                        ended.path.display(),
                        ended.topic
                    );
                    ended.full = true;
                    break;
                }
                Err(error) => {
                    ended.failed = Some(error.to_string());
                    break;
                }
            }
        }
        ended
    }
}

fn record(
    bus: &Bus,
    directory: &Path,
    topics: &[String],
    lasting: Option<Duration>,
    megabytes: f64,
) -> Result<ExitCode, Failure> {
    let most = (megabytes * 1024.0 * 1024.0) as u64;
    let mut paths: Vec<PathBuf> = Vec::new();
    for topic in topics {
        let path = directory.join(format!("{}.jsonl", topic.replace('/', "_")));
        if paths.contains(&path) {
            return Err(Failure::Usage(format!(
                "{topic} would be recorded to {}, as another topic is",
                path.display()
            )));
        }
        paths.push(path);
    }
    // Every subscriber is bound and every file open before any of them
    // runs: a name that is wrong stops the whole recording, not a part.
    let mut subscribers = Vec::new();
    for topic in topics {
        subscribers.push(bus.subscribe(topic, "record")?);
    }
    fs::create_dir_all(directory)?;
    let mut recordings = Vec::new();
    for (subscriber, path) in subscribers.into_iter().zip(paths) {
        let file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        let bytes = file.metadata()?.len();
        recordings.push(Recording {
            subscriber,
            path,
            file,
            bytes,
        });
    }
    let until = lasting.map(|lasting| Instant::now() + lasting);
    let running: Vec<_> = recordings
        .into_iter()
        .map(|recording| thread::spawn(move || recording.run(until, most, megabytes)))
        .collect();
    let mut failed = false;
    for thread in running {
        let Ok(ended) = thread.join() else {
            failed = true;
            continue;
        };
        eprintln!(
            "lamp-bus: record: {} of {} in {}, {} missed{}",
            ended.messages,
            ended.topic,
            ended.path.display(),
            ended.missed,
            if ended.full { ", file full" } else { "" }
        );
        if let Some(why) = ended.failed {
            eprintln!("lamp-bus: record: {}: {why}", ended.topic);
            failed = true;
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// A line of a recording: a message, which is a header and a payload, or
/// what a topic was declared to carry.
#[derive(Deserialize)]
struct Line {
    header: Option<Header>,
    payload_b64: Option<String>,
    topic: Option<String>,
    declaration: Option<Declaration>,
}

fn replay(bus: &Bus, file: &Path, speed: f64) -> Result<ExitCode, Failure> {
    let lines = io::BufReader::new(fs::File::open(file)?).lines();
    let mut publishers: Vec<Publisher> = Vec::new();
    // When each recorded publisher instance was first replayed, and what
    // its clock said then. The spacing kept is the one within an instance:
    // two publishers need not have shared a clock.
    let mut instances: HashMap<String, (Instant, u64)> = HashMap::new();
    let (mut sent, mut refused) = (0u64, 0u64);
    // What the publishers that were replaced had delivered and dropped.
    let mut before = (0u64, 0u64);
    for (index, text) in lines.enumerate() {
        let wrong =
            |why: String| Failure::Failed(format!("{} line {}: {why}", file.display(), index + 1));
        let text = text?;
        if text.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(&text).map_err(|error| wrong(error.to_string()))?;
        if let (Some(topic), Some(declaration)) = (&line.topic, &line.declaration) {
            // The topic is published from here on as it was declared, by a
            // publisher that says so: one made before this line goes.
            publishers.retain(|publisher| {
                let other = publisher.topic() != topic;
                if !other {
                    before.0 += publisher.totals().delivered;
                    before.1 += publisher.totals().dropped;
                }
                other
            });
            let publisher = bus
                .publisher_declared(topic, declaration, "replay")
                .map_err(|error| wrong(error.to_string()))?;
            publishers.push(publisher);
            continue;
        }
        let (Some(header), Some(payload_b64)) = (line.header, line.payload_b64) else {
            return Err(wrong(
                "neither a message, with header and payload_b64, nor a topic's declaration".into(),
            ));
        };
        let payload =
            unbase64(&payload_b64).ok_or_else(|| wrong("payload_b64 is not base64".into()))?;
        if speed > 0.0 {
            let now = Instant::now();
            let &mut (began, first_us) = instances
                .entry(header.boot.clone())
                .or_insert((now, header.at_us));
            let after = header.at_us.saturating_sub(first_us) as f64 / 1e6 / speed;
            let due = Duration::try_from_secs_f64(after)
                .ok()
                .and_then(|after| began.checked_add(after))
                .ok_or_else(|| wrong(format!("due {after} seconds on")))?;
            thread::sleep(due.saturating_duration_since(now));
        }
        let at = match publishers
            .iter()
            .position(|publisher| publisher.topic() == header.topic)
        {
            Some(at) => at,
            None => {
                let publisher = bus
                    .publisher(&header.topic, header.kind, "replay")
                    .map_err(|error| wrong(error.to_string()))?;
                publishers.push(publisher);
                publishers.len() - 1
            }
        };
        let publisher = &mut publishers[at];
        publisher.expires_after(header.expires_ms.map(Duration::from_millis));
        match publisher.publish_bytes(header.at_us, &header.enc, &payload) {
            Ok(_) => sent += 1,
            Err(error) => {
                eprintln!("lamp-bus: replay: line {}: {error}", index + 1);
                refused += 1;
            }
        }
    }
    let totals = publishers
        .iter()
        .map(Publisher::totals)
        .fold(before, |(delivered, dropped), totals| {
            (delivered + totals.delivered, dropped + totals.dropped)
        });
    eprintln!(
        "lamp-bus: replay: {sent} messages, {} delivered, {} dropped",
        totals.0, totals.1
    );
    Ok(if refused > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_the_standard_alphabet_with_padding_and_reads_back() {
        let text = |bytes: &[u8]| {
            let mut text = Vec::new();
            base64(bytes, &mut text);
            String::from_utf8(text).unwrap()
        };
        // RFC 4648, section 10.
        for (bytes, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(text(bytes.as_bytes()), expected);
            assert_eq!(unbase64(expected).unwrap(), bytes.as_bytes());
        }
        assert_eq!(text(&[0xfb, 0xff, 0xfe]), "+//+");
        // Every byte value, at every length up to a few groups.
        let all: Vec<u8> = (0..=255).collect();
        for length in 0..all.len() {
            assert_eq!(unbase64(&text(&all[..length])).unwrap(), &all[..length]);
        }
        for wrong in [
            "Zg", "Zg=", "Z===", "====", "Zg==Zg==", "Zm9v\n", "Zm9-", "=Zm9",
        ] {
            assert!(unbase64(wrong).is_none(), "{wrong}");
        }
    }

    #[test]
    fn options_are_taken_out_and_what_is_left_over_is_refused() {
        let read = |words: &[&str]| Arguments::read(words.iter().map(|word| word.to_string()));
        let Ok(mut arguments) = read(&["tap", "--count", "3", "servos", "--root", "/x"]) else {
            panic!("refused");
        };
        assert!(matches!(arguments.option::<u64>("count", ""), Ok(Some(3))));
        assert!(matches!(arguments.option::<u64>("count", ""), Ok(None)));
        // `--root` has not been taken: it does not go with the command.
        assert!(matches!(arguments.words(1, 1), Err(Failure::Usage(_))));
        let Ok(arguments) = read(&["tap", "servos"]) else {
            panic!("refused");
        };
        assert!(matches!(arguments.words(1, 1), Ok(words) if words == ["servos"]));
        for wrong in [
            &["tap", "--count"][..],
            &["tap", "--colour", "red"],
            &["tap", "--count", "1", "--count", "2"],
        ] {
            assert!(matches!(read(wrong), Err(Failure::Usage(_))), "{wrong:?}");
        }
        let Ok(mut arguments) = read(&["tap", "--seconds", "-1", "--count", "x"]) else {
            panic!("refused");
        };
        assert!(matches!(
            arguments.amount("seconds"),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            arguments.option::<u64>("count", "a whole number"),
            Err(Failure::Usage(why)) if why.contains("--count \"x\" is not a whole number")
        ));
    }
}
