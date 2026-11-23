//! A lamp already in a home moves its updater from the GitHub stable feed
//! to the stable feed's CDN mirror (#7728, `LAMP_UPDATE_CDN_FEED=on`).
//!
//! No round rewrites lamp-update's own unit (`round::OWN_UNIT`), so a lamp
//! installed before #6589 keeps `--channel-url` [`GITHUB_STABLE`]. With the
//! switch on, after each `serve` round (an up-to-date one too, so the build
//! that carries this moves the lamp itself, as #7018 asks of a new unit)
//! lamp-update: reads its own `ExecStart` (the unit, then its drop-ins);
//! only when that feed and the one it runs with are exactly
//! [`GITHUB_STABLE`] does it go on (the lamp-voice canary, a `file:///`
//! folder, the CDN already and any other feed are left alone); reads the
//! CDN's envelope and checks its signature with the lamp's key, its channel,
//! and that its build is not older than GitHub's; writes the drop-in
//! [`DROP_IN`] with the same `ExecStart` naming [`CDN`]; reads it back; and
//! reloads systemd. Any failure leaves the drop-in as it was, so the lamp
//! keeps the feed it had.
//!
//! On the CDN feed, switch or not (a product unit follows it from its
//! install, [`falls_back`]), every round reads the CDN first;
//! once the CDN has failed for an hour ([`DOWN_SINCE`] in the records) a
//! round that fails on it is done again on [`GITHUB_STABLE`]
//! ([`round_on`]). Nothing sticks: the next round reads the CDN first.
use crate::{
    fetch::Channel,
    manifest,
    round::{Config, OWN_UNIT, Outcome},
};
use lamp_msgs::harness_hold::feed;
use std::{fs, path::Path};

/// The switch: `on` exactly in lamp-update's environment, or, not set
/// there, in the carried `/etc/lampOS/update.env` (`switches::update_switch`).
pub const SWITCH: &str = "LAMP_UPDATE_CDN_FEED";
/// The GitHub stable feed a lamp installed before #6589 follows: the only
/// feed moved.
pub const GITHUB_STABLE: &str =
    "https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/";
/// The product's feed, `deploy/product/lamp-update.service`'s.
pub const CDN: &str = lamp_msgs::harness_hold::STABLE_FEED;
/// The drop-in written beside lamp-update's unit. Removing it and
/// `systemctl daemon-reload` puts the GitHub feed back.
pub const DROP_IN: &str = "zz-cdn-feed.conf";

#[derive(Debug, PartialEq, Eq)]
pub enum Moved {
    /// Not on the GitHub stable feed: nothing done.
    NotGitHub,
    /// The drop-in written, read back and loaded: lamp-update starts again
    /// with [`CDN`].
    Moved,
    /// Kept on the GitHub feed, and why.
    Kept(String),
}

/// lamp-update's own unit and its `.conf` drop-ins under `units`, in
/// systemd's order.
pub fn own_unit_texts(units: &Path) -> Vec<String> {
    let mut texts: Vec<String> = fs::read_to_string(units.join(OWN_UNIT))
        .into_iter()
        .collect();
    let mut drop_ins: Vec<_> = fs::read_dir(units.join(format!("{OWN_UNIT}.d")))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "conf")
        })
        .collect();
    drop_ins.sort();
    texts.extend(
        drop_ins
            .iter()
            .filter_map(|path| fs::read_to_string(path).ok()),
    );
    texts
}

/// lamp-update's `ExecStart` as its unit and drop-ins leave it: the last
/// one, an empty `ExecStart=` clearing those before.
pub fn exec_start(units: &Path) -> Option<String> {
    let mut found = None;
    for text in own_unit_texts(units) {
        for line in text.lines() {
            if let Some(command) = line.strip_prefix("ExecStart=") {
                found = (!command.is_empty()).then(|| command.to_owned());
            }
        }
    }
    found
}

