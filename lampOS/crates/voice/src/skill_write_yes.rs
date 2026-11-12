//! The person's yes to a made skill that sends or writes (#6066, the
//! owner's lift of rule (b), 10 Nov; `LAMP_SKILL_WRITES`).
//!
//! A made skill with a write is put on as any made skill is, and its
//! say-back ends on [`crate::made_skill::WAITS_FOR_YES`]: nothing is sent
//! until the person agrees. Its writes then wait here ([`wait_for_yes`]),
//! in memory, one skill at a time, for [`YES_WITHIN`], with an id the
//! voice's model is given (`approve_id`), never the writes themselves.
//!
//! A made skill with code of its own (#6307, `LAMP_MADE_SCRIPTS`) waits
//! here the same way, with every guard below, but is not on until then:
//! its say-back ends on [`crate::made_skill::RUNS_AFTER_YES`], and the yes
//! puts on the very package said back, by its SHA-256
//! ([`crate::made_skill::put_on_agreed`]), so a changed script asks again.
//!
//! - **The yes** is the model's to judge, in any words, as #6009 has a
//!   waiting mail's send judged: on an agreement it calls `make_skill` again
//!   with only that id. The voice itself then calls `lamp-services`'
//!   `skill_write_approve` ([`crate::services::skill_write_approve`]) with
//!   the skill's name, its writes' exact contents and the SHA-256 of the
//!   package said back, after the skill is on: `lamp-services` approves
//!   nothing when another package of its name is installed by then (App
//!   Only's P2 on #6142). A decline, a wait or a change calls nothing:
//!   nothing is approved, and a change is a new make.
//! - **Said whole first** (App Only's P1 on #6142): the lamp's own words in
//!   one request from the make's on, as played and not cut ([`lamp_said`],
//!   [`lamp_cut`], told by the thread as a waiting write's read-back is),
//!   must hold every word of each write's say-back
//!   ([`crate::made_skill::write_said`]) in order
//!   ([`crate::services::read_back_said`]). A yes before that is not taken,
//!   and the model is told to say it again whole ([`NOT_SAID_WHOLE`]); the
//!   wait goes on.
//! - **Only a later heard line** of the second route, the one with the echo
//!   judge ([`crate::services::sends_here`]): never the make's own request,
//!   a turn nobody spoke, or the Live route.
//! - **The echo hold.** While a yes is awaited, a line made only of the
//!   lamp's own words, begun over its voice or within the echo tail
//!   (`second_provider::own_echo::yes_echo`), is held even in the private
//!   room, so the say-back's own last words heard back never reach the
//!   model; such a line's request is also kept here ([`echo_held`]), and an
//!   approval in it is refused.
//! - **No identity check** (the owner, 10 Nov): whoever is at the lamp is
//!   its owner (`LAMP_PRIVATE_ROOM`).
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long a made skill's writes wait for the yes: as a prepared write
/// waits for its send (`crate::services::PENDING_FOR`).
pub const YES_WITHIN: Duration = crate::services::PENDING_FOR;
/// The `make_skill` argument that carries the yes.
pub const APPROVE_ID: &str = "approve_id";
/// How many of the lamp's answers a wait keeps, each at most
/// [`SAID_MOST`] bytes.
const SAID_KEPT: usize = 6;
const SAID_MOST: usize = 6_000;

/// What a made skill's yes would approve.
#[derive(Clone, Debug, PartialEq)]
pub struct Asks {
    pub skill: String,
    /// The package made, put on and said back.
    pub sha256: String,
    pub writes: Vec<Value>,
    /// What the lamp must have said whole before the yes: each write's
    /// plain words ([`crate::made_skill::write_said`]); for code of its
    /// own, what its code may use ([`crate::made_skill::code_said`]).
    pub say_back: String,
    /// For a skill's `do.task` (#6305, [`crate::skill_task_yes`]): the
    /// grants said back, which the yes covers with `sha256`, then the
    /// skill's folder's; `writes` is empty. None for a made skill's writes.
    pub grants: Option<Vec<String>>,
    /// It runs code of its own (#6307, `LAMP_MADE_SCRIPTS`), with no
    /// write: the yes puts that package on by its SHA-256
    /// ([`crate::made_skill::put_on_agreed`]), then keeps its task for the
    /// folder as it went on and the `grants` said
    /// ([`crate::make_skill_local::code_approve`]); never approves a send.
    pub code: bool,
}

