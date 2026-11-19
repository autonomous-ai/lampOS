//! A make that looks one named thing up checks once that it can be found
//! (`LAMP_MAKE_FOUND_CHECK`, off unless `on`; #7680: on lamp-9adf "…the
//! price of zorbo coin every morning" made `zorbo-coin-price` and put it
//! on in 2.8 s, and no such coin exists).
//!
//! With the switch on:
//! - **The make's one more call may name what it looks up**
//!   (`make_no_cannot::system`): one line `looks up: <kind> | <name as
//!   said>`, its kind one of [`KINDS`], read by `make_no_cannot::read` into
//!   a thread-local beside its answer, which stays as it was.
//! - **Only a make granted `net`** with such a line is checked: one that
//!   only speaks or lights is never searched for.
//! - **One search, then one judge** ([`Check`]): the voice's own web search
//!   (`second_provider::net::search`, `LAMP_SECOND_SEARCH`) for
//!   `"<name> <kind>"` ([`LooksUp::query`], the kind not said twice when
//!   the name already ends with it), begun as soon as the line is read,
//!   bounded at [`SEARCH_WITHIN`]; then one short call to the make's own
//!   chat, bounded at [`JUDGE_WITHIN`], answering `found`, `not_found` or
//!   `unsure`. Waited for just before the task is kept
//!   (`make_skill_local::agent_task`, [`refused`]). At most 6 s added,
//!   only on such makes.
//! - **`not_found` is not made, and nothing is asked** (the first
//!   builder's yes on #7680 under the owner's no-asks rule): nothing is
//!   written, no yes is waited for, and the lamp says the one plain line
//!   [`crate::made_skill::not_found_line`] alone
//!   (`make_skill_local::make_with_by`). Found, no search, a search with
//!   no results (the judge is never asked: a search misses small real
//!   things), a timeout, an error or `unsure` make it as before: the check
//!   fails open.
//! - **The journal names the kind, never the name** ([`Check::not_kept`],
//!   and the make's own line says [`NOT_FOUND`]).
//!
//! Off, the make's prompt and its parse are byte for byte as before, and
//! nothing is searched.
use crate::second_provider::net::{Chat, ChatEvent, Message, search::Search};
use serde_json::{Value, json};
use std::{
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};

/// The switch, in lamp-voice alone: `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_FOUND_CHECK";

/// What a make may look up by its name.
pub const KINDS: [&str; 4] = ["coin", "stock", "place", "site"];

/// The most characters of a name looked up.
pub const MOST_NAME_CHARS: usize = 60;

/// The longest the search is waited for, from when it began: the search's
/// own bound (`search::WITHIN`).
pub const SEARCH_WITHIN: Duration = crate::second_provider::net::search::WITHIN;

/// The longest the judge is waited for.
pub const JUDGE_WITHIN: Duration = Duration::from_secs(3);

/// The most bytes of the judge's answer read.
const JUDGE_MOST_BYTES: usize = 200;

/// The most characters of the search's results the judge is told.
const RESULTS_MOST_CHARS: usize = 1_800;

/// Why a make was not kept when what it looks up can't be found, as the
/// make's refusal carries it and the journal says it: never the name. The
/// person hears [`take_not_found_line`] instead.
pub const NOT_FOUND: &str =
    "what it looks up by name can't be found (LAMP_MAKE_FOUND_CHECK), so it wasn't made";

/// The judge's system prompt.
const JUDGE_SYSTEM: &str = "You check whether a web search found a thing a person named. Answer \
                            with one word: found, not_found or unsure.";

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// The search a test of this thread gives: the answer the voice's
    /// search would give for a query.
    static SEARCH_HERE: std::cell::RefCell<Option<FakeSearch>> =
        const { std::cell::RefCell::new(None) };
    /// The judge a test of this thread gives: its answer, or none (no
    /// answer in time).
    static JUDGE_HERE: std::cell::RefCell<Option<FakeJudge>> =
        const { std::cell::RefCell::new(None) };
    /// How many searches were begun on this thread.
    static SEARCHES_HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Both bounds, shorter, for a test of this thread.
    static WITHIN_HERE: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
type FakeSearch = Arc<dyn Fn(&str) -> Value + Send + Sync>;
#[cfg(test)]
type FakeJudge = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns the switch on: `on`, and nothing else.
pub fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// The search for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_search_here(search: Option<FakeSearch>) {
    SEARCH_HERE.with(|here| here.replace(search));
}

