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
