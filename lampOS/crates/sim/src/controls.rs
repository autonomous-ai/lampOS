//! The real `lamp-controls`, started beside the lamp when asked (`lamp-sim run DIR --controls`): it
//! publishes `controls/state` on the lamp's bus, which a real `lamp-voice`'s privacy route waits
//! for. No stand-in: on a desktop it runs as it does on a lamp with no slide
//! (`LAMP_PRIVACY_CONTROLS=no-slide`, no pins file, so it reads no pin and reports its input as
//! missing) and says `no_privacy_slide` on every state, on this boot's clock, as the voice's
//! fence requires (`crates/voice/src/privacy_bus.rs`). No button, touch or slide is simulated.
use std::path::{Path, PathBuf};

/// The flag that starts it; off unless given, because nothing uses it before a real voice does.
pub const FLAG: &str = "--controls";
/// The binary, beside `lamp-control` and `lamp-body`.
pub const PROGRAM: &str = "lamp-controls";
/// The route with no slide wired (`crates/controls/src/privacy.rs`).
pub const ROUTE: (&str, &str) = ("LAMP_PRIVACY_CONTROLS", "no-slide");

/// Whether `--controls` was given to `lamp-sim run`.
pub fn wanted(args: &[String]) -> bool {
    args.iter().any(|arg| arg == FLAG)
}

/// What to run: the program, its arguments and the environment it is given.
#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The binary, or why it is not there: said before anything else of the lamp starts.
pub fn present(bin: &Path) -> Result<PathBuf, String> {
    let program = bin.join(PROGRAM);
    if program.is_file() {
        Ok(program)
    } else {
        Err(format!(
            "--controls needs {PROGRAM} in {} (build it: cargo build --release -p lamp-controls)",
            bin.display()
        ))
    }
}

/// `lamp-controls serve --enable` on the lamp's bus and this boot's clock, with the no-slide route.
/// Refused in words when the binary is not beside the others.
pub fn launch(bin: &Path, bus_root: &Path, boot_id: &str) -> Result<Launch, String> {
    let program = present(bin)?;
    Ok(Launch {
        program,
        args: vec![
            "serve".to_owned(),
            "--enable".to_owned(),
            "--bus-root".to_owned(),
            bus_root.display().to_string(),
            "--clock-id".to_owned(),
            boot_id.to_owned(),
        ],
        env: vec![(ROUTE.0.to_owned(), ROUTE.1.to_owned())],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lsm-controls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_flag_is_off_unless_given() {
        let args = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert!(!wanted(&args(&["lamp-sim", "run", "/d", "--camera"])));
        assert!(wanted(&args(&["lamp-sim", "run", "/d", "--controls"])));
        assert!(
            !wanted(&args(&["lamp-sim", "run", "/d", "--controls=off"])),
            "only the exact flag"
        );
    }

    #[test]
    fn a_missing_binary_is_said_in_words_with_the_way_to_build_it() {
        let dir = folder("missing");
        let refusal = launch(&dir, Path::new("/bus"), "boot").unwrap_err();
        assert!(
            refusal.contains("--controls needs lamp-controls")
                && refusal.contains("cargo build --release -p lamp-controls"),
            "{refusal}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_is_started_on_the_lamps_bus_and_boot_with_the_no_slide_route_and_no_pins() {
        let dir = folder("argv");
        std::fs::write(dir.join(PROGRAM), "#!/bin/sh\n").unwrap();
        let made = launch(&dir, Path::new("/lamp/bus"), "boot-1234").unwrap();
        assert_eq!(made.program, dir.join(PROGRAM));
        assert_eq!(
            made.args,
            [
                "serve",
                "--enable",
                "--bus-root",
                "/lamp/bus",
                "--clock-id",
                "boot-1234"
            ]
        );
        assert_eq!(
            made.env,
            [("LAMP_PRIVACY_CONTROLS".to_owned(), "no-slide".to_owned())]
        );
        assert!(
            !made.args.iter().any(|a| a == "--pins" || a == "--touch"),
            "no pins file, no touch path: nothing to read"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fake_program_is_given_exactly_that_argv_and_environment() {
        let _serial = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = folder("fake");
        let record = dir.join("seen");
        let program = dir.join(PROGRAM);
        std::fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {0}\nprintf 'route=%s\\n' \"$LAMP_PRIVACY_CONTROLS\" >> {0}\n", record.display())).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let made = launch(&dir, Path::new("/lamp/bus"), "boot-1234").unwrap();
        let mut command = Command::new(&made.program);
        command.args(&made.args);
        for (name, value) in &made.env {
            command.env(name, value);
        }
        assert!(command.status().unwrap().success());
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            "serve\n--enable\n--bus-root\n/lamp/bus\n--clock-id\nboot-1234\nroute=no-slide\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