/// The judge for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_judge_here(judge: Option<FakeJudge>) {
    JUDGE_HERE.with(|here| here.replace(judge));
}

/// Both bounds for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_within_here(within: Option<Duration>) {
    WITHIN_HERE.with(|here| here.set(within));
}

/// How many searches were begun on this test's thread.
#[cfg(test)]
pub(crate) fn searches_here() -> usize {
    SEARCHES_HERE.with(std::cell::Cell::get)
}

/// What a make looks up by its name: its kind (one of [`KINDS`]) and the
/// name as the person said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LooksUp {
    pub kind: String,
    pub name: String,
}

impl LooksUp {
    /// The `looks up:` line's rest (`coin | zorbo`) read: its kind one of
    /// [`KINDS`], in any case, and its name one plain line of 1 to
    /// [`MOST_NAME_CHARS`] characters, no quote, backslash or control
    /// character; else none.
    pub fn read(rest: &str) -> Option<Self> {
        let (kind, name) = rest.split_once('|')?;
        let kind = kind.trim().trim_matches(['"', '\'', '`']).to_lowercase();
        let kind = KINDS.iter().find(|known| **known == kind)?;
        let name: String = name
            .trim()
            .trim_matches(['"', '\'', '“', '”', '`', '*'])
            .chars()
            .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '“' | '”' | '|'))
            .collect();
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        (!name.is_empty() && name.chars().count() <= MOST_NAME_CHARS).then(|| Self {
            kind: (*kind).to_owned(),
            name,
        })
    }

    /// The query searched for: `"<name> <kind>"`, or the name alone when
    /// it already ends with its kind ("zorbo coin", never "zorbo coin
    /// coin").
    pub fn query(&self) -> String {
        let ends_with_kind = self
            .name
            .split_whitespace()
            .last()
            .is_some_and(|last| last.eq_ignore_ascii_case(&self.kind));
        if ends_with_kind {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.kind)
        }
    }
}

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Found,
    NotFound,
    /// Not known, and why, in a few words of the lamp's own: no search,
    /// a timeout, an error or the judge unsure. Made as before.
    Unknown(&'static str),
}

impl Found {
    /// How the journal says it.
    pub fn journaled(&self) -> String {
        match self {
            Self::Found => "found".to_owned(),
            Self::NotFound => "not_found".to_owned(),
            Self::Unknown(why) => format!("unknown ({why})"),
        }
    }
}

/// The voice's own web search, as the second worker set it up
/// (`LAMP_SECOND_SEARCH`). Set once.
static SEARCH: OnceLock<Arc<Search>> = OnceLock::new();

/// Kept by the second worker as it starts, beside the make's own chat
/// (`make_skill_local::set_config`), when it has a search.
pub fn set_search(search: Arc<Search>) {
    let _ = SEARCH.set(search);
}

/// What one search gave: its answer, or why none.
type Searched = Result<Value, &'static str>;

/// One check under way: what is looked up, when its search began, and its
/// answer when it comes.
pub struct Check {
    looks_up: LooksUp,
    began: Instant,
    searched: Option<mpsc::Receiver<Searched>>,
}

thread_local! {
    /// Whether a voice make on this thread may begin a check ([`Armed`]).
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The check begun for the make on this thread, until it is taken.
    static PENDING: std::cell::RefCell<Option<Check>> = const { std::cell::RefCell::new(None) };
    /// What the make on this thread has checked already: a repair naming
    /// the same thing is not searched again.
    static CHECKED: std::cell::RefCell<Option<LooksUp>> = const { std::cell::RefCell::new(None) };
    /// The line the person hears for the make on this thread that was not
    /// made ([`NOT_FOUND`]), until it is taken.
    static NOT_FOUND_SAID: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// What this thread holds for one make, cleared.
fn cleared() {
    PENDING.with(|pending| pending.replace(None));
    CHECKED.with(|checked| checked.replace(None));
    NOT_FOUND_SAID.with(|said| said.replace(None));
}

/// Held by a voice make (`make_skill_local::make_with_by`) while it runs:
/// only then may a check begin, and whatever is left is dropped with it,
/// so no later make on this thread is refused by it, and the app's make
/// never searches.
pub struct Armed;

impl Armed {
    pub fn new() -> Self {
        cleared();
        ARMED.with(|armed| armed.set(true));
        Self
    }
}

impl Default for Armed {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        ARMED.with(|armed| armed.set(false));
        cleared();
    }
}

