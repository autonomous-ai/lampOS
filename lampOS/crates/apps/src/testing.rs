//! Test doubles for this crate and for piece B's tests: a fake `Effects`
//! that records the command lines, a fake for the companion skills, and a
//! stand-in for the manifest until nine's parser lands. Test-only data.
use crate::{
    effects::{Effects, Verb, argv},
    facts::AppFacts,
    remove::Companions,
};

#[derive(Default)]
pub struct FakeEffects {
    /// Every call, in order, as the command line (or `empty? <unit>`).
    pub calls: Vec<String>,
    /// The call (by index) that fails, once: a crash stand-in.
    pub fail_at: Option<usize>,
    /// How many `unit_empty` asks say "not yet" first.
    pub busy_polls: u32,
    /// `unit_empty` never says yes.
    pub never_empty: bool,
}

impl FakeEffects {
    fn record(&mut self, line: String) -> Result<(), String> {
        self.calls.push(line);
        if self.fail_at == Some(self.calls.len() - 1) {
            return Err("the fake failed here".into());
        }
        Ok(())
    }
}

impl Effects for FakeEffects {
    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String> {
        self.record(argv(verb, unit).join(" "))
    }

    fn unit_empty(&mut self, unit: &str) -> Result<bool, String> {
        self.record(format!("empty? {unit}"))?;
        if self.never_empty {
            return Ok(false);
        }
        if self.busy_polls > 0 {
            self.busy_polls -= 1;
            return Ok(false);
        }
        Ok(true)
    }
}

#[derive(Default)]
pub struct FakeCompanions {
    pub removed: Vec<String>,
    pub fail: bool,
}

impl Companions for FakeCompanions {
    fn remove_for(&mut self, app: &str) -> Result<(), String> {
        if self.fail {
            return Err("the fake companions failed".into());
        }
        self.removed.push(app.to_owned());
        Ok(())
    }
}

/// What nine's `AppManifest` will say, as plain fields.
pub struct StubApp {
    pub name: String,
    pub title: String,
    pub version: String,
    pub min_build: u64,
    pub net: bool,
    pub grants: Vec<String>,
    pub entry: String,
}

impl StubApp {
    pub fn good(name: &str) -> Self {
        Self {
            name: name.into(),
            title: "A good app".into(),
            version: "1.0.0".into(),
            min_build: 1,
            net: false,
            grants: vec!["speak".into(), "hear".into()],
            entry: "app.main".into(),
        }
    }
}

impl AppFacts for StubApp {
    fn name(&self) -> &str {
        &self.name
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> &str {
        &self.version
    }
    fn min_build(&self) -> u64 {
        self.min_build
    }
    fn net(&self) -> bool {
        self.net
    }
    fn grants(&self) -> &[String] {
        &self.grants
    }
    fn entry(&self) -> &str {
        &self.entry
    }
}
