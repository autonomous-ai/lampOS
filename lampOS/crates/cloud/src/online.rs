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
use rumqttc::v5::mqttbytes::v5::{ConnectProperties, LastWill, LastWillProperties, Packet};
use rumqttc::v5::{AsyncClient, Event, MqttOptions};
use std::collections::{HashMap, HashSet, VecDeque};
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
/// base64), with 1 MiB more for its envelope and for a sender that writes
/// base64's `/` as `\/` (one character in 64, about 170 KiB on the largest
/// photo; Reviewer-lead on #2272). rumqttc's own is 10 KiB, which dropped
/// the connection on every photo (Mobile on lamp-0c89, 20 Oct). The broker
/// holds back a packet past this one, so a photo over it gets no answer
/// rather than `photos::asked`'s refusal: the app sends at most 1 MiB.
pub const MOST_PACKET_BYTES: u32 = (crate::photos::MOST_BYTES.div_ceil(3) * 4 + (1 << 20)) as u32;

/// Where lamp-cloud reads whether it leaves the broker a will: `on`, and
/// nothing else ([`options_with_will`]). Off unless set; on in the product
/// unit (#2636), once the backend read it (Backend3 on Queue #2602, 21 Oct
/// 19:4x).
pub const OFFLINE_WILL_ENVIRONMENT: &str = "LAMP_CLOUD_OFFLINE_WILL";
/// How long the broker waits, after it finds the lamp's connection lost,
/// before it says the will. The backend marks a lamp offline only 5 to 8 min
/// after its last ping or `info` (a check every 3 min, a 5 min bound). With
/// the will, a killed or stopped lamp-cloud shows offline about 30 s after its
/// connection closes (it never sends a DISCONNECT), and a lost network or a
/// power cut within about 2 min (the keepalive's ~90 s, then this). A
/// reconnect within the delay sends no offline: on lamp-06c1 (23 Oct 09:11)
/// a restart with the will on brought only the new connection's `info`, so
/// the record stays online (docs/cloud.md, #3035).
pub const WILL_DELAY_S: u32 = 30;
// The broker says a will at the earlier of its delay and the session's end
// (MQTT v5 3.1.2.11.2, 3.1.3.2.2): a session that ends first, or a session
// expiry of 0, would say it sooner than `WILL_DELAY_S`.
const _: () = assert!(SESSION_EXPIRY_S > WILL_DELAY_S);

/// [`options`] with the lamp's will: `offline` from `identity`, on its own
/// reply channel `fd_channel`, at QoS 1, not retained, after
/// [`WILL_DELAY_S`]. The backend takes an `offline` with the lamp's `id` on
/// its own channel as the lamp gone, and its next ping or `info` as back.
/// The will's `time`, `timezone` and `local_ip` are those of when the client
/// was made (start, or a relink after a Wi-Fi join): a reconnect inside the
/// event loop keeps them. The backend reads only `type`, `id` and `device` of
/// an `offline`, and takes messages on a channel in the order they come.
pub fn options_with_will(account: &Account, identity: &Identity) -> Option<MqttOptions> {
    let mut options = options(account)?;
    let properties = LastWillProperties {
        delay_interval: Some(WILL_DELAY_S),
        payload_format_indicator: None,
        message_expiry_interval: None,
        content_type: None,
        response_topic: None,
        correlation_data: None,
        user_properties: Vec::new(),
    };
    options.set_last_will(LastWill::new(
        account.fd_channel.as_str(),
        identity.offline(&now()).to_string(),
        QoS::AtLeastOnce,
        false,
        Some(properties),
    ));
    Some(options)
}

/// Where lamp-cloud reads whether the broker is reached over TLS: `on`, and
/// nothing else (`docs/cloud.md`, "Stays on the broker"). The account's
/// port is then the broker's TLS port.
pub const TLS_ENVIRONMENT: &str = "LAMP_CLOUD_TLS";

/// The client's options for `account`, with TLS as the unit says; none
/// when TLS is asked and cannot be set up, so it never connects in the
/// clear in its place.
pub fn options(account: &Account) -> Option<MqttOptions> {
    options_with(
        account,
        std::env::var(TLS_ENVIRONMENT).is_ok_and(|value| value == "on"),
    )
}

/// The client's options for `account`: V1's, with this lamp's keepalive,
/// over TLS (1.2 and 1.3, the webpki roots, as the ping has it) when `tls`.
pub fn options_with(account: &Account, tls: bool) -> Option<MqttOptions> {
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
    if tls {
        let config = crate::beat::tls().ok()?;
        options.set_transport(rumqttc::Transport::tls_with_config(
            rumqttc::TlsConfiguration::Rustls(Arc::new(config)),
        ));
    }
    Some(options)
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
                    || crate::settings::KEYS_KINDS.contains(&kind.as_str())
                    || crate::settings::PLACE_KINDS.contains(&kind.as_str())
                    || kind == crate::settings::WIFI_KIND)
            {
                let mut starting = identity.starting(now, &kind);
                // The keys' kinds say the app's id back at once too (#2094).
                if crate::settings::KEYS_KINDS.contains(&kind.as_str())
                    && let Some(id) = data
                        .get("request_id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| {
                            !id.is_empty()
                                && id.len() <= 64
                                && id.bytes().all(|b| b.is_ascii_graphic())
                        })
                {
                    starting["data"] = serde_json::json!({"request_id": id});
                }
                return Some(starting.to_string());
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
    /// Every publish the client sent and the broker has not acknowledged,
    /// by packet, whatever it was: with [`Self::in_order`], what of
    /// lamp-cloud's own is ahead of a new publish on the session (#2562).
    unacked: HashSet<u16>,
    /// The skills' lines for the phone the broker acknowledged, by skill,
    /// for the journal.
    unasked_acked: Vec<String>,
    /// Those the connection took before their ack, oldest first, to go
    /// again.
    unasked_lost: Vec<crate::unasked::Piece>,
    /// Changes' results the broker acknowledged, for `Changes::acked`.
    changes_acked: Vec<crate::settings::Outcome>,
    /// Journal lines, one per step of a change's result (Mobile on #1959,
    /// 21:0x): handed, sent as a packet, acked or lost. Never its data.
    journal: Vec<String>,
}

