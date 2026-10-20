//! Tells a body one mood on its `--mood-socket`, as the voice does: one
//! datagram `{"mood":MOOD,"level":LEVEL}`. The factory's steps use it to
//! stand a body run by hand on a private socket (#1682), so the lamp needs
//! no other language for it. It never opens the servo port.

use std::{ffi::OsString, os::unix::net::UnixDatagram, path::PathBuf, process::ExitCode};

const USAGE: &str = "lamp-body-mood --socket ABSOLUTE_PATH --mood WORD [--level 0..255]\n\nSends one {\"mood\":WORD,\"level\":LEVEL} datagram to a body's --mood-socket, as the voice does\n(LEVEL 128 unless given). WORD is 1 to 32 lowercase letters or underscores; the body says\nwhich it takes. Exits 0 once sent, 2 for bad arguments, 1 when it could not be sent.\n";

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    socket: PathBuf,
    mood: String,
    level: u8,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let (mut socket, mut mood, mut level) = (None, None, None);
    while let Some(key) = args.next() {
        if key == "--help" {
            return if socket.is_none() && mood.is_none() && level.is_none() && args.next().is_none()
            {
                Ok(None)
            } else {
                Err("--help must be used alone")
            };
        }
        let value = args.next().ok_or("every option requires a value")?;
        let value = value
            .to_str()
            .ok_or("every value must be UTF-8")?
            .to_owned();
        match key.to_str() {
            Some("--socket") if socket.is_none() => socket = Some(PathBuf::from(value)),
            Some("--mood") if mood.is_none() => mood = Some(value),
            Some("--level") if level.is_none() => {
                level = Some(value.parse::<u8>().map_err(|_| "--level is 0 to 255")?)
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    let socket = socket.ok_or("--socket is required")?;
    if !socket.is_absolute() {
        return Err("--socket must be an absolute path");
    }
    let mood = mood.ok_or("--mood is required")?;
    if mood.is_empty()
        || mood.len() > 32
        || !mood.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return Err("--mood is 1 to 32 lowercase letters or underscores");
    }
    Ok(Some(Arguments {
        socket,
        mood,
        level: level.unwrap_or(128),
    }))
}

fn datagram(arguments: &Arguments) -> String {
    // The word is letters and underscores only, so it needs no escaping.
    format!(
        r#"{{"mood":"{}","level":{}}}"#,
        arguments.mood, arguments.level
    )
}

fn send(arguments: &Arguments) -> std::io::Result<()> {
    UnixDatagram::unbound()?
        .send_to(datagram(arguments).as_bytes(), &arguments.socket)
        .map(|_| ())
}

fn main() -> ExitCode {
    let arguments = match parse(std::env::args_os().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprint!("lamp-body-mood: {why}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match send(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!(
                "lamp-body-mood: could not send to {}: {why}",
                arguments.socket.display()
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn it_takes_a_socket_a_mood_and_a_level() {
        let parsed = parse(args(&[
            "--socket",
            "/run/x/mood.sock",
            "--mood",
            "listening",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(datagram(&parsed), r#"{"mood":"listening","level":128}"#);
        let parsed = parse(args(&["--mood", "rest", "--level", "0", "--socket", "/a"]))
            .unwrap()
            .unwrap();
        assert_eq!(datagram(&parsed), r#"{"mood":"rest","level":0}"#);
        assert_eq!(parse(args(&["--help"])), Ok(None));
    }

    #[test]
    fn it_refuses_what_the_body_could_not_read() {
        for bad in [
            &["--mood", "listening"][..],
            &["--socket", "relative.sock", "--mood", "listening"],
            &["--socket", "/a", "--mood", "Listening"],
            &["--socket", "/a", "--mood", "a\"b"],
            &["--socket", "/a", "--mood", ""],
            &["--socket", "/a", "--mood", "listening", "--level", "256"],
            &["--socket", "/a", "--socket", "/b", "--mood", "listening"],
            &["--socket", "/a", "--mood"],
            &["--socket", "/a", "--help"],
        ] {
            assert!(parse(args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_datagram_reaches_a_socket() {
        let dir = std::env::temp_dir().join(format!("lamp-body-mood-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("mood.sock");
        let listener = UnixDatagram::bind(&path).unwrap();
        let parsed = parse(args(&[
            "--socket",
            path.to_str().unwrap(),
            "--mood",
            "listening",
        ]))
        .unwrap()
        .unwrap();
        send(&parsed).unwrap();
        let mut buffer = [0; 64];
        let n = listener.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], br#"{"mood":"listening","level":128}"#);
        // Nobody listening is a failure, not silence.
        drop(listener);
        assert!(send(&parsed).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
