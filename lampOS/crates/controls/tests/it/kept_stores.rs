//! The controls' files as earlier builds wrote them (`docs/updates.md`,
//! "Every part's saved data, one version ahead"), kept in
//! `tests/fixtures/stores/` and read here as `lamp-controls serve` reads them
//! at a start. They live in `/etc/lampOS/controls`, so a restart in the same
//! boot and one after a reboot find the same bytes: both are this read.
use lamp_controls::{engine::Config, forward, pins::PinsConfig, touch_pins::TouchPinsConfig};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Each kept day: its `controls.json`, the product's pins file (install's,
/// read with `--touch`) and the pins file prepare gives a lamp moved from V1
/// (read without it).
const KEPT: [(&str, &str, &str, &str); 1] = [(
    "2026-10-22",
    include_str!("../fixtures/stores/controls-2026-10-22.json"),
    include_str!("../fixtures/stores/controls-pins-2026-10-22.json"),
    include_str!("../fixtures/stores/controls-pins-2026-10-22-moved.json"),
)];

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = if Path::new("/private/tmp").is_dir() {
            PathBuf::from("/private/tmp")
        } else {
            fs::canonicalize(std::env::temp_dir()).unwrap()
        };
        let path = parent.join(format!(
            "cks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `controls.json` as `serve` reads it: past a newer build's fields, then
/// checked.
fn controls(day: &str, path: &Path) -> Config {
    let (config, _) = forward::read::<Config>(path, &fs::read(path).unwrap())
        .unwrap_or_else(|error| panic!("{day}: controls.json: {error}"));
    config
        .check()
        .unwrap_or_else(|error| panic!("{day}: controls.json: {error}"));
    config
}

#[test]
fn each_kept_controls_file_reads_as_serve_reads_it() {
    for (day, config, product, moved) in KEPT {
        let root = Root::new();
        let config = controls(day, &root.file("controls.json", config));
        assert_eq!(config.buttons, ["primary"], "{day}");

        // The product's, with `--touch`: its slide, its reset and the
        // legacy head are read, and the wiring fits the controls.
        let (wiring, _) = TouchPinsConfig::read(&root.file("controls-pins.json", product))
            .unwrap_or_else(|error| panic!("{day}: product pins: {error}"));
        assert!(wiring.privacy.is_some(), "{day}: the product's slide");
        assert!(wiring.reset.is_some(), "{day}: the product's reset");
        wiring
            .check(&config)
            .unwrap_or_else(|error| panic!("{day}: product pins: {error}"));

        // A moved lamp's, without `--touch`: none of the three the generic
        // controls refuse is there, and the wiring fits the controls.
        let path = root.file("controls-pins-moved.json", moved);
        let (wiring, _) = forward::read::<PinsConfig>(&path, moved.as_bytes())
            .unwrap_or_else(|error| panic!("{day}: moved pins: {error}"));
        assert!(wiring.privacy.is_none(), "{day}: no slide on a moved lamp");
        wiring
            .check(&config)
            .unwrap_or_else(|error| panic!("{day}: moved pins: {error}"));
    }
}
