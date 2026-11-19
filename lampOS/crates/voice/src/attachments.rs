//! A photo the owner sent from the app, carried to the spoken turn
//! (`LAMP_ATTACHMENTS=on`, off by default; #7659 slice 1, with the first
//! builder's rulings of 18 Nov 10:24). `lamp-cloud` keeps it in
//! `/run/lampos-attachments` and says where on `attachments/arrived`
//! (`lamp_msgs::AttachRef`, a path, never its bytes); [`start`] listens.
//!
//! - **Who it reaches** (ruling (a)): the owner's next line within 120 s
//!   carries the photo once, beside their words as a look's picture is
//!   ([`carried`], the second route's `Message::user_seeing`). The owner's
//!   line is their kept face's, alone in view ([`crate::person::may_tell`]).
//!   An owner with no kept face: a line in the conversation that was already
//!   open when it arrived, with one face in view and no other known face. A
//!   guest's line, two faces, or a line with no lease get nothing.
//! - **On arrival** nothing is said.
//! - **Let go** ([`let_go`]): when the microphone's privacy lease closes
//!   (`voice/privacy`), on a forget or `forget_everything` ([`forget`]), and
//!   30 min after the conversation's last line ([`KEPT_AFTER`]). A reboot
//!   empties the folder (tmpfs).
//! - Never a face source, never kept in memory: the picture goes in that
//!   one model call only, as a look's does.
//!
//! The journal says the id, the kind, the bytes and the request, never what
//! it shows.
use lamp_msgs::{AttachRef, MicrophonePrivacy};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Only the exact value `on`, in the second route's worker.
pub const ENVIRONMENT: &str = "LAMP_ATTACHMENTS";
/// How long after the conversation's last line an attachment is let go.
pub const KEPT_AFTER: Duration = Duration::from_secs(30 * 60);
/// What the model is told beside the person's words when their line
/// carries the photo.
pub const NOTE: &str = "\n\n(With these words the person sent this photo from the app on their phone: it is not what your camera sees. Speak of it only if what they say is about it; never bring it up yourself.)";
/// How often the listener looks at the bus.
const TICK: Duration = Duration::from_secs(1);
/// The subscriber's name.
const SOURCE: &str = "voice";
/// The only publisher taken: `lamp-cloud`.
const CLOUD: &str = "cloud";
/// A privacy lease not renewed this long is closed: it lasts 50 ms and is
/// renewed every 10 ms.
const LEASE_SILENT_MS: u64 = 2_000;

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// An attachment told, as the listener took it.
#[derive(Clone, Debug)]
pub(crate) struct Arrived {
    pub(crate) attach: AttachRef,
    pub(crate) at: Instant,
    /// Whether a conversation was open as it arrived: a line within
    /// [`crate::thread::GOING_ON_FOR`] before it.
    pub(crate) open_at_arrival: bool,
    /// Whether a line has carried it: once only.
    pub(crate) carried: bool,
}

/// The newest attachment, and when the last spoken line began.
#[derive(Debug, Default)]
pub(crate) struct Inbox {
    pub(crate) arrived: Option<Arrived>,
    pub(crate) last_line: Option<Instant>,
}

static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    arrived: None,
    last_line: None,
});
/// The listener's bus, for the faces in view as a line begins.
static BUS: Mutex<Option<lamp_bus::Bus>> = Mutex::new(None);

fn inbox() -> MutexGuard<'static, Inbox> {
    INBOX.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Who a line is with as it begins.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'a> {
    /// The owner as the eyes still know them (`known_faces::owner_now`):
    /// none when the owner has no kept face.
    pub(crate) owner: Option<&'a str>,
    /// The line's memory person, `None` the room's.
    pub(crate) person: Option<&'a str>,
    /// How many faces the eyes see now, `None` with no look.
    pub(crate) faces: Option<usize>,
}

