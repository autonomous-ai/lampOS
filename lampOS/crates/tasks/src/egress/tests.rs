use super::*;
use std::sync::Mutex;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

/// The office's network, which the usual private ranges don't cover.
fn office() -> Vec<Net> {
    vec![
        Net::new(ip("172.168.20.0"), 24),
        Net::new(ip("172.168.20.1"), 32),
    ]
}

/// Every closed class closed, the lamp's own interface prefix closed
/// whatever its range, and public addresses open, IPv4 inside IPv6 checked
/// as IPv4.
#[test]
fn every_closed_class_is_closed_and_a_public_address_is_open() {
    let own = office();
    for (addr, wanted) in [
        ("0.1.2.3", "this-network"),
        ("10.0.0.1", "private"),
        ("100.64.0.1", "shared"),
        ("100.127.255.254", "shared"),
        ("127.0.0.1", "loopback"),
        ("127.255.0.9", "loopback"),
        ("169.254.169.254", "link-local"),
        ("172.16.0.1", "private"),
        ("172.31.255.255", "private"),
        ("192.0.0.8", "protocol"),
        ("192.0.2.1", "documentation"),
        ("198.51.100.7", "documentation"),
        ("203.0.113.9", "documentation"),
        ("192.168.1.1", "private"),
        ("198.18.0.1", "benchmarking"),
        ("198.19.255.1", "benchmarking"),
        ("224.0.0.251", "multicast"),
        ("239.255.255.250", "multicast"),
        ("240.0.0.1", "reserved"),
        ("255.255.255.255", "reserved"),
        ("::", "unspecified"),
        ("::1", "loopback"),
        ("::127.0.0.1", "reserved"),
        ("fc00::1", "unique-local"),
        ("fd12:3456::1", "unique-local"),
        ("fe80::1", "link-local"),
        ("ff02::1", "multicast"),
        ("2001:db8::1", "documentation"),
        // The lamp's own network, outside every fixed range.
        ("172.168.20.1", LAMP_NETWORK),
        ("172.168.20.57", LAMP_NETWORK),
        // IPv4 inside IPv6: mapped, NAT64 and 6to4.
        ("::ffff:10.0.0.1", "private"),
        ("::ffff:172.168.20.9", LAMP_NETWORK),
        ("64:ff9b::a00:1", "private"),
        ("64:ff9b:1::c0a8:101", "private"),
        ("64:ff9b::7f00:1", "loopback"),
        ("2002:c0a8:0101::1", "private"),
    ] {
        assert_eq!(class(ip(addr), &own), Some(wanted), "{addr}");
    }
    for addr in [
        "93.184.215.14",
        "1.1.1.1",
        "8.8.8.8",
        "172.15.255.255",
        "172.32.0.1",
        "172.168.21.1",
        "100.128.0.1",
        "198.20.0.1",
        "2606:4700:4700::1111",
        "::ffff:1.1.1.1",
        "64:ff9b::101:101",
    ] {
        assert_eq!(class(ip(addr), &own), None, "{addr}");
    }
    // Without the office given as the lamp's own, it is public: the
    // interfaces decide, not a list.
    assert_eq!(class(ip("172.168.20.57"), &[]), None);
}

#[test]
fn the_routes_give_each_gateway_and_each_network_but_the_default() {
    // eth0 on 172.168.20.0/24 with its gateway 172.168.20.1, as the
    // kernel prints it on a little-endian lamp; a static route via it.
    let text = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                eth0\t00000000\t0114A8AC\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                eth0\t0014A8AC\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n\
                wlan0\t0000000A\t0114A8AC\t0003\t0\t0\t600\t000000FF\t0\t0\t0\n";
    let nets = route_networks(text);
    if cfg!(target_endian = "little") {
        assert!(nets.contains(&Net::new(ip("172.168.20.1"), 32)), "{nets:?}");
        assert!(nets.contains(&Net::new(ip("172.168.20.0"), 24)), "{nets:?}");
        assert!(nets.contains(&Net::new(ip("10.0.0.0"), 8)), "{nets:?}");
    }
    assert!(nets.iter().all(|net| net.prefix != 0), "never the default");
    assert_eq!(route_networks(""), vec![]);
    assert_eq!(route_networks("Iface\nbad line\n"), vec![]);
}

