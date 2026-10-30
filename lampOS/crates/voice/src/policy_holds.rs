//! The Policy holds the lamp's actions (#800), off unless
//! `LAMP_POLICY_HOLDS_ACTIONS=on`: a call that changes the lamp (its volume
//! or its light, the executive's functions) or sets or takes away something
//! on its schedule ([`SCHEDULE_HELD`]: `set_timer`, `set_reminder`,
//! `cancel_reminder`) runs only when the Policy's attention decision for the
//! turn is not `stay_quiet`, or the lamp's name was heard. Every other call
//! runs as it did, `list_reminders`, `repeat_reminder` and `snooze_reminder`
//! among them ([`holds`]).
//!
//! On lamp-4ace on 2026-10-15 at 14:49 the lamp heard "Thailand" in a busy
//! office, ran `volume_up` and stayed 7 dB louder for eleven hours; the
//! Policy's decision in the same second was `stay_quiet` (p = 0.235). The
//! voice's own rule (`crate::attention`, #829) holds a whole answer it
//! judges not for the lamp; this holds only the action, on the Policy's
//! judgement, and speech stays as it is.
//!
//! - **The turn's decision** is an attention decision the Policy publishes
//!   for real (`policy/decisions`, `about` `attention`, `shadow` false:
//!   `lamp-policy --act attention`) about the turn by its number
//!   (`Decision.request`). The voice says on `voice/heard` the turn in which
//!   it heard each piece (`crate::bus_out`): the turn whose input began last
//!   before the piece came, as the voice's own rule takes it
//!   (`crate::attention`). The Policy's decision carries the number of the
//!   pieces it judged: a piece of another turn ends the utterance open
//!   (`lamp_policy::gather`, the reviewer on #908, 06:40), so one decision
//!   is never about two turns' words. So a decision is its own turn's
//!   however late it comes, and never another's (the reviewer on #908: a
//!   late decision about the turn before must never release or hold this
//!   turn's action). A decision with no number is nobody's. Where the
//!   number can still be wrong: a piece of a turn's words that the service
//!   writes only after the next turn's input began is that next turn's,
//!   here as in the voice's own rule.
//! - **Waited for** at most [`WAIT`] from the call; past it the action runs
//!   as it did without the switch. While it waits nothing else waits: the
//!   call is kept aside and looked at again on each tick of the worker.
//! - **Held:** the action is not run, the model is told so ([`HELD`]) and
//!   none of the turn's sound is let out. Asked for again in the same turn
//!   after it was told, it is told it will not be done ([`HELD_AGAIN`]) and
//!   the turn ends (#1465).
//! - **Withdrawn while it waits** (the service cancelled the call), or its
//!   turn retired or its session gone: it is let go, neither run nor
//!   answered, and that is looked at again right before it would run (the
//!   reviewer on #908).
//! - Without the bus, or on a turn the lamp wrote itself, nothing is held.
//! - **On the second route** (`crate::second_provider`, #1712) the same
//!   gate holds the same calls of a spoken turn, with the same journal
//!   lines, by the decision about the same turn: its request numbers are the
//!   ones `voice/heard` tells. The call waits in the turn's own task, so
//!   nothing else waits. Held, the model is told [`HELD`] and the turn ends
//!   there, its words not yet said dropped: with no session to keep, the
//!   model is never asked again, so never told [`HELD_AGAIN`]. What it said
//!   before the call may have been let out already, and the journal counts
//!   it. The turn's facts come from the route's own rule (`LAMP_ATTENTION`):
//!   without the rule, nothing is held there.
//!
//! - **A reply to the lamp is not held from its schedule** (#1040): a
//!   `stay_quiet` whose `follows_lamp` answer is [`FOLLOWS_FROM`] or more
//!   ([`Verdict::StayQuietFollowing`]) still holds the volume, the light
//!   and the executive's functions, but not `set_timer` or `set_reminder`
//!   ([`SETS`]). On lamp-4ace on 2026-10-16 at 11:20 "Set a timer for
//!   1 minute", said to the lamp in a conversation with it, was held, and no
//!   timer was set: the fitted weights count a follow-up to the lamp against
//!   answering, though Jev's `follows_lamp` told the lamp's lines apart
//!   (median 0.60 on 33 said to the lamp, 0.29 on 46 not). A timer nobody
//!   asked for is a smaller harm than one asked for and not set, and it can
//!   be cancelled by asking. Not so `cancel_reminder`, which stays held: a
//!   wrong cancel is silent, nothing puts the reminder back, and the person
//!   finds out only when it does not come (Fourth on #1054). Until the
//!   weights are fitted again (#1040). The 11:20 turn itself scored
//!   `follows_lamp` 0.35, so at 0.6 it is still held: this lets through 17
//!   of the 33 turns said to the lamp, not that one; 0.35 would let through
//!   28 of the 33 and 17 of the 46 not said to it (the reviewer on #1054).
//!   And only in conversation: the lamp's sound went out within
//!   [`IN_CONVERSATION_US`] before the turn, as the Policy counts it, since
//!   all 79 rows were in conversation and a turn follows no line of the
//!   lamp's when it has said none lately (the reviewer on #1054).
//!
//! - **With `LAMP_FOLLOWS_RUN=on`, a reply to the lamp runs its light and
//!   its volume** (#1465), off unless set, and only with the Policy holding
//!   actions: the same reply (a `stay_quiet` with `follows_lamp`
//!   [`FOLLOWS_FROM`] or more, in conversation, not after a line the lamp
//!   left unanswered) runs the light's tools (`light_*`) and
//!   [`REPLY_VOLUME`] as well as what the schedule [`sets`]
//!   ([`runs_on_a_reply`]). Still held on a reply: every skill's function
//!   (`check_my_work` sends a still of the desk to the model, `dance` and
//!   `show_verdict` move the body), `volume_mute` (a lamp muted wrongly
//!   cannot say what it did) and `cancel_reminder` (the first builder on
//!   #1474, after the reviewer's finding). On lamp-4ace on 2026-10-18 at
//!   10:57 "Thanks. Now turn yourself back up a little, please.", 13 s
//!   after the lamp had turned itself down when asked by name, scored
//!   `follows_lamp` 0.89 with p 0.31, `stay_quiet` (the Policy's room
//!   `unknown` after a restart): its `volume_up` was held, and the model
//!   asked again 72 times in 49 s. What it costs: a line the Policy scores
//!   as said back to the lamp that was not (6 of the 46 such rows not said
//!   to it on 2026-10-16) can now change the light or the volume, which the
//!   person sees or hears and can ask back.
//!
//! Nothing of what was heard is read here: the decision's choice and its
//! `follows_lamp` answer only.
use serde_json::{Value, json};
use std::{collections::VecDeque, io, path::Path, time::Duration};

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_POLICY_HOLDS_ACTIONS";
/// The provider worker's flag for it, after `--sounds-checked`.
pub const FLAG: &str = "--policy-holds-actions";
/// Where the coordinator reads whether a reply to the lamp runs its light
/// and its volume too ([`runs_on_a_reply`], #1465). `on` only, and only
/// with [`ENVIRONMENT`].
pub const FOLLOWS_ENVIRONMENT: &str = "LAMP_FOLLOWS_RUN";
/// The provider worker's flag for it, right after [`FLAG`].
pub const FOLLOWS_FLAG: &str = "--follows-run";
/// The longest an action waits for the turn's decision.
pub const WAIT: Duration = Duration::from_secs(2);
/// A hearing of the name at this score or more lets the action run, as the
/// Policy counts the name heard (`lamp_policy::attention::NAME_HEARD_FROM`).
pub const NAME_FROM: f32 = 0.9;
/// A `follows_lamp` answer this high or more makes a `stay_quiet` a reply
/// to the lamp, for its schedule ([`Verdict::StayQuietFollowing`]). On the
/// first builder's 79 hand-read `stay_quiet` turns in conversation on
/// 2026-10-16: 17 of the 33 said to the lamp and 6 of the 46 not are this
/// high (0.5: 19 and 14; 0.7: 14 and 1).
pub const FOLLOWS_FROM: f32 = 0.6;
/// A `stay_quiet` is a reply to the lamp only if the lamp's sound went out
/// within this before the turn: the Policy's own "in conversation"
/// (`lamp_policy::attention::IN_CONVERSATION_SECONDS`, 20 s). All of the
/// first builder's 79 rows were in conversation; out of it a high
/// `follows_lamp` is not measured, and follows no line of the lamp's (the
/// reviewer on #1054).
pub const IN_CONVERSATION_US: u64 = 20_000_000;
/// What the model is told of an action held.
pub const HELD: &str = "Not done: it was not clear that was said to you. Do not call it again for this request. Say nothing at all.";
/// What the model is told when it asks again in the same turn for an
/// action held: the last answer, and the turn is ended (#1465: on lamp-4ace
/// on 18 Oct the model asked 72 times in 49 s, each held, and the lamp said
/// nothing for most of a minute).
pub const HELD_AGAIN: &str = "Not done, and it will not be done for this request.";
/// Turns whose decisions are kept: some minutes' worth.
const KEPT: usize = 16;

