//! `LAMP_ACTION_CLAIM_CHECKED` on the second route (#5215): a reply that
//! claims a light, volume or Schedule action in the acting part's own words,
//! with no call of that kind in its turn, is never said as it is. The model
//! is asked once more; a second claim with no call is said as the plain line
//! that it wasn't done. A turn that made the call, and any answer that
//! claims nothing, go on byte for byte as without the switch.
use super::*;

/// What lamp-actions tells the voice of each call here, with
/// `LAMP_SHORT_DONE` on (as `short_done.rs`'s `told`).
fn told(name: &str) -> Value {
    let short = |result: &str, say: &str| json!({"result": result, "next": crate::second_provider::probe::tools::SHORT_NEXT, "say": say});
    match name {
        "light_brighter" => short("My light is a step brighter.", "Brighter."),
        "volume_up" => short("My volume is now 70 out of 100; it was 50.", "Louder."),
        "light_colour" => short("My light is blue now.", "Done."),
        "set_timer" => {
            json!({"status": "committed", "id": 1, "due_unix_ms": 1_792_454_700_000_u64, "local": "2026-10-31T07:52:00+07:00", "clock": "07:52"})
        }
        _ => json!({"ok": true}),
    }
}

/// One turn as it went.
#[derive(Debug, PartialEq)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The chat rounds asked.
    asked: usize,
    /// The calls run, by name.
    ran: Vec<String>,
    /// The notes the rounds were asked with.
    notes: Vec<String>,
}

/// One turn: `heard` asked with round 1's `first` events and the rounds
/// `later`, as the product runs (`LAMP_SHORT_DONE` and
/// `LAMP_SECOND_SAY_WITH_CALL` on), the claim switch `on`.
fn claimed(
    on: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    turn_as(on, true, heard, first, later)
}

/// [`claimed`], with the answer at once and short lines as `product`.
fn turn_as(
    on: bool,
    product: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    turn_with(on, false, product, heard, first, later)
}

/// [`turn_as`], with the worded claims (`LAMP_ACTION_CLAIM_WORDED`) as
/// `worded_on`.
fn turn_with(
    on: bool,
    worded_on: bool,
    product: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        // A set refused by a ledger that can't ring it (#5560).
        let answer = if args["cannot_ring"] == true {
            json!({"error": crate::reminders::CANNOT_RING})
        } else {
            told(&name)
        };
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .answering_short(product)
    .saying_at_once(product)
    .checking_action_claims(on)
    .checking_worded_claims(worded_on);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let asked = script.asked.lock().unwrap();
    let notes = asked
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with("(For you to know"))
        .map(str::to_owned)
        .collect();
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        asked: asked.len(),
        ran: ran
            .lock()
            .unwrap()
            .iter()
            .map(|(name, ..)| name.clone())
            .collect(),
        notes,
    }
}

fn calling(name: &str) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call("c0", name, json!({}))])),
        Ok(ChatEvent::Done),
    ]
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

fn told_claim(went: &Went) -> bool {
    went.notes
        .iter()
        .any(|note| note == crate::action_claim::NOTE)
}

/// lamp-52e6 at 9928 (4 Nov 09:38): "Lamp, set a timer to one minute." got
/// "Timer set. It rings at 9:39 am." and no call. With the switch the reply
/// is held and the model asked once more; the retry's call is answered by
/// the timer's own line. Without it, the claim is said, as on the lamp.
#[test]
fn a_timer_claimed_with_no_call_is_held_and_asked_again() {
    let heard = "Lamp, set a timer to one minute.";
    let went = claimed(
        true,
        heard,
        saying("Timer set. It rings at 9:39 am."),
        vec![calling("set_timer")],
    );
    assert_eq!(went.said, said(&["Timer set. It rings at 7:52 am."]));
    assert_eq!(
        (went.asked, went.ran.as_slice()),
        (2, ["set_timer".to_owned()].as_slice())
    );
    assert!(told_claim(&went), "{:?}", went.notes);
    // Off: said as it came, and nothing asked again.
    let went = claimed(
        false,
        heard,
        saying("Timer set. It rings at 9:39 am."),
        vec![calling("set_timer")],
    );
    assert_eq!(went.said, said(&["Timer set.", " It rings at 9:39 am."]));
    assert_eq!((went.asked, went.ran.len()), (1, 0));
    assert!(!told_claim(&went));
}

