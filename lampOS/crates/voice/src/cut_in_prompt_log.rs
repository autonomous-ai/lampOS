//! A debug switch for #6218: what the chat is asked on a cut-in, a line
//! begun over the lamp's audible answer (`over_the_lamps_voice`), with
//! `LAMP_CUT_IN_PROMPT_LOG=on`. Off unless set, and in no product unit.
//!
//! In a long conversation (requests 14 to 26, 19 to 26 messages, about 14k
//! prompt tokens) an ask said over the lamp's answer ("Hold on, Lamp, make
//! the light a little warmer") got no call and a short stop reply, where
//! early in a conversation the same ask calls `light_warmer`. Step 1 changes
//! nothing the lamp does: each chat request of such a turn, every round,
//! writes one journal block with the request, the round, the message count,
//! the tool count and the request's bytes, then each message's index, role
//! and the first [`PREFIX_CHARS`] characters of its words, newlines shown as
//! `⏎`, cut on a character boundary.
//!
//! It logs words people said, so it is for diagnosis on test lamps only,
//! and the block says so. It never writes while the privacy switch is
//! closed or the microphone is muted (the worker's authority: the camera or
//! the microphone not allowed, [`crate::second_provider::Engine::privacy`]).
//! A sign-in's code and email are taken out as the thread's file takes them
//! out ([`crate::sign_in::scrub`]), and any other run of four or more digits
//! shows only as `####`.

use crate::second_provider::net::Message;

/// Where the second route's worker reads the switch.
pub const ENVIRONMENT: &str = "LAMP_CUT_IN_PROMPT_LOG";
/// How many characters of each message's words the block shows.
pub const PREFIX_CHARS: usize = 80;
/// What a run of four or more digits shows as: a code's shape is not kept.
const DIGITS: &str = "####";
/// The fewest digits in a row that are masked.
const MASKED_FROM: usize = 4;

/// Whether the switch is on in this process's environment: `on` only.
pub fn on() -> bool {
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

/// One chat request of a turn, as the block tells it.
pub struct Asked<'a> {
    pub request: u64,
    pub round: usize,
    pub messages: &'a [Message],
    /// The request's bytes and its tool count, when the services can say.
    pub measured: Option<(usize, usize)>,
}

/// The block for `asked`, or nothing: with the switch off, for a request
/// not begun over the lamp's answer, or while the lamp is private or muted.
pub fn told(on: bool, cut_in: bool, private: bool, asked: &Asked<'_>) -> Option<String> {
    (on && cut_in && !private).then(|| block(asked))
}

/// The journal block for one chat request of a cut-in turn.
pub fn block(asked: &Asked<'_>) -> String {
    let (bytes, tools) = match asked.measured {
        Some((bytes, tools)) => (bytes.to_string(), tools.to_string()),
        None => ("unknown".to_owned(), "unknown".to_owned()),
    };
    let mut block = format!(
        "lamp-voice second: request {} chat round {}: a cut-in's prompt, {} messages, {tools} tools, {bytes} bytes; each message's first {PREFIX_CHARS} characters, the words people said among them ({ENVIRONMENT}, a debug switch for test lamps only)",
        asked.request,
        asked.round,
        asked.messages.len(),
    );
    for (index, message) in asked.messages.iter().enumerate() {
        block.push_str(&format!("\n  {index} {}", message.role));
        if let Some(words) = message.words() {
            block.push_str(&format!(
                " ({} chars): {}",
                words.chars().count(),
                prefix(words)
            ));
        }
        let calls: Vec<&str> = message
            .tool_calls
            .iter()
            .filter_map(|call| {
                call.pointer("/function/name")
                    .and_then(|name| name.as_str())
            })
            .collect();
        if !calls.is_empty() {
            block.push_str(&format!(" [calls {}]", calls.join(", ")));
        }
    }
    block
}

/// The first [`PREFIX_CHARS`] characters of `words`, a sign-in's code and
/// email and any run of four or more digits taken out first, each line
/// break shown as `⏎`.
pub fn prefix(words: &str) -> String {
    masked(&crate::sign_in::scrub(words))
        .replace("\r\n", "\n")
        .chars()
        .take(PREFIX_CHARS)
        .map(|c| if matches!(c, '\n' | '\r') { '⏎' } else { c })
        .collect()
}