/// What a handed publish was.
enum Tagged {
    Other,
    /// A report's `seq`.
    Report(u64),
    /// A change's result, said again if it is lost before its ack.
    Change(crate::settings::Outcome),
    /// A skill's line for the phone, sent again if it is lost before its
    /// ack (`crate::unasked`).
    Unasked(crate::unasked::Piece),
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
        if let Tagged::Change(outcome) = &tag {
            self.journal.push(if handed {
                format!("{}: handed to the client at QoS 1", outcome.journal())
            } else {
                format!("{}: not handed to the client, kept", outcome.journal())
            });
        }
        if handed {
            self.in_order.push_back(tag);
        }
        handed
    }

    /// Whether anything of lamp-cloud's own is handed to the client and not
    /// yet acknowledged, so a publish handed now may wait behind it in the
    /// client (a broker's receive maximum): a named text then goes unnamed
    /// (#2562).
    fn busy(&self) -> bool {
        !self.in_order.is_empty() || !self.unacked.is_empty()
    }

    /// The client sent the next one as packet `pkid`.
    fn sent(&mut self, pkid: u16) {
        self.unacked.insert(pkid);
        match self.in_order.pop_front() {
            Some(Tagged::Other) | None => {}
            Some(tag) => {
                if let Tagged::Change(outcome) = &tag {
                    self.journal
                        .push(format!("{}: sent as packet {pkid}", outcome.journal()));
                }
                self.by_packet.insert(pkid, tag);
            }
        }
    }

    /// The report the broker acknowledged as `pkid`, if it was one; a
    /// change's result acknowledged is done with.
    fn acked(&mut self, pkid: u16) -> Option<u64> {
        self.unacked.remove(&pkid);
        match self.by_packet.remove(&pkid) {
            Some(Tagged::Report(seq)) => Some(seq),
            Some(Tagged::Unasked(piece)) => {
                self.unasked_acked.push(piece.skill);
                None
            }
            Some(Tagged::Change(outcome)) => {
                self.journal.push(format!(
                    "{}: acked by the broker, packet {pkid}",
                    outcome.journal()
                ));
                self.changes_acked.push(outcome);
                None
            }
            _ => None,
        }
    }

    /// The connection went: the client drops what it had not had
    /// acknowledged (a clean start), so the lamp sends it again. The
    /// changes' results among it are handed back, oldest first.
    fn lost(&mut self) -> Vec<crate::settings::Outcome> {
        self.unacked.clear();
        let mut by_packet: Vec<_> = self.by_packet.drain().collect();
        by_packet.sort_by_key(|(pkid, _)| *pkid);
        let journal = &mut self.journal;
        by_packet
            .into_iter()
            .map(|(pkid, tag)| (Some(pkid), tag))
            .chain(self.in_order.drain(..).map(|tag| (None, tag)))
            .filter_map(|(pkid, tag)| match tag {
                Tagged::Change(outcome) => {
                    let packet =
                        pkid.map_or_else(|| "not sent".to_owned(), |p| format!("packet {p}"));
                    journal.push(format!(
                        "{}: no ack before the connection ended ({packet}), said again on the next",
                        outcome.journal()
                    ));
                    Some(outcome)
                }
                Tagged::Unasked(piece) => {
                    self.unasked_lost.push(piece);
                    None
                }
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

/// The connection is gone: what it held unacknowledged goes again on the
/// next one.
fn dropped(
    handed: &mut Handed,
    lamp: &mut impl Lamp,
    changes: &Option<Arc<crate::settings::Changes>>,
    reports: &Option<Arc<crate::reports::Shared>>,
) {
    let again = handed.lost();
    let unasked = std::mem::take(&mut handed.unasked_lost);
    if !unasked.is_empty() {
        lamp.owe_unasked(unasked);
    }
    if let Some(changes) = changes {
        changes.owe(again);
    }
    if let Some(shared) = reports {
        shared.with(|reports| reports.unhand(None));
    }
}

/// Woken when a Wi-Fi change may want the link made again; never without
/// the changes' switch.
async fn relink_asked(changes: &Option<Arc<crate::settings::Changes>>) {
    match changes {
        Some(changes) => changes.relink_asked().await,
        None => std::future::pending().await,
    }
}

/// Once a Wi-Fi change wants the link made again (taken).
async fn relink_wanted(changes: &Option<Arc<crate::settings::Changes>>) {
    loop {
        relink_asked(changes).await;
        if changes
            .as_ref()
            .is_some_and(|changes| changes.take_relink())
        {
            return;
        }
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
    // With the switch, the broker is left a will (`options_with_will`), on
    // the first connection and on every one made again after a Wi-Fi change.
    let will = lamp.offline_will();
    let made = || {
        if will {
            options_with_will(account, &identity.borrow())
        } else {
            options(account)
        }
    };
    let Some(options) = made() else {
        say.line("the broker's TLS cannot be set up (LAMP_CLOUD_TLS): not connecting in the clear in its place");
        return std::future::pending().await;
    };
    let (mut client, mut events) = AsyncClient::new(options, 64);
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
        let polled = {
            let poll = events.poll();
            tokio::pin!(poll);
            loop {
                tokio::select! {
                    polled = &mut poll => break Some(polled),
                    // A Wi-Fi change moved the lamp to another network (#2617's
                    // P2-1): the link is dropped at once, not left to the
                    // keepalive's 60 to 120 s.
                    () = relink_asked(&changes) => {
                        if changes.as_ref().is_some_and(|changes| changes.take_relink()) {
                            break None;
                        }
                    }
                    _ = tick.tick(), if texting || changes.is_some() => {
                        let me = identity.borrow().clone();
                        if texting {
                            for data in lamp.chat_events(crate::chat::now_ms()) {
                                let event = me.data(&now(), "chat.event", &Ok(Some(data))).to_string();
                                handed.publish(&client, account.fd_channel.as_str(), event, None);
                            }
                            // A skill's lines for the phone (`crate::unasked`):
                            // taken off the bus always, sent only while
                            // connected, and kept until the broker's ack.
                            let heard = lamp.unasked(crate::chat::now_ms(), connected);
                            for line in heard.lines {
                                say.line(&line);
                            }
                            let mut owed = Vec::new();
                            for mut piece in heard.due {
                                if !owed.is_empty() {
                                    owed.push(piece);
                                    continue;
                                }
                                // Named only straight to the wire: behind a
                                // publish not yet acknowledged it may wait in
                                // the client past the eyes' word (#2562).
                                if handed.busy() && piece.hold() {
                                    say.line(crate::unasked::UNACKED_AHEAD);
                                }
                                let event = me
                                    .data(&now(), "chat.event", &Ok(Some(piece.data.clone())))
                                    .to_string();
                                let tag = Tagged::Unasked(piece.clone());
                                if !handed.publish_tagged(&client, account.fd_channel.as_str(), event, tag) {
                                    owed.push(piece);
                                }
                            }
                            if !owed.is_empty() {
                                lamp.owe_unasked(owed);
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
            }
        };
        for line in std::mem::take(&mut handed.journal) {
            say.line(&line);
        }
        let Some(polled) = polled else {
            dropped(&mut handed, lamp, &changes, &reports);
            connected = false;
            for line in std::mem::take(&mut handed.journal) {
                say.line(&line);
            }
            tell(
                say,
                "a Wi-Fi change moved the lamp: the link is dropped and made again at once"
                    .to_owned(),
            );
            let Some(options) = made() else {
                drop(client);
                drop(events);
                say.line("the broker's TLS cannot be set up (LAMP_CLOUD_TLS): not connecting in the clear in its place");
                return std::future::pending().await;
            };
            (client, events) = AsyncClient::new(options, 64);
            wait = first;
            continue;
        };
        match polled {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                wait = first;
                if let Some(changes) = &changes {
                    changes.set_broker(account.mqtt_endpoint.trim(), account.mqtt_port);
                    changes.online();
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
            Ok(Event::Outgoing(Outgoing::Publish(pkid))) => {
                handed.sent(pkid);
                for line in std::mem::take(&mut handed.journal) {
                    say.line(&line);
                }
            }
            Ok(Event::Incoming(Packet::PubAck(ack))) => {
                if let (Some(seq), Some(shared)) = (handed.acked(ack.pkid), &reports) {
                    shared.with(|reports| reports.acked(seq));
                }
                for outcome in std::mem::take(&mut handed.changes_acked) {
                    if let Some(changes) = &changes {
                        changes.acked(&outcome);
                    }
                }
                for line in std::mem::take(&mut handed.journal) {
                    say.line(&line);
                }
                // One line a piece, the skill and the count, never its text.
                for skill in std::mem::take(&mut handed.unasked_acked) {
                    let count = lamp.unasked_sent();
                    say.line(&crate::unasked::sent_line(&skill, count));
                }
            }
            Ok(_) => {}
            Err(error) => {
                connected = false;
                dropped(&mut handed, lamp, &changes, &reports);
                for line in std::mem::take(&mut handed.journal) {
                    say.line(&line);
                }
                // The error names a code or an I/O kind, never the login.
                tell(say, format!("offline, trying again: {error}"));
                // A Wi-Fi change that moves the lamp meanwhile cuts the wait
                // short: one try at once, then the waits from the first.
                let relinked = tokio::select! {
                    () = tokio::time::sleep(wait) => false,
                    () = relink_wanted(&changes) => true,
                };
                wait = if relinked {
                    first
                } else {
                    (wait * 2).min(crate::ping::MOST)
                };
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
        ConnAck, ConnAckProperties, ConnectReturnCode, PingResp, PubAck, Publish, SubAck,
        SubscribeReasonCode,
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
            /// The will left with the broker: its topic, message, QoS,
            /// retain and delay.
            will: Option<(String, String, u8, bool, Option<u32>)>,
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
        /// Sends `commands` on the first connection, then goes silent on it
        /// after its second reply, the socket held open: the radio switched
        /// under a link only the keepalive would notice (#2617's P2-1).
        /// Later connections are taken, with a `wifi.joined` acked 1 s late.
        StaleFirstLateAck,
        /// Sends `commands` on the first connection and closes it after its
        /// second reply; later ones are refused until [`GATE`] opens.
        DropsThenGated,
        /// Takes every connection with a receive maximum of 1 in its
        /// CONNACK, and holds back the PUBACK of each connection's first
        /// publish (`info`) for [`HELD_ACK`], so the client keeps any
        /// publish after it in its queue meanwhile (#2562, the reviewer's
        /// correction read on #2769).
        HoldsFirstAck,
    }

    /// How long [`Broker::HoldsFirstAck`] holds the first PUBACK back.
    const HELD_ACK: Duration = Duration::from_millis(700);

    /// When the tests' brokers saw each CONNECT, by port.
    static CONNECTS: Mutex<Vec<(u16, std::time::Instant)>> = Mutex::new(Vec::new());
    /// [`Broker::DropsThenGated`]'s gate: open, its connections are taken.
    static GATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn connects_at(port: u16) -> Vec<std::time::Instant> {
        CONNECTS
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| *at == port)
            .map(|(_, when)| *when)
            .collect()
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
                        Packet::Connect(connect, will, login) => {
                            CONNECTS
                                .lock()
                                .unwrap()
                                .push((port, std::time::Instant::now()));
                            log.lock().unwrap().push(Seen::Connect {
                                client_id: connect.client_id.clone(),
                                keep_alive: connect.keep_alive,
                                clean_start: connect.clean_start,
                                expiry: connect
                                    .properties
                                    .as_ref()
                                    .and_then(|p| p.session_expiry_interval),
                                login: login.map(|l| (l.username, l.password)),
                                will: will.map(|w| {
                                    (
                                        String::from_utf8_lossy(&w.topic).into_owned(),
                                        String::from_utf8_lossy(&w.message).into_owned(),
                                        w.qos as u8,
                                        w.retain,
                                        w.properties.and_then(|p| p.delay_interval),
                                    )
                                }),
                            });
                            if behaves == Broker::SlowConnAck {
                                tokio::time::sleep(Duration::from_millis(300)).await;
                            }
                            let refuses = behaves == Broker::Refuses
                                || (behaves == Broker::DropsThenGated
                                    && connections > 1
                                    && !GATE.load(std::sync::atomic::Ordering::SeqCst));
                            let code = if refuses {
                                ConnectReturnCode::BadUserNamePassword
                            } else {
                                ConnectReturnCode::Success
                            };
                            let properties =
                                (behaves == Broker::HoldsFirstAck).then(|| ConnAckProperties {
                                    session_expiry_interval: None,
                                    receive_max: Some(1),
                                    max_qos: None,
                                    retain_available: None,
                                    max_packet_size: None,
                                    assigned_client_identifier: None,
                                    topic_alias_max: None,
                                    reason_string: None,
                                    user_properties: Vec::new(),
                                    wildcard_subscription_available: None,
                                    subscription_identifiers_available: None,
                                    shared_subscription_available: None,
                                    server_keep_alive: None,
                                    response_information: None,
                                    server_reference: None,
                                    authentication_method: None,
                                    authentication_data: None,
                                });
                            Packet::ConnAck(ConnAck {
                                session_present: false,
                                code,
                                properties,
                            })
                            .write(&mut out, None)
                            .unwrap();
                            let _ = socket.write_all(&out).await;
                            if refuses {
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
                            let once = matches!(
                                behaves,
                                Broker::CommandsOnceThenDrops
                                    | Broker::StaleFirstLateAck
                                    | Broker::DropsThenGated
                            );
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
                            let payload = String::from_utf8_lossy(&publish.payload).into_owned();
                            if behaves == Broker::StaleFirstLateAck
                                && payload.contains("\"kind\":\"wifi.joined\"")
                            {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                            log.lock().unwrap().push(Seen::Publish(
                                String::from_utf8_lossy(&publish.topic).into_owned(),
                                payload,
                            ));
                            if behaves == Broker::HoldsFirstAck && replies == 0 {
                                tokio::time::sleep(HELD_ACK).await;
                            }
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
                    if matches!(
                        behaves,
                        Broker::CommandsOnceThenDrops | Broker::DropsThenGated
                    ) && connections == 1
                        && replies == 2
                    {
                        break;
                    }
                    if behaves == Broker::StaleFirstLateAck && connections == 1 && replies == 2 {
                        // Silent, the socket held: reads go nowhere.
                        let mut chunk = [0u8; 4096];
                        while matches!(socket.read(&mut chunk).await, Ok(n) if n > 0) {}
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
    /// `fd/d-42`, not dropped with it as over rumqttc's 10 KiB, even from a
    /// sender that writes base64's `/` as `\/` (Reviewer-lead on #2272). The
    /// broker is told the same limit.
    #[tokio::test]
    async fn a_command_with_the_largest_photo_is_read_and_answered() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        // A photo's bytes look random, so its base64 has `/` once in 64.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut photo: Vec<u8> = (0..crate::photos::MOST_BYTES)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        photo[..2].copy_from_slice(&[0xFF, 0xD8]);
        let command = format!(
            r#"{{"cmd":"data","kind":"faces.add_photo","data":{{"name":"Quillo","photo":"{}"}}}}"#,
            STANDARD.encode(&photo).replace('/', "\\/")
        );
        let largest = crate::photos::MOST_BYTES.div_ceil(3) * 4;
        assert!(command.len() > largest + 128 * 1_024, "{}", command.len());
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
            options_with(&account(port), false)
                .unwrap()
                .max_packet_size(),
            Some(MOST_PACKET_BYTES)
        );
    }

    /// `LAMP_CLOUD_TLS`: on, the broker is reached over TLS; off, as V1's.
    #[test]
    fn with_its_switch_the_broker_is_reached_over_tls() {
        let plain = options_with(&account(1883), false).unwrap();
        assert!(matches!(plain.transport(), rumqttc::Transport::Tcp));
        let tls = options_with(&account(8883), true).unwrap();
        assert!(matches!(tls.transport(), rumqttc::Transport::Tls(_)));
        assert_eq!(tls.max_packet_size(), Some(MOST_PACKET_BYTES));
    }

    /// #2094: a person's own key from the app is acked `starting` at once
    /// and the helper's answer follows, with the app's id; the key goes to
    /// the helper's line and is in no message on the channel.
    #[tokio::test]
    async fn an_own_key_is_acked_at_once_and_is_in_no_reply() {
        let socket = std::env::temp_dir().join(format!("lco-{}-keys.sock", std::process::id()));
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
                .write_all(b"{\"refused\":\"that key was refused\",\"code\":\"refused\"}\n")
                .unwrap();
            line
        });
        let (port, seen) = broker(
            Broker::Takes,
            vec![
                r#"{"cmd":"data","kind":"keys.set","data":{"service":"deepgram","key":"sk-planted-2094-cloud","request_id":"r-7"}}"#,
            ],
        )
        .await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let changes = crate::settings::Changes::with_helpers(
            std::env::temp_dir().join("lco-no-change.sock"),
            Duration::from_secs(5),
            None,
            Some((socket.clone(), Duration::from_secs(5))),
        );
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                changes: Some(changes),
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
        });
        let sets = |seen: &[Seen]| {
            publishes(seen)
                .into_iter()
                .filter(|(_, reply)| reply["kind"] == "keys.set")
                .collect::<Vec<_>>()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while sets(&seen.lock().unwrap()).len() < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        assert_eq!(
            helper.join().unwrap(),
            "{\"key\":\"sk-planted-2094-cloud\",\"set\":\"deepgram\"}\n"
        );
        let replies = sets(&seen.lock().unwrap());
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].1["status"], "starting");
        assert_eq!(replies[0].1["data"]["request_id"], "r-7");
        assert_eq!(replies[1].1["status"], "failure");
        assert_eq!(replies[1].1["error"], "that key was refused");
        assert_eq!(
            replies[1].1["data"],
            serde_json::json!({"code": "refused", "request_id": "r-7"})
        );
        for (_, reply) in &replies {
            assert!(!reply.to_string().contains("sk-planted"), "{reply}");
        }
        let _ = std::fs::remove_file(&socket);
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

    struct JoiningHelper {
        finish: std::sync::mpsc::Sender<()>,
        thread: std::thread::JoinHandle<(std::time::Instant, std::time::Instant, String)>,
    }

    impl JoiningHelper {
        fn join(self) -> std::thread::Result<(std::time::Instant, std::time::Instant, String)> {
            // The scenario has stopped and awaited lamp-cloud. Keep its
            // helper socket open until then, and preserve any helper panic.
            let _ = self.finish.send(());
            self.thread.join()
        }
    }

    /// A fake `lamp-setup wifi` on `socket`: it proves the new network
    /// `after` its request and says `joined`, then waits for `{"online":
    /// true}`; with `back`, it then goes back and says so. It hands back
    /// when it said `joined` and when it heard `online`, after the scenario
    /// releases its socket with `join`.
    fn joining_helper(socket: &std::path::Path, after: Duration, back: bool) -> JoiningHelper {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let (finish, finished) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            std::thread::sleep(after);
            stream
                .write_all(b"{\"joined\":{\"ssid\":\"New Home\",\"online_within_ms\":10000,\"confirm_within_ms\":120000}}\n")
                .unwrap();
            let joined_at = std::time::Instant::now();
            let mut online = String::new();
            reader.read_line(&mut online).unwrap();
            let online_at = std::time::Instant::now();
            if back {
                stream
                    .write_all(b"{\"refused\":\"the app did not confirm the new Wi-Fi within 120 s; the lamp is back on its old Wi-Fi\"}\n")
                    .unwrap();
            }
            // Held open while lamp-cloud waits for the app's confirmation.
            // A failed scenario drops the sender; a stuck one fails this
            // bound, which exceeds wifi_against's ten-second deadline.
            match finished.recv_timeout(Duration::from_secs(15)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("Wi-Fi scenario did not release its helper within 15 s")
                }
            }
            (joined_at, online_at, online)
        });
        JoiningHelper { finish, thread }
    }

    /// The lamp with the Wi-Fi's switch against `port` until `until` holds
    /// of what the broker saw and the lines said, or 10 s; then the lines.
    async fn wifi_against(
        port: u16,
        seen: &Arc<Mutex<Vec<Seen>>>,
        socket: &std::path::Path,
        first: Duration,
        will: bool,
        until: impl Fn(&[Seen], &[String]) -> bool,
    ) -> Vec<String> {
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let changes = crate::settings::Changes::with_wifi(
            std::env::temp_dir().join("lco-never.sock"),
            Duration::from_secs(5),
            Some((socket.to_owned(), Duration::from_secs(10))),
        );
        let lines = Arc::new(Mutex::new(Vec::new()));
        let said = lines.clone();
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                changes: Some(changes),
                will,
                ..Fake::default()
            };
            let mut say = |line: &str| said.lock().unwrap().push(line.to_owned());
            run(accounts, me, &mut lamp, &mut say, first).await;
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !until(&seen.lock().unwrap(), &lines.lock().unwrap())
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        Arc::try_unwrap(lines).unwrap().into_inner().unwrap()
    }

    fn wifi_replies(seen: &[Seen]) -> Vec<serde_json::Value> {
        publishes(seen)
            .into_iter()
            .map(|(_, reply)| reply)
            .filter(|reply| {
                reply["kind"]
                    .as_str()
                    .is_some_and(|kind| kind.starts_with("wifi."))
            })
            .collect()
    }

    const WIFI_SET: &str = r#"{"cmd":"data","kind":"wifi.set","data":{"ssid":"New Home","password":"fake-pass-NEW-1234","request_id":"r-7"}}"#;

    /// The reviewer-lead's P2 and the first builder's P2-1 on #2617: once
    /// the helper says `joined`, the old link, which only the keepalive
    /// would notice dead after 60 to 120 s, is dropped and made again at
    /// once; `wifi.joined` goes out on the new one, and the window starts
    /// only when the broker acks it, 1 s late here. Each step of each
    /// result is journalled, with neither the name nor the password.
    #[tokio::test]
    async fn p2_a_joined_wifi_on_a_stale_link_reconnects_at_once_and_its_window_starts_at_the_ack()
    {
        let socket = std::env::temp_dir().join(format!("lco-{}-stale.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let helper = joining_helper(&socket, Duration::from_millis(300), false);
        let (port, seen) = broker(Broker::StaleFirstLateAck, vec![WIFI_SET]).await;
        let lines = wifi_against(
            port,
            &seen,
            &socket,
            Duration::from_secs(1),
            false,
            // The broker's count, and the client's line for its ack, which
            // can come after it.
            |seen, lines| {
                wifi_replies(seen).len() >= 2
                    && lines
                        .iter()
                        .any(|line| line.contains("wifi.joined success, request_id r-7: acked"))
            },
        )
        .await;
        let (joined_at, online_at, online) = helper.join().unwrap();
        assert_eq!(online, "{\"online\":true}\n");
        let connects = connects_at(port);
        assert_eq!(connects.len(), 2, "{connects:?}");
        // At once, not after the keepalive.
        let again = connects[1].duration_since(joined_at);
        assert!(again < Duration::from_millis(500), "{again:?}");
        // The window starts at the broker's ack, 1 s after the publish.
        let window = online_at.duration_since(connects[1]);
        assert!(window >= Duration::from_secs(1), "{window:?}");
        let seen = seen.lock().unwrap().clone();
        let replies = wifi_replies(&seen);
        assert_eq!(replies[0]["status"], "starting");
        assert_eq!(
            (
                &replies[1]["kind"],
                &replies[1]["status"],
                &replies[1]["data"]
            ),
            (
                &serde_json::json!("wifi.joined"),
                &serde_json::json!("success"),
                &serde_json::json!({"ssid": "New Home", "confirm_within_s": 120, "request_id": "r-7"})
            )
        );
        // On the second connection: after its info.
        let second_info = seen
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, Seen::Publish(_, p) if p.contains("\"type\":\"info\"")))
            .map(|(at, _)| at)
            .nth(1)
            .unwrap();
        let joined_at = seen
            .iter()
            .position(|s| matches!(s, Seen::Publish(_, p) if p.contains("wifi.joined")))
            .unwrap();
        assert!(joined_at > second_info);
        let result = "a change's result, wifi.joined success, request_id r-7";
        for step in [
            "handed to the client at QoS 1",
            "sent as packet",
            "acked by the broker, packet",
        ] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with(&format!("{result}: {step}"))),
                "{step}: {lines:?}"
            );
        }
        for line in &lines {
            assert!(
                !line.contains("New Home") && !line.contains("fake-pass"),
                "{line}"
            );
        }
        let _ = std::fs::remove_file(&socket);
    }

    /// #2683 over #2617: with both switches on, the connection made again at
    /// once on a Wi-Fi `joined` leaves the broker its will too, as the first
    /// one did, so a lamp whose new network fails afterwards still says
    /// offline.
    #[tokio::test]
    async fn a_connection_made_again_on_a_wifi_join_keeps_its_will() {
        let socket = std::env::temp_dir().join(format!("lco-{}-will.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let helper = joining_helper(&socket, Duration::from_millis(300), false);
        let (port, seen) = broker(Broker::StaleFirstLateAck, vec![WIFI_SET]).await;
        wifi_against(
            port,
            &seen,
            &socket,
            Duration::from_secs(1),
            true,
            |seen, _| wifi_replies(seen).len() >= 2,
        )
        .await;
        helper.join().unwrap();
        let seen = seen.lock().unwrap().clone();
        let wills: Vec<_> = seen
            .iter()
            .filter_map(|s| match s {
                Seen::Connect { will, .. } => Some(will.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(wills.len(), 2, "{seen:?}");
        for will in &wills {
            let (topic, message, qos, retain, delay) =
                will.clone().expect("a will on each connection");
            assert_eq!(
                (topic.as_str(), qos, retain, delay),
                ("fd/d-42", 1, false, Some(WILL_DELAY_S))
            );
            assert!(message.contains("\"type\":\"offline\""), "{message}");
        }
        let _ = std::fs::remove_file(&socket);
    }

    /// The reviewer-lead's P2 on #2617, the backoff: a `joined` that comes
    /// while the lamp waits between refused tries cuts the wait short, one
    /// try at once and then the waits from the first again, not the grown
    /// one. And the lamp's going back after it, a failure after an outage,
    /// is said on the link made again after it and acked (Mobile on #1959).
    #[tokio::test]
    async fn p2_a_joined_wifi_in_a_backoff_tries_at_once_and_the_backoff_is_reset() {
        let socket = std::env::temp_dir().join(format!("lco-{}-gated.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        GATE.store(false, std::sync::atomic::Ordering::SeqCst);
        // Waits of 100, 200, 400, 800 ms, then 1.6 s: `joined` at 1.6 s
        // comes in the 1.6 s wait.
        let helper = joining_helper(&socket, Duration::from_millis(1_600), true);
        let (port, seen) = broker(Broker::DropsThenGated, vec![WIFI_SET]).await;
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2_400));
            GATE.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let lines = wifi_against(
            port,
            &seen,
            &socket,
            Duration::from_millis(100),
            false,
            // The broker's count, and the client's line for its ack, which
            // can come after it.
            |seen, lines| {
                wifi_replies(seen).len() >= 3
                    && lines
                        .iter()
                        .any(|line| line.contains("wifi.set failure, request_id r-7: acked"))
            },
        )
        .await;
        opener.join().unwrap();
        let (joined_at, _, online) = helper.join().unwrap();
        assert_eq!(online, "{\"online\":true}\n");
        let connects = connects_at(port);
        let after: Vec<_> = connects
            .iter()
            .filter(|at| **at >= joined_at)
            .map(|at| at.duration_since(joined_at))
            .collect();
        assert!(after.len() >= 3, "{after:?}");
        assert!(after[0] < Duration::from_millis(300), "at once: {after:?}");
        // From the first wait again: 100 ms, not the grown 3.2 s.
        let next = after[1] - after[0];
        assert!(next < Duration::from_millis(600), "reset: {after:?}");
        let replies = wifi_replies(&seen.lock().unwrap());
        assert_eq!(replies.len(), 3, "{replies:?}");
        assert_eq!(replies[1]["kind"], "wifi.joined");
        assert_eq!(
            (&replies[2]["kind"], &replies[2]["status"]),
            (
                &serde_json::json!("wifi.set"),
                &serde_json::json!("failure")
            )
        );
        assert_eq!(replies[2]["data"], serde_json::json!({"request_id": "r-7"}));
        assert!(
            lines.iter().any(|line| line.starts_with(
                "a change's result, wifi.set failure, request_id r-7: acked by the broker"
            )),
            "{lines:?}"
        );
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
            code: None,
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

    /// The guard's line in the tests, and a second.
    const VISIT: &str = "Someone was at your desk at 14:31, for about 2 minutes.";
    const VISIT_TWO: &str = "Someone was at your desk at 15:02, for about 5 minutes.";

    /// A lamp with texts taken or not, and two of guard mode's lines on
    /// `messages/out` taken, as `Real` takes them (`crate::unasked`).
    fn lamp_with_lines(texting: bool) -> Fake {
        let mut lamp = Fake {
            texting,
            ..Fake::default()
        };
        let mut stream = crate::chat::Stream::default();
        for (seq, text) in [(1, VISIT), (2, VISIT_TWO)] {
            let out = lamp_msgs::MessageOut {
                from: "skill:guard-mode".into(),
                text: text.into(),
                unnamed: None,
                named: Vec::new(),
            };
            let at = 1_792_296_000_000 + seq * 1_000;
            let taken = lamp.unasked.take(("skills", "b1", seq), &out, at, |text| {
                stream.unasked(text, at)
            });
            assert_eq!(taken, crate::unasked::Taken::Kept);
        }
        lamp
    }

    /// Runs `lamp` against a broker that `behaves` until `enough` lines are
    /// said, or 5 s: what the broker saw and the lines.
    async fn lines_against(
        behaves: Broker,
        lamp: Fake,
        enough: impl Fn(&[String]) -> bool,
    ) -> (Vec<Seen>, Vec<String>) {
        let (port, seen) = broker(behaves, Vec::new()).await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lines = Arc::new(Mutex::new(Vec::new()));
        let said = lines.clone();
        let task = tokio::spawn(async move {
            let mut lamp = lamp;
            let mut say = |line: &str| said.lock().unwrap().push(line.to_owned());
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !enough(&lines.lock().unwrap()) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        task.await.unwrap();
        let seen = seen.lock().unwrap().clone();
        (seen, Arc::try_unwrap(lines).unwrap().into_inner().unwrap())
    }

    fn chat_events(seen: &[Seen]) -> Vec<(String, serde_json::Value)> {
        publishes(seen)
            .into_iter()
            .filter(|(_, reply)| reply["kind"] == "chat.event")
            .collect()
    }

    /// With `LAMP_MESSAGES=on`, a skill's line on `messages/out` goes up
    /// through the test broker as the backend reads unasked news (#1722):
    /// a `chat.event` with the lamp's id and no run, a `task_news` event
    /// with its own `id` and `time`, the text whole as its `summary` and
    /// `detail.message`. In order; the journal says one line a piece, the
    /// skill and the count, never the text.
    #[tokio::test]
    async fn a_skills_line_goes_up_as_task_news_with_no_run_and_the_journal_has_no_text() {
        let sent = crate::unasked::sent_line("guard-mode", 2);
        let (seen, lines) = lines_against(Broker::Takes, lamp_with_lines(true), |lines| {
            lines.contains(&sent)
        })
        .await;
        let events = chat_events(&seen);
        assert_eq!(events.len(), 2, "{events:?}");
        for ((topic, reply), (text, id)) in
            events.iter().zip([(VISIT, "evt-1"), (VISIT_TWO, "evt-2")])
        {
            assert_eq!(topic, "fd/d-42");
            assert_eq!(reply["kind"], "chat.event");
            assert_eq!(reply["type"], "data");
            assert_eq!(reply["status"], "success");
            assert_eq!(reply["id"], "d-42", "the lamp's own id");
            let data = &reply["data"];
            assert!(data.get("run_id").is_none(), "{data}");
            assert!(data.get("session_id").is_none(), "{data}");
            let event = &data["event"];
            assert_eq!(event["type"], "task_news");
            assert_eq!(event["id"], id);
            let time = event["time"].as_str().unwrap();
            assert!(time.starts_with("2026-") && time.ends_with('Z'), "{time}");
            assert_eq!(event["summary"], text);
            assert_eq!(event["detail"]["message"], text);
            assert!(event.get("runId").is_none(), "{event}");
            // Nothing added: no picture, no name.
            let keys: Vec<_> = event.as_object().unwrap().keys().cloned().collect();
            assert_eq!(keys, ["detail", "id", "summary", "time", "type"]);
        }
        assert!(lines.contains(&crate::unasked::sent_line("guard-mode", 1)));
        assert!(lines.contains(&sent));
        for line in &lines {
            assert!(!line.contains("desk") && !line.contains("14:31"), "{line}");
        }
    }

    /// #2562 (the reviewer's correction read on #2769), at the real session:
    /// the pinned client and its event loop against a broker whose CONNACK
    /// says a receive maximum of 1. With `info`'s PUBACK held back, a named
    /// text handed then would wait in the client past the eyes' word, so it
    /// goes unnamed, summary and detail, with the journal's line once; with
    /// nothing unacknowledged ahead it goes named.
    #[tokio::test]
    async fn a_named_text_behind_an_unacknowledged_publish_goes_unnamed() {
        const NAMED: &str = "Minh came in at 18:10 and was here for about 2 minutes.";
        const UNNAMED: &str =
            "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.";
        for (behaves, named) in [(Broker::Takes, true), (Broker::HoldsFirstAck, false)] {
            let mut lamp = Fake {
                texting: true,
                // After the connection's `info` is handed, within its held
                // PUBACK.
                unasked_from: Some(std::time::Instant::now() + Duration::from_millis(250)),
                ..Fake::default()
            };
            let out = lamp_msgs::MessageOut {
                from: "skill:guard-mode".into(),
                text: NAMED.into(),
                unnamed: Some(UNNAMED.into()),
                named: vec![lamp_msgs::known::person_id("Minh")],
            };
            let mut stream = crate::chat::Stream::default();
            let at = 1_792_296_000_000;
            let taken = lamp.unasked.take(("skills", "b1", 1), &out, at, |text| {
                stream.unasked(text, at)
            });
            assert_eq!(taken, crate::unasked::Taken::Kept);
            let sent = crate::unasked::sent_line("guard-mode", 1);
            let (seen, lines) = lines_against(behaves, lamp, |lines| lines.contains(&sent)).await;
            let events = chat_events(&seen);
            assert_eq!(events.len(), 1, "{events:?}");
            let event = &events[0].1["data"]["event"];
            let want = if named { NAMED } else { UNNAMED };
            assert_eq!(event["summary"], want, "{event}");
            assert_eq!(event["detail"]["message"], want, "{event}");
            let held = lines
                .iter()
                .filter(|line| *line == crate::unasked::UNACKED_AHEAD)
                .count();
            assert_eq!(held, usize::from(!named), "{lines:?}");
            assert!(lines.contains(&sent), "{lines:?}");
        }
    }

    /// Without `LAMP_MESSAGES=on` nothing of a skill's goes up.
    #[tokio::test]
    async fn without_the_switch_no_skills_line_goes_up() {
        // Online, and six of the texts' ticks after.
        let start = std::time::Instant::now();
        let (seen, _) = lines_against(Broker::Takes, lamp_with_lines(false), |lines| {
            lines.iter().any(|line| line.starts_with("online"))
                && start.elapsed() >= Duration::from_millis(300)
        })
        .await;
        assert!(chat_events(&seen).is_empty(), "{seen:?}");
        assert!(
            publishes(&seen)
                .iter()
                .any(|(_, reply)| reply["type"] == "info")
        );
    }

    /// A line the connection takes before its ack goes again on the next,
    /// with the same event `id` and `time`, so the backend pushes it once;
    /// in order.
    #[tokio::test]
    async fn a_skills_line_lost_with_the_connection_goes_again_unchanged() {
        let sent = crate::unasked::sent_line("guard-mode", 2);
        let (seen, _) = lines_against(Broker::DropsOnce, lamp_with_lines(true), |lines| {
            lines.contains(&sent)
        })
        .await;
        let events = chat_events(&seen);
        let ids: Vec<_> = events
            .iter()
            .map(|(_, reply)| {
                let event = &reply["data"]["event"];
                format!("{}@{}", event["id"], event["time"])
            })
            .collect();
        let mut once = ids.clone();
        once.dedup();
        assert_eq!(once.len(), 2, "{ids:?}");
        assert!(
            once[0].contains("evt-1") && once[1].contains("evt-2"),
            "{once:?}"
        );
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

    /// `LAMP_CLOUD_OFFLINE_WILL` (Backend3 on Queue #2602, 21 Oct 19:4x):
    /// with the switch, the connection leaves the broker a will on the lamp's
    /// own channel, at QoS 1, not retained, after `WILL_DELAY_S`: the
    /// identity with `"type": "offline"`. Without it, none (the test below).
    #[tokio::test]
    async fn with_its_switch_the_broker_is_left_a_will_that_says_offline() {
        let (port, seen) = broker(Broker::Takes, vec![]).await;
        let (accounts_tx, accounts) = watch::channel(Some(account(port)));
        let (_identity_tx, me) = watch::channel(identity());
        let lamp = tokio::spawn(async move {
            let mut lamp = Fake {
                will: true,
                ..Fake::default()
            };
            let mut say = |_: &str| {};
            run(accounts, me, &mut lamp, &mut say, Duration::from_millis(50)).await;
        });
        let connected = |seen: &[Seen]| seen.iter().any(|s| matches!(s, Seen::Connect { .. }));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !connected(&seen.lock().unwrap()) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(accounts_tx);
        lamp.await.unwrap();
        let seen = seen.lock().unwrap().clone();
        let Some(Seen::Connect {
            will: Some((topic, message, qos, retain, delay)),
            expiry,
            ..
        }) = seen.first().cloned()
        else {
            panic!("no will: {seen:?}");
        };
        assert_eq!(
            (topic.as_str(), qos, retain, delay),
            ("fd/d-42", 1, false, Some(WILL_DELAY_S))
        );
        // The broker says a will at the earlier of its delay and the
        // session's end (MQTT v5 3.1.3.2.2): with a session expiry of 0 the
        // will goes at once, so the session must outlast the delay.
        // `SESSION_EXPIRY_S > WILL_DELAY_S` holds at build time (below
        // `WILL_DELAY_S`); this is the expiry the connection asks for.
        assert_eq!(expiry, Some(SESSION_EXPIRY_S));
        let said: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(said["type"], "offline");
        assert_eq!(said["id"], "d-42");
        assert_eq!(said["generation"], "lamp-v2");
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
                will: None,
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
        let connects = |seen: &[Seen]| {
            seen.iter()
                .filter(|seen| matches!(seen, Seen::Connect { .. }))
                .count()
        };
        let began = tokio::time::Instant::now();
        // Until the third try, not a second of them (#1806): after waits of
        // 50 and 100 ms, so 150 ms at least. In a tight loop, or with waits
        // that do not double, it comes sooner.
        let lines = against(port, &seen, Duration::from_millis(50), |seen| {
            connects(seen) >= 3
        })
        .await;
        let connects = connects(&seen.lock().unwrap());
        assert!(connects >= 3, "{connects}");
        assert!(
            began.elapsed() >= Duration::from_millis(150),
            "{connects} tries in {:?}",
            began.elapsed()
        );
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