/// The schedule's functions that are held: those that set something to
/// come or take it away. On lamp-4ace on 16 October "Yes, give me five
/// minutes", said to someone else, set a five-minute timer (the first
/// builder on #908). Listing what is set, and changing one already set
/// (`repeat_reminder`, `snooze_reminder`), are not held (the first builder,
/// 05:57).
pub const SCHEDULE_HELD: [&str; 3] = ["set_timer", "set_reminder", "cancel_reminder"];

/// Of [`SCHEDULE_HELD`], the two that a reply to the lamp still runs
/// ([`Verdict::StayQuietFollowing`]): they set something to come, which can
/// be cancelled by asking. `cancel_reminder` is not one (Fourth on #1054).
pub const SETS: [&str; 2] = ["set_timer", "set_reminder"];

/// Whether a call, by its route and its name, waits for the Policy's
/// decision: every one of the executive's, and the schedule's in
/// [`SCHEDULE_HELD`].
pub fn holds(route: &crate::tools::Route, name: &str) -> bool {
    match route {
        crate::tools::Route::Executive(_) | crate::tools::Route::UseSkill(_) => true,
        crate::tools::Route::Schedule(_) => SCHEDULE_HELD.contains(&name),
        _ => false,
    }
}

/// Whether a call, by its route and its name, is one the schedule sets
/// ([`SETS`]), which a reply to the lamp still runs.
pub fn sets(route: &crate::tools::Route, name: &str) -> bool {
    matches!(route, crate::tools::Route::Schedule(_)) && SETS.contains(&name)
}