/// The build at `url`'s envelope, signed with `key` and of `channel`.
fn sequence(
    net: &mut dyn Channel,
    url: &str,
    key: &[u8; 32],
    channel: &str,
) -> Result<u64, String> {
    #[derive(serde::Deserialize)]
    struct Head {
        channel: String,
        sequence: u64,
    }
    let mut envelope = Vec::new();
    net.fetch(
        &format!("{url}{}", manifest::ENVELOPE),
        manifest::MOST_ENVELOPE_BYTES,
        &mut envelope,
    )
    .map_err(|error| format!("{url}: {error}"))?;
    let bytes = manifest::signed(&envelope, key).map_err(|why| format!("{url}: {why}"))?;
    let head: Head = serde_json::from_slice(&bytes).map_err(|error| format!("{url}: {error}"))?;
    if head.channel != channel {
        return Err(format!("{url}: channel {}, not {channel}", head.channel));
    }
    Ok(head.sequence)
}

/// Moves lamp-update under `units` from [`GITHUB_STABLE`] to [`CDN`]:
/// `following` is the feed it runs with, `reload` systemd's daemon-reload.
pub fn move_to_cdn(
    units: &Path,
    following: &str,
    key: &[u8; 32],
    channel: &str,
    net: &mut dyn Channel,
    reload: &mut dyn FnMut() -> Result<(), String>,
) -> Moved {
    let Some(line) = exec_start(units) else {
        return Moved::NotGitHub;
    };
    if following != GITHUB_STABLE || feed(&line) != Some(GITHUB_STABLE) {
        return Moved::NotGitHub;
    }
    let cdn = match sequence(net, CDN, key, channel) {
        Ok(cdn) => cdn,
        Err(why) => return Moved::Kept(format!("the CDN feed cannot be read: {why}")),
    };
    // GitHub out of reach (a lamp in China) is no reason to stay on it.
    if let Ok(github) = sequence(net, GITHUB_STABLE, key, channel)
        && cdn < github
    {
        return Moved::Kept(format!(
            "the CDN feed's build {cdn} is older than GitHub's {github}"
        ));
    }
    let moved: Vec<&str> = line
        .split(' ')
        .map(|word| if word == GITHUB_STABLE { CDN } else { word })
        .collect();
    let text = format!(
        "# lamp-update follows the stable feed's CDN mirror (#7728, {SWITCH}).\n\
         # Remove this file and run systemctl daemon-reload to go back to GitHub.\n\
         [Service]\nExecStart=\nExecStart={}\n",
        moved.join(" ")
    );
    let folder = units.join(format!("{OWN_UNIT}.d"));
    let path = folder.join(DROP_IN);
    let before = fs::read(&path).ok();
    let put_back = || match &before {
        Some(bytes) => crate::records::write_whole(&path, bytes, 0o644),
        None => fs::remove_file(&path),
    };
    if let Err(error) = fs::create_dir_all(&folder)
        .and_then(|()| crate::records::write_whole(&path, text.as_bytes(), 0o644))
    {
        let _ = put_back();
        return Moved::Kept(format!("{} cannot be written: {error}", path.display()));
    }
    let read_back = fs::read(&path).ok().as_deref() == Some(text.as_bytes())
        && exec_start(units).as_deref().and_then(feed) == Some(CDN);
    if !read_back {
        let _ = put_back();
        return Moved::Kept(format!(
            "{} read back does not leave the CDN feed: put back",
            path.display()
        ));
    }
    if let Err(why) = reload() {
        let _ = put_back();
        let _ = reload();
        return Moved::Kept(format!("systemd cannot reload ({why}): put back"));
    }
    Moved::Moved
}

/// `args` with the word after `--channel-url` made [`CDN`].
pub fn args_on_cdn(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut next_is_feed = false;
    args.map(|arg| {
        let out = if next_is_feed { CDN.into() } else { arg };
        next_is_feed = out == "--channel-url";
        out
    })
    .collect()
}

/// How long the CDN must have failed, round after round, before a round
/// reads [`GITHUB_STABLE`] after it.
pub const FALLBACK_AFTER_MS: u64 = 60 * 60 * 1000;
/// In the records folder: the unix ms of the first CDN round that failed
/// since one last read it. Kept there so lamp-update started again (its own
/// switch, a changed `update.env`, a restart) keeps counting.
pub const DOWN_SINCE: &str = "cdn-down-since";