/// A made skill's writes waiting for the yes.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub id: String,
    pub asks: Asks,
    /// The request the skill was made in: its yes comes in a later one.
    pub request: u64,
    pub at: Instant,
    /// Later requests whose line was held as the lamp's own echo.
    pub echoes: Vec<u64>,
    /// The lamp's words in each request from the make's on, and whether
    /// that answer was cut.
    pub said: VecDeque<(u64, String, bool)>,
    /// A wait the voice's model began with no call of its own (an app's
    /// make with code, #6307, [`wait_for_yes_noted`]). Every wait's id is
    /// told in [`waiting_note`] alike, and every wait's yes is the owner's
    /// only (`make_skill_local::not_owners_yes_refused`).
    pub noted: bool,
}

#[cfg(not(test))]
mod kept {
    use super::Waiting;
    use std::sync::Mutex;
    static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        f(&mut WAITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod kept {
    use super::Waiting;
    use std::cell::RefCell;
    thread_local! {
        static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
    }
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        WAITING.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// The skill `asks` names, made and put on in `request`, waits for the
/// yes to its writes: any other waiting is dropped (one at a time). Its
/// id, for the model.
pub fn wait_for_yes(asks: Asks, request: u64, now: Instant) -> String {
    wait_for_yes_with(asks, request, now, false)
}

/// [`wait_for_yes`] for a say-back the lamp was given to say, with no call
/// of the model's that it answers (an app's make with code of its own,
/// #6307, `make_skill_local::app_code_asked`): its id is told in a note to
/// each later heard line while it waits ([`waiting_note`]), as the history
/// keeps no call's answer.
pub fn wait_for_yes_noted(asks: Asks, request: u64, now: Instant) -> String {
    wait_for_yes_with(asks, request, now, true)
}

fn wait_for_yes_with(asks: Asks, request: u64, now: Instant, noted: bool) -> String {
    let id = format!(
        "yes-{:08x}",
        rand_u32(&asks.skill, request, now) // not a secret: only matched here
    );
    let count = asks.writes.len();
    let code = asks.code;
    kept::waiting(|waiting| {
        *waiting = Some(Waiting {
            id: id.clone(),
            asks,
            request,
            at: now,
            echoes: Vec::new(),
            said: VecDeque::new(),
            noted,
        });
    });
    if code {
        eprintln!(
            "lamp-voice: request {request}: a made skill with code of its own waits for the person's yes (#6307)"
        );
    } else {
        eprintln!(
            "lamp-voice: request {request}: a made skill's {count} write(s) wait for the person's yes (#6066)"
        );
    }
    id
}

/// A few bits that differ between makes, for an id the model can't guess
/// from another.
fn rand_u32(skill: &str, request: u64, now: Instant) -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write(skill.as_bytes());
    hasher.write_u64(request);
    hasher.write_u128(now.elapsed().as_nanos());
    hasher.finish() as u32
}

/// The note a later heard line `request` is told at `now` of the wait:
/// the skill said back waits for the person's yes, and how to take it
/// ([`then_wait`]'s words). Every wait, as the second route's history keeps
/// no call's answer, so the id a make or a read-back told is gone by the
/// next line (#6223's row 4 on 9adf, 12 Nov: "a made skill's yes found
/// nothing waiting" three times). None in the wait's own request, none
/// waiting, or past [`YES_WITHIN`]; the yes keeps every guard of
/// [`approve`].
pub fn waiting_note(request: u64, now: Instant) -> Option<String> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_ref().filter(|waiting| {
            request > waiting.request && now.saturating_duration_since(waiting.at) <= YES_WITHIN
        })?;
        let before = if waiting.asks.code {
            "before it runs any code"
        } else if waiting.asks.grants.is_some() {
            "before its task runs"
        } else {
            "before it sends anything"
        };
        Some(format!(
            "(For you to know, not to answer: the skill {} that the lamp just said back waits \
             for their yes {before}, approve_id \"{}\". If they agree, in any words, call \
             make_skill with only that approve_id. If they decline, wait or change it, don't \
             call it: it stays off.)",
            crate::made_skill::spoken_name(&waiting.asks.skill),
            waiting.id
        ))
    })
}

