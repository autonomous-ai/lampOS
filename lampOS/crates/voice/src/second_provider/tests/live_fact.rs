//! `LAMP_LIVE_FACT_CHECKED` on the second route (#7605): a sentence of a
//! reply stating a live figure that no lookup in its turn found is not
//! said, judged by the reply's own words, never the person's (the
//! reviewer-lead's HOLD on #7719). The model is asked once more with the
//! lookup required; a second such reply, a lookup that failed or found
//! nothing, the last round, or no lookup declared, is said as the plain
//! line that it couldn't look it up. A turn that looked it up, and an
//! answer with no live figure, go on byte for byte as without the switch.
use super::*;

/// The turns as they went.
#[derive(Debug, PartialEq)]
struct Went {
    /// What was said, as `said:` labels, every turn's.
    said: Vec<String>,
    /// The chat rounds asked.
    asked: usize,
    /// The calls run, by name.
    ran: Vec<String>,
    /// The live-figure notes the rounds were asked with, each once.
    notes: Vec<String>,
    /// Every "(For you to know" note any round was told, each once.
    told: Vec<String>,
    /// Each round's last two messages' words.
    lasts: Vec<Vec<String>>,
}

/// How a test line comes.
#[derive(Clone, Copy, Default, PartialEq)]
enum Input {
    /// Heard.
    #[default]
    Spoken,
    /// An add-on's quoted line (`say::quoted_turn`, `quoted: true`).
    Quoted,
    /// A due reminder, said by the lamp (`saying_reminders`).
    Reminder,
}

/// How a test turn is set up.
#[derive(Clone, Copy, Default)]
struct Setup {
    /// How each line comes.
    input: Input,
    /// `LAMP_LIVE_FACT_CHECKED`.
    on: bool,
    /// What every lookup answers, in place of what it finds.
    answers: Option<fn() -> Value>,
    /// `LAMP_ACTION_CLAIM_CHECKED`.
    claims: bool,
    /// The first words cut at four (`LAMP_SECOND_FIRST_WORDS`).
    cut: bool,
    /// Functions the chat declares none of.
    undeclared: &'static [&'static str],
    /// Round 1's words held to its end as a line that may not be for the
    /// lamp (`LAMP_DECLINE_SILENT`, `worker::DECLINE_NOTE`), with the
    /// don't-know recall on and nothing kept fitting
    /// (`LAMP_MEMORY_DONT_KNOW_RECALLS`): the round's end's don't-know
    /// release says them.
    declined: bool,
}

/// What a lookup finds here.
fn looked_up(name: &str) -> Value {
    match name {
        "weather" => json!({"place": "Da Nang, Vietnam", "weather": "27 degrees, light rain"}),
        _ => json!({"results": [{"title": "Gold price today", "text": "$4,061 an ounce"}]}),
    }
}

fn failing() -> Value {
    json!({"error": "the search did not answer in time"})
}

fn empty() -> Value {
    json!({"results": []})
}

/// Each of `lines` heard in its own turn, with its rounds' events.
fn turns(setup: Setup, lines: Vec<(&str, Vec<Vec<net::Result<ChatEvent>>>)>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new(
            lines
                .iter()
                .map(|(heard, _)| Ok((*heard).to_owned()))
                .collect(),
        ),
        undeclared: setup.undeclared,
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = if crate::live_fact::is_lookup(&name) {
            setup
                .answers
                .map_or_else(|| looked_up(&name), |answers| answers())
        } else {
            json!({"ok": true})
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
    .checking_action_claims(setup.claims)
    .saying_reminders(setup.input == Input::Reminder)
    .checking_live_facts(setup.on);
    if setup.cut {
        engine = engine.cutting_first_words(4);
    }
    if setup.declined {
        engine = engine
            .noting_whether_called(Arc::new(|_, _| Some(worker::DECLINE_NOTE)))
            .declining_silently(true)
            .recalling_before_a_dont_know(Arc::new(|_, _, _| None));
    }
    let mut labels = Vec::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (at, (line, rounds)) in lines.into_iter().enumerate() {
        let request = at as u64 + 1;
        script.chats.lock().unwrap().extend(rounds);
        labels.extend(runtime.block_on(async {
            if request > 1 {
                engine.retire_through(request - 1);
            }
            match setup.input {
                Input::Spoken => {
                    engine.submit(start(request));
                    engine.submit(audio(request, 1));
                    engine.submit(end(request));
                }
                Input::Quoted => engine.submit(ProviderInput::Written {
                    search: false,
                    request,
                    session: 1,
                    privacy_generation: 1,
                    text: crate::say::quoted_turn(line),
                    look: false,
                    quoted: true,
                }),
                Input::Reminder => engine.submit(reminder(request, line)),
            }
            turn(&mut engine).await
        }));
    }
    let asked = script.asked.lock().unwrap();
    // A note counts once, in the round it was asked with as the last
    // message (the round its lookup is forced); later rounds carry it.
    let notes = asked
        .iter()
        .filter_map(|messages| messages.last())
        .filter_map(Message::words)
        .filter(|words| words.starts_with(crate::live_fact::NOTE_OPENS))
        .map(str::to_owned)
        .collect();
    let mut told: Vec<String> = Vec::new();
    for words in asked
        .iter()
        .flatten()
        .filter(|message| message.role == "system")
        .filter_map(Message::words)
        .filter(|words| words.starts_with("(For you to know"))
    {
        if !told.iter().any(|note| note == words) {
            told.push(words.to_owned());
        }
    }
    let lasts = asked
        .iter()
        .map(|messages| {
            messages
                .iter()
                .rev()
                .take(2)
                .rev()
                .map(|message| message.words().unwrap_or_default().to_owned())
                .collect()
        })
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
        told,
        lasts,
    }
}

