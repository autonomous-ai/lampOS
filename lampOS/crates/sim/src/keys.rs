//! A developer's own voice keys for the virtual lamp: read from one private file
//! (`~/.config/lampos/voice-keys.toml`) or from environment variables, which win.
//! Nothing here starts or feeds a voice yet; it only finds the keys and says,
//! without ever saying one, which are set and from where.
//!
//! ```toml
//! [main]                     # the voice's main (Live) provider
//! gemini = "..."             # a Google (Gemini) API key
//! [own]                      # a person's own keys on the voice's second route
//! deepseek = "..."
//! deepseek_url = "https://..."   # optional: the person's DeepSeek endpoint
//! deepgram = "..."
//! ```
//!
//! The rules (the first builder, 12 Nov): the file is the user's own and mode
//! 0600 or it is refused with the line that fixes it; an environment variable
//! overrides the file; an empty value is no key; lamp-sim never writes or logs a
//! key, and a [`Key`] cannot be printed, only its name, where it came from and its
//! length. The file rule is lamp-voice's own for its credential files
//! (`crates/voice/src/config.rs` `read_private`): a regular file, no symbolic link,
//! owned by the user, no group or other bits.
use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// The file, from `$HOME` (or `$XDG_CONFIG_HOME`).
const FILE_UNDER_CONFIG: &str = "lampos/voice-keys.toml";
/// One environment variable names another file.
pub const FILE_ENVIRONMENT: &str = "LAMPOS_VOICE_KEYS_FILE";
/// The file may not be larger than this (it holds four short lines).
const MOST_FILE_BYTES: u64 = 16 * 1024;
/// A key may not be longer than this; lamp-voice reads at most 4098 bytes with its newline.
const MOST_KEY_BYTES: usize = 4096;

/// Every key: its table and name in the file, and the variable that overrides it.
pub const KEYS: [(&str, &str, &str); 4] = [
    ("main", "gemini", "LAMPOS_VOICE_GEMINI_KEY"),
    ("own", "deepseek", "LAMPOS_VOICE_DEEPSEEK_KEY"),
    ("own", "deepseek_url", "LAMPOS_VOICE_DEEPSEEK_URL"),
    ("own", "deepgram", "LAMPOS_VOICE_DEEPGRAM_KEY"),
];

/// Where a key came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    File,
    Environment,
}

impl fmt::Display for Origin {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Origin::File => "file",
            Origin::Environment => "environment",
        })
    }
}

/// A key's value, which prints only its name, its origin and its length, and is
/// overwritten with zeros when dropped. It has no `Serialize` and no `Clone`: a
/// copy is made on purpose, by [`Key::expose`], where a use needs the text.
pub struct Key {
    name: String,
    origin: Origin,
    value: String,
}

impl Key {
    fn new(name: &str, origin: Origin, value: String) -> Key {
        Key {
            name: name.to_owned(),
            origin,
            value,
        }
    }

    /// `table.name`, as the file and the command say it.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// The key itself, for the one place that hands it to a voice.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "Key({}, from {}, {} bytes)",
            self.name,
            self.origin,
            self.value.len()
        )
    }
}

impl fmt::Display for Key {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, out)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.value).into_bytes();
        bytes.iter_mut().for_each(|byte| *byte = 0);
        std::hint::black_box(&bytes);
    }
}

/// The keys found: each `None` when neither the environment nor the file has it.
#[derive(Debug, Default)]
pub struct Keys {
    pub gemini: Option<Key>,
    pub deepseek: Option<Key>,
    pub deepseek_url: Option<Key>,
    pub deepgram: Option<Key>,
}

impl Keys {
    /// Each key in [`KEYS`]' order, with its name.
    pub fn each(&self) -> [(&'static str, &'static str, Option<&Key>); 4] {
        [
            (KEYS[0].0, KEYS[0].1, self.gemini.as_ref()),
            (KEYS[1].0, KEYS[1].1, self.deepseek.as_ref()),
            (KEYS[2].0, KEYS[2].1, self.deepseek_url.as_ref()),
            (KEYS[3].0, KEYS[3].1, self.deepgram.as_ref()),
        ]
    }
}

