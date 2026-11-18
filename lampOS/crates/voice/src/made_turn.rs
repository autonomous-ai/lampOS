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
//! - **Made or added alike.** The skills they made are the made skills'
//!   index (`made.json`), which holds those made by voice, typed in the
//!   app, and added from a file in the app (`"added": "file"`, an agent
//!   skill's `SKILL.md` too): each is turned off and on the same way, told
//!   to the model as one they made or added (#7594: on lamp-9adf, 17 Nov,
//!   "turn off my btc price skill", one added from a file, was answered
//!   that it is done in the app, since the function told of one they made
//!   alone). A skill the lamp ships, or one from the Store or pushed from a
//!   computer, is none of theirs here, as for a remove.
//! - **As the app's switch.** One on the lamp, its folder gone on as that
//!   very package (`made_skill::went_on_as`), is turned off or on by the
//!   manage helper's own `{"off": NAME}` or `{"on": NAME}`, as the app's
//!   switch asks it (`lamp-cloud`'s `skills.off` and `skills.on`): kept in
//!   lamp-skills' `off` file, which a round and a restart keep. No yes:
//!   nothing is lost, and "turn it back on" undoes it. A remove of that
//!   very skill still waiting for its yes is dropped
//!   (`skill_write_yes::drop_remove_of`): "no, just turn it off" never
//!   leaves it for a later yes to take.
//!
//! No extra round trip: the match reads two local folders, then one line
//! to the manage helper over its local socket.
use crate::made_remove::{self, Found};
use crate::made_skill::{self, Keeping};
use crate::skill_write_yes;
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
pub const NOT_OWNER: &str =
    "only the lamp's owner can turn a skill they made or added off or on by voice";

/// What the model is told of a skill on the lamp the person didn't make.
pub const NOT_MADE: &str = "only a skill the person made or added is turned off or on this way";

/// The most of their skills' names said when none is near.
const NAMED_MOST: usize = 3;

/// The key of a turn-off's answer that tells the model how it is turned
/// back on later ([`BACK_ON_BY_CALL`]).
pub const LATER: &str = "later";

/// How a skill turned off is turned back on, told the model (#7562's row on
/// lamp-9adf, 18 Nov 03:2x: after "turn off my bitcoin price skill", "turn
/// my bitcoin price skill back on" made no call, and the lamp said
/// "Bitcoin price is back on" while it stayed off).
pub const BACK_ON_BY_CALL: &str = "A skill turned off is turned back on by calling make_skill \
     with turn_on and its name, when their line asks to turn it on, or by the lamp when the owner \
     asks to use it; never say it is on or back on unless a call's answer says so.";

/// What the model is told when it calls the function of a skill the person
/// made or added that is turned off (`lamp_skills::TOOL_TURNED_OFF` is
/// this very text): it ran nothing.
pub const FUNCTION_OFF: &str = "That skill is turned off, so it did nothing. Say so in a few \
     words: it is off and didn't run, and they can ask to turn it back on. Never turn it on for \
     this: it is turned back on only by calling make_skill with turn_on and its name, when their \
     line asks to turn it on; never say it is on or back on unless that call's answer says so.";

/// What the model is told of a skill's function that answered
/// [`FUNCTION_OFF`] after a `turn_on` turned that skill on in the same turn.
pub const JUST_ON: &str = "it was turned on just now and runs from their next ask";

/// How the note of a forced round after a turn's candidates opens.
pub const UNCHOSEN_OPENS: &str = "(Not said: you answered with no call";

/// The answer's key for the person's own skills offered when none is near
/// the name asked ([`offered`]).
pub const CANDIDATES: &str = "candidates";

/// The most of their skills offered when none is near the name asked.
pub const OFFERED_MOST: usize = 8;

/// The most words of a skill's description said beside its name.
const DOES_WORDS: usize = 8;

/// The most bytes of a skill's `SKILL.md` read for its description.
const CARD_MOST: u64 = 16 * 1_024;

/// The longest the manage helper is waited for, as a remove waits.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// A turn asked by the model's `make_skill` call: the name as heard, off
/// or back on (`off`), in the voice's request `request` (`heard`: someone
/// spoke it); `state`, lamp-skills' state folder, whose `off` file says
/// which skills are turned off (none: not told).
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    pub name: &'a str,
    pub off: bool,
    pub request: u64,
    pub heard: bool,
    pub state: Option<&'a Path>,
}

