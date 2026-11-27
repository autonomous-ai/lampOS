use lamp_sensor_score::archive::{self, Inventory};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn fixture() -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "sensor-inventory-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let raw = base.join("raw");
    fs::create_dir_all(raw.join("capture")).unwrap();
    fs::write(
        raw.join("README.md"),
        b"Invented fixture; no people or recordings.",
    )
    .unwrap();
    fs::write(raw.join("capture/data.bin"), [0_u8, 1, 2, 3]).unwrap();
    let out = base.join("inventory");
    (base, raw, out)
}
fn saved(out: &std::path::Path) -> (Inventory, Vec<u8>) {
    (
        serde_json::from_slice(&fs::read(out.join("inventory.json")).unwrap()).unwrap(),
        fs::read(out.join("README.md")).unwrap(),
    )
}

#[test]
fn hashes_every_file_and_refuses_overwrite() {
    let (_, raw, out) = fixture();
    let before = fs::read(raw.join("capture/data.bin")).unwrap();
    let (inventory_hash, readme_hash) = archive::create(&raw, &out).unwrap();
    let (inventory, readme) = saved(&out);
    assert_eq!(inventory.files.len(), 2);
    assert_eq!(
        inventory_hash,
        lamp_sensor_score::digest(&fs::read(out.join("inventory.json")).unwrap())
    );
    assert_eq!(readme_hash, inventory.readme_sha256);
    archive::verify(&raw, &inventory, &readme).unwrap();
    assert!(archive::create(&raw, &out).is_err());
    assert_eq!(fs::read(raw.join("capture/data.bin")).unwrap(), before);
}

#[test]
fn detects_changed_added_missing_files_and_readme() {
    let (_, raw, out) = fixture();
    archive::create(&raw, &out).unwrap();
    let (inventory, readme) = saved(&out);
    assert!(archive::verify(&raw, &inventory, b"changed README").is_err());
    fs::write(raw.join("capture/data.bin"), [9_u8, 1, 2, 3]).unwrap();
    assert!(archive::verify(&raw, &inventory, &readme).is_err());
    fs::write(raw.join("capture/data.bin"), [0_u8, 1, 2, 3]).unwrap();
    fs::write(raw.join("new.txt"), b"new evidence").unwrap();
    assert!(archive::verify(&raw, &inventory, &readme).is_err());
    // Only invented test bytes are moved; retained production artifacts are never modified.
    fs::rename(raw.join("new.txt"), out.join("extra-test-bytes.txt")).unwrap();
    fs::rename(
        raw.join("capture/data.bin"),
        out.join("moved-test-bytes.bin"),
    )
    .unwrap();
    assert!(archive::verify(&raw, &inventory, &readme).is_err());
}

#[test]
fn rejects_nested_output_and_traversal_inventory() {
    let (_, raw, out) = fixture();
    assert!(archive::create(&raw, &raw.join("inventory")).is_err());
    archive::create(&raw, &out).unwrap();
    let (mut inventory, readme) = saved(&out);
    inventory.files[0].path = "../outside".into();
    assert!(archive::verify(&raw, &inventory, &readme).is_err());
}

#[cfg(unix)]
#[test]
fn private_modes_and_symlink_rejection() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_, raw, out) = fixture();
    archive::create(&raw, &out).unwrap();
    assert_eq!(
        fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for name in ["README.md", "inventory.json"] {
        assert_eq!(
            fs::metadata(out.join(name)).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    symlink(raw.join("capture/data.bin"), raw.join("linked.bin")).unwrap();
    let (inventory, readme) = saved(&out);
    assert!(archive::verify(&raw, &inventory, &readme).is_err());
    assert!(archive::create(&raw, &out.with_file_name("second-inventory")).is_err());
}

#[test]
fn cli_round_trip_and_hash_failure_exit_codes() {
    let (_, raw, out) = fixture();
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lamp-sensor-score"))
            .args(args)
            .output()
            .unwrap()
    };
    let result = invoke(&["archive", raw.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("inventory_sha256="));
    let inventory = out.join("inventory.json");
    assert!(
        invoke(&[
            "verify-archive",
            raw.to_str().unwrap(),
            inventory.to_str().unwrap()
        ])
        .status
        .success()
    );
    fs::write(raw.join("capture/data.bin"), b"corrupted").unwrap();
    assert_eq!(
        invoke(&[
            "verify-archive",
            raw.to_str().unwrap(),
            inventory.to_str().unwrap()
        ])
        .status
        .code(),
        Some(2)
    );
}
