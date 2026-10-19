//! Whether the voice can start on the second route (#1712, the first
//! builder at 21:08): with `LAMP_VOICE_ROUTE=second` in the voice's unit,
//! the files it needs are there. That's the second route's configuration
//! (`LAMP_VOICE_SECOND_CONFIG`), every key file it names, and every
//! credential the unit loads (`LoadCredential=`). Without one, the voice does
//! not start and never falls back to Gemini Live, so this part names what is
//! missing: a lamp that cannot start its voice says why.
//!
//! A key is never read: a key file is only looked at, as there, a regular
//! file and not empty. The configuration is read for its key paths when it
//! can be. Run as `lampos` (the timer's), root's configuration is not
//! readable, and then the key it names is taken as the product's, the relay
//! key install places ([`RELAY_KEY`]).
use crate::Check;
use lamp_msgs::{PartResult, Verdict};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Instant,
};

/// The part's name in the report.
pub const PART: &str = "voice_route";
/// The voice's unit as install writes it, and its drop-ins.
pub const UNIT: &str = "/etc/systemd/system/lamp-voice.service";
pub const DROP_INS: &str = "/etc/systemd/system/lamp-voice.service.d";
/// The key the product's configuration names for every service: the relay
/// key `provider.json` names too (`lamp-update`'s `CREDENTIAL_PATH`).
pub const RELAY_KEY: &str = "/etc/lampOS/voice/credential";
/// The most of a configuration read: the voice's own bound
/// (`second_config::load_from_path`).
const MOST_CONFIG_BYTES: u64 = 16_384;

/// What the voice's unit and its drop-ins say, in systemd's order: the
/// route, the second configuration's path and the credentials it loads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    pub route: Option<String>,
    pub second: Option<String>,
    /// `(id, path)` of each `LoadCredential=`.
    pub credentials: Vec<(String, String)>,
}

impl Unit {
    /// The unit's text, then each drop-in's, in the order systemd reads
    /// them: a later assignment goes over an earlier, and an empty
    /// `LoadCredential=` clears those before it.
    pub fn read(texts: &[String]) -> Self {
        let mut unit = Self::default();
        for text in texts {
            for line in text.lines().map(str::trim) {
                if let Some(assignments) = line.strip_prefix("Environment=") {
                    for (name, value) in environment(assignments) {
                        match name.as_str() {
                            "LAMP_VOICE_ROUTE" => unit.route = Some(value),
                            "LAMP_VOICE_SECOND_CONFIG" => unit.second = Some(value),
                            _ => {}
                        }
                    }
                } else if let Some(credential) = line.strip_prefix("LoadCredential=") {
                    match credential.split_once(':') {
                        Some((id, path)) => unit.credentials.push((id.into(), path.into())),
                        None if credential.is_empty() => unit.credentials.clear(),
                        None => {}
                    }
                }
            }
        }
        unit
    }

    /// Whether the voice is the second route, as `lamp-voice` takes it:
    /// exactly `second`.
    pub fn second_route(&self) -> bool {
        self.route.as_deref() == Some("second")
    }
}

/// An `Environment=` line's assignments: `NAME=value`, separated by spaces,
/// each perhaps in double quotes, as the units here write them.
fn environment(assignments: &str) -> Vec<(String, String)> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for c in assignments.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .into_iter()
        .filter_map(|word| {
            word.split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// How a file looked, never its contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Looked {
    Ok,
    Missing,
    Empty,
    /// Not a regular file (a folder, say).
    NotAFile,
    /// It could not be looked at (its folder is closed to this user).
    Closed,
}

/// How the file at `path` looks.
pub fn look(path: &Path) -> Looked {
    match fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Looked::NotAFile,
        Ok(meta) if meta.len() == 0 => Looked::Empty,
        Ok(_) => Looked::Ok,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Looked::Missing,
        Err(_) => Looked::Closed,
    }
}

/// The key paths a configuration names (each `*_credential_file`), from
/// its text; `None` when it is not a JSON object.
pub fn named_keys(config: &str) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(config).ok()?;
    let mut keys: Vec<String> = value
        .as_object()?
        .iter()
        .filter(|(field, _)| field.ends_with("_credential_file"))
        .filter_map(|(_, path)| path.as_str().filter(|path| !path.is_empty()))
        .map(str::to_owned)
        .collect();
    keys.sort();
    keys.dedup();
    Some(keys)
}

/// The configuration's text, or why it was not read: `Err(true)` when it
/// was closed to this user, `Err(false)` when it could not be read at all.
fn read_config(path: &Path) -> Result<String, bool> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > MOST_CONFIG_BYTES => return Err(false),
        Ok(_) => {}
        Err(error) => return Err(error.kind() == io::ErrorKind::PermissionDenied),
    }
    fs::read_to_string(path).map_err(|error| error.kind() == io::ErrorKind::PermissionDenied)
}

