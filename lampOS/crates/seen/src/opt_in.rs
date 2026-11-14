//! The owner's opt-in for `lamp-seen` (#6543, phase 0's first line): off by
//! default, given in the app or by the owner's spoken yes, and off again on
//! any forget or a redo of setup.
//!
//! The yes is kept as room sounds and face expressions are (#5500,
//! `lamp_msgs::consent`): [`FILE`] in the consent folder,
//! `{"version":1,"seen_activity":"yes","at_unix_s":N,"by":"voice"|"app"}`,
//! present only while the yes stands (Fourteen's design on #6543). It is a
//! yes only when [`KEY`] is exactly `"yes"`; missing, unreadable, over
//! `lamp_msgs::consent::MOST_BYTES` (4096), not JSON or anything else is
//! no. It is read with `lamp_msgs::consent::read`, the same rule lamp-ears
//! keeps for `room-sounds.json`, and read again every [`READ_EVERY`], as
//! lamp-ears asks its yes again every 2 s: once the file is gone, the gate
//! closes at the next read.
//!
//! `lamp-seen` never writes the consent folder. The voice writes the file
//! (0640 `root:lampos`, whole, through a temp file and a rename), and the
//! app's yes goes through root's helper as room sounds' does; any forget and
//! a redo of setup remove it. `lamp-seen` only reads it, in the `lampos`
//! group.
//!
//! What the lamp last said back (phase 0's seventh line,
//! [`crate::said_back`]) is kept in `lamp-seen`'s own state folder
//! ([`Record`], [`SAID_FILE`]), never under `consent/`, so a restart says
//! neither again. A record left over after a removed yes means nothing,
//! because the yes is what the gate reads.
use lamp_msgs::consent::{self, Kept};
use serde_json::{Value, json};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The yes's key in its file.
pub const KEY: &str = "seen_activity";

/// The yes's file in the consent folder.
pub const FILE: &str = "seen-activity.json";

/// How often the kept yes is read again while `lamp-seen` runs.
pub const READ_EVERY: Duration = Duration::from_secs(2);

/// `lamp-seen`'s own state folder on the product (systemd's
/// `StateDirectory=lampos-seen`).
pub const STATE_FOLDER: &str = "/var/lib/lampos-seen";

/// The said-back record's file in the state folder: on, with the yes's
/// `at_unix_s` (`null` for a yes with no time) and whether the line said
/// named posture, or off.
pub const SAID_FILE: &str = "said-back.json";

/// Where the opt-in is kept.
pub trait Reader {
    /// What the kept opt-in says now. Anything missing or unreadable is
    /// [`Kept::default`]: no yes.
    fn read(&self) -> Kept;
}

/// What the lamp last said back of the opt-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// That it is on, for the kept yes with this `at_unix_s` (`None` for a
    /// yes with no time), and whether the line said named posture
    /// ([`crate::said_back::POSTURE_ON_LINE`]): posture sends no frame for
    /// a yes whose line did not name it.
    On {
        for_at_unix_s: Option<u64>,
        posture: bool,
    },
    /// That it is off.
    Off,
}

impl Said {
    /// The on said back for `kept`, naming posture or not.
    pub fn on_for(kept: &Kept, posture: bool) -> Self {
        Self::On {
            for_at_unix_s: kept.at_unix_s,
            posture,
        }
    }

    /// Whether this is the on said back for `kept`, whichever line it was.
    pub fn is_on_for(&self, kept: &Kept) -> bool {
        kept.yes
            && matches!(self, Self::On { for_at_unix_s, .. } if *for_at_unix_s == kept.at_unix_s)
    }

    /// Whether this is the on said back for `kept` with the line that named
    /// posture.
    pub fn names_posture_for(&self, kept: &Kept) -> bool {
        self.is_on_for(kept) && matches!(self, Self::On { posture: true, .. })
    }
}