/// The check of `looks_up` begun now, for the make on this thread: with
/// the switch on and an [`Armed`] make only; its search on a thread of its
/// own. One for the same thing begun or checked already stands.
pub fn begin(looks_up: LooksUp) {
    if !on() || !ARMED.with(std::cell::Cell::get) {
        return;
    }
    let same = PENDING.with(|pending| {
        pending
            .borrow()
            .as_ref()
            .is_some_and(|check| check.looks_up == looks_up)
    }) || CHECKED.with(|checked| checked.borrow().as_ref() == Some(&looks_up));
    if same {
        return;
    }
    let check = Check::begin(looks_up);
    PENDING.with(|pending| pending.replace(Some(check)));
}

/// The check begun for the make on this thread, taken, and what it looks
/// up kept as checked.
pub fn take() -> Option<Check> {
    let check = PENDING.with(|pending| pending.borrow_mut().take())?;
    CHECKED.with(|checked| checked.replace(Some(check.looks_up.clone())));
    Some(check)
}

/// The check begun for the make on this thread, waited for just before
/// its task is kept (`make_skill_local::agent_task`) on the make's own
/// `chat`: refused with [`NOT_FOUND`] when what it looks up can't be
/// found, so nothing is written and nothing waits, the line the person
/// hears kept for [`take_not_found_line`]. None, so it is kept as before,
/// with the switch off, no check begun, or found, unknown, a timeout or an
/// error.
pub fn refused(
    chat: &Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
) -> Option<crate::made_skill::Refused> {
    if !on() {
        return None;
    }
    let line = take()?.not_kept(Arc::clone(chat))?;
    NOT_FOUND_SAID.with(|said| said.replace(Some(line)));
    Some(crate::made_skill::Refused(NOT_FOUND.to_owned(), None))
}

/// The line the person hears for the make on this thread that was not
/// made ([`refused`]), taken.
pub fn take_not_found_line() -> Option<String> {
    NOT_FOUND_SAID.with(std::cell::RefCell::take)
}

/// The bound of the search, then of the judge.
fn bounds() -> (Duration, Duration) {
    #[cfg(test)]
    if let Some(within) = WITHIN_HERE.with(std::cell::Cell::get) {
        return (within, within);
    }
    (SEARCH_WITHIN, JUDGE_WITHIN)
}

/// The search of `query`, asked on a thread of its own with a runtime of
/// its own and a client of its own ([`Search::alone`]); its answer sent.
fn search_on_its_own(query: String) -> Option<mpsc::Receiver<Searched>> {
    #[cfg(test)]
    {
        SEARCHES_HERE.with(|count| count.set(count.get() + 1));
        if let Some(search) = SEARCH_HERE.with(|here| here.borrow().clone()) {
            let (sent, answered) = mpsc::sync_channel(1);
            let _ = std::thread::Builder::new()
                .name("lamp-voice-make-found".into())
                .spawn(move || {
                    let _ = sent.send(Ok(search(&query)));
                });
            return Some(answered);
        }
    }
    let search = SEARCH.get()?.alone().ok()?;
    let (sent, answered) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("lamp-voice-make-found".into())
        .spawn(move || {
            let searched = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "no runtime")
                .map(|runtime| {
                    runtime.block_on(search.answer(
                        crate::second_provider::net::search::NAME,
                        &json!({ "query": query }),
                    ))
                });
            let _ = sent.send(searched);
        })
        .ok()?;
    Some(answered)
}

impl Check {
    /// The check of `looks_up`, its search begun now.
    fn begin(looks_up: LooksUp) -> Self {
        let searched = search_on_its_own(looks_up.query());
        Self {
            looks_up,
            began: Instant::now(),
            searched,
        }
    }

