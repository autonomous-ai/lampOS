//! The outer service owns no audio. Only a verified physical/software mute, or
//! on the Controls route a switch that cannot be read (closed, #795), may
//! create another runtime; faults and cleanup failures never become retries.
use super::*;
use crate::service_privacy::{PhysicalMute, SoftwareMute, WaitOutcome, is_unavailable};

pub(super) fn supervise(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    mut wait: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<WaitOutcome>,
    mut active: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<()>,
) -> Result<()> {
    let mut cycle = 0u64;
    loop {
        if termination.signal_name().is_some() {
            return Ok(());
        }
        record(
            trace,
            json!({"kind":"service_waiting_for_privacy","at_us":monotonic_us(),"next_cycle":cycle.checked_add(1),"audio_workers_open":false}),
        )?;
        flush_cues(trace, cues);
        match wait(trace, cues)? {
            WaitOutcome::Stopped => return Ok(()),
            WaitOutcome::Allowed => {}
        }
        if termination.signal_name().is_some() {
            return Ok(());
        }
        cycle = cycle
            .checked_add(1)
            .ok_or_else(|| io::Error::other("service cycle overflow"))?;
        // The previous rig has been closed and reaped. New boots cannot settle
        // old commands. One logger/queue remains for the whole service invocation.
        trace.receipts = Default::default();
        record(
            trace,
            json!({"kind":"service_cycle_start","at_us":monotonic_us(),"cycle":cycle,"fresh_runtime_required":true,"context_continuity_claimed":false}),
        )?;
        match active(trace, cues) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.downcast_ref::<PhysicalMute>().is_some()
                    || error.downcast_ref::<SoftwareMute>().is_some() =>
            {
                let (reason, observed_at_us) =
                    if let Some(mute) = error.downcast_ref::<SoftwareMute>() {
                        let reason = if mute.asleep {
                            "head_sleep"
                        } else {
                            "software_muted"
                        };
                        (reason, mute.observed_at_us)
                    } else {
                        (
                            "physical_muted",
                            error
                                .downcast_ref::<PhysicalMute>()
                                .expect("matched a physical or software mute")
                                .observed_at_us,
                        )
                    };
                record(
                    trace,
                    json!({"kind":"service_cycle_muted","at_us":monotonic_us(),"cycle":cycle,"reason":reason,"observed_at_us":observed_at_us,"cleanup_verified":true}),
                )?;
                flush_cues(trace, cues);
            }
            // The switch could not be read: the microphones are closed and
            // the wait reads it again. A failed cleanup is never this.
            Err(error) if is_unavailable(&*error) => {
                eprintln!("lamp-voice: microphones closed until the switch can be read: {error}");
                record(
                    trace,
                    json!({"kind":"service_cycle_muted","at_us":monotonic_us(),"cycle":cycle,"reason":"privacy_unreadable","why":error.to_string(),"cleanup_verified":true}),
                )?;
                flush_cues(trace, cues);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The first builder on #795 (13:50): with the Controls privacy route
    /// on, the red breath is the hardware switch's and the asleep face stays
    /// off, so the two never write the ring at once.
    #[test]
    fn the_asleep_face_stays_off_with_the_controls_privacy_route() {
        assert!(super::super::asleep_face_allowed(true, false));
        assert!(!super::super::asleep_face_allowed(true, true));
        assert!(!super::super::asleep_face_allowed(false, false));
        assert!(!super::super::asleep_face_allowed(false, true));
    }

    /// A lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`): its wait
    /// paints no physical-mute indication, so its ring is as with the route
    /// off: the asleep face, the tap face and the reminder pulse all show.
    /// With a slide (`on`) they stay off, as before.
    #[test]
    fn the_ring_of_a_lamp_with_no_slide_is_as_with_the_route_off() {
        use super::super::{
            asleep_face_allowed, privacy_ring, pulse_without_face, tap_face_allowed,
        };
        use crate::privacy_bus::Route;
        assert!(!privacy_ring(None));
        assert!(privacy_ring(Some(Route::Slide)));
        assert!(!privacy_ring(Some(Route::NoSlide)));
        let no_slide = privacy_ring(Some(Route::NoSlide));
        assert!(asleep_face_allowed(true, no_slide));
        assert!(tap_face_allowed(true, false, no_slide));
        assert_eq!(pulse_without_face(true, true, true, no_slide), None);
        let slide = privacy_ring(Some(Route::Slide));
        assert!(!asleep_face_allowed(true, slide));
        assert!(!tap_face_allowed(true, false, slide));
        assert!(pulse_without_face(true, true, true, slide).is_some());
    }

    #[test]
    fn verified_mutes_repeat_the_privacy_wait_and_record_their_actual_source() {
        // Physical, software, and software because the head put it to
        // sleep (#711).
        for (software, asleep) in [(false, false), (true, false), (true, true)] {
            let mut trace = SessionState::default();
            let waits = Cell::new(0);
            let active = Cell::new(0);
            supervise(
                &mut trace,
                &mut None,
                &Termination::inactive(),
                |_, _| {
                    waits.set(waits.get() + 1);
                    assert_eq!(waits.get(), active.get() + 1);
                    Ok(WaitOutcome::Allowed)
                },
                |_, _| {
                    active.set(active.get() + 1);
                    if active.get() == 1 {
                        let mute: Box<dyn std::error::Error + Send + Sync> = if software {
                            Box::new(SoftwareMute {
                                observed_at_us: 71,
                                asleep,
                            })
                        } else {
                            Box::new(PhysicalMute { observed_at_us: 71 })
                        };
                        crate::service_privacy::finish(Err(mute), Ok(()))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();
            assert_eq!((waits.get(), active.get()), (2, 2));
            let muted: Vec<_> = trace
                .iter()
                .filter(|row| row["kind"] == "service_cycle_muted")
                .collect();
            assert_eq!(muted.len(), 1);
            assert_eq!(
                muted[0]["reason"],
                match (software, asleep) {
                    (true, true) => "head_sleep",
                    (true, false) => "software_muted",
                    _ => "physical_muted",
                }
            );
            assert_eq!(muted[0]["observed_at_us"], 71);
            assert_eq!(muted[0]["cleanup_verified"], true);
        }
    }

    /// #795: on the Controls route a switch that cannot be read closes the
    /// cycle; the service waits and reads again instead of ending, and says
    /// why. With its cleanup failed it still ends.
    #[test]
    fn an_unreadable_switch_repeats_the_wait_unless_its_cleanup_failed() {
        let mut trace = SessionState::default();
        let waits = Cell::new(0);
        let active = Cell::new(0);
        supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                waits.set(waits.get() + 1);
                Ok(WaitOutcome::Allowed)
            },
            |_, _| {
                active.set(active.get() + 1);
                if active.get() == 1 {
                    crate::service_privacy::finish(
                        Err(crate::service_privacy::unavailable(
                            "physical privacy closed or lost freshness",
                        )),
                        Ok(()),
                    )
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!((waits.get(), active.get()), (2, 2));
        let muted: Vec<_> = trace
            .iter()
            .filter(|row| row["kind"] == "service_cycle_muted")
            .collect();
        assert_eq!(muted.len(), 1);
        assert_eq!(muted[0]["reason"], "privacy_unreadable");
        let error = supervise(
            &mut SessionState::default(),
            &mut None,
            &Termination::inactive(),
            |_, _| Ok(WaitOutcome::Allowed),
            |_, _| {
                crate::service_privacy::finish(
                    Err(crate::service_privacy::unavailable("no switch")),
                    Err(io::Error::other("capture still open")),
                )
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("verified cleanup failed"));
    }

    #[test]
    fn software_mute_with_failed_cleanup_never_starts_a_second_wait() {
        let mut trace = SessionState::default();
        let waits = Cell::new(0);
        let active = Cell::new(0);
        let error = supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                waits.set(waits.get() + 1);
                assert_eq!(waits.get(), 1, "failed cleanup must not be retried");
                Ok(WaitOutcome::Allowed)
            },
            |_, _| {
                active.set(active.get() + 1);
                crate::service_privacy::finish(
                    Err(Box::new(SoftwareMute {
                        observed_at_us: 73,
                        asleep: false,
                    })),
                    Err(io::Error::other("synthetic cleanup failure")),
                )
            },
        )
        .unwrap_err();
        assert_eq!((waits.get(), active.get()), (1, 1));
        assert!(error.downcast_ref::<SoftwareMute>().is_none());
        assert!(error.to_string().contains("synthetic cleanup failure"));
        assert!(!trace.iter().any(|row| row["kind"] == "service_cycle_muted"));
    }
}