/// Where what was last said back is kept.
pub trait Store {
    /// What was last said back, if anything. Missing or unreadable is none.
    fn said(&self) -> Option<Said>;
    /// Keep what was said back.
    fn set_said(&self, said: Said) -> io::Result<()>;
}

/// The opt-in, read from its file in a consent folder. Read only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
}

impl File {
    /// The product's: [`FILE`] in `lamp_msgs::consent::FOLDER`.
    pub fn product() -> Self {
        Self::in_folder(Path::new(consent::FOLDER))
    }

    /// [`FILE`] in `folder`.
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join(FILE),
        }
    }

    /// Whether the file is gone (not there at all, as against there and
    /// not a yes).
    pub fn gone(&self) -> bool {
        matches!(
            std::fs::symlink_metadata(&self.path),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        )
    }
}

impl Reader for File {
    fn read(&self) -> Kept {
        consent::read(&self.path, KEY)
    }
}

/// The said-back record, as a file in `lamp-seen`'s own state folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub path: PathBuf,
}

impl Record {
    /// The product's: [`SAID_FILE`] in [`STATE_FOLDER`].
    pub fn product() -> Self {
        Self::in_folder(Path::new(STATE_FOLDER))
    }

    /// [`SAID_FILE`] in `folder`.
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join(SAID_FILE),
        }
    }
}

/// The said-back file's bytes: `{"version":1,"said":"on","for_at_unix_s":N|null}`,
/// with `"posture":true` when the line named posture, or
/// `{"version":1,"said":"off"}`. A record with no `posture` (one kept
/// before posture) is a line that did not name it.
pub fn said_bytes(said: Said) -> Vec<u8> {
    let value = match said {
        Said::On {
            for_at_unix_s,
            posture: false,
        } => {
            json!({"version": 1, "said": "on", "for_at_unix_s": for_at_unix_s})
        }
        Said::On {
            for_at_unix_s,
            posture: true,
        } => {
            json!({"version": 1, "said": "on", "for_at_unix_s": for_at_unix_s, "posture": true})
        }
        Said::Off => json!({"version": 1, "said": "off"}),
    };
    serde_json::to_vec(&value).unwrap_or_default()
}

impl Store for Record {
    fn said(&self) -> Option<Said> {
        let file = std::fs::File::open(&self.path).ok()?;
        let mut text = String::new();
        file.take(consent::MOST_BYTES + 1)
            .read_to_string(&mut text)
            .ok()?;
        if text.len() as u64 > consent::MOST_BYTES {
            return None;
        }
        let Value::Object(value) = serde_json::from_str::<Value>(&text).ok()? else {
            return None;
        };
        // Only `true` names posture: anything else is the plain line.
        let posture = value.get("posture").and_then(Value::as_bool) == Some(true);
        match value.get("said")?.as_str()? {
            "off" => Some(Said::Off),
            "on" => match value.get("for_at_unix_s")? {
                Value::Null => Some(Said::On {
                    for_at_unix_s: None,
                    posture,
                }),
                at => at.as_u64().map(|at| Said::On {
                    for_at_unix_s: Some(at),
                    posture,
                }),
            },
            _ => None,
        }
    }

    /// Written whole: to a file beside it, then renamed over it.
    fn set_said(&self, said: Said) -> io::Result<()> {
        let mut next = self.path.clone().into_os_string();
        next.push(".next");
        let next = PathBuf::from(next);
        std::fs::write(&next, said_bytes(said))?;
        std::fs::rename(&next, &self.path)
    }
}

/// The opt-in as the gate takes it: a kept yes, and no forget since it was
/// given.
///
/// Forget everything (`memory/forgot`) turns the opt-in off (phase 0's
/// fifth line). The voice removes the kept yes; `lamp-seen` does not. This
/// latch holds it off in the running process from the forget on, however
/// slow that removal is: a yes whose `at_unix_s` is not later than the
/// forget is refused, until the file is gone ([`OptIn::gone`]) or replaced
/// by a later yes. A yes with no time cannot be shown to be later, so it
/// stays off while the latch holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptIn {
    /// When the last forget came, in Unix seconds, if one has since start
    /// and its file has not been seen gone since.
    forgot_at_unix_s: Option<u64>,
}

