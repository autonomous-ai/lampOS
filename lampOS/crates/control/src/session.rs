//! One connection's conversation with the door, without the network
//! (docs/control.md): the transport hands each text frame to
//! [`Door::handle`] and sends back what it returns, in order.
//!
//! The order of the checks is the protocol's: the frame reads
//! ([`crate::protocol::parse`]); `hello` before anything but `pair`; the
//! owner's grants; a body lease where the op needs one, and no body move at
//! all while another client holds the body; the client's rate; then the
//! part. What the parts do is behind [`Parts`], so this is tested with a
//! fake and the routing to the lamp's sockets comes as its own piece.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ring::rand::SystemRandom;
use serde_json::{Value, json};

use crate::lease::{Change, Leases, Refusal};
use crate::pairing::{self, Clients, Codes, PairRefused};
use crate::protocol::{
    self, Error, ErrorCode, EventKind, Grant, LeaseWhy, Op, Part, StateKind, VERSION,
};
use crate::rate::{Class, Rates};

/// What the lamp's parts do for the door: each action, each piece of state,
/// and a client's `stop`.
pub trait Parts {
    /// Does `op` for `client`: what the reply adds, or why the part refused.
    fn act(&mut self, client: &str, op: &Op, now_us: u64) -> Result<Value, Error>;
    /// One piece of state, as `get` answers it.
    fn state(&self, kind: StateKind) -> Value;
    /// Ends everything `client` asked for that is still going.
    fn stop(&mut self, client: &str, now_us: u64);
}

/// The parts before actions are routed to them: every action is refused
/// with a why, and state is not known yet.
pub struct Unrouted;

impl Parts for Unrouted {
    fn act(&mut self, _: &str, _: &Op, _: u64) -> Result<Value, Error> {
        Err(Error::new(
            ErrorCode::Refused,
            "not routed to the lamp's parts yet",
        ))
    }
    fn state(&self, _: StateKind) -> Value {
        Value::Null
    }
    fn stop(&mut self, _: &str, _: u64) {}
}

/// What a revoke did: whether the computer was paired (or its revoke still
/// waited for the file), the lease changes to tell, and whether it reached
/// the file. Refused in memory either way. The caller tells the owner a
/// failed `saved`: a restart before a save succeeds brings the key back
/// (the Reviewer-lead's P3 on #3780).
#[must_use]
#[derive(Debug)]
pub struct Revoked {
    pub was: bool,
    pub changes: Vec<Change>,
    pub saved: std::io::Result<()>,
}

/// One connection's own state.
#[derive(Debug)]
pub struct Session {
    nonce: [u8; 32],
    client: Option<String>,
    events: BTreeSet<EventKind>,
    phrases: Vec<String>,
}

impl Session {
    /// The client it said `hello` as, if it has.
    pub fn client(&self) -> Option<&str> {
        self.client.as_deref()
    }

    /// Whether it subscribed to `event`.
    pub fn wants(&self, event: EventKind) -> bool {
        self.events.contains(&event)
    }

    /// The phrases it listens for.
    pub fn phrases(&self) -> &[String] {
        &self.phrases
    }
}

/// What to send for one frame, and whether to close after it.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub send: Vec<Value>,
    pub close: bool,
}

/// The door: pairing, the paired clients, the leases and the rates, shared
/// by every connection.
pub struct Door {
    lamp: Value,
    codes: Codes,
    /// The grants the owner chose with the code out.
    code_grants: BTreeSet<Grant>,
    clients: Clients,
    clients_path: Option<PathBuf>,
    leases: Leases,
    rates: Rates,
    open: BTreeMap<String, usize>,
    random: SystemRandom,
    /// Revokes taken in memory but not yet in the file: written out by the
    /// next save that succeeds.
    unsaved_revokes: BTreeSet<String>,
    /// The SHA-256 of the lamp's own TLS certificate, which every pairing
    /// proof is made over.
    pin: [u8; 32],
}

impl Door {
    /// A door for the lamp `id` named `name`, its clients kept at
    /// `clients_path` (none: in memory only, for tests).
    /// A door for the lamp `id` named `name`, its certificate's SHA-256
    /// `pin`, its clients kept at `clients_path` (none: in memory only).
    pub fn new(
        id: &str,
        name: &str,
        pin: [u8; 32],
        clients: Clients,
        clients_path: Option<PathBuf>,
    ) -> Self {
        Self {
            lamp: json!({"id": id, "name": name, "v": VERSION}),
            codes: Codes::new(),
            code_grants: BTreeSet::new(),
            clients,
            clients_path,
            leases: Leases::new(),
            rates: Rates::new(),
            open: BTreeMap::new(),
            random: SystemRandom::new(),
            unsaved_revokes: BTreeSet::new(),
            pin,
        }
    }

