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
//!
//! A file that cannot be looked at at all, as on a lamp whose
//! `/etc/lampOS/voice` is root's alone (lamp-06c1, 21 Oct), is not seen,
//! not missing: the voice runs as root and may well have it. With nothing
//! else wrong the part is not checked, never failed, so the lamp's health
//! is not made `error` (and "Not feeling well" does not droop) for what
//! this user cannot see.
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
    /// Each `ConfigurationDirectory=`, under `/etc`: systemd makes it before
    /// it loads credentials, so a credential's folder named here need not
    /// be there yet (the own keys', in `lamp-voice.service`).
    pub configuration_directories: Vec<String>,
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
                } else if let Some(folders) = line.strip_prefix("ConfigurationDirectory=") {
                    if folders.is_empty() {
                        unit.configuration_directories.clear();
                    }
                    unit.configuration_directories
                        .extend(folders.split_whitespace().map(str::to_owned));
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

    /// Whether systemd makes the folder `path` at the unit's start, as one of
    /// its `ConfigurationDirectory=`.
    pub fn made_at_start(&self, path: &str) -> bool {
        path.strip_prefix("/etc/").is_some_and(|under| {
            let under = under.trim_end_matches('/');
            self.configuration_directories
                .iter()
                .any(|folder| folder.trim_matches('/') == under)
        })
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
    /// A folder: never a key, but a unit's credential may be one, which
    /// systemd loads file by file (the own keys', #2094).
    Folder,
    /// Not a regular file nor a folder.
    NotAFile,
    /// It could not be looked at (its folder is closed to this user).
    Closed,
}

/// How the file at `path` looks.
pub fn look(path: &Path) -> Looked {
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Looked::Folder,
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
        Looked::Folder | Looked::NotAFile => "is not a file",
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
    // Each file that could not be looked at as this user, said apart.
    let mut unseen: Vec<String> = Vec::new();
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut note = String::new();
    let config_looked = look(config_path);
    let config_fault = fault(config, "the second route's configuration", config_looked);
    let config_there = config_fault.is_none();
    if config_looked == Looked::Closed {
        unseen.extend(config_fault);
        // Its key is the relay key the product names, as when it is
        // root's to read.
        keys.push((RELAY_KEY.into(), "its key".into()));
    } else {
        wrong.extend(config_fault);
    }
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
    let named = keys.len();
    for (id, path) in &unit.credentials {
        keys.push((path.clone(), format!("the unit's credential {id}")));
    }
    let checked = keys.len();
    for (at, (path, what)) in keys.iter().enumerate() {
        let looked = look(Path::new(path));
        // A credential's folder is loaded file by file, an empty one as
        // none: the person's own keys (#2094), there or not. One the unit's
        // `ConfigurationDirectory=` names systemd makes at start, so not
        // there yet is none either; the voice then uses the relay's key.
        if at >= named
            && (looked == Looked::Folder || (looked == Looked::Missing && unit.made_at_start(path)))
        {
            continue;
        }
        let found = fault(path, what, looked);
        if looked == Looked::Closed {
            unseen.extend(found);
        } else {
            wrong.extend(found);
        }
    }
    if wrong.is_empty() && !unseen.is_empty() {
        PartResult::new(
            PART,
            Verdict::NotChecked,
            &format!(
                "{}: not checked as this user (the voice, root, may have them){note}",
                unseen.join("; ")
            ),
        )
    } else if wrong.is_empty() {
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
                wrong
                    .iter()
                    .chain(&unseen)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
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

    /// The own keys' lines of the product's voice unit (#2094, #2846), as a
    /// drop-in; `the_product_unit_has_the_own_keys_lines` holds them in it.
    const OWN_KEYS: &str = "[Service]\nEnvironment=LAMP_OWN_KEYS=on\nConfigurationDirectory=lampOS/voice/own\nConfigurationDirectoryMode=0700\nLoadCredential=own:/etc/lampOS/voice/own\n";

    #[test]
    fn the_product_unit_has_the_own_keys_lines() {
        let shipped = include_str!("../../../deploy/product/lamp-voice.service");
        for line in OWN_KEYS.lines().skip(1) {
            assert!(
                shipped.lines().any(|kept| kept == line),
                "lamp-voice.service lacks {line}"
            );
        }
    }

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

    /// #2094: the own keys' drop-in loads a folder, which passes, empty or
    /// not; a key a configuration names that is a folder still fails.
    #[test]
    fn a_credentials_folder_passes_and_a_key_that_is_a_folder_does_not() {
        let with = Unit::read(&[PRODUCT.to_owned(), OWN_KEYS.to_owned()]);
        assert_eq!(
            with.credentials,
            [("own".into(), "/etc/lampOS/voice/own".into())]
        );
        let config = r#"{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"/etc/lampOS/voice/credential"}"#;
        let files = [
            ("/etc/lampOS/voice/second.json", Looked::Ok),
            (RELAY_KEY, Looked::Ok),
            ("/etc/lampOS/voice/own", Looked::Folder),
        ];
        let passed = judged(&with, &files, Ok(config));
        assert_eq!(passed.verdict, Verdict::Pass, "{passed:?}");
        let folder_key = judged(
            &with,
            &[
                ("/etc/lampOS/voice/second.json", Looked::Ok),
                (RELAY_KEY, Looked::Folder),
                ("/etc/lampOS/voice/own", Looked::Folder),
            ],
            Ok(config),
        );
        assert_eq!(folder_key.verdict, Verdict::Fail, "{folder_key:?}");
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

    /// lamp-06c1, 21 Oct: `/etc/lampOS/voice` root's alone (0700), so run
    /// as `lampos` nothing in it can be looked at, while the voice, root,
    /// runs the second route. Not seen is not missing: not checked, never
    /// a fail. A key the self-test can look at that is wrong still fails.
    #[test]
    fn a_closed_voice_folder_is_not_checked_and_a_missing_key_outside_it_still_fails() {
        let with = Unit::read(&[PRODUCT.to_owned(), OWN_KEYS.to_owned()]);
        let closed = [
            ("/etc/lampOS/voice/second.json", Looked::Closed),
            (RELAY_KEY, Looked::Closed),
            ("/etc/lampOS/voice/own", Looked::Closed),
        ];
        let result = judged(&with, &closed, Err(true));
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(
            result
                .reason
                .contains("/etc/lampOS/voice/second.json cannot be looked at"),
            "{result:?}"
        );
        let elsewhere = Unit::read(&[format!(
            "{PRODUCT}LoadCredential=deepseek.key:/etc/keys/deepseek.key\n"
        )]);
        let missing = judged(
            &elsewhere,
            &[("/etc/lampOS/voice/second.json", Looked::Closed)],
            Err(true),
        );
        assert_eq!(missing.verdict, Verdict::Fail, "{missing:?}");
        assert!(
            missing
                .reason
                .contains("the unit's credential deepseek.key /etc/keys/deepseek.key is missing"),
            "{missing:?}"
        );
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

    /// A voice unit at `root`: the product's, its second configuration and
    /// relay key there (each a regular file, never a real key), and the own
    /// keys' lines of the product's unit as a drop-in, pointed at `own`.
    fn lamp(root: &Path, own: &Path) -> VoiceRoute {
        let _ = fs::remove_dir_all(root);
        let drop_ins = root.join("lamp-voice.service.d");
        fs::create_dir_all(&drop_ins).unwrap();
        let config = root.join("second.json");
        let key = root.join("credential");
        fs::write(
            &config,
            format!(
                r#"{{"llm_model":"m","llm_credential_file":"{}"}}"#,
                key.display()
            ),
        )
        .unwrap();
        fs::write(&key, "x").unwrap();
        fs::write(
            root.join("lamp-voice.service"),
            format!(
                "[Service]\nEnvironment=LAMP_VOICE_ROUTE=second\nEnvironment=LAMP_VOICE_SECOND_CONFIG={}\n",
                config.display()
            ),
        )
        .unwrap();
        fs::write(
            drop_ins.join("own-keys.conf"),
            OWN_KEYS.replace("/etc/lampOS/voice/own", own.to_str().unwrap()),
        )
        .unwrap();
        VoiceRoute {
            unit: root.join("lamp-voice.service"),
            drop_ins,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("lamp-selftest-route-{name}-{}", std::process::id()))
    }

    /// lamp-06c1, 21 Oct, its voice folder opened to 0755: the own keys'
    /// drop-in loads the folder `own`, empty (no person's keys, the relay's
    /// used). systemd takes a folder for `LoadCredential=`, so it passes.
    #[test]
    fn the_own_keys_empty_folder_passes_as_on_lamp_06c1() {
        let root = scratch("own-empty");
        let own = root.join("own");
        let mut check = lamp(&root, &own);
        fs::create_dir_all(&own).unwrap();
        let result = check.run(Instant::now());
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_own_keys_folder_holding_keys_passes() {
        let root = scratch("own-keys");
        let own = root.join("own");
        let mut check = lamp(&root, &own);
        fs::create_dir_all(&own).unwrap();
        fs::write(own.join("deepseek.key"), "x").unwrap();
        fs::write(own.join("deepgram.key"), "x").unwrap();
        let result = check.run(Instant::now());
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        fs::remove_dir_all(&root).unwrap();
    }

    /// The own keys are optional, and the drop-in's
    /// `ConfigurationDirectory=` has systemd make the folder before it
    /// loads it: not there yet is no fault.
    #[test]
    fn the_own_keys_folder_not_made_yet_passes() {
        let root = scratch("own-unmade");
        let mut check = lamp(&root, Path::new("/etc/lampOS/voice/own"));
        let unit = check.unit().unwrap();
        assert_eq!(unit.configuration_directories, ["lampOS/voice/own"]);
        let result = judge(
            Some(&unit),
            |path| match path.to_str().unwrap() {
                "/etc/lampOS/voice/own" => Looked::Missing,
                _ => look(path),
            },
            read_config,
        );
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        let _ = check.run(Instant::now());
        fs::remove_dir_all(&root).unwrap();
    }

    /// An empty `ConfigurationDirectory=` in a later drop-in clears those
    /// before it: systemd no longer makes the own keys' folder, so its
    /// being missing is a fault again.
    #[test]
    fn the_own_keys_folder_not_made_fails_once_a_later_drop_in_clears_it() {
        let root = scratch("own-cleared");
        let check = lamp(&root, Path::new("/etc/lampOS/voice/own"));
        fs::write(
            check.drop_ins.join("zz-clear.conf"),
            "[Service]\nConfigurationDirectory=\n",
        )
        .unwrap();
        let unit = check.unit().unwrap();
        assert!(unit.configuration_directories.is_empty(), "{unit:?}");
        let result = judge(
            Some(&unit),
            |path| match path.to_str().unwrap() {
                "/etc/lampOS/voice/own" => Looked::Missing,
                _ => look(path),
            },
            read_config,
        );
        assert_eq!(result.verdict, Verdict::Fail, "{result:?}");
        assert!(
            result
                .reason
                .contains("the unit's credential own /etc/lampOS/voice/own is missing"),
            "{result:?}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_credential_file_that_is_missing_still_fails() {
        let root = scratch("cred-missing");
        let own = root.join("own");
        let mut check = lamp(&root, &own);
        fs::create_dir_all(&own).unwrap();
        let gone = root.join("deepseek.key");
        fs::write(
            check.drop_ins.join("deepseek.conf"),
            format!(
                "[Service]\nLoadCredential=deepseek.key:{}\n",
                gone.display()
            ),
        )
        .unwrap();
        let result = check.run(Instant::now());
        assert_eq!(result.verdict, Verdict::Fail, "{result:?}");
        assert!(
            result.reason.contains(&format!(
                "the unit's credential deepseek.key {} is missing",
                gone.display()
            )),
            "{result:?}"
        );
        fs::remove_dir_all(&root).unwrap();
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
        assert_eq!(look(&directory), Looked::Folder);
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
