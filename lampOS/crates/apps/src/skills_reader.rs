//! The real [`PackageReader`]: nine's FORMAT 2 reader in lamp-skills (the
//! Store's signature, every file's size and SHA-256, the manifest's own
//! rules), mapped to what the helper acts on. It does not run the skill
//! checks on companion skills and does not compare `min_build`: the helper's
//! [`crate::facts::check`] does the latter, and [`crate::real::SkillsFolder`]
//! refuses the former's input until companions can be placed.
use crate::package::{AppPackage, AppSpec, PackageReader, SkillFolder};
use lamp_skills::app_package::read_app_package_bytes;

/// What a package with companion skills is told.
pub const NO_COMPANIONS: &str = "this build cannot place an app's companion skills yet";

pub struct SkillsReader {
    /// The Store's public keys, as `lamp_skills::package::store_keys` reads them.
    pub keys: Vec<[u8; 32]>,
}

impl PackageReader for SkillsReader {
    fn read(&self, bytes: &[u8]) -> Result<AppPackage, String> {
        let read = read_app_package_bytes(bytes, &self.keys)?;
        // No companion skills in this build: refused here, so that the owner's yes is never
        // asked for, let alone spent, on a package that cannot be installed. (The yes is keyed
        // on the SHA-256 of the very bytes read here, taken once by the caller.)
        if !read.skills.is_empty() || !read.manifest.skills.is_empty() {
            return Err(NO_COMPANIONS.into());
        }
        let m = &read.manifest;
        let mut files = read.files.clone();
        files.insert("lamp-skill.json".to_owned(), read.manifest_bytes.clone());
        Ok(AppPackage {
            app: AppSpec {
                name: m.name.clone(),
                title: m.title.clone(),
                version: m.version.clone(),
                min_build: m.min_build,
                net: m.net,
                logs: m.logs,
                grants: m.grants.clone(),
                entry: m.runtime.entry.clone(),
            },
            files,
            skills: read
                .skills
                .iter()
                .map(|(name, files)| SkillFolder {
                    name: name.clone(),
                    files: files.clone(),
                })
                .collect(),
        })
    }
}
