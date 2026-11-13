//! A skill the person made, removed by voice (#6960, Twelve's three tries
//! on lamp-c0a4, 12843: "delete the skill called moon fact" went to
//! `lamp_help`, and "remove the moon fact skill" to a read-back that found
//! no "moon-fax", since `make_skill` named no remove and a read-back takes
//! only the exact name).
//!
//! - **One plain path, the model's to choose.** `make_skill` takes
//!   [`REMOVE`], the name as heard, given alone: whether a line asks to
//!   remove a skill is the model's judgment, never a list of words (the
//!   owner's ruling, 13 Nov). It is a parameter of the function the voice
//!   has, not a function of its own: the voice is at its 48.
//! - **Only the person's own.** The name is matched in code against the
//!   skills they made (the made skills' index, `made.json`) alone
//!   ([`found`]): its words as a skill's name is written, each number word
//!   as digits, then the spelling distance over the name's own words
//!   ([`apart`]), near enough at a quarter of its letters ([`near`]), the
//!   nearest one alone; several as near, or a near longer one holding the
//!   nearest's words, are asked which. A skill on the lamp they didn't
//!   make (core, from the Store, or pushed from a computer) is refused in
//!   words ([`NOT_MADE`]); the manage helper refuses a core one too. None
//!   near is said so. A name not the one asked, and theirs (three at most),
//!   are said only to the owner, as a read-back does: a name can carry a
//!   person's. While another yes waits, only the owner's remove asks.
//! - **Asked once, removed on the yes.** One short question by its spoken
//!   name, "Remove moon fact?" ([`question`]), and the yes waits as a made
//!   skill's does (`crate::skill_write_yes`, with every guard there: the
//!   question said whole and uncut first, a later heard line, the echo
//!   held, the route with the echo judge, the owner's yes only,
//!   `make_skill_local::not_owners_yes_refused`). The person's next line
//!   is made to call `make_skill` with its `approve_id` and their
//!   `answer` (`skill_write_yes::approve_force`); yes, no or unclear is the
//!   model's. On the yes, one that is on, its folder on the lamp the very
//!   package made, is taken off by the manage helper's own
//!   `{"remove": NAME}`, as the app's remove does, then it is dropped from
//!   the made skills with its package ([`removed`]). A no
//!   keeps it; an unclear answer asks once more, and a second keeps it.
//!
//! No extra round trip: the match reads two local folders, and only the
//! yes asks the helper, over its local socket.
use crate::made_skill::{self, Keeping};
use crate::skill_write_yes::{self, Asks};
use serde_json::{Value, json};
use std::time::Instant;

/// The `make_skill` argument that asks to remove a skill: its name as
/// heard, given alone.
pub const REMOVE: &str = "remove";

/// What the model is told of a skill on the lamp the person didn't make.
pub const NOT_MADE: &str = "only a skill the person made is removed by voice";
/// What the model is told when no skill they made is near the name asked.
pub const NO_MADE: &str = "no skill the person made has a name like that";
/// What the model is told of a remove asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str = "a skill is removed only when the person asks for it out loud";
/// What the model is told of a remove asked where no answer can be taken
/// (the Live route has no echo judge).
pub const NOT_HERE: &str = "this voice can't take the answer to a remove";

/// What the model is told of a remove asked by anyone but the owner while
/// another yes waits: nothing is asked, and that wait goes on.
pub const ANOTHER_WAITS: &str = "another answer is awaited from the lamp's owner";

/// The most of their skills' names said when none is near.
const NAMED_MOST: usize = 3;

/// A remove asked by the model's `make_skill` call: the name as heard, in
/// the voice's request `request` (`heard`: someone spoke it), on the route
/// with the echo judge or not (`sends_here`, `services::sends_here`).
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    pub name: &'a str,
    pub request: u64,
    pub heard: bool,
    pub sends_here: bool,
}

/// What a remove's name finds on the lamp ([`found`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// One skill the person made: its name and package's SHA-256, and
    /// whether the words asked were its own name (`exact`) or only nearest.
    Made {
        name: String,
        sha256: String,
        exact: bool,
    },
    /// More than one of theirs as near: their names.
    Alike(Vec<String>),
    /// A skill on the lamp the person didn't make: its name, and whether
    /// the words asked were its own name (`exact`) or only near it.
    NotMade { name: String, exact: bool },
    /// None near.
    Nothing,
}