/// [`turns`], the switch `on`, lookups found, or failing when `failing`.
fn lines(on: bool, fails: bool, lines: Vec<(&str, Vec<Vec<net::Result<ChatEvent>>>)>) -> Went {
    turns(
        Setup {
            on,
            answers: fails.then_some(failing as fn() -> Value),
            ..Setup::default()
        },
        lines,
    )
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

/// `words`, then calls of `names`, in one round.
fn saying_and_calling(words: &str, names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| call(&format!("c{at}"), name, json!({})))
        .collect();
    vec![
        text(words),
        Ok(ChatEvent::ToolCalls(calls)),
        Ok(ChatEvent::Done),
    ]
}

/// `words` streamed a word at a time.
fn streaming(words: &str) -> Vec<net::Result<ChatEvent>> {
    let mut events: Vec<_> = words.split_inclusive(' ').map(text).collect();
    events.push(Ok(ChatEvent::Done));
    events
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

const GOLD: &str = "Lamp, what is the gold price today?";
const GOLD_MADE_UP: &str = "Gold is around $4,050 an ounce today.";
const GOLD_FOUND: &str = "Gold is $4,061 an ounce today.";
const COULD_NOT: &str = crate::live_fact::COULD_NOT_LINE;

/// lamp-9adf, 18 Nov, request 5: "what is the gold price today?" got
/// "around $4,050 an ounce" with `calls=none`. With the switch it is not
/// said: the model is asked again with `web_search` required, and what the
/// search found is said. Off, the made-up price is said, as on the lamp.
#[test]
fn a_price_today_said_with_no_lookup_is_asked_again_with_the_search() {
    let went = lines(
        true,
        false,
        vec![(
            GOLD,
            vec![
                saying(GOLD_MADE_UP),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert_eq!(went.said, said(&[GOLD_FOUND]), "{went:?}");
    assert_eq!(went.ran, ["web_search"]);
    assert_eq!(went.asked, 3);
    assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
    assert!(went.said.iter().all(|said| !said.contains("4,050")));
    // Off: said as it came, nothing asked again.
    let went = lines(false, false, vec![(GOLD, vec![saying(GOLD_MADE_UP)])]);
    assert_eq!(went.said, said(&[GOLD_MADE_UP]));
    assert_eq!((went.asked, went.ran.len()), (1, 0));
    assert!(went.notes.is_empty());
}

/// Twelve's P2 on #7719: with `LAMP_ACTION_CLAIM_CHECKED` on, a reply
/// that claims no action is let go by the claim check's own path
/// (`answer()`'s no-claim release); a made-up price on it is held and asked
/// again just the same, never said.
#[test]
fn with_the_action_claims_a_price_with_no_claim_is_held_the_same() {
    let went = turns(
        Setup {
            on: true,
            claims: true,
            ..Setup::default()
        },
        vec![(
            GOLD,
            vec![
                saying(GOLD_MADE_UP),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert_eq!(went.said, said(&[GOLD_FOUND]), "{went:?}");
    assert_eq!(went.ran, ["web_search"]);
    assert_eq!(went.asked, 3);
    assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
    assert!(
        !went
            .told
            .iter()
            .any(|note| note == crate::action_claim::NOTE),
        "{went:?}"
    );
    // A claim that stands beside its call ("Timer set." beside `set_timer`) is
    // held to the round's end and let go there, sentence by sentence: the
    // made-up price after it is held there too (a sentence follows it, so
    // it is let go there, not at the split's finish), and the next round
    // must call `web_search`.
    let went = turns(
        Setup {
            on: true,
            claims: true,
            ..Setup::default()
        },
        vec![(
            "Lamp, set a timer for one minute and tell me the gold price today.",
            vec![
                saying_and_calling(
                    &format!("Timer set. {GOLD_MADE_UP} Prices move a lot."),
                    &["set_timer"],
                ),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert!(
        went.said.iter().all(|said| !said.contains("4,050")),
        "{went:?}"
    );
    assert_eq!(
        went.said.first().map(String::as_str),
        Some("said:Timer set.")
    );
    assert!(went.said.concat().contains("4,061"), "{went:?}");
    assert_eq!(went.ran, ["set_timer", "web_search"]);
    assert_eq!(
        went.notes,
        [crate::live_fact::note_after("web_search", "Timer set.")]
    );
}

/// A quoted line (an add-on's, asked with no functions) and a due
/// reminder's ring are not judged: a figure in them is said as with the
/// switch off, and nothing is asked again.
#[test]
fn a_quoted_line_and_a_reminder_are_not_judged() {
    for (input, line) in [
        (Input::Quoted, "Say the gold price."),
        (Input::Reminder, "check the gold price"),
    ] {
        let run = |on| {
            turns(
                Setup {
                    on,
                    input,
                    ..Setup::default()
                },
                vec![(line, vec![saying(GOLD_MADE_UP)])],
            )
        };
        let on = run(true);
        assert_eq!(on, run(false), "{line}");
        assert_eq!(on.said, said(&[GOLD_MADE_UP]), "{line}: {on:?}");
        assert_eq!(on.asked, 1, "{line}");
        assert!(on.notes.is_empty(), "{line}");
    }
}

/// The reviewer-lead's HOLD on #7719: the person's words are never read.
/// A line with no live word ("Gold?") answered with a made-up price is
/// held just the same; a line full of live words ("Tell me a joke now")
/// answered with no live figure stands.
#[test]
fn the_reply_alone_is_judged_never_the_persons_words() {
    let went = lines(
        true,
        false,
        vec![(
            "Lamp, gold?",
            vec![
                saying(GOLD_MADE_UP),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert_eq!(went.said, said(&[GOLD_FOUND]), "{went:?}");
    assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
    for (heard, reply) in [
        (
            "Tell me a joke now.",
            "Why did the lamp go to school? To get brighter. I'm 100% sure you'll laugh.",
        ),
        (
            "What should I cook tonight?",
            "Use 200 grams of chicken and bake it at 180°C.",
        ),
        (
            "What's the temperature of boiling water?",
            "Water boils at 100 degrees Celsius.",
        ),
    ] {
        let on = lines(true, false, vec![(heard, vec![saying(reply)])]);
        let off = lines(false, false, vec![(heard, vec![saying(reply)])]);
        assert_eq!(on, off, "{heard}");
        assert!(on.notes.is_empty(), "{heard}");
    }
}

/// lamp-9adf, 18 Nov, requests 14 and 15: Da Nang's weather "right now"
/// and "tomorrow" were answered from Ho Chi Minh City's, fetched in an
/// earlier turn, with `calls=none`. That earlier lookup backs nothing now:
/// "right now" is asked again with `weather` required and its answer said;
/// "tomorrow", a second figure with no call after the ask again, is said
/// as the line that it couldn't be looked up. Never Ho Chi Minh City's.
#[test]
fn another_places_weather_from_an_earlier_turn_is_held() {
    let hcmc = "It's 33 degrees and sunny in Ho Chi Minh City.";
    let copied_now = "It's 33 degrees and sunny in Da Nang too.";
    let found_now = "Da Nang is 27 degrees with light rain.";
    let copied_tomorrow = "Tomorrow Da Nang will be sunny and hot too.";
    let run = |on| {
        lines(
            on,
            false,
            vec![
                (
                    "What's the weather in Ho Chi Minh City right now?",
                    vec![calling("weather"), saying(hcmc)],
                ),
                // Off, each turn takes its first round alone: the rounds
                // after it are the ask again's.
                (
                    "Lamp, what is the weather like in Da Nang right now?",
                    if on {
                        vec![saying(copied_now), calling("weather"), saying(found_now)]
                    } else {
                        vec![saying(copied_now)]
                    },
                ),
                (
                    "And what about Da Nang tomorrow?",
                    if on {
                        vec![saying(copied_tomorrow), saying(copied_tomorrow)]
                    } else {
                        vec![saying(copied_tomorrow)]
                    },
                ),
            ],
        )
    };
    let went = run(true);
    assert_eq!(went.said, said(&[hcmc, found_now, COULD_NOT]), "{went:?}");
    assert_eq!(went.ran, ["weather", "weather"]);
    // The weather is asked for by the held words.
    assert_eq!(
        went.notes,
        [
            crate::live_fact::note("weather"),
            crate::live_fact::note("weather")
        ]
    );
    // Off: the copied weather is said both times, as on the lamp.
    let went = run(false);
    assert_eq!(went.said, said(&[hcmc, copied_now, copied_tomorrow]));
    assert_eq!(went.ran, ["weather"]);
}

/// A lookup that failed, or a search that found nothing (Twelve's P2 on
/// #7719: "gold price today?", a forced `web_search` finds nothing, then
/// "It's $4,050 an ounce."), backs no figure: one said after it is asked
/// again, then said as the line that it couldn't be looked up.
#[test]
fn a_figure_after_a_failed_or_empty_lookup_is_never_said() {
    for answers in [failing as fn() -> Value, empty] {
        let went = turns(
            Setup {
                on: true,
                answers: Some(answers),
                ..Setup::default()
            },
            vec![(
                GOLD,
                vec![
                    calling("web_search"),
                    saying(GOLD_MADE_UP),
                    saying(GOLD_MADE_UP),
                ],
            )],
        );
        assert_eq!(went.said, said(&[COULD_NOT]), "{:?}: {went:?}", answers());
        assert_eq!(went.ran, ["web_search"]);
        assert_eq!(went.asked, 3);
        assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
    }
}

/// The weather found backs the weather, never a price in the same turn.
#[test]
fn the_weather_found_backs_no_price() {
    let rain = "Da Nang is 27 degrees with light rain.";
    let went = lines(
        true,
        false,
        vec![(
            "What's the weather in Da Nang, and the gold price?",
            vec![
                calling("weather"),
                saying(&format!("{rain} {GOLD_MADE_UP}")),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert_eq!(
        went.said,
        said(&[rain, &format!(" {GOLD_FOUND}")]),
        "{went:?}"
    );
    assert_eq!(went.ran, ["weather", "web_search"]);
    // Told what it already said, so it goes on from it.
    assert_eq!(
        went.notes,
        [crate::live_fact::note_after("web_search", rain)]
    );
}

/// Beside other calls, those run, and the next round is told the note as
/// its last message, so its lookup is forced (`second_provider.rs`'s
/// beside-other-calls branch).
#[test]
fn a_figure_beside_other_calls_lets_them_run_and_forces_the_next_round() {
    let went = lines(
        true,
        false,
        vec![(
            "Turn the light on and tell me the gold price.",
            vec![
                saying_and_calling(GOLD_MADE_UP, &["light_on"]),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert_eq!(went.said, said(&[GOLD_FOUND]), "{went:?}");
    assert_eq!(went.ran, ["light_on", "web_search"]);
    assert_eq!(went.asked, 3);
    assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
    assert_eq!(
        went.lasts[1].last().map(String::as_str),
        Some(crate::live_fact::note("web_search").as_str())
    );
}

/// In the last round, or with no lookup declared, there is no ask again:
/// the figure is said as the line that it couldn't be looked up.
#[test]
fn the_last_round_or_no_lookup_declared_says_it_could_not() {
    let went = lines(
        true,
        false,
        vec![(
            GOLD,
            vec![
                calling("light_on"),
                calling("light_off"),
                calling("set_volume"),
                saying(GOLD_MADE_UP),
            ],
        )],
    );
    assert_eq!(went.said, said(&[COULD_NOT]), "{went:?}");
    assert_eq!(went.asked, MAX_ROUNDS);
    assert!(went.notes.is_empty());
    let went = turns(
        Setup {
            on: true,
            undeclared: &["web_search", "weather"],
            ..Setup::default()
        },
        vec![(GOLD, vec![saying(GOLD_MADE_UP)])],
    );
    assert_eq!(went.said, said(&[COULD_NOT]), "{went:?}");
    assert_eq!(went.asked, 1);
    assert!(went.notes.is_empty());
}

/// The chat's own web search (OpenRouter's plugin, `ChatEvent::Searched`)
/// backs the round's figure: said as without the switch.
#[test]
fn the_chats_own_search_backs_its_figure() {
    let rounds = || {
        vec![vec![
            Ok(ChatEvent::Searched),
            text(GOLD_FOUND),
            Ok(ChatEvent::Done),
        ]]
    };
    let on = lines(true, false, vec![(GOLD, rounds())]);
    let off = lines(false, false, vec![(GOLD, rounds())]);
    assert_eq!(on, off);
    assert_eq!(on.said, said(&[GOLD_FOUND]));
}

/// Twelve's order on #7719: one reply with both a false action claim and
/// a made-up price. The action claim is judged first and asks again; that
/// round's live sentences are dropped quietly, with no live note of their
/// own. The round asked again, with both calls run, says what it found.
#[test]
fn an_action_claim_and_a_live_figure_in_one_reply_ask_again_once_for_the_claim() {
    let went = turns(
        Setup {
            on: true,
            claims: true,
            ..Setup::default()
        },
        vec![(
            "Lamp, set a timer for one minute and tell me the gold price today.",
            vec![
                saying(&format!("Timer set. {GOLD_MADE_UP}")),
                vec![
                    Ok(ChatEvent::ToolCalls(vec![
                        call("c0", "set_timer", json!({})),
                        call("c1", "web_search", json!({})),
                    ])),
                    Ok(ChatEvent::Done),
                ],
                saying(&format!("Timer set. {GOLD_FOUND}")),
            ],
        )],
    );
    assert!(
        went.said.iter().all(|said| !said.contains("4,050")),
        "{went:?}"
    );
    assert!(went.said.concat().contains("4,061"), "{went:?}");
    assert_eq!(went.ran, ["set_timer", "web_search"]);
    assert_eq!(went.asked, 3);
    assert_eq!(went.told, [crate::action_claim::NOTE], "{went:?}");
    assert!(went.notes.is_empty());
}

/// The first words wait for their sentence's end only on a line asking a
/// live fact, before a lookup found one (`live.holds_back`; the first
/// builder on #8224, rule 7). Any other line is cut as with the switch off,
/// so its first sound is unchanged, a timer's or a count's number
/// included; on "the gold price today?" the price's sentence is judged
/// whole, and its made-up figure never begins to be said. After a search
/// found, the cut is as with the switch off.
#[test]
fn the_first_words_wait_only_on_a_line_asking_a_live_fact() {
    let reply = "Sure, it was a gift from a friend of mine.";
    let cut = |on, heard, rounds: Vec<Vec<net::Result<ChatEvent>>>| {
        turns(
            Setup {
                on,
                cut: true,
                ..Setup::default()
            },
            vec![(heard, rounds)],
        )
    };
    let asked = "Where did my lamp come from?";
    let on = cut(true, asked, vec![streaming(reply)]);
    let off = cut(false, asked, vec![streaming(reply)]);
    assert_eq!(on, off);
    assert!(on.said.len() > 1, "{on:?}");
    assert_eq!(
        on.said
            .iter()
            .map(|said| said.trim_start_matches("said:"))
            .collect::<String>(),
        reply
    );
    // A timer or a count asked plainly: never waits, number and all.
    for (line, reply) in [
        (
            "Lamp, set a timer for 5 minutes.",
            "Your timer is set for 5 minutes.",
        ),
        (
            "How many sessions did I do this week?",
            "That's 3 sessions this week, nice work.",
        ),
    ] {
        // Cut, never held: at its first words, or before its number when
        // that comes first (no later), and every word said as off.
        let on = cut(true, line, vec![streaming(reply)]);
        let off = cut(false, line, vec![streaming(reply)]);
        assert!(on.said.len() > 1, "{on:?}");
        assert!(on.said[0].len() <= off.said[0].len(), "{on:?} {off:?}");
        let words = |went: &Went| went.said.concat().replace("said:", "");
        assert_eq!(words(&on), words(&off), "{line}");
        assert_eq!((on.asked, on.notes.len()), (1, 0), "{line}");
    }
    // A live question: the made-up price is never cut, held whole, asked
    // again.
    let gold = || {
        vec![
            streaming(GOLD_MADE_UP),
            calling("web_search"),
            saying(GOLD_FOUND),
        ]
    };
    let held = cut(true, GOLD, gold());
    assert!(
        held.said.iter().all(|said| !said.contains("4,050")),
        "{held:?}"
    );
    assert!(held.said.concat().contains("4,061"), "{held:?}");
    assert_eq!(held.notes, [crate::live_fact::note("web_search")]);
    let off = cut(false, GOLD, gold());
    assert!(off.said.concat().contains("4,050"), "{off:?}");
    // A search found: the cut is as with the switch off.
    let searched = || vec![calling("web_search"), streaming(reply)];
    assert_eq!(cut(true, asked, searched()), cut(false, asked, searched()));
}

/// The live line's wait, end to end (Twelve's HOLD on #8224): on "the
/// gold price today?" a reply whose figure comes after its fourth word
/// says nothing of it before it is judged whole. Cut at four words, "The
/// gold price today" would be said first, and the rest asked again: this
/// fails when `asks_live` or `holds_back` is false.
#[test]
fn a_live_lines_figure_after_the_fourth_word_waits_for_its_sentence() {
    let made_up = "The gold price today is around 4,050 dollars an ounce.";
    let went = turns(
        Setup {
            on: true,
            cut: true,
            ..Setup::default()
        },
        vec![(
            GOLD,
            vec![
                streaming(made_up),
                calling("web_search"),
                saying(GOLD_FOUND),
            ],
        )],
    );
    assert!(
        went.said
            .iter()
            .all(|said| !said.contains("The gold price") && !said.contains("4,050")),
        "{went:?}"
    );
    assert!(went.said.concat().contains("4,061"), "{went:?}");
    assert_eq!(went.notes, [crate::live_fact::note("web_search")]);
}

/// A live line `asks_live` misses ("How much is an ounce of silver?")
/// still never says part of a figure (Twelve's HOLD on #8224): the first
/// piece is cut before "31", as "Silver is about", and the figure is
/// held whole and asked again. Off, it is said as it came.
#[test]
fn a_first_piece_never_ends_on_a_bare_number() {
    let went = |on| {
        turns(
            Setup {
                on,
                cut: true,
                ..Setup::default()
            },
            vec![(
                "How much is an ounce of silver?",
                vec![
                    streaming("Silver is about 31 dollars an ounce."),
                    calling("web_search"),
                    saying("Silver is $32 an ounce today."),
                ],
            )],
        )
    };
    let on = went(true);
    assert!(on.said.iter().all(|said| !said.contains("31")), "{on:?}");
    assert!(on.said.concat().contains("$32"), "{on:?}");
    assert_eq!(
        on.notes,
        [crate::live_fact::note_after(
            "web_search",
            "Silver is about"
        )]
    );
    let off = went(false);
    assert_eq!(
        off.said.first().map(String::as_str),
        Some("said:Silver is about 31")
    );
}

/// The 06c1 S1 leg on #8224: the four-word piece "Silver's around 52 to"
/// ended on "to", so "52" was said with no lookup. A first piece is cut
/// before its first number: "Silver's around" is said, and the rest is
/// judged whole at its sentence's end, held, and asked again. A timer's
/// number past the first four words is cut as with the switch off.
#[test]
fn a_first_piece_is_cut_before_its_first_number() {
    let went = |on, heard, rounds| {
        turns(
            Setup {
                on,
                cut: true,
                ..Setup::default()
            },
            vec![(heard, rounds)],
        )
    };
    let silver = went(
        true,
        "How much is an ounce of silver?",
        vec![
            streaming("Silver's around 52 to 55 dollars an ounce."),
            calling("web_search"),
            saying("Silver is $32 an ounce today."),
        ],
    );
    assert_eq!(
        silver.said.first().map(String::as_str),
        Some("said:Silver's around"),
        "{silver:?}"
    );
    assert!(
        silver
            .said
            .iter()
            .all(|said| !said.contains("52") && !said.contains("55")),
        "{silver:?}"
    );
    // The said piece is ended by the bridge, then the reply asked again.
    assert_eq!(
        silver.said.get(1).map(String::as_str),
        Some(
            format!(
                "said: {} Silver is $32 an ounce today.",
                crate::live_fact::BRIDGE
            )
            .as_str()
        ),
        "{silver:?}"
    );
    assert_eq!(silver.notes.len(), 1, "{silver:?}");
    let timer = "Your timer is set for 5 minutes.";
    let line = "Lamp, set a timer for 5 minutes.";
    let on = went(true, line, vec![streaming(timer)]);
    assert_eq!(on, went(false, line, vec![streaming(timer)]));
    assert_eq!(
        on.said.first().map(String::as_str),
        Some("said:Your timer is set"),
        "{on:?}"
    );
    assert_eq!(on.said.concat().replace("said:", ""), timer);
}

/// Thirteen's HOLD on #8224 (at 7b36d055f2): with every lookup failing,
/// round 1 says "Silver's around" and holds its figure, round 2's
/// `web_search` fails, and round 3's "Silver is $32 an ounce today." is
/// held too. The said piece still dangles, so the could-not line takes the
/// bridge first; the bridge was spent on round 3's held sentence before.
#[test]
fn the_bridge_comes_before_the_could_not_line_after_a_second_held_round() {
    let went = turns(
        Setup {
            on: true,
            cut: true,
            answers: Some(failing as fn() -> Value),
            ..Setup::default()
        },
        vec![(
            "How much is an ounce of silver?",
            vec![
                streaming("Silver's around 52 to 55 dollars an ounce."),
                calling("web_search"),
                saying("Silver is $32 an ounce today."),
            ],
        )],
    );
    assert_eq!(
        went.said,
        said(&[
            "Silver's around",
            &format!(" {} {COULD_NOT}", crate::live_fact::BRIDGE)
        ]),
        "{went:?}"
    );
    assert_eq!(went.ran, ["web_search"]);
}

/// #8224's lamp row (H2, the switch on): round 1's "In Hanoi right now" held
/// no figure and was said at once (here on a line asking no live fact, whose
/// first words are cut), its figure was held and asked again,
/// and the reply asked again began "In Hanoi right now it's 20°C…": the
/// lamp said "In Hanoi right now" twice. The words already said are taken
/// off the reply asked again, and its note tells the model them. A reply
/// asked again that begins otherwise, or that only shares part of a word
/// ("nowadays"), is said whole after the bridge that ends the said piece
/// (`live_fact::BRIDGE`, the first builder's c7066116904).
#[test]
fn the_reply_asked_again_never_says_the_words_already_said_again() {
    let opening = "In Hanoi right now";
    let went = |again: &str| {
        turns(
            Setup {
                on: true,
                cut: true,
                ..Setup::default()
            },
            vec![(
                "Lamp, how does it look outside?",
                vec![
                    streaming("In Hanoi right now it's 20°C and cloudy."),
                    calling("weather"),
                    saying(again),
                ],
            )],
        )
    };
    let note = crate::live_fact::note_after("weather", opening);
    assert!(note.contains("\"In Hanoi right now\""), "{note}");
    // The exact H2 case: only what follows the words said is said.
    let h2 = went("In Hanoi right now it's 20°C with light rain.");
    assert_eq!(
        h2.said,
        said(&[opening, " it's 20°C with light rain."]),
        "{h2:?}"
    );
    assert_eq!(h2.notes, [note]);
    assert_eq!(h2.ran, ["weather"]);
    // Begun otherwise: nothing dropped.
    let other = went("Hanoi is 20°C with light rain.");
    assert_eq!(
        other.said,
        said(&[
            opening,
            &format!(
                " {} Hanoi is 20°C with light rain.",
                crate::live_fact::BRIDGE
            )
        ]),
        "{other:?}"
    );
    // A word that only begins as the last word said did: no cut mid-word.
    let partial = went("In Hanoi right nowadays it's 20°C with light rain.");
    assert_eq!(
        partial.said,
        said(&[
            opening,
            &format!(
                " {} In Hanoi right nowadays it's 20°C with light rain.",
                crate::live_fact::BRIDGE
            )
        ]),
        "{partial:?}"
    );
}

/// The don't-know's release at the round's end (`second_provider.rs`'s
/// `DontKnow::Pass` after round 1's words were held as a line that may not
/// be for the lamp) judges its sentences too (Twelve's P3 on #7719): a
/// made-up price followed by another sentence, so it leaves through that
/// release and not the split's finish, is not said, and the model is asked
/// again with the search required. Off, it is said.
#[test]
fn a_dont_know_released_at_the_rounds_end_holds_its_figure() {
    let reply = format!("I'm not sure. {GOLD_MADE_UP} Prices move a lot.");
    let went = |on| {
        turns(
            Setup {
                on,
                declined: true,
                ..Setup::default()
            },
            vec![(
                "Lamp, what gold price should I expect today?",
                vec![saying(&reply), calling("web_search"), saying(GOLD_FOUND)],
            )],
        )
    };
    let on = went(true);
    assert!(on.said.iter().all(|said| !said.contains("4,050")), "{on:?}");
    assert!(on.said.concat().contains("4,061"), "{on:?}");
    assert_eq!(on.ran, ["web_search"]);
    assert_eq!(
        on.notes,
        [crate::live_fact::note_after("web_search", "I'm not sure.")]
    );
    let off = went(false);
    assert!(off.said.concat().contains("4,050"), "{off:?}");
}

/// The notes' order (#7605): the live note is told after the make line's
/// ask again, as the last message, so its lookup is forced; told before
/// another note, nothing would be forced.
#[test]
fn the_live_note_is_told_after_the_begun_note() {
    let live = crate::live_fact::note("web_search");
    let notes = last_notes(true, Some(live.clone()));
    assert_eq!(
        notes.iter().map(|note| note.words()).collect::<Vec<_>>(),
        [
            Some(crate::make_skill_agent::BEGUN_AGAIN_NOTE),
            Some(live.as_str())
        ]
    );
    let force = crate::live_fact::force_when(true).expect("on");
    assert_eq!(force(&notes).map(|forced| forced.name), Some("web_search"));
    assert_eq!(last_notes(true, None).len(), 1);
    assert!(last_notes(false, None).is_empty());
}

/// True answers stand, word for word as with the switch off: a price this
/// turn's search found, the weather of the place this turn fetched, a
/// general question with no live figure, and the time asked "now".
#[test]
fn true_answers_stand_as_without_the_switch() {
    type Rounds = fn() -> Vec<Vec<net::Result<ChatEvent>>>;
    let cases: [(&str, Rounds); 4] = [
        (GOLD, || vec![calling("web_search"), saying(GOLD_FOUND)]),
        ("What's the weather in Da Nang right now?", || {
            vec![
                calling("weather"),
                saying("Da Nang is 27 degrees with light rain."),
            ]
        }),
        ("Is gold a good investment?", || {
            vec![saying(
                "Gold can hold its value when prices rise, but it pays no interest.",
            )]
        }),
        ("What time is it now?", || vec![saying("It's 3:40 pm.")]),
    ];
    for (heard, rounds) in cases {
        let on = lines(true, false, vec![(heard, rounds())]);
        let off = lines(false, false, vec![(heard, rounds())]);
        let words = |went: &Went| went.said.concat().replace("said:", "");
        assert_eq!(words(&on), words(&off), "{heard}");
        assert_eq!(
            (on.asked, &on.ran, &on.told),
            (off.asked, &off.ran, &off.told),
            "{heard}"
        );
        assert!(!on.said.is_empty(), "{heard}");
        assert!(on.notes.is_empty(), "{heard}");
    }
}

/// #8556 (lamp-06c1, 25 Nov, build 15903, #8240's leg B): round 2 started
/// the rain poem, round 3 read it back ("…a short poem about rain"), and
/// the check took "rain" for the weather and "Code" for a place: held, the
/// model was told to look the weather up, called `start_agent_task` again
/// and a second task was prepared. A round after the start answered in the
/// same request is never held: the read-back is said, and the start runs
/// once.
#[test]
fn the_read_back_after_a_start_answered_is_said_and_the_start_runs_once() {
    const RAIN: &str = "Ask Claude Code to write a short poem about rain.";
    const READ_BACK: &str = "Claude Code will write a short poem about rain. Shall I go ahead?";
    let rounds = || {
        vec![
            calling(crate::agent_tasks::START),
            saying(READ_BACK),
            calling(crate::agent_tasks::START),
            saying(COULD_NOT),
        ]
    };
    let went = lines(true, false, vec![(RAIN, rounds())]);
    assert_eq!(went.ran, [crate::agent_tasks::START], "{went:?}");
    assert_eq!(went.asked, 2, "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
    // Said as with the switch off.
    let off = lines(false, false, vec![(RAIN, rounds())]);
    assert_eq!(went.said, off.said);
    assert!(!went.said.is_empty() && !went.said.concat().contains(COULD_NOT));
}
