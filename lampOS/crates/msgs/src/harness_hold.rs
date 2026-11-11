//! Whether Harness's own self-update is held on this lamp (#6300, (B)),
//! decided here once for every writer of the hold: lamp-tasks' hold file,
//! the environment it gives each Harness command and Harness's launcher at
//! its start (crates/tasks/src/hold.rs), and the launcher lamp-update
//! writes after an install or a move
//! (crates/update/src/harness_install/launcher.rs).
//!
//! The switch (`LAMP_HARNESS_HOLD_UPDATES=on`, lamp-tasks' own unit) asks
//! for the hold. A lamp whose updater follows the lamp-voice feed
//! ([`CANARY_FEED`]: a dev or beta lamp, which takes each lamp-voice
//! publish) is the canary: there the hold is lifted, so Harness's own
//! updater runs. Every other lamp keeps it exactly as before: one on the
//! stable feed (`lampos-stable`, every product lamp), a local `file:///`
//! feed, any other feed, and a lamp whose feed can't be read.
//!
//! The feed is the `--channel-url` of lamp-update's own `ExecStart`, which
//! no round switches (crates/update `round::OWN_UNIT`): the line its image
//! or its owner installed. The signed `--channel` is `lamp-voice` on every
//! feed (docs/updates.md), so it says nothing of which one a lamp follows.

/// The lamp-voice feed, exactly. A lamp's updater following it is a dev or
/// beta lamp: the canary for Harness's own releases.
pub const CANARY_FEED: &str =
    "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/";

/// The stable feed, which every product lamp follows. Named for the docs
/// and tests: any feed but [`CANARY_FEED`] keeps the hold.
pub const STABLE_FEED: &str =
    "https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/";

/// What a writer of the hold does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessHold {
    /// The switch is not on: nothing of the hold looked at or written, as
    /// before #6300.
    Off,
    /// The switch is on, the lamp not the canary: the hold, as before.
    Held,
    /// The switch is on, the lamp the canary: the hold taken off, so
    /// Harness's own updater runs (its hold file and launcher written
    /// without it).
    Lifted,
}

/// The `--channel-url` in `exec_start`, lamp-update's `ExecStart` as its
/// unit file has it or as `systemctl show --property=ExecStart --value`
/// gives it (the same words, in `argv[]=… ;`).
pub fn feed(exec_start: &str) -> Option<&str> {
    let mut words = exec_start.split_whitespace();
    words.find(|word| *word == "--channel-url")?;
    words.next()
}

/// The hold, from the switch's value (`on` exactly) and lamp-update's
/// `ExecStart` (`None` when it could not be read: held).
pub fn harness_hold(switch: Option<&str>, lamp_update_exec_start: Option<&str>) -> HarnessHold {
    if switch != Some("on") {
        return HarnessHold::Off;
    }
    match lamp_update_exec_start.and_then(feed) {
        Some(CANARY_FEED) => HarnessHold::Lifted,
        _ => HarnessHold::Held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(feed: &str) -> String {
        format!(
            "/opt/lampOS/update/current/lamp-update serve --channel-url {feed} --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on"
        )
    }

    #[test]
    fn stable_and_any_other_feed_keep_the_hold_and_only_lamp_voice_lifts_it() {
        let product = include_str!("../../../deploy/product/lamp-update.service")
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .unwrap();
        assert_eq!(feed(product), Some(STABLE_FEED));
        assert_eq!(harness_hold(Some("on"), Some(product)), HarnessHold::Held);
        for kept in [
            STABLE_FEED,
            "file:///var/tmp/lampos-channel/",
            "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-11308/",
            "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice",
            "https://example.test/lampos-lamp-voice/",
        ] {
            assert_eq!(
                harness_hold(Some("on"), Some(&line(kept))),
                HarnessHold::Held,
                "{kept}"
            );
        }
        assert_eq!(harness_hold(Some("on"), None), HarnessHold::Held);
        assert_eq!(harness_hold(Some("on"), Some("")), HarnessHold::Held);
        assert_eq!(
            harness_hold(Some("on"), Some("lamp-update serve --channel-url")),
            HarnessHold::Held
        );
        assert_eq!(
            harness_hold(Some("on"), Some(&line(CANARY_FEED))),
            HarnessHold::Lifted
        );
        // As `systemctl show --property=ExecStart --value` gives it.
        let shown = format!(
            "{{ path=/opt/lampOS/update/current/lamp-update ; argv[]={} ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}",
            line(CANARY_FEED)
        );
        assert_eq!(harness_hold(Some("on"), Some(&shown)), HarnessHold::Lifted);
    }

    #[test]
    fn with_the_switch_not_on_nothing_is_held_or_lifted() {
        for switch in [None, Some("off"), Some("ON"), Some("")] {
            for exec in [None, Some(line(STABLE_FEED)), Some(line(CANARY_FEED))] {
                assert_eq!(harness_hold(switch, exec.as_deref()), HarnessHold::Off);
            }
        }
    }
}
