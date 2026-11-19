//! Whether an installed app uses an op: its verified manifest
//! (`/opt/lampOS/apps/<name>/current/lamp-skill.json`, written by the apps'
//! root helper from the package the Store signed) names it in `npu`
//! (`lamp_skills::app_package::NPU_OPS`). A lamp with no such app fetches
//! nothing until an op is asked.
use std::{fs, io::Read, path::Path};

/// The most apps looked at.
pub const MOST_APPS: usize = 64;
/// The largest manifest read.
pub const MOST_MANIFEST_BYTES: u64 = 64 * 1024;

/// Whether an app under `root` names `op` in its manifest's `npu`.
pub fn any_uses(root: &Path, op: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().take(MOST_APPS).any(|entry| {
        let manifest = entry.path().join("current").join("lamp-skill.json");
        let mut bytes = Vec::new();
        let read = fs::File::open(&manifest)
            .and_then(|file| file.take(MOST_MANIFEST_BYTES).read_to_end(&mut bytes));
        read.is_ok()
            && serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|value| {
                value["npu"]
                    .as_array()
                    .is_some_and(|ops| ops.iter().any(|named| named == op))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_app_naming_the_op_counts() {
        let root = tempfile::tempdir().unwrap();
        assert!(!any_uses(&root.path().join("none"), "pose"));
        let app = |name: &str, manifest: &str| {
            let dir = root.path().join(name).join("current");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("lamp-skill.json"), manifest).unwrap();
        };
        app("clock", r#"{"name":"clock","grants":["light"]}"#);
        app("broken", "not json");
        assert!(!any_uses(root.path(), "pose"));
        app(
            "coach",
            r#"{"name":"coach","grants":["camera"],"npu":["pose"]}"#,
        );
        assert!(any_uses(root.path(), "pose"));
        assert!(!any_uses(root.path(), "faces"));
    }
}
