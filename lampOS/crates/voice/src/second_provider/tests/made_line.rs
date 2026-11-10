//! lamp-9adf, 11 Nov 02:16 and 02:19 (staging 11625): the person asked for
//! a skill, the model said "I'll make it" and called `make_skill`, and the
//! lamp's own echo, a cut-in with no words, cut its turn while the lamp's
//! model wrote the skill. The cut was answered "Um, excuse me, did anyone
//! say something?" while the skill was still being made. A wordless cut
//! while a voice make is under way, or its line is still to be said, is
//! never asked that.
use super::*;

/// What the test's runner does with `name`'s call: under way until `done`
/// is told, as a make on the blocking pool goes on past a cut, and held
/// as [`crate::make_skill_local::Pending`] when it is a make.
fn runner_under_way(
    began: std::sync::mpsc::Sender<()>,
    done: Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
) -> ToolRunner {
    Arc::new(move |_, _, name, _| {
        let pending =
            (name == crate::made_skill::MAKE).then(crate::make_skill_local::Pending::begin);
        let done = done.clone();
        let _ = std::thread::spawn(move || {
            let _ = done.lock().unwrap().recv_timeout(Duration::from_secs(10));
            drop(pending);
        });
        let _ = began.send(());
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            json!({"ok": true})
        })
    })
}

/// Line 1 asks for `name`'s call, its answer begun ("I'll make it."); a
/// wordless line 2, over the lamp's voice, cuts the turn `after` the call
/// began, by the engine's clock, while the call is still under way. Line
/// 2's outputs.
async fn cut_while(name: &str, after: Duration) -> Vec<String> {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Make a skill: when I come back to my desk, nod at me.".to_owned()),
                Ok(String::new()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [vec![
                text("I'll make it."),
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    name,
                    json!({"request": "when I come back to my desk, nod at me"}),
                )])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (began, under_way) = std::sync::mpsc::channel();
    let (finish, done) = std::sync::mpsc::channel();
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner_under_way(began, Arc::new(Mutex::new(done))),
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .asking_after_wordless_cut_ins(true);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    // Until the call is under way and "I'll make it." is heard.
    let (mut calling, mut heard) = (false, false);
    while !(calling && heard) {
        calling |= under_way.try_recv().is_ok();
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(20), engine.next()).await {
            heard |= matches!(event, ProviderOutput::Audio { .. });
        }
    }
    *now.lock().unwrap() += after;
    engine.submit(start_marked(2, true, false, false));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let mut out = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
            .await
            .expect("line 2's turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(2),
                text,
                ..
            } if !text.is_empty() => out.push(format!("said:{text}")),
            ProviderOutput::TurnComplete { request: 2, .. } => break,
            _ => {}
        }
    }
    let _ = finish.send(());
    out
}

/// While the skill is made, the wordless cut is left quiet.
#[tokio::test]
async fn a_wordless_cut_while_a_skill_is_made_never_asks_if_someone_spoke() {
    let out = cut_while(crate::made_skill::MAKE, Duration::ZERO).await;
    assert!(
        !out.contains(&format!("said:{}", crate::cut_in_wordless::LINE)),
        "{out:?}"
    );
}

/// The first builder on #6174: the question is held no longer than a make
/// can go on (`make_skill_local::MAKE_WITHIN`, 120 s), though its line may
/// wait longer for a good moment: past it, a wordless cut is asked again.
#[tokio::test]
async fn past_the_makes_bound_a_wordless_cut_asks_again() {
    let out = cut_while(
        crate::made_skill::MAKE,
        crate::make_skill_local::MAKE_WITHIN,
    )
    .await;
    assert!(
        out.contains(&format!("said:{}", crate::cut_in_wordless::LINE)),
        "{out:?}"
    );
}

/// Control: the same cut while another call is under way is asked.
#[tokio::test]
async fn a_wordless_cut_while_another_call_runs_still_asks() {
    let out = cut_while("the_time", Duration::ZERO).await;
    assert!(
        out.contains(&format!("said:{}", crate::cut_in_wordless::LINE)),
        "{out:?}"
    );
}
