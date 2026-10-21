//! `lamp-audio`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod cancelled_tail;
mod capture_linux;
mod dsp;
mod evidence;
mod fixtures;
mod notice;
mod pcm_linux;
mod playout_continuity;
mod playout_current_arrivals;
mod queued_ownership;
mod replay;
mod steady_reference;
