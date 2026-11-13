//! The lamp's parts that setup's finish leaves behind, started
//! (docs/setup.md, "The parts a target already up leaves behind"). Setup's
//! marker starts `lampos.target` through `lampos-setup-done.path`, and the
//! target pulls in every part it wants.
//! If the target is already active when the marker comes, starting it again
//! does nothing, and a part its own `ConditionPathExists=` skipped at boot,
//! before the marker was there, stays dead until the next reboot. A tester's
//! lamp showed it with `lamp-services`: skipped at boot, never started after
//! setup, and for five days the app showed accounts linked that went nowhere.
//!
//! So, just after writing the marker, setup asks systemd which units
//! `lampos.target` wants (its `Wants=`, which are the units enabled into it)
//! and starts each one that is inactive with `--no-block`. One-shots are left
//! out, since a one-shot part that already ran reads as inactive and would
//! run again, and so are templates. `lamp-services.service` goes first. A
//! running part is never restarted, and a failed one is left as it is. When
//! the target itself is not active, the path's start of it pulls them all in,
//! and nothing more is asked.
//!
//! Best effort and bounded: two reads of [`crate::reset::query::QUERY_WITHIN`]
//! at most, then one `systemctl start --no-block` per unit, each bounded by
//! [`START_WITHIN`]. Every start is one line in the log. None of it ever
//! fails setup.

use std::{io, time::Duration};

/// The lamp as one.
pub const TARGET: &str = "lampos.target";
/// The linked services' part, started first.
pub const SERVICES: &str = "lamp-services.service";
/// The longest one `systemctl start --no-block` may take: it returns once
/// systemd has the job.
pub const START_WITHIN: Duration = Duration::from_secs(5);
const SYSTEMCTL: &str = "/usr/bin/systemctl";

/// The units `lampos.target` wants, one line of names.
pub fn wants_command() -> Vec<String> {
    [
        SYSTEMCTL,
        "show",
        "--property=Wants",
        "--value",
        "--",
        TARGET,
    ]
    .map(str::to_owned)
    .to_vec()
}

/// How each of `units` and the target is.
pub fn states_command(units: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = [SYSTEMCTL, "show", "--property=Id,ActiveState,Type", "--"]
        .map(str::to_owned)
        .to_vec();
    argv.extend(units.iter().cloned());
    argv.push(TARGET.to_owned());
    argv
}

/// One part's start, not waited for.
pub fn start_command(unit: &str) -> Vec<String> {
    [SYSTEMCTL, "start", "--no-block", "--", unit]
        .map(str::to_owned)
        .to_vec()
}

/// What [`states_command`] printed: each unit's `Id`, `ActiveState` and
/// `Type` (empty for a unit with none), in blocks a blank line apart.
fn states(text: &str) -> Vec<(String, String, String)> {
    let mut all = Vec::new();
    for block in text.split("\n\n") {
        let mut id = String::new();
        let mut active = String::new();
        let mut kind = String::new();
        for line in block.lines() {
            match line.split_once('=') {
                Some(("Id", value)) => id = value.trim().to_owned(),
                Some(("ActiveState", value)) => active = value.trim().to_owned(),
                Some(("Type", value)) => kind = value.trim().to_owned(),
                _ => {}
            }
        }
        if !id.is_empty() {
            all.push((id, active, kind));
        }
    }
    all
}

/// The units to start, from `wants` (the target's `Wants=`) and `shown`
/// (what [`states_command`] printed): none unless the target is active;
/// each wanted unit that is inactive, no template and no one-shot;
/// [`SERVICES`] first, then in the order wanted.
pub fn waiting(wants: &[String], shown: &str) -> Vec<String> {
    let shown = states(shown);
    let is = |unit: &str| {
        shown
            .iter()
            .find(|(id, _, _)| id == unit)
            .map(|(_, active, kind)| (active.as_str(), kind.as_str()))
    };
    if is(TARGET).map(|(active, _)| active) != Some("active") {
        return Vec::new();
    }
    let mut picked: Vec<String> = wants
        .iter()
        .filter(|unit| !unit.contains('@'))
        .filter(|unit| {
            is(unit).is_some_and(|(active, kind)| active == "inactive" && kind != "oneshot")
        })
        .cloned()
        .collect();
    if let Some(at) = picked.iter().position(|unit| unit == SERVICES) {
        let services = picked.remove(at);
        picked.insert(0, services);
    }
    picked
}

