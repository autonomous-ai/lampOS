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

/// The longest `\…\`, `<…>` or `[…]` span [`Stage`] holds for its close,
/// in characters with its opener. Past it the span is no markup: it is said
/// as plain words without its opener, so a stray `[` can never swallow a
/// long answer, and a later stray `]` then drops only itself.
pub const STAGE_HELD_MOST: usize = 200;

/// Tool text, markup and stage directions taken out of one round of a
/// reply, piece by piece, before anything else reads it: what is spoken,
/// the lamp's words, the thread and the chat's own history all get what is
/// left (lamp-29e7, 29e7 item 5: `\light_dimmer\</antml> The light's…` and
/// `…soft warm white]` were kept as said). Always on, every voice.
///
/// - `\word\`: a backslash followed by a letter, digit or `_` opens a span
///   the next backslash closes. A backslash before anything else is a
///   character.
/// - `<tag>` and `</tag>`: a `<` followed by a letter, `/` or `!` opens a span
///   the next `>` closes; `3 < 5` is words.
/// - `[…]`: dropped whole, nested brackets counted. With `keep_listed` (the
///   speech tags switch, [`Tags`] after this) a [`LISTED`] tag is left in.
/// - A `]` with no `[` open drops only itself. What came before it was
///   already passed on toward the voice: holding every word in case a `]`
///   follows would delay all speech.
/// - An opener still unclosed at the round's end drops what follows it,
///   at most [`STAGE_HELD_MOST`] characters (longer was already said).
/// - Where a span was taken out, the whitespace around it closes to one
///   space, none at the reply's start or before `.`, `,`, `!`, `?`, `;`,
///   `:`, `…` or `)`. Elsewhere spaces are as written. Whitespace at a
///   piece's end goes on with it, so a sentence's end is never held back
///   for the next piece, unless a span was just taken out before it.
// ponytail: a space passed on at a piece's end can't be taken back, so
// "Fine " then "[smiles]." is said "Fine ." (one stray space); holding it
// would delay every sentence ending a piece.
#[derive(Debug, Default)]
pub struct Stage {
    keep_listed: bool,
    /// The span being taken out, its opener first.
    span: String,
    span_chars: usize,
    depth: usize,
    /// A `<` or `\` at a piece's end, waiting to see what follows it.
    maybe: Option<char>,
    /// Whitespace not yet passed on.
    space: String,
    /// A span was taken out since the last character passed on.
    cut: bool,
    /// Something other than whitespace was passed on this round.
    emitted: bool,
    /// The last thing passed on was whitespace.
    after_space: bool,
}

impl Stage {
    pub fn new(keep_listed: bool) -> Self {
        Self {
            keep_listed,
            ..Self::default()
        }
    }

    /// A piece of the round: what is left of it to pass on.
    pub fn piece(&mut self, piece: &str) -> String {
        let mut out = String::with_capacity(piece.len());
        for c in piece.chars() {
            self.char(c, &mut out);
        }
        if !self.cut && !self.space.is_empty() {
            out.push_str(&std::mem::take(&mut self.space));
            self.after_space = true;
        }
        out
    }