/// Whether the wait `id` is the one waiting: a voice make's, a read-back's,
/// a task's or an app make's code alike.
pub fn waits(id: &str) -> bool {
    kept::waiting(|waiting| waiting.as_ref().is_some_and(|waiting| waiting.id == id))
}

/// The wait `id` dropped: its yes was not the owner's (Heron's P1 on
/// #6558, as #6307's for the app's code make). Whether one was.
pub fn drop_wait(id: &str) -> bool {
    kept::waiting(|waiting| {
        let waits = waiting.as_ref().is_some_and(|waiting| waiting.id == id);
        if waits {
            *waiting = None;
        }
        waits
    })
}

/// Whether a made skill's writes wait for the yes at `now`: the echo hold
/// is on ([`crate::second_provider`]).
pub fn awaited(now: Instant) -> bool {
    kept::waiting(|waiting| {
        waiting
            .as_ref()
            .is_some_and(|waiting| now.saturating_duration_since(waiting.at) <= YES_WITHIN)
    })
}

/// The line of `request` was held as the lamp's own echo while the yes was
/// awaited: no approval in it.
pub fn echo_held(request: u64) {
    kept::waiting(|waiting| {
        if let Some(waiting) = waiting.as_mut()
            && request > waiting.request
            && !waiting.echoes.contains(&request)
        {
            waiting.echoes.push(request);
        }
    });
}

/// The lamp said `words` in `request` (`thread::Thread::lamp_for`, as
/// played): kept toward the say-back while a yes waits, for the make's
/// request and any after it.
pub fn lamp_said(request: u64, words: &str) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, said, _)) => {
                if said.len() + words.len() <= SAID_MOST {
                    said.push_str(words);
                }
            }
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                let words: String = words.chars().take(SAID_MOST).collect();
                waiting.said.push_back((request, words, false));
            }
        }
    });
}

/// The lamp's answer in `request` was cut (`thread::Thread::heard_only`):
/// a say-back cut is no say-back.
pub fn lamp_cut(request: u64) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, _, cut)) => *cut = true,
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                waiting.said.push_back((request, String::new(), true));
            }
        }
    });
}

/// Whether one of the lamp's answers before `request`, uncut, held the
/// whole say-back.
fn said_whole_before(waiting: &Waiting, request: u64) -> bool {
    waiting.said.iter().any(|(id, said, cut)| {
        *id < request && !cut && crate::services::read_back_said(&waiting.asks.say_back, said)
    })
}

/// Where `said` first falls short of `say_back`, as
/// [`crate::services::read_back_said`] checks it: the 1-based place of the
/// first say-back word it does not hold in order, and the say-back's word
/// count; none when it holds all. Places only, never a word: the say-back
/// is the write's own content, the person's.
fn first_missing(say_back: &str, said: &str) -> Option<(usize, usize)> {
    let mut said = crate::services::check_words(said).into_iter();
    let wanted = crate::services::check_words(say_back);
    wanted
        .iter()
        .position(|word| !said.any(|got| &got == word))
        .map(|at| (at + 1, wanted.len()))
}

/// The journal's line of a yes in `request` not taken because the
/// say-back was not heard whole (#6715): each of the lamp's answers kept
/// toward it, by request, cut or not and its word count, and the first
/// say-back word missing from the best of them (the one before `request`
/// that holds the most of it in order, an uncut one on a tie). Counts and
/// places only, never a word of the say-back nor of what the lamp said: a
/// say-back is the person's content, and the journal holds counts and
/// outcomes only (`LAMP_SKILL_WRITES`).
fn not_said_whole_line(waiting: &Waiting, request: u64) -> String {
    let kept: Vec<String> = waiting
        .said
        .iter()
        .map(|(id, said, cut)| {
            format!(
                "r{id} cut={cut} {} words",
                crate::services::check_words(said).len()
            )
        })
        .collect();
    let held = |said: &str| {
        let wanted = crate::services::check_words(&waiting.asks.say_back);
        let mut words = crate::services::check_words(said).into_iter();
        wanted
            .iter()
            .take_while(|word| words.any(|got| &got == *word))
            .count()
    };
    let best = waiting
        .said
        .iter()
        .filter(|(id, ..)| *id < request)
        .max_by_key(|(_, said, cut)| (held(said), !cut));
    let missing = match best {
        None => "no answer kept".to_owned(),
        Some((id, said, cut)) => match first_missing(&waiting.asks.say_back, said) {
            Some((at, of)) => format!("first missing word in r{id}: {at} of {of}"),
            None if *cut => format!("r{id} held it all but was cut"),
            None => format!("r{id} held it all"),
        },
    };
    let kept = if kept.is_empty() {
        "none".to_owned()
    } else {
        kept.join("; ")
    };
    format!(
        "lamp-voice: request {request}: a made skill's yes not taken: its say-back was not heard whole (kept: {kept}; {missing})"
    )
}