/// The lamp's own interfaces are read: loopback is among them on any
/// machine, so a run never reaches it even with no fixed range.
#[test]
fn the_lamps_interfaces_are_read() {
    let own = own_networks().unwrap();
    assert!(
        own.iter().any(|net| net.contains(ip("127.0.0.1"))),
        "{own:?}"
    );
    assert_eq!(lamp_closed(ip("127.0.0.1")), Some(LAMP_NETWORK));
    assert!(lamp_closed(ip("10.1.2.3")).is_some());
}

#[test]
fn the_name_servers_are_the_lamps_resolv_conf_ones() {
    let text = "# openresolv\nnameserver 172.168.20.1\nnameserver 1.1.1.1\n\
                search lan\nnameserver fe80::1%wlan0\nnameserver 8.8.8.8\n";
    assert_eq!(
        servers_in(text),
        vec![
            "172.168.20.1:53".parse().unwrap(),
            "1.1.1.1:53".parse().unwrap(),
            "[fe80::1]:53".parse().unwrap(),
        ]
    );
}

fn heard() -> (Say, Arc<Mutex<Vec<String>>>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let kept = lines.clone();
    (
        Arc::new(move |line: &str| kept.lock().unwrap().push(line.to_owned())),
        lines,
    )
}

/// A web server on loopback, port `port`: answers each request with its
/// head echoed back in a body, then closes.
fn web() -> (u16, std::sync::mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let tx = tx.clone();
            std::thread::spawn(move || {
                let Some((head, _)) = read_head(&mut stream) else {
                    return;
                };
                let _ = tx.send(head);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            });
        }
    });
    (port, rx)
}

/// A test policy: `port` alone, loopback open (the test's "public"),
/// 10.9.9.9 and 172.168.20.1 closed, and names resolved from a map.
fn policy(port: u16, servers: Vec<SocketAddr>) -> Policy {
    Policy {
        ports: vec![port],
        closed: Arc::new(|addr: IpAddr| match addr.to_string().as_str() {
            "127.0.0.1" => None,
            "10.9.9.9" => Some("private"),
            _ => Some(LAMP_NETWORK),
        }),
        resolve: Arc::new(|host: &str, port: u16, _| {
            let addrs: &[&str] = match host {
                "public.example" => &["127.0.0.1"],
                "rebind.example" => &["10.9.9.9"],
                "mixed.example" => &["10.9.9.9", "172.168.20.1", "127.0.0.1"],
                _ => return Err(io::ErrorKind::NotFound.into()),
            };
            Ok(addrs
                .iter()
                .map(|addr| SocketAddr::new(addr.parse().unwrap(), port))
                .collect())
        }),
        servers: Arc::new(move || servers.clone()),
    }
}

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("egr-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn grant(dir: &Path, policy: Policy, say: Say) -> Grant {
    Grant {
        dir: dir.join("egress"),
        policy,
        bounds: Bounds {
            most: 4,
            connect: Duration::from_secs(2),
            idle: Duration::from_secs(5),
            life: Duration::from_secs(20),
        },
        say,
    }
}

/// Inside a run: the proxy and the name server on loopback ephemeral
/// ports, piped to `egress`.
fn inside(egress: &Path) -> (SocketAddr, SocketAddr, Arc<AtomicBool>) {
    // The UDP and TCP name server on one port, as on the lamp.
    let (dns_udp, dns_tcp) = (0..20)
        .find_map(|_| {
            let udp = UdpSocket::bind("127.0.0.1:0").ok()?;
            let tcp = TcpListener::bind(udp.local_addr().ok()?).ok()?;
            Some((udp, tcp))
        })
        .unwrap();
    let inside = Inside {
        proxy: TcpListener::bind("127.0.0.1:0").unwrap(),
        dns_tcp,
        dns_udp,
    };
    let proxy = inside.proxy.local_addr().unwrap();
    let dns = inside.dns_udp.local_addr().unwrap();
    inside.proxy.set_nonblocking(true).unwrap();
    inside.dns_tcp.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    inside.serve(egress, BOUNDS, &stop, &heard().0).unwrap();
    (proxy, dns, stop)
}

fn through(proxy: SocketAddr, ask: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(proxy).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(ask.as_bytes()).unwrap();
    let mut got = Vec::new();
    let _ = stream.read_to_end(&mut got);
    got
}

