//! A few emotion tags in the second route's speech
//! (`LAMP_SECOND_SPEECH_TAGS=on`, #2901; the lead's proposal on #1711, 22
//! Oct 12:4x). ElevenLabs reads `[laughs]` and its like as how a line
//! sounds; the chat is told a short list ([`NOTE`]), and each piece of a
//! reply is cut here into what the voice is given and the lamp's words.
//!
//! The voice is given only a listed tag, at most one a reply, and none when
//! it doesn't read them (OpenAI, Gemini, or an ElevenLabs model other than
//! [`READING_MODELS`]'s, which would say the tag as a word). Any other `[...]` is dropped. The
//! lamp's words, which go to the transcript, the thread, the own-words list
//! and the body, never carry a tag. A tag cut across two pieces ("[lau",
//! "ghs] ok") is held until it closes.

/// The tags the voice may be given, as the chat is told them.
pub const LISTED: [&str; 4] = ["[laughs]", "[sighs]", "[whispers]", "[excited]"];

/// Told after the character in spoken turns, with the switch on.
pub const NOTE: &str = " Your voice can read a few tags: [laughs], [sighs], [whispers] and [excited]. Use one rarely, only where a person would make that sound, never more than one in a reply, and no other words in square brackets.";

/// The ElevenLabs models that read the tags as how a line sounds: any
/// other says them as words (`eleven_flash_v2_5`, the route's default).
pub const READING_MODELS: [&str; 1] = ["eleven_v4_turbo"];

/// Whether ElevenLabs speaks each piece of a reply with a model that reads
/// tags: the voice's model (`tts_model`, or the provider's default) and,
/// with the speech socket on, the socket's own model too, which says every
/// piece of an answer whose socket opened.
pub fn read_by(model: &str, socket_model: Option<&str>) -> bool {
    READING_MODELS.contains(&model)
        && socket_model.is_none_or(|socket| READING_MODELS.contains(&socket))
}

/// The longest unclosed `[...]` held for its `]` in the next piece; a longer
/// one is no tag, and its `[` is dropped.
pub(crate) const HELD_MOST: usize = 24;

/// One reply's tags, piece by piece.
#[derive(Debug, Default)]
pub struct Tags {
    /// The voice reads tags (ElevenLabs).
    voiced: bool,
    /// An unclosed `[...]` from the end of the last piece.
    held: String,
    /// A tag kept from a piece with no words, said with the next one.
    lead: String,
    /// Tags given to the voice, and dropped.
    pub kept: usize,
    pub dropped: usize,
}

impl Tags {
    pub fn new(voiced: bool) -> Self {
        Self {
            voiced,
            ..Self::default()
        }
    }

    /// A piece of the reply: what the voice is given, and the lamp's words.
    /// Both empty while it is only a tag still open.
    pub fn piece(&mut self, piece: &str) -> (String, String) {
        let mut text = std::mem::take(&mut self.held);
        text.push_str(piece);
        let mut spoken = String::with_capacity(text.len());
        let mut words = String::with_capacity(text.len());
        let mut cut = false;
        let mut rest = text.as_str();
        while let Some(open) = rest.find('[') {
            spoken.push_str(&rest[..open]);
            words.push_str(&rest[..open]);
            cut = true;
            let after = &rest[open..];
            let Some(close) = after.find(']') else {
                if after.len() <= HELD_MOST {
                    self.held = after.to_owned();
                    rest = "";
                } else {
                    // No tag: its `[` alone is dropped.
                    rest = &after[1..];
                }
                break;
            };
            let tag = after[..=close].to_ascii_lowercase();
            if self.voiced && self.kept == 0 && LISTED.contains(&tag.as_str()) {
                self.kept += 1;
                spoken.push_str(&tag);
            } else {
                self.dropped += 1;
            }
            rest = &after[close + 1..];
        }
        spoken.push_str(rest);
        words.push_str(rest);
        if cut {
            spoken = spaced(&spoken);
            words = spaced(&words);
        }
        if !words.chars().any(char::is_alphanumeric) {
            // A kept tag waits for the words it is said with.
            if spoken.contains('[') {
                self.lead = spoken.trim().to_owned();
            }
            return (String::new(), String::new());
        }
        if !self.lead.is_empty() {
            spoken = format!("{} {}", std::mem::take(&mut self.lead), spoken.trim_start());
        }
        (spoken, words)
    }

