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
use super::{
    Error, OPENING, Result, fetch_json_on, kept_builder,
    watch::{Opening, OpeningTold, Opens},
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet, time::Instant};

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

/// How each try's client is made beyond the lamp's own: unchanged on the
/// lamp, a connector that never opens in a test.
type Shape = Arc<dyn Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync>;

/// A JSON `GET` of `url`, bounded as [`super::get_json`] is once its
/// connection is open, which it must be within `open_within` from now
/// (the weather's place [`OPEN_WITHIN`], its forecast what is left of its
/// own bound).
pub(crate) async fn get_public_json_within(
    url: &str,
    open_within: Duration,
) -> Result<serde_json::Value> {
    raced(url, open_within, Arc::new(|builder| builder)).await
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

async fn raced(url: &str, open_within: Duration, shape: Shape) -> Result<serde_json::Value> {
    let began = Instant::now();
    let deadline = began + open_within;
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::Url)?;
    let port = parsed.port_or_known_default().ok_or(Error::Url)?;
    let host = parsed.host_str().ok_or(Error::Url)?;
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    let (domain, addresses) = match literal.parse::<std::net::IpAddr>() {
        Ok(ip) => (None, vec![SocketAddr::new(ip, port)]),
        Err(_) => (
            Some(host.to_owned()),
            addresses_of(host, port, deadline).await?,
        ),
    };
    let (opened_tx, mut opened) = mpsc::unbounded_channel::<usize>();
    let mut tries = JoinSet::new();
    let mut handles = Vec::new();
    let mut begin = |index: usize, tries: &mut JoinSet<(usize, Result<serde_json::Value>)>| {
        handles.push(tries.spawn(attempt(
            index,
            url.to_owned(),
            domain.clone(),
            addresses[index % addresses.len()],
            open_within,
            shape.clone(),
            opened_tx.clone(),
        )));
    };
    begin(0, &mut tries);
    let mut begun = 1;
    let mut next = next_begins(1, began, deadline);
    let winner = loop {
        tokio::select! {
            biased;
            Some(index) = opened.recv() => break index,
            joined = tries.join_next(), if !tries.is_empty() => match joined {
                Some(Ok((_, Err(Error::Transport(OPENING) | Error::Timeout(OPENING))))) | Some(Err(_)) => {
                    // A try that couldn't open (an address the lamp has no
                    // route to fails at once): the next begins now.
                    if begun < MOST_TRIES {
                        let now = Instant::now();
                        begin(begun, &mut tries);
                        begun += 1;
                        next = next_begins(begun, now, deadline);
                    } else if tries.is_empty() {
                        told_unopened(began, begun, addresses.len());
                        return Err(Error::Transport(OPENING));
                    }
                }
                Some(Ok((index, answered))) => {
                    told_opened(index, begun, addresses.len(), began);
                    return answered;
                }
                None => {}
            },
            _ = tokio::time::sleep_until(next), if begun < MOST_TRIES => {
                begin(begun, &mut tries);
                begun += 1;
                next = next_begins(begun, next, deadline);
            }
            _ = tokio::time::sleep_until(deadline) => {
                told_unopened(began, begun, addresses.len());
                return Err(Error::Timeout(OPENING));
            }
        }
    };
    told_opened(winner, begun, addresses.len(), began);
    for (index, handle) in handles.iter().enumerate() {
        if index != winner {
            handle.abort();
        }
    }
    while let Some(joined) = tries.join_next().await {
        if let Ok((index, answered)) = joined
            && index == winner
        {
            return answered;
        }
    }
    Err(Error::Transport("request"))
}

/// Every address `domain` has, each once, in the resolver's order.
async fn addresses_of(domain: &str, port: u16, deadline: Instant) -> Result<Vec<SocketAddr>> {
    let found = tokio::time::timeout_at(deadline, tokio::net::lookup_host((domain, port)))
        .await
        .map_err(|_| Error::Timeout("address"))?
        .map_err(|_| Error::Transport("address"))?;
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
    let client = match shape(builder).build() {
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

fn told_unopened(began: Instant, begun: usize, addresses: usize) {
    eprintln!(
        "lamp-voice second: no connection to a public service opened in {} ms, {begun} begun ({addresses} addresses)",
        began.elapsed().as_millis()
    );
}

#[cfg(test)]
mod tests {
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

    fn hanging_first(hung: usize) -> (Arc<AtomicUsize>, Shape) {
        hanging_first_then_slow(hung, Duration::ZERO)
    }

    fn hanging_first_then_slow(hung: usize, slow: Duration) -> (Arc<AtomicUsize>, Shape) {
        let began = Arc::new(AtomicUsize::new(0));
        let hang = FirstHang {
            hung,
            slow,
            began: began.clone(),
        };
        (
            began,
            Arc::new(move |builder| builder.connector_layer(hang.clone())),
        )
    }

    /// A plain local server answering every request with `body`.
    async fn answering(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
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
}
