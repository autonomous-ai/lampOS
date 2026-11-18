//! `lamp-voice probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS] --nightly`:
//! one nightly pass (`LAMP_MEMORY_NIGHTLY`, [`crate::nightly`]) for a test
//! person on a made-up day, through the relay, as the lamp's night would
//! run it, and the counts it ends with (the memory owner's extraction plan
//! C, step B, 3 Nov).
//!
//! The day is [`LINES`]: a person's own lines of yesterday, written as
//! their exchanges into a memory of the probe's own. With
//! `LAMP_NIGHTLY_MEMORY` naming a lamp-memory binary that has the nightly
//! ops, the probe starts it on a fresh folder of its own (never the lamp's
//! memory), writes the day, and runs the pass against it: Days' prepare,
//! the relay's model, Days' apply, and prints Days' accepted and dropped
//! counts per kind and its drop reasons. Without it, the day's input is
//! made here as the contract has it, and the probe prints what the model
//! proposed per kind (nothing applied). The patterns and feelings
//! switches are read from the environment, as the voice reads them.
//!
//! It prints counts only: never the character, a key, a URL or what the
//! model said.
use crate::nightly::{self, KINDS, Lamp, Memory, Night, Relay, Step, Switches};
use lamp_local_time::LocalTime;
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// The memory binary the pass is run against (`on`: a path).
pub const MEMORY_ENVIRONMENT: &str = "LAMP_NIGHTLY_MEMORY";
/// The made-up test person.
pub const PERSON: &str = "p-0000000000005e57";

/// A made-up day: what the person said, and the lamp's short answer. Some
/// lines are worth keeping (a move, a sister, a taste, a plan), some are
/// not (small talk, a question to the lamp), and one is health, which
/// must not be kept unasked.
pub const LINES: [(&str, &str); 12] = [
    ("Hey Lamp, good morning!", "Good morning!"),
    (
        "I'm moving flat on Saturday, to the one near the river.",
        "Exciting! Saturday it is.",
    ),
    (
        "My sister Lan is coming to help me pack.",
        "That's kind of her.",
    ),
    (
        "What's the weather like today?",
        "I can't see outside, sorry.",
    ),
    (
        "I always have peppermint tea in the evening.",
        "Sounds calming.",
    ),
    (
        "I used to drink jasmine, but not any more.",
        "Peppermint it is.",
    ),
    (
        "I've had a headache all afternoon.",
        "I'm sorry. Rest a bit.",
    ),
    (
        "Maybe I'll take a pottery class, I haven't booked anything.",
        "That could be fun.",
    ),
    (
        "I work night shifts at the bakery on weekends.",
        "Early bread!",
    ),
    ("Can you set a timer for ten minutes?", "Timer set."),
    ("I'm really excited about the new flat.", "I can hear it!"),
    ("Okay, goodnight Lamp.", "Goodnight."),
];

/// The lamp of the probe: quiet, the slide open, nobody at it (the test
/// person's day is noted directly).
struct Still;
impl Lamp for Still {
    fn stirs(&self) -> u64 {
        0
    }
    fn quiet_for(&self) -> Duration {
        nightly::QUIET_FOR
    }
    fn open(&self) -> bool {
        true
    }
    fn at_lamp(&self) -> Option<String> {
        None
    }
}

/// Without a nightly memory: the day's input made here, and the model's
/// proposal counted per kind, nothing kept.
#[derive(Default)]
pub struct Counting {
    pub at: u64,
    pub proposed: [u64; 6],
}
impl Memory for Counting {
    fn prepare(&mut self, line: &Value) -> Option<Value> {
        let records: Vec<Value> = LINES
            .iter()
            .enumerate()
            .map(|(n, (heard, said))| {
                json!({"id":n + 1,"at_unix_ms":self.at + n as u64 * 60_000,"kind":"exchange","heard":heard,"said":said,"text":""})
            })
            .collect();
        Some(
            json!({"ok":true,"input":{"job":"n1","person":line["person"],"day":line["day"],
            "generation":1,"records":records,"existing":[],"profile_lines":0,
            "profile_changes_left":0,"profile_adds_left":15,"more":false}}),
        )
    }
    fn apply(&mut self, line: &Value) -> Option<Value> {
        self.proposed = proposed(&line["proposal"]);
        Some(json!({"ok":true}))
    }
}

