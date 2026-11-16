//! The `net` grant's one way out (the design note on #7200, Heron's
//! answers): a run granted `net` is held like every run
//! ([`crate::skill_task::NET_ENV`] `run`: its own network namespace with
//! only `lo`, the model through [`crate::net_bridge::MODEL_SOCKET`]), and is
//! given one more socket, [`EGRESS_DIR`]`/make-<run>.sock`, for that run
//! alone (on the one run path `skill-<run>.sock`, which the wrapper finds
//! by the run's id, [`crate::skill_task::ONE_PATH_EGRESS_LINES`]).
//! lamp-tasks serves it ([`Grant::open`]) and removes it when the run
//! ends.
//!
//! - **Inside the run** ([`Inside`], `lamp-tasks net-relay --egress`,
//!   started by its namespace script): an HTTP proxy on [`PROXY_LISTEN`]
//!   (`CONNECT`, and absolute-form `http://`) and DNS on [`DNS_LISTEN`]
//!   (UDP and TCP), each connection piped to the egress socket with a
//!   first byte saying which ([`PROXY`], [`DNS_STREAM`], [`DNS_DATAGRAM`]).
//!   The relay checks nothing: the run could talk to the socket itself.
//! - **Outside** ([`Served`], in `lamp-tasks serve`): every check. A proxy
//!   request is read whole first (at most [`MOST_HEAD_BYTES`]); its port
//!   must be one of [`PORTS`]; its name is resolved here and each address
//!   checked ([`class`]), and only an address that passed is connected
//!   to, so a name that resolves somewhere private gets nothing. A closed
//!   address is never answered: the connection is closed (`curl` shows
//!   `000`). A name lookup is forwarded as it came to the lamp's own name
//!   servers ([`servers_in`], `/etc/resolv.conf`), but a single-label name
//!   and a reverse lookup in a closed range are refused ([`refusal`]).
//! - **Closed** ([`class`]): the lamp's own networks (every interface's
//!   address and prefix, every route's gateway and destination, read again
//!   at each connect, [`own_networks`]), the fixed IPv4 and IPv6 ranges,
//!   and an IPv4 address inside an IPv6 one (mapped, NAT64, 6to4) checked
//!   as IPv4.
//!
//! Bounded as [`crate::net_bridge`]'s pipes are: at most [`MOST_PIPES`]
//! at once, [`CONNECT_WITHIN`] to reach the other side, [`IDLE_WITHIN`]
//! with no byte, and nothing outlives the run's deadline. Bytes are never
//! logged; a line says fixed text and an address's class, never the
//! address or a name.
use crate::net_bridge::{Bounds, Counted, Say, pipe};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The folder of the egress sockets, in lamp-tasks' runtime folder
/// (`lampos`'s, 0700): made 0700, each socket in it 0600.
pub const EGRESS_DIR: &str = "/run/lampos-tasks/egress";
/// The variable naming a run's egress socket to the namespace script
/// (`lamp_update::hermes_install`'s `TASKS_EDITS`): the run's folder's
/// name, `make-<run id>`; the script shows [`EGRESS_DIR`]`/<name>.sock`.
pub const EGRESS_ENV: &str = "LAMP_NS_EGRESS";
/// The line lamp-update's namespace script carries when it shows a run's
/// egress socket: without it a `net` run is refused ([`NO_EGRESS`]).
pub const NS_LINE: &str = "V=/run/lampos-tasks/egress/$LAMP_NS_EGRESS.sock";
/// The line lamp-update's namespace script carries when it takes a run's
/// egress socket by the one run path's name too, `skill-<run id>`
/// ([`crate::skill_task::RUN_FOLDER_PREFIX`]): without it such a run is
/// refused ([`crate::skill_task::NET_NO_WAY_OUT`]).
pub const NS_SKILL_LINE: &str = "case $LAMP_NS_EGRESS in make-?*|skill-?*) ;;";
/// Said when the namespace script has no [`NS_LINE`]: nothing is started.
pub const NO_EGRESS: &str = "the lamp's agent can't be given the internet yet (its namespace script has no egress line: lamp-update's round hasn't run), so nothing was started";
/// The ports a run granted `net` reaches (Heron's answer 1): the web.
pub const PORTS: [u16; 2] = [80, 443];
/// The proxy inside the run.
pub const PROXY_LISTEN: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3128);
/// The name server inside the run (`resolv.conf` can't name a port).
pub const DNS_LISTEN: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53);
/// A connection's first byte: an HTTP proxy request follows.
pub const PROXY: u8 = b'P';
/// A connection's first byte: DNS over TCP framing follows (each message
/// after its two-byte length), forwarded over TCP.
pub const DNS_STREAM: u8 = b'D';
/// A connection's first byte: one framed DNS message from a datagram,
/// forwarded over UDP, its answer as it came (a truncated one too).
pub const DNS_DATAGRAM: u8 = b'U';
/// The most pipes at once, each side.
pub const MOST_PIPES: usize = 16;
/// How long reaching the other side may take.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(10);
/// How long one direction may pass no byte.
pub const IDLE_WITHIN: Duration = Duration::from_secs(60);
/// One pipe's whole life inside the run (outside, the run's deadline).
pub const PIPE_WITHIN: Duration = Duration::from_secs(700);
/// The bounds of every egress pipe.
pub const BOUNDS: Bounds = Bounds {
    most: MOST_PIPES,
    connect: CONNECT_WITHIN,
    idle: IDLE_WITHIN,
    life: PIPE_WITHIN,
};
/// The most bytes of a proxy request's head.
pub const MOST_HEAD_BYTES: usize = 8 * 1024;
/// The most bytes of a DNS query.
pub const MOST_QUERY_BYTES: usize = 4096;
/// How long one name server is waited for.
pub const DNS_WITHIN: Duration = Duration::from_secs(3);
/// The most name servers asked, as the C library's.
pub const MOST_SERVERS: usize = 3;
/// What a `CONNECT` that reached its address is answered.
pub const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\n";
/// The class [`class`] gives an address of the lamp's own networks.
pub const LAMP_NETWORK: &str = "lamp-network";

