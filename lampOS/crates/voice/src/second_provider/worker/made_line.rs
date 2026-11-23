//! lamp-9adf, 11 Nov 02:16 and 02:19 (staging 11625): "make a skill: when
//! I come back to my desk, nod at me". The lamp's own echo cut "I'll make
//! it" while the skill was made; the make went on, the skill was made and
//! put on, and its line, only an answer for a model round that never came,
//! was never said. A voice make whose turn is cut says its line once,
//! through the say service; one whose turn still waits says nothing there.
use super::made_running;
use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};
use crate::tools::ToolAnswer;
use std::{sync::mpsc, time::Duration};

const REQUEST: &str = "make me a skill that dims the light at 10 pm";
const NOW: u64 = 1_800_000_000_000;
const LINE: &str = "I made evening dim, and it's on: when the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most once every 12 hours, using its light and whether it is talking.";

/// A make, with the lamp's model and manage helper of the test's own,
/// begun once `go` is told: what it tells the model.
fn make(name: &str, go: mpsc::Receiver<()>) -> impl FnOnce() -> ToolAnswer + Send + 'static {
    let (root, keeping) = keeping(name);
    let (url, served) = service(vec![(200, good())]);
    let asked = manage_helper(&keeping.manage);
    move || {
        let _ = go.recv();
        let told = crate::make_skill_local::make_with(
            0,
            None,
            REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert!(asked.join().unwrap()["install_made"].is_string());
        let _ = std::fs::remove_dir_all(&root);
        ToolAnswer::plain(told)
    }
}

/// Cut while the skill is made: the make goes on, the skill is put on, and
/// its line, as the model would have been given it, is said once.
#[tokio::test]
async fn a_make_whose_turn_was_cut_still_says_its_line_once() {
    let (go, wait) = mpsc::channel();
    let (say, said) = mpsc::channel();
    let running = made_running(
        1,
        make("cut-made-line", wait),
        move |line| {
            say.send(line).unwrap();
        },
        |_| true,
    );
    // The cut: the turn's wait for the answer is dropped mid-make.
    drop(running);
    go.send(()).unwrap();
    let line = tokio::task::spawn_blocking(move || {
        let line = said.recv_timeout(Duration::from_secs(20));
        let again = said.recv_timeout(Duration::from_millis(300));
        (line, again)
    })
    .await
    .unwrap();
    // The made line as written; a later sentence (its manners) may follow.
    assert!(
        line.0.as_deref().is_ok_and(|l| l.starts_with(LINE)),
        "{:?}",
        line.0
    );
    assert!(line.1.is_err(), "said once: {:?}", line.1);
}

/// Cut with a wait of its own request's for code (#7245, c0a4 17 Nov
/// 22:39): the say service is given its say-back word for word, never the
/// cut turn's own line, so the yes after it is heard whole is forced.
#[tokio::test]
async fn a_cut_code_makes_say_back_goes_to_the_say_service_whole() {
    let (go, wait) = mpsc::channel();
    let (say, said) = mpsc::channel();
    let made = make("cut-code-wait", wait);
    // On the make's own thread, whose store the cut branch reads.
    let work = move || {
        let answer = made();
        let asks = crate::skill_write_yes::Asks {
            skill: "evening-dim".into(),
            sha256: "0".repeat(64),
            writes: Vec::new(),
            say_back: crate::made_skill::code_said_as(&["camera".to_owned()], false)
                .trim()
                .to_owned(),
            grants: Some(vec!["camera".to_owned()]),
            code: true,
            remove: false,
        };
        crate::skill_write_yes::wait_for_yes(asks, 3, std::time::Instant::now());
        // Its cut line as a code make's is (`made_skill::cut_code_line`).
        crate::make_skill_local::set_said_line(format!(
            "I made evening dim.{} {}",
            crate::made_skill::code_said_as(&["camera".to_owned()], false),
            crate::made_skill::CUT_ASK_AGAIN
        ));
        answer
    };
    let running = made_running(
        3,
        work,
        move |line| {
            say.send(line).unwrap();
        },
        |_| true,
    );
    drop(running);
    go.send(()).unwrap();
    let line = tokio::task::spawn_blocking(move || said.recv_timeout(Duration::from_secs(20)))
        .await
        .unwrap();
    assert_eq!(
        line.as_deref(),
        Ok(format!(
            "I made evening dim.{}",
            crate::made_skill::code_said_as(&["camera".to_owned()], false)
        )
        .as_str())
    );
}

