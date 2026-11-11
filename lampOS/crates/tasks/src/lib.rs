//! Optional durable work tracking through the provisioned Harness CLI.
//! Tasks never opens an output device or implements Harness authentication:
//! it only runs Harness's own sign-in, and passes the person's answer on
//! ([`sign_in`]).
pub mod engine;
pub mod follow;
pub mod gateway;
pub mod harness;
pub mod hermes_config;
pub mod hold;
pub mod net_bridge;
pub mod now;
pub mod openclaw_config;
pub mod progress;
pub mod protocol;
pub mod readiness;
pub mod runtime;
pub mod sign_in;
pub mod skill_call;
pub mod skill_task;
pub mod store;