/// An address prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl Net {
    /// `addr`/`prefix`, an IPv4 address inside an IPv6 one as IPv4
    /// ([`as_v4`]), the prefix at most the address's bits.
    pub fn new(addr: IpAddr, prefix: u8) -> Self {
        match (addr, as_v4(addr)) {
            (IpAddr::V6(_), IpAddr::V4(v4)) => Self {
                addr: IpAddr::V4(v4),
                prefix: prefix.saturating_sub(96).min(32),
            },
            (_, addr @ IpAddr::V4(_)) => Self {
                addr,
                prefix: prefix.min(32),
            },
            (_, addr) => Self {
                addr,
                prefix: prefix.min(128),
            },
        }
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, as_v4(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// `ip`, or the IPv4 address inside it: v4-mapped (`::ffff:0:0/96`),
/// NAT64 (`64:ff9b::/96`, `64:ff9b:1::/48`) and 6to4 (`2002::/16`).
pub fn as_v4(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = ip else { return ip };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let s = v6.segments();
    let low = |a: u16, b: u16| IpAddr::V4(Ipv4Addr::from((u32::from(a) << 16) | u32::from(b)));
    match s {
        [0x64, 0xff9b, 0, 0, 0, 0, a, b] | [0x64, 0xff9b, 1, _, _, _, a, b] => low(a, b),
        [0x2002, a, b, ..] => low(a, b),
        _ => ip,
    }
}

/// The fixed IPv4 ranges a run never reaches, with their class.
const V4_CLOSED: [(Ipv4Addr, u8, &str); 14] = [
    (Ipv4Addr::new(0, 0, 0, 0), 8, "this-network"),
    (Ipv4Addr::new(10, 0, 0, 0), 8, "private"),
    (Ipv4Addr::new(100, 64, 0, 0), 10, "shared"),
    (Ipv4Addr::new(127, 0, 0, 0), 8, "loopback"),
    (Ipv4Addr::new(169, 254, 0, 0), 16, "link-local"),
    (Ipv4Addr::new(172, 16, 0, 0), 12, "private"),
    (Ipv4Addr::new(192, 0, 0, 0), 24, "protocol"),
    (Ipv4Addr::new(192, 0, 2, 0), 24, "documentation"),
    (Ipv4Addr::new(198, 51, 100, 0), 24, "documentation"),
    (Ipv4Addr::new(203, 0, 113, 0), 24, "documentation"),
    (Ipv4Addr::new(192, 168, 0, 0), 16, "private"),
    (Ipv4Addr::new(198, 18, 0, 0), 15, "benchmarking"),
    (Ipv4Addr::new(224, 0, 0, 0), 4, "multicast"),
    (Ipv4Addr::new(240, 0, 0, 0), 4, "reserved"),
];

/// The fixed IPv6 ranges a run never reaches, with their class: `::` and
/// `::1` inside the IPv4-compatible `::/96`.
const V6_CLOSED: [(Ipv6Addr, u8, &str); 8] = [
    (Ipv6Addr::UNSPECIFIED, 128, "unspecified"),
    (Ipv6Addr::LOCALHOST, 128, "loopback"),
    (Ipv6Addr::UNSPECIFIED, 96, "reserved"),
    (
        Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0),
        7,
        "unique-local",
    ),
    (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10, "link-local"),
    (Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0), 10, "site-local"),
    (Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8, "multicast"),
    (
        Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0),
        32,
        "documentation",
    ),
];

