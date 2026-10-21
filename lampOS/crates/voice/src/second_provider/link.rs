//! The second route's link when it is the lamp's only voice (#1844). Before,
//! the worker said Ready once at start and never again, so with the Wi-Fi
//! off every turn failed in silence: the coordinator's offline line, its
//! breath, "I'm back online" and `lamp/online` all follow the link, and it
//! never went down.
//!
//! Now a turn that fails on the network (a connection refused or reset, a
//! connect that timed out, an answer that never came) asks whether the
//! services' hosts answer at all: a bare TCP connection to each, at once,
//! nothing sent, any answering enough, bounded at [`PROBE_WITHIN`]. One
//! answering: a service was slow or refused, the lamp is not offline, and
//! nothing changes. None: the link is Unavailable (`reason` the turn's
//! fault), and the same check runs every [`PROBE_EVERY_US`] until one
//! answers, when it is Ready again in a new session. Offline speech is a
//! local-only turn that never reaches this worker, so without the check the
//! link could never come back. Pure state here; the worker runs the probes.
use crate::provider_link::{LinkContext, ProviderLink, ProviderLinkState};
use crate::provider_worker::{Fault, ProviderOutput};
use std::time::Duration;

/// How long a check waits for any host to answer.
pub const PROBE_WITHIN: Duration = Duration::from_secs(2);
/// How often a down link checks again: every 5 s, so the link is back
/// within seconds of the network (#1844: on lamp-06c1 at 30 s the Wi-Fi
/// was back 38 s before the lamp knew, 20 Oct 16:15). A check is a bare
/// connection to each host, nothing sent.
pub const PROBE_EVERY_US: u64 = 5_000_000;
/// While down, how often the kernel's routes are read: a default route
/// appearing (the Wi-Fi rejoined) checks at once ([`Watch::tick`]).
pub const ROUTE_EVERY_US: u64 = 500_000;
/// Where the kernel lists its IPv4 routes.
pub const ROUTES: &str = "/proc/net/route";

/// Whether the kernel's route table (`/proc/net/route`'s text) has a
/// default route that is up: destination 0.0.0.0 with the up flag.
pub fn has_default_route(table: &str) -> bool {
    table.lines().skip(1).any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.get(1) == Some(&"00000000")
            && fields
                .get(3)
                .and_then(|flags| u32::from_str_radix(flags, 16).ok())
                .is_some_and(|flags| flags & 0x1 != 0)
    })
}

/// Whether a turn's fault says the network may be gone, rather than a
/// service that answered and refused (a status) or misspoke.
pub fn network(fault: &Fault) -> bool {
    matches!(
        fault,
        Fault::ConnectTimeout | Fault::Transport | Fault::ResponseTimeout
    )
}

/// Whether a turn lost on the network may be asked once more when a host
/// answers again (`LAMP_RESEND_LOST_LINE`, the first builder's 00:02 on
/// #1722): [`network`], less the answer that never came
/// (`ResponseTimeout`), which a second try would only wait for again, as
/// Live's `resend::mendable` holds. The rule this route's resend and the
/// link's check share: the same faults are the network's, and the link is
/// back when any service host takes a bare connection ([`any_answers`]).
pub fn lost(fault: &Fault) -> bool {
    network(fault) && *fault != Fault::ResponseTimeout
}

