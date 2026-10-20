//! Staying on the company broker, as V1's os-server does (Nine's contract on
//! #1396): MQTT v5 to `mqtt://{endpoint}:{port}` with the account's login and
//! V1's client id, a clean start and a 60 s session expiry, `fa_channel`
//! subscribed at QoS 1 on every connection and every reply on `fd_channel`
//! at QoS 1. On each connection the lamp says `info` unasked. A connection
//! that fails, or that the broker refuses, is tried again after a wait that
//! doubles to a minute, and said once in the journal, not on every try.
use crate::account::Account;
use crate::answer::{Lamp, QUIET_WHEN_DONE, answer};
use crate::wire::{Command, Identity, parse};
use rumqttc::Outgoing;
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::mqttbytes::v5::{ConnectProperties, Packet};
use rumqttc::v5::{AsyncClient, Event, MqttOptions};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

/// The keepalive: a dead link is noticed within about a minute and a half.
/// V1 asks the protocol's longest, 65,535 s, which never notices one.
pub const KEEP_ALIVE: Duration = Duration::from_secs(60);
/// V1's session expiry.
pub const SESSION_EXPIRY_S: u32 = 60;
/// The largest packet taken from the broker, and told it: a `faces.add_photo`
/// with the largest photo lamp-cloud takes (`crate::photos::MOST_BYTES`, as
/// base64), with room for its envelope. rumqttc's own is 10 KiB, which
/// dropped the connection on every photo (Mobile on lamp-0c89, 20 Oct).
pub const MOST_PACKET_BYTES: u32 = (crate::photos::MOST_BYTES.div_ceil(3) * 4 + 64 * 1_024) as u32;

/// The client's options for `account`: V1's, with this lamp's keepalive.
pub fn options(account: &Account) -> MqttOptions {
    let mut options = MqttOptions::new(
        account.client_id(),
        account.mqtt_endpoint.trim(),
        account.mqtt_port,
    );
    options
        .set_keep_alive(KEEP_ALIVE)
        .set_clean_start(true)
        .set_credentials(account.mqtt_username.trim(), account.mqtt_password.as_str())
        .set_connect_properties(ConnectProperties {
            session_expiry_interval: Some(SESSION_EXPIRY_S),
            receive_maximum: None,
            max_packet_size: Some(MOST_PACKET_BYTES),
            topic_alias_max: None,
            request_response_info: None,
            request_problem_info: None,
            user_properties: Vec::new(),
            authentication_method: None,
            authentication_data: None,
        });
    options
}

/// Whether a message on `topic` is a command for this lamp: only its own
/// account channel, `fa_channel`, which only the lamp's signed-in account
/// may publish to. Every kind rests on this, `face.remove` and
/// `faces.add_photo` among them (the reviewer on #2199): the whole topic,
/// never a part of it.
pub fn commanded(topic: &[u8], account: &Account) -> bool {
    !account.fa_channel.is_empty() && topic == account.fa_channel.as_bytes()
}

/// The reply to one message on `fa_channel`, or nothing for one that is no
/// command or a preview done (`QUIET_WHEN_DONE`).
pub fn reply(
    identity: &Identity,
    lamp: &mut impl Lamp,
    payload: &[u8],
    now: &str,
) -> Option<String> {
    let reply = match parse(payload).ok()? {
        Command::Info => identity.info(now),
        Command::Data { kind, data } => {
            let answered = answer(lamp, &kind, &data);
            if answered.is_ok() && QUIET_WHEN_DONE.contains(&kind.as_str()) {
                return None;
            }
            // A change after setup (#1959): taken now, its result later.
            if answered.is_ok()
                && (crate::settings::KINDS.contains(&kind.as_str())
                    || kind == crate::settings::WIFI_KIND)
            {
                return Some(identity.starting(now, &kind).to_string());
            }
            identity.data(now, &kind, &answered)
        }
        Command::Other(cmd) => identity.refused(now, &cmd),
    };
    Some(reply.to_string())
}

/// UTC now as RFC 3339, as V1's `time` field.
pub fn now() -> String {
    rfc3339(SystemTime::now())
}

pub fn rfc3339(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = ((seconds / 86_400) as i64, seconds % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest / 60 % 60,
        rest % 60
    )
}

/// What was handed to the client, so the broker's acknowledgement of a
/// report, or of a change's result (#1959), can be told from the rest
/// (#1683). The client numbers publishes as it sends them, in the order they
/// were handed over, so each handed publish is noted in that order.
#[derive(Default)]
struct Handed {
    in_order: VecDeque<Tagged>,
    by_packet: HashMap<u16, Tagged>,
}

/// What a handed publish was.
enum Tagged {
    Other,
    /// A report's `seq`.
    Report(u64),
    /// A change's result, said again if it is lost before its ack.
    Change(crate::settings::Outcome),
}

impl Handed {
    fn publish(
        &mut self,
        client: &AsyncClient,
        topic: &str,
        payload: String,
        report: Option<u64>,
    ) -> bool {
        self.publish_tagged(
            client,
            topic,
            payload,
            report.map_or(Tagged::Other, Tagged::Report),
        )
    }

