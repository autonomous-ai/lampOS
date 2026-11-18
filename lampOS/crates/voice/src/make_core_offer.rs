//! A make that asks for what a skill on the lamp already does, offered
//! that skill instead of a second one (#7747, `LAMP_MAKE_CORE_OFFER`, off
//! unless `on`). On lamp-9adf three water reminders made by voice sat
//! beside the core `sip-of-water`, none with its guards: the person asked
//! for a better water reminder and got a second, worse one.
//!
//! - **A model judges, before anything is made, within [`JUDGE_WITHIN`].**
//!   A voice `make_skill` call with a `request` and no [`NEW`] first lists
//!   the candidates ([`candidates`]): every skill on the lamp the person
//!   didn't make that is turned off, and the few on (or off in their files)
//!   whose words come nearest the request (`skill_about::nearest`), at most
//!   [`LISTED_MOST`]. With none, the make goes on at once. Else one short
//!   call to the make's own chat asks which one, if any, does the same
//!   thing on the same trigger and timing ([`question`]): a different
//!   timing ("every 20 minutes" against a nudge after a long sitting) is
//!   no match. Whether one matches is that model's judgment, never a list of
//!   words (the owner's ruling, 13 Nov). "none", an answer naming no
//!   candidate, a timeout or an error: the make goes on as before. No
//!   round of the voice's own model is spent on it.
//! - **One matches:** nothing is made, never a silent swap. The voice
//!   model is told that one skill ([`offer_told`]): to say what it does as
//!   its description says, and never that its timing can be changed (no
//!   core skill exposes a setting the voice may write); one turned off is
//!   offered to be turned on, any other is said as it is, with whether they
//!   still want a new one.
//! - **A yes** comes back as [`CORE_ON`], the name alone ([`turned_on`]):
//!   only heard, only the owner's line, only that one offered skill, only
//!   one turned off by voice or in the app (never one off in its files),
//!   and only in the first heard line after the offer ([`line_heard`]),
//!   within [`OFFER_WITHIN`]. Turned on by the manage helper's own
//!   `{"on": NAME}`, as the app's switch and `crate::made_turn` ask it.
//! - **A no**, when they still want a new one, is the same request with
//!   [`NEW`]: made as asked, and the offer is let go.
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

/// The `make_skill` argument, with `request`, that makes the skill as
/// asked: the person heard which skill does it and still wants a new one.
pub const NEW: &str = "new_skill";

/// The `make_skill` argument that turns on the one skill the lamp just
/// offered in place of a make: its name, given alone.
pub const CORE_ON: &str = "core_on";

/// The longest the judge is waited for; past it the make goes on.
pub const JUDGE_WITHIN: Duration = Duration::from_secs(2);

/// The most bytes of the judge's answer read.
const JUDGE_MOST_BYTES: usize = 400;

/// How long an offer waits for the person's yes.
pub const OFFER_WITHIN: Duration = Duration::from_secs(60);

/// The most skills the judge is shown.
pub const LISTED_MOST: usize = 40;

/// The most bytes of a skill's description told.
pub const DOES_MOST: usize = 200;

/// What [`NEW`]'s parameter says.
pub const NEW_TOLD: &str = "With request only: true once the lamp said which skill on the lamp already does it and the person still wants a new one.";

/// What [`CORE_ON`]'s parameter says.
pub const CORE_ON_TOLD: &str = "Only when the lamp just offered to turn on a skill on the lamp that already does what the person asked, and they agree: its name as told, given alone with no request.";

/// What the model is told of a turn-on asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str =
    "a skill is turned on by voice only when the person asks out loud";

/// What the model is told of a turn-on asked by anyone but the owner.
pub const NOT_OWNER: &str = "only the lamp's owner can turn a skill on by voice";

/// What the model is told of a turn-on that doesn't answer the offer.
pub const NOT_OFFERED: &str = "only the skill the lamp just offered, in the line that answers the offer, is turned on this way";

/// What the model is told of a turn-on of a skill off in its files.
pub const ONLY_IN_THE_APP: &str = "that skill can be turned on only in the app";

/// The judge's system prompt.
const JUDGE_SYSTEM: &str = "You check whether a skill already on a desk lamp does what a person \
                            asks a new skill to do. Answer one skill's name exactly as listed, \
                            or none.";

