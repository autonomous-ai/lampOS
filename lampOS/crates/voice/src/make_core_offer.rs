//! A make that asks for what a skill on the lamp already does, offered
//! that skill instead of a second one (#7747, `LAMP_MAKE_CORE_OFFER`, off
//! unless `on`). On lamp-9adf three water reminders made by voice sat
//! beside the core `sip-of-water`, none with its guards: the person asked
//! for a better water reminder and got a second, worse one.
//!
//! - **The model judges, before anything is made.** A `make_skill` call
//!   with a `request` and no [`NEW`] makes nothing yet: it is told, in one
//!   line each, every skill on the lamp the person didn't make (a core,
//!   Store or pushed one: its name, what its `SKILL.md` says it does, on or
//!   off, as `crate::skill_about` reads them) and asked whether one already
//!   does what was asked ([`offer`]). Whether it does is the model's
//!   judgment, never a list of words (the owner's ruling, 13 Nov). A lamp
//!   with none, or whose skills can't be read, makes at once, as before.
//! - **None does:** the model calls again at once with the same request
//!   and [`NEW`], and the make goes on as before.
//! - **One does:** nothing is made. The lamp says so and offers it
//!   instead, never a silent swap: one that is off, to turn it on; one that
//!   is on, what it does, and whether they still want a new one. Its
//!   timing can't be changed by voice yet (no core skill exposes a setting
//!   the voice may write), so that is never offered or claimed.
//! - **A yes** to turning it on comes back as [`CORE_ON`], the name alone:
//!   only heard, only the owner's line, and only a skill offered within
//!   [`OFFER_WITHIN`] ([`turned_on`]); turned on by the manage helper's own
//!   `{"on": NAME}`, as the app's switch and `crate::made_turn` ask it.
//! - **A no**, when they still want a new one, is the same request with
//!   [`NEW`]: made as asked. The offer is then let go.
//!
//! No socket and no model call of its own: two local folders and three
//! records are read (`skill_about::skills`), then one more round of the
//! voice's own model, only for a make. Off, `make_skill`'s parameters and
//! answers are byte for byte as before.
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
/// asked: no skill on the lamp does it, or the person still wants a new one.
pub const NEW: &str = "new_skill";

/// The `make_skill` argument that turns on a skill on the lamp the lamp
/// just offered in place of a make: its name, given alone.
pub const CORE_ON: &str = "core_on";

/// How long an offer waits for the person's answer.
pub const OFFER_WITHIN: Duration = Duration::from_secs(300);

/// The most bytes of a skill's description told in its line.
pub const DOES_MOST: usize = 200;

/// What [`NEW`]'s parameter says.
pub const NEW_TOLD: &str = "With request only: true once the lamp's skills were told for this ask and none already does it, or the person heard which one does and still wants a new one.";

/// What [`CORE_ON`]'s parameter says.
pub const CORE_ON_TOLD: &str = "Only when the lamp just offered a skill on the lamp that already does what the person asked, in place of a new one, and they agree to turn it on: its name as told, given alone with no request.";

/// What the model is told of a turn-on asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str =
    "a skill is turned on by voice only when the person asks out loud";

/// What the model is told of a turn-on asked by anyone but the owner.
pub const NOT_OWNER: &str = "only the lamp's owner can turn a skill on by voice";

/// What the model is told of a turn-on of a skill not just offered.
pub const NOT_OFFERED: &str =
    "only a skill the lamp just offered in place of a new one is turned on this way";

/// The skills offered in place of a make, by name, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub names: Vec<String>,
    pub at: Instant,
}

/// The newest offer on the lamp.
static OFFER: Mutex<Option<Offered>> = Mutex::new(None);

fn offer_now() -> std::sync::MutexGuard<'static, Option<Offered>> {
    OFFER
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

/// A skill's state as the offer tells it; none for one that doesn't load.
fn state_told(state: &State) -> Option<&'static str> {
    match state {
        State::TurnedOff => Some("off: may be turned on with core_on"),
        State::InstalledOff => Some("off: it can be turned on only in the app"),
        State::DoesNotLoad => None,
        State::On | State::OnRestUnknown | State::Snoozed | State::Resting(_) => Some("on"),
    }
}

