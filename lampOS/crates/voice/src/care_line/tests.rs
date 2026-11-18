use super::*;

/// Every direct and indirect line, and one said in passing, is caught,
/// in English and Vietnamese (#7689: a miss isn't acceptable).
#[test]
fn direct_indirect_and_in_passing_lines_are_caught() {
    for heard in [
        // Direct.
        "I want to kill myself.",
        "I'm thinking about ending my life",
        "I've been having suicidal thoughts",
        "Sometimes I hurt myself",
        "I just want to die",
        "I'm going to end it all",
        "I think about suicide a lot",
        // Indirect.
        "I don't want to be here anymore.",
        "Everyone would be better off without me",
        "I don't want to wake up tomorrow",
        "I wish I wasn't alive",
        "There's no reason to live",
        "I can't go on like this",
        "I can’t do this anymore",
        "I won't be around much longer",
        // In passing.
        "Set a timer for ten minutes, I don't want to live like this anyway",
        "Whatever, I wish I was dead, what's the weather",
        // Vietnamese.
        "Tôi muốn chết",
        "Mình không muốn sống nữa",
        "Em đã nghĩ đến chuyện tự tử",
        "Tôi chán sống lắm rồi",
        "Không có mình sẽ tốt hơn cho mọi người",
        "Mình muốn tự làm hại bản thân",
        "toi muon chet",
    ] {
        assert!(said_harm(heard), "{heard}");
    }
}

/// Jokes, lyrics and news lean to firing: a gentle line that wasn't needed
/// is acceptable (#7689), and the model judges them too.
#[test]
fn jokes_lyrics_and_news_lean_to_firing() {
    for heard in [
        "This traffic makes me want to die, haha",
        "The news said suicide rates went up",
        "Play that song, I want to die young",
    ] {
        assert!(said_harm(heard), "{heard}");
    }
}

/// Ordinary lines with near words are not caught: the words must stand as
/// the phrase, on word boundaries.
#[test]
fn ordinary_lines_are_not_caught() {
    for heard in [
        "What time is it?",
        "Kill the lights",
        "I could kill for a coffee",
        "This battery is about to die",
        "I'm dying to see that movie",
        "Tell me a joke",
        "Turn it up, I love this song",
        "Từ từ thôi",
        "tu tu thoi",
        "Hôm nay trời đẹp quá",
        "Bật đèn lên",
        "I hurt my knee",
        "My suicidaly long day", // not a word boundary
    ] {
        assert!(!said_harm(heard), "{heard}");
    }
}

/// The switch: `on` exactly.
#[test]
fn the_switch_is_on_exactly() {
    assert!(on(Some("on")));
    for value in [None, Some(""), Some("ON"), Some("1"), Some("off")] {
        assert!(!on(value), "{value:?}");
    }
}

/// Each region's line from the shipped file (the safety read, #7689): a
/// VN lamp with an English speaker gets VN's line in English, a region
/// with no entry or none known gets the default, and a known region is
/// never given the default.
#[test]
fn each_region_gets_its_own_line_in_the_language_or_english() {
    let lines = CareLines::shipped();
    assert_eq!(
        lines.line(Some("US"), "en"),
        "You can call or text 988, the Suicide and Crisis Lifeline, any time."
    );
    assert!(lines.line(Some("VN"), "vi").contains("115"));
    let vn_en = lines.line(Some("VN"), "en");
    assert!(vn_en.contains("115") && vn_en.is_ascii(), "{vn_en}");
    assert!(
        lines.line(Some(" vn\n"), "ja").contains("115"),
        "English for a language it has not"
    );
    assert!(lines.line(Some("CN"), "zh").contains("12356"));
    assert!(!lines.line(Some("CN"), "en").contains("any time"));
    for region in [Some("DE"), None] {
        assert_eq!(
            lines.line(region, "en"),
            "Please reach out to someone you trust, or your local emergency number, right now."
        );
    }
    assert!(lines.line(None, "vi").contains("khẩn cấp"));
    assert!(!lines.line(Some("US"), "vi").contains("local emergency"));
}

