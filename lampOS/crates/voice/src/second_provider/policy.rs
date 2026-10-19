//! The Policy holds the lamp's actions on the second route (#800, #1712,
//! `LAMP_POLICY_HOLDS_ACTIONS=on`, with `LAMP_FOLLOWS_RUN`), as it holds
//! them on the Live route (`crate::policy_holds`): a call of a spoken turn
//! that changes the lamp, or sets or takes away something on its schedule
//! ([`crate::policy_holds::holds`]), waits for the Policy's attention
//! decision about its turn, at most [`crate::policy_holds::WAIT`], and runs
//! or is held by the same gate. The journal says each as the Live route
//! says it, `lamp-voice second:` for `lamp-voice provider:`, so the
//! battery's counters (`followups.sh`'s "; run after N ms") read both.
//!
//! The decision is its own turn's by number: the request this route gives
//! a spoken turn is the one `voice/heard` tells its words in (the engine's
//! `Started` before the turn's heard words, the coordinator's
//! `bus_out::input_started`).
//!
//! The call waits in the turn's own task, looking for the decision every
//! [`LOOK_EVERY`]: nothing else waits, and a newer turn, which ends this
//! one's task, lets the call go unrun (said in the journal).
//!
//! The decision is read through [`Decided`], which the route builds on its
//! rule's one reader of the Policy's decisions (`worker::Heeding`), the
//! same a line going on in a busy room waits on (`LAMP_GOING_ON_ASKS`,
//! #1969): one way to read the Policy's decision by request.
use super::{Policed, PolicyGate};
use std::{sync::Arc, time::Duration};

/// How often a waiting call looks for its turn's decision.
pub const LOOK_EVERY: Duration = Duration::from_millis(20);

/// What the route knew of a spoken turn as its line was judged, for the
/// gate (as the Live worker's own `Turn`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    /// The lamp's name was heard in it
    /// ([`crate::policy_holds::NAME_FROM`]).
    pub name_heard: bool,
    /// It follows a line someone said that the lamp left unanswered, not
    /// the lamp's own last words (#1054).
    pub after_unanswered: bool,
    /// The lamp's words were heard within
    /// [`crate::policy_holds::IN_CONVERSATION_US`] before it.
    pub lamp_spoke_lately: bool,
}

/// Where the gate is told a turn's facts, by its request.
pub type Facts = Arc<dyn Fn(u64) -> Turn + Send + Sync>;
/// What the Policy has decided of a request, if it has yet: the kind of
/// its decision ([`crate::policy_holds::Verdict`]), read without waiting.
pub type Decided = Arc<dyn Fn(u64) -> Option<crate::policy_holds::Verdict> + Send + Sync>;
/// Whether the route answers a call of this name itself, before any is
/// routed (the web search's functions, `more_functions`): never the lamp's
/// action, though routed it would be the executive's.
pub type AnsweredHere = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// The route's place for the Policy's decisions.
pub struct Desk {
    decided: Decided,
    settings: crate::tools::Settings,
    /// A reply to the lamp runs its light and its volume too (#1465).
    follows_run: bool,
    facts: Facts,
    here: AnsweredHere,
}

impl Desk {
    pub fn new(
        decided: Decided,
        settings: crate::tools::Settings,
        follows_run: bool,
        facts: Facts,
        here: AnsweredHere,
    ) -> Self {
        Self {
            decided,
            settings,
            follows_run,
            facts,
            here,
        }
    }

    /// The engine's gate ([`super::Engine::holding_for_the_policy`]).
    pub fn gate(self: Arc<Self>) -> PolicyGate {
        Arc::new(move |request, name| Box::pin(self.clone().wait(request, name)))
    }

    /// What becomes of `request`'s call of `name`: at once [`Policed::Run`]
    /// for one the Policy never holds, or one the route answers itself;
    /// else run or held by its turn's decision, the name heard, or the
    /// wait's end.
    pub async fn wait(self: Arc<Self>, request: u64, name: String) -> Policed {
        let route = crate::tools::route(&self.settings, &name);
        if (self.here)(&name) || !crate::policy_holds::holds(&route, &name) {
            return Policed::Run;
        }
        eprintln!(
            "lamp-voice second: request {request} asked for {name}; waiting for the Policy's decision"
        );
        let called = tokio::time::Instant::now();
        let turn = (self.facts)(request);
        let on_a_reply =
            crate::policy_holds::runs_on_a_reply(&self.settings, &route, &name, self.follows_run);
        let mut waiting = Waiting {
            request,
            name: name.clone(),
            settled: false,
        };
        loop {
            let verdict = (self.decided)(request).map(|verdict| {
                crate::policy_holds::for_turn(
                    verdict,
                    turn.after_unanswered,
                    turn.lamp_spoke_lately,
                )
            });
            let waited = called.elapsed();
            match crate::policy_holds::gate(verdict, turn.name_heard, waited, on_a_reply) {
                crate::policy_holds::Gate::Wait => tokio::time::sleep(LOOK_EVERY).await,
                crate::policy_holds::Gate::Run => {
                    waiting.settled = true;
                    let why = crate::policy_holds::why_run(
                        verdict,
                        turn.name_heard,
                        crate::policy_holds::sets(&route, &name),
                    );
                    eprintln!(
                        "lamp-voice second: request {request} asked for {name}; run after {} ms: {why}",
                        waited.as_millis()
                    );
                    return Policed::Run;
                }
                crate::policy_holds::Gate::Hold => {
                    waiting.settled = true;
                    eprintln!(
                        "lamp-voice second: request {request} asked for {name}; not done: the Policy judged the turn not for the lamp, and its name was not heard ({} ms)",
                        waited.as_millis()
                    );
                    return Policed::Held;
                }
            }
        }
    }
}

