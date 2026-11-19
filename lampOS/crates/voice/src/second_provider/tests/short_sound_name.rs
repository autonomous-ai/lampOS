//! #7891 (lamp-52e6, 19 Nov, build 14751, request 12): a one-word call,
//! "Lamp?", came back as one word that is not the name, ended as a short
//! sound, and was held as one ("nothing shows it was said to the lamp"),
//! though lamp-name heard the name in it at 0.917 before its end was
//! judged. A hearing at lamp-name's own bar ([`crate::called::LEAST_SCORE`])
//! in the line shows it was said to the lamp: the line is no short sound,
//! and the rule judges it as any line, where a line of one or two words,
//! not a question, with nothing written for the name, is still not
//! answered on the hearing alone (#7918).
use super::*;

/// What the rule does with a short line heard only as the name in the
/// room's sound (#7918, `attention::short_line_without_the_name`): drops
/// it; anything else it lets out. Counts how often it was asked.
fn the_short_line_rule(asked: Arc<AtomicUsize>) -> Heed {
    Arc::new(move |_, _, heard, _, _| {
        asked.fetch_add(1, Ordering::SeqCst);
        crate::attention::short_line_without_the_name(heard)
            .then_some("a short line without the name")
    })
}

/// A line ended as a short sound with sounds checked, heard as `heard`,
/// lamp-name's best hearing in it `best`: whether it was answered, and
/// whether the rule was asked.
async fn heard_with(heard: &str, best: Option<f32>) -> (bool, bool) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Yes?"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let ruled = Arc::new(AtomicUsize::new(0));
    let mut engine = engine
        .announcing(false)
        .checking_sounds(true)
        .heeding(the_short_line_rule(ruled.clone()))
        .knowing_the_name_in_a_line(Arc::new(move |_: (u64, u64)| best));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound: true,
        cut_in: false,
    });
    let labels = turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().len();
    let answered = asked > 0 && labels.iter().any(|label| label.starts_with("audio:"));
    (answered, ruled.load(Ordering::SeqCst) > 0)
}

/// Request 12's shape: one word back that is not the name, a question or
/// a word written for it, lamp-name's hearing at 0.917 in the line: judged
/// by the rule and answered. At the bar itself too. A word that is neither,
/// heard at fault 1's 0.94, goes to the rule too, which drops it.
#[tokio::test(start_paused = true)]
async fn a_one_word_call_heard_by_lamp_name_at_its_bar_is_no_short_sound() {
    for heard in ["Lump?", "Lamb.", "Lynn?"] {
        for score in [0.917, crate::called::LEAST_SCORE] {
            assert_eq!(
                heard_with(heard, Some(score)).await,
                (true, true),
                "{heard} at {score}"
            );
        }
    }
    for heard in ["Camp.", "Clamp."] {
        assert_eq!(
            heard_with(heard, Some(0.94)).await,
            (false, true),
            "{heard} at 0.94"
        );
    }
}

/// The controls. A short sound with no hearing at the bar is held as
/// before, the rule never asked: the kit's near-names ("Camp.", "Clamp.",
/// "Damp.", "Ramp."), with no hearing, n4's near 0.643, or just under the
/// bar. The meeting words are short words, never this hold's: the rule
/// drops them, hearing or none, as now.
#[tokio::test(start_paused = true)]
async fn a_short_sound_without_a_hearing_at_the_bar_is_held_as_before() {
    for heard in ["Camp.", "Clamp.", "Damp.", "Ramp."] {
        for best in [None, Some(0.643), Some(0.899)] {
            assert_eq!(
                heard_with(heard, best).await,
                (false, false),
                "{heard} at {best:?}"
            );
        }
    }
    for meeting in ["Okay.", "Yeah.", "Thanks.", "Sure."] {
        for best in [None, Some(0.94)] {
            assert_eq!(
                heard_with(meeting, best).await,
                (false, true),
                "{meeting} at {best:?}"
            );
        }
    }
}
