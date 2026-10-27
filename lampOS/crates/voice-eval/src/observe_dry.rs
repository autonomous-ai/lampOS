//! `lamp-voice-eval observe-dry` (#1693 (4)): the memory's observation pass
//! (`LAMP_MEMORY_OBSERVE`) run over a copy of a lamp's Days store, before it
//! is turned on, to measure it against the first builder's bar: under one in
//! ten kept wrongly, and none a person would find intrusive.
//!
//! The copy is cut into conversations as the voice cuts them (45 s apart, or
//! another person), and each goes through the lamp's own path: the store's
//! `conversation_end`, the pass's prompt and DeepSeek client
//! (`second_provider::worker::observer_at`), and the store's `tidy_apply`,
//! which keeps or refuses as it would on the lamp. The lamp's own store is
//! never opened for writing; the copy is removed at the end.
//!
//! What was said and proposed goes only to `labels.jsonl` in `--out` (0600),
//! for a person to mark each proposal right, wrong or intrusive on the Mac
//! doing the labelling. What is printed is counts and milliseconds only.
use lamp_memory::{
    Options,
    days::{DATABASE, Days},
    store::{LocalTime, Person},
    tidy,
};
use lamp_voice_eval::{Result, invalid};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const USAGE: &str =
    "lamp-voice-eval observe-dry (--store FILE | --conversations FILE) --out NEW_DIR
      --key-file FILE [--base URL] [--model NAME] [--gap-seconds N] [--limit N]";

/// The voice's own idle between conversations (`remember::CONVERSATION_IDLE`).
const GAP_MS: u64 = 45_000;
/// The most exchanges one conversation may cite (`remember::SOURCE_IDS`).
const MOST_SOURCES: usize = 64;

/// What the pass is run over: a copy of a lamp's store, or a set of
/// made-up conversations with what each should keep (#1693, the first
/// builder's 01:36), built into a store of the run's own and scored.
enum Source {
    Store(PathBuf),
    Conversations(PathBuf),
}

struct Arguments {
    source: Source,
    out: PathBuf,
    key_file: PathBuf,
    base: String,
    model: String,
    gap_ms: u64,
    limit: usize,
}

fn parse(args: &[String]) -> Result<Arguments> {
    let mut store = None;
    let mut made_up = None;
    let mut out = None;
    let mut key_file = None;
    let mut base = "https://api.deepseek.com".to_owned();
    let mut model = "deepseek-flash".to_owned();
    let mut gap_ms = GAP_MS;
    let mut limit = usize::MAX;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| invalid(&format!("{flag} needs a value\n{USAGE}")))
        };
        match flag.as_str() {
            "--store" => store = Some(PathBuf::from(value()?)),
            "--conversations" => made_up = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--key-file" => key_file = Some(PathBuf::from(value()?)),
            "--base" => base = value()?,
            "--model" => model = value()?,
            "--gap-seconds" => {
                gap_ms = value()?
                    .parse::<u64>()
                    .ok()
                    .filter(|seconds| (1..=3_600).contains(seconds))
                    .ok_or_else(|| invalid("--gap-seconds is 1 to 3600"))?
                    * 1_000
            }
            "--limit" => {
                limit = value()?
                    .parse()
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or_else(|| invalid("--limit is a positive count"))?
            }
            _ => return Err(invalid(USAGE)),
        }
    }
    Ok(Arguments {
        source: match (store, made_up) {
            (Some(store), None) => Source::Store(store),
            (None, Some(made_up)) => Source::Conversations(made_up),
            _ => return Err(invalid(USAGE)),
        },
        out: out.ok_or_else(|| invalid(USAGE))?,
        key_file: key_file.ok_or_else(|| invalid(USAGE))?,
        base,
        model,
        gap_ms,
        limit,
    })
}

/// The journal cut as the voice cuts it: a new conversation after `gap_ms`
/// of quiet, at another person, or at the most sources one may cite.
pub fn conversations(exchanges: &[(Person, u64, u64)], gap_ms: u64) -> Vec<(Person, Vec<u64>)> {
    let mut found: Vec<(Person, Vec<u64>)> = vec![];
    let mut last = None;
    for (person, id, at) in exchanges {
        let same = last.is_some_and(|last: u64| at.saturating_sub(last) < gap_ms)
            && found
                .last()
                .is_some_and(|(whose, ids)| whose == person && ids.len() < MOST_SOURCES);
        match found.last_mut() {
            Some((_, ids)) if same => ids.push(*id),
            _ => found.push((person.clone(), vec![*id])),
        }
        last = Some(*at);
    }
    found
}