/// What is wrong with the file at `path`, called `what`, as it `looked`.
fn fault(path: &str, what: &str, looked: Looked) -> Option<String> {
    let wrong = match looked {
        Looked::Ok => return None,
        Looked::Missing => "is missing",
        Looked::Empty => "is empty",
        Looked::NotAFile => "is not a file",
        Looked::Closed => "cannot be looked at",
    };
    Some(format!("{what} {path} {wrong}"))
}

/// The part's verdict for `unit`, looking at files with `look` and reading
/// the configuration with `read` (both given, for the tests).
pub fn judge(
    unit: Option<&Unit>,
    look: impl Fn(&Path) -> Looked,
    read: impl Fn(&Path) -> Result<String, bool>,
) -> PartResult {
    let Some(unit) = unit else {
        return PartResult::new(
            PART,
            Verdict::NotChecked,
            &format!("the voice's unit cannot be read at {UNIT}"),
        );
    };
    if !unit.second_route() {
        return PartResult::new(
            PART,
            Verdict::NotFitted,
            "the voice is Gemini Live here (LAMP_VOICE_ROUTE is not second)",
        );
    }
    let Some(config) = unit.second.as_deref().filter(|path| !path.is_empty()) else {
        return PartResult::new(
            PART,
            Verdict::Fail,
            "LAMP_VOICE_ROUTE=second without LAMP_VOICE_SECOND_CONFIG: the voice does not start",
        );
    };
    let config_path = Path::new(config);
    let mut wrong: Vec<String> = Vec::new();
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut note = String::new();
    let config_fault = fault(
        config,
        "the second route's configuration",
        look(config_path),
    );
    let config_there = config_fault.is_none();
    wrong.extend(config_fault);
    if config_there {
        match read(config_path) {
            Ok(text) => match named_keys(&text) {
                Some(named) => keys.extend(named.into_iter().map(|path| (path, "its key".into()))),
                None => wrong.push(format!(
                    "the second route's configuration {config} is not one the voice reads"
                )),
            },
            Err(true) => {
                note = format!(
                    " (the configuration is root's: its key taken as the relay key, {RELAY_KEY})"
                );
                keys.push((RELAY_KEY.into(), "its key".into()));
            }
            Err(false) => wrong.push(format!(
                "the second route's configuration {config} cannot be read"
            )),
        }
    }
    for (id, path) in &unit.credentials {
        keys.push((path.clone(), format!("the unit's credential {id}")));
    }
    let checked = keys.len();
    for (path, what) in &keys {
        wrong.extend(fault(path, what, look(Path::new(path))));
    }
    if wrong.is_empty() {
        PartResult::new(
            PART,
            Verdict::Pass,
            &format!(
                "the second route's configuration and its {checked} key file{} are there{note}",
                if checked == 1 { "" } else { "s" }
            ),
        )
    } else {
        PartResult::new(
            PART,
            Verdict::Fail,
            &format!(
                "{}: the voice does not start on the second route{note}",
                wrong.join("; ")
            ),
        )
    }
}

/// The check, reading the voice's unit at `unit` and its drop-ins in
/// `drop_ins`.
pub struct VoiceRoute {
    pub unit: PathBuf,
    pub drop_ins: PathBuf,
}

impl VoiceRoute {
    fn unit(&self) -> Option<Unit> {
        let mut texts = vec![fs::read_to_string(&self.unit).ok()?];
        let mut drop_ins: Vec<PathBuf> = fs::read_dir(&self.drop_ins)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| path.extension().is_some_and(|ext| ext == "conf"))
                    .collect()
            })
            .unwrap_or_default();
        drop_ins.sort();
        texts.extend(
            drop_ins
                .iter()
                .filter_map(|path| fs::read_to_string(path).ok()),
        );
        Some(Unit::read(&texts))
    }
}

impl Check for VoiceRoute {
    fn part(&self) -> &str {
        PART
    }