/// Its turn still waiting: the answer goes to the turn, whose model says
/// the line, and nothing is said through the say service.
#[tokio::test]
async fn a_make_whose_turn_waits_never_uses_the_say_service() {
    let (go, wait) = mpsc::channel();
    let (say, said) = mpsc::channel::<String>();
    go.send(()).unwrap();
    let answer = made_running(
        2,
        make("kept-made-line", wait),
        move |line| {
            say.send(line).unwrap();
        },
        |_| false,
    )
    .await;
    assert_eq!(answer.told["made"], "evening-dim", "{}", answer.told);
    assert_eq!(answer.told["on"], true, "{}", answer.told);
    assert!(
        answer.told["next"].as_str().unwrap().contains(LINE),
        "{}",
        answer.told
    );
    let said = tokio::task::spawn_blocking(move || said.recv_timeout(Duration::from_millis(300)))
        .await
        .unwrap();
    assert!(said.is_err(), "nothing through the say service: {said:?}");
}

/// A make of `request`, `made_in` its voice request (a clash is asked only
/// for one), cut once begun: the model answers `answer`; `lapsed`, the
/// turn's answer is the lapsed scope's `error`, whatever the make kept, as
/// a cut turn's is on the lamp (#7554's ROW at 2e864a0: "the lamp's own
/// model did not answer"). `went_on` says whether the person went on after
/// it. What is said through the say service, or none once the make ended
/// with nothing said (the say closure dropped), within 30 s.
async fn cut_make(
    name: &str,
    made_in: u64,
    answer: String,
    clash_with: bool,
    (lapsed, went_on): (bool, bool),
) -> Result<String, mpsc::RecvTimeoutError> {
    let (go, wait) = mpsc::channel::<()>();
    let (say, said) = mpsc::channel::<String>();
    let (root, keeping) = keeping(name);
    if clash_with {
        // A skill on the lamp on the made one's trigger, using the light.
        let folder = keeping.installed.join("rest-glow");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\ntopic = \
             \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\ncooldown = \"1h\"\n\n\
             [[do]]\nlight = { colour = \"blue\" }\n",
        )
        .unwrap();
    }
    let (url, served) = service(vec![(200, answer)]);
    let running = made_running(
        made_in,
        move || {
            let _ = wait.recv();
            let told = crate::make_skill_local::make_with(
                made_in,
                (made_in != 0).then_some(REQUEST),
                REQUEST,
                &keeping,
                chat_at(url),
                NOW,
                || None,
            );
            assert!(served.join().unwrap() >= 1);
            let _ = std::fs::remove_dir_all(&root);
            ToolAnswer::plain(if lapsed {
                crate::tools::unavailable()
            } else {
                told
            })
        },
        move |line| {
            say.send(line).unwrap();
        },
        move |_| went_on,
    );
    drop(running);
    go.send(()).unwrap();
    tokio::task::spawn_blocking(move || said.recv_timeout(Duration::from_secs(30)))
        .await
        .unwrap()
}

const CANNOT: &str = "cannot: a skill can't do that.";

/// Cut while the skill is made, the make kept nothing, and the person went
/// on with a line of their own after it (lamp-9adf, 17 Nov 18:10: its "I
/// couldn't make that" was said 88 s later, in a later, unrelated turn):
/// nothing is said through the say service.
#[tokio::test]
async fn a_cut_make_that_kept_nothing_says_nothing() {
    let said = cut_make("cut-not-made-line", 3, CANNOT.into(), false, (false, true)).await;
    assert!(said.is_err(), "nothing said after the cut: {said:?}");
}

