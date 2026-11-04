//! `LAMP_MEMORY_DONT_KNOW_RECALLS` on the second route (task C57): a reply
//! that opens with a don't-know on a line about the person, while their own
//! kept things fit its words, is never said; the model is asked once more
//! with them. A don't-know with nothing found, any other reply, a line not
//! about the person, and every turn with the switch off go on as before.
use super::*;

/// One turn as it went.
#[derive(Debug, PartialEq)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The chat rounds asked.
    asked: usize,
    /// The notes the rounds were asked with.
    notes: Vec<String>,
    /// The words the person's kept things were searched for.
    searched: Vec<String>,
}

/// The person's own kept thing, as memory's hits note tells it.
const KEPT: &str = "- My favourite snack is pretzels.";

/// One spoken turn: `heard`, round 1's `first` reply and the rounds
/// `later`. `found` searches the person's kept things (`None`: the switch
/// off): `KEPT` for a line about snacks, nothing for any other.
fn turn_with(found: Option<bool>, heard: &str, first: &str, later: &[&str]) -> Went {
    let reply = |words: &str| vec![text(words), Ok(ChatEvent::Done)];
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(
            std::iter::once(reply(first))
                .chain(later.iter().map(|words| reply(words)))
                .collect(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine = engine.announcing(false);
    let searched: Arc<Mutex<Vec<String>>> = Arc::default();
    if let Some(kept) = found {
        let seen = searched.clone();
        engine = engine.recalling_before_a_dont_know(Arc::new(move |_, words, _| {
            seen.lock().unwrap().push(words.to_owned());
            (kept && words.contains("snack")).then(|| KEPT.to_owned())
        }));
    }
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
        .filter(|content| content.starts_with(crate::dont_know::NOTE))
        .map(str::to_owned)
        .collect();
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        asked: asked.len(),
        notes,
        searched: searched.lock().unwrap().clone(),
    }
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

const ASKED: &str = "What's my favourite snack?";

/// lamp-06c1, 30 Oct: "I don't know" with the answer kept. With the switch
/// and the person's kept thing found, the don't-know is never said: the
/// model is asked once more with it, and its answer is said.
#[test]
fn a_dont_know_with_a_kept_match_recalls_it_first() {
    let went = turn_with(
        Some(true),
        ASKED,
        "I don't know your favourite snack yet.",
        &["Your favourite snack is pretzels."],
    );
    assert_eq!(went.said, said(&["Your favourite snack is pretzels."]));
    assert_eq!(went.asked, 2);
    assert_eq!(went.searched, [ASKED.to_owned()]);
    assert_eq!(
        went.notes,
        [format!("{}\n{KEPT}", crate::dont_know::NOTE)],
        "told once, with the kept thing"
    );
}

/// "Hmm, I'm not sure" is the same don't-know; the round asked again that
/// says it again is said, never asked a third time.
#[test]
fn a_second_dont_know_after_the_recall_is_said() {
    let went = turn_with(
        Some(true),
        ASKED,
        "Hmm, I'm not sure.",
        &["I don't remember that."],
    );
    assert_eq!(went.said, said(&["I don't remember that."]));
    assert_eq!((went.asked, went.searched.len()), (2, 1));
}

/// Nothing kept fits the words: the don't-know stays, said as it came, one
/// round, searched once.
#[test]
fn a_dont_know_with_no_kept_match_stays() {
    let heard = "Where's my passport?";
    let went = turn_with(
        Some(true),
        heard,
        "I don't know where your passport is.",
        &[],
    );
    assert_eq!(went.said, said(&["I don't know where your passport is."]));
    assert_eq!((went.asked, went.notes.len()), (1, 0));
    assert_eq!(went.searched, [heard.to_owned()]);
}

/// An answer that isn't a don't-know, and a line not about the person,
/// search nothing and go on as they came.
#[test]
fn an_answer_or_a_line_about_anything_else_searches_nothing() {
    let answered = turn_with(Some(true), ASKED, "Pretzels, I think.", &[]);
    assert_eq!(answered.said, said(&["Pretzels, I think."]));
    assert!(answered.searched.is_empty());
    let other = turn_with(
        Some(true),
        "What's the capital of Peru?",
        "I'm not sure.",
        &[],
    );
    assert_eq!(other.said, said(&["I'm not sure."]));
    assert!(other.searched.is_empty());
}

/// Off (no search given), every reply goes on byte for byte as before.
#[test]
fn the_switch_off_changes_nothing() {
    let first = "I don't know your favourite snack yet.";
    let off = turn_with(None, ASKED, first, &["Pretzels."]);
    assert_eq!(off.said, said(&[first]));
    assert_eq!((off.asked, off.notes.len()), (1, 0));
    let answer = turn_with(None, ASKED, "Pretzels, I think.", &[]);
    assert_eq!(answer.said, said(&["Pretzels, I think."]));
}