/// Of the volume's functions, those a reply to the lamp runs with
/// [`FOLLOWS_FLAG`]: not `volume_mute`, since a lamp muted wrongly cannot
/// say what it did (the first builder on #1474).
pub const REPLY_VOLUME: [&str; 3] = ["volume_up", "volume_down", "volume_unmute"];

/// Whether a held call, by its route and its name, runs on a reply to the
/// lamp ([`Verdict::StayQuietFollowing`]): what the schedule [`sets`]; and
/// with [`FOLLOWS_FLAG`] (`light_and_volume`), the light's tools and
/// [`REPLY_VOLUME`] on the actions' own route too, never a skill's (#1465),
/// by the voice's `settings` ([`crate::tools::is_skills`]).
pub fn runs_on_a_reply(
    settings: &crate::tools::Settings,
    route: &crate::tools::Route,
    name: &str,
    light_and_volume: bool,
) -> bool {
    let actions = matches!(route, crate::tools::Route::Executive(_))
        && !crate::tools::is_skills(settings, route);
    sets(route, name)
        || light_and_volume
            && actions
            && (name.starts_with("light_") || REPLY_VOLUME.contains(&name))
}

/// The worker's arguments for the switch in the environment, and for
/// [`FOLLOWS_ENVIRONMENT`] after it, which means nothing without it.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(FOLLOWS_ENVIRONMENT).ok().as_deref(),
    )
}

