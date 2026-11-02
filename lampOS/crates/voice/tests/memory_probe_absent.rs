//! The memory benchmark's probe (`remember::probe`, #4748) is never in a
//! product build: it is compiled only for tests or with
//! `--cfg lamp_memory_probe`, `lib.rs` refuses it in an optimised build, and
//! nothing that makes or ships the lamp's binaries sets that flag.
use std::{fs, path::Path};

const FLAG: &str = "lamp_memory_probe";

/// Every file under `dir`, with its text, skipping what is not text.
fn texts(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            texts(&path, out);
        } else if let Ok(text) = fs::read_to_string(&path) {
            out.push((path.display().to_string(), text));
        }
    }
}

#[test]
fn no_product_build_or_package_sets_the_probe_flag() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    // Cargo's own settings, the lamp's packaging and install, and CI's
    // builds, the ARM64 product binaries' included.
    texts(&lamp_os.join(".cargo"), &mut files);
    texts(&lamp_os.join("deploy"), &mut files);
    texts(&lamp_os.join("../.github/workflows"), &mut files);
    assert!(
        files
            .iter()
            .any(|(path, _)| path.ends_with("lampos-arm64.yml")),
        "the ARM64 workflow was not read"
    );
    let setting: Vec<_> = files
        .iter()
        .filter(|(_, text)| text.contains(FLAG))
        .map(|(path, _)| path.as_str())
        .collect();
    assert!(setting.is_empty(), "{FLAG} set in {setting:?}");
}

#[test]
fn the_probe_is_refused_in_an_optimised_build() {
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("#[cfg(all(lamp_memory_probe, not(debug_assertions)))]\ncompile_error!("),
        "lib.rs no longer refuses the probe in an optimised build"
    );
}
