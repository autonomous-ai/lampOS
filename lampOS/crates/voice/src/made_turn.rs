//! A skill the person made, turned off and back on by voice (#7445: on
//! lamp-9adf, build 13252, "Lamp, turn off the coffee time skill" called
//! `make_skill`'s remove, the only verb it had for a made skill, and the
//! lamp asked "Remove coffee time?": a yes would have lost the skill for
//! good). With `LAMP_MADE_SKILL_OFF_SAID` on alone
//! (`made_skill::off_said_on`).
//!
//! - **One plain path, the model's to choose.** `make_skill` takes
//!   [`TURN_OFF`] or [`TURN_ON`], the name as heard, given alone: whether a
//!   line asks to turn a skill off, pause or stop it, to turn it back on,
//!   or to delete it, is the model's judgment, told in the function's
//!   description and parameters, never a list of words (the owner's
//!   ruling, 13 Nov). A call that carries a turn-off is never a remove: it
//!   is read before the remove (`make_skill_local`).
//! - **The owner's alone, heard.** A turn nobody spoke changes nothing
//!   ([`NOT_ASKED_ALOUD`]); a line not the owner's, as a run or a remove's
//!   yes holds it (`services::owner_hears`), changes nothing and names no
//!   skill ([`NOT_OWNER`]). The name is matched to the skills they made as
//!   a remove's is (`made_remove::found`); one they didn't make, several as
//!   near, or none, are said so.
//! - **As the app's switch.** One on the lamp, its folder gone on as that
//!   very package (`made_skill::went_on_as`), is turned off or on by the
//!   manage helper's own `{"off": NAME}` or `{"on": NAME}`, as the app's
//!   switch asks it (`lamp-cloud`'s `skills.off` and `skills.on`): kept in
//!   lamp-skills' `off` file, which a round and a restart keep. No yes:
//!   nothing is lost, and "turn it back on" undoes it.
//!
//! No extra round trip: the match reads two local folders, then one line
//! to the manage helper over its local socket.
use crate::made_remove::{self, Found};
use crate::made_skill::{self, Keeping};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

/// The `make_skill` argument that turns a skill the person made off: its
/// name as heard, given alone.
pub const TURN_OFF: &str = "turn_off";

/// The `make_skill` argument that turns a skill the person made back on:
/// its name as heard, given alone.
pub const TURN_ON: &str = "turn_on";

/// What the model is told of a turn asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str =
    "a skill is turned off or on by voice only when the person asks out loud";

/// What the model is told of a turn asked by anyone but the owner.
pub const NOT_OWNER: &str = "only the lamp's owner can turn a skill they made off or on by voice";

/// What the model is told of a skill on the lamp the person didn't make.
pub const NOT_MADE: &str = "only a skill the person made is turned off or on this way";

/// The most of their skills' names said when none is near.
const NAMED_MOST: usize = 3;

/// The longest the manage helper is waited for, as a remove waits.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// A turn asked by the model's `make_skill` call: the name as heard, off
/// or back on (`off`), in the voice's request `request` (`heard`: someone
/// spoke it).
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    pub name: &'a str,
    pub off: bool,
    pub request: u64,
    pub heard: bool,
}

/// "off" or "on".
fn way(off: bool) -> &'static str {
    if off { "off" } else { "on" }
}

/// What the model is told of a skill not turned, and why.
fn not_turned(spoken: &str, off: bool, why: &str) -> Value {
    json!({
        "error": why,
        "next": format!(
            "Say plainly, in a few words, in the conversation's language, that {spoken} wasn't \
             turned {}, and why: {why}. Never say it is removed. Call nothing.",
            way(off)
        )
    })
}