/// #5560: a `set_timer` refused because the delivery ledger can't ring it
/// set nothing, so it is no call of its kind. "Timer set." after it is held
/// as a claim with no call, then said as the line that it wasn't done; never
/// as it came. A set that ran still lets its line stand.
#[test]
fn a_timer_claimed_after_a_set_the_ledger_refused_is_never_said() {
    let heard = "Lamp, set a timer to one minute.";
    let refused = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c0",
                "set_timer",
                json!({"cannot_ring": true}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    for product in [false, true] {
        let went = turn_as(
            true,
            product,
            heard,
            refused(),
            vec![saying("Timer set."), saying("Timer set.")],
        );
        assert!(
            went.said.iter().all(|said| !said.contains("Timer set")),
            "{product}: {went:?}"
        );
        assert_eq!(
            went.said,
            said(&[crate::action_claim::NOT_DONE_LINE]),
            "{product}"
        );
        assert_eq!(went.ran, ["set_timer"], "{product}");
    }
    // The same words after a set that ran stand, untouched by the check.
    let ran = |on| {
        turn_as(
            on,
            false,
            heard,
            calling("set_timer"),
            vec![saying("Timer set.")],
        )
    };
    assert_eq!(ran(true), ran(false));
    assert_eq!(ran(true).said, said(&["Timer set."]));
}

/// lamp-52e6, the earlier run: "Louder." and "Brighter." with no call. The
/// first is held (without the switch too) and asked again; a second with no
/// call is the plain line that it wasn't done, never silence.
#[test]
fn a_step_word_claimed_twice_with_no_call_says_it_was_not_done() {
    for (heard, word) in [("Louder.", "Louder."), ("Make it brighter.", "Brighter.")] {
        let went = claimed(true, heard, saying(word), vec![saying(word)]);
        assert_eq!(
            went.said,
            said(&[crate::action_claim::NOT_DONE_LINE]),
            "{word}"
        );
        assert_eq!((went.asked, went.ran.len()), (2, 0), "{word}");
        // Off: the step word is held and asked again as before, and the
        // second goes unsaid: nothing at all.
        let went = claimed(false, heard, saying(word), vec![saying(word)]);
        assert_eq!(
            (went.said.len(), went.asked, went.ran.len()),
            (0, 2, 0),
            "{word}"
        );
    }
    // A claim, then a step word: one ask again for the turn, then the line.
    let went = claimed(true, "Louder.", saying("Done."), vec![saying("Louder.")]);
    assert_eq!(went.said, said(&[crate::action_claim::NOT_DONE_LINE]));
    assert_eq!(went.asked, 2);
    // Any claim twice with no call: the same.
    let went = claimed(
        true,
        "Make it blue.",
        saying("It already is."),
        vec![saying("Done.")],
    );
    assert_eq!(went.said, said(&[crate::action_claim::NOT_DONE_LINE]));
    assert_eq!((went.asked, went.ran.len()), (2, 0));
    assert!(told_claim(&went));
    // Asked again, the model makes the call: its own line answers.
    let went = claimed(
        true,
        "Louder.",
        saying("Louder."),
        vec![calling("volume_up")],
    );
    assert_eq!(went.said, said(&["Louder."]));
    assert_eq!(went.ran, ["volume_up"]);
}

/// A turn that made the call is untouched, byte for byte: the claim beside
/// its call, and the claim after it.
#[test]
fn a_claim_beside_its_call_is_untouched() {
    let mut beside = vec![text("Brighter.")];
    beside.extend(calling("light_brighter"));
    let on = claimed(true, "Make it brighter.", beside.clone(), vec![]);
    assert_eq!(on, claimed(false, "Make it brighter.", beside, vec![]));
    assert_eq!(on.said, said(&["Brighter."]));
    assert_eq!(on.ran, ["light_brighter"]);
    // The word after the call, without the answer at once.
    for product in [false, true] {
        let went = |on| {
            turn_as(
                on,
                product,
                "Make it brighter.",
                calling("light_brighter"),
                vec![saying("Brighter.")],
            )
        };
        assert_eq!(went(true), went(false), "{product}");
    }
    // Words and a colour call together.
    let mut colour = vec![text("My light is blue now.")];
    colour.extend(calling("light_colour"));
    let on = claimed(
        true,
        "Make it blue.",
        colour.clone(),
        vec![saying("Anything else?")],
    );
    assert_eq!(
        on,
        claimed(
            false,
            "Make it blue.",
            colour,
            vec![saying("Anything else?")]
        )
    );
    assert!(
        on.said.contains(&"said:My light is blue now.".to_owned()),
        "{:?}",
        on.said
    );
}

/// An answer that claims nothing is untouched, byte for byte.
#[test]
fn an_answer_that_claims_nothing_is_untouched() {
    for (heard, answer) in [
        ("What time is it?", "It's 9:39."),
        (
            "Did you set a timer?",
            "Yes, two: tea at 9:39 am, and the oven at 10:05 am.",
        ),
        ("Tell me something.", "Louder music helps me focus."),
        ("Is it bright enough?", "Brighter? I can, if you like."),
    ] {
        let on = claimed(true, heard, saying(answer), vec![]);
        assert_eq!(on, claimed(false, heard, saying(answer), vec![]), "{heard}");
        assert_eq!(on.asked, 1, "{heard}");
        assert!(!on.said.is_empty(), "{heard}");
        assert!(!told_claim(&on), "{heard}");
    }
}

/// A claim beside a call of another kind ("Timer set." with only a light
/// call) is not said; the call runs.
#[test]
fn a_claim_beside_another_kind_of_call_is_not_said() {
    let mut beside = vec![text("Timer set. It rings at 9:39 am.")];
    beside.extend(calling("light_brighter"));
    let went = claimed(
        true,
        "Set a timer and make it brighter.",
        beside,
        vec![saying("Okay.")],
    );
    assert!(
        went.said.iter().all(|said| !said.contains("Timer set")),
        "{:?}",
        went.said
    );
    assert_eq!(went.ran, ["light_brighter"]);
    // Thirteen's P2 on #5253: the next round is told the light change ran
    // and only the timer wasn't, never that no function was called.
    let beside = crate::action_claim::beside_calls_note(crate::action_claim::Claim::Schedule);
    assert!(went.notes.contains(&beside), "{:?}", went.notes);
    assert!(!told_claim(&went), "{:?}", went.notes);
}

/// One turn with the claim check on and the worded claims too
/// (`LAMP_ACTION_CLAIM_WORDED`, #5215's open shapes).
fn worded(
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    turn_with(true, true, true, heard, first, later)
}

/// #5215, what still counts as claimed-not-done after the acting part's own
/// lines are checked: the model words a timer it never set in its own way
/// ("I've set a timer for one minute.", "Timer set for one minute."). The
/// reply is held and the model asked once more; the retry's call is answered
/// by the timer's own line.
#[test]
fn a_timer_claimed_in_other_words_with_no_call_is_held_and_asked_again() {
    for claim in [
        "I've set a timer for one minute. It rings at 9:39 am.",
        "Timer set for one minute, it rings at 9:39 am.",
        "Okay, your one-minute timer is set.",
    ] {
        let went = worded(
            "Lamp, set a timer to one minute.",
            saying(claim),
            vec![calling("set_timer")],
        );
        assert_eq!(
            went.said,
            said(&["Timer set. It rings at 7:52 am."]),
            "{claim}"
        );
        assert_eq!(
            (went.asked, went.ran.as_slice()),
            (2, ["set_timer".to_owned()].as_slice()),
            "{claim}"
        );
        assert!(told_claim(&went), "{claim}: {:?}", went.notes);
    }
}

/// #5215: a light or volume change claimed in other words, twice, with no
/// call ("I've kept the light on until ten." on 10441, 5 Nov 04:24, with
/// `calls=none`; "Okay, louder."; "Brighter now."): said as the plain line
/// that it wasn't done.
#[test]
fn a_device_change_claimed_in_other_words_twice_with_no_call_says_it_was_not_done() {
    for (heard, claim) in [
        (
            "Hey Lamp, keep the light on until ten.",
            "I've kept the light on until ten.",
        ),
        ("Louder.", "Okay, louder."),
        ("Brighter.", "Brighter now."),
        ("Louder.", "I've turned the volume up."),
    ] {
        let went = worded(heard, saying(claim), vec![saying(claim)]);
        assert_eq!(
            went.said,
            said(&[crate::action_claim::NOT_DONE_LINE]),
            "{claim}"
        );
        assert_eq!((went.asked, went.ran.len()), (2, 0), "{claim}");
        assert!(told_claim(&went), "{claim}: {:?}", went.notes);
    }
}

/// A turn that made its call is untouched by the worded claims, byte for
/// byte: the words beside the call, and after it. A limit said ("for one
/// minute") is one the call carried.
#[test]
fn a_claim_in_other_words_beside_its_call_is_untouched() {
    for (heard, words, name, args) in [
        (
            "Lamp, set a timer to one minute.",
            "I've set a timer for one minute.",
            "set_timer",
            json!({"text": "Timer", "seconds": 60}),
        ),
        (
            "Louder.",
            "I've turned the volume up.",
            "volume_up",
            json!({}),
        ),
        ("Brighter.", "Okay, brighter.", "light_brighter", json!({})),
    ] {
        let mut beside = vec![text(words)];
        beside.extend(calling_with(name, args.clone()));
        let went = |worded_on: bool| {
            turn_with(
                true,
                worded_on,
                true,
                heard,
                beside.clone(),
                vec![saying("Anything else?")],
            )
        };
        assert_eq!(went(true), went(false), "{words}");
        assert_eq!(went(true).ran, [name], "{words}");
        // The call first, its words after.
        let after = |worded_on: bool| {
            turn_with(
                true,
                worded_on,
                true,
                heard,
                calling_with(name, args.clone()),
                vec![saying(words)],
            )
        };
        assert_eq!(after(true), after(false), "{words}");
    }
}

/// An answer that claims nothing in other words either is untouched, byte
/// for byte; and without the worded switch a worded claim is said as
/// before, as on the lamp.
#[test]
fn talk_and_the_switch_off_are_untouched_by_the_worded_claims() {
    for (heard, answer) in [
        ("Is my timer set?", "Is your timer set? Let me look."),
        (
            "Can you keep the light on until ten?",
            "I can keep my light on until ten, with a reminder. Want one?",
        ),
        ("What did you do today?", "I've set my heart on tea."),
        ("Put on some music.", "Okay, louder music it is."),
        ("What colour is the light?", "My light is blue now."),
    ] {
        let on = worded(heard, saying(answer), vec![]);
        assert_eq!(on, claimed(true, heard, saying(answer), vec![]), "{heard}");
        assert_eq!(on.asked, 1, "{heard}");
        assert!(!told_claim(&on), "{heard}");
    }
    let went = claimed(
        true,
        "Hey Lamp, keep the light on until ten.",
        saying("I've kept the light on until ten."),
        vec![],
    );
    assert_eq!(went.said, said(&["I've kept the light on until ten."]));
    assert_eq!((went.asked, went.ran.len()), (1, 0));
}

/// The first builder's P2 on #5588: a true status answer to a question the
/// person asked, with no call ("How long will the light stay on?" after a
/// kept rule, "Is my timer still set?" after a timer), is no claim on the
/// worded runs: said as it is, byte for byte as with them off, and never
/// asked again. Heard with or without its '?'.
#[test]
fn a_status_answer_to_a_question_is_untouched_by_the_worded_claims() {
    for (heard, answer) in [
        (
            "Lamp, how long will the light stay on?",
            "The light stays on until ten.",
        ),
        (
            "Lamp how long will the light stay on",
            "I'll keep my light on until ten, as you asked.",
        ),
        (
            "Lamp, is my timer still set?",
            "Your timer is set for one minute.",
        ),
        (
            "Hey Lamp, did you set my timer",
            "I've set a timer for 9:39 am.",
        ),
        (
            "Is the light staying on?",
            "The light is staying on until ten.",
        ),
    ] {
        let on = worded(heard, saying(answer), vec![]);
        // Said whole, though lamp-voice may speak it in pieces at a comma.
        let words: String = on
            .said
            .iter()
            .map(|piece| piece.strip_prefix("said:").unwrap_or(piece))
            .collect();
        assert_eq!(words, answer, "{heard}");
        assert_eq!((on.asked, on.ran.len()), (1, 0), "{heard}");
        assert!(!told_claim(&on), "{heard}: {:?}", on.notes);
        assert_eq!(on, claimed(true, heard, saying(answer), vec![]), "{heard}");
    }
}

/// #5253's strict runs are unchanged on a question: the acting part's own
/// line with no call is still held, asked again and said not done.
#[test]
fn a_strict_claim_to_a_question_is_still_held_with_the_worded_claims() {
    let heard = "Lamp, is my timer still set?";
    let went = worded(heard, saying("Timer set."), vec![saying("Timer set.")]);
    assert_eq!(went.said, said(&[crate::action_claim::NOT_DONE_LINE]));
    assert_eq!((went.asked, went.ran.len()), (2, 0));
    assert!(told_claim(&went), "{:?}", went.notes);
}

/// The first builder's P2 on #5588: work done by another tool ("I've added
/// a reminder to your calendar." beside `service_write`, "I've turned off
/// the lights." after `use_skill` or a hand-over) stands: the worded runs
/// judge only a turn with no calls at all. Byte for byte as with them off.
#[test]
fn a_worded_claim_in_a_turn_with_any_call_stands() {
    for (heard, words, name) in [
        (
            "Lamp, add a reminder to my calendar.",
            "I've added a reminder to your calendar.",
            crate::services::WRITE,
        ),
        (
            "Lamp, turn off the lights.",
            "I've turned off the lights.",
            crate::use_skill::NAME,
        ),
        (
            "Lamp, turn off the lights.",
            "I've turned off the lights.",
            crate::agent_tasks::START,
        ),
    ] {
        // The words beside the call.
        let mut beside = vec![text(words)];
        beside.extend(calling(name));
        let went = |worded_on: bool| {
            turn_with(
                true,
                worded_on,
                true,
                heard,
                beside.clone(),
                vec![saying("Anything else?")],
            )
        };
        assert_eq!(went(true), went(false), "{words} beside {name}");
        assert!(!told_claim(&went(true)), "{words} beside {name}");
        // The call first, its words after.
        let after = |worded_on: bool| {
            turn_with(
                true,
                worded_on,
                true,
                heard,
                calling(name),
                vec![saying(words)],
            )
        };
        assert_eq!(after(true), after(false), "{words} after {name}");
        assert!(!told_claim(&after(true)), "{words} after {name}");
    }
    // A skill or a linked service, with the words said.
    for name in [crate::services::WRITE, crate::use_skill::NAME] {
        let words = "I've turned off the lights.";
        let went = worded(
            "Lamp, turn off the lights.",
            calling(name),
            vec![saying(words)],
        );
        assert_eq!(went.said, said(&[words]), "{name}");
        assert_eq!(went.asked, 2, "{name}");
    }
}

/// Twelve's refinement of the first builder's P2 on #5588: a request asked
/// as a question ("Can you set a timer for one minute?") is still judged by
/// the worded runs, since a claim with no call there is #5215's own bug.
#[test]
fn a_request_asked_as_a_question_is_still_judged_by_the_worded_claims() {
    let went = worded(
        "Lamp, can you set a timer for one minute?",
        saying("Timer set for one minute."),
        vec![calling("set_timer")],
    );
    assert_eq!(went.said, said(&["Timer set. It rings at 7:52 am."]));
    assert_eq!(
        (went.asked, went.ran.as_slice()),
        (2, ["set_timer".to_owned()].as_slice())
    );
    assert!(told_claim(&went), "{:?}", went.notes);
    let claim = "I've kept the light on until ten.";
    let went = worded(
        "Lamp, could you keep the light on until ten?",
        saying(claim),
        vec![saying(claim)],
    );
    assert_eq!(went.said, said(&[crate::action_claim::NOT_DONE_LINE]));
    assert_eq!((went.asked, went.ran.len()), (2, 0));
    assert!(told_claim(&went), "{:?}", went.notes);
}

/// `calling`, with the call's arguments.
fn calling_with(name: &str, args: Value) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call("c0", name, args)])),
        Ok(ChatEvent::Done),
    ]
}