fn private_file(path: &Path) -> Result<fs::File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

/// The store's files copied into a private folder of the run's own.
fn copy_store(store: &Path, into: &Path) -> Result<()> {
    DirBuilder::new().mode(0o700).create(into)?;
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", store.display()));
        if from.exists() {
            let mut to = private_file(&into.join(format!("{DATABASE}{suffix}")))?;
            to.write_all(&fs::read(&from)?)?;
        } else if suffix.is_empty() {
            return Err(invalid("--store names no store file"));
        }
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<i32> {
    let args = parse(args)?;
    let key = fs::read_to_string(&args.key_file)?.trim().to_owned();
    if key.is_empty() {
        return Err(invalid("--key-file is empty"));
    }
    let observer = lamp_voice::second_provider::worker::observer_at(&args.base, &args.model, &key)?;
    let summary = run_with(&args, &observer)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(0)
}

/// [`run`] with its model given: the lamp's own (`observer_at`), or a
/// scripted one in a test.
/// What the model proposed as the lamp sends it: a `replaces` the model
/// wrote is dropped, as `observations_for_memory` drops it (the
/// reviewer-lead's P3 on #3853). This run has no session shown, so it
/// retires nothing.
fn as_the_lamp_sends(mut output: tidy::Output) -> tidy::Output {
    for observation in &mut output.observations {
        observation.replaces = None;
    }
    output
}

fn run_with(args: &Arguments, observer: &lamp_voice::remember::Observer) -> Result<Value> {
    DirBuilder::new().mode(0o700).create(&args.out)?;
    let copy = args.out.join("store");
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let outcome = (|| -> Result<Value> {
        // Inside, so a copy that fails partway is removed with the rest
        // (the reviewer on #1988).
        match &args.source {
            Source::Store(store) => copy_store(store, &copy)?,
            Source::Conversations(_) => DirBuilder::new().mode(0o700).create(&copy)?,
        }
        // Kept for ten years: nothing of the copy is pruned on opening.
        let mut days = Days::open(
            Options {
                dir: copy.clone(),
                keep_days: 3_650,
                local: LocalTime::Offset(0),
            },
            now,
        )?;
        let expected = match &args.source {
            Source::Store(_) => vec![],
            Source::Conversations(file) => {
                made_up_store(&mut days, &fs::read_to_string(file)?, now)?
            }
        };
        let mut score = Score::default();
        let mut labels = private_file(&args.out.join("labels.jsonl"))?;
        let mut counts = BTreeMap::<String, u64>::new();
        let mut asked_ms = vec![];
        let cut = conversations(&days.exchanges()?, args.gap_ms);
        // Scored one for one: the set's n-th conversation is the cut's n-th,
        // so a gap or limit that joins or drops some is refused (the peer
        // read's P3 on #2021).
        if !expected.is_empty() && (cut.len() != expected.len() || args.limit < cut.len()) {
            return Err(invalid(&format!(
                "the made-up set has {} conversations and the cut {} (limit {}): leave --gap-seconds under 600 and --limit out",
                expected.len(),
                cut.len(),
                args.limit.min(cut.len())
            )));
        }
        for (n, (person, ids)) in cut.iter().take(args.limit).enumerate() {
            *counts.entry("conversations".into()).or_default() += 1;
            let input = match days.conversation_end_with(person, &format!("dry-{n}"), ids, &[]) {
                Ok(input) => input,
                Err(error) => {
                    *counts
                        .entry(format!("not_prepared_{}", error.code))
                        .or_default() += 1;
                    continue;
                }
            };
            let started = Instant::now();
            let proposed = observer(&serde_json::to_value(&input)?)
                .and_then(|output| serde_json::from_value::<tidy::Output>(output).ok())
                .map(as_the_lamp_sends);
            asked_ms.push(started.elapsed().as_millis() as u64);
            let output = proposed.clone().unwrap_or(tidy::Output {
                observations: vec![],
            });
            let applied = days.tidy_apply(person, &input.job, &output);
            let outcome = match (&proposed, &applied) {
                (None, _) => "the model gave nothing usable".to_owned(),
                (Some(output), Ok(_)) if output.observations.is_empty() => {
                    "nothing worth keeping".into()
                }
                (Some(_), Ok(ids)) => format!("kept {}", ids.len()),
                (Some(_), Err(error)) => format!("refused: {}", error.code),
            };
            match (&proposed, &applied) {
                (None, _) => *counts.entry("model_gave_nothing".into()).or_default() += 1,
                (Some(output), Ok(ids)) => {
                    *counts.entry("proposed".into()).or_default() +=
                        output.observations.len() as u64;
                    *counts.entry("kept".into()).or_default() += ids.len() as u64;
                    for proposal in &output.observations {
                        *counts
                            .entry(format!("kept_{:?}", proposal.category).to_lowercase())
                            .or_default() += 1;
                    }
                }
                (Some(output), Err(error)) => {
                    *counts.entry("proposed".into()).or_default() +=
                        output.observations.len() as u64;
                    *counts.entry(format!("refused_{}", error.code)).or_default() += 1;
                }
            }
            let kept = applied.is_ok();
            if let Some(expect) = expected.get(n) {
                let texts: Vec<&str> = match (&proposed, &applied) {
                    (Some(output), Ok(_)) => output
                        .observations
                        .iter()
                        .map(|p| p.text.as_str())
                        .collect(),
                    _ => vec![],
                };
                score.add(expect, &texts);
            }
            let line = json!({
                "conversation": n,
                "person": if person.as_str() == Person::EVERYONE { "everyone".to_owned() } else { format!("{}…", person.as_str().chars().take(6).collect::<String>()) },
                "exchanges": input.records.iter().map(|r| json!({"heard": r.heard, "said": r.said})).collect::<Vec<_>>(),
                "proposals": proposed.as_ref().map_or(vec![], |output| output.observations.iter().map(|p| json!({
                    "text": p.text, "category": p.category, "kept": kept,
                    "mark": "right | wrong | intrusive",
                })).collect()),
                "outcome": outcome,
            });
            writeln!(labels, "{line}")?;
        }
        asked_ms.sort_unstable();
        let at = |q: f64| {
            asked_ms
                .get(((asked_ms.len().max(1) - 1) as f64 * q).round() as usize)
                .copied()
        };
        let mut summary = json!({"counts": counts, "asked_ms": {"n": asked_ms.len(), "median": at(0.5), "p90": at(0.9)},
            "labels": args.out.join("labels.jsonl")});
        if !expected.is_empty() {
            summary["bar"] = score.summary();
        }
        Ok(summary)
    })();
    // The copy of the store goes, whatever happened, a partial one too.
    let removed = !copy.exists() || fs::remove_dir_all(&copy).is_ok();
    let mut summary = outcome?;
    summary["store_copy_removed"] = json!(removed);
    Ok(summary)
}

/// One made-up conversation: its turns (heard, said) and what the pass
/// should keep of it: a thing carrying `keep`, or nothing; `intrusive` marks
/// one whose keeping, unasked, a person would mind (health, money).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MadeUp {
    pub turns: Vec<(String, String)>,
    #[serde(default)]
    pub keep: Option<String>,
    #[serde(default)]
    pub intrusive: bool,
}

