//! What a play may touch, checked before any connect (and again in the unit
//! that connects): `https` on port 443 only, no userinfo, a host the lamp's
//! cloud named for this play (exact name), and every address the name
//! resolves to a public IPv4 address. A redirect is checked the same way,
//! against the same set. No other scheme, no IPv6, no address literal.
use std::net::{IpAddr, Ipv4Addr};
use url::{Host, Url};

/// Why a URL or an address was refused. The words are fixed: nothing the URL
/// held is ever in them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refused {
    /// Not a URL, or longer than [`MOST_URL`].
    Malformed,
    /// Not `https`.
    Scheme,
    /// A user name or password in it.
    UserInfo,
    /// Not port 443.
    Port,
    /// An address literal, not a name.
    Literal,
    /// A name the cloud did not name for this play.
    Host,
    /// A name that resolved to an address a play may not reach, or to none.
    Address,
}

impl Refused {
    /// The fixed words for the app and the journal.
    pub fn why(self) -> &'static str {
        match self {
            Refused::Malformed => "not a URL this lamp can play",
            Refused::Scheme => "only https is played",
            Refused::UserInfo => "a URL with a user name or password is not played",
            Refused::Port => "only port 443 is played",
            Refused::Literal => "a host must be a name, not an address",
            Refused::Host => "that host was not named for this play",
            Refused::Address => "that host resolves to an address a play may not reach",
        }
    }
}

/// The longest URL taken, in bytes.
pub const MOST_URL: usize = 2_048;
/// The most hosts one play may be given.
pub const MOST_HOSTS: usize = 16;

/// The hosts the lamp's cloud named for one play: exact names, lower case,
/// no trailing dot, no address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allowed {
    hosts: Vec<String>,
}

/// Where a checked URL goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    /// The host, as the set names it.
    pub host: String,
    /// The path and query, as the URL carries them (never journaled).
    pub request: String,
}

impl Allowed {
    /// The set from the names the cloud gave. A name that is not a plain DNS
    /// name (an address, an empty label, a trailing dot, a wildcard) is
    /// refused as a whole: a set is all valid or none.
    pub fn new<S: AsRef<str>>(hosts: &[S]) -> Result<Self, Refused> {
        if hosts.is_empty() || hosts.len() > MOST_HOSTS {
            return Err(Refused::Host);
        }
        let mut kept = Vec::new();
        for host in hosts {
            let host = host.as_ref();
            if !plain_name(host) {
                return Err(Refused::Host);
            }
            let host = host.to_owned();
            if !kept.contains(&host) {
                kept.push(host);
            }
        }
        Ok(Self { hosts: kept })
    }

    /// `url`, if a play may go there: `https`, port 443, no userinfo, a name
    /// in the set. Nothing is resolved here.
    pub fn check(&self, url: &str) -> Result<Target, Refused> {
        if url.len() > MOST_URL {
            return Err(Refused::Malformed);
        }
        let parsed = Url::parse(url).map_err(|_| Refused::Malformed)?;
        if parsed.scheme() != "https" {
            return Err(Refused::Scheme);
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(Refused::UserInfo);
        }
        if parsed.port().is_some_and(|port| port != 443) {
            return Err(Refused::Port);
        }
        let host = match parsed.host() {
            Some(Host::Domain(name)) => name.to_owned(),
            Some(_) => return Err(Refused::Literal),
            None => return Err(Refused::Malformed),
        };
        if !self.hosts.contains(&host) {
            return Err(Refused::Host);
        }
        let mut request = parsed.path().to_owned();
        if let Some(query) = parsed.query() {
            request.push('?');
            request.push_str(query);
        }
        Ok(Target { host, request })
    }
}

/// A plain lower-case DNS name: labels of `a-z`, `0-9` and `-` (not first or
/// last), 1 to 63 bytes each, 253 in all, at least two labels, and not four
/// numbers (an address). Punycode is the `xn--` form.
fn plain_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let well_formed = labels.iter().all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    // The last label of an address is a number; a real top-level name never is.
    let last_is_number = labels
        .last()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    well_formed && !last_is_number
}

/// Whether a play may connect to `address`: a public IPv4 address, none of
/// this host, a private network, a link-local, a shared-address (carrier
/// grade) range, a multicast, a reserved, a documentation or a benchmark
/// range.
pub fn public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (18..=19).contains(&b))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

/// The addresses a name resolved to, if a play may connect to them: only the
/// IPv4 ones are used (there is no IPv6 socket in the unit), and every one of
/// them must be public. One that is not refuses the whole answer (a name that
/// answers with a private address beside a public one is never played), and
/// an answer with no IPv4 address is refused too.
pub fn public_addresses(answer: &[IpAddr]) -> Result<Vec<Ipv4Addr>, Refused> {
    let mut kept = Vec::new();
    for address in answer {
        match address {
            IpAddr::V4(v4) if public_ipv4(*v4) => kept.push(*v4),
            IpAddr::V4(_) => return Err(Refused::Address),
            // An IPv6 answer is ignored, as the unit has no v6 socket; the
            // mapped form of a v4 address is checked as that address.
            IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped()
                    && !public_ipv4(v4)
                {
                    return Err(Refused::Address);
                }
            }
        }
    }
    if kept.is_empty() {
        return Err(Refused::Address);
    }
    Ok(kept)
}

#[cfg(test)]
mod tests;
