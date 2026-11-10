//! The app told as each step of the setup walk ends (#5979, the design on
//! the issue): one unasked `data` reply on `fd_channel` per step, kind
//! [`KIND`], data `{"step", "outcome", "at", "walk_id"}`, as
//! `onboarding.done` is pushed. Only the four walk steps
//! (`lamp_msgs::OnboardingPending::steps_ended`), each with the walk's own
//! outcome (`done`, `skipped`, `failed` or `not_available`) and its end in
//! Unix ms. No names, no words.
//!
//! Read off the voice's `onboarding/pending` heartbeat every
//! [`crate::onboarding_done::EVERY`], only while connected. Each step's end,
//! `(step, at)`, is tried once per walk: kept in [`FILE`] beside the done
//! file, so a restart tries it no more; a new `walk_id` (a redo of the whole
//! walk) starts it over, and a step ended again (a redo of one step) is a
//! new `at`, sent again for the app to replace its mark. Best effort, the
//! lead's rule on #5979: one the client doesn't take is not tried again,
//! nothing waits for the broker's ack, and an end more than [`FOR`] old when
//! first looked at (ended offline) is let go. Behind `onboarding.done`'s
//! switch (`LAMP_APP_ONBOARDING_DONE`), and never holds `onboarding.done` up.
use lamp_msgs::OnboardingStepEnded;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Instant;

/// The kind the app follows the walk by.
pub const KIND: &str = "onboarding.step";

/// The file, beside `onboarding_done`'s, of what was sent for which walk.
pub const FILE: &str = "onboarding-step";

/// A step's end first looked at more than this long after it, in ms, is
/// let go.
pub const FOR: u64 = 120_000;

/// The reply's `data`.
pub fn data(walk_id: u64, ended: &OnboardingStepEnded) -> Value {
    json!({
        "step": ended.step,
        "outcome": ended.outcome,
        "at": ended.at,
        "walk_id": walk_id,
    })
}

/// What [`FILE`] keeps: the walk, and its steps' ends tried.
#[derive(Default, Serialize, Deserialize)]
struct Sent {
    walk_id: u64,
    steps: Vec<Tried>,
}

/// One step's end tried.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Tried {
    step: String,
    at: u64,
}

/// What was told of the walk's steps.
pub struct Steps {
    file: PathBuf,
    sent: Sent,
    looked: Option<Instant>,
}

impl Steps {
    /// Kept in `done`'s folder: `done` is `onboarding_done`'s file.
    pub fn new(done: PathBuf) -> Self {
        let file = done.with_file_name(FILE);
        let sent = std::fs::read(&file)
            .ok()
            .and_then(|text| serde_json::from_slice(&text).ok())
            .unwrap_or_default();
        Self {
            file,
            sent,
            looked: None,
        }
    }

    /// The steps to send now, with their walk's id: ended, not yet tried
    /// for this walk, and at most [`FOR`] old at `now_ms` (Unix ms). `read`
    /// gives the heartbeat's walk, at most every
    /// [`crate::onboarding_done::EVERY`].
    pub fn due(
        &mut self,
        now: Instant,
        now_ms: u64,
        read: impl FnOnce() -> Option<(u64, Vec<OnboardingStepEnded>)>,
    ) -> Option<(u64, Vec<OnboardingStepEnded>)> {
        if self
            .looked
            .is_some_and(|looked| now < looked + crate::onboarding_done::EVERY)
        {
            return None;
        }
        self.looked = Some(now);
        let (walk_id, ended) = read()?;
        if walk_id != self.sent.walk_id {
            self.sent = Sent {
                walk_id,
                steps: Vec::new(),
            };
        }
        let due: Vec<_> = ended
            .into_iter()
            .filter(|ended| {
                let tried = Tried {
                    step: ended.step.clone(),
                    at: ended.at,
                };
                !self.sent.steps.contains(&tried) && now_ms <= ended.at.saturating_add(FOR)
            })
            .collect();
        (!due.is_empty()).then_some((walk_id, due))
    }

