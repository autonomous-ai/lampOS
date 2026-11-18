//! A JSON `GET` of a public service that takes no key (Open-Meteo), its
//! connection raced as happy eyeballs race theirs: a try begins, another
//! [`STAGGER`] later while none has opened and later ones further apart
//! ([`GAPS`]), each on a new socket of its own and spread over every address
//! the name has, at most [`MOST_TRIES`]; the first whose TCP and TLS
//! handshakes end is asked, and the rest are dropped.
//!
//! lamp-9adf on 17 Nov: the api host 1.1.1.1 names for Open-Meteo never
//! answered the SYN of about half the connections opened to it, from the Mac
//! beside it too, while others opened in 0.3 s. Every weather call waited
//! out its 6 s on one connection, and three tries one after another (#7596's
//! first fix) still failed 4 rows of 4, while three curls at once opened
//! 12 of 12.
//!
//! lamp-9adf on 18 Nov (#7593): a place looked up in 1.2 s, then its
//! forecast's one address opened none of 4 tries in 2.5 s, all begun within
//! 1.2 s, and the ask failed in 3.7 s with more than 2 s of its 6 s unused.
//! The caller may now give a request a longer bound ([`get_public_json_within`]:
//! the forecast what is left of the weather's), and tries begun further
//! apart reach past a short stretch of dropped SYNs on one address.
//!
//! lamp-9adf on 18 Nov, again (#7593): the lamp's resolver (1.1.1.1) gave
//! the forecast host one address, 51.81.167.44, which opened no curl of 6,
//! while the same host at the address Google's resolver gives,
//! 157.90.83.168, answered in 240 ms: the host's own name server answers
//! each resolver with a server of its choosing. 2 of 5 forecasts failed. A
//! name none of whose tries has opened once a try to each of its addresses
//! has failed outright, or once [`ELSEWHERE_AFTER`] has passed with none
//! open, is now looked up a
//! second time through another resolver ([`ELSEWHERE`]), and its global
//! addresses not raced before join the race, the next try to the first of
//! them, up to [`MORE_TRIES`] tries more. Only with
//! `LAMP_WEATHER_DOH=on` ([`DOH_ENVIRONMENT`]): off
//! unless exactly `on`, and off in the product (El's ruling on #7700,
//! 14:23), so the product never looks a name up a second time.
use super::{
    Error, OPENING, Result, fetch_json_on, kept_builder,
    watch::{Opening, OpeningTold, Opens},
};
use futures_util::future::BoxFuture;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::{AbortHandle, JoinSet},
    time::Instant,
};

/// A new try begins at least this long after the one before while none
/// has opened: the second, to a name's second address, as happy eyeballs.
pub(crate) const STAGGER: Duration = Duration::from_millis(400);
/// The most tries one request begins.
pub(crate) const MOST_TRIES: usize = 4;
/// How long after the one before each later try begins while none has
/// opened, the bound allowing ([`TRY_OPENS_IN`]): the second [`STAGGER`]
/// after the first, then further apart, the last about a SYN's first resend
/// (1 s on Linux) after the third, so on a name with one address the tries
/// and their kernel's resends spread over 0, 0.4, 1, 2 s and on, not 1.2 s.
pub(crate) const GAPS: [Duration; MOST_TRIES - 1] = [
    STAGGER,
    Duration::from_millis(600),
    Duration::from_millis(1000),
];
/// A later try begins no later than this before the request's bound, so
/// it has time to open, though never sooner than [`STAGGER`] after the one
/// before: from lamp-9adf on 18 Nov, TLS ended 0.65–1.37 s after a curl
/// began. Within [`OPEN_WITHIN`] the tries begin at 0, 0.4, 1 and 1.4 s.
pub(crate) const TRY_OPENS_IN: Duration = Duration::from_millis(1500);
/// A connection opens within this from the request's start, or the request
/// fails: the name looked up and one try's handshakes ended. Half the
/// weather's [`super::weather::WITHIN`] and a little less, so a place looked
/// up leaves its forecast time of its own; the forecast's bound is what is
/// left of the weather's, never less than this
/// ([`super::weather::forecast_open_within`]).
pub(crate) const OPEN_WITHIN: Duration = Duration::from_millis(2500);
/// Where a name is looked up a second time ([`elsewhere`]): Google's
/// public resolver's JSON API over HTTPS, by its own address, so neither
/// the lamp's resolver nor the server it was given is asked again. Only
/// the service's host name goes, as it goes to the lamp's resolver anyway.
pub(crate) const ELSEWHERE: &str = "https://8.8.8.8/resolve";
/// The most tries one request begins beyond [`MOST_TRIES`] once the second
/// lookup gave addresses not raced before: two, so one dropped SYN on a
/// new address doesn't end the request.
pub(crate) const MORE_TRIES: usize = 2;
/// With not every address failed outright, the name is looked up a second time only
/// once this has passed from the first try's start with none open: past a
/// healthy open, so an ordinary fetch never asks [`ELSEWHERE`]. From
/// lamp-9adf on 18 Nov, TLS ended 0.65–1.37 s after a curl began and
/// healthy opens of the forecast came under about 1 s; Vietnam to Germany
/// takes past [`STAGGER`] often. The address it adds still has about 2.5 s
/// to open within a forecast's 4 s after a place looked up in 1.2 s.
pub(crate) const ELSEWHERE_AFTER: Duration = Duration::from_millis(1500);
/// The switch for the second lookup, in the voice service's environment:
/// off unless exactly `on` ([`doh_on`]); no product unit sets
/// it.
pub const DOH_ENVIRONMENT: &str = "LAMP_WEATHER_DOH";

