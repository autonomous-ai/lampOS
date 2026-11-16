//! An OpenClaw task's own steps on the bus (`tasks/step`, #7267), read
//! from OpenClaw's SQLite transcript. Not built yet: the tests come first.
use crate::engine::Watched;
use lamp_msgs::TaskStep;
#[cfg(test)]
use lamp_msgs::{StepKind, TASK_STEP_MOST_RESULT_BYTES};
#[cfg(test)]
use rusqlite::{Connection, params};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::{collections::BTreeMap, path::PathBuf};

pub const SWITCH: &str = "LAMP_TASKS_STEPS";
pub const STATE_SETTING: &str = "LAMP_TASKS_STEPS_STATE";
pub const EVERY_MS: u64 = 1_000;
pub const MOST_STEPS: u32 = 50;
pub const DATABASE: &str = "agent/openclaw-agent.sqlite";

pub fn switched(value: Option<&str>, roots: Vec<PathBuf>) -> Option<Steps> {
    (value == Some("on")).then(|| Steps::new(roots))
}

pub fn on() -> bool {
    false
}

pub fn roots(_override_dir: Option<&str>) -> Vec<PathBuf> {
    Vec::new()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Unread;

#[derive(Debug, Default)]
struct Tail {
    after: i64,
    dropped: u64,
    undecoded: u64,
}

#[derive(Debug)]
pub struct Steps {
    tails: BTreeMap<u64, Tail>,
}

impl Steps {
    pub fn new(_roots: Vec<PathBuf>) -> Self {
        Self {
            tails: BTreeMap::new(),
        }
    }

    pub fn tick(&mut self, _watched: &[Watched], _now_ms: u64, _wall_ms: u64) -> Vec<TaskStep> {
        Vec::new()
    }
}

pub fn session(_roots: &[PathBuf], _agent: &str) -> Result<Option<(PathBuf, String)>, Unread> {
    Err(Unread)
}

#[cfg(test)]
mod tests;
