//! The scheduling a worker takes as it starts (#1608). The voice runs at
//! real-time RR 20 (its units' `CPUSchedulingPolicy=rr`) without
//! `CPUSchedulingResetOnFork`, so every worker it starts begins there too. A
//! busy real-time worker takes the CPU from every ordinary part of the lamp:
//! `lamp-controls`, holding the privacy leases, lost them once while a
//! same-voice child warmed its model at RR 20 (18 Oct). So each worker sets
//! its role's scheduling (`WorkerRole::scheduling`) on its first thread,
//! before any other starts, and every thread it makes after takes it on.
//! Called by `lamp-voice worker ROLE` first of all, in `main`.
use lamp_voice_processes::Scheduling;
use std::io;

/// Off unless exactly `on`: with it, each worker takes its role's
/// scheduling; without it, every worker runs at the voice's RR 20 as before.
/// The voice passes it to every worker it starts (`worker_command`).
pub const ENVIRONMENT: &str = "LAMP_WORKER_SCHEDULING";

/// Whether the switch is on: `on`, and nothing else.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Sets `scheduling` on the calling thread, and checks it took.
#[cfg(target_os = "linux")]
pub fn take(scheduling: Scheduling) -> io::Result<()> {
    use thread_priority::{
        NormalThreadSchedulePolicy, RealtimeThreadSchedulePolicy, ThreadPriority,
        ThreadPriorityOsValue, ThreadPriorityValue, ThreadSchedulePolicy,
        set_thread_priority_and_policy, thread_native_id, thread_schedule_policy_param,
    };

    let thread = thread_native_id();
    match scheduling {
        Scheduling::KeepsVoices => Ok(()),
        Scheduling::Ordinary => {
            let ordinary = ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Other);
            // The ordinary policy at nice 0 (the value's default). Lowering
            // a thread's own priority needs no privilege. Nice 0 from a nicer
            // start would raise it, so one already nicer is refused that
            // part and stays nicer, at the ordinary policy all the same.
            let _ = set_thread_priority_and_policy(
                thread,
                ThreadPriority::Os(ThreadPriorityOsValue::default()),
                ordinary,
            );
            let (policy, _) = thread_schedule_policy_param(thread).map_err(io::Error::other)?;
            if policy != ordinary {
                return Err(io::Error::other(format!(
                    "the ordinary policy was not taken: {policy:?}"
                )));
            }
            Ok(())
        }
        Scheduling::Realtime(priority) => {
            let (policy, parameters) =
                thread_schedule_policy_param(thread).map_err(io::Error::other)?;
            // Only lowered: a worker not at a real-time policy (a test, a
            // lamp whose unit has none) or one already lower stays as it is.
            if !matches!(policy, ThreadSchedulePolicy::Realtime(_))
                || parameters.sched_priority <= i32::from(priority)
            {
                return Ok(());
            }
            let value = ThreadPriorityValue::try_from(priority).map_err(io::Error::other)?;
            set_thread_priority_and_policy(
                thread,
                ThreadPriority::Crossplatform(value),
                ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::RoundRobin),
            )
            .map_err(io::Error::other)
        }
    }
}

/// Off Linux (the Mac's tests) nothing is real-time: every worker runs as it
/// started.
#[cfg(not(target_os = "linux"))]
pub fn take(_scheduling: Scheduling) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::take;
    use lamp_voice_processes::Scheduling;
    use thread_priority::{
        NormalThreadSchedulePolicy, ThreadPriority, ThreadSchedulePolicy,
        set_thread_priority_and_policy, thread_native_id, thread_schedule_policy_param,
    };

    const BATCH: ThreadSchedulePolicy =
        ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch);
    const OTHER: ThreadSchedulePolicy =
        ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Other);

    /// A test can't start real-time without privilege, so each starts from
    /// `SCHED_BATCH`, which any thread may take: a worker that changes its
    /// policy shows it here as one that leaves BATCH.
    fn from_batch(scheduling: Scheduling) -> ThreadSchedulePolicy {
        std::thread::spawn(move || {
            let thread = thread_native_id();
            set_thread_priority_and_policy(thread, ThreadPriority::Min, BATCH).unwrap();
            assert_eq!(thread_schedule_policy_param(thread).unwrap().0, BATCH);
            take(scheduling).unwrap();
            thread_schedule_policy_param(thread).unwrap().0
        })
        .join()
        .unwrap()
    }

    #[test]
    fn an_ordinary_worker_takes_the_ordinary_policy() {
        assert_eq!(from_batch(Scheduling::Ordinary), OTHER);
    }

    #[test]
    fn the_audio_keeps_what_it_started_with() {
        assert_eq!(from_batch(Scheduling::KeepsVoices), BATCH);
    }

    /// Not real-time to start with: nothing to lower, and no real-time taken.
    #[test]
    fn the_ring_never_takes_a_real_time_policy_it_did_not_start_with() {
        assert_eq!(from_batch(Scheduling::Realtime(10)), BATCH);
    }

    /// From the voice's own RR 20, where the host allows a test to take it
    /// (a container with `SYS_NICE`; skipped elsewhere): the ring lowers to
    /// RR 10, an ordinary worker to `SCHED_OTHER`, and the audio keeps RR 20.
    #[test]
    fn from_the_voices_rr_20_each_role_takes_its_own() {
        use thread_priority::{
            RealtimeThreadSchedulePolicy, ThreadPriorityValue, ThreadSchedulePolicy,
        };
        const RR: ThreadSchedulePolicy =
            ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::RoundRobin);
        let from_rr_20 = |scheduling: Scheduling| {
            std::thread::spawn(move || {
                let thread = thread_native_id();
                let twenty =
                    ThreadPriority::Crossplatform(ThreadPriorityValue::try_from(20u8).unwrap());
                set_thread_priority_and_policy(thread, twenty, RR).ok()?;
                take(scheduling).unwrap();
                let (policy, parameters) = thread_schedule_policy_param(thread).unwrap();
                Some((policy, parameters.sched_priority))
            })
            .join()
            .unwrap()
        };
        let Some(ring) = from_rr_20(Scheduling::Realtime(10)) else {
            eprintln!("skipped: this host gives a test no real-time policy");
            return;
        };
        assert_eq!(ring, (RR, 10));
        assert_eq!(from_rr_20(Scheduling::Ordinary).unwrap(), (OTHER, 0));
        assert_eq!(from_rr_20(Scheduling::KeepsVoices).unwrap(), (RR, 20));
    }
}
