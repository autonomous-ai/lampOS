//! The person's yes to a made skill that sends or writes (#6066, the
//! owner's lift of rule (b), 10 Nov; `LAMP_SKILL_WRITES`).
//!
//! A made skill with a write is put on as any made skill is, and its
//! say-back ends on [`crate::made_skill::WAITS_FOR_YES`]: nothing is sent
//! until the person agrees. Its writes then wait here ([`wait_for_yes`]),
//! in memory, one skill at a time, for [`YES_WITHIN`], with an id the
//! voice's model is given (`approve_id`), never the writes themselves.
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
    /// plain words ([`crate::made_skill::write_said`]).
    pub say_back: String,
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
    let id = format!(
        "yes-{:08x}",
        rand_u32(&asks.skill, request, now) // not a secret: only matched here
    );
    let count = asks.writes.len();
    kept::waiting(|waiting| {
        *waiting = Some(Waiting {
            id: id.clone(),
            asks,
            request,
            at: now,
            echoes: Vec::new(),
            said: VecDeque::new(),
        });
    });
    eprintln!(
        "lamp-voice: request {request}: a made skill's {count} write(s) wait for the person's yes (#6066)"
    );
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

/// What the model is told when an approval is not taken.
pub const NOT_APPROVED: &str = "Not approved: nothing waits for a yes now, or that was not the person's own answer. Say nothing is sent, and that they can say yes in the app.";
/// What the model is told when the yes came before the say-back was
/// heard whole (it waits on).
pub const NOT_SAID_WHOLE: &str =
    "Not approved yet: what it sends was not said in full, or it was cut. Nothing is sent.";
/// What the model is told when the yes came on the Live route.
pub const NOT_HERE: &str = "Not approved: on this voice I can't take that yes. Say nothing is sent yet, and that they can say yes in the app.";
/// What the model is told once approved.
pub const APPROVED_NEXT: &str =
    "Say only, in the conversation's language, that it will now send as it said.";

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
        NotSaid(String, String),
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
            return Took::NotSaid(waiting.id.clone(), waiting.asks.say_back.clone());
        }
        kept.take()
            .map_or(Took::Nothing, |waiting| Took::Waiting(Box::new(waiting)))
    });
    let waiting = match took {
        Took::Nothing => {
            eprintln!("lamp-voice: request {request}: a made skill's yes found nothing waiting");
            return json!({ "error": NOT_APPROVED });
        }
        Took::NotSaid(id, say_back) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill's yes not taken: its say-back was not heard whole"
            );
            return json!({
                "error": NOT_SAID_WHOLE,
                "next": format!("{} {}", say_whole(&say_back), then_wait(&id)),
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
}