/// The light rule `keep_rule` keeps: always, with no end time.
fn light_kept() -> Vec<net::Result<ChatEvent>> {
    calling_with(
        crate::rules::KEEP_RULE,
        json!({"rule": "Keep the light on.", "about": "light", "never": true}),
    )
}

/// The first builder's ruling on #5215 (lamp-52e6): "Hey Lamp, keep the
/// light on until ten." got a `keep_rule` call, which keeps the light on
/// always, and "I'll keep my light on until ten.", which claims more than
/// was done. A said limit no call carried is a claim with no call: held,
/// asked again once, and a second such claim is said as the not-done line.
#[test]
fn a_worded_claims_limit_no_call_carried_is_held() {
    let claim = "I'll keep my light on until ten.";
    let went = worded(
        "Hey Lamp, keep the light on until ten.",
        light_kept(),
        vec![saying(claim), saying(claim)],
    );
    assert_eq!(went.said, said(&[crate::action_claim::NOT_DONE_LINE]));
    assert_eq!(went.asked, 3);
    assert_eq!(went.ran, [crate::rules::KEEP_RULE]);
    assert!(
        went.notes
            .iter()
            .any(|note| note == crate::action_claim::LIMIT_NOTE),
        "{:?}",
        went.notes
    );
}

/// The same words with no limit said beside the kept rule (no worded claim:
/// a kept light's runs need "until", "till" or "for" and a time) are said,
/// byte for byte as with the worded claims off.
#[test]
fn a_worded_claim_with_no_limit_beside_a_kept_rule_stands() {
    let claim = "I'll keep my light on.";
    let went = |worded_on: bool| {
        turn_with(
            true,
            worded_on,
            true,
            "Hey Lamp, keep the light on.",
            light_kept(),
            vec![saying(claim)],
        )
    };
    assert_eq!(went(true), went(false));
    assert_eq!(went(true).said, said(&[claim]));
    assert_eq!(went(true).asked, 2);
}