/// Whether `ip` is closed to a run, and its class: in one of `own` (the
/// lamp's own networks, [`LAMP_NETWORK`]) or a fixed range. `None`: a
/// public address.
pub fn class(ip: IpAddr, own: &[Net]) -> Option<&'static str> {
    let ip = as_v4(ip);
    if own.iter().any(|net| net.contains(ip)) {
        return Some(LAMP_NETWORK);
    }
    match ip {
        IpAddr::V4(_) => V4_CLOSED
            .iter()
            .find(|(addr, prefix, _)| Net::new(IpAddr::V4(*addr), *prefix).contains(ip))
            .map(|(_, _, class)| *class),
        IpAddr::V6(_) => V6_CLOSED
            .iter()
            .find(|(addr, prefix, _)| Net::new(IpAddr::V6(*addr), *prefix).contains(ip))
            .map(|(_, _, class)| *class),
    }
}

/// The networks in `/proc/net/route`'s text: each route's gateway, and
/// each destination but the default route's.
pub fn route_networks(text: &str) -> Vec<Net> {
    let hex = |field: &str| u32::from_str_radix(field, 16).ok();
    // The kernel prints each address as the u32 in memory: its bytes in
    // network order are the native bytes.
    let addr = |raw: u32| IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes()));
    let mut nets = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(dest), Some(gate), Some(mask)) = (
            fields.get(1).and_then(|f| hex(f)),
            fields.get(2).and_then(|f| hex(f)),
            fields.get(7).and_then(|f| hex(f)),
        ) else {
            continue;
        };
        if gate != 0 {
            nets.push(Net::new(addr(gate), 32));
        }
        if mask != 0 {
            let prefix = u32::from_be_bytes(mask.to_ne_bytes()).leading_ones() as u8;
            nets.push(Net::new(addr(dest), prefix));
        }
    }
    nets
}

/// The lamp's own networks now: every interface's address (and its
/// point-to-point peer) alone and with its prefix, and
/// [`route_networks`]. An error: they are not known, so nothing passes.
pub fn own_networks() -> io::Result<Vec<Net>> {
    let ip_of = |addr: &nix::sys::socket::SockaddrStorage| {
        addr.as_sockaddr_in()
            .map(|v4| IpAddr::V4(v4.ip()))
            .or_else(|| addr.as_sockaddr_in6().map(|v6| IpAddr::V6(v6.ip())))
    };
    let mut nets = Vec::new();
    for found in nix::ifaddrs::getifaddrs().map_err(io::Error::from)? {
        let Some(addr) = found.address.as_ref().and_then(ip_of) else {
            continue;
        };
        let full = if addr.is_ipv4() { 32 } else { 128 };
        nets.push(Net::new(addr, full));
        let prefix = match found.netmask.as_ref().and_then(ip_of) {
            Some(IpAddr::V4(mask)) => u32::from(mask).leading_ones() as u8,
            Some(IpAddr::V6(mask)) => u128::from(mask).leading_ones() as u8,
            None => full,
        };
        nets.push(Net::new(addr, prefix));
        if let Some(peer) = found.destination.as_ref().and_then(ip_of) {
            nets.push(Net::new(peer, full));
        }
    }
    nets.extend(route_networks(
        &std::fs::read_to_string("/proc/net/route").unwrap_or_default(),
    ));
    Ok(nets)
}

/// The lamp's check: [`class`] against [`own_networks`] read now; they
/// can't be read, every address is closed.
pub fn lamp_closed(ip: IpAddr) -> Option<&'static str> {
    match own_networks() {
        Ok(own) => class(ip, &own),
        Err(_) => Some("unknown-lamp-network"),
    }
}

/// The name servers a `resolv.conf` text names, on port 53, at most
/// [`MOST_SERVERS`].
pub fn servers_in(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("nameserver")).then_some(())?;
            let addr = words.next()?;
            let addr = addr.split('%').next()?.parse::<IpAddr>().ok()?;
            Some(SocketAddr::new(addr, 53))
        })
        .take(MOST_SERVERS)
        .collect()
}

/// Whether an address is closed, and its class.
pub type Closed = Arc<dyn Fn(IpAddr) -> Option<&'static str> + Send + Sync>;
/// A name's addresses for a port, within a time.
pub type Resolve = Arc<dyn Fn(&str, u16, Duration) -> io::Result<Vec<SocketAddr>> + Send + Sync>;
/// The name servers to forward to.
pub type Servers = Arc<dyn Fn() -> Vec<SocketAddr> + Send + Sync>;

