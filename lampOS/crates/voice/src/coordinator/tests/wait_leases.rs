//! The muted wait's gate keeps the privacy worker's lease renewed between
//! its checks (#4332). No worker: the wait is a stand-in that times each
//! `keep` the gate makes.
use super::*;

/// The gate's checks slowed to ~100 ms each (`TEST_WAIT_CHECK`), ~400 ms
/// a look in all, as on lamp-06c1's cold boot: the gate renews between
/// every two of them, so no stretch from the look's start, from one
/// renewal to the next or from the last to its end is longer than one
/// check, well inside the worker's 250 ms lease. A renewal grants nothing;
/// `service_privacy_controls` pins that it carries the wait's Denied
/// authority and that one check held past 250 ms still lapses it.
#[test]
fn the_muted_waits_slow_checks_renew_the_privacy_lease_between_them() {
    use crate::service_privacy::WaitOutcome;
    TEST_WAIT_CHECK.with(|slow| slow.set(Duration::from_millis(100)));
    let reminders = RefCell::new(None);
    let controls = RefCell::new(None);
    let mut trace = SessionState::default();
    let mut looks: Vec<(Instant, Vec<Instant>, Instant)> = Vec::new();
    let outcome = muted_wait(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        None,
        |_, gate| {
            for _ in 0..2 {
                let began = Instant::now();
                let mut kept = Vec::new();
                gate(&mut |_| {
                    kept.push(Instant::now());
                    Ok(())
                })?;
                looks.push((began, kept, Instant::now()));
            }
            Ok(WaitOutcome::Allowed)
        },
    );
    TEST_WAIT_CHECK.with(|slow| slow.set(Duration::ZERO));
    assert_eq!(outcome.unwrap(), WaitOutcome::Allowed);
    for (began, kept, ended) in looks {
        let took = ended.duration_since(began);
        assert!(took >= Duration::from_millis(400), "{took:?}: not slowed");
        assert!(kept.len() >= 4, "{} renewals in {took:?}", kept.len());
        let marks: Vec<Instant> = std::iter::once(began)
            .chain(kept)
            .chain(std::iter::once(ended))
            .collect();
        let longest = marks
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]))
            .max()
            .unwrap();
        assert!(
            longest < Duration::from_millis(200),
            "{longest:?} between renewals in a look of {took:?}"
        );
    }
}
