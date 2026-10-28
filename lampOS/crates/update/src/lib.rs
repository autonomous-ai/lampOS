//! Updates: a lamp keeps its lampOS current by itself (docs/updates.md).
//!
//! [`manifest`] is what a build is and its signature; [`publish`] makes
//! one where it is built; [`fetch`] reads a channel; [`unit`] reads and
//! changes a part's unit; [`quiet`] says when the voice may be switched;
//! [`judge`] whether a part is well after it; [`records`] is what is kept;
//! [`world`] is systemd, the bus, the disk and the clock behind traits;
//! [`product_skills`] is the product's skill folders a build carries;
//! [`round`] is one round of an update, with all of them; and [`own`] is
//! lamp-update switching itself, and putting itself back; [`models`] the
//! models a build carries; [`apply`] the settings and tmpfiles it carries,
//! written by `lamp-update-apply`; [`switches`] the product's switches,
//! and a kept file that turns one off.
pub mod apply;
pub mod check_units;
pub mod companion;
pub mod fetch;
pub mod fixed_copy;
mod head_touch;
pub mod image;
pub mod install;
pub mod judge;
pub mod manifest;
pub mod models;
pub mod own;
pub mod product_skills;
pub mod publish;
pub mod quiet;
pub mod records;
pub mod round;
mod setup_lifecycle;
pub mod status;
pub mod switches;
pub mod unit;
pub mod world;
pub mod writer_files;
pub(crate) mod writer_plan;
pub mod writer_preimages;
pub mod writer_ready;
pub mod writer_refresh;
pub mod writer_settings;
pub mod writer_swap;
pub mod writer_unit;

pub use round::{Config, Outcome, World, round};
