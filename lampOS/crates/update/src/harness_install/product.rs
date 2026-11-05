//! The pinned Harness a build names for a lamp installed as the product
//! (#5229; docs/install.md, "Harness before the sign-in", "The build's own
//! pin").
//!
//! A round never rewrites lamp-update's own unit (`round::OWN_UNIT`), so a
//! pin or a switch written into `deploy/product/lamp-update.service` would
//! reach only a lamp installed after it, never one already in a home. What
//! every build does reach a home lamp with is its own lamp-update, named by
//! its SHA-256 in the signed manifest and moved to by `LAMP_UPDATE_SELF=on`:
//! so the pinned Harness manifest itself is compiled in here
//! ([`MANIFEST`], a copy of Harness's CDN manifest committed at
//! `deploy/harness/`), and the build's Ed25519 signature covers it, as the
//! PM's decision (a) on #3693 asks. Nothing is fetched for it and nothing
//! is hosted: the one-shot hands it to Harness's installer from a listener
//! of its own on 127.0.0.1 ([`super::install_now`]). The installer checks
//! each file it names by its sha256. Harness's installer is compiled in
//! too ([`INSTALLER`]), and the four more manifests it reads ([`MANIFESTS`]:
//! the Node runtime, tmux, grid and hn), served from the same listener: a
//! pinned install fetches no script and no manifest from a server. A new
//! Harness version is a PR that copies these files again and updates
//! their constants.
//!
//! Which lamp is the product's: one whose lamp-update unit is a regular
//! file starting `/opt/lampOS/update/current/lamp-update`, the link naming
//! a build ([`product_lamp`]), the test a round makes before it writes the
//! product's settings or adds its units. A builder's lamp on
//! `deploy/units/` starts lamp-update from a build's own folder and is not.
//!
//! What the environment of lamp-update's unit sets always wins, so a lamp's
//! drop-in can turn the install off, or name another pin to fetch
//! ([`super::MANIFEST_URL`], [`super::MANIFEST_SHA256`]). With the switch
//! unset, a product lamp takes [`PRODUCT_DEFAULT`]: on since #5197's
//! switch-on, so a home lamp gets the pinned Harness by itself once this
//! build's lamp-update runs there, and the app signs it in. A product lamp
//! never installs an unpinned Harness.
use super::{Pin, pin_from};
use crate::{own, unit};
use std::{fs, path::Path};

/// Whether a lamp installed as the product installs Harness with
/// `LAMP_HARNESS_AT_ONBOARDING` unset. On (#5197): the PM's call on 5 Nov,
/// once the app's sign-in screen is in a TestFlight build; off only by the
/// environment (`LAMP_HARNESS_AT_ONBOARDING=off` in a drop-in).
pub const PRODUCT_DEFAULT: bool = true;

/// The Harness manifest this build pins for a product lamp: a byte-for-byte
/// copy of Harness's CDN manifest ([`ORIGIN`]) when it named [`VERSION`].
pub const MANIFEST: &[u8] = include_bytes!("../../../../deploy/harness/cli-0.5.39.metadata.json");
/// [`MANIFEST`]'s SHA-256, checked by a test and again before each install.
pub const MANIFEST_SHA256: &str =
    "3ce9710a498be4cd428c4d08ea9cde58b67ee68dff85864b156ffdfb2e48df5b";
/// The Harness version [`MANIFEST`] names (its `cli.version`).
pub const VERSION: &str = "0.5.39";
/// Where [`MANIFEST`] was copied from: said, never fetched.
pub const ORIGIN: &str =
    "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/cli/metadata.json";

/// Harness's installer, a byte-for-byte copy of [`INSTALLER_ORIGIN`] as it
/// was when [`MANIFEST`] was copied. Every pinned install runs this copy,
/// from a file, never the one at its URL (#5229).
pub const INSTALLER: &[u8] = include_bytes!("../../../../deploy/harness/cli-install-98f9d886.sh");
/// [`INSTALLER`]'s SHA-256, checked by a test and again before each run.
pub const INSTALLER_SHA256: &str =
    "98f9d886b386c3f2f0a452b37c6fdafa3dd27538b4894935080599b221943a76";
