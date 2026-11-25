//! Report counters in the health report (#6256 F8; `docs/cloud.md`,
//! "Report counters"): an added field under `lamp-report/1`, `metrics`, in
//! Backend's B10 shape (#6256, 12 Nov, with Twelve's `mqtt` block). Every
//! value counts what happened since the last report the broker took, in
//! fixed buckets: no per-event data, no ids, no words. Off unless
//! [`ENVIRONMENT`] is `on`; then still only with the person's yes, as every
//! report.
//!
//! - `voice_turn_ms`: end of speech to an answer's first audio, from the
//!   voice's `voice/turn_time`.
//! - `ota_download_s`: one update's `fetching` step, from `update/status`.
//! - `wifi_setup`: setup's joins (`lamp_setup::wifi_counts`, its file read
//!   here, [`read_wifi`]), which wait on the lamp until a report takes them.
//! - `mqtt`: the transport in use and its reconnects, from the connection.
//!
//! Counted and not yet acknowledged is kept in the reports' state, so a
//! restart or a report lost on the way sends them again with the next one,
//! and a report acknowledged takes away only what it carried.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `on` adds `metrics`, in `lamp-cloud`, `lamp-voice` and `lamp-setup`.
pub const ENVIRONMENT: &str = "LAMP_REPORT_COUNTERS";
/// B10's bucket edges: a value is in the first bucket whose edge it is
/// under, and past the last in the last.
pub const VOICE_TURN_EDGES_MS: [u64; 6] = [500, 1_000, 1_500, 2_000, 3_000, 5_000];
pub const OTA_DOWNLOAD_EDGES_S: [u64; 5] = [30, 60, 120, 300, 600];

/// Setup's Wi-Fi totals, as `lamp_setup::wifi_counts::FILE` (a test holds
/// the two the same; `lamp-cloud` doesn't link setup).
pub const WIFI_FILE: &str = "/etc/lampOS/setup/wifi-setup.json";

/// Setup's totals, by B10's names, as setup writes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiSetup {
    pub attempts: u64,
    pub ok: u64,
    pub fail_password: u64,
    pub fail_no_ip: u64,
    pub fail_other: u64,
}

/// The totals in setup's file, in B10's order; zero when there is none or
/// it can't be read. At most 4 KiB is read.
pub fn read_wifi(path: &Path) -> [u64; 5] {
    use std::io::Read;
    let mut text = String::new();
    let totals: WifiSetup = std::fs::File::open(path)
        .and_then(|file| file.take(4_096).read_to_string(&mut text))
        .ok()
        .and_then(|_| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    [
        totals.attempts,
        totals.ok,
        totals.fail_password,
        totals.fail_no_ip,
        totals.fail_other,
    ]
}

/// Whether the switch is on in this environment.
pub fn on_from_environment() -> bool {
    super::switched_on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// The bucket `value` falls in among `edges`.
pub fn bucket(edges: &[u64], value: u64) -> usize {
    edges
        .iter()
        .position(|edge| value < *edge)
        .unwrap_or(edges.len())
}

/// The counts, flat, as they are kept and taken away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counts {
    pub voice_turn_ms: [u64; 7],
    pub ota_download_s: [u64; 6],
    /// Setup's totals, as [`read_wifi`].
    pub wifi_setup: [u64; 5],
    /// `reconnects`, `tcp_fail`, `wss_switches`.
    pub mqtt: [u64; 3],
}

fn each(left: &mut Counts, right: &Counts, by: impl Fn(u64, u64) -> u64) {
    fn pair<const N: usize>(a: &mut [u64; N], b: &[u64; N], by: &impl Fn(u64, u64) -> u64) {
        for (a, b) in a.iter_mut().zip(b) {
            *a = by(*a, *b);
        }
    }
    pair(&mut left.voice_turn_ms, &right.voice_turn_ms, &by);
    pair(&mut left.ota_download_s, &right.ota_download_s, &by);
    pair(&mut left.wifi_setup, &right.wifi_setup, &by);
    pair(&mut left.mqtt, &right.mqtt, &by);
}

impl Counts {
    pub fn add(&mut self, other: &Self) {
        each(self, other, u64::saturating_add);
    }

    pub fn take_away(&mut self, other: &Self) {
        each(self, other, u64::saturating_sub);
    }

    pub fn voice_turn(&mut self, after_ms: u64) {
        let at = bucket(&VOICE_TURN_EDGES_MS, after_ms);
        self.voice_turn_ms[at] = self.voice_turn_ms[at].saturating_add(1);
    }

    pub fn ota_download(&mut self, seconds: u64) {
        let at = bucket(&OTA_DOWNLOAD_EDGES_S, seconds);
        self.ota_download_s[at] = self.ota_download_s[at].saturating_add(1);
    }

    /// The block as sent: a block with nothing in it left out, but `mqtt`
    /// always, zeros when nothing went wrong (B10).
    pub fn metrics(&self, transport: Transport) -> Metrics {
        let some = |counts: &[u64]| counts.iter().any(|count| *count > 0);
        let [reconnects, tcp_fail, wss_switches] = self.mqtt;
        Metrics {
            voice_turn_ms: some(&self.voice_turn_ms).then(|| Buckets {
                edges: &VOICE_TURN_EDGES_MS,
                counts: self.voice_turn_ms.to_vec(),
            }),
            ota_download_s: some(&self.ota_download_s).then(|| Buckets {
                edges: &OTA_DOWNLOAD_EDGES_S,
                counts: self.ota_download_s.to_vec(),
            }),
            wifi_setup: some(&self.wifi_setup).then(|| {
                let [attempts, ok, fail_password, fail_no_ip, fail_other] = self.wifi_setup;
                WifiSetup {
                    attempts,
                    ok,
                    fail_password,
                    fail_no_ip,
                    fail_other,
                }
            }),
            mqtt: Mqtt {
                transport,
                reconnects,
                tcp_fail,
                wss_switches,
            },
        }
    }
}

/// What the connection is over now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Tcp,
    Wss,
}

