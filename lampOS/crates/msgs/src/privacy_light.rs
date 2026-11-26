//! The privacy light's look (#8696, `docs/ring-choreography.md`, "The
//! person's look"): what the ring shows while the privacy slide is closed,
//! red (today's breath, the default), a dimmer red, or nothing. It changes
//! only the light: the microphones and the camera close the same in all
//! three, and nothing on the mute's path reads it.
//!
//! Kept in one small file, [`PATH`], under the light folder (0700 root),
//! which the reset's plan already wipes, so a reset gives red again. Root's
//! change helper (`lamp-setup change`) is its one writer, for `lamp-cloud`'s
//! `led.privacy.set` alone; the voice's privacy ring worker (root) reads it.
//! Every reader takes it with [`read`], which fails safe to red: a missing
//! file, one that is not a plain file, a link, one too long, not the shape
//! below, or a look it does not know, is red.
//!
//! The file is `{"look":"dim"}` and nothing else.
use serde_json::Value;
use std::io::Read;
use std::path::Path;

/// The switch: `on` exactly, in `lamp-voice` (the worker paints by the look)
/// and `lamp-cloud` (the app's two kinds). Off, the ring shows red as before
/// and the app is not offered the choice.
pub const ENVIRONMENT: &str = "LAMP_PRIVACY_LIGHT";

/// Where the look is kept: beside the person's light, in a folder the
/// reset's plan wipes.
pub const PATH: &str = "/var/lib/lampOS/light/privacy.json";

/// The most a kept look is read, in bytes. `{"look":"dim"}` and a newline
/// is 15.
pub const READ_MOST: u64 = 64;

/// The three looks the person may choose. No colour or level is chosen:
/// each look is one fixed frame the ring itself draws.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Look {
    /// Today's red breath, red 2 to 10. The default and the fail safe.
    #[default]
    Red,
    /// The same breath, dimmer: red 1 to 4.
    Dim,
    /// Dark: the slide still mutes, only the light is off.
    Off,
}

impl Look {
    /// Every look, in the app's order.
    pub const ALL: [Look; 3] = [Look::Red, Look::Dim, Look::Off];

    /// The look's name, as the app and the file say it.
    pub fn name(self) -> &'static str {
        match self {
            Look::Red => "red",
            Look::Dim => "dim",
            Look::Off => "off",
        }
    }

    /// The look named `name` exactly; none for anything else.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|look| look.name() == name)
    }
}

/// Whether the switch's value is `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`on`] for this process's environment.
pub fn on_in_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// The file's bytes for `look`.
pub fn bytes(look: Look) -> Vec<u8> {
    format!("{{\"look\":\"{}\"}}\n", look.name()).into_bytes()
}

/// The look in `text`: exactly `{"look": NAME}` with a known name; none for
/// anything else.
pub fn parse(text: &str) -> Option<Look> {
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object().filter(|object| object.len() == 1)?;
    Look::named(object.get("look")?.as_str()?)
}

/// The look kept at `path`, red whenever it cannot be read as one: missing,
/// a link (never followed), not a plain file, longer than [`READ_MOST`], not
/// UTF-8, or not [`parse`]'s shape. Never blocks on a FIFO.
pub fn read(path: &Path) -> Look {
    read_kept(path).unwrap_or_default()
}

fn read_kept(path: &Path) -> Option<Look> {
    use rustix::fs::OFlags;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > READ_MOST {
        return None;
    }
    let mut text = String::new();
    file.take(READ_MOST + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > READ_MOST {
        return None;
    }
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn folder(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "lamp-msgs-privacy-light-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// The first builder's fail safe (#8696): whatever cannot be read as a
    /// known look is red; only the file's exact shape gives dim or off.
    #[test]
    fn the_privacy_look_reads_red_when_missing_unknown_or_unreadable() {
        let folder = folder("read");
        let path = folder.join("privacy.json");
        assert_eq!(read(&path), Look::Red, "missing");
        for (text, want) in [
            ("{\"look\":\"dim\"}\n", Look::Dim),
            ("{\"look\":\"off\"}", Look::Off),
            ("{\"look\":\"red\"}\n", Look::Red),
            ("{}", Look::Red),
            ("{\"look\":\"blue\"}", Look::Red),
            ("{\"look\":\"Dim\"}", Look::Red),
            ("{\"look\":1}", Look::Red),
            ("{\"look\":\"dim\",\"level\":9}", Look::Red),
            ("[\"dim\"]", Look::Red),
            ("dim", Look::Red),
            ("", Look::Red),
        ] {
            fs::write(&path, text).unwrap();
            assert_eq!(read(&path), want, "{text:?}");
        }
        // Too long, though it begins as a good one.
        let long = format!("{{\"look\":\"dim\"}}{}", " ".repeat(READ_MOST as usize));
        fs::write(&path, long).unwrap();
        assert_eq!(read(&path), Look::Red, "too long");
        // Not UTF-8.
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(read(&path), Look::Red, "not UTF-8");
        // A link to a good file is never followed.
        let good = folder.join("good.json");
        fs::write(&good, bytes(Look::Off)).unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&good, &path).unwrap();
        assert_eq!(read(&path), Look::Red, "a link");
        // A folder.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(read(&path), Look::Red, "a folder");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn each_look_is_written_as_its_reader_reads_it() {
        let folder = folder("bytes");
        let path = folder.join("privacy.json");
        for look in Look::ALL {
            fs::write(&path, bytes(look)).unwrap();
            assert_eq!(read(&path), look);
            assert!(bytes(look).len() as u64 <= READ_MOST);
            assert_eq!(Look::named(look.name()), Some(look));
        }
        assert_eq!(Look::default(), Look::Red);
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("1"), Some("ON"), Some("on ")] {
            assert!(!on(value), "{value:?}");
        }
        let _ = fs::remove_dir_all(&folder);
    }
}
