//! Whether a line is said in the voice of the conversation the lamp is in
//! (#829, `docs/same-voice.md`, "In the voice").
//!
//! With `LAMP_SPOKEN_TO=on`, a line said within 45 s of the lamp's last
//! words is taken for the conversation going on, whoever says it. On
//! lamp-4ace on 16 October at 06:07, right after the lamp answered Daniel's
//! "Hey Lamp, how much is one hundred dollars in euros?", Samantha said
//! "Anyway, if we drop the trip we save almost half." to him. It rode on
//! his conversation, and only the model's own silence left it unanswered.
//! With a model (`LAMP_SAME_VOICE_MODEL`), a line whose voice is plainly not
//! the conversation's is no part of it.
//!
//! - **Its own process:** `lamp-same-voice serve MODEL`, installed beside
//!   `lamp-voice` and started by the provider worker ([`Prints`]). It makes
//!   a print of each line (CAM++, `crates/same-voice`). A child that is
//!   missing, still loading, busy or gone gives no print, and a line with
//!   no print is judged as without the model. One that ends is started
//!   again after [`RESTART_AFTER`], and one that gives no answer within
//!   [`ANSWER_WITHIN`] is killed and started again. Children that end
//!   [`MOST_UNREADY`] times in a row before they are ready are not started
//!   again: the journal says once that the check is off.
//! - **Only what was said with the lamp quiet:** a line that began while
//!   the lamp's reply could be heard (`ProviderInput::Start`'s
//!   `over_the_lamps_voice`) is never judged. Over the lamp's voice the model
//!   hears the lamp: on lamp-4ace's recordings it was right no more often
//!   than chance. One begun while a silent reply was in hand is judged:
//!   each scene's call on 18 October came so, and none was printed (#829).
//! - **Only a line of [`LEAST_SAMPLES`] or more**, of which the first
//!   [`ASKED_AT`] are judged, asked for as soon as they are heard.
//! - **The conversation's voice** is the prints of the lines the lamp
//!   answered in it that were long enough to know, the last [`KEPT`] of
//!   them. An answered line too short to know keeps them, unless it began a
//!   conversation of its own (answered for its name or a face, not as one
//!   going on): then no voice is known.
//! - **The rule:** a line's highest cosine against the conversation's
//!   prints, under [`DIFFERENT_BELOW`], is another voice, and no
//!   conversation is going on for it. The same voice, or no verdict, is as without the
//!   model.
use std::{
    collections::VecDeque,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

/// Where the same-voice check's journal lines go: stderr on a lamp. A test
/// may take them instead, to see each one said (#1444).
pub type Journal = Arc<dyn Fn(&str) + Send + Sync>;

/// The lamp's journal: each line to stderr, as `eprintln!` writes it.
pub fn to_stderr() -> Journal {
    Arc::new(|line: &str| eprintln!("{line}"))
}

/// Where the coordinator reads the model's absolute path.
pub const ENVIRONMENT: &str = "LAMP_SAME_VOICE_MODEL";
/// `on` exactly, beside [`ENVIRONMENT`], on the second route: the
/// conversation's voice is its caller's only, the prints of lines let out
/// for themselves (the name, a call, a face turned to it), never of a line
/// let out only as the conversation going on. On lamp-4ace on 21 Oct at
/// 20:06 (sv-ab, the first builder on #2602), one of the other person's
/// lines let out before its print was back made their voice the
/// conversation's, and the two people's talk then went on as one
/// conversation, at cosines of 0.55-0.84. Off unless set.
pub const CALLER_ONLY_ENVIRONMENT: &str = "LAMP_SAME_VOICE_CALLER_ONLY";
/// The provider worker's flag for it, with the path, right after
/// `--spoken-to`.
pub const FLAG: &str = "--same-voice-model";
/// The child's executable, installed beside `lamp-voice`.
pub const EXECUTABLE: &str = "lamp-same-voice";
/// Samples a second.
pub const RATE: usize = 16_000;
/// A line shorter than this is not judged: 1.5 s. Under it, the same voice
/// was taken for another 7.7% of the time on lamp-4ace's recordings.
pub const LEAST_SAMPLES: usize = RATE * 3 / 2;
/// The most of a line the child takes: 3 s.
pub const MOST_SAMPLES: usize = RATE * 3;
/// A line is asked for as soon as this much of it is heard, and only this
/// much is judged: the 0.3 s pre-roll each request begins with, then 1.5 s.
/// Asked for at 3 s or at their ends, scene 3's two short lines were printed
/// after their answers' first output on lamp-4ace on 18 October (#829). On
/// lamp-4ace's October recordings the first 1.8 s told the voices apart no
/// worse than the first 3 s, and the first 1.5 s took 6 of 37 same-voice
/// pairs for two (`docs/verification/same-voice-early-20261018`).
pub const ASKED_AT: usize =
    LEAST_SAMPLES + crate::activity::PRE_ROLL_BLOCKS * lamp_audio::CAPTURE_SAMPLES;
const _: () = assert!(ASKED_AT <= MOST_SAMPLES);
/// The pre-roll each line's clip begins with, sound from before its speech
/// began ([`ASKED_AT`]): not speech of the line, so not counted in how much
/// speech a print was made from ([`Voices::usable_speech`]).
pub const PRE_ROLL_SAMPLES: usize = crate::activity::PRE_ROLL_BLOCKS * lamp_audio::CAPTURE_SAMPLES;
/// Two prints with a cosine under this are taken for two voices, the one
/// bar for every rule that reads a voice: the conversation's own check
/// (#829) and the rules that hold or chain a line as another person's
/// (`LAMP_GOING_ON_NOT_OWN`, `LAMP_QUIET_GOING_ON_SAME_ONE`). Two bars for
/// one question would let the older check hold what the newer rules let
/// through (the first builder at 03:50 on Queue #1722). On lamp-4ace's mic2
/// (21 Oct, 02:29, the first builder at 02:33) the same voice in a quiet
/// room scored down to 0.35 and other voices up to 0.29
/// (`docs/verification/same-voice-20261021`, #2354); on 18 October's
/// labelled set, 89 clips of three played voices on mic2 with the lamp
/// quiet, cut as the worker prints them (`docs/verification/same-voice-20261018`),
/// 1 of 950 same-voice pairs fell under it (lowest 0.27), where #829's
/// earlier bar of 0.45, that set's equal-error bar, held about 1 in 13. The
/// trade: more other voices taken for the same, so more talk between people
/// answered, the case #829 was made for. That set's pairs of two at or over
/// 0.30 are not recountable (only its summary is kept): between about 220
/// (at 0.455, its 7.5% equal error) and 1,269 (at 0.25) of 2,966. Tonight's
/// 0.29 top for other voices is two synthetic voices in one room. Lines
/// under 1.5 s mostly read low (`docs/verification/same-voice-early-20261018`).
/// With the lamp's voice in a print it is at chance, so the check stays off, and is at most
/// one weak sign beside a face, lips and the name (#829).
pub const DIFFERENT_BELOW: f32 = 0.30;
/// Two lines' prints at or over this are one person's: a line that cut
/// the one before it short is asked with that one's words as one line
/// (`LAMP_JOIN_CUT_WORDS`, `second_provider::JOIN_CUT_WORDS_ENVIRONMENT`).
/// A question of its own, so a bar of its own: joining two people's words
/// answers neither, where letting the first person's go is today's way.
/// Over every pair of two voices on 18 October's labelled set (highest
/// 0.69, `docs/verification/same-voice-20261018`), whose own voices' pairs
/// had a median of 0.62, so fewer than half of one person's pairs reach it
/// there; two 6 s lines of one voice with the lamp quiet scored a median
/// 0.95 (`docs/same-voice.md`). The same proof moves a line into one
/// person's memory (`LAMP_MEMORY_FOLLOWS_FACE`, #2176).
pub const SAME_FROM: f32 = 0.70;
/// A child that has ended is started again after this long.
pub const RESTART_AFTER: Duration = Duration::from_secs(5);
/// A child that ends this many times in a row before it is ready is not
/// started again: the check is off until the voice restarts (the reviewer
/// on #1030: a missing program or model was retried every 5 s forever).
pub const MOST_UNREADY: u32 = 3;
/// How long a child has to load its model and say it is ready.
pub const READY_WITHIN: Duration = Duration::from_secs(30);
/// How long a child has to answer a line; one that doesn't is killed and
/// started again (the reviewer on #1030: a child hung after "ready" took
/// 1 of 186 lines and said nothing).
pub const ANSWER_WITHIN: Duration = Duration::from_secs(2);

/// The child's timings, apart for the tests.
#[derive(Clone, Copy)]
struct Timing {
    restart_after: Duration,
    ready_within: Duration,
    answer_within: Duration,
}

const TIMING: Timing = Timing {
    restart_after: RESTART_AFTER,
    ready_within: READY_WITHIN,
    answer_within: ANSWER_WITHIN,
};
/// Prints kept: of lines not yet decided, and of the conversation's
/// answered lines.
const KEPT: usize = 4;

/// The provider worker's arguments for the model in the environment: an
/// absolute path only.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var_os(ENVIRONMENT).as_deref().map(Path::new))
}

