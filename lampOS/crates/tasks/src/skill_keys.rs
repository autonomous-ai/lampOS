//! A skill's keys for its run (`LAMP_SKILL_SECRETS`, off unless `on`, on
//! in the product; the first builder's ruling, 12 Nov 19:44 on #6059: a file's
//! secrets "go in through the app, into root's store like service tokens,
//! and reach the run only as an environment file read once").
//!
//! - **Asked of root's lamp-services** ([`ServicesKeys`]): its tools socket
//!   ([`SOCKET`]), which answers lamp-tasks.service alone,
//!   `{"op":"run_secrets","skill"}` answered with the set values of the
//!   names the skill's `skill.toml` needs and the names missing
//!   (`lamp_services::skill_secrets`). lamp-tasks never keeps them.
//! - **A name missing, no run** ([`MISSING`]): the run is refused before
//!   its token is minted, in plain words naming what to add in the app.
//! - **The environment file** ([`write_env_file`]): `.run-env` in the run's
//!   own fresh folder, 0400, lamp-tasks' user's (the run's), one
//!   `export NAME='value'` a line. The run is started through
//!   [`LAUNCH`]: `/bin/sh` reads it once, deletes it, and only then starts
//!   the lamp's Hermes, so the values are in its environment alone. The
//!   folder goes when the run ends, however it ends.
//! - **Never elsewhere**: not in the run's token, its arguments, its words,
//!   its transcript, the skill's folder or a journal line ([`RunKeys`]'s
//!   `Debug` says the names alone).
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The switch, `on` exactly, as lamp-services, lamp-cloud, lamp-voice and
/// lamp-skills read it.
pub const SWITCH: &str = "LAMP_SKILL_SECRETS";
/// lamp-services' tools socket, where lamp-tasks.service alone may ask a
/// run's keys (`lamp_services::skill_secrets::RUN_ASKER_CGROUP`).
pub const SOCKET: &str = "/run/lampos-services/tools/call.sock";
/// The ask's `op` (`lamp_services::skill_secrets::RUN_OP`).
pub const RUN_OP: &str = "run_secrets";
/// How long the ask may take, each way: a small file read.
pub const WAIT: Duration = Duration::from_secs(2);
/// The most bytes of the answer read: 16 values of 4 KiB, escaped.
const MOST_ANSWER_BYTES: u64 = 512 * 1024;
/// The environment file's name in the run's own folder.
pub const ENV_FILE: &str = ".run-env";
/// The most names a skill needs (`lamp_services::skill_secrets::MOST_NAMES`).
pub const MOST_NAMES: usize = 16;
/// The most bytes of one value (`lamp_services::skill_secrets::MOST_VALUE_BYTES`).
pub const MOST_VALUE_BYTES: usize = 4_096;
/// What the shell the run is started through does with its first
/// argument, the environment file: read it once, delete it whatever came
/// of the read, and start the rest only if it was read. The values are
/// never in an argument.
pub const LAUNCH: &str = "f=$1; shift; s=1; [ -f \"$f\" ] && . \"$f\" && s=0; rm -f -- \"$f\"; \
                          [ \"$s\" = 0 ] || exit 97; exec \"$@\"";
/// The shell [`LAUNCH`] runs in.
pub const SHELL: &str = "/bin/sh";

/// Whether [`SWITCH`]'s value is on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A run's keys: the set values, by name, and the names missing. Its
/// `Debug` says the names alone.
#[derive(Default)]
pub struct RunKeys {
    pub values: Vec<(String, String)>,
    pub missing: Vec<String>,
}

impl std::fmt::Debug for RunKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.values.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("RunKeys")
            .field("set", &names)
            .field("missing", &self.missing)
            .finish()
    }
}

/// Where a run's keys come from.
pub trait Keys {
    /// `skill`'s keys, or why they couldn't be asked.
    fn of(&self, skill: &str) -> Result<RunKeys, String>;
}

/// lamp-services' tools socket ([`SOCKET`]).
pub struct ServicesKeys {
    pub socket: PathBuf,
    pub wait: Duration,
}