    fn run(&mut self, _deadline: Instant) -> PartResult {
        judge(self.unit().as_ref(), look, read_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn unit(text: &str) -> Unit {
        Unit::read(&[text.to_owned()])
    }

    const PRODUCT: &str = "Environment=LAMP_BUS=on\nEnvironment=LAMP_VOICE_ROUTE=second\nEnvironment=LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json\n";

    /// Files as `look` and `read` see them: the configuration's text, or
    /// closed, and the rest by how they look.
    fn judged(unit: &Unit, files: &[(&str, Looked)], config: Result<&str, bool>) -> PartResult {
        let files: HashMap<String, Looked> = files
            .iter()
            .map(|(path, looked)| ((*path).into(), *looked))
            .collect();
        judge(
            Some(unit),
            |path| {
                *files
                    .get(path.to_str().unwrap())
                    .unwrap_or(&Looked::Missing)
            },
            |_| config.map(str::to_owned),
        )
    }

    #[test]
    fn the_units_route_configuration_and_credentials_are_read_in_systemds_order() {
        let read = Unit::read(&[
            format!("{PRODUCT}LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key\n"),
            "[Service]\nEnvironment=\"LAMP_SECOND_KEY_TERMS=Hanoi,Da Nang\" LAMP_VOICE_ROUTE=gemini\nLoadCredential=\nLoadCredential=gemini.key:/k\n".into(),
        ]);
        assert_eq!(read.route.as_deref(), Some("gemini"));
        assert_eq!(
            read.second.as_deref(),
            Some("/etc/lampOS/voice/second.json")
        );
        assert_eq!(read.credentials, [("gemini.key".into(), "/k".into())]);
        assert!(!read.second_route());
        assert!(unit(PRODUCT).second_route());
    }

    #[test]
    fn a_product_lamp_with_its_two_files_passes_and_names_each_one_missing() {
        let product = unit(PRODUCT);
        let config = r#"{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"/etc/lampOS/voice/credential"}"#;
        let both = [
            ("/etc/lampOS/voice/second.json", Looked::Ok),
            (RELAY_KEY, Looked::Ok),
        ];
        let passed = judged(&product, &both, Ok(config));
        assert_eq!(passed.verdict, Verdict::Pass, "{passed:?}");
        // No configuration: said, and the voice does not start.
        let none = judged(&product, &[(RELAY_KEY, Looked::Ok)], Ok(config));
        assert_eq!(none.verdict, Verdict::Fail);
        assert!(
            none.reason.contains(
                "the second route's configuration /etc/lampOS/voice/second.json is missing"
            ),
            "{none:?}"
        );
        // No key: named.
        let keyless = judged(
            &product,
            &[
                ("/etc/lampOS/voice/second.json", Looked::Ok),
                (RELAY_KEY, Looked::Empty),
            ],
            Ok(config),
        );
        assert_eq!(keyless.verdict, Verdict::Fail);
        assert!(
            keyless
                .reason
                .contains("its key /etc/lampOS/voice/credential is empty")
        );
    }

    #[test]
    fn run_as_lampos_the_relay_key_is_looked_at_and_the_units_credentials_too() {
        let with = Unit::read(&[format!(
            "{PRODUCT}LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key\n"
        )]);
        let result = judged(
            &with,
            &[
                ("/etc/lampOS/voice/second.json", Looked::Ok),
                (RELAY_KEY, Looked::Ok),
            ],
            Err(true),
        );
        assert_eq!(result.verdict, Verdict::Fail);
        assert!(result.reason.contains(
            "the unit's credential deepseek.key /etc/lampOS/voice/deepseek.key is missing"
        ));
        assert!(result.reason.contains("the configuration is root's"));
    }

    #[test]
    fn live_is_not_fitted_and_a_second_route_without_its_configuration_fails() {
        let live = judged(&unit("Environment=LAMP_BUS=on\n"), &[], Ok("{}"));
        assert_eq!(live.verdict, Verdict::NotFitted);
        let bare = judged(
            &unit("Environment=LAMP_VOICE_ROUTE=second\n"),
            &[],
            Ok("{}"),
        );
        assert_eq!(bare.verdict, Verdict::Fail);
        assert!(bare.reason.contains("without LAMP_VOICE_SECOND_CONFIG"));
        let unread = judge(None, |_| Looked::Ok, |_| Ok(String::new()));
        assert_eq!(unread.verdict, Verdict::NotChecked);
        // Not the voice's configuration.
        let odd = judged(
            &unit(PRODUCT),
            &[("/etc/lampOS/voice/second.json", Looked::Ok)],
            Ok("[1, 2]"),
        );
        assert!(odd.reason.contains("is not one the voice reads"), "{odd:?}");
    }

    /// A key is only looked at, never read: the real look on files.
    #[test]
    fn a_file_is_looked_at_without_its_contents() {
        let directory =
            std::env::temp_dir().join(format!("lamp-selftest-route-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let key = directory.join("credential");
        fs::write(&key, "secret").unwrap();
        let empty = directory.join("empty");
        fs::write(&empty, "").unwrap();
        assert_eq!(look(&key), Looked::Ok);
        assert_eq!(look(&empty), Looked::Empty);
        assert_eq!(look(&directory), Looked::NotAFile);
        assert_eq!(look(&directory.join("gone")), Looked::Missing);
        assert_eq!(
            named_keys(
                r#"{"llm_credential_file":"/a","stt_credential_file":"/a","tts_credential_file":"","llm_model":"m"}"#
            ),
            Some(vec!["/a".to_owned()])
        );
        fs::remove_dir_all(&directory).unwrap();
    }
}
