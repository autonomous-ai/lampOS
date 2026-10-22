//! `LAMP_WORKERS_OTHER` (#1608): the voice's workers that leave its
//! real-time round-robin. The unit runs the voice at `CPUSchedulingPolicy=rr`
//! priority 20 with no `CPUSchedulingResetOnFork`, so every `lamp-voice
//! worker <role>` it starts, and every thread of one, inherits round-robin 20
//! (twelve's audit, 21 Oct). Only the microphone and the speaker need it
//! (`serve-runtime.md`, "Beside other work on the lamp"): the provider, the
//! second route, the camera, the privacy worker and the ring do network,
//! picture or light work that at round-robin 20 shares the audio threads'
//! time and starves everything at ordinary priority. With the switch on,
//! each of those takes `SCHED_OTHER` at [`NICE`] as the first thing it does,
//! before the log relay's thread or any of its own, so all its threads have
//! it; capture and speaker keep the voice's. A change that fails is logged
//! and the worker runs on: never a reason to stop the voice.
use lamp_voice_processes::WorkerRole;
use std::{ffi::OsStr, io};

pub const ENVIRONMENT: &str = "LAMP_WORKERS_OTHER";

/// The niceness a worker that leaves round-robin takes: ordinary, not
/// lowered as the same-voice print's 10 (#1445) or the schedule consumer's
/// 19 are. These workers carry the reply (the provider's and the second
/// route's sockets, the ring's light) and their work is short; a higher nice
/// would let the eyes' and the body's ordinary work hold up a reply.
pub const NICE: i32 = 0;

/// `on` exactly; anything else, unset included, is off (#1608).
pub fn switched_on(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("on"))
}

/// What a worker does with the voice's scheduling when the switch is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Keeps the voice's real-time round-robin: the audio's timing needs it.
    Kept,
    /// Takes `SCHED_OTHER` at [`NICE`].
    Ordinary,
}

/// Each role's choice (#1608). Every role is named, so a new one does not
/// build until it is decided here.
pub const fn policy(role: WorkerRole) -> Policy {
    match role {
        // A microphone read over 10 ms or a speaker reference about 20 ms
        // late ends the voice (`serve-runtime.md`).
        WorkerRole::Capture | WorkerRole::Speaker => Policy::Kept,
        // The test fixture's worker: no work to lower.
        WorkerRole::Probe => Policy::Kept,
        WorkerRole::Provider
        | WorkerRole::Second
        | WorkerRole::Camera
        | WorkerRole::Ring
        | WorkerRole::Privacy => Policy::Ordinary,
    }
}

/// Called first in `main`, before the log relay starts its thread: for
/// `worker ROLE` with the switch on, takes the role's policy and gives the
/// line to log once the relay is up. `None`, with nothing touched, for any
/// other command, a role not on the list (`main` refuses it next) or the
/// switch off.
pub fn at_start(command: Option<&str>, role: Option<&str>) -> Option<String> {
    if command != Some(lamp_voice_processes::Command::Worker.as_str()) {
        return None;
    }
    let role = WorkerRole::parse(role?)?;
    take_with(role, std::env::var_os(ENVIRONMENT).as_deref(), ordinary)
}

/// `at_start` over a given switch value and a given way to lower the
/// calling thread (the tests fake its failure).
fn take_with(
    role: WorkerRole,
    value: Option<&OsStr>,
    lower: impl FnOnce() -> io::Result<i32>,
) -> Option<String> {
    if !switched_on(value) {
        return None;
    }
    let name = role.as_str();
    Some(match policy(role) {
        Policy::Kept => {
            format!("worker {name}: keeps the voice's real-time scheduling ({ENVIRONMENT})")
        }
        Policy::Ordinary => match lower() {
            Ok(nice) => format!("worker {name}: took SCHED_OTHER nice {nice} ({ENVIRONMENT})"),
            Err(error) => format!(
                "worker {name}: could not take SCHED_OTHER ({error}); runs on with the voice's scheduling ({ENVIRONMENT})"
            ),
        },
    })
}

/// The value 0 to 99 that thread-priority 3.1.1 maps to nice 0 under a
/// normal policy on Linux: `(39 * (1 - 47 / 99)) as i8 - 20` is 0.
#[cfg(target_os = "linux")]
const CROSSPLATFORM_NICE_ZERO: u8 = 47;

