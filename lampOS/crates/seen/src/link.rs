//! The frame link's boots, shared by `lamp-eyes` and `lamp-seen` in the
//! product (#6543, docs/seen.md, "The product unit").
//!
//! The eyes serve the camera's private frame link (`--frame-link`), and
//! take a hello only from the reader boot they were given, under the
//! controller boot they were given; `lamp-seen` says the same two as
//! `--own-boot` and `--controller-boot`. The voice makes such boots for its
//! camera worker from `/dev/urandom` and hands them on its command line
//! (`lamp_voice::process::new_boot`); here the two are separate units, so
//! `lamp-seen frame-link`, run before `lamp-seen serve` by its unit, makes
//! them the same way and keeps them in one file in `lamp-seen`'s runtime
//! folder:
//!
//! ```text
//! LAMP_EYES_FRAME_LINK=--frame-link SOCKET --frame-link-controller HEX32 --frame-link-reader HEX32
//! LAMP_EYES_VISITS=--visits on
//! ```
//!
//! `lamp-eyes.service` passes `--frame-link SOCKET --frame-link-boots FILE`
//! and the eyes read the boots from it themselves (#6645): never an
//! `EnvironmentFile=`, so the boots are on no command line and in no
//! environment. The second line is the eyes' `--visits`, for the gate's
//! stranger check only: `humans/familiar`, a level with no name, face or
//! picture (the first builder's yes on #6543). The eyes don't read it from
//! this file; it is also kept alone in [`VISITS_FILE`] beside it, which the
//! unit loads as an `EnvironmentFile=` and puts `$LAMP_EYES_VISITS` on its
//! command line. `lamp-seen serve` reads the link file
//! (`--frame-link-file`). With `LAMP_SEEN_ACTIVITY` off both files are
//! removed, so the eyes' next start serves no link and keeps no visits.
//! Kept until the lamp
//! restarts (the folder is tmpfs, kept across a stop of `lamp-seen`), so a
//! restart of either part alone finds the same boots as the other.
use lamp_turn_owner::BootId;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The file, in `lamp-seen`'s runtime folder.
pub const FILE: &str = "/run/lampos-seen/frame-link.env";

/// The link's socket, in its private folder (0700, the eyes' user's,
/// made by the product's tmpfiles).
pub const SOCKET: &str = "/run/lampos-eyes-frames/camera.frames";

/// The variable `lamp-eyes.service` puts on the eyes' command line.
pub const VARIABLE: &str = "LAMP_EYES_FRAME_LINK";

/// The visits file's name, beside [`FILE`]: only [`VISITS_LINE`].
pub const VISITS_FILE: &str = "eyes-visits.env";

/// The file's second line: the eyes' familiarity levels on
/// `humans/familiar`, which the gate's stranger check reads.
pub const VISITS_LINE: &str = "LAMP_EYES_VISITS=--visits on";

/// The file is read up to this many bytes.
const MOST_BYTES: u64 = 1024;

/// The link as the eyes serve it and `lamp-seen` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub socket: PathBuf,
    /// The eyes' turn-owner controller, under which each frame's grant is.
    pub controller: BootId,
    /// `lamp-seen`'s own boot, the only reader the eyes take.
    pub reader: BootId,
}

/// A boot as 32 hex digits.
pub fn boot_text(boot: BootId) -> String {
    boot.bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// 32 hex digits as a boot.
pub fn parse_boot(text: &str) -> Option<BootId> {
    if text.len() != 32 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    BootId::new(bytes).ok()
}

/// A new boot from `/dev/urandom`, as the voice makes its workers'.
pub fn new_boot() -> io::Result<BootId> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    BootId::new(bytes).map_err(io::Error::other)
}

impl Link {
    /// The file's two lines.
    pub fn line(&self) -> String {
        format!(
            "{VARIABLE}=--frame-link {} --frame-link-controller {} --frame-link-reader {}\n{VISITS_LINE}\n",
            self.socket.display(),
            boot_text(self.controller),
            boot_text(self.reader)
        )
    }