/// A broken or missing copy falls back to the shipped lines, and a file
/// with an empty line or no English default is broken.
#[test]
fn a_broken_file_falls_back_to_the_shipped_lines() {
    let us = "You can call or text 988, the Suicide and Crisis Lifeline, any time.";
    for text in [
        None,
        Some("not json"),
        Some(r#"{"regions":{},"default":{"lines":{"vi":"x"}}}"#),
        Some(r#"{"regions":{"US":{"lines":{"en":" "}}},"default":{"lines":{"en":"x"}}}"#),
    ] {
        assert_eq!(
            CareLines::or_shipped(text).line(Some("US"), "en"),
            us,
            "{text:?}"
        );
    }
    let own = CareLines::or_shipped(Some(
        r#"{"regions":{"US":{"lines":{"en":"Call 988."}}},"default":{"lines":{"en":"Reach out."}}}"#,
    ));
    assert_eq!(own.line(Some("US"), "en"), "Call 988.");
}

/// The full line on the first care turn, not again within `AGAIN_AFTER`,
/// again after it, and at once when the person asks who they can call.
#[test]
fn the_line_is_not_said_again_within_five_minutes_unless_asked() {
    let start = std::time::Instant::now();
    let mut said = Said::default();
    assert!(said.due(start, false));
    assert!(!said.due(start + std::time::Duration::from_secs(60), false));
    assert!(!said.due(
        start + AGAIN_AFTER - std::time::Duration::from_secs(1),
        false
    ));
    let asked = start + std::time::Duration::from_secs(120);
    let mut asking = Said::default();
    asking.due(start, false);
    assert!(asking.due(asked, true), "asked who to call");
    assert!(said.due(start + AGAIN_AFTER, false));
    assert!(asks_who_to_call("Who can I call?"));
    assert!(asks_who_to_call("Có đường dây nóng nào không?"));
    assert!(!asks_who_to_call("Call mom"));
    assert_eq!(AGAIN_AFTER, std::time::Duration::from_secs(300));
    assert_eq!(HOLD, std::time::Duration::from_secs(1800));
}

/// A care turn's line follows its answer once, in the region and language
/// given; a turn not marked has none; the hold lasts `HOLD` from the last
/// care turn.
#[test]
fn a_care_turn_gets_its_line_once_and_holds_the_unasked() {
    let care = Care::new(CareLines::shipped(), Some("VN".into()), "en");
    let start = std::time::Instant::now();
    assert!(!care.holds(start));
    assert_eq!(care.line_after(7, start, false), None, "not a care turn");
    care.mark(7, start);
    assert!(care.marked(7));
    let line = care.line_after(7, start, false).unwrap();
    assert!(line.contains("115") && line.is_ascii(), "{line}");
    assert!(!care.marked(7), "let go once answered");
    assert_eq!(care.line_after(7, start, false), None);
    let soon = start + std::time::Duration::from_secs(60);
    care.mark(8, soon);
    assert_eq!(care.line_after(8, soon, false), None, "within five minutes");
    care.mark(9, soon);
    assert!(
        care.line_after(9, soon, true).is_some(),
        "asked who to call"
    );
    assert!(care.holds(soon + HOLD - std::time::Duration::from_secs(1)));
    assert!(!care.holds(soon + HOLD));
}

/// The coordinator's hold: words caught across pieces start it, it lasts
/// `HOLD`, a later line restarts it, and nothing of a finished line stays.
#[test]
fn the_hold_starts_on_words_caught_across_pieces_and_lasts_its_time() {
    let start = std::time::Instant::now();
    let mut hold = Hold::default();
    assert!(!hold.heard("I just want to ", false, start));
    assert!(!hold.holds(start));
    assert!(
        hold.heard("die, honestly", false, start),
        "caught across pieces"
    );
    assert!(!hold.heard(" today", true, start), "once a line");
    assert!(hold.holds(start + HOLD - std::time::Duration::from_secs(1)));
    assert!(!hold.holds(start + HOLD));
    assert!(
        !hold.heard("what's the weather", true, start),
        "the old line is let go"
    );
    let later = start + std::time::Duration::from_secs(20 * 60);
    assert!(hold.heard("i can't go on", true, later));
    assert!(
        hold.holds(start + HOLD + std::time::Duration::from_secs(60)),
        "restarted"
    );
}