/// The set built into the run's own store, the room's, each conversation
/// ten minutes after the last and its turns ten seconds apart, so the cut
/// gives them back one for one. What each should keep, in that order.
fn made_up_store(days: &mut Days, set: &str, now: u64) -> Result<Vec<MadeUp>> {
    let set: Vec<MadeUp> = serde_json::from_str(set)?;
    let room = Person::everyone();
    let start = now.saturating_sub(set.len() as u64 * 600_000 + 600_000);
    for (n, conversation) in set.iter().enumerate() {
        if conversation.turns.is_empty() || conversation.turns.len() > MOST_SOURCES {
            return Err(invalid("each made-up conversation has 1 to 64 turns"));
        }
        for (k, (heard, said)) in conversation.turns.iter().enumerate() {
            let at = start + n as u64 * 600_000 + k as u64 * 10_000;
            days.append(&room, at, heard, said, &[])?;
        }
    }
    Ok(set)
}

/// The first builder's bar, counted: things kept rightly (each carries the
/// word it should, as a whole word), things kept wrongly (anything else
/// kept), conversations missed, intrusive things kept, and conversations
/// rightly left with nothing.
#[derive(Default)]
struct Score {
    kept_rightly: u64,
    kept_wrongly: u64,
    missed: u64,
    intrusive_kept: u64,
    nothing_right: u64,
    /// Conversations with something worth keeping.
    worth: u64,
}

