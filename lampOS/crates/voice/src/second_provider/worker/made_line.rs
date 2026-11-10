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
        let told =
            crate::make_skill_local::make_with(REQUEST, &keeping, chat_at(url), NOW, || None);
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
    let running = made_running(1, make("cut-made-line", wait), move |line| {
        say.send(line).unwrap();
    });
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

/// Its turn still waiting: the answer goes to the turn, whose model says
/// the line, and nothing is said through the say service.
#[tokio::test]
async fn a_make_whose_turn_waits_never_uses_the_say_service() {
    let (go, wait) = mpsc::channel();
    let (say, said) = mpsc::channel::<String>();
    go.send(()).unwrap();
    let answer = made_running(2, make("kept-made-line", wait), move |line| {
        say.send(line).unwrap();
    })
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