impl Default for ServicesKeys {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(SOCKET),
            wait: WAIT,
        }
    }
}

/// Whether `name` is a plain variable's name as lamp-services keeps one:
/// `[A-Z][A-Z0-9_]{2,63}` (its reserved ones are refused there).
fn name_plain(name: &str) -> bool {
    (3..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// lamp-services' answer read as a run's keys: each name plain, each value
/// one line of at most [`MOST_VALUE_BYTES`], at most [`MOST_NAMES`] of them.
pub fn keys_of(answer: &Value) -> Result<RunKeys, String> {
    if answer["ok"] != true {
        return Err(answer["error"]
            .as_str()
            .unwrap_or("lamp-services refused the run's keys")
            .chars()
            .take(200)
            .collect());
    }
    let unread = || "lamp-services' answer for the run's keys is not one lamp-tasks reads";
    let mut keys = RunKeys::default();
    for (name, value) in answer["values"].as_object().ok_or_else(unread)? {
        let value = value.as_str().ok_or_else(unread)?;
        if !name_plain(name)
            || value.is_empty()
            || value.len() > MOST_VALUE_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(unread().into());
        }
        keys.values.push((name.clone(), value.to_owned()));
    }
    for name in answer["missing"].as_array().ok_or_else(unread)? {
        let name = name
            .as_str()
            .filter(|name| name_plain(name))
            .ok_or_else(unread)?;
        keys.missing.push(name.to_owned());
    }
    if keys.values.len() + keys.missing.len() > MOST_NAMES {
        return Err(unread().into());
    }
    Ok(keys)
}

impl Keys for ServicesKeys {
    fn of(&self, skill: &str) -> Result<RunKeys, String> {
        let unreachable =
            |error: std::io::Error| format!("lamp-services is not reachable: {error}");
        let mut stream = UnixStream::connect(&self.socket).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(self.wait))
            .map_err(unreachable)?;
        stream
            .set_write_timeout(Some(self.wait))
            .map_err(unreachable)?;
        let mut line = json!({"op": RUN_OP, "skill": skill}).to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(unreachable)?;
        let mut answer = String::new();
        BufReader::new(std::io::Read::take(stream, MOST_ANSWER_BYTES))
            .read_line(&mut answer)
            .map_err(unreachable)?;
        let answer: Value = serde_json::from_str(answer.trim())
            .map_err(|_| "lamp-services' answer is not JSON".to_owned())?;
        keys_of(&answer)
    }
}

/// What a run missing a key is refused with: which names to add in the
/// app, plainly, never a value.
pub fn missing_line(skill: &str, missing: &[String]) -> String {
    format!(
        "{} didn't run: it needs {} added in the app, on the skill's page",
        skill.replace('-', " "),
        missing.join(" and ")
    )
}

