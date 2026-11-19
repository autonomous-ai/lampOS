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
    heard_near_the_lamp(heard, best, false, None).await
}

/// [`heard_with`] for a line begun over the lamp's voice (`over`), or
/// `ended_ago_us` after the lamp's words played to their end.
async fn heard_near_the_lamp(
    heard: &str,
    best: Option<f32>,
    over: bool,
    ended_ago_us: Option<u64>,
) -> (bool, bool) {
    const BEGAN_US: u64 = 100_000_000;
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
    if let Some(ago) = ended_ago_us {
        engine
            .shared
            .said_until_us
            .store(BEGAN_US - ago, Ordering::Relaxed);
    }
    engine.submit(if over { start_over(1) } else { start(1) });
    engine.submit(ProviderInput::Audio {
        request: 1,
        privacy_generation: 1,
        sequence: 1,
        read_completed_at_us: BEGAN_US,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    });
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

/// The Reviewer-lead's P2 on #7994: in the product lamp-name runs without
/// `--not-while-speaking`, and on lamp-4ace it heard the lamp's own reply
/// at 0.990. A short sound begun over the lamp's voice, or within the echo
/// tail after its words ([`own_echo::TAIL_US`]), may be that reply heard
/// back: held as before, the rule never asked, however high the hearing.
/// Begun past the tail, request 12's "Lump?" at 0.917 is still answered.
#[tokio::test(start_paused = true)]
async fn a_short_sound_heard_by_lamp_name_over_the_lamps_voice_or_its_tail_stays_held() {
    for heard in ["Lump?", "Lamb."] {
        for score in [0.917, 0.990] {
            assert_eq!(
                heard_near_the_lamp(heard, Some(score), true, None).await,
                (false, false),
                "{heard} at {score} over the lamp's voice"
            );
            for ago in [300_000, crate::second_provider::own_echo::TAIL_US] {
                assert_eq!(
                    heard_near_the_lamp(heard, Some(score), false, Some(ago)).await,
                    (false, false),
                    "{heard} at {score}, {ago} us after the lamp's words"
                );
            }
        }
    }
    assert_eq!(
        heard_near_the_lamp("Lump?", Some(0.917), false, Some(3_000_000)).await,
        (true, true),
        "Lump? at 0.917, 3 s after the lamp's words"
    );
}
