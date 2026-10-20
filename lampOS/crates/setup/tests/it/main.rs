//! `lamp-setup`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod bus_names;
mod change;
mod keys;
mod light;
mod reset;
mod serve;
mod wake;
mod wifi;
mod wifi_change;
