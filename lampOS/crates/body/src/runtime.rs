//! Opt-in process boundary for the guarded motor core. No gesture policy.
//!
//! All clocks use the shared host monotonic domain. See motor-worker.md for the
//! caller obligations, especially qualification, placement and child reaping.
pub mod channel;
pub mod config;
#[cfg(unix)]
pub mod native;
pub mod qualification;
pub mod supervisor;
pub mod wire;
pub mod worker;

#[derive(Clone, Copy)]
pub struct SystemClock;
impl crate::control::Clock for SystemClock {
    fn now(&self) -> lamp_turn_owner::MonoTime {
        lamp_turn_owner::MonoTime::from_micros(lamp_ipc::monotonic_us())
    }
}