/// A name's words as a skill's name is written ([`made_skill::name_asked`]:
/// lower case, no "my" before it or "skill" after it), each number word as
/// digits ([`made_skill::name_digits`]).
fn words_of(name: &str) -> Vec<String> {
    made_skill::name_digits(&made_skill::name_asked(name))
        .split('-')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The edit distance between two spellings, by letter.
fn distance(one: &str, other: &str) -> usize {
    let other: Vec<char> = other.chars().collect();
    let mut row: Vec<usize> = (0..=other.len()).collect();
    for (at, letter) in one.chars().enumerate() {
        let mut before = row[0];
        row[0] = at + 1;
        for (next, theirs) in other.iter().enumerate() {
            let here = row[next + 1];
            row[next + 1] = if letter == *theirs {
                before
            } else {
                1 + before.min(here).min(row[next])
            };
            before = here;
        }
    }
    row[other.len()]
}

/// How far the words `asked` are from the skill `name`: the fewest letters
/// changed, put in or left out, over the whole of what was asked, or over
/// any run of its words as many as the name has ("the moon fact" holds
/// "moon fact"), with the spaces between words left out. Spelling only:
/// no word is judged by what it means.
pub fn apart(asked: &str, name: &str) -> usize {
    let asked = words_of(asked);
    let name = words_of(name);
    let wanted = name.concat();
    let whole = distance(&asked.concat(), &wanted);
    asked
        .windows(name.len().max(1))
        .map(|run| distance(&run.concat(), &wanted))
        .fold(whole, usize::min)
}

/// Whether `apart` letters from the skill `name` is near enough: at most a
/// quarter of its letters (a short name only as it is written).
pub fn near(apart: usize, name: &str) -> bool {
    apart * 4 <= words_of(name).concat().chars().count()
}

/// The person's made skills (the made skills' index), each name once with
/// its package's SHA-256, the newest kept for a name.
fn made(keeping: &Keeping) -> Vec<(String, String)> {
    let mut made: Vec<(String, String)> = Vec::new();
    for entry in made_skill::entries(&keeping.directory) {
        let (Some(name), Some(sha256)) = (entry["name"].as_str(), entry["sha256"].as_str()) else {
            continue;
        };
        made.retain(|(kept, _)| kept != name);
        made.push((name.to_owned(), sha256.to_owned()));
    }
    made
}

/// The skills on the lamp the person didn't make: the skills folder's and
/// the pushed folder's, by folder, but the made ones' names.
fn others(keeping: &Keeping, made: &[(String, String)]) -> Vec<String> {
    let mut others: Vec<String> = [
        keeping.installed.clone(),
        made_skill::pushed_folder(&keeping.installed),
    ]
    .iter()
    .flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten())
    .filter(|entry| entry.path().is_dir())
    .filter_map(|entry| entry.file_name().into_string().ok())
    .filter(|name| !name.starts_with('.'))
    .filter(|name| !made.iter().any(|(made, _)| made == name))
    .collect();
    others.sort();
    others.dedup();
    others
}

/// Whether every word of `short` lies inside `long`'s words, in a row
/// ("moon" in "moon fact"), and `long` has more.
fn inside(short: &str, long: &str) -> bool {
    let (short, long) = (words_of(short), words_of(long));
    short.len() < long.len() && long.windows(short.len().max(1)).any(|run| run == short)
}

/// The skill the words `asked` name for a remove: the person's own made
/// skill nearest to them ([`apart`], [`near`]), alone. Several as near, by
/// distance alone, whatever their length, are asked which ([`Found::Alike`],
/// Heron's and Thirteen's P2s on #6969); so is a near longer one whose
/// words hold the nearest's ("moon fact" beside "moon": "the moon fax
/// skill" is 0 from moon and 2 from moon fact), unless that longer one is
/// named as it is written too ("the moon fact skill" is moon fact). One
/// they didn't make when it is named as it is written, or when none of
/// theirs is near and it is; else nothing.
pub fn found(asked: &str, keeping: &Keeping) -> Found {
    let made = made(keeping);
    let theirs: Vec<(usize, &str, &str)> = made
        .iter()
        .map(|(name, sha256)| (apart(asked, name), name.as_str(), sha256.as_str()))
        .filter(|(apart, name, _)| near(*apart, name))
        .collect();
    let others = others(keeping, &made);
    let other = |exact: bool| {
        others
            .iter()
            .map(|name| (apart(asked, name), name))
            .filter(|(apart, name)| {
                if exact {
                    *apart == 0
                } else {
                    near(*apart, name)
                }
            })
            .min_by_key(|(apart, name)| (*apart, std::cmp::Reverse(name.len())))
            .map(|(apart, name)| Found::NotMade {
                name: name.clone(),
                exact: apart == 0,
            })
    };
    let Some(best) = theirs.iter().map(|(apart, ..)| *apart).min() else {
        return other(false).unwrap_or(Found::Nothing);
    };
    if best > 0
        && let Some(named) = other(true)
    {
        return named;
    }
    // The nearest, then each near one that holds a nearest one's words.
    let mut tied: Vec<(usize, &str, &str)> = theirs
        .iter()
        .filter(|(apart, ..)| *apart == best)
        .copied()
        .collect();
    for candidate in &theirs {
        if !tied.contains(candidate) && tied.iter().any(|(_, short, _)| inside(short, candidate.1))
        {
            tied.push(*candidate);
        }
    }
    // A name inside another named as it is written is that other's part.
    let whole: Vec<&str> = tied
        .iter()
        .filter(|(apart, ..)| *apart == 0)
        .map(|(_, name, _)| *name)
        .collect();
    tied.retain(|(_, name, _)| !whole.iter().any(|long| inside(name, long)));
    tied.sort_by_key(|(_, name, _)| (name.len(), *name));
    match tied.as_slice() {
        [(apart, name, sha256)] => Found::Made {
            name: (*name).to_owned(),
            sha256: (*sha256).to_owned(),
            exact: *apart == 0,
        },
        _ => Found::Alike(tied.iter().map(|(_, name, _)| (*name).to_owned()).collect()),
    }
}