/// What the egress checks, each a hook so tests reach a local server.
#[derive(Clone)]
pub struct Policy {
    pub ports: Vec<u16>,
    pub closed: Closed,
    pub resolve: Resolve,
    pub servers: Servers,
}

impl Policy {
    /// The lamp's: [`PORTS`], [`lamp_closed`], the C library's resolver
    /// (waited for within the connect bound) and `/etc/resolv.conf`'s name
    /// servers, read at each query.
    pub fn lamp() -> Self {
        Self {
            ports: PORTS.to_vec(),
            closed: Arc::new(lamp_closed),
            resolve: Arc::new(resolve_within),
            servers: Arc::new(|| {
                servers_in(&std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default())
            }),
        }
    }
}

/// `host`'s addresses for `port` by the C library, waited for within
/// `wait` (a lookup still running then is left to end on its own thread).
fn resolve_within(host: &str, port: u16, wait: Duration) -> io::Result<Vec<SocketAddr>> {
    use std::net::ToSocketAddrs;
    let (tx, rx) = std::sync::mpsc::channel();
    let host = host.to_owned();
    std::thread::Builder::new()
        .name("tasks-egress-name".into())
        .spawn(move || {
            let _ = tx.send((host.as_str(), port).to_socket_addrs().map(Vec::from_iter));
        })?;
    rx.recv_timeout(wait)
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

/// A host as a request names it: an IP literal, or a name of letters,
/// digits, `-` and `.`, lowercased, without its last dot.
fn plain_host(host: &str) -> Option<String> {
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    let name = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    (name || host.parse::<IpAddr>().is_ok()).then_some(host)
}

/// `host[:port]` or `[v6][:port]`, the port `default` when none; never
/// with user info.
fn authority(text: &str, default: Option<u16>) -> Option<(String, u16)> {
    if text.contains('@') {
        return None;
    }
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']')?;
            host.parse::<Ipv6Addr>().ok()?;
            let port = match after {
                "" => default?,
                after => after.strip_prefix(':')?.parse().ok()?,
            };
            (host, port)
        }
        None => match text.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (text, default?),
        },
    };
    Some((plain_host(host)?, port))
}

/// A proxy request read whole: where it goes, and for absolute-form
/// `http://` the head to send there (origin-form, `Connection: close`,
/// no `Proxy-*` header).
#[derive(Debug, PartialEq, Eq)]
pub struct Ask {
    pub tunnel: bool,
    pub host: String,
    pub port: u16,
    pub head: Vec<u8>,
}

impl Ask {
    /// The request in `head` (up to and with its blank line), or `None`.
    pub fn parse(head: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(head).ok()?;
        let mut lines = text.split("\r\n");
        let mut first = lines.next()?.split(' ');
        let (method, target, version) = (first.next()?, first.next()?, first.next()?);
        if first.next().is_some()
            || !version.starts_with("HTTP/1.")
            || method.is_empty()
            || !method.bytes().all(|b| b.is_ascii_uppercase())
        {
            return None;
        }
        if method == "CONNECT" {
            let (host, port) = authority(target, None)?;
            return Some(Self {
                tunnel: true,
                host,
                port,
                head: Vec::new(),
            });
        }
        let scheme = target.get(..7)?;
        if !scheme.eq_ignore_ascii_case("http://") {
            return None;
        }
        let rest = &target[7..];
        let (auth, path) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
        let (host, port) = authority(auth, Some(80))?;
        let path = match path {
            "" => "/".to_owned(),
            path if path.starts_with('?') => format!("/{path}"),
            path => path.to_owned(),
        };
        let mut out = format!("{method} {path} {version}\r\n");
        let mut has_host = false;
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, _) = line.split_once(':')?;
            let name = name.trim();
            if [
                "proxy-connection",
                "proxy-authorization",
                "connection",
                "keep-alive",
            ]
            .iter()
            .any(|drop| name.eq_ignore_ascii_case(drop))
            {
                continue;
            }
            has_host |= name.eq_ignore_ascii_case("host");
            out.push_str(line);
            out.push_str("\r\n");
        }
        if !has_host {
            out.push_str(&format!("Host: {auth}\r\n"));
        }
        out.push_str("Connection: close\r\n\r\n");
        Some(Self {
            tunnel: false,
            host,
            port,
            head: out.into_bytes(),
        })
    }
}

/// A request's head (through its blank line) and what came after it in
/// the same reads, at most [`MOST_HEAD_BYTES`].
fn read_head(from: &mut impl Read) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut got = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let read = from.read(&mut chunk).ok().filter(|read| *read > 0)?;
        let seen = got.len().saturating_sub(3);
        got.extend_from_slice(&chunk[..read]);
        if let Some(at) = got[seen..].windows(4).position(|w| w == b"\r\n\r\n") {
            let end = seen + at + 4;
            if end > MOST_HEAD_BYTES {
                return None;
            }
            let rest = got.split_off(end);
            return Some((got, rest));
        }
        if got.len() > MOST_HEAD_BYTES {
            return None;
        }
    }
}