    /// `ended`, of the walk `due` gave, was tried: `handed` to the client
    /// or not, never tried again. Kept in [`FILE`]. The journal's line.
    pub fn tried(&mut self, ended: &OnboardingStepEnded, handed: bool) -> String {
        self.sent.steps.push(Tried {
            step: ended.step.clone(),
            at: ended.at,
        });
        let walk = self.sent.walk_id;
        let step = &ended.step;
        let what = if handed {
            "handed to the client"
        } else {
            "not handed to the client, not tried again"
        };
        let text = serde_json::to_vec(&self.sent).unwrap_or_default();
        match std::fs::write(&self.file, text) {
            Ok(()) => format!("{KIND}: {step} {what}, walk {walk}"),
            Err(error) => format!(
                "{KIND}: {step} {what}, walk {walk}, not kept ({}): tried again after a restart",
                error.kind()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding_done::EVERY;

    const WALK: u64 = 1_762_675_100_000;

    fn ended(step: &str, outcome: &str, at: u64) -> OnboardingStepEnded {
        OnboardingStepEnded {
            step: step.into(),
            outcome: outcome.into(),
            at,
        }
    }

    fn steps(due: Option<(u64, Vec<OnboardingStepEnded>)>) -> Vec<String> {
        due.map(|(_, ended)| ended.into_iter().map(|ended| ended.step).collect())
            .unwrap_or_default()
    }

    /// Each step's end is due once per walk, handed or not, through a
    /// restart; one ended again (a redo of one step) is due again; a new
    /// walk's are due again; one first looked at past [`FOR`] is let go.
    #[test]
    fn each_steps_end_is_tried_once_a_walk() {
        let dir = tempfile::tempdir().unwrap();
        let done = dir.path().join(crate::onboarding_done::FILE);
        let mut told = Steps::new(done.clone());
        let t = Instant::now();
        let sit = ended("where_you_sit", "done", WALK + 10_000);
        let who = ended("who_you_are", "skipped", WALK + 40_000);
        let at = WALK + 41_000;
        assert_eq!(told.due(t, at, || None), None);
        let both = || Some((WALK, vec![sit.clone(), who.clone()]));
        let due = told.due(t + EVERY, at, both);
        assert_eq!(due.as_ref().map(|due| due.0), Some(WALK));
        assert_eq!(steps(due), ["where_you_sit", "who_you_are"]);
        // Looked at no more often than every second.
        assert_eq!(told.due(t + EVERY, at, both), None);
        // "Where you sit" handed; "who you are" not (a full client queue):
        // neither is tried again.
        assert!(told.tried(&sit, true).contains("handed to the client"));
        assert!(told.tried(&who, false).contains("not tried again"));
        assert_eq!(told.due(t + 2 * EVERY, at, both), None);
        // A restart reads what was tried; "touch" is new, told; one ended
        // past two minutes before it is first looked at is let go.
        let mut again = Steps::new(done.clone());
        let touch = ended("touch", "not_available", WALK + 50_000);
        let sound = ended("how_you_sound", "failed", WALK + 50_000 - FOR - 1);
        let four = || {
            Some((
                WALK,
                vec![sit.clone(), who.clone(), touch.clone(), sound.clone()],
            ))
        };
        assert_eq!(steps(again.due(t, WALK + 50_000, four)), ["touch"]);
        again.tried(&touch, true);
        // "Who you are" redone in the same walk: a new end, told again.
        let who_again = ended("who_you_are", "done", WALK + 90_000);
        let redone = || Some((WALK, vec![sit.clone(), who_again.clone()]));
        assert_eq!(
            steps(again.due(t + EVERY, WALK + 91_000, redone)),
            ["who_you_are"]
        );
        // A redo of the whole walk: a new id, its steps told again.
        let redo = WALK + 600_000;
        let sit_again = ended("where_you_sit", "failed", redo + 5_000);
        let mut fresh = Steps::new(done);
        assert_eq!(
            fresh.due(t, redo + 6_000, || Some((redo, vec![sit_again.clone()]))),
            Some((redo, vec![sit_again.clone()]))
        );
        fresh.tried(&sit_again, true);
        let kept: Sent =
            serde_json::from_slice(&std::fs::read(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(kept.walk_id, redo);
        assert_eq!(
            kept.steps,
            [Tried {
                step: "where_you_sit".into(),
                at: redo + 5_000
            }]
        );
    }

    /// The reply's data is the step, its outcome, when and the walk: no
    /// other key, so no name or words can ride along.
    #[test]
    fn the_data_holds_nothing_personal() {
        let value = data(WALK, &ended("who_you_are", "done", WALK + 1));
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["at", "outcome", "step", "walk_id"]);
        assert_eq!(
            value,
            json!({"step": "who_you_are", "outcome": "done", "at": WALK + 1, "walk_id": WALK})
        );
    }
}