/// Whether the skill's `skill.toml` in `folder` names any `secrets`, by a
/// line of its own: when lamp-services can't be asked, a skill that names
/// none still runs, and one that does is refused.
pub fn names_secrets(folder: &Path) -> bool {
    std::fs::read_to_string(folder.join("skill.toml")).is_ok_and(|text| {
        text.lines().any(|line| {
            line.trim_start()
                .strip_prefix("secrets")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
    })
}

/// `value` as one POSIX shell word: in single quotes, each `'` closed,
/// escaped and opened again, so nothing in it is read by the shell.
pub fn shell_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The environment file's text: `export NAME='value'`, a line each.
pub fn env_text(values: &[(String, String)]) -> String {
    values
        .iter()
        .map(|(name, value)| format!("export {name}={}\n", shell_quoted(value)))
        .collect()
}

/// The environment file in the run's own `folder`: made new (never over
/// another or through a link), 0400 from its first byte, its user
/// lamp-tasks' own, the run's.
pub fn write_env_file(folder: &Path, values: &[(String, String)]) -> std::io::Result<PathBuf> {
    let path = folder.join(ENV_FILE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&path)?;
    let written = file
        .write_all(env_text(values).as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(error) = written {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn the_answer_is_read_whole_or_not_at_all() {
        let keys = keys_of(&json!({"ok": true,
            "values": {"GEMINI_API_KEY": "test-value-123"},
            "missing": ["TELEGRAM_BOT_TOKEN"]}))
        .unwrap();
        assert_eq!(
            keys.values,
            [("GEMINI_API_KEY".to_owned(), "test-value-123".to_owned())]
        );
        assert_eq!(keys.missing, ["TELEGRAM_BOT_TOKEN"]);
        assert!(!format!("{keys:?}").contains("test-value-123"));
        for bad in [
            json!({"ok": true, "values": {"lower": "x"}, "missing": []}),
            json!({"ok": true, "values": {"GOOD_KEY": "a\nb"}, "missing": []}),
            json!({"ok": true, "values": {"GOOD_KEY": ""}, "missing": []}),
            json!({"ok": true, "values": {}, "missing": ["a b"]}),
            json!({"ok": true, "values": []}),
        ] {
            assert!(keys_of(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            keys_of(&json!({"ok": false, "error": "no skill of that name is on this lamp"}))
                .unwrap_err(),
            "no skill of that name is on this lamp"
        );
    }

    #[test]
    fn the_file_is_new_read_only_and_quoted_for_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let values = [
            ("GEMINI_API_KEY".to_owned(), "test-value-123".to_owned()),
            (
                "ODD_TOKEN".to_owned(),
                "it's $(touch /tmp/x) `id`".to_owned(),
            ),
        ];
        let path = write_env_file(dir.path(), &values).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o400);
        assert_eq!(meta.uid(), rustix::process::geteuid().as_raw());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "export GEMINI_API_KEY='test-value-123'\n\
             export ODD_TOKEN='it'\\''s $(touch /tmp/x) `id`'\n"
        );
        // Never over another, nor through a link.
        assert!(write_env_file(dir.path(), &values).is_err());
    }

    /// The shell reads the file once, deletes it before the program
    /// starts, and the program has the values in its environment alone,
    /// never in its arguments.
    #[test]
    fn the_launch_reads_the_file_deletes_it_and_starts_the_program_with_them() {
        let dir = tempfile::tempdir().unwrap();
        let values = [
            ("GEMINI_API_KEY".to_owned(), "test-value-123".to_owned()),
            ("ODD_TOKEN".to_owned(), "it's $(echo no)".to_owned()),
        ];
        let path = write_env_file(dir.path(), &values).unwrap();
        let out = std::process::Command::new(SHELL)
            .args(["-c", LAUNCH, "lamp-run"])
            .arg(&path)
            .args([
                "/bin/sh",
                "-c",
                "[ -e \"$1\" ] && echo left; echo \"$GEMINI_API_KEY|$ODD_TOKEN|$#\"",
                "x",
            ])
            .arg(&path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "test-value-123|it's $(echo no)|1\n"
        );
        assert!(!path.exists());
        // No file: nothing starts.
        let out = std::process::Command::new(SHELL)
            .args(["-c", LAUNCH, "lamp-run"])
            .arg(dir.path().join(ENV_FILE))
            .args(["/bin/echo", "started"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(97));
        assert!(out.stdout.is_empty());
    }

    #[test]
    fn a_missing_key_is_said_by_name_and_a_toml_naming_secrets_is_seen() {
        assert_eq!(
            missing_line("photo-booth", &["TELEGRAM_BOT_TOKEN".to_owned()]),
            "photo booth didn't run: it needs TELEGRAM_BOT_TOKEN added in the app, on the skill's page"
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("skill.toml"), "tool = \"x\"\n").unwrap();
        assert!(!names_secrets(dir.path()));
        std::fs::write(
            dir.path().join("skill.toml"),
            "tool = \"x\"\nsecrets = [\"GOOD_KEY\"]\n",
        )
        .unwrap();
        assert!(names_secrets(dir.path()));
    }
}
