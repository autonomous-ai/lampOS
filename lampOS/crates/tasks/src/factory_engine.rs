//! The lamp's factory engine (#6685): the engine a lamp was shipped with,
//! kept in [`FILE`] outside the tasks folder, so a reset (which wipes the
//! tasks folder and the app's `engine-default.json` with it) leaves it.
//! It ranks after the app's choice and `LAMP_LAMP_ENGINE`, ahead of the
//! build's order ([`crate::runtime::LampSide::factory`]). No file: the
//! build's order, as before.
//!
//! Written at the factory, as root, with `lamp-tasks factory-engine set
//! openclaw` (or `hermes`), and removed with `lamp-tasks factory-engine
//! clear`.

use crate::{harness::EngineKind, runtime::engine_named};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Where the lamp's factory engine is kept: under `/etc/lampOS`, outside
/// the reset plan's wipe, as `/etc/lampOS/body/lamp.env` is. The same
/// shape as the app's file: `{"version":1,"engine":"openclaw"}`.
pub const FILE: &str = "/etc/lampOS/tasks/factory-engine.json";

/// The most of the file read: it is a few dozen bytes.
const MOST_BYTES: u64 = 4096;

/// The factory engine in `file`: `Ok(None)` when there is no file, an
/// error saying why when it can't be read or is not valid.
pub fn read(file: &Path) -> Result<Option<EngineKind>, String> {
    let mut bytes = Vec::new();
    match fs::File::open(file) {
        Ok(opened) => opened
            .take(MOST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.kind().to_string())?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.kind().to_string()),
    };
    if bytes.len() as u64 > MOST_BYTES {
        return Err(format!("larger than {MOST_BYTES} bytes"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "not JSON".to_owned())?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("not version 1".into());
    }
    let name = value
        .get("engine")
        .and_then(Value::as_str)
        .ok_or("no engine named")?;
    engine_named(name)
        .map(Some)
        .ok_or_else(|| format!("no engine is called {name:?}"))
}

/// [`read`], said in the journal: a factory engine read, or one line why
/// the file was not read. Never a refusal: a file not read is no factory
/// engine, and the build's order is used.
pub fn load(file: &Path) -> Option<EngineKind> {
    match read(file) {
        Ok(Some(engine)) => {
            eprintln!("tasks: the lamp's factory engine: {}", engine.argument());
            Some(engine)
        }
        Ok(None) => None,
        Err(why) => {
            eprintln!(
                "tasks: the factory engine file {} not read: {why}",
                file.display()
            );
            None
        }
    }
}

