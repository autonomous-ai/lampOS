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
pub const BACK_ON_BY_CALL: &str = "A skill turned off is turned back on only by calling make_skill \
     with turn_on and its name, when their line asks to turn it on; never say it is on or back on \
     unless that call's answer says so.";

/// What the model is told when it calls the function of a skill the person
/// made or added that is turned off (`lamp_skills::TOOL_TURNED_OFF` is
/// this very text): it ran nothing.
pub const FUNCTION_OFF: &str = "That skill is turned off, so it did nothing. Say so in a few \
     words: it is off and didn't run, and they can ask to turn it back on. Never turn it on for \
     this: it is turned back on only by calling make_skill with turn_on and its name, when their \
     line asks to turn it on; never say it is on or back on unless that call's answer says so.";

/// What the model is told of a `turn_on` called in a turn where a skill's
/// function answered [`FUNCTION_OFF`].
pub const NOT_ASKED_ON: &str =
    "a skill turned off is turned back on only when the person asks to turn it on, never to use it";

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
         line that asks to use or run one runs nothing: it is off and didn't run, so say what \
         its function's answer says. Never turn it on for that. {BACK_ON_BY_CALL})"
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

/// Whether the call `name` with `args` is a `make_skill` turn back on, as
/// `make_skill_local` reads it (a turn-off beside it is a turn-off), with
/// the switch on.
pub fn turns_on(name: &str, args: &Value) -> bool {
    name == made_skill::MAKE
        && made_skill::off_said_on()
        && args[TURN_ON].is_string()
        && !args[TURN_OFF].is_string()
}

/// The words a line asks a skill on by, each alone or as a run in order:
/// the short fixed set of the first builder's 12:51 ruling on #7683 ("turn
/// on", "back on", "switch on", "enable", "put back on", "resume",
/// "unpause", Vietnamese "bật"), lower case. "Put back on" is "back on".
const ON_ASKS: [&[&str]; 6] = [
    &["turn", "on"],
    &["back", "on"],
    &["switch", "on"],
    &["enable"],
    &["resume"],
    &["unpause"],
];

/// The Vietnamese word a line asks a skill on by ("bật lại btc price").
const ON_ASK_VI: &str = "bật";

/// The verbs whose "on" may close the line apart from them ("turn btc price
/// on", "switch my moon fact skill on").
const ON_VERBS: [&str; 2] = ["turn", "switch"];

/// Whether `line` asks to turn something on: one of [`ON_ASKS`] as a run of
/// its words, [`ON_ASK_VI`], or one of [`ON_VERBS`] with "on" its last word.
/// Never a use, a run or a play ("use my bitcoin skill", "run btc price").
/// Kept to the fixed set on purpose: a line it misses costs the person
/// saying "turn it on"; one it took wrongly would turn on a skill they
/// turned off.
pub fn asks_on(line: &str) -> bool {
    let words = crate::agent_tasks::plain_words(line);
    let run_at = |at: usize, run: &[&str]| {
        words.len() - at >= run.len() && run.iter().zip(&words[at..]).all(|(a, b)| a == b)
    };
    (0..words.len()).any(|at| ON_ASKS.iter().any(|run| run_at(at, run)))
        || words.iter().any(|word| word == ON_ASK_VI)
        || (words.last().is_some_and(|last| last == "on")
            && words.iter().any(|word| ON_VERBS.contains(&word.as_str())))
}

