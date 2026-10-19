use lamp_turn_owner::BootId;
use lamp_voice::{
    camera_config::{CameraConfig, UsbIdentity},
    fixture_provider::FixtureOptions,
    options::SessionOptions,
    visual_context::Source,
};
use std::path::Path;
fn arguments(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| s.to_string()).collect()
}
fn config() -> CameraConfig {
    CameraConfig {
        device: "/dev/video-by-id".into(),
        lock_directory: "/run/lamp-camera".into(),
        source: "desk".into(),
        expected_usb: UsbIdentity {
            vendor: 1,
            product: 2,
            topology: "1-2".into(),
            serial: None,
            interface_number: 0,
            capture_index: 0,
        },
        width: 320,
        height: 240,
        interval: None,
        max_frame_bytes: 262144,
        buffers: 2,
    }
}
#[test]
fn camera_is_opt_in_and_cannot_leak_into_fixture_or_positional_arguments() {
    assert!(SessionOptions::parse(&[]).unwrap().camera_config.is_none());
    let opts = SessionOptions::parse(&arguments(&[
        "--noise-suppression",
        "off",
        "--camera-config",
        "/private/camera.json",
        "--diagnostics",
    ]))
    .unwrap();
    assert_eq!(
        opts.camera_config.as_deref(),
        Some(Path::new("/private/camera.json"))
    );
    assert!(opts.directed.diagnostics);
    for bad in [
        vec!["--camera-config"],
        vec!["--camera-config", "relative.json"],
        vec!["/private/camera.json"],
        vec!["--camera-config", "/a", "--camera-config", "/b"],
    ] {
        assert!(SessionOptions::parse(&arguments(&bad)).is_err());
    }
    assert!(
        FixtureOptions::parse(&arguments(&["--diagnostics", "--camera-config", "/a"])).is_err()
    );
}
#[test]
fn explicit_small_native_mode_and_buffer_bound_reach_child_without_resizing_or_identity_guess() {
    let dir = Path::new("/tmp/private-session");
    let boot = BootId::new([9; 16]).unwrap();
    let original = Source::configured(dir, boot, &config()).unwrap();
    let child = Source::parse(dir, &original.arguments()).unwrap().unwrap();
    assert_eq!(child.camera, boot);
    assert_eq!(child.socket, dir.join("camera.frames"));
    assert_eq!(
        (child.width, child.height, child.max_frame_bytes),
        (320, 240, 262144)
    );
    assert_eq!(child.source, "desk");
    for (width, height, bytes) in [
        (1920, 1080, 262144),
        (641, 480, 262144),
        (640, 481, 262144),
        (320, 240, 1048577),
    ] {
        let mut c = config();
        c.width = width;
        c.height = height;
        c.max_frame_bytes = bytes;
        assert!(Source::configured(dir, boot, &c).is_err());
    }
    let mut bad = original.arguments();
    bad[3] = "1080".into();
    assert!(Source::parse(dir, &bad).is_err());
    bad = original.arguments();
    bad.push("extra".into());
    assert!(Source::parse(dir, &bad).is_err());
}
