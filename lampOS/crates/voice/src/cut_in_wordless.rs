//! A cut-in that ended the lamp's answer with no words gets one short
//! question (the first builder on #4710, 2 Nov), on with
//! `LAMP_CUT_IN_WORDLESS_ASKS=on` (the product's, the memory owner's call
//! on 4 Nov).
//!
//! A cough, a beep or a knock over the lamp's answer can make it yield,
//! then commit as a cut-in: the answer is ended (`user_interrupted`) and
//! the line comes back with no words, so nothing is asked of the model and
//! the lamp went quiet mid-story, as if it had been told to. With the
//! switch, such a line is answered [`LINE`], a fixed line said in the
//! route's voice (in the owner's language when one is chosen), never the
//! model's, so the person who did speak can say it again.
//!
//! Only a cut-in that ended the answer: its `Start` came over the lamp's
//! audible answer (`over_the_lamps_voice`, sent only when the yield
//! committed). A yield that resumed sends no `Start`, so it stays silent; a
//! line in a busy room, or one with words, never gets it; a line that only
//! carries words cut short before it answers those instead. Asked at most
//! once in [`AGAIN_AFTER`]. The second route only.

/// The least time between two of its questions: a second knock that cuts
/// the question itself, or the next minute's, leaves the lamp quiet, never
/// asking again and again.
pub const AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
/// Where the second route's worker reads the switch.
pub const ENVIRONMENT: &str = "LAMP_CUT_IN_WORDLESS_ASKS";
/// What the lamp says when a cut-in ended its answer with no words: a
/// polite opener first, so the person hears that the lamp was cut off by a
/// sound, not that it simply stopped (the memory owner, 4 Nov: "'Did
/// someone say something?' is not enough"). "Um" is spelled as a word so
/// speech says it, and commas, never a dash, as the other rendered-ahead
/// lines have.
pub const LINE: &str = "Um, excuse me, did anyone say something?";

/// Whether the switch is on in this process's environment: `on` only.
pub fn on() -> bool {
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on_for(Some("on")));
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!on_for(off), "{off:?}");
        }
    }

    #[test]
    fn the_line_is_short_and_asks() {
        assert!(LINE.starts_with("Um, excuse me,"));
        assert!(LINE.ends_with('?'));
        assert!(!LINE.contains(['—', '–']));
        assert!(LINE.split_whitespace().count() <= 7);
    }
}