    /// The round is over: a `<` or `\` left waiting is a character; a span
    /// still open, and trailing whitespace, are dropped. Ready for the next
    /// round.
    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        if let Some(c) = self.maybe.take() {
            self.emit(c, &mut out);
        }
        *self = Self::new(self.keep_listed);
        out
    }

    fn char(&mut self, c: char, out: &mut String) {
        if let Some(opener) = self.maybe.take() {
            let opens = match opener {
                '<' => c.is_ascii_alphabetic() || c == '/' || c == '!',
                _ => c.is_alphanumeric() || c == '_',
            };
            if opens {
                self.open(opener);
            } else {
                self.emit(opener, out);
            }
        }
        if let Some(opener) = self.span.chars().next() {
            self.span.push(c);
            self.span_chars += 1;
            let closed = match (opener, c) {
                ('[', '[') => {
                    self.depth += 1;
                    false
                }
                ('[', ']') => {
                    self.depth -= 1;
                    self.depth == 0
                }
                ('<', '>') | ('\\', '\\') => true,
                _ => false,
            };
            if closed {
                let span = std::mem::take(&mut self.span);
                if opener == '['
                    && self.keep_listed
                    && LISTED.contains(&span.to_ascii_lowercase().as_str())
                {
                    for c in span.chars() {
                        self.emit(c, out);
                    }
                } else {
                    self.cut = true;
                }
            } else if self.span_chars > STAGE_HELD_MOST {
                // No markup this long: its words are said, its opener not.
                let span = std::mem::take(&mut self.span);
                for c in span.chars().skip(1) {
                    self.char(c, out);
                }
            }
            return;
        }
        match c {
            '[' => self.open('['),
            '<' | '\\' => self.maybe = Some(c),
            ']' => self.cut = true,
            c if c.is_whitespace() => self.space.push(c),
            c => self.emit(c, out),
        }
    }

    fn open(&mut self, opener: char) {
        self.span.push(opener);
        self.span_chars = 1;
        self.depth = 1;
    }

    fn emit(&mut self, c: char, out: &mut String) {
        let space = std::mem::take(&mut self.space);
        if !self.cut {
            out.push_str(&space);
        } else if self.emitted
            && !self.after_space
            && !matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '…' | ')')
        {
            out.push(' ');
        }
        out.push(c);
        self.cut = false;
        self.emitted = true;
        self.after_space = false;
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

    /// The pieces through one round's [`Stage`], and its end.
    fn staged(keep_listed: bool, pieces: &[&str]) -> String {
        let mut stage = Stage::new(keep_listed);
        let mut out: String = pieces.iter().map(|piece| stage.piece(piece)).collect();
        out.push_str(&stage.finish());
        out
    }

    /// 29e7 item 5's first line, as the lamp kept it.
    const TOOL_LINE: &str = "\\light_dimmer\\</antml> The light's already at its dimmest…";
    /// 29e7 item 5's second line: the end of a `[` opened in an earlier
    /// piece, then the answer.
    const STAGE_LINE: &str = "…lights warm up fromblue into a soft warm white]";
    const STAGE_ANSWER: &str = " How's that — warm white instead of blue";

    #[test]
    fn tool_text_and_tags_before_an_answer_are_not_said() {
        assert_eq!(
            staged(false, &[TOOL_LINE]),
            "The light's already at its dimmest…"
        );
    }

    #[test]
    fn a_stage_direction_opened_in_an_earlier_piece_is_not_said() {
        assert_eq!(
            staged(false, &["[The ", STAGE_LINE, STAGE_ANSWER]),
            "How's that — warm white instead of blue"
        );
        assert_eq!(
            staged(false, &["Sure. [The ", STAGE_LINE, STAGE_ANSWER]),
            "Sure. How's that — warm white instead of blue"
        );
    }

    #[test]
    fn a_reply_cut_into_two_pieces_anywhere_is_staged_as_whole() {
        for (whole, said) in [
            (TOOL_LINE.to_owned(), "The light's already at its dimmest…"),
            (
                format!("[The {STAGE_LINE}{STAGE_ANSWER}"),
                "How's that — warm white instead of blue",
            ),
            (
                "Okay <b>so</b> the \\set_timer\\ timer [beeps] is set [for ten minutes] now."
                    .to_owned(),
                "Okay so the timer is set now.",
            ),
        ] {
            assert_eq!(staged(false, &[&whole]), said);
            for (at, _) in whole.char_indices() {
                assert_eq!(
                    staged(false, &[&whole[..at], &whole[at..]]),
                    said,
                    "cut at {at}"
                );
            }
            let each: Vec<String> = whole.chars().map(String::from).collect();
            let each: Vec<&str> = each.iter().map(String::as_str).collect();
            assert_eq!(staged(false, &each), said, "a character a piece");
        }
    }

    #[test]
    fn nested_stray_and_odd_markup() {
        // Nested brackets close with their outer one.
        assert_eq!(staged(false, &["A [b [c] d] e."]), "A e.");
        // A stray `]` drops only itself.
        assert_eq!(staged(false, &["Done] and dusted."]), "Done and dusted.");
        // No space is left before punctuation, none at the start.
        assert_eq!(staged(false, &["  [nods] Fine [smiles]."]), "Fine.");
        // Whitespace ending a piece goes on with it: one space, not two.
        assert_eq!(staged(false, &["Fine ", "[smiles] ok."]), "Fine ok.");
        assert_eq!(staged(false, &["Fine. ", "Next."]), "Fine. Next.");
        // A `<` or backslash before no word is a character.
        assert_eq!(
            staged(false, &["3 < 5, and 2 <", " 4 \\ ok"]),
            "3 < 5, and 2 < 4 \\ ok"
        );
        assert_eq!(staged(false, &["Ends with <"]), "Ends with <");
        // A span still open at the end is dropped, the rest kept (the
        // space before it went on with its piece; sentences are trimmed).
        assert_eq!(staged(false, &["Fine. [nods slowly"]), "Fine. ");
        assert_eq!(staged(false, &["Fine. <thinking"]), "Fine. ");
        assert_eq!(staged(false, &["Fine. \\light_dim"]), "Fine. ");
        assert_eq!(staged(false, &["Fine. </ant"]), "Fine. ");
        // A listed tag is left for [`Tags`] only with the switch.
        assert_eq!(
            staged(true, &["[laughs] Ha. [clears throat] Ok."]),
            "[laughs] Ha. Ok."
        );
        assert_eq!(staged(false, &["[laughs] Ha."]), "Ha.");
    }

    #[test]
    fn plain_words_are_untouched() {
        for words in [
            "It's four,  more or less.",
            " Leading space and a trailing one",
            "Chào bạn, hôm nay trời đẹp quá! Đèn đã ở mức thấp nhất rồi.",
            "Mix 3 < 4 and a > b, a \\ b too.",
        ] {
            assert_eq!(staged(false, &[words]), words.trim_end());
        }
    }

    #[test]
    fn an_unclosed_opener_past_the_cap_is_said_without_it() {
        let long = "word ".repeat(50);
        assert!(long.chars().count() > STAGE_HELD_MOST);
        let said = staged(false, &["Okay [", &long, "and more] then."]);
        assert_eq!(said, format!("Okay {long}and more then."));
        // Under the cap at the end: dropped.
        let short = "word ".repeat(30);
        assert_eq!(staged(false, &["Okay [", &short]), "Okay ");
    }

    /// A sentence ending a piece goes on with its space at once: the
    /// sentence splitter needs that space to end it, and a stream that
    /// pauses after it (a slow chat, a call coming) must not hold it back.
    /// Held, `a_turn_spoken_over_stops_its_chat_and_speech_and_sends_nothing_more`
    /// never spoke and hung.
    #[test]
    fn a_pieces_last_sentence_is_never_held_for_the_next_piece() {
        let mut stage = Stage::new(false);
        assert_eq!(stage.piece("Long story. "), "Long story. ");
        assert_eq!(stage.piece("Then [pause] "), "Then");
        // Only around a span just taken out does the space wait.
        assert_eq!(stage.piece("more."), " more.");
    }

    #[test]
    fn each_round_begins_clean() {
        let mut stage = Stage::new(false);
        assert_eq!(stage.piece("Yes. [nods"), "Yes. ");
        assert_eq!(stage.finish(), "");
        assert_eq!(stage.piece("Next round."), "Next round.");
    }
}
