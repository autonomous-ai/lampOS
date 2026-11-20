use super::*;

fn allowed() -> Allowed {
    Allowed::new(&["cdn.example.com", "traffic.example.net"]).unwrap()
}

#[test]
fn a_named_https_host_is_taken_with_its_path_and_query() {
    let target = allowed()
        .check("https://cdn.example.com/a/b.mp3?sig=1&x=2")
        .unwrap();
    assert_eq!(target.host, "cdn.example.com");
    assert_eq!(target.request, "/a/b.mp3?sig=1&x=2");
    // Port 443 said out loud is the same.
    assert!(allowed().check("https://cdn.example.com:443/x").is_ok());
    // The host is compared in lower case, as the parser makes it.
    assert!(allowed().check("https://CDN.Example.COM/x").is_ok());
}

#[test]
fn everything_but_https_on_443_with_a_named_host_is_refused() {
    let table = [
        ("http://cdn.example.com/x", Refused::Scheme),
        ("ftp://cdn.example.com/x", Refused::Scheme),
        ("file:///etc/passwd", Refused::Scheme),
        ("data:audio/mpeg;base64,AAAA", Refused::Scheme),
        ("https://user@cdn.example.com/x", Refused::UserInfo),
        ("https://user:pw@cdn.example.com/x", Refused::UserInfo),
        ("https://:pw@cdn.example.com/x", Refused::UserInfo),
        ("https://cdn.example.com:8443/x", Refused::Port),
        ("https://cdn.example.com:80/x", Refused::Port),
        ("https://other.example.com/x", Refused::Host),
        ("https://cdn.example.com.evil.net/x", Refused::Host),
        ("https://evil.net/cdn.example.com", Refused::Host),
        ("https://cdn.example.com./x", Refused::Host),
        ("https://sub.cdn.example.com/x", Refused::Host),
        ("https://93.184.216.34/x", Refused::Literal),
        ("https://[2001:db8::1]/x", Refused::Literal),
        ("https://0x5db8d822/x", Refused::Literal),
        ("https://2130706433/x", Refused::Literal),
        ("https://127.1/x", Refused::Literal),
        ("not a url", Refused::Malformed),
        ("", Refused::Malformed),
        // The parser reads the third slash away and takes "x" for the host.
        ("https:///x", Refused::Host),
        ("https://", Refused::Malformed),
    ];
    for (url, why) in table {
        assert_eq!(allowed().check(url), Err(why), "{url}");
    }
}

#[test]
fn a_look_alike_name_is_not_the_named_one() {
    // Punycode is another name, and a name with a tab, a space or a
    // backslash trick is either refused or another host, never the named one.
    for url in [
        "https://xn--cdn-example-com.com/x",
        "https://cdn.example.com%2eevil.net/x",
        "https://cdn.example.com\\@evil.net/x",
        "https://evil.net\\.cdn.example.com/x",
        "https://cdn.example.com\t.evil.net/x",
    ] {
        if let Ok(target) = allowed().check(url) {
            assert_eq!(target.host, "cdn.example.com", "{url}: another host");
        }
    }
    assert!(
        allowed()
            .check("https://evil.net\\.cdn.example.com/x")
            .is_err()
    );
}

#[test]
fn a_url_over_the_most_is_refused() {
    let long = format!("https://cdn.example.com/{}", "a".repeat(MOST_URL));
    assert_eq!(allowed().check(&long), Err(Refused::Malformed));
}

#[test]
fn a_set_is_all_plain_names_or_none() {
    let none: [&str; 0] = [];
    assert_eq!(Allowed::new(&none), Err(Refused::Host));
    for bad in [
        "localhost",
        "93.184.216.34",
        "example.com.",
        "*.example.com",
        "EXAMPLE.com",
        "exa mple.com",
        ".example.com",
        "example..com",
        "-a.example.com",
        "a-.example.com",
        "1.2.3.4",
        "example.123",
        "[::1]",
        "",
    ] {
        assert_eq!(
            Allowed::new(&["cdn.example.com", bad]),
            Err(Refused::Host),
            "{bad:?}"
        );
    }
    let many: Vec<String> = (0..=MOST_HOSTS)
        .map(|n| format!("h{n}.example.com"))
        .collect();
    assert_eq!(Allowed::new(&many), Err(Refused::Host));
    // A name twice is one.
    assert!(Allowed::new(&["cdn.example.com", "cdn.example.com"]).is_ok());
}

#[test]
fn only_a_public_ipv4_address_may_be_connected_to() {
    let public = [
        "8.8.8.8",
        "93.184.216.34",
        "1.1.1.1",
        "100.63.255.255",
        "100.128.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "198.17.255.255",
        "198.20.0.0",
        "223.255.255.255",
    ];
    for address in public {
        assert!(public_ipv4(address.parse().unwrap()), "{address}");
    }
    let private = [
        "0.0.0.0",
        "0.1.2.3",
        "10.0.0.1",
        "10.255.255.255",
        "127.0.0.1",
        "127.255.255.254",
        "100.64.0.1",
        "100.127.255.255",
        "169.254.1.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "192.88.99.1",
        "192.168.0.1",
        "192.168.255.255",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.255",
        "240.0.0.1",
        "255.255.255.255",
    ];
    for address in private {
        assert!(!public_ipv4(address.parse().unwrap()), "{address}");
    }
}

#[test]
fn a_resolved_answer_is_used_only_if_every_address_in_it_is_public() {
    let a = |text: &str| text.parse::<IpAddr>().unwrap();
    assert_eq!(
        public_addresses(&[a("93.184.216.34"), a("8.8.8.8")]),
        Ok(vec![
            "93.184.216.34".parse().unwrap(),
            "8.8.8.8".parse().unwrap()
        ])
    );
    // IPv6 beside an IPv4 one is ignored: the unit has no v6 socket.
    assert_eq!(
        public_addresses(&[a("2606:2800:220:1::1"), a("93.184.216.34")]),
        Ok(vec!["93.184.216.34".parse().unwrap()])
    );
    // One private address refuses the whole answer, in any order.
    assert_eq!(
        public_addresses(&[a("93.184.216.34"), a("192.168.1.5")]),
        Err(Refused::Address)
    );
    assert_eq!(
        public_addresses(&[a("10.0.0.1"), a("93.184.216.34")]),
        Err(Refused::Address)
    );
    // Only IPv6, or nothing, is no address to play from.
    assert_eq!(
        public_addresses(&[a("2606:2800:220:1::1")]),
        Err(Refused::Address)
    );
    assert_eq!(public_addresses(&[]), Err(Refused::Address));
    // A v6 address that maps a private v4 one is refused as that address.
    assert_eq!(
        public_addresses(&[a("::ffff:10.0.0.1"), a("93.184.216.34")]),
        Err(Refused::Address)
    );
    // The mapped form of a public one is ignored like any v6 address.
    assert_eq!(
        public_addresses(&[a("::ffff:8.8.8.8")]),
        Err(Refused::Address)
    );
}

#[test]
fn the_refusal_words_hold_nothing_the_url_held() {
    for why in [
        Refused::Malformed,
        Refused::Scheme,
        Refused::UserInfo,
        Refused::Port,
        Refused::Literal,
        Refused::Host,
        Refused::Address,
    ] {
        let words = why.why();
        assert!(!words.is_empty() && !words.contains("example"), "{words}");
    }
}