    /// What was found: the search's answer, waited for until its bound
    /// from when it began, then the judge's on the make's own `chat`.
    pub fn found(
        self,
        chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    ) -> Found {
        let (search_within, judge_within) = bounds();
        let Some(searched) = self.searched else {
            return Found::Unknown("no search");
        };
        let left = (self.began + search_within + Duration::from_millis(100))
            .saturating_duration_since(Instant::now());
        let answer = match searched.recv_timeout(left) {
            Ok(Ok(answer)) => answer,
            Ok(Err(why)) => return Found::Unknown(why),
            Err(mpsc::RecvTimeoutError::Timeout) => return Found::Unknown("the search timed out"),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Found::Unknown("the search failed");
            }
        };
        if let Some(error) = answer.get("error").and_then(Value::as_str) {
            return Found::Unknown(if error.contains("in time") {
                "the search timed out"
            } else {
                "the search failed"
            });
        }
        // No hits (`{"results": []}`, or a 2xx with no `results`) says
        // nothing either way: a small real thing a search misses ("phở 24")
        // is made as before, and the judge is never asked (#7840 HOLD).
        if !has_results(&answer) {
            return Found::Unknown("no results");
        }
        judged(chat, question(&self.looks_up, &answer), judge_within)
    }

    /// [`Self::found`], journaled by its kind alone, never its name, with
    /// how long it took from its search's start: the one line the person
    /// hears ([`crate::made_skill::not_found_line`]) when it is not found,
    /// so the make is not kept and nothing is asked; none when it is made
    /// as before.
    pub fn not_kept(
        self,
        chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    ) -> Option<String> {
        let (looks_up, began) = (self.looks_up.clone(), self.began);
        let found = self.found(chat);
        let kind = &looks_up.kind;
        eprintln!(
            "lamp-voice: a skill made here: it looks a {kind} up by name: checked once: {}, {} ms",
            found.journaled(),
            began.elapsed().as_millis()
        );
        (found == Found::NotFound).then(|| {
            eprintln!(
                "lamp-voice: a skill made here: it looks a {kind} up by name: not found: not \
                 made, nothing asked"
            );
            crate::made_skill::not_found_line(kind, &looks_up.name)
        })
    }
}

/// One check of `kind` called `name`, at once: its search, then its judge
/// on the make's own `chat`.
pub fn found(
    kind: &str,
    name: &str,
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
) -> Found {
    Check::begin(LooksUp {
        kind: kind.to_owned(),
        name: name.to_owned(),
    })
    .found(chat)
}

/// Whether the search's `answer` has at least one hit.
fn has_results(answer: &Value) -> bool {
    answer["results"]
        .as_array()
        .is_some_and(|results| !results.is_empty())
}

/// What the judge is asked of `looks_up` and the search's `answer`.
fn question(looks_up: &LooksUp, answer: &Value) -> String {
    let results: Vec<String> = answer["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|result| {
            format!(
                "- {}: {}",
                result["title"].as_str().unwrap_or_default(),
                result["text"].as_str().unwrap_or_default()
            )
        })
        .collect();
    let mut results = if results.is_empty() {
        "(no results)".to_owned()
    } else {
        results.join("\n")
    };
    if results.chars().count() > RESULTS_MOST_CHARS {
        results = results.chars().take(RESULTS_MOST_CHARS).collect();
    }
    let query = looks_up.query();
    let LooksUp { kind, name } = looks_up;
    format!(
        "A person asked for a skill that looks up a {kind} called \"{name}\". A web search for \
         \"{query}\" gave:\n{results}\n\nIs there a real {kind} by that name (or by a name \
         that sounds the same)? Answer found if the results show one, not_found if they show no \
         such {kind}, unsure if you can't tell. One word."
    )
}

/// The judge's `text` read: `not_found` (or "not found"), `found`, or
/// anything else unsure.
fn verdict(text: &str) -> Found {
    let text = text
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
        .replace([' ', '-'], "_");
    if text.starts_with("not_found") {
        Found::NotFound
    } else if text.starts_with("found") {
        Found::Found
    } else {
        Found::Unknown("unsure")
    }
}