/// Why the keys could not be read, in words that never carry a key.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

impl fmt::Display for Refusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

/// The file's place: `LAMPOS_VOICE_KEYS_FILE`, else `$XDG_CONFIG_HOME` (when
/// absolute) or `$HOME/.config`, under `lampos/voice-keys.toml`. `None` when no
/// home is known.
pub fn path_from(environment: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |name: &str| environment(name).filter(|value| !value.is_empty());
    if let Some(path) = set(FILE_ENVIRONMENT) {
        return Some(PathBuf::from(path));
    }
    let config = set("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join(FILE_UNDER_CONFIG))
}

/// What the file holds for each key (`None`: not in it, or empty), after it has
/// been checked. A missing file is no keys, not an error.
fn from_file(path: &Path) -> Result<Vec<Option<String>>, Refusal> {
    let none = || vec![None; KEYS.len()];
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(none()),
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            return Err(Refusal(format!(
                "{} is a symbolic link; keep the keys in a file there, not a link to one",
                path.display()
            )));
        }
        Err(error) => {
            return Err(Refusal(format!(
                "{} cannot be read ({})",
                path.display(),
                error.kind()
            )));
        }
    };
    let info = file.metadata().map_err(|error| {
        Refusal(format!(
            "{} cannot be read ({})",
            path.display(),
            error.kind()
        ))
    })?;
    if !info.is_file() {
        return Err(Refusal(format!("{} is not a regular file", path.display())));
    }
    if info.uid() != rustix::process::geteuid().as_raw() {
        return Err(Refusal(format!(
            "{} is not owned by you; keys are read only from your own file",
            path.display()
        )));
    }
    if info.mode() & 0o077 != 0 {
        return Err(Refusal(format!(
            "{} can be read by others (mode {:o}); run: chmod 600 {}",
            path.display(),
            info.mode() & 0o777,
            path.display()
        )));
    }
    if info.len() > MOST_FILE_BYTES {
        return Err(Refusal(format!(
            "{} is over {MOST_FILE_BYTES} bytes",
            path.display()
        )));
    }
    let mut bytes = read_bounded(&mut file, path)?;
    let result = parse(&bytes, path);
    bytes.iter_mut().for_each(|byte| *byte = 0);
    result
}

