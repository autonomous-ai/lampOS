//! A hand's sleep that hears its name, through the muted wait itself
//! (migrate-P2, `LAMP_SLEEP_HEARS_NAME`; the Queue comment
//! <https://github.com/autonomous-ai/lampOS/issues/3997#issuecomment-6526292114>).
use super::*;
use crate::{
    privacy::PrivacyState,
    service_privacy::WaitOutcome,
    sleep_capture::{Mode, SleepName, tests::Rig, tests::say_name},
};
use std::{cell::RefCell, os::unix::fs::PermissionsExt, rc::Rc};

fn asleep(name: &str) -> (RefCell<Option<HandControls>>, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "lamp-voice-sleep-name-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    (
        RefCell::new(Some(
            HandControls::with_test_asleep_connected("test-hand-clock", monotonic_us(), &record)
                .unwrap(),
        )),
        directory,
    )
}

/// With the slide open, a sleep by a hold listens and says `hears_name`
/// on `voice/mute`; the name twice wakes it; the capture and its grant are
/// closed before the wait gives Allowed, so the woken cycle's rig opens
/// with nothing of the sleep's still open; and the wake is a tap's: its
/// "hm?" at the wake's time.
#[test]
fn the_name_wakes_a_sleep_and_its_capture_is_closed_before_the_wait_opens() {
    let (controls, directory) = asleep("wake");
    let reminders = RefCell::new(None);
    let (rig, _readers) = Rig::new(Mode::On, &["name"]);
    let rig = Rc::new(RefCell::new(rig));
    let starter = rig.clone();
    let name = SleepName::new(
        Mode::On,
        Box::new(move |_, _| Ok(starter.borrow_mut().capture.take().expect("one start"))),
    );
    let mut trace = SessionState::default();
    let mut heard_while_listening = None;
    let mut ended_when_allowed = None;
    let outcome = muted_wait_pulsing(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        None::<crate::ring_asleep::SleepFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        None,
        None,
        None,
        Some(name),
        |on_state, gate| {
            let boot = crate::process::new_boot().unwrap();
            on_state(PrivacyState::Allowed, boot, 1)?;
            assert!(!gate(&mut |_| Ok(()))?);
            heard_while_listening = controls
                .borrow()
                .as_ref()
                .and_then(HandControls::said_for_test)
                .map(|said| said.hears_name);
            let mut out = None;
            let bus = rig.borrow().bus.clone();
            say_name(&bus, &mut out, 0.95);
            say_name(&bus, &mut out, 0.95);
            let until = Instant::now() + Duration::from_secs(3);
            loop {
                if gate(&mut |_| Ok(()))? {
                    ended_when_allowed = Some(rig.borrow().worker_ended());
                    return Ok(WaitOutcome::Allowed);
                }
                assert!(Instant::now() < until, "the name never woke it");
                std::thread::sleep(Duration::from_millis(3));
            }
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Allowed);
    assert_eq!(heard_while_listening, Some(true));
    assert_eq!(
        ended_when_allowed,
        Some(true),
        "closed before the wait opened"
    );
    let wake = trace
        .iter()
        .find(|row| row["kind"] == "head_sleep_name_wake")
        .expect("the wake is in the trace");
    let at = wake["at_us"].as_u64().unwrap();
    let mut controls = controls.into_inner().unwrap();
    assert_eq!(controls.take_woken(at + 1), Some(at));
    assert!(
        controls
            .said_for_test()
            .is_some_and(|said| !said.muted && !said.hears_name)
    );
    let _ = fs::remove_dir_all(directory);
}

/// The slide closed: nothing starts, and `voice/mute` says the sleep as
/// before, with no `hears_name`. A person's own mute never listens either.
#[test]
fn a_closed_slide_or_a_persons_mute_never_listens() {
    for closed_slide in [true, false] {
        let (controls, directory) = if closed_slide {
            asleep("slide")
        } else {
            let directory = std::env::temp_dir().join(format!(
                "lamp-voice-sleep-name-muted-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&directory);
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            let record = directory.join("mute.json");
            (
                RefCell::new(Some(
                    HandControls::with_test_muted("test-hand-clock", monotonic_us(), &record)
                        .unwrap(),
                )),
                directory,
            )
        };
        let reminders = RefCell::new(None);
        let starts = std::cell::Cell::new(0);
        let name = SleepName::new(
            Mode::On,
            Box::new(|_, _| {
                starts.set(starts.get() + 1);
                Err(io::Error::other("never started here").into())
            }),
        );
        let mut trace = SessionState::default();
        muted_wait_pulsing(
            &mut trace,
            &mut None,
            &controls,
            &reminders,
            None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
            None::<crate::ring_asleep::SleepFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
            None,
            None,
            None,
            Some(name),
            |on_state, gate| {
                let boot = crate::process::new_boot().unwrap();
                let slide = if closed_slide {
                    PrivacyState::Muted
                } else {
                    PrivacyState::Allowed
                };
                on_state(slide, boot, 1)?;
                for _ in 0..3 {
                    assert!(!gate(&mut |_| Ok(()))?);
                }
                Ok(WaitOutcome::Stopped)
            },
        )
        .unwrap();
        assert_eq!(starts.get(), 0, "closed slide {closed_slide}");
        let said = controls
            .borrow()
            .as_ref()
            .and_then(HandControls::said_for_test)
            .unwrap();
        assert!(said.muted && !said.hears_name, "{said:?}");
        let _ = fs::remove_dir_all(directory);
    }
}
