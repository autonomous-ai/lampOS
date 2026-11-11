//! The host's camera as the eyes' sight: the newest frame in one JPEG file
//! (`--sight-file` of lamp-control), kept while the camera is wanted (and for a
//! while after), and removed when the camera closes. The host's camera is
//! opened by `ffmpeg`, so one path serves every OS; its frames come through a
//! pipe, and each is warped to what the lamp's camera would see at the pose of
//! the head (`pose.rs`, `warp.rs`) before it is written: facing the person, the
//! picture as it is; turned away, moved across, faded, then black.
use crate::{
    pose::{self, Model},
    servo::Pose,
    warp::{self, Picture},
};
use std::{
    io::Read,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// The camera stays open this long after the last mark asked for it.
const KEEP_OPEN: Duration = Duration::from_secs(10);
/// The quality of the picture written.
const QUALITY: u8 = 80;

/// What turns the picture: where the servos are, and a gain on how far they
/// turned from the pose that faces the person.
#[derive(Clone)]
pub struct Warp {
    pub pose: Pose,
    pub gain: f64,
}

/// A camera that ended is opened again no sooner than this.
const RETRY_AFTER: Duration = Duration::from_secs(5);

pub struct Camera {
    sight: PathBuf,
    child: Option<Child>,
    asked: Option<Instant>,
    /// No new try to open before this: ffmpeg that ends at once (not
    /// installed, the camera busy, a format refused) is tried again every
    /// [`RETRY_AFTER`], not every tick.
    retry_at: Option<Instant>,
    warp: Option<Warp>,
}

impl Camera {
    pub fn new(sight: &Path, warp: Option<Warp>) -> Camera {
        Camera {
            sight: sight.to_owned(),
            child: None,
            asked: None,
            retry_at: None,
            warp,
        }
    }

    /// Called often: `marked` is whether the camera should be open now; it
    /// stays open for [`KEEP_OPEN`] after the last time it should have been.
    pub fn step(&mut self, marked: bool) {
        if marked {
            self.asked = Some(Instant::now());
        }
        let wanted = self.asked.is_some_and(|at| at.elapsed() < KEEP_OPEN);
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(Some(_)))
        {
            self.child = None;
            self.retry_at = Some(Instant::now() + RETRY_AFTER);
            eprintln!(
                "lamp-sim: the host camera's ffmpeg ended (ffmpeg missing, camera busy or not allowed?); trying again in {} s",
                RETRY_AFTER.as_secs()
            );
        }
        let may_try = self.retry_at.is_none_or(|at| Instant::now() >= at);
        match (wanted, self.child.is_some()) {
            (true, false) if may_try => self.child = self.open(),
            (false, true) => self.close(),
            _ => {}
        }
    }

    fn open(&self) -> Option<Child> {
        // The device and the format reach the shell as environment values, never
        // as text of the script, so a value can't break out of it.
        let (format, default) = if cfg!(target_os = "macos") {
            ("avfoundation", "0")
        } else {
            ("v4l2", "/dev/video0")
        };
        let device = std::env::var("LAMP_SIM_CAMERA").unwrap_or_else(|_| default.to_owned());
        // ffmpeg under a shell that watches lamp-sim: when lamp-sim goes (even
        // killed), the camera closes with it. Its frames, JPEG after JPEG, go to
        // the standard output.
        let script = "ffmpeg -loglevel error -nostdin -f \"$FORMAT\" -framerate 15 -i \"$DEVICE\" \
             -vf fps=6,scale=320:-1 -q:v 5 -f mjpeg - & F=$!; \
             while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
             kill $F 2>/dev/null; rm -f \"$SIGHT\"";
        let mut child = match Command::new("sh")
            .args(["-c", script])
            .env("FORMAT", format)
            .env("DEVICE", &device)
            .env("SIGHT", &self.sight)
            .env("OWNER", std::process::id().to_string())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("lamp-sim: no camera: {error}");
                return None;
            }
        };
        let out = child.stdout.take()?;
        let (sight, warp) = (self.sight.clone(), self.warp.clone());
        std::thread::spawn(move || pump(out, &sight, warp.as_ref()));
        Some(child)
    }

    fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
            }
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.sight);
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.close();
    }
}

/// Reads the camera's JPEGs from `out` until it ends, and writes each, warped
/// by the head's pose, as the sight file (whole or not at all).
fn pump(mut out: ChildStdout, sight: &Path, warp: Option<&Warp>) {
    let model = Model::shipped();
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 65_536];
    let temp = sight.with_extension("jpg.tmp");
    loop {
        match out.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
        while let Some((start, end)) = frame(&buffer) {
            let jpeg = &buffer[start..end];
            let written = match warp {
                Some(warp) => warped(jpeg, &model, warp),
                None => Some(jpeg.to_vec()),
            };
            if let Some(bytes) = written
                && std::fs::write(&temp, bytes).is_ok()
            {
                let _ = std::fs::rename(&temp, sight);
            }
            buffer.drain(..end);
        }
        // Never hold more than a few frames of a stream that has no frames in it.
        if buffer.len() > 4 * 1024 * 1024 {
            buffer.clear();
        }
    }
}

/// The first whole JPEG in `bytes`, as its start and end (past its last byte):
/// from the first start-of-image to the first end-of-image after it (the entropy
/// coded data stuffs every 0xFF, so an 0xFFD9 is the end).
pub fn frame(bytes: &[u8]) -> Option<(usize, usize)> {
    let start = bytes.windows(2).position(|w| w == [0xff, 0xd8])?;
    let end = bytes[start + 2..]
        .windows(2)
        .position(|w| w == [0xff, 0xd9])?
        + start
        + 4;
    Some((start, end))
}

/// `jpeg` as the camera would see it at the head's pose now, as a JPEG.
fn warped(jpeg: &[u8], model: &Model, warp: &Warp) -> Option<Vec<u8>> {
    use zune_jpeg::{
        JpegDecoder,
        zune_core::{colorspace::ColorSpace, options::DecoderOptions},
    };
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(jpeg, options);
    let rgb = decoder.decode().ok()?;
    let (width, height) = decoder.dimensions()?;
    let counts: [f64; 5] = std::array::from_fn(|i| f64::from(warp.pose[i].load(Ordering::Relaxed)));
    let m = pose::relative(model, counts, warp.gain);
    let picture = warp::turned(&Picture { width, height, rgb }, &m);
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, QUALITY)
        .encode(
            &picture.rgb,
            u16::try_from(picture.width).ok()?,
            u16::try_from(picture.height).ok()?,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_runs_from_its_start_to_its_end_and_a_cut_one_waits() {
        let bytes = [0, 0, 0xff, 0xd8, 1, 2, 3, 0xff, 0xd9, 0xff, 0xd8, 9];
        assert_eq!(frame(&bytes), Some((2, 9)));
        assert_eq!(frame(&bytes[9..]), None);
        assert_eq!(frame(&[0xff, 0xd8, 1, 2]), None);
    }
}
