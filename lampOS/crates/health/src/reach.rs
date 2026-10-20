//! Whether the internet answers while the voice's link is down
//! (`lamp-health serve --reach-probe on`, #830).
//!
//! `lamp/online` follows the voice's link to its speech service. On a
//! product lamp that link goes through the company's relay, so a relay
//! outage (17 Oct, 02:29) would read as no internet, and the lamp would say
//! so with the internet fine. While the link is down, and only then, this
//! asks the internet itself: a TCP connection to any of [`TARGETS`], at
//! most every [`PROBE_EVERY`], each attempt bounded
//! by [`PROBE_WITHIN`]. Nothing is sent on a connection made; it is closed
//! at once. The probe runs on its own thread, so the loop never waits.
use std::{
    net::{SocketAddr, TcpStream},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

/// What the probe connects to: two public name servers, on 443 only. Never
/// port 53: a router that forces its own resolver redirects 53 to itself,
/// and that resolver answers with the WAN down, so a dead network would
/// read as online (the reviewer on #1247). Any answering is enough (the
/// first builder on #830, 17 Oct). IPv4 only.
pub const TARGETS: [&str; 2] = ["1.1.1.1:443", "8.8.8.8:443"];
/// How often the internet is asked while the link is down.
pub const PROBE_EVERY: Duration = Duration::from_secs(30);
/// How long one probe may take, for all targets at once.
pub const PROBE_WITHIN: Duration = Duration::from_secs(2);
/// With the idle probe (`--reach-idle on`, #1844), how often the internet
/// is asked once a probe found it not answering, so its coming back is
/// seen in seconds: on lamp-06c1 the Wi-Fi was back 38 s before the lamp
/// knew (20 Oct 16:15).
pub const PROBE_EVERY_DOWN: Duration = Duration::from_secs(5);

/// The default targets, parsed.
pub fn targets() -> Vec<SocketAddr> {
    TARGETS
        .iter()
        .map(|target| target.parse().expect("a fixed socket address"))
        .collect()
}

/// The probe's state between report times.
pub struct Reach {
    targets: Vec<SocketAddr>,
    every: Duration,
    within: Duration,
    /// When the last probe ended, and whether the internet answered.
    last: Option<(Instant, bool)>,
    /// A probe under way, and when it began.
    running: Option<(Instant, Receiver<bool>)>,
    /// Also asked with the link up ([`Reach::idle`]).
    idle: bool,
    /// Whether the link was down at the last report time.
    link_was_down: bool,
}

impl Reach {
    pub fn new(targets: Vec<SocketAddr>) -> Self {
        Self::with_times(targets, PROBE_EVERY, PROBE_WITHIN)
    }

    pub fn with_times(targets: Vec<SocketAddr>, every: Duration, within: Duration) -> Self {
        Self {
            targets,
            every,
            within,
            last: None,
            running: None,
            idle: false,
            link_was_down: false,
        }
    }

    /// Asked with the link up too (`--reach-idle on`, #1844): on the second
    /// route as the only voice the link is said down only once a turn fails
    /// on the network, so with nobody speaking the lamp would say online
    /// through a whole outage (lamp-06c1, 20 Oct: 38 s and 13 s). Every
    /// [`PROBE_EVERY`] while the internet answers, every
    /// [`PROBE_EVERY_DOWN`] once it doesn't.
    pub fn idle(mut self) -> Self {
        self.idle = true;
        self
    }

    /// Each report time: takes a probe that has ended, and while the link is
    /// down starts one when the last is [`PROBE_EVERY`] old. With the link
    /// up, what was found is forgotten, so the next outage asks at once;
    /// idle ([`Reach::idle`]), it is kept and asked again with the link up,
    /// and forgotten only as the link comes back.
    pub fn tend(&mut self, link_down: bool, now: Instant) {
        // The link back: a probe of the outage still running is dropped too,
        // or its "no" would be taken after the reset (the reviewer-lead on
        // #2261).
        if !link_down && self.link_was_down {
            self.running = None;
        }
        if let Some((began, result)) = &self.running {
            match result.try_recv() {
                Ok(answered) => {
                    self.last = Some((now, answered));
                    self.running = None;
                }
                // A thread that ended without a word, or one past its bound
                // and a margin, answered nothing.
                Err(TryRecvError::Disconnected) => {
                    self.last = Some((now, false));
                    self.running = None;
                }
                Err(TryRecvError::Empty)
                    if now.saturating_duration_since(*began) > self.within * 2 =>
                {
                    self.last = Some((now, false));
                    self.running = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let went_down = link_down && !self.link_was_down;
        let came_up = !link_down && self.link_was_down;
        self.link_was_down = link_down;
        if !link_down && !self.idle {
            self.last = None;
            return;
        }
        // A new outage asks at once, idle or not; and idle, the link coming
        // back forgets the outage's verdict, so `lamp/online` is the link's
        // again at once, not offline from `reach` until the next probe (the
        // reviewer-lead on #2261).
        if went_down || came_up {
            self.last = None;
        }
        let due = self.last.is_none_or(|(at, answered)| {
            let every = if self.idle && !answered {
                PROBE_EVERY_DOWN.min(self.every)
            } else {
                self.every
            };
            now.saturating_duration_since(at) >= every
        });
        if due && self.running.is_none() {
            self.running = Some((now, probe(self.targets.clone(), self.within)));
        }
    }

    /// What the last probe in this outage found: the internet answering or
    /// not; none while no probe has ended in it.
    pub fn verdict(&self) -> Option<bool> {
        self.last.map(|(_, answered)| answered)
    }

    /// Whether the last probe, in this outage, found the internet answering.
    pub fn answers(&self) -> bool {
        self.verdict() == Some(true)
    }
}

/// Connects to every target at once, each on its own thread, bounded by
/// `within`; says whether any connected.
fn probe(targets: Vec<SocketAddr>, within: Duration) -> Receiver<bool> {
    let (tell, told) = mpsc::sync_channel(1);
    let _ = thread::Builder::new()
        .name("lamp-health-reach".into())
        .spawn(move || {
            let (each, all) = mpsc::channel();
            for target in &targets {
                let each = each.clone();
                let target = *target;
                let _ = thread::Builder::new()
                    .name("lamp-health-reach-one".into())
                    .spawn(move || {
                        let _ = each.send(TcpStream::connect_timeout(&target, within).is_ok());
                    });
            }
            drop(each);
            let until = Instant::now() + within;
            let mut answered = false;
            while let Some(left) = until.checked_duration_since(Instant::now()) {
                match all.recv_timeout(left) {
                    Ok(true) => {
                        answered = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(_) => break,
                }
            }
            let _ = tell.send(answered);
        });
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn settle(reach: &mut Reach, link_down: bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while reach.running.is_some() {
            assert!(Instant::now() < until, "the probe never ended");
            thread::sleep(Duration::from_millis(2));
            reach.tend(link_down, Instant::now());
        }
    }

    /// A port nothing listens on: refused at once.
    fn closed() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        address
    }

    #[test]
    fn the_internet_answering_while_the_link_is_down_is_found_and_forgotten_when_it_is_up() {
        let open = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reach = Reach::new(vec![closed(), open.local_addr().unwrap()]);
        // The link up: nothing is asked.
        reach.tend(false, Instant::now());
        assert!(reach.running.is_none() && reach.verdict().is_none());
        // Down: one probe, and either target answering is enough.
        reach.tend(true, Instant::now());
        assert!(reach.running.is_some());
        settle(&mut reach, true);
        assert!(reach.answers());
        // Not asked again before its time.
        reach.tend(true, Instant::now());
        assert!(reach.running.is_none());
        // Up again: forgotten, so the next outage asks at once.
        reach.tend(false, Instant::now());
        assert!(!reach.answers());
    }

    #[test]
    fn with_no_target_answering_the_probe_says_no_and_asks_again_only_after_its_time() {
        let every = Duration::from_millis(200);
        let mut reach = Reach::with_times(vec![closed(), closed()], every, PROBE_WITHIN);
        let start = Instant::now();
        reach.tend(true, start);
        settle(&mut reach, true);
        assert!(!reach.answers());
        assert_eq!(reach.verdict(), Some(false), "a verdict: no internet");
        let ended = reach.last.unwrap().0;
        reach.tend(true, ended + every / 2);
        assert!(reach.running.is_none(), "not before its time");
        reach.tend(true, ended + every);
        assert!(reach.running.is_some(), "again after it");
    }

    /// #1844: idle, the internet is asked with the link up too; a probe
    /// that finds it not answering is asked again after `PROBE_EVERY_DOWN`,
    /// not `PROBE_EVERY`, and kept across a link that stays up.
    #[test]
    fn idle_the_internet_is_asked_with_the_link_up_and_sooner_once_it_does_not_answer() {
        let every = Duration::from_millis(400);
        let mut reach = Reach::with_times(vec![closed()], every, PROBE_WITHIN).idle();
        let start = Instant::now();
        reach.tend(false, start);
        assert!(reach.running.is_some(), "asked with the link up");
        settle(&mut reach, false);
        assert_eq!(reach.verdict(), Some(false));
        let ended = reach.last.unwrap().0;
        // Not answering: again after the shorter of the two (here `every`,
        // 400 ms, is shorter than 5 s, so it bounds it).
        reach.tend(false, ended + every / 2);
        assert!(reach.running.is_none());
        reach.tend(false, ended + every);
        assert!(reach.running.is_some());
        settle(&mut reach, false);
        // Answering: asked at `every`.
        let open = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut up =
            Reach::with_times(vec![open.local_addr().unwrap()], every, PROBE_WITHIN).idle();
        up.tend(false, Instant::now());
        settle(&mut up, false);
        assert_eq!(up.verdict(), Some(true), "kept with the link up");
        let ended = up.last.unwrap().0;
        up.tend(false, ended + every / 2);
        assert!(up.running.is_none());
        // Not idle: nothing with the link up, as before.
        let mut plain = Reach::with_times(vec![closed()], every, PROBE_WITHIN);
        plain.tend(false, Instant::now());
        assert!(plain.running.is_none() && plain.verdict().is_none());
    }

    /// The reviewer-lead on #2261: idle and not answering, asked again at
    /// `PROBE_EVERY_DOWN` (5 s), not `every`; and the link coming back up
    /// forgets the outage's verdict.
    #[test]
    fn idle_not_answering_is_asked_at_five_seconds_and_forgotten_when_the_link_comes_up() {
        let every = Duration::from_secs(60);
        let mut reach = Reach::with_times(vec![closed()], every, PROBE_WITHIN).idle();
        reach.tend(false, Instant::now());
        settle(&mut reach, false);
        assert_eq!(reach.verdict(), Some(false));
        let ended = reach.last.unwrap().0;
        reach.tend(false, ended + PROBE_EVERY_DOWN - Duration::from_millis(1));
        assert!(reach.running.is_none(), "not before 5 s");
        reach.tend(false, ended + PROBE_EVERY_DOWN);
        assert!(reach.running.is_some(), "at 5 s, not at 60 s");
        settle(&mut reach, false);
        // Down, then up again: the verdict from the outage is forgotten.
        reach.tend(true, Instant::now());
        settle(&mut reach, true);
        assert_eq!(reach.verdict(), Some(false));
        reach.tend(false, Instant::now());
        assert_eq!(reach.verdict(), None, "forgotten with the link up");
        // A probe of the outage still running as the link comes back is
        // dropped: the one running after is begun then, not the outage's.
        // The outage's probe never answers (its sender is held here), so
        // this holds on any host (the reviewer-lead on #2261).
        let mut slow = Reach::with_times(vec![closed()], every, PROBE_WITHIN).idle();
        let (_never, outage_probe) = mpsc::sync_channel::<bool>(1);
        let down_at = Instant::now();
        slow.link_was_down = true;
        slow.running = Some((down_at, outage_probe));
        let up_at = down_at + Duration::from_millis(10);
        slow.tend(false, up_at);
        assert_eq!(slow.verdict(), None);
        assert_eq!(slow.running.as_ref().map(|(began, _)| *began), Some(up_at));
        settle(&mut slow, false);
    }

    #[test]
    fn the_default_targets_are_never_on_port_53() {
        // A router forcing its own resolver answers on 53 with the WAN down.
        let targets = targets();
        assert!(targets.len() >= 2);
        assert!(targets.iter().all(|target| target.port() == 443));
    }

    #[test]
    fn a_target_that_never_answers_is_given_up_within_the_bound() {
        // A reserved, unroutable address: the connect hangs until its bound.
        let within = Duration::from_millis(300);
        let mut reach = Reach::with_times(
            vec!["10.255.255.1:53".parse().unwrap()],
            PROBE_EVERY,
            within,
        );
        let began = Instant::now();
        reach.tend(true, began);
        settle(&mut reach, true);
        assert!(!reach.answers());
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
    }
}
