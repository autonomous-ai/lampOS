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

pub const USAGE: &str = "lamp-voice-eval observe-dry --store FILE --out NEW_DIR --key-file FILE
      [--base URL] [--model NAME] [--gap-seconds N] [--limit N]";

/// The voice's own idle between conversations (`remember::CONVERSATION_IDLE`).
const GAP_MS: u64 = 45_000;
/// The most exchanges one conversation may cite (`remember::SOURCE_IDS`).
const MOST_SOURCES: usize = 64;

struct Arguments {
    store: PathBuf,
    out: PathBuf,
    key_file: PathBuf,
    base: String,
    model: String,
    gap_ms: u64,
    limit: usize,
}

fn parse(args: &[String]) -> Result<Arguments> {
    let mut store = None;
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
        store: store.ok_or_else(|| invalid(USAGE))?,
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
    run_with(&args, &observer)
}

/// [`run`] with its model given: the lamp's own (`observer_at`), or a
/// scripted one in a test.
fn run_with(args: &Arguments, observer: &lamp_voice::remember::Observer) -> Result<i32> {
    DirBuilder::new().mode(0o700).create(&args.out)?;
    let copy = args.out.join("store");
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let outcome = (|| -> Result<Value> {
        // Inside, so a copy that fails partway is removed with the rest
        // (the reviewer on #1988).
        copy_store(&args.store, &copy)?;
        // Kept for ten years: nothing of the copy is pruned on opening.
        let mut days = Days::open(
            Options {
                dir: copy.clone(),
                keep_days: 3_650,
                local: LocalTime::Offset(0),
            },
            now,
        )?;
        let mut labels = private_file(&args.out.join("labels.jsonl"))?;
        let mut counts = BTreeMap::<String, u64>::new();
        let mut asked_ms = vec![];
        let cut = conversations(&days.exchanges()?, args.gap_ms);
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
                .and_then(|output| serde_json::from_value::<tidy::Output>(output).ok());
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
        Ok(
            json!({"counts": counts, "asked_ms": {"n": asked_ms.len(), "median": at(0.5), "p90": at(0.9)},
            "labels": args.out.join("labels.jsonl")}),
        )
    })();
    // The copy of the store goes, whatever happened, a partial one too.
    let removed = !copy.exists() || fs::remove_dir_all(&copy).is_ok();
    let mut summary = outcome?;
    summary["store_copy_removed"] = json!(removed);
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(0)
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
        // own exchange when there is one to keep, and nothing otherwise.
        let observer: lamp_voice::remember::Observer = Box::new(|input: &Value| {
            let first = &input["records"][0];
            Some(if first["heard"].as_str()?.contains("sugar") {
                json!({"observations":[{"text":"They take their tea with no sugar","category":"preference","source_ids":[first["id"]],"supersedes":null}]})
            } else {
                json!({"observations":[]})
            })
        });
        assert_eq!(run_with(&args, &observer).unwrap(), 0);
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
