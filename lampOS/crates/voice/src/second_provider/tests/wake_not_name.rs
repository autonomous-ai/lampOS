//! #3504 finding N4: on lamp-06c1 the lamp's name was written "Ramon"
//! (T4), "Edeland" (P7r) and "Hey, mom" (P15p), and on 23 Oct "Elan"; the
//! lamp called the person by it, and P7r kept "Edeland's dog is called
//! Moe". With `LAMP_MEMORY_KEEP_HEARD` and lamp-name's hearing of the name
//! in the line (or the name note given), such an opening word is told to
//! the model as no name, and the request's line is held without it. Text
//! alone never decides it: Ramon, Ilan and Elan are people's names too.
use super::*;

/// The openings seen on the lamps are the lamp's name as heard.
#[test]
fn a_set_off_near_miss_of_the_name_is_the_name_as_heard() {
    for (heard, word, rest) in [
        (
            "Ramon. Forget everything you know about me.",
            "Ramon",
            "Forget everything you know about me.",
        ),
        (
            "Edeland, my dog is called Moe.",
            "Edeland",
            "my dog is called Moe.",
        ),
        (
            "Hey, mom. Forget low female allergy.",
            "mom",
            "Forget low female allergy.",
        ),
        (
            "Elan. What is your favorite drink?",
            "Elan",
            "What is your favorite drink?",
        ),
        (
            "Ilan, remember my locker is 12.",
            "Ilan",
            "remember my locker is 12.",
        ),
        ("Alone. What time is it?", "Alone", "What time is it?"),
    ] {
        assert_eq!(wake_heard_as(heard), Some((word, rest)), "{heard}");
    }
}

/// The name itself, a common opener, a word with no sound of the name, a
/// name alone and a first word not set off are left as they are.
#[test]
fn the_name_itself_and_common_openers_are_not_taken() {
    for heard in [
        "Lamp, remember my dog is called Moe.",
        "Hey Lamp, what time is it?",
        "Yes, please.",
        "Well, my dog is called Moe.",
        "No, it was the tulip.",
        "Please, remember my locker.",
        "Vâng, nhớ giúp tôi.",
        "Okay, thanks.",
        "Ramon.",
        "So far. The goals would be treat it.",
        "Ramon is my brother.",
        // The reviewer-lead's HOLD on `0990f0a27`: words that are said,
        // never a name heard for the lamp's.
        "Tomorrow, I see the dentist at 3, remember that.",
        "Today, I fed the cat.",
        "Tonight, remind me to call Mum.",
        "Remember, my locker is 12.",
        "Honestly, I prefer tea.",
        "Mai, tôi đi khám răng lúc 3 giờ.",
        "Nhớ, tủ của tôi là số 12.",
    ] {
        assert_eq!(wake_heard_as(heard), None, "{heard}");
    }
}

/// On a line lamp-name heard the name in, a day that opens it is kept, and
/// the misheard name is left out.
#[test]
fn a_named_line_keeps_its_day_and_loses_only_the_misheard_name() {
    let now = Instant::now();
    const DAY: &str = "Tomorrow, I see the dentist at 3, remember that.";
    assert_eq!(wake_not_a_name(9_904_301, DAY, true, true), None);
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(9_904_301, DAY);
    assert_eq!(thread.heard_for(9_904_301, now).as_deref(), Some(DAY));
    drop(thread);
    const RAMON: &str = "Ramon, forget everything you know about me.";
    assert!(wake_not_a_name(9_904_302, RAMON, true, true).is_some());
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(9_904_302, RAMON);
    assert_eq!(
        thread.heard_for(9_904_302, now).as_deref(),
        Some("forget everything you know about me.")
    );
}

/// The note names the word and says it is nobody's name.
#[test]
fn the_model_is_told_the_word_is_no_name() {
    let note = wake_note("Edeland");
    assert!(note.contains("\"Edeland\""), "{note}");
    assert!(note.contains("nobody's name"), "{note}");
    assert!(note.contains("never call them by it"), "{note}");
}

/// With the switch and the name note, the request's own line is held
/// without the word, so `KEEP_HEARD` never keeps it; without either,
/// nothing is told or held.
#[test]
fn the_requests_line_is_held_without_the_word_only_with_the_switch_and_the_name() {
    const HEARD: &str = "Edeland, my dog is called Moe.";
    let now = Instant::now();
    for (request, named, keep_heard) in [(9_904_101, false, true), (9_904_102, true, false)] {
        assert_eq!(wake_not_a_name(request, HEARD, named, keep_heard), None);
        let thread = crate::thread::THREAD.lock().unwrap();
        assert_ne!(
            thread.heard_for(request, now).as_deref(),
            Some("my dog is called Moe.")
        );
    }
    let request = 9_904_103;
    let note = wake_not_a_name(request, HEARD, true, true).expect("told");
    assert!(note.contains("\"Edeland\""), "{note}");
    // The line the answer then holds for the request is the first one kept.
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(request, HEARD);
    assert_eq!(
        thread.heard_for(request, now).as_deref(),
        Some("my dog is called Moe.")
    );
    // A line with no such word is held as it was said.
    drop(thread);
    assert_eq!(
        wake_not_a_name(9_904_104, "Lamp, my dog is called Moe.", true, true),
        None
    );
}

