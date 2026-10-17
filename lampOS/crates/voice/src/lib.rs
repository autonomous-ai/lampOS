//! Runtime integration for a directed conversation qualification session.
//!
//! Speech activity is an observation, not evidence of who is being addressed.
//! The initial harness requires an explicitly opened, finite directed session;
//! this does not qualify open-desk addressee recognition or background restraint.
/// A process-wide value the provider worker keeps, read through `$name()`.
///
/// In the service it is one static: the provider worker is one process.
/// Under test it is one per test thread (#882): many workers run in one
/// test process, each numbering its requests from 1, and one shared value
/// let one test's worker change what another test's worker reads. Each
/// worker test runs on its own thread (`#[tokio::test]` is current-thread)
/// and the worker reads and writes these only on its task.
// ponytail: keyed by thread, not by worker; a test that drives a worker
// from a multi-thread runtime needs the value passed to the worker instead.
macro_rules! worker_global {
    ($(#[$meta:meta])* $vis:vis fn $name:ident() -> $ty:ty = $init:expr;) => {
        $(#[$meta])*
        #[cfg(not(test))]
        $vis fn $name() -> &'static $ty {
            static VALUE: $ty = $init;
            &VALUE
        }
        $(#[$meta])*
        #[cfg(test)]
        $vis fn $name() -> &'static $ty {
            thread_local! {
                static VALUE: &'static $ty = Box::leak(Box::new($init));
            }
            VALUE.with(|value| *value)
        }
    };
}

pub mod activity;
pub mod addressed;
pub mod admission;
pub mod agent_tasks;
pub mod air_told;
pub mod asker_first;
pub mod attention;
pub mod body_link;
pub mod bus_out;
pub mod called;
pub mod camera_config;
pub mod camera_inspect;
pub mod camera_worker;
pub mod choreography;
pub mod config;
pub mod diagnostic_control;
pub mod diagnostics;
pub mod expression_out;
pub mod first_output;
pub mod fixture_provider;
pub mod follows_on;
pub mod hand_controls;
pub mod head_qualification;
pub mod known_faces;
pub mod listen_gaps;
pub mod local_time;
pub mod marked;
pub mod person;
pub use lamp_ipc::log_relay;
pub mod head_cue;
pub mod not_speech;
pub mod onboarding;
pub mod options;
#[cfg(target_os = "linux")]
pub mod physical_privacy;
#[cfg(any(target_os = "linux", test))]
mod playback;
pub mod policy_holds;
pub mod privacy;
pub mod privacy_bus;
#[cfg(any(target_os = "linux", test))]
pub mod privacy_indicator;
pub mod process;
pub(crate) mod provider_flow;
pub mod provider_link;
pub mod provider_worker;
pub mod quiet_pace;
pub mod reference;
pub mod remember;
pub mod reminders;
pub mod resend;
pub mod ring_asleep;
pub mod ring_face;
pub mod ring_head;
pub mod ring_light;
pub mod ring_requests;
pub mod ring_sight;
pub mod ring_wire;
pub mod ring_worker;
pub mod room_floor;
pub mod room_told;
pub mod rules;
pub mod same_voice;
pub mod say;
pub mod second_config;
pub mod second_look;
pub mod second_provider;
pub mod session_fact;
pub mod short_turns;
pub mod sight;
pub mod speak_up;
pub mod speaker_check;
pub mod staff_note;
pub mod stalls;
pub mod stop_words;
pub mod task_news;
mod termination;
pub mod thread;
pub mod tool_turns;
pub mod tools;
pub mod transport;
mod turn_taking;
pub mod turn_times;
pub mod visual_context;
pub mod wait_words;
pub mod wire;

#[cfg(target_os = "linux")]
pub mod audio_workers;
pub mod coordinator;

mod receipt_history;
mod service_trace;
mod session_lifetime;
mod session_state;

#[cfg(test)]
mod scenario_clock;
mod service_privacy;
mod service_privacy_controls;
