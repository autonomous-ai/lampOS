//! The person's quiet hours, kept (#8866, `quiet.get` and `quiet.set` from
//! the app through lamp-cloud).
//!
//! One window for the whole lamp: on or off, a start and an end in the
//! lamp's local time. `lamp-actions` is its only keeper, beside the output
//! caps it already keeps: the window is written to [`FILE`] in the caps
//! folder (`LAMP_OUTPUT_CAPS_DIR`, which the product reset wipes), synced,
//! then renamed into place, as `kept-volume` is, so a power cut leaves the
//! old window or the new one and an update keeps it. With no file, or one
//! that can't be read, the window is the lamp's old fixed night, on, 21:00
//! to 06:00 ([`QuietWindow::default`]), so a lamp nobody sets behaves as
//! before.
//!
//! It is asked on the caps' socket, only while the caps are on:
//! `{"op":"quiet_get"}` and `{"op":"quiet_put","on":bool,"start":"HH:MM",
//! "end":"HH:MM"}`, all three keys and no other. Both answer the window
//! kept then, `{"ok":true,"on","start","end"}`; a put refused changes
//! nothing, the file included. The window is said on the bus
//! (`lamp_msgs::QuietWindow`, `quiet/window`) each second the caps are
//! looked at, and at once after a put, for Skills, which holds the night
//! ceilings to it and anchors the evening glow and the morning hum to its
//! start and end. Actions itself caps nothing by it: Skills' caps do.

use lamp_bus::{Bus, Publisher};
use lamp_msgs::{QuietWindow, quiet_minute, quiet_time};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

/// The file in the caps folder: `{"on":true,"start":"21:00","end":"06:00"}`.
pub const FILE: &str = "quiet-window";
const NEW: &str = "quiet-window.new";

/// What a put must be, said when one isn't (cloud.md, "Quiet hours from
/// the app").
pub const SHAPE: &str = "quiet_put takes {\"on\": bool, \"start\": \"HH:MM\", \"end\": \"HH:MM\"}";
/// Why a put with the same start and end is refused.
pub const SAME: &str = "quiet hours need a start and an end that differ";
/// Why a put that could not be written changed nothing.
pub const NOT_KEPT: &str = "the quiet hours could not be kept";

/// Whether `op` is one of these, not a caps op or a call.
pub fn is_operation(op: &str) -> bool {
    matches!(op, "quiet_get" | "quiet_put")
}

/// The file's shape. Fields a later build may add are passed over, so a
/// lamp put back to this build keeps the person's window.
#[derive(Deserialize)]
struct Kept {
    on: bool,
    start: String,
    end: String,
}

/// The window `text` keeps, if it is one.
pub fn parse(text: &str) -> Option<QuietWindow> {
    let kept: Kept = serde_json::from_str(text).ok()?;
    let window = QuietWindow {
        on: kept.on,
        start: quiet_minute(&kept.start)?,
        end: quiet_minute(&kept.end)?,
    };
    window.check().ok().map(|()| window)
}

/// The window as the file and the socket write it.
pub fn text(window: &QuietWindow) -> Value {
    json!({"on": window.on, "start": quiet_time(window.start), "end": quiet_time(window.end)})
}

/// The window kept in `directory`: the default with no file or one that
/// can't be read.
pub fn read(directory: &Path) -> QuietWindow {
    fs::read_to_string(directory.join(FILE))
        .ok()
        .as_deref()
        .and_then(parse)
        .unwrap_or_default()
}

/// Write `window` so a power cut leaves either the old file or the new one.
pub fn write(directory: &Path, window: &QuietWindow) -> io::Result<()> {
    window.check().map_err(io::Error::other)?;
    let new = directory.join(NEW);
    let mut file = fs::File::create(&new)?;
    let mut line = text(window).to_string();
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&new, directory.join(FILE))?;
    fs::File::open(directory)?.sync_all()
}

/// The socket's two ops. Unknown fields, a missing one or a wrong type
/// refuse the line whole.
#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Request {
    #[serde(rename = "quiet_get")]
    Get {},
    #[serde(rename = "quiet_put")]
    Put {
        on: bool,
        start: String,
        end: String,
    },
}

/// The kept window, and the publisher that says it.
pub struct Quiet {
    directory: PathBuf,
    window: QuietWindow,
    publisher: Publisher,
    published: Option<bool>,
}