/// The judge's answer to `question` from the make's own chat, on a thread
/// of its own, within `within`, by a channel, never a join (as
/// `make_skill_local::judged`): a thread past its bound ends by itself.
fn judged(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    question: String,
    within: Duration,
) -> Found {
    #[cfg(test)]
    if let Some(judge) = JUDGE_HERE.with(|here| here.borrow().clone()) {
        let (sent, answered) = mpsc::sync_channel(1);
        let _ = std::thread::spawn(move || {
            if let Some(text) = judge(&question) {
                let _ = sent.send(text);
            } else {
                std::thread::sleep(within + Duration::from_millis(500));
            }
        });
        return match answered.recv_timeout(within + Duration::from_millis(100)) {
            Ok(text) => verdict(&text),
            Err(_) => Found::Unknown("the judge timed out"),
        };
    }
    let (sent, answered) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("lamp-voice-make-found-judge".into())
        .spawn(move || {
            let _ = sent.send(judge_once(chat, question, within));
        });
    match thread {
        Ok(_) => answered
            .recv_timeout(within + Duration::from_millis(100))
            .unwrap_or(Err("the judge timed out"))
            .map_or_else(Found::Unknown, |text| verdict(&text)),
        Err(_) => Found::Unknown("the judge failed"),
    }
}