/// Whether `line`, beginning at `now`, carries `arrived` (ruling (a)):
/// a photo not carried yet, within 120 s; the owner's own line, or, for an
/// owner with no kept face, a line of the conversation open at its arrival
/// (`last_line`, the line before, within the conversation's gap) with one
/// face in view and no known person.
pub(crate) fn may_carry(
    arrived: &Arrived,
    line: Line<'_>,
    last_line: Option<Instant>,
    now: Instant,
) -> bool {
    if arrived.carried
        || arrived.attach.kind != AttachRef::PHOTO
        || now.saturating_duration_since(arrived.at) > AttachRef::LASTS
    {
        return false;
    }
    match line.owner {
        Some(owner) => crate::person::may_tell(
            owner,
            crate::person::InView {
                person: line.person,
                faces: line.faces,
            },
        ),
        None => {
            arrived.open_at_arrival
                && last_line.is_some_and(|last| {
                    now.saturating_duration_since(last) <= crate::thread::GOING_ON_FOR
                })
                && line.person.is_none()
                && line.faces == Some(1)
        }
    }
}

/// A spoken line begins at `now` (`line` none: no lease, or a model that
/// takes no picture): noted as the conversation's newest, and the photo's
/// bytes when it carries it, marked carried.
pub(crate) fn take_photo(
    inbox: &mut Inbox,
    line: Option<Line<'_>>,
    now: Instant,
) -> Option<Vec<u8>> {
    let last = inbox.last_line.replace(now);
    let line = line?;
    let arrived = inbox.arrived.as_mut()?;
    if !may_carry(arrived, line, last, now) {
        return None;
    }
    let jpeg = read_photo(Path::new(&arrived.attach.path))?;
    arrived.carried = true;
    Some(jpeg)
}

/// The photo at `path`: a JPEG of at most 184 KiB, or nothing.
fn read_photo(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    (bytes.len() as u64 <= AttachRef::MOST_PHOTO_BYTES && bytes.starts_with(&[0xFF, 0xD8]))
        .then_some(bytes)
}

/// The second route's spoken line `request` begins, with its lease
/// (`scope`) and whether the model takes a picture (`sees`): the photo
/// from the app when this line carries it ([`may_carry`]), once. Off,
/// nothing and nothing noted.
pub(crate) fn carried(
    request: u64,
    scope: Option<&crate::memory_scope::ScopeLease>,
    sees: bool,
) -> Option<Vec<u8>> {
    if !on() {
        return None;
    }
    let person: Option<Option<String>> = scope
        .and_then(crate::memory_scope::ScopeLease::scope)
        .map(|snapshot| snapshot.person().map(str::to_owned));
    let owner = crate::known_faces::owner_now();
    let faces = BUS
        .lock()
        .ok()
        .and_then(|bus| bus.as_ref().and_then(crate::known_faces::faces_in_view));
    let line = person.as_ref().filter(|_| sees).map(|person| Line {
        owner: owner.as_deref(),
        person: person.as_deref(),
        faces,
    });
    let mut inbox = inbox();
    let jpeg = take_photo(&mut inbox, line, Instant::now())?;
    if let Some(arrived) = &inbox.arrived {
        eprintln!(
            "lamp-voice: attachment {}: a photo of {} bytes goes with request {request}, once (LAMP_ATTACHMENTS)",
            arrived.attach.id,
            jpeg.len()
        );
    }
    Some(jpeg)
}

/// `attach` arrived at `now`: the newest, in place of any before it (whose
/// file `lamp-cloud` removed already, and is removed here too).
pub(crate) fn arrive(inbox: &mut Inbox, attach: AttachRef, now: Instant) {
    let open_at_arrival = inbox
        .last_line
        .is_some_and(|last| now.saturating_duration_since(last) <= crate::thread::GOING_ON_FOR);
    if let Some(old) = inbox.arrived.take()
        && old.attach.path != attach.path
    {
        let _ = std::fs::remove_file(old.attach.path);
    }
    inbox.arrived = Some(Arrived {
        attach,
        at: now,
        open_at_arrival,
        carried: false,
    });
}

/// The attachment let go: its file removed, and its id for the journal.
pub(crate) fn let_go(inbox: &mut Inbox) -> Option<String> {
    let arrived = inbox.arrived.take()?;
    let _ = std::fs::remove_file(arrived.attach.path);
    Some(arrived.attach.id)
}

/// Let go when [`KEPT_AFTER`] has passed since the conversation's last
/// line, or since it arrived if no line came after.
pub(crate) fn expire(inbox: &mut Inbox, now: Instant) -> Option<String> {
    let arrived = inbox.arrived.as_ref()?;
    let since = inbox
        .last_line
        .map_or(arrived.at, |last| last.max(arrived.at));
    if now.saturating_duration_since(since) < KEPT_AFTER {
        return None;
    }
    let_go(inbox)
}