impl OptIn {
    /// A forget came at `now_unix_s`: the opt-in is off from now on until a
    /// later yes, or until the file is gone.
    pub fn forgot(&mut self, now_unix_s: u64) {
        self.forgot_at_unix_s = Some(
            self.forgot_at_unix_s
                .map_or(now_unix_s, |at| at.max(now_unix_s)),
        );
    }

    /// The file was read gone: the yes the forget refused is removed, so
    /// the latch has nothing left to hold.
    pub fn gone(&mut self) {
        self.forgot_at_unix_s = None;
    }

    /// Whether `kept` is a yes the gate takes.
    pub fn on(&self, kept: &Kept) -> bool {
        if !kept.yes {
            return false;
        }
        match self.forgot_at_unix_s {
            None => true,
            Some(forgot) => kept.at_unix_s.is_some_and(|given| given > forgot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("seen-opt-in-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn off_unless_a_kept_yes() {
        let folder = folder("off");
        let file = File::in_folder(&folder);
        let latch = OptIn::default();
        assert!(!latch.on(&file.read()), "missing");
        assert!(file.gone());
        for text in [
            "",
            "yes",
            "[1]",
            r#"{"seen_activity":"no"}"#,
            r#"{"seen_activity":true}"#,
            r#"{"seen_activity":"YES"}"#,
            r#"{"seen_activity":"yes "}"#,
            r#"{"face_expressions":"yes"}"#,
            r#"{"room_sounds":"yes"}"#,
        ] {
            std::fs::write(&file.path, text).unwrap();
            assert!(!latch.on(&file.read()), "{text}");
            assert!(!file.gone(), "{text}");
        }
        std::fs::write(&file.path, vec![b' '; consent::MOST_BYTES as usize + 1]).unwrap();
        assert!(!latch.on(&file.read()), "too long");
        // Unreadable: a folder where the file should be.
        std::fs::remove_file(&file.path).unwrap();
        std::fs::create_dir(&file.path).unwrap();
        assert!(!latch.on(&file.read()), "unreadable");
        std::fs::remove_dir(&file.path).unwrap();
        for by in [consent::BY_VOICE, consent::BY_APP] {
            std::fs::write(&file.path, consent::bytes(KEY, 7, by)).unwrap();
            let kept = file.read();
            assert!(latch.on(&kept), "a kept yes by {by}");
            assert_eq!(kept.at_unix_s, Some(7));
            assert_eq!(kept.by.as_deref(), Some(by));
        }
        // The shape the voice writes, word for word.
        std::fs::write(
            &file.path,
            r#"{"version":1,"seen_activity":"yes","at_unix_s":9,"by":"voice"}"#,
        )
        .unwrap();
        assert!(latch.on(&file.read()));
        // Once the file is gone, the next read is no.
        std::fs::remove_file(&file.path).unwrap();
        assert!(!latch.on(&file.read()));
        assert!(file.gone());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_product_reads_the_consent_folder_and_keeps_the_record_in_its_own() {
        assert_eq!(File::product().path, Path::new(consent::FOLDER).join(FILE));
        assert_eq!(
            File::product().path,
            Path::new("/var/lib/lampOS/consent/seen-activity.json")
        );
        assert_eq!(
            Record::product().path,
            Path::new(STATE_FOLDER).join(SAID_FILE)
        );
        assert!(!Record::product().path.starts_with(consent::FOLDER));
        assert_eq!(READ_EVERY, Duration::from_secs(2));
    }

    #[test]
    fn reading_never_writes_the_consent_folder() {
        let folder = folder("ro");
        let file = File::in_folder(&folder);
        std::fs::write(&file.path, consent::bytes(KEY, 7, consent::BY_APP)).unwrap();
        let before = std::fs::read(&file.path).unwrap();
        let mut latch = OptIn::default();
        latch.forgot(100);
        assert!(!latch.on(&file.read()));
        // The forget leaves the voice's file as it was, and adds nothing.
        assert_eq!(std::fs::read(&file.path).unwrap(), before);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_said_back_is_kept_in_the_state_folder() {
        let folder = folder("said");
        let record = Record::in_folder(&folder);
        assert_eq!(record.said(), None, "missing");
        for said in [
            Said::On {
                for_at_unix_s: Some(7),
                posture: false,
            },
            Said::On {
                for_at_unix_s: None,
                posture: false,
            },
            Said::On {
                for_at_unix_s: Some(7),
                posture: true,
            },
            Said::Off,
        ] {
            record.set_said(said).unwrap();
            assert_eq!(Record::in_folder(&folder).said(), Some(said));
        }
        for text in [
            "",
            "[1]",
            r#"{"said":"on","for_at_unix_s":"7"}"#,
            r#"{"said":"on"}"#,
            r#"{"said":"maybe"}"#,
            r#"{"for_at_unix_s":7}"#,
        ] {
            std::fs::write(&record.path, text).unwrap();
            assert_eq!(record.said(), None, "{text}");
        }
        let yes = |at| Kept {
            yes: true,
            by: None,
            at_unix_s: at,
        };
        // A record kept before posture: the plain line.
        std::fs::write(
            &record.path,
            r#"{"version":1,"said":"on","for_at_unix_s":7}"#,
        )
        .unwrap();
        assert_eq!(
            record.said(),
            Some(Said::On {
                for_at_unix_s: Some(7),
                posture: false
            })
        );
        let said = Said::On {
            for_at_unix_s: Some(7),
            posture: false,
        };
        assert!(said.is_on_for(&yes(Some(7))));
        assert!(!said.names_posture_for(&yes(Some(7))), "the plain line");
        let posture = Said::On {
            for_at_unix_s: Some(7),
            posture: true,
        };
        assert!(posture.is_on_for(&yes(Some(7))));
        assert!(posture.names_posture_for(&yes(Some(7))));
        assert!(!posture.names_posture_for(&yes(Some(8))), "another yes");
        assert!(!said.is_on_for(&yes(Some(8))), "another yes");
        assert!(!said.is_on_for(&yes(None)));
        assert!(!said.is_on_for(&Kept::default()), "no yes");
        assert!(!Said::Off.is_on_for(&yes(Some(7))));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_record_left_after_the_yes_was_removed_turns_nothing_on() {
        let folder = folder("left");
        let file = File::in_folder(&folder);
        let record = Record::in_folder(&folder);
        record
            .set_said(Said::On {
                for_at_unix_s: Some(7),
                posture: false,
            })
            .unwrap();
        assert!(!OptIn::default().on(&file.read()));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_forget_turns_it_off_until_a_later_yes_or_the_file_gone() {
        let yes_at = |at: Option<u64>| Kept {
            yes: true,
            by: Some("app".into()),
            at_unix_s: at,
        };
        let mut latch = OptIn::default();
        assert!(latch.on(&yes_at(Some(100))));
        latch.forgot(200);
        assert!(!latch.on(&yes_at(Some(100))), "given before the forget");
        assert!(!latch.on(&yes_at(Some(200))), "given in the same second");
        assert!(!latch.on(&yes_at(None)), "a yes with no time");
        assert!(latch.on(&yes_at(Some(201))), "given again after");
        // An earlier forget never moves it back.
        latch.forgot(150);
        assert!(!latch.on(&yes_at(Some(200))));
        assert!(!latch.on(&Kept::default()));
        // The file seen gone: the latch lets go, and the next yes counts.
        latch.gone();
        assert!(latch.on(&yes_at(Some(120))));
        assert!(latch.on(&yes_at(None)));
    }
}