/// When the CDN started failing, as `records` keeps it; a file that is not
/// a number, too long or ahead of `now` is none.
fn down_since(records: &Path, now: u64) -> Option<u64> {
    use std::io::Read;
    let mut text = String::new();
    fs::File::open(records.join(DOWN_SINCE))
        .ok()?
        .take(33)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() > 32 {
        return None;
    }
    text.trim().parse().ok().filter(|since| *since <= now)
}

/// Whether the CDN has failed for [`FALLBACK_AFTER_MS`] by `now`.
pub fn due(records: &Path, now: u64) -> bool {
    down_since(records, now).is_some_and(|since| now - since >= FALLBACK_AFTER_MS)
}

/// `config` reading [`GITHUB_STABLE`] instead: the same key, channel and
/// switches, so the same signature, channel and SHA-256 checks.
pub fn on_github(config: &Config) -> Config {
    Config {
        channel_url: GITHUB_STABLE.into(),
        ..config.clone()
    }
}

/// Whether `config`'s rounds, and its own confirm, fall back to
/// [`GITHUB_STABLE`] after an hour of CDN failures: a `serve` (`serve`)
/// that acts, on [`CDN`]: every such lamp, a product unit's too, with
/// [`SWITCH`] or without it (the lead, #7728).
pub fn falls_back(config: &Config, serve: bool) -> bool {
    serve && config.apply && config.channel_url == CDN
}