/// Start what `lampos.target` wants and does not run, as the module says.
/// `read` is given a query and returns its output; `run` is given a start
/// and its bound. Never fails setup: what went wrong is logged.
pub fn start(
    mut read: impl FnMut(&[String]) -> io::Result<String>,
    mut run: impl FnMut(&[String], Duration) -> io::Result<()>,
    mut log: impl FnMut(&str),
) {
    let wants: Vec<String> = match read(&wants_command()) {
        Ok(text) => text.split_whitespace().map(str::to_owned).collect(),
        Err(why) => {
            log(&format!(
                "what {TARGET} wants cannot be read ({why}); a part it skipped starts at the next boot"
            ));
            return;
        }
    };
    if wants.is_empty() {
        return;
    }
    let shown = match read(&states_command(&wants)) {
        Ok(text) => text,
        Err(why) => {
            log(&format!(
                "the parts of {TARGET} cannot be read ({why}); a part it skipped starts at the next boot"
            ));
            return;
        }
    };
    for unit in waiting(&wants, &shown) {
        match run(&start_command(&unit), START_WITHIN) {
            Ok(()) => log(&format!(
                "started {unit}, which {TARGET} wants and was not running"
            )),
            Err(why) => log(&format!(
                "{unit} could not be started ({why}); it starts at the next boot"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: &str = "Id=lamp-voice.service\nActiveState=active\nType=exec\n\n\
        Id=lamp-mood.service\nActiveState=inactive\nType=simple\n\n\
        Id=lamp-services.service\nActiveState=inactive\nType=simple\n\n\
        Id=lamp-air-clock.service\nActiveState=inactive\nType=oneshot\n\n\
        Id=lamp-cloud.service\nActiveState=failed\nType=simple\n\n\
        Id=lamp-body.service\nActiveState=activating\nType=notify\n\n\
        Id=lamp-setup-keys.socket\nActiveState=inactive\n\n\
        Id=lampos.target\nActiveState=active\n";

    fn wants() -> Vec<String> {
        [
            "lamp-voice.service",
            "lamp-mood.service",
            "lamp-services.service",
            "lamp-air-clock.service",
            "lamp-cloud.service",
            "lamp-body.service",
            "lamp-setup-keys.socket",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn only_inactive_wanted_parts_start_lamp_services_first_never_a_running_failed_or_one_shot() {
        assert_eq!(
            waiting(&wants(), SHOWN),
            [
                "lamp-services.service",
                "lamp-mood.service",
                "lamp-setup-keys.socket"
            ]
        );
    }

    #[test]
    fn with_the_target_not_active_its_own_start_pulls_them_and_none_is_started() {
        let shown = SHOWN.replace(
            "Id=lampos.target\nActiveState=active",
            "Id=lampos.target\nActiveState=inactive",
        );
        assert!(waiting(&wants(), &shown).is_empty());
        let unshown = SHOWN.replace("Id=lampos.target\nActiveState=active\n", "");
        assert!(waiting(&wants(), &unshown).is_empty());
    }

    #[test]
    fn it_asks_the_targets_wants_then_their_states_then_starts_each_without_blocking() {
        let mut asked = Vec::new();
        let mut ran = Vec::new();
        let mut lines = Vec::new();
        start(
            |argv| {
                asked.push(argv.join(" "));
                Ok(if argv.contains(&"--property=Wants".to_owned()) {
                    "lamp-voice.service lamp-mood.service lamp-services.service\n".into()
                } else {
                    SHOWN.into()
                })
            },
            |argv, most| {
                ran.push((argv.join(" "), most));
                Ok(())
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(
            asked,
            [
                "/usr/bin/systemctl show --property=Wants --value -- lampos.target",
                "/usr/bin/systemctl show --property=Id,ActiveState,Type -- lamp-voice.service \
                 lamp-mood.service lamp-services.service lampos.target",
            ]
        );
        assert_eq!(
            ran,
            [
                (
                    "/usr/bin/systemctl start --no-block -- lamp-services.service".to_owned(),
                    START_WITHIN
                ),
                (
                    "/usr/bin/systemctl start --no-block -- lamp-mood.service".to_owned(),
                    START_WITHIN
                ),
            ]
        );
        assert_eq!(
            lines,
            [
                "started lamp-services.service, which lampos.target wants and was not running",
                "started lamp-mood.service, which lampos.target wants and was not running",
            ]
        );
    }

    #[test]
    fn a_read_or_a_start_that_fails_is_logged_and_nothing_else_is_tried() {
        let mut ran = 0;
        let mut lines = Vec::new();
        start(
            |_| Err(io::Error::other("systemctl ended with exit status: 1")),
            |_, _| {
                ran += 1;
                Ok(())
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(ran, 0);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("cannot be read"), "{lines:?}");

        let mut lines = Vec::new();
        start(
            |argv| {
                Ok(if argv.contains(&"--property=Wants".to_owned()) {
                    "lamp-services.service lamp-mood.service".into()
                } else {
                    SHOWN.into()
                })
            },
            |argv, _| {
                if argv.last().is_some_and(|unit| unit == SERVICES) {
                    Err(io::Error::other("systemctl ended with exit status: 5"))
                } else {
                    Ok(())
                }
            },
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(
            lines,
            [
                "lamp-services.service could not be started (systemctl ended with exit status: 5); it starts at the next boot",
                "started lamp-mood.service, which lampos.target wants and was not running",
            ]
        );
    }
}