/// A timer's length said in other words stands beside a set that carried
/// it ("for one minute", 60 seconds), and is held beside one that didn't
/// (300 seconds), as a claim beside calls of another kind.
#[test]
fn a_timers_length_said_stands_only_beside_a_set_that_carried_it() {
    let claim = "Timer set for one minute.";
    let heard = "Lamp, set a timer for one minute.";
    let set =
        |seconds: u64| calling_with("set_timer", json!({"text": "Timer", "seconds": seconds}));
    let went =
        |worded_on: bool| turn_with(true, worded_on, true, heard, set(60), vec![saying(claim)]);
    assert_eq!(went(true), went(false));
    assert!(went(true).notes.is_empty(), "{:?}", went(true).notes);
    // The words beside a set for five minutes: not said, the set runs and
    // its own line is said (`LAMP_SECOND_SAY_WITH_CALL`). Without the worded
    // claims they are said, in its place.
    let mut beside = vec![text(claim)];
    beside.extend(set(300));
    let went = |worded_on: bool| turn_with(true, worded_on, true, heard, beside.clone(), vec![]);
    assert_eq!(went(true).said, said(&["Timer set. It rings at 7:52 am."]));
    assert_eq!(went(true).ran, ["set_timer"]);
    assert_eq!(went(false).said, said(&[claim]));
}