/// Whether a name is looked up a second time another way: only with the
/// switch exactly `on`; unset or anything else is off.
pub fn doh_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// How each try's client is made beyond the lamp's own, for the address
/// it goes to: unchanged on the lamp, a connector that never opens in a
/// test.
type Shape =
    Arc<dyn Fn(reqwest::ClientBuilder, SocketAddr) -> reqwest::ClientBuilder + Send + Sync>;

/// A lookup of a name's addresses for a port: the lamp's own resolver and
/// [`elsewhere`] on the lamp, set ones in a test.
type Lookup = Arc<dyn Fn(String, u16) -> BoxFuture<'static, Result<Vec<SocketAddr>>> + Send + Sync>;

/// How a request reaches its service: its name looked up (`lookup`), then
/// a second time another way while none of its tries has opened
/// (`elsewhere`), each try's client made by `shape`.
struct Ways {
    lookup: Lookup,
    elsewhere: Option<Lookup>,
    shape: Shape,
}

impl Ways {
    /// The lamp's: its own resolver, then Google's over HTTPS only with
    /// [`DOH_ENVIRONMENT`] exactly `on`.
    fn lamps() -> Self {
        Self::lamps_with(doh_on(std::env::var(DOH_ENVIRONMENT).ok().as_deref()))
    }

    /// The lamp's, with the second lookup or with none at all.
    fn lamps_with(second_lookup: bool) -> Self {
        Self {
            lookup: Arc::new(|domain, port| {
                Box::pin(async move {
                    let found = tokio::net::lookup_host((domain.as_str(), port))
                        .await
                        .map_err(|_| Error::Transport("address"))?;
                    Ok(found.collect())
                })
            }),
            elsewhere: second_lookup
                .then(|| -> Lookup { Arc::new(|domain, port| Box::pin(elsewhere(domain, port))) }),
            shape: Arc::new(|builder, _| builder),
        }
    }
}

/// A JSON `GET` of `url`, bounded as [`super::get_json`] is once its
/// connection is open, which it must be within `open_within` from now
/// (the weather's place [`OPEN_WITHIN`], its forecast what is left of its
/// own bound).
pub(crate) async fn get_public_json_within(
    url: &str,
    open_within: Duration,
) -> Result<serde_json::Value> {
    raced(url, open_within, Ways::lamps()).await
}

/// [`get_public_json_within`], each try sent with `agent` as its
/// `User-Agent`: MET Norway's terms ask every client to name itself (the
/// weather's backup, #7593).
pub(crate) async fn get_public_json_as(
    url: &str,
    open_within: Duration,
    agent: &'static str,
) -> Result<serde_json::Value> {
    let ways = Ways {
        shape: Arc::new(move |builder, _| builder.user_agent(agent)),
        ..Ways::lamps()
    };
    raced(url, open_within, ways).await
}

/// When the try after one begun at `previous` begins while none has
/// opened, `index` the try's own: [`GAPS`] after it, but not past
/// [`TRY_OPENS_IN`] before `deadline`, and never sooner than [`STAGGER`].
fn next_begins(index: usize, previous: Instant, deadline: Instant) -> Instant {
    let gap = index
        .checked_sub(1)
        .and_then(|at| GAPS.get(at))
        .copied()
        .unwrap_or(STAGGER);
    let latest = deadline.checked_sub(TRY_OPENS_IN).unwrap_or(previous);
    (previous + gap).min(latest).max(previous + STAGGER)
}

/// The tries begun so far, and where each went.
struct Race<'a> {
    url: &'a str,
    domain: Option<String>,
    addresses: Vec<SocketAddr>,
    /// How many tries each of `addresses` has been sent.
    sent: Vec<usize>,
    /// Whether a try to each of `addresses` has failed outright.
    refused: Vec<bool>,
    /// The address each try went to, by its index.
    went: Vec<usize>,
    open_within: Duration,
    shape: Shape,
    opened: mpsc::UnboundedSender<usize>,
    tries: JoinSet<(usize, Result<serde_json::Value>)>,
    handles: Vec<AbortHandle>,
}

impl Race<'_> {
    fn begun(&self) -> usize {
        self.handles.len()
    }

    /// Begin a try to the address sent the fewest so far, the first of
    /// those: each address once in the resolver's order, then round again,
    /// and an address the second lookup added next.
    fn begin(&mut self) {
        let at = (0..self.addresses.len())
            .min_by_key(|&at| self.sent[at])
            .unwrap_or(0);
        self.sent[at] += 1;
        self.went.push(at);
        let index = self.handles.len();
        self.handles.push(self.tries.spawn(attempt(
            index,
            self.url.to_owned(),
            self.domain.clone(),
            self.addresses[at],
            self.open_within,
            self.shape.clone(),
            self.opened.clone(),
        )));
    }

    /// Add the addresses `found` not raced before; how many.
    fn add(&mut self, found: Vec<SocketAddr>) -> usize {
        let before = self.addresses.len();
        for address in found {
            if !self.addresses.contains(&address) {
                self.addresses.push(address);
                self.sent.push(0);
                self.refused.push(false);
            }
        }
        self.addresses.len() - before
    }

    /// Mark the address try `index` went to as one a try failed outright.
    fn refused_at(&mut self, index: usize) {
        if let Some(&at) = self.went.get(index) {
            self.refused[at] = true;
        }
    }

    /// Whether a try to every address has failed outright: the lamp's
    /// resolver gave none it can reach. An IPv6 address a lamp with no
    /// IPv6 route fails at once beside a healthy IPv4 one is not that.
    fn every_address_refused(&self) -> bool {
        self.refused.iter().all(|&refused| refused)
    }
}

