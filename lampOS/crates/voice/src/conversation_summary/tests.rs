//! The running summary's tests: when it is due, what it keeps, whose it
//! is, a forget, a failure, and never waiting on the turn.
use super::*;
use crate::thread::Thread;
use std::sync::mpsc::channel;

/// Notes of a test's own: other tests' forgets never reach them.
fn own_summaries() -> &'static Mutex<Summaries> {
    Box::leak(Box::new(Mutex::new(Summaries::new())))
}

/// One exchange of `whose`: the person's line, then the lamp's answer.
fn exchange(
    thread: &mut Thread,
    request: u64,
    person: &str,
    lamp: &str,
    whose: Option<&str>,
    at: Instant,
) {
    thread.person(person, true, at);
    thread.lamp_for(request, lamp, at, whose);
}

/// A model that folds as a careful one would: the quiz's rules from the
/// first line that gives them, and the latest score the lamp said, under
/// `Current activity:`; each person's line beginning "I " as a fact.
fn folding(input: &str) -> Option<String> {
    let job: serde_json::Value = serde_json::from_str(input).ok()?;
    let previous = job["previous"].as_str().unwrap_or_default();
    let lines = job["exchanges"].as_str().unwrap_or_default();
    let mut rules = previous
        .lines()
        .find_map(|line| line.strip_prefix("Current activity: quiz; rules: "))
        .map(|rest| rest.split("; score ").next().unwrap_or_default().to_owned());
    let mut score = previous.lines().find_map(|line| {
        line.rsplit_once("; score ")
            .map(|(_, score)| score.to_owned())
    });
    let mut facts: Vec<String> = previous
        .lines()
        .filter(|line| line.starts_with("- "))
        .map(str::to_owned)
        .collect();
    for line in lines.lines() {
        if let Some(said) = line.strip_prefix("Person: ") {
            if let Some(given) = said.strip_prefix("Let's play a quiz: ") {
                rules = Some(given.to_owned());
            } else if said.starts_with("I ") {
                facts.push(format!("- {said}"));
            }
        } else if let Some(said) = line.strip_prefix("You: ")
            && let Some((_, now)) = said.split_once("Score: ")
        {
            score = Some(now.trim_end_matches('.').to_owned());
        }
    }
    let activity = match rules {
        Some(rules) => format!(
            "Current activity: quiz; rules: {rules}; score {}",
            score.unwrap_or_else(|| "0".into())
        ),
        None => "Current activity: none".to_owned(),
    };
    Some(format!("{activity}\nKey facts:\n{}", facts.join("\n")))
}

/// After each turn, as `after_turn` does, but at once on this thread.
fn turn_end(thread: &mut Thread, summaries: &Mutex<Summaries>, now: Instant) {
    for due in thread.summary_due(KEEP_VERBATIM, now) {
        summarize_now(summaries, due.whose, due.exchanges, &folding, now);
    }
}

