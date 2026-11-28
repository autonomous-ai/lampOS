//! Phonon, the voice that runs on the lamp (#8754, `docs/phonon.md`).
//!
//! `lamp-phonon serve` is one process with one model: Gradium's Phonon 31m,
//! English only, Apache 2.0. The voice asks it for a sentence at a time on
//! [`lamp_msgs::phonon::SOCKET`] and plays the 24 kHz samples as they come;
//! closing the connection (a barge-in, a stop) stops the sentence where it
//! is. Nothing it does needs a key or the cloud, once the model is kept.
//!
//! - [`fetch`]: the model and voices, fetched once and kept only when each
//!   file's size and SHA-256 match the pins.
//! - [`engine`]: the model behind [`engine::Speaker`], which the tests
//!   replace.
//! - [`serve`]: the socket, one sentence at a time.
pub mod cores;
pub mod engine;
pub mod fetch;
pub mod serve;

/// The service's own state: the model and voices it fetched and checked.
pub const STATE: &str = "/var/lib/lampos-phonon";

/// The threads the model's tensor work uses (`LAMP_PHONON_THREADS`, 1 to
/// 4). One by default: on lamp-361f (2 Cortex-A76 and 6 Cortex-A55),
/// pinned to the two A76 cores, one thread ran at 0.80 to 0.86 of real
/// time and two at 1.11 to 1.19, as the model's own two generation
/// threads (flow LM and decoder) already take both cores.
pub const THREADS_ENVIRONMENT: &str = "LAMP_PHONON_THREADS";
/// [`THREADS_ENVIRONMENT`]'s default.
pub const THREADS: usize = 1;

/// The thread count `value` asks for, within 1 to 4; the default otherwise.
pub fn threads(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.trim().parse().ok())
        .filter(|threads| (1..=4).contains(threads))
        .unwrap_or(THREADS)
}

/// The person's choice as its credential's text gives it: the voice's name
/// when it is one offered, else `None` (not chosen, or nothing this build
/// offers).
pub fn chosen(text: Option<&str>) -> Option<lamp_msgs::phonon::Voice> {
    text.and_then(|text| lamp_msgs::phonon::voice(text.lines().next().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_are_one_unless_one_to_four_is_asked() {
        assert_eq!(threads(None), 1);
        assert_eq!(threads(Some("2")), 2);
        assert_eq!(threads(Some(" 4 ")), 4);
        for value in ["0", "5", "-1", "two", ""] {
            assert_eq!(threads(Some(value)), 1, "{value}");
        }
    }

    #[test]
    fn the_choice_is_a_voice_offered_or_nothing() {
        assert_eq!(chosen(Some("Toby\n")).map(|v| v.name), Some("Toby"));
        assert_eq!(chosen(None), None);
        assert_eq!(chosen(Some("")), None);
        assert_eq!(chosen(Some("Kore\n")), None);
    }
}