/// With the worded claims off, a limit no call carried is said as before.
#[test]
fn a_said_limit_no_call_carried_is_said_with_the_worded_claims_off() {
    let claim = "I'll keep my light on until ten.";
    let went = claimed(
        true,
        "Hey Lamp, keep the light on until ten.",
        light_kept(),
        vec![saying(claim)],
    );
    assert_eq!(went.said, said(&[claim]));
    assert_eq!((went.asked, went.notes.len()), (2, 0));
}

/// The Reviewer-lead's P2 on #5737: the model often says a timer's result
/// as a clock; beside the set, which carries only its length, it stands.
#[test]
fn a_timers_ring_time_said_beside_its_set_stands() {
    let claim = "Your tea timer is set and will ring at 9:39.";
    let went = turn_with(
        true,
        true,
        false,
        "Lamp, set a tea timer for five minutes.",
        calling_with("set_timer", json!({"text": "Tea", "seconds": 300})),
        vec![saying(claim)],
    );
    assert_eq!(went.said, said(&[claim]));
    assert_eq!((went.asked, went.notes.len()), (2, 0), "{went:?}");
}

/// The Reviewer-lead's P3 on #5737, kept as it is: while the reply streams,
/// a worded claim is judged on what came by its first sentence's end, so a
/// limit in a later piece is not checked; in the same piece it is.
#[test]
fn a_limit_after_the_first_sentence_is_judged_only_in_its_piece() {
    let heard = "Lamp, remind me in ten minutes.";
    let set = || calling_with("set_timer", json!({"text": "Reminder", "seconds": 600}));
    let (first, then) = ("Reminder set. ", "It rings in five minutes.");
    let went = turn_with(
        true,
        true,
        false,
        heard,
        set(),
        vec![vec![text(first), text(then), Ok(ChatEvent::Done)]],
    );
    assert_eq!((went.asked, went.notes.len()), (2, 0), "{went:?}");
    assert!(
        went.said.iter().any(|said| said.contains("five minutes")),
        "{went:?}"
    );
    let whole = format!("{first}{then}");
    let went = turn_with(
        true,
        true,
        false,
        heard,
        set(),
        vec![saying(&whole), saying(&whole)],
    );
    assert_eq!(
        went.said,
        said(&[crate::action_claim::NOT_DONE_LINE]),
        "{went:?}"
    );
    assert_eq!(went.notes, [crate::action_claim::LIMIT_NOTE]);
}

