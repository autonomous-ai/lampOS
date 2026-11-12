//! A skill made by voice that clashes with one on the lamp (#6593): the
//! same trigger (its topic, field and condition) and both using the light,
//! or both moving the head. Two such skills outrank each other's light in
//! turn ("light is outranked by skill:X"), so on lamp-9adf six
//! `glow-*-on-return` skills piled up on one arrival and "glow green when
//! I'm present" showed only some of the time.
//!
//! A clash is found after the made skill is checked and kept, before it is
//! put on ([`found`]): nothing is put on, and the model is told the other
//! skill's spoken name and to ask the person to replace it, keep both (the
//! new one wins) or cancel ([`asked`]). Their answer, in a later line the
//! owner said, comes back as `make_skill` with [`CLASH_ID`] and [`CHOICE`]
//! alone ([`chosen`]): replace takes the other off through the manage
//! helper's own remove (as the app's remove does) and puts the new one on;
//! keep puts the new one on beside it; cancel puts nothing on.
use crate::made_skill::{self, Keeping, Made};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The `make_skill` argument that answers a clash: its id, as told.
pub const CLASH_ID: &str = "clash_id";
/// The `make_skill` argument with the person's choice: one of [`CHOICES`].
pub const CHOICE: &str = "choice";
/// What the person may choose.
pub const CHOICES: [&str; 3] = ["replace", "keep", "cancel"];

/// How long a clash waits for the person's choice.
pub const CLASH_WITHIN: Duration = Duration::from_secs(300);

/// How long the manage helper is waited on for a remove or a put-on.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// The keys of a trigger that say when it fires: its topic is matched on
/// its own. Its pace (`for`, `cooldown`, `rearm`, `gap`, `wait`, …) and its
/// window (`during`, `days`) are not: two skills on one arrival clash
/// whatever their cooldown.
const CONDITION_KEYS: [&str; 7] = [
    "field", "when", "equals", "above", "below", "present", "changes",
];

/// What a skill's `skill.toml` starts on and what it uses, as a clash is
/// judged: each trigger's topic and condition ([`CONDITION_KEYS`]), and
/// whether any `[[do]]` step uses the light or the body. Kept with a made
/// skill in its index (`made_skill`'s `"clash"`), so one made here and on
/// is judged even when its folder can't be read.
pub fn signature(toml: &str) -> Value {
    let Ok(table) = toml::from_str::<toml::Table>(toml) else {
        return json!({"triggers": [], "light": false, "body": false});
    };
    let triggers: Vec<Value> = match table.get("trigger") {
        Some(toml::Value::Table(trigger)) => vec![trigger_key(trigger)],
        Some(toml::Value::Array(list)) => list
            .iter()
            .filter_map(toml::Value::as_table)
            .map(trigger_key)
            .collect(),
        _ => Vec::new(),
    };
    let steps: Vec<&toml::Table> = table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .collect();
    json!({
        "triggers": triggers,
        "light": steps.iter().any(|step| step.contains_key("light")),
        "body": steps.iter().any(|step| step.contains_key("body")),
    })
}

fn trigger_key(trigger: &toml::Table) -> Value {
    let mut key = json!({ "topic": trigger.get("topic").and_then(toml::Value::as_str) });
    for name in CONDITION_KEYS {
        if let Some(value) = trigger.get(name) {
            key[name] = serde_json::to_value(value).unwrap_or(Value::Null);
        }
    }
    key
}

/// What two skills share, by their [`signature`]s: "the light", "the
/// head", both; none when no trigger is the same or no output is shared.
pub fn shared(one: &Value, other: &Value) -> Option<&'static str> {
    let triggers = |signature: &Value| -> Vec<Value> {
        signature["triggers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|trigger| trigger["topic"].is_string())
            .collect()
    };
    let theirs = triggers(other);
    if !triggers(one).iter().any(|trigger| theirs.contains(trigger)) {
        return None;
    }
    let both = |key: &str| one[key] == true && other[key] == true;
    match (both("light"), both("body")) {
        (true, true) => Some("the light and the head"),
        (true, false) => Some("the light"),
        (false, true) => Some("the head"),
        (false, false) => None,
    }
}

/// A skill on the lamp the made one clashes with: its folder's name, and
/// what both use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    pub name: String,
    pub shares: &'static str,
}

