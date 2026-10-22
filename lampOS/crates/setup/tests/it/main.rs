//! `lamp-setup`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! `tests/wake.rs` stays a binary of its own: it hands a real `flock` on its
//! device file over and opens it again at once, and a `lamp-setup` child
//! that `change`, `reset` or `serve` starts here would hold a copy of it
//! until its exec (the reviewer on #2409).
mod bus_names;
mod change;
mod keys;
mod light;
mod recover;
mod reset;
mod serve;
mod wifi;
mod wifi_change;
