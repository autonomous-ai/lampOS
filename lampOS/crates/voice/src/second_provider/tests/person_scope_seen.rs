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

/// The eyes' boot the known file was written under.
const BOOT: &str = "eyes";

thread_local! {
    /// A don't-know's search (`LAMP_MEMORY_DONT_KNOW_RECALLS`) the next
    /// [`lines_known`] on this thread is given, once.
    static DONT_KNOW_FOUND: std::cell::RefCell<Option<crate::second_provider::Remembered>> =
        const { std::cell::RefCell::new(None) };
    /// `LAMP_PERSON_SCOPE_ONE_FACE_READS` ([`Engine::reading_by_one_face`])
    /// for the next [`lines_known`] on this thread, once.
    static ONE_FACE_READS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `LAMP_CONVERSATION_CARRIED_MEMORY` ([`Engine::reading_carried`]) for
    /// the next [`lines_known`] on this thread, once.
    static CARRIED_MEMORY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The same-voice check not running for the route
    /// ([`Engine::reading_carried_where_heard`]) for the next
    /// [`lines_known`] on this thread, once.
    static CARRIED_UNHEARD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The murmured line's mouths the next [`lines_known`] on this thread
    /// reads in place of [`S4_OPENING_MOUTHS`], once.
    static MURMUR_MOUTHS: std::cell::Cell<Option<&'static [lamp_msgs::SpeakingState]>> =
        const { std::cell::Cell::new(None) };
}

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
    let script = Arc::new(Script {
        heard: Mutex::new(lines.iter().map(|(words, ..)| Ok(words.clone())).collect()),
        chats: Mutex::new(chats.into()),
        early: Mutex::new(
            early
                .map(|early| vec![net::Early::Words(early.to_owned())])
                .into_iter()
                .collect(),
        ),
        ..Script::default()
    });
    let (engine, _, now) = scripted(script.clone());
    // `LAMP_SECOND_PREPARED`, with early words to prepare on.
    let engine = engine.preparing(early.is_some());
    let noted: Arc<Mutex<Vec<Noted>>> = Arc::default();
    let seen_noted = noted.clone();
    let hits: Arc<Mutex<Vec<HitsRead>>> = Arc::default();
    let seen_hits = hits.clone();
    let cache = Arc::new(FaceCache::default());
    let calls: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let (seen_calls, seen_leases) = (calls.clone(), leases.clone());
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
        move |request, heard, name: String, args, lease: ScopeLease| {
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
    let engine = engine
        .naming_by_one_face(name_one_face)
        .reading_by_one_face(ONE_FACE_READS.with(|reads| reads.replace(false)));
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
            Some(Arc::new(move |request: u64| {
                kept.lock().unwrap().going_on_how(request, Instant::now())
            }))
        }
        None => going_on.map(|going_on| -> crate::second_provider::GoingOnWith {
            Arc::new(move |_| going_on.then(|| (a.clone(), false)))
        }),
    });
    let mut engine = engine
        .saving_carried(saves)
        .reading_carried(CARRIED_MEMORY.with(|on| on.replace(false)))
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
    if named {
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
    }
    let mut labels = Vec::new();
    // On the real route the voice's thread is the crate's own, shared by
    // every test: requests of this test's own.
    let base = real.map_or(0, |real| real.base);
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
        engine.submit(end(request));
        labels.extend(turn(&mut engine).await);
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
            });
        }
        let taken = take_held_forget(
            &engine.shared,
            2,
            true,
            "Yes.",
            Some(&asker),
            by_one_face,
            now,
        );
        match taken {
            Taken::Runs(..) => assert!(runs, "by one face: no yes"),
            Taken::Unseen(_) => assert!(!runs, "not by one face: runs"),
            Taken::Nothing | Taken::AskAgain => panic!("dropped for another reason"),
        }
    }
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
/// running for the route: the tracker could never record another voice, so
/// a carried line stays held and the room's, as with no tracker; nor is one
/// the rule never judged read as A's (Jarvis's P2 on #5842).
#[tokio::test]
async fn with_carried_memory_and_no_voice_check_a_carried_line_stays_held() {
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
            "Lamp, what did I say about my doctor?",
            recall_doctor(),
            vec![("f1", false)],
            named,
            true,
            Voice::One,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: the recall runs for no one");
        assert!(got.held, "{what}");
        assert!(
            got.noted.is_empty(),
            "{what}: nothing read: {:?}",
            got.noted
        );
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