/// Whether the call `name` with `args` is a `turn_on` ([`turns_on`]) that
/// is not run (#7562 point 2, the first builder's ranking, 18 Nov: no
/// `turn_on` of a turned-off skill in any order on a line that didn't ask
/// it on): the person's heard `line` doesn't ask it on ([`asks_on`]), or it
/// isn't the turn's only `make_skill` call (`makes_in_turn`, itself
/// counted), as #7683's strict yes holds a remove. "Lamp, use my btc price
/// skill." never turns it on, whether the model calls `turn_on` before the
/// skill's function, after it, or alone. A plain yes never lets the model's
/// own `turn_on` through (Thirteen's P2, 18 Nov): only the lamp's offer
/// ([`Offer`]) takes a yes, read in code, for its own skill, within its wait.
pub fn on_refused(name: &str, args: &Value, line: &str, makes_in_turn: usize) -> bool {
    turns_on(name, args) && !(makes_in_turn == 1 && asks_on(line))
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

/// How long the offer to turn a skill back on waits for its yes ([`Offer`]).
pub const OFFER_FOR: Duration = Duration::from_secs(60);

/// The ids of the calls a yes to the offer makes in code: the `turn_on`,
/// then the skill's own call for the ask. Ids only: the second route knows
/// these calls by the round it made in code, never by their ids.
pub const OFFER_ON_ID: &str = "turned_off_offer_on";
pub const OFFER_RUN_ID: &str = "turned_off_offer_run";

/// The offer to turn a skill back on and run it (#7684, the first builder's
/// 11:05 ruling on #7562): a heard line asked to use a skill the person
/// made or added that is turned off, in `request`, at `at`; its function's
/// call `name` with `args` is the ask a yes runs once.
#[derive(Clone, Debug)]
pub struct Offer {
    pub request: u64,
    pub at: std::time::Instant,
    pub name: String,
    pub args: Value,
}

impl Offer {
    /// The skill's name as said ("btc price" for `btc_price`).
    pub fn spoken(&self) -> String {
        crate::skill_named::spoken(crate::skill_named::function_of(&self.name, &self.args))
    }

    /// Whether the heard `line` of `request`, at `now`, takes the offer: the
    /// very next line, within [`OFFER_FOR`], a plain yes ([`offer_yes`]).
    pub fn taken_by(&self, request: u64, line: &str, now: std::time::Instant) -> bool {
        request != self.request
            && now.saturating_duration_since(self.at) < OFFER_FOR
            && offer_yes(line)
    }

    /// The `turn_on` a yes makes, by the skill's said name.
    pub fn turn_on_args(&self) -> Value {
        json!({ TURN_ON: self.spoken() })
    }
}

/// What the model is told of a turned-off skill's function asked for use
/// on a heard line (#7684), in place of [`FUNCTION_OFF`]: the offer, said
/// as it is, and nothing turned on.
pub fn offer_told(spoken: &str) -> Value {
    let mut said = spoken.chars();
    let said: String = said
        .next()
        .map(|first| first.to_uppercase().chain(said).collect())
        .unwrap_or_default();
    json!({
        "error": "that skill is turned off, so it did nothing",
        "next": format!(
            "Say only this, in the conversation's language: \"{said} is turned off. Want me to \
             turn it back on and check?\" Never turn it on now, and never say it is on. Call \
             nothing."
        )
    })
}

/// What the model is told before a yes to the offer is answered: the skill
/// was turned on and run for their ask, by the calls before it.
pub fn yes_note(spoken: &str) -> String {
    format!(
        "(For you to know: they said yes to turning {spoken} back on and checking, so the lamp \
         turned it on and ran it once for their ask, in the calls below. Say what it found in a \
         few words, and that it is on again; if a call says it didn't, say that plainly.)"
    )
}

/// What the model is told of its call of the skill the offer's yes ran in
/// code already (`ran`, that run's answer): never run again.
pub fn ran_already(ran: &Value) -> Value {
    json!({
        "error": "Not run again: their yes turned it back on and it was run for them already.",
        "ran": ran,
        "next": "Say only what its result above tells you to say. Call nothing."
    })
}

/// The yes words an offer takes ([`offer_yes`]), lower case.
const OFFER_YES: [&str; 11] = [
    "yes", "yeah", "yep", "yup", "sure", "ok", "okay", "vâng", "ừ", "có", "được",
];
/// The yes phrases an offer takes.
const OFFER_YES_PHRASES: [&[&str]; 5] = [
    &["go", "ahead"],
    &["do", "it"],
    &["turn", "it", "on"],
    &["bật", "lại"],
    &["bật", "lên"],
];
/// The words a plain yes may hold beside its yes.
const OFFER_BESIDE: [&str; 5] = ["please", "lamp", "hey", "đi", "nhé"];

/// Whether the heard `line` is a plain yes to the offer and nothing else:
/// at most six words, no question, only [`OFFER_YES`] words or
/// [`OFFER_YES_PHRASES`] beside [`OFFER_BESIDE`] ("Yes.", "Lamp, yeah, go
/// ahead.", "Okay, please.", "Vâng.", "Có.", "Được, bật lại đi."). "Yes, and
/// play some music." or "Yes?" is not. Read in code, as #7683's strict
/// remove-yes is.
// TODO(#7683): share #7683's plain-yes helper once it lands.
pub fn offer_yes(line: &str) -> bool {
    if line.trim_end().ends_with('?') {
        return false;
    }
    let words = crate::agent_tasks::plain_words(line);
    if words.is_empty() || words.len() > 6 {
        return false;
    }
    let (mut at, mut yes) = (0, false);
    while at < words.len() {
        let phrase = OFFER_YES_PHRASES.iter().find(|phrase| {
            words.len() - at >= phrase.len() && phrase.iter().zip(&words[at..]).all(|(a, b)| a == b)
        });
        if let Some(phrase) = phrase {
            yes = true;
            at += phrase.len();
        } else if OFFER_YES.contains(&words[at].as_str()) {
            yes = true;
            at += 1;
        } else if OFFER_BESIDE.contains(&words[at].as_str()) {
            at += 1;
        } else {
            return false;
        }
    }
    yes
}

/// What the model is told, the call not run, of a `turn_on` refused
/// ([`on_refused`]; #7562's row on lamp-9adf, 18 Nov 10:53: "Lamp, use my
/// btc price skill." with btc price turned off was turned on and said
/// "back on", never run). Turning it on stays for a line that asks it on.
pub fn not_asked_on() -> Value {
    json!({
        "error": NOT_ASKED_ON,
        "next": "Say only, in a few words, in the conversation's language, that the skill is \
                 turned off, so it didn't run, and that they can say turn it back on whenever \
                 they like. Never say it is on. Call nothing."
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
            note.contains("only by calling make_skill with turn_on"),
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

    /// #7562's row on lamp-9adf, 18 Nov 10:53: told one of theirs is off,
    /// "Lamp, use my btc price skill." was answered with a `turn_on` and
    /// "Bitcoin price is back on.", never run. The note and the function's
    /// own answer say a use is told off and didn't run, never turned on.
    #[test]
    fn a_turned_off_skill_asked_for_use_is_told_off_never_turned_on() {
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
        for told in [note.as_str(), FUNCTION_OFF] {
            assert!(told.contains("didn't run"), "{told}");
            assert!(told.contains("Never turn it on for"), "{told}");
            assert!(told.contains("asks to turn it on"), "{told}");
        }
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7562 point 2: a `turn_on` stands only on a line that asks it on, as
    /// the turn's only `make_skill` call; a use, a run, a play, or a plain
    /// yes to whatever the lamp said last, never (Thirteen's P2: only the
    /// lamp's own offer takes a yes, in code).
    #[test]
    fn a_turn_on_stands_only_on_a_line_that_asks_it_on() {
        made_skill::set_off_said_here(Some(true));
        let on = json!({ TURN_ON: "btc price" });
        for line in [
            "Turn btc price back on.",
            "Turn it on.",
            "Bật lại btc price.",
        ] {
            assert!(!on_refused(made_skill::MAKE, &on, line, 1), "{line}");
            // Beside another skill change in the turn, refused.
            assert!(on_refused(made_skill::MAKE, &on, line, 2), "{line}");
        }
        for line in [
            "Use my bitcoin skill.",
            "Run btc price.",
            "Play btc price.",
            "Yes.",
            "Có.",
        ] {
            assert!(on_refused(made_skill::MAKE, &on, line, 1), "{line}");
        }
        // Not a turn on: never refused here.
        let off = json!({ TURN_OFF: "btc price" });
        assert!(!on_refused(made_skill::MAKE, &off, "Use it.", 1));
        made_skill::set_off_said_here(None);
    }

    /// #7684: the offer is taken only by a plain yes, read in code, on the
    /// very next line, within its wait.
    #[test]
    fn the_offer_is_taken_only_by_a_plain_yes_on_the_next_line() {
        for yes in [
            "Yes.",
            "Lamp, yeah, go ahead.",
            "Okay, please.",
            "Do it.",
            "Turn it on.",
            "Sure",
            "Vâng.",
            "Ừ.",
            "Có.",
            "Được.",
            "Có, bật lại đi.",
            "Ok được.",
        ] {
            assert!(offer_yes(yes), "{yes}");
        }
        for not in [
            "No.",
            "Yes?",
            "Yes, and play some music.",
            "What time is it?",
            "Yes yes yes yes yes yes yes.",
            "Có không?",
            "Không.",
            "",
        ] {
            assert!(!offer_yes(not), "{not}");
        }
        let at = std::time::Instant::now();
        let offer = Offer {
            request: 4,
            at,
            name: "btc_price".into(),
            args: json!({}),
        };
        assert_eq!(offer.spoken(), "btc price");
        assert_eq!(offer.turn_on_args(), json!({ TURN_ON: "btc price" }));
        assert!(offer.taken_by(5, "Yes.", at));
        assert!(!offer.taken_by(4, "Yes.", at), "its own line");
        assert!(!offer.taken_by(5, "Yes.", at + OFFER_FOR), "past its wait");
        let told = offer_told(&offer.spoken());
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("Btc price is turned off. Want me to turn it back on and check?"),
            "{told}"
        );
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