/// lamp-9adf, 18 Nov (#7554's ROW at 2e864a0, requests 5 and 16): a make's
/// turn cut by a 0-word sound (requests 6 and 17) or a line held as not
/// said to the lamp (request 57), and the make failed: the person never went
/// on, so its failure is said, once, whatever its lapsed answer.
#[tokio::test]
async fn a_cut_make_that_failed_is_told_when_the_person_did_not_go_on() {
    for lapsed in [false, true] {
        let said = cut_make(
            &format!("cut-failed-told-{lapsed}"),
            5,
            CANNOT.into(),
            false,
            (lapsed, false),
        )
        .await;
        assert!(
            said.as_deref()
                .is_ok_and(|line| line.starts_with("I couldn't make that")),
            "lapsed {lapsed}: {said:?}"
        );
    }
}

/// lamp-9adf, 18 Nov (#7554's ROW at 2e864a0, 17:39:32): request 86's
/// `dim-light-at-night` clashed and was asked, its turn cut, and its lapsed
/// answer read as "kept nothing": its clash was never said. A clash is
/// said after a cut, the person gone on or not, and still waits.
#[tokio::test]
async fn a_cut_make_that_clashes_tells_its_clash() {
    for went_on in [false, true] {
        let said = cut_make(
            &format!("cut-clash-told-{went_on}"),
            86,
            good(),
            true,
            (true, went_on),
        )
        .await;
        assert!(
            said.as_deref()
                .is_ok_and(|line| line
                    .contains("but didn't put it on: rest glow starts at the same moment")),
            "went on {went_on}: {said:?}"
        );
    }
}

/// Only a line heard with words and answered is the person going on: the
/// second route marks it ([`crate::make_skill_local::person_went_on`]),
/// for a later request only.
#[test]
fn only_a_later_answered_line_is_going_on() {
    let request = 7_554_900_000;
    assert!(!crate::make_skill_local::went_on_after(request + 1_000));
    crate::make_skill_local::person_went_on(request);
    assert!(!crate::make_skill_local::went_on_after(request));
    assert!(crate::make_skill_local::went_on_after(request - 1));
}

/// #7621's refusal of a made skill that isn't what was asked, in a turn
/// that was cut: nothing was made, so it is a failure. Its ask-again line
/// is said while the person has not gone on, and never once they went on
/// with a line of their own.
#[tokio::test]
async fn a_cut_make_refused_as_not_what_was_asked_is_a_failure() {
    use crate::make_skill_local::tests::{FOURTEEN, FOURTEEN_ASK, MOON_TIME, service_sent};
    for went_on in [false, true] {
        let (go, wait) = mpsc::channel::<()>();
        let (say, said) = mpsc::channel::<String>();
        let (root, keeping) = keeping(&format!("cut-unrelated-{went_on}"));
        let (url, served) =
            service_sent(vec![(200, MOON_TIME.to_owned()), (200, "No.".to_owned())]);
        let running = made_running(
            7,
            move || {
                let _ = wait.recv();
                // The switches are this thread's: the make's own.
                crate::made_skill::set_unrelated_here(Some(true));
                crate::made_skill::set_english_here(Some(true));
                crate::made_skill::set_asks_here(Some(true));
                let told = crate::make_skill_local::make_with(
                    7,
                    Some(FOURTEEN),
                    FOURTEEN_ASK,
                    &keeping,
                    chat_at(url),
                    NOW,
                    || None,
                );
                crate::made_skill::set_asks_here(None);
                crate::made_skill::set_english_here(None);
                crate::made_skill::set_unrelated_here(None);
                assert_eq!(served.join().unwrap().len(), 2);
                let _ = std::fs::remove_dir_all(&root);
                assert_eq!(told["error"], crate::made_skill::UNRELATED, "{told}");
                ToolAnswer::plain(crate::tools::unavailable())
            },
            move |line| {
                say.send(line).unwrap();
            },
            move |_| went_on,
        );
        drop(running);
        go.send(()).unwrap();
        let line = tokio::task::spawn_blocking(move || said.recv_timeout(Duration::from_secs(30)))
            .await
            .unwrap();
        if went_on {
            assert!(line.is_err(), "nothing said once they went on: {line:?}");
        } else {
            assert_eq!(line.as_deref(), Ok(crate::made_skill::UNRELATED));
        }
    }
}