/// The one question a remove asks, by the skill's spoken name.
pub fn question(spoken: &str) -> String {
    format!("Remove {spoken}?")
}

/// The spoken names `names` as said in a line: "a", "a or b", "a, b or c".
fn said_or(names: &[String]) -> String {
    let spoken: Vec<String> = names
        .iter()
        .map(|name| made_skill::spoken_name(name))
        .collect();
    match spoken.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// What the model is told after the question: wait for the answer, whose
/// call the person's next line makes (`skill_write_yes::approve_force`).
pub fn then_wait(id: &str) -> String {
    format!(
        "End there, asking nothing else and calling nothing now: nothing is removed until they \
         agree. When they answer, call make_skill with only approve_id \"{id}\" and answer \
         \"yes\" if they agree, \"no\" if they decline or wait, \"unclear\" if you can't tell."
    )
}

/// What the model is told to say first: the question, word for word.
pub fn say_question(question: &str) -> String {
    format!(
        "Say only this, word for word: \"{question}\" If the conversation is in another \
         language, say it in that language too, after it."
    )
}

/// A remove asked in `ask` at `now`, of the skills in `keeping`: the
/// person's own made skill [`found`] by the name asked is asked about once
/// ([`question`]) and its yes awaited
/// ([`skill_write_yes::wait_for_yes`]); anything else is said in words and
/// nothing waits. A name not the very one asked, or their skills' names,
/// are said only when the owner hears (`owner_hears`, asked only then, as
/// a read-back's names are). Nothing is asked unheard, or on a route that
/// can't take the answer. What the model is told.
pub fn asked(
    ask: Ask<'_>,
    keeping: &Keeping,
    owner_hears: impl FnOnce() -> bool,
    now: Instant,
) -> Value {
    let Ask {
        name,
        request,
        heard,
        sends_here,
    } = ask;
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't removed: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    let found = found(name, keeping);
    // Only the kind found, never the name asked: the journal holds no
    // person's words.
    eprintln!(
        "lamp-voice: request {request}: a remove asked by voice found {}",
        match &found {
            Found::Made { exact: true, .. } => "one made skill by its name",
            Found::Made { exact: false, .. } => "one made skill by the nearest name",
            Found::Alike(_) => "more than one made skill as near",
            Found::NotMade { .. } => "a skill the person didn't make",
            Found::Nothing => "nothing near",
        }
    );
    // Asked once at most, and only when a name would be said.
    let mut owner_hears = Some(owner_hears);
    let mut hears = None;
    let mut owner = || *hears.get_or_insert_with(|| owner_hears.take().is_some_and(|ask| ask()));
    match found {
        // Named back only as asked, or to the owner (Heron's P3 2 on #6969).
        Found::NotMade { name: other, exact } if exact || owner() => json!({
            "error": NOT_MADE,
            "next": format!(
                "Say plainly, in a few words, that {} isn't a skill they made, so you can't \
                 remove it by voice. Call nothing.",
                made_skill::spoken_name(&other)
            )
        }),
        Found::Made {
            name,
            sha256,
            exact,
        } if exact || owner() => {
            let spoken = made_skill::spoken_name(&name);
            if !sends_here {
                return json!({
                    "error": NOT_HERE,
                    "next": format!(
                        "Say plainly, in a few words, that I can remove {spoken} only on my \
                         other voice, in a normal conversation with me. Call nothing."
                    )
                });
            }
            // Another yes waits (one wait at a time): only the owner's
            // remove may replace it, so a guest's never drops the owner's
            // (Heron's P3 1 on #6969). With nothing waiting, nothing is
            // asked of whose line it was: the yes is the owner's only.
            if skill_write_yes::awaited(now) && !owner() {
                eprintln!(
                    "lamp-voice: request {request}: a remove asked while another yes waits, not the owner's: nothing asked"
                );
                return json!({
                    "error": ANOTHER_WAITS,
                    "next": "Say plainly, in a few words, that I can't do that right now, while I \
                             wait for the lamp's owner's answer. Call nothing."
                });
            }
            let question = question(&spoken);
            let id = skill_write_yes::wait_for_yes(
                Asks {
                    skill: name.clone(),
                    sha256,
                    writes: Vec::new(),
                    say_back: question.clone(),
                    grants: None,
                    code: false,
                    remove: true,
                },
                request,
                now,
            );
            json!({
                REMOVE: name,
                skill_write_yes::APPROVE_ID: id,
                "next": format!("{} {}", say_question(&question), then_wait(&id)),
            })
        }
        Found::Alike(names) if owner() => json!({
            "error": "more than one skill they made has a name like that",
            "next": format!(
                "Ask them once, short, which one they mean: {}. Call nothing now.",
                said_or(&names[..names.len().min(NAMED_MOST)])
            )
        }),
        _ if owner() => {
            let theirs: Vec<String> = made(keeping).into_iter().map(|(name, _)| name).collect();
            let named = match theirs.len() {
                0 => "and that they haven't made any yet".to_owned(),
                count if count <= NAMED_MOST => format!(
                    "and name theirs: {}",
                    theirs
                        .iter()
                        .map(|name| made_skill::spoken_name(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                count => format!("and that they have made {count}"),
            };
            json!({
                "error": NO_MADE,
                "next": format!(
                    "Say plainly, in a few words, that you found no skill they made by that \
                     name, {named}. Call nothing."
                )
            })
        }
        _ => json!({
            "error": NO_MADE,
            "next": "Say plainly, in a few words, that there is no skill they made by that \
                     name. Name no other skill. Call nothing."
        }),
    }
}

/// [`asked`] on the lamp, by `make_skill`'s call: the made skills' own
/// folder (`LAMP_MAKE_SKILL`), the owner's hearing as a read-back's
/// (`services::owner_hears`), the route's echo judge.
pub fn asked_here(
    name: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    asked(
        Ask {
            name,
            request,
            heard,
            sends_here: crate::services::sends_here(),
        },
        &keeping,
        || crate::services::owner_hears(request, heard, lease),
        Instant::now(),
    )
}

/// What the folder `name` in the skills folder went on as: the SHA-256 of
/// its package (`.package.json`, written by the manage helper's install);
/// `Ok(None)` when no such folder is there, `Err` when it is there but says
/// no package.
fn went_on_as(keeping: &Keeping, name: &str) -> Result<Option<String>, ()> {
    let folder = keeping.installed.join(name);
    if !folder.is_dir() {
        return Ok(None);
    }
    std::fs::read(folder.join(".package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|record| record["sha256"].as_str().map(str::to_owned))
        .map(Some)
        .ok_or(())
}

/// The person's yes to the remove `asks` waited for: the made skill of
/// that name and SHA-256, if the lamp still keeps it, taken off by
/// `remove` (the manage helper's own remove) when it is on and its folder
/// on the lamp went on as that very package (Thirteen's P3 1 on #6969: a
/// Store or pushed skill may have come in under the name since the made one
/// was taken off in the app; refused in words, nothing changed); its
/// folder already gone, the helper is not asked. Then it is dropped from
/// the made skills with its package ([`made_skill::forget_named_checked`]).
/// `{"removed": NAME}`; taken off but still listed when the index could
/// not be written, said so (`"still_listed"`, its own `next`, Thirteen's
/// P3 4); or why not.
pub fn removed(
    keeping: &Keeping,
    asks: &Asks,
    remove: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Value, String> {
    let Some(entry) = made_skill::entries(&keeping.directory)
        .into_iter()
        .find(|entry| {
            entry["name"] == asks.skill.as_str() && entry["sha256"] == asks.sha256.as_str()
        })
    else {
        return Err("the lamp no longer has it".into());
    };
    let spoken = made_skill::spoken_name(&asks.skill);
    if entry["on"] == true {
        match went_on_as(keeping, &asks.skill) {
            Ok(Some(sha256)) if sha256 == asks.sha256 => remove(&asks.skill)?,
            Ok(None) => eprintln!(
                "lamp-voice: a made skill to remove by voice is no longer on the lamp: only dropped from the made skills"
            ),
            _ => {
                eprintln!(
                    "lamp-voice: a made skill wasn't removed by voice: the folder of its name on the lamp is not the package made"
                );
                return Err(format!(
                    "the skill called {spoken} on the lamp now isn't the one they made"
                ));
            }
        }
    }
    if let Err(why) = made_skill::forget_named_checked(&keeping.directory, &asks.skill) {
        eprintln!(
            "lamp-voice: a made skill was taken off the lamp by voice, but is still listed among the made skills: {why}"
        );
        return Ok(json!({
            "removed": asks.skill,
            "still_listed": true,
            "next": format!(
                "Say only, in a few words, in the conversation's language, that {spoken} is off \
                 the lamp, but still listed among the skills they made, and that they can ask \
                 me again. Ask nothing, call nothing."
            )
        }));
    }
    eprintln!("lamp-voice: a made skill was removed by voice, on the person's yes");
    Ok(json!({ "removed": asks.skill }))
}

/// [`removed`] on the lamp: through the manage helper's socket.
pub fn removed_here(asks: &Asks) -> Result<Value, String> {
    let keeping = Keeping::when(true).ok_or("the lamp can't make skills")?;
    removed(&keeping, asks, |name| {
        crate::made_clash::remove_by_helper(&keeping.manage, name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::keeping;
    use crate::skill_write_yes as yes;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    /// The person's made skills kept in `keeping`'s index, each by its name
    /// and whether it is on (an on one has its folder in the skills folder,
    /// as the manage helper put it there).
    fn made_here(keeping: &crate::made_skill::Keeping, made: &[(&str, bool)]) {
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let entries: Vec<Value> = made
            .iter()
            .enumerate()
            .map(|(at, (name, on))| {
                let package = format!("{name}-1.0.0.tar");
                std::fs::write(keeping.directory.join(&package), "tar").unwrap();
                let sha256 = format!("{at:x}").repeat(64);
                if *on {
                    installed(keeping, name, "installable");
                    went_on_as(keeping, name, &sha256);
                }
                json!({
                    "name": name,
                    "version": "1.0.0",
                    "sha256": sha256,
                    "description": "",
                    "package": package,
                    "on": on,
                    "from": format!("local:{name}"),
                })
            })
            .collect();
        std::fs::write(
            keeping.directory.join(crate::made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
    }

    /// A skill folder on the lamp named `name`, of `kind`.
    fn installed(keeping: &crate::made_skill::Keeping, name: &str, kind: &str) {
        let folder = keeping.installed.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            format!("kind = \"{kind}\"\nversion = \"1.0.0\"\n"),
        )
        .unwrap();
    }

    /// The folder `name` on the lamp went on as the package `sha256`, as
    /// the manage helper's install writes it (`.package.json`).
    fn went_on_as(keeping: &crate::made_skill::Keeping, name: &str, sha256: &str) {
        std::fs::write(
            keeping.installed.join(name).join(".package.json"),
            json!({"sha256": sha256, "manifest": {"name": name, "version": "1.0.0"}}).to_string(),
        )
        .unwrap();
    }

    fn names(directory: &Path) -> Vec<String> {
        crate::made_skill::entries(directory)
            .iter()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// Twelve's lamp: moon-fact made by voice and on, evening-dim beside
    /// it, faq (core) and look-for-things (from the Store) on the lamp.
    fn twelves_lamp(test: &str) -> (PathBuf, crate::made_skill::Keeping) {
        yes::clear_for_test();
        let (root, keeping) = keeping(test);
        made_here(&keeping, &[("moon-fact", true), ("evening-dim", true)]);
        installed(&keeping, "faq", "core");
        installed(&keeping, "look-for-things", "installable");
        (root, keeping)
    }

    fn ask(name: &str, request: u64) -> Ask<'_> {
        Ask {
            name,
            request,
            heard: true,
            sends_here: true,
        }
    }

    fn owner() -> bool {
        true
    }

    /// The id the remove's ask waits on.
    fn id_of(told: &Value) -> String {
        told[yes::APPROVE_ID]
            .as_str()
            .unwrap_or_else(|| panic!("no wait: {told}"))
            .to_owned()
    }

    /// Twelve's three lines on lamp-c0a4 (#6960), as the model passes the
    /// name it heard to `make_skill`'s `remove`: "moon fact", the
    /// speech-to-text's "moon-fax", and "the moon facts skill, the one you
    /// made for me". Each finds the person's own moon-fact and asks once,
    /// by its spoken name, "Remove moon fact?"; nothing is removed yet.
    #[test]
    fn twelves_three_lines_each_ask_remove_moon_fact() {
        for heard in ["moon fact", "moon-fax", "the moon facts skill"] {
            let (root, keeping) = twelves_lamp("remove-three-lines");
            let now = Instant::now();
            let told = asked(ask(heard, 1), &keeping, owner, now);
            assert_eq!(told["remove"], "moon-fact", "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("\"Remove moon fact?\""), "{heard}: {next}");
            assert!(yes::waits(&id_of(&told)), "{heard}");
            assert!(yes::awaited(now));
            assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// `make_skill` is offered for a remove (line 1 went to `lamp_help`,
    /// lines 2 and 3 to a read-back: nothing in `make_skill` named one), as
    /// a parameter of the function the voice has, never a new function
    /// (the voice is at its 48), and within the link's bounds with every
    /// switch on.
    #[test]
    fn make_skill_is_offered_for_a_remove_within_the_bounds() {
        crate::made_skill::set_writes_here(Some(true));
        crate::made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        let tool = crate::make_skill_local::tool();
        assert!(tool.description.contains("remove"), "{}", tool.description);
        assert!(tool.description.contains("delete"), "{}", tool.description);
        assert!(
            tool.description.len() <= 1_024,
            "{}",
            tool.description.len()
        );
        assert!(!tool.description.contains("yes"), "{}", tool.description);
        let parameters = tool.parameters.unwrap();
        assert!(parameters["properties"][REMOVE].is_object(), "{parameters}");
        assert!(parameters.to_string().len() <= 4_096);
        // Every switch off: still offered, as `make_skill` is.
        crate::skill_task_yes::set_on_here(Some(false));
        crate::made_skill::set_scripts_here(Some(false));
        crate::made_skill::set_writes_here(Some(false));
        let off = crate::make_skill_local::tool();
        assert!(off.description.contains("remove"), "{}", off.description);
        assert!(off.parameters.unwrap()["properties"][REMOVE].is_object());
        crate::skill_task_yes::set_on_here(None);
        crate::made_skill::set_scripts_here(None);
        crate::made_skill::set_writes_here(None);
    }

    /// The question said whole, the owner's yes in a later line removes
    /// moon-fact through the manage helper's own remove and drops it from
    /// the made skills; evening-dim stays.
    #[test]
    fn the_yes_removes_it_through_the_helper() {
        let (root, keeping) = twelves_lamp("remove-yes");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon-fax", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        assert!(yes::approve_forced_now(now), "the next line must answer it");
        assert_eq!(yes::answered(&id, 2, Some("yes")), None);
        assert_eq!(
            crate::make_skill_local::not_owners_yes_refused(&id, || true),
            None
        );
        let mut helper = Vec::new();
        let told = yes::approve(&id, 2, true, true, now, |asks| {
            removed(&keeping, asks, |name| {
                helper.push(name.to_owned());
                Ok(())
            })
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["removed"], "moon-fact", "{told}");
        assert!(
            told["next"].as_str().unwrap().contains("moon fact"),
            "{told}"
        );
        assert_eq!(helper, ["moon-fact"]);
        assert_eq!(names(&keeping.directory), ["evening-dim"]);
        assert!(!yes::awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A no keeps it, and the wait ends; an unclear answer asks once more by
    /// its name, and a second unclear keeps it too. Nothing is asked of the
    /// helper either way.
    #[test]
    fn a_no_or_twice_unclear_keeps_it() {
        let (root, keeping) = twelves_lamp("remove-no");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::answered(&id, 2, Some("no")).unwrap();
        assert_eq!(told["approved"], false);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon fact stays"), "{next}");
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);

        let id = id_of(&asked(ask("moon fact", 3), &keeping, owner, now));
        yes::lamp_said(3, "Remove moon fact?");
        let once = yes::answered(&id, 4, Some("unclear")).unwrap();
        assert!(
            once["next"].as_str().unwrap().contains("remove moon fact"),
            "{once}"
        );
        assert!(yes::awaited(now));
        let again = yes::answered(&id, 5, None).unwrap();
        assert!(again["next"].as_str().unwrap().contains("moon fact stays"));
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A visitor's yes removes nothing: the wait is dropped, as every made
    /// skill's yes is the owner's only.
    #[test]
    fn a_visitors_yes_removes_nothing() {
        let (root, keeping) = twelves_lamp("remove-visitor");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let refused = crate::make_skill_local::not_owners_yes_refused(&id, || false).unwrap();
        assert!(
            refused["error"].as_str().unwrap().contains("Not removed"),
            "{refused}"
        );
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A yes before the question was heard whole, or in its own line,
    /// removes nothing.
    #[test]
    fn a_yes_before_the_question_was_said_removes_nothing() {
        let (root, keeping) = twelves_lamp("remove-unsaid");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        let never = |_: &yes::Asks| -> Result<Value, String> { panic!("removed") };
        let told = yes::approve(&id, 2, true, true, now, never);
        assert!(told["error"].is_string(), "{told}");
        assert!(told["next"].as_str().unwrap().contains("Remove moon fact?"));
        assert!(yes::waits(&id));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::approve(&id, 1, true, true, now, never);
        assert!(told["error"].is_string(), "{told}");
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A core skill's name, or a Store skill's, is refused in words: it
    /// isn't one the person made, so it is never removed by voice, and
    /// nothing waits.
    #[test]
    fn a_core_or_store_skill_is_refused_in_words() {
        let (root, keeping) = twelves_lamp("remove-core");
        let now = Instant::now();
        for (heard, spoken) in [("faq", "faq"), ("look for things", "look for things")] {
            let told = asked(ask(heard, 1), &keeping, owner, now);
            assert_eq!(told["error"], NOT_MADE, "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(
                next.contains(&format!("{spoken} isn't a skill they made")),
                "{next}"
            );
            assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
            assert!(!yes::awaited(now));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No skill they made near the name: said so, naming theirs (two here)
    /// to the owner only; a near name is named to the owner only too, and
    /// the exact one asks without asking whose line it was.
    #[test]
    fn none_near_is_said_and_names_are_told_the_owner_only() {
        let (root, keeping) = twelves_lamp("remove-none");
        let now = Instant::now();
        let told = asked(ask("weather report", 1), &keeping, owner, now);
        assert_eq!(told["error"], NO_MADE, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("moon fact") && next.contains("evening dim"),
            "{next}"
        );
        assert!(!yes::awaited(now));
        for heard in ["weather report", "moon-fax"] {
            let told = asked(ask(heard, 1), &keeping, || false, now);
            assert_eq!(told["error"], NO_MADE, "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(!next.contains("moon fact"), "{heard}: {next}");
            assert!(!yes::awaited(now));
        }
        let told = asked(ask("moon fact", 1), &keeping, || panic!("asked whose"), now);
        assert_eq!(told["remove"], "moon-fact", "{told}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two of theirs as near: asked once which, short, and nothing waits.
    #[test]
    fn two_as_near_ask_which() {
        yes::clear_for_test();
        let (root, keeping) = keeping("remove-alike");
        made_here(&keeping, &[("moon-fact", true), ("noon-fact", false)]);
        let now = Instant::now();
        let told = asked(ask("loon fact", 1), &keeping, owner, now);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon fact or noon fact"), "{next}");
        assert!(!yes::awaited(now));
        // One made and off is removed from the made skills alone: the
        // helper has nothing to take off.
        let id = id_of(&asked(ask("noon fact", 2), &keeping, owner, now));
        yes::lamp_said(2, "Remove noon fact?");
        let told = yes::approve(&id, 3, true, true, now, |asks| {
            removed(&keeping, asks, |_| panic!("asked the helper"))
        });
        assert_eq!(told["removed"], "noon-fact", "{told}");
        assert_eq!(names(&keeping.directory), ["moon-fact"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Unheard, or on the Live route (no echo judge to take the answer),
    /// nothing is asked and nothing waits.
    #[test]
    fn nothing_is_asked_unheard_or_on_the_live_route() {
        let (root, keeping) = twelves_lamp("remove-live");
        let now = Instant::now();
        let mut unheard = ask("moon fact", 1);
        unheard.heard = false;
        assert!(asked(unheard, &keeping, owner, now)["error"].is_string());
        let mut live = ask("moon fact", 1);
        live.sends_here = false;
        let told = asked(live, &keeping, owner, now);
        assert!(told["error"].is_string(), "{told}");
        assert!(
            told["next"].as_str().unwrap().contains("other voice"),
            "{told}"
        );
        assert!(!yes::awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The nearest name is a spelling distance over the name's own words,
    /// as a skill's name is written, never a list of words.
    #[test]
    fn nearest_is_spelling_over_the_names_words() {
        assert_eq!(apart("moon fact", "moon-fact"), 0);
        assert_eq!(apart("My Moon Fact skill", "moon-fact"), 0);
        assert_eq!(apart("the moon fact", "moon-fact"), 0);
        assert_eq!(apart("moon-fax", "moon-fact"), 2);
        assert_eq!(apart("the moon facts skill", "moon-fact"), 1);
        assert_eq!(apart("lamp test email two", "lamp-test-email-2"), 0);
        assert!(near(2, "moon-fact"));
        assert!(!near(3, "moon-fact"));
        assert!(!near(1, "dim"));
    }

    /// Heron's and Thirteen's P2 on #6969: two of theirs equally near but
    /// of other lengths ("brain alert" is 1 from both) ask which, never
    /// the shorter or the longer alone.
    #[test]
    fn two_as_near_of_other_lengths_ask_which() {
        yes::clear_for_test();
        let (root, keeping) = keeping("remove-alike-lengths");
        made_here(&keeping, &[("rain-alert", true), ("train-alert", true)]);
        let now = Instant::now();
        let told = asked(ask("the brain alert skill", 1), &keeping, owner, now);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("rain alert or train alert"), "{next}");
        assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
        assert!(!yes::awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's and Thirteen's P2 on #6969: a shorter made name whose words
    /// lie inside a near longer one ("moon" in "moon fact") is as near as
    /// it, so "the moon fax skill" asks which; "moon fact" said whole is
    /// moon fact alone.
    #[test]
    fn a_shorter_name_inside_the_longer_one_meant_asks_which() {
        yes::clear_for_test();
        let (root, keeping) = keeping("remove-inside");
        made_here(&keeping, &[("moon", true), ("moon-fact", true)]);
        let now = Instant::now();
        let told = asked(ask("the moon fax skill", 1), &keeping, owner, now);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon or moon fact"), "{next}");
        assert!(!yes::awaited(now));
        let told = asked(ask("the moon fact skill", 1), &keeping, owner, now);
        assert_eq!(told["remove"], "moon-fact", "{told}");
        let told = asked(ask("moon", 2), &keeping, owner, now);
        assert_eq!(told["remove"], "moon", "{told}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 2 on #6969: a skill they didn't make is named back only
    /// as it was asked, or to the owner; to anyone else a near name is
    /// "no skill they made by that name".
    #[test]
    fn a_near_name_not_theirs_is_said_to_the_owner_only() {
        let (root, keeping) = twelves_lamp("remove-near-not-theirs");
        let now = Instant::now();
        let told = asked(ask("look for thing", 1), &keeping, || false, now);
        assert_eq!(told["error"], NO_MADE, "{told}");
        assert!(!told["next"].as_str().unwrap().contains("look for things"));
        let told = asked(ask("look for thing", 1), &keeping, owner, now);
        assert_eq!(told["error"], NOT_MADE, "{told}");
        let told = asked(ask("faq", 1), &keeping, || false, now);
        assert_eq!(told["error"], NOT_MADE, "{told}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 1 on #6969: while another yes waits, a remove asked by
    /// anyone but the owner opens nothing, so it never replaces that wait;
    /// the owner's does, one wait at a time.
    #[test]
    fn a_guests_remove_never_replaces_a_waiting_yes() {
        let (root, keeping) = twelves_lamp("remove-guest");
        let now = Instant::now();
        let other = yes::wait_for_yes(
            yes::Asks {
                skill: "leaving-mail".to_owned(),
                sha256: "ab".repeat(32),
                writes: Vec::new(),
                say_back: "mails mai".to_owned(),
                grants: None,
                code: false,
                remove: false,
            },
            1,
            now,
        );
        let told = asked(ask("moon fact", 2), &keeping, || false, now);
        assert!(told["error"].is_string(), "{told}");
        assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
        assert!(yes::waits(&other), "the owner's wait is kept");
        let told = asked(ask("moon fact", 3), &keeping, owner, now);
        assert_eq!(told["remove"], "moon-fact", "{told}");
        assert!(!yes::waits(&other));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thirteen's P3 1 on #6969: the folder of that name on the lamp is not
    /// the package made (one from the Store came in under the name after
    /// the made one was taken off in the app): refused in words, the
    /// helper never asked, nothing dropped.
    #[test]
    fn a_folder_not_the_made_package_is_not_removed() {
        let (root, keeping) = twelves_lamp("remove-other-folder");
        went_on_as(&keeping, "moon-fact", &"f".repeat(64));
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::approve(&id, 2, true, true, now, |asks| {
            removed(&keeping, asks, |_| panic!("asked the helper"))
        });
        assert!(
            told["error"].as_str().unwrap().contains("isn't the one"),
            "{told}"
        );
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// One already taken off in the app (its folder gone, the index still
    /// saying on) is dropped from the made skills without the helper.
    #[test]
    fn one_taken_off_in_the_app_is_dropped_without_the_helper() {
        let (root, keeping) = twelves_lamp("remove-gone");
        std::fs::remove_dir_all(keeping.installed.join("moon-fact")).unwrap();
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::approve(&id, 2, true, true, now, |asks| {
            removed(&keeping, asks, |_| panic!("asked the helper"))
        });
        assert_eq!(told["removed"], "moon-fact", "{told}");
        assert_eq!(names(&keeping.directory), ["evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thirteen's P3 4 on #6969: taken off the lamp, but the made skills'
    /// index not written: told and journaled as just that, never plainly
    /// "removed".
    #[test]
    fn taken_off_but_still_listed_is_told_so() {
        use std::os::unix::fs::PermissionsExt;
        let (root, keeping) = twelves_lamp("remove-still-listed");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let shut = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(&keeping.directory, shut).unwrap();
        let mut helper = Vec::new();
        let told = yes::approve(&id, 2, true, true, now, |asks| {
            removed(&keeping, asks, |name| {
                helper.push(name.to_owned());
                Ok(())
            })
        });
        let open = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&keeping.directory, open).unwrap();
        assert_eq!(helper, ["moon-fact"]);
        assert_eq!(told["removed"], "moon-fact", "{told}");
        assert_eq!(told["still_listed"], true, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("still listed"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
