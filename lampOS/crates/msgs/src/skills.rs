//! What the physical skills did and why (`skills/events`, `docs/skills.md`).
//! Published by `lamp-skills` at each stage of a skill's run.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// A stage of one run of a skill, in the order they come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillStage {
    /// A trigger's conditions held.
    Triggered,
    /// The Policy was asked whether the lamp may do it now.
    Asked,
    /// The Policy's answer, in `allowed`.
    Answered,
    /// The steps were run; `why` says how each went.
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEvent {
    /// The skill's name, as its `SKILL.md` says: 1 to 64 lowercase letters,
    /// digits and hyphens.
    pub skill: String,
    pub stage: SkillStage,
    /// In words: what held, what was asked, what each step did.
    pub why: String,
    /// Whether the Policy let it go ahead. Only with `answered`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<bool>,
}

impl SkillEvent {
    pub const TYPE: &str = "lamp_msgs/SkillEvent";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "skills/events";

    /// Something that happened: no period, nothing kept.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.skill.is_empty()
            || self.skill.len() > 64
            || !self
                .skill
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Invalid(
                "a skill is 1..64 lowercase letters, digits or hyphens".into(),
            ));
        }
        if self.why.len() > 2_000 {
            return Err(Invalid("why is at most 2,000 bytes".into()));
        }
        if self.allowed.is_some() != (self.stage == SkillStage::Answered) {
            return Err(Invalid(
                "allowed comes with answered and only with it".into(),
            ));
        }
        Ok(())
    }
}

/// A stage of a skill's `do.task` run, or of a skill's answer at its gate
/// (`skills/tasks`, #7873).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTaskStage {
    /// lamp-tasks began the run on its engine (`run`, `engine`).
    Started,
    /// The run ended well (`run`).
    Ran,
    /// The run ended without its agent's answer, or was refused (`run`).
    Failed,
    /// A skill's answer waits for its gate (`gate`): said again each time
    /// the reason changes.
    Held,
    /// The answer was said.
    Told,
    /// The answer was given up unsaid.
    Dropped,
}

/// What became of a skill's `do.task` run and of its answer (#7873):
/// lamp-tasks says when it began a run, on which engine, and how it ended;
/// lamp-skills says when a skill's answer (its agent's, or a read's) waits
/// for its gate and why, then whether it was told or dropped. The skill's
/// name, the run's id, the engine and the gate's own fixed reason only:
/// never a word of the task's, the agent's or the answer's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTask {
    /// As [`SkillEvent::skill`].
    pub skill: String,
    pub stage: SkillTaskStage,
    /// lamp-tasks' id of the run (`task-<ms>-<pid>-<n>`, as its journal
    /// names it): with `started`, `ran` and `failed`, and only with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Who has the run, as `tasks/status` names engines: with `started`,
    /// and only with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// Why the answer waits, as the gate says it (`not alone`, `a voice
    /// heard`, `private`): with `held`, and only with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
}

impl SkillTask {
    pub const TYPE: &str = "lamp_msgs/SkillTask";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "skills/tasks";

    /// Something that happened: no period, nothing kept.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        use SkillTaskStage::{Failed, Held, Ran, Started};
        let bad = |what: &str| Err(Invalid(what.into()));
        let all = |text: &str, most: usize, allowed: fn(u8) -> bool| {
            !text.is_empty() && text.len() <= most && text.bytes().all(allowed)
        };
        if !all(&self.skill, 64, |byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
        }) {
            return bad("a skill is 1..64 lowercase letters, digits or hyphens");
        }
        if self.run.is_some() != matches!(self.stage, Started | Ran | Failed) {
            return bad("run comes with started, ran and failed, and only with them");
        }
        if self
            .run
            .as_deref()
            .is_some_and(|run| !all(run, 64, |byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        {
            return bad("a run is 1..64 letters, digits or hyphens");
        }
        if self.engine.is_some() != (self.stage == Started) {
            return bad("engine comes with started and only with it");
        }
        if self.engine.as_deref().is_some_and(|engine| {
            !all(engine, 32, |byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
            })
        }) {
            return bad("an engine is 1..32 lowercase letters, digits or _");
        }
        if self.gate.is_some() != (self.stage == Held) {
            return bad("gate comes with held and only with it");
        }
        if self.gate.as_deref().is_some_and(|gate| {
            !all(gate, 40, |byte| {
                byte.is_ascii_lowercase() || b" '".contains(&byte)
            })
        }) {
            return bad("a gate is 1..40 lowercase letters, spaces or apostrophes");
        }
        Ok(())
    }
}

/// The skills part's optional topics, apart from the first topics.
pub fn skills_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (SkillEvent::TOPIC, SkillEvent::declaration()),
        (SkillTask::TOPIC, SkillTask::declaration()),
    ]
}
