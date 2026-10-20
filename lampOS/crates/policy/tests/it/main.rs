//! `lamp-policy`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod baseline;
mod fit_lamp_scenes;
mod hello;
mod judges;
mod judges_program;
mod local_comparison;
mod local_fit;
mod local_model;
mod local_timing;
mod pictures;
mod policy;
mod program;
mod progress;
mod replay_program;