/// A connection to `host`:`port` if the port is one of the policy's and
/// an address of the name passes the check; that exact address is the one
/// connected to. Each refusal said in fixed text and the class only.
pub fn open(
    policy: &Policy,
    host: &str,
    port: u16,
    within: Duration,
    say: &Say,
) -> Option<TcpStream> {
    if !policy.ports.contains(&port) {
        say("[lamp-tasks] egress: closed a connection to a port other than 80 and 443");
        return None;
    }
    let began = Instant::now();
    let addrs = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) if !host.contains('.') => {
            say("[lamp-tasks] egress: closed a connection to a single-label name");
            return None;
        }
        Err(_) => match (policy.resolve)(host, port, within) {
            Ok(addrs) if !addrs.is_empty() => addrs,
            _ => {
                say("[lamp-tasks] egress: a name did not resolve");
                return None;
            }
        },
    };
    let mut closed = None;
    for addr in addrs.into_iter().take(8) {
        let addr = SocketAddr::new(addr.ip(), port);
        if let Some(class) = (policy.closed)(addr.ip()) {
            closed = Some(class);
            continue;
        }
        let left = within.saturating_sub(began.elapsed());
        if left.is_zero() {
            break;
        }
        if let Ok(stream) = TcpStream::connect_timeout(&addr, left) {
            return Some(stream);
        }
    }
    match closed {
        Some(class) => say(&format!(
            "[lamp-tasks] egress: closed a connection to an address of class {class}"
        )),
        None => say("[lamp-tasks] egress: the other side did not answer"),
    }
    None
}

/// One proxy connection: its request read, its place opened, then piped
/// both ways until `until`.
fn proxy(mut client: UnixStream, policy: &Policy, bounds: Bounds, until: Instant, say: &Say) {
    let Some((head, rest)) = read_head(&mut client) else {
        return;
    };
    let Some(ask) = Ask::parse(&head) else {
        say("[lamp-tasks] egress: closed a connection that was not a proxy request");
        return;
    };
    let within = until
        .saturating_duration_since(Instant::now())
        .min(bounds.connect);
    let Some(mut upstream) = open(policy, &ask.host, ask.port, within, say) else {
        return;
    };
    let sent = if ask.tunnel {
        client.write_all(ESTABLISHED)
    } else {
        upstream.write_all(&ask.head)
    };
    if sent.and_then(|()| upstream.write_all(&rest)).is_err() {
        return;
    }
    let life = until.saturating_duration_since(Instant::now());
    if !life.is_zero() {
        pipe(
            Box::new(client),
            Box::new(upstream),
            Bounds { life, ..bounds },
        );
    }
}

/// A DNS query's question: its labels (lowercased) and type, and where it
/// ends.
#[derive(Debug, PartialEq, Eq)]
pub struct Question {
    pub labels: Vec<String>,
    pub qtype: u16,
    pub end: usize,
}

/// The type of a reverse lookup.
pub const PTR: u16 = 12;
const FORMERR: u8 = 1;
const SERVFAIL: u8 = 2;
/// The answer code of a refused lookup.
pub const REFUSED: u8 = 5;

/// `message`'s one question, if it is a query with exactly one, its name
/// uncompressed.
pub fn question(message: &[u8]) -> Option<Question> {
    if message.len() < 12 || message[2] & 0x80 != 0 || message[4..6] != [0, 1] {
        return None;
    }
    let (mut at, mut labels, mut total) = (12, Vec::new(), 0);
    loop {
        let len = usize::from(*message.get(at)?);
        at += 1;
        if len == 0 {
            break;
        }
        if len & 0xc0 != 0 {
            return None;
        }
        total += len + 1;
        if total > 255 {
            return None;
        }
        let label = message.get(at..at + len)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        at += len;
    }
    let qtype = u16::from_be_bytes(message.get(at..at + 2)?.try_into().ok()?);
    message.get(at + 2..at + 4)?;
    Some(Question {
        labels,
        qtype,
        end: at + 4,
    })
}