    /// A new connection, and the `welcome` to send it first.
    pub fn open(&mut self) -> (Session, Value) {
        let nonce = pairing::nonce(&self.random);
        let session = Session {
            nonce,
            client: None,
            events: BTreeSet::new(),
            phrases: Vec::new(),
        };
        use base64::Engine as _;
        let welcome = json!({
            "ev": "welcome",
            "nonce": base64::engine::general_purpose::STANDARD.encode(nonce),
            "lamp": self.lamp,
        });
        (session, welcome)
    }

    /// The owner's app asks for a pairing code with `grants`.
    pub fn issue_code(&mut self, grants: BTreeSet<Grant>, now_us: u64) -> String {
        self.code_grants = grants;
        self.codes.issue(&self.random, now_us)
    }

    /// The owner revokes `client`: its leases end, told. Whether it was
    /// paired. The server closes its connections at once; a frame that
    /// beats it is refused.
    pub fn revoke(&mut self, client: &str, now_us: u64) -> Revoked {
        let waiting = self.unsaved_revokes.contains(client);
        // At once in memory, whatever the disk does: the computer is refused
        // at its next frame and its leases and rates end (the Reviewer-lead's
        // P2 on #3780: a revoke that waited for a failing disk did nothing).
        let was = self.clients.revoke(client);
        let mut changes = Vec::new();
        if was {
            self.rates.forget(client);
            changes = self.leases.client_gone(client, LeaseWhy::Revoked, now_us);
            self.unsaved_revokes.insert(client.to_owned());
        }
        // To the file: now, or with the next save that succeeds (a retry,
        // a pairing), so a restart can't bring the key back once the disk
        // takes it (Reviewer-two's P2 on #3736).
        let saved = if was || waiting { self.save() } else { Ok(()) };
        Revoked {
            was: was || waiting,
            changes,
            saved,
        }
    }

    /// Revokes taken but not yet written to the file.
    pub fn unsaved_revokes(&self) -> &BTreeSet<String> {
        &self.unsaved_revokes
    }

    /// A connection closed: when its client's last one goes, its leases end.
    pub fn closed(&mut self, session: &Session, now_us: u64) -> Vec<Change> {
        let Some(client) = &session.client else {
            return Vec::new();
        };
        let left = self.open.get_mut(client).map(|count| {
            *count = count.saturating_sub(1);
            *count
        });
        if left == Some(0) {
            self.open.remove(client);
            return self.leases.client_gone(client, LeaseWhy::Released, now_us);
        }
        Vec::new()
    }

    /// The paired computers.
    pub fn clients(&self) -> &Clients {
        &self.clients
    }

    /// The lamp's own events for every lease: a conversation began or ended,
    /// a hand, privacy, a pause, sleep.
    pub fn leases(&mut self) -> &mut Leases {
        &mut self.leases
    }

