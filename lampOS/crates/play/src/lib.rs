//! `lamp-play`: the lamp plays an MP3 from a URL an app was given by the
//! lamp's cloud (#7415). It is a unit of its own, apart from `lamp-voice`,
//! that talks only to `lamp-control` and the internet.
//!
//! [`hosts`] is what a play may touch, checked before any connect;
//! [`fetch`] is the one GET (https, the redirects checked); [`decode`] turns the MP3 into 24 kHz mono PCM, [`resample`] is its
//! converter.
pub mod accept;
pub mod decode;
pub mod fetch;
pub mod hosts;
pub mod peer;
pub mod protocol;
pub mod resample;
pub mod serve;