/// A turn asked in `ask`, of the skills in `keeping`: only heard, and only
/// the owner's (`owner_hears`, asked first). The person's own made skill
/// [`made_remove::found`] by the name, on the lamp as the very package
/// made, is turned off or on by `turn` (the manage helper's own, as the
/// app's switch asks it: its name and whether off, or why not). Never a
/// remove, and nothing waits. What the model is told.
pub fn asked(
    ask: Ask<'_>,
    keeping: &Keeping,
    owner_hears: impl FnOnce() -> bool,
    turn: impl FnOnce(&str, bool) -> Result<(), String>,
) -> Value {
    let Ask {
        name,
        off,
        request,
        heard,
    } = ask;
    let way = way(off);
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't turned {way}: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    if !owner_hears() {
        eprintln!(
            "lamp-voice: request {request}: a skill wasn't turned {way}: not the owner's line"
        );
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can turn a skill \
                     they made off or on by voice. Name no skill. Call nothing."
        });
    }
    let found = made_remove::found(name, keeping);
    // Only the kind found, never the name asked: the journal holds no
    // person's words.
    eprintln!(
        "lamp-voice: request {request}: a turn {way} asked by voice found {}",
        match &found {
            Found::Made { exact: true, .. } => "one made skill by its name",
            Found::Made { exact: false, .. } => "one made skill by the nearest name",
            Found::Alike(_) => "more than one made skill as near",
            Found::NotMade { .. } => "a skill the person didn't make",
            Found::Nothing => "nothing near",
        }
    );
    let made = match found {
        Found::Made { name, .. } => name,
        Found::NotMade { name: other, .. } => {
            return json!({
                "error": NOT_MADE,
                "next": format!(
                    "Say plainly, in a few words, that {} isn't a skill they made, so I can't \
                     turn it {way} this way. Call nothing.",
                    made_skill::spoken_name(&other)
                )
            });
        }
        Found::Alike(names) => {
            return json!({
                "error": "more than one skill they made has a name like that",
                "next": format!(
                    "Ask them once, short, which one they mean: {}. Call nothing now.",
                    made_remove::said_or(&names[..names.len().min(NAMED_MOST)])
                )
            });
        }
        Found::Nothing => {
            let theirs = made_skill::kept_on_names(&keeping.directory);
            let named = match theirs.len() {
                0 => "and that none they made is on the lamp".to_owned(),
                count if count <= NAMED_MOST => format!(
                    "and name theirs on the lamp: {}",
                    theirs
                        .iter()
                        .map(|name| made_skill::spoken_name(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                count => format!("and that {count} they made are on the lamp"),
            };
            return json!({
                "error": made_remove::NO_MADE,
                "next": format!(
                    "Say plainly, in a few words, that you found no skill they made by that \
                     name, {named}. Call nothing."
                )
            });
        }
    };
    let spoken = made_skill::spoken_name(&made);
    let Some(kept) = made_skill::kept_on_named(&keeping.directory, &made) else {
        eprintln!(
            "lamp-voice: request {request}: a made skill wasn't turned {way}: it isn't on the lamp"
        );
        return not_turned(&spoken, off, "it isn't on the lamp now");
    };
    // The folder of its name on the lamp is the package made, as a
    // remove's yes holds it: never a Store or pushed one come in under it.
    match made_skill::went_on_as(keeping, &made) {
        Ok(Some(sha256)) if sha256 == kept.sha256 => {}
        Ok(None) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill wasn't turned {way}: it isn't on the lamp"
            );
            return not_turned(&spoken, off, "it isn't on the lamp now");
        }
        _ => {
            eprintln!(
                "lamp-voice: request {request}: a made skill wasn't turned {way}: the folder of its name on the lamp is not the package made"
            );
            return not_turned(
                &spoken,
                off,
                &format!("the skill called {spoken} on the lamp now isn't the one they made"),
            );
        }
    }
    if let Err(why) = turn(&made, off) {
        eprintln!("lamp-voice: request {request}: a made skill wasn't turned {way}: {why}");
        return not_turned(&spoken, off, &why);
    }
    eprintln!(
        "lamp-voice: request {request}: a made skill turned {way} by voice, as its owner asked"
    );
    if off {
        json!({
            TURN_OFF: made,
            "off": true,
            "next": format!(
                "Say only, in a few words, in the conversation's language, that {spoken} is off, \
                 and that they can say turn it back on whenever they like. Never say it is \
                 removed. Call nothing."
            )
        })
    } else {
        json!({
            TURN_ON: made,
            "off": false,
            "next": format!(
                "Say only, in a few words, in the conversation's language, that {spoken} is on \
                 again. Call nothing."
            )
        })
    }
}

/// The skill `name` turned off or on (`off`) by the manage helper on its
/// socket, its own `{"off": NAME}` or `{"on": NAME}`, as the app's switch
/// asks it: done, or why not in plain words.
pub fn turned_by_helper(socket: &Path, name: &str, off: bool) -> Result<(), String> {
    let way = way(off);
    match crate::tools::reached(socket, &json!({ way: name }), HELPER_WAIT) {
        None => Err("the lamp's skills couldn't be reached".into()),
        Some(None) => Err(format!("the lamp didn't say in time whether it is {way}")),
        Some(Some(answer)) if answer.get("done").is_some() => Ok(()),
        Some(Some(answer)) => {
            // The reason only: like the rest of this module, the journal
            // keeps no name a person said.
            eprintln!(
                "lamp-voice: the skills helper didn't turn a made skill {way}: {}",
                answer["refused"].as_str().unwrap_or_default()
            );
            Err(format!(
                "the lamp didn't let {} be turned {way}",
                made_skill::spoken_name(name)
            ))
        }
    }
}