/// The skill on the lamp that `made`, just kept and not yet on, clashes
/// with ([`shared`]): first those installed (`keeping.installed`, and the
/// skills pushed beside it), each by its `skill.toml`; then the made skills
/// kept here and on whose folder isn't there, by the signature kept with
/// them. None when nothing clashes, or the made one's signature isn't kept.
pub fn found(keeping: &Keeping, made: &Made) -> Option<Clash> {
    let entries = made_skill::entries(&keeping.directory);
    let mine = entries
        .iter()
        .find(|entry| entry["sha256"] == made.sha256.as_str())
        .map(|entry| entry["clash"].clone())
        .filter(Value::is_object)?;
    let mut seen = Vec::new();
    for folder in [
        keeping.installed.clone(),
        made_skill::pushed_folder(&keeping.installed),
    ] {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        names.sort();
        for path in names {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name == made.name {
                continue;
            }
            seen.push(name.to_owned());
            let Ok(toml) = std::fs::read_to_string(path.join("skill.toml")) else {
                continue;
            };
            if let Some(shares) = shared(&mine, &signature(&toml)) {
                return Some(Clash {
                    name: name.to_owned(),
                    shares,
                });
            }
        }
    }
    entries
        .iter()
        .filter(|entry| entry["on"] == true && entry["sha256"] != made.sha256.as_str())
        .filter_map(|entry| Some((entry["name"].as_str()?, &entry["clash"])))
        .filter(|(name, _)| *name != made.name && !seen.iter().any(|seen| seen == name))
        .find_map(|(name, theirs)| {
            shared(&mine, theirs).map(|shares| Clash {
                name: name.to_owned(),
                shares,
            })
        })
}

/// A clash waiting for the person's choice: one at a time, the newest.
struct Waiting {
    id: String,
    keeping: Keeping,
    sha256: String,
    name: String,
    other: String,
    request: u64,
    at: Instant,
}

static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The line said for a clash when the turn that made it was cut: the model
/// never had the question to ask, so the person is told it wasn't put on.
pub fn cut_line(made: &Made, clash: &Clash) -> String {
    format!(
        "I made {}, but didn't put it on: {} starts at the same moment and uses {}. Ask me \
         again to replace it or to keep both.",
        made_skill::spoken_name(&made.name),
        made_skill::spoken_name(&clash.name),
        clash.shares
    )
}

/// `made`, kept and not on, clashes with `clash`: it waits for the person's
/// choice, asked in a later line than `request`, at most [`CLASH_WITHIN`]
/// after `now`. What the model is told: the other skill's spoken name, and
/// to say the clash in a few words and ask the three choices.
pub fn asked(keeping: &Keeping, made: &Made, clash: &Clash, request: u64, now: Instant) -> Value {
    let id = format!(
        "clash-{}",
        IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    *WAITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Waiting {
        id: id.clone(),
        keeping: keeping.clone(),
        sha256: made.sha256.clone(),
        name: made.name.clone(),
        other: clash.name.clone(),
        request,
        at: now,
    });
    eprintln!(
        "lamp-voice: request {request}: a made skill {} clashes with {} ({}): not put on, the person is asked",
        made.name, clash.name, clash.shares
    );
    let new = made_skill::spoken_name(&made.name);
    let other = made_skill::spoken_name(&clash.name);
    json!({
        "made": made.name,
        "on": false,
        "clashes_with": other,
        "shares": clash.shares,
        CLASH_ID: id,
        "next": format!(
            "Nothing is on yet. Say in a few words, in the language of the conversation, that \
             {new} would start at the same moment as {other}, which is on the lamp, and both use \
             {}; then ask whether to replace {other}, keep both (then {new} wins), or cancel. \
             Call nothing now. When they answer, call make_skill with {CLASH_ID} \"{id}\" and \
             {CHOICE} \"replace\", \"keep\" or \"cancel\" alone.",
            clash.shares
        )
    })
}

/// How a choice is carried out: the other skill taken off by name, and the
/// new one put on by its SHA-256 (its name once on, or why not).
pub struct Hands<'a> {
    pub remove: &'a dyn Fn(&Keeping, &str) -> Result<(), String>,
    pub put_on: &'a dyn Fn(&Keeping, &str) -> Result<String, String>,
}

