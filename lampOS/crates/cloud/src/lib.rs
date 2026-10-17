//! `lamp-cloud`: the lamp on the company broker, so the Autonomous app sees
//! it online and its page can ask it things (#1396). It keeps V1's wire:
//! `info` and `data` commands on `fa_channel`, replies on `fd_channel`, and
//! the backend's 15 s HTTP ping beside them. Off unless `LAMP_CLOUD=on`.
//! See `docs/cloud.md`.
pub mod account;
pub mod answer;
pub mod beat;
pub mod lamp;
pub mod online;
pub mod ping;
pub mod wire;