/// One judge call within `within`, on its own runtime ([`judged`]).
fn judge_once(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    question: String,
    within: Duration,
) -> Result<String, &'static str> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "the judge failed")?;
    let chat = chat().map_err(|_| "the judge failed")?;
    runtime.block_on(async {
        tokio::time::timeout(within, async {
            let mut stream = chat
                .send(&[Message::system(JUDGE_SYSTEM), Message::user(&question)])
                .await
                .map_err(|_| "the judge failed")?;
            let mut text = String::new();
            while let Some(event) = stream.next().await.map_err(|_| "the judge failed")? {
                if let ChatEvent::Text(words) = event {
                    text.push_str(&words);
                    if text.len() > JUDGE_MOST_BYTES {
                        break;
                    }
                }
            }
            Ok(text)
        })
        .await
        .unwrap_or(Err("the judge timed out"))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn looks_up(kind: &str, name: &str) -> LooksUp {
        LooksUp {
            kind: kind.into(),
            name: name.into(),
        }
    }

    /// A search that finds other coins, none called zorbo: what the judge
    /// reads to say `not_found`.
    pub(crate) fn other_coins() -> Value {
        json!({"results": [
            {"title": "Bitcoin (BTC) price", "text": "Bitcoin is the first cryptocurrency."},
            {"title": "Zcash (ZEC)", "text": "Zcash is a privacy coin."},
            {"title": "Zora (ZORA)", "text": "Zora is a coin of the Zora network."}
        ]})
    }

    /// #7840 HOLD: a search with no hits, `{"results": []}` or a 2xx with
    /// no `results`, is unknown ("no results") and the judge is never
    /// asked, so a small real thing a search misses is made as before.
    #[test]
    fn an_empty_search_is_unknown_and_never_judged() {
        let judged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        set_judge_here(Some(Arc::new({
            let judged = Arc::clone(&judged);
            move |_: &str| {
                judged.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some("not_found".to_owned())
            }
        })));
        for empty in [json!({"results": []}), json!({}), json!({"results": null})] {
            set_search_here(Some(Arc::new(move |_: &str| empty.clone())));
            let chat = Arc::new(|| -> Result<Chat, String> { Err("no chat".into()) });
            assert_eq!(found("place", "phở 24", chat), Found::Unknown("no results"));
        }
        assert_eq!(judged.load(std::sync::atomic::Ordering::SeqCst), 0);
        set_search_here(None);
        set_judge_here(None);
    }

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on_with(Some("on")));
        for value in [None, Some(""), Some("1"), Some("ON"), Some("yes")] {
            assert!(!on_with(value), "{value:?}");
        }
    }

    #[test]
    fn a_looks_up_line_is_one_of_four_kinds_and_a_plain_name() {
        assert_eq!(
            LooksUp::read(" Coin | “zorbo” "),
            Some(looks_up("coin", "zorbo"))
        );
        assert_eq!(
            LooksUp::read("place|Ho Chi  Minh City"),
            Some(looks_up("place", "Ho Chi Minh City"))
        );
        for rest in ["coin", "fruit | apple", "coin | ", "coin | \"\"", ""] {
            assert_eq!(LooksUp::read(rest), None, "{rest}");
        }
        assert_eq!(LooksUp::read(&format!("site | {}", "a".repeat(61))), None);
        assert_eq!(looks_up("coin", "zorbo").query(), "zorbo coin");
    }

    /// #7680's proposal fix: a name the model wrote with its kind already
    /// on the end ("zorbo coin", kind coin) is searched as it is, never
    /// "zorbo coin coin", and the judge is told that same query.
    #[test]
    fn a_name_ending_with_its_kind_is_not_given_it_twice() {
        assert_eq!(looks_up("coin", "zorbo coin").query(), "zorbo coin");
        assert_eq!(looks_up("coin", "Zorbo Coin").query(), "Zorbo Coin");
        assert_eq!(looks_up("place", "coin place").query(), "coin place");
        assert_eq!(looks_up("coin", "coinbase").query(), "coinbase coin");
        let searched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let asked = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        set_search_here(Some(Arc::new({
            let searched = Arc::clone(&searched);
            move |query: &str| {
                searched.lock().unwrap().push(query.to_owned());
                other_coins()
            }
        })));
        set_judge_here(Some(Arc::new({
            let asked = Arc::clone(&asked);
            move |question: &str| {
                asked.lock().unwrap().push(question.to_owned());
                Some("not_found".to_owned())
            }
        })));
        let chat = Arc::new(|| -> Result<Chat, String> { Err("no chat".into()) });
        assert_eq!(found("coin", "zorbo coin", chat), Found::NotFound);
        assert_eq!(*searched.lock().unwrap(), ["zorbo coin"]);
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert!(
            asked[0].contains("search for \"zorbo coin\" gave"),
            "{}",
            asked[0]
        );
        assert!(!asked[0].contains("coin coin"), "{}", asked[0]);
        set_search_here(None);
        set_judge_here(None);
    }

    #[test]
    fn the_judges_word_is_read_and_anything_else_is_unsure() {
        assert_eq!(verdict("not_found"), Found::NotFound);
        assert_eq!(verdict("Not found."), Found::NotFound);
        assert_eq!(verdict("FOUND"), Found::Found);
        assert_eq!(verdict("unsure"), Found::Unknown("unsure"));
        assert_eq!(verdict("maybe"), Found::Unknown("unsure"));
        assert_eq!(verdict(""), Found::Unknown("unsure"));
    }

    /// The judge over the make's own chat: one short call, its answer read.
    #[test]
    fn the_judge_asks_the_makes_own_chat_once() {
        use crate::make_skill_local::tests::{chat_at, service_sent};
        let (url, served) = service_sent(vec![(200, "not_found".to_owned())]);
        let answer = json!({"results": [{"title": "Zorbo", "text": "a toy"}]});
        let judged = judged(
            Arc::new(chat_at(url)),
            question(&looks_up("coin", "zorbo"), &answer),
            JUDGE_WITHIN,
        );
        let sent = served.join().unwrap();
        assert_eq!(judged, Found::NotFound);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("zorbo coin"), "{}", sent[0]);
        assert!(
            sent[0].contains("found, not_found or unsure"),
            "{}",
            sent[0]
        );
    }

    /// No search set up (no `LAMP_SECOND_SEARCH`): unknown, made as before.
    #[test]
    fn with_no_search_it_is_unknown() {
        set_search_here(None);
        let chat = Arc::new(|| -> Result<Chat, String> { Err("no chat".into()) });
        assert_eq!(found("coin", "zorbo", chat), Found::Unknown("no search"));
    }

    /// A search's error or a search past its bound is unknown; the bound
    /// is kept from the search's own start.
    #[test]
    fn a_failed_or_late_search_is_unknown_within_its_bound() {
        let chat = Arc::new(|| -> Result<Chat, String> { panic!("no judge without results") });
        set_search_here(Some(Arc::new(
            |_: &str| json!({"error": "the search failed: 500"}),
        )));
        assert_eq!(
            found("coin", "zorbo", Arc::clone(&chat)),
            Found::Unknown("the search failed")
        );
        set_within_here(Some(Duration::from_millis(100)));
        set_search_here(Some(Arc::new(|_: &str| {
            std::thread::sleep(Duration::from_millis(400));
            json!({"results": []})
        })));
        let began = Instant::now();
        assert_eq!(
            found("coin", "zorbo", chat),
            Found::Unknown("the search timed out")
        );
        assert!(
            began.elapsed() < Duration::from_millis(350),
            "{:?}",
            began.elapsed()
        );
        set_within_here(None);
        set_search_here(None);
    }
}
