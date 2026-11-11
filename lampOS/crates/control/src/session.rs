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

use crate::journal::{self, Journal};
use crate::lease::{Change, Leases, Refusal};
use crate::pairing::{self, Clients, Codes, PairRefused};
use crate::parts::Media;
use crate::protocol::{
    self, Error, ErrorCode, EventKind, Grant, LeaseWhy, Op, Part, StateKind, VERSION,
};
use crate::push::{self, Ask, Pushes, Step};
use crate::rate::{Class, Rates};

/// What the lamp's parts do for the door: each action, each piece of state,
/// and a client's `stop`.
pub trait Parts {
    /// Does `op` for `client`, request `re`: what the reply adds, or why
    /// the part refused. An action that ends later says so with a `done`
    /// event naming `re`.
    fn act(&mut self, client: &str, re: u64, op: &Op, now_us: u64) -> Result<Value, Error>;
    /// One piece of state, as `get` answers it.
    fn state(&self, kind: StateKind) -> Value;
    /// Ends everything `client` asked for that is still going.
    fn stop(&mut self, client: &str, now_us: u64);
    /// Ends `client`'s camera or microphone stream, and nothing else of
    /// it: its pairing no longer grants that one (Thirteen's P2 on #4521).
    fn media_end(&mut self, _client: &str, _media: Media) {}
    /// Whether `client`'s camera or microphone stream that made a frame
    /// still runs: a picture or a block already queued for it goes only then
    /// (Thirteen's P3 on #4521). With a `turn`, only that very stream: a
    /// newer one never carries an ended one's blocks (Second's P1 on #5839).
    fn streaming(&self, _client: &str, _media: Media, _turn: Option<u64>) -> bool {
        false
    }
    /// The turn of `client`'s newest camera or microphone stream, running or
    /// ended by itself, until its `off`: the writer tells an older stream's
    /// late end from that stream's own (`crate::camera::older_end`).
    fn newest_turn(&self, _client: &str, _media: Media) -> Option<u64> {
        None
    }
    /// Does `op` for `client`, as [`Parts::act`], where the reply may wait
    /// on the part: a `turn.end` or a `listen.set` the voice answers. What
    /// to run for it off the door's lock comes back as [`Reply::Later`].
    fn reply(&mut self, client: &str, re: u64, op: &Op, now_us: u64) -> Result<Reply, Error> {
        self.act(client, re, op, now_us).map(Reply::Now)
    }
    /// `client` is gone, its last connection closed or its pairing revoked:
    /// as its `stop`, and what it holds on the lamp (its mute) is let go,
    /// so a computer gone never leaves the lamp deaf.
    fn gone(&mut self, client: &str, now_us: u64) {
        self.stop(client, now_us);
    }
    /// Whether a `gesture`, `move` or feeling's move gets a `done` once
    /// the body says what became of it (#4209), told in hello's `limits`
    /// as `motion_done`.
    fn motion_done(&self) -> bool {
        false
    }
    /// The connection the next `act` or `reply` is for, told just before
    /// it: a part that tells an action's end later sends it there alone.
    fn asking(&mut self, _connection: Option<u64>) {}
}

/// A part's answer: now, or what to run off the door's lock for it.
pub enum Reply {
    Now(Value),
    Later(Pending),
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Now(with) => write!(f, "Now({with})"),
            Self::Later(_) => write!(f, "Later"),
        }
    }
}

/// The part's own wait for its answer (the voice's), run on a thread of its
/// own: what the reply adds, or why the part refused.
pub type Pending = Box<dyn FnOnce() -> Result<Value, Error> + Send>;

/// A reply that comes later, to the asking connection only: the request,
/// its computer and op (for the journal), and the part's wait.
pub struct Later {
    pub re: u64,
    pub client: String,
    pub op: &'static str,
    pub run: Pending,
}

impl Later {
    /// The reply: the part's answer as the protocol says it.
    pub fn reply(self) -> Value {
        match (self.run)() {
            Ok(with) => protocol::ok(self.re, with),
            Err(error) => protocol::refused(Some(self.re), &error),
        }
    }
}

impl std::fmt::Debug for Later {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Later({} {} {})", self.re, self.client, self.op)
    }
}

impl PartialEq for Later {
    fn eq(&self, other: &Self) -> bool {
        (self.re, &self.client, self.op) == (other.re, &other.client, other.op)
    }
}

/// The parts before actions are routed to them: every action is refused
/// with a why, and state is not known yet.
pub struct Unrouted;

impl Parts for Unrouted {
    fn act(&mut self, _: &str, _: u64, _: &Op, _: u64) -> Result<Value, Error> {
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

/// What [`Door::set_grants`] did.
#[must_use]
#[derive(Debug, PartialEq, Eq)]
pub struct GrantsSet {
    /// The grants the computer has now.
    pub grants: BTreeSet<Grant>,
    pub added: Vec<Grant>,
    pub removed: Vec<Grant>,
    /// The streams its pairing no longer grants, to end at once.
    pub media: Vec<Media>,
}

/// Why [`Door::set_grants`] changed nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum SetGrantsRefused {
    NoSuchComputer,
    /// The file would not take it, so the computer keeps the grants it had.
    NotSaved(String),
}

/// One connection's own state.
#[derive(Debug)]
pub struct Session {
    nonce: [u8; 32],
    client: Option<String>,
    events: BTreeSet<EventKind>,
    phrases: Vec<String>,
    /// From beyond the lamp's own network (`--remote on`): never the camera
    /// or the microphone.
    remote: bool,
    /// The server's number for this connection, so an action's later word
    /// (a motion's `done`) goes to this connection alone, never another of
    /// the same computer's (Second's P2 on #5835).
    connection: Option<u64>,
    /// Set for a connection that came by the agent socket: the local agent's
    /// client id. Its `hello` names no key and signs nothing; the socket's
    /// peer check has already said who it is (docs/control.md, "The local
    /// agent").
    local: Option<String>,
    /// Set for a connection that came by the run socket: its `hello` must
    /// hold a one-run token (`crate::runs`), and says nothing else of who it
    /// is.
    run_socket: bool,
}

/// What a computer beyond the lamp's own network is told when it asks for
/// the camera or the microphone, whatever it was granted.
pub const LOCAL_ONLY: &str = "local network only: the camera and the microphone are never given beyond the lamp's own network";

impl Session {
    /// This connection comes from beyond the lamp's own network.
    /// The server's number for this connection.
    pub fn set_connection(&mut self, connection: u64) {
        self.connection = Some(connection);
    }

    pub fn connection(&self) -> Option<u64> {
        self.connection
    }

    pub fn set_remote(&mut self) {
        self.remote = true;
    }

    /// Whether it comes from beyond the lamp's own network.
    pub fn remote(&self) -> bool {
        self.remote
    }

    /// The client it said `hello` as, if it has.
    pub fn client(&self) -> Option<&str> {
        self.client.as_deref()
    }

    /// The events it subscribed to.
    pub fn events(&self) -> &BTreeSet<EventKind> {
        &self.events
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
    /// A request for the manage helper (a pushed skill installed, listed or
    /// removed), asked off the door's lock; its answer goes to the client.
    pub ask: Option<Ask>,
    /// A reply the part gives later (the voice's answer to a `turn.end` or
    /// a `listen.set`), waited for off the door's lock and sent to this
    /// connection only.
    pub later: Option<Later>,
}

/// The door: pairing, the paired clients, the leases and the rates, shared
/// by every connection.
pub struct Door {
    lamp: Value,
    /// The file the owner's language is read from at each welcome and hello
    /// (`language`); none, as in tests, says English.
    language_file: Option<PathBuf>,
    codes: Codes,
    /// The grants the owner chose with the code out.
    code_grants: BTreeSet<Grant>,
    /// A computer re-paired without the camera or the microphone: that
    /// stream is ended once the pairing is answered.
    media_dropped: Vec<(String, Media)>,
    clients: Clients,
    clients_path: Option<PathBuf>,
    leases: Leases,
    rates: Rates,
    open: BTreeMap<String, usize>,
    /// The phrases each client's connections listen for, by connection, so
    /// a client holds at most [`protocol::PHRASES_MAX`] across them all
    /// (Fourteen's P2 on #3868).
    phrases_held: BTreeMap<String, BTreeMap<[u8; 32], usize>>,
    random: SystemRandom,
    /// Revokes taken in memory but not yet in the file: written out by the
    /// next save that succeeds.
    unsaved_revokes: BTreeSet<String>,
    /// The skill runs going now, by their token (`crate::runs`).
    runs: crate::runs::Runs,
    /// The SHA-256 of the lamp's own TLS certificate, which every pairing
    /// proof is made over.
    pin: [u8; 32],
    /// The quiet hours, once wired: the lamp's local time, if known.
    quiet: Option<Quiet>,
    /// Where a pushed skill's package is kept for the manage helper: none,
    /// and skill push is off on this lamp.
    push_folder: Option<PathBuf>,
    pushes: Pushes,
    /// The computers' own clips (`crate::sounds`): none, and `sound` plays
    /// the hum only.
    sounds: Option<crate::sounds::Sounds>,
    /// The computers' own notes, kept under the state folder's `notes`.
    notes: Option<crate::notes::Notes>,
    /// Each computer's pairing generation, raised at its every pair,
    /// re-pair and revoke from one counter, so an install asked under one
    /// pairing knows when it's no longer the same (the same key gets the
    /// same id back: Fourteen's and Reviewer-two's P2 on #3910).
    revs: BTreeMap<String, u64>,
    last_rev: u64,
    /// Computers with a `skill.list` or `skill.remove` still with the
    /// helper: one each, and [`ASKING_MOST`] in all.
    asking: BTreeSet<String>,
    /// What the helper must be told of pairings, in order: drained by the
    /// server into one queue, so a later change is never served first.
    helper_changes: Vec<Value>,
    /// One line for every command and its answer ([`crate::journal`]).
    journal: Journal,
}

/// The most `skill.list` and `skill.remove` with the helper at once, all
/// computers together (Reviewer-two's P2 on #3910: each is a connection
/// to root's helper).
pub const ASKING_MOST: usize = 4;

/// The lamp's quiet hours on the door: off by default (the owner's "no
/// limits for developers", 30 Oct). Set (`--quiet-hours`, a
/// `lamp_local_time::QuietWindow`), `say` and `sound` are refused inside
/// this lamp's window and while the lamp's local time isn't known; never
/// with the window `off`, whatever the clock.
pub struct Quiet {
    /// The lamp's zone, or none when it couldn't be read.
    pub local_time: Option<std::sync::Arc<lamp_local_time::LocalTime>>,
    /// This lamp's window; none when its setting is `off`.
    pub window: Option<lamp_local_time::QuietWindow>,
    /// When each computer's refusal was last journaled (unix s), so the
    /// journal says it once a night.
    journaled: BTreeMap<String, u64>,
}

/// Before this, the clock isn't set (a lamp has no battery clock and boots
/// in the past): 2026-01-01 UTC.
pub const CLOCK_SET_AFTER_S: u64 = 1_767_225_600;

impl Quiet {
    pub fn new(local_time: Option<std::sync::Arc<lamp_local_time::LocalTime>>) -> Self {
        Self {
            local_time,
            window: None,
            journaled: BTreeMap::new(),
        }
    }

    /// This lamp's own window, or `None` for `off` (the default).
    pub fn with_window(mut self, window: Option<lamp_local_time::QuietWindow>) -> Self {
        self.window = window;
        self
    }

    /// Why `client`'s say or sound is refused at `wall_s`, if it is.
    fn refuses(&mut self, client: &str, wall_s: u64) -> Option<String> {
        // Off by this lamp's own setting: never quiet.
        let window = self.window?;
        let why = match &self.local_time {
            Some(local) if wall_s >= CLOCK_SET_AFTER_S => {
                let minute = local.minute_of_day(wall_s.saturating_mul(1_000));
                if !window.contains(minute) {
                    return None;
                }
                format!("quiet hours: {window} local, this lamp's setting")
            }
            _ => "quiet hours: the lamp's local time isn't known".to_owned(),
        };
        let last = self.journaled.get(client).copied();
        if last.is_none_or(|last| wall_s.saturating_sub(last) >= 12 * 3_600) {
            eprintln!("lamp-control: {client}'s say or sound refused: {why}");
            self.journaled.insert(client.to_owned(), wall_s);
        }
        Some(why)
    }
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
            language_file: None,
            codes: Codes::new(),
            code_grants: BTreeSet::new(),
            media_dropped: Vec::new(),
            clients,
            clients_path,
            leases: Leases::new(),
            rates: Rates::new(),
            open: BTreeMap::new(),
            phrases_held: BTreeMap::new(),
            random: SystemRandom::new(),
            unsaved_revokes: BTreeSet::new(),
            runs: crate::runs::Runs::default(),
            pin,
            quiet: None,
            push_folder: None,
            pushes: Pushes::default(),
            sounds: None,
            notes: None,
            revs: BTreeMap::new(),
            last_rev: 0,
            asking: BTreeSet::new(),
            helper_changes: Vec::new(),
            journal: Journal::default(),
        }
    }

    /// The door's lines handed to `journal`, not stderr (tests).
    pub fn with_journal(mut self, journal: Journal) -> Self {
        self.journal = journal;
        self
    }