/// The first [`DOES_WORDS`] words of the description in the `SKILL.md` of
/// the skill `name` on the lamp, read at most [`CARD_MOST`] bytes.
fn does(keeping: &Keeping, name: &str) -> Option<String> {
    use std::io::Read;
    let mut card = String::new();
    std::fs::File::open(keeping.installed.join(name).join("SKILL.md"))
        .ok()?
        .take(CARD_MOST)
        .read_to_string(&mut card)
        .ok()?;
    let description = made_skill::written_description(&card)?;
    let words: Vec<&str> = description.split_whitespace().collect();
    let mut said = words[..words.len().min(DOES_WORDS)].join(" ");
    if words.len() > DOES_WORDS {
        said.push_str("...");
    }
    Some(said)
}

/// The person's own skills, made or added and on the lamp, that a turn
/// `off` (or back on) could mean when none is near the name asked
/// (#7562's row on lamp-9adf, 18 Nov: "turn my bitcoin price skill back
/// on" found nothing near "btc price", and the model, told no other,
/// called it the lamp's own skill). For a turn off those not turned off,
/// for a turn on those turned off, as lamp-skills' `off` file in `state`
/// says; all of theirs when it can't be read. Newest first, at most
/// [`OFFERED_MOST`], each its said name and its description's first words.
/// Which one, if any, the person means is the model's to judge: no word is
/// matched here.
pub fn offered(keeping: &Keeping, off: bool, state: Option<&Path>) -> Vec<Value> {
    let turned_off = state.and_then(|state| crate::skill_about::turned_off(state).ok());
    let mut names: Vec<String> = Vec::new();
    for name in made_skill::kept_on_names(&keeping.directory)
        .into_iter()
        .rev()
    {
        let fits = turned_off
            .as_ref()
            .is_none_or(|turned_off| turned_off.contains(&name) != off);
        if fits && !names.contains(&name) {
            names.push(name);
        }
    }
    names
        .iter()
        .take(OFFERED_MOST)
        .map(|name| {
            let mut offered = json!({ "name": made_skill::spoken_name(name) });
            if let Some(does) = does(keeping, name) {
                offered["does"] = json!(does);
            }
            offered
        })
        .collect()
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
/// remove, and nothing waits: a remove of it waiting is dropped. What the
/// model is told.
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
        state,
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
                     they made or added off or on by voice. Name no skill. Call nothing."
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
                    "Say plainly, in a few words, that {} isn't a skill they made or added, so \
                     I can't turn it {way} this way. Call nothing.",
                    made_skill::spoken_name(&other)
                )
            });
        }
        Found::Alike(names) => {
            return json!({
                "error": "more than one skill they made or added has a name like that",
                "next": format!(
                    "Ask them once, short, which one they mean: {}. Call nothing now.",
                    made_remove::said_or(&names[..names.len().min(NAMED_MOST)])
                )
            });
        }
        Found::Nothing => {
            // Theirs it could mean, for the model to choose from in this
            // turn: one they added named otherwise by the model ("bitcoin
            // price" for btc price) is never called the lamp's own.
            let offered = offered(keeping, off, state);
            if !offered.is_empty() {
                let call = if off { TURN_OFF } else { TURN_ON };
                let which = if off {
                    "not turned off"
                } else {
                    "turned off now"
                };
                eprintln!(
                    "lamp-voice: request {request}: a turn {way} found nothing near: {} of theirs offered",
                    offered.len()
                );
                return json!({
                    "error": made_remove::NO_MADE,
                    CANDIDATES: offered,
                    "next": format!(
                        "No skill they made or added has that name. Their skills {which} are \
                         in candidates, each with its name and what it does. If one is the \
                         skill they mean, by its name or what it does, call make_skill again \
                         now with {call} and that name alone, saying nothing first. If none \
                         is, say plainly, in a few words, that you found no skill they made or \
                         added like that, and call nothing. Nothing was turned {way}: never \
                         say one is {way} or already {way} unless a make_skill call's answer \
                         says so. Never say it is the lamp's own skill or that it is turned \
                         {way} in the app."
                    )
                });
            }
            let theirs = made_skill::kept_on_names(&keeping.directory);
            let named = match theirs.len() {
                0 => "and that none they made or added is on the lamp".to_owned(),
                count if count <= NAMED_MOST => format!(
                    "and name theirs on the lamp: {}",
                    theirs
                        .iter()
                        .map(|name| made_skill::spoken_name(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                count => format!("and that {count} they made or added are on the lamp"),
            };
            return json!({
                "error": made_remove::NO_MADE,
                "next": format!(
                    "Say plainly, in a few words, that you found no skill they made or added \
                     by that name, {named}. Never say it is the lamp's own skill. Call \
                     nothing."
                )
            });
        }
    };
    // A remove of this very skill waiting for its yes is dropped, whether
    // it turns or not ("Remove coffee time?", "no, just turn it off"): a
    // later yes never takes it (Twelve's P2 on #7453).
    if skill_write_yes::drop_remove_of(&made) {
        eprintln!(
            "lamp-voice: request {request}: a remove waiting for the same made skill was dropped"
        );
    }
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
                &format!(
                    "the skill called {spoken} on the lamp now isn't the one they made or added"
                ),
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
            ),
            LATER: BACK_ON_BY_CALL
        })
    } else {
        json!({
            TURN_ON: made,
            "off": false,
            // #7562's row on lamp-9adf, 18 Nov, request 9: "tell me a space
            // fact with my space facts skill" was this turn on alone, and
            // never ran. Whether the line asked to use it is the model's.
            "next": format!(
                "If their line asked to use or run it, not only to turn it on, call make_skill \
                 now with run and its name alone, saying nothing first: its answer says what to \
                 say. Otherwise say only, in a few words, in the conversation's language, that \
                 {spoken} is on again, and call nothing."
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

/// What the model is told in a heard turn while any skill the person made
/// or added is turned off, with the switch on: how many, never which (a
/// name can carry a person's, and the line may not be the owner's), and
/// [`BACK_ON_BY_CALL`]. None when none is off, or the `off` file in `state`
/// can't be read. A structural guard, no words matched: on lamp-9adf, 18
/// Nov 03:2x, "turn my bitcoin price skill back on" made no call and was
/// answered "back on" while btc price stayed off; whether a line asks to
/// turn one back on stays the model's to judge.
pub fn off_note(keeping: &Keeping, state: &Path) -> Option<String> {
    let turned_off = crate::skill_about::turned_off(state).ok()?;
    let mut theirs = made_skill::kept_on_names(&keeping.directory);
    theirs.sort();
    theirs.dedup();
    let count = theirs
        .iter()
        .filter(|name| turned_off.contains(*name))
        .count();
    let (count, are) = match count {
        0 => return None,
        1 => ("One skill".to_owned(), "is"),
        count => (format!("{count} skills"), "are"),
    };
    Some(format!(
        "(For you to know, not to answer: {count} they made or added {are} turned off now. A \
         line that asks to use or run one: call its function as for any skill, or make_skill \
         with run and its name, never turn_on alone; the lamp turns it on for the owner's ask \
         and runs it, and its answer says what to say. \
         {BACK_ON_BY_CALL})"
    ))
}

/// [`off_note`] on the lamp, in the voice's request `request` (`heard`:
/// someone spoke it): with the switch on and a heard line only.
pub fn off_note_here(request: u64, heard: bool) -> Option<String> {
    if !heard || !made_skill::off_said_on() {
        return None;
    }
    let keeping = Keeping::when(true)?;
    let note = off_note(&keeping, Path::new(crate::skill_about::STATE_DIR))?;
    eprintln!("lamp-voice: request {request}: told that a skill they made or added is turned off");
    Some(note)
}

/// Whether a call's answer `told` is a turned-off skill's function that
/// ran nothing ([`FUNCTION_OFF`]).
pub fn ran_turned_off(told: &Value) -> bool {
    told["error"] == FUNCTION_OFF
}

/// Whether the call `name`'s answer `told` says the skill is still off: its
/// function's [`FUNCTION_OFF`], or `make_skill`'s run now refused by
/// lamp-skills as off or snoozed (`made_run::STOPPED`).
pub fn still_off(name: &str, told: &Value) -> bool {
    ran_turned_off(told) || name == made_skill::MAKE && crate::made_run::stopped(told)
}

/// What the model is told of a skill's function that still answered
/// [`FUNCTION_OFF`] after a `turn_on` turned it on in the same turn
/// ("turn on btc price and run it": lamp-skills reads its `off` file again
/// within 2 s): never that it is off.
pub fn just_on() -> Value {
    json!({
        "error": JUST_ON,
        "next": "Say only, in a few words, in the conversation's language, that the skill is on \
                 again, and that they can ask to run it now. Never say it is off. Call nothing."
    })
}

/// How many times a skill's function is run again when it still answered
/// [`FUNCTION_OFF`] just after a `turn_on` turned it on in the turn, and how
/// long before each: lamp-skills reads its `off` file again within 2 s.
pub const RERUNS: usize = 2;
pub const RERUN_AFTER: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_millis(1_200)
};

/// The id of the `turn_on` the lamp calls itself for an ask to use a skill
/// turned off ([`on_for_ask`]): an id only, in no chat's thread.
pub const ON_FOR_ASK_ID: &str = "turned_off_on_for_ask";

/// The skill's name as said, of its function's call `name` with `args`
/// ("btc price" for `btc_price`).
pub fn spoken_of(name: &str, args: &Value) -> String {
    crate::skill_named::spoken(crate::skill_named::function_of(name, args))
}

/// `spoken` with its first letter upper case, as a line opens.
fn opening(spoken: &str) -> String {
    let mut said = spoken.chars();
    said.next()
        .map(|first| first.to_uppercase().chain(said).collect())
        .unwrap_or_default()
}

/// The `make_skill` call the lamp makes itself when a heard line asked to
/// use a skill turned off, its function's call `name` with `args`
/// answering [`FUNCTION_OFF`] (the first builder, 18:41 on 18 Nov, from the
/// product owner: nothing is asked of the person, an offer becomes the
/// action and one line with an undo). Its `turn_on` goes through
/// `make_skill` as the model's own would ([`asked`]): heard, the owner's
/// alone, the name matched to theirs; a guest's is refused there. No words
/// of the line are read: the model's call of the skill is the ask.
pub fn on_for_ask(name: &str, args: &Value) -> (String, Value) {
    (
        made_skill::MAKE.to_owned(),
        json!({ TURN_ON: spoken_of(name, args) }),
    )
}

/// What the model is told of its call of a skill turned off that the lamp
/// turned on for the ask and ran again (`ran`, that run's answer, or
/// [`just_on`] if it still answered off): the one line to say first, in
/// the conversation's language, with its undo, then what it found.
pub fn turned_on_for_ask(spoken: &str, ran: &Value) -> Value {
    json!({
        "turned_on": spoken,
        "ran": ran,
        "next": format!(
            "It was off, so the lamp turned it on and ran it for their ask. Say first, in the \
             conversation's language: \"{}\" Then say what ran tells you to say, in a few \
             words. Call nothing.",
            undo_line(spoken)
        )
    })
}

/// The one line said of a skill the lamp turned on for an ask to use it,
/// with its undo ("Btc price was off, so I turned it on. Say 'turn it off'
/// to undo."): by its function's call ([`turned_on_for_ask`]) or by
/// `make_skill`'s run now (`made_run::told_turned_on`).
pub fn undo_line(spoken: &str) -> String {
    format!(
        "{} was off, so I turned it on. Say 'turn it off' to undo.",
        opening(spoken)
    )
}

/// What the model is told of its call of a skill turned off that the lamp
/// did not turn on for the ask: the `turn_on`'s answer `on` refused it (a
/// guest's line, [`NOT_OWNER`], or another why) or found no such skill of
/// theirs. Said plainly, never that it is on.
pub fn not_turned_on(on: &Value) -> Value {
    let why = on["error"].as_str().unwrap_or("it couldn't be turned on");
    json!({
        "error": "that skill is turned off, so it did nothing",
        "not_turned_on": why,
        "next": format!(
            "Say only, plainly, in a few words, in the conversation's language, that it is \
             turned off, so it didn't run, and why it wasn't turned on: {why}. Name no skill. \
             Never say it is on. Call nothing."
        )
    })
}

/// What the model is told of its call of the skill the lamp turned on and
/// ran for the ask already (`ran`, what it was told of that run): never
/// run again.
pub fn ran_already(ran: &Value) -> Value {
    json!({
        "error": "Not run again: it was turned on and run for their ask already.",
        "ran": ran,
        "next": "Say only what its result above tells you to say. Call nothing."
    })
}

/// Whether the answer `told` to the call `name` with `args` turned a skill
/// off or back on: a `make_skill` turn that found and turned it.
pub fn turned(name: &str, args: &Value, told: &Value) -> bool {
    name == made_skill::MAKE
        && (args[TURN_OFF].is_string() || args[TURN_ON].is_string())
        && told["off"].is_boolean()
        && (told[TURN_OFF].is_string() || told[TURN_ON].is_string())
}

/// The note of a round asked once more when the model answered with no call
/// after a `make_skill` turn (`name`, `args`) found no skill by the name and
/// offered theirs ([`CANDIDATES`] in its answer `told`); none for any other
/// answer. #7562's row on lamp-9adf, 18 Nov 10:57: "turn off my bitcoin
/// price skill" offered btc price (on), and the model, with no second call,
/// said "Bitcoin price is already off" while it was on. A skill is said off
/// (or back on) only after the turn that turned it; which of the offered,
/// if any, they meant stays the model's to judge, the candidates named
/// again.
pub fn unchosen_note(name: &str, args: &Value, told: &Value) -> Option<String> {
    if name != made_skill::MAKE {
        return None;
    }
    let off = args[TURN_OFF].is_string();
    if !off && !args[TURN_ON].is_string() {
        return None;
    }
    let offered: Vec<&str> = told
        .get(CANDIDATES)?
        .as_array()?
        .iter()
        .filter_map(|offered| offered["name"].as_str())
        .collect();
    if offered.is_empty() {
        return None;
    }
    let (call, way, still) = if off {
        (TURN_OFF, "off", "on")
    } else {
        (TURN_ON, "on", "off")
    };
    let back = if off { "" } else { " or back on" };
    Some(format!(
        "{UNCHOSEN_OPENS} after make_skill's {call} found no skill by that name and offered \
         theirs in candidates. Nothing was turned {way}: each in candidates is {still} now, \
         whatever was said before, so never say one is {way}{back} or already {way}. If one of \
         them is the skill they mean, by its name or what it does, call make_skill now with \
         {call} and that name alone, saying nothing first. Only if none is, say plainly, in a \
         few words, that you found no skill they made or added like that, and call nothing. \
         Candidates: {}.)",
        offered.join(", ")
    ))
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
            state: Some(Path::new(crate::skill_about::STATE_DIR)),
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
            state: None,
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
        // #7562's request 9: whether the line asked to use it is the
        // model's to judge, told to call the run now, which says the undo.
        assert!(next.contains("asked to use or run it"), "{next}");
        assert!(next.contains("call make_skill now with run"), "{next}");
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

    /// A `SKILL.md` of the kind added in the owner's app on lamp-9adf, 17
    /// Nov: an agent skill that reads a web API, `grants = ["speak",
    /// "net"]`.
    const BTC: &str = "---\nname: btc-price\ndescription: Says the current Bitcoin price when \
                       asked.\n---\n\nFetch the current BTC price in US dollars from \
                       https://api.coinbase.com/v2/prices/spot and say it in one short \
                       sentence.\n";

    /// `BTC` added from the file as the app's upload adds it (the agent
    /// skill, `grants = ["speak", "net"]`), and put on: its folder on the
    /// lamp as installed, and lamp-skills' records' places beside it.
    fn uploaded_btc(test: &str) -> (PathBuf, Keeping, crate::skill_about::Places) {
        crate::upload_agent::set_on_here(Some(true));
        crate::skill_task_yes::set_net_here(Some(true));
        let (root, keeping) = keeping(test);
        let place = crate::upload_skill::Place {
            skills: keeping.installed.clone(),
            pushed: root.join("pushed"),
        };
        std::fs::create_dir_all(&place.skills).unwrap();
        std::fs::create_dir_all(&place.pushed).unwrap();
        let made =
            crate::upload_skill::take(&place, &keeping, None, BTC, 1_800_000_000_000).unwrap();
        assert_eq!(made.name, "btc-price");
        assert_eq!(made_skill::entries(&keeping.directory)[0]["added"], "file");
        let (files, _) = crate::upload_agent::files(BTC).unwrap();
        assert!(
            files.toml.contains("kind = \"installable\"") && files.toml.contains("\"net\""),
            "{}",
            files.toml
        );
        let install = crate::make_skill_local::tests::manage_helper(&keeping.manage);
        made_skill::put_on(&keeping, &made).unwrap();
        install.join().unwrap();
        let folder = keeping.installed.join("btc-price");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("skill.toml"), &files.toml).unwrap();
        std::fs::write(folder.join("SKILL.md"), &files.card).unwrap();
        std::fs::write(
            folder.join(".package.json"),
            json!({"sha256": made.sha256}).to_string(),
        )
        .unwrap();
        crate::skill_task_yes::set_net_here(None);
        crate::upload_agent::set_on_here(None);
        let places = crate::skill_about::Places {
            installed: keeping.installed.clone(),
            pushed: place.pushed.clone(),
            state: root.join("state"),
            tools: root.join("tools"),
        };
        std::fs::create_dir_all(&places.state).unwrap();
        (root, keeping, places)
    }

    /// The manage helper's off and on, kept in lamp-skills' `off` file at
    /// `off_file`, a name a line.
    fn kept_in(off_file: &Path) -> impl Fn(&str, bool) -> Result<(), String> + Copy + '_ {
        move |name, off| {
            let mut names: String = std::fs::read_to_string(off_file)
                .unwrap_or_default()
                .lines()
                .filter(|kept| *kept != name)
                .map(|kept| format!("{kept}\n"))
                .collect();
            if off {
                names.push_str(&format!("{name}\n"));
            }
            std::fs::write(off_file, names).map_err(|error| error.to_string())
        }
    }

    /// #7594, on lamp-9adf, 17 Nov 23:1x: "Lamp, turn off my btc price
    /// skill", a skill added from a `SKILL.md` in the app, was answered "I
    /// can't turn skills off myself, that's done in the app", since the
    /// function and its `turn_off` told only of a skill they made. With
    /// the switch on they tell of one they made or added; the uploaded
    /// skill is turned off through the helper's own off, "is it on?" is
    /// told off from lamp-skills' `off` file, and it is turned back on, as
    /// a made skill is.
    #[test]
    fn an_uploaded_skill_is_turned_off_told_off_and_turned_back_on() {
        use crate::skill_about::{self, State};
        made_skill::set_off_said_here(Some(true));
        yes::clear_for_test();
        // The function and its turns tell of a skill they added too.
        let tool = crate::make_skill_local::tool();
        assert!(
            tool.description.contains("one they made or added"),
            "{}",
            tool.description
        );
        let parameters = tool.parameters.unwrap();
        for key in [TURN_OFF, TURN_ON] {
            let said = parameters["properties"][key]["description"]
                .as_str()
                .unwrap();
            assert!(said.contains("a skill they made or added"), "{key}: {said}");
            assert!(said.contains("from a file"), "{key}: {said}");
        }
        let (root, keeping, places) = uploaded_btc("turn-uploaded");
        let folder = keeping.installed.join("btc-price");
        let off_file = places.state.join(skill_about::OFF_FILE);
        let helper = kept_in(&off_file);
        let state = || {
            skill_about::skills(&places, 0, false)
                .unwrap()
                .into_iter()
                .find(|skill| skill.name == "btc-price")
                .unwrap()
                .state
        };
        assert_ne!(state(), State::TurnedOff);
        // "Lamp, turn off my btc price skill."
        let told = asked(ask("my btc price skill", true), &keeping, owner, helper);
        assert_eq!(told[TURN_OFF], "btc-price", "{told}");
        assert_eq!(told["off"], true, "{told}");
        assert!(
            told["next"].as_str().unwrap().contains("btc price is off"),
            "{told}"
        );
        // "Is it on?": off, from the lamp's own records.
        assert_eq!(state(), State::TurnedOff);
        // Still added from the file, on the lamp, nothing waiting.
        assert_eq!(names(&keeping), ["btc-price"]);
        assert!(folder.is_dir());
        assert!(!yes::awaited(std::time::Instant::now()));
        // "Turn my btc price skill back on."
        let told = asked(ask("btc price", false), &keeping, owner, helper);
        assert_eq!(told[TURN_ON], "btc-price", "{told}");
        assert_eq!(told["off"], false, "{told}");
        assert_ne!(state(), State::TurnedOff);
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7562's row on lamp-9adf, 18 Nov 01:3x: btc price, added from a
    /// file, was turned off; "turn my bitcoin price skill back on" found
    /// nothing near, and the model, told of no other, called it the lamp's
    /// own skill and said to turn it on in the app. Now the answer offers
    /// theirs that are turned off, btc price with what it does, to call
    /// again with in the same turn; it is never told with the lamp's skills
    /// and "offer to turn on nothing"; the second call turns it on. A turn
    /// off offers those not turned off.
    #[test]
    fn a_turn_on_by_another_name_offers_theirs_turned_off_to_call_again() {
        use crate::skill_about::{self, State};
        made_skill::set_off_said_here(Some(true));
        yes::clear_for_test();
        let (root, keeping, places) = uploaded_btc("turn-offered");
        let off_file = places.state.join(skill_about::OFF_FILE);
        let helper = kept_in(&off_file);
        let mut off = ask("btc price", true);
        off.state = Some(&places.state);
        assert_eq!(asked(off, &keeping, owner, helper)["off"], true);
        // "Lamp, turn my bitcoin price skill back on."
        let mut bitcoin = ask("my bitcoin price skill", false);
        bitcoin.state = Some(&places.state);
        let told = asked(bitcoin, &keeping, owner, |_, _| panic!("turned"));
        assert_eq!(told["error"], made_remove::NO_MADE, "{told}");
        let offered = told[CANDIDATES].as_array().unwrap();
        assert_eq!(offered.len(), 1, "{told}");
        assert_eq!(offered[0]["name"], "btc price", "{told}");
        assert!(
            offered[0]["does"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("bitcoin"),
            "{told}"
        );
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("call make_skill again now with turn_on"),
            "{next}"
        );
        assert!(
            next.contains("Never say it is the lamp's own skill"),
            "{next}"
        );
        let args = json!({ TURN_ON: "my bitcoin price skill" });
        assert!(!skill_about::tells_their_skills(&args, &told), "{told}");
        // The second call, with the name offered, turns it on.
        let mut again = ask("btc price", false);
        again.state = Some(&places.state);
        let told = asked(again, &keeping, owner, helper);
        assert_eq!(told[TURN_ON], "btc-price", "{told}");
        let state = skill_about::skills(&places, 0, false)
            .unwrap()
            .into_iter()
            .find(|skill| skill.name == "btc-price")
            .unwrap()
            .state;
        assert_ne!(state, State::TurnedOff);
        // On now: a turn on by another name offers none of theirs, and
        // never calls it the lamp's own; a turn off offers it.
        let mut bitcoin = ask("my bitcoin price skill", false);
        bitcoin.state = Some(&places.state);
        let told = asked(bitcoin, &keeping, owner, |_, _| panic!("turned"));
        assert!(told.get(CANDIDATES).is_none(), "{told}");
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("Never say it is the lamp's own skill"),
            "{told}"
        );
        let mut bitcoin = ask("my bitcoin price skill", true);
        bitcoin.state = Some(&places.state);
        let told = asked(bitcoin, &keeping, owner, |_, _| panic!("turned"));
        assert_eq!(told[CANDIDATES][0]["name"], "btc price", "{told}");
        assert!(told["next"].as_str().unwrap().contains("with turn_off"));
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7562's row on lamp-9adf, 18 Nov 03:2x: btc price turned off by
    /// voice, then "turn my bitcoin price skill back on" made no call and
    /// was answered "Bitcoin price is back on" while it stayed off. While
    /// one they made or added is off, each heard turn tells the model that
    /// it is turned back on only by `make_skill`'s `turn_on`, never said on
    /// without it; how many, never which. The turn-off's own answer says
    /// so too. None off, or the switch off, or unheard: nothing told.
    #[test]
    fn while_one_of_theirs_is_off_back_on_is_told_as_a_turn_on_call() {
        use crate::skill_about;
        made_skill::set_off_said_here(Some(true));
        yes::clear_for_test();
        let (root, keeping, places) = uploaded_btc("turn-off-note");
        let off_file = places.state.join(skill_about::OFF_FILE);
        let helper = kept_in(&off_file);
        assert_eq!(off_note(&keeping, &places.state), None);
        let mut off = ask("btc price", true);
        off.state = Some(&places.state);
        let told = asked(off, &keeping, owner, helper);
        assert_eq!(told[LATER], BACK_ON_BY_CALL, "{told}");
        let note = off_note(&keeping, &places.state).unwrap();
        assert!(
            note.contains("One skill they made or added is turned off"),
            "{note}"
        );
        assert!(
            note.contains("turned back on by calling make_skill with turn_on"),
            "{note}"
        );
        assert!(
            note.contains("never say it is on or back on unless"),
            "{note}"
        );
        assert!(!note.contains("btc") && !note.contains("bitcoin"), "{note}");
        // Unheard, or with the switch off, nothing is told.
        assert_eq!(off_note_here(86, false), None);
        made_skill::set_off_said_here(Some(false));
        assert_eq!(off_note_here(86, true), None);
        made_skill::set_off_said_here(Some(true));
        // Back on: nothing told.
        let mut on = ask("btc price", false);
        on.state = Some(&places.state);
        assert_eq!(asked(on, &keeping, owner, helper)["off"], false);
        assert_eq!(off_note(&keeping, &places.state), None);
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first builder, 18:41 on 18 Nov (no offer): told one of theirs is
    /// off, a line that asks to use it calls its function as any skill's;
    /// the lamp turns it on for the owner's ask, by `make_skill`'s own
    /// `turn_on` of its said name, and the model says one line with its
    /// undo, in the conversation's language. A guest's turn on is refused
    /// and told plainly, never said on.
    #[test]
    fn a_turned_off_skill_asked_for_use_is_turned_on_with_an_undo_line() {
        use crate::skill_about;
        made_skill::set_off_said_here(Some(true));
        yes::clear_for_test();
        let (root, keeping, places) = uploaded_btc("turn-off-use");
        let off_file = places.state.join(skill_about::OFF_FILE);
        let helper = kept_in(&off_file);
        let mut off = ask("btc price", true);
        off.state = Some(&places.state);
        asked(off, &keeping, owner, helper);
        let note = off_note(&keeping, &places.state).unwrap();
        assert!(note.contains("call its function"), "{note}");
        assert!(note.contains("the lamp turns it on"), "{note}");
        assert!(!note.contains("Never turn it on"), "{note}");
        let (name, args) = on_for_ask("btc_price", &json!({}));
        assert_eq!(name, made_skill::MAKE);
        assert_eq!(args, json!({ TURN_ON: "btc price" }));
        let told = turned_on_for_ask("btc price", &json!({"price": "90,000 dollars"}));
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("\"Btc price was off, so I turned it on. Say 'turn it off' to undo.\""),
            "{told}"
        );
        assert!(next.contains("conversation's language"), "{told}");
        assert_eq!(told["ran"]["price"], "90,000 dollars", "{told}");
        // A guest's: the turn on refused by `asked`, told plainly.
        let mut on = ask("btc price", false);
        on.state = Some(&places.state);
        let refused = asked(on, &keeping, || false, helper);
        let told = not_turned_on(&refused);
        assert_eq!(told["not_turned_on"], NOT_OWNER, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("didn't run") && next.contains("Never say it is on"));
        assert!(
            skill_about::turned_off(&places.state)
                .unwrap()
                .contains("btc-price")
        );
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// At most eight of theirs are offered, newest first.
    #[test]
    fn at_most_eight_of_theirs_are_offered() {
        yes::clear_for_test();
        let (root, keeping) = keeping("turn-offered-most");
        let made: Vec<String> = (0..12).map(|at| format!("skill-{at}")).collect();
        let pairs: Vec<(&str, bool)> = made.iter().map(|name| (name.as_str(), true)).collect();
        made_here(&keeping, &pairs);
        let offered = offered(&keeping, true, None);
        assert_eq!(offered.len(), OFFERED_MOST);
        assert_eq!(offered[0]["name"], "skill 11");
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

    /// The remove of `name` asked in `request` at `now`: its wait's id.
    fn remove_waits(keeping: &Keeping, name: &str, request: u64, now: Instant) -> String {
        let told = made_remove::asked(
            made_remove::Ask {
                name,
                request,
                heard: true,
                sends_here: true,
            },
            keeping,
            owner,
            now,
        );
        let id = told[yes::APPROVE_ID].as_str().unwrap().to_owned();
        assert!(yes::waits(&id), "{told}");
        id
    }

    /// Twelve's P2 on #7453: the lamp asks "Remove coffee time?" and the
    /// person says "no, just turn it off". The turn drops that remove's
    /// wait, turned off or back on alike: no waiting note names it, and a
    /// later yes finds nothing to take. A remove of another skill stays.
    #[test]
    fn a_turn_drops_a_remove_of_the_same_skill() {
        for off in [true, false] {
            let (root, keeping) = nines_lamp("turn-drops-remove");
            let now = Instant::now();
            let id = remove_waits(&keeping, "coffee time", 81, now);
            let told = asked(ask("coffee time", off), &keeping, owner, |_, _| Ok(()));
            assert_eq!(told["off"], off, "{told}");
            assert!(!yes::waits(&id), "{told}");
            assert!(!yes::awaited(now));
            assert!(yes::waiting_note(83, now).is_none());
            assert_eq!(names(&keeping), ["coffee-time", "moon-fact"]);
            let id = remove_waits(&keeping, "moon fact", 83, now);
            asked(ask("coffee time", off), &keeping, owner, |_, _| Ok(()));
            assert!(yes::waits(&id));
            yes::clear_for_test();
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// `make_skill`'s call `args` answered as the voice answers it, in a
    /// turn nobody spoke: what each path says then is its own, so which
    /// path took the call shows, and nothing on the lamp is read.
    fn answered_unheard(args: Value) -> Value {
        let call = lamp_gemini::ToolCall {
            id: "1".into(),
            name: made_skill::MAKE.into(),
            args,
        };
        crate::make_skill_local::answer(&call, 84, false, None, None)
    }

    /// Switch on: a `turn_off` or `turn_on` call is taken by `answer` as a
    /// turn, never as no request.
    #[test]
    fn switch_on_a_turn_call_is_routed_as_a_turn() {
        made_skill::set_off_said_here(Some(true));
        for key in [TURN_OFF, TURN_ON] {
            let told = answered_unheard(json!({ key: "coffee time" }));
            assert_eq!(told["error"], NOT_ASKED_ALOUD, "{key}: {told}");
        }
        made_skill::set_off_said_here(None);
    }

    /// #7562's row on lamp-9adf, 18 Nov, request 9 ("Lamp, tell me a space
    /// fact with my space facts skill."): switch on, one call that carries
    /// a `turn_on` and a run now is the run now, which turns it on itself
    /// (`made_run::asked_turning`; a run with no lamp-skills folder here is
    /// unavailable, a turn unheard is `NOT_ASKED_ALOUD`). Switch off, as
    /// before: a run now.
    #[test]
    fn a_turn_on_with_a_run_now_is_the_run_now() {
        for switch in [true, false] {
            made_skill::set_off_said_here(Some(switch));
            let told = answered_unheard(json!({
                TURN_ON: "space facts",
                crate::made_run::RUN: "space facts"
            }));
            assert_eq!(told, crate::tools::unavailable(), "{switch}: {told}");
        }
        made_skill::set_off_said_here(None);
    }

    /// Switch on: one call that carries both a turn-off and a remove is
    /// read as the turn-off, before the remove, and never asks a remove.
    #[test]
    fn switch_on_a_turn_off_with_a_remove_is_a_turn_off() {
        made_skill::set_off_said_here(Some(true));
        yes::clear_for_test();
        let now = Instant::now();
        let told = answered_unheard(json!({
            TURN_OFF: "coffee time",
            made_remove::REMOVE: "coffee time",
        }));
        assert_eq!(told["error"], NOT_ASKED_ALOUD, "{told}");
        assert_ne!(told["error"], made_remove::NOT_ASKED_ALOUD, "{told}");
        assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
        assert!(!yes::awaited(now));
        made_skill::set_off_said_here(None);
    }
}