/// The journal's line when the lamp's words of `request` skip the thread
/// (`second_provider::worker`, `guard` naming why) while a skill's yes
/// waits (#6715): they never count toward its say-back. None while
/// nothing waits.
pub fn skipped_line(request: u64, guard: &str) -> Option<String> {
    kept::waiting(|waiting| {
        waiting.as_ref().map(|waiting| {
            format!(
                "lamp-voice: request {request}: the lamp's words skip the thread ({guard}) while a skill's yes waits (made in request {}): they don't count toward its say-back",
                waiting.request
            )
        })
    })
}

/// What the model is told to say of a made skill whose writes wait: `line`
/// word for word, every part of it.
pub fn say_whole(line: &str) -> String {
    format!(
        "Say this word for word as written, leaving nothing out: every address, every quoted \
         word, when and how often: \"{line}\" If the conversation is in another language, say \
         it in that language too, after it."
    )
}

/// What the model is told after the say-back: ask nothing; on an
/// agreement, the call with `id`; on anything else, no call.
pub fn then_wait(id: &str) -> String {
    format!(
        "End as the line ends, asking nothing. Nothing is sent until they agree. If their next \
         words agree, in any words, call make_skill again with only approve_id \"{id}\". If they \
         decline, wait or change it, don't call it: nothing is sent, and a change is a new \
         make_skill."
    )
}

/// What the model is told when an approval is not taken: the way to a
/// yes is the read-back (`make_skill_local::read_back`), never the app,
/// which has none (Thirteen's HOLD on #6223: lamp-cloud has no kind for
/// `skill_write_approve`).
pub const NOT_APPROVED: &str = "Not approved: nothing waits for a yes now, or that was not the person's own answer. Say nothing is sent, and that they can ask me what that skill sends (\"what does my … skill send?\") to hear it again and say yes then.";
/// What the model is told when the yes came before the say-back was
/// heard whole (it waits on).
pub const NOT_SAID_WHOLE: &str =
    "Not approved yet: what it sends was not said in full, or it was cut. Nothing is sent.";
/// What the model is told when the yes came on the Live route: nothing
/// sent, and the yes taken on the lamp's other voice, never here, where
/// asking again ends at `make_skill_local::yes_not_here` (App Only's P3 on
/// #6250).
pub const NOT_HERE: &str = "Not approved: this live voice can't take that yes. Say plainly that nothing was sent, and that I take that yes only on my other voice: in a normal conversation with me, not this live one.";
/// What the model is told once approved.
pub const APPROVED_NEXT: &str =
    "Say only, in the conversation's language, that it will now send as it said.";
/// What the model is told once a skill with code of its own is agreed to
/// and on (#6307).
pub const CODE_APPROVED_NEXT: &str =
    "Say only, in the conversation's language, that it's on now and runs as it said.";

/// What the model is told once a skill with code of its own is on but its
/// task wasn't kept (#6307): a skill's tasks off, or its folder's grants
/// not those said back, so its code runs nowhere.
pub const CODE_ON_TASK_WAITS_NEXT: &str = "Say only, in the conversation's language, that it's \
     on now, but this lamp doesn't run its code yet.";

/// Why a yes is never taken, or `None`.
fn refused(waiting: &Waiting, request: u64, heard: bool, now: Instant) -> Option<&'static str> {
    if !heard {
        Some("no one spoke it")
    } else if request <= waiting.request {
        Some("it came in the make's own request")
    } else if waiting.echoes.contains(&request) {
        Some("its line was the lamp's own words heard back")
    } else if now.saturating_duration_since(waiting.at) > YES_WITHIN {
        Some("it came too late")
    } else {
        None
    }
}