fn arguments(value: Option<&str>, follows: Option<&str>) -> Vec<String> {
    match (value == Some("on"), follows == Some("on")) {
        (true, true) => vec![FLAG.to_owned(), FOLLOWS_FLAG.to_owned()],
        (true, false) => vec![FLAG.to_owned()],
        (false, _) => Vec::new(),
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading [`FOLLOWS_FLAG`] off a worker's flags.
pub fn take_follows(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FOLLOWS_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// What the model is told of an action held.
pub fn held() -> Value {
    json!({"done": false, "next": HELD})
}

/// What the model is told of an action held again in the same turn.
pub fn held_again() -> Value {
    json!({"done": false, "next": HELD_AGAIN})
}

/// What becomes of an action kept aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Run it, as without the switch.
    Run,
    /// Do not run it: the Policy judged the turn not for the lamp.
    Hold,
    /// Wait for the turn's decision.
    Wait,
}

/// What the Policy's decision says of one turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// `stay_quiet`: not for the lamp.
    StayQuiet,
    /// `stay_quiet`, but said back to the lamp about what it last said, by
    /// the decision's `follows_lamp` ([`FOLLOWS_FROM`]): what it sets on the
    /// schedule ([`SETS`]) is not held (#1040), nor, with [`FOLLOWS_FLAG`],
    /// its light and its volume ([`runs_on_a_reply`], #1465).
    StayQuietFollowing,
    /// `answer` or `look_up`: the lamp's.
    TakenUp,
}

/// The gate for an action, from its turn's verdict if one has come,
/// whether the name was heard in the turn, how long the action has waited,
/// and whether a reply to the lamp runs it ([`runs_on_a_reply`]).
pub fn gate(
    verdict: Option<Verdict>,
    name_heard: bool,
    waited: Duration,
    on_a_reply: bool,
) -> Gate {
    gate_within(verdict, name_heard, waited, on_a_reply, WAIT)
}

/// As [`gate`], waiting at most `cap` for the turn's verdict. The lamp's is
/// [`WAIT`]; a test gives a shorter one so as not to wait the lamp's out
/// (#1806).
pub fn gate_within(
    verdict: Option<Verdict>,
    name_heard: bool,
    waited: Duration,
    on_a_reply: bool,
    cap: Duration,
) -> Gate {
    match verdict {
        _ if name_heard => Gate::Run,
        Some(Verdict::StayQuiet) => Gate::Hold,
        Some(Verdict::StayQuietFollowing) if on_a_reply => Gate::Run,
        Some(Verdict::StayQuietFollowing) => Gate::Hold,
        Some(Verdict::TakenUp) => Gate::Run,
        None if waited < cap => Gate::Wait,
        None => Gate::Run,
    }
}

/// A decision's verdict as it stands for its turn: a reply to the lamp
/// ([`Verdict::StayQuietFollowing`]) is not one after someone else's line
/// the lamp left unanswered (`after_unanswered`, #1054, the quiet-room case
/// on #1090), nor when the lamp has not spoken lately (`lamp_spoke_lately`,
/// [`IN_CONVERSATION_US`]): out of conversation it follows no line of the
/// lamp's (the reviewer on #1054).
pub fn for_turn(verdict: Verdict, after_unanswered: bool, lamp_spoke_lately: bool) -> Verdict {
    match verdict {
        Verdict::StayQuietFollowing if after_unanswered || !lamp_spoke_lately => Verdict::StayQuiet,
        verdict => verdict,
    }
}

/// Why an action [`gate`] let run ran, as the journal says it after
/// "run after N ms: ", by its turn's verdict, whether the name was heard,
/// and whether the call is one the schedule [`sets`].
pub fn why_run(verdict: Option<Verdict>, name_heard: bool, sets: bool) -> &'static str {
    match verdict {
        _ if name_heard => "its name was heard",
        Some(Verdict::StayQuietFollowing) if !sets => {
            "the turn follows the lamp's last line, and its light and its volume are not held from it"
        }
        Some(Verdict::StayQuietFollowing) => {
            "the turn follows the lamp's last line, and what it sets on the schedule is not held from it"
        }
        Some(_) => "the Policy took the turn as the lamp's",
        None => "no decision came in time",
    }
}

/// The Policy's attention decisions, each its own turn's by number.
pub struct Verdicts {
    decisions: lamp_bus::Subscriber,
    /// What the Policy decided of each turn, by the turn's number, and
    /// whether its choice was `answer` (not `look_up`), for a line let out
    /// only as a conversation going on ([`Self::says`]). Of two decisions
    /// about one turn (its words judged in two pieces), the newer.
    decided: VecDeque<(u64, Verdict, bool)>,
    /// The turns the Policy decided only in shadow, by number: none of them
    /// is acted on, but a line going on need not wait for a decision that
    /// is not coming ([`Self::says`], the reviewer on #1538).
    shadowed: VecDeque<u64>,
}

impl Verdicts {
    pub fn on(root: &Path) -> io::Result<Self> {
        Ok(Self {
            decisions: lamp_bus::Bus::new(root)?.subscribe_typed(
                lamp_msgs::Decision::TOPIC,
                crate::bus_out::SOURCE,
                lamp_msgs::Decision::TYPE,
            )?,
            decided: VecDeque::new(),
            shadowed: VecDeque::new(),
        })
    }

    /// Take what has come, without waiting: attention decisions made for
    /// real only.
    pub fn take(&mut self) {
        for _ in 0..KEPT {
            let Ok(Some(message)) = self.decisions.recv(Duration::ZERO) else {
                break;
            };
            if let Ok(decision) = message.parse::<lamp_msgs::Decision>() {
                self.took(&decision);
            }
        }
    }

