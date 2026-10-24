use std::{fs, io::{BufRead, BufReader, Read, Write}, os::unix::{fs::PermissionsExt, net::{UnixListener, UnixStream}}, path::PathBuf, time::Duration};
use rustix::fs::{fcntl_getfl, OFlags};
struct Home(PathBuf);
impl Drop for Home { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
fn main() {
    assert!(cfg!(target_os = "macos"));
    let home = Home(PathBuf::from(format!("/tmp/second-eyes-fd-{}", std::process::id())));
    fs::create_dir(&home.0).unwrap(); fs::set_permissions(&home.0, fs::Permissions::from_mode(0o700)).unwrap();
    let path = home.0.join("eyes.sock"); let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap(); let mut client = UnixStream::connect(&path).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    let inherited = fcntl_getfl(&stream).unwrap().contains(OFlags::NONBLOCK);
    println!("accepted_inherited_nonblock={inherited}"); assert!(inherited);
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let error = stream.read(&mut [0_u8; 1]).unwrap_err();
    println!("old_empty_read={:?}", error.kind()); assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    stream.set_nonblocking(false).unwrap();
    assert!(!fcntl_getfl(&stream).unwrap().contains(OFlags::NONBLOCK));
    assert!(fcntl_getfl(&listener).unwrap().contains(OFlags::NONBLOCK));
    assert_eq!(stream.read_timeout().unwrap(), Some(Duration::from_secs(2)));
    println!("accepted_blocking=true\nlistener_still_polling=true\nexisting_two_second_timeout_retained=true");
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    client.write_all(b"{}\n").unwrap(); let mut request = String::new();
    BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap(); assert_eq!(request, "{}\n");
    stream.write_all(b"{\"ok\":true}\n").unwrap(); let mut reply = String::new();
    BufReader::new(client.try_clone().unwrap()).read_line(&mut reply).unwrap(); assert_eq!(reply, "{\"ok\":true}\n");
    drop(stream); drop(client); drop(listener); fs::remove_dir_all(&home.0).unwrap();
    println!("request_reply_round_trip=true\nowned_root_removed=true");
}