/// A line lamp-name did not hear the name in keeps a real name that opens
/// it, word for word; the same line heard with the name loses only it.
#[test]
fn an_unnamed_line_keeps_the_name_it_opens_with() {
    let now = Instant::now();
    for (request, heard) in [
        (9_904_201, "Ramon, my dog is called Moe."),
        (9_904_202, "Tom, remember my locker is 12."),
    ] {
        assert_eq!(wake_not_a_name(request, heard, false, true), None);
        let mut thread = crate::thread::THREAD.lock().unwrap();
        thread.line_of(request, heard);
        assert_eq!(thread.heard_for(request, now).as_deref(), Some(heard));
    }
    let request = 9_904_203;
    assert!(wake_not_a_name(request, "Edeland, remember my dog is Moe.", true, true).is_some());
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(request, "Edeland, remember my dog is Moe.");
    assert_eq!(
        thread.heard_for(request, now).as_deref(),
        Some("remember my dog is Moe.")
    );
    drop(thread);
    let kept = crate::remember::heard::heard_instead(
        "Edeland's dog is Moe.",
        Some("remember my dog is Moe."),
        true,
    )
    .unwrap();
    assert_eq!(kept, "They said: my dog is Moe.");
}

/// N4b (lamp-06c1, 28 Oct): "Hey Lamp, guess what" was written "Hello.
/// Gaswa, I changed jobs…", so the lamp kept "Gaswa" in the heard line and
/// asked a known face what to call them. One greeting of the line's own,
/// set off, is passed over before the name as heard.
#[test]
fn a_greeting_before_the_name_as_heard_is_passed_over() {
    for (heard, word, rest) in [
        (
            "Hello. Gaswa, I changed jobs. I'm at the library now.",
            "Gaswa",
            "I changed jobs. I'm at the library now.",
        ),
        (
            "Hi. Ramon, remember my locker is 12.",
            "Ramon",
            "remember my locker is 12.",
        ),
        (
            "Good morning, Edeland. My dog is called Moe.",
            "Edeland",
            "My dog is called Moe.",
        ),
        (
            "Xin chào, Ramon. Nhớ giúp tôi tủ số 12.",
            "Ramon",
            "Nhớ giúp tôi tủ số 12.",
        ),
        // The Reviewer-lead's P2 on #4078: the noun later in the line is
        // no call of the lamp, so the name as heard is still passed over.
        (
            "Hello. Gaswa, turn off the lamp.",
            "Gaswa",
            "turn off the lamp.",
        ),
        (
            "Hello. Gaswa, dim the lamp please.",
            "Gaswa",
            "dim the lamp please.",
        ),
        (
            "Good morning, Ramon. Is the lamp too bright? My lamp's shade, I mean.",
            "Ramon",
            "Is the lamp too bright? My lamp's shade, I mean.",
        ),
        ("Ramon, turn on the lamps.", "Ramon", "turn on the lamps."),
        (
            "Gaswa, dim my lamp, please.",
            "Gaswa",
            "dim my lamp, please.",
        ),
        (
            "Gaswa, the lamp is too bright.",
            "Gaswa",
            "the lamp is too bright.",
        ),
    ] {
        assert_eq!(wake_heard_as(heard), Some((word, rest)), "{heard}");
    }
    // A greeting before the name itself, a common opener or a day is left
    // as it was; so is a greeting with nothing set off after it.
    for heard in [
        "Hello, Lamp. What time is it?",
        "Hello. Thanks for that.",
        "Good morning. Tomorrow, I see the dentist.",
        "Hello.",
        "Hello there, my dog is called Moe.",
        // Reviewer-three's P2 on #4069: past a greeting, the name said as
        // itself means the first word was heard right, a person's name.
        "Good morning, Anna. Lamp, remember Anna's birthday is the fifth of May.",
        "Hello, Anna. Lamp, what's the weather today?",
        // The Reviewer-lead's P3 on #4078: past "Hey" or "Hi" as well, and
        // with no greeting at all. "Hi, honey. …" was read as a wake
        // before; the call after it says "honey" was said to a person.
        "Hey, Anna. Lamp, what's the weather today?",
        "Hi, honey. Lamp, what time is it?",
        "Anna. Lamp, remember Anna's birthday.",
        "Hello, Anna. Hey Lamp what's the weather?",
        // Both readers' P2 on #4274: a call whose comma speech to text
        // dropped is still a call.
        "Hello, Anna. Lamp what's the weather?",
        "Hello, Anna. Hey, Lamp what time is it?",
        "Anna. Lamp what's the weather?",
        "Anna. Lamp ơi, mấy giờ rồi?",
        "Anna. Lamp à, mấy giờ rồi?",
        "Anna. Lamp nhé, nhớ giúp.",
    ] {
        assert_eq!(wake_heard_as(heard), None, "{heard}");
    }
}

