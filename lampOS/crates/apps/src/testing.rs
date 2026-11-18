//! Test doubles for this crate and for piece B's tests: a fake `Effects`
//! that records the command lines, a fake for the companion skills, and a
//! stand-in for the manifest until nine's parser lands. Test-only data.
use crate::{
    effects::{Effects, Verb, argv},
    facts::AppFacts,
    lock::AppsLock,
    package::{AppPackage, SkillFolder},
    remove::Companions,
    yes::{Yes, YesKey},
};
use std::{path::PathBuf, time::Duration};

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
    /// The exit status `systemctl` gives a command line (default 0): the
    /// fake judges it as the real one does ([`crate::effects::judge_status`]).
    pub exits: Vec<(String, i32)>,
    /// A `clean` removes the app's data link and folder under this layout,
    /// as systemd would (the real thing is first seen on a lamp).
    pub cleans: Option<crate::layout::Layout>,
}

impl FakeEffects {
    pub fn cleaning(layout: &crate::layout::Layout) -> Self {
        Self {
            cleans: Some(layout.clone()),
            ..Default::default()
        }
    }
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
    fn forget(&mut self, name: &str) -> Result<(), String> {
        self.record(format!("app_forget {name}"))
    }

    fn systemctl(&mut self, verb: Verb, unit: &str) -> Result<(), String> {
        let line = argv(verb, unit).join(" ");
        self.record(line.clone())?;
        let code = self
            .exits
            .iter()
            .find(|(l, _)| *l == line)
            .map_or(0, |(_, c)| *c);
        crate::effects::judge_status(verb, unit, Some(code))?;
        if verb == Verb::Clean
            && let Some(layout) = &self.cleans
            && let Some(name) = unit
                .strip_prefix("lampapp-net@")
                .or_else(|| unit.strip_prefix("lampapp@"))
                .and_then(|rest| rest.strip_suffix(".service"))
        {
            for path in [layout.data_link(name), layout.data_real(name)] {
                let _ = match std::fs::symlink_metadata(&path) {
                    Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path),
                    Ok(_) => std::fs::remove_file(&path),
                    Err(_) => Ok(()),
                };
            }
        }
        Ok(())
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
    /// Runs inside `place`, before it answers: another caller's turn in the
    /// gap where the apps lock is let go.
    pub on_place: Option<Box<dyn FnMut()>>,
    pub removed: Vec<String>,
    pub fail: bool,
    /// Each placement: the app and its skills' names.
    pub placed: Vec<(String, Vec<String>)>,
    pub fail_place: bool,
    /// Names a skill already has.
    pub taken: Vec<String>,
    /// The state folder whose apps lock must NOT be held in a call here.
    pub lock_probe: Option<PathBuf>,
    /// A call was made with the apps lock held.
    pub violation: bool,
}

impl FakeCompanions {
    fn probe(&mut self) {
        // A little patience: a test that starts a process in another thread
        // holds a copy of every open file until its exec, the lock with it.
        // A lock held by the caller is held for much longer than this.
        if let Some(state) = &self.lock_probe
            && AppsLock::take(state, Duration::from_millis(400)).is_err()
        {
            self.violation = true;
        }
    }
}

impl Companions for FakeCompanions {
    fn remove_for(&mut self, app: &str) -> Result<(), String> {
        self.probe();
        if self.fail {
            return Err("the fake companions failed".into());
        }
        self.removed.push(app.to_owned());
        Ok(())
    }

    fn place(&mut self, app: &str, folders: &[SkillFolder]) -> Result<(), String> {
        self.probe();
        if let Some(turn) = self.on_place.as_mut() {
            turn();
        }
        if self.fail_place {
            return Err("the fake companions failed to place".into());
        }
        self.placed.push((
            app.to_owned(),
            folders.iter().map(|f| f.name.clone()).collect(),
        ));
        Ok(())
    }

    fn taken_by_skill(&mut self, name: &str) -> bool {
        self.taken.iter().any(|n| n == name)
    }
}

/// The owner's yes as lamp-control would keep it: a list of keys, each used
/// up once.
pub struct FakeYes {
    granted: Vec<YesKey>,
    /// How many times the yes was asked for.
    pub asked: usize,
}

impl FakeYes {
    pub fn granting(keys: Vec<YesKey>) -> Self {
        Self {
            granted: keys,
            asked: 0,
        }
    }
}

impl Yes for FakeYes {
    fn take(&mut self, key: &YesKey) -> Result<(), String> {
        self.asked += 1;
        match self.granted.iter().position(|granted| granted == key) {
            Some(at) => {
                self.granted.remove(at);
                Ok(())
            }
            None => Err("no yes for this".into()),
        }
    }
}

/// A reader that returns a fixed package whatever the bytes (the real one
/// verifies them).
pub struct StubReader {
    package: AppPackage,
}

impl StubReader {
    pub fn new(package: AppPackage) -> Self {
        Self { package }
    }
}

impl crate::package::PackageReader for StubReader {
    fn read(&self, _bytes: &[u8]) -> Result<AppPackage, String> {
        Ok(self.package.clone())
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