/// The proxy from inside a run to a local server: `CONNECT` to a public
/// address is tunnelled, to a closed one (by literal, by a name that
/// resolves there, or a single-label name) is closed with no answer (curl
/// shows 000), a port other than the policy's is closed, and a name with
/// a closed and an open address reaches the open one only. Absolute-form
/// `http://` is sent on in origin form, with `Connection: close` and no
/// `Proxy-*` header. Each refusal is said in fixed text, never a name or
/// an address.
#[test]
fn the_proxy_reaches_a_public_address_and_closes_every_closed_one() {
    let (port, heads) = web();
    let dir = private();
    let (say, lines) = heard();
    let grant = grant(dir.path(), policy(port, vec![]), say);
    let served = grant
        .open("make-task-1", Instant::now() + Duration::from_secs(60))
        .unwrap();
    let (proxy, _, stop) = inside(served.path());
    // A tunnel to a public address: established, then the server's.
    let got = through(
        proxy,
        &format!(
            "CONNECT public.example:{port} HTTP/1.1\r\nHost: public.example:{port}\r\n\r\n\
             GET /in HTTP/1.1\r\nHost: x\r\n\r\n"
        ),
    );
    let got = String::from_utf8_lossy(&got);
    assert!(
        got.starts_with("HTTP/1.1 200 Connection established\r\n\r\nHTTP/1.1 200 OK"),
        "{got}"
    );
    assert!(got.ends_with("ok"), "{got}");
    let head = heads.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(head.starts_with(b"GET /in HTTP/1.1\r\n"));
    // Absolute-form http: origin form, closed after.
    let got = through(
        proxy,
        &format!(
            "GET http://public.example:{port}/page?q=1 HTTP/1.1\r\nHost: public.example:{port}\r\n\
             Proxy-Connection: keep-alive\r\nProxy-Authorization: Basic eA==\r\nUser-Agent: curl\r\n\r\n"
        ),
    );
    assert!(got.starts_with(b"HTTP/1.1 200 OK"));
    let head = String::from_utf8(heads.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(
        head,
        format!(
            "GET /page?q=1 HTTP/1.1\r\nHost: public.example:{port}\r\nUser-Agent: curl\r\n\
             Connection: close\r\n\r\n"
        )
    );
    // A name with closed and open addresses: the open one, checked.
    let got = through(
        proxy,
        &format!("CONNECT mixed.example:{port} HTTP/1.1\r\n\r\nGET / HTTP/1.1\r\n\r\n"),
    );
    assert!(got.starts_with(ESTABLISHED), "{got:?}");
    assert!(got.ends_with(b"ok"), "{got:?}");
    heads.recv_timeout(Duration::from_secs(10)).unwrap();
    // Every closed one: nothing answered at all.
    for ask in [
        format!("CONNECT 10.9.9.9:{port} HTTP/1.1\r\n\r\n"),
        format!("CONNECT rebind.example:{port} HTTP/1.1\r\n\r\n"),
        format!("CONNECT [::ffff:10.9.9.9]:{port} HTTP/1.1\r\n\r\n"),
        format!("CONNECT printer:{port} HTTP/1.1\r\n\r\n"),
        format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n"),
        format!("CONNECT nowhere.example:{port} HTTP/1.1\r\n\r\n"),
        format!("GET http://rebind.example:{port}/ HTTP/1.1\r\n\r\n"),
        format!("GET http://user@public.example:{port}/ HTTP/1.1\r\n\r\n"),
        "CONNECT public.example:22 HTTP/1.1\r\n\r\n".to_owned(),
        "CONNECT public.example:8080 HTTP/1.1\r\n\r\n".to_owned(),
        "GET http://public.example/ HTTP/1.1\r\n\r\n".to_owned(),
        format!("GET /relative HTTP/1.1\r\nHost: public.example:{port}\r\n\r\n"),
        "not http at all\r\n\r\n".to_owned(),
    ] {
        assert!(through(proxy, &ask).is_empty(), "{ask}");
    }
    assert!(heads.try_recv().is_err(), "the server was reached by none");
    let said = lines.lock().unwrap().clone();
    for wanted in [
        "[lamp-tasks] egress: closed a connection to an address of class private",
        "[lamp-tasks] egress: closed a connection to a single-label name",
        "[lamp-tasks] egress: closed a connection to a port other than 80 and 443",
        "[lamp-tasks] egress: a name did not resolve",
        "[lamp-tasks] egress: closed a connection that was not a proxy request",
    ] {
        assert!(
            said.iter().any(|line| line == wanted),
            "{wanted}: {said:#?}"
        );
    }
    for line in &said {
        assert!(
            !line.contains("example") && !line.contains("10.9") && !line.contains("printer"),
            "{line}"
        );
    }
    stop.store(true, Ordering::Release);
    drop(served);
}

/// The lamp's ports are the web's alone; any other is closed before a
/// name is resolved.
#[test]
fn only_ports_80_and_443_are_open() {
    assert_eq!(PORTS, [80, 443]);
    assert_eq!(Policy::lamp().ports, vec![80, 443]);
    let resolved = Arc::new(AtomicUsize::new(0));
    let counted = resolved.clone();
    let mut lamp = policy(80, vec![]);
    lamp.ports = PORTS.to_vec();
    lamp.resolve = Arc::new(move |_, _, _| {
        counted.fetch_add(1, Ordering::AcqRel);
        Err(io::ErrorKind::NotFound.into())
    });
    let (say, lines) = heard();
    for port in [22, 25, 53, 587, 7741, 8080, 51793] {
        assert!(open(&lamp, "public.example", port, Duration::from_secs(1), &say).is_none());
    }
    assert_eq!(resolved.load(Ordering::Acquire), 0);
    assert_eq!(lines.lock().unwrap().len(), 7);
}

#[test]
fn a_proxy_request_is_read_whole_and_checked() {
    let ask = |text: &str| Ask::parse(text.as_bytes());
    assert_eq!(
        ask("CONNECT Example.COM.:443 HTTP/1.1\r\nHost: x\r\n\r\n"),
        Some(Ask {
            tunnel: true,
            host: "example.com".into(),
            port: 443,
            head: vec![]
        })
    );
    assert_eq!(
        ask("CONNECT [2606:4700::1111]:443 HTTP/1.1\r\n\r\n").map(|a| (a.host, a.port)),
        Some(("2606:4700::1111".into(), 443))
    );
    assert_eq!(
        ask("GET http://example.com HTTP/1.0\r\n\r\n").map(|a| (a.port, a.head)),
        Some((
            80,
            b"GET / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n".to_vec()
        ))
    );
    for bad in [
        "CONNECT example.com HTTP/1.1\r\n\r\n",
        "CONNECT example.com:99999 HTTP/1.1\r\n\r\n",
        "CONNECT ex ample.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT exa_mple.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT [nothex]:443 HTTP/1.1\r\n\r\n",
        "GET https://example.com/ HTTP/1.1\r\n\r\n",
        "GET ftp://example.com/ HTTP/1.1\r\n\r\n",
        "get http://example.com/ HTTP/1.1\r\n\r\n",
        "GET http://example.com/ HTTP/2\r\n\r\n",
        "GET http://example.com/ HTTP/1.1\r\nno colon\r\n\r\n",
    ] {
        assert_eq!(ask(bad), None, "{bad}");
    }
    let mut big = format!(
        "GET http://example.com/ HTTP/1.1\r\nX: {}",
        "a".repeat(9000)
    );
    big.push_str("\r\n\r\n");
    assert_eq!(read_head(&mut big.as_bytes()), None, "past the most bytes");
}

/// A query for `name`, type `qtype`, id 0x1234, recursion desired.
fn query(name: &str, qtype: u16) -> Vec<u8> {
    let mut message = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.').filter(|label| !label.is_empty()) {
        message.push(label.len() as u8);
        message.extend_from_slice(label.as_bytes());
    }
    message.push(0);
    message.extend_from_slice(&qtype.to_be_bytes());
    message.extend_from_slice(&[0, 1]);
    message
}