/// The address a reverse name names, whole (`d.c.b.a.in-addr.arpa`, or
/// 32 nibbles then `ip6.arpa`), or `None`.
fn reverse_ip(labels: &[String]) -> Option<IpAddr> {
    let (head, tail) = labels.split_at(labels.len().checked_sub(2)?);
    match tail {
        [a, b] if a == "in-addr" && b == "arpa" && head.len() == 4 => {
            let mut octets = [0u8; 4];
            for (at, label) in head.iter().rev().enumerate() {
                octets[at] = label.parse().ok()?;
            }
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        [a, b] if a == "ip6" && b == "arpa" && head.len() == 32 => {
            let mut value = 0u128;
            for label in head.iter().rev() {
                if label.len() != 1 {
                    return None;
                }
                value = (value << 4) | u128::from(u8::from_str_radix(label, 16).ok()?);
            }
            Some(IpAddr::V6(Ipv6Addr::from(value)))
        }
        _ => None,
    }
}

/// Why a lookup is refused (Heron's answer 2), in fixed text: a
/// single-label name (`printer`, `nas`), or a name under the reverse
/// zones that isn't a whole public address. `None`: forwarded.
pub fn refusal(question: &Question, closed: &Closed) -> Option<&'static str> {
    let n = question.labels.len();
    if n <= 1 {
        return Some("a single-label name");
    }
    let reverse = question.labels[n - 1] == "arpa"
        && (question.labels[n - 2] == "in-addr" || question.labels[n - 2] == "ip6");
    if reverse {
        return match reverse_ip(&question.labels) {
            Some(ip) if closed(ip).is_none() => None,
            _ => Some("a reverse lookup in a closed range"),
        };
    }
    None
}

/// `query`'s header and its first `end` bytes, answered with `code` and
/// nothing else.
fn coded(query: &[u8], end: usize, code: u8) -> Vec<u8> {
    let mut reply = query[..end.min(query.len())].to_vec();
    reply[2] = 0x80 | (query[2] & 0x79);
    reply[3] = 0x80 | code;
    if end <= 12 {
        reply[4..6].fill(0);
    }
    reply[6..12].fill(0);
    reply
}

/// Whether `reply` answers `query`: a response with its id.
fn answers(query: &[u8], reply: &[u8]) -> bool {
    reply.len() >= 12 && reply[..2] == query[..2] && reply[2] & 0x80 != 0
}

fn ask_udp(server: SocketAddr, query: &[u8]) -> io::Result<Vec<u8>> {
    let local: SocketAddr = if server.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(local)?;
    socket.connect(server)?;
    socket.send(query)?;
    let until = Instant::now() + DNS_WITHIN;
    let mut reply = vec![0u8; 65_535];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        socket.set_read_timeout(Some(left))?;
        let read = socket.recv(&mut reply)?;
        if answers(query, &reply[..read]) {
            reply.truncate(read);
            return Ok(reply);
        }
    }
}

fn read_framed(from: &mut impl Read, most: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    from.read_exact(&mut len)?;
    let len = usize::from(u16::from_be_bytes(len));
    if len == 0 || len > most {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut message = vec![0u8; len];
    from.read_exact(&mut message)?;
    Ok(message)
}

fn write_framed(to: &mut impl Write, message: &[u8]) -> io::Result<()> {
    let len =
        u16::try_from(message.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    let mut framed = Vec::with_capacity(message.len() + 2);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(message);
    to.write_all(&framed)
}

fn ask_tcp(server: SocketAddr, query: &[u8]) -> io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect_timeout(&server, DNS_WITHIN)?;
    stream.set_read_timeout(Some(DNS_WITHIN))?;
    stream.set_write_timeout(Some(DNS_WITHIN))?;
    write_framed(&mut stream, query)?;
    let reply = read_framed(&mut stream, 65_535)?;
    if answers(query, &reply) {
        Ok(reply)
    } else {
        Err(io::ErrorKind::InvalidData.into())
    }
}

/// `query` answered: refused ([`refusal`]), malformed, or forwarded to the
/// policy's name servers (over TCP for a `stream`), the first answer; none
/// answering, a server failure. `None`: not a DNS message at all.
pub fn answer(query: &[u8], policy: &Policy, stream: bool, say: &Say) -> Option<Vec<u8>> {
    let Some(question) = question(query) else {
        return (query.len() >= 12).then(|| coded(query, 12, FORMERR));
    };
    if let Some(why) = refusal(&question, &policy.closed) {
        say(&format!(
            "[lamp-tasks] egress: refused a name lookup: {why}"
        ));
        return Some(coded(query, question.end, REFUSED));
    }
    for server in (policy.servers)().into_iter().take(MOST_SERVERS) {
        let asked = if stream {
            ask_tcp(server, query)
        } else {
            ask_udp(server, query)
        };
        if let Ok(reply) = asked {
            return Some(reply);
        }
    }
    say("[lamp-tasks] egress: no name server answered");
    Some(coded(query, question.end, SERVFAIL))
}

/// Framed DNS messages answered on `client` until it ends, goes idle or
/// `until`; a datagram's (`!stream`) one only.
fn dns(mut client: UnixStream, policy: &Policy, stream: bool, until: Instant, say: &Say) {
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero()
            || client
                .set_read_timeout(Some(left.min(IDLE_WITHIN)))
                .is_err()
        {
            return;
        }
        let Ok(query) = read_framed(&mut client, MOST_QUERY_BYTES) else {
            return;
        };
        let Some(reply) = answer(&query, policy, stream, say) else {
            return;
        };
        if write_framed(&mut client, &reply).is_err() || !stream {
            return;
        }
    }
}