async fn raced(url: &str, open_within: Duration, ways: Ways) -> Result<serde_json::Value> {
    let began = Instant::now();
    let deadline = began + open_within;
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::Url)?;
    let port = parsed.port_or_known_default().ok_or(Error::Url)?;
    let host = parsed.host_str().ok_or(Error::Url)?;
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    let (domain, addresses) = match literal.parse::<IpAddr>() {
        Ok(ip) => (None, vec![SocketAddr::new(ip, port)]),
        Err(_) => (
            Some(host.to_owned()),
            addresses_of((ways.lookup)(host.to_owned(), port), deadline).await?,
        ),
    };
    // A name, not an address, may be looked up again another way.
    let mut elsewhere = ways.elsewhere.filter(|_| domain.is_some());
    let mut asking: Option<(BoxFuture<'static, Result<Vec<SocketAddr>>>, Instant)> = None;
    let (opened_tx, mut opened) = mpsc::unbounded_channel::<usize>();
    let mut race = Race {
        url,
        domain,
        sent: vec![0; addresses.len()],
        refused: vec![false; addresses.len()],
        went: Vec::new(),
        addresses,
        open_within,
        shape: ways.shape,
        opened: opened_tx,
        tries: JoinSet::new(),
        handles: Vec::new(),
    };
    let mut most = MOST_TRIES;
    let stalled = began + ELSEWHERE_AFTER;
    race.begin();
    let mut next = next_begins(1, began, deadline);
    let winner = loop {
        // None open, and every address refused or the first try stalled
        // past a healthy open: the name is looked up again.
        if (race.every_address_refused() || Instant::now() >= stalled)
            && let (Some(lookup), Some(domain)) = (elsewhere.take(), race.domain.clone())
        {
            asking = Some((lookup(domain, port), Instant::now()));
        }
        tokio::select! {
            biased;
            Some(index) = opened.recv() => break index,
            joined = race.tries.join_next(), if !race.tries.is_empty() => match joined {
                Some(unopened @ (Ok((_, Err(Error::Transport(OPENING) | Error::Timeout(OPENING)))) | Err(_))) => {
                    // A try that couldn't open (an address the lamp has no
                    // route to fails at once): the next begins now.
                    if let Ok((index, _)) = unopened {
                        race.refused_at(index);
                    }
                    if race.begun() < most {
                        let now = Instant::now();
                        race.begin();
                        next = next_begins(race.begun(), now, deadline);
                    } else if race.tries.is_empty() && asking.is_none() {
                        told_unopened(began, race.begun(), race.addresses.len());
                        return Err(Error::Transport(OPENING));
                    }
                }
                Some(Ok((index, answered))) => {
                    told_opened(index, race.begun(), race.addresses.len(), began);
                    return answered;
                }
                None => {}
            },
            found = async {
                match asking.as_mut() {
                    Some((lookup, _)) => lookup.await,
                    None => std::future::pending().await,
                }
            } => {
                let asked_at = asking.take().map_or(began, |(_, at)| at);
                let more = found.map(|found| race.add(found));
                told_elsewhere(&more, asked_at);
                if matches!(more, Ok(more) if more > 0) {
                    // The new addresses are raced at once, the next try to
                    // the first of them.
                    most = MOST_TRIES + MORE_TRIES;
                    let now = Instant::now();
                    race.begin();
                    next = next_begins(race.begun(), now, deadline);
                } else if race.tries.is_empty() && race.begun() >= most {
                    told_unopened(began, race.begun(), race.addresses.len());
                    return Err(Error::Transport(OPENING));
                }
            }
            _ = tokio::time::sleep_until(next), if race.begun() < most => {
                race.begin();
                next = next_begins(race.begun(), next, deadline);
            }
            // The first try stalled past a healthy open: the loop's top
            // asks the second lookup.
            _ = tokio::time::sleep_until(stalled), if elsewhere.is_some() => {}
            _ = tokio::time::sleep_until(deadline) => {
                told_unopened(began, race.begun(), race.addresses.len());
                return Err(Error::Timeout(OPENING));
            }
        }
    };
    told_opened(winner, race.begun(), race.addresses.len(), began);
    for (index, handle) in race.handles.iter().enumerate() {
        if index != winner {
            handle.abort();
        }
    }
    while let Some(joined) = race.tries.join_next().await {
        if let Ok((index, answered)) = joined
            && index == winner
        {
            return answered;
        }
    }
    Err(Error::Transport("request"))
}

