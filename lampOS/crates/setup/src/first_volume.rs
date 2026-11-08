//! A new lamp's first volume (#5551, `LAMP_FIRST_VOLUME`, off unless set):
//! the speaker's "Speaker" softvol control, the one `lamp-actions` changes
//! for "louder" and "quieter", set to [`LEVEL`] once, as setup finishes and
//! before its marker starts the lamp, so the voice's first line is never at
//! the full 100 % a fresh control starts at.
//!
//! Setup is where "not yet set up" is known: it runs only on a lamp with no
//! marker, a new one or one just reset, and so a level the person chose
//! later, by voice, a button or the app, is never touched, by an update or
//! by anything else. Recovery's network-only run (`serve::NetworkOnly`)
//! never sets it. The level is then stored for that card alone
//! (`alsactl store allwinnerhdmi`), so a power cut before the first clean
//! shutdown still boots at it; the C-Media hardware pin (`lamp-cmedia-speaker`,
//! `factory-run.sh`) is a different control and left as it is.
//!
//! Best effort and bounded: a control not there yet, or a tool that fails
//! or hangs, is a line in the log, never a failed setup, and holds the
//! marker back at most [`SET_WITHIN`] plus [`STORE_WITHIN`].

use std::{io, time::Duration};

/// The switch, in `lamp-setup`'s unit.
pub const ENVIRONMENT: &str = "LAMP_FIRST_VOLUME";
/// The level, in points of 100: about 9 dB below full on the control's
/// -30 to 0 dB (`/etc/asound.conf`, `device_speaker`).
pub const LEVEL: u8 = 70;
/// The card and control the product's `lamp-actions` is given
/// (`--mixer-card`, `--mixer-control`); a test holds them equal.
pub const CARD: &str = "allwinnerhdmi";
pub const CONTROL: &str = "Speaker";
/// How long the level's write may take before it is given up.
pub const SET_WITHIN: Duration = Duration::from_millis(800);
/// How long storing it may take before it is given up.
pub const STORE_WITHIN: Duration = Duration::from_secs(2);

/// Whether the switch is on, as `var` gives it: `on` exactly.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// The level's write.
pub fn set_command() -> Vec<String> {
    [
        "/usr/bin/amixer",
        "-q",
        "-c",
        CARD,
        "sset",
        CONTROL,
        &format!("{LEVEL}%"),
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Its store, for this card alone, so the other cards' saved state stays.
pub fn store_command() -> Vec<String> {
    ["/usr/sbin/alsactl", "store", CARD]
        .map(str::to_owned)
        .to_vec()
}

/// Set the level, then store it; stored only once it is set. `run` is given
/// each command and its bound. Never fails setup: what went wrong is logged.
pub fn apply(
    mut run: impl FnMut(&[String], Duration) -> io::Result<()>,
    mut log: impl FnMut(&str),
) {
    if let Err(why) = run(&set_command(), SET_WITHIN) {
        log(&format!(
            "the speaker's first volume was not set ({why}); it stays as it was"
        ));
        return;
    }
    match run(&store_command(), STORE_WITHIN) {
        Ok(()) => log(&format!("the speaker's first volume is {LEVEL}%")),
        Err(why) => log(&format!(
            "the speaker's first volume is {LEVEL}%, not stored ({why}); a power cut before a clean shutdown may lose it"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_when_it_says_on() {
        assert!(on(Some("on")));
        for off in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some("on "),
            Some("1"),
        ] {
            assert!(!on(off), "{off:?}");
        }
    }

    #[test]
    fn it_sets_the_control_lamp_actions_changes_to_seventy_then_stores_that_card() {
        let mut ran = Vec::new();
        let mut lines = Vec::new();
        apply(
            |argv, most| {
                ran.push((argv.join(" "), most));
                Ok(())
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(
            ran,
            [
                (
                    "/usr/bin/amixer -q -c allwinnerhdmi sset Speaker 70%".to_owned(),
                    SET_WITHIN
                ),
                (
                    "/usr/sbin/alsactl store allwinnerhdmi".to_owned(),
                    STORE_WITHIN
                ),
            ]
        );
        assert_eq!(lines, ["the speaker's first volume is 70%"]);
    }

    #[test]
    fn a_level_that_cannot_be_set_is_not_stored_and_only_logged() {
        let mut ran = Vec::new();
        let mut lines = Vec::new();
        apply(
            |argv, _| {
                ran.push(argv[0].clone());
                Err(io::Error::other("amixer ended with exit status: 1"))
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(ran, ["/usr/bin/amixer"]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("not set"), "{lines:?}");
    }

    #[test]
    fn a_store_that_fails_still_leaves_the_level_set_and_says_so() {
        let mut lines = Vec::new();
        apply(
            |argv, _| {
                if argv[0].ends_with("alsactl") {
                    Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"))
                } else {
                    Ok(())
                }
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("70%, not stored"), "{lines:?}");
    }

    /// The control is the product `lamp-actions`' own, so "louder" and
    /// "quieter" start from this level and nothing else is changed.
    #[test]
    fn the_card_and_control_are_the_product_lamp_actions_ones() {
        let unit = include_str!("../../../deploy/product/lamp-actions.service");
        let start = unit
            .lines()
            .find(|line| line.starts_with("ExecStart="))
            .unwrap();
        assert!(start.contains(&format!("--mixer-card {CARD} ")), "{start}");
        assert!(
            start.contains(&format!("--mixer-control {CONTROL} ")),
            "{start}"
        );
    }
}