/// The offer for a make, of `skills` on the lamp: what the model is told
/// in place of a make, and what is offered; none when no skill the person
/// didn't make is on the lamp (made at once, as before).
pub fn offer(skills: &[Skill], now: Instant) -> Option<(Value, Offered)> {
    let lines: Vec<(String, String)> = skills
        .iter()
        .filter(|skill| !skill.made_here)
        .filter_map(|skill| {
            let state = state_told(&skill.state)?;
            let does = skill.description.as_deref().unwrap_or("no description");
            Some((
                skill.name.clone(),
                format!("{}: {} ({state})", skill.name, cut(does, DOES_MOST).trim()),
            ))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let told = json!({
        "not_made_yet": "nothing was made: first judge whether a skill already on the lamp does what was asked",
        "on_the_lamp": lines.iter().map(|(_, line)| line.as_str()).collect::<Vec<_>>(),
        "next": format!(
            "Judge by meaning whether one of these skills already does what the person asked: \
             the same kind of nudge or action, on the same kind of moment. If none does, call \
             make_skill again now with the same request and {NEW} true, saying nothing first. \
             If one does, make nothing and never a second one: say in a few words, in the \
             conversation's language, that it already does that, by its name with spaces for \
             hyphens. If it is off and may be turned on, ask whether to turn it on instead; \
             otherwise say what it does and ask whether they still want a new one. Never offer \
             or claim to change its timing or settings: that can't be done by voice yet. Then \
             wait. A yes to turning it on: make_skill with {CORE_ON}, its name, alone. If they \
             still want a new one: make_skill with the same request and {NEW} true."
        ),
    });
    let offered = Offered {
        names: lines.into_iter().map(|(name, _)| name).collect(),
        at: now,
    };
    Some((told, offered))
}

/// A `make_skill` call's `args` before a make, with the switch on: the
/// offer when it carries a `request` and no [`NEW`] and `skills` reads
/// any skill the person didn't make (kept as the newest offer); none to
/// make as before. With [`NEW`], the offer waiting is let go.
pub fn before_make(
    args: &Value,
    request: u64,
    skills: impl FnOnce() -> Result<Vec<Skill>, String>,
    now: Instant,
) -> Option<Value> {
    if !on() {
        return None;
    }
    if args[NEW] == true {
        *offer_now() = None;
        return None;
    }
    let skills = match skills() {
        Ok(skills) => skills,
        Err(why) => {
            eprintln!(
                "lamp-voice: request {request}: the lamp's skills couldn't be read for the make's offer, made as asked: {why}"
            );
            return None;
        }
    };
    let (told, offered) = offer(&skills, now)?;
    eprintln!(
        "lamp-voice: request {request}: a make waits for the model's judgment of {} skills on the lamp (#7747)",
        offered.names.len()
    );
    *offer_now() = Some(offered);
    Some(told)
}

/// [`before_make`] on the lamp: the product's places with the voice's
/// skills folder `skills`.
pub fn before_make_here(args: &Value, request: u64, skills: Option<&Path>) -> Option<Value> {
    let present = std::env::var(skill_about::PRESENT_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    before_make(
        args,
        request,
        || {
            skill_about::skills(
                &skill_about::Places::product(skills),
                lamp_ipc::monotonic_us(),
                present,
            )
        },
        Instant::now(),
    )
}

/// `text`'s words, lowercase, letters and digits, one space between.
fn words(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which of `offered`'s names `asked` names: the same words ("Sip of
/// water", "sip-of-water"), or a name's words alone within it ("a sip of
/// water"); none when no one, or more than one, is.
fn named<'a>(asked: &str, offered: &'a Offered) -> Option<&'a str> {
    let asked = words(asked);
    if let Some(name) = offered.names.iter().find(|name| words(name) == asked) {
        return Some(name);
    }
    let padded = format!(" {asked} ");
    let within: Vec<&String> = offered
        .names
        .iter()
        .filter(|name| padded.contains(&format!(" {} ", words(name))))
        .collect();
    match within.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

/// A turn-on of `name` asked with [`CORE_ON`] in the voice's request
/// `request` (`heard`: someone spoke it), of the newest offer `offered`:
/// only heard, only the owner's (`owner_hears`), only a skill offered
/// within [`OFFER_WITHIN`] of `now`; turned on by `turn` (done, or why
/// not). What the model is told, and whether the offer is spent.
pub fn turned_on(
    name: &str,
    request: u64,
    heard: bool,
    offered: Option<&Offered>,
    now: Instant,
    owner_hears: impl FnOnce() -> bool,
    turn: impl FnOnce(&str) -> Result<(), String>,
) -> (Value, bool) {
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't turned on: no one asked aloud");
        return (json!({ "error": NOT_ASKED_ALOUD }), false);
    }
    if !owner_hears() {
        eprintln!("lamp-voice: request {request}: a skill wasn't turned on: not the owner's line");
        return (
            json!({
                "error": NOT_OWNER,
                "next": "Say plainly, in a few words, that only the lamp's owner can turn a skill \
                         on by voice. Name no skill. Call nothing."
            }),
            false,
        );
    }
    let Some(skill) = offered
        .filter(|offered| now.saturating_duration_since(offered.at) <= OFFER_WITHIN)
        .and_then(|offered| named(name, offered))
    else {
        eprintln!(
            "lamp-voice: request {request}: a skill wasn't turned on: not one just offered (#7747)"
        );
        return (
            json!({
                "error": NOT_OFFERED,
                "next": "Say plainly, in a few words, that you can't turn that one on this way; \
                         it can be turned on in the app. Call nothing."
            }),
            false,
        );
    };
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
    let offered = offer_now().clone();
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
        *offer_now() = None;
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

    /// lamp-9adf on 18 Nov: the core water reminder turned off, Stretch
    /// break on, and a skill the person made.
    fn nines_lamp(sip: State) -> Vec<Skill> {
        vec![
            skill("sip-of-water", SIP, sip, false),
            skill(
                "stretch-break",
                "A short stretch after a long sitting.",
                State::On,
                false,
            ),
            skill("coffee-time", "Says coffee time at 3 pm.", State::On, true),
        ]
    }

    fn ask(request: &str) -> Value {
        json!({ "request": request })
    }

    /// #7747: "remind me to drink water every hour" is not made: the model
    /// is told every skill on the lamp the person didn't make, with what it
    /// does and whether it is on, and asked to judge and offer it; the
    /// person's own made skills are not among them.
    #[test]
    fn a_make_with_a_core_skill_on_the_lamp_is_offered_not_made() {
        set_on_here(Some(true));
        let now = Instant::now();
        let told = before_make(
            &ask("remind me to drink water every hour"),
            1,
            || Ok(nines_lamp(State::TurnedOff)),
            now,
        )
        .expect("offered, not made");
        set_on_here(None);
        let lines = told["on_the_lamp"].as_array().unwrap();
        assert_eq!(lines.len(), 2, "{told}");
        assert_eq!(
            lines[0],
            format!("sip-of-water: {SIP} (off: may be turned on with core_on)")
        );
        assert_eq!(
            lines[1],
            "stretch-break: A short stretch after a long sitting. (on)"
        );
        assert!(!told.to_string().contains("coffee"), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("Judge by meaning"), "{next}");
        assert!(next.contains("never a second one"), "{next}");
        assert!(
            next.contains("Never offer or claim to change its timing"),
            "{next}"
        );
        assert!(told["not_made_yet"].is_string(), "{told}");
        // A lamp with only skills the person made, or none to read: made
        // at once, as before.
        set_on_here(Some(true));
        let only_theirs = before_make(
            &ask("glow blue at 9"),
            2,
            || Ok(vec![skill("coffee-time", "Coffee.", State::On, true)]),
            now,
        );
        let unreadable = before_make(&ask("glow blue at 9"), 3, || Err("no folder".into()), now);
        set_on_here(None);
        assert_eq!(only_theirs, None);
        assert_eq!(unreadable, None);
    }

    /// #7747: the person's yes turns the offered core skill on through the
    /// manage helper, by its folder name however it was said, and the
    /// model says it is on and nothing new was made.
    #[test]
    fn a_yes_turns_the_offered_core_skill_on() {
        let now = Instant::now();
        let (_, offered) = offer(&nines_lamp(State::TurnedOff), now).unwrap();
        for said in ["sip-of-water", "Sip of water", "a sip of water"] {
            let mut turned = None;
            let (told, spent) = turned_on(
                said,
                4,
                true,
                Some(&offered),
                now,
                || true,
                |name| {
                    turned = Some(name.to_owned());
                    Ok(())
                },
            );
            assert!(spent, "{said}: {told}");
            assert_eq!(turned.as_deref(), Some("sip-of-water"), "{said}");
            assert_eq!(told[CORE_ON], "sip-of-water");
            assert_eq!(told["on"], true);
            assert!(
                told["next"]
                    .as_str()
                    .unwrap()
                    .contains("sip of water is on now, and that no new skill was made"),
                "{told}"
            );
        }
        let never = |_: &str| -> Result<(), String> { panic!("turned on") };
        // Not heard, not the owner's, not offered, or offered too long ago:
        // nothing is turned on.
        let refused = |name: &str, heard: bool, owner: bool, at: Instant| {
            turned_on(name, 5, heard, Some(&offered), at, || owner, never)
        };
        assert_eq!(
            refused("sip of water", false, true, now).0["error"],
            NOT_ASKED_ALOUD
        );
        assert_eq!(
            refused("sip of water", true, false, now).0["error"],
            NOT_OWNER
        );
        assert_eq!(
            refused("guard mode", true, true, now).0["error"],
            NOT_OFFERED
        );
        let late = now + OFFER_WITHIN + Duration::from_secs(1);
        assert_eq!(
            refused("sip of water", true, true, late).0["error"],
            NOT_OFFERED
        );
        let (none, _) = turned_on("sip of water", 6, true, None, now, || true, never);
        assert_eq!(none["error"], NOT_OFFERED);
        // The helper's refusal is said as it is, and the offer stands.
        let (failed, spent) = turned_on(
            "sip of water",
            7,
            true,
            Some(&offered),
            now,
            || true,
            |_| Err("the lamp's skills couldn't be reached".into()),
        );
        assert!(!spent);
        assert_eq!(failed["error"], "the lamp's skills couldn't be reached");
    }

    /// #7747: when the person still wants a new one (a no to the offer, or
    /// "remind me to call mum at 3", which nothing on the lamp does), the
    /// same request with new_skill is made as asked, and the offer waiting
    /// is let go, so a later core_on turns nothing on.
    #[test]
    fn a_new_skill_still_wanted_is_made_as_asked_and_the_offer_let_go() {
        set_on_here(Some(true));
        let now = Instant::now();
        let first = before_make(
            &ask("remind me to call mum at 3"),
            8,
            || Ok(nines_lamp(State::On)),
            now,
        );
        assert!(first.is_some());
        assert!(offer_now().is_some());
        let again = before_make(
            &json!({ "request": "remind me to call mum at 3", NEW: true }),
            8,
            || panic!("the skills read again"),
            now,
        );
        set_on_here(None);
        assert_eq!(again, None, "made as asked");
        assert_eq!(*offer_now(), None, "the offer let go");
    }

    /// #7747: off, nothing changes: no offer, and make_skill's parameters
    /// are byte for byte as before; on, only new_skill and core_on added.
    #[test]
    fn the_switch_off_changes_nothing() {
        set_on_here(Some(false));
        let off = before_make(
            &ask("remind me to drink water"),
            9,
            || panic!("the skills read with the switch off"),
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
        assert_eq!(parameters["properties"].as_object().unwrap().len(), 3);
    }
}