/// Every address a lookup found, each once, in the resolver's order, by
/// `deadline`.
async fn addresses_of(
    lookup: BoxFuture<'static, Result<Vec<SocketAddr>>>,
    deadline: Instant,
) -> Result<Vec<SocketAddr>> {
    let found = tokio::time::timeout_at(deadline, lookup)
        .await
        .map_err(|_| Error::Timeout("address"))??;
    let mut addresses: Vec<SocketAddr> = Vec::new();
    for address in found {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    if addresses.is_empty() {
        return Err(Error::Transport("address"));
    }
    Ok(addresses)
}

/// `domain`'s addresses for `port` from [`ELSEWHERE`], its IPv4 and IPv6
/// answers asked together, IPv4's first; either alone when the other
/// fails. The race bounds it.
async fn elsewhere(domain: String, port: u16) -> Result<Vec<SocketAddr>> {
    let client = kept_builder(None)?
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|_| Error::Transport("HTTPS client setup"))?;
    let ask = |kind: &'static str| {
        let client = client.clone();
        let url =
            reqwest::Url::parse_with_params(ELSEWHERE, [("name", domain.as_str()), ("type", kind)]);
        async move {
            let url = url.map_err(|_| Error::Url)?;
            fetch_json_on(&client, url.as_str(), None).await
        }
    };
    let (v4, v6) = tokio::join!(ask("A"), ask("AAAA"));
    if let (Err(error), Err(_)) = (&v4, &v6) {
        return Err(error.clone());
    }
    let found: Vec<SocketAddr> = [v4, v6]
        .iter()
        .flatten()
        .flat_map(|answer| resolved(answer, port))
        .collect();
    if found.is_empty() {
        return Err(Error::Transport("address"));
    }
    Ok(found)
}

/// The global addresses in a JSON DNS answer (`Answer[].data` of type A
/// or AAAA, an alias's name left out, [`global`]), for `port`.
fn resolved(answer: &serde_json::Value, port: u16) -> Vec<SocketAddr> {
    answer["Answer"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|record| matches!(record["type"].as_u64(), Some(1 | 28)))
        .filter_map(|record| record["data"].as_str()?.parse::<IpAddr>().ok())
        .filter(|ip| global(*ip))
        .map(|ip| SocketAddr::new(ip, port))
        .collect()
}

/// Whether `ip` is a global unicast address: not private, loopback,
/// link-local, shared (CGNAT, 100.64.0.0/10), multicast, unspecified,
/// broadcast, documentation, benchmarking or reserved; an IPv6 one not
/// unique-local, link-local or documentation either, and one mapping an
/// IPv4 address as that address. The second lookup's answer never sends a
/// try into the lamp's own network.
fn global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (18..20).contains(&b))
                || (a == 192 && b == 0 && v4.octets()[2] == 0))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return global(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || (first == 0x2001 && v6.segments()[1] == 0x0db8))
        }
    }
}

/// One try: a client of its own, keeping no connection, so its socket is
/// new; sent to `address` alone; `opened` told its index once its
/// connection's handshakes end.
async fn attempt(
    index: usize,
    url: String,
    domain: Option<String>,
    address: SocketAddr,
    open_within: Duration,
    shape: Shape,
    opened: mpsc::UnboundedSender<usize>,
) -> (usize, Result<serde_json::Value>) {
    let told: OpeningTold = Arc::new(move |opening| {
        if matches!(opening, Opening::Opened(_)) {
            let _ = opened.send(index);
        }
    });
    let builder = match kept_builder(None) {
        Ok(builder) => builder,
        Err(error) => return (index, Err(error)),
    };
    let mut builder = builder
        .connect_timeout(open_within)
        .pool_max_idle_per_host(0)
        .connector_layer(Opens::telling(told));
    if let Some(domain) = &domain {
        builder = builder.resolve(domain, address);
    }
    let client = match shape(builder, address).build() {
        Ok(client) => client,
        Err(_) => return (index, Err(Error::Transport("HTTPS client setup"))),
    };
    (index, fetch_json_on(&client, &url, None).await)
}

/// The journal's line for a try that opened first: which, of how many
/// begun, and when. No word of the URL.
fn told_opened(winner: usize, begun: usize, addresses: usize, began: Instant) {
    eprintln!(
        "lamp-voice second: a public service's connection {} of {begun} begun ({addresses} addresses) opened first, after {} ms",
        winner + 1,
        began.elapsed().as_millis()
    );
}

/// The journal's line for the second lookup: how many addresses it added,
/// or why it gave none, and how long it took. No word of the name.
fn told_elsewhere(more: &Result<usize>, asked_at: Instant) {
    let took = asked_at.elapsed().as_millis();
    match more {
        Ok(more) => eprintln!(
            "lamp-voice second: a public service's name looked up through another resolver gave {more} address(es) more, after {took} ms"
        ),
        Err(error) => eprintln!(
            "lamp-voice second: a public service's name looked up through another resolver failed after {took} ms: {error}"
        ),
    }
}

