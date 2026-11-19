//! The lamp's NPU service (docs/npu.md, #7415): the one process that opens
//! the NPU (`/dev/vipcore`), so the eyes' models and the apps' share one
//! owner. Its first op is `pose`: one JPEG in, one person's 17 COCO
//! keypoints out, for a client that holds the `camera` grant, asked through
//! lamp-control. The model is not in the image: it is fetched from the Store
//! on first use, or when a pose-using app is installed, and kept only once
//! the Store's signature and its SHA-256 are checked. Off unless
//! `LAMP_NPU=on`.
pub mod apps;
pub mod engine;
pub mod head;
pub mod image;
pub mod model;
pub mod serve;
pub mod service;
// The vendor's C library, opened at run time: the crate's only unsafe code.
#[allow(unsafe_code)]
pub mod vip;

/// The switch, in lamp-npu.service's and lamp-control.service's
/// environment: `on`, or off.
pub const SWITCH: &str = "LAMP_NPU";
/// The service's socket, lamp-control's way in.
pub const SOCKET: &str = "/run/lampos-npu/npu.sock";
/// The service's own state: the models it keeps.
pub const STATE: &str = "/var/lib/lampos-npu";
/// The A733's NPU, as a model's manifest names it.
pub const BOARD: &str = "VIP9000NANODI_PID0X1000003B";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}