fn arguments(model: Option<&Path>) -> Vec<String> {
    match model
        .filter(|path| path.is_absolute())
        .and_then(Path::to_str)
    {
        Some(path) => vec![FLAG.to_owned(), path.to_owned()],
        None => Vec::new(),
    }
}

/// Take a leading `--same-voice-model PATH` off a worker's flags, after
/// `--spoken-to`.
pub fn take(flags: &[String]) -> io::Result<(Option<PathBuf>, &[String])> {
    match flags {
        [flag, path, rest @ ..] if flag == FLAG && Path::new(path).is_absolute() => {
            Ok((Some(PathBuf::from(path)), rest))
        }
        [flag, ..] if flag == FLAG => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{FLAG} takes an absolute path"),
        )),
        _ => Ok((None, flags)),
    }
}

/// The cosine between two prints, as `lamp_same_voice::cosine` takes it
/// (the voice does not link the model's runtime): summed in f64, and none
/// where it is not defined.
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.iter().chain(b).any(|value| !value.is_finite()) {
        return None;
    }
    let (mut dot, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        return None;
    }
    Some((dot / (aa.sqrt() * bb.sqrt())).clamp(-1.0, 1.0) as f32)
}

/// What makes prints: the child, or a stand-in in tests.
pub trait Printer: Send {
    /// Ask for the print of `samples`, the line `request`. Never waits:
    /// false when it cannot be taken now.
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool;
    /// A print that has come, or why there is none, if any has.
    fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)>;
}

impl Printer for Box<dyn Printer> {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        (**self).ask(request, samples)
    }
    fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
        (**self).answer()
    }
}

/// The child `lamp-same-voice serve MODEL`, on a thread of its own: one
/// line asked for at a time, and none while one is being printed.
pub struct Prints {
    asked: mpsc::SyncSender<(u64, Vec<i16>)>,
    answers: mpsc::Receiver<(u64, Result<Vec<f32>, String>)>,
    #[cfg(test)]
    test_worker: Option<thread::JoinHandle<()>>,
    #[cfg(test)]
    test_done: Option<mpsc::Receiver<()>>,
}

impl Prints {
    /// `program serve model`, started now and again whenever it ends,
    /// unless it never gets ready.
    pub fn start(program: PathBuf, model: PathBuf) -> Self {
        Self::timed(program, model, TIMING)
    }

    fn timed(program: PathBuf, model: PathBuf, timing: Timing) -> Self {
        Self::journaled(program, model, timing, to_stderr())
    }

    /// [`Self::timed`], its child's lines said to `journal`.
    fn journaled(program: PathBuf, model: PathBuf, timing: Timing, journal: Journal) -> Self {
        let (asked, jobs) = mpsc::sync_channel(0);
        let (told, answers) = mpsc::channel();
        #[cfg(test)]
        let (finished, done) = mpsc::channel();
        let started = thread::Builder::new()
            .name("same-voice".into())
            .spawn(move || {
                keep(&program, &model, &jobs, &told, timing, &journal);
                #[cfg(test)]
                let _ = finished.send(());
            });
        if let Err(error) = &started {
            eprintln!("lamp-voice provider: the same voice is not judged: {error}");
        }
        Self {
            asked,
            answers,
            #[cfg(test)]
            test_worker: started.ok(),
            #[cfg(test)]
            test_done: Some(done),
        }
    }

    /// The child installed beside the running executable.
    pub fn beside_voice(model: PathBuf) -> io::Result<Self> {
        let program = std::env::current_exe()?.with_file_name(EXECUTABLE);
        Ok(Self::start(program, model))
    }
}

impl Printer for Prints {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        self.asked.try_send((request, samples.to_vec())).is_ok()
    }
    fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
        self.answers.try_recv().ok()
    }
}

/// How one child ended.
enum Ended {
    /// The worker has gone: nothing more is asked.
    Closed,
    /// Why, and whether it had said it was ready.
    Failed(String, bool),
}

/// One child after another until the worker goes, or until children end
/// [`MOST_UNREADY`] times in a row before they are ready.
fn keep(
    program: &Path,
    model: &Path,
    jobs: &mpsc::Receiver<(u64, Vec<i16>)>,
    told: &mpsc::Sender<(u64, Result<Vec<f32>, String>)>,
    timing: Timing,
    journal: &Journal,
) {
    let mut unready = 0;
    loop {
        let why = match child(program, model, jobs, told, timing, journal) {
            Ended::Closed => return,
            Ended::Failed(why, ready) => {
                unready = if ready { 0 } else { unready + 1 };
                why
            }
        };
        if unready >= MOST_UNREADY {
            eprintln!(
                "lamp-voice provider: the same-voice check is off: the child ended {unready} times before it was ready, the last: {why}"
            );
            // Each line asked for from now on is told so, and nothing more
            // is started.
            while let Ok((request, _)) = jobs.recv() {
                if told
                    .send((request, Err("the same-voice check is off".into())))
                    .is_err()
                {
                    return;
                }
            }
            return;
        }
        eprintln!(
            "lamp-voice provider: the same-voice child ended: {why}; it is started again in {} ms",
            timing.restart_after.as_millis()
        );
        // Meanwhile no print can be made: each line asked for is told so.
        let until = Instant::now() + timing.restart_after;
        loop {
            match jobs.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok((request, _)) => {
                    if told
                        .send((request, Err("no same-voice child".into())))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Killed and reaped when dropped, however the child's serving ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One child, from its start until it or the worker goes. Its answers are
/// read on a thread of their own, so that each is waited for with a
/// deadline.
fn child(
    program: &Path,
    model: &Path,
    jobs: &mpsc::Receiver<(u64, Vec<i16>)>,
    told: &mpsc::Sender<(u64, Result<Vec<f32>, String>)>,
    timing: Timing,
    journal: &Journal,
) -> Ended {
    let started = Command::new(program)
        .arg("serve")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let mut running = match started {
        Ok(child) => Running(child),
        Err(error) => return Ended::Failed(format!("{}: {error}", program.display()), false),
    };
    let (Some(mut input), Some(output)) = (running.0.stdin.take(), running.0.stdout.take()) else {
        return Ended::Failed("no pipes".into(), false);
    };
    // Its lines as they come; the reader ends with the child's output.
    let (said, lines) = mpsc::channel::<io::Result<String>>();
    let reading = thread::Builder::new()
        .name("same-voice-out".into())
        .spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = String::new();
                let read = output.read_line(&mut line);
                let over = !matches!(read, Ok(read) if read > 0);
                if said.send(read.map(|_| line)).is_err() || over {
                    return;
                }
            }
        });
    if let Err(error) = reading {
        return Ended::Failed(error.to_string(), false);
    }
    let next = |within: Duration| -> Result<String, String> {
        match lines.recv_timeout(within) {
            Ok(Ok(line)) if !line.is_empty() => Ok(line),
            Ok(Err(error)) => Err(error.to_string()),
            Ok(Ok(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => Err("it ended".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(format!("no answer in {} ms", within.as_millis()))
            }
        }
    };
    match next(timing.ready_within) {
        Ok(line) => match ready(&line) {
            Some(loaded_ms) => eprintln!(
                "lamp-voice provider: the same-voice child is ready, its model loaded in {loaded_ms} ms"
            ),
            None => return Ended::Failed(format!("not ready: {}", line.trim_end()), false),
        },
        Err(why) => return Ended::Failed(format!("before its model was loaded: {why}"), false),
    }
    loop {
        let Ok((request, samples)) = jobs.recv() else {
            return Ended::Closed;
        };
        let asked = Instant::now();
        let mut frame = Vec::with_capacity(12 + samples.len() * 2);
        frame.extend(request.to_le_bytes());
        frame.extend((samples.len() as u32).to_le_bytes());
        frame.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
        if let Err(error) = input.write_all(&frame).and_then(|()| input.flush()) {
            let _ = told.send((request, Err("the child is gone".into())));
            return Ended::Failed(error.to_string(), true);
        }
        let answer = next(timing.answer_within).and_then(|line| answer(&line, request));
        // From its ask to its answer, so that a run with the check on times
        // the child on the lamp (#829: a short line's print lost to
        // Gemini's first output on the A733, which had never been timed).
        if answer.is_ok() {
            journal(&format!(
                "lamp-voice provider: request {request}: its voice print came {} ms after it was asked ({:.2} s of sound)",
                asked.elapsed().as_millis(),
                samples.len() as f32 / RATE as f32
            ));
        }
        let failed = answer.as_ref().err().cloned();
        if told
            .send((request, answer.map_err(|why| format!("no print: {why}"))))
            .is_err()
        {
            return Ended::Closed;
        }
        // An answer that is not one ends the child: what it says next
        // cannot be trusted to be the next line's.
        if let Some(why) = failed.filter(|why| !why.starts_with("refused: ")) {
            return Ended::Failed(why, true);
        }
    }
}

/// How long the model took to load, if `line` says the child is ready.
fn ready(line: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    (value["ready"] == true).then(|| value["loaded_ms"].as_u64().unwrap_or(0))
}

/// The print in the child's answer to `request`. "refused: …" when the
/// child answered it and could make no print of it.
fn answer(line: &str, request: u64) -> Result<Vec<f32>, String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|error| format!("not an answer: {error}"))?;
    if value["request"].as_u64() != Some(request) {
        return Err(format!("an answer to another line: {}", line.trim_end()));
    }
    if let Some(why) = value["error"].as_str() {
        return Err(format!("refused: {why}"));
    }
    let print: Vec<f32> = value["print"]
        .as_array()
        .ok_or("no print in the answer")?
        .iter()
        .map(|value| value.as_f64().map(|value| value as f32))
        .collect::<Option<_>>()
        .ok_or("a print with something not a number")?;
    if print.is_empty() {
        return Err("an empty print".into());
    }
    Ok(print)
}

/// What the check says at a line's verdict ([`Voices::at_verdict`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtVerdict {
    pub likeness: Option<f32>,
    pub unjudged: Option<&'static str>,
    pub late_other: Option<u64>,
}

