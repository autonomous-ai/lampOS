//! The pinned manifest handed to Harness's installer from 127.0.0.1
//! (#5229): the bytes lamp-update has already checked by their sha256,
//! served while the installer runs and at no other time.
//!
//! Harness's installer reads its manifest with Node's `fetch`, so it takes
//! an http URL but no `file://` one (openharness `install.sh`). Served here,
//! the manifest the installer reads is exactly the one checked: no server
//! can change it between lamp-update's read and the installer's.
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// The path the manifest is served at; any other is answered 404.
pub const PATH: &str = "/metadata.json";
/// How long one connection may take to ask.
const ASK_WITHIN: Duration = Duration::from_secs(5);
/// The longest request read.
const MOST_REQUEST: usize = 8 * 1024;
/// How often the listener looks whether it is to stop.
const POLL: Duration = Duration::from_millis(20);

/// The manifest served on 127.0.0.1 until dropped.
pub struct Served {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Served {
    /// `bytes` served at [`PATH`] on 127.0.0.1, a port the system picks.
    pub fn start(bytes: Vec<u8>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("harness-manifest".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = answer(stream, &bytes);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(POLL);
                        }
                        Err(_) => thread::sleep(POLL),
                    }
                }
            })?;
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    /// The URL the installer is given: `http://127.0.0.1:<port>/metadata.json`.
    pub fn url(&self) -> String {
        format!("http://{}{PATH}", self.address)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One connection: a `GET` of [`PATH`] gets the bytes, anything else 404.
fn answer(mut stream: TcpStream, bytes: &[u8]) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(ASK_WITHIN))?;
    stream.set_write_timeout(Some(ASK_WITHIN))?;
    let mut asked = Vec::new();
    let mut chunk = [0u8; 1024];
    while !asked.windows(4).any(|end| end == b"\r\n\r\n") && asked.len() < MOST_REQUEST {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        asked.extend_from_slice(&chunk[..read]);
    }
    let line = asked.split(|&b| b == b'\r').next().unwrap_or_default();
    let wanted = line.starts_with(format!("GET {PATH} ").as_bytes())
        || line.starts_with(format!("GET {PATH}?").as_bytes());
    if wanted {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )?;
        stream.write_all(bytes)?;
    } else {
        stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str, path: &str) -> String {
        let address = url
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
        let mut said = String::new();
        stream.read_to_string(&mut said).unwrap();
        said
    }

    #[test]
    fn the_checked_bytes_are_served_on_loopback_until_dropped() {
        let served = Served::start(b"{\"cli\":{}}".to_vec()).unwrap();
        let url = served.url();
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(url.ends_with(PATH), "{url}");
        for _ in 0..2 {
            let said = get(&url, PATH);
            assert!(said.starts_with("HTTP/1.1 200 OK\r\n"), "{said}");
            assert!(said.contains("Content-Length: 10\r\n"), "{said}");
            assert!(said.ends_with("\r\n\r\n{\"cli\":{}}"), "{said}");
        }
        assert!(get(&url, "/other.json").starts_with("HTTP/1.1 404"));
        let address = url
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .unwrap()
            .to_owned();
        drop(served);
        assert!(TcpStream::connect(address).is_err());
    }
}