    /// One frame of `session`'s.
    pub fn handle(
        &mut self,
        session: &mut Session,
        frame: &str,
        parts: &mut dyn Parts,
        now_us: u64,
        wall_s: u64,
    ) -> Outcome {
        let request = match protocol::parse(frame) {
            Ok(request) => request,
            Err(refused) => return reply_refused(refused.re, refused.error),
        };
        let re = request.id;
        let refuse = |code: ErrorCode, why: String| reply_refused(Some(re), Error::new(code, why));
        let op = request.op;
        // A client revoked since its hello is gone at its next frame.
        if let Some(client) = &session.client
            && self.clients.get(client).is_none()
        {
            let mut outcome = refuse(
                ErrorCode::Unpaired,
                "this computer's pairing was revoked".to_owned(),
            );
            outcome.close = true;
            return outcome;
        }
        match &op {
            Op::Pair { name, key, proof } => {
                return self.pair(session, re, name, key, proof, now_us, wall_s);
            }
            Op::Hello { client, sig } => return self.hello(session, re, client, sig),
            _ => {}
        }
        let Some(client) = session.client.clone() else {
            return refuse(ErrorCode::NoHello, "say hello first".to_owned());
        };
        let grants = self
            .clients
            .get(&client)
            .map(|paired| paired.grants.clone())
            .unwrap_or_default();
        let missing: Vec<String> = op
            .needs()
            .into_iter()
            .filter(|grant| !grants.contains(grant))
            .map(|grant| json!(grant).as_str().unwrap_or_default().to_owned())
            .collect();
        if !missing.is_empty() {
            return refuse(
                ErrorCode::NoGrant,
                format!("not granted: {}", missing.join(", ")),
            );
        }
        match &op {
            Op::Subscribe { events, phrases } => {
                session.events.extend(events.iter().copied());
                if !phrases.is_empty() {
                    session.phrases = phrases.clone();
                }
                return ok(re, json!({"events": session.events}));
            }
            Op::Unsubscribe { events } => {
                for event in events {
                    session.events.remove(event);
                }
                if !session.events.contains(&EventKind::PhraseHeard) {
                    session.phrases.clear();
                }
                return ok(re, json!({"events": session.events}));
            }
            Op::Get { state } => {
                let mut answer = serde_json::Map::new();
                for kind in state {
                    answer.insert(
                        json!(kind).as_str().unwrap_or_default().to_owned(),
                        parts.state(*kind),
                    );
                }
                return ok(re, json!({"state": answer}));
            }
            Op::LeaseTake {
                parts: wanted,
                for_ms,
                ..
            } => {
                return match self.leases.take(&client, wanted, *for_ms, now_us) {
                    Ok(held) => ok(re, json!({"lease": held.lease, "for_ms": for_ms})),
                    Err(refusal) => refused_by_lease(re, refusal),
                };
            }
            Op::LeaseRenew { lease, for_ms } => {
                return match self.leases.renew(&client, lease, *for_ms, now_us) {
                    Ok(held) => ok(
                        re,
                        json!({"lease": held.lease, "for_ms": (held.until_us - now_us).div_ceil(1_000)}),
                    ),
                    Err(refusal) => refused_by_lease(re, refusal),
                };
            }
            Op::LeaseRelease { lease } => {
                return match self.leases.release(&client, lease, now_us) {
                    Ok(()) => ok(re, Value::Null),
                    Err(refusal) => refused_by_lease(re, refusal),
                };
            }
            Op::Stop {} => {
                parts.stop(&client, now_us);
                return ok(re, Value::Null);
            }
            _ => {}
        }
        // A body lease where the op needs one: `joints` names its lease; a
        // move, play or look needs this client to hold the body.
        if let Op::Joints { lease, .. } = &op {
            if let Err(refusal) = self.leases.may_act(&client, lease, Part::Body, now_us) {
                return refused_by_lease(re, refusal);
            }
        } else if op.needs_lease() == Some(Part::Body)
            && !self.leases.holds(&client, Part::Body, now_us)
        {
            return refuse(
                ErrorCode::NoLease,
                "take a lease on the body first".to_owned(),
            );
        }
        // No body move at all while another client holds the body, lease or
        // not: a gesture or a feeling would cut across its moves (the
        // Reviewer-lead's P3 on #3715).
        if op.needs().contains(&Grant::Body)
            && let Some(holder) = self.leases.holder(Part::Body, now_us)
            && holder != client
        {
            return refuse(
                ErrorCode::Busy,
                "another computer holds the body".to_owned(),
            );
        }
        let class = Class::of(&op);
        if let Some(class) = class
            && let Err(retry_ms) = self.rates.check(&client, class, now_us)
        {
            let mut error = Error::new(ErrorCode::Rate, class.says());
            error.retry_ms = Some(retry_ms);
            return reply_refused(Some(re), error);
        }
        match parts.act(&client, &op, now_us) {
            Ok(with) => {
                // Counted only once the part took it.
                if let Some(class) = class {
                    self.rates.count(&client, class, now_us);
                }
                ok(re, with)
            }
            Err(error) => reply_refused(Some(re), error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pair(
        &mut self,
        session: &mut Session,
        re: u64,
        name: &str,
        key: &str,
        proof: &str,
        now_us: u64,
        wall_s: u64,
    ) -> Outcome {
        let refuse = |why: &str| reply_refused(Some(re), Error::new(ErrorCode::Unpaired, why));
        let (Some(key), Some(proof)) =
            (pairing::bytes_of::<32>(key), pairing::bytes_of::<32>(proof))
        else {
            return refuse("key or proof is not 32 bytes");
        };
        // Room first, so the owner's code is never spent on a lamp that
        // can't keep the computer.
        if !self.clients.has_room_for(&key) {
            return refuse("this lamp keeps as many computers as it can");
        }
        match self
            .codes
            .redeem(&self.pin, &key, &session.nonce, &proof, now_us)
        {
            Ok(()) => {}
            Err(PairRefused::NoCode) => {
                return refuse("no pairing code is out: ask for one in the app");
            }
            // A wrong proof closes its connection: one try each, and
            // never another on the same nonce (the Reviewer-lead's P3).
            Err(PairRefused::WrongProof) => {
                let mut outcome = refuse("the proof does not hold for the code out");
                outcome.close = true;
                return outcome;
            }
            Err(PairRefused::TooSoon(wait_us)) => {
                let mut error = Error::new(ErrorCode::Rate, "one pairing try a second");
                error.retry_ms = Some(wait_us.div_ceil(1_000));
                let mut outcome = reply_refused(Some(re), error);
                outcome.close = true;
                return outcome;
            }
            Err(PairRefused::Full) => return refuse("this lamp keeps as many computers as it can"),
        }
        let grants = std::mem::take(&mut self.code_grants);
        let (id, before) = match self.clients.add(name, &key, grants.clone(), wall_s) {
            Ok(added) => added,
            Err(_) => return refuse("this lamp keeps as many computers as it can"),
        };
        if let Err(error) = self.save() {
            // Memory and the file agree again: a re-paired key keeps its
            // old entry (the Reviewer-lead's P3).
            self.clients.undo_add(&id, before);
            return reply_refused(
                Some(re),
                Error::new(
                    ErrorCode::Refused,
                    format!("could not keep the pairing: {error}"),
                ),
            );
        }
        ok(re, json!({"client": id, "grants": grants}))
    }

    fn hello(&mut self, session: &mut Session, re: u64, client: &str, sig: &str) -> Outcome {
        // One hello a connection, so its count of open connections stays
        // right (the Reviewer-lead's P3: a second id kept the first's
        // leases open after it closed).
        if session.client.is_some() {
            return reply_refused(
                Some(re),
                Error::new(ErrorCode::BadRequest, "hello once a connection"),
            );
        }
        let Some(paired) = self.clients.hello(client, &session.nonce, sig) else {
            let mut outcome = reply_refused(
                Some(re),
                Error::new(
                    ErrorCode::Unpaired,
                    "not paired, or the signature does not hold",
                ),
            );
            outcome.close = true;
            return outcome;
        };
        let grants = paired.grants.clone();
        *self.open.entry(client.to_owned()).or_default() += 1;
        session.client = Some(client.to_owned());
        ok(
            re,
            json!({
                "lamp": self.lamp,
                "grants": grants,
                "limits": limits(),
            }),
        )
    }

    /// Writes the clients out; on success, every revoke not yet written is.
    fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.clients_path {
            self.clients.save(path)?;
        }
        self.unsaved_revokes.clear();
        Ok(())
    }
}

/// The rates and caps `hello` tells, so a client keeps inside them.
pub fn limits() -> Value {
    let class = |class: Class| -> Value {
        class
            .windows()
            .iter()
            .map(|(most, within)| json!({"most": most, "within_ms": within / 1_000}))
            .collect()
    };
    json!({
        "joints": class(Class::Joints),
        "motion": class(Class::Motion),
        "light": class(Class::Light),
        "say": class(Class::Say),
        "express": class(Class::Express),
        "hold_max_ms": protocol::HOLD_MAX_MS,
        "take_max_ms": crate::lease::TAKE_MAX_US / 1_000,
        "say_max_chars": protocol::SAY_MAX_CHARS,
    })
}

/// A lease change as the event its holder is told.
pub fn event_of(change: &Change) -> Value {
    protocol::lease_event(&change.lease, change.state, change.why)
}

fn ok(re: u64, with: Value) -> Outcome {
    Outcome {
        send: vec![protocol::ok(re, with)],
        close: false,
    }
}

fn reply_refused(re: Option<u64>, error: Error) -> Outcome {
    Outcome {
        send: vec![protocol::refused(re, &error)],
        close: false,
    }
}

fn refused_by_lease(re: u64, refusal: Refusal) -> Outcome {
    let error = match refusal {
        Refusal::Busy { part, .. } => Error::new(
            ErrorCode::Busy,
            format!(
                "another computer holds the {}",
                json!(part).as_str().unwrap_or_default()
            ),
        ),
        Refusal::NoLease => Error::new(ErrorCode::NoLease, "no such lease of this computer's"),
        Refusal::Lost(why) => Error::new(
            ErrorCode::LeaseLost,
            json!(why).as_str().unwrap_or_default(),
        ),
        Refusal::Suspended => {
            Error::new(ErrorCode::Busy, "suspended while someone talks to the lamp")
        }
        Refusal::BadHold => Error::new(
            ErrorCode::BadRequest,
            format!("for_ms: from 1 to {}", protocol::HOLD_MAX_MS),
        ),
    };
    reply_refused(Some(re), error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use ring::signature::{Ed25519KeyPair, KeyPair as _};

    const S: u64 = 1_000_000;
    const PIN: [u8; 32] = [7; 32];

    #[derive(Default)]
    struct Fake {
        did: Vec<String>,
        stopped: Vec<String>,
        refuse_with: Option<Error>,
    }

    impl Parts for Fake {
        fn act(&mut self, client: &str, op: &Op, _: u64) -> Result<Value, Error> {
            if let Some(error) = self.refuse_with.clone() {
                return Err(error);
            }
            self.did.push(format!(
                "{client} {}",
                serde_json::to_value(op).unwrap()["op"].as_str().unwrap()
            ));
            Ok(Value::Null)
        }
        fn state(&self, kind: StateKind) -> Value {
            json!({"kind": kind})
        }
        fn stop(&mut self, client: &str, _: u64) {
            self.stopped.push(client.to_owned());
        }
    }

    struct Computer {
        keys: Ed25519KeyPair,
    }

    impl Computer {
        fn new(seed: u8) -> Self {
            Self {
                keys: Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap(),
            }
        }
        fn key(&self) -> [u8; 32] {
            self.keys.public_key().as_ref().try_into().unwrap()
        }
        fn nonce_of(welcome: &Value) -> [u8; 32] {
            pairing::bytes_of(welcome["nonce"].as_str().unwrap()).unwrap()
        }
        fn pair_frame(&self, code: &str, welcome: &Value) -> String {
            let proof = pairing::pair_proof(code, &PIN, &self.key(), &Self::nonce_of(welcome));
            json!({"id": 1, "op": "pair", "name": "Dev's laptop", "key": STANDARD.encode(self.key()), "proof": STANDARD.encode(proof)}).to_string()
        }
        fn hello_frame(&self, welcome: &Value) -> String {
            let sig = self
                .keys
                .sign(&[pairing::HELLO_CONTEXT, &Self::nonce_of(welcome)].concat());
            json!({"id": 2, "op": "hello", "client": pairing::client_id(&self.key()), "sig": STANDARD.encode(sig)}).to_string()
        }
    }

    fn reply(outcome: &Outcome) -> &Value {
        assert_eq!(outcome.send.len(), 1, "{outcome:?}");
        &outcome.send[0]
    }

    fn code_of(outcome: &Outcome) -> Option<&str> {
        reply(outcome)["error"]["code"].as_str()
    }

    /// A door with `computer` paired with `grants`, and a session said hello.
    fn paired(computer: &Computer, grants: &[Grant]) -> (Door, Session, Fake) {
        let mut door = Door::new("lamp-75c9", "Lamp", PIN, Clients::default(), None);
        let mut fake = Fake::default();
        let code = door.issue_code(grants.iter().copied().collect(), 0);
        let (mut session, welcome) = door.open();
        let outcome = door.handle(
            &mut session,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        let outcome = door.handle(
            &mut session,
            &computer.hello_frame(&welcome),
            &mut fake,
            S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        (door, session, fake)
    }

    #[test]
    fn a_computer_pairs_with_the_owners_code_then_says_hello_on_each_connection() {
        let computer = Computer::new(1);
        let mut door = Door::new("lamp-75c9", "Lamp", PIN, Clients::default(), None);
        let mut fake = Fake::default();
        let (mut session, welcome) = door.open();
        assert_eq!(welcome["ev"], "welcome");
        assert_eq!(
            welcome["lamp"],
            json!({"id": "lamp-75c9", "name": "Lamp", "v": 1})
        );
        // Before anything: only pair and hello.
        let outcome = door.handle(
            &mut session,
            r#"{"id":9,"op":"gesture","name":"nod"}"#,
            &mut fake,
            S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("no_hello"));
        // No code out yet.
        let outcome = door.handle(
            &mut session,
            &computer.pair_frame("12345678", &welcome),
            &mut fake,
            S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("unpaired"));
        let code = door.issue_code([Grant::Body, Grant::Light].into(), S);
        let outcome = door.handle(
            &mut session,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            2 * S,
            1,
        );
        let paired = reply(&outcome);
        assert_eq!(paired["client"], pairing::client_id(&computer.key()));
        assert_eq!(paired["grants"], json!(["body", "light"]));
        // Paired, but not hello'd on this connection yet.
        let outcome = door.handle(
            &mut session,
            r#"{"id":9,"op":"gesture","name":"nod"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("no_hello"));
        // A hello signed over another connection's nonce closes the connection.
        let (_, other_welcome) = door.open();
        let outcome = door.handle(
            &mut session,
            &computer.hello_frame(&other_welcome),
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("unpaired"));
        assert!(outcome.close);
        let outcome = door.handle(
            &mut session,
            &computer.hello_frame(&welcome),
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&outcome)["grants"], json!(["body", "light"]));
        assert_eq!(
            reply(&outcome)["limits"]["say"][0],
            json!({"most": 6, "within_ms": 60000})
        );
        let outcome = door.handle(
            &mut session,
            r#"{"id":9,"op":"gesture","name":"nod"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&outcome), &json!({"re": 9, "ok": true}));
        assert_eq!(
            fake.did,
            [format!("{} gesture", pairing::client_id(&computer.key()))]
        );
        // A second connection says hello with the same key and no new code.
        let (mut again, welcome) = door.open();
        let outcome = door.handle(
            &mut again,
            &computer.hello_frame(&welcome),
            &mut fake,
            3 * S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true);
        // A stranger never gets in.
        let stranger = Computer::new(2);
        let (mut theirs, welcome) = door.open();
        let outcome = door.handle(
            &mut theirs,
            &stranger.hello_frame(&welcome),
            &mut fake,
            3 * S,
            1,
        );
        assert!(outcome.close);
    }

    /// harness-reviewer's P2 on #3778: an action the part refuses isn't
    /// counted against its rate.
    #[test]
    fn an_action_the_part_refuses_is_not_counted() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Body, Grant::Light]);
        fake.refuse_with = Some(Error::new(ErrorCode::Refused, "not routed"));
        let express = r#"{"id":3,"op":"express","feeling":"happy"}"#;
        let refused = door.handle(&mut session, express, &mut fake, 2 * S, 1);
        assert_eq!(code_of(&refused), Some("refused"));
        fake.refuse_with = None;
        let taken = door.handle(&mut session, express, &mut fake, 3 * S, 1);
        assert_eq!(reply(&taken)["ok"], true, "{:?}", taken.send);
        let again = door.handle(&mut session, express, &mut fake, 4 * S, 1);
        assert_eq!(code_of(&again), Some("rate"));
        assert_eq!(reply(&again)["error"]["why"], "express: 1 a minute");
    }

    #[test]
    fn grants_leases_the_body_holder_and_rates_are_checked_before_the_part() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Body, Grant::Light]);
        let say = door.handle(
            &mut session,
            r#"{"id":3,"op":"say","text":"Hi."}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&say), Some("no_grant"));
        assert_eq!(reply(&say)["error"]["why"], "not granted: speak");
        let subscribe = door.handle(
            &mut session,
            r#"{"id":3,"op":"subscribe","events":["person.arrived"]}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&subscribe), Some("no_grant"));
        let play = door.handle(
            &mut session,
            r#"{"id":4,"op":"play","name":"wave"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&play), Some("no_lease"));
        let take = door.handle(
            &mut session,
            r#"{"id":5,"op":"lease.take","parts":["body"],"for_ms":30000}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&take)["lease"], "l_1");
        let play = door.handle(
            &mut session,
            r#"{"id":6,"op":"play","name":"wave"}"#,
            &mut fake,
            3 * S,
            1,
        );
        assert_eq!(reply(&play)["ok"], true);
        // Motion is one each 2 s.
        let again = door.handle(
            &mut session,
            r#"{"id":7,"op":"gesture","name":"nod"}"#,
            &mut fake,
            4 * S,
            1,
        );
        assert_eq!(code_of(&again), Some("rate"));
        assert_eq!(reply(&again)["error"]["retry_ms"], 1000);
        assert_eq!(reply(&again)["error"]["why"], "motion: 1 each 2 s");
        let joints = door.handle(
            &mut session,
            r#"{"id":8,"op":"joints","lease":"l_9","targets":{"pitch":3},"within_ms":200}"#,
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(code_of(&joints), Some("no_lease"));
        let joints = door.handle(
            &mut session,
            r#"{"id":8,"op":"joints","lease":"l_1","targets":{"pitch":3},"within_ms":200}"#,
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(reply(&joints)["ok"], true);
        // A conversation suspends it; a hand loses it, and says how.
        door.leases()
            .suspend(Some(Part::Body), LeaseWhy::Conversation, 6 * S);
        let joints = door.handle(
            &mut session,
            r#"{"id":9,"op":"joints","lease":"l_1","targets":{"pitch":3},"within_ms":200}"#,
            &mut fake,
            6 * S,
            1,
        );
        assert_eq!(code_of(&joints), Some("busy"));
        door.leases().lose(Some(Part::Body), LeaseWhy::Hand, 7 * S);
        let joints = door.handle(
            &mut session,
            r#"{"id":9,"op":"joints","lease":"l_1","targets":{"pitch":3},"within_ms":200}"#,
            &mut fake,
            7 * S,
            1,
        );
        assert_eq!(code_of(&joints), Some("lease_lost"));
        assert_eq!(reply(&joints)["error"]["why"], "hand");
        // The part's own refusal comes back as it said it.
        fake.refuse_with = Some(Error::new(
            ErrorCode::Refused,
            "centre of gravity 27 mm > 22 mm",
        ));
        let gesture = door.handle(
            &mut session,
            r#"{"id":10,"op":"gesture","name":"nod"}"#,
            &mut fake,
            20 * S,
            1,
        );
        assert_eq!(
            reply(&gesture)["error"],
            json!({"code": "refused", "why": "centre of gravity 27 mm > 22 mm"})
        );
    }

    #[test]
    fn no_body_move_while_another_computer_holds_the_body() {
        let one = Computer::new(1);
        let two = Computer::new(2);
        let (mut door, mut first, mut fake) = paired(&one, &[Grant::Body, Grant::Light]);
        let code = door.issue_code([Grant::Body, Grant::Light].into(), S);
        let (mut second, welcome) = door.open();
        door.handle(
            &mut second,
            &two.pair_frame(&code, &welcome),
            &mut fake,
            2 * S,
            1,
        );
        door.handle(&mut second, &two.hello_frame(&welcome), &mut fake, S, 1);
        door.handle(
            &mut first,
            r#"{"id":5,"op":"lease.take","parts":["body"],"for_ms":30000}"#,
            &mut fake,
            2 * S,
            1,
        );
        let taken = door.handle(
            &mut second,
            r#"{"id":5,"op":"lease.take","parts":["body"],"for_ms":30000}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&taken), Some("busy"));
        for frame in [
            r#"{"id":6,"op":"gesture","name":"nod"}"#,
            r#"{"id":6,"op":"express","feeling":"glad"}"#,
        ] {
            let outcome = door.handle(&mut second, frame, &mut fake, 3 * S, 1);
            assert_eq!(code_of(&outcome), Some("busy"), "{frame}");
        }
        // The light alone isn't the body.
        let light = door.handle(
            &mut second,
            r#"{"id":7,"op":"light","colour":"warm","level":2,"for_ms":1000}"#,
            &mut fake,
            3 * S,
            1,
        );
        assert_eq!(reply(&light)["ok"], true);
        // The holder's last connection closing lets the body go.
        door.closed(&first, 4 * S);
        let gesture = door.handle(
            &mut second,
            r#"{"id":8,"op":"gesture","name":"nod"}"#,
            &mut fake,
            4 * S,
            1,
        );
        assert_eq!(reply(&gesture)["ok"], true);
    }

    #[test]
    fn get_subscribe_stop_and_a_revoke() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Look, Grant::Hear]);
        let id = pairing::client_id(&computer.key());
        let outcome = door.handle(&mut session, r#"{"id":3,"op":"subscribe","events":["person.arrived","phrase.heard"],"phrases":["lights please"]}"#, &mut fake, 2 * S, 1);
        assert_eq!(
            reply(&outcome)["events"],
            json!(["person.arrived", "phrase.heard"])
        );
        assert!(session.wants(EventKind::PersonArrived));
        assert_eq!(session.phrases(), ["lights please"]);
        door.handle(
            &mut session,
            r#"{"id":4,"op":"unsubscribe","events":["phrase.heard"]}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert!(session.phrases().is_empty());
        let outcome = door.handle(
            &mut session,
            r#"{"id":5,"op":"get","state":["presence","lamp"]}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(
            reply(&outcome)["state"],
            json!({"presence": {"kind": "presence"}, "lamp": {"kind": "lamp"}})
        );
        let outcome = door.handle(&mut session, r#"{"id":6,"op":"stop"}"#, &mut fake, 2 * S, 1);
        assert_eq!(reply(&outcome)["ok"], true);
        assert_eq!(fake.stopped, std::slice::from_ref(&id));
        let revoked = door.revoke(&id, 3 * S);
        assert!(revoked.was && revoked.saved.is_ok());
        let outcome = door.handle(
            &mut session,
            r#"{"id":7,"op":"get","state":["lamp"]}"#,
            &mut fake,
            3 * S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("unpaired"));
        assert!(outcome.close);
    }

    #[test]
    fn a_wrong_proof_or_a_try_too_soon_closes_and_a_second_hello_is_refused() {
        let computer = Computer::new(1);
        let mut door = Door::new("lamp-75c9", "Lamp", PIN, Clients::default(), None);
        let mut fake = Fake::default();
        let code = door.issue_code([Grant::Body].into(), 0);
        let (mut session, welcome) = door.open();
        let outcome = door.handle(
            &mut session,
            &computer.pair_frame("00000000", &welcome),
            &mut fake,
            S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("unpaired"));
        assert!(outcome.close, "a wrong proof closes");
        let (mut again, welcome) = door.open();
        let outcome = door.handle(
            &mut again,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            S + 1,
            1,
        );
        assert_eq!(code_of(&outcome), Some("rate"));
        assert!(outcome.close);
        // A proof made for another lamp's certificate, through a host in the
        // middle, never pairs here.
        let (mut middle, welcome) = door.open();
        let nonce = Computer::nonce_of(&welcome);
        let proof = pairing::pair_proof(&code, &[1; 32], &computer.key(), &nonce);
        let frame = json!({"id": 1, "op": "pair", "name": "x", "key": STANDARD.encode(computer.key()), "proof": STANDARD.encode(proof)}).to_string();
        assert!(door.handle(&mut middle, &frame, &mut fake, 3 * S, 1).close);
        // The owner's code still stands for the real computer.
        let (mut real, welcome) = door.open();
        let outcome = door.handle(
            &mut real,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        let outcome = door.handle(
            &mut real,
            &computer.hello_frame(&welcome),
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true);
        let outcome = door.handle(
            &mut real,
            &computer.hello_frame(&welcome),
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("bad_request"));
        // Its one connection closing ends its count: no lease left behind.
        door.handle(
            &mut real,
            r#"{"id":3,"op":"lease.take","parts":["body"],"for_ms":30000}"#,
            &mut fake,
            6 * S,
            1,
        );
        door.closed(&real, 7 * S);
        assert_eq!(door.leases().holder(Part::Body, 7 * S), None);
    }

    #[test]
    fn a_revoke_takes_effect_at_once_and_waits_for_the_file_when_its_save_fails() {
        let directory =
            std::env::temp_dir().join(format!("lamp-control-revoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clients.json");
        let computer = Computer::new(1);
        let mut door = Door::new(
            "lamp-75c9",
            "Lamp",
            PIN,
            Clients::default(),
            Some(path.clone()),
        );
        let mut fake = Fake::default();
        let code = door.issue_code([Grant::Body].into(), 0);
        let (mut session, welcome) = door.open();
        let outcome = door.handle(
            &mut session,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            S,
            1,
        );
        let id = reply(&outcome)["client"].as_str().unwrap().to_owned();
        door.handle(
            &mut session,
            &computer.hello_frame(&welcome),
            &mut fake,
            S,
            1,
        );
        door.handle(
            &mut session,
            r#"{"id":3,"op":"lease.take","parts":["body"],"for_ms":30000}"#,
            &mut fake,
            S,
            1,
        );
        // The save is blocked: the revoke still takes effect at once (the
        // Reviewer-lead's P2 on #3780): its lease ends, told, and its next
        // frame is refused. Only the file is behind, and the revoke waits.
        std::fs::create_dir_all(directory.join("clients.json.new")).unwrap();
        let revoked = door.revoke(&id, 2 * S);
        assert!(revoked.was);
        assert!(revoked.saved.is_err());
        assert_eq!(revoked.changes.len(), 1);
        assert!(door.clients().get(&id).is_none());
        assert!(door.unsaved_revokes().contains(&id));
        let outcome = door.handle(
            &mut session,
            r#"{"id":4,"op":"get","state":["lamp"]}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&outcome), Some("unpaired"));
        assert!(outcome.close);
        assert!(
            Clients::load(&path).unwrap().get(&id).is_some(),
            "the file is behind"
        );
        // Unblocked, a retry writes it out, and a restart keeps it gone.
        std::fs::remove_dir_all(directory.join("clients.json.new")).unwrap();
        let retried = door.revoke(&id, 3 * S);
        assert!(retried.was && retried.saved.is_ok());
        assert!(door.unsaved_revokes().is_empty());
        assert!(Clients::load(&path).unwrap().get(&id).is_none());
        // Any later save writes a waiting revoke out too: another
        // computer's pairing.
        let code = door.issue_code([Grant::Body].into(), 6 * S);
        let (mut again, welcome) = door.open();
        door.handle(
            &mut again,
            &computer.pair_frame(&code, &welcome),
            &mut fake,
            6 * S,
            1,
        );
        let id = door.clients().clients[0].id.clone();
        std::fs::create_dir_all(directory.join("clients.json.new")).unwrap();
        assert!(door.revoke(&id, 7 * S).saved.is_err());
        std::fs::remove_dir_all(directory.join("clients.json.new")).unwrap();
        let other = Computer::new(2);
        let code = door.issue_code([Grant::Body].into(), 8 * S);
        let (mut theirs, welcome) = door.open();
        let outcome = door.handle(
            &mut theirs,
            &other.pair_frame(&code, &welcome),
            &mut fake,
            8 * S,
            1,
        );
        assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        assert!(door.unsaved_revokes().is_empty());
        let kept = Clients::load(&path).unwrap();
        assert!(kept.get(&id).is_none());
        assert!(kept.get(&pairing::client_id(&other.key())).is_some());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_bad_frame_is_answered_with_its_id_when_read() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Body]);
        let outcome = door.handle(&mut session, "not json", &mut fake, 2 * S, 1);
        assert_eq!(reply(&outcome)["re"], Value::Null);
        assert_eq!(code_of(&outcome), Some("bad_request"));
        let outcome = door.handle(
            &mut session,
            r#"{"id":4,"op":"gesture","name":"idle"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&outcome)["re"], 4);
        assert!(fake.did.is_empty());
    }
}