/// The one skill offered in place of a make, in which request, and when;
/// and the first heard line after it, the only one whose yes is taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub name: String,
    /// Turned off by voice or in the app: the manage helper's `on` turns it
    /// back on. Never one off in its files, nor one already on.
    pub can_turn_on: bool,
    pub request: u64,
    pub at: Instant,
    pub answer_line: Option<u64>,
}

impl Offered {
    /// The person's line of `request` heard (`name_only`: only the lamp's
    /// name, or no words): the first after the offer that is more than the
    /// name is the one that may answer it.
    pub fn heard(&mut self, request: u64, name_only: bool) {
        if !name_only && request > self.request && self.answer_line.is_none() {
            self.answer_line = Some(request);
        }
    }
}

/// The newest offer on the lamp.
static OFFER: Mutex<Option<Offered>> = Mutex::new(None);

fn slot(offer: &Mutex<Option<Offered>>) -> std::sync::MutexGuard<'_, Option<Offered>> {
    offer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// [`Offered::heard`] for the newest offer, from the second route's heard
/// lines.
pub fn line_heard(request: u64, name_only: bool) {
    if let Some(offered) = slot(&OFFER).as_mut() {
        offered.heard(request, name_only);
    }
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

/// What the voice model is told of the one `skill` offered in place of a
/// make.
pub fn offer_told(skill: &Skill) -> Value {
    let name = &skill.name;
    let spoken = crate::made_skill::spoken_name(name);
    let does = does(skill);
    let (state, ask) = match skill.state {
        State::TurnedOff => (
            "off",
            format!(
                "Ask whether to turn it on instead. A yes: make_skill with {CORE_ON} \"{name}\" \
                 alone. If they still want a new one: make_skill with the same request and {NEW} \
                 true."
            ),
        ),
        State::InstalledOff => (
            "off, and it can be turned on only in the app",
            format!(
                "Say it is off and can be turned on in the app, and ask whether they still want a \
                 new one. If they do: make_skill with the same request and {NEW} true."
            ),
        ),
        _ => (
            "on",
            format!(
                "Say it is on, and ask whether they still want a new one. If they do: make_skill \
                 with the same request and {NEW} true."
            ),
        ),
    };
    json!({
        "not_made": format!("nothing was made: {spoken}, on the lamp, already does this"),
        "offer": name,
        "does": does,
        "state": state,
        "next": format!(
            "Make nothing. Say in a few words, in the conversation's language, that {spoken} \
             already does that, and what it does, only as this says: {does} Never say its timing \
             or settings can be changed. {ask} Otherwise call nothing."
        ),
    })
}

/// A `make_skill` call's `args` before a make, with the switch on, in the
/// voice's request `request`: the offer of the one skill `judge` names
/// among the [`candidates`] `skills` reads (kept in `offer` as the newest);
/// none to make as before: [`NEW`] given (the offer waiting let go), the
/// skills not read, no candidate (the judge not asked), or the judge's
/// "none", another answer, a timeout or an error.
pub fn before_make(
    args: &Value,
    request: u64,
    offer: &Mutex<Option<Offered>>,
    skills: impl FnOnce() -> Result<Vec<Skill>, String>,
    judge: impl FnOnce(&str) -> Option<String>,
    now: Instant,
) -> Option<Value> {
    if !on() {
        return None;
    }
    if args[NEW] == true {
        *slot(offer) = None;
        return None;
    }
    let asked = args["request"].as_str().unwrap_or_default();
    let skills = match skills() {
        Ok(skills) => skills,
        Err(why) => {
            eprintln!(
                "lamp-voice: request {request}: the lamp's skills couldn't be read for a make's offer, made as asked: {why}"
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
            "lamp-voice: request {request}: a make's offer judge didn't answer in time, made as asked (#7747)"
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
    eprintln!(
        "lamp-voice: request {request}: a skill on the lamp does it: offered, nothing made (#7747)"
    );
    *slot(offer) = Some(Offered {
        name: skill.name.clone(),
        can_turn_on: skill.state == State::TurnedOff,
        request,
        at: now,
        answer_line: None,
    });
    Some(offer_told(skill))
}

/// [`before_make`] on the lamp: the product's places with the voice's
/// skills folder `skills`, the make's own chat as the judge.
pub fn before_make_here(args: &Value, request: u64, skills: Option<&Path>) -> Option<Value> {
    let present = std::env::var(skill_about::PRESENT_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    before_make(
        args,
        request,
        &OFFER,
        || {
            skill_about::skills(
                &skill_about::Places::product(skills),
                lamp_ipc::monotonic_us(),
                present,
            )
        },
        judge_here,
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

/// `text`'s words, lowercase, letters and digits, one space between.
fn words(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `asked` names `name`: the same words ("Sip of water",
/// "sip-of-water"), or its words within it ("a sip of water").
fn names(asked: &str, name: &str) -> bool {
    let (asked, name) = (words(asked), words(name));
    !name.is_empty() && format!(" {asked} ").contains(&format!(" {name} "))
}

/// A turn-on of `name` asked with [`CORE_ON`] in the voice's request
/// `request` (`heard`: someone spoke it), of the newest offer `offered`:
/// only heard, only the owner's (`owner_hears`), only in the first heard
/// line after the offer and within [`OFFER_WITHIN`] of it as of `now`,
/// only the one skill offered, and only one turned off by voice or in the
/// app; turned on by `turn` (done, or why not). What the model is told,
/// and whether the offer is spent.
pub fn turned_on(
    name: &str,
    request: u64,
    heard: bool,
    offered: Option<&Offered>,
    now: Instant,
    owner_hears: impl FnOnce() -> bool,
    turn: impl FnOnce(&str) -> Result<(), String>,
) -> (Value, bool) {
    let refused = |why: &str, said: &str| {
        eprintln!("lamp-voice: request {request}: a skill wasn't turned on: {why} (#7747)");
        (
            json!({ "error": said, "next": format!(
            "Say plainly, in a few words, that you can't turn that on this way: {said}. Call nothing."
        ) }),
            false,
        )
    };
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't turned on: no one asked aloud");
        return (json!({ "error": NOT_ASKED_ALOUD }), false);
    }
    if !owner_hears() {
        return refused("not the owner's line", NOT_OWNER);
    }
    let Some(offered) = offered else {
        return refused("nothing offered", NOT_OFFERED);
    };
    if offered.answer_line != Some(request)
        || now.saturating_duration_since(offered.at) > OFFER_WITHIN
    {
        return refused("not the line that answers the offer", NOT_OFFERED);
    }
    if !names(name, &offered.name) {
        return refused("not the skill offered", NOT_OFFERED);
    }
    if !offered.can_turn_on {
        return refused("not one turned off by voice or in the app", ONLY_IN_THE_APP);
    }
    let skill = offered.name.as_str();
    let spoken = crate::made_skill::spoken_name(skill);
    if let Err(why) = turn(skill) {
        eprintln!("lamp-voice: request {request}: an offered skill wasn't turned on: {why}");
        return (
            json!({
                "error": why,
                "next": format!(
                    "Say plainly, in a few words, in the conversation's language, that {spoken} \
                     wasn't turned on, and why: {why}. Call nothing."
                )
            }),
            false,
        );
    }
    eprintln!(
        "lamp-voice: request {request}: an offered skill turned on by voice, in place of a make (#7747)"
    );
    (
        json!({
            CORE_ON: skill,
            "on": true,
            "next": format!(
                "Say only, in a few words, in the conversation's language, that {spoken} is on \
                 now, and that no new skill was made. Call nothing."
            )
        }),
        true,
    )
}

/// [`turned_on`] on the lamp, by `make_skill`'s call: the newest offer,
/// the owner's hearing as a turn's (`services::owner_hears`), the manage
/// helper's socket.
pub fn turned_on_here(
    name: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let offered = slot(&OFFER).clone();
    let manage = Path::new(crate::made_skill::MANAGE_SOCKET);
    let (told, spent) = turned_on(
        name,
        request,
        heard,
        offered.as_ref(),
        Instant::now(),
        || crate::services::owner_hears(request, heard, lease),
        |skill| crate::made_turn::turned_by_helper(manage, skill, false),
    );
    if spent {
        *slot(&OFFER) = None;
    }
    told
}

/// `make_skill`'s parameters with the switch on: [`NEW`] and [`CORE_ON`]
/// added; off, untouched.
pub fn with_parameters(parameters: &mut Value) {
    if !on() {
        return;
    }
    parameters["properties"][NEW] = json!({ "type": "boolean", "description": NEW_TOLD });
    parameters["properties"][CORE_ON] = json!({ "type": "string", "description": CORE_ON_TOLD });
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

    /// lamp-9adf on 18 Nov, more skills than any one offer: the core water
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

    /// The offer of `name` for `request` made in request 4 on `lamp`, its
    /// answer line 5.
    fn offered_on(lamp: &[Skill], request: &str, name: &str) -> Offered {
        set_on_here(Some(true));
        let slot = Mutex::new(None);
        let told = before_make(
            &ask(request),
            4,
            &slot,
            || Ok(lamp.to_vec()),
            |_| Some(name.to_owned()),
            Instant::now(),
        );
        set_on_here(None);
        assert_eq!(told.unwrap()["offer"], name);
        let mut offered = slot.lock().unwrap().clone().unwrap();
        offered.heard(5, false);
        offered
    }

    /// #7747: a make whose ask a skill on the lamp already does is not
    /// made: the judge is shown the turned-off skills and the nearest
    /// others (never the person's own), and the one it names is offered,
    /// with what it does as its description says and never a timing change.
    #[test]
    fn a_make_with_a_core_skill_on_the_lamp_is_offered_not_made() {
        set_on_here(Some(true));
        let slot = Mutex::new(None);
        let mut asked = String::new();
        let told = before_make(
            &ask(SAT),
            1,
            &slot,
            || Ok(nines_lamp(State::On)),
            |question| {
                asked = question.to_owned();
                Some("sip-of-water".into())
            },
            Instant::now(),
        )
        .expect("offered, not made");
        set_on_here(None);
        assert!(asked.contains(&format!("- sip-of-water: {SIP}")), "{asked}");
        assert!(asked.contains("- welcome-back: "), "turned off: {asked}");
        assert!(!asked.contains("coffee"), "{asked}");
        assert!(!asked.contains("dance"), "not near: {asked}");
        assert!(asked.contains("A different timing or trigger is no match"));
        assert_eq!(told["offer"], "sip-of-water");
        assert_eq!(told["state"], "on");
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains(&format!("only as this says: {SIP}")),
            "{next}"
        );
        assert!(next.contains("Never say its timing or settings can be changed"));
        assert!(next.contains("still want a new one"), "{next}");
        assert!(
            !next.contains(CORE_ON),
            "an on skill is never turned on: {next}"
        );
        let offered = slot.lock().unwrap().clone().unwrap();
        assert_eq!(offered.name, "sip-of-water");
        assert!(!offered.can_turn_on);
        // Only the person's own skills: no judge, made at once; none read.
        set_on_here(Some(true));
        let theirs = || Ok(vec![skill("coffee-time", "Coffee.", State::On, true)]);
        let only_theirs = before_make(
            &ask("glow blue"),
            2,
            &slot,
            theirs,
            never_judged,
            Instant::now(),
        );
        let unreadable = before_make(
            &ask("glow blue"),
            3,
            &slot,
            || Err("no folder".into()),
            never_judged,
            Instant::now(),
        );
        set_on_here(None);
        assert_eq!((only_theirs, unreadable), (None, None));
    }

    /// Twelve's and El's P2s on #7763: a different timing ("every 20
    /// minutes" against a nudge after a long sitting), a judge that answers
    /// none or a skill not listed, or one that doesn't answer in time, goes
    /// on to make; nothing is offered.
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
                Instant::now(),
            );
            set_on_here(None);
            assert_eq!(told, None, "{answer:?}");
            assert_eq!(*slot.lock().unwrap(), None, "{answer:?}");
        }
    }

    /// #7747: the owner's yes in the line that answers the offer turns the
    /// offered skill on through the manage helper, however its name is said.
    #[test]
    fn a_yes_turns_the_offered_core_skill_on() {
        let offered = offered_on(&nines_lamp(State::TurnedOff), SAT, "sip-of-water");
        assert!(offered.can_turn_on);
        for said in ["sip-of-water", "Sip of water", "a sip of water"] {
            let mut turned = None;
            let (told, spent) = turned_on(
                said,
                5,
                true,
                Some(&offered),
                offered.at,
                || true,
                |n| {
                    turned = Some(n.to_owned());
                    Ok(())
                },
            );
            assert!(spent, "{said}: {told}");
            assert_eq!(turned.as_deref(), Some("sip-of-water"), "{said}");
            assert_eq!(told[CORE_ON], "sip-of-water");
            assert!(
                told["next"]
                    .as_str()
                    .unwrap()
                    .contains("sip of water is on now, and that no new skill was made")
            );
        }
    }

    /// The P1 on #7763 (Twelve, El): core_on turns on only the one skill
    /// offered, never another listed one, never one off in its files, and
    /// only in the first heard line after the offer, within a minute; not
    /// heard or not the owner's, nothing.
    #[test]
    fn core_on_turns_on_only_the_one_offered_in_the_line_that_answers_it() {
        let never = |_: &str| -> Result<(), String> { panic!("turned on") };
        let lamp = nines_lamp(State::TurnedOff);
        let offered = offered_on(&lamp, SAT, "sip-of-water");
        let at = offered.at;
        let refused = |name: &str, request: u64, offered: &Offered, now: Instant| {
            turned_on(name, request, true, Some(offered), now, || true, never).0["error"].clone()
        };
        // Listed beside it, or on the lamp and never listed.
        assert_eq!(refused("welcome back", 5, &offered, at), NOT_OFFERED);
        assert_eq!(refused("guard mode", 5, &offered, at), NOT_OFFERED);
        assert_eq!(refused("stretch break", 5, &offered, at), NOT_OFFERED);
        // A yes to a later question, or past the minute.
        assert_eq!(refused("sip of water", 6, &offered, at), NOT_OFFERED);
        let late = at + OFFER_WITHIN + Duration::from_secs(1);
        assert_eq!(refused("sip of water", 5, &offered, late), NOT_OFFERED);
        // No line heard after the offer yet, or only the lamp's name.
        let mut unanswered = offered.clone();
        unanswered.answer_line = None;
        unanswered.heard(5, true);
        assert_eq!(refused("sip of water", 5, &unanswered, at), NOT_OFFERED);
        // Off in its files: offered as such, never turned on by voice.
        let guard = offered_on(&lamp, "watch my desk when I'm away", "guard-mode");
        assert!(!guard.can_turn_on);
        assert_eq!(refused("guard mode", 5, &guard, guard.at), ONLY_IN_THE_APP);
        // Not heard, not the owner's, nothing offered.
        let (told, _) = turned_on("sip of water", 5, false, Some(&offered), at, || true, never);
        assert_eq!(told["error"], NOT_ASKED_ALOUD);
        let (told, _) = turned_on("sip of water", 5, true, Some(&offered), at, || false, never);
        assert_eq!(told["error"], NOT_OWNER);
        let (told, _) = turned_on("sip of water", 5, true, None, at, || true, never);
        assert_eq!(told["error"], NOT_OFFERED);
    }

    /// #7747: a no that still wants a new one is the same request with
    /// new_skill: made as asked, with no judge, and the offer let go.
    #[test]
    fn a_new_skill_still_wanted_is_made_as_asked_and_the_offer_let_go() {
        let offered = offered_on(&nines_lamp(State::On), SAT, "sip-of-water");
        let slot = Mutex::new(Some(offered));
        set_on_here(Some(true));
        let again = before_make(
            &json!({ "request": "remind me to drink water", NEW: true }),
            7,
            &slot,
            || panic!("the skills read again"),
            never_judged,
            Instant::now(),
        );
        set_on_here(None);
        assert_eq!(again, None, "made as asked");
        assert_eq!(*slot.lock().unwrap(), None, "the offer let go");
    }

    /// El's P3 on #7763: the judge is shown at most LISTED_MOST skills.
    #[test]
    fn the_judge_is_shown_at_most_forty_skills() {
        let lamp: Vec<Skill> = (0..50)
            .map(|n| skill(&format!("skill-{n:02}"), "Glows.", State::TurnedOff, false))
            .collect();
        assert_eq!(candidates("glow", &lamp).len(), LISTED_MOST);
    }

    /// #7747: off, nothing changes: no read, no judge, no offer, and
    /// make_skill's parameters are byte for byte as before.
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
        assert_eq!(parameters["properties"][NEW]["type"], "boolean");
        assert_eq!(parameters["properties"][CORE_ON]["type"], "string");
    }
}
