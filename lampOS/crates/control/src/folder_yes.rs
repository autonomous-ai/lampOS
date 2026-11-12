//! The real [`SkillYes`] for the run socket (#6304, #6071 PR 3): a one-run
//! token is minted only from the owner's yes (`task_yes`, kept here in
//! `--state-dir` and nowhere else) for exactly the skill's folder as it is at
//! the mint.
//!
//! - lamp-control **hashes the folder itself**, at each mint, from the
//!   skills' own folders ([`STORE_DIR`], and [`PUSHED_DIR`] for a skill a
//!   paired computer pushed): never from a hash or a list of grants lamp-tasks
//!   (or anyone) hands it. [`folder_sha256`] is the same definition as
//!   `lamp_skills::task::folder_sha256`, which lamp-control cannot call
//!   (lamp-skills is built on top of this crate); a fixture folder with its
//!   hash written down holds the two together ([`FIXTURE_SHA256`], pinned
//!   by the same test in lamp-skills).
//! - **No yes** for the name is refused ([`WAITING`]); **a yes for other
//!   bytes** than the folder has now is refused ([`CHANGED`]), so a script
//!   changed after the person's yes asks again; and the grants are the yes's
//!   own, which `task_yes::yes_for_token` gives only when the name, the hash
//!   and the grants all match. The grants cannot be changed apart from the
//!   folder: they are declared in the skill's own `skill.toml`, inside the
//!   hash.
//! - The switch ([`task_yes::ENVIRONMENT`]) is read at each mint: off, no
//!   token.
use crate::protocol::Grant;
use crate::runs::SkillYes;
use crate::task_yes;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Where skills from the Store are on the lamp (lamp-skills' own folder).
pub const STORE_DIR: &str = "/var/lib/lampOS/skills";
/// Where the skills a paired computer pushed are (`lamp_skills::PUSHED_DIR`).
pub const PUSHED_DIR: &str = "/var/lib/lampOS/skills-dev";

/// What a mint is told when no yes was ever said for the skill.
pub const WAITING: &str = "the owner has not said yes to this skill's task yet";
/// What a mint is told when the yes was for other bytes or other grants.
pub const CHANGED: &str = "the skill changed since the owner said yes: ask again";
/// What a mint is told for a skill with no folder on the lamp.
pub const NOT_THERE: &str = "no such skill on this lamp";
/// What a mint is told when the skill is in both folders.
pub const TWO_PLACES: &str = "this skill is in both the Store's folder and the pushed folder";

/// `lamp_skills::package`'s bounds (a skill's folder is at most this big).
const MOST_FILES: usize = 512;
const MOST_BYTES: u64 = 16 * 1024 * 1024;
/// The lamp's own records at the top of a skill's folder, never the skill's
/// (`lamp_skills::task::LAMP_RECORDS`).
const LAMP_RECORDS: [&str; 3] = [".package.json", ".pushed.json", ".revoked.json"];

/// The owner's yes as lamp-control keeps it, held to the folder as it is at
/// the mint.
pub struct FolderYes {
    state: PathBuf,
    store: PathBuf,
    pushed: PathBuf,
    /// Whether the switch is on now (`task_yes::on`): read at each mint.
    switch: fn() -> bool,
}

impl FolderYes {
    /// Yeses in `state` (`--state-dir`), skills in `store` and `pushed`.
    pub fn new(state: PathBuf, store: PathBuf, pushed: PathBuf) -> Self {
        Self {
            state,
            store,
            pushed,
            switch: task_yes::on,
        }
    }

    /// The same, with `switch` in place of the process's environment.
    pub fn with_switch(mut self, switch: fn() -> bool) -> Self {
        self.switch = switch;
        self
    }

    /// The skill's folder: where it is, in exactly one of the two places.
    fn folder(&self, skill: &str) -> Result<PathBuf, String> {
        let plain = (1..=64).contains(&skill.len())
            && skill
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !plain {
            return Err(NOT_THERE.to_owned());
        }
        let is_folder = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
        let (store, pushed) = (self.store.join(skill), self.pushed.join(skill));
        match (is_folder(&store), is_folder(&pushed)) {
            (true, true) => Err(TWO_PLACES.to_owned()),
            (true, false) => Ok(store),
            (false, true) => Ok(pushed),
            (false, false) => Err(NOT_THERE.to_owned()),
        }
    }
}

impl SkillYes for FolderYes {
    fn grants(&self, skill: &str) -> Result<BTreeSet<Grant>, String> {
        if !(self.switch)() {
            return Err(task_yes::OFF.to_owned());
        }
        let folder = self.folder(skill)?;
        let sha256 = folder_sha256(&folder)
            .map_err(|why| format!("the skill's folder can't be read: {why}"))?;
        let Some(yes) = task_yes::yeses(&self.state)
            .into_iter()
            .find(|yes| yes.name == skill)
        else {
            return Err(WAITING.to_owned());
        };
        // The one place a token's grants come from: a yes for exactly this
        // name, this folder's hash now and the grants it named.
        let Some(yes) = task_yes::yes_for_token(&self.state, skill, &sha256, &yes.grants) else {
            return Err(CHANGED.to_owned());
        };
        let mut grants = BTreeSet::new();
        for name in &yes.grants {
            match serde_json::from_value::<Grant>(Value::String(name.clone())) {
                Ok(Grant::Skills) | Err(_) => {
                    return Err(format!(
                        "the owner's yes names {name:?}, which is no grant of a skill's run"
                    ));
                }
                Ok(grant) => {
                    grants.insert(grant);
                }
            }
        }
        Ok(grants)
    }
}

/// Every plain file under `dir`, hidden ones too, by its path in the folder,
/// but the lamp's own records at its top; a link, a name that is not UTF-8 or
/// has a control character, or a folder past a skill's bounds refuses it
/// (`lamp_skills::task::hashed_files`).
fn hashed_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries =
            std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", folder.display()))?;
            let name = entry.file_name();
            let Some(name) = name
                .to_str()
                .filter(|name| !name.chars().any(char::is_control))
            else {
                return Err(format!("{}: not a plain name", entry.path().display()));
            };
            if prefix.is_empty() && LAMP_RECORDS.contains(&name) {
                continue;
            }
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let meta = std::fs::symlink_metadata(entry.path())
                .map_err(|error| format!("{path}: {error}"))?;
            if meta.is_dir() {
                folders.push((entry.path(), path));
                continue;
            }
            if !meta.is_file() {
                return Err(format!("{path}: a link or not a plain file"));
            }
            total += meta.len();
            if total > MOST_BYTES || files.len() >= MOST_FILES {
                return Err(format!(
                    "more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for a skill"
                ));
            }
            let bytes = read_small(&entry.path(), meta.len())?
                .ok_or_else(|| format!("{path}: gone while read"))?;
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

/// A plain file's bytes, never through a link, at most `most`; none when it
/// isn't there.
fn read_small(path: &Path, most: u64) -> Result<Option<Vec<u8>>, String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > most {
        return Err(format!("{}: more than {most} bytes", path.display()));
    }
    Ok(Some(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The SHA-256 of the skill folder `dir` as the yes is tied to it: every
/// file but the lamp's own records, by its path in order, each as a line
/// `SHA256  PATH`, those lines hashed together, lowercase hex. The same
/// definition as `lamp_skills::task::folder_sha256` (held by
/// [`FIXTURE_SHA256`]).
pub fn folder_sha256(dir: &Path) -> Result<String, String> {
    let files = hashed_files(dir)?;
    let mut listed = String::new();
    for (path, bytes) in &files {
        listed.push_str(&hex(&Sha256::digest(bytes)));
        listed.push_str("  ");
        listed.push_str(path);
        listed.push('\n');
    }
    Ok(hex(&Sha256::digest(listed.as_bytes())))
}

/// The hash of the fixture folder the tests of both crates build
/// (`SKILL.md` = "# fixture\n", `skill.toml` = "version = \"1.0.0\"\n",
/// `scripts/run.py` = "print(1)\n", `.hidden` = "x", and the lamp's records
/// `.package.json`, `.pushed.json`, `.revoked.json` = "{}"): one value, so a
/// change to either definition of the hash fails a test here and in
/// lamp-skills.
pub const FIXTURE_SHA256: &str = "4f4b9e83e681e2eb0f28bf27547435dced9b803080f253889ee5458f83f17f16";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_yes::{Asked, record};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lamp-control-folder-yes-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(folder: &Path) {
        std::fs::create_dir_all(folder.join("scripts")).unwrap();
        std::fs::write(folder.join("SKILL.md"), "# fixture\n").unwrap();
        std::fs::write(folder.join("skill.toml"), "version = \"1.0.0\"\n").unwrap();
        std::fs::write(folder.join("scripts/run.py"), "print(1)\n").unwrap();
        std::fs::write(folder.join(".hidden"), "x").unwrap();
        for record in LAMP_RECORDS {
            std::fs::write(folder.join(record), "{}").unwrap();
        }
    }

    #[test]
    fn the_fixture_folder_hashes_as_lamp_skills_hashes_it() {
        let root = scratch("fixture");
        fixture(&root.join("booth"));
        assert_eq!(folder_sha256(&root.join("booth")).unwrap(), FIXTURE_SHA256);
        let _ = std::fs::remove_dir_all(&root);
    }

    struct Lamp {
        root: PathBuf,
        yes: FolderYes,
    }

    impl Lamp {
        fn new(tag: &str) -> Self {
            let root = scratch(tag);
            for sub in ["state", "store", "pushed"] {
                std::fs::create_dir_all(root.join(sub)).unwrap();
            }
            let yes = FolderYes::new(root.join("state"), root.join("store"), root.join("pushed"))
                .with_switch(|| true);
            Self { root, yes }
        }

        fn say_yes(&self, skill: &str, sha256: &str, grants: &[&str]) {
            let asked = Asked {
                name: skill.to_owned(),
                sha256: sha256.to_owned(),
                grants: grants.iter().map(|g| (*g).to_owned()).collect(),
            };
            record(&self.root.join("state"), &asked, 1).unwrap();
        }
    }

    impl Drop for Lamp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_yes_for_the_folder_as_it_is_gives_exactly_its_grants() {
        let lamp = Lamp::new("yes");
        fixture(&lamp.root.join("store/booth"));
        lamp.say_yes("booth", FIXTURE_SHA256, &["light", "camera"]);
        let grants = lamp.yes.grants("booth").unwrap();
        assert_eq!(
            grants,
            [Grant::Light, Grant::Camera]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        // A pushed skill's folder is the pushed folder's.
        fixture(&lamp.root.join("pushed/dev-booth"));
        lamp.say_yes("dev-booth", FIXTURE_SHA256, &["light"]);
        assert_eq!(
            lamp.yes.grants("dev-booth").unwrap(),
            [Grant::Light].into_iter().collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn no_yes_is_no_token() {
        let lamp = Lamp::new("none");
        fixture(&lamp.root.join("store/booth"));
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), WAITING);
        // A yes for another skill is none for this one.
        lamp.say_yes("other", FIXTURE_SHA256, &["light"]);
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), WAITING);
        // A skill with no folder is refused, whatever yes there is.
        lamp.say_yes("ghost", FIXTURE_SHA256, &["light"]);
        assert_eq!(lamp.yes.grants("ghost").unwrap_err(), NOT_THERE);
        for name in ["", "Booth", "../store/booth", &"a".repeat(65)] {
            assert_eq!(lamp.yes.grants(name).unwrap_err(), NOT_THERE, "{name:?}");
        }
    }

    #[test]
    fn a_folder_changed_after_the_yes_asks_again() {
        let lamp = Lamp::new("changed");
        let booth = lamp.root.join("store/booth");
        fixture(&booth);
        lamp.say_yes("booth", FIXTURE_SHA256, &["light"]);
        assert!(lamp.yes.grants("booth").is_ok());
        // A script changed.
        std::fs::write(booth.join("scripts/run.py"), "print(2)\n").unwrap();
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), CHANGED);
        // Back as it was: the yes holds again.
        std::fs::write(booth.join("scripts/run.py"), "print(1)\n").unwrap();
        assert!(lamp.yes.grants("booth").is_ok());
        // A file added, a hidden one included, and the skill's own `skill.toml`
        // (where its grants are declared).
        std::fs::write(booth.join(".more"), "y").unwrap();
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), CHANGED);
        std::fs::remove_file(booth.join(".more")).unwrap();
        std::fs::write(
            booth.join("skill.toml"),
            "version = \"1.0.0\"\ngrants = [\"body\"]\n",
        )
        .unwrap();
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), CHANGED);
        // The lamp's own records are not the skill's: a record changed is no change.
        std::fs::write(booth.join("skill.toml"), "version = \"1.0.0\"\n").unwrap();
        std::fs::write(booth.join(".package.json"), "{\"x\":1}").unwrap();
        assert!(lamp.yes.grants("booth").is_ok());
    }

    #[test]
    fn a_link_two_places_or_the_switch_off_is_no_token() {
        let lamp = Lamp::new("odd");
        let booth = lamp.root.join("store/booth");
        fixture(&booth);
        lamp.say_yes("booth", FIXTURE_SHA256, &["light"]);
        // The same skill in both places is refused.
        fixture(&lamp.root.join("pushed/booth"));
        assert_eq!(lamp.yes.grants("booth").unwrap_err(), TWO_PLACES);
        std::fs::remove_dir_all(lamp.root.join("pushed/booth")).unwrap();
        // A link in the folder refuses the hash (so no yes can hold for it).
        std::os::unix::fs::symlink("/etc/passwd", booth.join("link")).unwrap();
        assert!(lamp.yes.grants("booth").unwrap_err().contains("a link"));
        std::fs::remove_file(booth.join("link")).unwrap();
        assert!(lamp.yes.grants("booth").is_ok());
        // The switch off: no token, whatever was said.
        let off = FolderYes::new(
            lamp.root.join("state"),
            lamp.root.join("store"),
            lamp.root.join("pushed"),
        )
        .with_switch(|| false);
        assert_eq!(off.grants("booth").unwrap_err(), task_yes::OFF);
    }
}