/// The line being heard.
struct Clip {
    request: u64,
    samples: Vec<i16>,
}

/// The conversation's voice and the lines' prints, as the provider worker
/// follows them.
pub struct Voices<P: Printer> {
    printer: P,
    /// The line being heard while its first samples are gathered, until
    /// they are asked for or it ends.
    clip: Option<Clip>,
    /// Lines whose print was asked for and has not come.
    asked: VecDeque<u64>,
    /// Prints that have come, newest last.
    prints: VecDeque<(u64, Vec<f32>)>,
    /// The voice of the conversation the lamp is in: the prints of the
    /// lines it answered in it, newest last, at most [`KEPT`]. A line is
    /// judged by its highest cosine against them, so that one mixed print,
    /// such as one with the lamp's tail in it, can't set the bar for every
    /// line after it (the first builder on #1445, 10:3x, after the Sixth's
    /// finding there).
    conversation: VecDeque<Vec<f32>>,
    /// A line whose answer was let out and whose print is still to come,
    /// and whether it began a conversation of its own.
    answered: Option<(u64, bool)>,
    /// The line last judged, so that it is judged, and said, once (the
    /// reviewer on #1030).
    judged: Option<u64>,
    /// A line let out as the conversation going on whose print, come after
    /// its verdict, was another voice's: the worker ends the conversation
    /// at the next verdict ([`Self::take_late_other`]).
    late_other: Option<u64>,
    /// The conversation's voice is its caller's only
    /// ([`CALLER_ONLY_ENVIRONMENT`], [`Self::callers_only`]).
    caller_only: bool,
    /// With it, a going-on line the eyes saw only the caller speaking
    /// ([`Self::seen_from_the_caller`]): its print becomes the voice of a
    /// conversation that has none.
    caller_seen: Option<u64>,
    /// Where its lines are said ([`to_stderr`] on a lamp).
    journal: Journal,
    /// How many samples each line's print was asked from, pre-roll and
    /// all, the newest [`KEPT`] ([`Self::usable_speech`]).
    lengths: VecDeque<(u64, usize)>,
    /// A print come after its line's verdict is another voice's only when it
    /// is sure ([`Self::sure_of_another_only`]): from this many samples of
    /// speech or more, and under this cosine. `None`: as [`DIFFERENT_BELOW`].
    sure_of_another: Option<(usize, f32)>,
}

impl<P: Printer> Voices<P> {
    pub fn new(printer: P) -> Self {
        Self {
            printer,
            clip: None,
            asked: VecDeque::new(),
            prints: VecDeque::new(),
            conversation: VecDeque::new(),
            answered: None,
            judged: None,
            late_other: None,
            caller_only: false,
            caller_seen: None,
            journal: to_stderr(),
            lengths: VecDeque::new(),
            sure_of_another: None,
        }
    }

    /// From now on, a print come after its line's verdict ends the
    /// conversation as another voice's only from `least_samples` samples
    /// of speech or more and under `below`; an unsure one says nothing
    /// either way, and is not kept as the conversation's voice
    /// (`LAMP_SAME_SPEAKER_CARRIES`: on lamp-4ace on 23 Oct short prints of
    /// what may have been the owner's own lines read 0.25 and 0.27).
    pub fn sure_of_another_only(&mut self, least_samples: usize, below: f32) {
        self.sure_of_another = Some((least_samples, below));
    }

    /// How much speech the print of the line `request` was made from: the
    /// samples it was asked from less the pre-roll ([`PRE_ROLL_SAMPLES`]),
    /// not the clip's whole length. Nothing for a line not asked of late.
    pub fn usable_speech(&self, request: u64) -> Option<usize> {
        self.lengths
            .iter()
            .rev()
            .find(|(of, _)| *of == request)
            .map(|(_, samples)| samples.saturating_sub(PRE_ROLL_SAMPLES))
    }

    /// The conversation's voice is its caller's only, with `on`
    /// ([`CALLER_ONLY_ENVIRONMENT`]): a line let out only as the
    /// conversation going on is judged against it, and never becomes it.
    pub fn callers_only(mut self, on: bool) -> Self {
        self.caller_only = on;
        self
    }

    /// Whether the conversation's voice is its caller's only.
    pub fn is_callers_only(&self) -> bool {
        self.caller_only
    }

    /// [`Self::new`], its lines said to `journal` (#1444); the second
    /// route's tests read it too.
    #[cfg(test)]
    pub(crate) fn journaled(printer: P, journal: Journal) -> Self {
        Self {
            journal,
            ..Self::new(printer)
        }
    }

    /// Say that `request`, long enough to know, was judged without the
    /// voice check, and `why` (#829, the reviewer on #1435): the worker
    /// calls it at the verdict.
    pub fn judged_without(&self, request: u64, why: &str) {
        (self.journal)(&format!(
            "lamp-voice provider: request {request}: judged without the voice check: {why}"
        ));
    }

    /// The line `request` began; it is judged only if it was said with the
    /// lamp quiet (`judged`).
    pub fn began(&mut self, request: u64, judged: bool) {
        self.clip = judged.then(|| Clip {
            request,
            samples: Vec::with_capacity(ASKED_AT),
        });
    }

    /// More of the line `request` was heard.
    pub fn heard(&mut self, request: u64, samples: &[i16]) {
        let Some(clip) = self.clip.as_mut().filter(|clip| clip.request == request) else {
            return;
        };
        let room = ASKED_AT - clip.samples.len();
        clip.samples
            .extend_from_slice(&samples[..samples.len().min(room)]);
        if clip.samples.len() == ASKED_AT {
            self.ask();
        }
    }

    /// The line `request` ended: what was heard of it is asked for if it is
    /// long enough to know.
    pub fn ended(&mut self, request: u64) {
        match &self.clip {
            Some(clip) if clip.request == request && clip.samples.len() >= LEAST_SAMPLES => {
                self.ask()
            }
            Some(clip) if clip.request == request => {
                self.clip = None;
                self.unprinted(request);
            }
            _ => {}
        }
    }

    fn ask(&mut self) {
        let Some(clip) = self.clip.take() else {
            return;
        };
        if self.printer.ask(clip.request, &clip.samples) {
            if self.asked.len() == KEPT {
                self.asked.pop_front();
            }
            self.asked.push_back(clip.request);
            if self.lengths.len() == KEPT {
                self.lengths.pop_front();
            }
            self.lengths.push_back((clip.request, clip.samples.len()));
        } else {
            // The line is then judged without the check: said, not
            // silently (the reviewer on #1435).
            (self.journal)(&format!(
                "lamp-voice provider: request {}: no voice print: the same-voice child was busy (loading its model or printing another line)",
                clip.request
            ));
            self.unprinted(clip.request);
        }
    }