/// A call waiting for its turn's decision: let go unrun if its turn ends
/// first (a newer turn, the turn spoken over), as the Live route says it.
struct Waiting {
    request: u64,
    name: String,
    settled: bool,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if !self.settled {
            eprintln!(
                "lamp-voice second: request {} asked for {}; its turn ended while it waited for the Policy's decision; not run",
                self.request, self.name
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The gate on a bus of the test's own, the Policy's place on it, and
    /// the executive's and the schedule's calls routed as on a lamp.
    struct Bench {
        dir: std::path::PathBuf,
        decisions: lamp_bus::Publisher,
    }

    impl Bench {
        fn new(tag: &str) -> Self {
            // Short: a socket's whole path must fit in 103 bytes.
            let dir = std::path::PathBuf::from(format!("/tmp/lsp-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let decisions = lamp_bus::Bus::new(&dir)
                .unwrap()
                .publisher_declared(
                    lamp_msgs::Decision::TOPIC,
                    &lamp_msgs::Decision::declaration(),
                    "policy",
                )
                .unwrap();
            Self { dir, decisions }
        }

        fn desk(&self, follows_run: bool, turn: Turn) -> Arc<Desk> {
            let settings = crate::tools::Settings {
                exec: Some(self.dir.clone()),
                schedule_tools: true,
                ..Default::default()
            };
            let search = Arc::new(
                crate::second_provider::net::search::Search::deepseek_at("http://127.0.0.1:9", "k")
                    .unwrap(),
            );
            let verdicts =
                std::sync::Mutex::new(crate::policy_holds::Verdicts::on(&self.dir).unwrap());
            Arc::new(Desk::new(
                Arc::new(move |request| {
                    let mut verdicts = verdicts.lock().unwrap();
                    verdicts.take();
                    verdicts.verdict(request)
                }),
                settings,
                follows_run,
                Arc::new(move |_| turn),
                super::super::worker::answered_here(Some(search)),
            ))
        }

        /// The Policy decides about turn `request`, for real, as it
        /// publishes one (`lamp-policy --act attention`).
        fn decide(&mut self, request: u64, choice: &str, follows_lamp: Option<f32>) {
            let answers = follows_lamp.map_or(json!({}), |yes| json!({ "follows_lamp": yes }));
            let decision = json!({"about": "attention", "choice": choice, "probability": 0.235,
                                  "answers": answers, "by": "typesafe/jev-1.13-20260917",
                                  "took_ms": 540, "shadow": false, "request": request});
            self.decisions.publish_json(0, &decision).unwrap();
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// In conversation, after no line left unanswered.
    const TALKING: Turn = Turn {
        name_heard: false,
        after_unanswered: false,
        lamp_spoke_lately: true,
    };

    /// #800 on the second route (#1712): the lamp's action in a turn the
    /// Policy judged `stay_quiet` is held; in one it took up (`answer`,
    /// `look_up`) it runs; each by its own turn's number.
    #[tokio::test]
    async fn a_stay_quiet_holds_the_lamps_action_and_a_turn_taken_up_runs_it() {
        let mut bench = Bench::new("taken");
        let desk = bench.desk(false, TALKING);
        bench.decide(1, "stay_quiet", None);
        bench.decide(2, "answer", None);
        bench.decide(3, "look_up", None);
        assert_eq!(desk.clone().wait(1, "light_on".into()).await, Policed::Held);
        assert_eq!(
            desk.clone().wait(1, "set_timer".into()).await,
            Policed::Held
        );
        assert_eq!(desk.clone().wait(2, "light_on".into()).await, Policed::Run);
        assert_eq!(desk.clone().wait(3, "volume_up".into()).await, Policed::Run);
    }

    /// With no decision about its own turn, the action runs after
    /// `policy_holds::WAIT`, as without the switch; another turn's
    /// decision never settles it (the reviewer on #908).
    #[tokio::test(start_paused = true)]
    async fn with_no_decision_of_its_own_turn_it_runs_after_the_wait() {
        let mut bench = Bench::new("none");
        let desk = bench.desk(false, TALKING);
        bench.decide(2, "stay_quiet", None);
        let called = tokio::time::Instant::now();
        assert_eq!(desk.wait(1, "light_on".into()).await, Policed::Run);
        let waited = called.elapsed();
        assert!(
            waited >= crate::policy_holds::WAIT
                && waited < crate::policy_holds::WAIT + 2 * LOOK_EVERY,
            "{waited:?}"
        );
    }

    /// The lamp's name heard in the turn runs its action whatever the
    /// Policy said, at once.
    #[tokio::test(start_paused = true)]
    async fn the_name_heard_runs_it_at_once_whatever_the_decision() {
        let mut bench = Bench::new("name");
        let named = Turn {
            name_heard: true,
            ..TALKING
        };
        let desk = bench.desk(false, named);
        bench.decide(1, "stay_quiet", None);
        let called = tokio::time::Instant::now();
        assert_eq!(desk.clone().wait(1, "light_on".into()).await, Policed::Run);
        assert_eq!(desk.wait(2, "volume_up".into()).await, Policed::Run);
        assert_eq!(called.elapsed(), Duration::ZERO);
    }

    /// A call the Policy never holds (a lookup, the reminders' list) runs
    /// at once, with no decision waited for.
    #[tokio::test(start_paused = true)]
    async fn a_call_the_policy_never_holds_runs_at_once() {
        let bench = Bench::new("never");
        let desk = bench.desk(false, TALKING);
        let called = tokio::time::Instant::now();
        for name in [
            "the_time",
            "list_reminders",
            "snooze_reminder",
            "web_search",
            "fetch_page",
            super::super::worker::MORE,
        ] {
            assert_eq!(
                desk.clone().wait(1, name.into()).await,
                Policed::Run,
                "{name}"
            );
        }
        assert_eq!(called.elapsed(), Duration::ZERO);
    }

    /// #1040 and #1465 on the second route: a `stay_quiet` said back to the
    /// lamp (`follows_lamp` 0.6 or more), in conversation and not after a
    /// line left unanswered, runs what the schedule sets; with
    /// `LAMP_FOLLOWS_RUN` its light and its volume too, never `volume_mute`
    /// or `cancel_reminder`. Out of conversation, or after a line left
    /// unanswered, it is no reply to the lamp.
    #[tokio::test]
    async fn a_reply_to_the_lamp_runs_what_it_sets_and_with_follows_run_its_light() {
        let mut bench = Bench::new("reply");
        let desk = bench.desk(false, TALKING);
        let follows = bench.desk(true, TALKING);
        let unanswered = bench.desk(
            true,
            Turn {
                after_unanswered: true,
                ..TALKING
            },
        );
        let quiet = bench.desk(
            true,
            Turn {
                lamp_spoke_lately: false,
                ..TALKING
            },
        );
        bench.decide(1, "stay_quiet", Some(0.7));
        assert_eq!(desk.clone().wait(1, "set_timer".into()).await, Policed::Run);
        assert_eq!(
            desk.clone().wait(1, "set_reminder".into()).await,
            Policed::Run
        );
        assert_eq!(desk.clone().wait(1, "light_on".into()).await, Policed::Held);
        assert_eq!(desk.wait(1, "cancel_reminder".into()).await, Policed::Held);
        for (name, policed) in [
            ("light_on", Policed::Run),
            ("volume_up", Policed::Run),
            ("volume_mute", Policed::Held),
            ("cancel_reminder", Policed::Held),
            ("set_timer", Policed::Run),
        ] {
            assert_eq!(
                follows.clone().wait(1, name.into()).await,
                policed,
                "{name}"
            );
        }
        for desk in [unanswered, quiet] {
            assert_eq!(
                desk.clone().wait(1, "set_timer".into()).await,
                Policed::Held
            );
            assert_eq!(desk.wait(1, "light_on".into()).await, Policed::Held);
        }
    }

    /// A decision that comes while the call waits settles it then, on the
    /// next look.
    #[tokio::test]
    async fn a_decision_that_comes_while_the_call_waits_settles_it() {
        let mut bench = Bench::new("late");
        let desk = bench.desk(false, TALKING);
        let called = tokio::time::Instant::now();
        let (policed, ()) = tokio::join!(desk.wait(1, "light_on".into()), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            bench.decide(1, "stay_quiet", None);
        });
        assert_eq!(policed, Policed::Held);
        let waited = called.elapsed();
        assert!(
            waited >= Duration::from_millis(300) && waited < crate::policy_holds::WAIT,
            "{waited:?}"
        );
    }
}