/// #449 defect 4 on lamp-06c1 (29 Oct): the quiz's rules at turn 1, then
/// 20 turns. The rules' exchange is long out of the thread, and the note
/// still has the rules and the latest score; the newest exchanges stay in
/// the thread word for word.
#[test]
fn a_quiz_twenty_turns_on_still_has_its_rules_and_score() {
    let summaries = own_summaries();
    let mut thread = Thread::new();
    let start = Instant::now();
    let rules = "ten questions on capitals, one point each, no hints";
    exchange(
        &mut thread,
        1,
        &format!("Let's play a quiz: {rules}"),
        "Great, first question: the capital of Peru?",
        Some("tom"),
        start,
    );
    turn_end(&mut thread, summaries, start);
    for turn in 2..=21u64 {
        let at = start + Duration::from_secs(turn * 10);
        exchange(
            &mut thread,
            turn,
            &format!("Answer number {turn}"),
            &format!("Right. Score: {turn}. Next one."),
            Some("tom"),
            at,
        );
        turn_end(&mut thread, summaries, at);
    }
    let now = start + Duration::from_secs(215);
    let thread_told = thread
        .recall_for(Some("tom"), now, 64 * 1024)
        .unwrap_or_default();
    assert!(!thread_told.contains(rules), "the rules left the thread");
    let told = told_from(summaries, Some("tom"), now, 64 * 1024);
    assert!(told.starts_with(HEADING), "{told}");
    let note = told.strip_prefix(HEADING).unwrap();
    assert!(note.starts_with("Current activity: quiz"), "{note}");
    assert!(note.contains(rules), "{note}");
    // The score of the last exchange given: older than the newest four
    // kept word for word, and at most a batch behind them.
    let score: u64 = note
        .lines()
        .next()
        .and_then(|line| line.rsplit_once("; score "))
        .and_then(|(_, score)| score.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|score| score.parse().ok())
        .expect(note);
    let newest_given = 21 - KEEP_VERBATIM as u64;
    let batch = (Thread::KEPT_A_SCOPE - KEEP_VERBATIM - 1) as u64;
    assert!(
        (newest_given + 1 - batch..=newest_given).contains(&score),
        "{note}"
    );
    assert!(thread_told.contains("Answer number 21"));
}

/// Every exchange is given to the summary before the thread drops it,
/// once each, oldest first, never the newest four.
#[test]
fn every_exchange_is_summarised_once_before_it_leaves_the_thread() {
    let mut thread = Thread::new();
    let start = Instant::now();
    let mut given = Vec::new();
    for request in 1..=40u64 {
        let at = start + Duration::from_secs(request);
        exchange(&mut thread, request, "a line", "an answer", None, at);
        for due in thread.summary_due(KEEP_VERBATIM, at) {
            assert_eq!(due.whose, None);
            given.extend(due.exchanges.iter().map(|(request, _, _)| *request));
        }
        // Whatever the thread still has from the start of the run is
        // given, or still in the thread: it keeps the newest
        // `Thread::KEPT_A_SCOPE` of a scope.
        let oldest_kept = request
            .saturating_sub(Thread::KEPT_A_SCOPE as u64 - 1)
            .max(1);
        assert!(
            (1..oldest_kept).all(|old| given.contains(&old)),
            "request {request}: {given:?}"
        );
        assert!(
            given
                .iter()
                .all(|&old| old + KEEP_VERBATIM as u64 <= request)
        );
    }
    let mut sorted = given.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, given, "once each, oldest first");
}

/// A slow talk, too few exchanges to fill a batch, is given before its
/// oldest ages out of the thread.
#[test]
fn a_slow_talk_is_summarised_before_it_ages_out() {
    let mut thread = Thread::new();
    let start = Instant::now();
    for request in 1..=5u64 {
        let at = start + Duration::from_secs(request);
        exchange(
            &mut thread,
            request,
            "a slow line",
            "an answer",
            Some("ana"),
            at,
        );
        assert!(thread.summary_due(KEEP_VERBATIM, at).is_empty());
    }
    let later = start + crate::thread::REMEMBERED_FOR * 3 / 4 + Duration::from_secs(2);
    let due = thread.summary_due(KEEP_VERBATIM, later);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].whose.as_deref(), Some("ana"));
    assert_eq!(due[0].exchanges.len(), 1);
}

/// A scope's note is its own: another person's turns, the room's and an
/// unplaced voice's (room scope) are never told it.
#[test]
fn a_note_is_told_to_its_own_scope_alone() {
    let summaries = own_summaries();
    let now = Instant::now();
    summarize_now(
        summaries,
        Some("tom".into()),
        vec![(1, "I live in Lima".into(), "Nice.".into())],
        &folding,
        now,
    );
    assert!(told_from(summaries, Some("tom"), now, 64 * 1024).contains("I live in Lima"));
    assert_eq!(told_from(summaries, Some("mai"), now, 64 * 1024), "");
    assert_eq!(told_from(summaries, None, now, 64 * 1024), "");
    // And the room's note is never a person's.
    summarize_now(
        summaries,
        None,
        vec![(2, "I am the room's".into(), "Fine.".into())],
        &folding,
        now,
    );
    assert!(!told_from(summaries, Some("tom"), now, 64 * 1024).contains("room's"));
    assert!(told_from(summaries, None, now, 64 * 1024).contains("room's"));
}