    /// The reply is over: a tag still held or never said with words was
    /// never given to the voice.
    pub fn finish(&mut self) {
        if !std::mem::take(&mut self.lead).is_empty() {
            self.kept -= 1;
            self.dropped += 1;
        }
        if !std::mem::take(&mut self.held).is_empty() {
            self.dropped += 1;
        }
    }
}

/// `text` with the space a tag left closed up: one space between words,
/// none at either end.
fn spaced(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(tags: &mut Tags, piece: &str) -> (String, String) {
        tags.piece(piece)
    }

    #[test]
    fn only_a_model_that_reads_tags_is_given_them() {
        assert!(read_by("eleven_v4_turbo", None));
        assert!(read_by("eleven_v4_turbo", Some("eleven_v4_turbo")));
        // The route's default model, and a socket on the default model,
        // would say "laughs" as a word.
        assert!(!read_by("eleven_flash_v2_5", None));
        assert!(!read_by("eleven_v4_turbo", Some("eleven_flash_v2_5")));
    }

    #[test]
    fn a_listed_tag_is_given_to_the_voice_and_never_in_the_words() {
        let mut tags = Tags::new(true);
        assert_eq!(
            said(&mut tags, "[laughs] That's a good one."),
            (
                "[laughs] That's a good one.".into(),
                "That's a good one.".into()
            )
        );
        assert_eq!((tags.kept, tags.dropped), (1, 0));
    }

    #[test]
    fn an_unlisted_tag_is_dropped() {
        let mut tags = Tags::new(true);
        assert_eq!(
            said(&mut tags, "Well [clears throat] maybe."),
            ("Well maybe.".into(), "Well maybe.".into())
        );
        assert_eq!((tags.kept, tags.dropped), (0, 1));
    }

    #[test]
    fn two_tags_in_a_reply_are_cut_to_one() {
        let mut tags = Tags::new(true);
        assert_eq!(said(&mut tags, "[sighs] Long day.").0, "[sighs] Long day.");
        assert_eq!(said(&mut tags, "[laughs] Still.").0, "Still.");
        assert_eq!(said(&mut tags, "Ok [whispers] [excited] go.").0, "Ok go.");
        assert_eq!((tags.kept, tags.dropped), (1, 3));
    }

    #[test]
    fn a_voice_that_reads_no_tags_is_given_none() {
        let mut tags = Tags::new(false);
        assert_eq!(
            said(&mut tags, "[laughs] Fine."),
            ("Fine.".into(), "Fine.".into())
        );
        assert_eq!((tags.kept, tags.dropped), (0, 1));
    }

    #[test]
    fn a_tag_cut_across_pieces_is_held_until_it_closes() {
        let mut tags = Tags::new(true);
        assert_eq!(said(&mut tags, "[lau"), (String::new(), String::new()));
        assert_eq!(
            said(&mut tags, "ghs] ok"),
            ("[laughs] ok".into(), "ok".into())
        );
        // A tag alone waits for the next words.
        let mut tags = Tags::new(true);
        assert_eq!(said(&mut tags, "[sighs]"), (String::new(), String::new()));
        assert_eq!(
            said(&mut tags, "Fine."),
            ("[sighs] Fine.".into(), "Fine.".into())
        );
        assert_eq!((tags.kept, tags.dropped), (1, 0));
    }

    #[test]
    fn what_is_still_held_at_the_end_is_never_said() {
        let mut tags = Tags::new(true);
        assert_eq!(said(&mut tags, "Bye. [lau").1, "Bye.");
        tags.finish();
        assert_eq!((tags.kept, tags.dropped), (0, 1));
        let mut tags = Tags::new(true);
        said(&mut tags, "[laughs]");
        tags.finish();
        assert_eq!((tags.kept, tags.dropped), (0, 1));
        // A long `[` with no `]` is no tag: its words are said.
        let mut tags = Tags::new(true);
        let long = "see [this is far too long to be a tag at all";
        assert_eq!(
            said(&mut tags, long).1,
            "see this is far too long to be a tag at all"
        );
    }

    #[test]
    fn a_piece_without_brackets_is_unchanged() {
        let mut tags = Tags::new(true);
        let piece = "It's four,  more or less.";
        assert_eq!(said(&mut tags, piece), (piece.into(), piece.into()));
    }
}