    fn publish_tagged(
        &mut self,
        client: &AsyncClient,
        topic: &str,
        payload: String,
        tag: Tagged,
    ) -> bool {
        let handed = client
            .try_publish(topic, QoS::AtLeastOnce, false, payload)
            .is_ok();
        if handed {
            self.in_order.push_back(tag);
        }
        handed
    }

    /// The client sent the next one as packet `pkid`.
    fn sent(&mut self, pkid: u16) {
        match self.in_order.pop_front() {
            Some(Tagged::Other) | None => {}
            Some(tag) => {
                self.by_packet.insert(pkid, tag);
            }
        }
    }

    /// The report the broker acknowledged as `pkid`, if it was one; a
    /// change's result acknowledged is done with.
    fn acked(&mut self, pkid: u16) -> Option<u64> {
        match self.by_packet.remove(&pkid) {
            Some(Tagged::Report(seq)) => Some(seq),
            _ => None,
        }
    }

    /// The connection went: the client drops what it had not had
    /// acknowledged (a clean start), so the lamp sends it again. The
    /// changes' results among it are handed back, oldest first.
    fn lost(&mut self) -> Vec<crate::settings::Outcome> {
        let mut by_packet: Vec<_> = self.by_packet.drain().collect();
        by_packet.sort_by_key(|(pkid, _)| *pkid);
        by_packet
            .into_iter()
            .map(|(_, tag)| tag)
            .chain(self.in_order.drain(..))
            .filter_map(|tag| match tag {
                Tagged::Change(outcome) => Some(outcome),
                _ => None,
            })
            .collect()
    }
}

/// The reports due, handed to the client while online, on
/// `<fd_channel>/report` with the device's id (`crate::reports::topic`).
fn hand_reports(
    reports: &Option<Arc<crate::reports::Shared>>,
    client: &AsyncClient,
    account: &Account,
    id: &str,
    handed: &mut Handed,
) {
    let Some(shared) = reports else {
        return;
    };
    let topic = crate::reports::topic(account.fd_channel.as_str());
    for (seq, bytes) in shared.with(|reports| reports.due(id)) {
        if !handed.publish(client, &topic, bytes, Some(seq)) {
            shared.with(|reports| reports.unhand(Some(seq)));
        }
    }
}

/// Waits for the reports' wake, or forever without reports.
async fn woken(reports: &Option<Arc<crate::reports::Shared>>) {
    match reports {
        Some(shared) => shared.wake.notified().await,
        None => std::future::pending().await,
    }
}

/// Where `run` says what happens: one line for each change of state.
pub trait Say {
    fn line(&mut self, line: &str);
}

impl<F: FnMut(&str)> Say for F {
    fn line(&mut self, line: &str) {
        self(line);
    }
}

/// Stays online with the newest account in `accounts` until `accounts`
/// closes. A new account (a ping rotated it) ends the connection and
/// starts one with it. `first` is the backoff's first wait.
pub async fn run(
    mut accounts: watch::Receiver<Option<Account>>,
    identity: watch::Receiver<Identity>,
    lamp: &mut impl Lamp,
    say: &mut impl Say,
    first: Duration,
) {
    loop {
        let account = accounts.borrow_and_update().clone();
        let Some(account) = account.filter(Account::complete) else {
            if accounts.changed().await.is_err() {
                return;
            }
            continue;
        };
        tokio::select! {
            () = session(&account, &identity, lamp, say, first) => return,
            changed = accounts.changed() => {
                if changed.is_err() {
                    return;
                }
                say.line("the account changed: connecting with it");
            }
        }
    }
}

