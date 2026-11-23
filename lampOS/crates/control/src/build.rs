//! The lamp's build, said to a program in the welcome's and the hello's `lamp`
//! (`build`), so a developer can quote it in a bug report: the same
//! `<sequence>+<commit>` the owner's app shows (`lamp-cloud`'s `version`),
//! read from `lamp-update`'s `state.json`, which only root reads. The unit
//! hands it in as the credential `build` (`LoadCredential=`), as
//! `lamp-cloud.service` does, with a blank fallback (`SetCredential=`) so a
//! lamp without that file still starts.
//!
//! Read once at the start: a new build means a new `lamp-control`. No state,
//! a blank or unreadable credential, or a state without an installed build
//! says nothing: the field is left out, never a made-up value.
use std::{fs, io::Read, path::Path};

/// The credential's name in `$CREDENTIALS_DIRECTORY`.
pub const CREDENTIAL: &str = "build";

/// The most of the state taken: well past any real one, so a truncated file
/// (which does not parse) is the only way it can be too big.
const FILE_MOST: u64 = 4 * 1024 * 1024;

/// The build `state` (the text of `state.json`) names: `<sequence>+<commit>`,
/// the commit's first twelve; none when it names no installed build.
pub fn of(state: &str) -> Option<String> {
    let state: serde_json::Value = serde_json::from_str(state).ok()?;
    let installed = &state["installed"];
    let sequence = installed["sequence"].as_u64()?;
    let commit = installed["commit"].as_str()?;
    let first = commit.get(..12).unwrap_or(commit);
    let text = first.bytes().all(|b| b.is_ascii_hexdigit()) && !first.is_empty();
    text.then(|| format!("{sequence}+{first}"))
}

/// The build in the credential `name` of the folder `credentials`
/// (`$CREDENTIALS_DIRECTORY`), if there is one.
pub fn read(credentials: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(credentials.join(CREDENTIAL))
        .ok()?
        .take(FILE_MOST)
        .read_to_string(&mut text)
        .ok()?;
    of(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installed_build_is_its_sequence_and_the_first_twelve_of_its_commit() {
        let state =
            r#"{"installed":{"sequence":143,"commit":"ab12cd34ef56789012345678"},"other":1}"#;
        assert_eq!(of(state).as_deref(), Some("143+ab12cd34ef56"));
        let short = r#"{"installed":{"sequence":7,"commit":"abc1234"}}"#;
        assert_eq!(of(short).as_deref(), Some("7+abc1234"));
    }

    #[test]
    fn no_state_or_no_installed_build_says_nothing() {
        for state in [
            "",
            "\n",
            "not json",
            "{}",
            r#"{"installed":null}"#,
            r#"{"installed":{"sequence":1}}"#,
            r#"{"installed":{"commit":"abc1234"}}"#,
            r#"{"installed":{"sequence":"1","commit":"abc1234"}}"#,
            r#"{"installed":{"sequence":1,"commit":""}}"#,
            r#"{"installed":{"sequence":1,"commit":"not a hash"}}"#,
        ] {
            assert_eq!(of(state), None, "{state:?}");
        }
    }

    #[test]
    fn the_credential_is_read_from_its_folder_and_a_missing_or_blank_one_says_nothing() {
        let folder = std::env::temp_dir().join(format!("lampos-build-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(read(&folder), None, "no file");
        fs::write(folder.join(CREDENTIAL), "\n").unwrap();
        assert_eq!(read(&folder), None, "the unit's blank fallback");
        fs::write(
            folder.join(CREDENTIAL),
            r#"{"installed":{"sequence":9,"commit":"0123456789abcdef"}}"#,
        )
        .unwrap();
        assert_eq!(read(&folder).as_deref(), Some("9+0123456789ab"));
        assert_eq!(read(&folder.join("nowhere")), None);
        let _ = fs::remove_dir_all(&folder);
    }
}