/// `host:port` of a service URL (`https://`, `wss://`, `http://`, `ws://`),
/// for a bare connection: the URL's own port, or the scheme's.
pub fn target(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let port = match scheme {
        "https" | "wss" => 443,
        "http" | "ws" => 80,
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    if authority.is_empty() {
        return None;
    }
    // A port of its own: `host:port`, or `[v6]:port`.
    let has_port = if authority.starts_with('[') {
        authority.contains("]:")
    } else {
        authority.contains(':')
    };
    Some(if has_port {
        authority.to_owned()
    } else {
        format!("{authority}:{port}")
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Up,
    /// A network failure at `since_us`: a check is running.
    Checking {
        since_us: u64,
        fault: Fault,
    },
    /// No host answered: checked again at `probe_at_us`.
    Down {
        since_us: u64,
        fault: Fault,
        probe_at_us: u64,
    },
}

/// The link this worker tells the coordinator.
#[derive(Debug)]
pub struct Link {
    revision: u64,
    session: u64,
    state: State,
    /// How often a down link checks again: [`PROBE_EVERY_US`].
    every_us: u64,
}

impl Link {
    /// Ready, as the worker has always said at start.
    pub fn initial(started_us: u64, now_us: u64) -> (Self, ProviderLink) {
        let link = Self {
            revision: 1,
            session: 1,
            state: State::Up,
            every_us: PROBE_EVERY_US,
        };
        let ready = ProviderLink {
            revision: 1,
            session: 1,
            observed_at_us: started_us,
            source_clock: None,
            state: ProviderLinkState::Ready {
                setup_us: now_us.saturating_sub(started_us),
                attempts: 1,
                context: LinkContext::Initial,
                outage_started_at_us: None,
                after: None,
            },
        };
        (link, ready)
    }

    /// A turn failed with `fault` at `now_us`: true when a check should run
    /// now (a network fault while the link is up and no check is running).
    pub fn failed(&mut self, now_us: u64, fault: Fault) -> bool {
        if self.state != State::Up || !network(&fault) {
            return false;
        }
        self.state = State::Checking {
            since_us: now_us,
            fault,
        };
        true
    }

    /// The network's route came back while down: the next check is now.
    pub fn route_back(&mut self, now_us: u64) {
        if let State::Down { probe_at_us, .. } = &mut self.state {
            *probe_at_us = (*probe_at_us).min(now_us);
        }
    }

    /// Whether a down link's next check is due.
    pub fn probe_due(&self, now_us: u64) -> bool {
        matches!(self.state, State::Down { probe_at_us, .. } if now_us >= probe_at_us)
    }

    /// Whether the link is down now.
    pub fn down(&self) -> bool {
        matches!(self.state, State::Down { .. })
    }

    /// A check that began at `began_us` ended at `now_us`, a host answering
    /// or none: the link to tell the coordinator, if it changed.
    pub fn probed(&mut self, began_us: u64, now_us: u64, answered: bool) -> Option<ProviderLink> {
        match (self.state, answered) {
            (State::Checking { .. }, true) => {
                self.state = State::Up;
                None
            }
            (State::Checking { since_us, fault }, false) => {
                self.state = State::Down {
                    since_us,
                    fault,
                    probe_at_us: now_us + self.every_us,
                };
                self.revision += 1;
                Some(ProviderLink {
                    revision: self.revision,
                    session: self.session,
                    observed_at_us: now_us,
                    source_clock: None,
                    state: ProviderLinkState::Unavailable { reason: fault },
                })
            }
            (
                State::Down {
                    since_us, fault, ..
                },
                true,
            ) => {
                self.state = State::Up;
                self.revision += 1;
                self.session += 1;
                Some(ProviderLink {
                    revision: self.revision,
                    session: self.session,
                    observed_at_us: now_us,
                    source_clock: None,
                    state: ProviderLinkState::Ready {
                        setup_us: now_us.saturating_sub(began_us).max(1),
                        attempts: 1,
                        context: LinkContext::Fresh,
                        outage_started_at_us: Some(since_us.max(1)),
                        after: Some(fault),
                    },
                })
            }
            (
                State::Down {
                    since_us, fault, ..
                },
                false,
            ) => {
                self.state = State::Down {
                    since_us,
                    fault,
                    probe_at_us: now_us + self.every_us,
                };
                None
            }
            (State::Up, _) => None,
        }
    }
}

/// The link and the checks the worker runs for it, ticked from its loop.
/// Without targets (a standby, or a test) the link stays Ready.
pub struct Watch {
    link: Link,
    targets: Option<Vec<String>>,
    /// A check running, and when it began.
    probing: Option<(u64, tokio::task::JoinHandle<bool>)>,
    /// The route table read while down ([`ROUTES`]), when it was last read,
    /// and whether it had a default route then.
    routes: std::path::PathBuf,
    route_read: Option<(u64, bool)>,
}

impl Watch {
    /// The watch, and the Ready the worker says at start.
    pub fn new(targets: Option<Vec<String>>, started_us: u64, now_us: u64) -> (Self, ProviderLink) {
        let (link, ready) = Link::initial(started_us, now_us);
        let watch = Self {
            link,
            targets,
            probing: None,
            routes: ROUTES.into(),
            route_read: None,
        };
        (watch, ready)
    }

    /// An output the worker is about to send: a turn failed on the network
    /// starts a check.
    pub fn output(&mut self, now_us: u64, event: &ProviderOutput) {
        if let ProviderOutput::TurnFailed { reason, .. } = event
            && self.targets.is_some()
            && self.probing.is_none()
            && self.link.failed(now_us, *reason)
        {
            self.check(now_us);
        }
    }

    /// While down, every [`ROUTE_EVERY_US`]: a default route that wasn't
    /// there at the last read makes the next check now. Up, nothing is read.
    fn watch_routes(&mut self, now_us: u64) {
        if !self.link.down() {
            self.route_read = None;
            return;
        }
        if self
            .route_read
            .is_some_and(|(at, _)| now_us.saturating_sub(at) < ROUTE_EVERY_US)
        {
            return;
        }
        let routed = std::fs::read_to_string(&self.routes)
            .map(|table| has_default_route(&table))
            .unwrap_or(false);
        if routed && self.route_read.is_some_and(|(_, had)| !had) {
            self.link.route_back(now_us);
        }
        self.route_read = Some((now_us, routed));
    }

    fn check(&mut self, now_us: u64) {
        if let Some(targets) = &self.targets {
            self.probing = Some((now_us, tokio::spawn(any_answers(targets.clone()))));
        }
    }

    /// Once a tick: a due check begun, a finished one read. The link to
    /// tell the coordinator if it changed, and whether it came back.
    pub async fn tick(&mut self, now_us: u64) -> Option<(ProviderLink, bool)> {
        self.watch_routes(now_us);
        if self.probing.is_none() && self.link.probe_due(now_us) {
            self.check(now_us);
        }
        if !self
            .probing
            .as_ref()
            .is_some_and(|(_, check)| check.is_finished())
        {
            return None;
        }
        let (began, check) = self.probing.take()?;
        let answered = check.await.unwrap_or(false);
        let was_down = self.link.down();
        self.link
            .probed(began, now_us, answered)
            .map(|changed| (changed, was_down))
    }
}

/// Whether any of `targets` takes a TCP connection within [`PROBE_WITHIN`].
/// Nothing is sent on a connection made.
pub async fn any_answers(targets: Vec<String>) -> bool {
    let mut tries = tokio::task::JoinSet::new();
    for target in targets {
        tries.spawn(async move {
            matches!(
                tokio::time::timeout(PROBE_WITHIN, tokio::net::TcpStream::connect(target)).await,
                Ok(Ok(_))
            )
        });
    }
    while let Some(result) = tries.join_next().await {
        if result.unwrap_or(false) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// The rule the resend shares with the link's check (#1722): a line is
    /// asked again only for the network's faults the check runs for, less
    /// an answer that never came; never for a service that answered.
    #[test]
    fn a_line_is_lost_only_on_the_network_and_never_for_a_silent_answer() {
        for fault in [Fault::Transport, Fault::ConnectTimeout] {
            assert!(lost(&fault) && network(&fault), "{fault:?}");
        }
        assert!(network(&Fault::ResponseTimeout) && !lost(&Fault::ResponseTimeout));
        for fault in [
            Fault::RateLimited,
            Fault::Authentication,
            Fault::QuotaExceeded,
            Fault::ServerUnavailable,
            Fault::Protocol,
            Fault::Local,
        ] {
            assert!(!lost(&fault), "{fault:?}");
        }
    }

    #[test]
    fn a_network_failure_with_a_host_answering_changes_nothing() {
        let (mut link, ready) = Link::initial(S, S + 10);
        ready.validate(2 * S).unwrap();
        assert!(link.failed(10 * S, Fault::Transport));
        assert!(
            !link.failed(10 * S, Fault::Transport),
            "one check at a time"
        );
        assert_eq!(link.probed(10 * S, 11 * S, true), None);
        assert!(!link.down());
        // A service that answered and refused is not the network.
        assert!(!link.failed(20 * S, Fault::RateLimited));
        assert!(!link.failed(20 * S, Fault::Authentication));
        assert!(!link.failed(20 * S, Fault::ServerUnavailable));
    }

    #[test]
    fn no_host_answering_is_down_until_a_check_finds_one() {
        let (mut link, _) = Link::initial(S, S + 10);
        assert!(link.failed(10 * S, Fault::ConnectTimeout));
        let down = link.probed(10 * S, 12 * S, false).expect("down");
        down.validate(12 * S).unwrap();
        assert_eq!((down.revision, down.session), (2, 1));
        assert_eq!(
            down.state,
            ProviderLinkState::Unavailable {
                reason: Fault::ConnectTimeout
            }
        );
        assert!(link.down());
        // No further failure while down asks for another check.
        assert!(!link.failed(13 * S, Fault::Transport));
        // Checked every 5 s.
        assert!(!link.probe_due(12 * S + PROBE_EVERY_US - 1));
        assert!(link.probe_due(12 * S + PROBE_EVERY_US));
        assert_eq!(link.probed(42 * S, 44 * S, false), None);
        assert!(!link.probe_due(44 * S + PROBE_EVERY_US - 1));
        // Back: Ready in a new session, after the outage begun at 10 s.
        let back = link.probed(74 * S, 74 * S + 300_000, true).expect("back");
        back.validate(75 * S).unwrap();
        assert_eq!((back.revision, back.session), (3, 2));
        assert_eq!(
            back.state,
            ProviderLinkState::Ready {
                setup_us: 300_000,
                attempts: 1,
                context: LinkContext::Fresh,
                outage_started_at_us: Some(10 * S),
                after: Some(Fault::ConnectTimeout),
            }
        );
        assert!(!link.down());
        // And it can go down again.
        assert!(link.failed(90 * S, Fault::ResponseTimeout));
        assert_eq!(link.probed(90 * S, 92 * S, false).unwrap().revision, 4);
    }

    /// #1844 (lamp-06c1, 20 Oct): a down link checks every 5 s, and at once
    /// when the kernel's default route comes back.
    #[test]
    fn down_it_checks_every_five_seconds_and_at_once_when_the_route_comes_back() {
        assert_eq!(PROBE_EVERY_US, 5 * S);
        let table = |default: bool| {
            let mut text = String::from(
                "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                 wlan0\t0014A8AC\t00000000\t0001\t0\t0\t3006\t00FFFFFF\t0\t0\t0\n",
            );
            if default {
                text.push_str("wlan0\t00000000\t0114A8AC\t0003\t0\t0\t3006\t00000000\t0\t0\t0\n");
            }
            text
        };
        assert!(has_default_route(&table(true)));
        assert!(!has_default_route(&table(false)));
        assert!(!has_default_route(""));
        let dir = std::env::temp_dir().join(format!("link-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let routes = dir.join("route");
        std::fs::write(&routes, table(false)).unwrap();
        let (mut watch, _) = Watch::new(None, S, S + 10);
        watch.routes = routes.clone();
        assert!(watch.link.failed(10 * S, Fault::Transport));
        watch.link.probed(10 * S, 12 * S, false).expect("down");
        // No route yet: the next check stays at 17 s.
        watch.watch_routes(12 * S);
        assert!(!watch.link.probe_due(13 * S));
        // Read again only after 500 ms; the route is back: due now.
        std::fs::write(&routes, table(true)).unwrap();
        watch.watch_routes(12 * S + ROUTE_EVERY_US - 1);
        assert!(!watch.link.probe_due(13 * S), "read too soon");
        watch.watch_routes(12 * S + ROUTE_EVERY_US);
        assert!(watch.link.probe_due(12 * S + ROUTE_EVERY_US));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_services_url_gives_its_host_and_port() {
        assert_eq!(
            target("https://api.deepseek.com/v1").as_deref(),
            Some("api.deepseek.com:443")
        );
        assert_eq!(
            target("wss://api.deepgram.com/v1/listen?model=nova-3").as_deref(),
            Some("api.deepgram.com:443")
        );
        assert_eq!(
            target("http://127.0.0.1:8080/x").as_deref(),
            Some("127.0.0.1:8080")
        );
        assert_eq!(target("ftp://x"), None);
        assert_eq!(target("api.example.com"), None);
    }

    /// The worker's watch end to end with real connections: a turn failed
    /// on the network, nobody answering, the link down; a host answering
    /// again at the next check, the link back.
    #[tokio::test]
    async fn the_watch_takes_the_link_down_and_back_with_real_connections() {
        use crate::provider_worker::FailedStage;
        let port = {
            let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            gone.local_addr().unwrap().port()
        };
        let target = format!("127.0.0.1:{port}");
        let (mut watch, _) = Watch::new(Some(vec![target]), S, S + 10);
        watch.link.every_us = 50_000;
        let failed = ProviderOutput::TurnFailed {
            request: 7,
            stage: FailedStage::InputOpen,
            reason: Fault::Transport,
        };
        let mut now = 10 * S;
        watch.output(now, &failed);
        let mut seen = None;
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            now += 5_000;
            if let Some(changed) = watch.tick(now).await {
                seen = Some(changed);
                break;
            }
        }
        let (down, was_down) = seen.expect("down");
        assert!(!was_down);
        assert_eq!(
            down.state,
            ProviderLinkState::Unavailable {
                reason: Fault::Transport
            }
        );
        // The host answers again: back at a check.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let mut seen = None;
        for _ in 0..400 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            now += 5_000;
            if let Some(changed) = watch.tick(now).await {
                seen = Some(changed);
                break;
            }
        }
        let (back, was_down) = seen.expect("back");
        assert!(was_down);
        assert!(matches!(
            back.state,
            ProviderLinkState::Ready {
                context: LinkContext::Fresh,
                ..
            }
        ));
        back.validate(back.observed_at_us).unwrap();
        drop(listener);
        // A standby (no targets) never changes its link.
        let (mut standby, _) = Watch::new(None, S, S + 10);
        standby.output(now, &failed);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(standby.tick(now + S).await.is_none());
        assert!(!standby.link.down());
    }

    #[tokio::test]
    async fn a_listening_host_answers_and_a_closed_port_does_not() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().to_string();
        let closed = {
            let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            gone.local_addr().unwrap().to_string()
        };
        assert!(any_answers(vec![closed.clone(), open]).await);
        assert!(!any_answers(vec![closed]).await);
        assert!(!any_answers(Vec::new()).await);
    }
}