/// One egress connection: its first byte, then the proxy or DNS.
fn handle(mut client: UnixStream, policy: &Policy, bounds: Bounds, until: Instant, say: &Say) {
    let set = client
        .set_nonblocking(false)
        .and_then(|()| client.set_read_timeout(Some(bounds.connect)))
        .and_then(|()| client.set_write_timeout(Some(bounds.idle)));
    let mut mode = [0u8];
    if set.is_err() || client.read_exact(&mut mode).is_err() {
        return;
    }
    match mode[0] {
        PROXY => proxy(client, policy, bounds, until, say),
        DNS_STREAM => dns(client, policy, true, until, say),
        DNS_DATAGRAM => dns(client, policy, false, until, say),
        _ => say("[lamp-tasks] egress: closed a connection of no known kind"),
    }
}

/// A run's egress: its folder, what it checks, its bounds and its words.
#[derive(Clone)]
pub struct Grant {
    pub dir: PathBuf,
    pub policy: Policy,
    pub bounds: Bounds,
    pub say: Say,
}

/// Whether `name` is a run's folder's, `make-<run id>`, or a one run path
/// run's, `skill-<run id>`: plain, at most 80 bytes, as the namespace
/// script takes it.
pub fn plain_name(name: &str) -> bool {
    [
        crate::skill_make::FOLDER_HEAD,
        crate::skill_task::RUN_FOLDER_PREFIX,
    ]
    .iter()
    .any(|head| name.strip_prefix(head).is_some_and(|id| !id.is_empty()))
        && name.len() <= 80
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Grant {
    /// The lamp's: [`EGRESS_DIR`], [`Policy::lamp`], [`BOUNDS`], said on
    /// lamp-tasks' journal.
    pub fn lamp() -> Self {
        Self {
            dir: PathBuf::from(EGRESS_DIR),
            policy: Policy::lamp(),
            bounds: BOUNDS,
            say: Arc::new(|line| eprintln!("{line}")),
        }
    }

    /// The socket of the run whose folder is `name`.
    pub fn socket(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.sock"))
    }

    /// `name`'s socket made (its folder 0700 and ours, the socket 0600;
    /// any other run's socket left there swept: one runs at a time) and
    /// served on a thread of its own until the [`Served`] is dropped or
    /// `until` passes, the socket removed then.
    pub fn open(&self, name: &str, until: Instant) -> io::Result<Served> {
        let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_owned());
        if !plain_name(name) {
            return Err(invalid("not a run's folder name"));
        }
        match std::fs::symlink_metadata(&self.dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::DirBuilder::new().mode(0o700).create(&self.dir)?;
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        let folder = std::fs::symlink_metadata(&self.dir)?;
        if !folder.is_dir()
            || folder.permissions().mode() & 0o077 != 0
            || folder.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(invalid("the egress folder is not a private folder of ours"));
        }
        sweep(&self.dir);
        let path = self.socket(name);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let meta = std::fs::symlink_metadata(&path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stopped, grant, at) = (stop.clone(), self.clone(), path.clone());
        let (dev, ino) = (meta.dev(), meta.ino());
        let thread = std::thread::Builder::new()
            .name("tasks-egress".into())
            .spawn(move || {
                serve(&listener, &grant, until, &stopped);
                drop(listener);
                remove_socket(&at, dev, ino);
            })?;
        Ok(Served {
            path,
            dev,
            ino,
            stop,
            thread: Some(thread),
        })
    }
}

/// Every socket in `dir` removed: a run that ended with lamp-tasks.
fn sweep(dir: &Path) {
    use std::os::unix::fs::FileTypeExt;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_socket()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn remove_socket(path: &Path, dev: u64, ino: u64) {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.dev() == dev && m.ino() == ino) {
        let _ = std::fs::remove_file(path);
    }
}