    /// `client` by its id and paired name, as a line names it.
    fn who(&self, client: &str) -> String {
        journal::who(
            client,
            self.clients.get(client).map(|paired| paired.name.as_str()),
        )
    }

    /// A lease the lamp took back, or suspended, or held again
    /// (docs/control.md, "The journal").
    pub fn journal_lease(&mut self, change: &Change, now_us: u64) {
        let state = json!(change.state);
        let why = change
            .why
            .map(|why| format!(" ({})", json!(why).as_str().unwrap_or_default()))
            .unwrap_or_default();
        let line = format!(
            "{}: lease {} {}{why}",
            self.who(&change.client),
            change.lease,
            state.as_str().unwrap_or_default()
        );
        self.journal.line(&change.client, now_us, &line);
    }

    /// The door's tick: lines left out past a computer's second are said
    /// ([`Journal::tick`]).
    pub fn journal_tick(&mut self, now_us: u64) {
        self.journal.tick(now_us);
    }

    /// A reply that came later ([`Later`]): its op and how it ended, the
    /// refusal's code, never its why.
    pub fn journal_answer(&mut self, client: &str, op: &str, reply: &Value, now_us: u64) {
        let how = if reply["ok"] == json!(true) {
            "ok".to_owned()
        } else {
            format!(
                "refused: {}",
                reply["error"]["code"].as_str().unwrap_or("unknown")
            )
        };
        let line = format!("{}: {op}: answered {how}", self.who(client));
        self.journal.line(client, now_us, &line);
    }

    /// An event sent to `client`: its kind, never its fields.
    pub fn journal_event(&mut self, client: &str, kind: &str, now_us: u64) {
        let line = format!("{}: event {kind} sent", self.who(client));
        self.journal.line(client, now_us, &line);
    }

    /// The owner's language read from `file` (the product's
    /// `/etc/lampOS/setup/language`) for the welcome's and the hello's
    /// `lamp`, at each: `language` is `en` without it.
    pub fn with_language_file(mut self, file: PathBuf) -> Self {
        self.language_file = Some(file);
        self
    }

    /// The `lamp` object the welcome and the hello carry: id, name, version
    /// and the owner's language (always one of [`crate::language::CODES`]).
    fn lamp_info(&self) -> Value {
        let mut lamp = self.lamp.clone();
        let language = self
            .language_file
            .as_deref()
            .map_or(crate::language::CODES[0], crate::language::read);
        lamp["language"] = json!(language);
        lamp
    }

    /// Every rate lifted but the refused actions' guard and the phrase
    /// changes (`--rates off`, lamp-control's default).
    pub fn with_rates_off(mut self) -> Self {
        self.rates = Rates::off();
        self
    }

    /// The quiet hours held on the door (lamp-control's `serve` always
    /// wires them; a door without, as in tests, holds none).
    pub fn with_quiet_hours(mut self, quiet: Quiet) -> Self {
        self.quiet = Some(quiet);
        self
    }

    /// Skill push on, its packages kept in `folder` for the manage helper.
    pub fn with_push_folder(mut self, folder: PathBuf) -> Self {
        self.push_folder = Some(folder);
        self
    }

    /// The computers' own notes kept under `folder` (the state folder's
    /// `notes`).
    pub fn with_notes_folder(mut self, folder: PathBuf) -> Self {
        self.notes = Some(crate::notes::Notes::new(folder));
        self
    }

    /// The computers' own clips kept under `folder` (the state folder's
    /// `sounds`, which the voice plays from).
    pub fn with_sound_folder(mut self, folder: PathBuf) -> Self {
        self.sounds = Some(crate::sounds::Sounds::new(folder));
        self
    }

    /// The manage helper has answered `client`'s install, asked under
    /// pairing generation `rev` (or failed to): another may begin. If the
    /// pairing changed while the install was with the helper (a revoke, a
    /// re-pair, the same key paired again with other grants), the helper
    /// is told again what the computer may have now, after the install,
    /// whatever order it serves requests in (Fourteen's P2 on #3910).
    pub fn push_installed(&mut self, client: &str, rev: u64) {
        self.pushes.installed(self.push_folder.as_deref());
        if self.rev_of(client) == rev {
            return;
        }
        match self.clients.get(client).map(|paired| paired.grants.clone()) {
            Some(grants) => self.tell_helper_grants(client, &grants),
            None => self.tell_helper_off(client),
        }
    }

    /// The helper has answered `client`'s `skill.list` or `skill.remove`.
    pub fn helper_answered(&mut self, client: &str) {
        self.asking.remove(client);
    }