    fn took(&mut self, decision: &lamp_msgs::Decision) {
        if decision.about != "attention" {
            return;
        }
        if decision.shadow {
            if let Some(request) = decision.request
                && !self.shadowed.contains(&request)
            {
                if self.shadowed.len() == KEPT {
                    self.shadowed.pop_front();
                }
                self.shadowed.push_back(request);
            }
            return;
        }
        // Which turn it judged is not known: nobody's.
        let Some(request) = decision.request else {
            return;
        };
        let follows = decision
            .answers
            .get("follows_lamp")
            .is_some_and(|yes| *yes >= FOLLOWS_FROM);
        let verdict = match (decision.choice.as_str(), follows) {
            ("stay_quiet", true) => Verdict::StayQuietFollowing,
            ("stay_quiet", false) => Verdict::StayQuiet,
            _ => Verdict::TakenUp,
        };
        let answer = decision.choice == "answer";
        if let Some(known) = self
            .decided
            .iter_mut()
            .find(|(turn, _, _)| *turn == request)
        {
            known.1 = verdict;
            known.2 = answer;
            return;
        }
        if self.decided.len() == KEPT {
            self.decided.pop_front();
        }
        self.decided.push_back((request, verdict, answer));
    }

    /// What the Policy decided of `request`, if it has yet.
    pub fn verdict(&self, request: u64) -> Option<Verdict> {
        self.decided
            .iter()
            .find(|(turn, _, _)| *turn == request)
            .map(|(_, verdict, _)| *verdict)
    }

