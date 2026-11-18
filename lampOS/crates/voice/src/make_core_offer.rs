//! A make that asks for what a skill on the lamp already does, turned off,
//! answered by turning that skill on instead of making a second one
//! (#7747, `LAMP_MAKE_CORE_OFFER`, off unless `on`). On lamp-9adf three
//! water reminders made by voice sat beside the core `sip-of-water`, none
//! with its guards: the person asked for a better water reminder and got a
//! second, worse one. Turned on, never offered: the first builder's rule of
//! 18 Nov 18:41 (Queue #6811), "offers become the action plus one line
//! with an undo".
//!
//! - **A model judges, before anything is made, within [`JUDGE_WITHIN`].**
//!   A voice `make_skill` call with a `request` first lists the candidates
//!   ([`candidates`]): every skill on the lamp the person didn't make that
//!   is turned off, and the few on (or off in their files) whose words come
//!   nearest the request (`skill_about::nearest`), at most [`LISTED_MOST`].
//!   With none, the make goes on at once. Else one short call to the make's
//!   own chat asks which one, if any, does the same thing on the same
//!   trigger and timing ([`question`]): a different timing ("every 20
//!   minutes" against a nudge after a long sitting) is no match. Whether one
//!   matches is that model's judgment, never a list of words (the owner's
//!   ruling, 13 Nov). "none", an answer naming no candidate, a timeout or an
//!   error: the make goes on as before.
//! - **It matches and is turned off by voice or in the app:** it is turned
//!   on at once by the manage helper's own `{"on": NAME}`, as the app's
//!   switch asks it, nothing is made, and the lamp says one line with its
//!   undo ([`turned_on_line`]).
//! - **It matches but can't be turned on** (already on, off in its files,
//!   or the helper refused): the make goes on as asked, and nothing is said
//!   of that skill.
//! - **The undo:** "make my own instead", or the like, judged by the voice
//!   model, comes back as [`UNDO`] alone within [`UNDO_WITHIN`] ([`undo`]):
//!   that one skill is turned back off and the person's first ask is made,
//!   once.
//!
//! Off, `make_skill`'s parameters and answers are byte for byte as before.
use crate::second_provider::net::{ChatEvent, Message};
use crate::skill_about::{self, Skill, State};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The switch, in lamp-voice alone: `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_CORE_OFFER";

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// The `make_skill` argument that undoes a skill turned on in place of a
/// make: true, given alone.
pub const UNDO: &str = "make_own_instead";

/// The longest the judge is waited for; past it the make goes on.
pub const JUDGE_WITHIN: Duration = Duration::from_secs(2);

/// The most bytes of the judge's answer read.
const JUDGE_MOST_BYTES: usize = 400;

/// How long a turn-on in place of a make may be undone.
pub const UNDO_WITHIN: Duration = Duration::from_secs(300);

/// The most skills the judge is shown.
pub const LISTED_MOST: usize = 40;

/// The most bytes of a skill's description shown.
pub const DOES_MOST: usize = 200;

/// What [`UNDO`]'s parameter says.
pub const UNDO_TOLD: &str = "Only when the lamp just turned on a skill in place of making one and the person asks, in any words, to make their own instead: true, given alone with no request.";

/// What the model is told of an undo with nothing to undo.
pub const NOTHING_TO_UNDO: &str =
    "no skill was turned on in place of a make in the last few minutes";

/// The judge's system prompt.
const JUDGE_SYSTEM: &str = "You check whether a skill already on a desk lamp does what a person \
                            asks a new skill to do. Answer one skill's name exactly as listed, \
                            or none.";

/// The skill turned on in place of a make, the person's ask for it, and
/// when: what [`UNDO`] undoes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turned {
    pub name: String,
    pub request: String,
    pub at: Instant,
}

/// The newest turn-on in place of a make.
static TURNED: Mutex<Option<Turned>> = Mutex::new(None);

fn slot(turned: &Mutex<Option<Turned>>) -> std::sync::MutexGuard<'_, Option<Turned>> {
    turned
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `text` cut to at most `most` bytes, on a character's edge.
fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn does(skill: &Skill) -> &str {
    cut(
        skill.description.as_deref().unwrap_or("no description"),
        DOES_MOST,
    )
    .trim()
}

/// The skills a make's `request` is judged against, of `skills` on the
/// lamp: those the person didn't make that load, every one turned off by
/// voice or in the app, and of the rest the few nearest the request
/// (`skill_about::nearest`); at most [`LISTED_MOST`].
pub fn candidates<'a>(request: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let theirs: Vec<Skill> = skills
        .iter()
        .filter(|skill| !skill.made_here && skill.state != State::DoesNotLoad)
        .cloned()
        .collect();
    let near: Vec<String> = skill_about::nearest(request, &theirs)
        .into_iter()
        .map(|skill| skill.name.clone())
        .collect();
    skills
        .iter()
        .filter(|skill| !skill.made_here && skill.state != State::DoesNotLoad)
        .filter(|skill| skill.state == State::TurnedOff || near.contains(&skill.name))
        .take(LISTED_MOST)
        .collect()
}

/// The judge's question: the request and the `candidates`, each its name
/// and what it does.
pub fn question(request: &str, candidates: &[&Skill]) -> String {
    let lines: Vec<String> = candidates
        .iter()
        .map(|skill| format!("- {}: {}", skill.name, does(skill)))
        .collect();
    format!(
        "Asked for a new skill: {request}\nSkills on the lamp:\n{}\nDoes one of them already do \
         what was asked: the same thing, on the same trigger and the same timing? A different \
         timing or trigger is no match: \"every 20 minutes\" or \"at 3 pm\" is not a nudge \
         after a long sitting. Answer one name exactly as listed, or none.",
        lines.join("\n")
    )
}

/// The candidate the judge's `answer` names; none for "none" or anything
/// else.
fn judged<'a>(answer: &str, candidates: &[&'a Skill]) -> Option<&'a Skill> {
    let answer = answer
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    candidates
        .iter()
        .find(|skill| skill.name.to_lowercase() == answer)
        .copied()
}

/// The one line said when the skill `name` was turned on in place of a
/// make, with its undo.
pub fn turned_on_line(name: &str) -> String {
    let spoken = crate::made_skill::spoken_name(name);
    let mut spoken_chars = spoken.chars();
    let spoken = spoken_chars
        .next()
        .map(|first| first.to_uppercase().chain(spoken_chars).collect::<String>())
        .unwrap_or_default();
    format!("{spoken} already does that, so I turned it on. Say 'make my own instead' to undo.")
}

/// A `make_skill` call's `args` before a make, with the switch on, in the
/// voice's request `request`: when `judge` names one of the [`candidates`]
/// `skills` reads that is turned off by voice or in the app, and `turn`
/// turns it on, what the model is told in place of a make (kept in
/// `turned` for its undo). Else none, to make as asked: the skills not
/// read, no candidate (the judge not asked), the judge's "none", another
/// answer, a timeout or an error, or a match that is on, off in its files,
/// or not turned on.
pub fn before_make(
    args: &Value,
    request: u64,
    turned: &Mutex<Option<Turned>>,
    skills: impl FnOnce() -> Result<Vec<Skill>, String>,
    judge: impl FnOnce(&str) -> Option<String>,
    turn: impl FnOnce(&str) -> Result<(), String>,
    now: Instant,
) -> Option<Value> {
    if !on() {
        return None;
    }
    let asked = args["request"].as_str().unwrap_or_default();
    let skills = match skills() {
        Ok(skills) => skills,
        Err(why) => {
            eprintln!(
                "lamp-voice: request {request}: the lamp's skills couldn't be read before a make, made as asked: {why}"
            );
            return None;
        }
    };
    let candidates = candidates(asked, &skills);
    if candidates.is_empty() {
        return None;
    }
    let Some(answer) = judge(&question(asked, &candidates)) else {
        eprintln!(
            "lamp-voice: request {request}: the judge of the skills on the lamp didn't answer in time, made as asked (#7747)"
        );
        return None;
    };
    let Some(skill) = judged(&answer, &candidates) else {
        eprintln!(
            "lamp-voice: request {request}: no skill of {} on the lamp does it, made as asked (#7747)",
            candidates.len()
        );
        return None;
    };
    if skill.state != State::TurnedOff {
        eprintln!(
            "lamp-voice: request {request}: a skill on the lamp does it but can't be turned on here, made as asked (#7747)"
        );
        return None;
    }
    if let Err(why) = turn(&skill.name) {
        eprintln!(
            "lamp-voice: request {request}: a skill on the lamp does it but wasn't turned on, made as asked: {why}"
        );
        return None;
    }
    eprintln!(
        "lamp-voice: request {request}: a skill on the lamp does it: turned on, nothing made (#7747)"
    );
    *slot(turned) = Some(Turned {
        name: skill.name.clone(),
        request: asked.to_owned(),
        at: now,
    });
    let line = turned_on_line(&skill.name);
    Some(json!({
        "not_made": "nothing was made: a skill on the lamp already does this, and it was turned on",
        "turned_on": skill.name,
        "next": format!(
            "Say only this line, once, in the conversation's language: \"{line}\" Call nothing."
        ),
    }))
}

/// [`before_make`] on the lamp: the product's places with the voice's
/// skills folder `skills`, the make's own chat as the judge, the manage
/// helper's socket.
pub fn before_make_here(args: &Value, request: u64, skills: Option<&Path>) -> Option<Value> {
    let present = std::env::var(skill_about::PRESENT_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    let manage = Path::new(crate::made_skill::MANAGE_SOCKET);
    before_make(
        args,
        request,
        &TURNED,
        || {
            skill_about::skills(
                &skill_about::Places::product(skills),
                lamp_ipc::monotonic_us(),
                present,
            )
        },
        judge_here,
        |name| crate::made_turn::turned_by_helper(manage, name, false),
        Instant::now(),
    )
}

/// The judge's answer to `question` from the make's own chat, on a thread
/// of its own, within [`JUDGE_WITHIN`]: none on a timeout or an error. Its
/// answer comes by a channel, never a join, so a hung lookup never holds
/// the make past the bound (as `make_skill_local`'s judge).
fn judge_here(question: &str) -> Option<String> {
    let (sent, answered) = std::sync::mpsc::sync_channel(1);
    let question = question.to_owned();
    std::thread::Builder::new()
        .name("lamp-voice-core-offer".into())
        .spawn(move || {
            let _ = sent.send(judge_once(&question));
        })
        .ok()?;
    answered
        .recv_timeout(JUDGE_WITHIN + Duration::from_millis(100))
        .ok()
        .flatten()
}

fn judge_once(question: &str) -> Option<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let chat = crate::make_skill_local::own_chat().ok()?;
    runtime.block_on(async {
        tokio::time::timeout(JUDGE_WITHIN, async {
            let mut stream = chat
                .send(&[Message::system(JUDGE_SYSTEM), Message::user(question)])
                .await
                .ok()?;
            let mut text = String::new();
            while let Some(event) = stream.next().await.ok()? {
                if let ChatEvent::Text(words) = event {
                    text.push_str(&words);
                    if text.len() > JUDGE_MOST_BYTES {
                        break;
                    }
                }
            }
            Some(text)
        })
        .await
        .ok()
        .flatten()
    })
}

/// The undo of the newest turn-on in `turned`, asked in the voice's
/// request `request` at `now`: within [`UNDO_WITHIN`], that one skill
/// turned back off by `turn_off`, then the person's first ask, to make as
/// asked (the undo spent). Else what the model is told: nothing to undo,
/// or why it wasn't turned off (the undo kept, to ask again).
pub fn undo(
    request: u64,
    turned: &Mutex<Option<Turned>>,
    now: Instant,
    turn_off: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, Value> {
    let mut kept = slot(turned);
    let Some(done) = kept
        .clone()
        .filter(|done| now.saturating_duration_since(done.at) <= UNDO_WITHIN)
    else {
        *kept = None;
        eprintln!("lamp-voice: request {request}: an undo with nothing to undo (#7747)");
        return Err(json!({
            "error": NOTHING_TO_UNDO,
            "next": "Say plainly, in a few words, that there's nothing to undo, and ask what skill \
                     they'd like made. Call nothing now."
        }));
    };
    if let Err(why) = turn_off(&done.name) {
        let spoken = crate::made_skill::spoken_name(&done.name);
        eprintln!("lamp-voice: request {request}: an undo didn't turn the skill back off: {why}");
        return Err(json!({
            "error": why,
            "next": format!(
                "Say plainly, in a few words, in the conversation's language, that {spoken} \
                 couldn't be turned back off, so nothing was made: {why}. Call nothing."
            )
        }));
    }
    *kept = None;
    eprintln!(
        "lamp-voice: request {request}: a skill turned on in place of a make turned back off; the first ask is made (#7747)"
    );
    Ok(done.request)
}

/// [`undo`] on the lamp, by `make_skill`'s call: the newest turn-on, the
/// manage helper's socket.
pub fn undone_here(request: u64) -> Result<String, Value> {
    let manage = Path::new(crate::made_skill::MANAGE_SOCKET);
    undo(request, &TURNED, Instant::now(), |name| {
        crate::made_turn::turned_by_helper(manage, name, true)
    })
}

/// `make_skill`'s parameters with the switch on: [`UNDO`] added; off,
/// untouched.
pub fn with_parameters(parameters: &mut Value) {
    if !on() {
        return;
    }
    parameters["properties"][UNDO] = json!({ "type": "boolean", "description": UNDO_TOLD });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIP: &str = "One short nudge to drink some water after a long sitting at the desk, \
                       at most three a day, never mid-conversation.";

    /// #7747's ask that Sip of water already does.
    const SAT: &str = "remind me to drink water when I've sat a while";

    fn skill(name: &str, does: &str, state: State, made_here: bool) -> Skill {
        Skill {
            name: name.to_owned(),
            description: Some(does.to_owned()),
            state,
            made_here,
            task: false,
            ran_us: None,
        }
    }

    /// lamp-9adf on 18 Nov, more skills than any one match: the core water
    /// reminder as `sip`, Stretch break on, Guard mode off in its files,
    /// Dance on, Welcome back turned off, and a skill the person made.
    fn nines_lamp(sip: State) -> Vec<Skill> {
        vec![
            skill("dance", "Dances.", State::On, false),
            skill(
                "guard-mode",
                "Watches the desk.",
                State::InstalledOff,
                false,
            ),
            skill("sip-of-water", SIP, sip, false),
            skill(
                "stretch-break",
                "A short stretch after a long sitting.",
                State::On,
                false,
            ),
            skill(
                "welcome-back",
                "Says welcome back when you return.",
                State::TurnedOff,
                false,
            ),
            skill("coffee-time", "Says coffee time at 3 pm.", State::On, true),
        ]
    }

    fn ask(request: &str) -> Value {
        json!({ "request": request })
    }

    fn never_judged(_: &str) -> Option<String> {
        panic!("the judge asked")
    }

    fn never_turned(_: &str) -> Result<(), String> {
        panic!("turned on")
    }

    /// `request` on `lamp`, the judge naming `named`; what is told, what was
    /// turned on and what is kept for the undo.
    fn made(
        lamp: Vec<Skill>,
        request: &str,
        named: Option<&str>,
        turn: Result<(), String>,
    ) -> (Option<Value>, Option<String>, Option<Turned>) {
        set_on_here(Some(true));
        let slot = Mutex::new(None);
        let mut turned = None;
        let told = before_make(
            &ask(request),
            4,
            &slot,
            || Ok(lamp),
            |_| named.map(str::to_owned),
            |name| {
                turned = Some(name.to_owned());
                turn
            },
            Instant::now(),
        );
        set_on_here(None);
        let kept = slot.lock().unwrap().clone();
        (told, turned, kept)
    }

    /// #7747 and the first builder's 18:41: a make whose ask a turned-off
    /// skill on the lamp already does turns that skill on at once, with no
    /// offer and no yes; nothing is made, and one line is said once, with
    /// its undo. The judge is shown the turned-off skills and the nearest
    /// others, never the person's own.
    #[test]
    fn a_make_a_turned_off_core_skill_does_turns_it_on_with_one_line_and_its_undo() {
        set_on_here(Some(true));
        let slot = Mutex::new(None);
        let mut asked = String::new();
        let mut turned = Vec::new();
        let told = before_make(
            &ask(SAT),
            1,
            &slot,
            || Ok(nines_lamp(State::TurnedOff)),
            |question| {
                asked = question.to_owned();
                Some("sip-of-water".into())
            },
            |name| {
                turned.push(name.to_owned());
                Ok(())
            },
            Instant::now(),
        )
        .expect("turned on, not made");
        set_on_here(None);
        assert!(asked.contains(&format!("- sip-of-water: {SIP}")), "{asked}");
        assert!(asked.contains("- welcome-back: "), "turned off: {asked}");
        assert!(!asked.contains("coffee"), "{asked}");
        assert!(!asked.contains("dance"), "not near: {asked}");
        assert!(asked.contains("A different timing or trigger is no match"));
        assert_eq!(turned, ["sip-of-water"]);
        assert_eq!(told["turned_on"], "sip-of-water");
        let line = "Sip of water already does that, so I turned it on. Say 'make my own instead' \
                    to undo.";
        let next = told["next"].as_str().unwrap();
        assert_eq!(next.matches(line).count(), 1, "{next}");
        assert!(next.contains("once"), "{next}");
        assert!(!next.contains('?'), "nothing asked: {next}");
        let kept = slot.lock().unwrap().clone().unwrap();
        assert_eq!(
            (kept.name.as_str(), kept.request.as_str()),
            ("sip-of-water", SAT)
        );
        // Only the person's own skills: no judge, made at once; none read.
        set_on_here(Some(true));
        let theirs = || Ok(vec![skill("coffee-time", "Coffee.", State::On, true)]);
        let only_theirs = before_make(
            &ask("glow blue"),
            2,
            &slot,
            theirs,
            never_judged,
            never_turned,
            Instant::now(),
        );
        let unreadable = before_make(
            &ask("glow blue"),
            3,
            &slot,
            || Err("no folder".into()),
            never_judged,
            never_turned,
            Instant::now(),
        );
        set_on_here(None);
        assert_eq!((only_theirs, unreadable), (None, None));
    }

    /// A match that can't be turned on (already on, off in its files, or the
    /// helper refused) goes straight on to make as asked: nothing told of
    /// it, nothing kept for an undo.
    #[test]
    fn a_match_that_cant_be_turned_on_is_made_as_asked_and_never_named() {
        for (sip, request, named, turn) in [
            (State::On, SAT, "sip-of-water", Ok(())),
            (State::Snoozed, SAT, "sip-of-water", Ok(())),
            (
                State::TurnedOff,
                "watch my desk when I'm away",
                "guard-mode",
                Ok(()),
            ),
            (
                State::TurnedOff,
                SAT,
                "sip-of-water",
                Err("refused".to_owned()),
            ),
        ] {
            let refused = turn.is_err();
            let (told, turned, kept) = made(nines_lamp(sip), request, Some(named), turn);
            assert_eq!(told, None, "{named}");
            assert_eq!(kept, None, "{named}");
            assert_eq!(
                turned.is_some(),
                refused,
                "{named}: asked only of one turned off"
            );
        }
    }

    /// Twelve's and El's P2s on #7763: a different timing ("every 20
    /// minutes" against a nudge after a long sitting), a judge that answers
    /// none or a skill not listed, or one that doesn't answer in time, goes
    /// on to make; nothing is turned on.
    #[test]
    fn a_different_timing_or_a_lost_judge_goes_on_to_make() {
        for answer in [Some("none"), Some("None."), Some("guard-mode-2"), None] {
            set_on_here(Some(true));
            let slot = Mutex::new(None);
            let told = before_make(
                &ask("remind me to drink water every 20 minutes"),
                6,
                &slot,
                || Ok(nines_lamp(State::TurnedOff)),
                |question| {
                    assert!(question.contains("\"every 20 minutes\""), "{question}");
                    answer.map(str::to_owned)
                },
                never_turned,
                Instant::now(),
            );
            set_on_here(None);
            assert_eq!(told, None, "{answer:?}");
            assert_eq!(*slot.lock().unwrap(), None, "{answer:?}");
        }
    }

    /// The undo: within the minutes, that one skill turned back off, then
    /// the person's first ask made as asked, once; a second undo, a late
    /// one, or one with nothing turned on, turns nothing off; a helper's
    /// refusal is said and the undo kept.
    #[test]
    fn the_undo_turns_it_back_off_and_makes_the_first_ask() {
        let (_, _, kept) = made(
            nines_lamp(State::TurnedOff),
            SAT,
            Some("sip-of-water"),
            Ok(()),
        );
        let kept = kept.unwrap();
        let at = kept.at;
        let slot = Mutex::new(Some(kept.clone()));
        let refused = undo(8, &slot, at, |_| {
            Err("the lamp's skills couldn't be reached".into())
        });
        assert_eq!(
            refused.unwrap_err()["error"],
            "the lamp's skills couldn't be reached"
        );
        let mut off = Vec::new();
        let undone = undo(9, &slot, at + Duration::from_secs(30), |name| {
            off.push(name.to_owned());
            Ok(())
        });
        assert_eq!(undone.as_deref(), Ok(SAT));
        assert_eq!(off, ["sip-of-water"]);
        let again = undo(10, &slot, at, never_turned);
        assert_eq!(again.unwrap_err()["error"], NOTHING_TO_UNDO);
        let late_slot = Mutex::new(Some(kept));
        let late = undo(
            11,
            &late_slot,
            at + UNDO_WITHIN + Duration::from_secs(1),
            never_turned,
        );
        assert_eq!(late.unwrap_err()["error"], NOTHING_TO_UNDO);
        assert_eq!(*late_slot.lock().unwrap(), None);
    }

    /// El's P3 on #7763: the judge is shown at most LISTED_MOST skills.
    #[test]
    fn the_judge_is_shown_at_most_forty_skills() {
        let lamp: Vec<Skill> = (0..50)
            .map(|n| skill(&format!("skill-{n:02}"), "Glows.", State::TurnedOff, false))
            .collect();
        assert_eq!(candidates("glow", &lamp).len(), LISTED_MOST);
    }

    /// #7747: off, nothing changes: no read, no judge, nothing turned on,
    /// and make_skill's parameters are byte for byte as before.
    #[test]
    fn the_switch_off_changes_nothing() {
        set_on_here(Some(false));
        let slot = Mutex::new(None);
        let off = before_make(
            &ask("remind me to drink water"),
            9,
            &slot,
            || panic!("the skills read with the switch off"),
            never_judged,
            never_turned,
            Instant::now(),
        );
        let before = json!({"type": "object", "properties": {"request": {}}});
        let mut parameters = before.clone();
        with_parameters(&mut parameters);
        set_on_here(None);
        assert_eq!(off, None);
        assert_eq!(parameters.to_string(), before.to_string());
        set_on_here(Some(true));
        with_parameters(&mut parameters);
        set_on_here(None);
        assert_eq!(parameters["properties"][UNDO]["type"], "boolean");
        assert_eq!(parameters["properties"].as_object().unwrap().len(), 2);
    }
}
