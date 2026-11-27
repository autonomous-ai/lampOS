//! `LAMP_PERSON_SCOPE_SEEN`: a line is in a known person's scope only when
//! their own face was seen speaking it, else the room's (reviewer-three's P1
//! and P2 on #3435, Second's late-keeper case at 09:24).
use super::*;
use crate::memory_scope::{FaceCache, KEEPER_WAIT, ScopeLease};
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use std::sync::atomic::{AtomicBool, Ordering};

/// The eyes' known file naming A with face `f1`, written by the eyes of
/// boot [`BOOT`] after their first frame (as `lamp-eyes` writes it), so
/// `known_with_boot` pairs it with that boot.
fn known_file(what: &str, eyes: &SpeakingFaces) -> std::path::PathBuf {
    eyes.look(
        Instant::now(),
        Some(((BOOT, 0), &lamp_msgs::Faces { faces: vec![] })),
    );
    // No tolerance at the boot's first frame (Fourth's P2 on #3038).
    std::thread::sleep(Duration::from_millis(5));
    let path = std::env::temp_dir().join(format!(
        "scope-seen-{what}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    path
}

/// How long a one-face forget's listing waits, and the marks it leaves
/// at the wait's start and end ([`FORGET_STALL`]).
type ForgetStall = (Duration, Arc<Mutex<Vec<Instant>>>);

/// Whom the face cache names before a line, by line ([`NAMED_LINES`]).
type NamedBy = fn(u64) -> Option<bool>;

/// The eyes' boot the known file was written under.
const BOOT: &str = "eyes";

thread_local! {
    /// The kept id a one-face line's `remember` is told and marked with as
    /// the memory's route marks it (`tools::ONE_FACE_KEPT`), for the bare
    /// forget after it (`LAMP_ONE_FACE_FORGETS_ON_YES`); none, as before.
    static KEEP_MARKS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// A don't-know's search (`LAMP_MEMORY_DONT_KNOW_RECALLS`) the next
    /// [`lines_known`] on this thread is given, once.
    static DONT_KNOW_FOUND: std::cell::RefCell<Option<crate::second_provider::Remembered>> =
        const { std::cell::RefCell::new(None) };
    /// `LAMP_PERSON_SCOPE_ONE_FACE_READS` ([`Engine::reading_by_one_face`])
    /// for the next [`lines_known`] on this thread, once.
    static ONE_FACE_READS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The owner's language ([`Engine::in_language`]) for the next
    /// [`lines_known`] on this thread, once (#8185).
    static LANGUAGE: std::cell::Cell<Option<crate::language::Language>> =
        const { std::cell::Cell::new(None) };
    /// Lines the voice gives no sound, once each, in the next
    /// [`lines_known`] on this thread (#8185: a fixed line never rendered).
    static SILENT_ONCE: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// The first request a fixed `going_on` answers for, in the next
    /// [`lines_known`] on this thread, once: lines before it are in no
    /// conversation going on (#7454: a one-face line, then a carried yes).
    static GOING_ON_FROM: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// `LAMP_MEMORY_DOUBT_UNDOES` ([`Engine::undoing_doubted_keeps`]) for
    /// the next [`lines_known`] on this thread, once: a `remember` is then
    /// answered as a new keep with id 391, and a `forget` as forgotten.
    static DOUBT_UNDOES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `LAMP_CONVERSATION_CARRIED_MEMORY` ([`Engine::reading_carried`]) for
    /// the next [`lines_known`] on this thread, once.
    static CARRIED_MEMORY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `LAMP_CONVERSATION_CARRIES_OWN_TRACK` ([`Engine::carrying_own_track`])
    /// for the next [`lines_known`] on this thread, once.
    static OWN_TRACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `LAMP_FACE_STICKY` ([`Engine::holding_sticky`]) for the next
    /// [`lines_known`] on this thread, once.
    static STICKY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The same-voice check not running for the route
    /// ([`Engine::reading_carried_where_heard`]) for the next
    /// [`lines_known`] on this thread, once.
    static CARRIED_UNHEARD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The murmured line's mouths the next [`lines_known`] on this thread
    /// reads in place of [`S4_OPENING_MOUTHS`], once.
    static MURMUR_MOUTHS: std::cell::Cell<Option<&'static [lamp_msgs::SpeakingState]>> =
        const { std::cell::Cell::new(None) };
    /// For the next [`lines_known`] on this thread, once: line 1's answer
    /// stops before this sentence is played, and line 2 begins then, so
    /// line 1 is cut short and its words wait for line 2's verdict
    /// (`LAMP_JOIN_CUT_WORDS`, #1712); with the flag, short sounds are
    /// checked and line 2's end is marked as one, so a line 2 with words
    /// is held unjudged.
    static CUT_FIRST: std::cell::Cell<Option<(&'static str, bool)>> =
        const { std::cell::Cell::new(None) };
    /// The one kept thing of A's a named forget on a one-face line names
    /// ([`Engine::targeting_one_face_forgets`]) for the next
    /// [`lines_known`] on this thread, once: its id and subject, for any
    /// forget whose line or `about` says "umbrella".
    static FORGET_TARGET: std::cell::Cell<Option<(u64, &'static str)>> =
        const { std::cell::Cell::new(None) };
    /// How long [`FORGET_TARGET`]'s listing waits, as a frozen memory's
    /// does (up to `tools::ASK_DEADLINE`), and where it marks its start and
    /// end, for the next [`lines_known`] on this thread, once (#8204).
    static FORGET_STALL: std::cell::RefCell<Option<ForgetStall>> =
        const { std::cell::RefCell::new(None) };
    /// `LAMP_SESSION_PERSON` ([`Engine::keeping_session_person`]) for the
    /// next [`lines_known`] on this thread, once.
    static SESSION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whom the face cache names before each line of the next
    /// [`lines_known`] on this thread, once, by line: `Some(false)` the
    /// known file's person matched, `Some(true)` their name the eyes hold
    /// (sticky), `None` nobody. Unset, as `named` says once at the start.
    static NAMED_LINES: std::cell::Cell<Option<NamedBy>> =
        const { std::cell::Cell::new(None) };
    /// The line of the next [`lines_known`] on this thread, once, before
    /// which privacy goes on (the camera not allowed, `memory_camera`).
    static PRIVACY_AT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Added to each line's request in the next [`lines_known`] on this
    /// thread, once, for a test reading what is kept by request across the
    /// crate (`remember::kept_out`).
    static REQUEST_BASE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// One test at a time that reads a one-face keep's mark: the marks
/// (`tools::ONE_FACE_KEPT`) are the process's, by worker and session, and
/// these tests share both, so one's keep would stand for another's.
static KEEP_MARK_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The worker's own setup (`worker::see_who_speaks`) with
/// `LAMP_SECOND_SPEAKING_FACE` on and `LAMP_PERSON_SCOPE_SEEN` as `seen`,
/// whose eyes' reader fails to start: a relative bus root, which
/// `lamp_bus::Bus::new` refuses (reviewer-two's P2, Second's P3 on #3437).
fn failed_reader(engine: Engine, known: &std::path::Path, seen: bool) -> Engine {
    assert!(engine.scopes_context());
    let (engine, eyes, reader) = super::super::worker::see_who_speaks(
        engine,
        Some(&known.to_path_buf()),
        Some(std::path::Path::new("relative-bus-root")),
        true,
        seen,
    );
    assert!(
        eyes.is_none() && reader.is_none(),
        "the reader never started"
    );
    engine
}

fn clean(path: &std::path::Path) {
    let _ = std::fs::remove_file(known_face_of(path));
    let _ = std::fs::remove_file(path);
}

/// The eyes of boot `boot` while a line is said: a face `f1` in view,
/// speaking it or silent, until `done`. Under another boot than [`BOOT`]
/// the eyes started again and their `f1` is a different face.
fn feed(
    eyes: Arc<SpeakingFaces>,
    f1_speaks: bool,
    boot: &'static str,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    feed_faces(eyes, vec![("f1", f1_speaks)], &[], boot, done)
}

/// [`feed`] with `in_view` the faces in each look, by passing id, each
/// speaking it or silent; none, looks with no face in them. With `mouths`,
/// each look reads every face's mouth by the next of them in turn instead.
fn feed_faces(
    eyes: Arc<SpeakingFaces>,
    in_view: Vec<(&'static str, bool)>,
    mouths: &'static [lamp_msgs::SpeakingState],
    boot: &'static str,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut frame = 0u64;
        while !done.load(Ordering::Acquire) {
            let mouth = (!mouths.is_empty()).then(|| mouths[frame as usize % mouths.len()]);
            let faces = lamp_msgs::Faces {
                faces: in_view
                    .iter()
                    .map(|&(id, speaks)| lamp_msgs::Face {
                        pitch: None,
                        looking: None,
                        id: id.to_owned(),
                        xmin: 0.1,
                        ymin: 0.1,
                        xmax: 0.3,
                        ymax: 0.4,
                        confidence: 1.0,
                        yaw: None,
                        facing: None,
                        speaking: Some(lamp_msgs::Speaking {
                            state: mouth.unwrap_or(if speaks {
                                lamp_msgs::SpeakingState::Yes
                            } else {
                                lamp_msgs::SpeakingState::No
                            }),
                            score: 0.9,
                            from: None,
                        }),
                        follows: None,
                    })
                    .collect(),
            };
            frame += 1;
            eyes.look(Instant::now(), Some(((boot, frame), &faces)));
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

/// A face's mouth read speaking in one look of four and unread in the
/// rest: never seen speaking it (no majority), but a face alone in view is
/// then named for the reply by `named_alone`
/// ([`SpeakingFaces::alone_in_view`]; lamp-06c1, 5 Nov, S4's opening line:
/// read speaking in 1 of 9 looks).
const S4_OPENING_MOUTHS: &[lamp_msgs::SpeakingState] = &[
    lamp_msgs::SpeakingState::Yes,
    lamp_msgs::SpeakingState::Unknown,
    lamp_msgs::SpeakingState::Unknown,
    lamp_msgs::SpeakingState::Unknown,
];

/// What one line admitted as the owner A's came to: the scope each memory
/// call ran in, the scope the line's lease ends in, and what its first
/// round was told.
/// A memory note asked: whose and on which words.
type Noted = (Option<String>, String);
/// A memory hits ask: whose scope it read and by which rule; none, revoked.
type HitsRead = Option<(Option<String>, crate::remember::HitsPath)>;

struct Outcome {
    calls: Vec<Option<String>>,
    lease: Option<String>,
    told: Message,
    /// Each memory note asked, as it was asked.
    noted: Vec<Noted>,
    /// Whose context each chat was told, in the order asked.
    asked: Vec<Message>,
    /// Every output of the turn, as [`turn`]'s labels.
    labels: Vec<String>,
    /// Whether the line's lease ends held, told from the conversation as
    /// it stood ([`crate::memory_scope::ScopeSnapshot::held`]).
    held: bool,
    /// Every chat round, as asked.
    rounds: Vec<Vec<Message>>,
    /// Whose scope, and by which rule, each memory hits ask read
    /// (`worker::hits_of_lease`, as the product's callback reads it).
    hits: Vec<HitsRead>,
    /// Whose name the line's reply is told by the face name, as the worker
    /// picks it (`worker::known_person_for`, by lease).
    named_for_reply: Option<String>,
    /// Every call run, by name and arguments, in order.
    ran: Vec<(String, Value)>,
}

async fn one_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    f1_speaks: bool,
    boot: &'static str,
    seen: bool,
) -> Outcome {
    one_line_eyes(what, words, chats, f1_speaks, boot, seen, true, None).await
}

/// [`one_line`], with `eyes_up` false set up by the worker with a reader
/// that fails to start ([`failed_reader`]): the engine never gets the
/// eyes.
#[allow(clippy::too_many_arguments)]
async fn one_line_eyes(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    f1_speaks: bool,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
) -> Outcome {
    let in_view = vec![("f1", f1_speaks)];
    one_line_with(
        what, words, chats, in_view, boot, seen, eyes_up, early, None, true,
    )
    .await
}

/// [`one_line_eyes`] with `in_view` the faces in each of the eyes' looks
/// ([`feed_faces`]) and `going_on` whether A is in a conversation going on
/// (`LAMP_CONVERSATION_CARRIES`, #4339); none, the switch off. With
/// `named` false the face cache names nobody, so the line is admitted as the
/// room's (lamp-06c1's turns with no face line, 29 Oct).
#[allow(clippy::too_many_arguments)]
async fn one_line_with(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
) -> Outcome {
    one_line_saving(
        what, words, chats, in_view, boot, seen, eyes_up, early, going_on, named, false,
    )
    .await
}

/// [`one_line_with`] with `saves` as `LAMP_CONVERSATION_CARRIED_SAVES`
/// ([`Engine::saving_carried`]).
#[allow(clippy::too_many_arguments)]
async fn one_line_saving(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
) -> Outcome {
    lines_saving(
        what,
        vec![(words.to_owned(), in_view, 0)],
        chats,
        boot,
        seen,
        eyes_up,
        early,
        going_on,
        named,
        saves,
        Voice::Off,
    )
    .await
}

/// `LAMP_PERSON_SCOPE_ONE_FACE` for [`lines_saving`]: off, or on with the
/// rule's record of another voice in the conversation.
#[derive(Clone, Copy, PartialEq)]
enum Voice {
    /// The switch off.
    Off,
    /// On, no other voice heard.
    One,
    /// On, another voice heard earlier in the conversation.
    AnotherBefore,
    /// On, another voice heard as this line was judged.
    AnotherInIt,
    /// On, the same-voice check not running: the rule judges the line, but
    /// nothing ever records another voice.
    Unchecked,
    /// On, the rule judging this line and hearing no other voice, as on a
    /// route whose same-voice check runs.
    Judged,
}

/// One line said: its words, the faces in the eyes' looks while it is said
/// ([`feed_faces`]), and the seconds since the line before.
type Line = (String, Vec<(&'static str, bool)>, u64);

/// [`lines_known`]'s calls run on the voice's own memory route against a
/// memory socket at `socket`, and `said`, the lamp's words for each line,
/// kept in the voice's thread as the worker keeps them.
struct Real {
    socket: std::path::PathBuf,
    said: Vec<&'static str>,
    /// Added to each line's request: the voice's thread is shared by the
    /// crate's tests, so the real route's lines are numbered apart.
    base: u64,
}

/// A short socket path for a test's memory, by its requests' base: a Unix
/// socket's path has at most 104 bytes on macOS, which the temp dir with a
/// thread id passes.
fn memory_socket(base: u64) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/tmp/lv-{base}-{}.sock", std::process::id()))
}

/// A memory at `socket`, answering every ask kept: the asks it is sent,
/// one JSON object each, until `stop`.
fn memory_at(
    socket: &std::path::Path,
    stop: Arc<AtomicBool>,
) -> (Arc<Mutex<Vec<Value>>>, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, Write};
    let _ = std::fs::remove_file(socket);
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let asked: Arc<Mutex<Vec<Value>>> = Arc::default();
    let kept = asked.clone();
    let serving = std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            if std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .is_err()
            {
                continue;
            }
            if let Ok(ask) = serde_json::from_str::<Value>(&line) {
                kept.lock().unwrap().push(ask);
            }
            let _ = stream.write_all(b"{\"ok\":true,\"id\":1}\n");
        }
    });
    (asked, serving)
}

/// [`one_line_saving`] over `lines`, said one after another, requests 1,
/// 2, … With `saves` the eyes also judge a held call's yes
/// ([`Engine::confirming_forgets_by_sight`]), as on the lamp.
#[allow(clippy::too_many_arguments)]
async fn lines_saving(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    voice: Voice,
) -> Outcome {
    lines_past_silent(
        what, lines, chats, boot, seen, eyes_up, early, going_on, named, saves, None, voice,
    )
    .await
}

/// [`lines_saving`] with `past_silent` as `LAMP_CONVERSATION_CARRIES_PAST_SILENT`
/// ([`Engine::carrying_past_silent_where_heard`], the rule keeping its sign):
/// whether the same-voice check runs, and whether the conversation has shown
/// another voice, asked each time; none, the switch off.
#[allow(clippy::too_many_arguments)]
async fn lines_past_silent(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    past_silent: Option<(bool, crate::second_provider::AnotherVoice)>,
    voice: Voice,
) -> Outcome {
    lines_known(
        what,
        lines,
        chats,
        (boot, seen, eyes_up),
        early,
        going_on,
        named,
        saves,
        past_silent,
        voice,
        (false, &[]),
        |_, _| {},
        (false, None, None),
    )
    .await
}

/// [`lines_past_silent`] with `name_one_face` as `LAMP_SECOND_NAME_ONE_FACE`
/// ([`Engine::naming_by_one_face`]), and `mouths`, when any, each look's
/// mouth read for every face in turn ([`feed_faces`]).
#[allow(clippy::too_many_arguments)]
async fn lines_named(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    past_silent: Option<(bool, crate::second_provider::AnotherVoice)>,
    voice: Voice,
    name_one_face: bool,
    mouths: &'static [lamp_msgs::SpeakingState],
) -> Outcome {
    lines_known(
        what,
        lines,
        chats,
        (boot, seen, eyes_up),
        early,
        going_on,
        named,
        saves,
        past_silent,
        voice,
        (name_one_face, mouths),
        |_, _| {},
        (false, None, None),
    )
    .await
}

/// [`lines_named`], with `known_at` called before each line with its
/// request and the eyes' known file, as the eyes rewrite or stop writing
/// it between lines. With `thread`, whose conversation is going on is
/// read from a thread of the test's own, kept after each line as the worker
/// keeps it (`Thread::lamp_for_named`, with the line's told lease), in place
/// of `going_on`'s fixed answer.
#[allow(clippy::too_many_arguments)]
async fn lines_known(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    (boot, seen, eyes_up): (&'static str, bool, bool),
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    past_silent: Option<(bool, crate::second_provider::AnotherVoice)>,
    voice: Voice,
    (name_one_face, mouths): (bool, &'static [lamp_msgs::SpeakingState]),
    known_at: fn(u64, &std::path::Path),
    (thread, murmur, real): (bool, Option<u64>, Option<&Real>),
) -> Outcome {
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file(what, &eyes);
    let doubt_undoes = DOUBT_UNDOES.with(|on| on.replace(false));
    let script = Arc::new(Script {
        heard: Mutex::new(lines.iter().map(|(words, ..)| Ok(words.clone())).collect()),
        chats: Mutex::new(chats.into()),
        early: Mutex::new(
            early
                .map(|early| vec![net::Early::Words(early.to_owned())])
                .into_iter()
                .collect(),
        ),
        silent_once: Mutex::new(SILENT_ONCE.with(std::cell::RefCell::take)),
        ..Script::default()
    });
    let cut_first = CUT_FIRST.with(std::cell::Cell::take);
    let (_release, hold) = tokio::sync::oneshot::channel::<()>();
    if let Some((sentence, _)) = cut_first {
        *script.speech_hold.lock().unwrap() = Some((sentence, hold));
    }
    let (engine, _, now) = scripted(script.clone());
    // `LAMP_SECOND_PREPARED`, with early words to prepare on.
    let engine = engine
        .preparing(early.is_some())
        .checking_sounds(cut_first.is_some_and(|(_, sound)| sound));
    let noted: Arc<Mutex<Vec<Noted>>> = Arc::default();
    let seen_noted = noted.clone();
    let hits: Arc<Mutex<Vec<HitsRead>>> = Arc::default();
    let seen_hits = hits.clone();
    let cache = Arc::new(FaceCache::default());
    let calls: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let (seen_calls, seen_leases) = (calls.clone(), leases.clone());
    let ran: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen_ran = ran.clone();
    let keep_marks = KEEP_MARKS.with(std::cell::Cell::take);
    // With `real`, each call runs on the voice's own memory route
    // (`tools::run_call_scoped`, as the worker's runner does) against the
    // test's memory socket, here on the test's thread so the test's switches
    // hold for it.
    let route = real.map(|real| {
        crate::tools::route(
            &crate::tools::Settings {
                memory: Some(real.socket.clone()),
                ..Default::default()
            },
            crate::remember::REMEMBER,
        )
    });
    let runner: ScopedToolRunner = Arc::new(
        move |request, heard, name: String, args: Value, lease: ScopeLease| {
            seen_ran.lock().unwrap().push((name.clone(), args.clone()));
            // Whose scope the call runs in first, as the memory's route picks
            // it: a `remember` or single-item `forget` on a one-face line is
            // its person's (`ScopeLease::reached_by_one_face`, the route's own
            // decision), all else the lease's scope. A `list_reminders` is
            // whose reminders the Schedule is asked for, as the route's caller
            // tells it with `LAMP_ONE_FACE_REMINDERS` on.
            seen_calls.lock().unwrap().push(if name == "list_reminders" {
            crate::tools::schedule_whose::caller_among(Some(&lease), &name, &[], true)["asker"]
                ["person"]
                .as_str()
                .map(str::to_owned)
        } else {
            lease
                .reached_by_one_face(&name)
                .or_else(|| lease.scope().unwrap().person().map(str::to_owned))
        });
            let told = match &route {
                Some(route) if crate::remember::is_tool(&name) => {
                    let call = lamp_gemini::ToolCall {
                        id: String::new(),
                        name,
                        args,
                    };
                    crate::tools::run_call_scoped(route, &call, request, heard, &lease)
                        .1
                        .0
                }
                _ if doubt_undoes && name == crate::remember::REMEMBER => {
                    json!({"remembered": true, "id": 391})
                }
                _ if keep_marks.is_some() && name == crate::remember::REMEMBER => {
                    if let (Some(id), Some(person)) = (keep_marks, lease.kept_by_one_face()) {
                        crate::tools::one_face_kept_for_test(lease.key(), person, Some(id));
                    }
                    json!({"remembered": true, "id": keep_marks})
                }
                _ if doubt_undoes && name == crate::remember::FORGET => json!({"forgotten": true}),
                _ => json!({"ok":true}),
            };
            Box::pin(async move { told })
        },
    );
    // The lease each context was told for: a held line asks memory nothing,
    // so it never reaches `remembering` below.
    let told_leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let seen_told = told_leases.clone();
    let engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([41; 16]).unwrap(),
        cache.clone(),
        runner,
        Some(Arc::new(move |lease: &ScopeLease| {
            seen_told.lock().unwrap().push(lease.clone());
            Told::from(format!("for {:?}", lease.scope().unwrap().person()))
        })),
    );
    let engine = if eyes_up {
        engine
            .seeing_who_speaks(eyes.clone(), path.clone())
            .scoping_by_sight(seen)
    } else {
        failed_reader(engine, &path, seen)
    };
    let stall = FORGET_STALL.with(|stall| stall.borrow_mut().take());
    let engine = match FORGET_TARGET.with(std::cell::Cell::take) {
        Some((id, text)) => engine.targeting_one_face_forgets(Arc::new(
            move |call: &net::ToolCall, said: &str, _: &str, _: &ScopeLease| {
                if let Some((wait, marks)) = &stall {
                    let began = Instant::now();
                    std::thread::sleep(*wait);
                    marks.lock().unwrap().extend([began, Instant::now()]);
                }
                (said.contains("umbrella")
                    || call.args["about"]
                        .as_str()
                        .is_some_and(|about| about.contains("umbrella")))
                .then(|| (id, text.to_owned()))
                .ok_or(crate::remember::ForgetNotAsked::NoMatch(
                    crate::remember::ForgetWant::About("umbrella".to_owned()),
                ))
            },
        )),
        None => engine,
    };
    let engine = engine
        .undoing_doubted_keeps(doubt_undoes)
        .naming_by_one_face(name_one_face)
        .reading_by_one_face(ONE_FACE_READS.with(|reads| reads.replace(false)))
        .in_language(LANGUAGE.with(std::cell::Cell::take));
    let engine = if voice == Voice::Off {
        engine
    } else {
        let another = Arc::new(AtomicBool::new(voice == Voice::AnotherBefore));
        let heard = another.clone();
        let engine = engine.scoping_by_one_face(another);
        if voice == Voice::AnotherInIt {
            // The rule's judging of this line hears another voice.
            engine.heeding(Arc::new(move |_, _, _, _, _| {
                heard.store(true, Ordering::Release);
                None
            }))
        } else if voice == Voice::Unchecked || voice == Voice::Judged {
            // The rule judges this line with no voice print to hear by.
            engine.heeding(Arc::new(|_, _, _, _, _| None))
        } else {
            engine
        }
    };
    let a = crate::remember::person_id("A");
    let kept_thread = thread.then(|| Arc::new(Mutex::new(crate::thread::Thread::new())));
    let engine = engine.carrying_conversations_by(match &kept_thread {
        Some(kept) => {
            let kept = kept.clone();
            Some(Arc::new(move |request: u64, asked| {
                kept.lock()
                    .unwrap()
                    .going_on_asked(request, Instant::now(), asked)
            }))
        }
        None => {
            let from = GOING_ON_FROM.with(|from| from.replace(0));
            going_on.map(|going_on| -> crate::second_provider::GoingOnWith {
                Arc::new(move |request, _| {
                    (going_on && request >= from).then(|| (a.clone(), false))
                })
            })
        }
    });
    let session_on = SESSION.with(|on| on.replace(false));
    let mut engine = engine
        .saving_carried(saves)
        .reading_carried(CARRIED_MEMORY.with(|on| on.replace(false)))
        .carrying_own_track(OWN_TRACK.with(|on| on.replace(false)))
        .holding_sticky(STICKY.with(|on| on.replace(false)))
        .keeping_session_person(session_on)
        .reading_carried_where_heard(!CARRIED_UNHEARD.with(|unheard| unheard.replace(false)));
    if let Some(found) = DONT_KNOW_FOUND.with(|found| found.borrow_mut().take()) {
        engine = engine.recalling_before_a_dont_know(found);
    }
    if let Some((hears, another)) = past_silent {
        engine = engine.carrying_past_silent_where_heard(true, hears, another);
    }
    if saves {
        engine = engine.confirming_forgets_by_sight(eyes.clone(), path.clone());
    }
    let mut engine = engine
        .remembering(Arc::new(
            move |_, words: &str, lease: Option<&ScopeLease>| {
                seen_noted.lock().unwrap().push((
                    lease
                        .and_then(|lease| lease.scope())
                        .and_then(|scope| scope.person().map(str::to_owned)),
                    words.to_owned(),
                ));
                seen_leases.lock().unwrap().extend(lease.cloned());
                seen_hits
                    .lock()
                    .unwrap()
                    .push(lease.and_then(super::super::worker::hits_of_lease));
                None
            },
        ))
        .announcing(false);
    engine.memory_camera(true, 1);
    // With the session, a lamp running a while: past the cache's cold
    // start, so a room's session may begin.
    if session_on {
        cache.cold_since(Instant::now() - Duration::from_secs(60));
    }
    let named_lines = NAMED_LINES.with(std::cell::Cell::take);
    let privacy_at = PRIVACY_AT.with(|at| at.replace(0));
    if named {
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
    }
    let mut labels = Vec::new();
    // On the real route the voice's thread is the crate's own, shared by
    // every test: requests of this test's own.
    // A cut's lines too: what each line was settled as is kept by request
    // across the crate (`memory_scope::said_line`).
    static CUT_BASE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let base = match (real, cut_first) {
        (Some(real), _) => real.base,
        (None, Some(_)) => 9_240_000 + 10 * CUT_BASE.fetch_add(1, Ordering::Relaxed),
        (None, None) => REQUEST_BASE.with(|base| base.replace(0)),
    };
    let murmured = MURMUR_MOUTHS
        .with(std::cell::Cell::take)
        .unwrap_or(S4_OPENING_MOUTHS);
    for (at, (_, in_view, after)) in lines.into_iter().enumerate() {
        let line = at as u64 + 1;
        let request = base + line;
        // The line before is over; the last stays current, for its lease.
        if line > 1 {
            engine.retire_through(request - 1);
        }
        *now.lock().unwrap() += Duration::from_secs(after);
        known_at(line, &path);
        if privacy_at == line {
            engine.memory_camera(false, 1);
        }
        if let Some(named_lines) = named_lines {
            let observation = named_lines(line)
                .and_then(|held| Some((crate::known_faces::FaceObservation::read(&path)?, held)));
            let held = observation.as_ref().is_some_and(|(_, held)| *held);
            cache.replace_marked(
                cache.begin_refresh(),
                observation.map(|(observation, _)| observation),
                None,
                held,
            );
        }
        let done = Arc::new(AtomicBool::new(false));
        // S4's opening line: her mouth read speaking in one look of four,
        // unread in the rest (lamp-06c1, 5 Nov: read speaking in 1 of 9).
        let mouths = if murmur == Some(line) {
            murmured
        } else {
            mouths
        };
        let feeder = feed_faces(eyes.clone(), in_view, mouths, boot, done.clone());
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        match cut_first {
            // Line 1, until its held sentence's text is out, never played:
            // the next line cuts it short.
            Some((sentence, _)) if line == 1 => {
                engine.submit(end(request));
                loop {
                    let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
                        .await
                        .expect("line 1's text comes");
                    if matches!(&event, ProviderOutput::Transcript { request: Some(of), text, .. } if *of == request && text.trim() == sentence)
                    {
                        break;
                    }
                }
                while tokio::time::timeout(Duration::from_millis(100), engine.next())
                    .await
                    .is_ok()
                {}
            }
            Some((_, sound)) if line == 2 => {
                engine.submit(ProviderInput::End {
                    request,
                    privacy_generation: 1,
                    sound,
                    cut_in: false,
                });
                labels.extend(turn(&mut engine).await);
            }
            _ => {
                engine.submit(end(request));
                labels.extend(turn(&mut engine).await);
            }
        }
        done.store(true, Ordering::Release);
        feeder.join().unwrap();
        // The line's answer kept in the thread as the worker keeps it.
        if let Some(kept) = &kept_thread {
            let told = told_leases
                .lock()
                .unwrap()
                .iter()
                .find(|lease| lease.key().request == request)
                .cloned();
            if let Some(lease) = told
                && let Some(scope) = lease.scope()
            {
                kept.lock().unwrap().lamp_for_named(
                    request,
                    "Answered.",
                    Instant::now(),
                    scope.person(),
                    lease.named_for_conversation(),
                );
                // The lamp's own words in the voice's thread, as the worker
                // keeps them: what memory's route reads for a yes.
                if let Some(said) = real.and_then(|real| real.said.get(at)) {
                    crate::thread::THREAD.lock().unwrap().lamp_for_named(
                        request,
                        said,
                        Instant::now(),
                        scope.person(),
                        lease.named_for_conversation(),
                    );
                }
            }
        }
    }
    clean(&path);
    // The line's lease: as its context was told, else as memory was asked.
    let first = told_leases
        .lock()
        .unwrap()
        .first()
        .cloned()
        .or_else(|| leases.lock().unwrap().first().cloned());
    let lease = first.as_ref().and_then(|lease| {
        lease
            .scope()
            .and_then(|scope| scope.person().map(str::to_owned))
    });
    let held = first
        .as_ref()
        .and_then(ScopeLease::scope)
        .is_some_and(crate::memory_scope::ScopeSnapshot::held);
    // Whose name the reply's face name is told, as the worker picks it.
    let named_for_reply = first.as_ref().and_then(|lease| {
        super::super::worker::known_person_for(Some(lease), super::super::worker::Chosen::ByLease)
    });
    let rounds = script.asked.lock().unwrap().clone();
    let asked: Vec<Message> = rounds.iter().map(|chat| chat[0].clone()).collect();
    Outcome {
        calls: calls.lock().unwrap().clone(),
        lease,
        told: asked[0].clone(),
        noted: noted.lock().unwrap().clone(),
        asked,
        labels,
        held,
        rounds,
        hits: hits.lock().unwrap().clone(),
        named_for_reply,
        ran: ran.lock().unwrap().clone(),
    }
}

fn forget_everything() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                crate::remember::FORGET_EVERYTHING,
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done."), Ok(ChatEvent::Done)],
    ]
}

const FORGET_ME: &str = "Lamp, forget everything about me.";

/// The owner's face in view and silent; an off-camera voice asks the lamp
/// to forget everything about "me". The forget runs in the room's scope,
/// where `forget_everything` refuses (`remember.rs`, nobody known), never
/// in the owner's: their memory is untouched (reviewer-three's P1).
#[tokio::test]
async fn an_off_camera_forget_everything_never_reaches_the_silent_owners_memory() {
    let got = one_line("forget", FORGET_ME, forget_everything(), false, BOOT, true).await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// An off-camera colleague's "I'm flying to Tokyo next week" with the owner
/// silent in view: told as the room's and its lease the room's, which is
/// the scope its exchange is kept and observed under
/// (`FinishedExchange::whose`), never the owner's (reviewer-three's P2).
#[tokio::test]
async fn an_off_camera_line_is_not_observed_or_kept_as_the_silent_owners() {
    let chats = vec![vec![text("Have a good trip."), Ok(ChatEvent::Done)]];
    let got = one_line(
        "tokyo",
        "I'm flying to Tokyo next week.",
        chats,
        false,
        BOOT,
        true,
    )
    .await;
    assert_eq!(got.lease, None, "the line's lease is the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// The owner's own face seen speaking the line keeps it theirs.
#[tokio::test]
async fn the_owners_own_face_seen_speaking_keeps_their_scope() {
    let a = crate::remember::person_id("A");
    let got = one_line("owner", FORGET_ME, forget_everything(), true, BOOT, true).await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(
        got.told,
        Message::system(format!("for {:?}", Some(a.as_str())))
    );
}

/// Second's P1 on #3437: A's fresh files name `f1` from before the eyes
/// started again, and the new eyes' `f1`, a different face, says "forget
/// everything about me". The bare id matches, the boot does not: the line
/// is the room's. Off, it stays A's, as before.
#[tokio::test]
async fn a_reused_id_after_the_eyes_restart_is_not_the_known_persons_face() {
    let a = crate::remember::person_id("A");
    let got = one_line(
        "restart",
        FORGET_ME,
        forget_everything(),
        true,
        "restarted",
        true,
    )
    .await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.lease, None);
    assert_eq!(got.told, Message::system("for None"));
    let got = one_line(
        "restart-off",
        FORGET_ME,
        forget_everything(),
        true,
        "restarted",
        false,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

/// Reviewer-two's P2 on #3437: the eyes' reader never started (the bus
/// would not open), so nothing reads who spoke. With the switch that is no
/// face seen speaking it: the line is the room's. Off, it stays A's.
#[tokio::test]
async fn with_no_eyes_to_read_a_line_is_the_rooms() {
    let a = crate::remember::person_id("A");
    let got = one_line_eyes(
        "blind",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        true,
        false,
        None,
    )
    .await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.lease, None);
    let got = one_line_eyes(
        "blind-off",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        false,
        false,
        None,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

const TOKYO: &str = "I'm flying to Tokyo next week.";
/// The scripted voice's audio label for the reply.
const AUDIO: &str = "audio:17";
const TOKYO_EARLY: &str = "i'm flying to tokyo next week";

/// A chat that answers in words, enough for a prepared round and another.
fn answers() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![text("Have a good trip."), Ok(ChatEvent::Done)],
        vec![text("Have a good trip."), Ok(ChatEvent::Done)],
    ]
}

/// The scripted reply said once, its audio played, and the turn complete,
/// not failed.
fn spoken(got: &Outcome, why: &str) {
    let said: Vec<&String> = got
        .labels
        .iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    assert_eq!(said, ["said:Have a good trip."], "{why}: {:?}", got.labels);
    assert!(
        got.labels.iter().any(|label| label == AUDIO),
        "{why}: its audio, {:?}",
        got.labels
    );
    assert_eq!(
        got.labels.last().map(String::as_str),
        Some("complete"),
        "{why}: {:?}",
        got.labels
    );
}

/// One line with `LAMP_SECOND_PREPARED` and early words.
async fn prepared_line(what: &str, f1_speaks: bool, seen: bool) -> Outcome {
    one_line_eyes(
        what,
        TOKYO,
        answers(),
        f1_speaks,
        BOOT,
        seen,
        true,
        Some(TOKYO_EARLY),
    )
    .await
}

/// Second's P2 on #3437: with `LAMP_SECOND_PREPARED`, the early words
/// would ask memory and a chat told the owner's context before the line's
/// end shows their face never spoke it. With the switch nothing is asked
/// for them early: memory and the one chat are the room's, on the final
/// words, and the room's answer comes.
#[tokio::test]
async fn an_off_camera_lines_early_words_ask_nothing_for_the_silent_owner() {
    let got = prepared_line("early-silent", false, true).await;
    assert_eq!(
        got.noted,
        [(None, TOKYO.to_owned())],
        "memory, the room's, on the final words"
    );
    assert_eq!(
        got.asked,
        [Message::system("for None")],
        "one chat, the room's"
    );
    assert_eq!(got.lease, None);
    spoken(&got, "the room's answer comes");
}

/// The owner seen speaking: not prepared early either, and their answer is
/// asked on the final words in their scope.
#[tokio::test]
async fn the_owner_seen_speaking_is_answered_as_theirs_on_the_final_words() {
    let a = crate::remember::person_id("A");
    let got = prepared_line("early-owner", true, true).await;
    assert_eq!(got.noted, [(Some(a.clone()), TOKYO.to_owned())]);
    assert_eq!(
        got.asked,
        [Message::system(format!("for {:?}", Some(a.as_str())))]
    );
    spoken(&got, "answered as theirs");
}

/// Off, the early words prepare as before: memory asked on them for the
/// owner, and the prepared round stands.
#[tokio::test]
async fn without_the_switch_the_early_words_still_prepare() {
    let a = crate::remember::person_id("A");
    let got = prepared_line("early-off", false, false).await;
    assert_eq!(got.noted, [(Some(a.clone()), TOKYO_EARLY.to_owned())]);
    assert_eq!(
        got.asked,
        [Message::system(format!("for {:?}", Some(a.as_str())))]
    );
}

/// Off, nothing changes: the silent owner's line stays theirs, as on
/// lamp-voice before the switch.
#[tokio::test]
async fn without_the_switch_an_off_camera_line_stays_the_silent_owners() {
    let a = crate::remember::person_id("A");
    let got = one_line("off", FORGET_ME, forget_everything(), false, BOOT, false).await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(got.lease, Some(a.clone()));
    let got = one_line(
        "owner-off",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        false,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

/// Second's case at 09:24 on #3435: after a restart the cache is cold, so
/// the line is admitted as the room's and looks later. The owner's face is
/// in view and silent from the line's start, their identity arrives within
/// the keeper bound, and an off-camera voice asks for a keep: the late
/// identity alone gives no keeper with the switch, and does without it.
#[tokio::test]
async fn a_late_looks_keeper_needs_the_owners_own_face_seen_speaking() {
    let a = crate::remember::person_id("A");
    for (what, f1_speaks, boot, seen, eyes_up, expected) in [
        ("silent, on", false, BOOT, true, true, None),
        ("silent, off", false, BOOT, false, true, Some(a.clone())),
        ("speaking, on", true, BOOT, true, true, Some(a.clone())),
        // Second's P1 on #3437: the eyes started again and their new `f1`,
        // a different face, says it.
        ("restarted, on", true, "restarted", true, true, None),
        (
            "restarted, off",
            true,
            "restarted",
            false,
            true,
            Some(a.clone()),
        ),
        // Reviewer-two's P2 on #3437: the eyes' reader never started.
        ("no eyes, on", true, BOOT, true, false, None),
        ("no eyes, off", true, BOOT, false, false, Some(a.clone())),
    ] {
        let eyes = Arc::new(SpeakingFaces::default());
        let path = known_file("late", &eyes);
        let script = Arc::new(Script {
            heard: Mutex::new(
                [Ok(
                    "Lamp, remember my sister's wedding is Saturday.".to_owned()
                )]
                .into(),
            ),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "m1",
                            "remember",
                            json!({"text":"sister's wedding is Saturday"}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("Got it."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let cache = Arc::new(FaceCache::default());
        let keepers: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let kept = keepers.clone();
        let runner: ScopedToolRunner = Arc::new(move |_, _, _, _, lease: ScopeLease| {
            let kept = kept.clone();
            Box::pin(async move {
                let keeper = tokio::task::spawn_blocking(move || lease.keeper_within(KEEPER_WAIT))
                    .await
                    .unwrap();
                kept.lock().unwrap().push(keeper);
                json!({"ok":true})
            })
        });
        let engine = engine.with_memory_scope(
            lamp_turn_owner::BootId::new([42; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        );
        let engine = if eyes_up {
            engine
                .seeing_who_speaks(eyes.clone(), path.clone())
                .scoping_by_sight(seen)
        } else {
            failed_reader(engine, &path, seen)
        };
        let mut engine = engine.announcing(false);
        engine.memory_camera(true, 1);
        // Cold: the voice restarted a second ago and nobody is named yet.
        cache.cold_since(Instant::now() - Duration::from_secs(1));
        assert!(cache.is_cold(Instant::now()), "{what}: cold");
        let done = Arc::new(AtomicBool::new(false));
        let feeder = feed(eyes, f1_speaks, boot, done.clone());
        engine.submit(start(1));
        let lease = engine
            .turn
            .as_ref()
            .unwrap()
            .memory_scope
            .as_ref()
            .unwrap()
            .lease();
        assert_eq!(lease.scope().unwrap().person(), None, "{what}: the room's");
        assert!(lease.looks_later(), "{what}");
        // The eyes name A while the line is said.
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        engine.submit(audio(1, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(1));
        turn(&mut engine).await;
        done.store(true, Ordering::Release);
        feeder.join().unwrap();
        clean(&path);
        assert_eq!(*keepers.lock().unwrap(), [expected], "{what}");
    }
}

/// #4339, lamp-3e83: A's line no face was seen speaking, A's own face in
/// view and silent, in A's conversation going on (`LAMP_CONVERSATION_CARRIES`):
/// told from A's conversation as it stood, a held scope, not the room's,
/// so the chat keeps the talk it was part of. With no face in view at all
/// from the start, it is the room's: an unseen line carries only within
/// `CARRY_UNSEEN_WITHIN` of their face in view (the Reviewer-lead's P2 on
/// #4479).
#[tokio::test]
async fn an_unseen_line_in_their_conversation_going_on_keeps_that_conversation() {
    let a = crate::remember::person_id("A");
    for (what, in_view) in [("carried", vec![("f1", false)])] {
        let chats = vec![vec![text("Floral, then."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
        )
        .await;
        assert_eq!(
            got.lease,
            Some(a.clone()),
            "{what}: A's conversation, not the room's"
        );
        assert!(got.held, "{what}: told from the conversation as it stood");
        assert_eq!(
            got.told,
            Message::system(format!("for {:?}", Some(a.as_str()))),
            "{what}"
        );
    }
}

/// #4339: carried, a memory call on that line runs for no one, as a
/// gap-held line's does: an off-camera "forget everything about me" reaches
/// nobody's memory (reviewer-three's P1 on #3435 holds).
#[tokio::test]
async fn a_carried_lines_memory_calls_run_for_no_one() {
    let got = one_line_with(
        "carried-forget",
        FORGET_ME,
        forget_everything(),
        vec![("f1", false)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert!(got.calls.is_empty(), "no memory call runs: {:?}", got.calls);
    assert!(got.held);
}

/// #4339: a line no face was seen speaking stays the room's, as before,
/// when A isn't in a conversation going on (a 45 s quiet, or someone
/// else's exchange between), with the switch off, with a face not A's in
/// view, and with another face seen speaking it.
#[tokio::test]
async fn without_their_conversation_or_with_another_face_an_unseen_line_is_the_rooms() {
    for (what, in_view, going_on) in [
        ("quiet", vec![("f1", false)], Some(false)),
        ("off", vec![("f1", false)], None),
        ("empty-from-start", vec![], Some(true)),
        ("other-face", vec![("f2", false)], Some(true)),
        ("two-faces", vec![("f1", false), ("f2", false)], Some(true)),
        (
            "other-speaks",
            vec![("f1", false), ("f2", true)],
            Some(true),
        ),
    ] {
        let chats = vec![vec![text("Okay."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            true,
        )
        .await;
        assert_eq!(got.lease, None, "{what}: the room's");
        assert!(!got.held, "{what}");
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
}

/// #4339: A's own face seen speaking it is A's as ever, with memory, not
/// merely carried.
#[tokio::test]
async fn their_face_seen_speaking_is_theirs_not_carried() {
    let a = crate::remember::person_id("A");
    let chats = vec![vec![text("Floral."), Ok(ChatEvent::Done)]];
    let got = one_line_with(
        "own",
        "The floral one, I think.",
        chats,
        vec![("f1", true)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(!got.held, "seen speaking: theirs, with memory");
}

/// #4339, lamp-06c1's turns with no face line (29 Oct): a line admitted as
/// the room's, the face cache naming nobody, in A's conversation going on,
/// with only A's face in view and none seen speaking it: carried as A's
/// conversation, held. Not in one, with an empty view from the start, or
/// with another face in view or speaking it, it stays the room's.
#[tokio::test]
async fn a_rooms_line_in_their_conversation_going_on_is_carried_as_it() {
    let a = crate::remember::person_id("A");
    for (what, in_view, going_on, carried) in [
        ("room-own", vec![("f1", false)], Some(true), true),
        ("room-none", vec![], Some(true), false),
        ("room-quiet", vec![("f1", false)], Some(false), false),
        ("room-off", vec![("f1", false)], None, false),
        ("room-other", vec![("f2", false)], Some(true), false),
        ("room-other-speaks", vec![("f2", true)], Some(true), false),
    ] {
        let chats = vec![vec![text("Okay."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            false,
        )
        .await;
        if carried {
            assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
            assert!(got.held, "{what}: held, never memory's");
        } else {
            assert_eq!(got.lease, None, "{what}: the room's");
            assert!(!got.held, "{what}");
        }
    }
}

/// One line of A's conversation going on, the faces `in_view`, with
/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT` and `another` as the
/// conversation's sign of another voice, the same-voice check running.
async fn past_silent_line(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    named: bool,
    another: crate::second_provider::AnotherVoice,
) -> Outcome {
    past_silent_line_hearing(what, in_view, named, true, another).await
}

/// [`past_silent_line`] with `hears` as whether the same-voice check runs.
async fn past_silent_line_hearing(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    named: bool,
    hears: bool,
    another: crate::second_provider::AnotherVoice,
) -> Outcome {
    lines_past_silent(
        what,
        vec![("The floral one, I think.".to_owned(), in_view, 0)],
        vec![vec![text("Floral, then."), Ok(ChatEvent::Done)]],
        BOOT,
        true,
        true,
        None,
        Some(true),
        named,
        false,
        Some((hears, another)),
        Voice::Off,
    )
    .await
}

/// #4339, lamp-3e83 on 30 Oct (`LAMP_CONVERSATION_CARRIES_PAST_SILENT`): the
/// known face in all 30 looks of a line and none with it alone, so a line no
/// face was seen speaking went to the room's. A silent second face doesn't
/// count: carried as A's conversation, held, whether admitted as A's or the
/// room's. A second face seen speaking it, another voice in the
/// conversation, or no face of A's in view from the start, and it is the
/// room's, as before.
#[tokio::test]
async fn a_silent_second_face_does_not_keep_their_conversations_line_the_rooms() {
    let a = crate::remember::person_id("A");
    let quiet: crate::second_provider::AnotherVoice = Arc::new(|| false);
    let voice: crate::second_provider::AnotherVoice = Arc::new(|| true);
    for (what, in_view, named, another, carried) in [
        (
            "silent-guest",
            vec![("f1", false), ("f2", false)],
            true,
            &quiet,
            true,
        ),
        (
            "silent-guests",
            vec![("f1", false), ("f2", false), ("f3", false)],
            true,
            &quiet,
            true,
        ),
        (
            "silent-guest-room",
            vec![("f1", false), ("f2", false)],
            false,
            &quiet,
            true,
        ),
        ("alone", vec![("f1", false)], true, &quiet, true),
        (
            "guest-speaks",
            vec![("f1", false), ("f2", true)],
            true,
            &quiet,
            false,
        ),
        (
            "guest-speaks-room",
            vec![("f1", false), ("f2", true)],
            false,
            &quiet,
            false,
        ),
        (
            "another-voice",
            vec![("f1", false), ("f2", false)],
            true,
            &voice,
            false,
        ),
        (
            "another-voice-alone",
            vec![("f1", false)],
            true,
            &voice,
            false,
        ),
        (
            "another-voice-room",
            vec![("f1", false)],
            false,
            &voice,
            false,
        ),
        ("guest-only", vec![("f2", false)], true, &quiet, false),
        ("empty-from-start", vec![], true, &quiet, false),
    ] {
        let got = past_silent_line(what, in_view, named, another.clone()).await;
        if carried {
            assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
            assert!(got.held, "{what}: held, never memory's");
        } else {
            assert_eq!(got.lease, None, "{what}: the room's");
            assert!(!got.held, "{what}");
            assert_eq!(got.told, Message::system("for None"), "{what}");
        }
    }
}

/// #4339 (`LAMP_CONVERSATION_CARRIES_PAST_SILENT`): a line carried past a
/// silent face is judged again once the rule has heard its voice; another
/// voice shown by then, and it is the room's before anything is told.
#[tokio::test]
async fn a_line_carried_past_a_silent_face_is_the_rooms_once_another_voice_shows() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = asked.clone();
    // No other voice as the line is heard; one once the rule has judged it.
    let another: crate::second_provider::AnotherVoice =
        Arc::new(move || counted.fetch_add(1, Ordering::AcqRel) > 0);
    let got = past_silent_line(
        "late-voice",
        vec![("f1", false), ("f2", false)],
        true,
        another,
    )
    .await;
    assert!(
        asked.load(Ordering::Acquire) >= 2,
        "asked again after the rule"
    );
    assert_eq!(got.lease, None, "the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// The Reviewer-lead's P2 on #4610: with the switch on but the same-voice
/// check not running (no printer started, a standby beside Live, none of
/// its switches on), nothing could show another voice, so a silent second
/// face still keeps the line the room's, as before the switch; alone, the
/// line is carried as before. The rule keeping no sign is the same.
#[tokio::test]
async fn without_the_same_voice_check_a_silent_second_face_still_blocks_the_carry() {
    let a = crate::remember::person_id("A");
    let quiet: crate::second_provider::AnotherVoice = Arc::new(|| false);
    let got = past_silent_line_hearing(
        "silent-guest-unheard",
        vec![("f1", false), ("f2", false)],
        true,
        false,
        quiet.clone(),
    )
    .await;
    assert_eq!(got.lease, None, "the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"));
    let got =
        past_silent_line_hearing("alone-unheard", vec![("f1", false)], true, false, quiet).await;
    assert_eq!(got.lease, Some(a), "carried as before the switch");
    assert!(got.held);
    assert!(crate::second_provider::past_silent_inactive(true, false).is_some());
    assert!(crate::second_provider::past_silent_inactive(false, true).is_some());
    assert!(crate::second_provider::past_silent_inactive(false, false).is_some());
    assert!(crate::second_provider::past_silent_inactive(true, true).is_none());
}

/// The Reviewer-lead's P3 on #4610: a line the rule skipped (held, empty,
/// its own echo) is judged again on its own result, not on the sign the
/// line before left: unknown, so not carried, and the stale sign never
/// asked. A line the rule judged reads the sign; no rule, the sign alone.
#[test]
fn a_line_the_rule_skipped_is_not_carried_on_the_line_befores_sign() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = asked.clone();
    let quiet: crate::second_provider::AnotherVoice = Arc::new(move || {
        counted.fetch_add(1, Ordering::AcqRel);
        false
    });
    let voice: crate::second_provider::AnotherVoice = Arc::new(|| true);
    let undone = crate::second_provider::carried_past_silent_undone;
    assert!(undone(true, false, &quiet).is_some(), "skipped: the room's");
    assert_eq!(
        asked.load(Ordering::Acquire),
        0,
        "the stale sign never read"
    );
    assert!(
        undone(true, true, &quiet).is_none(),
        "judged, no voice: carried"
    );
    assert!(
        undone(true, true, &voice).is_some(),
        "judged, another voice"
    );
    assert!(
        undone(false, false, &quiet).is_none(),
        "no rule: the sign alone"
    );
    assert!(undone(false, false, &voice).is_some());
}

/// #4339 with the new switch off: a silent second face keeps the line the
/// room's, as #4479 shipped it.
#[tokio::test]
async fn without_past_silent_a_silent_second_face_keeps_the_line_the_rooms() {
    let got = one_line_with(
        "silent-guest-off",
        "The floral one, I think.",
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false), ("f2", false)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert_eq!(got.lease, None, "the room's");
    assert!(!got.held);
}

/// A `remember` of "We booked the Pine Hill Hotel" and its answer.
fn keep_the_hotel() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "k1",
                crate::remember::REMEMBER,
                json!({"text": "They booked the Pine Hill Hotel."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Got it."), Ok(ChatEvent::Done)],
    ]
}

const THE_HOTEL: &str = "We booked the Pine Hill Hotel.";

/// #4339, approved 30 Oct (`LAMP_CONVERSATION_CARRIED_SAVES`), with the
/// first builder's call on #4508: A's line no face was seen speaking, in
/// A's conversation going on, keeps nothing at once, its own line or one
/// admitted as the room's alike, the scope still held. With the switch its
/// keep is held for A's yes, and the model is told it is not kept yet and
/// to ask; off, it runs for no one, as before.
#[tokio::test]
async fn a_carried_lines_keep_is_held_never_written_at_once() {
    let a = crate::remember::person_id("A");
    for (what, named, saves) in [
        ("save-own", true, true),
        ("save-room", false, true),
        ("save-own-off", true, false),
        ("save-room-off", false, false),
    ] {
        let got = one_line_saving(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            BOOT,
            true,
            true,
            None,
            Some(true),
            named,
            saves,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: nothing written");
        assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
        assert!(got.held, "{what}: still held, no reads");
        assert!(
            got.noted.is_empty(),
            "{what}: no note read: {:?}",
            got.noted
        );
        let told = if saves {
            crate::remember::held_keep_told().to_string()
        } else {
            json!({ "error": HELD_MEMORY_CALL }).to_string()
        };
        assert!(
            got.rounds[1].contains(&Message::tool("k1".to_owned(), told)),
            "{what}: {:?}",
            got.rounds[1]
        );
    }
}

/// A held keep's case: its name, its lines, whether the lamp asks, and
/// the scopes the keep is written in.
type HeldCase = (&'static str, Vec<Line>, bool, Vec<Option<String>>);

/// A carried `remember` and the lamp asking about it.
fn keep_the_hotel_asking() -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = keep_the_hotel();
    chats[1] = vec![text("Want me to keep that for you?"), Ok(ChatEvent::Done)];
    chats
}

/// The first builder's call on #4508, with Reviewer-three's 14:46: a
/// carried line's held keep is written in A's scope only on A's plain yes,
/// their face seen saying it, and only after the lamp asked. A's face in
/// view and silent through the line, then through a yes (a guest's off
/// camera), keeps nothing; so does a yes after the conversation ended
/// (past `HELD_FORGET_FOR`), another face seen saying it, a forget between,
/// or a reply that never asked. Never another scope.
#[tokio::test]
async fn a_held_carried_keep_is_written_only_on_their_seen_yes() {
    let a = crate::remember::person_id("A");
    let quiet = || vec![("f1", false)];
    let yes = |in_view, after| ("Yes.".to_owned(), in_view, after);
    let hotel = || (THE_HOTEL.to_owned(), vec![("f1", false)], 0);
    let okay = || vec![text("Okay."), Ok(ChatEvent::Done)];
    let cases: Vec<HeldCase> = vec![
        (
            "held-seen-yes",
            vec![hotel(), yes(vec![("f1", true)], 5)],
            true,
            vec![Some(a.clone())],
        ),
        (
            "held-quiet-yes",
            vec![hotel(), yes(quiet(), 5)],
            true,
            vec![],
        ),
        (
            "held-too-late",
            vec![hotel(), yes(vec![("f1", true)], 61)],
            true,
            vec![],
        ),
        (
            "held-other-speaks",
            vec![hotel(), yes(vec![("f1", false), ("f2", true)], 5)],
            true,
            vec![],
        ),
        (
            "held-forget-between",
            vec![
                hotel(),
                ("Forget it.".to_owned(), vec![("f1", true)], 5),
                yes(vec![("f1", true)], 5),
            ],
            true,
            vec![],
        ),
        (
            "held-never-asked",
            vec![hotel(), yes(vec![("f1", true)], 5)],
            false,
            vec![],
        ),
    ];
    for (what, lines, asks, expected) in cases {
        let mut chats = if asks {
            keep_the_hotel_asking()
        } else {
            keep_the_hotel()
        };
        for _ in 1..lines.len() {
            chats.push(okay());
        }
        let got = lines_saving(
            what,
            lines,
            chats,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, expected, "{what}");
    }
}

/// lamp-06c1, 5 Nov, build 10343, S4 (C54, C55): A opens the conversation,
/// seen saying it; her face leaves view and the eyes stop writing their
/// known file (fresh 3 s), and she says "remember that my car is silver"
/// at once, off camera. The known face read at the line's end named no one,
/// so the line was the room's and its keep refused ("I don't know who's
/// speaking yet"). Her face as last named, gone within
/// `CARRY_UNSEEN_WITHIN` before the line, carries it: the keep is held and
/// written in her scope on her plain yes, seen on her face's new track. Her face
/// still in view while the eyes no longer name it is not hers: the room's.
#[tokio::test]
async fn an_off_camera_keep_just_after_her_face_left_is_carried_and_held_for_her_yes() {
    let a = crate::remember::person_id("A");
    fn left_then_back(request: u64, known: &std::path::Path) {
        match request {
            // Her face gone: the eyes write their known file no more.
            2 => {
                let _ = std::fs::remove_file(known);
                let _ = std::fs::remove_file(known_face_of(known));
            }
            // Back, on a new track.
            3 => {
                std::fs::write(known, "A").unwrap();
                std::fs::write(known_face_of(known), "f2\n").unwrap();
            }
            _ => {}
        }
    }
    let opening = || {
        (
            "I'm going to the garden centre on Saturday.".to_owned(),
            vec![("f1", true)],
            0,
        )
    };
    // A plain yes: S4's own "Yes, please keep that." is not one
    // (`known_faces::plain_agreement`: "keep" stands beside no yes).
    let yes = || ("Yes, please.".to_owned(), vec![("f2", true)], 5);
    for (what, car_in_view, expected) in [
        ("s4-left", vec![], vec![Some(a.clone())]),
        ("s4-unnamed-in-view", vec![("f1", false)], vec![None]),
    ] {
        let mut chats = vec![vec![
            text("Garden centre on Saturday, noted."),
            Ok(ChatEvent::Done),
        ]];
        chats.extend(keep_the_hotel_asking());
        chats.push(vec![text("Okay."), Ok(ChatEvent::Done)]);
        let got = lines_known(
            what,
            vec![
                opening(),
                ("Remember that my car is silver.".to_owned(), car_in_view, 0),
                yes(),
            ],
            chats,
            (BOOT, true, true),
            None,
            Some(true),
            true,
            true,
            None,
            Voice::Off,
            (false, &[]),
            left_then_back,
            (false, None, None),
        )
        .await;
        assert_eq!(got.calls, expected, "{what}");
    }
}

/// lamp-06c1, 5 Nov, S4 rerun on #5448's head: Olivia's opening line, her
/// face alone in view and none seen speaking it, was kept as the room's
/// with its reply told her name (`the known person's face alone in view:
/// kept as the room's, its reply told their name`). Her car line, off
/// camera just after, then found no conversation of hers going on: no
/// carry line, `speaker unknown (skipped=1)`, and her "Yes, please keep
/// that." had nothing held. That room's exchange, named for her face
/// alone, opens her conversation for a keep only: the car line's keep is
/// carried, held, and written in her scope on her seen yes, while the line
/// itself is told as the room's, never her context or thread. A second face in view
/// at the opening (not named for her) or at the car line: no carry, the
/// room's keep.
#[tokio::test]
async fn a_keep_just_after_her_face_alone_opened_as_the_rooms_is_carried_for_her_yes() {
    let a = crate::remember::person_id("A");
    fn left_then_back(request: u64, known: &std::path::Path) {
        match request {
            2 => {
                let _ = std::fs::remove_file(known);
                let _ = std::fs::remove_file(known_face_of(known));
            }
            3 => {
                std::fs::write(known, "A").unwrap();
                std::fs::write(known_face_of(known), "f2\n").unwrap();
            }
            _ => {}
        }
    }
    for (at, (what, opening_in_view, car_in_view, expected)) in [
        (
            "s4-room-named",
            vec![("f1", false)],
            vec![],
            vec![Some(a.clone())],
        ),
        (
            "s4-room-two-faces",
            vec![("f1", false), ("f3", false)],
            vec![],
            vec![None],
        ),
        (
            "s4-room-named-other-face",
            vec![("f1", false)],
            vec![("f3", false)],
            vec![None],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut chats = vec![vec![
            text("Off to the garden center on Saturday, Olivia?"),
            Ok(ChatEvent::Done),
        ]];
        chats.extend(keep_the_hotel_asking());
        chats.push(vec![text("Okay."), Ok(ChatEvent::Done)]);
        // On the voice's own memory route, with the lamp's switches: the
        // held keep's run must reach the memory, not only the runner (C55,
        // lamp-06c1, 6 Nov: the runner ran it, and memory's route kept
        // nothing).
        crate::remember::heard::set_on_for_test(true);
        crate::keep_quietly::set_on_for_test(true);
        let socket = memory_socket(9_905_600 + 10 * at as u64);
        let stop = Arc::new(AtomicBool::new(false));
        let (memory, serving) = memory_at(&socket, stop.clone());
        let real = Real {
            socket: socket.clone(),
            said: vec![
                "Off to the garden center on Saturday, Olivia?",
                "Want me to keep that for you?",
                "Okay.",
            ],
            base: 9_905_600 + 10 * at as u64,
        };
        let got = lines_known(
            what,
            vec![
                (
                    "I'm going to the garden centre on Saturday.".to_owned(),
                    opening_in_view,
                    0,
                ),
                (
                    "Can you keep that my car is in for service?".to_owned(),
                    car_in_view,
                    0,
                ),
                ("Yes, please keep that.".to_owned(), vec![("f2", true)], 5),
            ],
            chats,
            (BOOT, true, true),
            None,
            None,
            true,
            true,
            None,
            Voice::One,
            (false, &[]),
            left_then_back,
            (true, Some(1), Some(&real)),
        )
        .await;
        stop.store(true, Ordering::Release);
        serving.join().unwrap();
        let _ = std::fs::remove_file(&socket);
        crate::remember::heard::set_on_for_test(false);
        crate::keep_quietly::set_on_for_test(false);
        assert_eq!(got.calls, expected, "{what}");
        let memory = memory.lock().unwrap().clone();
        let kept: Vec<&Value> = memory
            .iter()
            .filter(|ask| ask["op"] == "remember")
            .collect();
        assert_eq!(
            kept.len(),
            1,
            "{what}: one keep reached the memory: {memory:?}"
        );
        assert!(
            kept[0].to_string().contains("car is in for service"),
            "{what}: the car line kept: {memory:?}"
        );
        assert_eq!(
            kept[0]["person"] == json!(a),
            expected == [Some(a.clone())],
            "{what}: whose scope: {memory:?}"
        );
        // Line 2's rounds (the keep and its ask) are never told A's context
        // or thread: a conversation opened only by a line named for her
        // carries the keep alone (Heron's P2 on #5448: her partner off
        // camera, the conversation's only voice, looks the same).
        let as_a = Message::system(format!("for {:?}", Some(a.as_str())));
        assert_eq!(got.asked.len(), 4, "{what}: rounds");
        assert!(
            got.asked[1..3].iter().all(|told| *told != as_a),
            "{what}: line 2 told A's context: {:?}",
            &got.asked[1..3]
        );
    }
}

/// lamp-06c1, 6 Nov, kit S4 on #5448's head (C55), replayed on the voice's
/// own memory route with `LAMP_MEMORY_KEEP_HEARD` and `LAMP_KEEP_QUIETLY`
/// on, as on the lamp: Olivia's opening line, her face alone in view, kept
/// as the room's and named for her; her car line just after, her face
/// hidden, carried, its keep held for her yes and the offer told as the
/// room's; then "Yes, please keep that.", her face back and seen saying it.
/// The yes ran the held keep, but memory's route found no offer of hers in
/// the thread (the offer was the room's), took the line for a yes alone and
/// kept nothing: no `remember` reached the memory, and the model was told
/// nothing was waiting ("I don't have a pending question from you to
/// keep"). Her seen yes keeps the car line in her scope, and the model is
/// told it is kept.
#[tokio::test]
async fn her_seen_yes_to_a_carried_keep_writes_the_car_in_her_scope() {
    let a = crate::remember::person_id("A");
    fn hidden_then_back(request: u64, known: &std::path::Path) {
        match request {
            2 => {
                let _ = std::fs::remove_file(known);
                let _ = std::fs::remove_file(known_face_of(known));
            }
            3 => {
                std::fs::write(known, "A").unwrap();
                std::fs::write(known_face_of(known), "f2\n").unwrap();
            }
            _ => {}
        }
    }
    crate::remember::heard::set_on_for_test(true);
    crate::keep_quietly::set_on_for_test(true);
    const OPENING: &str = "Off to the garden center on Saturday, Olivia?";
    const ASKS: &str = "Your car is silver. Should I keep that for you?";
    const KEPT: &str = "Kept: your car is silver.";
    let chats = vec![
        vec![text(OPENING), Ok(ChatEvent::Done)],
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                crate::remember::REMEMBER,
                json!({"text": "Olivia's car is silver."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text(ASKS), Ok(ChatEvent::Done)],
        vec![text(KEPT), Ok(ChatEvent::Done)],
        // Spare: a "Kept." with nothing kept is asked again
        // (`held_keep_ran`), so a failing run reaches the asserts.
        vec![text("Okay."), Ok(ChatEvent::Done)],
    ];
    let socket = memory_socket(9_905_500);
    let stop = Arc::new(AtomicBool::new(false));
    let (asked, serving) = memory_at(&socket, stop.clone());
    let real = Real {
        socket: socket.clone(),
        said: vec![OPENING, ASKS, KEPT],
        base: 9_905_500,
    };
    let got = lines_known(
        "c55-carried-yes",
        vec![
            (
                "I'm going to the garden centre on Saturday.".to_owned(),
                vec![("f1", false)],
                0,
            ),
            ("My car is silver.".to_owned(), vec![], 0),
            ("Yes, please keep that.".to_owned(), vec![("f2", true)], 5),
        ],
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (false, &[]),
        hidden_then_back,
        (true, Some(1), Some(&real)),
    )
    .await;
    stop.store(true, Ordering::Release);
    serving.join().unwrap();
    let _ = std::fs::remove_file(&socket);
    crate::remember::heard::set_on_for_test(false);
    crate::keep_quietly::set_on_for_test(false);
    // The held keep ran once, on her yes, in her scope.
    assert_eq!(got.calls, [Some(a.clone())], "the yes's run");
    let asked = asked.lock().unwrap().clone();
    let kept: Vec<&Value> = asked.iter().filter(|ask| ask["op"] == "remember").collect();
    assert_eq!(kept.len(), 1, "one remember reached the memory: {asked:?}");
    assert_eq!(kept[0]["person"], json!(a), "in her scope: {asked:?}");
    assert!(
        kept[0].to_string().to_lowercase().contains("car is silver"),
        "the car line kept: {asked:?}"
    );
    // The yes's round is told it is kept, never that nothing waited.
    let yes_round = got.rounds.last().unwrap();
    let told: Vec<String> = yes_round
        .iter()
        .map(|message| format!("{message:?}"))
        .collect();
    assert!(
        told.iter().any(
            |told| told.contains("what you asked to keep for them is kept now")
                && told.contains("\\\"remembered\\\":true")
        ),
        "told it is kept: {told:?}"
    );
    assert!(
        told.iter()
            .all(|told| !told.contains("no offer of yours to keep")),
        "never told nothing was waiting: {told:?}"
    );
}

/// lamp-06c1, 6 Nov, stock 10538, S4 requests 291 to 293 (C54, C55), with
/// the product unit's switches (`LAMP_SECOND_NAME_ONE_FACE`,
/// `LAMP_PERSON_SCOPE_ONE_FACE`, `LAMP_CONVERSATION_CARRIED_SAVES`;
/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT` inactive, as the journal's "no
/// other face in view" shows). Olivia's opening (291) had her face alone in
/// 9 of 9 looks, read speaking in 1, still in 7, unread in 1: never
/// `alone_in_view` (still in over half), so named by the one-face count
/// alone, and its room's exchange opened no conversation of hers. Her car
/// line (292) began 21 s later, her face hidden under a second before it:
/// no carry, `speaker unknown (skipped=1)`; and her yes (293), 21 s after
/// that, her face back and seen saying it, had nothing held. Her face alone
/// in view through the opening, its mouth read speaking at least once,
/// opens her conversation for a keep only: the car is held and written in
/// her scope on her seen yes.
#[tokio::test]
async fn stock_s4_carried_keep_on_the_product_switches_writes_the_car_on_her_yes() {
    use lamp_msgs::SpeakingState::{No, Unknown, Yes};
    // Request 291's looks: speaking 1, still 7, unread 1.
    const OPENING_291: &[lamp_msgs::SpeakingState] = &[Yes, No, No, No, No, No, No, No, Unknown];
    let a = crate::remember::person_id("A");
    fn hidden_then_back(request: u64, known: &std::path::Path) {
        match request {
            2 => {
                let _ = std::fs::remove_file(known);
                let _ = std::fs::remove_file(known_face_of(known));
            }
            3 => {
                std::fs::write(known, "A").unwrap();
                std::fs::write(known_face_of(known), "f2\n").unwrap();
            }
            _ => {}
        }
    }
    crate::remember::heard::set_on_for_test(true);
    crate::keep_quietly::set_on_for_test(true);
    const OPENING: &str = "Garden centre on Saturday, Olivia? Nice.";
    const ASKS: &str = "Your car is silver. Should I keep that for you?";
    const KEPT: &str = "Kept: your car is silver.";
    let chats = vec![
        vec![text(OPENING), Ok(ChatEvent::Done)],
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                crate::remember::REMEMBER,
                json!({"text": "Olivia's car is silver."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text(ASKS), Ok(ChatEvent::Done)],
        vec![text(KEPT), Ok(ChatEvent::Done)],
        vec![text("Okay."), Ok(ChatEvent::Done)],
    ];
    let socket = memory_socket(9_905_700);
    let stop = Arc::new(AtomicBool::new(false));
    let (asked, serving) = memory_at(&socket, stop.clone());
    let real = Real {
        socket: socket.clone(),
        said: vec![OPENING, ASKS, KEPT],
        base: 9_905_700,
    };
    MURMUR_MOUTHS.with(|mouths| mouths.set(Some(OPENING_291)));
    let got = lines_known(
        "c54-c55-stock-10538",
        vec![
            (
                "Lamp, I'm going to the garden centre on Saturday.".to_owned(),
                vec![("f1", false)],
                0,
            ),
            (
                "Lamp, remember that my car is silver.".to_owned(),
                vec![],
                21,
            ),
            ("Yes, please keep that.".to_owned(), vec![("f2", true)], 21),
        ],
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (true, &[]),
        hidden_then_back,
        (true, Some(1), Some(&real)),
    )
    .await;
    stop.store(true, Ordering::Release);
    serving.join().unwrap();
    let _ = std::fs::remove_file(&socket);
    crate::remember::heard::set_on_for_test(false);
    crate::keep_quietly::set_on_for_test(false);
    assert_eq!(got.calls, [Some(a.clone())], "the yes's run, in her scope");
    let asked = asked.lock().unwrap().clone();
    let kept: Vec<&Value> = asked.iter().filter(|ask| ask["op"] == "remember").collect();
    assert_eq!(kept.len(), 1, "one remember reached the memory: {asked:?}");
    assert_eq!(kept[0]["person"], json!(a), "in her scope: {asked:?}");
    assert!(
        kept[0].to_string().to_lowercase().contains("car is silver"),
        "the car line kept: {asked:?}"
    );
    // The car line's rounds are never told her context or thread.
    let as_a = Message::system(format!("for {:?}", Some(a.as_str())));
    assert!(
        got.asked[1..3].iter().all(|told| *told != as_a),
        "line 2 told A's context: {:?}",
        &got.asked[1..3]
    );
}

/// [`stock_s4_carried_keep_on_the_product_switches_writes_the_car_on_her_yes`]'s
/// guard: her lone face named by the one-face count but never read
/// speaking (still or unread in every look), no other voice heard, opens no
/// conversation of hers: the car line's keep is the room's, never held for
/// her, and never told her context.
#[tokio::test]
async fn a_lone_face_never_read_speaking_opens_no_carried_keep() {
    use lamp_msgs::SpeakingState::{No, Unknown};
    const NEVER_SPEAKING: &[lamp_msgs::SpeakingState] = &[No, No, No, Unknown];
    let a = crate::remember::person_id("A");
    fn left(request: u64, known: &std::path::Path) {
        if request == 2 {
            let _ = std::fs::remove_file(known);
            let _ = std::fs::remove_file(known_face_of(known));
        }
    }
    let mut chats = vec![vec![text("Sure."), Ok(ChatEvent::Done)]];
    chats.extend(keep_the_hotel_asking());
    MURMUR_MOUTHS.with(|mouths| mouths.set(Some(NEVER_SPEAKING)));
    let got = lines_known(
        "c54-never-speaking",
        vec![
            (
                "Lamp, I'm going to the garden centre on Saturday.".to_owned(),
                vec![("f1", false)],
                0,
            ),
            (
                "Lamp, remember that my car is silver.".to_owned(),
                vec![],
                21,
            ),
        ],
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (true, &[]),
        left,
        (true, Some(1), None),
    )
    .await;
    assert_eq!(got.calls, [None], "the room's keep, never held for A");
    let as_a = Message::system(format!("for {:?}", Some(a.as_str())));
    assert!(
        got.asked[1..].iter().all(|told| *told != as_a),
        "line 2 never told A's context"
    );
}

/// Thirteen's P1 on #5448: a guest speaks line 1 off camera while A's
/// face sits alone in view and is named for the reply, by the one-face
/// count (her still face) or by `named_alone` (read speaking in one look
/// of four), and the voice rule hears another voice. The guest's line 2,
/// off camera within 5 s, A's face gone, carries a keep. Line 1 opens no
/// conversation of A's, so line 2 is not carried: its keep runs as the
/// room's (not held for A's yes), no memory hits read A's scope, and it is
/// never told A's context or thread. With
/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT` inactive, active with its sign
/// shown, and active with its sign missed. Also with no voice rule at all
/// (fails closed) and with another voice heard before line 1.
#[tokio::test]
async fn a_guests_line_beside_her_named_lone_face_opens_no_conversation_of_hers() {
    use lamp_msgs::SpeakingState::No;
    const STILL: &[lamp_msgs::SpeakingState] = &[No];
    let a = crate::remember::person_id("A");
    fn left(request: u64, known: &std::path::Path) {
        if request == 2 {
            let _ = std::fs::remove_file(known);
            let _ = std::fs::remove_file(known_face_of(known));
        }
    }
    let shown = |shown: bool| -> Option<(bool, crate::second_provider::AnotherVoice)> {
        Some((true, Arc::new(move || shown)))
    };
    // (what, the one-face count on, line 1's mouths, `named_alone` murmur on
    // line 1, voice, past_silent)
    type PastSilent = Option<(bool, crate::second_provider::AnotherVoice)>;
    type Mouths = &'static [lamp_msgs::SpeakingState];
    type Case = (&'static str, bool, Mouths, Option<u64>, Voice, PastSilent);
    let cases: Vec<Case> = vec![
        (
            "p1-count-voice",
            true,
            STILL,
            None,
            Voice::AnotherInIt,
            None,
        ),
        (
            "p1-count-voice-ps-shown",
            true,
            STILL,
            None,
            Voice::AnotherInIt,
            shown(true),
        ),
        (
            "p1-count-voice-ps-missed",
            true,
            STILL,
            None,
            Voice::AnotherInIt,
            shown(false),
        ),
        (
            "p1-mouth-voice",
            false,
            &[],
            Some(1),
            Voice::AnotherInIt,
            None,
        ),
        (
            "p1-mouth-voice-ps-shown",
            false,
            &[],
            Some(1),
            Voice::AnotherInIt,
            shown(true),
        ),
        (
            "p1-mouth-voice-ps-missed",
            false,
            &[],
            Some(1),
            Voice::AnotherInIt,
            shown(false),
        ),
        (
            "p1-mouth-before",
            false,
            &[],
            Some(1),
            Voice::AnotherBefore,
            None,
        ),
        ("p1-mouth-no-rule", false, &[], Some(1), Voice::Off, None),
    ];
    for (what, name_one_face, mouths, murmur, voice, past_silent) in cases {
        let mut chats = vec![vec![text("Sure."), Ok(ChatEvent::Done)]];
        chats.extend(keep_the_hotel_asking());
        let got = lines_known(
            what,
            vec![
                (
                    "I'll grab a coffee first.".to_owned(),
                    vec![("f1", false)],
                    0,
                ),
                (
                    "Can you keep that my car is in for service?".to_owned(),
                    vec![],
                    0,
                ),
            ],
            chats,
            (BOOT, true, true),
            None,
            None,
            true,
            true,
            past_silent,
            voice,
            (name_one_face, mouths),
            left,
            (true, murmur, None),
        )
        .await;
        assert_eq!(
            got.calls,
            [None],
            "{what}: the room's keep, never held for A"
        );
        // Line 2's hits (line 1's own may fall back to her face with no
        // voice rule: `LAMP_SECOND_MEMORY_HITS`'s accepted risk, unchanged).
        assert!(
            !matches!(got.hits.last(), Some(Some((Some(whose), _))) if *whose == a),
            "{what}: line 2 reads no hits in A's scope: {:?}",
            got.hits
        );
        let as_a = Message::system(format!("for {:?}", Some(a.as_str())));
        assert!(
            got.asked.iter().all(|told| *told != as_a),
            "{what}: never told A's context or thread"
        );
    }
}

/// lamp-06c1, 5 Nov, S4 rerun: the eyes' last look with Olivia's face was
/// 1.7 s before the car line began, and the line ended 5.5 s after it.
/// The window counts from the line's start (`CARRY_UNSEEN_WITHIN`), so her
/// face as last named is the line's, and seen within it. The locker line,
/// her face gone 12 s before it began: neither.
#[tokio::test]
async fn the_carry_window_counts_from_the_lines_start_not_its_end() {
    for (what, gone_before_start, line_for, carries) in [
        ("s4-car-timing", 1.7_f32, 3.8_f32, true),
        ("s4-locker-timing", 12.0, 3.0, false),
    ] {
        let eyes = Arc::new(SpeakingFaces::default());
        let now = Instant::now();
        let span = Duration::from_secs_f32(gone_before_start + line_for + 0.5);
        let t0 = now - span;
        let face = |id: &str| lamp_msgs::Faces {
            faces: vec![lamp_msgs::Face {
                pitch: None,
                looking: None,
                id: id.to_owned(),
                xmin: 0.1,
                ymin: 0.1,
                xmax: 0.3,
                ymax: 0.4,
                confidence: 1.0,
                yaw: None,
                facing: None,
                speaking: Some(lamp_msgs::Speaking {
                    state: lamp_msgs::SpeakingState::No,
                    score: 0.9,
                    from: None,
                }),
                follows: None,
            }],
        };
        let empty = lamp_msgs::Faces { faces: vec![] };
        eyes.look(t0 - Duration::from_millis(200), Some(((BOOT, 0), &empty)));
        eyes.look(t0, Some(((BOOT, 1), &face("f1"))));
        std::thread::sleep(Duration::from_millis(5));
        let known = std::env::temp_dir().join(format!(
            "scope-seen-{what}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&known, "A").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        // Read while her face was named: the pair the line may find.
        assert!(eyes.known_with_boot(&known).is_some(), "{what}");
        clean(&known);
        // The eyes look every 100 ms and see nobody from then on.
        let mut seq = 2;
        let mut at = t0 + crate::speaking_faces::EVERY;
        while at <= now {
            eyes.look(at, Some(((BOOT, seq), &empty)));
            seq += 1;
            at += crate::speaking_faces::EVERY;
        }
        let begun = t0 + Duration::from_secs_f32(gone_before_start);
        let finishing = begun + Duration::from_secs_f32(line_for);
        let (speakers, pair, _) =
            crate::second_provider::line_sight(&eyes, &known, begun, finishing, true).await;
        assert!(speakers.is_empty(), "{what}");
        let her_face = format!("{BOOT}/f1");
        let expected = carries.then(|| ("A".to_owned(), her_face.clone()));
        assert_eq!(pair, expected, "{what}");
        let since = begun
            .checked_sub(crate::second_provider::CARRY_UNSEEN_WITHIN)
            .unwrap();
        assert_eq!(
            eyes.face_seen_between(Some(&her_face), since, finishing),
            carries,
            "{what}: seen within the window from the line's start"
        );
        clean(&known);
    }
}

/// lamp-06c1, 5 Nov, S4: "Yes, please keep that.", A's face seen saying
/// it, writes a carried keep held for A's yes. The same words to a forget
/// A asked for and the lamp held for a yes are no yes: the forget is
/// dropped, as before; a plain "Yes." still runs it.
#[tokio::test]
async fn yes_please_keep_that_writes_a_held_keep_and_runs_no_held_forget() {
    let a = crate::remember::person_id("A");
    const KEEP_THAT: &str = "Yes, please keep that.";
    let okay = || vec![text("Okay."), Ok(ChatEvent::Done)];
    let forget_asking = || {
        let mut chats = forget_everything();
        chats[1] = vec![
            text("Want me to forget everything about you?"),
            Ok(ChatEvent::Done),
        ];
        chats.push(okay());
        chats
    };
    let mut keep_chats = keep_the_hotel_asking();
    keep_chats.push(okay());
    let cases = [
        (
            "keep-that-held-keep",
            (THE_HOTEL, vec![("f1", false)]),
            keep_chats,
            KEEP_THAT,
            vec![Some(a.clone())],
        ),
        (
            "keep-that-held-forget",
            (FORGET_ME, vec![("f1", true)]),
            forget_asking(),
            KEEP_THAT,
            vec![],
        ),
        (
            "plain-yes-held-forget",
            (FORGET_ME, vec![("f1", true)]),
            forget_asking(),
            "Yes.",
            vec![Some(a.clone())],
        ),
    ];
    for (what, (first, in_view), chats, yes, expected) in cases {
        let got = lines_saving(
            what,
            vec![
                (first.to_owned(), in_view, 0),
                (yes.to_owned(), vec![("f1", true)], 5),
            ],
            chats,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, expected, "{what}");
    }
}

/// A near yes's case: its name, its lines, whether each line after the
/// held one is told to ask again, and the scopes the keep is written in.
type AskedAgainCase = (&'static str, Vec<Line>, Vec<bool>, Vec<Option<String>>);

/// lamp-06c1, 5 Nov, S14 run 14:21 (requests 422 and 424): A's carried
/// "my brother lives in Denver" was held and the lamp asked "Want me to keep
/// that for you?"; her "Yes, please keep that." was heard as "Yes. Please
/// keep off.", dropped as not a plain yes, and the fact was never kept. A
/// near yes, seen on her face, writes it in her scope. An answer neither a
/// yes nor a no holds it on, the model is told to ask once more, and her
/// next plain yes writes it; asked once only, so a second unclear answer
/// drops it. A no, however said, drops it, and her yes after runs nothing.
#[tokio::test]
async fn a_near_yes_to_a_held_keep_writes_it_and_an_unclear_one_is_asked_again() {
    let a = crate::remember::person_id("A");
    let hotel = || (THE_HOTEL.to_owned(), vec![("f1", false)], 0);
    let seen = |said: &str| (said.to_owned(), vec![("f1", true)], 5);
    let okay = || vec![text("Okay."), Ok(ChatEvent::Done)];
    let again = || vec![text("Should I keep that?"), Ok(ChatEvent::Done)];
    let told_again = Message::system(crate::remember::KEEP_ASKED_AGAIN.to_owned());
    let cases: Vec<AskedAgainCase> = vec![
        (
            "keep-off",
            vec![hotel(), seen("Yes. Please keep off.")],
            vec![false],
            vec![Some(a.clone())],
        ),
        (
            "keep-it-alone",
            vec![hotel(), seen("Keep it.")],
            vec![false],
            vec![Some(a.clone())],
        ),
        (
            "unclear-then-yes",
            vec![hotel(), seen("Yes. Please key that."), seen("Yes.")],
            vec![true, false],
            vec![Some(a.clone())],
        ),
        (
            "unclear-twice",
            vec![
                hotel(),
                seen("Yes. Please key that."),
                seen("Yes, keep it off."),
                seen("Yes."),
            ],
            vec![true, false, false],
            vec![],
        ),
        (
            "no-then-yes",
            vec![hotel(), seen("No, don't keep it."), seen("Yes.")],
            vec![false, false],
            vec![],
        ),
        (
            "keep-off-no",
            vec![hotel(), seen("No, keep off."), seen("Yes.")],
            vec![false, false],
            vec![],
        ),
    ];
    for (what, lines, asked_again, expected) in cases {
        let mut chats = keep_the_hotel_asking();
        for asks in &asked_again {
            chats.push(if *asks { again() } else { okay() });
        }
        let got = lines_saving(
            what,
            lines,
            chats,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, expected, "{what}");
        // Round 1 and its tool round are the held line's; one round a line
        // after it.
        let told: Vec<bool> = got.rounds[2..]
            .iter()
            .map(|round| round.contains(&told_again))
            .collect();
        assert_eq!(told, asked_again, "{what}: who was told to ask again");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES`: a carried line still
/// cannot forget, one thing or everything; only `remember` runs.
#[tokio::test]
async fn a_carried_line_that_saves_still_cannot_forget() {
    let forget_one = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"text": "the hotel"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done."), Ok(ChatEvent::Done)],
    ];
    for (what, words, chats) in [
        ("saves-forget-all", FORGET_ME, forget_everything()),
        ("saves-forget-one", "Forget the hotel.", forget_one),
    ] {
        let got = one_line_saving(
            what,
            words,
            chats,
            vec![("f1", false)],
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
        )
        .await;
        assert!(
            got.calls.is_empty(),
            "{what}: no forget runs: {:?}",
            got.calls
        );
        assert!(got.held, "{what}");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES`: a line not carried keeps
/// as before, never in A's scope: another face in view, another face seen
/// speaking it, no conversation going on, or no face of A's in view through
/// it or within `CARRY_UNSEEN_WITHIN` before it (an empty view from the
/// start), and it is the room's.
#[tokio::test]
async fn a_line_not_carried_never_saves_into_their_scope() {
    for (what, in_view, going_on) in [
        ("saves-empty-from-start", vec![], Some(true)),
        ("saves-other-face", vec![("f2", false)], Some(true)),
        (
            "saves-two-faces",
            vec![("f1", false), ("f2", false)],
            Some(true),
        ),
        (
            "saves-other-speaks",
            vec![("f1", false), ("f2", true)],
            Some(true),
        ),
        ("saves-quiet", vec![("f1", false)], Some(false)),
    ] {
        let got = one_line_saving(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            true,
            true,
        )
        .await;
        assert_eq!(got.calls, [None], "{what}: the room's keep, never A's");
        assert_eq!(got.lease, None, "{what}");
        assert!(!got.held, "{what}");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES` and the first builder's
/// call on #4508: a carried line is kept out of the journal whether or not
/// it may save, so nothing said off camera is journalled as the carried
/// person's; only a keep they say yes to, seen, is written. Its scope stays
/// held, so its exchange is marked held in the thread (#4479's
/// `Thread::held_for`) and never given to the summary or a talk note.
#[test]
fn a_carried_line_is_kept_out_of_the_journal_whether_or_not_it_saves() {
    use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
    // A person of this test's own: the thread is shared by the crate's tests.
    let a = crate::remember::person_id("Carried Saves A");
    for (request, saves) in [(4_339_101_u64, true), (4_339_102, false)] {
        let scope = RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([43; 16]).unwrap(),
                session: 1,
                request,
            },
            ScopeSnapshot::room(),
        );
        let lease = scope.lease();
        assert!(lease.carry_conversation(&a));
        if saves {
            assert!(lease.save_carried());
        }
        assert_eq!(lease.saves_carried(), saves.then_some(a.as_str()));
        super::super::worker::keep_admitted(
            &ProviderOutput::HoldingAnswer {
                request,
                session: 1,
            },
            &lease,
        );
        assert!(crate::remember::kept_out(request), "{request}");
        assert!(lease.scope().unwrap().held(), "{request}: still held");
        super::super::worker::keep_admitted(
            &ProviderOutput::Transcript {
                request: Some(request),
                text: "Got it, the Pine Hill Hotel.".to_owned(),
                finished: true,
            },
            &lease,
        );
        let mut thread = crate::thread::THREAD.lock().unwrap();
        let later = Instant::now() + Duration::from_secs(1);
        let given = thread.summary_rest(Some(&a), later);
        assert!(
            given.iter().all(|(given, ..)| *given != request),
            "{request}: never in a talk note: {given:?}"
        );
    }
}

/// A chat that keeps what was said, then answers.
fn remember_tokyo() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                crate::remember::REMEMBER,
                json!({"text": "Flying to Tokyo next week."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Noted. Have a good trip."), Ok(ChatEvent::Done)],
    ]
}

async fn one_face_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    going_on: Option<bool>,
    voice: Voice,
) -> Outcome {
    lines_saving(
        what,
        vec![(words.to_owned(), in_view, 0)],
        chats,
        BOOT,
        true,
        true,
        None,
        going_on,
        true,
        false,
        voice,
    )
    .await
}

/// The launch testers' item 1 on #3997 (`LAMP_PERSON_SCOPE_ONE_FACE`), as
/// on lamp-220c: A's known face the one face in every look, never read
/// speaking, and no other voice. Narrowed to option 2 of reviewer-three's
/// P1 on #4584: A's keep lands in A's scope, but the line is told and read
/// as the room's (no A note, context or thread). In A's conversation going
/// on, the carry wins, as without the rule: held, never memory's.
#[tokio::test]
async fn one_known_face_and_one_voice_keep_their_keep_but_read_the_rooms_unseen_speaking() {
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.calls, [Some(a.clone())], "the keep lands in A's scope");
    assert_eq!(got.told, Message::system("for None"), "no A context");
    assert!(
        got.noted.iter().all(|(person, _)| person.is_none()),
        "no A note: {:?}",
        got.noted
    );
    let got = one_face_line(
        "one-face-carried",
        TOKYO,
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        Some(true),
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(got.held, "carried, as without the rule");
}

/// The first builder's option 1 on #3997 (22:12): the rule does not wait on
/// the same-voice check. Without it (no printer, or a standby beside Live)
/// no voice is ever heard as another, and a one-face line's keep still lands
/// in A's scope while the line is told and read as the room's.
#[tokio::test]
async fn without_the_same_voice_check_a_one_face_keep_is_theirs_and_its_reads_the_rooms() {
    let got = one_face_line(
        "one-face-unchecked",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::Unchecked,
    )
    .await;
    assert_eq!(
        got.calls,
        [Some(crate::remember::person_id("A"))],
        "the keep lands in A's scope"
    );
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"), "no A context");
    assert!(
        got.noted.iter().all(|(person, _)| person.is_none()),
        "no A note: {:?}",
        got.noted
    );
}

/// Reviewer-three's P1 on #4584: A sits silent, the one face in view; a
/// guest off camera begins the conversation (the first voice, so no other
/// voice is heard) and asks about A's memory. Nothing of A's is read or
/// told: the recall runs in the room's scope, the turn is told the room's
/// context and no A note; with the switch off, the same.
/// With `LAMP_PERSON_SCOPE_ONE_FACE_READS` (#5522) the guest is told A's
/// recall: the risk the memory owner accepted with it, asserted here so it
/// is never taken silently
/// ([`a_lone_known_face_never_read_speaking_writes_its_words_in_their_memory_and_record`]).
#[tokio::test]
async fn a_guest_off_camera_asking_about_a_silent_one_face_reads_nothing_of_theirs() {
    let a = crate::remember::person_id("A");
    let got = one_face_reading(
        "guest-asks-reads",
        "Lamp, what did she say about her doctor?",
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "r1",
                    crate::remember::RECALL,
                    json!({"query": "doctor"}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![text("I don't have anything on that."), Ok(ChatEvent::Done)],
        ],
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(
        got.calls,
        [Some(a.clone())],
        "reads: the accepted guest risk"
    );
    assert_eq!(got.lease, Some(a));
    for voice in [Voice::One, Voice::Off] {
        let got = one_face_line(
            "guest-asks",
            "Lamp, what did she say about her doctor?",
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "r1",
                        crate::remember::RECALL,
                        json!({"query": "doctor"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("I don't have anything on that."), Ok(ChatEvent::Done)],
            ],
            vec![("f1", false)],
            None,
            voice,
        )
        .await;
        let what = if voice == Voice::One { "on" } else { "off" };
        assert_eq!(got.calls, [None], "{what}: the recall is the room's");
        assert_eq!(got.lease, None, "{what}");
        assert_eq!(
            got.told,
            Message::system("for None"),
            "{what}: no A context"
        );
        assert!(
            got.noted.iter().all(|(person, _)| person.is_none()),
            "{what}: no A note: {:?}",
            got.noted
        );
    }
}

/// A second face, a face the eyes don't know as A's, no face at all, another
/// voice before it or in it, or the switch off: as today, the room's (or
/// carried, in A's conversation going on, with no other face).
#[tokio::test]
async fn a_second_face_an_unknown_face_no_face_or_another_voice_is_as_before() {
    for (what, in_view, voice) in [
        (
            "second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        ("unknown-face", vec![("f2", false)], Voice::One),
        ("no-face", vec![], Voice::One),
        ("voice-before", vec![("f1", false)], Voice::AnotherBefore),
        ("voice-in-it", vec![("f1", false)], Voice::AnotherInIt),
        ("off", vec![("f1", false)], Voice::Off),
    ] {
        let got = one_face_line(what, TOKYO, remember_tokyo(), in_view, None, voice).await;
        assert_eq!(got.lease, None, "{what}: the room's");
        assert!(!got.held, "{what}");
        assert_eq!(got.calls, [None], "{what}: the keep is the room's");
        // The chat is asked only after the line is judged, so another
        // voice heard as it was judged is told the room's too.
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
    // Another voice, in A's conversation going on: carried, as before.
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "voice-carried",
        TOKYO,
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        Some(true),
        Voice::AnotherBefore,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(got.held, "carried, never memory's");
}

/// One line, "What's my name?", A's known face in every look per
/// `in_view`, with `LAMP_SECOND_NAME_ONE_FACE` as `name_one_face`.
async fn whats_my_name(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    voice: Voice,
    name_one_face: bool,
) -> Outcome {
    whats_my_name_read(what, in_view, &[], voice, name_one_face).await
}

/// [`whats_my_name`] with each look's mouth read as the next of `mouths`.
async fn whats_my_name_read(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    mouths: &'static [lamp_msgs::SpeakingState],
    voice: Voice,
    name_one_face: bool,
) -> Outcome {
    lines_named(
        what,
        vec![("Lamp, what's my name?".to_owned(), in_view, 0)],
        vec![vec![text("You're A."), Ok(ChatEvent::Done)]],
        BOOT,
        true,
        true,
        None,
        None,
        true,
        false,
        None,
        voice,
        name_one_face,
        mouths,
    )
    .await
}

/// lamp-06c1 (build 10343, request 42, `LAMP_SECOND_NAME_ONE_FACE`): the test
/// face, known and not the owner, alone in 12 of 12 looks, read speaking in
/// 2, still in 4 and unread in 6, no other voice. The mouth rule
/// (`speaking_faces::named_alone`) held too, and once it had named the line
/// the one-face count was never checked: that rule reaches the reply only as
/// the owner's typed name, so she was told no name and said "I don't
/// remember your name". The one-face count names her for the reply whether
/// or not the mouth rule held; the line stays the room's.
#[tokio::test]
async fn a_lone_known_face_the_mouth_rule_named_is_named_by_the_one_face_count_too() {
    use lamp_msgs::SpeakingState::{No, Unknown, Yes};
    const MOUTHS: &[lamp_msgs::SpeakingState] = &[
        Yes, Yes, No, No, No, No, Unknown, Unknown, Unknown, Unknown, Unknown, Unknown,
    ];
    let a = crate::remember::person_id("A");
    let got = whats_my_name_read(
        "name-one-face-mouth-rule",
        vec![("f1", false)],
        MOUTHS,
        Voice::One,
        true,
    )
    .await;
    assert_eq!(got.named_for_reply, Some(a), "the reply told A's name");
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"), "no A context");
    // The switch off: named by the mouth rule alone, no face name.
    let got = whats_my_name_read(
        "name-one-face-mouth-rule-off",
        vec![("f1", false)],
        MOUTHS,
        Voice::One,
        false,
    )
    .await;
    assert_eq!(got.named_for_reply, None);
    assert_eq!(got.lease, None, "the room's");
}

/// lamp-06c1 (5 Nov 11:12, request 6, `LAMP_SECOND_NAME_ONE_FACE`): the test
/// face alone in 11 of 11 looks, its mouth read still in 6, so the mouth
/// rule (`speaking_faces::named_alone`) failed and "what's my name?" got "I
/// don't have a name for you". With the switch on, A's known face the one
/// face in every look, read still throughout and never speaking, and no
/// other voice: the reply is told A's name, and only that: the line is told
/// and read as the room's. The memory owner's call of 5 Nov.
#[tokio::test]
async fn a_lone_known_face_never_read_speaking_is_named_for_the_reply_with_the_switch_on() {
    let a = crate::remember::person_id("A");
    let got = whats_my_name("name-one-face", vec![("f1", false)], Voice::One, true).await;
    assert_eq!(got.named_for_reply, Some(a), "the reply told A's name");
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"), "no A context");
    assert!(
        got.noted.iter().all(|(person, _)| person.is_none()),
        "no A note: {:?}",
        got.noted
    );
}

/// The switch off: the same line, as before, names nobody (the mouth was
/// never read speaking, so `named_alone` fails).
#[tokio::test]
async fn a_lone_known_face_never_read_speaking_is_not_named_with_the_switch_off() {
    let got = whats_my_name("name-one-face-off", vec![("f1", false)], Voice::One, false).await;
    assert_eq!(got.named_for_reply, None);
    assert_eq!(got.lease, None, "the room's");
}

/// Any sign of someone else wins (`LAMP_SECOND_NAME_ONE_FACE`): a second
/// face in the looks, another voice heard before the line or in it, or a
/// face other than A's seen speaking it names nobody, and the line stays
/// the room's.
#[tokio::test]
async fn another_face_another_voice_or_another_speaker_names_nobody() {
    for (what, in_view, voice) in [
        (
            "name-second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        (
            "name-voice-before",
            vec![("f1", false)],
            Voice::AnotherBefore,
        ),
        ("name-voice-in-it", vec![("f1", false)], Voice::AnotherInIt),
        (
            "name-other-speaks",
            vec![("f1", false), ("f2", true)],
            Voice::One,
        ),
    ] {
        let got = whats_my_name(what, in_view, voice, true).await;
        assert_eq!(got.named_for_reply, None, "{what}: named nobody");
        assert_eq!(got.lease, None, "{what}: the room's");
    }
}

fn list_reminders() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "l1",
                "list_reminders",
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Call your mom, tomorrow."), Ok(ChatEvent::Done)],
    ]
}

/// lamp-29e7 (3 Nov, `LAMP_ONE_FACE_REMINDERS`): A's face the one face in
/// every look, never read speaking, no other voice. "What reminders do I
/// have?" asks the Schedule for A's own (and the room's) while the line is
/// still told and read as the room's. A second face, a face the eyes don't
/// know, no face, another voice before it or in it, or the one-face rule
/// off: the room's, as before.
#[tokio::test]
async fn a_one_face_line_asks_for_its_persons_own_reminders_and_nobody_elses() {
    const ASKED: &str = "Lamp, what reminders do I have?";
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face-reminders",
        ASKED,
        list_reminders(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.calls, [Some(a)], "A's own reminders");
    assert_eq!(got.lease, None, "told and read as the room's");
    assert_eq!(got.told, Message::system("for None"), "no A context");
    for (what, in_view, voice) in [
        (
            "reminders-second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        ("reminders-unknown-face", vec![("f2", false)], Voice::One),
        ("reminders-no-face", vec![], Voice::One),
        (
            "reminders-voice-before",
            vec![("f1", false)],
            Voice::AnotherBefore,
        ),
        (
            "reminders-voice-in-it",
            vec![("f1", false)],
            Voice::AnotherInIt,
        ),
        ("reminders-off", vec![("f1", false)], Voice::Off),
    ] {
        let got = one_face_line(what, ASKED, list_reminders(), in_view, None, voice).await;
        assert_eq!(got.calls, [None], "{what}: the room's reminders only");
        assert_eq!(got.lease, None, "{what}");
    }
}

fn forget_coffee() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"source_id": null}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done."), Ok(ChatEvent::Done)],
    ]
}

const FORGET_COFFEE: &str = "Lamp, forget my coffee.";

/// Fourteen's forget FAIL on lamp-9adf (build 9699): A's face the one face
/// in view, never seen speaking, and no other voice. "Forget my coffee"
/// reaches A's scope, where A's `remember` on the same rule kept it, while
/// the line is still told and read as the room's. A second face, a face the
/// eyes don't know, no face, another voice, or the switch off: the room's
/// only, never A's.
#[tokio::test]
async fn a_one_face_forget_reaches_what_its_keep_kept_and_nobody_elses() {
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face-forget-one",
        FORGET_COFFEE,
        forget_coffee(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())], "the forget reaches A's scope");
    assert_eq!(got.lease, None, "told and read as the room's");
    assert_eq!(got.told, Message::system("for None"), "no A context");
    // With `LAMP_PERSON_SCOPE_ONE_FACE_READS` (#5522): the mouth not read
    // saying it, the forget reaches nobody's scope (the memory owner, 16:49).
    let got = one_face_reading(
        "one-face-forget-one-reads",
        FORGET_COFFEE,
        forget_coffee(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert!(got.calls.is_empty(), "reads: never run: {:?}", got.calls);
    for (what, in_view, voice) in [
        (
            "forget-second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        ("forget-unknown-face", vec![("f2", false)], Voice::One),
        ("forget-no-face", vec![], Voice::One),
        (
            "forget-voice-before",
            vec![("f1", false)],
            Voice::AnotherBefore,
        ),
        (
            "forget-voice-in-it",
            vec![("f1", false)],
            Voice::AnotherInIt,
        ),
        ("forget-off", vec![("f1", false)], Voice::Off),
    ] {
        let got = one_face_line(what, FORGET_COFFEE, forget_coffee(), in_view, None, voice).await;
        assert_eq!(got.calls, [None], "{what}: the room's only");
        assert_eq!(got.lease, None, "{what}");
    }
}

/// "Forget everything about me" on a line whose keep is theirs by one face
/// alone never runs in their scope: the line is the room's, as without the
/// rule, and a yes is never one either
/// ([`a_yes_on_a_line_theirs_by_one_face_alone_runs_no_held_forget`]).
#[tokio::test]
async fn forget_everything_on_a_one_face_line_never_runs_in_their_scope() {
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face-forget",
        FORGET_ME,
        forget_everything(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert!(
        !got.calls.contains(&Some(a)),
        "never run in A's scope: {:?}",
        got.calls
    );
    assert_eq!(got.lease, None, "the room's");
    // With `LAMP_PERSON_SCOPE_ONE_FACE_READS`: read as A's, but never run.
    let got = one_face_reading(
        "one-face-forget-reads",
        FORGET_ME,
        forget_everything(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert!(got.calls.is_empty(), "never run: {:?}", got.calls);
}

/// A held forget's yes on a line theirs only by their face alone in view
/// (`ScopeLease::by_one_face`) runs nothing, whatever else the yes passes:
/// a forget is never relaxed by `LAMP_PERSON_SCOPE_ONE_FACE`. The same yes
/// on a line not so taken runs it.
#[tokio::test]
async fn a_yes_on_a_line_theirs_by_one_face_alone_runs_no_held_forget() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    for (by_one_face, runs) in [(true, false), (false, true)] {
        let now = Instant::now();
        if let Ok(mut hold) = engine.shared.held_forget.lock() {
            hold.held = Some(HeldForget {
                call: call("f1", crate::remember::FORGET_EVERYTHING, json!({})),
                request: 1,
                asker: asker.clone(),
                at: now,
                asked: true,
                spelling: false,
                carried: false,
                line: None,
                asked_again: false,
                one_face: false,
                near: None,
                offered_alone: false,
                face_yes: false,
            });
        }
        let taken = take_held_forget(
            &engine.shared,
            2,
            true,
            "Yes.",
            (Some(&asker), Some(&asker)),
            by_one_face,
            now,
        );
        match taken {
            Taken::Runs(..) => assert!(runs, "by one face: no yes"),
            Taken::Unseen(_) => assert!(!runs, "not by one face: runs"),
            Taken::Nothing
            | Taken::AskAgain
            | Taken::Unknown(_)
            | Taken::Agrees(_)
            | Taken::NotTheirs
            | Taken::OnForget(..)
            | Taken::KeptAtOnce(..) => {
                panic!("dropped for another reason")
            }
        }
    }
}

/// The memory owner's rule (17 Nov, "mouth detection only blocks the
/// forget and nothing else"), with `LAMP_KEEP_QUIETLY_ONE_FACE`: a held
/// keep (a carried line's, `LAMP_CONVERSATION_CARRIED_SAVES`) is let go by
/// its person's plain yes on a line theirs by their face alone in view,
/// none seen speaking it, as their mouth seen saying it lets it go. Off, or
/// another face seen speaking the yes, or no face-in-view rule, or another
/// person: it stays dropped. A held forget's yes is unchanged.
#[tokio::test]
async fn a_held_keeps_yes_on_their_one_face_line_keeps_it_with_the_switch() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file("held-keep-one-face", &eyes);
    let engine = engine.confirming_forgets_by_sight(eyes, path.clone());
    let asker = (7, crate::remember::person_id("A"));
    let other = (7, crate::remember::person_id("B"));
    let take = |name: &str, on: bool, by_one_face: bool, speakers: &[&str], line: &HeldAsker| {
        crate::keep_quietly::set_one_face_for_test(on);
        let now = Instant::now();
        if let Ok(mut hold) = engine.shared.held_forget.lock() {
            hold.held = Some(HeldForget {
                call: call(
                    "k1",
                    name,
                    json!({"text": "I start at the bank on Monday."}),
                ),
                request: 1,
                asker: asker.clone(),
                at: now,
                asked: true,
                spelling: false,
                carried: true,
                line: Some("I start at the bank on Monday.".to_owned()),
                asked_again: false,
                one_face: false,
                near: None,
                offered_alone: false,
                face_yes: false,
            });
            hold.sight = Some((
                2,
                speakers.iter().map(|id| (*id).to_owned()).collect(),
                None,
            ));
        }
        // A line theirs by one face alone has no forget asker.
        let taken = take_held_forget(
            &engine.shared,
            2,
            true,
            "Yes.",
            ((!by_one_face).then_some(line), Some(line)),
            by_one_face,
            now,
        );
        crate::keep_quietly::set_one_face_for_test(false);
        matches!(taken, Taken::Runs(..))
    };
    let keep = crate::remember::REMEMBER;
    assert!(!take(keep, false, true, &[], &asker), "off: unseen");
    assert!(take(keep, true, true, &[], &asker), "their one face: kept");
    assert!(
        !take(keep, true, true, &["f9"], &asker),
        "another face seen"
    );
    assert!(!take(keep, true, false, &[], &asker), "no one-face rule");
    assert!(!take(keep, true, true, &[], &other), "another person");
    let forget = crate::remember::FORGET;
    assert!(
        !take(forget, true, true, &[], &asker),
        "a forget: unchanged"
    );
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(path);
}

/// How the held one-face forget is taken on a line ([`take_held_forget`]):
/// left to the model's call, kept on a no, or asked once more.
#[derive(Debug, PartialEq)]
enum OneFaceTaken {
    PlainYes,
    ModelsCall,
    OnlyNo,
    SaidNo,
    AskedAgain,
}

/// Thirteen's P2 on #6673, under the first builder's leg (e) ruling (no yes
/// word list): a yes, and any answer that is not a no, to "Shall I forget
/// what you kept about …?", in English or Vietnamese, leaves the held
/// one-face forget to the model's call on that line (`Taken::Agrees`,
/// carrying exactly the held call). #8165: a no keeps it whatever the
/// model calls (`Taken::OnForget` with the said-no note), a negated forget
/// word ("No, don't forget it.") among them, and a line that says forget and
/// takes it back is asked once more. Run through
/// `take_held_forget` in the private room, on a line theirs by one face.
#[tokio::test]
async fn the_natural_answers_to_the_one_face_question_run_it_and_a_no_keeps_it() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    crate::person::set_private_room_for_test(Some(true));
    use OneFaceTaken::{AskedAgain, ModelsCall, OnlyNo, PlainYes, SaidNo};
    for (said, want) in [
        ("Yes.", PlainYes),
        ("Yes please.", PlainYes),
        ("Yeah, go ahead.", PlainYes),
        ("Okay lamp, please do.", PlainYes),
        ("Loan. Yes.", ModelsCall),
        ("C\u{f3}.", ModelsCall),
        ("Yes, forget it.", ModelsCall),
        ("Yes, please forget that.", ModelsCall),
        ("Sure, forget it.", ModelsCall),
        ("Sure, go ahead and forget it.", ModelsCall),
        ("\u{1eea}, qu\u{ea}n \u{111}i.", ModelsCall),
        ("V\u{e2}ng, qu\u{ea}n \u{111}i \u{1ea1}.", ModelsCall),
        ("C\u{f3}, qu\u{ea}n \u{111}i.", ModelsCall),
        ("Forget it.", ModelsCall),
        ("Qu\u{ea}n \u{111}i.", ModelsCall),
        ("No problem, go ahead.", ModelsCall),
        ("No, just forget it.", ModelsCall),
        ("No no, delete it.", ModelsCall),
        ("No, I don't need it.", ModelsCall),
        ("No.", OnlyNo),
        ("No, keep it.", OnlyNo),
        ("No, no, keep it.", OnlyNo),
        ("Don't.", OnlyNo),
        ("Lamp, no.", OnlyNo),
        ("Kh\u{f4}ng, gi\u{1eef} l\u{1ea1}i.", OnlyNo),
        ("Don't forget it.", OnlyNo),
        ("No, don't forget it.", OnlyNo),
        ("No, don't delete it.", OnlyNo),
        ("\u{110}\u{1eeb}ng qu\u{ea}n.", OnlyNo),
        ("Yes, but don't forget it.", SaidNo),
        ("No, turn the lights on.", SaidNo),
        (
            "No, don't keep it, forget it. No wait, don't delete it.",
            AskedAgain,
        ),
        // Not a no in words: the model's to judge, as today.
        ("Keep it.", ModelsCall),
        ("Wait.", ModelsCall),
        ("Forget it?", ModelsCall),
        ("Yes, what's the weather?", ModelsCall),
    ] {
        let now = Instant::now();
        if let Ok(mut hold) = engine.shared.held_forget.lock() {
            hold.held = Some(HeldForget {
                call: call("f1", crate::remember::FORGET, json!({"source_id": 7})),
                request: 1,
                asker: asker.clone(),
                at: now,
                asked: true,
                spelling: false,
                carried: false,
                line: None,
                asked_again: false,
                one_face: true,
                near: None,
                offered_alone: true,
                face_yes: false,
            });
        }
        let taken = take_held_forget(
            &engine.shared,
            2,
            true,
            said,
            (None, Some(&asker)),
            true,
            now,
        );
        let got = match taken {
            Taken::Runs(call, line, one_face) => {
                assert_eq!(call.args, json!({"source_id": 7}), "{said:?}");
                assert_eq!((line, one_face), (None, true), "{said:?}");
                PlainYes
            }
            Taken::KeptAtOnce(call, note) => {
                assert_eq!(call.args, json!({"source_id": 7}), "{said:?}");
                assert_eq!(note, crate::remember::said_no_told(None), "{said:?}");
                OnlyNo
            }
            Taken::Agrees(call) => {
                assert_eq!(call.args, json!({"source_id": 7}), "{said:?}");
                ModelsCall
            }
            Taken::OnForget(call, note) => {
                assert_eq!(call.args, json!({"source_id": 7}), "{said:?}");
                if note == crate::remember::FORGET_ASKED_AGAIN {
                    AskedAgain
                } else {
                    assert_eq!(note, crate::remember::said_no_told(None), "{said:?}");
                    SaidNo
                }
            }
            _ => panic!("{said:?}: dropped for another reason"),
        };
        assert_eq!(got, want, "{said:?}");
        let held_on = engine
            .shared
            .held_forget
            .lock()
            .unwrap()
            .held
            .as_ref()
            .map(|held| (held.request, held.asked, held.asked_again));
        if want == AskedAgain {
            assert_eq!(held_on, Some((2, false, true)), "{said:?}: held on, once");
        } else {
            assert_eq!(held_on, None, "{said:?}: taken");
        }
    }
    crate::person::set_private_room_for_test(None);
}

/// [`one_face_line`] with `LAMP_PERSON_SCOPE_ONE_FACE_READS` on (#5522).
async fn one_face_reading(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    going_on: Option<bool>,
    voice: Voice,
) -> Outcome {
    ONE_FACE_READS.with(|reads| reads.set(true));
    one_face_line(what, words, chats, in_view, going_on, voice).await
}

/// C37, the memory owner's call A (11 Nov), with
/// `LAMP_PERSON_SCOPE_ONE_FACE_READS` and `LAMP_MEMORY_DOUBT_UNDOES`: A's
/// known face alone in view, never read speaking, keeps "renunctinus" as
/// A's, and the next round asks "is that ranunculus?". That line's own keep
/// is taken back in A's scope, though no other forget runs on such a line.
/// A forget the model calls there is still refused.
#[tokio::test]
async fn a_lone_known_faces_line_takes_back_its_own_keep_its_next_round_doubts() {
    let a = crate::remember::person_id("A");
    let flower = || {
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "m1",
                    crate::remember::REMEMBER,
                    json!({"text": "Their favorite flower is a renunctinus."}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![
                text("Kept as you said it \u{2026} is that ranunculus?"),
                Ok(ChatEvent::Done),
            ],
        ]
    };
    let lone = |what: &'static str, chats| async move {
        ONE_FACE_READS.with(|reads| reads.set(true));
        DOUBT_UNDOES.with(|on| on.set(true));
        lines_saving(
            what,
            vec![(
                "Remember that my favorite flower is a renunctinus.".to_owned(),
                vec![("f1", false)],
                0,
            )],
            chats,
            BOOT,
            true,
            true,
            None,
            None,
            true,
            true,
            Voice::One,
        )
        .await
    };
    let got = lone("one-face-doubt-undoes", flower()).await;
    assert_eq!(got.lease, Some(a.clone()), "a lone known face's line");
    assert_eq!(
        got.calls,
        [Some(a.clone()), Some(a.clone())],
        "the keep, then its own undo, both in A's scope"
    );
    // The model's own forget on such a line is refused as before.
    let got = lone(
        "one-face-doubt-model-forget",
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "f1",
                    crate::remember::FORGET,
                    json!({"source_id": 391}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Okay."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(got.calls, [], "no forget runs on it");
}

/// Whether any chat round was told a memory call refused by the one-face
/// rule's reads (`second_provider::ONE_FACE_FORGET`).
fn refused_by_one_face(got: &Outcome) -> bool {
    got.rounds
        .iter()
        .flatten()
        .any(|message| format!("{message:?}").contains("say it again"))
        && got
            .rounds
            .iter()
            .flatten()
            .any(|message| format!("{message:?}").contains("can't forget anything"))
}

/// #5522 with `LAMP_PERSON_SCOPE_ONE_FACE_READS` (the memory owner's 16:43
/// "apply it", read for saving and recall): A's known face the one face in
/// every look, never read speaking (lamp-220c, 29e7: 0 of 17), no other
/// voice. The line is told, read and kept as A's: A's context and note,
/// their hits, their keep. Its exchange's record is theirs too
/// (`remember::filed_by_one_face`, the remember test of that name).
#[tokio::test]
async fn a_lone_known_face_never_read_speaking_writes_its_words_in_their_memory_and_record() {
    let a = crate::remember::person_id("A");
    let got = one_face_reading(
        "one-face-reads",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, Some(a.clone()), "told and read as A's");
    assert!(!got.held);
    assert_eq!(got.calls, [Some(a.clone())], "the keep lands in A's scope");
    assert_eq!(
        got.told,
        Message::system(format!("for {:?}", Some(a.as_str()))),
        "A's context"
    );
    assert!(
        !got.noted.is_empty()
            && got
                .noted
                .iter()
                .all(|(person, _)| person.as_ref() == Some(&a)),
        "A's note: {:?}",
        got.noted
    );
    assert!(
        got.hits.iter().all(|hits| hits
            .as_ref()
            .is_some_and(|(person, _)| person.as_ref() == Some(&a))),
        "A's hits: {:?}",
        got.hits
    );
    // Recall too: the guest risk, accepted with the switch (a voice off
    // camera while A sits silent alone in view reads A's memory).
    let got = one_face_reading(
        "one-face-reads-recall",
        "Lamp, what did I say about my doctor?",
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "r1",
                    crate::remember::RECALL,
                    json!({"query": "doctor"}),
                )])),
                Ok(ChatEvent::Done),
            ],
            vec![text("You said Tuesday."), Ok(ChatEvent::Done)],
        ],
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())], "the recall is A's");
    assert_eq!(got.lease, Some(a));
}

/// With `LAMP_PERSON_SCOPE_ONE_FACE_READS` on, every privacy limit holds:
/// a second face, an unknown face, no face, another voice before the line
/// or heard as it is judged, or the one-face rule off, and the line is the
/// room's, its keep and reads alike; in A's conversation going on it is
/// carried, held, never read.
#[tokio::test]
async fn with_reads_a_second_face_an_unknown_face_or_another_voice_is_the_rooms() {
    for (what, in_view, voice) in [
        (
            "reads-second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        ("reads-unknown-face", vec![("f2", false)], Voice::One),
        ("reads-no-face", vec![], Voice::One),
        (
            "reads-voice-before",
            vec![("f1", false)],
            Voice::AnotherBefore,
        ),
        ("reads-voice-in-it", vec![("f1", false)], Voice::AnotherInIt),
        ("reads-rule-off", vec![("f1", false)], Voice::Off),
    ] {
        let got = one_face_reading(what, TOKYO, remember_tokyo(), in_view, None, voice).await;
        assert_eq!(got.lease, None, "{what}: the room's");
        assert!(!got.held, "{what}");
        assert_eq!(got.calls, [None], "{what}: the keep is the room's");
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
    let a = crate::remember::person_id("A");
    let got = one_face_reading(
        "reads-carried",
        TOKYO,
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        Some(true),
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(got.held, "carried: held, no reads (#5508)");
}

/// With `LAMP_PERSON_SCOPE_ONE_FACE_READS` on, a forget on a line A's only
/// by their face alone in view never runs: not "forget my coffee", which
/// the keep's rule alone lets reach A's scope (`reached_by_one_face`), not
/// "forget everything", in A's scope or the room's. Forgetting still needs
/// the mouth seen speaking (the memory owner, 16:49 on #5385: "tạm thời
/// chưa cho xóa nếu ko detect đc mở miệng").
#[tokio::test]
async fn with_reads_a_forget_the_mouth_was_not_read_saying_never_runs() {
    for (what, words, chats) in [
        ("reads-forget-one", FORGET_COFFEE, forget_coffee()),
        ("reads-forget-all", FORGET_ME, forget_everything()),
    ] {
        let got = one_face_reading(what, words, chats, vec![("f1", false)], None, Voice::One).await;
        assert!(got.calls.is_empty(), "{what}: never run: {:?}", got.calls);
        assert!(refused_by_one_face(&got), "{what}: told why");
    }
}

/// With `LAMP_PERSON_SCOPE_ONE_FACE_READS` on, A's own face seen speaking
/// the forget is as before: the line is A's by sight, not by the one-face
/// rule, and "forget my coffee" runs in A's scope.
#[tokio::test]
async fn with_reads_a_forget_their_mouth_was_read_saying_runs_as_before() {
    let a = crate::remember::person_id("A");
    let got = one_face_reading(
        "reads-forget-seen",
        FORGET_COFFEE,
        forget_coffee(),
        vec![("f1", true)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())], "runs in A's scope");
    assert_eq!(got.lease, Some(a));
    assert!(!refused_by_one_face(&got));
}

const FORGET_UMBRELLA: &str = "Lamp, forget my umbrella.";
const ASKS_UMBRELLA: &str = "Shall I forget that your umbrella is blue?";
/// The kept thing the one-face forget names ([`FORGET_TARGET`]): its id,
/// and the subject the lamp asks about in their words.
const UMBRELLA: (u64, &str) = (7, "umbrella");

/// "Forget my umbrella" called by its `about`, the lamp's question, then
/// one answer for each line after it.
fn forget_umbrella_then(after: &[&'static str]) -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"about": "umbrella"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text(ASKS_UMBRELLA), Ok(ChatEvent::Done)],
    ];
    chats.extend(
        after
            .iter()
            .map(|said| vec![text(said), Ok(ChatEvent::Done)]),
    );
    chats
}

/// The PM's cost ruling on #5627, with `LAMP_PERSON_SCOPE_ONE_FACE_READS`
/// and the eyes judging a held call's yes, as on the lamp: `lines`, the
/// first "Lamp, forget my umbrella." on a line A's only by their face alone
/// in view, A's umbrella the one kept thing it names ([`UMBRELLA`]), and
/// the private room as `private_room`.
async fn asked_once(what: &str, lines: Vec<Line>, after: &[&'static str], room: bool) -> Outcome {
    asked_once_going(what, lines, after, room, None).await
}

/// [`asked_once`] with `going_on` whether A is in a conversation going on
/// (`LAMP_CONVERSATION_CARRIES`, [`lines_saving`]).
async fn asked_once_going(
    what: &str,
    lines: Vec<Line>,
    after: &[&'static str],
    room: bool,
    going_on: Option<bool>,
) -> Outcome {
    asked_once_chats(what, lines, forget_umbrella_then(after), room, going_on).await
}

/// The model's answer to a line after the question that it takes as their
/// yes: its `forget` call with `args` (naming nothing else, `{}`), then
/// `said` (the first builder's ruling on leg (e) of #7960's ROW).
fn agreed_with(args: Value, said: &'static str) -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f2",
                crate::remember::FORGET,
                args,
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text(said), Ok(ChatEvent::Done)],
    ]
}

/// [`forget_umbrella_then`] with the model taking the next line as their
/// yes: its bare `forget` call, then "Done.".
fn forget_umbrella_agreed() -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = forget_umbrella_then(&[]);
    chats.extend(agreed_with(json!({}), "Done."));
    chats
}

/// [`asked_once_going`] with the chats after the forget's line given.
async fn asked_once_chats(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    room: bool,
    going_on: Option<bool>,
) -> Outcome {
    ONE_FACE_READS.with(|reads| reads.set(true));
    FORGET_TARGET.with(|target| target.set(Some(UMBRELLA)));
    crate::person::set_private_room_for_test(Some(room));
    let got = lines_saving(
        what,
        lines,
        chats,
        BOOT,
        true,
        true,
        None,
        going_on,
        true,
        true,
        Voice::One,
    )
    .await;
    crate::person::set_private_room_for_test(None);
    got
}

fn line(words: &str, in_view: Vec<(&'static str, bool)>, after: u64) -> Line {
    (words.to_owned(), in_view, after)
}

/// The forgets run, by their arguments.
fn forgets_ran(got: &Outcome) -> Vec<Value> {
    got.ran
        .iter()
        .filter(|(name, _)| name == crate::remember::FORGET)
        .map(|(_, args)| args.clone())
        .collect()
}

/// Whether a chat round was told to ask about the kept thing once.
fn asked_about_umbrella(got: &Outcome) -> bool {
    got.rounds.iter().flatten().any(|message| {
        let message = format!("{message:?}");
        message.contains("this waits for their yes") && message.contains(UMBRELLA.1)
    })
}

/// #5627 (1): A's face alone in view, the mouth not seen saying it: a
/// named forget is neither refused nor run. Nothing is removed, and the
/// model is told to ask once, about the subject in their words.
#[tokio::test]
async fn a_named_one_face_forget_the_mouth_was_not_seen_saying_asks_once() {
    let got = asked_once(
        "ask-once-asks",
        vec![line(FORGET_UMBRELLA, vec![("f1", false)], 0)],
        &[],
        false,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "nothing removed");
    assert!(asked_about_umbrella(&got), "asked once: {:?}", got.rounds);
    assert!(!refused_by_one_face(&got), "not refused");
}

/// #8204: a named one-face forget whose listing waits 1.5 s, as a frozen
/// lamp-memory's does (`tools::ASK_DEADLINE`), waits off the worker's
/// thread: the runtime its control loop shares keeps reading the
/// coordinator's 20 ms renewals through the wait, none refused as a full
/// socket, and the forget is still asked about once. On the runtime's own
/// thread it read none for 1.5 s, the coordinator's sends to the full
/// socket were refused, and the voice ended.
#[tokio::test]
async fn a_one_face_forget_whose_listing_waits_keeps_the_control_loop_reading() {
    let directory = crate::process::SessionDirectory::create().unwrap();
    let (parent, worker) = (
        crate::process::new_boot().unwrap(),
        crate::process::new_boot().unwrap(),
    );
    let mut coordinator =
        crate::transport::WorkerChannels::bind(&directory.path, "second", parent, worker, false)
            .unwrap();
    let mut control =
        crate::transport::WorkerChannels::bind(&directory.path, "second", parent, worker, true)
            .unwrap();
    coordinator
        .connect(&directory.path, "second", false)
        .unwrap();
    control.connect(&directory.path, "second", true).unwrap();
    let marks: Arc<Mutex<Vec<Instant>>> = Arc::default();
    FORGET_STALL.with(|stall| {
        *stall.borrow_mut() = Some((Duration::from_millis(1_500), marks.clone()));
    });
    // The coordinator's renewals, every 20 ms, from a thread of its own.
    let done = Arc::new(AtomicBool::new(false));
    let renewing = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut refused = 0u32;
            let mut renewal = 0u64;
            while !done.load(Ordering::Acquire) {
                renewal += 1;
                if coordinator.control.send(renewal).is_err() {
                    refused += 1;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            refused
        })
    };
    // The worker's control loop: on the runtime the turn runs on.
    let read: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let reading = {
        let read = read.clone();
        tokio::spawn(async move {
            loop {
                while let Ok(Some(_)) = control.control.receive::<u64>() {
                    read.lock().unwrap().push(Instant::now());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    };
    let got = asked_once(
        "ask-once-listing-waits",
        vec![line(FORGET_UMBRELLA, vec![("f1", false)], 0)],
        &[],
        false,
    )
    .await;
    reading.abort();
    done.store(true, Ordering::Release);
    let refused = renewing.join().unwrap();
    assert!(asked_about_umbrella(&got), "asked once: {:?}", got.rounds);
    let marks = marks.lock().unwrap().clone();
    let [began, ended] = marks[..] else {
        panic!("the listing waited once: {marks:?}");
    };
    // Inside the wait, past its first and before its last 100 ms: about 65
    // renewals read on a free runtime, none on a blocked one.
    let inside = read
        .lock()
        .unwrap()
        .iter()
        .filter(|at| **at > began + Duration::from_millis(100))
        .filter(|at| **at + Duration::from_millis(100) < ended)
        .count();
    assert!(
        inside >= 10,
        "the control loop read {inside} renewals while the listing waited"
    );
    assert_eq!(refused, 0, "no renewal refused as a full socket");
}

/// #8038: a named one-face forget held for their yes is kept out of the
/// journal, as a forget the memory ran is: the observation pass never sees
/// it, so it never keeps "they want the lamp to forget the umbrella".
/// Decided by the forget call, not by the line's words.
#[tokio::test]
async fn a_held_one_face_forget_is_kept_out_of_the_journal() {
    REQUEST_BASE.with(|base| base.set(9_803_800));
    let got = asked_once(
        "ask-once-kept-out",
        vec![line(FORGET_UMBRELLA, vec![("f1", false)], 0)],
        &[],
        false,
    )
    .await;
    assert!(asked_about_umbrella(&got), "held: {:?}", got.rounds);
    assert!(
        crate::remember::kept_out(9_803_801),
        "the held forget's exchange is not journalled"
    );
}

/// #8038: a named one-face forget that names nothing kept is told so, and
/// never told the lamp couldn't see who was talking: their face was in
/// view. Nothing is run.
#[tokio::test]
async fn a_one_face_forget_matching_nothing_is_told_the_real_cause() {
    let chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"about": "scarf"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Nothing is kept about a scarf."), Ok(ChatEvent::Done)],
    ];
    let got = asked_once_chats(
        "ask-once-no-match",
        vec![line("Lamp, forget my scarf.", vec![("f1", false)], 0)],
        chats,
        false,
        None,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "nothing removed");
    assert!(!refused_by_one_face(&got), "not told to face the lamp");
    assert!(
        got.rounds
            .iter()
            .flatten()
            .any(|message| format!("{message:?}").contains("nothing kept for them matches")),
        "told nothing matches: {:?}",
        got.rounds
    );
}

/// #5627 (2): their yes within 30 s, their face seen saying it, runs that
/// forget by the kept thing's id in A's scope, and nothing else.
#[tokio::test]
async fn their_seen_yes_within_thirty_seconds_runs_that_forget_by_id() {
    let a = crate::remember::person_id("A");
    let got = asked_once_chats(
        "ask-once-seen-yes",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", true)], 20),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    assert_eq!(
        forgets_ran(&got),
        [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})]
    );
    assert_eq!(got.calls, [Some(a)], "in A's scope only");
}

/// #5627: in the private room their yes on a line theirs by their face
/// alone in view runs it too; outside it, that unseen yes runs nothing.
#[tokio::test]
async fn an_unseen_yes_runs_it_only_in_the_private_room() {
    let a = crate::remember::person_id("A");
    for (room, runs) in [(true, true), (false, false)] {
        let got = asked_once_chats(
            &format!("ask-once-room-{room}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("Yes.", vec![("f1", false)], 5),
            ],
            forget_umbrella_agreed(),
            room,
            None,
        )
        .await;
        if runs {
            assert_eq!(
                forgets_ran(&got),
                [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})]
            );
            assert_eq!(got.calls, [Some(a.clone())], "in A's scope only");
        } else {
            assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
        }
    }
}

/// #7454: a yes to the one-face forget on a line nobody is known on (no
/// face in view, an unknown face, or a line carried in their conversation
/// going on) is dropped, and the model's `forget` on it (#7971) is told it
/// did not count: nothing was forgotten, it is kept, and they may say it
/// facing the lamp. It never says it is forgotten.
#[tokio::test]
async fn a_yes_on_a_line_nobody_is_known_on_tells_the_model_it_is_kept() {
    for (what, in_view, going_on) in [
        ("ask-once-nobody-no-face", vec![], None),
        ("ask-once-nobody-unknown-face", vec![("f2", false)], None),
        ("ask-once-nobody-carried", vec![], Some(true)),
    ] {
        // Only the yes is carried: the forget's line is A's by one face.
        GOING_ON_FROM.with(|from| from.set(2));
        let got = asked_once_chats(
            what,
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("Yes.", in_view, 5),
            ],
            forget_umbrella_agreed(),
            false,
            going_on,
        )
        .await;
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{what}: kept");
        let told = Message::tool(
            "f2",
            crate::remember::unknown_yes_told(crate::remember::FORGET),
        );
        assert!(
            got.rounds.iter().flatten().any(|message| *message == told),
            "{what}: told it is kept: {:?}",
            got.rounds
        );
    }
}

/// Whether any chat round was told a dropped held forget's note.
fn told_dropped(got: &Outcome) -> bool {
    got.rounds.iter().flatten().any(|message| {
        let message = format!("{message:?}");
        message.contains("did not count") || message.contains("not theirs to give")
    })
}

/// The Reviewer-lead's P2 on #7490: a line the model answers with no
/// `forget`, after the 30 s ("Good morning." 40 s or an hour later, their
/// own) or
/// in time from no one known or another person, is told nothing of the
/// dropped forget, and nothing runs.
#[tokio::test]
async fn a_line_the_model_answers_with_no_forget_is_told_nothing() {
    for (what, said, in_view, after) in [
        (
            "ask-once-morning-late",
            "Good morning.",
            vec![("f1", true)],
            40,
        ),
        (
            "ask-once-morning-hour",
            "Good morning.",
            vec![("f1", true)],
            3600,
        ),
        ("ask-once-nobody-question", "What time is it?", vec![], 5),
        (
            "ask-once-other-question",
            "What time is it?",
            vec![("f2", true)],
            5,
        ),
    ] {
        let got = asked_once(
            what,
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, in_view, after),
            ],
            &["Good morning!"],
            false,
        )
        .await;
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{what}: kept");
        assert!(!told_dropped(&got), "{what}: told: {:?}", got.rounds);
    }
}

/// Leg (e) of #7960's ROW (lamp-06c1, 19 Nov: "Lamp, yes." heard as
/// "Loan. Yes." was no plain yes, and the forget was lost), the first
/// builder's ruling: no yes word list. While the one-face forget is held,
/// the model's `forget` on the next line, naming nothing else, is their
/// yes: the held forget runs exactly, by its kept id and words, never the
/// model's raw call.
#[tokio::test]
async fn the_models_forget_on_a_misheard_yes_runs_the_held_forget_exactly() {
    let a = crate::remember::person_id("A");
    let got = asked_once_chats(
        "ask-once-loan-yes",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Loan. Yes.", vec![("f1", true)], 5),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    assert_eq!(
        forgets_ran(&got),
        [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})]
    );
    assert_eq!(got.calls, [Some(a)], "in A's scope only");
}

/// The same ruling: a "No." or a line on something else, which the model
/// answers with no `forget`, runs nothing; the umbrella is kept.
#[tokio::test]
async fn a_no_or_an_unrelated_line_the_model_takes_as_no_yes_runs_nothing() {
    for (what, said) in [
        ("ask-once-plain-no", "No."),
        ("ask-once-unrelated", "What time is it?"),
    ] {
        let got = asked_once(
            what,
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            &["Okay."],
            false,
        )
        .await;
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{what}: kept");
    }
}

/// The same ruling: a yes line that names another thing to forget is that
/// other forget, never the held one.
#[tokio::test]
async fn a_yes_line_naming_another_thing_does_not_run_the_held_forget() {
    let mut chats = forget_umbrella_then(&[]);
    chats.extend(agreed_with(json!({"about": "coffee"}), "Okay."));
    let got = asked_once_chats(
        "ask-once-other-thing",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes, and forget my coffee.", vec![("f1", true)], 5),
        ],
        chats,
        false,
        None,
    )
    .await;
    assert!(
        !forgets_ran(&got).contains(&json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})),
        "the umbrella is kept: {:?}",
        forgets_ran(&got)
    );
}

/// The first builder on #7490: the lamp asked about the umbrella and got
/// no answer; an hour later "Lamp, forget my umbrella.", their face seen
/// saying it, is a new forget that runs as any forget would, never a late
/// yes to the old question: nothing tells the model it did not count.
#[tokio::test]
async fn a_new_forget_an_hour_after_an_unanswered_question_is_a_new_ask() {
    let a = crate::remember::person_id("A");
    let mut chats = forget_umbrella_then(&[]);
    chats.extend(agreed_with(json!({"about": "umbrella"}), "Done."));
    let got = asked_once_chats(
        "ask-once-new-ask-hour",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line(FORGET_UMBRELLA, vec![("f1", true)], 3600),
        ],
        chats,
        false,
        None,
    )
    .await;
    assert!(!told_dropped(&got), "told as a late yes: {:?}", got.rounds);
    assert!(
        !forgets_ran(&got).is_empty(),
        "runs as a new forget: {:?}",
        got.rounds
    );
    assert!(
        got.calls.contains(&Some(a)),
        "in A's scope: {:?}",
        got.calls
    );
}

/// #5627 (3): a no keeps it.
#[tokio::test]
async fn a_no_to_the_one_face_forget_keeps_it() {
    let got = asked_once(
        "ask-once-no",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("No.", vec![("f1", true)], 5),
        ],
        &["Okay."],
        false,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
}

/// Whether the model's forget `f2` was answered with `note`.
fn forget_told(got: &Outcome, note: &str) -> bool {
    let told = Message::tool("f2", note.to_owned());
    got.rounds.iter().flatten().any(|message| *message == told)
}

/// Whether the lamp said `line` itself.
fn said_line(got: &Outcome, line: &str) -> bool {
    got.labels
        .iter()
        .filter_map(|label| label.strip_prefix("said:"))
        .any(|said| said.trim() == line)
}

/// #8165 (lamp-06c1: "No." to the one-face question, and the model called
/// forget anyway, so it ran): a no that says more than a no (#8185: one
/// that is only a no the lamp answers itself), their face seen saying it,
/// keeps the umbrella whatever the model calls. Its `forget` runs nothing
/// and is answered that they said no and it is kept.
#[tokio::test]
async fn a_no_to_the_one_face_forget_drops_it_whatever_the_model_calls() {
    for (at, said) in [
        "No, turn the lights on.",
        "Yes, but don't forget it.",
        "No. What time is it?",
        "No, no, no, please don't forget that, lamp.",
        "Don't scratch that.",
    ]
    .into_iter()
    .enumerate()
    {
        // Someone else may be in view: the note names nothing kept.
        crate::services::set_alone_for_test(Some(false));
        let got = asked_once_chats(
            &format!("ask-once-no-called-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            forget_umbrella_agreed(),
            false,
            None,
        )
        .await;
        crate::services::set_alone_for_test(None);
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{said:?}: kept");
        assert!(
            forget_told(&got, &crate::remember::said_no_told(None)),
            "{said:?}: told it is kept: {:?}",
            got.rounds
        );
    }
}

/// #8165: a line that says forget and takes it back ("No, don't keep it,
/// forget it. No wait, don't delete it."), which the model takes as a yes,
/// runs nothing: the model is told to ask once more, and its next line's
/// yes, seen said, runs it.
#[tokio::test]
async fn a_no_and_a_forget_said_together_is_asked_once_more() {
    let mut chats = forget_umbrella_then(&[]);
    chats.extend(agreed_with(json!({}), "Should I forget it?"));
    chats.extend(agreed_with(json!({}), "Done."));
    let got = asked_once_chats(
        "ask-once-no-and-forget",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line(
                "No, don't keep it, forget it. No wait, don't delete it.",
                vec![("f1", true)],
                5,
            ),
            line("Yes.", vec![("f1", true)], 10),
        ],
        chats,
        false,
        None,
    )
    .await;
    assert!(
        forget_told(&got, crate::remember::FORGET_ASKED_AGAIN),
        "asked once more: {:?}",
        got.rounds
    );
    assert_eq!(
        forgets_ran(&got),
        [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})],
        "their yes after runs it, once"
    );
}

/// #8165: a yes, a misheard "Lamp, yes.", a pleasantry's no ("No problem,
/// go ahead.") and a no that forgets ("No, just forget it.") still run the
/// one-face forget on the model's call.
#[tokio::test]
async fn a_yes_and_lamp_yes_still_run_the_one_face_forget() {
    for (at, said) in [
        "Yes.",
        "Lamp, yes.",
        "No problem, go ahead.",
        "No, just forget it.",
        "No, get rid of it.",
        "No, drop it.",
    ]
    .into_iter()
    .enumerate()
    {
        let got = asked_once_chats(
            &format!("ask-once-yes-runs-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            forget_umbrella_agreed(),
            false,
            None,
        )
        .await;
        assert_eq!(
            forgets_ran(&got),
            [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})],
            "{said:?}: runs"
        );
    }
}

/// #8165, Friday's third point: the said-no note names the kept thing only
/// with no one else in view (the private read-backs' gate); with another
/// face in view it names nothing. A no that says more, so the model
/// answers it (#8185).
#[tokio::test]
async fn a_no_with_someone_else_in_view_is_told_naming_nothing_kept() {
    for (alone, in_view, note) in [
        (
            false,
            vec![("f1", true), ("f2", false)],
            crate::remember::said_no_told(None),
        ),
        (
            true,
            vec![("f1", true)],
            crate::remember::said_no_told(Some(UMBRELLA.1)),
        ),
    ] {
        crate::services::set_alone_for_test(Some(alone));
        let got = asked_once_chats(
            &format!("ask-once-no-named-{alone}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("No, turn the lights on.", in_view, 5),
            ],
            forget_umbrella_agreed(),
            false,
            None,
        )
        .await;
        crate::services::set_alone_for_test(None);
        assert_eq!(
            forgets_ran(&got),
            Vec::<Value>::new(),
            "alone {alone}: kept"
        );
        assert!(
            forget_told(&got, &note),
            "alone {alone}: told {note:?}: {:?}",
            got.rounds
        );
        let named = got.rounds.iter().flatten().any(|message| {
            format!("{message:?}").contains("they said no")
                && format!("{message:?}").contains(UMBRELLA.1)
        });
        assert_eq!(named, alone, "alone {alone}: the umbrella named");
    }
}

/// #8185 (1): their plain yes, seen saying it, runs the one-face forget
/// before round 1 though the model calls no forget, and round 1 is told it
/// ran.
#[tokio::test]
async fn a_plain_yes_runs_the_one_face_forget_when_the_model_calls_none() {
    let a = crate::remember::person_id("A");
    for (at, said) in ["Yes.", "Yes please.", "Okay lamp, go ahead.", "Sure."]
        .into_iter()
        .enumerate()
    {
        let got = asked_once(
            &format!("ask-once-plain-yes-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            &["Done, I forgot it."],
            false,
        )
        .await;
        assert_eq!(
            forgets_ran(&got),
            [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})],
            "{said:?}: runs"
        );
        assert_eq!(got.calls, [Some(a.clone())], "{said:?}: in A's scope only");
        assert_eq!(got.rounds.len(), 3, "{said:?}: one round for the yes");
    }
}

/// #8185 (1): a plain yes runs the held forget before round 1, and the
/// model's own `forget` on that line, bare or by the held id, runs nothing
/// more: it is told the forget ran.
#[tokio::test]
async fn a_plain_yes_and_the_models_forget_run_it_once() {
    for (at, args) in [
        json!({}),
        json!({"source_id": UMBRELLA.0}),
        json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut chats = forget_umbrella_then(&[]);
        chats.extend(agreed_with(args.clone(), "Done."));
        let got = asked_once_chats(
            &format!("ask-once-plain-yes-once-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("Yes.", vec![("f1", true)], 5),
            ],
            chats,
            false,
            None,
        )
        .await;
        assert_eq!(
            forgets_ran(&got),
            [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})],
            "{args}: once"
        );
        assert!(
            forget_told(
                &got,
                &json!({"error": crate::remember::HELD_FORGET_RAN}).to_string()
            ),
            "{args}: told it ran: {:?}",
            got.rounds
        );
    }
}

/// #8185 (1): a yes that is not a plain yes ("Sure, forget it.", a
/// misheard "Loan. Yes.") stays the model's call: with no `forget` from
/// the model it runs nothing.
#[tokio::test]
async fn a_natural_yes_still_waits_for_the_models_call() {
    for (at, said) in ["Sure, forget it.", "Loan. Yes.", "Yes, what's the weather?"]
        .into_iter()
        .enumerate()
    {
        let got = asked_once(
            &format!("ask-once-natural-yes-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            &["Okay."],
            false,
        )
        .await;
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{said:?}: kept");
    }
}

/// #8185 (3): a plain yes their face was not seen saying, outside the
/// private room, runs nothing, whatever the model calls.
#[tokio::test]
async fn a_plain_yes_not_seen_saying_it_runs_nothing() {
    let got = asked_once_chats(
        "ask-once-plain-yes-unseen",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", false)], 5),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
}

/// #8185 (2): a no that is only a no, their face seen saying it, is
/// answered "Okay, I'll keep it." by the lamp, with no model round, and
/// nothing is deleted, though the model would have called forget.
#[tokio::test]
async fn a_plain_no_is_answered_okay_ill_keep_it_without_the_model() {
    for (at, said) in [
        "No.",
        "Lamp, no.",
        "No, keep it.",
        "No, no, keep it.",
        "Don't.",
        "No, don't forget it.",
        "No, don't get rid of it.",
        "Don't drop it.",
        "No, never drop that.",
    ]
    .into_iter()
    .enumerate()
    {
        let got = asked_once_chats(
            &format!("ask-once-only-no-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line(said, vec![("f1", true)], 5),
            ],
            forget_umbrella_agreed(),
            false,
            None,
        )
        .await;
        assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "{said:?}: kept");
        assert!(
            said_line(&got, crate::remember::KEPT_IT_LINE),
            "{said:?}: the lamp's line: {:?}",
            got.labels
        );
        assert_eq!(
            got.rounds.len(),
            2,
            "{said:?}: no model round for the no: {:?}",
            got.rounds
        );
        // The lamp's own words, as its other own lines (#8215's P3).
        assert_eq!(
            got.labels
                .iter()
                .filter(|label| label.starts_with("own_words:"))
                .count(),
            1,
            "{said:?}: {:?}",
            got.labels
        );
    }
}

/// #8185 (2): a no carrying another request goes to the model, told they
/// said no, so the request is not lost; nothing is deleted, and the lamp
/// does not say its own line.
#[tokio::test]
async fn a_no_with_another_request_goes_to_the_model() {
    let got = asked_once_chats(
        "ask-once-no-and-lights",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("No, turn the lights on.", vec![("f1", true)], 5),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
    assert!(got.rounds.len() > 2, "the model answers: {:?}", got.rounds);
    assert!(
        !said_line(&got, crate::remember::KEPT_IT_LINE),
        "{:?}",
        got.labels
    );
}

/// #8185 (2): "Okay, I'll keep it." in English is said by the voice as the
/// turn's piece; in the owner's language only from its clip rendered
/// ahead, which is rendered with `LAMP_PERSON_SCOPE_ONE_FACE_READS`. Until
/// then the model answers the no.
#[tokio::test]
async fn the_kept_line_waits_for_its_clip_in_the_owners_language() {
    use crate::language::Language;
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    assert_eq!(kept_line_clip(&engine.shared), Some(Vec::new()), "English");
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let engine = engine
        .in_language(Some(Language::Vietnamese))
        .reading_by_one_face(true);
    assert_eq!(
        crate::language::fixed_in(Some(Language::Vietnamese), crate::remember::KEPT_IT_LINE),
        "\u{110}\u{1b0}\u{1ee3}c, m\u{ec}nh s\u{1ebd} gi\u{1eef} l\u{1ea1}i."
    );
    assert_eq!(kept_line_clip(&engine.shared), None, "not rendered yet");
    engine.render_lines();
    tokio::time::timeout(Duration::from_secs(2), async {
        while kept_line_clip(&engine.shared).is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("rendered ahead in the owner's language");
    assert!(kept_line_clip(&engine.shared).is_some_and(|clip| !clip.is_empty()));
}

/// #8185 (2), the second read's P3 on #8215: in the owner's language with
/// the kept line not rendered ahead yet, a no that is only a no goes to the
/// model, told beside the line that they said no and it is kept, and its
/// forget call answered the same; nothing is deleted, and the lamp says no
/// line of its own.
#[tokio::test]
async fn a_plain_no_with_its_clip_not_rendered_is_the_models_with_the_note() {
    let vietnamese = Some(crate::language::Language::Vietnamese);
    LANGUAGE.with(|language| language.set(vietnamese));
    // The line is rendered ahead as each line starts: the voice gives it
    // no sound, so it is never kept.
    let kept = crate::language::fixed_in(vietnamese, crate::remember::KEPT_IT_LINE);
    SILENT_ONCE.with(|silent| *silent.borrow_mut() = vec![kept; 8]);
    crate::services::set_alone_for_test(Some(false));
    let got = asked_once_chats(
        "ask-once-only-no-unrendered",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("No.", vec![("f1", true)], 5),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    crate::services::set_alone_for_test(None);
    let note = crate::remember::said_no_told(None);
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
    assert!(got.rounds.len() > 2, "the model answers: {:?}", got.rounds);
    assert!(
        got.rounds
            .iter()
            .flatten()
            .any(|message| *message == Message::system(note.clone())),
        "told beside the line: {:?}",
        got.rounds
    );
    assert!(forget_told(&got, &note), "on its forget: {:?}", got.rounds);
    assert!(
        !got.labels
            .iter()
            .any(|label| label.starts_with("own_words:")),
        "{:?}",
        got.labels
    );
}

/// The forget call, then the lamp's question `asks`, then the model's
/// "Okay." with no call for the line after it.
fn forget_umbrella_asked(asks: &'static str) -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"about": "umbrella"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text(asks), Ok(ChatEvent::Done)],
        vec![text("Okay."), Ok(ChatEvent::Done)],
    ]
}

/// The second read's P2 on #8215: a plain yes runs the forget in code only
/// when the lamp's last question offered it alone. After "…, or keep it?"
/// or a question after it ("Or shall I set a reminder instead?") "Yes." is
/// the model's call, and with no forget from the model nothing runs; after
/// the plain question it runs.
#[tokio::test]
async fn a_plain_yes_to_a_question_offering_more_is_the_models_call() {
    for (at, asks, runs) in [
        (
            0,
            "Shall I forget what you kept about the umbrella, or keep it?",
            false,
        ),
        (
            1,
            "Shall I forget what you kept about the umbrella? Or shall I set a reminder instead?",
            false,
        ),
        (2, "Shall I forget what you kept about the umbrella?", true),
    ] {
        let got = asked_once_chats(
            &format!("ask-once-offers-more-{at}"),
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("Yes.", vec![("f1", true)], 5),
            ],
            forget_umbrella_asked(asks),
            false,
            None,
        )
        .await;
        let want = if runs {
            vec![json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})]
        } else {
            vec![]
        };
        assert_eq!(forgets_ran(&got), want, "{asks:?}");
    }
}

/// A held one-face forget of the umbrella (id 7), asked about alone, held
/// in request 1 for `asker` at `at`.
fn held_umbrella(engine: &Engine, asker: &HeldAsker, at: Instant) {
    if let Ok(mut hold) = engine.shared.held_forget.lock() {
        hold.held = Some(HeldForget {
            call: call("f1", crate::remember::FORGET, json!({"source_id": 7})),
            request: 1,
            asker: asker.clone(),
            at,
            asked: true,
            spelling: false,
            carried: false,
            line: None,
            asked_again: false,
            one_face: true,
            near: None,
            offered_alone: true,
            face_yes: false,
        });
    }
}

/// The second read's P3 on #8215, (a) and (d): a plain yes from the same
/// person in a later session runs nothing, and "Yes, but keep the
/// umbrella." is no plain yes: the model's call. Run through
/// `take_held_forget` in the private room, on a line theirs by one face.
#[tokio::test]
async fn a_plain_yes_from_a_later_session_or_with_a_but_runs_nothing_in_code() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    let later = (8, crate::remember::person_id("A"));
    crate::person::set_private_room_for_test(Some(true));
    let now = Instant::now();
    held_umbrella(&engine, &asker, now);
    let taken = take_held_forget(
        &engine.shared,
        2,
        true,
        "Yes.",
        (None, Some(&later)),
        true,
        now,
    );
    assert!(
        !matches!(taken, Taken::Runs(..) | Taken::Agrees(_)),
        "a later session runs nothing"
    );
    held_umbrella(&engine, &asker, now);
    let taken = take_held_forget(
        &engine.shared,
        2,
        true,
        "Yes, but keep the umbrella.",
        (None, Some(&asker)),
        true,
        now,
    );
    assert!(matches!(taken, Taken::Agrees(_)), "the model's call");
    crate::person::set_private_room_for_test(None);
}

/// The second read's P3 on #8215, (b): a held forget replaced by a new
/// held call (`hold_forget_as`) is gone: a plain yes after it never runs
/// the old one, only the new one once the lamp asked about it alone.
#[tokio::test]
async fn a_plain_yes_after_the_hold_was_replaced_runs_only_the_new_one() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    crate::person::set_private_room_for_test(Some(true));
    let now = Instant::now();
    held_umbrella(&engine, &asker, now);
    if let Ok(mut hold) = engine.shared.held_forget.lock() {
        hold.latest = 2;
    }
    let coffee = call("f2", crate::remember::FORGET, json!({"source_id": 9}));
    assert!(matches!(
        hold_forget_as(
            &engine.shared,
            2,
            &coffee,
            asker.clone(),
            now,
            (true, false)
        ),
        Holding::Held
    ));
    // Not yet asked about: a yes runs nothing.
    let taken = take_held_forget(
        &engine.shared,
        3,
        true,
        "Yes.",
        (None, Some(&asker)),
        true,
        now,
    );
    assert!(!matches!(taken, Taken::Runs(..)), "the old one never runs");
    // Held again, and asked about alone: the plain yes runs the new one.
    if let Ok(mut hold) = engine.shared.held_forget.lock() {
        hold.latest = 4;
    }
    assert!(matches!(
        hold_forget_as(
            &engine.shared,
            4,
            &coffee,
            asker.clone(),
            now,
            (true, false)
        ),
        Holding::Held
    ));
    settle_held_forget(
        &engine.shared,
        4,
        Some("Shall I forget what you kept about your coffee?"),
    );
    match take_held_forget(
        &engine.shared,
        5,
        true,
        "Yes.",
        (None, Some(&asker)),
        true,
        now,
    ) {
        Taken::Runs(call, None, true) => assert_eq!(call.args, json!({"source_id": 9})),
        _ => panic!("the new one runs"),
    }
    crate::person::set_private_room_for_test(None);
}

/// The second read's P3 on #8215, (c), and the person's ruling on it: the
/// privacy switch closed and opened again between the question and the
/// yes drops the held forget (`Engine::privacy`), as it drops a waiting
/// write, so the same person's plain yes within 30 s runs nothing in code
/// and goes to the model as any line; the umbrella is kept.
#[tokio::test]
async fn a_plain_yes_after_privacy_closed_and_opened_runs_nothing_in_code() {
    let script = Arc::new(Script::default());
    let (mut engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    crate::person::set_private_room_for_test(Some(true));
    let now = Instant::now();
    engine.privacy((true, 1, true, 1));
    held_umbrella(&engine, &asker, now);
    engine.privacy((false, 2, true, 1));
    engine.privacy((true, 3, true, 1));
    let held = engine.shared.held_forget.lock().unwrap().held.is_some();
    assert!(!held, "a privacy change drops the held forget");
    let taken = take_held_forget(
        &engine.shared,
        2,
        true,
        "Yes.",
        (None, Some(&asker)),
        true,
        now,
    );
    assert!(matches!(taken, Taken::Nothing), "nothing runs in code");
    crate::person::set_private_room_for_test(None);
}

/// #5627 (4): a yes 31 s after the question keeps it, and a yes after that
/// runs nothing either.
#[tokio::test]
async fn a_yes_past_thirty_seconds_keeps_it_and_a_later_yes_does_nothing() {
    let mut chats = forget_umbrella_agreed();
    chats.extend(agreed_with(json!({}), "Okay."));
    let got = asked_once_chats(
        "ask-once-late",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", true)], 31),
            line("Yes.", vec![("f1", true)], 2),
        ],
        chats,
        false,
        None,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
}

/// #7478 (1): their own yes to the one-face forget, seen saying it but
/// past 30 s, runs nothing, and the model is told it did not count, nothing
/// was forgotten and it is still kept, so it never says it is forgotten.
/// The model takes the line as their yes and calls `forget` (#7971): that
/// call runs nothing either.
#[tokio::test]
async fn their_late_yes_tells_the_model_it_is_kept() {
    let got = asked_once_chats(
        "ask-once-late-told",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", true)], 31),
        ],
        forget_umbrella_agreed(),
        false,
        None,
    )
    .await;
    assert_eq!(forgets_ran(&got), Vec::<Value>::new(), "kept");
    let told = Message::tool(
        "f2",
        crate::remember::unknown_yes_told(crate::remember::FORGET),
    );
    assert!(
        got.rounds.iter().flatten().any(|message| *message == told),
        "told it is kept, on its forget call: {:?}",
        got.rounds
    );
}

/// #5627 (5): with two faces in view the line is nobody's by one face: no
/// question, and the forget never reaches A's scope, as before.
#[tokio::test]
async fn with_two_faces_in_view_a_named_forget_is_not_asked_or_run_as_theirs() {
    let a = crate::remember::person_id("A");
    let got = asked_once(
        "ask-once-two-faces",
        vec![line(FORGET_UMBRELLA, vec![("f1", false), ("f2", false)], 0)],
        &[],
        false,
    )
    .await;
    assert!(!asked_about_umbrella(&got), "not asked");
    assert!(!got.calls.contains(&Some(a)), "never A's: {:?}", got.calls);
}

/// #5627 (6): a yes from another face, seen saying it, runs nothing: the
/// yes is bound to the person the question was about.
#[tokio::test]
async fn a_yes_from_another_person_does_not_run_the_one_face_forget() {
    for (what, in_view, room) in [
        ("ask-once-other-seen", vec![("f2", true)], false),
        ("ask-once-other-room", vec![("f2", true)], true),
        (
            "ask-once-other-beside",
            vec![("f1", false), ("f2", true)],
            true,
        ),
    ] {
        let got = asked_once_chats(
            what,
            vec![
                line(FORGET_UMBRELLA, vec![("f1", false)], 0),
                line("Yes.", in_view, 5),
            ],
            forget_umbrella_agreed(),
            room,
            None,
        )
        .await;
        assert!(
            !forgets_ran(&got).contains(&json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})),
            "{what}: kept: {:?}",
            forgets_ran(&got)
        );
        // The model's forget on it is told nothing was forgotten (#7490).
        assert!(told_dropped(&got), "{what}: not told: {:?}", got.rounds);
    }
}

/// One chat that makes the call `name` with `args`, then answers.
fn one_call(name: &str, args: Value) -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call("c1", name, args)])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Okay."), Ok(ChatEvent::Done)],
    ]
}

/// The privacy read's P2 on #5592, with `LAMP_PERSON_SCOPE_ONE_FACE_READS`:
/// A's face silent and alone in view. A `remember` that corrects a kept
/// thing by id (`replaces`) is refused like a forget, since it retires the
/// old fact; a plain new keep still lands in A's scope.
#[tokio::test]
async fn with_reads_a_correction_is_refused_and_a_plain_keep_still_writes() {
    let got = one_face_reading(
        "reads-replaces",
        "Lamp, my class is on Thursday now.",
        one_call(
            crate::remember::REMEMBER,
            json!({"text": "Class is on Thursday.", "replaces": 3}),
        ),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert!(got.calls.is_empty(), "never run: {:?}", got.calls);
    assert!(refused_by_one_face(&got), "told why");
    let a = crate::remember::person_id("A");
    let got = one_face_reading(
        "reads-plain-keep",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.calls, [Some(a)], "a plain keep: A's");
    assert!(!refused_by_one_face(&got));
    assert!(!one_face_refuses(
        crate::remember::REMEMBER,
        &json!({"text": "x", "replaces": 0}),
        false
    ));
}

/// The privacy read's P2 on #5592: on a one-face-read line no standing rule
/// is kept, listed or forgotten. A rule is told to every later session of
/// theirs, which acts for them beyond saving and recall.
#[tokio::test]
async fn with_reads_rules_are_never_kept_listed_or_forgotten() {
    for (what, name, args) in [
        (
            "reads-keep-rule",
            crate::rules::KEEP_RULE,
            json!({"rule": "Always call me Captain."}),
        ),
        ("reads-list-rules", crate::rules::LIST_RULES, json!({})),
        (
            "reads-forget-rule",
            crate::rules::FORGET_RULE,
            json!({"id": 1}),
        ),
    ] {
        let got = one_face_reading(
            what,
            "Lamp, about my rules.",
            one_call(name, args),
            vec![("f1", false)],
            None,
            Voice::One,
        )
        .await;
        assert!(got.calls.is_empty(), "{what}: never run: {:?}", got.calls);
        assert!(refused_by_one_face(&got), "{what}: told why");
    }
}

/// The privacy read on #5385 (before `LAMP_MEMORY_ROLE` went on in the
/// product): on a one-face-read line no role is set or ended. A guest off
/// camera, with the owner's face silent and alone in view, saying "Be
/// yourself again" or "You are Harry Potter now" would end or replace the
/// owner's standing role, which every later session of theirs is told.
/// Seen speaking it, the same calls run as theirs.
#[tokio::test]
async fn with_reads_a_role_is_never_set_or_ended() {
    let a = crate::remember::person_id("A");
    for (what, words, name, args) in [
        (
            "reads-end-role",
            "Be yourself again.",
            crate::remember::role::END_ROLE,
            json!({}),
        ),
        (
            "reads-set-role",
            "You are Harry Potter now.",
            crate::remember::role::SET_ROLE,
            json!({"name": "Harry Potter"}),
        ),
    ] {
        assert!(one_face_refuses(name, &args, false), "{what}");
        let got = one_face_reading(
            what,
            words,
            one_call(name, args.clone()),
            vec![("f1", false)],
            None,
            Voice::One,
        )
        .await;
        assert!(got.calls.is_empty(), "{what}: never run: {:?}", got.calls);
        assert!(refused_by_one_face(&got), "{what}: told why");
        let seen = format!("{what}-seen");
        let got = one_face_reading(
            &seen,
            words,
            one_call(name, args),
            vec![("f1", true)],
            None,
            Voice::One,
        )
        .await;
        assert_eq!(got.calls, [Some(a.clone())], "{what}: seen speaking: A's");
        assert!(!refused_by_one_face(&got), "{what}: seen speaking");
    }
}

/// The privacy read's P2 on #5592: a reminder's cancel or snooze on a
/// one-face-read line is refused, unless `LAMP_ONE_FACE_REMINDERS` is on
/// (its cancel risk accepted by the memory owner on 4 Nov). Listing stays.
#[tokio::test]
async fn with_reads_a_reminders_cancel_or_snooze_is_refused_unless_one_face_reminders() {
    for (what, name) in [
        ("reads-cancel-reminder", "cancel_reminder"),
        ("reads-snooze-reminder", "snooze_reminder"),
    ] {
        assert!(
            one_face_refuses(name, &json!({"id": 1}), false),
            "{what}: off"
        );
        assert!(
            !one_face_refuses(name, &json!({"id": 1}), true),
            "{what}: on"
        );
        if crate::tools::schedule_whose::one_face_on() {
            continue;
        }
        let got = one_face_reading(
            what,
            "Lamp, cancel my reminder.",
            one_call(name, json!({"id": 1})),
            vec![("f1", false)],
            None,
            Voice::One,
        )
        .await;
        assert!(got.calls.is_empty(), "{what}: never run: {:?}", got.calls);
        assert!(refused_by_one_face(&got), "{what}: told why");
    }
    assert!(!one_face_refuses("list_reminders", &json!({}), false));
}

/// One line by A, their face `f1` seen speaking it, with
/// [`Engine::keeping_sight_for_services`] as `on`: what is kept of who said
/// it, read after the turn.
async fn services_sight(on: bool, request: u64) -> Option<crate::speaking_faces::LineSeen> {
    let what = if on { "services-on" } else { "services-off" };
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file(what, &eyes);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, add lunch to my calendar.".to_owned())].into()),
        chats: Mutex::new([vec![text("Done."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let cache = Arc::new(FaceCache::default());
    let runner: ScopedToolRunner = Arc::new(|_, _, _, _, _| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([43; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|_: &ScopeLease| Told::from("SYSTEM".to_owned()))),
        )
        .seeing_who_speaks(eyes.clone(), path.clone())
        .scoping_by_sight(true)
        .keeping_sight_for_services(on)
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    let done = Arc::new(AtomicBool::new(false));
    let feeder = feed(eyes, true, BOOT, done.clone());
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    tokio::time::sleep(Duration::from_millis(40)).await;
    engine.submit(end(request));
    turn(&mut engine).await;
    done.store(true, Ordering::Release);
    feeder.join().unwrap();
    clean(&path);
    crate::speaking_faces::line_seen(request)
}

/// With `LAMP_SERVICES`, a line A was seen saying is kept as theirs for its
/// services write, without `LAMP_MEMORY_ROOM_SPEAKER` (lamp-9adf, 3 Nov:
/// every calendar write refused as not seen said); without it, nothing.
#[tokio::test]
async fn a_line_seen_said_by_its_person_is_kept_for_a_services_write() {
    let kept = services_sight(true, 5_126_301).await;
    assert_eq!(
        crate::remember::speaker_of(kept.as_ref()),
        Some(crate::remember::person_id("A"))
    );
    assert!(services_sight(false, 5_126_302).await.is_none());
}

/// One line said with `in_view` in the eyes' looks, admitted as A's (the
/// face cache names A, face `f1`), and the scope each of its memory hits
/// asks read, with the rule that chose it (`LAMP_SECOND_MEMORY_HITS`, the
/// memory owner's order of 4 Nov).
async fn hits_for(what: &str, in_view: Vec<(&'static str, bool)>) -> Vec<HitsRead> {
    let chats = vec![vec![text("Pretzels?"), Ok(ChatEvent::Done)]];
    one_line_with(
        what,
        "Lamp, I'm hungry, any ideas?",
        chats,
        in_view,
        BOOT,
        true,
        true,
        None,
        None,
        true,
    )
    .await
    .hits
}

/// (a) A's own face `f1` seen speaking the line while another face, `f2`
/// (B, or anyone), sits silent in view: A's hits, read as seen speaking,
/// never B's.
#[tokio::test]
async fn a_line_seen_spoken_by_a_gets_as_hits_with_b_silent_in_view() {
    let a = crate::remember::person_id("A");
    let hits = hits_for("hits-a-speaks", vec![("f1", true), ("f2", false)]).await;
    assert_eq!(
        hits,
        [Some((Some(a), crate::remember::HitsPath::Speaking))],
        "A's, by the face seen speaking"
    );
}

/// (b) A's known face silent and alone in view, no face seen speaking the line at
/// all: no speaking evidence, so the hits fall back to the known face's
/// scope (the off-camera risk of #3371 the owner accepted on 4 Nov). Only
/// the hits: the line's lease, its context and every call stay the room's.
#[tokio::test]
async fn a_line_with_one_silent_known_face_and_no_speaking_evidence_gets_the_faces_hits() {
    let a = crate::remember::person_id("A");
    for (what, in_view) in [("hits-silent", vec![("f1", false)])] {
        let chats = vec![vec![text("Pretzels?"), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "Lamp, I'm hungry, any ideas?",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            None,
            true,
        )
        .await;
        assert_eq!(
            got.hits,
            [Some((Some(a.clone()), crate::remember::HitsPath::Face))],
            "{what}: the face's hits, by the fallback"
        );
        assert_eq!(got.lease, None, "{what}: all else the room's");
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
}

/// The first builder's call on #5272: the fallback needs A's face to be
/// the only face in view. Two silent faces, A's among them, and no other
/// voice heard (with or without the one-face rule): a guest sitting there
/// would hear A's kept notes, so the hits are the room's. A alone, as the
/// control: A's, by the face.
#[tokio::test]
async fn a_second_silent_face_in_view_keeps_the_hits_the_rooms() {
    let hits = hits_for("hits-two-silent", vec![("f1", false), ("f2", false)]).await;
    assert_eq!(
        hits,
        [Some((None, crate::remember::HitsPath::Room))],
        "two silent faces: the room's, never A's"
    );
    let got = one_face_line(
        "hits-two-silent-one-voice",
        "Lamp, I'm hungry, any ideas?",
        vec![vec![text("Pretzels?"), Ok(ChatEvent::Done)]],
        vec![("f1", false), ("f2", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(
        got.hits,
        [Some((None, crate::remember::HitsPath::Room))],
        "two silent faces, no other voice: the room's"
    );
    assert_eq!(got.lease, None);
    let hits = hits_for("hits-alone-control", vec![("f1", false)]).await;
    assert_eq!(
        hits,
        [Some((
            Some(crate::remember::person_id("A")),
            crate::remember::HitsPath::Face
        ))],
        "A alone: the face's hits"
    );
}

/// (c) A's known face silent in view while another face, unknown (or
/// another known person's), is seen speaking the line: the speaking
/// evidence says someone else said it, so the silent face's scope is never
/// read. The room's, as the line's lease and every other read of it are:
/// the owner accepted the fallback only where nothing was seen speaking.
#[tokio::test]
async fn a_line_seen_spoken_by_another_face_never_gets_the_silent_known_faces_hits() {
    let hits = hits_for("hits-other-speaks", vec![("f1", false), ("f2", true)]).await;
    assert_eq!(
        hits,
        [Some((None, crate::remember::HitsPath::Room))],
        "the room's, never A's"
    );
    // Two faces seen speaking it, A's among them: not A's alone, the room's.
    let hits = hits_for("hits-both-speak", vec![("f1", true), ("f2", true)]).await;
    assert_eq!(hits, [Some((None, crate::remember::HitsPath::Room))]);
}

/// No known face in view (an unknown face silent, or nobody): nobody to
/// fall back to, the room's.
#[tokio::test]
async fn a_line_with_no_known_face_in_view_gets_the_rooms_hits() {
    for (what, in_view) in [
        ("hits-stranger", vec![("f2", false)]),
        ("hits-empty", vec![]),
    ] {
        let hits = hits_for(what, in_view).await;
        assert_eq!(
            hits,
            [Some((None, crate::remember::HitsPath::Room))],
            "{what}"
        );
    }
}

/// Heron's P2 on #5272: another voice the one-face rule
/// (`LAMP_PERSON_SCOPE_ONE_FACE`) heard in the conversation, earlier or as
/// the line was judged, takes the face-in-view fallback back, with A's face
/// alone in view or beside a second silent face: any sign of someone else
/// wins. With no other voice heard, the fallback still holds.
#[tokio::test]
async fn another_voice_heard_takes_the_face_in_view_hits_back() {
    for (what, in_view, voice) in [
        (
            "hits-voice-before",
            vec![("f1", false)],
            Voice::AnotherBefore,
        ),
        ("hits-voice-in-it", vec![("f1", false)], Voice::AnotherInIt),
        (
            "hits-two-voice-before",
            vec![("f1", false), ("f2", false)],
            Voice::AnotherBefore,
        ),
        (
            "hits-two-voice-in-it",
            vec![("f1", false), ("f2", false)],
            Voice::AnotherInIt,
        ),
    ] {
        let got = one_face_line(
            what,
            "Lamp, I'm hungry, any ideas?",
            vec![vec![text("Pretzels?"), Ok(ChatEvent::Done)]],
            in_view,
            None,
            voice,
        )
        .await;
        assert_eq!(
            got.hits,
            [Some((None, crate::remember::HitsPath::Room))],
            "{what}: the room's, never A's"
        );
        assert_eq!(got.lease, None, "{what}");
    }
    let got = one_face_line(
        "hits-one-voice",
        "Lamp, I'm hungry, any ideas?",
        vec![vec![text("Pretzels?"), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(
        got.hits,
        [Some((
            Some(crate::remember::person_id("A")),
            crate::remember::HitsPath::Face
        ))],
        "no other voice: the face's hits"
    );
}

/// Heron's P2 on #5508 (`LAMP_MEMORY_DONT_KNOW_RECALLS`): a line no face
/// was seen speaking, A's face in view and silent, in A's conversation going
/// on, is carried (`LAMP_CONVERSATION_CARRIES`), and an off-camera guest
/// beside A looks the same. Its don't-know, "I don't have a time for your
/// yoga class" (lamp-06c1, 5 Nov, S14, request 75), searches nothing of A's:
/// the guest is never told A's kept class time. The room's line the same.
#[tokio::test]
async fn a_carried_lines_dont_know_searches_nothing_of_theirs() {
    let first = "I don't have a time for your yoga class — that's not something you've told me. When is it?";
    let kept = "- My yoga class is at seven in the morning.";
    for (what, going_on) in [("carried", Some(true)), ("room's", None)] {
        let searched: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let seen = searched.clone();
        DONT_KNOW_FOUND.with(|found| {
            *found.borrow_mut() = Some(Arc::new(
                move |_, words: &str, lease: Option<&ScopeLease>| {
                    let person = lease
                        .and_then(ScopeLease::scope)
                        .and_then(|scope| scope.person().map(str::to_owned));
                    seen.lock().unwrap().push(person.clone());
                    // Nobody's for the room's, as `worker::dont_know_whose`.
                    (person.is_some() && words.contains("yoga")).then(|| kept.to_owned())
                },
            ));
        });
        let chats = vec![
            vec![text(first), Ok(ChatEvent::Done)],
            vec![text("Seven in the morning."), Ok(ChatEvent::Done)],
        ];
        let got = one_line_with(
            what,
            "What time is my yoga class?",
            chats,
            vec![("f1", false)],
            BOOT,
            true,
            true,
            None,
            going_on,
            true,
        )
        .await;
        let said: String = got
            .labels
            .iter()
            .filter_map(|label| label.strip_prefix("said:"))
            .collect();
        if going_on.is_some() {
            assert!(got.held, "{what}: told from the conversation as it stood");
        }
        assert!(
            searched.lock().unwrap().iter().all(Option::is_none),
            "{what}: nobody's kept things searched"
        );
        assert_eq!(said, first, "{what}: said as it came");
        assert_eq!(got.rounds.len(), 1, "{what}: never asked again");
    }
}

/// One line no face was seen speaking, A's face silent in view, in A's
/// conversation going on: carried, with `LAMP_CONVERSATION_CARRIED_SAVES`
/// on as on the product, and `memory` as `LAMP_CONVERSATION_CARRIED_MEMORY`.
/// With `named` false it is admitted as the room's.
async fn carried_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    named: bool,
    memory: bool,
    voice: Voice,
) -> Outcome {
    CARRIED_MEMORY.with(|on| on.set(memory));
    lines_saving(
        what,
        vec![(words.to_owned(), in_view, 0)],
        chats,
        BOOT,
        true,
        true,
        None,
        Some(true),
        named,
        true,
        voice,
    )
    .await
}

/// A recall about A's doctor, and its answer.
fn recall_doctor() -> Vec<Vec<net::Result<ChatEvent>>> {
    one_call(crate::remember::RECALL, json!({"query": "doctor"}))
}

/// The memory owner's decision of 9 Nov (`LAMP_CONVERSATION_CARRIED_MEMORY`,
/// after lamp-06c1's S14 run): a carried line's keep is written in A's scope
/// at once, its own line or one admitted as the room's, told and read as
/// A's, not held. Off, it is held for A's seen yes, as before.
#[tokio::test]
async fn with_carried_memory_a_carried_keep_is_written_in_their_scope_at_once() {
    let a = crate::remember::person_id("A");
    for named in [true, false] {
        let what = if named { "cm-keep-own" } else { "cm-keep-room" };
        let got = carried_line(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            named,
            true,
            Voice::Judged,
        )
        .await;
        assert_eq!(got.calls, [Some(a.clone())], "{what}: written in A's scope");
        assert_eq!(got.lease, Some(a.clone()), "{what}");
        assert!(!got.held, "{what}: read as A's");
        assert!(
            !got.rounds[1].contains(&Message::tool(
                "k1".to_owned(),
                crate::remember::held_keep_told().to_string()
            )),
            "{what}: not held for a yes"
        );
        let what = if named {
            "cm-keep-own-off"
        } else {
            "cm-keep-room-off"
        };
        let got = carried_line(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            named,
            false,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: nothing written at once");
        assert!(got.held, "{what}");
        assert!(
            got.rounds[1].contains(&Message::tool(
                "k1".to_owned(),
                crate::remember::held_keep_told().to_string()
            )),
            "{what}: held for A's yes"
        );
    }
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY`: a carried line's recall reads A's
/// memory, and so do its note and hits, as a lone known face's line does
/// (lamp-06c1, S14: 11 of 14 asks refused on carried lines). Off, the recall
/// runs for no one and nothing is read, as before.
#[tokio::test]
async fn with_carried_memory_a_carried_recall_reads_their_memory() {
    let a = crate::remember::person_id("A");
    let ask = "Lamp, what did I say about my doctor?";
    let got = carried_line(
        "cm-recall",
        ask,
        recall_doctor(),
        vec![("f1", false)],
        true,
        true,
        Voice::Judged,
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())], "the recall is A's");
    assert_eq!(
        got.told,
        Message::system(format!("for {:?}", Some(a.as_str()))),
        "A's context"
    );
    assert!(
        !got.noted.is_empty()
            && got
                .noted
                .iter()
                .all(|(person, _)| person.as_ref() == Some(&a)),
        "A's note: {:?}",
        got.noted
    );
    assert!(
        got.hits.iter().all(|hits| hits
            .as_ref()
            .is_some_and(|(person, _)| person.as_ref() == Some(&a))),
        "A's hits: {:?}",
        got.hits
    );
    let got = carried_line(
        "cm-recall-off",
        ask,
        recall_doctor(),
        vec![("f1", false)],
        true,
        false,
        Voice::Off,
    )
    .await;
    assert_eq!(got.calls, [], "off: the recall runs for no one");
    assert!(got.held);
    assert!(got.noted.is_empty(), "off: nothing read: {:?}", got.noted);
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` never forgets, retires or changes: on
/// a carried line a forget of one thing or of everything, a correction of a
/// kept thing and a standing rule never run, on or off; on, the lamp is told
/// why, as on a lone known face's line.
#[tokio::test]
async fn with_carried_memory_a_carried_line_never_forgets_corrects_or_keeps_rules() {
    for memory in [true, false] {
        for (what, words, chats) in [
            ("cm-forget-one", FORGET_COFFEE, forget_coffee()),
            ("cm-forget-all", FORGET_ME, forget_everything()),
            (
                "cm-replaces",
                "Lamp, my class is on Thursday now.",
                one_call(
                    crate::remember::REMEMBER,
                    json!({"text": "Class is on Thursday.", "replaces": 3}),
                ),
            ),
            (
                "cm-keep-rule",
                "Lamp, always call me Captain.",
                one_call(
                    crate::rules::KEEP_RULE,
                    json!({"rule": "Always call me Captain."}),
                ),
            ),
        ] {
            let got = carried_line(
                what,
                words,
                chats,
                vec![("f1", false)],
                true,
                memory,
                Voice::Judged,
            )
            .await;
            assert!(
                got.calls.is_empty(),
                "{what} {memory}: never run: {:?}",
                got.calls
            );
            if memory {
                assert!(refused_by_one_face(&got), "{what}: told why");
            }
        }
    }
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` widens nothing about who is carried:
/// a line carried for A reads and keeps A's only, and a line with another
/// face in view or seen speaking it, no conversation going on, or another
/// voice recorded in the conversation (before it or as it is judged) reads
/// and keeps nobody's but the room's, or stays held, never anyone else's.
#[tokio::test]
async fn with_carried_memory_a_carried_line_never_reads_another_persons_memory() {
    let a = crate::remember::person_id("A");
    for (what, in_view) in [
        ("cm-other-face", vec![("f2", false)]),
        ("cm-two-faces", vec![("f1", false), ("f2", false)]),
        ("cm-other-speaks", vec![("f1", false), ("f2", true)]),
    ] {
        let got = carried_line(
            what,
            "Lamp, what did I say about my doctor?",
            recall_doctor(),
            in_view,
            true,
            true,
            Voice::Judged,
        )
        .await;
        assert_eq!(got.calls, [None], "{what}: the room's, never A's");
        assert_eq!(got.lease, None, "{what}");
    }
    CARRIED_MEMORY.with(|on| on.set(true));
    let got = lines_saving(
        "cm-quiet",
        vec![(THE_HOTEL.to_owned(), vec![("f1", false)], 0)],
        keep_the_hotel(),
        BOOT,
        true,
        true,
        None,
        Some(false),
        true,
        true,
        Voice::Off,
    )
    .await;
    assert_eq!(got.calls, [None], "no conversation going on: the room's");
    // Another voice in the conversation: never read as A's.
    for (what, voice) in [
        ("cm-voice-before", Voice::AnotherBefore),
        ("cm-voice-in-it", Voice::AnotherInIt),
    ] {
        let got = carried_line(
            what,
            "Lamp, what did I say about my doctor?",
            recall_doctor(),
            vec![("f1", false)],
            true,
            true,
            voice,
        )
        .await;
        assert!(
            got.calls.iter().all(|whose| whose.as_ref() != Some(&a)),
            "{what}: never A's: {:?}",
            got.calls
        );
        assert!(
            got.noted
                .iter()
                .all(|(person, _)| person.as_ref() != Some(&a)),
            "{what}: {:?}",
            got.noted
        );
    }
    // A's own carried line: every call, note and hit is A's, nobody else's.
    let got = carried_line(
        "cm-only-a",
        "Lamp, what did I say about my doctor?",
        recall_doctor(),
        vec![("f1", false)],
        false,
        true,
        Voice::Judged,
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())]);
    assert!(
        got.noted
            .iter()
            .all(|(person, _)| person.as_ref() == Some(&a))
    );
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` with no one-face tracker
/// (`LAMP_PERSON_SCOPE_ONE_FACE` off, so no other voice could ever be
/// recorded): a carried line is never read as A's. Its keep stays held for
/// A's seen yes and its recall runs for no one, as with the switch off: an
/// off-camera guest just after A's face is never told A's memory nor kept
/// into it (Thirteen's P2 on #5842).
#[tokio::test]
async fn with_carried_memory_and_no_voice_tracker_a_carried_line_stays_held() {
    for named in [true, false] {
        let what = if named {
            "cm-no-tracker-keep-own"
        } else {
            "cm-no-tracker-keep-room"
        };
        let got = carried_line(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            named,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: nothing written at once");
        assert!(got.held, "{what}: held");
        assert!(
            got.rounds[1].contains(&Message::tool(
                "k1".to_owned(),
                crate::remember::held_keep_told().to_string()
            )),
            "{what}: held for A's yes"
        );
        let what = if named {
            "cm-no-tracker-recall-own"
        } else {
            "cm-no-tracker-recall-room"
        };
        let got = carried_line(
            what,
            "Lamp, what did I say about my doctor?",
            recall_doctor(),
            vec![("f1", false)],
            named,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: the recall runs for no one");
        assert!(got.held, "{what}");
        assert!(
            got.noted.is_empty(),
            "{what}: nothing read: {:?}",
            got.noted
        );
    }
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` where the same-voice check is not
/// running for the route: it still acts (the memory owner's call, 10 Nov),
/// so a carried line the rule judged is read as A's; one the rule never
/// judged is still never read as A's (Jarvis's P2 on #5842).
#[tokio::test]
async fn with_carried_memory_and_no_voice_check_a_carried_line_is_theirs() {
    let a = crate::remember::person_id("A");
    for named in [true, false] {
        let what = if named {
            "cm-unheard-own"
        } else {
            "cm-unheard-room"
        };
        CARRIED_UNHEARD.with(|unheard| unheard.set(true));
        let got = carried_line(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            named,
            true,
            Voice::Judged,
        )
        .await;
        assert_eq!(got.calls, [Some(a.clone())], "{what}: written in A's scope");
        assert!(!got.held, "{what}: read as A's");
        // A tracker but no rule judging this very line: its voice unknown,
        // never read as A's.
        let what = if named {
            "cm-unjudged-own"
        } else {
            "cm-unjudged-room"
        };
        let got = carried_line(
            what,
            "Lamp, what did I say about my doctor?",
            recall_doctor(),
            vec![("f1", false)],
            named,
            true,
            Voice::One,
        )
        .await;
        assert!(
            got.calls.iter().all(|whose| whose.as_ref() != Some(&a)),
            "{what}: never A's: {:?}",
            got.calls
        );
        assert!(
            got.noted
                .iter()
                .all(|(person, _)| person.as_ref() != Some(&a)),
            "{what}: {:?}",
            got.noted
        );
    }
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` needs A's own face the one face in
/// view through the carried line, as a lone known face's read does: A seen
/// saying the line before, then gone as an off-camera voice asks at once,
/// within `CARRY_UNSEEN_WITHIN`, is carried but never read as A's (Jarvis's
/// P2 on #5842: A walked out). A's face still silent alone in view: read as
/// A's.
#[tokio::test]
async fn with_carried_memory_a_carried_line_with_their_face_gone_is_not_theirs() {
    let a = crate::remember::person_id("A");
    fn left(request: u64, known: &std::path::Path) {
        // Her face gone: the eyes write their known file no more.
        if request == 2 {
            let _ = std::fs::remove_file(known);
            let _ = std::fs::remove_file(known_face_of(known));
        }
    }
    fn stays(_: u64, _: &std::path::Path) {}
    type Faces = fn(u64, &std::path::Path);
    let cases = [
        ("cm-face-gone", vec![], left as Faces, false),
        ("cm-face-in-view", vec![("f1", false)], stays, true),
    ];
    for (what, in_view, faces, theirs) in cases {
        let mut chats = vec![vec![
            text("Garden centre on Saturday, noted."),
            Ok(ChatEvent::Done),
        ]];
        chats.extend(recall_doctor());
        CARRIED_MEMORY.with(|on| on.set(true));
        let got = lines_known(
            what,
            vec![
                (
                    "I'm going to the garden centre on Saturday.".to_owned(),
                    vec![("f1", true)],
                    0,
                ),
                (
                    "Lamp, what did I say about my doctor?".to_owned(),
                    in_view,
                    0,
                ),
            ],
            chats,
            (BOOT, true, true),
            None,
            Some(true),
            true,
            true,
            None,
            Voice::Judged,
            (false, &[]),
            faces,
            (false, None, None),
        )
        .await;
        assert_eq!(
            got.calls.contains(&Some(a.clone())),
            theirs,
            "{what}: {:?}",
            got.calls
        );
        assert_eq!(
            got.noted
                .iter()
                .any(|(person, words)| person.as_ref() == Some(&a) && words.contains("doctor")),
            theirs,
            "{what}: {:?}",
            got.noted
        );
    }
}

/// `LAMP_CONVERSATION_CARRIED_MEMORY` leaves the one-face rule as it was: a
/// lone known face's line not in a conversation going on is taken by that
/// rule, read and kept as theirs with its reads switch, the room's reads
/// without it, whether or not this switch is on.
#[tokio::test]
async fn with_carried_memory_a_one_face_line_is_as_before() {
    let a = crate::remember::person_id("A");
    for (memory, reads) in [(true, true), (true, false), (false, true), (false, false)] {
        let what = format!("cm-one-face-{memory}-{reads}");
        CARRIED_MEMORY.with(|on| on.set(memory));
        ONE_FACE_READS.with(|on| on.set(reads));
        let got = one_face_line(
            &what,
            TOKYO,
            remember_tokyo(),
            vec![("f1", false)],
            None,
            Voice::One,
        )
        .await;
        assert_eq!(got.calls, [Some(a.clone())], "{what}: the keep is A's");
        assert_eq!(got.lease, reads.then(|| a.clone()), "{what}");
        assert!(!got.held, "{what}");
    }
}

/// A carried line read as its person's (`LAMP_CONVERSATION_CARRIED_MEMORY`)
/// is still kept out of the journal and every talk note: only its keep is
/// written; nothing said off camera is journalled as theirs.
#[test]
fn a_carried_line_read_as_theirs_is_still_kept_out_of_the_journal() {
    use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
    let a = crate::remember::person_id("Carried Memory A");
    let request = 9_110_901_u64;
    let scope = RequestScope::new(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([44; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::room(),
    );
    let lease = scope.lease();
    assert!(lease.carry_conversation(&a));
    assert!(lease.read_carried());
    assert!(!lease.scope().unwrap().held(), "read as theirs");
    super::super::worker::keep_admitted(
        &ProviderOutput::HoldingAnswer {
            request,
            session: 1,
        },
        &lease,
    );
    assert!(crate::remember::kept_out(request));
    super::super::worker::keep_admitted(
        &ProviderOutput::Transcript {
            request: Some(request),
            text: "Your doctor is on Tuesday.".to_owned(),
            finished: true,
        },
        &lease,
    );
    let mut thread = crate::thread::THREAD.lock().unwrap();
    let later = Instant::now() + Duration::from_secs(1);
    let given = thread.summary_rest(Some(&a), later);
    assert!(
        given.iter().all(|(given, ..)| *given != request),
        "never in a talk note: {given:?}"
    );
}

/// Line 1, A's doctor asked, carried in A's conversation, A's face silent
/// alone in view, cut short before its answer played by line 2, `second`
/// (no words: a noise; or a short sound with words, held unjudged with
/// `sound`), which answers line 1's words in its place, with
/// `LAMP_CONVERSATION_CARRIED_MEMORY` on and `past_silent` as
/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT`.
async fn carried_ask_cut_by(
    what: &str,
    second: &str,
    sound: bool,
    past_silent: Option<(bool, crate::second_provider::AnotherVoice)>,
) -> Outcome {
    CARRIED_MEMORY.with(|on| on.set(true));
    CUT_FIRST.with(|cut| cut.set(Some(("Let me look.", sound))));
    let chats = std::iter::once(vec![text("Let me look."), Ok(ChatEvent::Done)])
        .chain(recall_doctor())
        .collect();
    lines_past_silent(
        what,
        vec![
            (
                "Lamp, what did I say about my doctor?".to_owned(),
                vec![("f1", false)],
                0,
            ),
            (second.to_owned(), vec![("f1", false)], 0),
        ],
        chats,
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
        true,
        past_silent,
        Voice::Judged,
    )
    .await
}

/// lamp-0c4e, 10 Nov, requests 238 and 239: a carried ask read as the
/// person's, its memory told with 3 hits, was cut before its answer played
/// by a 0-word line, which answered its words in its place; the rule never
/// judges a line with no words, so `LAMP_CONVERSATION_CARRIED_MEMORY`'s
/// fail-closed narrow made that line, and so the cut words' answer, the
/// room's: "nothing in my memory says that". The cut words keep the scope
/// their own line settled: A's recall, note and hits.
#[tokio::test]
async fn with_carried_memory_cut_words_answered_by_a_0_word_line_stay_theirs() {
    let a = crate::remember::person_id("A");
    let got = carried_ask_cut_by("cm-cut-empty", "", false, None).await;
    assert_eq!(got.calls, [Some(a.clone())], "the recall is A's");
    assert!(
        got.noted
            .iter()
            .all(|(person, words)| person.as_ref() == Some(&a) && words.contains("doctor")),
        "A's notes, on the doctor ask: {:?}",
        got.noted
    );
    // Told the cut words' answer in A's context, as line 1 was.
    assert_eq!(
        got.asked.last(),
        Some(&Message::system(format!("for {:?}", Some(a.as_str())))),
        "A's context"
    );
    assert!(
        got.hits.iter().all(|hits| hits
            .as_ref()
            .is_some_and(|(person, _)| person.as_ref() == Some(&a))),
        "A's hits: {:?}",
        got.hits
    );
}

/// The same cut with `LAMP_CONVERSATION_CARRIES_PAST_SILENT` on: its "the
/// rule never heard its voice" on the 0-word line leaks no narrow into the
/// cut words' answer either.
#[tokio::test]
async fn past_silent_cut_words_answered_by_a_0_word_line_stay_theirs() {
    let a = crate::remember::person_id("A");
    let got = carried_ask_cut_by(
        "ps-cut-empty",
        "",
        false,
        Some((
            true,
            Arc::new(|| false) as crate::second_provider::AnotherVoice,
        )),
    )
    .await;
    assert_eq!(got.calls, [Some(a.clone())], "the recall is A's");
    assert!(
        got.noted
            .iter()
            .all(|(person, _)| person.as_ref() == Some(&a)),
        "{:?}",
        got.noted
    );
}

/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT` alone (no carried reads): A's
/// face seen saying line 1, cut short by a 0-word line 2 carried past a
/// silent face, which answers line 1's words in its place. The rule never
/// heard line 2, and its "whether another voice spoke is unknown" made the
/// answer to A's own seen words the room's thread; now line 2 stays carried
/// in A's conversation. Another voice shown still makes it the room's.
#[tokio::test]
async fn past_silent_alone_seen_words_answered_by_a_0_word_line_stay_in_their_conversation() {
    let a = crate::remember::person_id("A");
    for (what, another) in [("ps-seen-cut", false), ("ps-seen-cut-another", true)] {
        CUT_FIRST.with(|cut| cut.set(Some(("Let me look.", false))));
        let got = lines_past_silent(
            what,
            vec![
                (
                    "Lamp, what did I say about my doctor?".to_owned(),
                    vec![("f1", true)],
                    0,
                ),
                (String::new(), vec![("f1", false)], 0),
            ],
            vec![
                vec![text("Let me look."), Ok(ChatEvent::Done)],
                vec![text("Tuesday."), Ok(ChatEvent::Done)],
            ],
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            false,
            Some((
                true,
                Arc::new(move || another) as crate::second_provider::AnotherVoice,
            )),
            Voice::Judged,
        )
        .await;
        assert_eq!(
            got.told,
            Message::system(format!("for {:?}", Some(a.as_str()))),
            "{what}: line 1 is A's, seen"
        );
        let whose = if another { None } else { Some(a.as_str()) };
        assert_eq!(
            got.asked.last(),
            Some(&Message::system(format!("for {whose:?}"))),
            "{what}: the cut words' answer"
        );
    }
}

/// A line with words that no rule judged (a short sound, held) is still
/// the room's when it answers cut words in its place: only a line with no
/// words of its own adds no doubt to whose they are.
#[tokio::test]
async fn with_carried_memory_cut_words_answered_by_a_held_line_with_words_are_the_rooms() {
    let a = crate::remember::person_id("A");
    let got = carried_ask_cut_by("cm-cut-held", "¿Qué?", true, None).await;
    assert!(
        got.calls.iter().all(|whose| whose.as_ref() != Some(&a)),
        "never A's: {:?}",
        got.calls
    );
    assert!(
        got.noted
            .iter()
            .skip(1)
            .all(|(person, _)| person.as_ref() != Some(&a)),
        "the held line's note is never A's: {:?}",
        got.noted
    );
}

/// Thirteen's row on #5627: the refusal of a forget on a one-face line
/// says why, after the words it said before (old greps still match), and
/// never with what they asked to forget (the Reviewer-lead's P2 on #6841).
#[test]
fn a_one_face_forget_not_asked_says_why_in_the_journal() {
    let why = crate::remember::ForgetNotAsked::SeveralMatch(
        crate::remember::ForgetWant::About("umbrella".to_owned()),
        2,
    );
    let line = crate::second_provider::one_face_forget_not_asked(12, &why);
    assert_eq!(
        line,
        "lamp-voice second: request 12: a forget on a one-face line names no one kept thing of theirs: not asked: two or more kept things match the words (2) (LAMP_PERSON_SCOPE_ONE_FACE_READS)"
    );
    assert!(
        !line.contains("umbrella"),
        "the about's words are never journaled: {line}"
    );
}

/// C63(b), lamp-06c1, 18 Nov (request 111): A names herself on `f1`, then
/// says the next line with a hand over her face. The eyes let her name go
/// (their known file gone), her face still the one `f1` alone in view. With
/// `LAMP_CONVERSATION_CARRIES_OWN_TRACK` that line is carried in her
/// conversation going on: the same unbroken track as the face last named as
/// her, alone in view. Another face alone, or a second face beside hers,
/// is still the room's; with the switch off, as before, the room's. A line
/// admitted as the room's, the face cache naming nobody, carries the same.
#[tokio::test]
async fn her_own_covered_face_carries_with_the_own_track_switch() {
    let a = crate::remember::person_id("A");
    fn let_go(request: u64, known: &std::path::Path) {
        if request == 2 {
            let _ = std::fs::remove_file(known);
            let _ = std::fs::remove_file(known_face_of(known));
        }
    }
    for (what, on, covered, carried, named) in [
        ("own-track-on", true, vec![("f1", false)], true, true),
        // With `LAMP_FACE_STICKY` too, the eyes' let-go on her box is
        // someone else's: never carried as hers (Friday's P2 on #7933).
        ("own-track-sticky", true, vec![("f1", false)], false, true),
        (
            "own-track-sticky-room",
            true,
            vec![("f1", false)],
            false,
            false,
        ),
        ("own-track-other", true, vec![("f2", false)], false, true),
        (
            "own-track-two",
            true,
            vec![("f1", false), ("f2", false)],
            false,
            true,
        ),
        ("own-track-off", false, vec![("f1", false)], false, true),
        ("own-track-room", true, vec![("f1", false)], true, false),
        (
            "own-track-room-off",
            false,
            vec![("f1", false)],
            false,
            false,
        ),
        (
            "own-track-room-two",
            true,
            vec![("f1", false), ("f2", false)],
            false,
            false,
        ),
    ] {
        OWN_TRACK.with(|own| own.set(on));
        STICKY.with(|sticky| sticky.set(what.starts_with("own-track-sticky")));
        let chats = vec![
            vec![text("Lunch, then."), Ok(ChatEvent::Done)],
            vec![text("Okay."), Ok(ChatEvent::Done)],
        ];
        let got = lines_known(
            what,
            vec![
                (
                    "Lamp, I'm planning lunch.".to_owned(),
                    vec![("f1", true)],
                    0,
                ),
                ("What do you remember about my food?".to_owned(), covered, 0),
            ],
            chats,
            (BOOT, true, true),
            None,
            Some(true),
            named,
            false,
            None,
            Voice::Off,
            (false, &[]),
            let_go,
            (false, None, None),
        )
        .await;
        if named {
            assert_eq!(
                got.asked[0],
                Message::system(format!("for {:?}", Some(a.as_str()))),
                "{what}: her opening line, seen said, hers"
            );
        }
        let told = if carried {
            Message::system(format!("for {:?}", Some(a.as_str())))
        } else {
            Message::system("for None")
        };
        assert_eq!(got.asked[1], told, "{what}: the covered line");
    }
}

/// `LAMP_SESSION_PERSON` (the memory owner's decisions of 23 Nov): `lines`
/// said with the switch `on` or off, the product's one-face rule with reads
/// on and no other voice heard, the face cache naming by `named` before each
/// line, and the known file rewritten by `known_at`.
async fn session_lines(
    what: &str,
    on: bool,
    lines: Vec<Line>,
    named: NamedBy,
    known_at: fn(u64, &std::path::Path),
    chats: Vec<Vec<net::Result<ChatEvent>>>,
) -> Outcome {
    SESSION.with(|session| session.set(on));
    ONE_FACE_READS.with(|reads| reads.set(true));
    NAMED_LINES.with(|lines| lines.set(Some(named)));
    lines_known(
        what,
        lines,
        chats,
        (BOOT, true, true),
        None,
        None,
        false,
        false,
        None,
        Voice::One,
        (false, &[]),
        known_at,
        (false, None, None),
    )
    .await
}

/// One "Okay." answer for each of `lines` lines.
fn session_answers(lines: usize) -> Vec<Vec<net::Result<ChatEvent>>> {
    (0..lines)
        .map(|_| vec![text("Okay."), Ok(ChatEvent::Done)])
        .collect()
}

/// A's name matched before every line.
fn a_matched(_: u64) -> Option<bool> {
    Some(false)
}

/// The known file as written at the start, every line.
fn known_unchanged(_: u64, _: &std::path::Path) {}

/// Whose scope each line's memory note was asked in, by its words.
fn whose_lines(got: &Outcome, words: &[&str]) -> Vec<Option<String>> {
    words
        .iter()
        .map(|words| {
            got.noted
                .iter()
                .find(|(_, noted)| noted == words)
                .unwrap_or_else(|| panic!("{words:?} not noted: {:?}", got.noted))
                .0
                .clone()
        })
        .collect()
}

fn said(words: &str, in_view: Vec<(&'static str, bool)>) -> Line {
    (words.to_owned(), in_view, 0)
}

#[tokio::test]
async fn a_session_person_alone_is_theirs_with_the_switch_on_and_off() {
    let a = crate::remember::person_id("A");
    for on in [true, false] {
        let got = session_lines(
            &format!("session-alone-{on}"),
            on,
            vec![said("I like green tea.", vec![("f1", false)])],
            a_matched,
            known_unchanged,
            session_answers(1),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea."]),
            [Some(a.clone())],
            "{on}"
        );
    }
}

/// The owner's failure: the user alone, a passer-by's silent face in view
/// through a line. On, the line stays the user's; off, the room's, as
/// before.
#[tokio::test]
async fn a_silent_passerby_keeps_the_session_persons_line_theirs_only_with_the_switch() {
    let a = crate::remember::person_id("A");
    for (on, want) in [(true, Some(a.clone())), (false, None)] {
        let got = session_lines(
            &format!("session-passer-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said(
                    "My sister is called Lan.",
                    vec![("f1", false), ("f2", false)],
                ),
            ],
            a_matched,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "My sister is called Lan."]),
            [Some(a.clone()), want],
            "{on}"
        );
    }
}

/// The user covered or misread, the eyes holding their name on their
/// followed face (sticky), a passer-by in view: theirs with the switch, the
/// room's without.
#[tokio::test]
async fn a_misread_on_the_session_persons_followed_face_stays_theirs_only_with_the_switch() {
    let a = crate::remember::person_id("A");
    fn held_after_the_first(line: u64) -> Option<bool> {
        Some(line > 1)
    }
    for (on, want) in [(true, Some(a.clone())), (false, None)] {
        STICKY.with(|sticky| sticky.set(true));
        let got = session_lines(
            &format!("session-misread-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said(
                    "My sister is called Lan.",
                    vec![("f1", false), ("f2", false)],
                ),
            ],
            held_after_the_first,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "My sister is called Lan."]),
            [Some(a.clone()), want],
            "{on}"
        );
    }
}

/// Another face seen speaking a line: that line the room's either way; the
/// next line, the passer-by silent again, the user's with the switch and the
/// room's without.
#[tokio::test]
async fn another_face_seen_speaking_takes_that_line_only_and_the_next_is_the_session_persons() {
    let a = crate::remember::person_id("A");
    for (on, next) in [(true, Some(a.clone())), (false, None)] {
        let got = session_lines(
            &format!("session-spoken-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said("I prefer coffee.", vec![("f1", false), ("f2", true)]),
                said(
                    "My sister is called Lan.",
                    vec![("f1", false), ("f2", false)],
                ),
            ],
            a_matched,
            known_unchanged,
            session_answers(3),
        )
        .await;
        assert_eq!(
            whose_lines(
                &got,
                &[
                    "I like green tea.",
                    "I prefer coffee.",
                    "My sister is called Lan."
                ]
            ),
            [Some(a.clone()), None, next],
            "{on}"
        );
    }
}

/// The user leaves and an unknown face comes: its line the room's, switch
/// on or off.
#[tokio::test]
async fn the_session_person_gone_and_an_unknown_face_is_the_rooms_either_way() {
    let a = crate::remember::person_id("A");
    fn gone_after_the_first(line: u64) -> Option<bool> {
        (line == 1).then_some(false)
    }
    for on in [true, false] {
        let got = session_lines(
            &format!("session-unknown-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said("What does she like?", vec![("f2", false)]),
            ],
            gone_after_the_first,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "What does she like?"]),
            [Some(a.clone()), None],
            "{on}"
        );
    }
}

/// The user leaves and B, enrolled, is taken alone by the eyes: B's line is
/// B's, switch on or off.
#[tokio::test]
async fn the_session_person_gone_and_b_alone_is_bs_either_way() {
    let (a, b) = (
        crate::remember::person_id("A"),
        crate::remember::person_id("B"),
    );
    fn b_from_the_second(line: u64, path: &std::path::Path) {
        if line == 2 {
            std::fs::write(path, "B").unwrap();
            std::fs::write(known_face_of(path), "f2\n").unwrap();
        }
    }
    for on in [true, false] {
        let got = session_lines(
            &format!("session-b-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said("I like black tea.", vec![("f2", false)]),
            ],
            a_matched,
            b_from_the_second,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "I like black tea."]),
            [Some(a.clone()), Some(b.clone())],
            "{on}"
        );
    }
}

/// A guest's room conversation, then the user comes with the guest still in
/// view: the user's silent-mouthed line the room's either way (the session
/// waits); their line seen said by them, theirs either way; then, the guest
/// silent beside them, theirs only with the switch.
#[tokio::test]
async fn a_guests_session_holds_the_user_until_they_are_seen_speaking() {
    let a = crate::remember::person_id("A");
    fn after_the_guest(line: u64) -> Option<bool> {
        (line > 1).then_some(false)
    }
    for (on, last) in [(true, Some(a.clone())), (false, None)] {
        let got = session_lines(
            &format!("session-guest-{on}"),
            on,
            vec![
                said("Is it raining?", vec![("f2", false)]),
                said("I like green tea.", vec![("f1", false), ("f2", false)]),
                said(
                    "My sister is called Lan.",
                    vec![("f1", true), ("f2", false)],
                ),
                said("I prefer oolong.", vec![("f1", false), ("f2", false)]),
            ],
            after_the_guest,
            known_unchanged,
            session_answers(4),
        )
        .await;
        assert_eq!(
            whose_lines(
                &got,
                &[
                    "Is it raining?",
                    "I like green tea.",
                    "My sister is called Lan.",
                    "I prefer oolong."
                ]
            ),
            [None, None, Some(a.clone()), last],
            "{on}"
        );
    }
}

/// 2 min quiet ends a guest's room session: the user's line after it, the
/// guest's face silent beside them, begins their own session with the
/// switch, and is the room's without.
#[tokio::test]
async fn two_minutes_quiet_ends_a_guests_session() {
    let a = crate::remember::person_id("A");
    fn after_the_guest(line: u64) -> Option<bool> {
        (line > 1).then_some(false)
    }
    for (on, want) in [(true, Some(a.clone())), (false, None)] {
        let got = session_lines(
            &format!("session-quiet-{on}"),
            on,
            vec![
                said("Is it raining?", vec![("f2", false)]),
                (
                    "I like green tea.".to_owned(),
                    vec![("f1", false), ("f2", false)],
                    crate::memory_scope::NO_FACE_QUIET.as_secs(),
                ),
            ],
            after_the_guest,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["Is it raining?", "I like green tea."]),
            [None, want],
            "{on}"
        );
    }
}

/// Privacy on: the line is the room's, switch on or off; the session is
/// over, and the user's next line, the camera back and the user alone,
/// begins another.
#[tokio::test]
async fn privacy_on_makes_the_line_the_rooms_either_way() {
    let a = crate::remember::person_id("A");
    for on in [true, false] {
        PRIVACY_AT.with(|at| at.set(2));
        let got = session_lines(
            &format!("session-privacy-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said(
                    "My sister is called Lan.",
                    vec![("f1", false), ("f2", false)],
                ),
            ],
            a_matched,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "My sister is called Lan."]),
            [Some(a.clone()), None],
            "{on}"
        );
    }
}

/// A forget on a session line their mouth wasn't seen saying, a passer-by in
/// view: with the switch it is refused, nothing forgotten in anyone's
/// memory; without, the line is the room's and the forget runs there, as
/// before.
#[tokio::test]
async fn a_forget_without_the_session_persons_mouth_is_refused() {
    for on in [true, false] {
        let mut chats = session_answers(1);
        chats.push(vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"about": "the coffee"}),
            )])),
            Ok(ChatEvent::Done),
        ]);
        chats.push(vec![text("Okay."), Ok(ChatEvent::Done)]);
        let got = session_lines(
            &format!("session-forget-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said(
                    "Forget what I said about the coffee.",
                    vec![("f1", false), ("f2", false)],
                ),
            ],
            a_matched,
            known_unchanged,
            chats,
        )
        .await;
        let forgets = got
            .ran
            .iter()
            .filter(|(name, _)| name == crate::remember::FORGET)
            .count();
        if on {
            assert_eq!(forgets, 0, "refused: {:?}", got.ran);
            assert!(
                got.rounds
                    .iter()
                    .flatten()
                    .any(|message| format!("{message:?}").contains("say it again")),
                "told why"
            );
        } else {
            assert_eq!(forgets, 1, "the room's forget: {:?}", got.ran);
            assert_eq!(got.calls, [None]);
        }
    }
}

/// The seat swap (the memory owner's choice "B with 5s", 23 Nov): with
/// `LAMP_SESSION_PERSON` on, the eyes follow a new face (`f2`) in the user's
/// seat only within `who::FOLLOW_GAP` (5 s) of their last sighting. Past it
/// the eyes name nobody on it, so its silent line, another face beside it,
/// is the room's, the switch on or off.
#[tokio::test]
async fn a_new_face_in_the_users_seat_past_the_follow_gap_is_the_rooms() {
    let a = crate::remember::person_id("A");
    fn gone_after_the_first(line: u64) -> Option<bool> {
        (line == 1).then_some(false)
    }
    for on in [true, false] {
        let got = session_lines(
            &format!("session-seat-swap-{on}"),
            on,
            vec![
                said("I like green tea.", vec![("f1", false)]),
                said("I like black tea.", vec![("f2", false), ("f3", false)]),
            ],
            gone_after_the_first,
            known_unchanged,
            session_answers(2),
        )
        .await;
        assert_eq!(
            whose_lines(&got, &["I like green tea.", "I like black tea."]),
            [Some(a.clone()), None],
            "{on}"
        );
    }
}

// #8570 S1a/S1b, `LAMP_ONE_FACE_FORGETS_ON_YES` (the memory owner's ruling
// of 25 Nov, 14:33): a forget, a correction, `forget_everything` and
// `forget_my_face` on a line theirs by their face alone in view, the mouth
// not seen, are each held for one yes, and their plain yes by their face
// alone in view runs it. Every test runs with the private room off, so it
// never passes by the private room's yes.

/// [`asked_once_chats`] (A's face alone in view, the eyes judging a held
/// call's yes, the private room off) with the switch `on`.
async fn face_yes_chats(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    on: bool,
) -> Outcome {
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(on));
    let got = asked_once_chats(what, lines, chats, false, None).await;
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
    got
}

/// The model's call `name` with `args` on the asking line, then its
/// question `asks`; then "Done." for each line after it.
fn asks_then(
    name: &str,
    args: Value,
    asks: &'static str,
    after: usize,
) -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call("h1", name, args)])),
            Ok(ChatEvent::Done),
        ],
        vec![text(asks), Ok(ChatEvent::Done)],
    ];
    chats.extend((0..after).map(|_| vec![text("Done."), Ok(ChatEvent::Done)]));
    chats
}

/// The calls run named `name`, by their arguments.
fn ran_named(got: &Outcome, name: &str) -> Vec<Value> {
    got.ran
        .iter()
        .filter(|(ran, _)| ran == name)
        .map(|(_, args)| args.clone())
        .collect()
}

/// Whether the asking line's call was held for their yes, told `note`.
fn held_told(got: &Outcome, note: &str) -> bool {
    got.rounds
        .iter()
        .flatten()
        .any(|message| format!("{message:?}").contains(note))
}

const CORRECTS: &str = "Actually, my gym is on Ninth Street.";
const FORGET_FACE: &str = "Lamp, forget my face.";
/// The held note of a forget, `forget_my_face` and a correction, as told.
const HELD: &str = "this waits for their yes";

/// The four calls held for a yes by their face: words, call, arguments,
/// the model's question.
fn held_cases() -> [(&'static str, &'static str, Value, &'static str); 4] {
    [
        (
            "named",
            FORGET_UMBRELLA,
            json!({"about": "umbrella"}),
            ASKS_UMBRELLA,
        ),
        (
            "everything",
            FORGET_ME,
            json!({}),
            "Shall I forget everything I know about you, including your name?",
        ),
        ("face", FORGET_FACE, json!({}), "Shall I forget your face?"),
        (
            "correction",
            CORRECTS,
            json!({"text": "Gym is on Ninth Street.", "replaces": 7}),
            "Shall I remember that instead?",
        ),
    ]
}

/// The call each of [`held_cases`] makes.
fn held_call(case: &str) -> &'static str {
    match case {
        "named" => crate::remember::FORGET,
        "everything" => crate::remember::FORGET_EVERYTHING,
        "face" => crate::known_faces::FORGET,
        _ => crate::remember::REMEMBER,
    }
}

/// S1a: "Remember my locker number is 4417." kept as A's (id 91), then a
/// bare "Forget that." is held as the forget of id 91 and asked about
/// without the number; "Yes." by their face alone in view runs exactly
/// that forget, in A's scope, and nothing else.
#[tokio::test]
async fn a_bare_forget_right_after_a_one_face_keep_asks_once_and_forgets_on_yes() {
    let a = crate::remember::person_id("A");
    let _one = KEEP_MARK_TESTS.lock().await;
    KEEP_MARKS.with(|marks| marks.set(Some(91)));
    let mut chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "k1",
                crate::remember::REMEMBER,
                json!({"text": "Locker number is 4417."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Okay."), Ok(ChatEvent::Done)],
    ];
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        1,
    ));
    let got = face_yes_chats(
        "face-yes-bare",
        vec![
            line("Remember my locker number is 4417.", vec![("f1", false)], 0),
            line("Forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        true,
    )
    .await;
    assert!(held_told(&got, HELD), "asked once: {:?}", got.rounds);
    let asked = got.rounds.iter().flatten().any(|message| {
        let message = format!("{message:?}");
        message.contains(HELD) && message.contains("4417")
    });
    assert!(!asked, "the ask never names the kept text");
    assert_eq!(
        ran_named(&got, crate::remember::FORGET),
        [json!({"source_id": 91})]
    );
    assert_eq!(
        got.calls,
        [Some(a.clone()), Some(a)],
        "the keep and the forget: A's"
    );
}

/// S1a, named: "forget my umbrella" asked about once; their yes, no face
/// seen saying it and their face the one face in view, runs that forget by
/// id outside the private room.
#[tokio::test]
async fn a_named_one_face_forget_runs_on_an_unseen_yes_by_their_face() {
    let a = crate::remember::person_id("A");
    let got = face_yes_chats(
        "face-yes-named",
        vec![
            line(FORGET_UMBRELLA, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", false)], 5),
        ],
        forget_umbrella_agreed(),
        true,
    )
    .await;
    assert_eq!(
        forgets_ran(&got),
        [json!({"source_id": UMBRELLA.0, "about": UMBRELLA.1})]
    );
    assert_eq!(got.calls, [Some(a)], "in A's scope only");
}

/// S1b: a correction (`remember` with `replaces`) is held, never run on the
/// asking line, and asked about without the old fact; their yes runs it
/// exactly, retiring id 7 for the new words.
#[tokio::test]
async fn a_one_face_correction_asks_and_retires_the_old_fact_on_yes() {
    let a = crate::remember::person_id("A");
    let args = json!({"text": "Gym is on Ninth Street.", "replaces": 7});
    let got = face_yes_chats(
        "face-yes-correction",
        vec![
            line(CORRECTS, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", false)], 5),
        ],
        asks_then(
            crate::remember::REMEMBER,
            args.clone(),
            "Shall I remember that instead?",
            1,
        ),
        true,
    )
    .await;
    assert!(
        held_told(&got, "Not changed yet: this waits for their yes"),
        "asked once: {:?}",
        got.rounds
    );
    assert_eq!(ran_named(&got, crate::remember::REMEMBER), [args]);
    assert_eq!(got.calls, [Some(a)], "in A's scope");
}

/// `forget_everything` on their one-face line: nothing runs on the asking
/// line, which is told to ask; their yes runs it once, in A's scope alone.
#[tokio::test]
async fn forget_everything_on_a_one_face_line_asks_and_runs_on_their_yes() {
    let a = crate::remember::person_id("A");
    let got = face_yes_chats(
        "face-yes-everything",
        vec![
            line(FORGET_ME, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", false)], 5),
        ],
        asks_then(
            crate::remember::FORGET_EVERYTHING,
            json!({}),
            "Shall I forget everything I know about you, including your name?",
            1,
        ),
        true,
    )
    .await;
    assert!(
        held_told(&got, "forgetting everything always waits for their yes"),
        "asked once: {:?}",
        got.rounds
    );
    assert_eq!(
        ran_named(&got, crate::remember::FORGET_EVERYTHING),
        [json!({})]
    );
    assert_eq!(got.calls, [Some(a)], "A's scope alone");
}

/// `forget_my_face` on their one-face line: nothing sent on the asking
/// line; their yes sends it once.
#[tokio::test]
async fn forget_my_face_on_a_one_face_line_asks_and_runs_on_their_yes() {
    let a = crate::remember::person_id("A");
    let got = face_yes_chats(
        "face-yes-face",
        vec![
            line(FORGET_FACE, vec![("f1", false)], 0),
            line("Yes.", vec![("f1", false)], 5),
        ],
        asks_then(
            crate::known_faces::FORGET,
            json!({}),
            "Shall I forget your face?",
            1,
        ),
        true,
    )
    .await;
    assert!(held_told(&got, HELD), "asked once: {:?}", got.rounds);
    assert_eq!(ran_named(&got, crate::known_faces::FORGET), [json!({})]);
    assert_eq!(got.calls, [Some(a)]);
}

/// The switch off: stock. A forget of everything, of their face and a
/// correction on their one-face line are refused as the mouth was not seen
/// ("say it again"), a bare forget after their keep runs nothing, and
/// their unseen yes to a named forget runs nothing outside the private
/// room.
#[tokio::test]
async fn with_the_switch_off_the_mouth_still_gates_forgets_and_corrections() {
    for (case, words, args, asks) in held_cases() {
        let got = face_yes_chats(
            &format!("face-yes-off-{case}"),
            vec![
                line(words, vec![("f1", false)], 0),
                line("Yes.", vec![("f1", false)], 5),
            ],
            if case == "named" {
                forget_umbrella_agreed()
            } else {
                asks_then(held_call(case), args, asks, 1)
            },
            false,
        )
        .await;
        assert!(
            ran_named(&got, held_call(case)).is_empty(),
            "{case}: never run: {:?}",
            got.ran
        );
        assert!(
            !held_told(&got, "waits for their yes") || case == "named",
            "{case}: not held"
        );
        if case != "named" {
            assert!(refused_by_one_face(&got), "{case}: refused as stock");
        }
    }
    let _one = KEEP_MARK_TESTS.lock().await;
    KEEP_MARKS.with(|marks| marks.set(Some(92)));
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Locker number is 4417."}),
    );
    chats.extend(asks_then(crate::remember::FORGET, json!({}), "Okay.", 1));
    let got = face_yes_chats(
        "face-yes-off-bare",
        vec![
            line("Remember my locker number is 4417.", vec![("f1", false)], 0),
            line("Forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        false,
    )
    .await;
    assert_eq!(
        ran_named(&got, crate::remember::FORGET),
        Vec::<Value>::new()
    );
    assert!(!held_told(&got, HELD), "off: nothing held");
}

/// A second face in view at the yes, silent or seen speaking it: each held
/// call is dropped and none runs. The ask itself was held.
#[tokio::test]
async fn a_second_face_at_the_yes_still_runs_nothing() {
    for (case, words, args, asks) in held_cases() {
        for (at, in_view) in [
            ("silent", vec![("f1", false), ("f2", false)]),
            ("speaking", vec![("f1", false), ("f2", true)]),
        ] {
            let got = face_yes_chats(
                &format!("face-yes-two-{case}-{at}"),
                vec![
                    line(words, vec![("f1", false)], 0),
                    line("Yes.", in_view, 5),
                ],
                if case == "named" {
                    forget_umbrella_agreed()
                } else {
                    asks_then(held_call(case), args.clone(), asks, 1)
                },
                true,
            )
            .await;
            let held = if case == "named" {
                asked_about_umbrella(&got)
            } else {
                held_told(&got, "waits for their yes")
            };
            assert!(held, "{case} {at}: held first: {:?}", got.rounds);
            assert!(
                ran_named(&got, held_call(case)).is_empty(),
                "{case} {at}: nothing runs: {:?}",
                got.ran
            );
        }
    }
}

/// A carried line (`LAMP_CONVERSATION_CARRIED_MEMORY`) with the switch on:
/// a forget of everything, of their face, a correction and a forget are
/// refused as before, never held.
#[tokio::test]
async fn a_carried_line_still_forgets_or_corrects_nothing_with_the_switch() {
    for (case, words, args, _) in held_cases() {
        crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
        let got = carried_line(
            &format!("face-yes-carried-{case}"),
            words,
            one_call(held_call(case), args),
            vec![("f1", false)],
            true,
            true,
            Voice::Judged,
        )
        .await;
        crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
        assert!(got.calls.is_empty(), "{case}: never run: {:?}", got.calls);
        assert!(!held_told(&got, "waits for their yes"), "{case}: not held");
    }
}

/// An engine whose eyes judge a held call's yes, and a held `name` call
/// with `args` asked of A in session 7 for a yes by their face
/// ([`HeldForget::face_yes`]), at `at`; `speakers` the faces seen saying
/// line 2.
fn held_for_face_yes(
    what: &str,
    name: &str,
    args: Value,
    speakers: &[&str],
    at: Instant,
) -> (Engine, std::path::PathBuf) {
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file(what, &eyes);
    let engine = engine.confirming_forgets_by_sight(eyes, path.clone());
    if let Ok(mut hold) = engine.shared.held_forget.lock() {
        hold.held = Some(HeldForget {
            call: call("h1", name, args),
            request: 1,
            asker: (7, crate::remember::person_id("A")),
            at,
            asked: true,
            spelling: false,
            carried: false,
            line: None,
            asked_again: false,
            one_face: false,
            near: None,
            offered_alone: false,
            face_yes: true,
        });
        hold.sight = Some((
            2,
            speakers.iter().map(|id| (*id).to_owned()).collect(),
            None,
        ));
    }
    (engine, path)
}

/// A yes line theirs by their face alone in view.
const IN_VIEW: YesLine = YesLine {
    by_one_face: true,
    in_view: true,
};

/// `take_held_forget` on a call [`held_for_face_yes`], with the switch on.
fn take_face_yes(
    what: &str,
    (name, args): (&str, Value),
    (said, heard): (&str, bool),
    (line, yes_line, speakers): (&HeldAsker, YesLine, &[&str]),
    after: Duration,
) -> Taken {
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
    let at = Instant::now();
    let (engine, path) = held_for_face_yes(what, name, args, speakers, at);
    let taken = take_held_forget(
        &engine.shared,
        2,
        heard,
        said,
        (None, Some(line)),
        yes_line,
        at + after,
    );
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(path);
    taken
}

/// The F1 branch: a yes to a held `forget_everything`, by their face alone
/// in view and none seen saying it, runs it and is marked agreed
/// (`Taken::Runs(.., true)`), where it was dropped as unseen.
#[test]
fn a_yes_to_a_held_forget_everything_by_their_face_alone_runs() {
    let a = (7, crate::remember::person_id("A"));
    for (name, args) in [
        (crate::remember::FORGET_EVERYTHING, json!({})),
        (crate::known_faces::FORGET, json!({})),
        (
            crate::remember::REMEMBER,
            json!({"text": "Gym is on Ninth Street.", "replaces": 7}),
        ),
        (crate::remember::FORGET, json!({"source_id": 91})),
    ] {
        let taken = take_face_yes(
            &format!("face-yes-runs-{name}"),
            (name, args.clone()),
            ("Yes.", true),
            (&a, IN_VIEW, &[]),
            Duration::from_secs(5),
        );
        assert!(
            matches!(&taken, Taken::Runs(call, _, true) if call.name == name && call.args == args),
            "{name}: runs, agreed"
        );
    }
}

/// A late yes by their face, past `HELD_FORGET_FOR`, runs nothing and is
/// told it is kept.
#[test]
fn a_late_yes_by_their_face_is_still_too_late() {
    let a = (7, crate::remember::person_id("A"));
    let taken = take_face_yes(
        "face-yes-late",
        (crate::remember::FORGET_EVERYTHING, json!({})),
        ("Yes.", true),
        (&a, IN_VIEW, &[]),
        crate::remember::HELD_FORGET_FOR + Duration::from_secs(1),
    );
    assert!(
        matches!(taken, Taken::Unknown(_)),
        "too late: kept and told"
    );
}

/// With the switch on: another person's yes, a no, a written turn and a yes
/// another face is seen saying all run nothing, as before; the same yes by
/// their face alone runs.
#[test]
fn another_persons_no_late_or_written_yes_still_runs_nothing_with_the_switch() {
    let a = (7, crate::remember::person_id("A"));
    let b = (7, crate::remember::person_id("B"));
    let everything = || (crate::remember::FORGET_EVERYTHING, json!({}));
    let five = Duration::from_secs(5);
    let runs = |taken: &Taken| matches!(taken, Taken::Runs(..));
    let control = take_face_yes(
        "face-yes-control",
        everything(),
        ("Yes.", true),
        (&a, IN_VIEW, &[]),
        five,
    );
    assert!(runs(&control), "their yes by their face runs");
    let other = take_face_yes(
        "face-yes-other",
        everything(),
        ("Yes.", true),
        (&b, IN_VIEW, &[]),
        five,
    );
    assert!(matches!(other, Taken::NotTheirs), "another person's");
    let no = take_face_yes(
        "face-yes-no",
        everything(),
        ("No.", true),
        (&a, IN_VIEW, &[]),
        five,
    );
    assert!(!runs(&no), "a no");
    let written = take_face_yes(
        "face-yes-written",
        everything(),
        ("Yes.", false),
        (&a, IN_VIEW, &[]),
        five,
    );
    assert!(!runs(&written), "a written turn");
    let spoken_by = take_face_yes(
        "face-yes-spoken-by",
        everything(),
        ("Yes.", true),
        (&a, IN_VIEW, &["f9"]),
        five,
    );
    assert!(!runs(&spoken_by), "another face seen saying it");
}

/// A sticky, gap-held, carried or narrowed yes line is no line by their
/// face alone in view (`ScopeLease::in_view_by_one_face`): a call held for
/// a yes by their face is dropped as unseen there, with the switch on.
#[test]
fn a_sticky_or_gap_line_is_unchanged_with_the_switch() {
    let a = (7, crate::remember::person_id("A"));
    for (what, yes_line) in [
        ("face-yes-sticky", YesLine::from(true)),
        (
            "face-yes-not-one-face",
            YesLine {
                by_one_face: false,
                in_view: false,
            },
        ),
    ] {
        let taken = take_face_yes(
            what,
            (crate::remember::FORGET_EVERYTHING, json!({})),
            ("Yes.", true),
            (&a, yes_line, &[]),
            Duration::from_secs(5),
        );
        assert!(!matches!(taken, Taken::Runs(..)), "{what}: runs nothing");
    }
    // A held correction is no carried line's keep: its yes there runs
    // nothing, `LAMP_KEEP_QUIETLY_ONE_FACE` on.
    crate::keep_quietly::set_one_face_for_test(true);
    let taken = take_face_yes(
        "face-yes-carried-keep",
        (
            crate::remember::REMEMBER,
            json!({"text": "Gym is on Ninth Street.", "replaces": 7}),
        ),
        ("Yes.", true),
        (&a, YesLine::from(true), &[]),
        Duration::from_secs(5),
    );
    crate::keep_quietly::set_one_face_for_test(false);
    assert!(!matches!(taken, Taken::Runs(..)), "carried: runs nothing");
}

/// Only the held run's mark lets `forget_everything`, `forget_my_face` or a
/// correction through the one-face refusal on such a line, with the switch
/// on: called directly, each is refused; marked, only that call passes.
#[test]
fn the_held_run_alone_passes_one_face_refused() {
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
    let key = crate::memory_scope::RequestKey {
        worker: lamp_turn_owner::BootId::new([0x85; 16]).unwrap(),
        session: 1,
        request: 8_570_001,
    };
    let correction = json!({"text": "Gym is on Ninth Street.", "replaces": 7});
    for (name, args) in [
        (crate::remember::FORGET_EVERYTHING, json!({})),
        (crate::known_faces::FORGET, json!({})),
        (crate::remember::REMEMBER, correction.clone()),
    ] {
        let scope = crate::memory_scope::RequestScope::new(
            key,
            crate::memory_scope::ScopeSnapshot::of("A"),
        );
        let lease = scope.lease();
        assert!(lease.read_by_one_face());
        let held = call("h1", name, args.clone());
        assert!(one_face_refused(Some(&lease), &held), "{name}: unmarked");
        lease.mark_held_yes(
            &crate::second_provider::one_face_forget_yes_for_test(),
            name,
            &args,
        );
        assert!(!one_face_refused(Some(&lease), &held), "{name}: marked");
    }
    let scope =
        crate::memory_scope::RequestScope::new(key, crate::memory_scope::ScopeSnapshot::of("A"));
    let lease = scope.lease();
    assert!(lease.read_by_one_face());
    lease.mark_held_yes(
        &crate::second_provider::one_face_forget_yes_for_test(),
        crate::remember::REMEMBER,
        &correction,
    );
    let other = call(
        "h2",
        crate::remember::REMEMBER,
        json!({"text": "x", "replaces": 8}),
    );
    assert!(one_face_refused(Some(&lease), &other), "another correction");
    let everything = call("h3", crate::remember::FORGET_EVERYTHING, json!({}));
    assert!(one_face_refused(Some(&lease), &everything), "not marked");
    let rule = call(
        "h4",
        crate::rules::KEEP_RULE,
        json!({"rule": "Always call me Captain."}),
    );
    assert!(one_face_refused(Some(&lease), &rule), "a rule");
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(false));
    let held = call("h1", crate::remember::REMEMBER, correction);
    assert!(one_face_refused(Some(&lease), &held), "switch off: refused");
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
}

/// Row 8654 L4 (lamp-06c1, build 16102, 2 of 2): with `LAMP_KEEP_QUIETLY`
/// and `LAMP_KEEP_QUIETLY_ONE_FACE` on, as in the product, a correction on
/// their one-face line was held, and dropped at once as its answer asked
/// nothing. The hold's round is told the quiet note's "never ask whether
/// you should" no more, and the model's ask is said though the lamp
/// offered once already in this talk (`Quiet::held_keep`), so the hold
/// stands and their yes runs exactly the correction.
#[tokio::test]
async fn row_8654_l4_a_held_correction_is_asked_with_the_quiet_switches_on() {
    crate::keep_quietly::set_on_for_test(true);
    crate::keep_quietly::set_one_face_for_test(true);
    let args = json!({"text": "Gym is on Ninth Street.", "replaces": 7});
    let mut chats = vec![vec![
        text("Want me to keep that for you?"),
        Ok(ChatEvent::Done),
    ]];
    chats.extend(asks_then(
        crate::remember::REMEMBER,
        args.clone(),
        "Shall I remember that instead?",
        1,
    ));
    let got = face_yes_chats(
        "row-8654-l4",
        vec![
            line("My gym is on Fourth Street.", vec![("f1", false)], 0),
            line(CORRECTS, vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 5),
        ],
        chats,
        true,
    )
    .await;
    crate::keep_quietly::set_on_for_test(false);
    crate::keep_quietly::set_one_face_for_test(false);
    let quiet = |message: &Message| {
        message.role == "system"
            && message.words().is_some_and(|words| {
                words == crate::keep_quietly::QUIET_NOTE
                    || words == crate::keep_quietly::QUIET_NOTE_UNSURE
            })
    };
    assert!(
        got.rounds.iter().flatten().any(quiet),
        "the quiet switches are on for these lines: {:?}",
        got.rounds
    );
    let after_hold: Vec<&Vec<Message>> = got
        .rounds
        .iter()
        .filter(|round| {
            round
                .iter()
                .any(|message| format!("{message:?}").contains("Not changed yet"))
        })
        .collect();
    assert!(!after_hold.is_empty(), "held and told: {:?}", got.rounds);
    assert!(
        after_hold.iter().all(|round| !round.iter().any(quiet)),
        "the hold's round is not told never to ask: {after_hold:?}"
    );
    assert_eq!(
        ran_named(&got, crate::remember::REMEMBER),
        [args],
        "their yes runs the correction: {:?}",
        got.labels
    );
}

/// The model's turns for row 8654 L2 as the lamp saw it: the keep, "Forget
/// that." asked back in words with no call, then a bare forget called on
/// their "Yes, please.", and its answer.
fn row_8654_l2_chats() -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Parking spot is B12."}),
    );
    chats.push(vec![
        text("Do you want me to forget your parking spot?"),
        Ok(ChatEvent::Done),
    ]);
    chats.extend(one_call(crate::remember::FORGET, json!({})));
    // Spare, so a second ask (the bug) still reaches the asserts.
    chats.push(vec![text("Shall I forget that?"), Ok(ChatEvent::Done)]);
    chats
}

/// Row 8654 L2 (lamp-06c1, build 16102, tries 2 and 3): "Forget that." right
/// after their one-face keep was asked back in words, the model calling
/// nothing; on "Yes, please." it called a bare forget, the keep's request
/// + 2, and it was refused as a bare forget. That call is their agreement:
/// it runs at once as the forget of the kept id, once, and nothing is asked
/// again (CLAUDE.md rule 8: one ask, one yes). A second face in view at the
/// yes runs nothing.
#[tokio::test]
async fn row_8654_l2_a_bare_forget_on_the_yes_after_a_word_ask_runs_at_once() {
    let _one = KEEP_MARK_TESTS.lock().await;
    let a = crate::remember::person_id("A");
    for (at, in_view) in [
        ("alone", vec![("f1", false)]),
        ("two", vec![("f1", false), ("f2", false)]),
    ] {
        KEEP_MARKS.with(|marks| marks.set(Some(93)));
        let got = face_yes_chats(
            &format!("row-8654-l2-{at}"),
            vec![
                line(
                    "Remember my parking spot is B twelve.",
                    vec![("f1", false)],
                    0,
                ),
                line("Forget that.", vec![("f1", false)], 3),
                line("Yes, please.", in_view, 3),
            ],
            row_8654_l2_chats(),
            true,
        )
        .await;
        assert!(
            !held_told(&got, HELD),
            "{at}: never held and asked again: {:?}",
            got.rounds
        );
        if at == "alone" {
            assert_eq!(
                ran_named(&got, crate::remember::FORGET),
                [json!({"source_id": 93})],
                "{at}: {:?}",
                got.labels
            );
            assert_eq!(
                got.calls,
                [Some(a.clone()), Some(a.clone())],
                "A's keep and forget"
            );
        } else {
            assert!(
                ran_named(&got, crate::remember::FORGET).is_empty(),
                "{at}: a second face at the yes runs nothing: {:?}",
                got.ran
            );
        }
    }
}

/// A plain yes after their one-face keep, `between` the line between,
/// answered `answer` (after a `recall` with `called`), and the model's bare
/// forget on the yes: whether it ran, and whether it was held and asked.
async fn bare_forget_on_a_later_yes(
    what: &str,
    between: &str,
    answer: &'static str,
    called: Option<(&'static str, Value)>,
) -> (bool, bool) {
    let _one = KEEP_MARK_TESTS.lock().await;
    KEEP_MARKS.with(|marks| marks.set(Some(94)));
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Parking spot is B12."}),
    );
    if let Some((name, args)) = called {
        chats.push(vec![
            Ok(ChatEvent::ToolCalls(vec![call("w1", name, args)])),
            Ok(ChatEvent::Done),
        ]);
    }
    chats.push(vec![text(answer), Ok(ChatEvent::Done)]);
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        0,
    ));
    let got = face_yes_chats(
        what,
        vec![
            line(
                "Remember my parking spot is B twelve.",
                vec![("f1", false)],
                0,
            ),
            line(between, vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        true,
    )
    .await;
    (
        !ran_named(&got, crate::remember::FORGET).is_empty(),
        held_told(&got, HELD),
    )
}

/// The Reviewer-lead's must-fix on #8654 (comment 7167529440): their
/// one-face keep, then "Want the forecast too?" after a call, then "Yes.",
/// and the model wrongly calls a bare forget: held and asked, nothing run.
/// So too when the lamp's answer asked nothing.
#[tokio::test]
async fn a_bare_forget_on_a_yes_to_the_forecast_is_held_not_run() {
    for (at, answer, called) in [
        (
            "forecast",
            "Sunny tomorrow. Want the forecast too?",
            Some((
                crate::remember::RECALL,
                json!({"query": "weather tomorrow"}),
            )),
        ),
        ("no-ask", "Sunny tomorrow.", None),
    ] {
        let (ran, held) = bare_forget_on_a_later_yes(
            &format!("row-8654-must-fix-{at}"),
            "What's the weather tomorrow?",
            answer,
            called,
        )
        .await;
        assert!(!ran, "{at}: the keep is never forgotten on that yes");
        assert!(held, "{at}: held and asked instead");
    }
}

/// El's P2 on 45f6ce4a18: "Forget my dentist note." answered in words
/// ("Shall I forget your dentist note?"), then "Yes." and a bare forget:
/// never the forget of their last keep, the parking spot, but held and
/// asked; nothing runs.
#[tokio::test]
async fn a_bare_forget_on_a_yes_after_a_named_forget_is_held_not_run() {
    let (ran, held) = bare_forget_on_a_later_yes(
        "row-8654-el-named",
        "Forget my dentist note.",
        "Shall I forget your dentist note?",
        None,
    )
    .await;
    assert!(!ran, "the parking spot is never forgotten on that yes");
    assert!(held, "held and asked instead");
}

/// Set in the child process [`row_8654_l2_on_the_real_route`] runs in.
const REAL_ROUTE_CHILD: &str = "LAMP_TEST_ROW_8654_REAL_ROUTE";

/// El's lead on #8654 (20:23): the host tests faked the keep's mark
/// (`KEEP_MARKS`), so only the row ran the real keep, mark and peek. Here
/// the keep runs on the voice's own memory route against a Days memory
/// with `LAMP_MEMORY_REPLACES` on, as on lamp-06c1, which marks the kept id
/// (`tools::ONE_FACE_KEPT`); then L2 as the lamp saw it: "Forget that."
/// answered in words, no call, and a bare forget, no source named, called
/// on "Yes, please.": it runs at once as the forget of that id and reaches
/// the memory, never asked again. In a child process: the Days client is once per process.
#[tokio::test]
async fn row_8654_l2_on_the_real_route() {
    const NAME: &str = "second_provider::tests::person_scope_seen::row_8654_l2_on_the_real_route";
    if std::env::var(REAL_ROUTE_CHILD).is_err() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(REAL_ROUTE_CHILD, "1")
            .output()
            .unwrap();
        let said = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{said}");
        assert!(said.contains("1 passed"), "the child ran it: {said}");
        return;
    }
    let base = 9_865_400;
    let socket = memory_socket(base);
    let stop = Arc::new(AtomicBool::new(false));
    let (memory, serving) = memory_at(&socket, stop.clone());
    assert!(crate::remember::days_for_test(&socket));
    let real = Real {
        socket: socket.clone(),
        said: vec![
            "Okay.",
            "Do you want me to forget your parking spot?",
            "Okay.",
        ],
        base,
    };
    let chats = row_8654_l2_chats();
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
    ONE_FACE_READS.with(|reads| reads.set(true));
    crate::person::set_private_room_for_test(Some(false));
    let got = lines_known(
        "row-8654-real",
        vec![
            line(
                "Remember my parking spot is B twelve.",
                vec![("f1", false)],
                0,
            ),
            line("Forget that.", vec![("f1", false)], 3),
            line("Yes, please.", vec![("f1", false)], 3),
        ],
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (false, &[]),
        |_, _| {},
        (false, None, Some(&real)),
    )
    .await;
    crate::person::set_private_room_for_test(None);
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
    stop.store(true, Ordering::Release);
    serving.join().unwrap();
    let _ = std::fs::remove_file(&socket);
    let memory = memory.lock().unwrap().clone();
    assert!(
        memory.iter().any(|ask| ask["op"] == "remember"),
        "the keep reached the memory: {memory:?}"
    );
    assert!(!held_told(&got, HELD), "not asked again: {:?}", got.rounds);
    assert_eq!(
        ran_named(&got, crate::remember::FORGET),
        [json!({"source_id": 1})],
        "the kept id's forget, on their yes: {:?}",
        got.labels
    );
    assert!(
        memory
            .iter()
            .any(|ask| ask["op"].as_str().is_some_and(|op| op.contains("forget"))),
        "the forget reached the memory: {memory:?}"
    );
}

const KEEP_THEN_FORGET_CHILD: &str = "LAMP_TEST_ROW_8654_KEEP_THEN_FORGET";

/// Row 8654 L2 (lamp-06c1, build 16102, 2c7cdd0d21, 2 of 2), in its order:
/// "Lamp, remember my parking spot is B twelve." kept as theirs by the
/// one-face rule on the voice's own memory route, then "Lamp, forget
/// that." on the next line, the model calling a bare forget at once. On
/// the lamp that forget was not held: the memory route's mark of the keep
/// (`tools::ONE_FACE_KEPT`) was not found, and the hold peeked only it.
/// Here that mark is let go before the forget's line, as on the lamp, and
/// the second route's own mark of the keep's told id holds it: asked once,
/// and their "Yes." runs exactly the kept id's forget. With the product's
/// switches the harness reads (`LAMP_PRIVATE_ROOM` on, the memory's
/// `LAMP_MEMORY_REMEMBER_ONCE` and `LAMP_MEMORY_REPLACES`).
#[tokio::test]
async fn row_8654_l2_a_bare_forget_right_after_the_keep_is_held_on_the_real_route() {
    const NAME: &str = "second_provider::tests::person_scope_seen::row_8654_l2_a_bare_forget_right_after_the_keep_is_held_on_the_real_route";
    if std::env::var(KEEP_THEN_FORGET_CHILD).is_err() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(KEEP_THEN_FORGET_CHILD, "1")
            .env(crate::remember::REMEMBER_ONCE_ENVIRONMENT, "on")
            .output()
            .unwrap();
        let said = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{said}");
        assert!(said.contains("1 passed"), "the child ran it: {said}");
        assert!(
            said.contains("face-hold: own_mark"),
            "the journal says the second route's own mark held it: {said}"
        );
        return;
    }
    let base = 9_865_500;
    let socket = memory_socket(base);
    let stop = Arc::new(AtomicBool::new(false));
    let (memory, serving) = memory_at(&socket, stop.clone());
    assert!(crate::remember::days_for_test(&socket));
    let real = Real {
        socket: socket.clone(),
        said: vec!["Okay.", "Shall I forget that?", "Done."],
        base,
    };
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Parking spot is B12."}),
    );
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        2,
    ));
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
    ONE_FACE_READS.with(|reads| reads.set(true));
    crate::person::set_private_room_for_test(Some(true));
    let got = lines_known(
        "row-8654-keep-then-forget",
        vec![
            line(
                "Lamp, remember my parking spot is B twelve.",
                vec![("f1", false)],
                0,
            ),
            line("Lamp, forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (false, &[]),
        // The memory route's mark of the keep is gone by the forget's
        // line, as on the lamp.
        |line, _| {
            if line == 2 {
                crate::tools::one_face_kept_clear_for_test();
            }
        },
        (false, None, Some(&real)),
    )
    .await;
    crate::person::set_private_room_for_test(None);
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
    stop.store(true, Ordering::Release);
    serving.join().unwrap();
    let _ = std::fs::remove_file(&socket);
    let memory = memory.lock().unwrap().clone();
    assert!(
        memory.iter().any(|ask| ask["op"] == "remember"),
        "the keep reached the memory: {memory:?}"
    );
    assert!(
        held_told(&got, HELD),
        "held and asked once: {:?}",
        got.rounds
    );
    assert_eq!(
        ran_named(&got, crate::remember::FORGET),
        [json!({"source_id": 1})],
        "only the kept id's forget, on their yes: {:?}",
        got.labels
    );
    assert!(
        memory
            .iter()
            .any(|ask| ask["op"].as_str().is_some_and(|op| op.contains("forget"))),
        "the forget reached the memory: {memory:?}"
    );
}

/// El's P3 4 at 2c7cdd0d21: condition (c) alone. Their one-face keep, then a
/// line whose request called something, answered with an ask about
/// forgetting, then "Yes.", and the model calls a bare forget: the ask and
/// the unnamed line pass, but a call came between the keep and the yes, so
/// it is held and asked, nothing run.
#[tokio::test]
async fn a_bare_forget_on_a_yes_after_a_call_between_is_held_not_run() {
    let (ran, held) = bare_forget_on_a_later_yes(
        "row-8654-p3-called",
        "What's the weather tomorrow?",
        "Sunny tomorrow. Shall I forget your parking spot?",
        Some((
            crate::remember::RECALL,
            json!({"query": "weather tomorrow"}),
        )),
    )
    .await;
    assert!(!ran, "the keep is never forgotten on that yes");
    assert!(held, "held and asked instead");
}

/// El's P3 2 at 2c7cdd0d21: "Want me to set a reminder so you don't
/// forget?" reads as asking about forgetting
/// (`remember::asks_about_forgetting`, its wording rules the owner's, left
/// as they are). A reminder ask comes with a call: their one-face keep, a
/// reminder set in between, that ask, then "Yes.", and the model calls a
/// bare forget: held and asked, nothing run (condition (c)).
#[tokio::test]
async fn a_bare_forget_on_a_yes_to_a_reminder_offer_is_held_not_run() {
    let (ran, held) = bare_forget_on_a_later_yes(
        "row-8654-p3-reminder",
        "I need to move the car at six.",
        "Want me to set a reminder so you don't forget?",
        Some((
            "set_reminder",
            json!({"text": "Move the car", "at": "18:00"}),
        )),
    )
    .await;
    assert!(!ran, "the keep is never forgotten on that yes");
    assert!(held, "held and asked instead");
}

/// Runs test `name` again, alone, in a child process with `key` set, for a
/// test whose memory client or marks are the process's own: what the child
/// said, once it passed. `None` in that child, which runs the test.
fn in_child(name: &str, key: &str) -> Option<String> {
    if std::env::var(key).is_ok() {
        return None;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(key, "1")
        .output()
        .unwrap();
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{said}");
    assert!(said.contains("1 passed"), "the child ran it: {said}");
    Some(said)
}

/// `lines` said on the voice's own memory route (a Days memory answering
/// every ask kept, id 1), the switch on, reads on, the private room on (the
/// product), with `known_at` before each line: what the lamp did, and the
/// asks the memory was sent.
async fn on_the_real_route(
    what: &str,
    base: u64,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    said: Vec<&'static str>,
    known_at: fn(u64, &std::path::Path),
) -> (Outcome, Vec<Value>) {
    let socket = memory_socket(base);
    let stop = Arc::new(AtomicBool::new(false));
    let (memory, serving) = memory_at(&socket, stop.clone());
    assert!(crate::remember::days_for_test(&socket));
    let real = Real {
        socket: socket.clone(),
        said,
        base,
    };
    crate::second_provider::set_one_face_forgets_on_yes_for_test(Some(true));
    ONE_FACE_READS.with(|reads| reads.set(true));
    crate::person::set_private_room_for_test(Some(true));
    let got = lines_known(
        what,
        lines,
        chats,
        (BOOT, true, true),
        None,
        None,
        true,
        true,
        None,
        Voice::One,
        (false, &[]),
        known_at,
        (false, None, Some(&real)),
    )
    .await;
    crate::person::set_private_room_for_test(None);
    crate::second_provider::set_one_face_forgets_on_yes_for_test(None);
    stop.store(true, Ordering::Release);
    serving.join().unwrap();
    let _ = std::fs::remove_file(&socket);
    let memory = memory.lock().unwrap().clone();
    (got, memory)
}

/// Whether the memory was sent a forget of any kind.
fn memory_forgot(memory: &[Value]) -> bool {
    memory
        .iter()
        .any(|ask| ask["op"].as_str().is_some_and(|op| op.contains("forget")))
}

/// El's P2 1 at 6c76c3257f: their one-face keep, then a recall in the same
/// request ("that" may now be what it read: Thirteen's P2 on #5022, the
/// memory route's mark ends there too), then "Lamp, forget that." and
/// "Yes.": the keep is never forgotten. The second route's own mark ends on
/// any memory call but a bare forget, its own request included.
#[tokio::test]
async fn a_recall_in_the_keeps_own_request_ends_the_own_mark() {
    const NAME: &str = "second_provider::tests::person_scope_seen::a_recall_in_the_keeps_own_request_ends_the_own_mark";
    if let Some(said) = in_child(NAME, "LAMP_TEST_ROW_8654_SAME_REQUEST_RECALL") {
        assert!(
            !said.contains("face-hold: own_mark"),
            "no own mark after the recall: {said}"
        );
        return;
    }
    let mut chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "k1",
                crate::remember::REMEMBER,
                json!({"text": "Parking spot is B12."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "r1",
                crate::remember::RECALL,
                json!({"query": "car"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Okay. Your car is the blue one."), Ok(ChatEvent::Done)],
    ];
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        2,
    ));
    let (got, memory) = on_the_real_route(
        "row-8654-same-request-recall",
        9_865_600,
        vec![
            line(
                "Lamp, remember my parking spot is B twelve, and what car do I have?",
                vec![("f1", false)],
                0,
            ),
            line("Lamp, forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        vec![
            "Okay. Your car is the blue one.",
            "Shall I forget that?",
            "Done.",
        ],
        |_, _| {},
    )
    .await;
    assert!(
        memory.iter().any(|ask| ask["op"] == "remember"),
        "the keep reached the memory: {memory:?}"
    );
    assert!(
        !ran_named(&got, crate::remember::FORGET).contains(&json!({"source_id": 1})),
        "the keep is never forgotten: {:?}",
        got.labels
    );
    assert!(!memory_forgot(&memory), "nothing forgotten: {memory:?}");
}

/// El's P2 2 (b) at 6c76c3257f: their one-face keep, then a recall on a
/// later line, then "Lamp, forget that." and "Yes.": the recall ended both
/// marks, so the keep is never forgotten.
#[tokio::test]
async fn a_recall_on_a_later_line_ends_the_own_mark() {
    const NAME: &str =
        "second_provider::tests::person_scope_seen::a_recall_on_a_later_line_ends_the_own_mark";
    if let Some(said) = in_child(NAME, "LAMP_TEST_ROW_8654_LATER_RECALL") {
        assert!(
            !said.contains("face-hold: own_mark"),
            "no own mark after the recall: {said}"
        );
        return;
    }
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Parking spot is B12."}),
    );
    chats.extend(one_call(crate::remember::RECALL, json!({"query": "car"})));
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        2,
    ));
    let (got, memory) = on_the_real_route(
        "row-8654-later-recall",
        9_865_800,
        vec![
            line(
                "Lamp, remember my parking spot is B twelve.",
                vec![("f1", false)],
                0,
            ),
            line("Lamp, what car do I have?", vec![("f1", false)], 3),
            line("Lamp, forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        vec!["Okay.", "Okay.", "Shall I forget that?", "Done."],
        |_, _| {},
    )
    .await;
    assert!(
        !ran_named(&got, crate::remember::FORGET).contains(&json!({"source_id": 1})),
        "the keep is never forgotten: {:?}",
        got.labels
    );
    assert!(!memory_forgot(&memory), "nothing forgotten: {memory:?}");
}

/// El's P2 2 (a) at 6c76c3257f: a keep on a line from a name the eyes hold
/// (`LAMP_FACE_STICKY`, not their face alone in view), then "Lamp, forget
/// that." by their face alone in view and "Yes.": the second route's own
/// mark is set only on a line by their face alone in view, so nothing is
/// forgotten. The memory route's mark is let go before the forget's line,
/// so only the own mark is tried.
#[tokio::test]
async fn a_sticky_keep_sets_no_own_mark() {
    const NAME: &str = "second_provider::tests::person_scope_seen::a_sticky_keep_sets_no_own_mark";
    if let Some(said) = in_child(NAME, "LAMP_TEST_ROW_8654_STICKY_KEEP") {
        assert!(
            !said.contains("face-hold: own_mark"),
            "no own mark from a sticky keep: {said}"
        );
        return;
    }
    fn sticky_first(line: u64) -> Option<bool> {
        Some(line == 1)
    }
    STICKY.with(|sticky| sticky.set(true));
    NAMED_LINES.with(|lines| lines.set(Some(sticky_first)));
    let mut chats = one_call(
        crate::remember::REMEMBER,
        json!({"text": "Parking spot is B12."}),
    );
    chats.extend(asks_then(
        crate::remember::FORGET,
        json!({}),
        "Shall I forget that?",
        2,
    ));
    let (got, memory) = on_the_real_route(
        "row-8654-sticky-keep",
        9_865_900,
        vec![
            line(
                "Lamp, remember my parking spot is B twelve.",
                vec![("f1", false)],
                0,
            ),
            line("Lamp, forget that.", vec![("f1", false)], 3),
            line("Yes.", vec![("f1", false)], 3),
        ],
        chats,
        vec!["Okay.", "Shall I forget that?", "Done."],
        |line, _| {
            if line == 2 {
                crate::tools::one_face_kept_clear_for_test();
            }
        },
    )
    .await;
    assert!(
        memory.iter().any(|ask| ask["op"] == "remember"),
        "the sticky line's keep reached the memory: {memory:?}"
    );
    assert!(
        !ran_named(&got, crate::remember::FORGET).contains(&json!({"source_id": 1})),
        "the keep is never forgotten: {:?}",
        got.labels
    );
    assert!(!memory_forgot(&memory), "nothing forgotten: {memory:?}");
}

/// Row 8654 L2 at 6c76c3257f (lamp-06c1, 4 of 4): "Lamp, forget that."
/// right after their one-face keep, the model's bare forget carrying an
/// `about` of `""` (0 words) or `[]`. `ask_once_forget` read it as bare,
/// but the hold took it as named and let it go with no line. The hold now
/// reads a bare forget as `ask_once_forget` does (`remember::pick_forget`):
/// held as the forget of the kept id, asked once, and their "Yes." runs
/// exactly that.
#[tokio::test]
async fn a_bare_forget_with_an_empty_about_is_held_as_bare() {
    let _one = KEEP_MARK_TESTS.lock().await;
    for (at, about) in [("empty", json!("")), ("list", json!([]))] {
        KEEP_MARKS.with(|marks| marks.set(Some(95)));
        let mut chats = one_call(
            crate::remember::REMEMBER,
            json!({"text": "Parking spot is B12."}),
        );
        chats.extend(asks_then(
            crate::remember::FORGET,
            json!({ "about": about }),
            "Shall I forget that?",
            1,
        ));
        let got = face_yes_chats(
            &format!("row-8654-about-{at}"),
            vec![
                line(
                    "Lamp, remember my parking spot is B twelve.",
                    vec![("f1", false)],
                    0,
                ),
                line("Lamp, forget that.", vec![("f1", false)], 3),
                line("Yes.", vec![("f1", false)], 3),
            ],
            chats,
            true,
        )
        .await;
        assert!(
            held_told(&got, HELD),
            "{at}: held and asked once: {:?}",
            got.rounds
        );
        assert_eq!(
            ran_named(&got, crate::remember::FORGET),
            [json!({"source_id": 95})],
            "{at}: their yes runs the kept id's forget"
        );
    }
}