/// The Reviewer-lead's P2 on #5737 at 09483c9afa: a timer set beside the
/// kept rule backs no kept light's clock: the rule still holds always.
#[test]
fn a_kept_lights_clock_beside_a_rule_and_a_timer_is_held() {
    let claim = "I'll keep my light on until ten.";
    let went = turn_with(
        true,
        true,
        false,
        "Keep the light on until ten and set a tea timer for five minutes.",
        vec![
            Ok(ChatEvent::ToolCalls(vec![
                call(
                    "c0",
                    crate::rules::KEEP_RULE,
                    json!({"rule": "Keep the light on.", "about": "light", "never": true}),
                ),
                call("c1", "set_timer", json!({"text": "Tea", "seconds": 300})),
            ])),
            Ok(ChatEvent::Done),
        ],
        vec![saying(claim), saying(claim)],
    );
    assert_eq!(
        went.said,
        said(&[crate::action_claim::NOT_DONE_LINE]),
        "{went:?}"
    );
    assert_eq!(went.notes, [crate::action_claim::LIMIT_NOTE]);
}

/// The same P2: a listing backs a time said only when the line asked to
/// read one; after "remind me at ten" it set nothing.
#[test]
fn a_reminders_time_beside_only_a_listing_after_a_set_ask_is_held() {
    let claim = "Your reminder is set for ten.";
    let went = turn_with(
        true,
        true,
        false,
        "Lamp, remind me at ten.",
        calling("list_reminders"),
        vec![saying(claim), saying(claim)],
    );
    assert_eq!(
        went.said,
        said(&[crate::action_claim::NOT_DONE_LINE]),
        "{went:?}"
    );
    assert_eq!(went.notes, [crate::action_claim::LIMIT_NOTE]);
}
