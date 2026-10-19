//! Optional hardware-owner metadata for the physical privacy-switch location.
//! No shipping profile or location is assumed. Read once when onboarding is
//! initialized, never on an interaction pass. Missing or invalid means unknown.
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

pub(crate) const ENVIRONMENT: &str = "LAMP_ONBOARDING_HARDWARE_PROFILE";
const MOST_BYTES: u64 = 4096;
const MOST_CHARS: usize = 160;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    privacy_switch_location: String,
}

/// Only explicit hardware-owner configuration supplies a location. This is
/// neither user identity nor evidence that a person performed a privacy check.
pub(crate) fn from_environment() -> Option<String> {
    let path = std::env::var_os(ENVIRONMENT)?;
    read(Path::new(&path))
}

fn parse(bytes: &[u8]) -> Option<String> {
    if bytes.len() as u64 > MOST_BYTES {
        return None;
    }
    let profile: Profile = serde_json::from_slice(bytes).ok()?;
    let phrase = profile.privacy_switch_location;
    if phrase.chars().any(char::is_control) || phrase.chars().count() > MOST_CHARS {
        return None;
    }
    let phrase = phrase.trim();
    (!phrase.is_empty()).then(|| phrase.to_owned())
}

fn read(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    // NONBLOCK prevents opening a FIFO from waiting for a writer before the
    // regular-file check. NOFOLLOW rejects a symlink at the configured leaf.
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .ok()?;
    let file = File::from(descriptor);
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MOST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MOST_BYTES + 1).read_to_end(&mut bytes).ok()?;
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    fn encoded(phrase: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"privacy_switch_location":phrase})).unwrap()
    }

    #[test]
    fn only_a_bounded_location_phrase_is_a_profile() {
        assert_eq!(
            parse(&encoded("  beside the base  ")),
            Some("beside the base".into())
        );
        assert!(parse(&encoded(&"é".repeat(160))).is_some());
        for phrase in [
            "".to_owned(),
            "   ".into(),
            "at\nthe back".into(),
            "x\0".into(),
            "x".repeat(161),
        ] {
            assert_eq!(parse(&encoded(&phrase)), None);
        }
        for bytes in [
            b"not json".as_slice(),
            b"{}",
            br#"{"privacy_switch_location":null}"#,
            br#"{"privacy_switch_location":"back","extra":true}"#,
            br#"{"privacy_switch_location":"back","privacy_switch_location":"front"}"#,
        ] {
            assert_eq!(parse(bytes), None);
        }
        let mut oversized = encoded("beside the base");
        oversized.resize(MOST_BYTES as usize + 1, b' ');
        assert_eq!(parse(&oversized), None);
    }

    #[test]
    fn only_an_explicit_absolute_regular_file_is_read_without_following_its_link() {
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = Root(std::env::temp_dir().join(format!(
            "lamp-onboarding-hardware-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        )));
        fs::create_dir(&root.0).unwrap();
        let profile = root.0.join("hardware.json");
        fs::write(&profile, encoded("beside the base")).unwrap();
        assert_eq!(read(&profile), Some("beside the base".into()));
        assert_eq!(read(Path::new("hardware.json")), None);
        assert_eq!(read(&root.0.join("missing")), None);
        assert!(!root.0.join("missing").exists());
        assert_eq!(read(&root.0), None);
        let link = root.0.join("linked.json");
        symlink(&profile, &link).unwrap();
        assert_eq!(read(&link), None);
        fs::write(&profile, vec![b' '; MOST_BYTES as usize + 1]).unwrap();
        assert_eq!(read(&profile), None);
    }
}