/// Items per kind in a proposal, as the contract names its arrays.
pub fn proposed(proposal: &Value) -> [u64; 6] {
    let arrays = [
        "episodes", "facts", "profile", "patterns", "ahead", "feelings",
    ];
    let mut counts = [0; 6];
    for (array, count) in arrays.iter().zip(counts.iter_mut()) {
        *count = proposal[array]
            .as_array()
            .map_or(0, |items| items.len() as u64);
    }
    counts
}

/// A lamp-memory of the probe's own, on a fresh folder, stopped and
/// removed when dropped.
struct OwnMemory {
    child: std::process::Child,
    dir: PathBuf,
}
impl OwnMemory {
    fn start(binary: &Path) -> io::Result<(Self, PathBuf)> {
        let dir = std::env::temp_dir().join(format!("lamp-nightly-probe-{}", std::process::id()));
        private_folder(&dir)?;
        let socket = dir.join("memory.sock");
        let child = std::process::Command::new(binary)
            .args(["--dir"])
            .arg(dir.join("store"))
            .arg("--socket")
            .arg(&socket)
            .args(["--memory-days", "--utc-offset-minutes", "0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let own = Self { child, dir };
        for _ in 0..100 {
            if crate::tools::exchange(&socket, &json!({"op":"status"}), Duration::from_secs(1))
                .is_some()
            {
                return Ok((own, socket));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(io::Error::other(
            "the probe's own lamp-memory did not answer",
        ))
    }
}
/// `dir` made afresh with its `store`, each 0700 whatever the umask:
/// lamp-memory refuses a store folder its group or everyone can enter
/// (`store::private_dir`). Under the default umask the probe's was 755 and
/// its own lamp-memory never answered (C108, lamp-06c1, 10 Nov: it ran only
/// under `umask 077`).
fn private_folder(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let _ = std::fs::remove_dir_all(dir);
    let store = dir.join("store");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&store)?;
    for folder in [dir, store.as_path()] {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

impl Drop for OwnMemory {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Unix ms of 00:00 UTC of today, and of a moment of yesterday.
fn days_now() -> (u64, u64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let today = now - now % 86_400_000;
    (today, today - 86_400_000 + 10 * 3_600_000)
}

fn night_of<M: Memory>(
    memory: M,
    model: Relay,
    at: u64,
    today: u64,
) -> io::Result<Night<M, Still, Relay>> {
    let mut night = Night::new(memory, Still, model, LocalTime::Offset(0))?;
    night.note(PERSON, at);
    // The night is 03:30 of today, UTC, as the probe's memory keeps time.
    night.clock = Box::new(move || today + 3 * 3_600_000 + 1_800_000);
    night.switches = Switches::from_environment();
    night.log = std::sync::Arc::new(|_| {});
    Ok(night)
}

fn list(counts: &[u64]) -> String {
    KINDS
        .iter()
        .zip(counts)
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One printed line for a pass's end: counts only.
pub fn verdict(run: u32, step: &Step, proposed: Option<[u64; 6]>) -> String {
    match (step, proposed) {
        (Step::Applied(_), Some(proposed)) => {
            format!(
                "nightly run={run} proposed (no nightly memory, nothing applied): {}",
                list(&proposed)
            )
        }
        (Step::Applied(counts), None) => format!(
            "nightly run={run} accepted {}: {}; dropped {}: {}; reasons {}",
            counts.accepted_total(),
            list(&counts.accepted),
            counts.dropped_total(),
            list(&counts.dropped),
            counts.reasons_told()
        ),
        (step, _) => format!("nightly run={run} ended {step:?}"),
    }
}

pub fn run(second_config: &Path, runs: u32) -> io::Result<()> {
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let switches = Switches::from_environment();
    let memory = std::env::var_os(MEMORY_ENVIRONMENT)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    println!(
        "nightly: DeepSeek's {} {}, thinking off, {} lines of a made-up day, patterns {}, feelings {}, {}, {runs} runs",
        config.hosted.deepseek_model,
        super::super::worker::deepseek_where(&config.hosted),
        LINES.len(),
        if switches.patterns { "on" } else { "off" },
        if switches.feelings { "on" } else { "off" },
        if memory.is_some() {
            "against the probe's own lamp-memory (LAMP_NIGHTLY_MEMORY)"
        } else {
            "no nightly memory: proposed counts only"
        }
    );
    for run in 1..=runs {
        let model = nightly::relay_of(&config).map_err(io::Error::other)?;
        let (today, at) = days_now();
        let line = match &memory {
            Some(binary) => {
                let (own, socket) = OwnMemory::start(binary)?;
                for (n, (heard, said)) in LINES.iter().enumerate() {
                    let written = crate::tools::exchange(
                        &socket,
                        &json!({"op":"append","person":PERSON,"at_unix_ms":at + n as u64 * 60_000,"heard":heard,"said":said}),
                        Duration::from_secs(3),
                    );
                    if written.is_none_or(|answer| answer["ok"] != true) {
                        return Err(io::Error::other(
                            "the probe's own lamp-memory refused the day",
                        ));
                    }
                }
                let mut night = night_of(nightly::Socket(socket), model, at, today)?;
                let step = night.step();
                drop(own);
                verdict(run, &step, None)
            }
            None => {
                let counting = Counting {
                    at,
                    ..Counting::default()
                };
                let mut night = night_of(counting, model, at, today)?;
                let step = night.step();
                verdict(run, &step, Some(night.memory.proposed))
            }
        };
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nightly::Counts;

    #[test]
    fn the_made_up_day_is_the_test_persons_and_a_known_id() {
        assert!(lamp_msgs::known::is_person_id(PERSON));
        let mut counting = Counting {
            at: 1_000,
            ..Counting::default()
        };
        let answer = counting
            .prepare(&nightly::prepare_line(PERSON, "2026-10-13", 0))
            .unwrap();
        let prepared = nightly::prepared_of(&answer).unwrap().unwrap();
        assert_eq!(
            prepared.input["records"].as_array().unwrap().len(),
            LINES.len()
        );
        assert_eq!(prepared.input["person"], PERSON);
    }

    /// C108: the probe's folder and its store are its owner's alone, even
    /// made afresh over a folder others could enter, so its own lamp-memory
    /// takes the store under the default umask.
    #[test]
    fn the_probes_own_folder_is_its_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("c108-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("store")).unwrap();
        for folder in [dir.clone(), dir.join("store")] {
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        private_folder(&dir).unwrap();
        for folder in [dir.clone(), dir.join("store")] {
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", folder.display());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_printed_lines_are_counts_only() {
        let counts = Counts::of(&json!({"ok":true,
            "accepted":{"episode":1,"fact":2},"dropped":{"fact":1},"reasons":{"private_kind":1}}))
        .unwrap();
        let line = verdict(1, &Step::Applied(Box::new(counts)), None);
        assert_eq!(
            line,
            "nightly run=1 accepted 3: episode 1, fact 2, profile 0, pattern 0, ahead 0, feeling 0; dropped 1: episode 0, fact 1, profile 0, pattern 0, ahead 0, feeling 0; reasons private_kind 1"
        );
        let proposal = json!({"facts":[{"claim":"secret words"}],"episodes":[]});
        let line = verdict(2, &Step::Applied(Box::default()), Some(proposed(&proposal)));
        assert!(line.contains("fact 1") && !line.contains("secret"));
        for (heard, _) in LINES {
            assert!(!line.contains(heard));
        }
    }
}