/// The product's [`Hands`]: the manage helper's own `{"remove": NAME}`, as
/// the app's remove asks it, and [`made_skill::put_on_within`].
pub fn helper_hands() -> Hands<'static> {
    Hands {
        remove: &|keeping, name| remove_by_helper(&keeping.manage, name),
        put_on: &|keeping, sha256| {
            made_skill::put_on_within(
                &keeping.directory,
                &keeping.manage,
                sha256,
                crate::app_make::now_unix_ms(),
                HELPER_WAIT,
            )
        },
    }
}

fn remove_by_helper(socket: &Path, name: &str) -> Result<(), String> {
    match crate::tools::reached(socket, &json!({ "remove": name }), HELPER_WAIT) {
        None => Err("the lamp's skills couldn't be reached".into()),
        Some(None) => Err("the lamp didn't say in time whether it is off".into()),
        Some(Some(answer)) if answer.get("done").is_some() => Ok(()),
        Some(Some(answer)) => {
            eprintln!(
                "lamp-voice: the skills helper didn't remove {name}: {}",
                answer["refused"].as_str().unwrap_or_default()
            );
            Err(format!(
                "the lamp didn't let {} be removed",
                made_skill::spoken_name(name)
            ))
        }
    }
}

/// The person's `choice` for the clash `id`, in the voice's request
/// `request` (`heard`: someone spoke it), at `now`. Only the owner's line
/// answers (`owners`, as for a yes: `LAMP_PRIVATE_ROOM` on, any heard line),
/// and only a later line than the one that made it, within
/// [`CLASH_WITHIN`]. What the model is told.
pub fn chosen(
    id: &str,
    choice: Option<&str>,
    request: u64,
    heard: bool,
    owners: impl FnOnce() -> bool,
    now: Instant,
    hands: &Hands<'_>,
) -> Value {
    let mut held = WAITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(waiting) = held.as_ref().filter(|waiting| waiting.id == id) else {
        return json!({
            "error": "no skill waits for that choice now",
            "next": "Say plainly, in a few words, that nothing is waiting for that now. Call nothing."
        });
    };
    if now.saturating_duration_since(waiting.at) > CLASH_WITHIN {
        held.take();
        return json!({
            "error": "the choice came too late",
            "next": "Say plainly, in a few words, that it took too long and nothing was put on; \
                     they can ask for the skill again. Call nothing."
        });
    }
    let Some(choice) = choice.filter(|choice| CHOICES.contains(choice)) else {
        return json!({
            "error": "the choice is replace, keep or cancel",
            "next": "Ask them again, in a few words: replace it, keep both, or cancel."
        });
    };
    if !heard || request <= waiting.request || !owners() {
        eprintln!(
            "lamp-voice: request {request}: a made skill's clash wasn't answered: not the owner's own later line"
        );
        return json!({
            "error": "only the lamp's owner can choose, out loud",
            "next": "Say plainly, in a few words, that only the lamp's owner can choose that. \
                     Call nothing."
        });
    }
    let Some(waiting) = held.take() else {
        return Value::Null;
    };
    drop(held);
    let new = made_skill::spoken_name(&waiting.name);
    let other = made_skill::spoken_name(&waiting.other);
    let said = |line: String| made_skill::in_their_language(&line);
    let not_on = |why: String| {
        json!({
            "error": format!("not put on: {why}"),
            "next": said(made_skill::not_on_line(&why))
        })
    };
    match choice {
        "cancel" => {
            eprintln!(
                "lamp-voice: request {request}: a made skill's clash: cancelled, {} not put on",
                waiting.name
            );
            json!({
                "made": waiting.name,
                "on": false,
                "cancelled": true,
                "next": said(format!("OK, I didn't put {new} on. {other} stays as it was."))
            })
        }
        "replace" => {
            if let Err(why) = (hands.remove)(&waiting.keeping, &waiting.other) {
                return not_on(why);
            }
            made_skill::forget_named(&waiting.keeping.directory, &waiting.other);
            eprintln!(
                "lamp-voice: request {request}: a made skill's clash: {} removed for {}",
                waiting.other, waiting.name
            );
            match (hands.put_on)(&waiting.keeping, &waiting.sha256) {
                Ok(_) => json!({
                    "made": waiting.name,
                    "on": true,
                    "replaced": waiting.other,
                    "next": said(format!("Done: I removed {other}, and {new} is on."))
                }),
                Err(why) => not_on(why),
            }
        }
        _ => match (hands.put_on)(&waiting.keeping, &waiting.sha256) {
            Ok(_) => {
                eprintln!(
                    "lamp-voice: request {request}: a made skill's clash: {} kept beside {}",
                    waiting.name, waiting.other
                );
                json!({
                    "made": waiting.name,
                    "on": true,
                    "kept_both": true,
                    "next": said(format!(
                        "Done: both are on. When they start together, {new} wins over {other}."
                    ))
                })
            }
            Err(why) => not_on(why),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};

    /// The one waiting clash is the process's: these tests take turns.
    static TURNS: Mutex<()> = Mutex::new(());

    const NOW: u64 = 1_800_000_000_000;

    fn forget_waiting() {
        WAITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }

    /// A skill on the lamp named `name` on `trigger`, with one `step`.
    fn installed(keeping: &Keeping, name: &str, trigger: &str, step: &str) {
        let folder = keeping.installed.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            format!(
                "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n{trigger}\
                 cooldown = \"1h\"\n\n[[do]]\n{step}\n"
            ),
        )
        .unwrap();
    }

    /// The made test skill's trigger: the voice resting.
    const RESTING: &str = "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n";

    fn made(name: &str) -> Made {
        Made {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            sha256: "ab".repeat(32),
            description: String::new(),
            uses: Default::default(),
            package: PathBuf::from("/nonexistent"),
        }
    }

    /// Lamp-9adf, 12 Nov: a glow made on the trigger another glow on the
    /// lamp has. The made skill is kept but not put on (no helper is
    /// there to ask), and the model is told the other's spoken name and to
    /// ask replace, keep both or cancel.
    #[test]
    fn a_clash_is_found_and_nothing_is_put_on() {
        let _turn = TURNS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (root, keeping) = keeping("clash-found");
        installed(
            &keeping,
            "rest-glow",
            RESTING,
            "light = { colour = \"blue\" }",
        );
        let (url, served) = service(vec![(200, good())]);
        let told = crate::make_skill_local::make_with(
            3,
            Some("make me a skill that dims the light at 10 pm"),
            "dim the light at 10 pm",
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["clashes_with"], "rest glow", "{told}");
        assert_eq!(told["shares"], "the light", "{told}");
        assert!(told[CLASH_ID].is_string(), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("replace rest glow"), "{next}");
        assert!(next.contains("keep both (then evening dim wins)"), "{next}");
        assert!(next.contains("or cancel"), "{next}");
        assert_eq!(served.join().unwrap(), 1);
        let entries = made_skill::entries(&keeping.directory);
        assert_eq!(entries.len(), 1);
        assert_ne!(entries[0]["on"], true, "nothing put on");
        let line = crate::make_skill_local::take_said_line().unwrap();
        assert!(
            line.contains("didn't put it on: rest glow starts at the same moment"),
            "{line}"
        );
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No clash: another trigger, or the same trigger with no output both
    /// use. The made skill goes on as before.
    #[test]
    fn no_clash_on_another_trigger_or_no_shared_output() {
        let _turn = TURNS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (root, keeping) = keeping("clash-none");
        installed(
            &keeping,
            "arrival-glow",
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n",
            "light = { colour = \"blue\" }",
        );
        installed(
            &keeping,
            "rest-nod",
            RESTING,
            "body = { gesture = \"nod\" }",
        );
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let told = crate::make_skill_local::make_with(
            3,
            Some("make me a skill that dims the light at 10 pm"),
            "dim the light at 10 pm",
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(told["on"], true, "{told}");
        assert!(told.get(CLASH_ID).is_none(), "{told}");
        asked.join().unwrap();
        served.join().unwrap();
        let _ = crate::make_skill_local::take_said_line();
        let _ = std::fs::remove_dir_all(&root);

        let light = signature(&format!(
            "[trigger]\n{RESTING}[[do]]\nlight = {{ dim = 1 }}\n"
        ));
        let head = signature(&format!(
            "[trigger]\n{RESTING}[[do]]\nbody = {{ gesture = \"nod\" }}\n"
        ));
        let both = signature(&format!(
            "[[trigger]]\n{RESTING}[[do]]\nlight = {{ dim = 1 }}\nbody = {{ gesture = \"nod\" }}\n"
        ));
        let speaking = signature(&format!(
            "[trigger]\n{}[[do]]\nlight = {{ dim = 1 }}\n",
            RESTING.replace("resting", "speaking")
        ));
        assert_eq!(shared(&light, &head), None);
        assert_eq!(shared(&light, &speaking), None);
        assert_eq!(shared(&light, &both), Some("the light"));
        assert_eq!(shared(&head, &both), Some("the head"));
        assert_eq!(shared(&both, &both), Some("the light and the head"));
        // Its pace alone differs: the same trigger.
        let paced = signature(&format!(
            "[trigger]\n{RESTING}cooldown = \"12h\"\nfor = \"5s\"\n[[do]]\nlight = {{ dim = 1 }}\n"
        ));
        assert_eq!(shared(&light, &paced), Some("the light"));
    }

    /// The person's choice: only the owner's own later line; replace takes
    /// the other off, then puts the new one on; keep puts it on beside the
    /// other; cancel does nothing.
    #[test]
    fn replace_keep_and_cancel_each_do_what_they_say() {
        let _turn = TURNS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (root, keeping) = keeping("clash-choice");
        let done: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let taken =
            |done: &Mutex<Vec<String>>| -> Vec<String> { done.lock().unwrap().drain(..).collect() };
        let remove = |_: &Keeping, name: &str| {
            done.lock().unwrap().push(format!("remove {name}"));
            Ok(())
        };
        let put_on = |_: &Keeping, sha256: &str| {
            done.lock().unwrap().push(format!("on {}", &sha256[..4]));
            Ok("glow-green".to_owned())
        };
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
        };
        let clash = Clash {
            name: "glow-blue-on-return".to_owned(),
            shares: "the light",
        };
        let now = Instant::now();
        let ask = || asked(&keeping, &made("glow-green"), &clash, 3, now);
        let id = |told: &Value| told[CLASH_ID].as_str().unwrap().to_owned();

        // Not the owner's, the same line, or no choice: nothing done, and
        // it still waits.
        let first = id(&ask());
        let refused = chosen(&first, Some("replace"), 4, true, || false, now, &hands);
        assert!(
            refused["error"].as_str().unwrap().contains("owner"),
            "{refused}"
        );
        let refused = chosen(&first, Some("replace"), 3, true, || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        let refused = chosen(&first, Some("replace"), 4, false, || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        let refused = chosen(&first, Some("maybe"), 4, true, || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        assert!(taken(&done).is_empty());

        let replaced = chosen(&first, Some("replace"), 4, true, || true, now, &hands);
        assert_eq!(replaced["on"], true, "{replaced}");
        assert_eq!(replaced["replaced"], "glow-blue-on-return", "{replaced}");
        assert_eq!(taken(&done), ["remove glow-blue-on-return", "on abab"]);
        // Answered once: asked again, nothing waits.
        let again = chosen(&first, Some("keep"), 5, true, || true, now, &hands);
        assert!(again["error"].is_string(), "{again}");
        assert!(taken(&done).is_empty());

        let kept = chosen(&id(&ask()), Some("keep"), 4, true, || true, now, &hands);
        assert_eq!(kept["kept_both"], true, "{kept}");
        assert!(
            kept["next"]
                .as_str()
                .unwrap()
                .contains("glow green wins over glow blue on return"),
            "{kept}"
        );
        assert_eq!(taken(&done), ["on abab"]);

        let cancelled = chosen(&id(&ask()), Some("cancel"), 4, true, || true, now, &hands);
        assert_eq!(cancelled["cancelled"], true, "{cancelled}");
        assert_eq!(cancelled["on"], false, "{cancelled}");
        assert!(taken(&done).is_empty());

        // Too late: nothing done.
        let late = chosen(
            &id(&ask()),
            Some("keep"),
            4,
            true,
            || true,
            now + CLASH_WITHIN + Duration::from_secs(1),
            &hands,
        );
        assert!(late["error"].is_string(), "{late}");
        assert!(taken(&done).is_empty());
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }
}
