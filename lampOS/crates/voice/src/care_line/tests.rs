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
        // The misses on #7752.
        "I'm going to end it",
        "I want to end it",
        "I've been thinking about taking my life",
        "I want to hang myself",
        "I might shoot myself",
        "I want to slit my wrists",
        "I want to end my own life",
        "I dont wanna live",
        "I want to be dead",
        "I could kill my self",
        "I cut myself again last night",
        "Mình không còn muốn sống nữa",
        "Tôi muốn rạch tay",
        "Tôi nghĩ đến chuyện treo cổ",
        "Tôi muốn nhảy lầu",
        "Tôi muốn kết thúc cuộc sống này",
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
        // The false hits on #7752.
        "I don't want to wake up late",
        "I can't go on Friday",
        "I can't go on the trip",
        "I cut myself chopping onions",
        "Trời nóng muốn chết",
        "Đói muốn chết",
        "I'm killing myself laughing",
    ] {
        assert!(!said_harm(heard), "{heard}");
    }
    // The guarded phrases still catch on their own.
    for heard in [
        "I don't want to wake up tomorrow",
        "I can't go on like this",
        "Tôi muốn chết",
        "I'm going to kill myself",
    ] {
        assert!(said_harm(heard), "{heard}");
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
    for asked in ["What's the number?", "Who do I call?", "Tôi nên gọi ai?"] {
        assert!(asks_who_to_call(asked), "{asked}");
    }
    assert!(!asks_who_to_call("Call mom"));
    assert_eq!(AGAIN_AFTER, std::time::Duration::from_secs(300));
    assert_eq!(HOLD, std::time::Duration::from_secs(1800));
}

/// A care turn owes the line; any answer's end says it once, and then it
/// is not due again within five minutes unless asked; one owed and not
/// played stays owed (the P1s and P2 on #7752).
#[test]
fn a_line_is_owed_until_played_once_and_holds_the_unasked() {
    let care = Care::new(CareLines::shipped(), Some("VN".into()), "en");
    let start = std::time::Instant::now();
    assert!(!care.holds(start));
    assert_eq!(care.owed_line(start), None, "nothing owed");
    care.mark(start, false);
    assert!(care.owes(start));
    let line = care.owed_line(start).unwrap();
    assert!(line.contains("115") && line.is_ascii(), "{line}");
    assert!(care.owes(start), "not played yet: still owed");
    care.said(&line, start);
    assert!(!care.owes(start));
    assert_eq!(care.owed_line(start), None);
    let soon = start + std::time::Duration::from_secs(60);
    care.mark(soon, false);
    assert_eq!(care.owed_line(soon), None, "within five minutes");
    assert!(!care.owes(soon), "let go, not due");
    care.mark(soon, true);
    assert!(care.owed_line(soon).is_some(), "asked who to call");
    assert!(care.holds(soon + HOLD - std::time::Duration::from_secs(1)));
    assert!(!care.holds(soon + HOLD));
    // An owed line older than five minutes is let go.
    let care = Care::new(CareLines::shipped(), None, "en");
    care.mark(start, false);
    assert_eq!(care.owed_line(start + AGAIN_AFTER), None);
}

/// The lamp's own line heard back is not the person's words of harm.
#[test]
fn the_lamps_own_line_heard_back_is_not_caught() {
    let care = Care::new(CareLines::shipped(), Some("US".into()), "en");
    let start = std::time::Instant::now();
    let line = care.line();
    assert!(said_harm(&line), "the US line names the lifeline");
    care.said(&line, start);
    assert!(care.own_line_heard("the Suicide and Crisis Lifeline", start));
    assert!(!care.own_line_heard("I want to kill myself", start));
    assert!(!care.own_line_heard(
        "the Suicide and Crisis Lifeline",
        start + std::time::Duration::from_secs(31)
    ));
}