impl Score {
    fn add(&mut self, expect: &MadeUp, kept: &[&str]) {
        match &expect.keep {
            Some(word) => {
                // A whole word, as lamp-skills' `if_heard` matches: "mum" is
                // not in "minimum" (the peer read's P3 on #2021).
                let carries = |text: &&&str| {
                    text.split(|c: char| !c.is_alphanumeric())
                        .any(|w| w.to_lowercase() == word.to_lowercase())
                };
                let right = kept.iter().filter(carries).count() as u64;
                self.worth += 1;
                if right == 0 {
                    self.missed += 1;
                }
                // Each thing kept counts once: right if it carries the word,
                // wrong if not; two right ones are both right.
                self.kept_rightly += right;
                self.kept_wrongly += kept.len() as u64 - right;
            }
            None if kept.is_empty() => self.nothing_right += 1,
            None => {
                self.kept_wrongly += kept.len() as u64;
                if expect.intrusive {
                    self.intrusive_kept += kept.len() as u64;
                }
            }
        }
    }

    fn summary(&self) -> Value {
        let kept = self.kept_rightly + self.kept_wrongly;
        json!({
            "kept_rightly": self.kept_rightly,
            "kept_wrongly": self.kept_wrongly,
            "missed": self.missed,
            "intrusive_kept": self.intrusive_kept,
            "nothing_right": self.nothing_right,
            "wrong_per_kept": if kept == 0 { Value::Null } else { json!(self.kept_wrongly as f64 / kept as f64) },
            // The conversations worth keeping from that were missed: beside
            // the bar, not in it, so the bar's line can't hide them (the
            // reviewer's P3 on #2021).
            "missed_of_worth": if self.worth == 0 { Value::Null } else { json!(self.missed as f64 / self.worth as f64) },
            // The first builder's bar on #1693: under 1 in 10 kept wrongly,
            // none intrusive. It says nothing of what was missed.
            "passes_wrong_and_intrusive": kept > 0 && self.kept_wrongly * 10 < kept && self.intrusive_kept == 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str) -> Person {
        Person::new(id).unwrap()
    }

    /// Cut as the voice cuts: 45 s of quiet, another person, or 64 sources.
    #[test]
    fn the_journal_is_cut_into_conversations_as_the_voice_cuts_it() {
        let room = Person::everyone();
        let a = person("p-aaaa");
        let mut exchanges = vec![
            (room.clone(), 1, 0),
            (room.clone(), 2, 44_999),
            (room.clone(), 3, 44_999 + GAP_MS),
            (a.clone(), 4, 44_999 + GAP_MS + 1),
            (room.clone(), 5, 44_999 + GAP_MS + 2),
        ];
        let cut = conversations(&exchanges, GAP_MS);
        let ids: Vec<Vec<u64>> = cut.iter().map(|(_, ids)| ids.clone()).collect();
        assert_eq!(ids, [vec![1, 2], vec![3], vec![4], vec![5]]);
        assert_eq!(cut[2].0, a);
        exchanges = (0..70).map(|n| (room.clone(), n + 1, n * 1_000)).collect();
        let long = conversations(&exchanges, GAP_MS);
        assert_eq!(
            long.iter().map(|(_, ids)| ids.len()).collect::<Vec<_>>(),
            [64, 6]
        );
    }

    /// A whole dry run on a copy: a seeded store, a stand-in for DeepSeek
    /// that proposes one preference cited from the conversation, the store's
    /// own checks, and nothing but counts printed. The labels hold the words;
    /// the copy is gone after.
    #[test]
    fn a_dry_run_keeps_what_the_store_allows_and_removes_its_copy() {
        let root = std::env::temp_dir().join(format!("lamp-observe-dry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let room = Person::everyone();
        {
            let mut days = Days::open(
                Options {
                    dir: root.join("lamp"),
                    keep_days: 30,
                    local: LocalTime::Offset(0),
                },
                now,
            )
            .unwrap();
            days.append(
                &room,
                now - 200_000,
                "synthetic: I take my tea with no sugar",
                "Noted",
                &[],
            )
            .unwrap();
            days.append(
                &room,
                now - 100_000,
                "synthetic: what's the weather",
                "Sunny",
                &[],
            )
            .unwrap();
        }
        let lamp_files = || -> Vec<(PathBuf, Vec<u8>)> {
            let mut files: Vec<_> = fs::read_dir(root.join("lamp"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .map(|path| (path.clone(), fs::read(&path).unwrap()))
                .collect();
            files.sort();
            files
        };
        let before = lamp_files();
        let out = root.join("out");
        let args = parse(
            &[
                "--store",
                root.join("lamp").join(DATABASE).to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--key-file",
                "unused",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        // The scripted model: one preference cited from the conversation's
        // own exchange when there is one to keep, with a `replaces` of its
        // own (dropped, [`as_the_lamp_sends`]), and nothing otherwise.
        let observer: lamp_voice::remember::Observer = Box::new(|input: &Value| {
            let first = &input["records"][0];
            Some(if first["heard"].as_str()?.contains("sugar") {
                json!({"observations":[{"text":"They take their tea with no sugar","category":"preference","source_ids":[first["id"]],"supersedes":null,"replaces":1}]})
            } else {
                json!({"observations":[]})
            })
        });
        run_with(&args, &observer).unwrap();
        assert!(!out.join("store").exists());
        let labels: Vec<Value> = fs::read_to_string(out.join("labels.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0]["outcome"], "kept 1");
        assert_eq!(
            labels[0]["proposals"][0]["text"],
            "They take their tea with no sugar"
        );
        assert_eq!(labels[1]["outcome"], "nothing worth keeping");
        // The lamp's own store was never opened for writing: every one of
        // its files is byte for byte as it was.
        assert_eq!(lamp_files(), before);
        let _ = fs::remove_dir_all(&root);
    }

    /// The reviewer-lead's P3 on #3853: a model's own `replaces` is dropped
    /// before `tidy_apply`, as the lamp drops it.
    #[test]
    fn a_models_own_replaces_is_dropped_as_on_the_lamp() {
        let output: tidy::Output = serde_json::from_value(json!({"observations":[
            {"text":"Works at a library","category":"preference","source_ids":[2],"supersedes":null,"replaces":1}
        ]}))
        .unwrap();
        assert_eq!(output.observations[0].replaces, Some(1));
        let sent = as_the_lamp_sends(output);
        assert_eq!(sent.observations[0].replaces, None);
        assert_eq!(sent.observations[0].text, "Works at a library");
    }

    /// A copy that fails partway, its `-wal` unreadable after the main file
    /// was copied, leaves nothing of the store behind (the reviewer on #1988).
    #[test]
    fn a_copy_that_fails_partway_is_removed() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("lamp-observe-dry-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let store = root.join(DATABASE);
        fs::write(&store, b"synthetic main file").unwrap();
        let wal = root.join(format!("{DATABASE}-wal"));
        fs::write(&wal, b"synthetic wal").unwrap();
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o000)).unwrap();
        let out = root.join("out");
        let args = parse(
            &[
                "--store",
                store.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--key-file",
                "unused",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let observer: lamp_voice::remember::Observer = Box::new(|_: &Value| None);
        assert!(run_with(&args, &observer).is_err());
        assert!(!out.join("store").exists());
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    /// The made-up set (#1693, the first builder's 01:36) built into the
    /// run's own store and scored against what each should keep. A scripted
    /// model that keeps the ten right things, misses none, and keeps the
    /// health line it should leave: 10 right, 1 wrong, 1 intrusive, and the
    /// bar not passed.
    #[test]
    fn the_made_up_set_is_scored_against_what_each_conversation_should_keep() {
        const SET: &str = include_str!("../observe/synthetic-20.json");
        let set: Vec<MadeUp> = serde_json::from_str(SET).unwrap();
        assert_eq!(set.len(), 20);
        let root =
            std::env::temp_dir().join(format!("lamp-observe-made-up-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let file = root.join("set.json");
        fs::write(&file, SET).unwrap();
        let out = root.join("out");
        let args = parse(
            &[
                "--conversations",
                file.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--key-file",
                "unused",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let keep: Vec<String> = set.iter().filter_map(|c| c.keep.clone()).collect();
        let observer: lamp_voice::remember::Observer = Box::new(move |input: &Value| {
            let first = &input["records"][0];
            let heard = first["heard"].as_str()?;
            let worth = keep.iter().any(|word| heard.contains(word.as_str()))
                || heard.contains("cholesterol");
            Some(if worth {
                json!({"observations":[{"text":heard,"category":"thread","source_ids":[first["id"]],"supersedes":null}]})
            } else {
                json!({"observations":[]})
            })
        });
        let summary = run_with(&args, &observer).unwrap();
        assert_eq!(
            summary["bar"],
            json!({"kept_rightly":10,"kept_wrongly":1,"missed":0,"intrusive_kept":1,"nothing_right":9,
                "wrong_per_kept":1.0/11.0,"missed_of_worth":0.0,"passes_wrong_and_intrusive":false})
        );
        assert_eq!(summary["counts"]["conversations"], 20);
        assert!(!out.join("store").exists());
        assert_eq!(
            fs::read_to_string(out.join("labels.jsonl"))
                .unwrap()
                .lines()
                .count(),
            20
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// The peer read's P3s on #2021: a whole word counts, a part of one
    /// doesn't, and two right things are both right.
    #[test]
    fn the_score_counts_whole_words_and_each_thing_once() {
        let made_up = |keep: Option<&str>| MadeUp {
            turns: vec![],
            keep: keep.map(str::to_owned),
            intrusive: false,
        };
        let mut score = Score::default();
        score.add(&made_up(Some("mum")), &["Visiting mum on Sunday"]);
        score.add(&made_up(Some("mum")), &["At least the minimum"]);
        score.add(&made_up(Some("warm")), &["Warm light", "Warm, never blue"]);
        assert_eq!(
            (score.kept_rightly, score.kept_wrongly, score.missed),
            (3, 1, 1)
        );
    }

    /// A gap or a limit that doesn't give the made-up set back one for one
    /// is refused, so the bar is never scored on a mismatch.
    #[test]
    fn a_cut_that_joins_or_drops_made_up_conversations_is_refused() {
        const SET: &str = include_str!("../observe/synthetic-20.json");
        let root =
            std::env::temp_dir().join(format!("lamp-observe-made-up-cut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let file = root.join("set.json");
        fs::write(&file, SET).unwrap();
        let observer: lamp_voice::remember::Observer =
            Box::new(|_: &Value| Some(json!({"observations":[]})));
        for (n, extra) in [["--gap-seconds", "900"], ["--limit", "5"]]
            .iter()
            .enumerate()
        {
            let out = root.join(format!("out{n}"));
            let args = parse(
                &[
                    "--conversations",
                    file.to_str().unwrap(),
                    "--out",
                    out.to_str().unwrap(),
                    "--key-file",
                    "unused",
                    extra[0],
                    extra[1],
                ]
                .map(str::to_owned),
            )
            .unwrap();
            assert!(run_with(&args, &observer).is_err(), "{extra:?}");
            assert!(!out.join("store").exists(), "{extra:?}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn its_arguments_need_a_store_an_out_folder_and_a_key() {
        let given = |args: &[&str]| parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        assert!(given(&["--store", "s", "--out", "o"]).is_err());
        let parsed = given(&[
            "--store",
            "s",
            "--out",
            "o",
            "--key-file",
            "k",
            "--gap-seconds",
            "60",
        ])
        .unwrap();
        assert_eq!(
            (parsed.gap_ms, parsed.model.as_str()),
            (60_000, "deepseek-flash")
        );
        assert!(
            given(&[
                "--store",
                "s",
                "--out",
                "o",
                "--key-file",
                "k",
                "--gap-seconds",
                "0"
            ])
            .is_err()
        );
        assert!(given(&["--nope"]).is_err());
    }
}
