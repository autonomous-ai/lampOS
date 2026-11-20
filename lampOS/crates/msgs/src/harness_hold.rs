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
//! stable feed (its CDN mirror or `lampos-stable`, every product lamp), a local `file:///`
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

/// The stable feed's CDN mirror, which the product unit follows (#6256 F2);
/// a lamp installed before it keeps the GitHub stable feed it was given.
/// Named for the docs and tests: any feed but [`CANARY_FEED`] keeps the hold.
pub const STABLE_FEED: &str = "https://cdn.autonomous.ai/lampos/stable/";

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

/// The line a held launcher carries before its `exec` line (#5957): `harness
/// update` refused, with one line on stderr and exit 1, before Harness runs.
/// The daemon's hold (`ADAPTER_UPDATE_DISABLE`) holds only its own
/// self-update; `harness update` from a shell (by hand, or an agent's
/// terminal tool, as the tasks user) installs the newest Harness whatever
/// it says, which put lamp-c0a4 above the pin on 11 Nov. lamp-update moves
/// Harness to the pin itself and never calls it. Harness takes its command
/// from its first argument (`const [, , cmd, ...rest] = process.argv`,
/// openharness `cli/src/cli.ts`), so only that is read. Written and taken
/// out with the hold by every writer of the launcher: lamp-tasks
/// (crates/tasks/src/hold.rs) and lamp-update
/// (crates/update/src/harness_install/launcher.rs).
pub const REFUSE_UPDATE: &str = "case \"${1-}\" in update) echo \"harness: this lamp keeps Harness at its pinned version and updates it itself; nothing was changed\" >&2; exit 1 ;; esac";

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

/// The variable that keeps Harness's home in `~/.harness` (#7906):
/// Harness 0.6.72 (openharness#4469) moves `~/.harness` into the XDG
/// folders at a daemon start unless a name in its `OVERRIDE_NAMES`
/// (`cli/src/homeMove/conditions.ts`) points inside it (`skipped-override`).
pub const DATA_DIR: &str = "ADAPTER_DATA_DIR";
/// Its value under Harness's home, Harness's own default
/// (`harnessPath('cli') + '/data'`, `cli/src/config/env.ts`).
pub const HARNESS_DATA: &str = ".harness/cli/data";

/// `ADAPTER_DATA_DIR=<data>` and its newline: the one line `harness-hold.env`
/// carries for Harness's data folder (#7906), written alike by lamp-tasks
/// (crates/tasks/src/hold.rs) and lamp-update after an install or a move
/// (crates/update/src/harness_install/launcher.rs).
pub fn data_line(data: &str) -> String {
    format!("{DATA_DIR}={data}\n")
}

/// `text`, an env file's words, with [`data_line`] of `data` last (#7906):
/// any earlier `ADAPTER_DATA_DIR=` line taken out, every other line kept
/// as it is, a last line with no newline given one.
pub fn with_data_line(text: &str, data: &str) -> String {
    let mut kept = String::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with(&format!("{DATA_DIR}=")) {
            continue;
        }
        kept.push_str(line);
        if !line.ends_with('\n') {
            kept.push('\n');
        }
    }
    kept.push_str(&data_line(data));
    kept
}

/// Which sign says Harness's home under `home` was moved to the XDG
/// folders after all (#7906): Harness's bridge record, its data root's
/// marker, or an entry of `~/.harness` left as a link. `None` when none is
/// there. Nothing is moved back.
pub fn home_moved(home: &std::path::Path) -> Option<&'static str> {
    if home.join(".local/state/harness/bridge.json").exists() {
        return Some(".local/state/harness/bridge.json");
    }
    if home.join(".local/share/harness/.harness-root").exists() {
        return Some(".local/share/harness/.harness-root");
    }
    let link = |entry: &str| {
        std::fs::symlink_metadata(home.join(".harness").join(entry))
            .is_ok_and(|meta| meta.file_type().is_symlink())
    };
    if link("runtime") {
        return Some(".harness/runtime is a link");
    }
    if link("cli") {
        return Some(".harness/cli is a link");
    }
    None
}

/// The one fixed line said when [`home_moved`] finds a sign.
pub fn home_moved_line(home: &std::path::Path) -> Option<String> {
    home_moved(home)
        .map(|sign| format!("harness: home moved to XDG ({sign}): lampOS paths need ~/.harness"))
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
            "https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/",
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

    /// #7906: the env file's data line put last, once, every other line kept.
    #[test]
    fn the_data_line_is_put_last_once_and_other_lines_are_kept() {
        let data = "/var/lib/lampos-tasks/.harness/cli/data";
        let line = format!("ADAPTER_DATA_DIR={data}\n");
        assert_eq!(data_line(data), line);
        assert_eq!(with_data_line("", data), line);
        let held = "ADAPTER_UPDATE_DISABLE=true\nHERMES_HOME=/h\n";
        assert_eq!(with_data_line(held, data), format!("{held}{line}"));
        assert_eq!(
            with_data_line("ADAPTER_DATA_DIR=/old\nSHELL=/bin/sh", data),
            format!("SHELL=/bin/sh\n{line}")
        );
        let done = with_data_line(held, data);
        assert_eq!(with_data_line(&done, data), done);
    }

    #[test]
    fn each_sign_of_a_moved_home_is_said_and_none_without() {
        let home = std::env::temp_dir().join(format!("lampos-home-moved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let at = home.as_path();
        std::fs::create_dir_all(at.join(".harness/cli/data")).unwrap();
        std::fs::create_dir_all(at.join(".harness/runtime")).unwrap();
        assert_eq!(home_moved(at), None);
        assert_eq!(home_moved_line(at), None);
        std::fs::remove_dir_all(at.join(".harness/cli")).unwrap();
        std::os::unix::fs::symlink(at.join(".local/share/harness/cli"), at.join(".harness/cli"))
            .unwrap();
        assert_eq!(home_moved(at), Some(".harness/cli is a link"));
        std::fs::remove_dir(at.join(".harness/runtime")).unwrap();
        std::os::unix::fs::symlink("/elsewhere", at.join(".harness/runtime")).unwrap();
        assert_eq!(home_moved(at), Some(".harness/runtime is a link"));
        std::fs::create_dir_all(at.join(".local/share/harness")).unwrap();
        std::fs::write(at.join(".local/share/harness/.harness-root"), b"").unwrap();
        assert_eq!(home_moved(at), Some(".local/share/harness/.harness-root"));
        std::fs::create_dir_all(at.join(".local/state/harness")).unwrap();
        std::fs::write(at.join(".local/state/harness/bridge.json"), b"{}").unwrap();
        assert_eq!(
            home_moved_line(at).unwrap(),
            "harness: home moved to XDG (.local/state/harness/bridge.json): lampOS paths need ~/.harness"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }
}