fn rcode(reply: &[u8]) -> u8 {
    reply[3] & 0x0f
}

/// A name server on loopback, UDP and TCP on one port: each query answered
/// with its own bytes, the response bit set and `7` as its last byte, so
/// the test sees it was forwarded.
fn name_server() -> SocketAddr {
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = udp.local_addr().unwrap();
    let tcp = TcpListener::bind(addr).unwrap();
    let answer = |query: &[u8]| {
        let mut reply = query.to_vec();
        reply[2] |= 0x80;
        reply.push(7);
        reply
    };
    std::thread::spawn(move || {
        let mut got = [0u8; 4096];
        while let Ok((read, from)) = udp.recv_from(&mut got) {
            let _ = udp.send_to(&answer(&got[..read]), from);
        }
    });
    std::thread::spawn(move || {
        for stream in tcp.incoming().flatten() {
            let mut stream = stream;
            while let Ok(query) = read_framed(&mut stream, 4096) {
                if write_framed(&mut stream, &answer(&query)).is_err() {
                    break;
                }
            }
        }
    });
    addr
}

/// Heron's answer 2: a single-label name and a reverse lookup in a closed
/// range are refused; a whole public reverse name and any other name are
/// forwarded as they came.
#[test]
fn single_label_names_and_closed_reverse_lookups_are_refused() {
    let mut lamp = policy(80, vec![]);
    lamp.closed = Arc::new(|addr| class(addr, &office()));
    let (say, lines) = heard();
    let refused = |name: &str, qtype: u16| {
        let question = question(&query(name, qtype)).unwrap();
        refusal(&question, &lamp.closed)
    };
    for name in ["printer", "nas.", "localhost", ""] {
        assert_eq!(refused(name, 1), Some("a single-label name"), "{name}");
    }
    for name in [
        "1.20.168.172.in-addr.arpa",
        "9.9.9.10.in-addr.arpa",
        "1.0.0.127.in-addr.arpa",
        "20.168.172.in-addr.arpa",
        "in-addr.arpa",
        "x.1.20.168.172.in-addr.arpa",
        "300.1.1.1.in-addr.arpa",
        "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.e.f.ip6.arpa",
        "8.e.f.ip6.arpa",
    ] {
        assert_eq!(
            refused(name, PTR),
            Some("a reverse lookup in a closed range"),
            "{name}"
        );
    }
    // Under the reverse zones whatever the type.
    assert_eq!(
        refused("1.0.0.127.in-addr.arpa", 1),
        Some("a reverse lookup in a closed range")
    );
    for (name, qtype) in [
        ("example.com", 1),
        ("example.com.", 28),
        ("a.b.example.org", 16),
        ("8.8.8.8.in-addr.arpa", PTR),
        (
            "1.1.1.1.4.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.7.4.0.0.7.4.6.0.6.2.ip6.arpa",
            PTR,
        ),
    ] {
        assert_eq!(refused(name, qtype), None, "{name}");
    }
    // The refusal answered: same id and question, REFUSED, no record.
    let asked = query("printer", 1);
    let reply = answer(&asked, &lamp, false, &say).unwrap();
    assert_eq!(&reply[..2], &asked[..2]);
    assert_eq!(reply[2] & 0x80, 0x80);
    assert_eq!(rcode(&reply), REFUSED);
    assert_eq!(&reply[4..12], &[0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&reply[12..], &asked[12..]);
    // Not one question, or a response: a format error, or nothing.
    let mut two = query("example.com", 1);
    two[5] = 2;
    assert_eq!(rcode(&answer(&two, &lamp, false, &say).unwrap()), FORMERR);
    assert!(answer(&[1, 2, 3], &lamp, false, &say).is_none());
    // No name server: a server failure, said.
    let reply = answer(&query("example.com", 1), &lamp, false, &say).unwrap();
    assert_eq!(rcode(&reply), SERVFAIL);
    for line in lines.lock().unwrap().iter() {
        assert!(
            !line.contains("printer") && !line.contains("example"),
            "{line}"
        );
    }
}

/// From inside a run over UDP and TCP, through the egress socket: a name
/// is forwarded to the lamp's name server and its answer returned as it
/// came; a refused one never reaches it.
#[test]
fn names_resolve_through_the_egress_and_refusals_never_leave() {
    let server = name_server();
    let dir = private();
    let grant = grant(dir.path(), policy(80, vec![server]), heard().0);
    let served = grant
        .open("make-task-2", Instant::now() + Duration::from_secs(60))
        .unwrap();
    let (_, dns, stop) = inside(served.path());
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut got = [0u8; 4096];
    let asked = query("example.com", 1);
    udp.send_to(&asked, dns).unwrap();
    let read = udp.recv(&mut got).unwrap();
    assert_eq!(
        &got[..read - 1],
        &{
            let mut reply = asked.clone();
            reply[2] |= 0x80;
            reply
        }[..]
    );
    assert_eq!(got[read - 1], 7, "forwarded");
    let asked = query("nas", 1);
    udp.send_to(&asked, dns).unwrap();
    let read = udp.recv(&mut got).unwrap();
    assert_eq!(rcode(&got[..read]), REFUSED);
    assert_eq!(read, asked.len(), "never forwarded");
    // TCP, two in one connection.
    let mut tcp = TcpStream::connect(dns).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write_framed(&mut tcp, &query("example.org", 28)).unwrap();
    assert_eq!(*read_framed(&mut tcp, 4096).unwrap().last().unwrap(), 7);
    write_framed(&mut tcp, &query("1.20.168.172.in-addr.arpa", PTR)).unwrap();
    assert_eq!(rcode(&read_framed(&mut tcp, 4096).unwrap()), REFUSED);
    stop.store(true, Ordering::Release);
    drop(served);
}

/// The socket: 0600 in its 0700 folder, for a run's folder name alone,
/// removed when dropped and when the run's deadline passes; one left by a
/// lamp-tasks that ended is swept.
#[test]
fn the_egress_socket_lives_only_while_its_run_does() {
    let dir = private();
    let grant = grant(dir.path(), policy(80, vec![]), heard().0);
    for bad in [
        "model",
        "make-",
        "make-A",
        "make-../x",
        "skill-",
        "skill-A",
        "run-1",
    ] {
        assert!(
            grant
                .open(bad, Instant::now() + Duration::from_secs(5))
                .is_err(),
            "{bad}"
        );
    }
    assert!(!grant.dir.exists() || std::fs::read_dir(&grant.dir).unwrap().count() == 0);
    let served = grant
        .open("make-task-3", Instant::now() + Duration::from_secs(60))
        .unwrap();
    assert_eq!(served.path(), grant.socket("make-task-3"));
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&grant.dir), 0o700);
    assert_eq!(mode(served.path()), 0o600);
    let path = served.path().to_path_buf();
    drop(served);
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_dir(&grant.dir).unwrap().count(),
        0,
        "nothing left"
    );
    // Its deadline passed: gone though not yet dropped.
    let served = grant
        .open("make-task-4", Instant::now() + Duration::from_millis(100))
        .unwrap();
    let began = Instant::now();
    while served.path().exists() {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "outlived its deadline"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(UnixStream::connect(served.path()).is_err());
    drop(served);
    // A socket left by an ended lamp-tasks: swept at the next run.
    let left = grant.socket("make-task-old");
    let _listener = UnixListener::bind(&left).unwrap();
    let served = grant
        .open("make-task-5", Instant::now() + Duration::from_secs(60))
        .unwrap();
    assert!(!left.exists());
    drop(served);
    // A folder that isn't private: refused.
    std::fs::set_permissions(&grant.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        grant
            .open("make-task-6", Instant::now() + Duration::from_secs(5))
            .is_err()
    );
}

