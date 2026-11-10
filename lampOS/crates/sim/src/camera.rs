//! The host's camera as the eyes' sight: the newest frame in one JPEG file
//! (`--sight-file` of lamp-control), kept while the camera is wanted (and for a
//! while after), and removed when the camera closes.
//! The host's camera is opened by `ffmpeg`, so one path serves every OS.
use std::{
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The camera stays open this long after the last mark asked for it.
const KEEP_OPEN: Duration = Duration::from_secs(10);

pub struct Camera {
    sight: PathBuf,
    child: Option<Child>,
    asked: Option<Instant>,
}

impl Camera {
    pub fn new(sight: &Path) -> Camera {
        Camera {
            sight: sight.to_owned(),
            child: None,
            asked: None,
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
        }
        match (wanted, self.child.is_some()) {
            (true, false) => self.child = self.open(),
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
        // killed), the camera closes with it.
        let script = "ffmpeg -loglevel error -nostdin -f \"$FORMAT\" -framerate 15 -i \"$DEVICE\" \
             -vf fps=6,scale=320:-1 -f image2 -update 1 -atomic_writing 1 -y \"$SIGHT\" & F=$!; \
             while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
             kill $F 2>/dev/null; rm -f \"$SIGHT\"";
        match Command::new("sh")
            .args(["-c", script])
            .env("FORMAT", format)
            .env("DEVICE", &device)
            .env("SIGHT", &self.sight)
            .env("OWNER", std::process::id().to_string())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => Some(child),
            Err(error) => {
                eprintln!("lamp-sim: no camera: {error}");
                None
            }
        }
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
