use super::*;
use std::collections::HashMap;
use std::io::Cursor;

/// A wire that reads a canned answer and records what was written.
struct Duplex {
    input: Cursor<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for Duplex {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for Duplex {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A resolver and a connector in one: names to addresses, and what each
/// connect answers, in order.
#[derive(Default)]
struct World {
    names: HashMap<String, Vec<IpAddr>>,
    answers: Vec<Vec<u8>>,
    resolved: Vec<String>,
    connected: Vec<(String, SocketAddrV4)>,
    refuse: Vec<Ipv4Addr>,
    requests: Arc<Mutex<Vec<u8>>>,
}

impl World {
    fn name(mut self, host: &str, address: &str) -> Self {
        self.names
            .insert(host.into(), vec![address.parse().unwrap()]);
        self
    }
    fn answer(mut self, bytes: &[u8]) -> Self {
        self.answers.push(bytes.to_vec());
        self
    }
}

impl Resolve for World {
    fn resolve(&mut self, host: &str) -> io::Result<Vec<IpAddr>> {
        self.resolved.push(host.to_owned());
        self.names
            .get(host)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

impl Connect for World {
    fn connect(&mut self, host: &str, address: SocketAddrV4) -> io::Result<Box<dyn Wire>> {
        self.connected.push((host.to_owned(), address));
        if self.refuse.contains(address.ip()) {
            return Err(io::Error::from(io::ErrorKind::ConnectionRefused));
        }
        let at = self.connected.len()
            - 1
            - self
                .connected
                .iter()
                .filter(|(_, a)| self.refuse.contains(a.ip()))
                .count();
        let bytes = self.answers.get(at).cloned().unwrap_or_default();
        Ok(Box::new(Duplex {
            input: Cursor::new(bytes),
            written: Arc::clone(&self.requests),
        }))
    }
}

fn run(world: &mut World, url: &str) -> Result<Vec<u8>, FetchError> {
    let allowed = Allowed::new(&["cdn.example.com", "files.example.net"]).unwrap();
    let mut body = get(&allowed, url, 1_000_000, world)?;
    let mut bytes = Vec::new();
    body.read_to_end(&mut bytes)
        .map_err(|_| FetchError::Unreachable)?;
    Ok(bytes)
}

fn ok(kind: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn redirect(location: &str) -> Vec<u8> {
    format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n").into_bytes()
}

const PUBLIC_A: &str = "93.184.216.34";
const PUBLIC_B: &str = "8.8.4.4";

#[test]
fn an_allowed_host_is_fetched_with_a_plain_get_and_nothing_else_sent() {
    let mut world = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(&ok("audio/mpeg", "MP3DATA"));
    assert_eq!(
        run(&mut world, "https://cdn.example.com/a/b.mp3?sig=1").unwrap(),
        b"MP3DATA"
    );
    assert_eq!(
        world.connected,
        [(
            "cdn.example.com".to_owned(),
            SocketAddrV4::new(PUBLIC_A.parse().unwrap(), 443)
        )]
    );
    let sent = String::from_utf8(world.requests.lock().unwrap().clone()).unwrap();
    assert!(
        sent.starts_with("GET /a/b.mp3?sig=1 HTTP/1.1\r\nHost: cdn.example.com\r\n"),
        "{sent}"
    );
    assert!(sent.contains("Connection: close") && sent.contains("Accept-Encoding: identity"));
    let lower = sent.to_ascii_lowercase();
    assert!(
        !lower.contains("cookie") && !lower.contains("authorization") && !lower.contains("referer"),
        "{sent}"
    );
}

#[test]
fn a_url_that_is_not_allowed_is_refused_before_any_lookup_or_connect() {
    for url in [
        "http://cdn.example.com/x",
        "https://evil.example.org/x",
        "https://cdn.example.com:8443/x",
        "https://user@cdn.example.com/x",
        "https://93.184.216.34/x",
        "https://[::1]/x",
        "ftp://cdn.example.com/x",
    ] {
        let mut world = World::default().name("cdn.example.com", PUBLIC_A);
        assert!(
            matches!(run(&mut world, url), Err(FetchError::Refused(_))),
            "{url}"
        );
        assert!(world.resolved.is_empty(), "{url} looked a name up");
        assert!(world.connected.is_empty(), "{url} connected");
    }
}

#[test]
fn a_name_that_resolves_to_a_private_or_mixed_answer_is_never_connected_to() {
    for address in [
        "10.0.0.5",
        "192.168.1.20",
        "127.0.0.1",
        "169.254.169.254",
        "100.64.0.9",
        "0.0.0.0",
    ] {
        let mut world = World::default()
            .name("cdn.example.com", address)
            .answer(&ok("audio/mpeg", "x"));
        assert_eq!(
            run(&mut world, "https://cdn.example.com/x"),
            Err(FetchError::Refused(Refused::Address)),
            "{address}"
        );
        assert!(world.connected.is_empty(), "{address}");
    }
    let mut world = World::default().answer(&ok("audio/mpeg", "x"));
    world.names.insert(
        "cdn.example.com".into(),
        vec![PUBLIC_A.parse().unwrap(), "10.0.0.1".parse().unwrap()],
    );
    assert_eq!(
        run(&mut world, "https://cdn.example.com/x"),
        Err(FetchError::Refused(Refused::Address))
    );
    assert!(world.connected.is_empty());
}

#[test]
fn a_redirect_to_another_named_host_is_followed_and_a_relative_one_too() {
    let mut world = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .name("files.example.net", PUBLIC_B)
        .answer(&redirect("https://files.example.net/ep/1.mp3"))
        .answer(&redirect("/ep/final.mp3"))
        .answer(&ok("audio/mpeg", "EPISODE"));
    assert_eq!(
        run(&mut world, "https://cdn.example.com/start").unwrap(),
        b"EPISODE"
    );
    let hosts: Vec<&str> = world
        .connected
        .iter()
        .map(|(host, _)| host.as_str())
        .collect();
    assert_eq!(
        hosts,
        ["cdn.example.com", "files.example.net", "files.example.net"]
    );
}

#[test]
fn a_redirect_to_a_host_not_named_or_a_bad_scheme_or_a_private_address_connects_to_nothing_new() {
    let hostile = [
        (
            "https://evil.example.org/x",
            FetchError::Refused(Refused::Host),
        ),
        (
            "http://files.example.net/x",
            FetchError::Refused(Refused::Scheme),
        ),
        (
            "https://files.example.net:9000/x",
            FetchError::Refused(Refused::Port),
        ),
        ("https://10.0.0.1/x", FetchError::Refused(Refused::Literal)),
        ("file:///etc/passwd", FetchError::Refused(Refused::Scheme)),
    ];
    for (location, why) in hostile {
        let mut world = World::default()
            .name("cdn.example.com", PUBLIC_A)
            .name("files.example.net", PUBLIC_B)
            .answer(&redirect(location))
            .answer(&ok("audio/mpeg", "SHOULD NOT BE REACHED"));
        assert_eq!(
            run(&mut world, "https://cdn.example.com/start"),
            Err(why),
            "{location}"
        );
        assert_eq!(
            world.connected.len(),
            1,
            "{location}: connected past the first hop"
        );
    }
    // A named host that a hop resolves to a private address is refused too.
    let mut world = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .name("files.example.net", "192.168.0.9")
        .answer(&redirect("https://files.example.net/x"));
    assert_eq!(
        run(&mut world, "https://cdn.example.com/start"),
        Err(FetchError::Refused(Refused::Address))
    );
    assert_eq!(world.connected.len(), 1);
}

#[test]
fn redirects_stop_at_three_and_a_loop_is_cut() {
    let mut world = World::default().name("cdn.example.com", PUBLIC_A);
    for n in 1..=5 {
        world = world.answer(&redirect(&format!("https://cdn.example.com/{n}")));
    }
    assert_eq!(
        run(&mut world, "https://cdn.example.com/0"),
        Err(FetchError::TooManyRedirects)
    );
    assert_eq!(world.connected.len(), MOST_REDIRECTS + 1);
    let mut looped = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(&redirect("https://cdn.example.com/a"))
        .answer(&redirect("https://cdn.example.com/a"));
    assert_eq!(
        run(&mut looped, "https://cdn.example.com/a"),
        Err(FetchError::TooManyRedirects)
    );
}

#[test]
fn only_a_200_with_audio_is_a_body() {
    type Case = (Vec<u8>, Result<Vec<u8>, FetchError>);
    let cases: Vec<Case> = vec![
        (ok("audio/mpeg; charset=binary", "A"), Ok(b"A".to_vec())),
        (ok("audio/mp3", "B"), Ok(b"B".to_vec())),
        (ok("application/octet-stream", "C"), Ok(b"C".to_vec())),
        (ok("text/html", "<html>"), Err(FetchError::NotAudio)),
        (ok("application/json", "{}"), Err(FetchError::NotAudio)),
        (ok("video/mp4", "V"), Err(FetchError::NotAudio)),
        (
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            Err(FetchError::NotAudio),
        ),
        (
            b"HTTP/1.1 500 Oops\r\n\r\n".to_vec(),
            Err(FetchError::NotAudio),
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 1\r\n\r\nX".to_vec(),
            Err(FetchError::NotAudio),
        ),
        (b"not http at all".to_vec(), Err(FetchError::NotAudio)),
        (
            b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            Err(FetchError::NotAudio),
        ),
        (Vec::new(), Err(FetchError::Unreachable)),
    ];
    for (answer, expected) in cases {
        let mut world = World::default()
            .name("cdn.example.com", PUBLIC_A)
            .answer(&answer);
        assert_eq!(
            run(&mut world, "https://cdn.example.com/x"),
            expected,
            "{}",
            String::from_utf8_lossy(&answer)
        );
    }
    // No Content-Type is taken as audio (a bare MP3 server).
    let mut world = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
    assert_eq!(run(&mut world, "https://cdn.example.com/x").unwrap(), b"OK");
}

#[test]
fn a_chunked_body_is_decoded_and_a_cut_one_is_an_error() {
    let mut world = World::default().name("cdn.example.com", PUBLIC_A).answer(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n",
    );
    assert_eq!(
        run(&mut world, "https://cdn.example.com/x").unwrap(),
        b"Wikipedia"
    );
    let mut cut = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\nWiki");
    assert_eq!(
        run(&mut cut, "https://cdn.example.com/x"),
        Err(FetchError::Unreachable)
    );
    let mut junk = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nWiki");
    assert_eq!(
        run(&mut junk, "https://cdn.example.com/x"),
        Err(FetchError::Unreachable)
    );
}

#[test]
fn a_body_longer_than_its_content_length_is_not_read_past_it_and_one_over_the_cap_is_refused() {
    let mut world = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcEXTRA");
    assert_eq!(
        run(&mut world, "https://cdn.example.com/x").unwrap(),
        b"abc"
    );
    let allowed = Allowed::new(&["cdn.example.com"]).unwrap();
    let mut big = World::default()
        .name("cdn.example.com", PUBLIC_A)
        .answer(b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n");
    assert!(matches!(
        get(&allowed, "https://cdn.example.com/x", 4_999, &mut big),
        Err(FetchError::TooBig)
    ));
}

#[test]
fn a_refused_first_address_is_followed_by_the_next_and_all_refused_is_unreachable() {
    let mut world = World::default().answer(&ok("audio/mpeg", "BODY"));
    world.names.insert(
        "cdn.example.com".into(),
        vec![PUBLIC_A.parse().unwrap(), PUBLIC_B.parse().unwrap()],
    );
    world.refuse.push(PUBLIC_A.parse().unwrap());
    assert_eq!(
        run(&mut world, "https://cdn.example.com/x").unwrap(),
        b"BODY"
    );
    assert_eq!(world.connected.len(), 2);
    let mut all = World::default().name("cdn.example.com", PUBLIC_A);
    all.refuse.push(PUBLIC_A.parse().unwrap());
    assert_eq!(
        run(&mut all, "https://cdn.example.com/x"),
        Err(FetchError::Unreachable)
    );
    let mut unknown = World::default();
    assert_eq!(
        run(&mut unknown, "https://cdn.example.com/x"),
        Err(FetchError::Unreachable)
    );
}

#[test]
fn the_words_for_the_app_are_fixed() {
    assert_eq!(FetchError::Refused(Refused::Host).word(), "refused_host");
    assert_eq!(FetchError::Refused(Refused::Address).word(), "refused_host");
    assert_eq!(FetchError::Unreachable.word(), "unreachable");
    assert_eq!(FetchError::Timeout.word(), "timeout");
    assert_eq!(FetchError::NotAudio.word(), "bad_audio");
    assert_eq!(FetchError::TooManyRedirects.word(), "refused_host");
    assert_eq!(FetchError::TooBig.word(), "too_big");
}