/// A round on `config`'s feed. On [`CDN`], a round that cannot read it
/// (out of reach, or no build published there) starts or keeps the clock
/// in `records`; once that clock reaches [`FALLBACK_AFTER_MS`] the same
/// round is done again on [`GITHUB_STABLE`]. Each round reads the CDN first,
/// and one that reads it stops the clock. Any other feed: `round` alone.
pub fn round_on(
    config: &Config,
    records: &Path,
    now: u64,
    round: &mut dyn FnMut(&Config) -> Outcome,
    say: &mut dyn FnMut(&str),
) -> Outcome {
    let outcome = round(config);
    if config.channel_url != CDN {
        return outcome;
    }
    let path = records.join(DOWN_SINCE);
    if !matches!(outcome, Outcome::Unreachable(_) | Outcome::NothingPublished) {
        if fs::remove_file(&path).is_ok() {
            say("the CDN feed is read again: GitHub is not read");
        }
        return outcome;
    }
    let since = down_since(records, now).unwrap_or_else(|| {
        if let Err(error) = crate::records::write_whole(&path, now.to_string().as_bytes(), 0o644) {
            say(&format!("{} cannot be written: {error}", path.display()));
        }
        now
    });
    if now - since < FALLBACK_AFTER_MS {
        return outcome;
    }
    say(&format!(
        "the CDN feed has failed for {} min ({outcome}): this round reads {GITHUB_STABLE}, the next one the CDN first",
        (now - since) / 60_000
    ));
    let github = round(&on_github(config));
    say(&format!("on {GITHUB_STABLE}: {github}"));
    github
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::Fetch;
    use ring::{rand::SystemRandom, signature::Ed25519KeyPair, signature::KeyPair};
    use std::collections::HashMap;

    const CANARY: &str = lamp_msgs::harness_hold::CANARY_FEED;

    #[derive(Default)]
    struct Feeds(HashMap<String, Vec<u8>>);

    impl Channel for Feeds {
        fn fetch(
            &mut self,
            url: &str,
            _most: u64,
            out: &mut dyn std::io::Write,
        ) -> Result<u64, Fetch> {
            let bytes = self.0.get(url).ok_or(Fetch::Missing)?;
            out.write_all(bytes).unwrap();
            Ok(bytes.len() as u64)
        }
    }

    struct Lamp {
        _folder: tempfile::TempDir,
        units: std::path::PathBuf,
        key: Ed25519KeyPair,
        feeds: Feeds,
        reloads: usize,
    }

    impl Lamp {
        /// A lamp whose unit follows `url`, both feeds at build 14922.
        fn on(url: &str) -> Self {
            let folder = tempfile::tempdir().unwrap();
            let units = folder.path().join("etc/systemd/system");
            fs::create_dir_all(&units).unwrap();
            let unit =
                include_str!("../../../deploy/product/lamp-update.service").replace(CDN, url);
            fs::write(units.join(OWN_UNIT), unit).unwrap();
            let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            let mut lamp = Self {
                _folder: folder,
                units,
                key: Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap(),
                feeds: Feeds::default(),
                reloads: 0,
            };
            lamp.publish(CDN, 14922);
            lamp.publish(GITHUB_STABLE, 14922);
            lamp
        }

        fn publish(&mut self, url: &str, sequence: u64) {
            let manifest = format!(r#"{{"channel":"lamp-voice","sequence":{sequence}}}"#);
            let envelope = manifest::seal_unchecked(manifest.as_bytes(), &self.key).unwrap();
            self.feeds
                .0
                .insert(format!("{url}{}", manifest::ENVELOPE), envelope);
        }

        fn run(&mut self, following: &str) -> Moved {
            let key: [u8; 32] = self.key.public_key().as_ref().try_into().unwrap();
            let reloads = &mut self.reloads;
            move_to_cdn(
                &self.units,
                following,
                &key,
                "lamp-voice",
                &mut self.feeds,
                &mut || {
                    *reloads += 1;
                    Ok(())
                },
            )
        }

        fn drop_in(&self) -> std::path::PathBuf {
            self.units.join(format!("{OWN_UNIT}.d")).join(DROP_IN)
        }
    }

    #[test]
    fn the_github_stable_feed_moves_to_the_cdn_and_keeps_the_rest_of_its_line() {
        let mut lamp = Lamp::on(GITHUB_STABLE);
        let before = exec_start(&lamp.units).unwrap();
        assert_eq!(lamp.run(GITHUB_STABLE), Moved::Moved);
        assert_eq!(lamp.reloads, 1);
        let after = exec_start(&lamp.units).unwrap();
        assert_eq!(feed(&after), Some(CDN));
        assert_eq!(after, before.replace(GITHUB_STABLE, CDN));
        // The unit itself is the one installed; a second round has nothing to move.
        assert!(
            fs::read_to_string(lamp.units.join(OWN_UNIT))
                .unwrap()
                .contains(GITHUB_STABLE)
        );
        assert_eq!(lamp.run(CDN), Moved::NotGitHub);
        assert_eq!(lamp.reloads, 1);
    }

    #[test]
    fn a_cdn_that_cannot_be_read_or_trusted_keeps_github() {
        // Missing.
        let mut lamp = Lamp::on(GITHUB_STABLE);
        lamp.feeds.0.remove(&format!("{CDN}{}", manifest::ENVELOPE));
        assert!(
            matches!(lamp.run(GITHUB_STABLE), Moved::Kept(why) if why.contains("cannot be read"))
        );
        // Signed with another key.
        let mut other = Lamp::on(GITHUB_STABLE);
        let stranger = Lamp::on(GITHUB_STABLE);
        let key = format!("{CDN}{}", manifest::ENVELOPE);
        other
            .feeds
            .0
            .insert(key.clone(), stranger.feeds.0[&key].clone());
        assert!(matches!(other.run(GITHUB_STABLE), Moved::Kept(_)));
        // Behind GitHub.
        let mut behind = Lamp::on(GITHUB_STABLE);
        behind.publish(GITHUB_STABLE, 14923);
        assert!(matches!(behind.run(GITHUB_STABLE), Moved::Kept(why) if why.contains("older")));
        for lamp in [&lamp, &other, &behind] {
            assert!(!lamp.drop_in().exists());
            assert_eq!(feed(&exec_start(&lamp.units).unwrap()), Some(GITHUB_STABLE));
            assert_eq!(lamp.reloads, 0);
        }
        // GitHub out of reach does not hold the move.
        let mut china = Lamp::on(GITHUB_STABLE);
        china
            .feeds
            .0
            .remove(&format!("{GITHUB_STABLE}{}", manifest::ENVELOPE));
        assert_eq!(china.run(GITHUB_STABLE), Moved::Moved);
    }

    #[test]
    fn a_read_back_that_does_not_name_the_cdn_puts_the_drop_in_back() {
        let mut lamp = Lamp::on(GITHUB_STABLE);
        // A later drop-in of the lamp's own sets the GitHub line again.
        let folder = lamp.units.join(format!("{OWN_UNIT}.d"));
        fs::create_dir_all(&folder).unwrap();
        let line = exec_start(&lamp.units).unwrap();
        fs::write(
            folder.join("zzz-own.conf"),
            format!("[Service]\nExecStart=\nExecStart={line}\n"),
        )
        .unwrap();
        assert!(matches!(lamp.run(GITHUB_STABLE), Moved::Kept(why) if why.contains("read back")));
        assert!(!lamp.drop_in().exists());
        assert_eq!(lamp.reloads, 0);
    }

    #[test]
    fn a_failed_reload_puts_the_drop_in_back() {
        let mut lamp = Lamp::on(GITHUB_STABLE);
        let key: [u8; 32] = lamp.key.public_key().as_ref().try_into().unwrap();
        let mut tries = 0;
        let moved = move_to_cdn(
            &lamp.units,
            GITHUB_STABLE,
            &key,
            "lamp-voice",
            &mut lamp.feeds,
            &mut || {
                tries += 1;
                if tries == 1 {
                    Err("no bus".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(moved, Moved::Kept(why) if why.contains("reload")));
        assert_eq!(tries, 2, "reloaded again once put back");
        assert!(!lamp.drop_in().exists());
    }

    #[test]
    fn the_canary_a_file_feed_the_cdn_and_others_are_left_alone() {
        for url in [
            CANARY,
            "file:///var/tmp/lampos-channel/",
            CDN,
            "https://example.test/lampos-stable/",
        ] {
            let mut lamp = Lamp::on(url);
            assert_eq!(lamp.run(url), Moved::NotGitHub, "{url}");
            assert!(!lamp.drop_in().exists(), "{url}");
        }
        // A unit on GitHub stable while this program runs with another feed
        // (a `once` by hand) is left alone too.
        let mut lamp = Lamp::on(GITHUB_STABLE);
        assert_eq!(lamp.run(CANARY), Moved::NotGitHub);
        assert!(!lamp.drop_in().exists());
    }

    #[test]
    fn the_switch_is_off_unless_set_and_the_product_file_sets_it_on() {
        let folder = tempfile::tempdir().unwrap();
        assert!(!crate::switches::update_switch(folder.path(), SWITCH, None));
        let product = include_str!("../../../deploy/product/settings/update.env");
        assert!(crate::models::file_says_on(product.as_bytes(), SWITCH));
        // A lamp in a home takes it from the carried file, its unit not setting it.
        let at = folder
            .path()
            .join(manifest::UPDATE_ENV.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(&at, product).unwrap();
        assert!(crate::switches::update_switch(folder.path(), SWITCH, None));
        // Its environment set off still holds it off.
        assert!(!crate::switches::update_switch(
            folder.path(),
            SWITCH,
            Some("off".as_ref())
        ));
        fs::remove_file(&at).unwrap();
        assert!(crate::switches::update_switch(
            folder.path(),
            SWITCH,
            Some("on".as_ref())
        ));
    }

    fn following(url: &str) -> crate::round::Config {
        crate::round::Config {
            root: "/".into(),
            units: "/etc/systemd/system/".into(),
            records: "/var/lib/lampos-update/".into(),
            channel: "lamp-voice".into(),
            channel_url: url.into(),
            key: [7; 32],
            lamp: "lamp-06c1".into(),
            apply: true,
            quiet_wait: std::time::Duration::ZERO,
            watch: std::time::Duration::ZERO,
            keep_free: 0,
            self_switch: true,
            running: None,
            updater: manifest::UPDATER,
            models: false,
            models_file: false,
            settings: false,
            product_settings: false,
            image: false,
            first_update: false,
            writer_from: None,
        }
    }

    const MINUTE: u64 = 60 * 1000;

    /// One round at `now`, the CDN answering with `cdn` and GitHub with
    /// `Switched(14923)`: the feeds read, in order.
    fn round_at(
        records: &Path,
        config: &crate::round::Config,
        now: u64,
        cdn: Outcome,
    ) -> (Outcome, Vec<String>) {
        let mut read = Vec::new();
        let outcome = round_on(
            config,
            records,
            now,
            &mut |config: &crate::round::Config| {
                read.push(config.channel_url.clone());
                if config.channel_url == CDN {
                    cdn.clone()
                } else {
                    // The same key, channel and switches as the CDN round's.
                    assert_eq!(config.key, [7; 32]);
                    assert_eq!(config.channel, "lamp-voice");
                    assert!(config.apply && config.self_switch);
                    Outcome::Switched(14923)
                }
            },
            &mut |_| {},
        );
        (outcome, read)
    }

    fn down() -> Outcome {
        Outcome::Unreachable("no answer".into())
    }

    #[test]
    fn the_cdn_stays_first_and_github_is_read_only_after_an_hour_of_cdn_failures() {
        let folder = tempfile::tempdir().unwrap();
        let records = folder.path();
        let config = following(CDN);
        let t0 = 1_800_000_000_000;
        // The CDN up: the CDN only.
        let (outcome, read) = round_at(records, &config, t0, Outcome::UpToDate(14922));
        assert_eq!(
            (outcome, read),
            (Outcome::UpToDate(14922), vec![CDN.to_owned()])
        );
        // Down under the hour: no GitHub, the CDN again next round.
        for minutes in [0, 15, 59] {
            let (outcome, read) = round_at(records, &config, t0 + minutes * MINUTE, down());
            assert_eq!((outcome, read), (down(), vec![CDN.to_owned()]), "{minutes}");
        }
        // Down for an hour: this round reads GitHub after the CDN.
        let (outcome, read) = round_at(records, &config, t0 + 60 * MINUTE, down());
        assert_eq!(outcome, Outcome::Switched(14923));
        assert_eq!(read, [CDN, GITHUB_STABLE]);
        // Never sticky: the next round tries the CDN first again.
        let (_, read) = round_at(records, &config, t0 + 75 * MINUTE, down());
        assert_eq!(read, [CDN, GITHUB_STABLE]);
        let (_, read) = round_at(records, &config, t0 + 90 * MINUTE, Outcome::UpToDate(14923));
        assert_eq!(read, [CDN]);
        // A CDN round that read it resets the clock: down again, an hour again.
        let (_, read) = round_at(records, &config, t0 + 91 * MINUTE, down());
        assert_eq!(read, [CDN]);
        let (_, read) = round_at(records, &config, t0 + 150 * MINUTE, down());
        assert_eq!(read, [CDN]);
        let (_, read) = round_at(records, &config, t0 + 151 * MINUTE, down());
        assert_eq!(read, [CDN, GITHUB_STABLE]);
        // No build published on the CDN is the CDN failing too.
        let (_, read) = round_at(
            records,
            &config,
            t0 + 152 * MINUTE,
            Outcome::NothingPublished,
        );
        assert_eq!(read, [CDN, GITHUB_STABLE]);
    }

    #[test]
    fn the_failure_clock_is_kept_in_the_records_across_a_restart_and_a_bad_file_starts_it_again() {
        let folder = tempfile::tempdir().unwrap();
        let records = folder.path();
        let config = following(CDN);
        let t0 = 1_800_000_000_000;
        round_at(records, &config, t0, down());
        assert_eq!(
            fs::read_to_string(records.join(DOWN_SINCE)).unwrap().trim(),
            t0.to_string()
        );
        // Each call reads the file afresh, as lamp-update started again does.
        assert!(!due(records, t0 + 59 * MINUTE));
        assert!(due(records, t0 + 60 * MINUTE));
        let (_, read) = round_at(records, &config, t0 + 60 * MINUTE, down());
        assert_eq!(read, [CDN, GITHUB_STABLE]);
        // Garbage, a huge file, or a time ahead of the clock: not due, and
        // the next failure starts the clock again from now.
        for bad in [
            b"soon".to_vec(),
            vec![b'1'; 1 << 20],
            (t0 + 600 * MINUTE).to_string().into_bytes(),
        ] {
            fs::write(records.join(DOWN_SINCE), &bad).unwrap();
            assert!(!due(records, t0 + 120 * MINUTE));
            let (_, read) = round_at(records, &config, t0 + 120 * MINUTE, down());
            assert_eq!(read, [CDN]);
            assert_eq!(
                fs::read_to_string(records.join(DOWN_SINCE)).unwrap().trim(),
                (t0 + 120 * MINUTE).to_string()
            );
        }
        // A CDN round that read it removes the file.
        round_at(
            records,
            &config,
            t0 + 121 * MINUTE,
            Outcome::UpToDate(14923),
        );
        assert!(!records.join(DOWN_SINCE).exists());
    }

    #[test]
    fn a_product_unit_lamp_on_the_cdn_falls_back_with_the_switch_unset() {
        // The product unit's own feed, and no switch in its environment or
        // under its root.
        assert!(
            include_str!("../../../deploy/product/lamp-update.service")
                .contains(&format!("serve --channel-url {CDN} "))
        );
        let folder = tempfile::tempdir().unwrap();
        let records = folder.path();
        let config = Config {
            root: folder.path().into(),
            ..following(CDN)
        };
        assert!(!crate::switches::update_switch(
            &config.root,
            SWITCH,
            std::env::var_os(SWITCH).as_deref()
        ));
        // The round falls back (main.rs: `round_on` when this holds), and so
        // does its own confirm, done again on GitHub once `due`.
        assert!(falls_back(&config, true));
        let t0 = 1_800_000_000_000;
        for minutes in [0, 59] {
            let (_, read) = round_at(records, &config, t0 + minutes * MINUTE, down());
            assert_eq!(read, [CDN], "{minutes}");
        }
        assert!(!due(records, t0 + 59 * MINUTE));
        assert!(due(records, t0 + 60 * MINUTE));
        let (outcome, read) = round_at(records, &config, t0 + 60 * MINUTE, down());
        assert_eq!(outcome, Outcome::Switched(14923));
        assert_eq!(read, [CDN, GITHUB_STABLE]);
        // Never sticky: the CDN read again, the CDN only.
        let (_, read) = round_at(records, &config, t0 + 61 * MINUTE, Outcome::UpToDate(14923));
        assert_eq!(read, [CDN]);
        assert!(!due(records, t0 + 61 * MINUTE));
        // Only a serve that acts, only on the CDN.
        assert!(!falls_back(&config, false));
        let dry = Config {
            apply: false,
            ..config.clone()
        };
        assert!(!falls_back(&dry, true));
        for url in [GITHUB_STABLE, CANARY] {
            let other = Config {
                channel_url: url.into(),
                ..config.clone()
            };
            assert!(!falls_back(&other, true), "{url}");
        }
    }

    #[test]
    fn a_lamp_on_another_feed_never_falls_back_and_never_starts_the_clock() {
        let folder = tempfile::tempdir().unwrap();
        let records = folder.path();
        let t0 = 1_800_000_000_000;
        for url in [GITHUB_STABLE, CANARY, "file:///var/tmp/lampos-channel/"] {
            let config = following(url);
            for minutes in [0, 61, 200] {
                let mut read = Vec::new();
                round_on(
                    &config,
                    records,
                    t0 + minutes * MINUTE,
                    &mut |config: &crate::round::Config| {
                        read.push(config.channel_url.clone());
                        down()
                    },
                    &mut |_| {},
                );
                assert_eq!(read, [url], "{url}");
            }
            assert!(!records.join(DOWN_SINCE).exists(), "{url}");
        }
    }

    #[test]
    fn on_github_changes_only_the_feed() {
        let config = following(CDN);
        let github = on_github(&config);
        assert_eq!(github.channel_url, GITHUB_STABLE);
        assert_eq!(
            format!(
                "{:?}",
                Config {
                    channel_url: CDN.into(),
                    ..github
                }
            ),
            format!("{config:?}")
        );
    }

    #[test]
    fn only_the_word_after_channel_url_becomes_the_cdn() {
        let args = [
            "serve",
            "--channel-url",
            GITHUB_STABLE,
            "--channel",
            "lamp-voice",
        ]
        .map(std::ffi::OsString::from);
        let moved = args_on_cdn(args.into_iter());
        assert_eq!(
            moved,
            ["serve", "--channel-url", CDN, "--channel", "lamp-voice"]
        );
    }
}