    /// No print will come of `request`.
    fn unprinted(&mut self, request: u64) {
        if let Some((answered, new)) = self.answered
            && answered == request
        {
            if new {
                self.begin_afresh();
            }
            self.answered = None;
        }
    }

    fn poll(&mut self) {
        while let Some((request, print)) = self.printer.answer() {
            self.asked.retain(|asked| *asked != request);
            match print {
                Ok(print) => {
                    if let Some((_, new)) =
                        self.answered.filter(|(answered, _)| *answered == request)
                    {
                        self.answered = None;
                        self.adopt(request, print.clone(), !new);
                    }
                    if self.prints.len() == KEPT {
                        self.prints.pop_front();
                    }
                    self.prints.push_back((request, print));
                }
                Err(why) => {
                    eprintln!("lamp-voice provider: request {request}: no voice print: {why}");
                    self.unprinted(request);
                }
            }
        }
    }

    /// The print of the line `request`, once it has come.
    pub fn print_of(&mut self, request: u64) -> Option<Vec<f32>> {
        self.poll();
        self.print(request).cloned()
    }

    fn print(&self, request: u64) -> Option<&Vec<f32>> {
        self.prints
            .iter()
            .rev()
            .find(|(of, _)| *of == request)
            .map(|(_, print)| print)
    }

    /// Whether the print of the line `request` is on its way: asked for and
    /// not come, or still being gathered. Its answer's verdict then comes
    /// without it (#829, lamp-4ace on 18 October: a line asked for only at
    /// its end was printed after Gemini's first output).
    pub fn print_coming(&self, request: u64) -> bool {
        self.asked.contains(&request)
            || self
                .clip
                .as_ref()
                .is_some_and(|clip| clip.request == request)
    }

    /// How like the conversation's voice the line `request` is, when both
    /// are known: the highest cosine of its print against the
    /// conversation's. Once a line: asked again for the same line, nothing.
    pub fn likeness(&mut self, request: u64) -> Option<f32> {
        self.poll();
        if self.judged == Some(request) {
            return None;
        }
        let likeness = self.likest(self.print(request)?)?;
        self.judged = Some(request);
        Some(likeness)
    }

    /// [`Self::likeness`] without using up the line's one judgement, for a
    /// sign read before its verdict (`LAMP_TURNED_FACE_SPEAKS`): nothing
    /// while its print or the conversation's voice is not known.
    pub fn likeness_so_far(&mut self, request: u64) -> Option<f32> {
        self.poll();
        self.likest(self.print(request)?)
    }

    /// How alike the lines `a` and `b` sound: the cosine of their two
    /// prints, once both have come ([`SAME_FROM`]). Asked of a line going
    /// on from the one it cut short, which the rule judged already.
    pub fn alike(&mut self, a: u64, b: u64) -> Option<f32> {
        self.poll();
        cosine(self.print(a)?, self.print(b)?)
    }

    /// Whether the line `request` has a print, come and kept, for the
    /// journal of a line not joined ([`Self::alike`]).
    pub fn printed(&mut self, request: u64) -> bool {
        self.poll();
        self.print(request).is_some()
    }

    /// The highest cosine of `print` against the conversation's prints.
    fn likest(&self, print: &[f32]) -> Option<f32> {
        self.conversation
            .iter()
            .filter_map(|voice| cosine(voice, print))
            .reduce(f32::max)
    }

    /// One more print of the conversation's voice, the oldest let go past
    /// [`KEPT`].
    fn keep(&mut self, print: Vec<f32>) {
        if self.conversation.len() == KEPT {
            self.conversation.pop_front();
        }
        self.conversation.push_back(print);
    }

