//! The owner's language, said to a program in the welcome's and the hello's
//! `lamp` (`language`), so a program's own lines can follow it. One code from
//! the file the setup writes (the product's `/etc/lampOS/setup/language`):
//! `en`, `vi`, `ja`, `zh` or `ko`. A missing or empty file, an unreadable one
//! or a value this build does not know is `en`, as the voice reads it
//! (`lamp-voice`'s `language.rs`): a program never has to handle an absence.
//!
//! Read at each welcome and hello (a file of one short line), so a language
//! the owner changes shows on the next connection, with no restart.
use std::{fs, io::Read, path::Path};

/// The product's file.
pub const PATH: &str = "/etc/lampOS/setup/language";

/// The longest file taken: a code and a newline, with room to spare.
const FILE_MOST: u64 = 64;

/// The codes a program is told, English first.
pub const CODES: [&str; 5] = ["en", "vi", "ja", "zh", "ko"];

/// The language `file` names, or `en`.
pub fn read(file: &Path) -> &'static str {
    let Ok(opened) = fs::File::open(file) else {
        return CODES[0];
    };
    let mut text = String::new();
    if opened.take(FILE_MOST).read_to_string(&mut text).is_err() {
        return CODES[0];
    }
    let word = text.trim().to_ascii_lowercase();
    CODES
        .iter()
        .copied()
        .find(|code| *code == word)
        .unwrap_or(CODES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of its own under the temp folder, kept for the test's span.
    struct Held(std::path::PathBuf);
    impl Drop for Held {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn file(tag: &str, bytes: impl AsRef<[u8]>) -> Held {
        let path =
            std::env::temp_dir().join(format!("lampos-language-{}-{tag}", std::process::id()));
        fs::write(&path, bytes).unwrap();
        Held(path)
    }

    #[test]
    fn a_known_code_is_said_as_it_is_and_the_rest_is_english() {
        for (text, said) in [
            ("vi\n", "vi"),
            ("ja", "ja"),
            ("  ZH \n", "zh"),
            ("ko\n", "ko"),
            ("en\n", "en"),
            ("", "en"),
            ("fr\n", "en"),
            ("vi extra\n", "en"),
        ] {
            let held = file("known", text);
            assert_eq!(read(&held.0), said, "{text:?}");
        }
    }

    #[test]
    fn no_file_is_english() {
        assert_eq!(read(Path::new("/nonexistent/lampos/language")), "en");
    }

    #[test]
    fn a_huge_or_binary_file_is_english() {
        let huge = file("huge", "vi".repeat(100));
        assert_eq!(read(&huge.0), "en");
        let binary = file("binary", [0xff, 0xfe, 0x00]);
        assert_eq!(read(&binary.0), "en");
    }
}