    /// What the helper must be told of pairings since the last call, in
    /// order.
    pub fn take_helper_changes(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.helper_changes)
    }

    fn rev_of(&self, client: &str) -> u64 {
        self.revs.get(client).copied().unwrap_or(0)
    }

    fn new_rev(&mut self, client: &str) {
        self.last_rev += 1;
        self.revs.insert(client.to_owned(), self.last_rev);
    }

    /// `client`'s pushed skills off (not deleted), with skill push on.
    fn tell_helper_off(&mut self, client: &str) {
        if self.push_folder.is_some() {
            self.helper_changes.push(json!({ "off_pushed_by": client }));
        }
    }

    /// `client`'s pushed skills held to `grants`: one asking a grant
    /// outside them is turned off (Eighth's `grants_pushed_by`).
    fn tell_helper_grants(&mut self, client: &str, grants: &BTreeSet<Grant>) {
        if self.push_folder.is_some() {
            self.helper_changes
                .push(json!({ "grants_pushed_by": { "client": client, "grants": grants } }));
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
            remote: false,
            connection: None,
            local: None,
            run_socket: false,
        };
        use base64::Engine as _;
        let welcome = json!({
            "ev": "welcome",
            "nonce": base64::engine::general_purpose::STANDARD.encode(nonce),
            "lamp": self.lamp_info(),
        });
        (session, welcome)
    }

    /// A connection that came by the agent socket, whose peer the socket
    /// has already checked: its `hello` is taken as the local agent `client`
    /// (a record [`Door::ensure_local`] made), with no key and no signature.
    pub fn open_local(&mut self, client: &str) -> (Session, Value) {
        let (mut session, welcome) = self.open();
        session.local = Some(client.to_owned());
        (session, welcome)
    }

    /// A connection that came by the run socket, whose peer the socket has
    /// already checked: its `hello` is taken only with a live one-run token.
    pub fn open_run(&mut self) -> (Session, Value) {
        let (mut session, welcome) = self.open();
        session.run_socket = true;
        (session, welcome)
    }

    /// A one-run token for run `run` of `skill`, with `grants` (what the
    /// owner's yes named), until `for_ms` from `now_us`: the run, and its
    /// client now known to the door. Another token for the same run replaces
    /// the first, and the connections of that run are told to close
    /// (the caller does it with [`crate::runs::Run::client`]).
    pub fn mint_run(
        &mut self,
        token: &str,
        run: &str,
        skill: &str,
        grants: BTreeSet<Grant>,
        for_ms: Option<u64>,
        now_us: u64,
    ) -> Result<crate::runs::Run, String> {
        let live = self.runs.mint(token, run, skill, grants, for_ms, now_us)?;
        self.clients
            .add_run(&live.client, skill, live.grants.clone(), now_us / 1_000_000);
        Ok(live)
    }

    /// Ends run `run` (and, when `serial` is given, only if it is still that
    /// mint): the run, if it was going. Its client is gone from the door.
    pub fn end_run(&mut self, run: &str, serial: Option<u64>) -> Option<crate::runs::Run> {
        let ended = match serial {
            Some(serial) => self.runs.end_if(run, serial),
            None => self.runs.end(run),
        }?;
        self.clients.end_run(&ended.client);
        Some(ended)
    }

    /// Ends every run whose time is past: the runs, their clients gone.
    pub fn sweep_runs(&mut self, now_us: u64) -> Vec<crate::runs::Run> {
        let ended = self.runs.expire(now_us);
        for run in &ended {
            self.clients.end_run(&run.client);
        }
        ended
    }

    /// The run `token` holds now.
    fn run_of(&self, token: &str, now_us: u64) -> Option<crate::runs::Run> {
        self.runs.of(token, now_us).cloned()
    }

    /// The local agent `user`'s record, made on the first start with the
    /// first grants and kept as the owner's app changes it; its client id.
    pub fn ensure_local(&mut self, user: &str, wall_s: u64) -> std::io::Result<String> {
        let (id, made) = self.clients.ensure_local(user, wall_s);
        // Not kept: the record made in memory is taken out again, so a retry
        // makes and keeps it, and nothing runs on one the file never held;
        // the socket stays off (the caller says why).
        if made && let Err(error) = self.save() {
            self.clients.undo_add(&id, None);
            return Err(error);
        }
        Ok(id)
    }

    /// The owner's app asks for a pairing code with `grants`.
    pub fn issue_code(&mut self, grants: BTreeSet<Grant>, now_us: u64) -> String {
        self.code_grants = grants;
        self.codes.issue(&self.random, now_us)
    }

    /// The owner's app drops the code out, so nobody can pair with it:
    /// whether there was one.
    pub fn cancel_code(&mut self) -> bool {
        self.code_grants.clear();
        self.codes.cancel()
    }

    /// The owner's app takes the lamp back: every lease of `client`, or of
    /// every computer for `None`, is lost as `taken_by_owner_app`, told to
    /// its holder. The computers stay paired.
    pub fn take_back(&mut self, client: Option<&str>, now_us: u64) -> Vec<Change> {
        match client {
            Some(client) => self
                .leases
                .client_gone(client, LeaseWhy::TakenByOwnerApp, now_us),
            None => self.leases.lose(None, LeaseWhy::TakenByOwnerApp, now_us),
        }
    }

    /// The owner's app changes the grants of a paired computer to `grants`,
    /// whole, without a new code (`control.set_grants`): the owner's tap is the
    /// consent, as it is for a code; nothing else ever gives a pairing a grant.
    /// Saved at once (the old grants put back when the file won't take them);
    /// a stream whose grant goes is to end now, and its pushed skills are held
    /// to the new grants, as a re-pair does. The server closes the computer's
    /// connections so it reads its new grants at its next `hello`.
    pub fn set_grants(
        &mut self,
        client: &str,
        grants: BTreeSet<Grant>,
    ) -> Result<GrantsSet, SetGrantsRefused> {
        let Some((before, unknown)) = self.clients.set_grants(client, grants.clone()) else {
            return Err(SetGrantsRefused::NoSuchComputer);
        };
        let added: Vec<Grant> = grants.difference(&before).copied().collect();
        let removed: Vec<Grant> = before.difference(&grants).copied().collect();
        // Clearing grants this build does not know is a change too: it is saved.
        if added.is_empty() && removed.is_empty() && unknown.is_empty() {
            return Ok(GrantsSet {
                grants,
                added,
                removed,
                media: Vec::new(),
            });
        }
        if let Err(error) = self.save() {
            self.clients.restore_grants(client, (before, unknown));
            return Err(SetGrantsRefused::NotSaved(error.to_string()));
        }
        self.new_rev(client);
        self.tell_helper_grants(client, &grants);
        let media: Vec<Media> = [Media::Camera, Media::Microphone]
            .into_iter()
            .filter(|media| removed.contains(&media.grant()))
            .collect();
        Ok(GrantsSet {
            grants,
            added,
            removed,
            media,
        })
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
            self.pushes.forget(client);
            changes = self.leases.client_gone(client, LeaseWhy::Revoked, now_us);
            self.unsaved_revokes.insert(client.to_owned());
            self.new_rev(client);
        }
        // Its pushed skills off (not deleted), whether or not it had any;
        // its clips deleted (the local leader's 3 Nov).
        if was || waiting {
            self.tell_helper_off(client);
            if let Some(sounds) = &mut self.sounds {
                sounds.forget(client);
            }
            if let Some(notes) = &mut self.notes {
                notes.forget(client);
            }
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
        self.hold_phrases(client, session.nonce, 0);
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

    /// Records how many phrases one connection of `client` listens for.
    fn hold_phrases(&mut self, client: &str, connection: [u8; 32], count: usize) {
        let held = self.phrases_held.entry(client.to_owned()).or_default();
        if count == 0 {
            held.remove(&connection);
        } else {
            held.insert(connection, count);
        }
        if held.is_empty() {
            self.phrases_held.remove(client);
        }
    }

    /// The phrases `client`'s other connections listen for.
    fn phrases_elsewhere(&self, client: &str, connection: [u8; 32]) -> usize {
        self.phrases_held.get(client).map_or(0, |held| {
            held.iter()
                .filter(|(other, _)| **other != connection)
                .map(|(_, count)| count)
                .sum()
        })
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
        let started = std::time::Instant::now();
        let before = session.client.clone();
        let (outcome, op, fields, went) = match protocol::parse(frame) {
            Ok(request) => {
                let (op, fields, went) = (
                    journal::op_name(&request.op),
                    self.journal.fields(&request.op),
                    journal::went(&request.op),
                );
                let outcome = self.answer(session, request, parts, now_us, wall_s);
                (outcome, op, fields, went)
            }
            Err(refused) => (
                reply_refused(refused.re, refused.error),
                "a frame it couldn't read",
                String::new(),
                "",
            ),
        };
        // The computer as it is after the command (a hello names it), or as
        // it was before (a revoke forgets it).
        let client = session.client.clone().or(before);
        let client = client.as_deref().unwrap_or(journal::NOBODY);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let head = format!(
            "{}: {op}{fields}: {}",
            self.who(client),
            answered(&outcome, went)
        );
        self.journal.command(client, now_us, &head, ms);
        outcome
    }

    /// One command's answer, `handle`'s own.
    fn answer(
        &mut self,
        session: &mut Session,
        request: protocol::Request,
        parts: &mut dyn Parts,
        now_us: u64,
        wall_s: u64,
    ) -> Outcome {
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
            Op::Pair { .. } if session.local.is_some() || session.run_socket => {
                return reply_refused(
                    Some(re),
                    Error::new(
                        ErrorCode::BadRequest,
                        if session.run_socket {
                            "this socket is not for pairing: it is a skill run's"
                        } else {
                            "this socket is not for pairing: it is the local agent's"
                        },
                    ),
                );
            }
            Op::Pair { name, key, proof } => {
                let outcome = self.pair(session, re, name, key, proof, now_us, wall_s);
                for (client, media) in std::mem::take(&mut self.media_dropped) {
                    parts.media_end(&client, media);
                }
                return outcome;
            }
            Op::Hello { client, sig, token } => {
                return self.hello(
                    session,
                    re,
                    client,
                    (sig, token.as_deref()),
                    now_us,
                    parts.motion_done(),
                );
            }
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
            return refuse(ErrorCode::NoGrant, not_granted(&missing));
        }
        if session.remote
            && op
                .needs()
                .iter()
                .any(|grant| matches!(grant, Grant::Camera | Grant::Microphone))
        {
            return refuse(ErrorCode::Refused, LOCAL_ONLY.to_owned());
        }
        match &op {
            Op::Subscribe { events, phrases } => {
                if !phrases.is_empty() && *phrases != session.phrases {
                    // Across all its connections, and a change at most 6 a
                    // minute (Fourteen's P2 on #3868).
                    if self.phrases_elsewhere(&client, session.nonce) + phrases.len()
                        > protocol::PHRASES_MAX
                    {
                        return refuse(
                            ErrorCode::BadRequest,
                            format!(
                                "phrases: at most {} across this computer's connections",
                                protocol::PHRASES_MAX
                            ),
                        );
                    }
                    if let Err(retry_ms) = self.rates.check(&client, Class::Phrases, now_us) {
                        let mut error = Error::new(ErrorCode::Rate, Class::Phrases.says());
                        error.retry_ms = Some(retry_ms);
                        return reply_refused(Some(re), error);
                    }
                    self.rates.count(&client, Class::Phrases, now_us);
                    session.phrases = phrases.clone();
                    self.hold_phrases(&client, session.nonce, phrases.len());
                }
                session.events.extend(events.iter().copied());
                return ok(re, json!({"events": session.events}));
            }
            Op::Unsubscribe { events } => {
                for event in events {
                    session.events.remove(event);
                }
                if !session.events.contains(&EventKind::PhraseHeard) {
                    session.phrases.clear();
                    self.hold_phrases(&client, session.nonce, 0);
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
                    Err(Refusal::Busy { part, holder }) if holder == client => {
                        let suspended = self.leases.has_suspended(&client, part, now_us);
                        reply_refused(Some(re), own_busy(part, suspended))
                    }
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
            Op::SkillPush { .. } | Op::SkillList {} | Op::SkillRemove { .. }
                if self.push_folder.is_none() =>
            {
                return refuse(ErrorCode::Refused, PUSH_OFF.to_owned());
            }
            Op::SkillPush {
                name,
                sha256,
                size,
                part,
                parts: count,
                data,
            } => {
                // A push counts once, at its first part, whatever its parts.
                if *part == 0 {
                    if let Err(retry_ms) = self.rates.check(&client, Class::Push, now_us) {
                        let mut error = Error::new(ErrorCode::Rate, Class::Push.says());
                        error.retry_ms = Some(retry_ms);
                        return reply_refused(Some(re), error);
                    }
                    self.rates.count(&client, Class::Push, now_us);
                }
                // What the helper holds the skill to: this computer, and its
                // grants as they are now; the helper takes it only within
                // 60 s of `at_unix_ms` on its own clock (Eighth on #3504).
                let at_unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_millis())
                    .unwrap_or_default();
                let about = json!({
                    "client": client,
                    "client_name": self.clients.get(&client).map(|paired| paired.name.clone()),
                    "grants": grants,
                    "at_unix_ms": at_unix_ms,
                });
                let folder = self.push_folder.clone().expect("push on");
                let part = push::Part {
                    name,
                    sha256,
                    size: *size,
                    part: *part,
                    parts: *count,
                    data,
                };
                return match self
                    .pushes
                    .part(&client, &part, now_us, Some((&folder, &about)))
                {
                    Ok(Step::Got(got)) => ok(re, json!({"got": got})),
                    Ok(Step::Whole { sha256 }) => Outcome {
                        ask: Some(Ask {
                            rev: self.rev_of(&client),
                            ..Ask::install(re, &client, &sha256)
                        }),
                        ..Outcome::default()
                    },
                    Err(error) => reply_refused(Some(re), error),
                };
            }
            Op::NotesSet { .. }
            | Op::NotesGet { .. }
            | Op::NotesDelete { .. }
            | Op::NotesList { .. }
                if self.notes.is_none() =>
            {
                return refuse(ErrorCode::Refused, NOTES_OFF.to_owned());
            }
            Op::NotesSet { .. }
            | Op::NotesGet { .. }
            | Op::NotesDelete { .. }
            | Op::NotesList { .. } => {
                // Each is a read or a write of the lamp's disk: held to its
                // rate, and counted once the notes took it.
                let class = Class::of(&op).expect("a notes op has a rate");
                if let Err(retry_ms) = self.rates.check(&client, class, now_us) {
                    let mut error = Error::new(ErrorCode::Rate, class.says());
                    error.retry_ms = Some(retry_ms);
                    return reply_refused(Some(re), error);
                }
                let notes = self.notes.as_ref().expect("notes on");
                // A write is kept off the door's lock (two fsyncs took 11 to
                // 15 ms on lamp-75c9, 10 Nov), its answer coming when it is;
                // a read is from memory.
                let answered = match &op {
                    Op::NotesSet { key, value } => notes.set(&client, key, value),
                    Op::NotesDelete { key } => notes.delete(&client, key),
                    Op::NotesGet { key } => notes.get(&client, key).map(|found| {
                        crate::notes::Step::Now(match found {
                            Some(value) => json!({"key": key, "found": true, "value": value}),
                            None => json!({"key": key, "found": false}),
                        })
                    }),
                    Op::NotesList { prefix } => {
                        notes.list(&client, prefix).map(crate::notes::Step::Now)
                    }
                    _ => unreachable!("a notes op"),
                };
                // A write counts whether or not it took: a failed write
                // still cost the disk a try (nine's P3 on banana/store).
                // A busy refusal cost the disk nothing: not counted.
                let busy = matches!(&answered, Err(error) if error.code == ErrorCode::Busy);
                if answered.is_ok() || (class == Class::NotesWrite && !busy) {
                    self.rates.count(&client, class, now_us);
                }
                return match answered {
                    Ok(crate::notes::Step::Now(with)) => ok(re, with),
                    Ok(crate::notes::Step::Later(run)) => Outcome {
                        later: Some(Later {
                            re,
                            client,
                            op: journal::op_name(&op),
                            run,
                        }),
                        ..Outcome::default()
                    },
                    Err(error) => reply_refused(Some(re), error),
                };
            }
            Op::SoundUpload { .. } | Op::SoundList {} | Op::SoundRemove { .. }
                if self.sounds.is_none() =>
            {
                return refuse(ErrorCode::Refused, SOUNDS_OFF.to_owned());
            }
            Op::SoundUpload {
                name,
                sha256,
                size,
                part,
                parts: count,
                data,
            } => {
                let part = crate::sounds::Part {
                    name,
                    sha256,
                    size: *size,
                    part: *part,
                    parts: *count,
                    data,
                };
                let sounds = self.sounds.as_mut().expect("sounds on");
                return match sounds.part(&client, &part, now_us) {
                    Ok(crate::sounds::Step::Got(got)) => ok(re, json!({"got": got})),
                    Ok(crate::sounds::Step::Kept(sound)) => ok(re, json!({"sound": sound})),
                    Err(error) => reply_refused(Some(re), error),
                };
            }
            Op::SoundList {} => {
                let sounds = self.sounds.as_ref().expect("sounds on");
                return ok(re, json!({"sounds": sounds.list(&client)}));
            }
            Op::SoundRemove { name } => {
                let sounds = self.sounds.as_ref().expect("sounds on");
                return match sounds.remove(&client, name) {
                    Ok(()) => ok(re, json!({"removed": name})),
                    Err(error) => reply_refused(Some(re), error),
                };
            }
            Op::SkillList {} | Op::SkillRemove { .. } => {
                // Bounded before the helper is asked: one at a time for each
                // computer, a few for the lamp, and a rate (Reviewer-two's
                // and Fourteen's P2 on #3910).
                if self.asking.contains(&client) {
                    return refuse(
                        ErrorCode::Busy,
                        "this computer's last skill request is still with the lamp's skill helper"
                            .to_owned(),
                    );
                }
                if self.asking.len() >= ASKING_MOST {
                    return refuse(
                        ErrorCode::Busy,
                        "the lamp's skill helper is busy: try again".to_owned(),
                    );
                }
                if let Err(retry_ms) = self.rates.check(&client, Class::Manage, now_us) {
                    let mut error = Error::new(ErrorCode::Rate, Class::Manage.says());
                    error.retry_ms = Some(retry_ms);
                    return reply_refused(Some(re), error);
                }
                self.rates.count(&client, Class::Manage, now_us);
                self.asking.insert(client.clone());
                let ask = match &op {
                    Op::SkillRemove { name } => Ask::remove(re, &client, name),
                    _ => Ask::list(re, &client),
                };
                return Outcome {
                    ask: Some(ask),
                    ..Outcome::default()
                };
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
            // A lease a conversation suspended is still this client's: say
            // so, so a program waits for the `held` event instead of taking
            // a second lease (the lead's 4-lamp show, 10 Nov: `no_lease` on
            // about 1 turn in 6 while `Lease.state` still read held).
            if self.leases.has_suspended(&client, Part::Body, now_us) {
                return reply_refused(Some(re), own_busy(Part::Body, true));
            }
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
        // The quiet hours, before the rate, so a refusal isn't counted. A
        // turn meets them with or without a prompt: it opens the lamp to
        // talk.
        if matches!(op, Op::Say { .. } | Op::Sound { .. } | Op::TurnStart { .. })
            && let Some(why) = self
                .quiet
                .as_mut()
                .and_then(|quiet| quiet.refuses(&client, wall_s))
        {
            return refuse(ErrorCode::QuietHours, why);
        }
        let class = Class::of(&op);
        if let Some(class) = class
            && let Err(retry_ms) = self.rates.check(&client, class, now_us)
        {
            let mut error = Error::new(ErrorCode::Rate, class.says());
            error.retry_ms = Some(retry_ms);
            return reply_refused(Some(re), error);
        }
        parts.asking(session.connection);
        match parts.reply(&client, re, &op, now_us) {
            Ok(reply) => {
                // Counted only once the part took it.
                if let Some(class) = class {
                    self.rates.count(&client, class, now_us);
                }
                match reply {
                    Reply::Now(with) => ok(re, with),
                    Reply::Later(run) => Outcome {
                        later: Some(Later {
                            re,
                            client,
                            op: journal::op_name(&op),
                            run,
                        }),
                        ..Outcome::default()
                    },
                }
            }
            Err(error) => {
                let mut outcome = reply_refused(Some(re), error);
                // Past the refusals' bound the connection closes.
                outcome.close = self.rates.take(&client, Class::Refused, now_us).is_err();
                outcome
            }
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
        // A new pairing generation; and a key paired before, now with
        // other grants, has its pushed skills held to the new ones.
        self.new_rev(&id);
        if before
            .as_ref()
            .is_some_and(|before| before.grants != grants)
        {
            self.tell_helper_grants(&id, &grants);
        }
        // Re-paired without the camera or the microphone: a stream it had
        // going ends now, not when it next asks (Thirteen's P2 on #4521).
        for media in [Media::Camera, Media::Microphone] {
            if before
                .as_ref()
                .is_some_and(|before| before.grants.contains(&media.grant()))
                && !grants.contains(&media.grant())
            {
                self.media_dropped.push((id.clone(), media));
            }
        }
        ok(re, json!({"client": id, "grants": grants}))
    }

    fn hello(
        &mut self,
        session: &mut Session,
        re: u64,
        client: &str,
        proof: (&str, Option<&str>),
        now_us: u64,
        motion_done: bool,
    ) -> Outcome {
        let (sig, token) = proof;
        // One hello a connection, so its count of open connections stays
        // right (the Reviewer-lead's P3: a second id kept the first's
        // leases open after it closed).
        if session.client.is_some() {
            return reply_refused(
                Some(re),
                Error::new(ErrorCode::BadRequest, "hello once a connection"),
            );
        }
        // By the run socket: a live one-run token and nothing else; the run's
        // own client and the grants the token names. Any other way in, a
        // token is no part of hello.
        if session.run_socket {
            // The hello an SDK sends here, exactly: the placeholder id and the
            // zero signature beside the token. A real client's id or signature
            // is not this socket's way in and is refused, not ignored.
            if client != crate::runs::HELLO_CLIENT || sig != crate::runs::hello_sig() {
                let mut outcome = reply_refused(
                    Some(re),
                    Error::new(
                        ErrorCode::BadRequest,
                        "on this socket hello is client c_0, a zero sig and the token",
                    ),
                );
                outcome.close = true;
                return outcome;
            }
            let run = token.and_then(|token| self.run_of(token, now_us));
            let Some(run) = run else {
                let mut outcome = reply_refused(
                    Some(re),
                    Error::new(ErrorCode::NoGrant, crate::runs::NO_TOKEN),
                );
                outcome.close = true;
                return outcome;
            };
            *self.open.entry(run.client.clone()).or_default() += 1;
            session.client = Some(run.client.clone());
            let limits = limits(
                self.quiet.as_ref().and_then(|quiet| quiet.window),
                self.rates.lifted(),
            );
            return ok(
                re,
                json!({
                    "lamp": self.lamp,
                    "grants": run.grants,
                    "limits": limits,
                    "run": run.run,
                    "skill": run.skill,
                }),
            );
        }
        if token.is_some() {
            return reply_refused(
                Some(re),
                Error::new(ErrorCode::BadRequest, "token: only on a skill run's socket"),
            );
        }
        // By the agent socket: no key, no signature; the record the socket
        // named, with grants left (the owner may have taken them all away).
        let local = session
            .local
            .clone()
            .and_then(|id| self.clients.get(&id).filter(|c| c.is_local()).cloned());
        let (client, local_grants) = match (&session.local, local) {
            (Some(_), Some(record)) if !record.grants.is_empty() => {
                (record.id.clone(), Some(record.grants.clone()))
            }
            (Some(_), _) => {
                let mut outcome = reply_refused(
                    Some(re),
                    Error::new(
                        ErrorCode::Unpaired,
                        "the owner has not given the local agent anything: give it grants in the app",
                    ),
                );
                outcome.close = true;
                return outcome;
            }
            (None, _) => (client.to_owned(), None),
        };
        let client = client.as_str();
        let paired = match local_grants {
            Some(grants) => Some(grants),
            None => self
                .clients
                .hello(client, &session.nonce, sig)
                .map(|paired| paired.grants.clone()),
        };
        let Some(grants) = paired else {
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
        *self.open.entry(client.to_owned()).or_default() += 1;
        session.client = Some(client.to_owned());
        let mut limits = limits(
            self.quiet.as_ref().and_then(|quiet| quiet.window),
            self.rates.lifted(),
        );
        // A gesture's, a move's and a feeling's move's `done` (#4209): told
        // so an SDK waits for it only where it comes.
        if motion_done {
            limits["motion_done"] = json!(true);
        }
        ok(
            re,
            json!({
                "lamp": self.lamp_info(),
                "grants": grants,
                "limits": limits,
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

/// Why a notes op is refused on a lamp that keeps no computer's notes.
pub const NOTES_OFF: &str = "this lamp keeps no computer's notes";

/// Why a clip op is refused on a lamp that keeps no clips.
pub const SOUNDS_OFF: &str = "this lamp keeps no computer's clips";

/// Why a skill op is refused on a lamp where skill push is off.
pub const PUSH_OFF: &str = "skill push is not on this lamp";

/// The rates and caps `hello` tells, so a client keeps inside them, and
/// this lamp's quiet hours (`"off"` when none). With the rates lifted
/// (`--rates off`) it says `"rates": "off"` and only the windows still held,
/// the refused actions' and the phrase changes', so a client doesn't pace
/// itself to rates the door doesn't hold.
pub fn limits(quiet: Option<lamp_local_time::QuietWindow>, rates_off: bool) -> Value {
    let class = |class: Class| -> Value {
        class
            .windows()
            .iter()
            .map(|(most, within)| json!({"most": most, "within_ms": within / 1_000}))
            .collect()
    };
    if rates_off {
        return json!({
            "rates": "off",
            "refused": class(Class::Refused),
            "phrases": class(Class::Phrases),
            "hold_max_ms": protocol::HOLD_MAX_MS,
            "lease_max_ms": protocol::LEASE_MAX_MS,
            "say_max_bytes": protocol::SAY_MAX_BYTES,
            "sounds": sounds_limits(),
            "notes": notes_limits(),
            "quiet_hours": quiet_hours_of(quiet),
            "ring": ring_limits(),
        });
    }
    json!({
        "joints": class(Class::Joints),
        "motion": class(Class::Motion),
        "light": class(Class::Light),
        "say": class(Class::Say),
        "express": class(Class::Express),
        "refused": class(Class::Refused),
        "phrases": class(Class::Phrases),
        "push": class(Class::Push),
        "manage": class(Class::Manage),
        "hold_max_ms": protocol::HOLD_MAX_MS,
        "lease_max_ms": protocol::LEASE_MAX_MS,
        "say_max_bytes": protocol::SAY_MAX_BYTES,
        "sounds": sounds_limits(),
        "notes": notes_limits(),
        "quiet_hours": quiet_hours_of(quiet),
        "ring": ring_limits(),
    })
}

/// A computer's notes, as hello tells them (`crate::notes`): what a key and
/// a value may be, how much each computer keeps, and the rates of writes and
/// reads.
fn notes_limits() -> Value {
    let class = |class: Class| -> Value {
        class
            .windows()
            .iter()
            .map(|(most, within)| json!({"most": most, "within_ms": within / 1_000}))
            .collect()
    };
    json!({
        "key_max_bytes": crate::notes::KEY_MOST_BYTES,
        "value_max_bytes": crate::notes::VALUE_MOST_BYTES,
        "keys_max": crate::notes::KEYS_MOST,
        "bytes_max": crate::notes::TOTAL_MOST_BYTES,
        "writes": class(Class::NotesWrite),
        "reads": class(Class::NotesRead),
    })
}

/// A computer's clips, as hello tells them (`crate::sounds`): a clip's
/// format and length, how it is sent, and the store each computer has.
fn sounds_limits() -> Value {
    json!({
        "rate_hz": crate::sounds::CLIP_RATE,
        "clip_max_ms": u64::from(crate::sounds::CLIP_MOST_FRAMES) * 1_000
            / u64::from(crate::sounds::CLIP_RATE),
        "clip_max_bytes": crate::sounds::CLIP_MOST_BYTES,
        "part_max_chars": protocol::SKILL_PART_MAX_CHARS,
        "parts_max": protocol::SOUND_PARTS_MAX,
        "clips_max": crate::sounds::CLIPS_MOST,
        "store_max_bytes": crate::sounds::STORE_MOST_BYTES,
    })
}

/// The ring as `light`'s `pixels` see it (docs/control.md, "The ring, pixel
/// by pixel"): how many pixels, in order around the ring from pixel 0, so a
/// computer never assumes the count.
fn ring_limits() -> Value {
    json!({"pixels": lamp_msgs::LightRequest::PIXELS})
}

/// A window as hello tells it: `{"from": "22:00", "to": "07:00"}`, or
/// `"off"`.
fn quiet_hours_of(quiet: Option<lamp_local_time::QuietWindow>) -> Value {
    let hhmm = |minute: u16| format!("{:02}:{:02}", minute / 60, minute % 60);
    match quiet {
        Some(window) => json!({"from": hhmm(window.from), "to": hhmm(window.to)}),
        None => json!("off"),
    }
}

/// Whether the rates are lifted, from `--rates off|default` first, then
/// `LAMP_CONTROL_RATES`, else off: lifted on every lamp, the person's
/// decision (30 Oct 02:1x, "remove the rate"), the refused actions' guard and the phrase
/// changes kept ([`crate::rate::Class::kept_when_off`]). `default` turns
/// the rates back on. A bad value stops lamp-control at its start.
pub fn rates_off_of(flag: Option<&str>, env: Option<&str>) -> Result<bool, String> {
    let (from, value) = match (flag, env) {
        (Some(flag), _) => ("--rates", flag),
        (None, Some(env)) => ("LAMP_CONTROL_RATES", env),
        (None, None) => return Ok(true),
    };
    match value.trim() {
        "off" => Ok(true),
        "default" => Ok(false),
        other => Err(format!("{from} {other:?}: off or default")),
    }
}

/// This lamp's quiet hours from its setting: `--quiet-hours` first, then
/// `LAMP_CONTROL_QUIET`, else off (the owner's "no limits for developers",
/// 30 Oct). A window turns them on; a bad value stops lamp-control at its
/// start.
pub fn quiet_window_of(
    flag: Option<&str>,
    env: Option<&str>,
) -> Result<Option<lamp_local_time::QuietWindow>, String> {
    match (flag, env) {
        (Some(flag), _) => {
            lamp_local_time::QuietWindow::parse(flag).map_err(|e| format!("--quiet-hours {e}"))
        }
        (None, Some(env)) => {
            lamp_local_time::QuietWindow::parse(env).map_err(|e| format!("LAMP_CONTROL_QUIET {e}"))
        }
        (None, None) => Ok(None),
    }
}

/// A lease change as the event its holder is told.
pub fn event_of(change: &Change) -> Value {
    protocol::lease_event(&change.lease, change.state, change.why)
}

/// How a command ended, for its line: `ok` and where it went, handed to the
/// skill helper, or the refusal's code, never its why (it may carry what was
/// asked).
fn answered(outcome: &Outcome, went: &str) -> String {
    let closing = if outcome.close { ", closing" } else { "" };
    if outcome.ask.is_some() {
        return format!("handed to the skill helper{closing}");
    }
    if outcome.later.is_some() {
        return format!("handed to {went}{closing}");
    }
    let Some(reply) = outcome.send.first() else {
        return format!("no answer{closing}");
    };
    if reply["ok"] == json!(true) {
        format!("ok, to {went}{closing}")
    } else {
        let code = reply["error"]["code"].as_str().unwrap_or("unknown");
        format!("refused: {code}{closing}")
    }
}

fn ok(re: u64, with: Value) -> Outcome {
    Outcome {
        send: vec![protocol::ok(re, with)],
        ..Outcome::default()
    }
}

fn reply_refused(re: Option<u64>, error: Error) -> Outcome {
    Outcome {
        send: vec![protocol::refused(re, &error)],
        ..Outcome::default()
    }
}

/// A take refused because this computer holds the part itself: by a lease
/// a conversation suspended (it comes back when they stop), or by one it
/// still has, not "another computer" (the lead's lamp run, 10 Nov).
fn own_busy(part: Part, suspended: bool) -> Error {
    let part = json!(part);
    let part = part.as_str().unwrap_or_default();
    Error::new(
        ErrorCode::Busy,
        if suspended {
            format!(
                "your lease on the {part} is suspended while someone talks to the lamp: it is yours again when they stop"
            )
        } else {
            format!("you already hold the {part}: use the lease you have")
        },
    )
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
            format!("for_ms: from 1 to {}", protocol::LEASE_MAX_MS),
        ),
    };
    reply_refused(Some(re), error)
}

/// What a refusal for a missing grant says. For `camera` and `microphone` it adds the honest way out: the app
/// does not offer these two (App Only, 11 Nov), only a pairing code made on the lamp itself carries them (a
/// developer who needed them, #6257's author, edited the pairing by hand).
pub(crate) const APP_DOES_NOT_GRANT_HINT: &str =
    "the app does not grant it yet: only a pairing code made on the lamp itself carries it";

fn not_granted(missing: &[String]) -> String {
    let said = format!("not granted: {}", missing.join(", "));
    if missing
        .iter()
        .any(|grant| grant == "camera" || grant == "microphone")
    {
        format!("{said} ({APP_DOES_NOT_GRANT_HINT})")
    } else {
        said
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A computer refused its own body take says whose lease holds it
    /// (the lead's lamp run, 10 Nov: "another computer" for its own).
    #[test]
    fn a_take_refused_by_its_own_lease_does_not_blame_another_computer() {
        assert_eq!(
            own_busy(Part::Body, true).why,
            "your lease on the body is suspended while someone talks to the lamp: it is yours again when they stop"
        );
        assert_eq!(
            own_busy(Part::Body, false).why,
            "you already hold the body: use the lease you have"
        );
    }
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use ring::signature::{Ed25519KeyPair, KeyPair as _};

    const S: u64 = 1_000_000;
    const PIN: [u8; 32] = [7; 32];

    #[derive(Default)]
    struct Fake {
        did: Vec<String>,
        stopped: Vec<String>,
        media_ended: Vec<(String, Media)>,
        refuse_with: Option<Error>,
    }

    impl Parts for Fake {
        fn act(&mut self, client: &str, _: u64, op: &Op, _: u64) -> Result<Value, Error> {
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
        fn media_end(&mut self, client: &str, media: Media) {
            self.media_ended.push((client.to_owned(), media));
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

    /// Thirteen's P2 on #4521: a computer re-paired without the camera has
    /// its stream ended at once, and nothing else of it; re-paired keeping
    /// the camera, its stream goes on.
    #[test]
    fn a_re_pairing_that_drops_the_camera_ends_its_stream_at_once() {
        let computer = Computer::new(41);
        let (mut door, _, mut fake) = paired(&computer, &[Grant::Light, Grant::Camera]);
        let id = pairing::client_id(&computer.key());
        // One pairing try a second: each a few seconds after the last.
        let pair_again = |door: &mut Door, fake: &mut Fake, grants: &[Grant], at: u64| {
            let code = door.issue_code(grants.iter().copied().collect(), at);
            let (mut session, welcome) = door.open();
            let outcome = door.handle(
                &mut session,
                &computer.pair_frame(&code, &welcome),
                fake,
                at,
                1,
            );
            assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        };
        pair_again(&mut door, &mut fake, &[Grant::Light, Grant::Camera], 5 * S);
        assert!(
            fake.media_ended.is_empty(),
            "the camera kept, the stream goes on"
        );
        pair_again(&mut door, &mut fake, &[Grant::Light], 10 * S);
        assert_eq!(fake.media_ended, [(id, Media::Camera)]);
        assert!(
            fake.stopped.is_empty(),
            "only the camera, not its light or leases"
        );
    }

    /// The owner's app changes a paired computer's grants, whole, with no new
    /// code: a grant gained works at once for the computer's open connection
    /// (the lamp reads grants at each op), a grant lost is refused at once and
    /// its stream is to end, and the same list changes nothing.
    #[test]
    fn the_owners_app_changes_a_computers_grants_without_a_new_code() {
        let computer = Computer::new(43);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Light]);
        let id = pairing::client_id(&computer.key());
        let snapshot = r#"{"id":9,"op":"camera.snapshot"}"#;
        let refused = door.handle(&mut session, snapshot, &mut fake, 2 * S, 1);
        assert_eq!(code_of(&refused), Some("no_grant"));
        // Camera gained.
        let set = door
            .set_grants(&id, [Grant::Light, Grant::Camera].into())
            .expect("a paired computer");
        assert_eq!(set.added, [Grant::Camera]);
        assert!(set.removed.is_empty() && set.media.is_empty());
        let after = door.handle(&mut session, snapshot, &mut fake, 3 * S, 1);
        assert_ne!(code_of(&after), Some("no_grant"), "{after:?}");
        // The same list: nothing changes.
        let same = door
            .set_grants(&id, [Grant::Light, Grant::Camera].into())
            .expect("a paired computer");
        assert!(same.added.is_empty() && same.removed.is_empty() && same.media.is_empty());
        // Camera lost, microphone gained: the camera's stream is to end.
        let set = door
            .set_grants(&id, [Grant::Light, Grant::Microphone].into())
            .expect("a paired computer");
        assert_eq!(set.added, [Grant::Microphone]);
        assert_eq!(set.removed, [Grant::Camera]);
        assert_eq!(set.media, [Media::Camera]);
        let refused = door.handle(&mut session, snapshot, &mut fake, 4 * S, 1);
        assert_eq!(code_of(&refused), Some("no_grant"));
        // Another computer's id, or none, is refused and nothing changes.
        assert_eq!(
            door.set_grants("c_nobody", [Grant::Body].into()),
            Err(SetGrantsRefused::NoSuchComputer)
        );
        assert_eq!(
            door.clients().clients[0].grants,
            [Grant::Light, Grant::Microphone].into()
        );
    }

    /// Grants this build does not know (a newer lamp-control's, kept in the file) are cleared by a change of grants,
    /// saved even when the known list is the same: "exactly that list", and none comes back with an update.
    #[test]
    fn a_change_of_grants_clears_the_ones_this_build_does_not_know() {
        let newer = pairing::Client {
            id: "c_newer".to_owned(),
            name: "laptop".to_owned(),
            key: "k".to_owned(),
            grants: [Grant::Light].into(),
            unknown_grants: ["teleport".to_owned()].into(),
            paired_at: 1,
        };
        let mut door = Door::new(
            "lamp-75c9",
            "Lamp",
            PIN,
            Clients {
                clients: vec![newer],
                ..Clients::default()
            },
            None,
        );
        // The same known list still clears the unknown one, and says what moved (nothing known).
        let same = door
            .set_grants("c_newer", [Grant::Light].into())
            .expect("a paired computer");
        assert!(same.added.is_empty() && same.removed.is_empty());
        assert!(door.clients().clients[0].unknown_grants.is_empty());
        let _ = door
            .set_grants("c_newer", [Grant::Body].into())
            .expect("a paired computer");
        let kept = &door.clients().clients[0];
        assert_eq!(kept.grants, [Grant::Body].into());
        assert!(kept.unknown_grants.is_empty());
    }

    /// A client with no key at all (the lamp's own local agent) has its grants
    /// changed like any other: `set_grants` reads and writes only the grants,
    /// never the key, and its key stays empty.
    #[test]
    fn a_client_with_an_empty_key_can_have_its_grants_changed() {
        let local = pairing::Client {
            id: "c_local".to_owned(),
            name: "local agent".to_owned(),
            key: String::new(),
            grants: [Grant::Light].into(),
            unknown_grants: BTreeSet::new(),
            paired_at: 1,
        };
        let mut door = Door::new(
            "lamp-75c9",
            "Lamp",
            PIN,
            Clients {
                clients: vec![local],
                ..Clients::default()
            },
            None,
        );
        let set = door
            .set_grants("c_local", [Grant::Light, Grant::Notes].into())
            .expect("a client with an empty key");
        assert_eq!(set.added, [Grant::Notes]);
        let kept = &door.clients().clients[0];
        assert_eq!(kept.key, "");
        assert_eq!(kept.grants, [Grant::Light, Grant::Notes].into());
    }

    /// As the camera's: a computer re-paired without the microphone has its
    /// stream ended at once; one re-paired without both has both ended.
    #[test]
    fn a_re_pairing_that_drops_the_microphone_ends_its_stream_at_once() {
        let computer = Computer::new(42);
        let all = [Grant::Light, Grant::Camera, Grant::Microphone];
        let (mut door, _, mut fake) = paired(&computer, &all);
        let id = pairing::client_id(&computer.key());
        let pair_again = |door: &mut Door, fake: &mut Fake, grants: &[Grant], at: u64| {
            let code = door.issue_code(grants.iter().copied().collect(), at);
            let (mut session, welcome) = door.open();
            let outcome = door.handle(
                &mut session,
                &computer.pair_frame(&code, &welcome),
                fake,
                at,
                1,
            );
            assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
        };
        pair_again(&mut door, &mut fake, &[Grant::Light, Grant::Camera], 5 * S);
        assert_eq!(fake.media_ended, [(id.clone(), Media::Microphone)]);
        pair_again(&mut door, &mut fake, &all, 10 * S);
        assert_eq!(fake.media_ended.len(), 1, "granted again, nothing ends");
        pair_again(&mut door, &mut fake, &[Grant::Light], 15 * S);
        assert_eq!(
            fake.media_ended[1..],
            [(id.clone(), Media::Camera), (id, Media::Microphone)]
        );
        assert!(fake.stopped.is_empty(), "only the streams, not its light");
    }

    /// docs/control.md, "The journal": one line a command, naming the
    /// computer by id and paired name, its op, how it ended and where it
    /// went, and the time it took; never a say's words, a phrase, a proof or
    /// a signature. Leases taken back and events sent are lines too.
    #[test]
    fn every_command_is_one_journal_line_with_nothing_it_carried() {
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let into = lines.clone();
        let mut door = Door::new("lamp-75c9", "Lamp", PIN, Clients::default(), None).with_journal(
            Journal::to(move |line| into.lock().unwrap().push(line.to_owned())),
        );
        let mut fake = Fake::default();
        let computer = Computer::new(3);
        let code = door.issue_code([Grant::Light, Grant::Speak, Grant::Hear].into(), 0);
        let (mut session, welcome) = door.open();
        let pair = computer.pair_frame(&code, &welcome);
        let hello = computer.hello_frame(&welcome);
        let said = "The build on desk two passed.";
        let frames = [
            pair.clone(),
            hello.clone(),
            r#"{"id":3,"op":"light","colour":"blue","level":2,"for_ms":10000}"#.to_owned(),
            json!({"id": 4, "op": "say", "text": said}).to_string(),
            r#"{"id":5,"op":"subscribe","events":["phrase.heard"],"phrases":["good night lamp"]}"#
                .to_owned(),
            r#"{"id":6,"op":"gesture","name":"nod"}"#.to_owned(),
            "not json".to_owned(),
        ];
        for (n, frame) in frames.iter().enumerate() {
            door.handle(&mut session, frame, &mut fake, S + n as u64, 1);
        }
        let client = pairing::client_id(&computer.key());
        // Its paired name, "Dev's laptop", isn't a plain word: the id names it.
        let who = format!("{client} (a name the door doesn't write)");
        door.journal_lease(
            &Change {
                client: client.clone(),
                lease: "l_9".into(),
                state: protocol::LeaseState::Lost,
                why: Some(protocol::LeaseWhy::Hand),
            },
            2 * S,
        );
        door.journal_event(&client, "phrase.heard", 2 * S);
        let lines = lines.lock().unwrap().clone();
        let shape = |line: &str| {
            // The time each took, then nothing after it.
            let (head, ms) = line.rsplit_once(", ").unwrap();
            assert!(
                ms.ends_with(" ms") && ms[..ms.len() - 3].parse::<f64>().is_ok(),
                "{line}"
            );
            head.to_owned()
        };
        assert_eq!(shape(&lines[0]), "lamp-control: -: pair: ok, to the door");
        assert_eq!(
            shape(&lines[1]),
            format!("lamp-control: {who}: hello: ok, to the door")
        );
        assert_eq!(
            shape(&lines[2]),
            format!(
                "lamp-control: {who}: light (blue, level 2, steady, for_ms 10000): ok, to the ring, by light/request"
            )
        );
        assert_eq!(
            shape(&lines[3]),
            format!(
                "lamp-control: {who}: say ({}, exact): ok, to the voice, by say.sock",
                door.journal.said(said)
            )
        );
        assert_eq!(
            shape(&lines[4]),
            format!("lamp-control: {who}: subscribe (phrase.heard): ok, to the door")
        );
        assert_eq!(
            shape(&lines[5]),
            format!("lamp-control: {who}: gesture (nod): refused: no_grant")
        );
        assert_eq!(
            shape(&lines[6]),
            format!("lamp-control: {who}: a frame it couldn't read: refused: bad_request")
        );
        assert_eq!(
            lines[7],
            format!("lamp-control: {who}: lease l_9 lost (hand)")
        );
        assert_eq!(
            lines[8],
            format!("lamp-control: {who}: event phrase.heard sent")
        );
        assert_eq!(lines.len(), 9);
        // Nothing a command carried is in any line.
        let pair: Value = serde_json::from_str(&pair).unwrap();
        let hello: Value = serde_json::from_str(&hello).unwrap();
        let carried = [
            said,
            "desk two",
            "good night lamp",
            pair["proof"].as_str().unwrap(),
            pair["key"].as_str().unwrap(),
            hello["sig"].as_str().unwrap(),
            code.as_str(),
            "Dev's laptop",
        ];
        for line in &lines {
            for secret in carried {
                assert!(!line.contains(secret), "{secret:?} in {line}");
            }
        }
    }

    /// Fourteen's P2 on #3868: a computer holds at most 16 phrases across
    /// all its connections, and changes them at most 6 times a minute; the
    /// same phrases again cost nothing.
    #[test]
    fn phrases_are_capped_across_connections_and_their_changes_are_rated() {
        let computer = Computer::new(1);
        let (mut door, mut first, mut fake) = paired(&computer, &[Grant::Hear]);
        let subscribe = |n: std::ops::Range<usize>| {
            let phrases: Vec<String> = n.map(|n| format!("\"phrase number {n}\"")).collect();
            format!(
                r#"{{"id":3,"op":"subscribe","events":["phrase.heard"],"phrases":[{}]}}"#,
                phrases.join(",")
            )
        };
        let ten = door.handle(&mut first, &subscribe(0..10), &mut fake, 2 * S, 1);
        assert_eq!(reply(&ten)["ok"], true, "{ten:?}");
        // The same ten again: no change, no rate.
        for _ in 0..8 {
            let again = door.handle(&mut first, &subscribe(0..10), &mut fake, 2 * S, 1);
            assert_eq!(reply(&again)["ok"], true, "{again:?}");
        }
        // A second connection of the same computer: 10 + 7 is past 16.
        let (mut second, welcome) = door.open();
        let hello = door.handle(
            &mut second,
            &computer.hello_frame(&welcome),
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&hello)["ok"], true, "{hello:?}");
        let over = door.handle(&mut second, &subscribe(10..17), &mut fake, 2 * S, 1);
        assert_eq!(code_of(&over), Some("bad_request"));
        assert!(
            reply(&over)["error"]["why"]
                .as_str()
                .unwrap()
                .contains("across")
        );
        let six = door.handle(&mut second, &subscribe(10..16), &mut fake, 2 * S, 1);
        assert_eq!(reply(&six)["ok"], true, "{six:?}");
        // The first connection closing gives its ten back.
        door.closed(&first, 3 * S);
        let more = door.handle(&mut second, &subscribe(10..26), &mut fake, 3 * S, 1);
        assert_eq!(reply(&more)["ok"], true, "{more:?}");
        // Changes: 3 so far; 3 more fit the minute, the seventh is rated.
        for n in 0..3 {
            let change = door.handle(&mut second, &subscribe(n..n + 2), &mut fake, 4 * S, 1);
            assert_eq!(reply(&change)["ok"], true, "{change:?}");
        }
        let rated = door.handle(&mut second, &subscribe(5..7), &mut fake, 4 * S, 1);
        assert_eq!(code_of(&rated), Some("rate"));
        assert_eq!(reply(&rated)["error"]["why"], "phrases: 6 changes a minute");
    }

    /// harness-reviewer's P2 on #4360: `--rates off` keeps the phrase
    /// changes at 6 a minute, a privacy bound on `hear`, not pacing.
    #[test]
    fn rates_off_still_rates_phrase_changes() {
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Hear]);
        let mut door = door.with_rates_off();
        let subscribe = |n: usize| {
            format!(
                r#"{{"id":3,"op":"subscribe","events":["phrase.heard"],"phrases":["phrase number {n}"]}}"#
            )
        };
        for n in 0..6 {
            let change = door.handle(&mut session, &subscribe(n), &mut fake, 2 * S, 1);
            assert_eq!(reply(&change)["ok"], true, "{n}: {change:?}");
        }
        let rated = door.handle(&mut session, &subscribe(6), &mut fake, 2 * S, 1);
        assert_eq!(code_of(&rated), Some("rate"));
        assert_eq!(reply(&rated)["error"]["why"], "phrases: 6 changes a minute");
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
            json!({"id": "lamp-75c9", "name": "Lamp", "v": 1, "language": "en"})
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
        // Twenty a minute are taken (the refused one wasn't counted), and
        // the twenty-first in that minute is the rate's.
        for n in 0..20 {
            let taken = door.handle(&mut session, express, &mut fake, 3 * S + n, 1);
            assert_eq!(reply(&taken)["ok"], true, "{n}: {:?}", taken.send);
        }
        let again = door.handle(&mut session, express, &mut fake, 4 * S, 1);
        assert_eq!(code_of(&again), Some("rate"));
        assert_eq!(reply(&again)["error"]["why"], "express: 20 a minute");
        // A minute after the first, one has room again.
        let later = door.handle(&mut session, express, &mut fake, 63 * S, 1);
        assert_eq!(reply(&later)["ok"], true, "{:?}", later.send);
    }

    /// Fourteen's P3 on #3798: refused actions are bounded, 20 a second;
    /// past that the connection closes.
    #[test]
    fn spinning_refused_actions_closes_the_connection() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Light]);
        fake.refuse_with = Some(Error::new(ErrorCode::Refused, "no"));
        let light = r#"{"id":3,"op":"light","colour":"warm","level":3,"for_ms":1000}"#;
        for n in 0..20 {
            let outcome = door.handle(&mut session, light, &mut fake, 2 * S + n, 1);
            assert_eq!(code_of(&outcome), Some("refused"));
            assert!(!outcome.close, "closed at {n}");
        }
        let outcome = door.handle(&mut session, light, &mut fake, 2 * S + 20, 1);
        assert!(outcome.close);
        // A second later the window has room again.
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Light]);
        fake.refuse_with = Some(Error::new(ErrorCode::Refused, "no"));
        for n in 0..20 {
            door.handle(&mut session, light, &mut fake, 2 * S + n, 1);
        }
        let later = door.handle(&mut session, light, &mut fake, 3 * S + 20, 1);
        assert!(!later.close);
    }

    /// The quiet hours on the door (#3917, one definition in #3924): a
    /// computer's `say` and `sound` are refused `quiet_hours` from 22:00 to
    /// 07:00 local, and while the lamp's local time isn't known; the refusal
    /// reaches no part and isn't counted against the rate.
    #[test]
    fn say_and_sound_are_refused_in_the_quiet_hours_and_while_the_time_is_unknown() {
        let computer = Computer::new(1);
        let (door, mut session, mut fake) =
            paired(&computer, &[Grant::Speak, Grant::Light, Grant::Hear]);
        // UTC+7, as the lamps' own; 2026-10-28 at 00:00 UTC is 07:00 there.
        let utc7 = std::sync::Arc::new(lamp_local_time::LocalTime::Offset(7 * 60));
        let mut door = door.with_quiet_hours(
            Quiet::new(Some(utc7)).with_window(Some(lamp_local_time::QuietWindow::DEFAULT)),
        );
        const MIDNIGHT_UTC: u64 = 1_793_145_600;
        let at = |hour_local: u64| MIDNIGHT_UTC + ((hour_local + 24 - 7) % 24) * 3_600;
        macro_rules! send {
            ($door:expr, $frame:expr, $wall_s:expr) => {
                $door.handle(&mut session, $frame, &mut fake, 2 * S, $wall_s)
            };
        }
        let say = r#"{"id":3,"op":"say","text":"Hi."}"#;
        let sound = r#"{"id":4,"op":"sound","name":"hum"}"#;
        // A turn, with a prompt or without, opens the lamp to talk.
        let turn = r#"{"id":6,"op":"turn.start"}"#;
        let prompted = r#"{"id":7,"op":"turn.start","prompt":"Shall we go over the list?"}"#;
        for hour in [22, 23, 0, 3, 6] {
            for frame in [say, sound, turn, prompted] {
                let refused = send!(door, frame, at(hour));
                assert_eq!(
                    code_of(&refused),
                    Some("quiet_hours"),
                    "{hour}: {refused:?}"
                );
                assert_eq!(
                    reply(&refused)["error"]["why"],
                    "quiet hours: 22:00 to 07:00 local, this lamp's setting"
                );
            }
        }
        // Nothing reached a part, and a light at night is the ring's to clamp.
        let light = r#"{"id":5,"op":"light","colour":"warm","level":2,"for_ms":1000}"#;
        assert_eq!(reply(&send!(door, light, at(23)))["ok"], true);
        assert_eq!(fake.did.len(), 1, "{:?}", fake.did);
        assert!(fake.did[0].ends_with(" light"), "{:?}", fake.did);
        // By day it goes through, and the night's refusals weren't counted:
        // six says a minute is still the rate.
        for id in 0..6 {
            let frame = format!(r#"{{"id":{},"op":"say","text":"Hi."}}"#, 10 + id);
            assert_eq!(reply(&send!(door, &frame, at(12)))["ok"], true, "{id}");
        }
        // A clock that isn't set yet, or a zone that couldn't be read: quiet.
        let unset = send!(door, say, 1_000);
        assert_eq!(code_of(&unset), Some("quiet_hours"));
        assert_eq!(
            reply(&unset)["error"]["why"],
            "quiet hours: the lamp's local time isn't known"
        );
        let (door, mut session, mut fake) = paired(&Computer::new(2), &[Grant::Speak]);
        let mut door = door.with_quiet_hours(
            Quiet::new(None).with_window(Some(lamp_local_time::QuietWindow::DEFAULT)),
        );
        let unknown = door.handle(&mut session, say, &mut fake, 2 * S, at(12));
        assert_eq!(code_of(&unknown), Some("quiet_hours"));
        assert!(fake.did.is_empty());
    }

    /// A refusal for `camera` or `microphone` says the app doesn't grant it yet (only a code made on the lamp does); the others keep
    /// their short words.
    #[test]
    fn a_missing_camera_or_microphone_grant_says_the_app_does_not_grant_it() {
        let send = |door: &mut Door, session: &mut Session, fake: &mut Fake, frame: &str| {
            door.handle(session, frame, fake, 2 * S, 1)
        };
        let (mut door, mut session, mut fake) = paired(&Computer::new(1), &[Grant::Light]);
        let refused = send(
            &mut door,
            &mut session,
            &mut fake,
            r#"{"id":3,"op":"camera.snapshot"}"#,
        );
        assert_eq!(code_of(&refused), Some("no_grant"));
        assert_eq!(
            reply(&refused)["error"]["why"],
            "not granted: camera (the app does not grant it yet: only a pairing code made on the lamp itself carries it)"
        );
        let refused = send(
            &mut door,
            &mut session,
            &mut fake,
            r#"{"id":4,"op":"microphone.stream","on":true}"#,
        );
        assert_eq!(
            reply(&refused)["error"]["why"],
            "not granted: microphone (the app does not grant it yet: only a pairing code made on the lamp itself carries it)"
        );
        let refused = send(
            &mut door,
            &mut session,
            &mut fake,
            r#"{"id":5,"op":"say","text":"x"}"#,
        );
        assert_eq!(reply(&refused)["error"]["why"], "not granted: speak");
    }

    /// docs/control.md, "Turns and listening": `turn.start` needs `speak`
    /// and `hear`, `turn.end` `speak` and `listen.set` `hear`; a turn counts
    /// in say's rate; none needs a lease, and another computer's `voice`
    /// lease doesn't stop a turn, as it doesn't stop a say. The journal
    /// names a turn's prompt by its length and tag, never its words.
    #[test]
    fn turns_and_listening_need_their_grants_and_a_turn_counts_as_a_say() {
        let send = |door: &mut Door, session: &mut Session, fake: &mut Fake, frame: &str| {
            door.handle(session, frame, fake, 2 * S, 1)
        };
        let start = r#"{"id":3,"op":"turn.start"}"#;
        let end = r#"{"id":4,"op":"turn.end"}"#;
        let mute = r#"{"id":5,"op":"listen.set","on":false}"#;
        let (mut door, mut session, mut fake) = paired(&Computer::new(1), &[Grant::Speak]);
        let refused = send(&mut door, &mut session, &mut fake, start);
        assert_eq!(code_of(&refused), Some("no_grant"));
        assert_eq!(reply(&refused)["error"]["why"], "not granted: hear");
        assert_eq!(
            code_of(&send(&mut door, &mut session, &mut fake, mute)),
            Some("no_grant")
        );
        assert_eq!(
            reply(&send(&mut door, &mut session, &mut fake, end))["ok"],
            true
        );
        let (mut door, mut session, mut fake) = paired(&Computer::new(2), &[Grant::Hear]);
        let refused = send(&mut door, &mut session, &mut fake, start);
        assert_eq!(reply(&refused)["error"]["why"], "not granted: speak");
        assert_eq!(
            code_of(&send(&mut door, &mut session, &mut fake, end)),
            Some("no_grant")
        );
        assert_eq!(
            reply(&send(&mut door, &mut session, &mut fake, mute))["ok"],
            true
        );
        assert_eq!(fake.did.len(), 1, "{:?}", fake.did);

        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let into = lines.clone();
        let (door, mut session, mut fake) = paired(&Computer::new(3), &[Grant::Speak, Grant::Hear]);
        let mut door = door.with_journal(Journal::to(move |line| {
            into.lock().unwrap().push(line.to_owned())
        }));
        // Another computer holds the voice: a turn goes on, as a say would.
        door.leases()
            .take("c_other", &[Part::Voice], 30_000, 2 * S)
            .unwrap();
        let prompt = "Shall we go over today's list?";
        for id in 0..3 {
            let frame = json!({"id": 10 + id, "op": "turn.start", "prompt": prompt}).to_string();
            assert_eq!(
                reply(&send(&mut door, &mut session, &mut fake, &frame))["ok"],
                true
            );
            let frame = format!(r#"{{"id":{},"op":"say","text":"Hi."}}"#, 20 + id);
            assert_eq!(
                reply(&send(&mut door, &mut session, &mut fake, &frame))["ok"],
                true
            );
        }
        // Six in the minute: say's rate.
        let refused = send(&mut door, &mut session, &mut fake, start);
        assert_eq!(code_of(&refused), Some("rate"), "{refused:?}");
        let lines = lines.lock().unwrap();
        let turn = lines
            .iter()
            .find(|line| line.contains("turn.start ("))
            .expect("a turn's line");
        assert!(turn.contains("30 characters, tag "), "{turn}");
        assert!(turn.contains("ok, to the voice, by say.sock"), "{turn}");
        assert!(
            !lines.iter().any(|line| line.contains("today")),
            "{lines:?}"
        );
    }

    /// A lamp's own quiet hours (`--quiet-hours`): `off` refuses nothing,
    /// not even with the time unknown; a window within a day and one across
    /// midnight refuse inside it only, saying it; hello's limits tell it.
    #[test]
    fn a_lamps_own_quiet_hours_are_held_and_told() {
        use lamp_local_time::QuietWindow;
        let utc7 = std::sync::Arc::new(lamp_local_time::LocalTime::Offset(7 * 60));
        const MIDNIGHT_UTC: u64 = 1_793_145_600;
        let at = |minute_local: u64| MIDNIGHT_UTC + ((minute_local + 1_440 - 420) % 1_440) * 60;
        let say = r#"{"id":3,"op":"say","text":"Hi."}"#;
        let hello_limits = |door: &mut Door, fake: &mut Fake, computer: &Computer| {
            let (mut session, welcome) = door.open();
            let outcome = door.handle(&mut session, &computer.hello_frame(&welcome), fake, S, 1);
            reply(&outcome)["limits"]["quiet_hours"].clone()
        };
        // A window, local minutes and whether each is quiet, and hello's word.
        type Case = (Option<QuietWindow>, &'static [(u64, bool)], Value);
        let cases: [Case; 4] = [
            (None, &[(23 * 60, false), (3 * 60, false)], json!("off")),
            (
                Some(QuietWindow::DEFAULT),
                &[
                    (21 * 60 + 59, false),
                    (22 * 60, true),
                    (6 * 60 + 59, true),
                    (7 * 60, false),
                ],
                json!({"from": "22:00", "to": "07:00"}),
            ),
            (
                QuietWindow::parse("23:30-06:15").unwrap(),
                &[
                    (23 * 60 + 29, false),
                    (23 * 60 + 30, true),
                    (6 * 60 + 14, true),
                    (6 * 60 + 15, false),
                ],
                json!({"from": "23:30", "to": "06:15"}),
            ),
            (
                QuietWindow::parse("13:00-14:00").unwrap(),
                &[
                    (12 * 60 + 59, false),
                    (13 * 60, true),
                    (13 * 60 + 59, true),
                    (14 * 60, false),
                    (23 * 60, false),
                ],
                json!({"from": "13:00", "to": "14:00"}),
            ),
        ];
        for (n, (window, minutes, told)) in cases.into_iter().enumerate() {
            let computer = Computer::new(10 + n as u8);
            let (door, mut session, mut fake) = paired(&computer, &[Grant::Speak]);
            let mut door =
                door.with_quiet_hours(Quiet::new(Some(utc7.clone())).with_window(window));
            assert_eq!(
                hello_limits(&mut door, &mut fake, &computer),
                told,
                "{window:?}"
            );
            for (i, (minute, quiet)) in minutes.iter().enumerate() {
                // Each say a minute apart from the last, inside the rate.
                let outcome = door.handle(
                    &mut session,
                    say,
                    &mut fake,
                    (2 + 61 * i as u64) * S,
                    at(*minute),
                );
                if *quiet {
                    assert_eq!(
                        code_of(&outcome),
                        Some("quiet_hours"),
                        "{window:?} {minute}"
                    );
                    let why = format!(
                        "quiet hours: {} local, this lamp's setting",
                        window.unwrap()
                    );
                    assert_eq!(reply(&outcome)["error"]["why"], json!(why));
                } else {
                    assert_eq!(
                        reply(&outcome)["ok"],
                        true,
                        "{window:?} {minute}: {outcome:?}"
                    );
                }
            }
        }
        // Off holds even while the lamp's time isn't known.
        let (door, mut session, mut fake) = paired(&Computer::new(20), &[Grant::Speak]);
        let mut door = door.with_quiet_hours(Quiet::new(None).with_window(None));
        assert_eq!(
            reply(&door.handle(&mut session, say, &mut fake, 2 * S, 1_000))["ok"],
            true
        );
    }

    /// Quiet hours are off by default (the owner's "no limits for
    /// developers", 30 Oct): a door wired as `serve` wires it with no
    /// setting refuses no say or sound, by night or with the lamp's time
    /// unknown, and hello says `off`, the lease bound and no say length.
    #[test]
    fn quiet_hours_are_off_by_default_and_a_say_at_an_unknown_time_goes_through() {
        let computer = Computer::new(30);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Speak]);
        let window = quiet_window_of(None, None).unwrap();
        let mut door = door.with_quiet_hours(Quiet::new(None).with_window(window));
        let say = r#"{"id":3,"op":"say","text":"Hi."}"#;
        assert_eq!(
            reply(&door.handle(&mut session, say, &mut fake, 2 * S, 1_000))["ok"],
            true
        );
        let sound = r#"{"id":4,"op":"sound","name":"chime"}"#;
        assert_eq!(
            reply(&door.handle(&mut session, sound, &mut fake, 3 * S, 1_000))["ok"],
            true
        );
        // 23:00 at UTC+7 on 2026-10-28, the zone known, no window set: said.
        let utc7 = std::sync::Arc::new(lamp_local_time::LocalTime::Offset(7 * 60));
        let other = Computer::new(31);
        let (door, mut session, mut fake) = paired(&other, &[Grant::Speak]);
        let mut door = door.with_quiet_hours(Quiet::new(Some(utc7)));
        let night = 1_793_145_600 + 16 * 3_600;
        assert_eq!(
            reply(&door.handle(&mut session, say, &mut fake, 2 * S, night))["ok"],
            true
        );
        let (mut session, welcome) = door.open();
        let hello = door.handle(&mut session, &other.hello_frame(&welcome), &mut fake, S, 1);
        let limits = &reply(&hello)["limits"];
        assert_eq!(limits["quiet_hours"], "off");
        assert_eq!(limits["hold_max_ms"], 30_000);
        assert_eq!(limits["lease_max_ms"], 600_000);
        assert_eq!(limits["say_max_bytes"], 900);
        assert!(limits.get("say_max_chars").is_none());
        assert!(limits.get("take_max_ms").is_none());
    }

    /// The welcome and the hello both say the owner's language, read at each
    /// connection: English with no file, the file's code with one, and a
    /// change shows on the next connection with no restart.
    #[test]
    fn welcome_and_hello_say_the_owners_language_read_at_each_connection() {
        let computer = Computer::new(41);
        let (door, _, mut fake) = paired(&computer, &[Grant::Light]);
        let mut door = door;
        fn said(door: &mut Door, computer: &Computer, fake: &mut Fake, at: u64) -> (Value, Value) {
            let (mut session, welcome) = door.open();
            let hello = door.handle(&mut session, &computer.hello_frame(&welcome), fake, S, at);
            (
                welcome["lamp"]["language"].clone(),
                reply(&hello)["lamp"]["language"].clone(),
            )
        }
        assert_eq!(
            said(&mut door, &computer, &mut fake, 1),
            ("en".into(), "en".into()),
            "no file"
        );
        let file =
            std::env::temp_dir().join(format!("lampos-door-language-{}", std::process::id()));
        std::fs::write(&file, "vi\n").unwrap();
        let mut door = door.with_language_file(file.clone());
        assert_eq!(
            said(&mut door, &computer, &mut fake, 2),
            ("vi".into(), "vi".into()),
            "vi"
        );
        std::fs::write(&file, "ko\n").unwrap();
        assert_eq!(
            said(&mut door, &computer, &mut fake, 3),
            ("ko".into(), "ko".into()),
            "changed"
        );
        std::fs::write(&file, "klingon\n").unwrap();
        assert_eq!(
            said(&mut door, &computer, &mut fake, 4),
            ("en".into(), "en".into()),
            "unknown"
        );
        let _ = std::fs::remove_file(&file);
    }

    /// Hello's `hold_max_ms` keeps its meaning, a light's longest `for_ms`
    /// (and a renewal an older client sizes from it), so a client written
    /// before `lease_max_ms` still works; `lease_max_ms` is the longest one
    /// lease take or renewal.
    #[test]
    fn hello_says_a_lights_hold_and_a_leases_and_each_is_taken() {
        for rates_off in [true, false] {
            let computer = Computer::new(40);
            let (door, _, mut fake) = paired(&computer, &[Grant::Light]);
            let mut door = if rates_off {
                door.with_rates_off()
            } else {
                door
            };
            let (mut session, welcome) = door.open();
            let hello = door.handle(
                &mut session,
                &computer.hello_frame(&welcome),
                &mut fake,
                S,
                1,
            );
            let limits = reply(&hello)["limits"].clone();
            let hold = limits["hold_max_ms"].as_u64().unwrap();
            let lease = limits["lease_max_ms"].as_u64().unwrap();
            assert_eq!((hold, lease), (30_000, 600_000), "{rates_off}");
            // An old-style client: a light for `hold_max_ms`, a lease taken
            // and renewed for it.
            let light =
                format!(r#"{{"id":3,"op":"light","colour":"warm","level":3,"for_ms":{hold}}}"#);
            let outcome = door.handle(&mut session, &light, &mut fake, 2 * S, 1);
            assert_eq!(reply(&outcome)["ok"], true, "{outcome:?}");
            let take = format!(r#"{{"id":4,"op":"lease.take","parts":["light"],"for_ms":{hold}}}"#);
            let taken = door.handle(&mut session, &take, &mut fake, 3 * S, 1);
            assert_eq!(reply(&taken)["ok"], true, "{taken:?}");
            let id = reply(&taken)["lease"].as_str().unwrap().to_owned();
            let renew = format!(r#"{{"id":5,"op":"lease.renew","lease":"{id}","for_ms":{hold}}}"#);
            let renewed = door.handle(&mut session, &renew, &mut fake, 4 * S, 1);
            assert_eq!(reply(&renewed)["ok"], true, "{renewed:?}");
            // A new client: the lease for `lease_max_ms`.
            let renew = format!(r#"{{"id":6,"op":"lease.renew","lease":"{id}","for_ms":{lease}}}"#);
            let renewed = door.handle(&mut session, &renew, &mut fake, 5 * S, 1);
            assert_eq!(reply(&renewed)["ok"], true, "{renewed:?}");
            let release = format!(r#"{{"id":7,"op":"lease.release","lease":"{id}"}}"#);
            assert_eq!(
                reply(&door.handle(&mut session, &release, &mut fake, 6 * S, 1))["ok"],
                true
            );
            let take =
                format!(r#"{{"id":8,"op":"lease.take","parts":["light"],"for_ms":{lease}}}"#);
            let taken = door.handle(&mut session, &take, &mut fake, 7 * S, 1);
            assert_eq!(reply(&taken)["ok"], true, "{taken:?}");
        }
    }

    /// `--rates off`: hello says so and holds only the refused actions' and
    /// the phrase changes' windows; each class is lifted at the door, the refused guard still
    /// closes a connection; the setting is read as the quiet hours' is.
    #[test]
    fn rates_off_lifts_the_doors_rates_but_not_the_refused_guard() {
        let computer = Computer::new(30);
        let (door, mut session, mut fake) =
            paired(&computer, &[Grant::Body, Grant::Light, Grant::Speak]);
        let mut door = door.with_rates_off();
        let (mut hello_session, welcome) = door.open();
        let hello = door.handle(
            &mut hello_session,
            &computer.hello_frame(&welcome),
            &mut fake,
            S,
            1,
        );
        let limits = &reply(&hello)["limits"];
        assert_eq!(limits["rates"], "off");
        assert_eq!(
            limits["sounds"],
            json!({
                "rate_hz": 24_000,
                "clip_max_ms": 30_000,
                "clip_max_bytes": 1_444_140,
                "part_max_chars": 49_152,
                "parts_max": 64,
                "clips_max": 64,
                "store_max_bytes": 20_000_000,
            })
        );
        // The ring's pixel count, for `light`'s `pixels`.
        assert_eq!(limits["ring"], json!({"pixels": 32}));
        assert!(
            limits["motion"].is_null() && limits["express"].is_null(),
            "{limits}"
        );
        assert_eq!(limits["refused"][0], json!({"most": 20, "within_ms": 1000}));
        assert_eq!(
            limits["phrases"][0],
            json!({"most": 6, "within_ms": 60_000})
        );
        // Motion, express, light and say at any pace.
        for n in 0..10 {
            for frame in [
                r#"{"id":3,"op":"gesture","name":"nod"}"#,
                r#"{"id":4,"op":"express","feeling":"glad"}"#,
                r#"{"id":5,"op":"light","colour":"warm","level":2,"for_ms":1000}"#,
                r#"{"id":6,"op":"say","text":"Hi."}"#,
            ] {
                let outcome = door.handle(&mut session, frame, &mut fake, 2 * S + n, 1);
                assert_eq!(reply(&outcome)["ok"], true, "{n} {frame}: {outcome:?}");
            }
        }
        // The refused actions' guard still closes the connection.
        fake.refuse_with = Some(Error::new(ErrorCode::Refused, "no"));
        let light = r#"{"id":7,"op":"light","colour":"warm","level":2,"for_ms":1000}"#;
        for n in 0..20 {
            assert!(
                !door
                    .handle(&mut session, light, &mut fake, 3 * S + n, 1)
                    .close,
                "{n}"
            );
        }
        assert!(
            door.handle(&mut session, light, &mut fake, 3 * S + 20, 1)
                .close
        );
        // Off unless `default` turns them back on (the person's decision, 30 Oct).
        assert_eq!(rates_off_of(None, None), Ok(true));
        assert_eq!(rates_off_of(None, Some("default")), Ok(false));
        assert_eq!(rates_off_of(Some("off"), None), Ok(true));
        assert_eq!(rates_off_of(None, Some("off")), Ok(true));
        assert_eq!(rates_off_of(Some("default"), Some("off")), Ok(false));
        assert!(
            rates_off_of(Some("none"), None)
                .unwrap_err()
                .starts_with("--rates ")
        );
        assert!(
            rates_off_of(None, Some(""))
                .unwrap_err()
                .starts_with("LAMP_CONTROL_RATES ")
        );
    }

    /// The setting at start: the flag, else the environment, else off; a
    /// bad value stops lamp-control.
    #[test]
    fn the_quiet_hours_setting_is_read_at_start_and_a_bad_one_refused() {
        use lamp_local_time::QuietWindow;
        assert_eq!(quiet_window_of(None, None), Ok(None));
        assert_eq!(quiet_window_of(None, Some("off")), Ok(None));
        assert_eq!(quiet_window_of(Some("off"), None), Ok(None));
        assert_eq!(
            quiet_window_of(Some("23:00-06:00"), Some("off")),
            Ok(Some(QuietWindow {
                from: 23 * 60,
                to: 6 * 60
            }))
        );
        assert!(
            quiet_window_of(Some("22-07"), None)
                .unwrap_err()
                .starts_with("--quiet-hours ")
        );
        assert!(
            quiet_window_of(None, Some(""))
                .unwrap_err()
                .starts_with("LAMP_CONTROL_QUIET ")
        );
        assert!(quiet_window_of(Some("08:00-08:00"), None).is_err());
    }

    #[test]
    fn a_microphone_stream_is_journaled_on_and_off_and_never_given_beyond_the_lamps_network() {
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Microphone]);
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let into = lines.clone();
        let mut door = door.with_journal(Journal::to(move |line| {
            into.lock().unwrap().push(line.to_owned())
        }));
        for (n, frame) in [
            r#"{"id":3,"op":"microphone.stream","on":true}"#,
            r#"{"id":4,"op":"microphone.stream","on":false}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let near = door.handle(&mut session, frame, &mut fake, 2 * S + n as u64 * S, 1);
            assert_eq!(code_of(&near), None, "{near:?}");
        }
        let client = pairing::client_id(&computer.key());
        door.journal_event(&client, "microphone.ended", 4 * S);
        let lines = lines.lock().unwrap().clone();
        let who = format!("{client} (a name the door doesn't write)");
        assert!(
            lines[0].starts_with(&format!(
                "lamp-control: {who}: microphone.stream (on): ok, to the ears, by mic1/audio, "
            )),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with(&format!(
                "lamp-control: {who}: microphone.stream (off): ok, to the ears, by mic1/audio, "
            )),
            "{lines:?}"
        );
        assert_eq!(
            lines[2],
            format!("lamp-control: {who}: event microphone.ended sent")
        );
        session.set_remote();
        let far = door.handle(
            &mut session,
            r#"{"id":5,"op":"microphone.stream","on":true}"#,
            &mut fake,
            5 * S,
            1,
        );
        assert_eq!(code_of(&far), Some("refused"));
        assert_eq!(reply(&far)["error"]["why"], LOCAL_ONLY);
    }

    #[test]
    fn a_computer_beyond_the_lamps_own_network_never_gets_the_camera_whatever_it_was_granted() {
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Light, Grant::Camera]);
        // On the lamp's own network the camera reaches the part.
        let near = door.handle(
            &mut session,
            r#"{"id":3,"op":"camera.snapshot"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(code_of(&near), None, "{near:?}");
        session.set_remote();
        for frame in [
            r#"{"id":4,"op":"camera.snapshot"}"#,
            r#"{"id":5,"op":"camera.stream","on":true}"#,
        ] {
            let far = door.handle(&mut session, frame, &mut fake, 3 * S, 1);
            assert_eq!(code_of(&far), Some("refused"));
            assert_eq!(reply(&far)["error"]["why"], LOCAL_ONLY);
        }
        // Everything else it was granted still goes.
        let light = door.handle(
            &mut session,
            r#"{"id":6,"op":"light","colour":"warm","level":2,"for_ms":1000}"#,
            &mut fake,
            4 * S,
            1,
        );
        assert_eq!(code_of(&light), None, "{light:?}");
    }

    /// `get` `body` says where the body is and what it may do: only with
    /// the body grant.
    #[test]
    fn get_body_needs_the_body_grant() {
        let get = r#"{"id":3,"op":"get","state":["body"]}"#;
        let computer = Computer::new(1);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Light]);
        let refused = door.handle(&mut session, get, &mut fake, 2 * S, 1);
        assert_eq!(code_of(&refused), Some("no_grant"));
        assert_eq!(reply(&refused)["error"]["why"], "not granted: body");
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Body]);
        let answered = door.handle(&mut session, get, &mut fake, 2 * S, 1);
        assert_eq!(code_of(&answered), None, "{answered:?}");
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
        // Motion is one each second.
        let again = door.handle(
            &mut session,
            r#"{"id":7,"op":"gesture","name":"nod"}"#,
            &mut fake,
            3 * S + 500_000,
            1,
        );
        assert_eq!(code_of(&again), Some("rate"));
        assert_eq!(reply(&again)["error"]["retry_ms"], 500);
        assert_eq!(reply(&again)["error"]["why"], "motion: 1 each second");
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

    /// A computer's own clips at the door (docs/control.md, "Sounds"): the
    /// speak grant, uploaded, listed and removed as its own, refused on a
    /// lamp keeping none, and deleted with its revoke.
    #[test]
    fn clips_are_uploaded_listed_removed_and_deleted_with_a_revoke() {
        use base64::Engine as _;
        use sha2::Digest as _;
        let folder =
            std::env::temp_dir().join(format!("lamp-control-door-clips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut clip = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut clip,
            hound::WavSpec {
                channels: 1,
                sample_rate: 24_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..2_400 {
            writer.write_sample(1_000_i16).unwrap();
        }
        writer.finalize().unwrap();
        let clip = clip.into_inner();
        let sha256: String = sha2::Sha256::digest(&clip)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let upload = format!(
            r#"{{"id":2,"op":"sound.upload","name":"chime","sha256":"{sha256}","size":{},"part":0,"parts":1,"data":"{}"}}"#,
            clip.len(),
            base64::engine::general_purpose::STANDARD.encode(&clip)
        );

        // No speak grant: refused.
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Look]);
        let mut door = door.with_sound_folder(folder.clone());
        let outcome = door.handle(&mut session, &upload, &mut fake, 2 * S, 1);
        assert_eq!(code_of(&outcome), Some("no_grant"));

        // A lamp keeping none: refused with why.
        let computer = Computer::new(2);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Speak]);
        let outcome = door.handle(&mut session, &upload, &mut fake, 2 * S, 1);
        assert_eq!(reply(&outcome)["error"]["why"], SOUNDS_OFF);

        let computer = Computer::new(3);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Speak]);
        let id = pairing::client_id(&computer.key());
        let mut door = door.with_sound_folder(folder.clone());
        let outcome = door.handle(&mut session, &upload, &mut fake, 2 * S, 1);
        assert_eq!(
            reply(&outcome)["sound"],
            json!({"name": "chime", "bytes": clip.len(), "ms": 100})
        );
        assert!(folder.join(&id).join("chime.wav").is_file());
        let outcome = door.handle(
            &mut session,
            r#"{"id":3,"op":"sound.list"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(
            reply(&outcome)["sounds"],
            json!([{"name": "chime", "bytes": clip.len(), "ms": 100}])
        );
        let outcome = door.handle(
            &mut session,
            r#"{"id":4,"op":"sound.remove","name":"chime"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&outcome)["removed"], "chime");
        assert!(!folder.join(&id).join("chime.wav").exists());

        // Kept again, then the computer revoked: its clips go with it.
        door.handle(&mut session, &upload, &mut fake, 2 * S, 1);
        assert!(folder.join(&id).join("chime.wav").is_file());
        assert!(door.revoke(&id, 3 * S).was);
        assert!(!folder.join(&id).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A computer's own notes at the door (docs/control.md, "Notes"): the
    /// notes grant, set, read, listed and deleted as its own, refused on a
    /// lamp keeping none, a bad key refused, and deleted with its revoke.
    #[test]
    fn notes_are_set_read_listed_deleted_and_deleted_with_a_revoke() {
        let folder =
            std::env::temp_dir().join(format!("lamp-control-door-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let set = r#"{"id":2,"op":"notes.set","key":"mood","value":{"calm":true}}"#;

        // No notes grant: refused, for each op.
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Speak]);
        let mut door = door.with_notes_folder(folder.clone());
        for frame in [
            set,
            r#"{"id":2,"op":"notes.get","key":"mood"}"#,
            r#"{"id":2,"op":"notes.delete","key":"mood"}"#,
            r#"{"id":2,"op":"notes.list"}"#,
        ] {
            let outcome = door.handle(&mut session, frame, &mut fake, 2 * S, 1);
            assert_eq!(code_of(&outcome), Some("no_grant"), "{frame}");
        }

        // A lamp keeping none: refused with why.
        let computer = Computer::new(2);
        let (mut door, mut session, mut fake) = paired(&computer, &[Grant::Notes]);
        let outcome = door.handle(&mut session, set, &mut fake, 2 * S, 1);
        assert_eq!(reply(&outcome)["error"]["why"], NOTES_OFF);

        let computer = Computer::new(3);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Notes]);
        let id = pairing::client_id(&computer.key());
        let mut door = door.with_notes_folder(folder.clone());
        // A write's answer comes when its disk write has run, off the door's
        // lock: run it here.
        let mut ask = |door: &mut Door, frame: &str| {
            let mut outcome = door.handle(&mut session, frame, &mut fake, 2 * S, 1);
            match outcome.later.take() {
                Some(later) => later.reply(),
                None => reply(&outcome).clone(),
            }
        };
        let kept = ask(&mut door, set);
        assert_eq!(kept["key"], "mood");
        assert_eq!(kept["bytes"], 13);
        assert!(folder.join(format!("{id}.json")).is_file());
        let read = ask(&mut door, r#"{"id":3,"op":"notes.get","key":"mood"}"#);
        assert_eq!(
            (read["found"].clone(), read["value"].clone()),
            (json!(true), json!({"calm": true}))
        );
        let missing = ask(&mut door, r#"{"id":3,"op":"notes.get","key":"nope"}"#);
        assert_eq!(missing["found"], false);
        assert!(missing.get("value").is_none(), "{missing}");
        ask(
            &mut door,
            r#"{"id":4,"op":"notes.set","key":"mode","value":1}"#,
        );
        let listed = ask(&mut door, r#"{"id":5,"op":"notes.list","prefix":"mo"}"#);
        assert_eq!(
            listed["keys"],
            json!([{"key": "mode", "bytes": 1}, {"key": "mood", "bytes": 13}])
        );
        let all = ask(&mut door, r#"{"id":5,"op":"notes.list"}"#);
        assert_eq!(all["count"], 2);
        assert_eq!(all["keys_max"], 256);
        // A bad key or a missing value is a bad request, nothing kept.
        for frame in [
            r#"{"id":6,"op":"notes.set","key":"a b","value":1}"#,
            r#"{"id":6,"op":"notes.set","key":"","value":1}"#,
            r#"{"id":6,"op":"notes.set","key":"k"}"#,
            r#"{"id":6,"op":"notes.get","key":"../x"}"#,
            r#"{"id":6,"op":"notes.list","prefix":"a/b"}"#,
        ] {
            assert_eq!(
                ask(&mut door, frame)["error"]["code"],
                "bad_request",
                "{frame}"
            );
        }
        let deleted = ask(&mut door, r#"{"id":7,"op":"notes.delete","key":"mode"}"#);
        assert_eq!(deleted["deleted"], true);
        let again = ask(&mut door, r#"{"id":7,"op":"notes.delete","key":"mode"}"#);
        assert_eq!(again["deleted"], false);

        // The computer revoked: its notes go with it.
        assert!(door.revoke(&id, 3 * S).was);
        assert!(!folder.join(format!("{id}.json")).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// nine's P3 on banana/store: a write the notes refused still cost the
    /// disk a try, so it counts in the rate; the 31st write in a minute is
    /// `rate` whether or not the earlier ones took.
    #[test]
    fn a_refused_notes_write_still_counts_in_the_rate() {
        let folder = std::env::temp_dir().join(format!(
            "lamp-control-door-notes-rate-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Notes]);
        let mut door = door.with_notes_folder(folder.clone());
        let too_big = format!(
            r#"{{"id":2,"op":"notes.set","key":"k","value":"{}"}}"#,
            "x".repeat(16_400)
        );
        for n in 0..30 {
            let outcome = door.handle(&mut session, &too_big, &mut fake, 2 * S + n, 1);
            assert_eq!(code_of(&outcome), Some("bad_request"), "{n}");
        }
        let outcome = door.handle(&mut session, &too_big, &mut fake, 2 * S + 40, 1);
        assert_eq!(code_of(&outcome), Some("rate"));
        // A read has its own, untouched window.
        let outcome = door.handle(
            &mut session,
            r#"{"id":3,"op":"notes.get","key":"k"}"#,
            &mut fake,
            2 * S + 50,
            1,
        );
        assert_eq!(reply(&outcome)["found"], false);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// eighth's P3 on banana/store: a write refused as busy (the last one
    /// is still being kept) costs the disk nothing, so it is not in the rate.
    #[test]
    fn a_busy_notes_write_does_not_count_in_the_rate() {
        let folder = std::env::temp_dir().join(format!(
            "lamp-control-door-notes-busy-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let computer = Computer::new(1);
        let (door, mut session, mut fake) = paired(&computer, &[Grant::Notes]);
        let mut door = door.with_notes_folder(folder.clone());
        let mut first = door.handle(
            &mut session,
            r#"{"id":2,"op":"notes.set","key":"k","value":1}"#,
            &mut fake,
            2 * S,
            1,
        );
        let kept = first.later.take().expect("a write is kept later");
        for n in 0..40 {
            let outcome = door.handle(
                &mut session,
                r#"{"id":3,"op":"notes.set","key":"j","value":1}"#,
                &mut fake,
                2 * S + n,
                1,
            );
            assert_eq!(code_of(&outcome), Some("busy"), "{n}");
        }
        assert_eq!(kept.reply()["key"], "k");
        // Forty busy tries later this computer is not rate-limited.
        let mut next = door.handle(
            &mut session,
            r#"{"id":4,"op":"notes.set","key":"j","value":1}"#,
            &mut fake,
            2 * S + 100,
            1,
        );
        assert_eq!(next.later.take().expect("kept").reply()["key"], "j");
        let _ = std::fs::remove_dir_all(&folder);
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
            r#"{"id":4,"op":"gesture","name":"Idle"}"#,
            &mut fake,
            2 * S,
            1,
        );
        assert_eq!(reply(&outcome)["re"], 4);
        assert!(fake.did.is_empty());
    }
}