/// One account's connection, tried again until it is replaced. Never
/// returns on its own.
async fn session(
    account: &Account,
    identity: &watch::Receiver<Identity>,
    lamp: &mut impl Lamp,
    say: &mut impl Say,
    first: Duration,
) {
    let (client, mut events) = AsyncClient::new(options(account), 64);
    let mut wait = first;
    // Health and crash reports, with their switch (#1683): handed over only
    // while connected, and kept until the broker acknowledges them.
    let reports = lamp.reports();
    let mut handed = Handed::default();
    let mut connected = false;
    // Pieces of the answers to texts, looked for between the broker's
    // events (#1673, `crate::chat`).
    let mut tick = tokio::time::interval(crate::chat::TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // What was last said, so a state is said once.
    let mut said = String::new();
    let mut tell = |say: &mut dyn Say, line: String| {
        if said != line {
            say.line(&line);
            said = line;
        }
    };
    loop {
        // `poll` is not cancel-safe: it makes the connection itself (TCP,
        // CONNECT, CONNACK) and flushes what it writes, so a `poll` dropped
        // part way loses a connection half made or a packet half written.
        // One `poll` is kept until it ends, and the tick runs beside it, only
        // while texts are taken.
        let texting = lamp.texting();
        // Changes after setup (#1959): their results are looked for on the
        // same tick, only with their switch.
        let changes = lamp.changes();
        let poll = events.poll();
        tokio::pin!(poll);
        let polled = loop {
            tokio::select! {
                polled = &mut poll => break polled,
                _ = tick.tick(), if texting || changes.is_some() => {
                    let me = identity.borrow().clone();
                    if texting {
                        for data in lamp.chat_events(crate::chat::now_ms()) {
                            let event = me.data(&now(), "chat.event", &Ok(Some(data))).to_string();
                            handed.publish(&client, account.fd_channel.as_str(), event, None);
                        }
                    }
                    // Only while connected, and kept until the broker's ack:
                    // a Wi-Fi change's result is often ready only once the
                    // lamp is back online.
                    if connected && let Some(changes) = &changes {
                        for outcome in changes.take() {
                            let reply = outcome.reply(&me, &now()).to_string();
                            let tag = Tagged::Change(outcome.clone());
                            if !handed.publish_tagged(&client, account.fd_channel.as_str(), reply, tag) {
                                changes.owe(vec![outcome]);
                            }
                        }
                    }
                }
                () = woken(&reports), if connected => {
                    let id = identity.borrow().id.clone();
                    hand_reports(&reports, &client, account, &id, &mut handed);
                }
            }
        };
        match polled {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                wait = first;
                if let Some(changes) = &changes {
                    changes.set_broker(account.mqtt_endpoint.trim(), account.mqtt_port);
                }
                let _ = client.try_subscribe(account.fa_channel.as_str(), QoS::AtLeastOnce);
                let info = identity.borrow().info(&now()).to_string();
                handed.publish(&client, account.fd_channel.as_str(), info, None);
                connected = true;
                if let Some(shared) = &reports {
                    shared.with(|reports| reports.connected());
                }
                let id = identity.borrow().id.clone();
                hand_reports(&reports, &client, account, &id, &mut handed);
                tell(
                    say,
                    format!(
                        "online on the broker {}:{}",
                        account.mqtt_endpoint.trim(),
                        account.mqtt_port
                    ),
                );
            }
            Ok(Event::Incoming(Packet::Publish(message))) => {
                if !commanded(message.topic.as_ref(), account) {
                    continue;
                }
                let me = identity.borrow().clone();
                if let Some(reply) = reply(&me, lamp, &message.payload, &now()) {
                    handed.publish(&client, account.fd_channel.as_str(), reply, None);
                }
                // `reports.set` may have put a last word in the queue.
                hand_reports(&reports, &client, account, &me.id, &mut handed);
            }
            Ok(Event::Outgoing(Outgoing::Publish(pkid))) => handed.sent(pkid),
            Ok(Event::Incoming(Packet::PubAck(ack))) => {
                if let (Some(seq), Some(shared)) = (handed.acked(ack.pkid), &reports) {
                    shared.with(|reports| reports.acked(seq));
                }
            }
            Ok(_) => {}
            Err(error) => {
                connected = false;
                let again = handed.lost();
                if let Some(changes) = &changes {
                    changes.owe(again);
                }
                if let Some(shared) = &reports {
                    shared.with(|reports| reports.unhand(None));
                }
                // The error names a code or an I/O kind, never the login.
                tell(say, format!("offline, trying again: {error}"));
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(crate::ping::MOST);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the lamp's own account channel commands it: not its reply
    /// channel, another lamp's, a longer or shorter topic, or an empty one.
    #[test]
    fn only_the_lamps_own_account_channel_commands_it() {
        let account: Account = serde_json::from_value(serde_json::json!({
            "mqtt_endpoint": "broker.example", "mqtt_port": 8883,
            "mqtt_username": "u", "mqtt_password": "p",
            "fa_channel": "fa/d-42", "fd_channel": "fd/d-42", "llm_base_url": ""
        }))
        .unwrap();
        assert!(commanded(b"fa/d-42", &account));
        for other in [
            &b"fd/d-42"[..],
            b"fa/d-43",
            b"fa/d-42/x",
            b"fa/d-4",
            b"",
            b"FA/D-42",
        ] {
            assert!(!commanded(other, &account), "{other:?}");
        }
        let unset = Account {
            fa_channel: String::new(),
            ..account
        };
        assert!(!commanded(b"", &unset));
    }

    #[test]
    fn the_time_is_rfc3339_in_utc() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let at = UNIX_EPOCH + Duration::from_secs(1_760_713_200);
        assert_eq!(rfc3339(at), "2025-10-17T15:00:00Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(rfc3339(leap), "2000-02-29T00:00:00Z");
    }

    use crate::answer::tests::Fake;
    use bytes::BytesMut;
    use rumqttc::v5::mqttbytes::v5::{
        ConnAck, ConnectReturnCode, PingResp, PubAck, Publish, SubAck, SubscribeReasonCode,
    };
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// What the tests' broker saw.
    #[derive(Clone, Debug, PartialEq)]
    enum Seen {
        Connect {
            client_id: String,
            keep_alive: u16,
            clean_start: bool,
            expiry: Option<u32>,
            login: Option<(String, String)>,
        },
        Subscribe(String),
        Publish(String, String),
    }

    /// How the tests' broker behaves.
    #[derive(Clone, Copy, PartialEq)]
    enum Broker {
        /// Takes every connection and, once subscribed, sends `commands`.
        Takes,
        /// Refuses every connection: a wrong login.
        Refuses,
        /// Takes the first connection, then closes it after its first reply.
        DropsOnce,
        /// Sends `commands` on the first connection only, and closes it
        /// after its second reply: the Wi-Fi changed under it (#1959).
        CommandsOnceThenDrops,
        /// Takes every connection, answering CONNECT only after 300 ms, six
        /// of the texts' ticks: a broker across the internet.
        SlowConnAck,
        /// As `Takes`, but sends each command first on topics that aren't
        /// the lamp's account channel, then on it (the reviewer's P3 1 on
        /// #2214).
        OtherTopicsFirst,
    }

    /// Topics a command must never be taken from: the lamp's own reply
    /// channel, another lamp's, longer and shorter ones, and another case.
    const NOT_OURS: [&str; 5] = ["fd/d-42", "fa/d-43", "fa/d-42/x", "fa/d-4", "FA/D-42"];

    /// A broker on a free local port, with V1's packets, enough for one
    /// lamp: CONNECT, SUBSCRIBE, PUBLISH at QoS 1 both ways, PINGREQ.
    async fn broker(behaves: Broker, commands: Vec<&'static str>) -> (u16, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let mut connections = 0;
            while let Ok((mut socket, _)) = listener.accept().await {
                connections += 1;
                let mut buffer = BytesMut::new();
                let mut replies = 0;
                'connection: loop {
                    let packet = loop {
                        match Packet::read(&mut buffer, None) {
                            Ok(packet) => break packet,
                            Err(_) => {
                                let mut chunk = [0u8; 4096];
                                match socket.read(&mut chunk).await {
                                    Ok(0) | Err(_) => break 'connection,
                                    Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                                }
                            }
                        }
                    };
                    let mut out = BytesMut::new();
                    match packet {
                        Packet::Connect(connect, _, login) => {
                            log.lock().unwrap().push(Seen::Connect {
                                client_id: connect.client_id.clone(),
                                keep_alive: connect.keep_alive,
                                clean_start: connect.clean_start,
                                expiry: connect
                                    .properties
                                    .as_ref()
                                    .and_then(|p| p.session_expiry_interval),
                                login: login.map(|l| (l.username, l.password)),
                            });
                            if behaves == Broker::SlowConnAck {
                                tokio::time::sleep(Duration::from_millis(300)).await;
                            }
                            let code = if behaves == Broker::Refuses {
                                ConnectReturnCode::BadUserNamePassword
                            } else {
                                ConnectReturnCode::Success
                            };
                            Packet::ConnAck(ConnAck {
                                session_present: false,
                                code,
                                properties: None,
                            })
                            .write(&mut out, None)
                            .unwrap();
                            let _ = socket.write_all(&out).await;
                            if behaves == Broker::Refuses {
                                break 'connection;
                            }
                            continue;
                        }
                        Packet::Subscribe(subscribe) => {
                            for filter in &subscribe.filters {
                                log.lock()
                                    .unwrap()
                                    .push(Seen::Subscribe(filter.path.clone()));
                            }
                            Packet::SubAck(SubAck {
                                pkid: subscribe.pkid,
                                return_codes: vec![SubscribeReasonCode::Success(QoS::AtLeastOnce)],
                                properties: None,
                            })
                            .write(&mut out, None)
                            .unwrap();
                            let once = behaves == Broker::CommandsOnceThenDrops;
                            let commands = if once && connections > 1 {
                                &[][..]
                            } else {
                                &commands[..]
                            };
                            let topics: &[&str] = if behaves == Broker::OtherTopicsFirst {
                                &[
                                    NOT_OURS[0],
                                    NOT_OURS[1],
                                    NOT_OURS[2],
                                    NOT_OURS[3],
                                    NOT_OURS[4],
                                    "fa/d-42",
                                ]
                            } else {
                                &["fa/d-42"]
                            };
                            let mut pkid = 100u16;
                            for command in commands {
                                for topic in topics {
                                    let mut publish =
                                        Publish::new(*topic, QoS::AtLeastOnce, *command, None);
                                    publish.pkid = pkid;
                                    pkid += 1;
                                    Packet::Publish(publish).write(&mut out, None).unwrap();
                                }
                            }
                        }
                        Packet::Publish(publish) => {
                            log.lock().unwrap().push(Seen::Publish(
                                String::from_utf8_lossy(&publish.topic).into_owned(),
                                String::from_utf8_lossy(&publish.payload).into_owned(),
                            ));
                            Packet::PubAck(PubAck::new(publish.pkid, None))
                                .write(&mut out, None)
                                .unwrap();
                            replies += 1;
                        }
                        Packet::PingReq(_) => {
                            Packet::PingResp(PingResp).write(&mut out, None).unwrap();
                        }
                        _ => {}
                    }
                    if socket.write_all(&out).await.is_err() {
                        break;
                    }
                    if behaves == Broker::DropsOnce && connections == 1 && replies == 1 {
                        break;
                    }
                    if behaves == Broker::CommandsOnceThenDrops && connections == 1 && replies == 2
                    {
                        break;
                    }
                }
            }
        });
        (port, seen)
    }

    fn account(port: u16) -> Account {
        Account {
            device_id: "d-42".into(),
            mqtt_endpoint: "127.0.0.1".into(),
            mqtt_port: port,
            mqtt_username: "lamp".into(),
            mqtt_password: "s3cret-pw".into(),
            fa_channel: "fa/d-42".into(),
            fd_channel: "fd/d-42".into(),
            llm_base_url: String::new(),
        }
    }

    /// V1's preview (`docs/mqtt.md`): a drag sends five a second, so one
    /// done is not answered and one that fails is.
    #[test]
    fn a_preview_done_is_not_answered_and_one_that_fails_is() {
        let mut lamp = Fake {
            shown: Some(crate::light::first()),
            ..Fake::default()
        };
        let preview = |color: &str| {
            format!(r#"{{"cmd":"data","kind":"led.resting.preview","data":{{"color":{color}}}}}"#)
        };
        let now = "2026-10-18T10:00:00Z";
        assert_eq!(
            reply(&identity(), &mut lamp, preview("[20,0,10]").as_bytes(), now),
            None
        );
        assert_eq!(lamp.previews.len(), 1);
        let failed: serde_json::Value = serde_json::from_str(
            &reply(&identity(), &mut lamp, preview("[1,2]").as_bytes(), now).unwrap(),
        )
        .unwrap();
        assert_eq!(failed["status"], "failure");
        assert_eq!(failed["kind"], "led.resting.preview");
        assert_eq!(failed["error"], "color must be [r, g, b]");
        // A save is answered, as every other kind.
        let saved = br#"{"cmd":"data","kind":"led.resting.set","data":{"mode":"off"}}"#;
        assert!(reply(&identity(), &mut lamp, saved, now).is_some());
    }

    fn identity() -> Identity {
        Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: String::new(),
            local_ip: String::new(),
        }
    }

    /// Runs the lamp against the broker until `until` holds of what it saw,
    /// or 5 s; then what it saw and the lines said.
    async fn against(
        port: u16,
        seen: &Arc<Mutex<Vec<Seen>>>,
        first: Duration,
        until: impl Fn(&[Seen]) -> bool,
    ) -> Vec<String> {
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lines = Arc::new(Mutex::new(Vec::new()));
        let said = lines.clone();
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                raw: 60,
                ceiling: 80,
                ..Fake::default()
            };
            let mut say = |line: &str| said.lock().unwrap().push(line.to_owned());
            run(accounts, me, &mut lamp, &mut say, first).await;
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !until(&seen.lock().unwrap()) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The account's sender gone: `run` ends.
        drop(accounts_tx);
        lamp.await.unwrap();
        // The run has ended, and with it its copy of the lines.
        Arc::try_unwrap(lines).unwrap().into_inner().unwrap()
    }

    fn publishes(seen: &[Seen]) -> Vec<(String, serde_json::Value)> {
        seen.iter()
            .filter_map(|seen| match seen {
                Seen::Publish(topic, payload) => {
                    Some((topic.clone(), serde_json::from_str(payload).unwrap()))
                }
                _ => None,
            })
            .collect()
    }

    /// #1959: a change after setup is acked `starting` on `fd_channel` at
    /// once, and its result goes out after it as the same kind, from the
    /// broker's own loop.
    #[tokio::test]
    async fn a_change_is_acked_starting_and_its_result_follows_on_the_channel() {
        let socket = std::env::temp_dir().join(format!("lco-{}-change.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let helper = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(b"{\"done\":{\"name\":\"Desk lamp\"}}\n")
                .unwrap();
            line
        });
        let (port, seen) = broker(
            Broker::Takes,
            vec![r#"{"cmd":"data","kind":"device.rename","data":{"name":"Desk lamp"}}"#],
        )
        .await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let changes = crate::settings::Changes::new(&socket, Duration::from_secs(5));
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                changes: Some(changes),
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
        });
        let renames = |seen: &[Seen]| {
            publishes(seen)
                .into_iter()
                .filter(|(_, reply)| reply["kind"] == "device.rename")
                .collect::<Vec<_>>()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while renames(&seen.lock().unwrap()).len() < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        assert_eq!(helper.join().unwrap(), "{\"display_name\":\"Desk lamp\"}\n");
        let replies = renames(&seen.lock().unwrap());
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].0, "fd/d-42");
        assert_eq!(replies[0].1["status"], "starting");
        assert_eq!(
            (&replies[1].1["status"], &replies[1].1["data"]["name"]),
            (
                &serde_json::json!("success"),
                &serde_json::json!("Desk lamp")
            )
        );
        let _ = std::fs::remove_file(&socket);
    }

    /// The reviewer's P3 1 on #2214, which #1968, #2199 and #2214 rest on:
    /// a command on any topic but the lamp's own account channel is neither
    /// done nor answered. The same command then on `fa/d-42` is done once
    /// and answered once, on `fd/d-42`.
    #[tokio::test]
    async fn only_a_command_on_the_lamps_own_account_channel_is_done_and_answered() {
        let (port, seen) = broker(
            Broker::OtherTopicsFirst,
            vec![r#"{"cmd":"data","kind":"volume.set","data":{"volume":44}}"#],
        )
        .await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                ceiling: 80,
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
            lamp
        });
        let sets = |seen: &[Seen]| {
            publishes(seen)
                .into_iter()
                .filter(|(_, reply)| reply["kind"] == "volume.set")
                .collect::<Vec<_>>()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while sets(&seen.lock().unwrap()).is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Every copy was sent in one write, the account channel's last: any
        // other answered would be here by now.
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(accounts_tx);
        let lamp = lamp.await.unwrap();
        let replies = sets(&seen.lock().unwrap());
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert_eq!(replies[0].0, "fd/d-42");
        assert_eq!(replies[0].1["status"], "success");
        assert_eq!(lamp.set, [44], "done once, from the account channel only");
    }

    /// Mobile on lamp-0c89 (20 Oct): a `faces.add_photo` with the largest
    /// photo lamp-cloud takes is read on the connection and answered on
    /// `fd/d-42`, not dropped with it as over rumqttc's 10 KiB. The broker is
    /// told the same limit.
    #[tokio::test]
    async fn a_command_with_the_largest_photo_is_read_and_answered() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let mut photo = vec![0u8; crate::photos::MOST_BYTES];
        photo[..2].copy_from_slice(&[0xFF, 0xD8]);
        let command = format!(
            r#"{{"cmd":"data","kind":"faces.add_photo","data":{{"name":"Lan","photo":"{}"}}}}"#,
            STANDARD.encode(&photo)
        );
        assert!(command.len() > 10 * 1_024 * 1_024);
        let (port, seen) = broker(Broker::Takes, vec![command.leak()]).await;
        let lines = against(port, &seen, Duration::from_millis(50), |seen| {
            publishes(seen)
                .iter()
                .any(|(_, reply)| reply["kind"] == "faces.add_photo")
        })
        .await;
        let replies: Vec<_> = publishes(&seen.lock().unwrap())
            .into_iter()
            .filter(|(_, reply)| reply["kind"] == "faces.add_photo")
            .collect();
        assert_eq!(replies.len(), 1, "{:?}", &lines[..lines.len().min(4)]);
        assert_eq!(replies[0].0, "fd/d-42");
        assert!(
            !lines.iter().any(|line| line.contains("offline")),
            "{lines:?}"
        );
        assert_eq!(
            options(&account(port)).max_packet_size(),
            Some(MOST_PACKET_BYTES)
        );
    }

    /// #1959: the Wi-Fi changed under the connection. The ack goes out on
    /// the old one, which then drops; the helper's result comes while the
    /// lamp is offline and is said on the next connection, with the app's
    /// id, and the password is in no message.
    #[tokio::test]
    async fn a_wifi_result_ready_while_offline_is_said_once_back_online() {
        let socket = std::env::temp_dir().join(format!("lco-{}-wifi.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let helper = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            // The radio switching: answered while the lamp is offline.
            std::thread::sleep(Duration::from_millis(200));
            stream
                .write_all(b"{\"done\":{\"ssid\":\"New Home\"}}\n")
                .unwrap();
            line
        });
        let (port, seen) = broker(
            Broker::CommandsOnceThenDrops,
            vec![
                r#"{"cmd":"data","kind":"wifi.set","data":{"ssid":"New Home","password":"fake-pass-NEW-1234","request_id":"r-7"}}"#,
            ],
        )
        .await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let changes = crate::settings::Changes::with_wifi(
            std::env::temp_dir().join("lco-never.sock"),
            Duration::from_secs(5),
            Some((socket.clone(), Duration::from_secs(5))),
        );
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                changes: Some(changes),
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            // Back online only after the helper has answered.
            run(
                accounts,
                me,
                &mut lamp,
                &mut say,
                Duration::from_millis(600),
            )
            .await;
        });
        let wifi = |seen: &[Seen]| {
            publishes(seen)
                .into_iter()
                .filter(|(_, reply)| reply["kind"] == "wifi.set")
                .collect::<Vec<_>>()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while wifi(&seen.lock().unwrap()).len() < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        let line: serde_json::Value = serde_json::from_str(&helper.join().unwrap()).unwrap();
        assert_eq!(line["wifi"]["broker"], format!("127.0.0.1:{port}"));
        let seen = seen.lock().unwrap().clone();
        let connects = seen
            .iter()
            .filter(|seen| matches!(seen, Seen::Connect { .. }))
            .count();
        assert_eq!(connects, 2);
        let replies = wifi(&seen);
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert_eq!(replies[0].1["status"], "starting");
        assert_eq!(replies[1].1["status"], "success");
        assert_eq!(
            replies[1].1["data"],
            serde_json::json!({"ssid": "New Home", "request_id": "r-7"})
        );
        // The result went out on the second connection: after its info.
        let second_info = seen
            .iter()
            .enumerate()
            .filter(
                |(_, seen)| matches!(seen, Seen::Publish(_, p) if p.contains("\"type\":\"info\"")),
            )
            .map(|(at, _)| at)
            .nth(1)
            .unwrap();
        let result_at = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Publish(_, p) if p.contains("\"status\":\"success\"") && p.contains("wifi.set")))
            .unwrap();
        assert!(result_at > second_info);
        for seen in &seen {
            if let Seen::Publish(_, payload) = seen {
                assert!(!payload.contains("fake-pass-NEW-1234"), "{payload}");
            }
        }
        let _ = std::fs::remove_file(&socket);
    }

    /// A change's result handed but lost with the connection before its ack
    /// is handed back, to be said again; one acked is done with.
    #[test]
    fn a_result_lost_before_its_ack_is_handed_back() {
        let outcome = |id: &str| crate::settings::Outcome {
            kind: "wifi.set".into(),
            result: Ok(None),
            request_id: Some(id.into()),
        };
        let mut handed = Handed::default();
        handed.in_order.push_back(Tagged::Change(outcome("acked")));
        handed.in_order.push_back(Tagged::Other);
        handed.in_order.push_back(Tagged::Change(outcome("sent")));
        handed.in_order.push_back(Tagged::Change(outcome("queued")));
        handed.sent(1);
        handed.sent(2);
        handed.sent(3);
        assert_eq!(handed.acked(1), None);
        let again: Vec<_> = handed
            .lost()
            .into_iter()
            .map(|outcome| outcome.request_id.unwrap())
            .collect();
        assert_eq!(again, ["sent", "queued"]);
    }

    /// #1673: with texts taken, a `chat.send` on `fa_channel` is acked on
    /// `fd_channel` with its run, as V1's, and the answer's pieces the lamp
    /// gives go out after it as `chat.event`. Without texts taken it is V1's
    /// refusal of a kind this lamp lacks.
    #[tokio::test]
    async fn a_text_is_acked_with_its_run_and_its_answer_goes_out_as_chat_events() {
        let (port, seen) = broker(
            Broker::Takes,
            vec![r#"{"cmd":"data","kind":"chat.send","data":{"message":"What time is it?","session_id":"s1"}}"#],
        )
        .await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                texting: true,
                to_give: vec![serde_json::json!({
                    "run_id": "run-1",
                    "event": {"type": "chat_response", "state": "final"},
                })],
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
            lamp.texts
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while publishes(&seen.lock().unwrap())
            .iter()
            .all(|(_, reply)| reply["kind"] != "chat.event")
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        let texts = lamp.await.unwrap();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].text, "What time is it?");
        assert_eq!(texts[0].session_id.as_deref(), Some("s1"));
        let replies = publishes(&seen.lock().unwrap());
        let ack = replies
            .iter()
            .find(|(_, reply)| reply["kind"] == "chat.send")
            .expect("the ack");
        assert_eq!(ack.0, "fd/d-42");
        assert_eq!(ack.1["status"], "success");
        assert_eq!(ack.1["data"]["run_id"], texts[0].run_id.as_str());
        assert_eq!(ack.1["data"]["session_id"], "s1");
        let events: Vec<_> = replies
            .iter()
            .filter(|(_, reply)| reply["kind"] == "chat.event")
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["type"], "data");
        assert_eq!(events[0].1["status"], "success");
        assert_eq!(events[0].1["data"]["event"]["type"], "chat_response");
        // The ack goes before the answer's pieces.
        let at = |kind: &str| replies.iter().position(|(_, reply)| reply["kind"] == kind);
        assert!(at("chat.send") < at("chat.event"));

        // Without texts taken: refused as a kind this lamp lacks.
        let mut lamp = Fake::default();
        let refused: serde_json::Value = serde_json::from_str(
            &reply(
                &identity(),
                &mut lamp,
                br#"{"cmd":"data","kind":"chat.send","data":{"message":"hi"}}"#,
                "2026-10-19T01:00:00Z",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(refused["status"], "failure");
        assert_eq!(refused["error"], crate::wire::not_here("chat.send"));
        assert!(lamp.texts.is_empty());
    }

    /// #1683, against the test broker: a report goes on `<fd_channel>/report`
    /// at QoS 1 with `info`'s id and leaves the queue on the broker's ack;
    /// one a dropped connection took before its ack goes again on the next.
    #[tokio::test]
    async fn reports_go_below_the_channel_and_leave_the_queue_on_the_ack() {
        for behaves in [Broker::Takes, Broker::DropsOnce] {
            let places = crate::reports::tests::places("online");
            std::fs::write(&places.setup, r#"{"share":true,"asked":1}"#).unwrap();
            let shared = crate::reports::Shared::new(crate::reports::Reports::open(places));
            shared
                .with(|reports| {
                    reports.health(1_792_296_000, None, &crate::reports::Seen::default())
                })
                .unwrap();
            let (port, seen) = broker(behaves, Vec::new()).await;
            let (accounts_tx, accounts) = watch::channel(Some(account(port)));
            let (_identity_tx, me) = watch::channel(identity());
            let reports = shared.clone();
            let lamp = tokio::spawn(async move {
                let mut lamp = Fake {
                    reports: Some(reports),
                    ..Fake::default()
                };
                let mut say = |_: &str| {};
                run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
            });
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while shared.with(|reports| reports.queued()) > 0
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            drop(accounts_tx);
            lamp.await.unwrap();
            assert_eq!(shared.with(|reports| reports.queued()), 0, "acked");
            let sent = publishes(&seen.lock().unwrap());
            let reports: Vec<_> = sent
                .iter()
                .filter(|(topic, _)| topic == "fd/d-42/report")
                .collect();
            assert!(!reports.is_empty(), "{sent:?}");
            assert!(reports.iter().all(|(_, report)| report["seq"] == 1
                && report["id"] == "d-42"
                && report["kind"] == "health"));
            // Never a reply on the channel itself.
            assert!(
                sent.iter()
                    .filter(|(topic, _)| topic == "fd/d-42")
                    .all(|(_, reply)| reply["type"] != "report")
            );
        }
    }

    /// #1673: `poll` makes the connection itself, so the texts' tick beside it
    /// must never drop it part way. A broker that answers CONNECT after
    /// 300 ms, six ticks, is joined at the first try, and only once.
    #[tokio::test]
    async fn a_slow_connack_is_waited_for_while_texts_are_taken() {
        let (port, seen) = broker(Broker::SlowConnAck, Vec::new()).await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                texting: true,
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
        });
        let subscribed = Seen::Subscribe("fa/d-42".into());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !seen.lock().unwrap().contains(&subscribed) && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        let seen = seen.lock().unwrap().clone();
        let connects = seen
            .iter()
            .filter(|s| matches!(s, Seen::Connect { .. }))
            .count();
        assert!(seen.contains(&subscribed), "{seen:?}");
        assert_eq!(connects, 1, "{seen:?}");
    }

    /// #1396: the lamp connects as V1 does, subscribes to `fa_channel`, says
    /// `info` unasked, and answers each command on `fd_channel`: a kind it
    /// has, and a command it lacks, refused plainly.
    #[tokio::test]
    async fn the_lamp_comes_online_as_v1_and_answers_on_its_channel() {
        let (port, seen) = broker(
            Broker::Takes,
            vec![
                r#"{"cmd":"data","kind":"volume.get"}"#,
                r#"{"cmd":"add_channel","channel":"telegram"}"#,
            ],
        )
        .await;
        let lines = against(port, &seen, Duration::from_millis(50), |seen| {
            publishes(seen).len() >= 3
        })
        .await;
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen[0],
            Seen::Connect {
                client_id: "os-server-d-42".into(),
                keep_alive: 60,
                clean_start: true,
                expiry: Some(60),
                login: Some(("lamp".into(), "s3cret-pw".into())),
            }
        );
        assert!(seen.contains(&Seen::Subscribe("fa/d-42".into())));
        let sent = publishes(&seen);
        assert!(sent.iter().all(|(topic, _)| topic == "fd/d-42"));
        assert_eq!(sent[0].1["type"], "info");
        assert_eq!(sent[0].1["id"], "d-42");
        // Which Lamp it is, beside V1's `device`, on every reply: the
        // unasked `info`, which the backend keeps on the device's record,
        // included (#1396).
        assert_eq!(sent[0].1["device"], "lamp");
        assert!(
            sent.iter()
                .all(|(_, reply)| reply["generation"] == crate::wire::GENERATION),
            "{sent:?}"
        );
        let volume = sent
            .iter()
            .find(|(_, reply)| reply["kind"] == "volume.get")
            .unwrap();
        assert_eq!(
            volume.1["data"],
            serde_json::json!({"volume": 75, "raw": 60, "max_volume": 80})
        );
        let refused = sent
            .iter()
            .find(|(_, reply)| reply["type"] == "add_channel")
            .unwrap();
        assert_eq!(refused.1["error"], "not on this lamp: add_channel");
        assert_eq!(
            lines,
            ["online on the broker 127.0.0.1:".to_owned() + &port.to_string()]
        );
        assert!(lines.iter().all(|line| !line.contains("s3cret-pw")));
    }

    /// The first builder's test on #1396: a broker that refuses the login is
    /// tried again after a wait that doubles, never in a tight loop, and the
    /// journal hears of it once.
    #[tokio::test]
    async fn a_refused_login_backs_off_and_is_said_once() {
        let (port, seen) = broker(Broker::Refuses, Vec::new()).await;
        let began = tokio::time::Instant::now();
        let lines = against(port, &seen, Duration::from_millis(50), |_| {
            began.elapsed() >= Duration::from_millis(1_000)
        })
        .await;
        let connects = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| matches!(seen, Seen::Connect { .. }))
            .count();
        // 50, 100, 200, 400 ms of waiting in the second: five tries at most.
        assert!((2..=6).contains(&connects), "{connects}");
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("offline, trying again"), "{lines:?}");
        assert!(lines[0].contains("BadUserNamePassword"), "{lines:?}");
        assert!(!lines[0].contains("s3cret-pw"));
    }

    /// A connection the broker drops is made again, and `fa_channel` is
    /// subscribed again on it, as V1's re-applied subscription.
    #[tokio::test]
    async fn a_dropped_connection_is_made_again_and_subscribed_again() {
        let (port, seen) = broker(Broker::DropsOnce, Vec::new()).await;
        let _lines = against(port, &seen, Duration::from_millis(50), |seen| {
            seen.iter()
                .filter(|seen| **seen == Seen::Subscribe("fa/d-42".into()))
                .count()
                >= 2
        })
        .await;
        let seen = seen.lock().unwrap().clone();
        let connects = seen
            .iter()
            .filter(|s| matches!(s, Seen::Connect { .. }))
            .count();
        let subscribes = seen
            .iter()
            .filter(|s| matches!(s, Seen::Subscribe(_)))
            .count();
        assert!(connects >= 2 && subscribes >= 2, "{seen:?}");
    }
}