/// [`asked`] on the lamp, by `make_skill`'s call: the made skills' own
/// folder (`LAMP_MAKE_SKILL`), the owner's hearing as a run's
/// (`services::owner_hears`), the manage helper's socket.
pub fn asked_here(
    name: &str,
    off: bool,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    made_skill::drop_gone(&keeping);
    let manage = keeping.manage.clone();
    asked(
        Ask {
            name,
            off,
            request,
            heard,
        },
        &keeping,
        || crate::services::owner_hears(request, heard, lease),
        |skill, off| turned_by_helper(&manage, skill, off),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::made_remove::tests::made_here;
    use crate::make_skill_local::tests::keeping;
    use crate::skill_write_yes as yes;
    use std::path::PathBuf;
    use std::time::Instant;

    /// lamp-9adf on 17 Nov: coffee-time made and on, moon-fact beside it.
    fn nines_lamp(test: &str) -> (PathBuf, Keeping) {
        yes::clear_for_test();
        let (root, keeping) = keeping(test);
        made_here(&keeping, &[("coffee-time", true), ("moon-fact", true)]);
        (root, keeping)
    }

    fn ask(name: &str, off: bool) -> Ask<'_> {
        Ask {
            name,
            off,
            request: 82,
            heard: true,
        }
    }

    fn owner() -> bool {
        true
    }

    fn names(keeping: &Keeping) -> Vec<String> {
        made_skill::entries(&keeping.directory)
            .iter()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// The line of #7445, as the model passes the name it heard to
    /// `make_skill`'s `turn_off`: coffee time is turned off through the
    /// helper's own off, as the app's switch, never removed and never
    /// asked about; it stays a made skill, on the lamp, and nothing waits.
    #[test]
    fn turn_off_sets_the_off_state_and_removes_nothing() {
        let (root, keeping) = nines_lamp("turn-off");
        let now = Instant::now();
        let mut helper = Vec::new();
        let told = asked(
            ask("the coffee time skill", true),
            &keeping,
            owner,
            |name, off| {
                helper.push((name.to_owned(), off));
                Ok(())
            },
        );
        assert_eq!(helper, [("coffee-time".to_owned(), true)], "{told}");
        assert_eq!(told[TURN_OFF], "coffee-time", "{told}");
        assert_eq!(told["off"], true, "{told}");
        assert!(told.get(made_remove::REMOVE).is_none(), "{told}");
        assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("coffee time is off"), "{next}");
        assert!(next.contains("turn it back on"), "{next}");
        assert!(next.contains("Never say it is removed"), "{next}");
        assert!(!next.contains("Remove coffee time?"), "{next}");
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping), ["coffee-time", "moon-fact"]);
        assert!(keeping.installed.join("coffee-time").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// "Turn coffee time back on": the helper's own on, as the app's
    /// switch, and said in a few words.
    #[test]
    fn turn_on_clears_the_off_state() {
        let (root, keeping) = nines_lamp("turn-on");
        let mut helper = Vec::new();
        let told = asked(ask("coffee time", false), &keeping, owner, |name, off| {
            helper.push((name.to_owned(), off));
            Ok(())
        });
        assert_eq!(helper, [("coffee-time".to_owned(), false)], "{told}");
        assert_eq!(told[TURN_ON], "coffee-time", "{told}");
        assert_eq!(told["off"], false, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("coffee time is on again"), "{next}");
        assert_eq!(names(&keeping), ["coffee-time", "moon-fact"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The manage helper is asked exactly the app's own lines on its
    /// socket, `{"off": NAME}` then `{"on": NAME}`, never a remove; a
    /// helper that keeps them as lamp-skills' `off` file does holds coffee
    /// time after the turn-off and not after the turn-on (lamp-skills' own
    /// test runs these lines through the real helper).
    #[test]
    fn the_helper_is_asked_the_apps_off_and_on() {
        use std::io::{BufRead, Write};
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!("made-turn-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("manage.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut off: Vec<String> = Vec::new();
            let mut asked = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(line.trim()).unwrap();
                if let Some(name) = request["off"].as_str() {
                    off.push(name.to_owned());
                } else if let Some(name) = request["on"].as_str() {
                    off.retain(|kept| kept != name);
                }
                asked.push((request, off.clone()));
                let mut stream = &stream;
                writeln!(stream, "{}", json!({"done": "turned"})).unwrap();
            }
            asked
        });
        turned_by_helper(&socket, "coffee-time", true).unwrap();
        turned_by_helper(&socket, "coffee-time", false).unwrap();
        let asked = server.join().unwrap();
        assert_eq!(asked[0].0, json!({"off": "coffee-time"}));
        assert_eq!(asked[0].1, ["coffee-time"]);
        assert_eq!(asked[1].0, json!({"on": "coffee-time"}));
        assert!(asked[1].1.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A line not the owner's turns nothing and names no skill; unheard,
    /// nothing either.
    #[test]
    fn a_stranger_cant_turn_one_off_or_on() {
        let (root, keeping) = nines_lamp("turn-stranger");
        for off in [true, false] {
            let told = asked(
                ask("coffee time", off),
                &keeping,
                || false,
                |_, _| panic!("turned"),
            );
            assert_eq!(told["error"], NOT_OWNER, "{told}");
            assert!(!told["next"].as_str().unwrap().contains("coffee"));
            let mut unheard = ask("coffee time", off);
            unheard.heard = false;
            let told = asked(
                unheard,
                &keeping,
                || panic!("asked whose"),
                |_, _| panic!("turned"),
            );
            assert_eq!(told["error"], NOT_ASKED_ALOUD, "{told}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// One they didn't make, several as near, none near, one not on the
    /// lamp, or a folder not the package made: said so, nothing turned.
    #[test]
    fn only_their_own_made_skill_on_the_lamp_is_turned() {
        yes::clear_for_test();
        let (root, keeping) = keeping("turn-not-theirs");
        made_here(
            &keeping,
            &[
                ("moon-fact", true),
                ("noon-fact", true),
                ("evening-dim", false),
            ],
        );
        let folder = keeping.installed.join("faq");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("skill.toml"), "kind = \"core\"\n").unwrap();
        let never = |_: &str, _: bool| -> Result<(), String> { panic!("turned") };
        let told = asked(ask("faq", true), &keeping, owner, never);
        assert_eq!(told["error"], NOT_MADE, "{told}");
        let told = asked(ask("loon fact", true), &keeping, owner, never);
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("moon fact or noon fact"),
            "{told}"
        );
        let told = asked(ask("weather report", true), &keeping, owner, never);
        assert_eq!(told["error"], made_remove::NO_MADE, "{told}");
        let told = asked(ask("evening dim", false), &keeping, owner, never);
        assert_eq!(told["error"], "it isn't on the lamp now", "{told}");
        std::fs::write(
            keeping.installed.join("moon-fact").join(".package.json"),
            json!({"sha256": "f".repeat(64)}).to_string(),
        )
        .unwrap();
        let told = asked(ask("moon fact", true), &keeping, owner, never);
        assert!(
            told["error"].as_str().unwrap().contains("isn't the one"),
            "{told}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The helper's refusal is said as not turned, never as removed.
    #[test]
    fn a_refusal_is_said_not_turned() {
        let (root, keeping) = nines_lamp("turn-refused");
        let told = asked(ask("coffee time", true), &keeping, owner, |_, _| {
            Err("the lamp's skills couldn't be reached".to_owned())
        });
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("coffee time wasn't turned off"), "{next}");
        assert!(next.contains("Never say it is removed"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The words of a remove, word for word, as they are with the switch
    /// off: the function's last sentence and the remove's parameter.
    const REMOVE_SAID: &str = "Only when the person asks to delete or remove a skill: its name \
         as they said it, given alone with no request.";

    /// Switch off (unset or anything but `on`): `make_skill`'s description
    /// and parameters are byte for byte as before #7445, and a call that
    /// carries a turn-off is not taken as one: alone, it makes nothing, as
    /// any call with no request did.
    #[test]
    fn switch_off_is_byte_for_byte_as_before() {
        made_skill::set_off_said_here(None);
        let unset = crate::make_skill_local::tool();
        made_skill::set_off_said_here(Some(false));
        let off = crate::make_skill_local::tool();
        assert_eq!(unset.description, off.description);
        assert_eq!(unset.parameters, off.parameters);
        assert!(
            off.description
                .ends_with(crate::make_skill_local::REMOVE_TOLD),
            "{}",
            off.description
        );
        assert!(!off.description.contains(TURN_OFF), "{}", off.description);
        let parameters = off.parameters.unwrap();
        assert!(parameters["properties"].get(TURN_OFF).is_none());
        assert!(parameters["properties"].get(TURN_ON).is_none());
        assert_eq!(
            parameters["properties"][made_remove::REMOVE]["description"],
            REMOVE_SAID
        );
        for (key, name) in [(TURN_OFF, "coffee time"), (TURN_ON, "coffee time")] {
            let call = lamp_gemini::ToolCall {
                id: "1".into(),
                name: made_skill::MAKE.into(),
                args: json!({ key: name }),
            };
            let told = crate::make_skill_local::answer(&call, 82, true, None, None);
            assert_eq!(
                told["error"],
                crate::make_skill_local::NO_REQUEST,
                "{key}: {told}"
            );
        }
        made_skill::set_off_said_here(None);
    }

    /// Switch on: the function's description tells a turn-off, a pause or
    /// a stop from a remove, in the remove's sentence's place, and its
    /// parameters give `turn_off` and `turn_on` beside a remove told as for
    /// good; with every switch on, within the link's bounds (a description
    /// of at most 1,024 bytes with no "yes", parameters of at most 4,096).
    #[test]
    fn switch_on_tells_a_turn_off_from_a_remove_within_the_bounds() {
        made_skill::set_off_said_here(Some(true));
        made_skill::set_writes_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        let tool = crate::make_skill_local::tool();
        assert!(
            tool.description
                .ends_with(crate::make_skill_local::TURN_OFF_TOLD),
            "{}",
            tool.description
        );
        for verb in [
            "turn off",
            "pause",
            "stop",
            "turn_off",
            "never remove",
            "turn_on",
        ] {
            assert!(
                tool.description.contains(verb),
                "{verb}: {}",
                tool.description
            );
        }
        // A remove and a delete are still offered (#6960), as are the run
        // and the list.
        for verb in ["delete: remove", "run now: run", "list: made"] {
            assert!(
                tool.description.contains(verb),
                "{verb}: {}",
                tool.description
            );
        }
        assert!(
            tool.description.len() <= 1_024,
            "{}",
            tool.description.len()
        );
        assert!(!tool.description.contains("yes"), "{}", tool.description);
        let parameters = tool.parameters.unwrap();
        assert!(
            parameters.to_string().len() <= 4_096,
            "{}",
            parameters.to_string().len()
        );
        let off = parameters["properties"][TURN_OFF]["description"]
            .as_str()
            .unwrap();
        assert!(off.contains("turn off, pause or stop"), "{off}");
        assert!(off.contains("Never remove"), "{off}");
        assert!(
            parameters["properties"][TURN_ON].is_object(),
            "{parameters}"
        );
        let remove = parameters["properties"][made_remove::REMOVE]["description"]
            .as_str()
            .unwrap();
        assert!(remove.contains("for good"), "{remove}");
        assert!(remove.contains("never to turn one off"), "{remove}");
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_scripts_here(None);
        made_skill::set_writes_here(None);
        made_skill::set_off_said_here(None);
    }

    /// Remove still waits for its yes (#6960) with the switch on: a remove
    /// asks "Remove moon fact?" and removes nothing until the owner's yes,
    /// and a turn-off of the same skill before it removes nothing.
    #[test]
    fn remove_still_waits_for_its_yes() {
        made_skill::set_off_said_here(Some(true));
        let (root, keeping) = nines_lamp("turn-remove-waits");
        let now = Instant::now();
        let told = asked(ask("moon fact", true), &keeping, owner, |_, _| Ok(()));
        assert_eq!(told[TURN_OFF], "moon-fact", "{told}");
        assert!(!yes::awaited(now));
        let told = made_remove::asked(
            made_remove::Ask {
                name: "moon fact",
                request: 83,
                heard: true,
                sends_here: true,
            },
            &keeping,
            owner,
            now,
        );
        assert_eq!(told[made_remove::REMOVE], "moon-fact", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("\"Remove moon fact?\""), "{next}");
        assert!(yes::awaited(now));
        assert_eq!(names(&keeping), ["coffee-time", "moon-fact"]);
        yes::clear_for_test();
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