    /// The file's two lines read back; anything else is none.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.strip_suffix('\n')?.split('\n');
        let (first, visits) = (lines.next()?, lines.next()?);
        if visits != VISITS_LINE || lines.next().is_some() {
            return None;
        }
        let rest = first.strip_prefix(VARIABLE)?.strip_prefix('=')?;
        let words: Vec<&str> = rest.split(' ').collect();
        let [
            "--frame-link",
            socket,
            "--frame-link-controller",
            controller,
            "--frame-link-reader",
            reader,
        ] = words.as_slice()
        else {
            return None;
        };
        let socket = PathBuf::from(socket);
        if !socket.is_absolute() {
            return None;
        }
        let (controller, reader) = (parse_boot(controller)?, parse_boot(reader)?);
        (controller != reader).then_some(Self {
            socket,
            controller,
            reader,
        })
    }
}

/// The link kept in `file`, if there is one and it reads.
pub fn read(file: &Path) -> io::Result<Option<Link>> {
    let mut text = String::new();
    match fs::File::open(file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(opened) => opened.take(MOST_BYTES + 1).read_to_string(&mut text)?,
    };
    Ok(Link::parse(&text))
}

/// What [`prepare`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// The link already kept, for this socket, left as it was.
    Kept(Link),
    /// New boots, written.
    Made(Link),
    /// Off: no link kept (removed if there was one).
    Off,
}

/// The switch on: the link kept in `file` for `socket`, made with two new
/// boots from `boot` when there is none or it doesn't read, and the visits
/// file ([`VISITS_FILE`]) beside it. Off: both removed. Each written whole
/// (a file beside it, mode 0600, renamed over it).
pub fn prepare(
    file: &Path,
    socket: &Path,
    on: bool,
    mut boot: impl FnMut() -> io::Result<BootId>,
) -> io::Result<Prepared> {
    let visits = file.with_file_name(VISITS_FILE);
    if !on {
        remove(&visits)?;
        remove(file)?;
        return Ok(Prepared::Off);
    }
    let visits_text = format!("{VISITS_LINE}\n");
    if fs::read_to_string(&visits).ok().as_deref() != Some(visits_text.as_str()) {
        write_whole(&visits, visits_text.as_bytes())?;
    }
    if let Ok(Some(link)) = read(file)
        && link.socket == socket
    {
        return Ok(Prepared::Kept(link));
    }
    let controller = boot()?;
    let mut reader = boot()?;
    while reader == controller {
        reader = boot()?;
    }
    let link = Link {
        socket: socket.to_path_buf(),
        controller,
        reader,
    };
    write_whole(file, link.line().as_bytes())?;
    Ok(Prepared::Made(link))
}