/// Where [`INSTALLER`] was copied from: said, never fetched.
pub const INSTALLER_ORIGIN: &str = crate::install::HARNESS_INSTALLER;

/// One more manifest [`INSTALLER`] reads besides the CLI's: copied, served on
/// 127.0.0.1 at `path` and handed to the installer in `env`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The installer's variable for its URL.
    pub env: &'static str,
    /// Where lamp-update's listener serves it.
    pub path: &'static str,
    pub bytes: &'static [u8],
    /// Its SHA-256, checked by a test and again before each install.
    pub sha256: &'static str,
    /// Where it was copied from: said, never fetched.
    pub origin: &'static str,
}

/// The Node runtime's, tmux's, grid's and hn's manifests, as Harness's CDN
/// had them when [`INSTALLER`] was copied. Each names its files by their
/// sha256, which the installer checks.
pub const MANIFESTS: [Manifest; 4] = [
    Manifest {
        env: "HARNESS_RUNTIME_METADATA_URL",
        path: "/runtime/metadata.json",
        bytes: include_bytes!("../../../../deploy/harness/runtime-node-v22.23.2.metadata.json"),
        sha256: "7e229eb66d8af4001672336373e66909e1d2428f560facb0359d6a203b293ffc",
        origin: "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/metadata.json",
    },
    Manifest {
        env: "HARNESS_TMUX_METADATA_URL",
        path: "/runtime/tmux/metadata.json",
        bytes: include_bytes!("../../../../deploy/harness/runtime-tmux-3.7c.metadata.json"),
        sha256: "f618bb8b9faac7e906b5223c1900797208d99885c1faa7de62c108b005c1f470",
        origin: "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/tmux/metadata.json",
    },
    Manifest {
        env: "HARNESS_GRID_METADATA_URL",
        path: "/runtime/grid/metadata.json",
        bytes: include_bytes!("../../../../deploy/harness/runtime-grid-0.3.63.metadata.json"),
        sha256: "5dacba2bde0f19c34bcd2a174b830d80905eef50f94059adf5091b8f01e1ae5e",
        origin: "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/grid/metadata.json",
    },
    Manifest {
        env: "HARNESS_TUI_MANIFEST_URL",
        path: "/tui/metadata.json",
        bytes: include_bytes!("../../../../deploy/harness/tui-0.1.46.metadata.json"),
        sha256: "0b1ace516bf03198c9fca238946c918ff1bd9cd297cc6c7315304d39e6e3ecd5",
        origin: "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/tui/metadata.json",
    },
];

/// This build's own pin: [`MANIFEST`], compiled in.
pub fn built_in() -> Pin {
    Pin {
        url: ORIGIN.to_owned(),
        sha256: MANIFEST_SHA256.to_owned(),
        built_in: true,
    }
}

/// Where the switch and the pin came from, as `serve` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// lamp-update's environment set the switch.
    Environment,
    /// A product lamp, the switch unset: the build's own default.
    Build,
    /// Neither: not a product lamp, the switch unset.
    Unset,
}

/// What `serve` does about Harness: on or not, the pin, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub on: bool,
    pub pin: Result<Option<Pin>, String>,
    pub source: Source,
}

/// Whether the lamp at `root` runs lamp-update from the product's unit:
/// its unit at `units`/`lamp-update.service` a regular file whose
/// `ExecStart=` is lamp-update's `current` link, that link naming a build.
pub fn product_lamp(root: &Path, units: &Path) -> bool {
    let path = units.join(crate::round::OWN_UNIT);
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return false;
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|text| unit::linked_program(&text, root))
        .is_some_and(|program| program.builds == own::FOLDER && program.binary == own::BINARY)
}