fn told_unopened(began: Instant, begun: usize, addresses: usize) {
    eprintln!(
        "lamp-voice second: no connection to a public service opened in {} ms, {begun} begun ({addresses} addresses)",
        began.elapsed().as_millis()
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The first `hung` connections a client with this layer opens never
    /// open, as a SYN Open-Meteo's api host never answered (lamp-9adf, 17
    /// Nov); the rest open as they would, `slow` later. `began` counts them
    /// all, across the clients it is given to.
    #[derive(Clone)]
    struct FirstHang {
        hung: usize,
        slow: Duration,
        began: Arc<AtomicUsize>,
    }

    impl<S> tower_layer::Layer<S> for FirstHang {
        type Service = Hanging<S>;

        fn layer(&self, inner: S) -> Hanging<S> {
            Hanging {
                inner,
                hang: self.clone(),
            }
        }
    }

    #[derive(Clone)]
    struct Hanging<S> {
        inner: S,
        hang: FirstHang,
    }

    impl<S, R> tower_service::Service<R> for Hanging<S>
    where
        S: tower_service::Service<R>,
        S::Future: Send + 'static,
        S::Response: 'static,
        S::Error: 'static,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = std::pin::Pin<
            Box<
                dyn std::future::Future<Output = std::result::Result<S::Response, S::Error>> + Send,
            >,
        >;

        fn poll_ready(
            &mut self,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::result::Result<(), S::Error>> {
            self.inner.poll_ready(context)
        }

        fn call(&mut self, request: R) -> Self::Future {
            let opening = self.inner.call(request);
            if self.hang.began.fetch_add(1, Ordering::SeqCst) < self.hang.hung {
                drop(opening);
                return Box::pin(std::future::pending());
            }
            let slow = self.hang.slow;
            Box::pin(async move {
                tokio::time::sleep(slow).await;
                opening.await
            })
        }
    }

    fn hanging_first(hung: usize) -> (Arc<AtomicUsize>, Ways) {
        hanging_first_then_slow(hung, Duration::ZERO)
    }

    fn hanging_first_then_slow(hung: usize, slow: Duration) -> (Arc<AtomicUsize>, Ways) {
        let began = Arc::new(AtomicUsize::new(0));
        let hang = FirstHang {
            hung,
            slow,
            began: began.clone(),
        };
        (
            began,
            Ways {
                elsewhere: None,
                shape: Arc::new(move |builder, _| builder.connector_layer(hang.clone())),
                ..Ways::lamps()
            },
        )
    }

    /// A plain local server answering every request with `body`.
    async fn answering(body: impl Into<String>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body: Arc<str> = body.into().into();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move {
                    let mut read = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !read.windows(4).any(|end| end == b"\r\n\r\n") {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => read.extend_from_slice(&buffer[..n]),
                        }
                    }
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(answer.as_bytes()).await;
                });
            }
        });
        format!("http://{address}/v1/forecast")
    }

    /// Two connections that never open: the third, begun 1 s in ([`GAPS`]),
    /// is asked, all within about a second and a half.
    #[tokio::test]
    async fn the_first_connection_to_open_of_those_raced_is_asked() {
        let url = answering(r#"{"ok":1}"#).await;
        let (began, shape) = hanging_first(2);
        let asked = std::time::Instant::now();
        let answered =
            tokio::time::timeout(Duration::from_millis(1500), raced(&url, OPEN_WITHIN, shape))
                .await;
        let took = asked.elapsed();
        assert_eq!(
            answered.ok().and_then(|answered| answered.ok()),
            Some(serde_json::json!({"ok": 1})),
            "answered on the third connection, after {took:?}"
        );
        assert_eq!(began.load(Ordering::SeqCst), 3);
        assert!(took >= GAPS[0] + GAPS[1], "{took:?}");
    }

    /// The first try opens: it alone is begun and asked, at once, whatever
    /// the bound it was given.
    #[tokio::test]
    async fn a_first_try_that_opens_is_asked_alone_and_at_once() {
        let url = answering(r#"{"ok":1}"#).await;
        for open_within in [OPEN_WITHIN, Duration::from_secs(5)] {
            let (began, shape) = hanging_first(0);
            let asked = std::time::Instant::now();
            let answered = raced(&url, open_within, shape).await;
            let took = asked.elapsed();
            assert_eq!(answered.ok(), Some(serde_json::json!({"ok": 1})));
            assert_eq!(began.load(Ordering::SeqCst), 1);
            assert!(took < STAGGER, "{took:?}");
        }
    }

    /// The tries' starts while none opens: further apart within a longer
    /// bound, the last a SYN resend's 1 s after the third (0, 0.4, 1, 2 s),
    /// and drawn in to leave the last time to open within [`OPEN_WITHIN`]
    /// (0, 0.4, 1, 1.4 s).
    #[test]
    fn later_tries_begin_further_apart_as_the_bound_allows() {
        let starts = |open_within: Duration| {
            let began = Instant::now();
            let deadline = began + open_within;
            let mut at = vec![began];
            for index in 1..MOST_TRIES {
                let previous = *at.last().unwrap();
                at.push(next_begins(index, previous, deadline));
            }
            at.iter()
                .map(|at| at.duration_since(began).as_millis())
                .collect::<Vec<_>>()
        };
        assert_eq!(starts(Duration::from_secs(4)), [0, 400, 1000, 2000]);
        assert_eq!(starts(OPEN_WITHIN), [0, 400, 1000, 1400]);
        assert_eq!(starts(Duration::from_secs(1)), [0, 400, 800, 1200]);
    }

    /// lamp-9adf on 18 Nov (#7593): a forecast host with one address whose
    /// first three tries never open and whose fourth takes 1.2 s. Within
    /// what is left of the weather's bound (4 s here) the fourth, begun 2 s
    /// in, opens and is asked; within [`OPEN_WITHIN`], as before, it is
    /// begun at 1.4 s and cut at 2.5 s.
    #[tokio::test]
    async fn a_late_try_that_opens_within_a_longer_bound_is_asked() {
        let url = answering(r#"{"ok":1}"#).await;
        let slow = Duration::from_millis(1200);
        let (began_long, long) = hanging_first_then_slow(3, slow);
        let (began_short, short) = hanging_first_then_slow(3, slow);
        let asked = std::time::Instant::now();
        let (within_long, within_short) = tokio::join!(
            async {
                let answered = raced(&url, Duration::from_secs(4), long).await;
                (answered, asked.elapsed())
            },
            raced(&url, OPEN_WITHIN, short),
        );
        let (answered, took) = within_long;
        assert_eq!(answered.ok(), Some(serde_json::json!({"ok": 1})));
        assert_eq!(began_long.load(Ordering::SeqCst), MOST_TRIES);
        assert!(
            took >= GAPS.iter().sum::<Duration>() + slow && took < Duration::from_secs(4),
            "{took:?}"
        );
        assert!(
            matches!(within_short, Err(Error::Timeout(OPENING))),
            "{within_short:?}"
        );
        assert_eq!(began_short.load(Ordering::SeqCst), MOST_TRIES);
    }

    /// No connection ever opens: the request fails once [`OPEN_WITHIN`] has
    /// passed, [`MOST_TRIES`] begun, well inside the weather's own bound.
    #[tokio::test]
    async fn a_request_whose_connections_never_open_ends_within_its_bound() {
        let url = answering(r#"{"ok":1}"#).await;
        let (began, shape) = hanging_first(usize::MAX);
        let asked = std::time::Instant::now();
        let answered = tokio::time::timeout(
            super::super::weather::WITHIN,
            raced(&url, OPEN_WITHIN, shape),
        )
        .await
        .expect("ended within the weather's bound");
        let took = asked.elapsed();
        assert!(
            matches!(answered, Err(Error::Timeout(OPENING))),
            "{answered:?}"
        );
        assert!(
            took >= OPEN_WITHIN && took < OPEN_WITHIN + Duration::from_millis(500),
            "{took:?}"
        );
        assert_eq!(began.load(Ordering::SeqCst), MOST_TRIES);
    }

    /// Nothing listening: each try fails at once, the next begins at once,
    /// and the request fails once all have, long before its bound.
    #[tokio::test]
    async fn tries_refused_at_once_begin_the_next_at_once() {
        let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/forecast", gone.local_addr().unwrap());
        drop(gone);
        let (began, shape) = hanging_first(0);
        let asked = std::time::Instant::now();
        let answered = raced(&url, OPEN_WITHIN, shape).await;
        assert!(
            matches!(answered, Err(Error::Transport(OPENING))),
            "{answered:?}"
        );
        assert!(asked.elapsed() < STAGGER, "{:?}", asked.elapsed());
        assert_eq!(began.load(Ordering::SeqCst), MOST_TRIES);
    }

    /// The address [`one_address_dead`]'s name is given by the lamp's own
    /// resolver: it never opens, as 51.81.167.44 didn't from lamp-9adf.
    const DEAD: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 2));

    /// Ways to a name whose one address ([`DEAD`]) never opens, as the
    /// lamp's resolver gave the forecast host on lamp-9adf on 18 Nov
    /// (#7593); `elsewhere`, a second lookup that answers 300 ms after it
    /// is asked with that address and a second, 127.0.0.1, that opens.
    /// `sent` counts the tries to each, the dead one's first.
    fn one_dead_address(elsewhere: bool) -> (Arc<[AtomicUsize; 2]>, Ways) {
        let sent = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
        let counted = sent.clone();
        let never = FirstHang {
            hung: usize::MAX,
            slow: Duration::ZERO,
            began: Arc::new(AtomicUsize::new(0)),
        };
        let ways = Ways {
            lookup: Arc::new(|_, port| {
                Box::pin(async move { Ok(vec![SocketAddr::new(DEAD, port)]) })
            }),
            elsewhere: elsewhere.then(|| -> Lookup {
                Arc::new(|_, port| {
                    Box::pin(async move {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        Ok(vec![
                            SocketAddr::new(DEAD, port),
                            SocketAddr::from(([127, 0, 0, 1], port)),
                        ])
                    })
                })
            }),
            shape: Arc::new(move |builder, address| {
                if address.ip() == DEAD {
                    counted[0].fetch_add(1, Ordering::SeqCst);
                    builder.connector_layer(never.clone())
                } else {
                    counted[1].fetch_add(1, Ordering::SeqCst);
                    builder
                }
            }),
        };
        (sent, ways)
    }

    /// `body` fetched from a name whose one address never opens, the name
    /// looked up again another way with `elsewhere`, within `open_within`:
    /// for the weather's own test of its 6 s.
    pub(crate) async fn one_address_dead(
        body: String,
        open_within: Duration,
        elsewhere: bool,
    ) -> Result<serde_json::Value> {
        let local = answering(body).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        raced(&url, open_within, one_dead_address(elsewhere).1).await
    }

    /// lamp-9adf on 18 Nov (#7593): the forecast host's one address never
    /// opens. Looked up again another way once the first try has stalled
    /// past a healthy open ([`ELSEWHERE_AFTER`], 1.5 s), the address it adds
    /// is tried at once (1.8 s) and asked, within what a forecast has to
    /// open after a place looked up in 1.2 s (4 s), inside the weather's
    /// 6 s.
    #[tokio::test]
    async fn a_name_whose_only_address_never_opens_is_reached_through_a_second_lookup() {
        let local = answering(r#"{"ok":1}"#).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        let (sent, ways) = one_dead_address(true);
        let open_within = super::super::weather::forecast_open_within(
            super::super::weather::WITHIN - Duration::from_millis(1200),
        );
        let asked = std::time::Instant::now();
        let answered = raced(&url, open_within, ways).await;
        let took = asked.elapsed();
        assert_eq!(
            answered.ok(),
            Some(serde_json::json!({"ok": 1})),
            "{took:?}"
        );
        assert!(
            took >= ELSEWHERE_AFTER + Duration::from_millis(300)
                && took < ELSEWHERE_AFTER + Duration::from_millis(1000),
            "{took:?}"
        );
        assert_eq!(
            sent[0].load(Ordering::SeqCst),
            3,
            "the dead address at 0, 0.4 and 1 s"
        );
        assert_eq!(
            sent[1].load(Ordering::SeqCst),
            1,
            "the address added, at once"
        );
    }

    /// With no other way, or another way that finds no address more, the
    /// same request still ends at its bound with the connection unopened,
    /// [`MOST_TRIES`] begun, inside the weather's 6 s.
    #[tokio::test]
    async fn a_name_whose_only_address_never_opens_with_no_other_way_ends_within_its_bound() {
        let local = answering(r#"{"ok":1}"#).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        let open_within = super::super::weather::forecast_open_within(
            super::super::weather::WITHIN - Duration::from_millis(1200),
        );
        let (sent_none, none) = one_dead_address(false);
        let (sent_same, mut same) = one_dead_address(false);
        same.elsewhere = Some(Arc::new(|_, port| {
            Box::pin(async move { Ok(vec![SocketAddr::new(DEAD, port)]) })
        }));
        let asked = std::time::Instant::now();
        let (none, same) = tokio::join!(
            raced(&url, open_within, none),
            raced(&url, open_within, same)
        );
        let took = asked.elapsed();
        for (answered, sent) in [(none, sent_none), (same, sent_same)] {
            assert!(
                matches!(answered, Err(Error::Timeout(OPENING))),
                "{answered:?}"
            );
            assert_eq!(sent[0].load(Ordering::SeqCst), MOST_TRIES);
            assert_eq!(sent[1].load(Ordering::SeqCst), 0);
        }
        assert!(
            took >= open_within && took < open_within + Duration::from_millis(500),
            "{took:?}"
        );
    }

    /// A JSON DNS answer's addresses, an alias's name left out, for the
    /// port asked.
    #[test]
    fn a_json_dns_answer_gives_its_addresses() {
        let answer = serde_json::json!({"Status": 0, "Answer": [
            {"name": "api.example.", "type": 5, "data": "edge.example."},
            {"name": "edge.example.", "type": 1, "data": "157.90.83.168"},
            {"name": "edge.example.", "type": 28, "data": "2a01:4f8::1"}
        ]});
        assert_eq!(
            resolved(&answer, 443),
            [
                "157.90.83.168:443".parse::<SocketAddr>().unwrap(),
                "[2a01:4f8::1]:443".parse().unwrap()
            ]
        );
        assert!(resolved(&serde_json::json!({"Status": 3}), 443).is_empty());
    }

    /// The second lookup on the lamp's network: Google's resolver answers
    /// over HTTPS by its address. Not run by default (it needs the
    /// network); `cargo test -- --ignored elsewhere_answers`.
    #[tokio::test]
    #[ignore]
    async fn elsewhere_answers_on_the_network() {
        let found = elsewhere("api.open-meteo.com".into(), 443).await;
        assert!(
            matches!(&found, Ok(found) if !found.is_empty()),
            "{found:?}"
        );
    }

    /// A connector that refuses every connection at once, as an address
    /// nothing listens at does.
    #[derive(Clone)]
    struct Refusing;

    impl<S> tower_layer::Layer<S> for Refusing {
        type Service = Refused<S>;

        fn layer(&self, inner: S) -> Refused<S> {
            Refused(inner)
        }
    }

    #[derive(Clone)]
    struct Refused<S>(S);

    impl<S, R> tower_service::Service<R> for Refused<S>
    where
        S: tower_service::Service<R>,
        S::Response: Send + 'static,
        S::Error: From<std::io::Error> + Send + 'static,
    {
        type Response = S::Response;
        type Error = S::Error;
        type Future = std::pin::Pin<
            Box<
                dyn std::future::Future<Output = std::result::Result<S::Response, S::Error>> + Send,
            >,
        >;

        fn poll_ready(
            &mut self,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::result::Result<(), S::Error>> {
            self.0.poll_ready(context)
        }

        fn call(&mut self, _: R) -> Self::Future {
            Box::pin(std::future::ready(Err(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused,
            )
            .into())))
        }
    }

    /// A second lookup that counts how often it is asked, and answers with
    /// `found` at once.
    fn counted_lookup(found: SocketAddr) -> (Arc<AtomicUsize>, Lookup) {
        let asked = Arc::new(AtomicUsize::new(0));
        let count = asked.clone();
        (
            asked,
            Arc::new(move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { Ok(vec![found]) })
            }),
        )
    }

    /// A healthy but slow open, 0.8 s as Vietnam to Germany may take, never
    /// asks the second lookup: the first try opens and is asked, the
    /// second begun at 0.4 s is dropped, and nothing goes to Google.
    #[tokio::test]
    async fn a_healthy_open_at_eight_tenths_of_a_second_never_asks_the_second_lookup() {
        let local = answering(r#"{"ok":1}"#).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        let alive: SocketAddr = local
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let (asked_elsewhere, elsewhere) = counted_lookup(alive);
        let (began, slow) = hanging_first_then_slow(0, Duration::from_millis(800));
        let ways = Ways {
            lookup: Arc::new(move |_, _| Box::pin(async move { Ok(vec![alive]) })),
            elsewhere: Some(elsewhere),
            ..slow
        };
        let asked = std::time::Instant::now();
        let answered = raced(&url, Duration::from_secs(4), ways).await;
        let took = asked.elapsed();
        assert_eq!(answered.ok(), Some(serde_json::json!({"ok": 1})));
        assert!(took < Duration::from_millis(1100), "{took:?}");
        assert_eq!(began.load(Ordering::SeqCst), 2, "tries at 0 and 0.4 s");
        assert_eq!(asked_elsewhere.load(Ordering::SeqCst), 0);
    }

    /// A try that fails outright (nothing listening at the one address the
    /// lamp's resolver gave) asks the second lookup at once, and the
    /// address it adds is asked well before [`ELSEWHERE_AFTER`].
    #[tokio::test]
    async fn a_try_failed_outright_asks_the_second_lookup_at_once() {
        let local = answering(r#"{"ok":1}"#).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        let alive: SocketAddr = local
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // A try to the URL's own port goes there whatever address it is
        // sent to, so the refusal is the connector's own, at once.
        let refused = SocketAddr::new(DEAD, alive.port());
        let (asked_elsewhere, elsewhere) = counted_lookup(alive);
        let ways = Ways {
            lookup: Arc::new(move |_, _| Box::pin(async move { Ok(vec![refused]) })),
            elsewhere: Some(elsewhere),
            shape: Arc::new(|builder, address| {
                if address.ip() == DEAD {
                    builder.connector_layer(Refusing)
                } else {
                    builder
                }
            }),
        };
        let asked = std::time::Instant::now();
        let answered = raced(&url, Duration::from_secs(4), ways).await;
        let took = asked.elapsed();
        assert_eq!(
            answered.ok(),
            Some(serde_json::json!({"ok": 1})),
            "{took:?}"
        );
        assert!(took < STAGGER, "{took:?}");
        assert_eq!(asked_elsewhere.load(Ordering::SeqCst), 1);
    }

    /// One address that fails at once (as IPv6 does on a lamp with no IPv6
    /// route) beside a healthy one that opens in 0.8 s: the healthy one is
    /// asked and the second lookup never is, as not every address failed.
    #[tokio::test]
    async fn one_address_refused_beside_a_healthy_one_never_asks_the_second_lookup() {
        let local = answering(r#"{"ok":1}"#).await;
        let url = local.replace("127.0.0.1", "forecast.test");
        let alive: SocketAddr = local
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let refused = SocketAddr::new(DEAD, alive.port());
        let (asked_elsewhere, elsewhere) = counted_lookup(alive);
        let slow = FirstHang {
            hung: 0,
            slow: Duration::from_millis(800),
            began: Arc::new(AtomicUsize::new(0)),
        };
        let ways = Ways {
            lookup: Arc::new(move |_, _| Box::pin(async move { Ok(vec![refused, alive]) })),
            elsewhere: Some(elsewhere),
            shape: Arc::new(move |builder, address| {
                if address.ip() == DEAD {
                    builder.connector_layer(Refusing)
                } else {
                    builder.connector_layer(slow.clone())
                }
            }),
        };
        let asked = std::time::Instant::now();
        let answered = raced(&url, Duration::from_secs(4), ways).await;
        let took = asked.elapsed();
        assert_eq!(
            answered.ok(),
            Some(serde_json::json!({"ok": 1})),
            "{took:?}"
        );
        assert!(took < Duration::from_millis(1100), "{took:?}");
        assert_eq!(asked_elsewhere.load(Ordering::SeqCst), 0);
    }

    /// `LAMP_WEATHER_DOH`: off unless exactly `on`; off, the
    /// lamp's ways have no second lookup at all.
    #[test]
    fn the_second_lookup_is_off_unless_switched_on() {
        assert!(doh_on(Some("on")));
        assert!(!doh_on(None));
        assert!(!doh_on(Some("")));
        assert!(!doh_on(Some("off")));
        assert!(!doh_on(Some("ON")));
        assert!(!doh_on(Some("yes")));
        assert!(Ways::lamps_with(true).elsewhere.is_some());
        assert!(Ways::lamps_with(false).elsewhere.is_none());
    }

    /// Only global unicast addresses of a second lookup's answer are raced.
    #[test]
    fn a_second_lookups_private_and_special_addresses_are_left_out() {
        let record = |data: &str| {
            let kind = if data.contains(':') { 28 } else { 1 };
            serde_json::json!({"type": kind, "data": data})
        };
        let left_out = [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.254",
            "224.0.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.1",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:192.168.1.1",
        ];
        let kept = [
            "157.90.83.168",
            "100.128.0.1",
            "2a01:4f8::1",
            "::ffff:157.90.83.168",
        ];
        let answer = serde_json::json!({"Answer": left_out
            .iter()
            .chain(kept.iter())
            .map(|data| record(data))
            .collect::<Vec<_>>()});
        let found: Vec<IpAddr> = resolved(&answer, 443).iter().map(SocketAddr::ip).collect();
        let expected: Vec<IpAddr> = kept.iter().map(|ip| ip.parse().unwrap()).collect();
        assert_eq!(found, expected);
    }
}