/// `text` with each run of [`MASKED_FROM`] or more ASCII digits as
/// [`DIGITS`].
fn masked(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, kept: &mut String| {
        if run.len() >= MASKED_FROM {
            kept.push_str(DIGITS);
        } else {
            kept.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut run, &mut kept);
            kept.push(c);
        }
    }
    flush(&mut run, &mut kept);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(messages: &[Message]) -> Asked<'_> {
        Asked {
            request: 21,
            round: 1,
            messages,
            measured: Some((14_210, 61)),
        }
    }

    #[test]
    fn the_switch_is_on_only_as_on() {
        assert!(on_for(Some("on")));
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!on_for(value), "{value:?}");
        }
    }

    #[test]
    fn off_there_is_no_block() {
        let messages = [Message::user(
            "Hold on, Lamp, make the light a little warmer",
        )];
        assert_eq!(told(false, true, false, &asked(&messages)), None);
    }

    #[test]
    fn a_request_not_begun_over_the_lamp_has_no_block() {
        let messages = [Message::user("Make the light a little warmer")];
        assert_eq!(told(true, false, false, &asked(&messages)), None);
    }

    #[test]
    fn private_or_muted_there_is_no_block() {
        let messages = [Message::user(
            "Hold on, Lamp, make the light a little warmer",
        )];
        assert_eq!(told(true, true, true, &asked(&messages)), None);
    }

    #[test]
    fn a_cut_in_logs_each_messages_role_and_prefix_and_says_it_is_a_debug_switch() {
        let messages = [
            Message::system("You are Lamp."),
            Message::user("Tell me a story."),
            Message::assistant("Once upon a time"),
            Message::user("Hold on, Lamp, make the light a little warmer"),
        ];
        let block = told(true, true, false, &asked(&messages)).unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines.len(), 5, "{block}");
        assert!(lines[0].starts_with("lamp-voice second: request 21 chat round 1: "));
        assert!(
            lines[0].contains("4 messages, 61 tools, 14210 bytes"),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains("LAMP_CUT_IN_PROMPT_LOG, a debug switch"),
            "{}",
            lines[0]
        );
        assert_eq!(lines[1], "  0 system (13 chars): You are Lamp.");
        assert_eq!(lines[2], "  1 user (16 chars): Tell me a story.");
        assert_eq!(lines[3], "  2 assistant (16 chars): Once upon a time");
        assert_eq!(
            lines[4],
            "  3 user (45 chars): Hold on, Lamp, make the light a little warmer"
        );
    }

    #[test]
    fn a_long_message_is_cut_at_80_characters_on_a_character_boundary() {
        // Multi-byte letters across the cut: 79 ASCII, then accented ones.
        let words = format!("{}{}", "a".repeat(79), "đèn ấm hơn");
        let shown = prefix(&words);
        assert_eq!(shown.chars().count(), PREFIX_CHARS);
        assert_eq!(shown, format!("{}đ", "a".repeat(79)));
        let long = "x".repeat(500);
        let messages = [Message::system(long)];
        let block = block(&asked(&messages));
        assert!(
            block.ends_with(&format!("  0 system (500 chars): {}", "x".repeat(80))),
            "{block}"
        );
    }

    #[test]
    fn newlines_are_shown() {
        assert_eq!(prefix("one\ntwo\r\nthree\rfour"), "one⏎two⏎three⏎four");
    }

    #[test]
    fn a_codes_shape_is_masked_and_short_numbers_stay() {
        assert_eq!(
            prefix("my code is 482913, at 7 pm, room 12, pin 1234"),
            "my code is ####, at 7 pm, room 12, pin ####"
        );
        // Masked before the cut, so a code across it shows none of its digits.
        let words = format!("{}123456", "a".repeat(78));
        assert_eq!(prefix(&words), format!("{}##", "a".repeat(78)));
    }

    #[test]
    fn a_call_without_words_shows_its_names_and_unknown_measures_say_so() {
        let call = crate::second_provider::net::ToolCall {
            id: "c1".to_owned(),
            name: "light_warmer".to_owned(),
            args: serde_json::json!({}),
            extra: None,
        };
        let messages = [
            Message::assistant_calls(None, &[call]),
            Message::tool("c1", "done"),
        ];
        let block = block(&Asked {
            measured: None,
            ..asked(&messages)
        });
        assert!(
            block.contains("2 messages, unknown tools, unknown bytes"),
            "{block}"
        );
        assert!(
            block.contains("\n  0 assistant [calls light_warmer]"),
            "{block}"
        );
        assert!(block.ends_with("\n  1 tool (4 chars): done"), "{block}");
    }
}