/// What happened on the broker's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// The broker's CONNACK, over that transport.
    Connected(Transport),
    /// A TCP connection that never reached a CONNACK.
    TcpFailed,
    /// On to WSS after TCP kept failing (F9).
    ToWss,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Buckets {
    pub edges: &'static [u64],
    pub counts: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Mqtt {
    pub transport: Transport,
    pub reconnects: u64,
    pub tcp_fail: u64,
    pub wss_switches: u64,
}

/// `metrics` in a health report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Metrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_turn_ms: Option<Buckets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ota_download_s: Option<Buckets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi_setup: Option<WifiSetup>,
    pub mqtt: Mqtt,
}

/// One download's time from `update/status`: the `fetching` step's start,
/// and the next step's start in the same round. A round that ends in
/// `fetching` (a failed download) or that `lamp-cloud` did not see begin
/// counts nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fetching(Option<(u64, u64)>);

impl Fetching {
    /// `status` looked at: the download's whole seconds once it is over.
    pub fn look(&mut self, status: &lamp_msgs::UpdateStatus) -> Option<u64> {
        let Some(round) = &status.round else {
            self.0 = None;
            return None;
        };
        if status.now == "fetching" {
            self.0 = Some((round.started_at_unix_ms, round.step_started_at_unix_ms));
            return None;
        }
        let (of, began) = self.0.take()?;
        (of == round.started_at_unix_ms && round.step_started_at_unix_ms >= began)
            .then(|| (round.step_started_at_unix_ms - began) / 1_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fall_in_b10s_buckets_at_their_edges() {
        assert_eq!(bucket(&VOICE_TURN_EDGES_MS, 0), 0);
        assert_eq!(bucket(&VOICE_TURN_EDGES_MS, 499), 0);
        assert_eq!(bucket(&VOICE_TURN_EDGES_MS, 500), 1);
        assert_eq!(bucket(&VOICE_TURN_EDGES_MS, 4_999), 5);
        assert_eq!(bucket(&VOICE_TURN_EDGES_MS, 5_000), 6);
        assert_eq!(bucket(&OTA_DOWNLOAD_EDGES_S, 600), 5);
        let mut counts = Counts::default();
        for ms in [900, 1_100, 1_200, 7_000] {
            counts.voice_turn(ms);
        }
        counts.ota_download(45);
        assert_eq!(counts.voice_turn_ms, [0, 1, 2, 0, 0, 0, 1]);
        assert_eq!(counts.ota_download_s, [0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn the_block_is_b10s_shape_and_leaves_out_a_block_with_nothing() {
        let quiet = Counts::default().metrics(Transport::Tcp);
        assert_eq!(
            serde_json::to_string(&quiet).unwrap(),
            r#"{"mqtt":{"transport":"tcp","reconnects":0,"tcp_fail":0,"wss_switches":0}}"#
        );
        let mut counts = Counts::default();
        counts.voice_turn(1_200);
        counts.ota_download(45);
        counts.wifi_setup = [2, 1, 1, 0, 0];
        counts.mqtt = [1, 3, 1];
        assert_eq!(
            serde_json::to_value(counts.metrics(Transport::Wss)).unwrap(),
            serde_json::json!({
                "voice_turn_ms": {"edges": [500,1000,1500,2000,3000,5000], "counts": [0,0,1,0,0,0,0]},
                "ota_download_s": {"edges": [30,60,120,300,600], "counts": [0,1,0,0,0,0]},
                "wifi_setup": {"attempts": 2, "ok": 1, "fail_password": 1, "fail_no_ip": 0, "fail_other": 0},
                "mqtt": {"transport": "wss", "reconnects": 1, "tcp_fail": 3, "wss_switches": 1}
            })
        );
    }

    #[test]
    fn adding_and_taking_away_saturate() {
        let mut counts = Counts {
            mqtt: [u64::MAX, 2, 0],
            ..Counts::default()
        };
        let more = Counts {
            mqtt: [1, 5, 1],
            ..Counts::default()
        };
        counts.add(&more);
        assert_eq!(counts.mqtt, [u64::MAX, 7, 1]);
        counts.take_away(&more);
        counts.take_away(&more);
        assert_eq!(counts.mqtt, [u64::MAX - 2, 0, 0]);
    }

    /// What setup writes is what is read here, by the same names and file.
    #[test]
    fn setups_wifi_totals_are_read_as_setup_writes_them() {
        use lamp_setup::wifi::{Joined, Reason};
        assert_eq!(WIFI_FILE, lamp_setup::wifi_counts::FILE);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wifi-setup.json");
        assert_eq!(read_wifi(&path), [0; 5]);
        for joined in [
            Joined::Failed {
                reason: Reason::WrongPassword,
            },
            Joined::Failed {
                reason: Reason::NoDhcp,
            },
            Joined::Failed {
                reason: Reason::NoInternet,
            },
            Joined::Connected,
        ] {
            lamp_setup::wifi_counts::count(&path, &joined).unwrap();
        }
        assert_eq!(read_wifi(&path), [4, 1, 1, 1, 1]);
        assert_eq!(
            read_wifi(&path),
            lamp_setup::wifi_counts::read(&path).as_array()
        );
    }

    fn status(now: &str, round: Option<(u64, u64)>) -> lamp_msgs::UpdateStatus {
        serde_json::from_value(serde_json::json!({
            "running": null,
            "now": now,
            "last": null,
            "round": round.map(|(started, step)| serde_json::json!({
                "started_at_unix_ms": started,
                "step_started_at_unix_ms": step,
            })),
        }))
        .unwrap()
    }

    #[test]
    fn a_download_is_timed_from_fetching_to_the_next_step_of_its_round() {
        let mut fetching = Fetching::default();
        assert_eq!(
            fetching.look(&status("checking", Some((1_000, 1_000)))),
            None
        );
        assert_eq!(
            fetching.look(&status("fetching", Some((1_000, 2_000)))),
            None
        );
        assert_eq!(
            fetching.look(&status("fetching", Some((1_000, 2_000)))),
            None
        );
        assert_eq!(
            fetching.look(&status("switching", Some((1_000, 47_900)))),
            Some(45)
        );
        // Said once only.
        assert_eq!(
            fetching.look(&status("switching", Some((1_000, 47_900)))),
            None
        );
        // A download that failed ends its round: nothing.
        assert_eq!(
            fetching.look(&status("fetching", Some((9_000, 9_000)))),
            None
        );
        assert_eq!(fetching.look(&status("idle", None)), None);
        assert_eq!(
            fetching.look(&status("switching", Some((9_000, 60_000)))),
            None
        );
        // Another round's step is not this download's end.
        assert_eq!(
            fetching.look(&status("fetching", Some((10_000, 10_000)))),
            None
        );
        assert_eq!(
            fetching.look(&status("switching", Some((11_000, 70_000)))),
            None
        );
    }
}