/// The worker's system message tells a turn its own scope's note, before
/// the thread's lines, and no other scope's.
#[test]
fn the_system_message_tells_the_turns_own_note_only() {
    let _one = crate::provider_worker::tests::one_thread();
    *crate::thread::THREAD.lock().unwrap() = Thread::new();
    let whose = "summary-test-person";
    let fact = "I keep bees on the roof";
    // A forget elsewhere in the test run may clear the global notes in
    // between: kept again, a few times at most.
    let mut told = String::new();
    for _ in 0..5 {
        summarize_now(
            &SUMMARIES,
            Some(whose.into()),
            vec![(1, fact.into(), "Lovely.".into())],
            &folding,
            Instant::now(),
        );
        told = crate::second_provider::worker::steady_first_message_withheld(
            String::new(),
            "",
            None,
            Some(whose),
            None,
        )
        .after
        .unwrap_or_default();
        if told.contains(fact) {
            break;
        }
    }
    assert!(told.contains("Earlier in this conversation"), "{told}");
    assert!(told.contains(fact), "{told}");
    for other in [None, Some("summary-test-other")] {
        let told = crate::second_provider::worker::steady_first_message_withheld(
            String::new(),
            "",
            None,
            other,
            None,
        )
        .after
        .unwrap_or_default();
        assert!(!told.contains(fact), "{other:?}: {told}");
    }
    forget_all();
}

/// Any forget drops every note, and a note being made when it came is
/// never kept; the thread's own forget does too.
#[test]
fn a_forget_clears_every_note_and_any_in_the_making() {
    let summaries = own_summaries();
    let now = Instant::now();
    summarize_now(
        summaries,
        Some("tom".into()),
        vec![(1, "I live in Lima".into(), "Nice.".into())],
        &folding,
        now,
    );
    summaries.lock().unwrap().forget();
    assert_eq!(told_from(summaries, Some("tom"), now, 64 * 1024), "");
    // A forget while the model answers: the answer is dropped.
    let forgetting = |input: &str| {
        summaries.lock().unwrap().forget();
        folding(input)
    };
    summarize_now(
        summaries,
        Some("tom".into()),
        vec![(2, "I live in Hue".into(), "Nice.".into())],
        &forgetting,
        now,
    );
    assert_eq!(told_from(summaries, Some("tom"), now, 64 * 1024), "");
    // The process's notes, by the thread's forget.
    summarize_now(
        &SUMMARIES,
        Some("summary-forget-test".into()),
        vec![(3, "I live in Hue".into(), "Nice.".into())],
        &folding,
        now,
    );
    Thread::new().forget();
    assert_eq!(for_scope(Some("summary-forget-test"), now), None);
}

/// A failed or empty answer keeps the note before it, and is counted.
#[test]
fn a_failure_keeps_the_previous_note_and_is_counted() {
    let summaries = own_summaries();
    let now = Instant::now();
    summarize_now(
        summaries,
        None,
        vec![(1, "I live in Lima".into(), "Nice.".into())],
        &folding,
        now,
    );
    let before = told_from(summaries, None, now, 64 * 1024);
    let failed = FAILED.load(Ordering::Acquire);
    summarize_now(
        summaries,
        None,
        vec![(2, "x".into(), "y".into())],
        &|_| None,
        now,
    );
    assert!(FAILED.load(Ordering::Acquire) > failed);
    assert_eq!(told_from(summaries, None, now, 64 * 1024), before);
    let empty = EMPTY.load(Ordering::Acquire);
    summarize_now(
        summaries,
        None,
        vec![(3, "x".into(), "y".into())],
        &|_| Some("  \n".into()),
        now,
    );
    assert!(EMPTY.load(Ordering::Acquire) > empty);
    assert_eq!(told_from(summaries, None, now, 64 * 1024), before);
}