/// The Reviewer-lead's P3 on #4069: a greeting written decomposed ("chào"
/// as "cha", a combining grave and "o") is passed over whole, never cut
/// inside its accent.
#[test]
fn a_decomposed_greeting_is_passed_over_whole() {
    const NFD: &str = "Xin cha\u{300}o. Gaswa, nho\u{31b}\u{301} giu\u{301}p to\u{302}i.";
    assert_eq!(
        wake_heard_as(NFD),
        Some(("Gaswa", "nho\u{31b}\u{301} giu\u{301}p to\u{302}i."))
    );
    assert_eq!(
        wake_heard_as("Cha\u{300}o, Ramon. Nhớ giúp tôi."),
        Some(("Ramon", "Nhớ giúp tôi."))
    );
}

/// A greeting and a person's own name on a line lamp-name did not hear the
/// name in are kept word for word; with the name heard, only the greeting
/// and the word go.
#[test]
fn a_greeting_and_a_real_name_stay_when_the_name_was_not_heard() {
    let now = Instant::now();
    const HEARD: &str = "Hello, Tom here. Remember my locker is 12.";
    assert_eq!(wake_not_a_name(9_904_401, HEARD, false, true), None);
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(9_904_401, HEARD);
    assert_eq!(thread.heard_for(9_904_401, now).as_deref(), Some(HEARD));
    drop(thread);
    const N11C: &str = "Hello. Gaswa, I changed jobs. I'm at the library now.";
    let note = wake_not_a_name(9_904_402, N11C, true, true).expect("told");
    assert!(note.contains("\"Gaswa\""), "{note}");
    let mut thread = crate::thread::THREAD.lock().unwrap();
    thread.line_of(9_904_402, N11C);
    let kept = thread.heard_for(9_904_402, now).unwrap();
    assert_eq!(kept, "I changed jobs. I'm at the library now.");
    assert!(!kept.contains("Gaswa") && !kept.contains("Hello"), "{kept}");
}

/// What `KEEP_HEARD` keeps for the model's "Edeland's dog is called Moe."
/// against the line held without the word: the heard line, not the wake.
#[test]
fn keep_heard_keeps_the_line_without_the_wake() {
    let (_, rest) = wake_heard_as("Edeland, my dog is called Moe.").unwrap();
    let kept =
        crate::remember::heard::heard_instead("Edeland's dog is called Moe.", Some(rest), true)
            .expect("the model's word was not heard");
    assert!(!kept.contains("Edeland"), "{kept}");
    assert!(kept.contains("my dog is called Moe"), "{kept}");
}

/// lamp-06c1, 28 Oct (night run): with `LAMP_MEMORY_KEEP_HEARD`, a line
/// that called the lamp by its name as itself was kept with the call
/// ("They said: Hey, Lamp. …"). The line is now held without it, and the
/// model is told nothing; a line without the name heard, or with the switch
/// off, is held as it was said.
#[test]
fn a_line_that_calls_the_lamp_by_name_is_held_without_the_call() {
    let now = Instant::now();
    for (request, heard, rest) in [
        (
            9_904_601,
            "Hey, Lamp. Remember I went to the dentist last Friday.",
            "Remember I went to the dentist last Friday.",
        ),
        (
            9_904_602,
            "Lamp, please remember this. I went to the dentist.",
            "please remember this. I went to the dentist.",
        ),
        (
            9_904_603,
            "Hello. Hey Lamp, guess what? I changed jobs.",
            "guess what? I changed jobs.",
        ),
    ] {
        assert_eq!(wake_not_a_name(request, heard, true, true), None, "{heard}");
        let mut thread = crate::thread::THREAD.lock().unwrap();
        thread.line_of(request, heard);
        assert_eq!(
            thread.heard_for(request, now).as_deref(),
            Some(rest),
            "{heard}"
        );
    }
    for (request, heard, named, keep_heard) in [
        (9_904_604, "Hey, Lamp. My dog is called Moe.", false, true),
        (9_904_605, "Hey, Lamp. My dog is called Moe.", true, false),
        // A person greeted by name keeps theirs; the call is not the opening.
        (
            9_904_606,
            "Hello, Anna. Lamp, remember Anna's birthday.",
            true,
            true,
        ),
        // The call alone is all there is.
        (9_904_607, "Hey, Lamp.", true, true),
    ] {
        assert_eq!(
            wake_not_a_name(request, heard, named, keep_heard),
            None,
            "{heard}"
        );
        let mut thread = crate::thread::THREAD.lock().unwrap();
        thread.line_of(request, heard);
        assert_eq!(
            thread.heard_for(request, now).as_deref(),
            Some(heard),
            "{heard}"
        );
    }
}