    /// What the Policy's decision of `request` lets the lamp say of a line
    /// let out only as a conversation going on, in a busy room
    /// ([`crate::going_on_asks`], #1522): `answer`, or a `stay_quiet` said
    /// back to the lamp ([`Verdict::StayQuietFollowing`]). `look_up` turns
    /// to whoever spoke without speaking: its words are held, and its call
    /// runs as [`gate`] says ([`crate::going_on_asks::Says::LookUp`]).
    pub fn says(&self, request: u64) -> Option<crate::going_on_asks::Says> {
        use crate::going_on_asks::Says;
        self.decided
            .iter()
            .find(|(turn, _, _)| *turn == request)
            .map(|(_, verdict, answer)| match verdict {
                Verdict::TakenUp if *answer => Says::Answer,
                Verdict::TakenUp => Says::LookUp,
                Verdict::StayQuietFollowing => Says::Following,
                Verdict::StayQuiet => Says::Not,
            })
            .or_else(|| self.shadowed.contains(&request).then_some(Says::Shadow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As the Policy publishes one, read as the voice reads it.
    fn decision(
        about: &str,
        choice: &str,
        shadow: bool,
        request: Option<u64>,
    ) -> lamp_msgs::Decision {
        serde_json::from_value(json!({
            "about": about, "choice": choice, "probability": 0.235, "answers": {},
            "by": "typesafe/jev-1.13-20260917", "took_ms": 540, "shadow": shadow,
            "request": request
        }))
        .unwrap()
    }

    /// #1040: a `stay_quiet` whose `follows_lamp` is 0.6 or more is a reply
    /// to the lamp; below it, or with no such answer, it is not.
    #[test]
    fn a_stay_quiet_that_follows_the_lamp_is_told_apart_by_its_follows_lamp() {
        let (mut verdicts, dir) = verdicts("follows");
        for (follows, request) in [(Some(0.7), 1), (Some(0.6), 2), (Some(0.59), 3), (None, 4)] {
            let mut value =
                serde_json::to_value(decision("attention", "stay_quiet", false, Some(request)))
                    .unwrap();
            if let Some(follows) = follows {
                value["answers"] = json!({"calls_lamp": 0.05, "follows_lamp": follows});
            }
            verdicts.took(&serde_json::from_value(value).unwrap());
        }
        let read: Vec<Option<Verdict>> = (1..=4).map(|request| verdicts.verdict(request)).collect();
        assert_eq!(
            read,
            [
                Some(Verdict::StayQuietFollowing),
                Some(Verdict::StayQuietFollowing),
                Some(Verdict::StayQuiet),
                Some(Verdict::StayQuiet)
            ]
        );
        // An answer is taken up whatever it follows.
        let mut value =
            serde_json::to_value(decision("attention", "answer", false, Some(5))).unwrap();
        value["answers"] = json!({"follows_lamp": 0.1});
        verdicts.took(&serde_json::from_value(value).unwrap());
        assert_eq!(verdicts.verdict(5), Some(Verdict::TakenUp));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// #1522: for a line going on in a busy room, `answer` says it is the
    /// lamp's, a `stay_quiet` said back to the lamp says so too, `look_up`
    /// turns to whoever spoke without speaking whatever its `follows_lamp`,
    /// and any other `stay_quiet` says it is not; a newer decision about
    /// the same turn takes the place of the older.
    #[test]
    fn what_a_decision_says_of_a_line_going_on() {
        use crate::going_on_asks::Says;
        let (mut verdicts, dir) = verdicts("says");
        for (request, choice, follows) in [
            (1, "answer", 0.1),
            (2, "look_up", 0.1),
            (3, "look_up", 0.9),
            (4, "stay_quiet", 0.66),
            (5, "stay_quiet", 0.3),
        ] {
            let mut value =
                serde_json::to_value(decision("attention", choice, false, Some(request))).unwrap();
            value["answers"] = json!({"follows_lamp": follows});
            verdicts.took(&serde_json::from_value(value).unwrap());
        }
        // A decision in shadow only is acted on by nothing, the actions'
        // gate included, and says only that it came (the reviewer on
        // #1538): a real one about the same turn wins.
        verdicts.took(&decision("attention", "stay_quiet", true, Some(6)));
        assert_eq!(verdicts.says(6), Some(Says::Shadow));
        assert_eq!(verdicts.verdict(6), None);
        verdicts.took(&decision("attention", "stay_quiet", true, Some(7)));
        verdicts.took(&decision("attention", "answer", false, Some(7)));
        assert_eq!(verdicts.says(7), Some(Says::Answer));
        let said: Vec<Option<Says>> = (1..=6).map(|request| verdicts.says(request)).collect();
        assert_eq!(
            said,
            [
                Some(Says::Answer),
                Some(Says::LookUp),
                Some(Says::LookUp),
                Some(Says::Following),
                Some(Says::Not),
                Some(Says::Shadow)
            ]
        );
        // The actions' verdicts are as they were: `look_up` takes the turn
        // up for them.
        assert_eq!(verdicts.verdict(2), Some(Verdict::TakenUp));
        verdicts.took(&decision("attention", "stay_quiet", false, Some(1)));
        assert_eq!(verdicts.says(1), Some(Says::Not));
        verdicts.took(&decision("attention", "answer", false, Some(2)));
        assert_eq!(verdicts.says(2), Some(Says::Answer));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_action_runs_unless_the_turn_was_judged_not_for_the_lamp_and_the_name_was_not_heard() {
        let short = Duration::from_millis(300);
        assert_eq!(
            gate(Some(Verdict::StayQuiet), false, short, false),
            Gate::Hold
        );
        assert_eq!(
            gate(Some(Verdict::StayQuiet), true, short, false),
            Gate::Run
        );
        assert_eq!(gate(Some(Verdict::TakenUp), false, short, false), Gate::Run);
        assert_eq!(gate(None, false, short, false), Gate::Wait);
        assert_eq!(gate(None, true, short, false), Gate::Run);
        // Past the wait it runs as today.
        assert_eq!(gate(None, false, WAIT, false), Gate::Run);
        // A reply to the lamp (#1040): what it sets on the schedule runs,
        // the rest is held, a cancel among them (Fourth on #1054).
        let following = Some(Verdict::StayQuietFollowing);
        let schedule =
            crate::tools::Route::Schedule(std::path::PathBuf::from("/run/lamp-schedule"));
        for (name, run) in [
            ("set_timer", Gate::Run),
            ("set_reminder", Gate::Run),
            ("cancel_reminder", Gate::Hold),
        ] {
            assert_eq!(
                gate(following, false, short, sets(&schedule, name)),
                run,
                "{name}"
            );
        }
        assert_eq!(gate(following, false, short, false), Gate::Hold);
        assert_eq!(
            gate(Some(Verdict::StayQuiet), false, short, true),
            Gate::Hold
        );
        assert_eq!(held()["done"], false);
        assert!(HELD.contains("Say nothing"));
    }

    /// Only the schedule's `set_timer` and `set_reminder` are what a reply
    /// to the lamp still runs: never a cancel, and never another route's
    /// function of the same name.
    #[test]
    fn only_the_schedule_s_two_that_set_run_on_a_reply_to_the_lamp() {
        use crate::tools::Route;
        let place = std::path::PathBuf::from("/run/lamp-actions");
        for name in crate::tools::SCHEDULE_NAMES {
            assert_eq!(
                sets(&Route::Schedule(place.clone()), name),
                ["set_timer", "set_reminder"].contains(&name),
                "{name}"
            );
        }
        assert!(!sets(&Route::Executive(place), "set_timer"));
        assert!(!sets(&Route::Nobody, "set_reminder"));
    }

    /// #1465: with `--follows-run` a reply to the lamp runs the light's
    /// tools and the volume but its mute on the actions' route, never a
    /// skill's function nor a cancel; without it, only what [`sets`] says.
    #[test]
    fn with_follows_run_a_reply_runs_the_light_and_the_volume_only() {
        use crate::tools::Route;
        let place = std::path::PathBuf::from("/run/lamp-actions");
        let settings = crate::tools::Settings {
            exec: Some(place.clone()),
            skills: Some("/run/lamp-skills".into()),
            ..Default::default()
        };
        // The skills' own folder, and the old one in the actions'.
        let skills = [
            Route::Executive("/run/lamp-skills".into()),
            Route::Executive(place.join(crate::tools::SKILLS_DIR)),
        ];
        for (name, runs) in [
            ("volume_up", true),
            ("volume_down", true),
            ("volume_unmute", true),
            ("volume_mute", false),
            ("light_on", true),
            ("light_off", true),
            ("light_colour", true),
            ("light_dimmer", true),
            ("cancel_reminder", false),
            ("dance", false),
        ] {
            assert_eq!(
                runs_on_a_reply(&settings, &Route::Executive(place.clone()), name, true),
                runs,
                "{name}"
            );
            assert!(
                !runs_on_a_reply(&settings, &Route::Executive(place.clone()), name, false),
                "{name}"
            );
        }
        for name in [
            "check_my_work",
            "dance",
            "show_verdict",
            "light_on",
            "volume_up",
        ] {
            for skills in &skills {
                assert!(
                    !runs_on_a_reply(&settings, skills, name, true),
                    "the skill's {name} at {skills:?}"
                );
            }
        }
        for name in crate::tools::SCHEDULE_NAMES {
            for light_and_volume in [true, false] {
                assert_eq!(
                    runs_on_a_reply(
                        &settings,
                        &Route::Schedule(place.clone()),
                        name,
                        light_and_volume
                    ),
                    sets(&Route::Schedule(place.clone()), name),
                    "{name}"
                );
            }
        }
    }

    /// The executive's functions and the schedule's three that set or take
    /// away are held; the schedule's others, and every other route, are not.
    #[test]
    fn the_executive_and_three_of_the_schedule_are_held() {
        use crate::tools::Route;
        let place = std::path::PathBuf::from("/run/lamp-actions");
        assert!(holds(&Route::Executive(place.clone()), "volume_up"));
        for name in crate::tools::SCHEDULE_NAMES {
            assert_eq!(
                holds(&Route::Schedule(place.clone()), name),
                ["set_timer", "set_reminder", "cancel_reminder"].contains(&name),
                "{name}"
            );
        }
        for route in [
            Route::Clock,
            Route::Decline,
            Route::Memory(place.clone(), None, None),
            Route::Nobody,
        ] {
            assert!(!holds(&route, "set_timer"));
        }
    }

    /// #1712: both routes say why a call ran in the same words, which the
    /// battery's counters read, and take a reply to the lamp the same way:
    /// not one after a line left unanswered, nor out of conversation.
    #[test]
    fn why_a_call_ran_and_a_reply_to_the_lamp_are_one_for_both_routes() {
        use Verdict::*;
        assert_eq!(why_run(Some(StayQuiet), true, false), "its name was heard");
        assert_eq!(
            why_run(Some(StayQuietFollowing), false, false),
            "the turn follows the lamp's last line, and its light and its volume are not held from it"
        );
        assert_eq!(
            why_run(Some(StayQuietFollowing), false, true),
            "the turn follows the lamp's last line, and what it sets on the schedule is not held from it"
        );
        assert_eq!(
            why_run(Some(TakenUp), false, false),
            "the Policy took the turn as the lamp's"
        );
        assert_eq!(why_run(None, false, false), "no decision came in time");
        assert_eq!(
            for_turn(StayQuietFollowing, false, true),
            StayQuietFollowing
        );
        assert_eq!(for_turn(StayQuietFollowing, true, true), StayQuiet);
        assert_eq!(for_turn(StayQuietFollowing, false, false), StayQuiet);
        assert_eq!(for_turn(TakenUp, true, false), TakenUp);
        assert_eq!(for_turn(StayQuiet, false, true), StayQuiet);
    }

    #[test]
    fn the_switch_is_on_only_and_its_flag_is_taken_once() {
        assert_eq!(arguments(Some("on"), None), [FLAG]);
        assert_eq!(arguments(Some("on"), Some("on")), [FLAG, FOLLOWS_FLAG]);
        for not_on in [None, Some("yes"), Some("ON"), Some("1")] {
            assert!(arguments(not_on, None).is_empty());
            // A reply runs nothing more where nothing is held.
            assert!(arguments(not_on, Some("on")).is_empty());
            assert_eq!(arguments(Some("on"), not_on), [FLAG]);
        }
        let flags: Vec<String> = [FLAG, FOLLOWS_FLAG, "--next"].map(String::from).into();
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
        assert_eq!(take_follows(&flags[1..]), (true, &flags[2..]));
        assert_eq!(take_follows(&flags[2..]), (false, &flags[2..]));
    }

    fn verdicts(tag: &str) -> (Verdicts, std::path::PathBuf) {
        // Short: a socket's whole path must fit in 103 bytes.
        let dir = std::path::PathBuf::from(format!("/tmp/lphu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Verdicts::on(&dir).unwrap(), dir)
    }

    /// Each decision is its own turn's, by number, whenever it comes: also
    /// before the turn's action is asked for. Only attention decisions made
    /// for real count; one with no number is nobody's.
    #[test]
    fn each_decision_is_its_own_turns_by_number() {
        let (mut verdicts, dir) = verdicts("one");
        verdicts.took(&decision("attention", "answer", false, None));
        assert!(verdicts.decided.is_empty());
        // A shadow decision, and a progress one, are not the turn's.
        verdicts.took(&decision("attention", "answer", true, Some(1)));
        verdicts.took(&decision("progress", "say_now", false, Some(1)));
        assert_eq!(verdicts.verdict(1), None);
        verdicts.took(&decision("attention", "stay_quiet", false, Some(1)));
        verdicts.took(&decision("attention", "look_up", false, Some(2)));
        assert_eq!(
            (
                verdicts.verdict(1),
                verdicts.verdict(2),
                verdicts.verdict(3)
            ),
            (Some(Verdict::StayQuiet), Some(Verdict::TakenUp), None)
        );
        // A turn's words judged in two pieces: the newer decision.
        verdicts.took(&decision("attention", "answer", false, Some(1)));
        assert_eq!(verdicts.verdict(1), Some(Verdict::TakenUp));
        // Never more than some minutes' worth is kept.
        for turn in 10..40 {
            verdicts.took(&decision("attention", "answer", false, Some(turn)));
        }
        assert_eq!(verdicts.decided.len(), KEPT);
        assert_eq!(verdicts.verdict(39), Some(Verdict::TakenUp));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reviewer on #908, 05:09: A's sound ends, B's 2.8 s later, and A's
    /// decision comes after B's sound ended, then B's own. A's decision
    /// never settles B: B waits for its own, in both orders of answer and
    /// stay_quiet.
    #[test]
    fn a_late_decision_of_the_turn_before_never_releases_or_holds_this_turns_action() {
        for (a, b, expected) in [
            ("answer", "stay_quiet", Gate::Hold),
            ("stay_quiet", "answer", Gate::Run),
        ] {
            let (mut verdicts, dir) = verdicts("late");
            verdicts.took(&decision("attention", a, false, Some(1)));
            let waited = Duration::from_millis(500);
            assert_eq!(
                gate(verdicts.verdict(2), false, waited, false),
                Gate::Wait,
                "{a}"
            );
            verdicts.took(&decision("attention", b, false, Some(2)));
            assert_eq!(
                gate(verdicts.verdict(2), false, waited, false),
                expected,
                "{a}"
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    /// The reviewer's second case: two turns whose decisions were once
    /// taken as ambiguous, then a third. The second turn's decision coming
    /// late does not settle the third, whose own decision does.
    #[test]
    fn a_decision_still_owed_to_an_earlier_turn_never_settles_a_later_one() {
        let (mut verdicts, dir) = verdicts("owed");
        verdicts.took(&decision("attention", "stay_quiet", false, Some(1)));
        verdicts.took(&decision("attention", "answer", false, Some(2)));
        assert_eq!(verdicts.verdict(3), None);
        verdicts.took(&decision("attention", "stay_quiet", false, Some(3)));
        assert_eq!(
            [1, 2, 3].map(|turn| verdicts.verdict(turn)),
            [
                Some(Verdict::StayQuiet),
                Some(Verdict::TakenUp),
                Some(Verdict::StayQuiet)
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