/// The turn's end hands the job on and returns at once, the model still
/// asking; the next turn is told the note before it; then the new one.
#[test]
fn the_summary_never_holds_up_a_turn() {
    let summaries = own_summaries();
    let start = Instant::now();
    summaries.lock().unwrap().keep(
        None,
        "Current activity: none\nKey facts:\n- first".into(),
        start,
    );
    let (release, released) = channel::<()>();
    let released = Mutex::new(released);
    let (asked, asking) = channel::<()>();
    let asked = Mutex::new(asked);
    let queue = Queue::start(
        Box::new(move |input| {
            let _ = asked.lock().unwrap().send(());
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10));
            folding(input)
        }),
        summaries,
        Box::leak(Box::new(Mutex::new(Thread::new()))),
    )
    .unwrap();
    let thread = Mutex::new(Thread::new());
    // The fewest exchanges that make a batch due: one less than a scope
    // keeps.
    let due_at = Thread::KEPT_A_SCOPE as u64 - 1;
    for request in 1..=due_at {
        let at = start + Duration::from_secs(request);
        exchange(
            &mut thread.lock().unwrap(),
            request,
            &format!("I said line {request}"),
            "Mm.",
            None,
            at,
        );
    }
    let now = start + Duration::from_secs(due_at + 1);
    let began = Instant::now();
    queue.after_turn(&thread, now);
    assert!(
        began.elapsed() < Duration::from_millis(200),
        "{:?}",
        began.elapsed()
    );
    asking
        .recv_timeout(Duration::from_secs(5))
        .expect("the model was asked");
    // The model still answering: the previous note.
    assert!(told_from(summaries, None, now, 64 * 1024).contains("- first"));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !told_from(summaries, None, now, 64 * 1024).contains("I said line 3") {
        assert!(Instant::now() < deadline, "the note was never updated");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A note over the cap loses its oldest facts, never its activity or its
/// newest facts, and never its end.
#[test]
fn a_long_note_drops_its_oldest_facts_first() {
    let facts: Vec<String> = (0..400).map(|n| format!("- fact number {n:03}")).collect();
    let text = format!(
        "Current activity: quiz, question 9, score 7\nKey facts:\n{}",
        facts.join("\n")
    );
    let kept = bounded(&text);
    assert!(kept.len() <= MAX_BYTES);
    assert!(kept.starts_with("Current activity: quiz, question 9, score 7\nKey facts:\n"));
    assert!(kept.ends_with("- fact number 399"));
    assert!(!kept.contains("- fact number 000"));
    // One line alone over the cap keeps its end.
    let long = "x".repeat(MAX_BYTES) + "the end";
    assert!(bounded(&long).ends_with("the end"));
    assert!(bounded(&long).len() <= MAX_BYTES);
}

/// The model is given the person's words as the person's and the lamp's as
/// its own, as quoted data with the previous note.
#[test]
fn the_job_marks_whose_words_are_whose() {
    let job: serde_json::Value = serde_json::from_str(&input(
        "Current activity: none",
        &[(1, "my budget is 300".into(), "Got it.".into())],
    ))
    .unwrap();
    assert_eq!(job["previous"], "Current activity: none");
    assert_eq!(job["exchanges"], "Person: my budget is 300\nYou: Got it.\n");
}

/// A note no exchange of its scope kept fresh is gone after the thread's
/// time; one whose scope still talks stays.
#[test]
fn a_note_goes_when_its_conversation_is_over() {
    let summaries = own_summaries();
    let start = Instant::now();
    summaries
        .lock()
        .unwrap()
        .keep(Some("tom".into()), "n".into(), start);
    summaries
        .lock()
        .unwrap()
        .keep(Some("mai".into()), "m".into(), start);
    let later = start + crate::thread::REMEMBERED_FOR - Duration::from_secs(1);
    summaries
        .lock()
        .unwrap()
        .touch(&[Some("tom".into())], later);
    let after = start + crate::thread::REMEMBERED_FOR + Duration::from_secs(1);
    assert_eq!(summaries.lock().unwrap().of(Some("mai"), after), None);
    assert_eq!(
        summaries.lock().unwrap().of(Some("tom"), after).as_deref(),
        Some("n")
    );
}

/// What a conversation's end kept as its talk note, through [`end`].
fn ended_with(
    summaries: &Mutex<Summaries>,
    thread: &Mutex<Thread>,
    whose: Option<&str>,
    until: Instant,
    still: bool,
    with: &(dyn Fn(&str) -> Option<String> + Send + Sync),
) -> Option<String> {
    let kept = Arc::new(Mutex::new(None));
    let into = kept.clone();
    let forgets = summaries.lock().unwrap().forgets;
    end(
        Ended {
            whose: whose.map(str::to_owned),
            until,
            keep: Box::new(move |text| {
                *into.lock().unwrap() = Some(text.to_owned());
                true
            }),
            still: Box::new(move || still),
        },
        forgets,
        with,
        summaries,
        thread,
    );
    kept.lock().unwrap().clone()
}

/// At a conversation's end its note, the exchanges not yet in it folded
/// in, is kept as its talk note in its own scope, and goes from here; one
/// too short to have a note keeps none; the room's conversation is the
/// room's.
#[test]
fn a_conversations_end_keeps_its_note_with_the_rest_folded_in() {
    let summaries = own_summaries();
    let thread = Mutex::new(Thread::new());
    let start = Instant::now();
    for (request, whose) in [(1, Some("tom")), (2, None)] {
        exchange(
            &mut thread.lock().unwrap(),
            request,
            "Let's play a quiz: three questions on rivers",
            "First: the longest river?",
            whose,
            start,
        );
    }
    // Too short: no note, nothing kept.
    assert_eq!(
        ended_with(summaries, &thread, Some("tom"), start, true, &folding),
        None
    );
    summarize_now(
        summaries,
        Some("tom".into()),
        vec![(9, "I live in Lima".into(), "Nice.".into())],
        &folding,
        start,
    );
    let kept = ended_with(summaries, &thread, Some("tom"), start, true, &folding).unwrap();
    assert!(kept.contains("I live in Lima"), "{kept}");
    assert!(kept.contains("three questions on rivers"), "{kept}");
    assert_eq!(told_from(summaries, Some("tom"), start, 64 * 1024), "");
    // The room's: its own note, its own exchanges.
    summarize_now(
        summaries,
        None,
        vec![(8, "I am the room's".into(), "Fine.".into())],
        &folding,
        start,
    );
    let room = ended_with(summaries, &thread, None, start, true, &folding).unwrap();
    assert!(room.contains("room's") && !room.contains("Lima"), "{room}");
}

/// A forget since the conversation began, or one while its end is made,
/// keeps no talk note; a model that fails at the end keeps the note so far.
#[test]
fn a_forget_keeps_no_talk_note_and_a_failure_keeps_the_note_so_far() {
    let summaries = own_summaries();
    let thread = Mutex::new(Thread::new());
    let now = Instant::now();
    let note = |summaries: &Mutex<Summaries>| {
        summarize_now(
            summaries,
            Some("tom".into()),
            vec![(1, "I live in Lima".into(), "Nice.".into())],
            &folding,
            now,
        );
    };
    note(summaries);
    assert_eq!(
        ended_with(summaries, &thread, Some("tom"), now, false, &folding),
        None
    );
    note(summaries);
    let forgetting = |input: &str| {
        summaries.lock().unwrap().forget();
        folding(input)
    };
    exchange(
        &mut thread.lock().unwrap(),
        2,
        "I like rivers",
        "Mm.",
        Some("tom"),
        now,
    );
    assert_eq!(
        ended_with(summaries, &thread, Some("tom"), now, true, &forgetting),
        None
    );
    note(summaries);
    exchange(
        &mut thread.lock().unwrap(),
        3,
        "I like lakes",
        "Mm.",
        Some("tom"),
        now,
    );
    let failed = FAILED.load(Ordering::Acquire);
    let kept = ended_with(summaries, &thread, Some("tom"), now, true, &|_| None).unwrap();
    assert!(FAILED.load(Ordering::Acquire) > failed);
    assert!(
        kept.contains("I live in Lima") && !kept.contains("lakes"),
        "{kept}"
    );
}

/// The reviewer-lead's P2 on #4479: an exchange answered in a held scope (a
/// face's gap, or a line carried as the person's conversation) is told in
/// the thread as ever, but never reaches the model for the running note,
/// nor the rest folded in at the conversation's end, so nothing of it is in
/// the note or the talk note kept on disk.
#[test]
fn a_held_exchange_never_reaches_the_summary_or_the_talk_note() {
    let summaries = own_summaries();
    let thread = Mutex::new(Thread::new());
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = asked.clone();
    let recording = move |input: &str| {
        seen.lock().unwrap().push(input.to_owned());
        folding(input)
    };
    let start = Instant::now();
    let tom = Some("tom");
    for request in 1..=(Thread::KEPT_A_SCOPE as u64 + 2) {
        let at = start + Duration::from_secs(request);
        let mut thread = thread.lock().unwrap();
        if request % 2 == 0 {
            exchange(
                &mut thread,
                request,
                &format!("I held-{request}"),
                "Mm.",
                tom,
                at,
            );
            thread.held_for(request);
        } else {
            exchange(
                &mut thread,
                request,
                &format!("I plain-{request}"),
                "Ok.",
                tom,
                at,
            );
        }
        for due in thread.summary_due(KEEP_VERBATIM, at) {
            summarize_now(summaries, due.whose, due.exchanges, &recording, at);
        }
    }
    // The held lines are still told in the thread.
    let told = thread
        .lock()
        .unwrap()
        .recall_for(tom, start + Duration::from_secs(20), usize::MAX)
        .unwrap();
    assert!(told.contains("held-14"), "{told}");
    let note = told_from(summaries, tom, start, 64 * 1024);
    assert!(note.contains("plain-1"), "{note}");
    let until = start + Duration::from_secs(30);
    let kept = ended_with(summaries, &thread, tom, until, true, &recording).unwrap();
    assert!(kept.contains("plain-13"), "{kept}");
    assert!(!kept.contains("held-"), "{kept}");
    let asked = asked.lock().unwrap();
    assert!(asked.len() >= 2, "a fold and the end were asked");
    assert!(
        asked.iter().all(|input| !input.contains("held-")),
        "{asked:?}"
    );
}

/// The reviewer-lead's P2 on #4479: memory's forget from the app
/// (`memory/forgot`) reaches the conversation as a voice-tool forget does:
/// a person's, the room's ("everyone") or every person's drops the thread,
/// every note and a job being made; only memory's are taken, and a rules
/// scope is no conversation.
#[test]
fn a_forget_from_the_app_drops_the_notes_and_any_in_the_making() {
    let root = std::path::PathBuf::from(format!("/tmp/lvs-forgot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut forgot = crate::bus_out::ForgotIn::on(&bus).unwrap();
    let publisher = |source| {
        bus.publisher_declared(
            lamp_msgs::MemoryForgot::TOPIC,
            &lamp_msgs::MemoryForgot::declaration(),
            source,
        )
        .unwrap()
    };
    let (mut memory, mut eyes) = (publisher("memory"), publisher("eyes"));
    let tom = crate::remember::person_id("Tom");
    let told = |person: Option<&str>| lamp_msgs::MemoryForgot {
        person: person.map(str::to_owned),
    };
    assert!(memory_forgot_talk(None) && memory_forgot_talk(Some("everyone")));
    assert!(memory_forgot_talk(Some(&tom)) && !memory_forgot_talk(Some("r-house")));
    let summaries = own_summaries();
    let thread = Mutex::new(Thread::new());
    let now = Instant::now();
    let noted = |summaries: &Mutex<Summaries>, request| {
        summarize_now(
            summaries,
            Some(tom.clone()),
            vec![(request, "I live in Lima".into(), "Nice.".into())],
            &folding,
            now,
        );
    };
    noted(summaries, 1);
    exchange(
        &mut thread.lock().unwrap(),
        1,
        "I live in Lima",
        "Nice.",
        Some(&tom),
        now,
    );
    // Someone else's forget is not memory's: nothing goes.
    eyes.publish_json(1, &told(Some(&tom))).unwrap();
    assert_eq!(
        memory_forgot_from(&mut forgot, 1_000_000, summaries, &thread),
        0
    );
    assert!(told_from(summaries, Some(&tom), now, 64 * 1024).contains("Lima"));
    // The app forgets Tom: the thread and the note go.
    memory.publish_json(2, &told(Some(&tom))).unwrap();
    assert_eq!(
        memory_forgot_from(&mut forgot, 2_000_000, summaries, &thread),
        1
    );
    assert_eq!(told_from(summaries, Some(&tom), now, 64 * 1024), "");
    assert_eq!(
        thread
            .lock()
            .unwrap()
            .recall_for(Some(&tom), now, usize::MAX),
        None
    );
    // A forget of everything while a note is being made: it is never kept.
    let (memory, forgot) = (Mutex::new(memory), Mutex::new(forgot));
    let forgetting = |input: &str| {
        memory.lock().unwrap().publish_json(3, &told(None)).unwrap();
        let mut forgot = forgot.lock().unwrap();
        assert_eq!(
            memory_forgot_from(&mut forgot, 3_000_000, summaries, &thread),
            1
        );
        folding(input)
    };
    summarize_now(
        summaries,
        Some(tom.clone()),
        vec![(2, "I live in Hue".into(), "Nice.".into())],
        &forgetting,
        now,
    );
    assert_eq!(told_from(summaries, Some(&tom), now, 64 * 1024), "");
    let _ = std::fs::remove_dir_all(root);
}

/// Tests' P2 on #4479: a forget landing after a turn's end has read the
/// forget count but before it takes the exchanges leaves the job's count
/// stale, so the model's answer is never kept: no forgotten word stays in
/// the note.
#[test]
fn a_forget_between_the_count_and_the_exchanges_keeps_no_note() {
    let summaries = own_summaries();
    let thread = Mutex::new(Thread::new());
    let start = Instant::now();
    for request in 1..=(Thread::KEPT_A_SCOPE as u64) {
        exchange(
            &mut thread.lock().unwrap(),
            request,
            &format!("I said-{request}"),
            "Ok.",
            Some("tom"),
            start,
        );
    }
    let now = start + Duration::from_secs(1);
    let jobs = jobs_due(summaries, &thread, now, || {
        summaries.lock().unwrap().forget()
    });
    assert!(!jobs.is_empty(), "the exchanges were due");
    for job in jobs {
        summarize(job, &folding, summaries, || now);
    }
    assert_eq!(told_from(summaries, Some("tom"), now, 64 * 1024), "");
    // With no forget, the same order keeps the note.
    let thread = Mutex::new(Thread::new());
    for request in 1..=(Thread::KEPT_A_SCOPE as u64) {
        exchange(
            &mut thread.lock().unwrap(),
            request,
            &format!("I said-{request}"),
            "Ok.",
            Some("tom"),
            start,
        );
    }
    for job in jobs_due(summaries, &thread, now, || {}) {
        summarize(job, &folding, summaries, || now);
    }
    assert!(told_from(summaries, Some("tom"), now, 64 * 1024).contains("said-1"));
}