/// `lamp-tasks factory-engine set openclaw|hermes [--file PATH]` and
/// `lamp-tasks factory-engine clear [--file PATH]`: writes the file
/// (0644, beside it and renamed over) or removes it. Says what it did.
pub fn command(args: &[String]) -> Result<String, String> {
    const USAGE: &str =
        "usage: lamp-tasks factory-engine set openclaw|hermes [--file PATH] | clear [--file PATH]";
    let (engine, rest) = match args {
        [verb, engine, rest @ ..] if verb == "set" => {
            let engine = match engine.as_str() {
                "openclaw" => EngineKind::Openclaw,
                "hermes" => EngineKind::Hermes,
                _ => return Err(USAGE.into()),
            };
            (Some(engine), rest)
        }
        [verb, rest @ ..] if verb == "clear" => (None, rest),
        _ => return Err(USAGE.into()),
    };
    let file = match rest {
        [] => PathBuf::from(FILE),
        [flag, path] if flag == "--file" => PathBuf::from(path),
        _ => return Err(USAGE.into()),
    };
    match engine {
        Some(engine) => {
            write(&file, engine).map_err(|error| format!("{}: {error}", file.display()))?;
            Ok(format!(
                "the lamp's factory engine is {} ({})",
                engine.argument(),
                file.display()
            ))
        }
        None => match fs::remove_file(&file) {
            Ok(()) => Ok(format!(
                "the lamp's factory engine is cleared ({})",
                file.display()
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(format!("no factory engine was kept ({})", file.display()))
            }
            Err(error) => Err(format!("{}: {error}", file.display())),
        },
    }
}

/// Writes `engine` to `file`, 0644, beside it and renamed over, its folder
/// made (0755) when missing.
pub fn write(file: &Path, engine: EngineKind) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let body = json!({"version":1,"engine":engine.argument()}).to_string();
    let mut name = file.as_os_str().to_owned();
    name.push(format!(".next-{}", std::process::id()));
    let next = PathBuf::from(name);
    let _ = fs::remove_file(&next);
    let mut opened = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&next)?;
    opened.write_all(body.as_bytes())?;
    opened.write_all(b"\n")?;
    opened.sync_all()?;
    // The umask may have taken bits off: the file is readable by all.
    fs::set_permissions(&next, fs::Permissions::from_mode(0o644))?;
    fs::rename(&next, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #6685: the factory file is read as the app's file is; none is no
    /// factory engine, and a bad one is no factory engine, said why.
    #[test]
    fn the_factory_file_is_read_and_a_missing_or_bad_one_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("factory-engine.json");
        assert_eq!(read(&file), Ok(None));
        assert_eq!(load(&file), None);
        fs::write(&file, br#"{"version":1,"engine":"openclaw"}"#).unwrap();
        assert_eq!(read(&file), Ok(Some(EngineKind::Openclaw)));
        assert_eq!(load(&file), Some(EngineKind::Openclaw));
        for (bad, why) in [
            (&b"not json"[..], "not JSON"),
            (br#"{"version":2,"engine":"openclaw"}"#, "not version 1"),
            (br#"{"version":1}"#, "no engine named"),
            (
                br#"{"version":1,"engine":"cursor"}"#,
                "no engine is called \"cursor\"",
            ),
        ] {
            fs::write(&file, bad).unwrap();
            assert_eq!(read(&file), Err(why.to_owned()));
            assert_eq!(load(&file), None);
        }
        fs::write(&file, vec![b' '; MOST_BYTES as usize + 1]).unwrap();
        assert!(read(&file).unwrap_err().starts_with("larger than"));
        // A folder in its place can't be read: none, said.
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(read(&file).is_err());
        assert_eq!(load(&file), None);
    }

    /// #6685: `factory-engine set` writes the file 0644 in a folder it
    /// makes, read back as written; `clear` removes it; anything else is
    /// refused with the usage and writes nothing.
    #[test]
    fn the_factory_engine_is_set_and_cleared_by_its_command() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tasks/factory-engine.json");
        let at = file.to_str().unwrap().to_owned();
        let run = |words: &[&str]| {
            let mut args: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
            args.extend(["--file".to_owned(), at.clone()]);
            command(&args)
        };
        for refused in [&["set", "claude"][..], &["set"], &["show"], &[]] {
            assert!(run(refused).unwrap_err().starts_with("usage:"));
        }
        assert!(
            command(&["clear".into(), "--other".into(), at.clone()])
                .unwrap_err()
                .starts_with("usage:")
        );
        assert!(!file.exists());
        assert!(run(&["set", "openclaw"]).unwrap().contains("openclaw"));
        assert_eq!(read(&file), Ok(Some(EngineKind::Openclaw)));
        let kept: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(kept, json!({"version":1,"engine":"openclaw"}));
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert!(run(&["set", "hermes"]).is_ok());
        assert_eq!(read(&file), Ok(Some(EngineKind::Hermes)));
        let left: Vec<_> = fs::read_dir(file.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "no file left beside it");
        assert!(run(&["clear"]).unwrap().contains("cleared"));
        assert_eq!(read(&file), Ok(None));
        assert!(run(&["clear"]).unwrap().starts_with("no factory engine"));
    }
}