/// `file` removed; none there is no error.
fn remove(file: &Path) -> io::Result<()> {
    match fs::remove_file(file) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// `bytes` written whole to `file`: a file beside it, mode 0600, renamed
/// over it.
fn write_whole(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut next = file.as_os_str().to_owned();
    next.push(".next");
    let next = PathBuf::from(next);
    let _ = fs::remove_file(&next);
    let mut written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)?;
    written.write_all(bytes)?;
    written.sync_all()?;
    drop(written);
    fs::rename(&next, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("seen-link-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn counting() -> impl FnMut() -> io::Result<BootId> {
        let mut next = 0u8;
        move || {
            next += 1;
            Ok(BootId::new([next; 16]).unwrap())
        }
    }

    #[test]
    fn on_makes_two_boots_once_and_keeps_them_for_both_parts() {
        let folder = folder("on");
        let file = folder.join("frame-link.env");
        let socket = Path::new(SOCKET);
        let Prepared::Made(made) = prepare(&file, socket, true, counting()).unwrap() else {
            panic!("made");
        };
        assert_ne!(made.controller, made.reader);
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(
            text,
            "LAMP_EYES_FRAME_LINK=--frame-link /run/lampos-eyes-frames/camera.frames \
             --frame-link-controller 01010101010101010101010101010101 \
             --frame-link-reader 02020202020202020202020202020202\n\
             LAMP_EYES_VISITS=--visits on\n"
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let visits = folder.join(VISITS_FILE);
        assert_eq!(
            fs::read_to_string(&visits).unwrap(),
            "LAMP_EYES_VISITS=--visits on\n"
        );
        assert_eq!(
            fs::metadata(&visits).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // A restart of either part finds the same boots.
        assert_eq!(
            prepare(&file, socket, true, || panic!("no new boot")).unwrap(),
            Prepared::Kept(made.clone())
        );
        assert_eq!(read(&file).unwrap(), Some(made));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn off_removes_the_link_so_the_eyes_serve_none() {
        let folder = folder("off");
        let file = folder.join("frame-link.env");
        assert_eq!(
            prepare(&file, Path::new(SOCKET), false, counting()).unwrap(),
            Prepared::Off
        );
        prepare(&file, Path::new(SOCKET), true, counting()).unwrap();
        assert_eq!(
            prepare(&file, Path::new(SOCKET), false, counting()).unwrap(),
            Prepared::Off
        );
        assert!(!file.exists());
        assert!(!folder.join(VISITS_FILE).exists());
        assert_eq!(read(&file).unwrap(), None);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_that_does_not_read_or_names_another_socket_is_made_again() {
        let folder = folder("again");
        let file = folder.join("frame-link.env");
        let boot = "0123456789abcdef0123456789abcdef";
        let other = "fedcba9876543210fedcba9876543210";
        for text in [
            String::new(),
            "LAMP_EYES_FRAME_LINK=".into(),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link relative --frame-link-controller {boot} --frame-link-reader {other}\n"
            ),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link /x --frame-link-controller {boot} --frame-link-reader {boot}\n"
            ),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link /x --frame-link-controller {boot} --frame-link-reader 00\n"
            ),
            format!(
                "OTHER=--frame-link /x --frame-link-controller {boot} --frame-link-reader {other}\n"
            ),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link /run/other.frames --frame-link-controller {boot} --frame-link-reader {other}\nLAMP_EYES_VISITS=--visits on\n"
            ),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link {SOCKET} --frame-link-controller {boot} --frame-link-reader {other}\n"
            ),
            format!(
                "LAMP_EYES_FRAME_LINK=--frame-link {SOCKET} --frame-link-controller {boot} --frame-link-reader {other}\nLAMP_EYES_VISITS=--visits off\n"
            ),
        ] {
            fs::write(&file, &text).unwrap();
            assert!(
                matches!(
                    prepare(&file, Path::new(SOCKET), true, counting()).unwrap(),
                    Prepared::Made(_)
                ),
                "{text}"
            );
        }
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_with_both_lines_reads_back() {
        let boot = "0123456789abcdef0123456789abcdef";
        let other = "fedcba9876543210fedcba9876543210";
        let link = Link::parse(&format!(
            "LAMP_EYES_FRAME_LINK=--frame-link {SOCKET} --frame-link-controller {boot} --frame-link-reader {other}\nLAMP_EYES_VISITS=--visits on\n"
        ))
        .unwrap();
        assert_eq!(link.controller, parse_boot(boot).unwrap());
        assert_eq!(link.reader, parse_boot(other).unwrap());
        assert_eq!(Link::parse(&link.line()), Some(link));
    }

    #[test]
    fn boots_are_32_hex_digits_both_ways() {
        let boot = new_boot().unwrap();
        assert_eq!(parse_boot(&boot_text(boot)), Some(boot));
        assert_eq!(parse_boot("00"), None);
        assert_eq!(parse_boot(&"0".repeat(32)), None, "a zero boot");
        assert_eq!(parse_boot(&"g".repeat(32)), None);
    }
}
