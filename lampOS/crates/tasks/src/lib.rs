//! Optional durable work tracking through the provisioned Harness CLI.
//! Tasks never opens an output device or implements Harness authentication:
//! it only runs Harness's own sign-in, and passes the person's answer on
//! ([`sign_in`]).
pub mod engine;
pub mod harness;
pub mod progress;
pub mod protocol;
pub mod runtime;
pub mod sign_in;
pub mod store;