impl Quiet {
    /// Reads the kept window and declares `quiet/window` on `bus`.
    pub fn open(directory: &Path, bus: &Bus) -> io::Result<Self> {
        let publisher = bus.publisher_declared(
            QuietWindow::TOPIC,
            &QuietWindow::declaration(),
            QuietWindow::SOURCE,
        )?;
        Ok(Self {
            directory: directory.into(),
            window: read(directory),
            publisher,
            published: None,
        })
    }

    /// The window kept now.
    pub fn window(&self) -> QuietWindow {
        self.window
    }

    /// Says the window on the bus. A failure is said in the journal once,
    /// and again once it is said after; it never stops the caps.
    pub fn publish(&mut self, at_us: u64) {
        let published = self.publisher.publish_json(at_us, &self.window).is_ok();
        if self.published != Some(published) {
            if !published || self.published.is_some() {
                eprintln!(
                    "lamp-actions: the quiet window {}",
                    if published {
                        "is said again"
                    } else {
                        "could not be said"
                    }
                );
            }
            self.published = Some(published);
        }
    }

    /// The answer to a `quiet_get` or `quiet_put` line. A put is checked
    /// whole, then written, and only once written is it the window kept
    /// and said; a refusal leaves both as they were.
    pub fn answer(&mut self, line: &[u8], at_us: u64) -> Value {
        let refused = |code: &str, error: &str| json!({"ok":false,"code":code,"error":error});
        let request = match serde_json::from_slice::<Request>(line) {
            Ok(request) => request,
            Err(_) => return refused("bad_arguments", SHAPE),
        };
        if let Request::Put { on, start, end } = request {
            let (Some(start), Some(end)) = (quiet_minute(&start), quiet_minute(&end)) else {
                return refused("bad_arguments", SHAPE);
            };
            let window = QuietWindow { on, start, end };
            if window.check().is_err() {
                return refused("bad_arguments", SAME);
            }
            if let Err(error) = write(&self.directory, &window) {
                // A path may be in the error: the journal gets its kind.
                eprintln!(
                    "lamp-actions: the quiet window was not kept: {:?}",
                    error.kind()
                );
                return refused("storage", NOT_KEPT);
            }
            self.window = window;
            self.publish(at_us);
        }
        let mut answer = text(&self.window);
        answer["ok"] = json!(true);
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> (tempfile::TempDir, PathBuf, Bus) {
        let root = tempfile::Builder::new().prefix("aq-").tempdir().unwrap();
        let caps = root.path().join("caps");
        fs::create_dir(&caps).unwrap();
        let bus = Bus::new(root.path().join("bus")).unwrap();
        (root, caps, bus)
    }

    fn said(bus: &Bus) -> Option<QuietWindow> {
        QuietWindow::from_message(&bus.latest(QuietWindow::TOPIC).unwrap()?)
    }

    const NIGHT: QuietWindow = QuietWindow {
        on: true,
        start: 22 * 60,
        end: 7 * 60,
    };

    #[test]
    fn with_no_file_or_an_unreadable_one_the_window_is_the_old_night() {
        let (_root, caps, bus) = folder();
        let mut quiet = Quiet::open(&caps, &bus).unwrap();
        assert_eq!(quiet.window(), QuietWindow::default());
        assert_eq!(
            quiet.answer(br#"{"op":"quiet_get"}"#, 1),
            json!({"ok":true,"on":true,"start":"21:00","end":"06:00"})
        );
        for broken in [
            "",
            "{",
            r#"{"on":true,"start":"25:00","end":"06:00"}"#,
            r#"{"on":true,"start":"06:00","end":"06:00"}"#,
            r#"{"on":1,"start":"21:00","end":"06:00"}"#,
        ] {
            fs::write(caps.join(FILE), broken).unwrap();
            assert_eq!(read(&caps), QuietWindow::default(), "{broken:?}");
        }
        // A later build's field is passed over: the window is still read.
        fs::write(
            caps.join(FILE),
            r#"{"on":false,"start":"22:00","end":"07:00","weekdays":"later"}"#,
        )
        .unwrap();
        assert_eq!(read(&caps), QuietWindow { on: false, ..NIGHT });
    }

    #[test]
    fn a_put_is_kept_said_and_read_again_after_a_restart() {
        let (_root, caps, bus) = folder();
        let mut quiet = Quiet::open(&caps, &bus).unwrap();
        let answer = quiet.answer(
            br#"{"op":"quiet_put","on":true,"start":"22:00","end":"07:00"}"#,
            5,
        );
        assert_eq!(
            answer,
            json!({"ok":true,"on":true,"start":"22:00","end":"07:00"})
        );
        assert_eq!(said(&bus), Some(NIGHT), "said at once");
        assert_eq!(
            fs::read_to_string(caps.join(FILE)).unwrap(),
            "{\"end\":\"07:00\",\"on\":true,\"start\":\"22:00\"}\n"
        );
        assert!(!caps.join(NEW).exists());
        drop(quiet);
        // A new service, as after a restart or an update.
        let mut again = Quiet::open(&caps, &bus).unwrap();
        assert_eq!(again.window(), NIGHT);
        assert_eq!(again.answer(br#"{"op":"quiet_get"}"#, 9), answer);
        let off = again.answer(
            br#"{"op":"quiet_put","on":false,"start":"22:00","end":"07:00"}"#,
            10,
        );
        assert_eq!(off["on"], false);
        assert_eq!(read(&caps), QuietWindow { on: false, ..NIGHT });
    }

    #[test]
    fn a_bad_put_is_refused_and_changes_nothing() {
        let (_root, caps, bus) = folder();
        let mut quiet = Quiet::open(&caps, &bus).unwrap();
        quiet.answer(
            br#"{"op":"quiet_put","on":true,"start":"22:00","end":"07:00"}"#,
            1,
        );
        let kept = fs::read(caps.join(FILE)).unwrap();
        for (line, error) in [
            (r#"{"op":"quiet_put","on":true,"start":"22:00"}"#, SHAPE),
            (
                r#"{"op":"quiet_put","on":"yes","start":"22:00","end":"07:00"}"#,
                SHAPE,
            ),
            (
                r#"{"op":"quiet_put","on":true,"start":"22:00","end":"07:00","days":1}"#,
                SHAPE,
            ),
            (
                r#"{"op":"quiet_put","on":true,"start":"10pm","end":"07:00"}"#,
                SHAPE,
            ),
            (
                r#"{"op":"quiet_put","on":true,"start":"24:00","end":"07:00"}"#,
                SHAPE,
            ),
            (
                r#"{"op":"quiet_put","on":true,"start":22,"end":"07:00"}"#,
                SHAPE,
            ),
            (r#"{"op":"quiet_get","on":true}"#, SHAPE),
            (
                r#"{"op":"quiet_put","on":false,"start":"07:00","end":"07:00"}"#,
                SAME,
            ),
        ] {
            let answer = quiet.answer(line.as_bytes(), 2);
            assert_eq!(
                answer,
                json!({"ok":false,"code":"bad_arguments","error":error}),
                "{line}"
            );
            assert_eq!(fs::read(caps.join(FILE)).unwrap(), kept, "{line}");
            assert_eq!(quiet.window(), NIGHT);
        }
        // A folder that can't be written keeps the old window, kept and said.
        fs::remove_dir_all(&caps).unwrap();
        let answer = quiet.answer(
            br#"{"op":"quiet_put","on":false,"start":"20:00","end":"05:00"}"#,
            3,
        );
        assert_eq!(answer["code"], "storage");
        assert_eq!(answer["error"], NOT_KEPT);
        assert_eq!(quiet.window(), NIGHT);
        assert_eq!(said(&bus), Some(NIGHT));
    }

    #[test]
    fn each_publish_carries_the_window_kept() {
        let (_root, caps, bus) = folder();
        write(&caps, &NIGHT).unwrap();
        let mut quiet = Quiet::open(&caps, &bus).unwrap();
        assert_eq!(said(&bus), None, "nothing said before the first look");
        quiet.publish(1);
        let message = bus.latest(QuietWindow::TOPIC).unwrap().unwrap();
        assert_eq!(message.header.source, "actions");
        assert_eq!(QuietWindow::from_message(&message), Some(NIGHT));
        assert!(
            write(
                &caps,
                &QuietWindow {
                    start: 420,
                    ..NIGHT
                }
            )
            .is_err()
        );
    }
}