/// The switch and the pin from lamp-update's environment (`switch`, `url`,
/// `sha256`), whether the lamp is the product's, and the build's own
/// `default` ([`PRODUCT_DEFAULT`]):
///
/// - the switch set: on only when `on` exactly; unset: `default` on a
///   product lamp, else off;
/// - either of `url` and `sha256` set: the pin they name ([`pin_from`]);
/// - neither, on a product lamp: this build's own ([`built_in`]), so a
///   home lamp never installs an unpinned Harness;
/// - neither, elsewhere: no pin, Harness's own manifest (#5197).
pub fn wanted(
    switch: Option<&str>,
    url: Option<&str>,
    sha256: Option<&str>,
    product: bool,
    default: bool,
) -> Wanted {
    let (on, source) = match switch {
        Some(value) => (super::on(Some(value)), Source::Environment),
        None if product => (default, Source::Build),
        None => (false, Source::Unset),
    };
    let pin = if url.is_none() && sha256.is_none() && product {
        Ok(Some(built_in()))
    } else {
        pin_from(url, sha256)
    };
    Wanted { on, pin, source }
}

/// The `cli.version` a Harness manifest names, when it is `a.b.c`.
pub fn version_of(manifest: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(manifest).ok()?;
    let version = value.get("cli")?.get("version")?.as_str()?;
    let parts: Vec<&str> = version.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
    .then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const SHA: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";
    const URL: &str = "https://example.invalid/harness/manifest-0.4.27.json";
    const BUILD: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn fetched(url: &str, sha256: &str) -> Pin {
        Pin {
            url: url.to_owned(),
            sha256: sha256.to_owned(),
            built_in: false,
        }
    }

    #[test]
    fn the_built_in_manifest_is_the_one_its_constants_name() {
        assert_eq!(crate::manifest::sha256_hex(MANIFEST), MANIFEST_SHA256);
        assert_eq!(version_of(MANIFEST).as_deref(), Some(VERSION));
        const { assert!(MANIFEST.len() < 4096) };
        const { assert!(PRODUCT_DEFAULT) };
        let text = std::str::from_utf8(MANIFEST).unwrap();
        assert!(
            text.contains(&format!(
                "https://cdn.autonomous.ai/harness/cli/{VERSION}/cli.js"
            )),
            "{text}"
        );
    }

    /// The installer and the four manifests it reads are held to their
    /// constants too, and the installer reads each from the variable it is
    /// handed in, with the URL it was copied from as its default.
    #[test]
    fn the_built_in_installer_and_manifests_are_the_ones_their_constants_name() {
        assert_eq!(crate::manifest::sha256_hex(INSTALLER), INSTALLER_SHA256);
        let installer = std::str::from_utf8(INSTALLER).unwrap();
        assert!(installer.starts_with("#!"), "a shell script");
        assert!(
            installer.contains(&format!("${{HARNESS_METADATA_URL:-{ORIGIN}}}")),
            "the CLI manifest's variable"
        );
        for manifest in MANIFESTS {
            assert_eq!(
                crate::manifest::sha256_hex(manifest.bytes),
                manifest.sha256,
                "{}",
                manifest.origin
            );
            let value: serde_json::Value = serde_json::from_slice(manifest.bytes).unwrap();
            assert!(value.is_object(), "{}", manifest.origin);
            assert!(
                installer.contains(&format!("${{{}:-{}}}", manifest.env, manifest.origin)),
                "{} is read from {}",
                manifest.origin,
                manifest.env
            );
            assert!(manifest.path.starts_with('/') && manifest.path != super::super::serve::PATH);
        }
        // The OpenCode step runs only with a person there.
        assert!(installer.contains("[ \"${HARNESS_INSTALL_ATTENDED:-}\" = 1 ]"));
    }

    #[test]
    fn a_product_lamp_with_the_switch_unset_takes_the_builds_default_and_pin() {
        let off = wanted(None, None, None, true, false);
        assert_eq!((off.on, off.source), (false, Source::Build));
        let on = wanted(None, None, None, true, true);
        assert!(on.on);
        assert_eq!(on.pin, Ok(Some(built_in())));
    }

    #[test]
    fn the_environment_wins_over_the_builds_default() {
        let off = wanted(Some("off"), None, None, true, true);
        assert_eq!((off.on, off.source), (false, Source::Environment));
        let on = wanted(Some("on"), None, None, true, false);
        assert_eq!((on.on, on.source), (true, Source::Environment));
        assert_eq!(on.pin, Ok(Some(built_in())));
        // Anything but `on` exactly is off.
        assert!(!wanted(Some("ON"), None, None, true, true).on);
    }

    /// This build's own default (#5197's switch-on): a product lamp with
    /// the switch unset installs the built-in pin, and a drop-in's
    /// `LAMP_HARNESS_AT_ONBOARDING=off` (or any value but `on`) still turns
    /// it off; a builder's lamp is untouched by it.
    #[test]
    fn this_builds_default_is_on_for_a_product_lamp_and_the_environment_still_turns_it_off() {
        let unset = wanted(None, None, None, true, PRODUCT_DEFAULT);
        assert_eq!((unset.on, unset.source), (true, Source::Build));
        assert_eq!(unset.pin, Ok(Some(built_in())));
        for value in ["off", "", "OFF", "no"] {
            let set = wanted(Some(value), None, None, true, PRODUCT_DEFAULT);
            assert_eq!(
                (set.on, set.source),
                (false, Source::Environment),
                "{value:?}"
            );
        }
        let builder = wanted(None, None, None, false, PRODUCT_DEFAULT);
        assert_eq!((builder.on, builder.source), (false, Source::Unset));
    }

    #[test]
    fn a_pin_in_the_environment_wins_over_the_builds_own() {
        let said = wanted(Some("on"), Some(URL), Some(SHA), true, false);
        assert_eq!(said.pin, Ok(Some(fetched(URL, SHA))));
        // One alone is refused, never filled in from the build's.
        assert!(
            wanted(Some("on"), Some(URL), None, true, false)
                .pin
                .is_err()
        );
    }

    #[test]
    fn a_builders_lamp_keeps_5197s_behaviour() {
        let unset = wanted(None, None, None, false, true);
        assert_eq!((unset.on, unset.source), (false, Source::Unset));
        assert_eq!(wanted(Some("on"), None, None, false, true).pin, Ok(None));
    }

    #[test]
    fn a_manifests_version_is_read_only_when_plain() {
        assert_eq!(
            version_of(br#"{"cli":{"version":"1.2.3"}}"#).as_deref(),
            Some("1.2.3")
        );
        for bad in [
            &br#"{"cli":{"version":"1.2"}}"#[..],
            br#"{"cli":{"version":"1.2.x"}}"#,
            br#"{"version":"1.2.3"}"#,
            b"not json",
        ] {
            assert_eq!(version_of(bad), None);
        }
    }

    fn lamp_with(exec: &str, link: Option<&str>) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let units = root.path().join("etc/systemd/system");
        fs::create_dir_all(&units).unwrap();
        fs::write(
            units.join("lamp-update.service"),
            format!("[Service]\nExecStart={exec} serve --apply on\n"),
        )
        .unwrap();
        if let Some(build) = link {
            let folder = root.path().join("opt/lampOS/update");
            fs::create_dir_all(folder.join(build)).unwrap();
            symlink(build, folder.join("current")).unwrap();
        }
        root
    }

    #[test]
    fn the_products_unit_is_a_product_lamp_and_a_builders_is_not() {
        let units = |root: &tempfile::TempDir| root.path().join("etc/systemd/system");
        let product = lamp_with("/opt/lampOS/update/current/lamp-update", Some(BUILD));
        assert!(product_lamp(product.path(), &units(&product)));
        // deploy/units/: a build's own folder, no `current`.
        let builder = lamp_with(&format!("/opt/lampOS/update/{BUILD}/lamp-update"), None);
        assert!(!product_lamp(builder.path(), &units(&builder)));
        // The link names no build.
        let dangling = lamp_with("/opt/lampOS/update/current/lamp-update", Some("next"));
        assert!(!product_lamp(dangling.path(), &units(&dangling)));
        // No unit at all.
        let none = tempfile::tempdir().unwrap();
        assert!(!product_lamp(none.path(), &units(&none)));
    }

    #[test]
    fn the_products_own_lamp_update_unit_is_one() {
        let root = lamp_with("/opt/lampOS/update/current/lamp-update", Some(BUILD));
        let units = root.path().join("etc/systemd/system");
        fs::write(
            units.join("lamp-update.service"),
            include_str!("../../../../deploy/product/lamp-update.service"),
        )
        .unwrap();
        assert!(product_lamp(root.path(), &units));
    }
}
