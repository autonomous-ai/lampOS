//! Staying on the company broker, as V1's os-server does (Nine's contract on
//! #1396): MQTT v5 to `mqtt://{endpoint}:{port}` with the account's login and
//! V1's client id, a clean start and a 60 s session expiry, `fa_channel`
//! subscribed at QoS 1 on every connection and every reply on `fd_channel`
//! at QoS 1. On each connection the lamp says `info` unasked. A connection
//! that fails, or that the broker refuses, is tried again after a wait that
//! doubles to a minute, and said once in the journal, not on every try.
use crate::account::Account;
use crate::answer::{Lamp, answer};
use crate::wire::{Command, Identity, parse};
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::mqttbytes::v5::{ConnectProperties, Packet};
use rumqttc::v5::{AsyncClient, Event, MqttOptions};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

/// The keepalive: a dead link is noticed within about a minute and a half.
/// V1 asks the protocol's longest, 65,535 s, which never notices one.
pub const KEEP_ALIVE: Duration = Duration::from_secs(60);
/// V1's session expiry.
pub const SESSION_EXPIRY_S: u32 = 60;

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
            max_packet_size: None,
            topic_alias_max: None,
            request_response_info: None,
            request_problem_info: None,
            user_properties: Vec::new(),
            authentication_method: None,
            authentication_data: None,
        });
    options
}

/// The reply to one message on `fa_channel`, or nothing for one that is no
/// command.
pub fn reply(
    identity: &Identity,
    lamp: &mut impl Lamp,
    payload: &[u8],
    now: &str,
) -> Option<String> {
    let reply = match parse(payload).ok()? {
        Command::Info => identity.info(now),
        Command::Data { kind, data } => identity.data(now, &kind, &answer(lamp, &kind, &data)),
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
    // What was last said, so a state is said once.
    let mut said = String::new();
    let mut tell = |say: &mut dyn Say, line: String| {
        if said != line {
            say.line(&line);
            said = line;
        }
    };
    loop {
        match events.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                wait = first;
                let _ = client.try_subscribe(account.fa_channel.as_str(), QoS::AtLeastOnce);
                let info = identity.borrow().info(&now()).to_string();
                let _ =
                    client.try_publish(account.fd_channel.as_str(), QoS::AtLeastOnce, false, info);
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
                if message.topic.as_ref() != account.fa_channel.as_bytes() {
                    continue;
                }
                let me = identity.borrow().clone();
                if let Some(reply) = reply(&me, lamp, &message.payload, &now()) {
                    let _ = client.try_publish(
                        account.fd_channel.as_str(),
                        QoS::AtLeastOnce,
                        false,
                        reply,
                    );
                }
            }
            Ok(_) => {}
            Err(error) => {
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
    }

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
                            for (n, command) in commands.iter().enumerate() {
                                let mut publish =
                                    Publish::new("fa/d-42", QoS::AtLeastOnce, *command, None);
                                publish.pkid = 100 + n as u16;
                                Packet::Publish(publish).write(&mut out, None).unwrap();
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
                set: Vec::new(),
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