/// The calling thread to `SCHED_OTHER` at [`NICE`], as the schedule
/// consumer's (`reminders/scheduling.rs`), through thread-priority with no
/// unsafe code of ours; the threads it starts afterwards inherit it. The
/// nice it reads back: one already nicer than [`NICE`] stays so, as the
/// same-voice child's does (#1445), since raising it again may need
/// privilege.
#[cfg(target_os = "linux")]
fn ordinary() -> io::Result<i32> {
    use thread_priority::{
        NormalThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
        set_thread_priority_and_policy, thread_native_id, thread_schedule_policy_param,
    };

    let thread = thread_native_id();
    let policy = ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Other);
    let value = ThreadPriorityValue::try_from(CROSSPLATFORM_NICE_ZERO).map_err(io::Error::other)?;
    // The policy is set before the nice: a refused nice still leaves the
    // policy changed, so what reads back decides.
    let set = set_thread_priority_and_policy(thread, ThreadPriority::Crossplatform(value), policy);
    let (actual, parameters) = thread_schedule_policy_param(thread).map_err(io::Error::other)?;
    let nice = rustix::process::getpriority_process(None)?;
    if actual == policy && parameters.sched_priority == 0 && nice >= NICE {
        return Ok(nice);
    }
    Err(match set {
        Err(error) => io::Error::other(error.to_string()),
        Ok(()) => io::Error::other(format!(
            "read back {actual:?} priority {} nice {nice}",
            parameters.sched_priority
        )),
    })
}

/// Elsewhere the voice does not run at real-time priority.
#[cfg(not(target_os = "linux"))]
fn ordinary() -> io::Result<i32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "only Linux runs the voice at real-time priority",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1608: the roles that leave round-robin, and the ones that keep it.
    #[test]
    fn only_the_audio_workers_and_the_probe_keep_round_robin() {
        for role in WorkerRole::ALL {
            let expected = match role.as_str() {
                "capture" | "speaker" | "probe" => Policy::Kept,
                "provider" | "second" | "camera" | "ring" | "privacy" => Policy::Ordinary,
                other => panic!("{other} is not decided in this test"),
            };
            assert_eq!(policy(role), expected, "{}", role.as_str());
        }
    }

    #[test]
    fn the_switch_is_on_only_for_the_exact_on() {
        assert!(switched_on(Some(OsStr::new("on"))));
        for off in [
            None,
            Some("ON"),
            Some("On"),
            Some("on "),
            Some(""),
            Some("1"),
            Some("true"),
            Some("off"),
        ] {
            assert!(!switched_on(off.map(OsStr::new)), "{off:?}");
        }
    }

    #[test]
    fn off_touches_nothing_and_says_nothing() {
        for role in WorkerRole::ALL {
            for value in [None, Some(OsStr::new("ON")), Some(OsStr::new("off"))] {
                let line = take_with(role, value, || panic!("lowered with the switch off"));
                assert_eq!(line, None);
            }
        }
    }

    #[test]
    fn on_lowers_only_the_ordinary_roles_with_one_line_each() {
        let on = Some(OsStr::new("on"));
        for role in WorkerRole::ALL {
            let mut lowered = false;
            let line = take_with(role, on, || {
                lowered = true;
                Ok(NICE)
            })
            .unwrap();
            let name = role.as_str();
            match policy(role) {
                Policy::Kept => {
                    assert!(!lowered, "{name}");
                    assert_eq!(
                        line,
                        format!(
                            "worker {name}: keeps the voice's real-time scheduling (LAMP_WORKERS_OTHER)"
                        )
                    );
                }
                Policy::Ordinary => {
                    assert!(lowered, "{name}");
                    assert_eq!(
                        line,
                        format!("worker {name}: took SCHED_OTHER nice 0 (LAMP_WORKERS_OTHER)")
                    );
                }
            }
        }
    }

    /// A refused change is said and the worker runs on.
    #[test]
    fn a_refused_change_is_said_and_not_fatal() {
        let line = take_with(WorkerRole::Second, Some(OsStr::new("on")), || {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap();
        assert!(
            line.starts_with("worker second: could not take SCHED_OTHER ("),
            "{line}"
        );
        assert!(
            line.ends_with("runs on with the voice's scheduling (LAMP_WORKERS_OTHER)"),
            "{line}"
        );
    }

    #[test]
    fn only_a_listed_worker_is_considered() {
        assert_eq!(at_start(Some("serve"), Some("provider")), None);
        assert_eq!(at_start(Some("worker"), Some("unknown")), None);
        assert_eq!(at_start(Some("worker"), None), None);
        assert_eq!(at_start(None, None), None);
    }

    /// On Linux the real setter: a thread of this test takes `SCHED_OTHER`
    /// and reads back at least nice 0; the test's other threads keep theirs.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_calling_thread_takes_sched_other() {
        use thread_priority::{
            NormalThreadSchedulePolicy, ThreadSchedulePolicy, thread_native_id,
            thread_schedule_policy_param,
        };
        let parent = thread_schedule_policy_param(thread_native_id()).unwrap().0;
        let (policy, nice) = std::thread::spawn(|| {
            let nice = ordinary().unwrap();
            (
                thread_schedule_policy_param(thread_native_id()).unwrap().0,
                nice,
            )
        })
        .join()
        .unwrap();
        assert_eq!(
            policy,
            ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Other)
        );
        assert!(nice >= NICE);
        assert_eq!(
            thread_schedule_policy_param(thread_native_id()).unwrap().0,
            parent
        );
    }
}
