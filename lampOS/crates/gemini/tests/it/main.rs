//! `lamp-gemini`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod comparison_config;
mod review_2026_10_14;
mod wire;
