//! The helper's real doubles for what this build does not have yet. They
//! refuse, never pretend: a package reader that reads nothing, and
//! companions that place nothing (so removing them is truthfully nothing).
use crate::{
    package::{AppPackage, PackageReader, SkillFolder},
    remove::Companions,
};
use std::path::PathBuf;

/// Until the FORMAT 2 reader of lamp-skills is wired in, no package is read.
pub struct NoReader;

impl PackageReader for NoReader {
    fn read(&self, _bytes: &[u8]) -> Result<AppPackage, String> {
        Err("this build cannot read app packages yet".into())
    }
}

/// No companion skill is ever placed by this build, so none is there to
/// remove. A package that brings one is refused rather than half-installed.
pub struct SkillsFolder {
    /// The skills folder, read only to see whether a name is taken.
    pub skills: PathBuf,
}

impl Companions for SkillsFolder {
    fn remove_for(&mut self, _app: &str) -> Result<(), String> {
        Ok(())
    }

    fn place(&mut self, _app: &str, folders: &[SkillFolder]) -> Result<(), String> {
        if folders.is_empty() {
            Ok(())
        } else {
            Err("this build cannot place an app's companion skills yet".into())
        }
    }

    fn taken_by_skill(&mut self, name: &str) -> bool {
        std::fs::symlink_metadata(self.skills.join(name)).is_ok()
    }
}

/// The uid of `name` in `/etc/passwd`'s text.
pub fn uid_in(passwd: &str, name: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == name).then_some(())?;
        fields.nth(1)?.parse().ok()
    })
}