/// Where the lamp is: the hand-set region first, then lamp-cloud's CN,
/// then the country of its set time zone; none of those, the default
/// (both P1s on #7752: a US or VN lamp's placement is only ever empty).
#[test]
fn the_region_comes_from_the_set_time_zone_unless_set_or_in_china() {
    let lines = CareLines::shipped();
    for (zone, region) in [
        ("America/New_York", Some("US")),
        ("America/Indiana/Indianapolis", Some("US")),
        ("Pacific/Honolulu", Some("US")),
        ("Asia/Ho_Chi_Minh", Some("VN")),
        ("Asia/Saigon", Some("VN")),
        ("Asia/Shanghai", Some("CN")),
        ("America/Toronto", None),
        ("Europe/Paris", None),
    ] {
        assert_eq!(country_of_zone(zone), region, "{zone}");
    }
    let us = region_from(None, Some(""), Some("America/Chicago"), "en");
    assert_eq!(us.as_deref(), Some("US"));
    assert!(lines.line(us.as_deref(), "en").contains("988"));
    let vn = region_from(None, None, Some("Asia/Ho_Chi_Minh"), "vi");
    assert!(lines.line(vn.as_deref(), "vi").contains("115"));
    assert_eq!(
        region_from(None, Some("CN\n"), Some("America/Chicago"), "en").as_deref(),
        Some("CN"),
        "lamp-cloud's placement says China"
    );
    assert_eq!(
        region_from(Some("vn"), Some("CN"), Some("America/Chicago"), "en").as_deref(),
        Some("VN"),
        "set by hand first"
    );
    assert_eq!(region_from(Some("nowhere"), None, None, "en"), None);
    assert_eq!(
        region_from(None, Some("US"), None, "en"),
        None,
        "placement is CN or nothing"
    );
}

/// The images' own zone, maybe never set: 988 only for an owner who
/// speaks English; any other gets the default, never a wrong number (the
/// safety read's P2 on #7752). Old US zone names count, and an installed
/// copy missing a shipped region is not used.
#[test]
fn the_images_zone_counts_only_in_english_and_a_partial_copy_is_not_used() {
    assert_eq!(
        region_from(None, None, Some(IMAGE_ZONE), "en").as_deref(),
        Some("US")
    );
    assert_eq!(region_from(None, None, Some(IMAGE_ZONE), "vi"), None);
    assert_eq!(
        region_from(None, None, Some("America/Los_Angeles"), "vi").as_deref(),
        Some("US"),
        "a zone set by hand counts in any language"
    );
    for zone in [
        "America/Indianapolis",
        "America/Louisville",
        "America/Puerto_Rico",
    ] {
        assert_eq!(country_of_zone(zone), Some("US"), "{zone}");
    }
    let missing = r#"{"regions":{"CN":{"lines":{"en":"x"}}},"default":{"lines":{"en":"y"}}}"#;
    assert!(
        CareLines::or_shipped_whole(Some(missing))
            .line(Some("US"), "en")
            .contains("988")
    );
}

/// Every region's entry, and the default, names its source and the date it
/// was checked (the safety read's P3 on #7752).
#[test]
fn every_line_names_its_source_and_when_it_was_checked() {
    let file: serde_json::Value = serde_json::from_str(SHIPPED).unwrap();
    let entries = file["regions"]
        .as_object()
        .unwrap()
        .values()
        .chain([&file["default"]]);
    for entry in entries {
        for key in ["source", "checked"] {
            assert!(
                entry[key]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty()),
                "{key} in {entry}"
            );
        }
    }
}

/// The coordinator's still-here turn: only after a care turn's answer was
/// cut, once, after the quiet, never past its window, and not when a later
/// answer played whole.
#[test]
fn the_still_here_turn_comes_once_after_a_cut() {
    let mut still = StillHere::default();
    still.settled(4, false, 0);
    assert!(!still.take(10_000_000, true), "nothing caught");
    still.caught(Some(5));
    still.settled(4, false, 0);
    assert!(!still.take(10_000_000, true), "an older turn's end");
    still.settled(5, false, 1_000_000);
    assert!(!still.take(2_000_000, true), "too soon");
    assert!(!still.take(5_000_000, false), "not quiet");
    assert!(still.take(5_000_000, true));
    assert!(!still.take(6_000_000, true), "once");
    still.caught(Some(7));
    still.settled(7, true, 0);
    assert!(
        !still.take(10_000_000, true),
        "played whole: its line followed"
    );
    still.caught(None);
    still.settled(8, false, 0);
    assert!(
        !still.take(STILL_HERE_WITHIN_US, true),
        "the moment is gone"
    );
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

/// Vietnamese decomposed, as some transcripts give it, is caught as
/// composed is (the safety read's P2 on #7752).
#[test]
fn decomposed_vietnamese_is_caught() {
    use unicode_normalization::UnicodeNormalization as _;
    for heard in [
        "Tôi muốn tự tử",
        "Mình không muốn sống nữa",
        "Tôi chán sống",
    ] {
        let decomposed: String = heard.nfd().collect();
        assert_ne!(decomposed, heard);
        assert!(said_harm(&decomposed), "{heard}");
    }
}
