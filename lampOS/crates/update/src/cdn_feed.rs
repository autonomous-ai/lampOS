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
use crate::{fetch::Channel, manifest, round::OWN_UNIT};
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
    fn the_switch_is_off_unless_set_and_the_product_file_does_not_set_it() {
        let folder = tempfile::tempdir().unwrap();
        assert!(!crate::switches::update_switch(folder.path(), SWITCH, None));
        let product = include_str!("../../../deploy/product/settings/update.env");
        assert!(!product.contains(SWITCH));
        assert!(crate::switches::update_switch(
            folder.path(),
            SWITCH,
            Some("on".as_ref())
        ));
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