    /// Why the line `request`, long enough to know, has no verdict from
    /// [`Self::likeness`]: its own print still on its way, the
    /// conversation's, or no voice of the conversation known, in which
    /// case waiting for its print would change nothing (the reviewer on
    /// #1435). None for a line judged, too short to know, said over the
    /// lamp, or whose print could not be asked for.
    pub fn unjudged(&self, request: u64) -> Option<&'static str> {
        let coming = self.print_coming(request);
        if self.judged == Some(request) || !coming && self.print(request).is_none() {
            return None;
        }
        match (self.conversation.is_empty(), self.answered) {
            (false, _) => coming.then_some("its voice print was not back at its answer"),
            (true, Some(_)) => Some("the conversation's voice print was not back at its answer"),
            (true, None) => Some("no voice of the conversation is known"),
        }
    }

    /// `print`, of the line `request` let out as the conversation going on
    /// (`going_on`), is one more of the conversation's prints, unless the
    /// line was let out without being judged and its print is another
    /// voice's. Then the conversation's prints are kept as they were, and
    /// the conversation is over (the Seventh on #1445: a print that came
    /// after its verdict made the other person the conversation's voice, so
    /// on a slow lamp scene 3 went on with B). Let out for itself, the line
    /// began a conversation of its own, and its print is that one's voice.
    /// With [`Self::callers_only`], a going-on line's print is never one
    /// more: the conversation's voice is its caller's alone.
    /// With [`Self::sure_of_another_only`] (`LAMP_SAME_SPEAKER_CARRIES`), a
    /// going-on line's print is one more only when it is surely the
    /// conversation's voice (at [`SAME_FROM`] or over) or the line was seen
    /// spoken by the caller ([`Self::seen_from_the_caller`]), at its verdict
    /// or late: a print that was no evidence never becomes the evidence a
    /// later line is judged by (reviewer-two's P2 at 04:31 on #3296, an
    /// unknown voice let out by the ordinary follow-up at 3 s matched itself
    /// at 12 s).
    fn adopt(&mut self, request: u64, print: Vec<f32>, going_on: bool) {
        let likeness = (going_on && self.judged != Some(request))
            .then(|| self.likest(&print))
            .flatten();
        // With `sure_of_another`, an unsure print is neither another voice
        // nor the conversation's.
        let unsure = |likeness: f32| {
            self.sure_of_another.is_some_and(|(least, below)| {
                likeness >= below
                    || self
                        .usable_speech(request)
                        .is_none_or(|speech| speech < least)
            })
        };
        match likeness {
            Some(likeness) if likeness < DIFFERENT_BELOW && unsure(likeness) => {
                (self.journal)(&format!(
                    "lamp-voice provider: request {request}: a print known after its answer at cosine {likeness:.2}, from {} ms of speech: unsure, neither another voice nor the conversation's",
                    self.usable_speech(request).unwrap_or(0) * 1_000 / RATE
                ));
            }
            Some(likeness) if likeness < DIFFERENT_BELOW => {
                eprintln!(
                    "lamp-voice provider: request {request}: another voice than the conversation's, known after its answer (cosine {likeness:.2}): the conversation is over"
                );
                self.late_other = Some(request);
            }
            _ if going_on
                && self.sure_of_another.is_some()
                && self.caller_seen != Some(request)
                && self
                    .likest(&print)
                    .is_none_or(|likeness| likeness < SAME_FROM) =>
            {
                (self.journal)(&format!(
                    "lamp-voice provider: request {request}: let out with no sure sign of the conversation's voice or the caller's face: its print is not kept as the conversation's voice"
                ));
            }
            // The caller's voice only: a going-on line is judged against
            // it, never added to it.
            _ if going_on && self.caller_only => {
                // Only a conversation with no voice yet, from a line the
                // eyes tie to its caller.
                if self.conversation.is_empty() && self.caller_seen == Some(request) {
                    (self.journal)(&format!(
                        "lamp-voice provider: request {request}: seen spoken by the caller alone: its print is the conversation's voice"
                    ));
                    self.keep(print);
                }
            }
            _ if going_on => self.keep(print),
            _ => {
                self.begin_afresh();
                self.keep(print);
            }
        }
    }

    /// A conversation of its own begins: no voice of the one before
    /// counts, a late print from it neither ends this one nor becomes its
    /// voice (Eighth on #829: a late "another voice" from before a named
    /// call ended the call's own conversation at the next line).
    fn begin_afresh(&mut self) {
        self.conversation.clear();
        self.late_other = None;
        self.answered = None;
    }

    /// What the check says at the verdict of the line `request`, all at
    /// once:
    /// - how like the conversation's voice it is, and if it is long enough
    ///   to know but has no likeness, why ([`Self::unjudged`]), both only
    ///   when a conversation is going on for it (`going_on`);
    /// - a line before it, let out as the conversation going on, known since
    ///   for another voice's ([`Self::take_late_other`]). Never for a line
    ///   that called the lamp for itself (`called`): that one begins a
    ///   conversation of its own, and a late voice of the one before ends
    ///   nothing of it (Eighth on #829). In a strict busy room a named call
    ///   that nobody faces plays only as the conversation going on, so a
    ///   late voice taken at it would hold the call.
    pub fn at_verdict(&mut self, request: u64, going_on: bool, called: bool) -> AtVerdict {
        let likeness = going_on.then(|| self.likeness(request)).flatten();
        let unjudged = (going_on && likeness.is_none())
            .then(|| self.unjudged(request))
            .flatten();
        let late_other = (!called).then(|| self.take_late_other()).flatten();
        AtVerdict {
            likeness,
            unjudged,
            late_other,
        }
    }

    /// A line let out as the conversation going on, known since for another
    /// voice's: the conversation is over, as for one held (#1415). Taken at
    /// the next line's verdict, once the lamp's answer to that line, which
    /// began the count afresh, has been spoken.
    pub fn take_late_other(&mut self) -> Option<u64> {
        self.poll();
        self.late_other.take()
    }

    /// With [`Self::callers_only`]: the line `request`, going on, was seen
    /// spoken by the caller's face and no other. If the conversation has no
    /// voice yet (its call was too short to print), this line's print becomes
    /// it (the first builder on #2602, 21 Oct 21:19). Told before
    /// [`Self::decided`].
    pub fn seen_from_the_caller(&mut self, request: u64) {
        self.caller_seen = Some(request);
    }

    /// The answer to the line `request` was judged: let out (`played`) or
    /// not, as part of a conversation going on (`going_on`) or not.
    pub fn decided(&mut self, request: u64, played: bool, going_on: bool) {
        if !played {
            return;
        }
        self.poll();
        // Let out for itself, even with its print still to come: the next
        // line is no part of the conversation before (Eighth on #829).
        if !going_on {
            self.begin_afresh();
        }
        if let Some(print) = self.print(request).cloned() {
            self.adopt(request, print, going_on);
            return;
        }
        if self.print_coming(request) {
            self.answered = Some((request, !going_on));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints by the line's first sample: 1 is one voice, -1 another, 0
    /// none to make; asked lines answered at once, unless `slow`.
    #[derive(Default)]
    struct StandIn {
        slow: bool,
        busy: bool,
        waiting: VecDeque<(u64, Result<Vec<f32>, String>)>,
        asked: Vec<(u64, usize)>,
    }

    impl Printer for StandIn {
        fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
            if self.busy {
                return false;
            }
            self.asked.push((request, samples.len()));
            let print = match samples[0] {
                1 => Ok(vec![1.0, 0.0, 0.0]),
                -1 => Ok(vec![0.0, 1.0, 0.0]),
                // A's voice with something else in it: 0.50 against A's,
                // over the bar.
                2 => Ok(vec![0.5, 0.0, 0.866]),
                // A's voice, a little off: 0.90 against A's, 0.07 against
                // the mixed one, under the bar.
                3 => Ok(vec![0.9, 0.0, -0.436]),
                // Another voice at 0.20 against A's, under the bar.
                4 => Ok(vec![0.2, 0.0, 0.98]),
                // A's voice at 0.35 against A's: another under the bar of
                // 0.45 before this one, the same voice at 0.30.
                5 => Ok(vec![0.35, 0.0, 0.937]),
                _ => Err("refused: none".into()),
            };
            self.waiting.push_back((request, print));
            true
        }
        fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
            if self.slow {
                return None;
            }
            self.waiting.pop_front()
        }
    }

    /// The line `request` said by `voice` for `samples`, with the lamp quiet.
    fn said(voices: &mut Voices<StandIn>, request: u64, voice: i16, samples: usize) {
        voices.began(request, true);
        for _ in 0..samples.div_ceil(320) {
            voices.heard(request, &[voice; 320]);
        }
        voices.ended(request);
    }

    /// #829, lamp-4ace on 18 October: a line's print is on its way while
    /// its first samples are gathered and once it is asked for, until it
    /// comes. A line too short to know, or said over the lamp, has none
    /// coming.
    #[test]
    fn a_print_is_coming_from_its_first_sample_until_it_comes() {
        let mut voices = Voices::new(StandIn {
            slow: true,
            ..StandIn::default()
        });
        voices.began(7, true);
        voices.heard(7, &[1; 320]);
        assert!(voices.print_coming(7));
        for _ in 0..LEAST_SAMPLES.div_ceil(320) {
            voices.heard(7, &[1; 320]);
        }
        voices.ended(7);
        assert!(voices.print_coming(7));
        assert!(!voices.print_coming(8));
        assert_eq!(voices.likeness(7), None);
        voices.printer.slow = false;
        voices.poll();
        assert!(!voices.print_coming(7));
        // Too short to know, and over the lamp: none coming.
        said(&mut voices, 9, 1, LEAST_SAMPLES - 320);
        assert!(!voices.print_coming(9));
        voices.began(10, false);
        voices.heard(10, &[1; 320]);
        assert!(!voices.print_coming(10));
    }

    /// The reviewer on #1435: a line long enough to know but judged without
    /// the check says what was missing: its own print, the conversation's,
    /// or any voice of the conversation. A line judged, too short to know,
    /// said over the lamp, or that the child was too busy to take, says
    /// nothing here.
    #[test]
    fn a_line_judged_without_the_check_says_which_print_was_missing() {
        let mut voices = Voices::new(StandIn {
            slow: true,
            ..StandIn::default()
        });
        said(&mut voices, 1, 1, 48_000);
        assert_eq!(voices.likeness(1), None);
        assert_eq!(
            voices.unjudged(1),
            Some("no voice of the conversation is known")
        );
        // Line 1 let out, its print still to come: line 2 waits on it.
        voices.decided(1, true, false);
        said(&mut voices, 2, -1, 48_000);
        assert_eq!(voices.likeness(2), None);
        assert_eq!(
            voices.unjudged(2),
            Some("the conversation's voice print was not back at its answer")
        );
        // Both come: line 3's own is still to come.
        voices.printer.slow = false;
        voices.poll();
        voices.printer.slow = true;
        said(&mut voices, 3, -1, 48_000);
        assert_eq!(voices.likeness(3), None);
        assert_eq!(
            voices.unjudged(3),
            Some("its voice print was not back at its answer")
        );
        voices.printer.slow = false;
        assert_eq!(voices.likeness(3), Some(0.0));
        assert_eq!(voices.unjudged(3), None);
        said(&mut voices, 4, 1, LEAST_SAMPLES - 320);
        assert_eq!(voices.likeness(4), None);
        assert_eq!(voices.unjudged(4), None);
        voices.began(5, false);
        voices.heard(5, &[1; 320]);
        voices.ended(5);
        assert_eq!(voices.unjudged(5), None);
        voices.printer.busy = true;
        said(&mut voices, 6, 1, 48_000);
        assert_eq!(voices.likeness(6), None);
        assert_eq!(voices.unjudged(6), None);
    }

    #[test]
    fn a_line_is_judged_from_its_first_1_8_s_when_it_is_long_enough_and_said_with_the_lamp_quiet() {
        let mut voices = Voices::new(StandIn::default());
        // 1.8 s and more: asked for at 1.8 s, while it is still heard (#829:
        // asked for at its end, a short line's print came too late on the
        // lamp).
        voices.began(1, true);
        for _ in 1..ASKED_AT / 320 {
            voices.heard(1, &[1; 320]);
        }
        assert!(voices.printer.asked.is_empty());
        voices.heard(1, &[1; 320]);
        assert_eq!(voices.printer.asked, [(1, ASKED_AT)]);
        for _ in 0..60 {
            voices.heard(1, &[1; 320]);
        }
        voices.ended(1);
        // 1.8 s is 28,800 samples, so a 2 s line is asked for before its
        // end (the Seventh on #1445: the rest of this test would pass with
        // the ask at 3 s).
        assert_eq!(ASKED_AT, 28_800);
        voices.began(7, true);
        voices.heard(7, &[1; 32_000]);
        assert_eq!(voices.printer.asked, [(1, ASKED_AT), (7, ASKED_AT)]);
        voices.ended(7);
        // 1.5 to 1.8 s: asked for at its end, all of it.
        said(&mut voices, 2, 1, 25_600);
        // Under 1.5 s, or over the lamp: never.
        said(&mut voices, 3, 1, 22_400);
        voices.began(4, false);
        voices.heard(4, &[1; 32_000]);
        voices.ended(4);
        assert_eq!(
            voices.printer.asked,
            [(1, ASKED_AT), (7, ASKED_AT), (2, 25_600)]
        );
        // Sound of another line is not this one's.
        voices.began(5, true);
        voices.heard(6, &[1; 48_000]);
        voices.ended(5);
        assert_eq!(voices.printer.asked.len(), 3);
    }

    /// A stand-in for the child: a shell script that writes a line to
    /// `starts` beside it each time it is started.
    fn program(name: &str, script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "lamp-same-voice-{name}-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, starts) = (dir.join(EXECUTABLE), dir.join("starts"));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\necho x >> '{}'\n{script}\n",
                starts.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // macOS checks a fresh executable on its first exec, which can take
        // seconds on a loaded Mac: longer than `QUICK`'s 2 s to be ready, so
        // a first start was killed before it counted itself (El on #705, 17
        // Oct). Paid here, untimed, as the voice's other fixtures do; the
        // warm-up run ends before it counts a start. A sibling's fork may
        // still hold the new file open for writing (ETXTBSY on Linux), so
        // that is waited out too.
        let until = Instant::now() + Duration::from_secs(15);
        let warm = loop {
            match std::process::Command::new(&path).arg("warm-up").status() {
                Err(error)
                    if error.kind() == io::ErrorKind::ExecutableFileBusy
                        && Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                status => break status.unwrap(),
            }
        };
        assert!(warm.success(), "same-voice test program warm-up: {warm}");
        (path, starts)
    }

    fn starts(file: &Path) -> usize {
        std::fs::read_to_string(file).map_or(0, |text| text.lines().count())
    }

    const QUICK: Timing = Timing {
        restart_after: Duration::from_millis(20),
        ready_within: Duration::from_secs(2),
        answer_within: Duration::from_millis(200),
    };

    /// A line asked for every 5 ms until an answer `wanted` comes, within 5 s.
    fn ask_until(prints: &mut Prints, wanted: impl Fn(&Result<Vec<f32>, String>) -> bool) {
        let began = Instant::now();
        loop {
            assert!(began.elapsed() < Duration::from_secs(5), "it never came");
            prints.ask(1, &[0; 160]);
            while let Some((_, answer)) = prints.answer() {
                if wanted(&answer) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The reviewer on #1030: a program or model that can't load is not
    /// retried forever. It is started three times, then the check is off,
    /// each line is told so, and nothing more is started.
    #[test]
    fn a_child_that_never_gets_ready_is_started_three_times_and_then_the_check_is_off() {
        let (path, file) = program("unready", "exit 1");
        let mut prints = Prints::timed(path, "model".into(), QUICK);
        ask_until(&mut prints, |answer| {
            answer.as_ref().err().map(String::as_str) == Some("the same-voice check is off")
        });
        assert_eq!(starts(&file), MOST_UNREADY as usize);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(starts(&file), MOST_UNREADY as usize);
    }

    /// A journal that keeps what is said to it, for a test to read.
    fn kept_journal() -> (Journal, Arc<std::sync::Mutex<Vec<String>>>) {
        let kept = Arc::new(std::sync::Mutex::new(Vec::new()));
        let into = Arc::clone(&kept);
        let journal: Journal =
            Arc::new(move |line: &str| into.lock().unwrap().push(line.to_owned()));
        (journal, kept)
    }

    /// These fixtures own their actual constructor's worker. A journal line
    /// or a restart is not evidence that the worker's child was reaped.
    struct FixtureWorker {
        prints: Option<Prints>,
        done: mpsc::Receiver<()>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl FixtureWorker {
        fn start(program: PathBuf, journal: Journal) -> Self {
            Self::new(Prints::journaled(program, "model".into(), QUICK, journal))
        }

        fn new(mut prints: Prints) -> Self {
            let worker = prints.test_worker.take();
            let done = prints.test_done.take().unwrap();
            Self {
                prints: Some(prints),
                done,
                worker,
            }
        }

        fn finish(&mut self) {
            drop(self.prints.take());
            self.done
                .recv_timeout(Duration::from_secs(5))
                .expect("same-voice fixture worker did not finish after its channels closed");
            self.worker.take().unwrap().join().unwrap();
        }
    }

    impl Drop for FixtureWorker {
        fn drop(&mut self) {
            drop(self.prints.take());
            if let Some(worker) = self.worker.take() {
                // Assertion failure or a completion timeout still owns this
                // join. Rust cannot cancel a thread: a stuck OS spawn/reap
                // needs the enclosing test-process watchdog, never detachment.
                eprintln!("same-voice fixture: joining its worker during cleanup");
                if worker.join().is_err() {
                    eprintln!("same-voice fixture: its worker panicked");
                }
            }
        }
    }

    /// #1444: each of the check's three journal lines is said, word for
    /// word, through the journal it was given. Removing any of them fails
    /// here: the busy child's, the verdict's "judged without", and the
    /// timing of a print, from the child's own thread.
    #[test]
    fn each_line_the_check_says_is_said_to_its_journal() {
        // A child too busy to take the line.
        let (journal, kept) = kept_journal();
        let mut voices = Voices::journaled(
            StandIn {
                busy: true,
                ..StandIn::default()
            },
            journal,
        );
        said(&mut voices, 7, 1, ASKED_AT);
        assert_eq!(
            *kept.lock().unwrap(),
            [
                "lamp-voice provider: request 7: no voice print: the same-voice child was busy (loading its model or printing another line)"
            ]
        );
        // The worker's line at a verdict without a print.
        kept.lock().unwrap().clear();
        voices.judged_without(8, "no voice of the conversation is known");
        assert_eq!(
            *kept.lock().unwrap(),
            [
                "lamp-voice provider: request 8: judged without the voice check: no voice of the conversation is known"
            ]
        );
        // A real child that answers one print: its time, once.
        let (path, _) = program(
            "timed",
            r#"printf '{"ready":true,"loaded_ms":1}\n'
dd bs=332 count=1 of=/dev/null 2>/dev/null
printf '{"request":1,"print":[1.0,0.0]}\n'
exec sleep 30"#,
        );
        let (journal, kept) = kept_journal();
        let mut worker = FixtureWorker::start(path, journal);
        ask_until(worker.prints.as_mut().unwrap(), |answer| answer.is_ok());
        let timed: Vec<String> = kept.lock().unwrap().clone();
        assert_eq!(timed.len(), 1, "{timed:?}");
        assert!(
            timed[0].starts_with("lamp-voice provider: request 1: its voice print came ")
                && timed[0].ends_with(" ms after it was asked (0.01 s of sound)"),
            "{timed:?}"
        );
        worker.finish();
    }

    /// The reviewer on #1030: a child that hangs after "ready" is killed
    /// when a line goes unanswered past its deadline, the line is told so,
    /// and another is started.
    #[test]
    fn a_child_that_hangs_after_it_is_ready_is_killed_and_started_again() {
        let (path, file) = program(
            "hung",
            r#"printf '{"ready":true,"loaded_ms":1}\n'; exec sleep 30"#,
        );
        let mut worker = FixtureWorker::new(Prints::timed(path, "model".into(), QUICK));
        ask_until(worker.prints.as_mut().unwrap(), |answer| {
            answer
                .as_ref()
                .err()
                .is_some_and(|why| why == "no print: no answer in 200 ms")
        });
        let began = Instant::now();
        while starts(&file) < 2 {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "never started again"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.finish();
    }

    #[test]
    fn another_voice_is_known_only_against_the_conversation_the_lamp_last_answered_in() {
        let mut voices = Voices::new(StandIn::default());
        // No conversation yet: no verdict.
        said(&mut voices, 1, 1, 48_000);
        assert_eq!(voices.likeness(1), None);
        // Daniel's call answered: his voice is the conversation's.
        voices.decided(1, true, false);
        said(&mut voices, 2, -1, 48_000);
        assert_eq!(voices.likeness(2), Some(0.0));
        said(&mut voices, 3, 1, 32_000);
        assert_eq!(voices.likeness(3), Some(1.0));
        // A line is judged once: asked again, as each chunk of its answer
        // would (the reviewer on #1030), nothing.
        assert_eq!(voices.likeness(3), None);
        // A held answer changes nothing.
        voices.decided(2, false, false);
        said(&mut voices, 30, 1, 32_000);
        assert_eq!(voices.likeness(30), Some(1.0));
        // A short line answered as the conversation going on keeps it.
        said(&mut voices, 4, -1, 16_000);
        assert_eq!(voices.likeness(4), None);
        voices.decided(4, true, true);
        said(&mut voices, 5, 1, 48_000);
        assert_eq!(voices.likeness(5), Some(1.0));
        // A short line that began a conversation of its own: no voice known.
        said(&mut voices, 6, -1, 16_000);
        voices.decided(6, true, false);
        said(&mut voices, 7, 1, 48_000);
        assert_eq!(voices.likeness(7), None);
        // Samantha answered for her name: hers is the conversation's now.
        said(&mut voices, 8, -1, 48_000);
        voices.decided(8, true, false);
        said(&mut voices, 9, 1, 48_000);
        assert_eq!(voices.likeness(9), Some(0.0));
    }

    /// The first builder on #1445 (10:3x), after the Sixth's finding: the
    /// conversation keeps the print of each line the lamp answered in it,
    /// and a line is judged by its highest cosine against them, so that a
    /// mixed print kept among them doesn't make its own speaker's next line
    /// another voice.
    #[test]
    fn a_mixed_print_among_the_conversations_does_not_make_its_own_voice_another() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        // A's line with something else in it: still A's, at 0.50, and kept.
        said(&mut voices, 2, 2, 48_000);
        assert!(
            voices
                .likeness(2)
                .is_some_and(|likeness| (likeness - 0.5).abs() < 0.01)
        );
        voices.decided(2, true, true);
        // A's next line is far from that one but near A's first: the
        // conversation's voice. Against the mixed print alone it would be
        // another voice.
        said(&mut voices, 3, 3, 48_000);
        assert!(
            voices
                .likeness(3)
                .is_some_and(|likeness| (likeness - 0.9).abs() < 0.01)
        );
        voices.decided(3, true, true);
        // Another voice is another against all of them.
        said(&mut voices, 4, -1, 48_000);
        assert_eq!(voices.likeness(4), Some(0.0));
    }

    /// The conversation keeps the last [`KEPT`] of its prints, and a line
    /// that began a conversation of its own starts them afresh.
    #[test]
    fn the_conversation_keeps_its_last_prints_and_a_new_one_begins_afresh() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 2, 48_000);
        voices.decided(1, true, false);
        for request in 2..=(KEPT as u64 + 1) {
            said(&mut voices, request, 1, 48_000);
            voices.likeness(request);
            voices.decided(request, true, true);
        }
        // The mixed first print is let go: A's slightly-off line is judged
        // against A's prints alone.
        assert_eq!(voices.conversation.len(), KEPT);
        said(&mut voices, 10, 3, 48_000);
        assert!(
            voices
                .likeness(10)
                .is_some_and(|likeness| (likeness - 0.9).abs() < 0.01)
        );
        // B let out for her name: hers alone now.
        said(&mut voices, 11, -1, 48_000);
        voices.decided(11, true, false);
        assert_eq!(voices.conversation.len(), 1);
        said(&mut voices, 12, 1, 48_000);
        assert_eq!(voices.likeness(12), Some(0.0));
    }

    /// Eighth on #829: a line let out for itself begins a conversation of
    /// its own, and a late "another voice" from the one before ends
    /// nothing of it: neither one already known nor one whose print comes
    /// after.
    #[test]
    fn a_new_conversation_ends_any_late_voice_of_the_one_before() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        // B let out as A's conversation going on, its print known late:
        // another voice.
        voices.printer.slow = true;
        said(&mut voices, 2, -1, 48_000);
        assert_eq!(voices.likeness(2), None);
        voices.decided(2, true, true);
        voices.printer.slow = false;
        voices.poll();
        // B calls the lamp by name: hers is a conversation of its own.
        said(&mut voices, 3, -1, 48_000);
        voices.decided(3, true, false);
        assert_eq!(voices.take_late_other(), None);
        // A's line let out as B's conversation going on, its print late,
        // then A calls by name before that print comes.
        voices.printer.slow = true;
        said(&mut voices, 4, 1, 48_000);
        voices.decided(4, true, true);
        said(&mut voices, 5, 1, 48_000);
        voices.decided(5, true, false);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), None);
        said(&mut voices, 6, 1, 48_000);
        assert_eq!(voices.likeness(6), Some(1.0));
    }

    /// The reviewer on #1467: at the verdict of a line that called the lamp
    /// for itself, no late "another voice" from before it is taken. In a
    /// strict busy room a named call nobody faces plays only as the
    /// conversation going on, so taking it there would hold the call. Let
    /// out, the call drops it. A line that didn't call takes it.
    #[test]
    fn a_call_takes_no_late_voice_from_before_it_at_its_verdict() {
        let late_before = |voices: &mut Voices<StandIn>| {
            said(voices, 1, 1, 48_000);
            voices.decided(1, true, false);
            voices.printer.slow = true;
            said(voices, 2, -1, 48_000);
            voices.decided(2, true, true);
            voices.printer.slow = false;
            said(voices, 3, 1, 48_000);
        };
        let mut voices = Voices::new(StandIn::default());
        late_before(&mut voices);
        assert_eq!(voices.at_verdict(3, true, true).late_other, None);
        voices.decided(3, true, false);
        said(&mut voices, 4, 1, 48_000);
        assert_eq!(voices.at_verdict(4, true, false).late_other, None);
        let mut voices = Voices::new(StandIn::default());
        late_before(&mut voices);
        let at = voices.at_verdict(3, true, false);
        assert_eq!(at.late_other, Some(2));
        assert_eq!(at.likeness, Some(1.0));
        // With no conversation going on for it, no likeness is asked.
        said(&mut voices, 5, 1, 48_000);
        assert_eq!(voices.at_verdict(5, false, false).likeness, None);
    }

    /// The Seventh on #1445: the print of a line let out as the
    /// conversation going on, come after its verdict, is compared before it
    /// becomes the conversation's voice. Another voice's is not taken, and
    /// the conversation is over; the same voice's is. One that began a
    /// conversation of its own becomes its voice unchecked.
    #[test]
    fn a_late_print_is_compared_before_it_becomes_the_conversations_voice() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        voices.printer.slow = true;
        said(&mut voices, 2, -1, 48_000);
        assert_eq!(voices.likeness(2), None);
        voices.decided(2, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), Some(2));
        assert_eq!(voices.take_late_other(), None);
        // A's voice is still the conversation's.
        said(&mut voices, 3, 1, 48_000);
        assert_eq!(voices.likeness(3), Some(1.0));
        voices.decided(3, true, true);
        // The same voice, late: taken.
        voices.printer.slow = true;
        said(&mut voices, 4, 1, 48_000);
        assert_eq!(voices.likeness(4), None);
        voices.decided(4, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), None);
        // B let out for her name, late: hers is the conversation's now.
        voices.printer.slow = true;
        said(&mut voices, 5, -1, 48_000);
        voices.decided(5, true, false);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), None);
        said(&mut voices, 6, -1, 48_000);
        assert_eq!(voices.likeness(6), Some(1.0));
    }

    /// `LAMP_SAME_SPEAKER_CARRIES` (reviewer-two at 03:07 on Queue #3126): a
    /// print's speech is counted without the 0.3 s pre-roll, 1.5 s for a
    /// full print of 1.8 s; and with [`Voices::sure_of_another_only`] a late
    /// print of another voice from less speech than asked is unsure: no
    /// late other, and not the conversation's voice. From enough speech it
    /// is another voice, as without it.
    #[test]
    fn a_late_print_from_too_little_speech_is_unsure() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        assert_eq!(voices.usable_speech(1), Some(RATE * 3 / 2));
        voices.decided(1, true, false);
        voices.sure_of_another_only(RATE * 5 / 2, DIFFERENT_BELOW);
        voices.printer.slow = true;
        said(&mut voices, 2, -1, 48_000);
        voices.decided(2, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), None, "unsure");
        said(&mut voices, 3, -1, 48_000);
        assert_eq!(voices.likeness(3), Some(0.0), "not the conversation's");
        voices.sure_of_another_only(RATE, DIFFERENT_BELOW);
        voices.printer.slow = true;
        said(&mut voices, 4, -1, 48_000);
        voices.decided(4, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), Some(4), "sure");
    }

    /// `LAMP_SAME_SPEAKER_CARRIES` (reviewer-two's P2 at 04:31 on #3296):
    /// another voice's line let out with no sure sign (the ordinary
    /// follow-up), its print back at its verdict or after it, is not kept as
    /// the conversation's voice, caller-only on or off: the same voice later
    /// still reads as another, not 1.0 against itself. A line seen spoken by
    /// the caller is kept, and so is a print surely the conversation's.
    #[test]
    fn an_unsure_print_let_out_is_not_the_conversations_voice() {
        for caller_only in [false, true] {
            for late in [false, true] {
                let mut voices = Voices::new(StandIn::default()).callers_only(caller_only);
                voices.sure_of_another_only(RATE * 5 / 2, DIFFERENT_BELOW);
                said(&mut voices, 1, 1, 48_000);
                voices.decided(1, true, false);
                voices.printer.slow = late;
                said(&mut voices, 2, -1, 48_000);
                if !late {
                    assert_eq!(voices.likeness(2), Some(0.0));
                }
                voices.decided(2, true, true);
                voices.printer.slow = false;
                said(&mut voices, 3, -1, 48_000);
                let case = format!("caller-only {caller_only}, late {late}");
                assert_eq!(voices.likeness(3), Some(0.0), "{case}: not kept");
                // Control: the caller's own line, seen spoken, its print not
                // sure, is kept (with caller-only, only into a voiceless
                // conversation, as before).
                if !caller_only {
                    voices.seen_from_the_caller(4);
                    // 0.50 against the caller's: not sure by itself.
                    said(&mut voices, 4, 2, 48_000);
                    voices.decided(4, true, true);
                    said(&mut voices, 5, 2, 48_000);
                    assert_eq!(voices.likeness(5), Some(1.0), "{case}: the caller's kept");
                }
            }
        }
    }

    /// sv-ab on lamp-4ace (21 Oct 20:06, the first builder on #2602): with
    /// [`CALLER_ONLY_ENVIRONMENT`], the conversation's voice is its caller's
    /// alone. A calls and A's follow-up goes on. B's line, its print back
    /// only after its answer, is let out as going on (0.35 against A's, over
    /// the bar). Without the switch, B's print joins the conversation's, and
    /// B's next line (0.20 against A's, 0.99 against B's) goes on with it.
    /// With the switch, that line is another voice. A's own follow-ups go on
    /// either way.
    #[test]
    fn with_the_caller_only_a_going_on_line_never_becomes_the_conversations_voice() {
        for caller_only in [false, true] {
            let mut voices = Voices::new(StandIn::default()).callers_only(caller_only);
            said(&mut voices, 1, 1, 48_000);
            voices.decided(1, true, false);
            said(&mut voices, 2, 1, 48_000);
            assert_eq!(voices.likeness(2), Some(1.0));
            voices.decided(2, true, true);
            voices.printer.slow = true;
            said(&mut voices, 3, 5, 48_000);
            assert_eq!(voices.likeness(3), None, "its print not back");
            voices.decided(3, true, true);
            voices.printer.slow = false;
            assert_eq!(voices.take_late_other(), None, "over the bar");
            said(&mut voices, 4, 4, 48_000);
            let likeness = voices.likeness(4).unwrap();
            if caller_only {
                assert!(likeness < DIFFERENT_BELOW, "{likeness}: another voice");
            } else {
                assert!(likeness > 0.9, "{likeness}: B's voice was taken in");
            }
            said(&mut voices, 5, 1, 48_000);
            assert_eq!(voices.likeness(5), Some(1.0), "the caller goes on");
        }
    }

    /// The first builder on #2602 (21 Oct 21:19): with the caller's voice
    /// only, a call too short to print leaves the conversation with none,
    /// until a follow-up the eyes saw spoken by the caller alone: its print
    /// becomes the voice, and the next other voice is another. A follow-up
    /// not so seen leaves it with none; once there is a voice, a later one
    /// seen so adds nothing.
    #[test]
    fn a_short_call_takes_its_voice_from_the_callers_first_follow_up_seen_spoken() {
        for seen in [true, false] {
            let mut voices = Voices::new(StandIn::default()).callers_only(true);
            said(&mut voices, 1, 1, 8_000);
            voices.decided(1, true, false);
            said(&mut voices, 2, 1, 48_000);
            assert_eq!(voices.likeness(2), None, "no voice yet");
            if seen {
                voices.seen_from_the_caller(2);
            }
            voices.decided(2, true, true);
            said(&mut voices, 3, -1, 48_000);
            if seen {
                assert_eq!(voices.likeness(3), Some(0.0), "another voice");
            } else {
                assert_eq!(voices.likeness(3), None, "still no voice");
            }
        }
        // A voice known: a later line seen from the caller is never added.
        let mut voices = Voices::new(StandIn::default()).callers_only(true);
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        said(&mut voices, 2, 5, 48_000);
        voices.seen_from_the_caller(2);
        voices.decided(2, true, true);
        said(&mut voices, 3, 4, 48_000);
        assert!(
            voices
                .likeness(3)
                .is_some_and(|likeness| likeness < DIFFERENT_BELOW)
        );
    }

    /// A voice at 0.20 against the conversation's is another, whether its
    /// print is back at its verdict or comes after it.
    #[test]
    fn a_voice_at_0_20_against_the_conversations_is_another() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        said(&mut voices, 2, 4, 48_000);
        assert!(
            voices.likeness(2).is_some_and(
                |likeness| (likeness - 0.20).abs() < 0.01 && likeness < DIFFERENT_BELOW
            )
        );
        // Late, and let out as the conversation going on: known after its
        // answer for another voice's, and not kept as the conversation's.
        voices.printer.slow = true;
        said(&mut voices, 3, 4, 48_000);
        voices.decided(3, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), Some(3));
        said(&mut voices, 4, 4, 48_000);
        assert!(
            voices
                .likeness(4)
                .is_some_and(|likeness| (likeness - 0.20).abs() < 0.01)
        );
    }

    /// One bar for the voice (the first builder at 03:50 on Queue #1722): a
    /// print at 0.35 against the conversation's, another voice under #829's
    /// bar of 0.45, is the conversation's own at 0.30. Late too, it is kept
    /// and ends nothing. On lamp-4ace's mic2 one voice in a quiet room read
    /// down to 0.35 against itself (`docs/verification/same-voice-20261021`).
    #[test]
    fn a_voice_at_0_35_was_another_at_0_45_and_is_the_conversations_at_0_30() {
        let mut voices = Voices::new(StandIn::default());
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        said(&mut voices, 2, 5, 48_000);
        assert!(
            voices.likeness(2).is_some_and(
                |likeness| (likeness - 0.35).abs() < 0.01 && likeness >= DIFFERENT_BELOW
            )
        );
        voices.printer.slow = true;
        said(&mut voices, 3, 5, 48_000);
        voices.decided(3, true, true);
        voices.printer.slow = false;
        assert_eq!(voices.take_late_other(), None);
    }

    #[test]
    fn a_print_that_comes_after_its_answer_still_becomes_the_conversations() {
        let mut voices = Voices::new(StandIn {
            slow: true,
            ..Default::default()
        });
        said(&mut voices, 1, 1, 48_000);
        voices.decided(1, true, false);
        voices.printer.slow = false;
        said(&mut voices, 2, -1, 48_000);
        assert_eq!(voices.likeness(2), Some(0.0));
        // A print refused, or a child too busy to take the line: no voice
        // known for a conversation that line began.
        voices.printer.slow = true;
        said(&mut voices, 3, 0, 48_000);
        voices.decided(3, true, false);
        voices.printer.slow = false;
        said(&mut voices, 4, 1, 48_000);
        assert_eq!(voices.likeness(4), None);
        voices.decided(4, true, false);
        voices.printer.busy = true;
        said(&mut voices, 5, -1, 48_000);
        voices.decided(5, true, false);
        voices.printer.busy = false;
        said(&mut voices, 6, 1, 48_000);
        assert_eq!(voices.likeness(6), None);
    }

    #[test]
    fn the_model_path_is_absolute_and_its_flag_follows_the_spoken_to_one() {
        assert_eq!(
            arguments(Some(Path::new("/opt/lampOS/same-voice/campplus.onnx"))),
            [FLAG, "/opt/lampOS/same-voice/campplus.onnx"]
        );
        assert!(arguments(Some(Path::new("campplus.onnx"))).is_empty());
        assert!(arguments(None).is_empty());
        let flags = [FLAG, "/m.onnx", "--rest"].map(str::to_owned);
        let (model, rest) = take(&flags).unwrap();
        assert_eq!(model.as_deref(), Some(Path::new("/m.onnx")));
        assert_eq!(rest, ["--rest"]);
        assert_eq!(take(&flags[2..]).unwrap(), (None, &flags[2..]));
        assert!(take(&[FLAG.to_owned(), "m.onnx".into()]).is_err());
        assert!(take(&[FLAG.to_owned()]).is_err());
    }

    #[test]
    fn the_childs_answers_are_read_as_its_serving_writes_them() {
        assert_eq!(ready(r#"{"ready":true,"loaded_ms":1800}"#), Some(1_800));
        assert_eq!(ready(r#"{"request":1,"print":[1]}"#), None);
        assert_eq!(
            answer(r#"{"request":7,"print":[0.5,-0.25],"ms":75}"#, 7),
            Ok(vec![0.5, -0.25])
        );
        assert!(answer(r#"{"request":7,"print":[0.5],"ms":75}"#, 8).is_err());
        assert_eq!(
            answer(r#"{"request":7,"error":"shorter than one frame"}"#, 7),
            Err("refused: shorter than one frame".into())
        );
        assert!(answer(r#"{"request":7,"print":[]}"#, 7).is_err());
        assert!(answer("lamp", 7).is_err());
    }
}