#[test]
fn the_names_agree_with_the_namespace_script() {
    assert_eq!(EGRESS_DIR, "/run/lampos-tasks/egress");
    assert_eq!(
        Path::new(EGRESS_DIR).parent(),
        Path::new(crate::net_bridge::MODEL_SOCKET).parent()
    );
    let source = include_str!("../../../update/src/hermes_install.rs");
    assert!(source.contains(&format!("pub const EGRESS_ENV: &str = \"{EGRESS_ENV}\";")));
    assert!(source.contains(&format!("pub const EGRESS_DIR: &str = \"{EGRESS_DIR}\";")));
    assert!(source.contains(NS_LINE), "{NS_LINE}");
    assert!(source.contains(NS_SKILL_LINE), "{NS_SKILL_LINE}");
    assert!(source.contains("--egress \\\"$V\\\""));
    assert!(plain_name("make-task-1") && plain_name("skill-task-1"));
    assert!(!plain_name("skill-") && !plain_name("other-task-1") && !plain_name("skill-A"));
    assert_eq!(PROXY_LISTEN.to_string(), "127.0.0.1:3128");
    assert_eq!(DNS_LISTEN.to_string(), "127.0.0.1:53");
    assert!(source.contains("http_proxy=http://127.0.0.1:3128"));
    assert!(source.contains("NODE_USE_ENV_PROXY=1"));
    assert!(source.contains("NO_PROXY=127.0.0.1,localhost"));
}