/// The file's bytes, read through `take(MOST + 1)`: a file that grew after the length check (or that is
/// not what `metadata` said) is never read past one byte over the bound, and is refused when it came.
fn read_bounded(reader: &mut dyn Read, path: &Path) -> Result<Vec<u8>, Refusal> {
    let mut bytes = Vec::new();
    reader
        .take(MOST_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Refusal(format!(
                "{} cannot be read ({})",
                path.display(),
                error.kind()
            ))
        })?;
    if bytes.len() as u64 > MOST_FILE_BYTES {
        bytes.iter_mut().for_each(|byte| *byte = 0);
        return Err(Refusal(format!(
            "{} is over {MOST_FILE_BYTES} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

/// The four keys out of the file's text. A refusal names the file, a line and a
/// key's name, never any text of a value: the parser's own message quotes the
/// line, so it is not used.
fn parse(bytes: &[u8], path: &Path) -> Result<Vec<Option<String>>, Refusal> {
    let shown = path.display();
    let text =
        std::str::from_utf8(bytes).map_err(|_| Refusal(format!("{shown} is not UTF-8 text")))?;
    let line_of = |at: usize| text[..at.min(text.len())].matches('\n').count() + 1;
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| {
        let line = error.span().map_or_else(String::new, |span| {
            format!(" at line {}", line_of(span.start))
        });
        Refusal(format!(
            "{shown} is not valid TOML{line} (the text is not shown, it may hold a key)"
        ))
    })?;
    // The line a name stands on, by looking for it; the refusal says the line, never an unknown name
    // (a key pasted where a name belongs would be echoed).
    let line_with = |name: &str| {
        text.lines()
            .position(|line| {
                let line = line.trim_start();
                line.starts_with(name)
                    || line
                        .strip_prefix('[')
                        .is_some_and(|rest| rest.trim_start().starts_with(name))
            })
            .map_or_else(String::new, |at| format!(" at line {}", at + 1))
    };
    let mut found = vec![None; KEYS.len()];
    for (group, content) in &table {
        let known_group = KEYS.iter().any(|(g, _, _)| g == group);
        let Some(content) = content.as_table().filter(|_| known_group) else {
            return Err(Refusal(format!(
                "{shown}: an unknown table{}: lamp-sim knows only [main] and [own]",
                line_with(group)
            )));
        };
        for (name, value) in content {
            let Some(at) = KEYS.iter().position(|(g, n, _)| g == group && n == name) else {
                return Err(Refusal(format!(
                    "{shown}: an unknown key{} of [{group}]: lamp-sim knows {}",
                    line_with(name),
                    KEYS.iter()
                        .map(|(g, n, _)| format!("{g}.{n}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                )));
            };
            let Some(value) = value.as_str() else {
                return Err(Refusal(format!(
                    "{shown}: `{group}.{name}` must be a quoted text{}",
                    line_with(name)
                )));
            };
            found[at] = Some(value.to_owned());
        }
    }
    Ok(found)
}

/// One value as a key's text: the trailing line end taken off (lamp-voice does
/// the same to a credential file), empty is none, a control character inside or
/// too long is refused by the key's name.
fn text_of(name: &str, from: &str, raw: &str) -> Result<Option<String>, Refusal> {
    let value = raw.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MOST_KEY_BYTES {
        return Err(Refusal(format!(
            "{name} (from {from}) is over {MOST_KEY_BYTES} bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(Refusal(format!(
            "{name} (from {from}) holds a control character or a line break"
        )));
    }
    Ok(Some(value.to_owned()))
}

/// The keys, from `environment` (a variable wins) and the file at `path`
/// (`None`: no file is looked for). The file is read even when every variable is
/// set, so that a bad file is always said.
pub fn load(
    environment: &dyn Fn(&str) -> Option<OsString>,
    path: Option<&Path>,
) -> Result<Keys, Refusal> {
    let filed = match path {
        Some(path) => from_file(path)?,
        None => vec![None; KEYS.len()],
    };
    let mut keys = Vec::new();
    for (at, (group, name, variable)) in KEYS.iter().enumerate() {
        let full = format!("{group}.{name}");
        let from_environment = match environment(variable) {
            Some(value) => {
                let text = value
                    .into_string()
                    .map_err(|_| Refusal(format!("{variable} is not UTF-8 text")))?;
                text_of(variable, "environment", &text)?
            }
            None => None,
        };
        keys.push(if let Some(value) = from_environment {
            Some(Key::new(&full, Origin::Environment, value))
        } else {
            match &filed[at] {
                Some(value) => {
                    text_of(&full, "file", value)?.map(|value| Key::new(&full, Origin::File, value))
                }
                None => None,
            }
        });
    }
    let mut keys = keys.into_iter();
    let mut next = || keys.next().flatten();
    Ok(Keys {
        gemini: next(),
        deepseek: next(),
        deepseek_url: next(),
        deepgram: next(),
    })
}

/// `lamp-sim voice-keys`: one line for the file and one per key, set (file or
/// environment) or not set, never a value. Returns the text and the exit code:
/// 0, or 2 with the refusal when the file is not acceptable.
pub fn report(environment: &dyn Fn(&str) -> Option<OsString>) -> (String, i32) {
    let path = path_from(environment);
    match load(environment, path.as_deref()) {
        Ok(keys) => {
            let mut out = match &path {
                Some(path) => format!(
                    "keys file: {} ({})\n",
                    path.display(),
                    if path.exists() { "found" } else { "absent" }
                ),
                None => "keys file: no home folder is known (set HOME or LAMPOS_VOICE_KEYS_FILE)\n"
                    .to_owned(),
            };
            for (group, name, key) in keys.each() {
                match key {
                    Some(key) => out.push_str(&format!("{group}.{name}: set ({})\n", key.origin())),
                    None => out.push_str(&format!("{group}.{name}: not set\n")),
                }
            }
            (out, 0)
        }
        Err(refusal) => (format!("lamp-sim: {refusal}\n"), 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    const GEMINI: &str = "test-key-not-real-gemini";
    const DEEPSEEK: &str = "test-key-not-real-deepseek";
    const URL: &str = "https://test-not-real.invalid/v1";
    const DEEPGRAM: &str = "test-key-not-real-deepgram";
    const FILE: &str = "[main]\ngemini = \"test-key-not-real-gemini\"\n[own]\ndeepseek = \"test-key-not-real-deepseek\"\ndeepseek_url = \"https://test-not-real.invalid/v1\"\ndeepgram = \"test-key-not-real-deepgram\"\n";

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lsm-keys-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(dir: &Path, text: &str, mode: u32) -> PathBuf {
        let path = dir.join("voice-keys.toml");
        // A file left at 0400 by the last call cannot be written over: it goes first.
        let _ = fs::remove_file(&path);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn no_environment(_: &str) -> Option<OsString> {
        None
    }

    /// An environment of these variables, owned so that a test can build it inline.
    fn with(set: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let set: Vec<(String, String)> = set
            .iter()
            .map(|(variable, value)| ((*variable).to_owned(), (*value).to_owned()))
            .collect();
        move |name| {
            set.iter()
                .find(|(variable, _)| variable == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn a_private_file_gives_every_key_and_a_missing_one_is_no_keys() {
        let dir = folder("taken");
        let path = file(&dir, FILE, 0o600);
        let keys = load(&no_environment, Some(&path)).unwrap();
        assert_eq!(keys.gemini.as_ref().unwrap().expose(), GEMINI);
        assert_eq!(keys.deepseek.as_ref().unwrap().expose(), DEEPSEEK);
        assert_eq!(keys.deepseek_url.as_ref().unwrap().expose(), URL);
        assert_eq!(keys.deepgram.as_ref().unwrap().expose(), DEEPGRAM);
        assert!(
            keys.each()
                .iter()
                .all(|(_, _, key)| key.unwrap().origin() == Origin::File)
        );
        // Absent: no keys and no error; no home: no file looked for.
        let absent = load(&no_environment, Some(&dir.join("nothing.toml"))).unwrap();
        assert!(absent.each().iter().all(|(_, _, key)| key.is_none()));
        assert!(load(&no_environment, None).unwrap().gemini.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_others_can_read_is_refused_with_the_line_that_fixes_it() {
        let dir = folder("modes");
        for mode in [0o644, 0o640, 0o604, 0o660, 0o666, 0o700 | 0o010] {
            let path = file(&dir, FILE, mode);
            let refusal = load(&no_environment, Some(&path)).unwrap_err().0;
            assert!(
                refusal.contains(&format!("chmod 600 {}", path.display())),
                "{mode:o}: {refusal}"
            );
        }
        // 0400 and 0600 are the user's alone.
        for mode in [0o400, 0o600] {
            let path = file(&dir, FILE, mode);
            assert!(load(&no_environment, Some(&path)).is_ok(), "{mode:o}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symbolic_link_a_directory_and_an_oversize_file_are_refused() {
        let dir = folder("kinds");
        let real = file(&dir, FILE, 0o600);
        let link = dir.join("link.toml");
        symlink(&real, &link).unwrap();
        assert!(
            load(&no_environment, Some(&link))
                .unwrap_err()
                .0
                .contains("symbolic link")
        );
        let refusal = load(&no_environment, Some(&dir)).unwrap_err().0;
        assert!(
            refusal.contains("not a regular file") || refusal.contains("cannot be read"),
            "{refusal}"
        );
        let big = file(
            &dir,
            &format!("[main]\ngemini = \"{}\"\n", "x".repeat(20_000)),
            0o600,
        );
        assert!(
            load(&no_environment, Some(&big))
                .unwrap_err()
                .0
                .contains("is over")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_another_owner_is_refused_when_one_can_be_found() {
        // Only root can make a file of another owner; /etc/passwd is root's and world-readable,
        // so it is refused for its owner or its mode either way, and is never taken as keys.
        let refusal = load(&no_environment, Some(Path::new("/etc/passwd")))
            .unwrap_err()
            .0;
        assert!(
            refusal.contains("not owned by you") || refusal.contains("chmod 600"),
            "{refusal}"
        );
    }

    #[test]
    fn an_environment_variable_wins_and_an_empty_one_is_no_key() {
        let dir = folder("override");
        let path = file(&dir, FILE, 0o600);
        let environment = with(&[
            (
                "LAMPOS_VOICE_GEMINI_KEY",
                "test-key-not-real-from-environment",
            ),
            ("LAMPOS_VOICE_DEEPSEEK_KEY", ""),
        ]);
        let keys = load(&environment, Some(&path)).unwrap();
        let gemini = keys.gemini.as_ref().unwrap();
        assert_eq!(
            (gemini.expose(), gemini.origin()),
            ("test-key-not-real-from-environment", Origin::Environment)
        );
        // An empty variable is not a key: the file's stands.
        let deepseek = keys.deepseek.as_ref().unwrap();
        assert_eq!(
            (deepseek.expose(), deepseek.origin()),
            (DEEPSEEK, Origin::File)
        );
        // A variable alone, with no file at all.
        let alone = load(
            &with(&[("LAMPOS_VOICE_DEEPGRAM_KEY", "test-key-not-real-d\n")]),
            None,
        )
        .unwrap();
        assert_eq!(
            alone.deepgram.as_ref().unwrap().expose(),
            "test-key-not-real-d",
            "a trailing line end is taken off"
        );
        assert!(alone.gemini.is_none());
        // An empty value in the file is no key either.
        let blank = file(&dir, "[main]\ngemini = \"\"\n", 0o600);
        assert!(
            load(&no_environment, Some(&blank))
                .unwrap()
                .gemini
                .is_none()
        );
        // A control character inside is refused by the key's name.
        let refusal = load(
            &with(&[("LAMPOS_VOICE_GEMINI_KEY", "test-key\nnot-real")]),
            None,
        )
        .unwrap_err()
        .0;
        assert!(
            refusal.contains("LAMPOS_VOICE_GEMINI_KEY") && !refusal.contains("test-key"),
            "{refusal}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_found_in_the_users_config_folder_or_where_one_variable_says() {
        let at = |set: &[(&str, &str)]| path_from(&with(set));
        assert_eq!(
            at(&[("HOME", "/h")]),
            Some(PathBuf::from("/h/.config/lampos/voice-keys.toml"))
        );
        assert_eq!(
            at(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")]),
            Some(PathBuf::from("/x/lampos/voice-keys.toml"))
        );
        assert_eq!(
            at(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "relative")]),
            Some(PathBuf::from("/h/.config/lampos/voice-keys.toml")),
            "a relative XDG folder is ignored"
        );
        assert_eq!(
            at(&[("HOME", "/h"), (FILE_ENVIRONMENT, "/elsewhere/k.toml")]),
            Some(PathBuf::from("/elsewhere/k.toml"))
        );
        assert_eq!(at(&[]), None);
    }

    #[test]
    fn a_malformed_file_is_named_by_line_and_never_echoes_a_value() {
        let dir = folder("malformed");
        let secret = "test-key-not-real-in-a-bad-file";
        for text in [
            format!("[main]\ngemini = \"{secret}\n"),
            format!("[main]\ngemini = {secret}\n"),
            format!("[main\ngemini = \"{secret}\"\n"),
            format!("[main]\ngemini = \"{secret}\"\ngemini = \"{secret}\"\n"),
        ] {
            let path = file(&dir, &text, 0o600);
            let refusal = load(&no_environment, Some(&path)).unwrap_err().0;
            assert!(
                refusal.contains("not valid TOML") && refusal.contains("line"),
                "{refusal}"
            );
            assert!(!refusal.contains(secret), "{refusal}");
        }
        // A name the sim does not know (a key pasted where a name belongs, a typo), a table that is not
        // one, an unknown table: said by line, the name never echoed; a known key of the wrong type by
        // its own name.
        for (text, shown) in [
            (format!("[main]\n{secret} = \"x\"\n"), "at line 2"),
            (format!("[main]\ngemin = \"{secret}\"\n"), "at line 2"),
            (format!("[{secret}]\ngemini = \"x\"\n"), "at line 1"),
            (format!("{secret} = \"x\"\n"), "at line 1"),
            (format!("gemini = \"{secret}\"\n"), "at line 1"),
        ] {
            let path = file(&dir, &text, 0o600);
            let refusal = load(&no_environment, Some(&path)).unwrap_err().0;
            assert!(refusal.contains(shown), "{refusal}");
            assert!(
                refusal.contains("unknown") && refusal.contains("lamp-sim knows"),
                "{refusal}"
            );
            assert!(
                !refusal.contains(secret)
                    && !refusal.contains("gemin ")
                    && !refusal.contains("\"x\""),
                "{refusal}"
            );
        }
        let typed = file(&dir, "[main]\ngemini = 5\n", 0o600);
        assert!(
            load(&no_environment, Some(&typed))
                .unwrap_err()
                .0
                .contains("`main.gemini` must be a quoted text")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_value_is_in_the_report_or_in_any_debug_or_display_text() {
        let dir = folder("silent");
        let path = file(&dir, FILE, 0o600);
        let environment = with(&[
            ("LAMPOS_VOICE_KEYS_FILE", path.to_str().unwrap()),
            (
                "LAMPOS_VOICE_DEEPGRAM_KEY",
                "test-key-not-real-from-environment",
            ),
        ]);
        let (report, code) = report(&environment);
        assert_eq!(code, 0);
        assert!(report.contains("main.gemini: set (file)"), "{report}");
        assert!(
            report.contains("own.deepgram: set (environment)"),
            "{report}"
        );
        let keys = load(&environment, Some(&path)).unwrap();
        let mut seen = report.clone();
        seen.push_str(&format!("{keys:?}"));
        for (_, _, key) in keys.each() {
            let key = key.unwrap();
            seen.push_str(&format!("{key:?} {key} {key:#?}"));
        }
        for value in [GEMINI, DEEPSEEK, URL, DEEPGRAM, "test-key-not-real"] {
            assert!(!seen.contains(value), "a value is in the output: {value}");
        }
        assert!(
            seen.contains("Key(main.gemini, from file, 24 bytes)"),
            "{seen}"
        );
        // A refused file: exit 2, the refusal, no value.
        let open = file(&dir, FILE, 0o644);
        let refused = with(&[("LAMPOS_VOICE_KEYS_FILE", open.to_str().unwrap())]);
        let (text, code) = super::report(&refused);
        assert_eq!(code, 2);
        assert!(
            text.contains("chmod 600") && !text.contains("test-key-not-real"),
            "{text}"
        );
        // No file at all is not an error: every key says not set.
        let (none, code) = super::report(&with(&[(
            "LAMPOS_VOICE_KEYS_FILE",
            dir.join("nothing").to_str().unwrap(),
        )]));
        assert_eq!(code, 0);
        assert_eq!(none.matches("not set").count(), 4, "{none}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_stops_one_byte_over_the_bound_even_if_the_file_grew_after_the_length_check() {
        let path = Path::new("/keys.toml");
        // An endless reader stands for a file that grew: it is read to the bound plus one byte and refused.
        let refusal = read_bounded(&mut io::repeat(b'x'), path).unwrap_err().0;
        assert!(refusal.contains("is over 16384 bytes"), "{refusal}");
        // Exactly the bound is taken; one byte over is refused.
        let exact = vec![b'a'; MOST_FILE_BYTES as usize];
        assert_eq!(
            read_bounded(&mut exact.as_slice(), path).unwrap().len(),
            exact.len()
        );
        let over = vec![b'a'; MOST_FILE_BYTES as usize + 1];
        assert!(read_bounded(&mut over.as_slice(), path).is_err());
    }
}