/// A forget or `forget_everything` ran: what the app sent goes with it.
pub fn forget() {
    if let Some(id) = let_go(&mut inbox()) {
        eprintln!("lamp-voice: attachment {id} let go: a forget (LAMP_ATTACHMENTS)");
    }
}

/// With the switch on, listens on `bus_root` for `attachments/arrived`, and
/// lets go on a closed privacy lease and after [`KEPT_AFTER`], on a thread
/// of its own; once.
pub fn start(bus_root: &Path) {
    if !on() {
        return;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    let root = bus_root.to_owned();
    STARTED.call_once(move || {
        let started = std::thread::Builder::new()
            .name("lamp-voice-attachments".into())
            .spawn(move || listen(&root));
        if started.is_err() {
            eprintln!(
                "lamp-voice: attachments from the app can't be listened for (LAMP_ATTACHMENTS)"
            );
        }
    });
}

fn listen(root: &Path) {
    let Ok(bus) = lamp_bus::Bus::new(root) else {
        eprintln!("lamp-voice: no bus, so no attachment from the app is taken (LAMP_ATTACHMENTS)");
        return;
    };
    if let Ok(mut kept) = BUS.lock() {
        *kept = Some(bus.clone());
    }
    eprintln!(
        "lamp-voice: a photo from the app goes with the owner's next line within 120 s, once; nothing is said as it arrives (LAMP_ATTACHMENTS)"
    );
    let mut arrivals = None;
    loop {
        if arrivals.is_none() {
            arrivals = bus
                .subscribe_typed(AttachRef::TOPIC, SOURCE, AttachRef::TYPE)
                .ok();
        }
        let message = match arrivals.as_mut() {
            Some(arrivals) => arrivals.recv(TICK).ok().flatten(),
            None => {
                std::thread::sleep(TICK);
                None
            }
        };
        let now = Instant::now();
        if let Some(message) = message {
            match taken(&message) {
                Some(attach) => {
                    eprintln!(
                        "lamp-voice: attachment {}: a {} of {} bytes arrived from the app (LAMP_ATTACHMENTS)",
                        attach.id, attach.kind, attach.bytes
                    );
                    arrive(&mut inbox(), attach, now);
                }
                None => eprintln!(
                    "lamp-voice: an attachments/arrived message not taken: not lamp-cloud's, or not in its folder (LAMP_ATTACHMENTS)"
                ),
            }
        }
        if privacy_closed(&bus)
            && let Some(id) = let_go(&mut inbox())
        {
            eprintln!(
                "lamp-voice: attachment {id} let go: the microphone's privacy lease closed (LAMP_ATTACHMENTS)"
            );
        }
        if let Some(id) = expire(&mut inbox(), now) {
            eprintln!(
                "lamp-voice: attachment {id} let go: 30 min after the conversation (LAMP_ATTACHMENTS)"
            );
        }
    }
}

/// The attachment `message` tells, only from `lamp-cloud` and only in its
/// folder (`AttachRef::DIR`): a file the voice, as root, reads and removes.
pub(crate) fn taken(message: &lamp_bus::Message) -> Option<AttachRef> {
    if message.header.source != CLOUD {
        return None;
    }
    let attach = message.parse::<AttachRef>().ok()?;
    (attach.check().is_ok() && attach.in_inbox()).then_some(attach)
}

/// Whether the microphone's privacy lease (`voice/privacy`) is closed: not
/// permitted, the switch read muted, or not renewed for
/// [`LEASE_SILENT_MS`]. A lamp with no lease on its bus has none to close.
fn privacy_closed(bus: &lamp_bus::Bus) -> bool {
    let Ok(Some(message)) = bus.latest(MicrophonePrivacy::TOPIC) else {
        return false;
    };
    let Ok(lease) = message.parse::<MicrophonePrivacy>() else {
        return false;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    closed(
        &lease,
        message.expired,
        now_ms.saturating_sub(message.header.at_unix_ms),
    )
}

/// [`privacy_closed`]'s rule, for a lease read `silent_ms` after it was
/// published.
pub(crate) fn closed(lease: &MicrophonePrivacy, expired: bool, silent_ms: u64) -> bool {
    !lease.permitted
        || lease.physical_muted == Some(true)
        || (expired && silent_ms > LEASE_SILENT_MS)
}

#[cfg(test)]
mod tests;