/// `make_skill` with `approve_id` `id` in `request` (`heard`: a line
/// someone spoke), at `now`: the waiting skill's writes approved through
/// `approve` (`skill_write_approve`, with its name, its package's SHA-256
/// and its writes) when it is the very one waiting, in a later heard
/// request of the route with the echo judge (`sends_here`), not a line
/// held as the lamp's echo, within [`YES_WITHIN`], once the lamp's words
/// held its whole say-back uncut. A yes before that is told to say it
/// again, and the wait goes on; any other try ends it. What the model is
/// told.
pub fn approve(
    id: &str,
    request: u64,
    heard: bool,
    sends_here: bool,
    now: Instant,
    approve: impl FnOnce(&Asks) -> Result<Value, String>,
) -> Value {
    enum Took {
        Nothing,
        NotSaid(String, String, bool, String),
        Waiting(Box<Waiting>),
    }
    let took = kept::waiting(|kept| {
        let Some(waiting) = kept.as_ref().filter(|waiting| waiting.id == id) else {
            return Took::Nothing;
        };
        if refused(waiting, request, heard, now).is_none()
            && sends_here
            && !said_whole_before(waiting, request)
        {
            return Took::NotSaid(
                waiting.id.clone(),
                waiting.asks.say_back.clone(),
                waiting.asks.grants.is_some(),
                not_said_whole_line(waiting, request),
            );
        }
        kept.take()
            .map_or(Took::Nothing, |waiting| Took::Waiting(Box::new(waiting)))
    });
    let waiting = match took {
        Took::Nothing => {
            eprintln!("lamp-voice: request {request}: a made skill's yes found nothing waiting");
            return json!({ "error": NOT_APPROVED });
        }
        Took::NotSaid(id, say_back, task, line) => {
            eprintln!("{line}");
            let wait = if task {
                crate::skill_task_yes::then_wait(&id)
            } else {
                then_wait(&id)
            };
            return json!({
                "error": NOT_SAID_WHOLE,
                "next": format!("{} {wait}", say_whole(&say_back)),
            });
        }
        Took::Waiting(waiting) => waiting,
    };
    if let Some(why) = refused(&waiting, request, heard, now) {
        eprintln!("lamp-voice: request {request}: a made skill's yes not taken: {why}");
        return json!({ "error": NOT_APPROVED });
    }
    if !sends_here {
        eprintln!(
            "lamp-voice: request {request}: a made skill's yes not taken: this route has no echo judge"
        );
        return json!({ "error": NOT_HERE });
    }
    match approve(&waiting.asks) {
        // Code of its own (#6307): that package put on, nothing sent, and
        // its task kept when the lamp hands tasks over (#6305).
        Ok(result) if waiting.asks.code => {
            let task = result["task"] == true;
            eprintln!(
                "lamp-voice: request {request}: a made skill with code of its own is on, on the person's yes (#6307); its task {}",
                if task { "kept" } else { "not kept" }
            );
            let next = if task {
                CODE_APPROVED_NEXT
            } else {
                CODE_ON_TASK_WAITS_NEXT
            };
            json!({ "approved": true, "on": true, "task": task, "next": next })
        }
        // A skill's task (#6305): its grants and folder kept, as said.
        Ok(_) if waiting.asks.grants.is_some() => {
            eprintln!(
                "lamp-voice: request {request}: a skill's task approved on the person's yes (#6305)"
            );
            json!({ "approved": true, "next": crate::skill_task_yes::APPROVED_NEXT })
        }
        Ok(result) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill's {} write(s) approved on the person's yes (#6066)",
                waiting.asks.writes.len()
            );
            let public: Vec<&str> = result["public"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let next = match public.as_slice() {
                [] => APPROVED_NEXT.to_owned(),
                repos => format!(
                    "{APPROVED_NEXT} Say too that anyone can see what it posts in {}.",
                    repos.join(" and ")
                ),
            };
            json!({ "approved": true, "next": next })
        }
        Err(why) => {
            // `lamp-services`' words name no recipient or text.
            eprintln!("lamp-voice: request {request}: a made skill's yes not kept: {why}");
            json!({
                "error": format!("Not approved: {why}."),
                "next": "Say nothing is sent, and why, in a few words."
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> Value {
        json!({"service": "gmail", "action": "send_mail", "to": ["mai@example.com"],
               "subject": "Leaving", "text": "I'm leaving now."})
    }

    const SAY_BACK: &str =
        "mails mai@example.com from your Gmail, subject “Leaving”, saying “I'm leaving now.”";

    fn asks(skill: &str, writes: &[Value]) -> Asks {
        Asks {
            skill: skill.to_owned(),
            sha256: "ab".repeat(32),
            writes: writes.to_vec(),
            say_back: SAY_BACK.to_owned(),
            grants: None,
            code: false,
        }
    }

    /// A wait whose say-back the lamp said whole in the make's `request`.
    fn said_back(skill: &str, writes: &[Value], request: u64, now: Instant) -> String {
        let id = wait_for_yes(asks(skill, writes), request, now);
        lamp_said(request, "I made leaving mail: when you get up, it ");
        lamp_said(request, &format!("{SAY_BACK}, at most once an hour."));
        id
    }

    fn never(_: &Asks) -> Result<Value, String> {
        panic!("approved")
    }

    /// A yes in a later heard request approves exactly the skill's writes
    /// and the package said back, once; "anyone can see it" only for a
    /// public repository.
    #[test]
    fn a_later_heard_yes_approves_the_waiting_writes_once() {
        let now = Instant::now();
        let id = said_back("leaving-mail", &[mail()], 4, now);
        assert!(awaited(now));
        let mut asked = None;
        let told = approve(&id, 5, true, true, now, |asks| {
            asked = Some(asks.clone());
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["next"], APPROVED_NEXT);
        assert_eq!(asked, Some(asks("leaving-mail", &[mail()])));
        assert!(!awaited(now));
        // Once only.
        let again = approve(&id, 6, true, true, now, never);
        assert_eq!(again["error"], NOT_APPROVED);
        // A public repository is said.
        let id = said_back("standup", &[json!({"service": "github"})], 7, now);
        let told = approve(&id, 8, true, true, now, |_| {
            Ok(json!({"approved": 1, "public": ["acme/site"]}))
        });
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("anyone can see what it posts in acme/site")
        );
    }

    /// #6223's row 4 on 9adf (12 Nov): a voice make's wait, whose id only
    /// the make's call answer told, is noted on each later heard line, so
    /// the next line's yes can name it; a task's and code's say which.
    #[test]
    fn every_wait_is_noted_on_the_lines_after_it() {
        let now = Instant::now();
        let id = said_back("leaving-mail", &[mail()], 4, now);
        assert!(waiting_note(4, now).is_none());
        let next = waiting_note(5, now).unwrap();
        assert!(next.contains(&format!("approve_id \"{id}\"")), "{next}");
        assert!(next.contains("before it sends anything"), "{next}");
        let told = approve(&id, 5, true, true, now, |_| {
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert!(waiting_note(6, now).is_none());
        // Past YES_WITHIN: no note.
        let id = said_back("leaving-mail", &[mail()], 7, now);
        assert!(waiting_note(8, now + YES_WITHIN + Duration::from_secs(1)).is_none());
        assert!(waiting_note(8, now).unwrap().contains(&id));
        let task = Asks {
            grants: Some(vec!["camera".to_owned()]),
            writes: Vec::new(),
            ..asks("desk-watch", &[])
        };
        wait_for_yes(task, 9, now);
        assert!(
            waiting_note(10, now)
                .unwrap()
                .contains("before its task runs")
        );
        let code = Asks {
            code: true,
            writes: Vec::new(),
            ..asks("desk-watch", &[])
        };
        wait_for_yes(code, 11, now);
        assert!(
            waiting_note(12, now)
                .unwrap()
                .contains("before it runs any code")
        );
    }

    /// App Only's P1 on #6142: a say-back cut, summed up or missing a
    /// recipient approves nothing; the model is told to say it whole again,
    /// the wait goes on, and a yes after it is said whole approves.
    #[test]
    fn a_cut_or_summarised_say_back_does_not_approve() {
        let now = Instant::now();
        for said in [
            None,
            Some("I made leaving mail: it mails Mai when you leave."),
            Some("it mails from your Gmail, subject “Leaving”, saying “I'm leaving now.”"),
            Some("it mails mai@example.com from your Gmail, subject “Leaving”"),
        ] {
            let id = wait_for_yes(asks("leaving-mail", &[mail()]), 4, now);
            if let Some(said) = said {
                lamp_said(4, said);
            }
            let told = approve(&id, 5, true, true, now, never);
            assert_eq!(told["error"], NOT_SAID_WHOLE, "{said:?}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains(SAY_BACK) && next.contains(&id), "{next}");
            assert!(awaited(now), "the wait goes on");
        }
        // Said whole but cut (a line or a Stop over it): not taken either.
        let id = said_back("leaving-mail", &[mail()], 4, now);
        lamp_cut(4);
        assert_eq!(
            approve(&id, 5, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
        // Said again whole in that answer, a later yes approves.
        lamp_said(5, SAY_BACK);
        let told = approve(&id, 6, true, true, now, |_| Ok(json!({"approved": 1})));
        assert_eq!(told["approved"], true, "{told}");
        // Said whole only in the yes's own answer: not before it.
        let id = wait_for_yes(asks("leaving-mail", &[mail()]), 7, now);
        lamp_said(8, SAY_BACK);
        assert_eq!(
            approve(&id, 8, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
    }

    /// #6715: a yes refused for a say-back not heard whole journals each
    /// answer kept toward it (request, cut, word count) and the first
    /// say-back word missing from the best one, and no more of the
    /// say-back; the words skipping the thread are journalled only while a
    /// yes waits.
    #[test]
    fn a_refused_yes_journals_the_answers_kept_and_the_first_word_missing() {
        let now = Instant::now();
        assert_eq!(
            skipped_line(3, "the lease's request"),
            None,
            "nothing waits"
        );
        let id = wait_for_yes(asks("leaving-mail", &[mail()]), 1, now);
        let waiting = || kept::waiting(|waiting| waiting.clone().unwrap());
        assert_eq!(
            not_said_whole_line(&waiting(), 2),
            "lamp-voice: request 2: a made skill's yes not taken: its say-back was not heard whole (kept: none; no answer kept)"
        );
        lamp_said(
            1,
            "it mails mai@example.com from your Gmail, subject “Leaving”",
        );
        lamp_said(2, "I need to read it back in full first.");
        lamp_cut(2);
        let line = not_said_whole_line(&waiting(), 3);
        assert_eq!(
            line,
            "lamp-voice: request 3: a made skill's yes not taken: its say-back was not heard whole (kept: r1 cut=false 10 words; r2 cut=true 9 words; first missing word in r1: 10 of 14)"
        );
        // No word of the say-back reaches the journal: places and counts only.
        let line_words = crate::services::check_words(&line);
        for word in crate::services::check_words(SAY_BACK) {
            if word.len() > 2 {
                assert!(!line_words.contains(&word), "{word} in {line}");
            }
        }
        // Held whole but cut: said so.
        lamp_said(3, SAY_BACK);
        lamp_cut(3);
        assert!(
            not_said_whole_line(&waiting(), 4).ends_with("r3 held it all but was cut)"),
            "{}",
            not_said_whole_line(&waiting(), 4)
        );
        assert_eq!(
            approve(&id, 4, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
        assert_eq!(
            skipped_line(5, "no scope").as_deref(),
            Some(
                "lamp-voice: request 5: the lamp's words skip the thread (no scope) while a skill's yes waits (made in request 1): they don't count toward its say-back"
            )
        );
    }

    /// The say-back's own last words heard back within the tail are held
    /// (`own_echo::yes_echo`) and their request kept: an approval in it is
    /// refused, and so is one in the make's own request, a turn nobody
    /// spoke, another id, one too late, or one on the Live route.
    #[test]
    fn an_echo_or_an_unheard_or_stale_yes_approves_nothing() {
        let now = Instant::now();
        for (request, heard, sends, at, id_ok) in [
            (5, true, true, now, true),
            (3, true, true, now, true),
            (6, false, true, now, true),
            (6, true, true, now, false),
            (
                6,
                true,
                true,
                now + YES_WITHIN + Duration::from_secs(1),
                true,
            ),
        ] {
            let id = said_back("leaving-mail", &[mail()], 3, now);
            echo_held(5);
            let id = if id_ok { id } else { "yes-other".into() };
            let told = approve(&id, request, heard, sends, at, never);
            assert_eq!(told["error"], NOT_APPROVED, "{request} {heard}");
        }
        let id = said_back("leaving-mail", &[mail()], 3, now);
        assert_eq!(approve(&id, 4, true, false, now, never)["error"], NOT_HERE);
        // A refusal by lamp-services is told, nothing approved.
        let id = said_back("leaving-mail", &[mail()], 3, now);
        let told = approve(&id, 4, true, true, now, |_| {
            Err("the skill on the lamp is not the one said back: nothing was approved".into())
        });
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("not the one said back")
        );
    }

    /// A decline or a change calls nothing: the wait lapses, and a new make
    /// replaces it, its old id approving nothing.
    #[test]
    fn a_decline_or_a_change_approves_nothing() {
        let now = Instant::now();
        let old = said_back("leaving-mail", &[mail()], 3, now);
        assert!(!awaited(now + YES_WITHIN + Duration::from_secs(1)));
        let _changed = said_back("leaving-mail-2", &[mail()], 5, now);
        let told = approve(&old, 6, true, true, now, never);
        assert_eq!(told["error"], NOT_APPROVED);
    }

    /// Thirteen's HOLD on #6223 (App Only confirming): the app has no yes
    /// for a made skill's writes (lamp-cloud has no kind for
    /// `skill_write_approve`, the app nothing), so no line the voice's
    /// model is told of a skill's writes, here, in the made-skill lines or
    /// in the read-back's, names the app; and no line outside the tests
    /// of those files sends the person to the app for the yes.
    #[test]
    fn no_line_sends_the_person_to_the_app_for_a_yes() {
        let lines = [
            NOT_APPROVED.to_owned(),
            NOT_SAID_WHOLE.to_owned(),
            NOT_HERE.to_owned(),
            APPROVED_NEXT.to_owned(),
            then_wait("ab12"),
            say_whole("I made leaving mail."),
            crate::made_skill::WAITS_FOR_YES.to_owned(),
            crate::made_skill::APPROVED_ALREADY.to_owned(),
            crate::made_skill::CUT_WRITE_UNSAID.to_owned(),
            crate::make_skill_local::READ_BACK_TOLD.to_owned(),
            crate::make_skill_local::READ_BACK_NOT_ASKED.to_owned(),
            crate::make_skill_local::sends_unsaid("leaving mail"),
            crate::make_skill_local::yes_not_here("leaving mail"),
            crate::make_skill_local::changed_since_made("leaving mail"),
            crate::skill_task_yes::then_wait("ab12"),
            crate::skill_task_yes::APPROVED_NEXT.to_owned(),
            CODE_APPROVED_NEXT.to_owned(),
            CODE_ON_TASK_WAITS_NEXT.to_owned(),
            crate::skill_task_yes::yes_not_here("photo booth"),
            crate::skill_task_yes::unread("photo booth"),
            crate::make_skill_local::TASKS_READ_BACK_TOLD.to_owned(),
        ];
        for line in &lines {
            let words: Vec<String> = line
                .split(|c: char| !c.is_alphanumeric() && c != '\'')
                .map(str::to_lowercase)
                .collect();
            assert!(
                !words.iter().any(|word| word == "app" || word == "app's"),
                "{line}"
            );
        }
        // The way to a yes now: asking what the skill sends; on the Live
        // route, which can't take one, the lamp's other voice, never
        // asking again there (App Only's P3 on #6250).
        assert!(
            NOT_APPROVED.contains("what does my … skill send?"),
            "{NOT_APPROVED}"
        );
        assert!(!NOT_HERE.contains("what does my"), "{NOT_HERE}");
        for words in ["nothing was sent", "my other voice", "not this live one"] {
            assert!(NOT_HERE.contains(words), "{NOT_HERE}");
        }
        let app_yes = [
            "yes in the app",
            "app's yes",
            "yes from the app",
            "agree in the app",
            "approve in the app",
            "approve it in the app",
        ];
        for (file, source) in [
            ("skill_write_yes.rs", include_str!("skill_write_yes.rs")),
            ("made_skill.rs", include_str!("made_skill.rs")),
            ("make_skill_local.rs", include_str!("make_skill_local.rs")),
            ("skill_task_yes.rs", include_str!("skill_task_yes.rs")),
        ] {
            let code = source.split("mod tests {").next().unwrap();
            for (number, line) in code.lines().enumerate() {
                let line = line.to_lowercase();
                assert!(
                    !app_yes.iter().any(|said| line.contains(said)),
                    "{file}:{}: {line}",
                    number + 1
                );
            }
        }
    }
}
