//! `lamp-apps-manage run NAME`: what `lamp-app@NAME.service` and `lamp-app-net@NAME.service` start (#7956).
//!
//! The two templates run this part's own program, so a round carries them as companions of the part
//! (a round carries only units that start a lamp program) and no new kind of unit or updater level is
//! needed. All it does is check the name and become the fixed launcher: `/usr/bin/python3 -I
//! <code>/.run.py NAME`, a fixed argv with no shell and no `PATH` lookup, and nothing read from the
//! environment to pick the program. The unit's sandbox is the app's own (dynamic user, strict, no
//! capabilities).
use crate::{layout::Layout, name::app_name_ok};

/// The program the launcher is run by.
pub const PYTHON: &str = "/usr/bin/python3";

/// The argv of the launcher for the app `name`, or why not.
pub fn argv(layout: &Layout, name: &str) -> Result<Vec<String>, String> {
    if !app_name_ok(name) {
        return Err(format!("not an app's name: {name:?}"));
    }
    Ok(vec![
        PYTHON.into(),
        "-I".into(),
        layout.launcher().display().to_string(),
        name.into(),
    ])
}
