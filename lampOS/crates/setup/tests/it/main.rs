//! `lamp-setup`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! `tests/wake.rs` stays a binary of its own: it hands a real `flock` on its
//! device file over and opens it again at once, and a `lamp-setup` child
//! that `change`, `reset` or `serve` starts here would hold a copy of it
//! until its exec (the reviewer on #2409). `tests/update_lock.rs` stays apart
//! too: its tests let go of lamp-update's lock and take it again at once, and
//! on macOS a non-blocking `flock` on a lock nobody holds is turned down now
//! and then while this binary's other tests are busy with files (#1569).
mod bus_names;
mod change;
mod change_rig;
mod keys;
mod light;
mod recover;
mod reset;
mod serve;
mod wifi;
mod wifi_change;