fn serve(listener: &UnixListener, grant: &Grant, until: Instant, stop: &AtomicBool) {
    let open = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Acquire) && Instant::now() < until {
        let client = match listener.accept() {
            Ok((client, _)) => client,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(crate::net_bridge::ACCEPT_TICK);
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                (grant.say)("[lamp-tasks] egress: a connection was not accepted");
                std::thread::sleep(crate::net_bridge::ACCEPT_TICK);
                continue;
            }
        };
        if open.fetch_add(1, Ordering::AcqRel) >= grant.bounds.most {
            open.fetch_sub(1, Ordering::AcqRel);
            (grant.say)("[lamp-tasks] egress: closed a connection past the most at once");
            continue;
        }
        let held = Counted(open.clone());
        let grant = grant.clone();
        let _ = std::thread::Builder::new()
            .name("tasks-egress-pipe".into())
            .spawn(move || {
                let _held = held;
                handle(client, &grant.policy, grant.bounds, until, &grant.say);
            });
    }
}

/// A run's egress socket while it is served: dropped, it is stopped and
/// removed.
pub struct Served {
    path: PathBuf,
    dev: u64,
    ino: u64,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Served {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        remove_socket(&self.path, self.dev, self.ino);
    }
}

/// The relay's listeners inside a run granted `net`: the proxy and the
/// name server, TCP and UDP.
pub struct Inside {
    pub proxy: TcpListener,
    pub dns_tcp: TcpListener,
    pub dns_udp: UdpSocket,
}

impl Inside {
    /// Each bound, or why not (the run is then refused: no `ready`).
    pub fn bind(proxy: SocketAddr, dns: SocketAddr) -> Result<Self, String> {
        let at = |addr: SocketAddr| move |error: io::Error| format!("{addr}: {error}");
        Ok(Self {
            proxy: TcpListener::bind(proxy).map_err(at(proxy))?,
            dns_tcp: TcpListener::bind(dns).map_err(at(dns))?,
            dns_udp: UdpSocket::bind(dns).map_err(at(dns))?,
        })
    }

    /// Each served on a thread of its own, each connection (and each
    /// datagram) piped to `to` after its first byte, until `stop`.
    pub fn serve(
        self,
        to: &Path,
        bounds: Bounds,
        stop: &Arc<AtomicBool>,
        say: &Say,
    ) -> io::Result<()> {
        for (listener, mode) in [(self.proxy, PROXY), (self.dns_tcp, DNS_STREAM)] {
            let (to, stop, say) = (to.to_path_buf(), stop.clone(), say.clone());
            std::thread::Builder::new()
                .name("tasks-egress-relay".into())
                .spawn(move || {
                    crate::net_bridge::serve(
                        || Ok(Box::new(listener.accept()?.0) as Box<dyn crate::net_bridge::End>),
                        Arc::new(move |_wait| {
                            let mut stream = UnixStream::connect(&to)?;
                            stream.write_all(&[mode])?;
                            Ok(Box::new(stream) as Box<dyn crate::net_bridge::End>)
                        }),
                        bounds,
                        &stop,
                        &say,
                        "egress relay",
                    );
                })?;
        }
        let (socket, to, stop) = (self.dns_udp, to.to_path_buf(), stop.clone());
        std::thread::Builder::new()
            .name("tasks-egress-dns".into())
            .spawn(move || serve_datagrams(&socket, &to, bounds, &stop))?;
        Ok(())
    }
}

/// Each datagram on `socket` asked through `to` on a thread of its own, at
/// most `bounds.most` at once (one more dropped), its answer sent back.
fn serve_datagrams(socket: &UdpSocket, to: &Path, bounds: Bounds, stop: &AtomicBool) {
    let open = Arc::new(AtomicUsize::new(0));
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let mut got = vec![0u8; MOST_QUERY_BYTES];
    while !stop.load(Ordering::Acquire) {
        let (read, from) = match socket.recv_from(&mut got) {
            Ok(received) => received,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => {
                std::thread::sleep(crate::net_bridge::ACCEPT_TICK);
                continue;
            }
        };
        if open.fetch_add(1, Ordering::AcqRel) >= bounds.most {
            open.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let held = Counted(open.clone());
        let (Ok(back), query, to) = (socket.try_clone(), got[..read].to_vec(), to.to_path_buf())
        else {
            continue;
        };
        let _ = std::thread::Builder::new()
            .name("tasks-egress-ask".into())
            .spawn(move || {
                let _held = held;
                if let Ok(reply) = ask_framed(&to, DNS_DATAGRAM, &query, bounds.connect) {
                    let _ = back.send_to(&reply, from);
                }
            });
    }
}

/// `query` sent through the socket `to` after `mode`, its framed answer.
fn ask_framed(to: &Path, mode: u8, query: &[u8], within: Duration) -> io::Result<Vec<u8>> {
    let mut stream = UnixStream::connect(to)?;
    stream.set_read_timeout(Some(within))?;
    stream.set_write_timeout(Some(within))?;
    stream.write_all(&[mode])?;
    write_framed(&mut stream, query)?;
    read_framed(&mut stream, 65_535)
}

#[cfg(test)]
mod tests;
