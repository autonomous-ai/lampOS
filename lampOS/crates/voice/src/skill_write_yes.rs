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
//!   the skill's name and its writes' exact contents, after the skill is on
//!   (the approval binds its installed package). A decline, a wait or a
//!   change calls nothing: nothing is approved, and a change is a new make.
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
use std::time::{Duration, Instant};

/// How long a made skill's writes wait for the yes: as a prepared write
/// waits for its send (`crate::services::PENDING_FOR`).
pub const YES_WITHIN: Duration = crate::services::PENDING_FOR;
/// The `make_skill` argument that carries the yes.
pub const APPROVE_ID: &str = "approve_id";

/// A made skill's writes waiting for the yes.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub id: String,
    pub skill: String,
    pub writes: Vec<Value>,
    /// The request the skill was made in: its yes comes in a later one.
    pub request: u64,
    pub at: Instant,
    /// Later requests whose line was held as the lamp's own echo.
    pub echoes: Vec<u64>,
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

/// The skill `skill`, made and put on in `request`, waits for the yes to
/// its `writes`: any other waiting is dropped (one at a time). Its id, for
/// the model.
pub fn wait_for_yes(skill: &str, writes: &[Value], request: u64, now: Instant) -> String {
    let id = format!(
        "yes-{:08x}",
        rand_u32(skill, request, now) // not a secret: only matched here
    );
    kept::waiting(|waiting| {
        *waiting = Some(Waiting {
            id: id.clone(),
            skill: skill.to_owned(),
            writes: writes.to_vec(),
            request,
            at: now,
            echoes: Vec::new(),
        });
    });
    eprintln!(
        "lamp-voice: request {request}: a made skill's {} write(s) wait for the person's yes (#6066)",
        writes.len()
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

/// What the model is told when an approval is not taken.
pub const NOT_APPROVED: &str = "Not approved: nothing waits for a yes now, or that was not the person's own answer. Say nothing is sent, and that they can say yes in the app.";
/// What the model is told when the yes came on the Live route.
pub const NOT_HERE: &str = "Not approved: on this voice I can't take that yes. Say nothing is sent yet, and that they can say yes in the app.";
/// What the model is told once approved.
pub const APPROVED_NEXT: &str =
    "Say only, in the conversation's language, that it will now send as it said.";

/// `make_skill` with `approve_id` `id` in `request` (`heard`: a line
/// someone spoke), at `now`: the waiting skill's writes approved through
/// `approve` (`skill_write_approve`, with its name and writes) when it is
/// the very one waiting, in a later heard request of the route with the
/// echo judge (`sends_here`), not a line held as the lamp's echo, within
/// [`YES_WITHIN`]. Taken once: any try ends the wait. What the model is
/// told.
pub fn approve(
    id: &str,
    request: u64,
    heard: bool,
    sends_here: bool,
    now: Instant,
    approve: impl FnOnce(&str, &[Value]) -> Result<Value, String>,
) -> Value {
    let waiting = kept::waiting(|waiting| {
        let matches = waiting.as_ref().is_some_and(|waiting| waiting.id == id);
        if matches { waiting.take() } else { None }
    });
    let Some(waiting) = waiting else {
        eprintln!("lamp-voice: request {request}: a made skill's yes found nothing waiting");
        return json!({ "error": NOT_APPROVED });
    };
    let why = if !heard {
        Some("no one spoke it")
    } else if request <= waiting.request {
        Some("it came in the make's own request")
    } else if waiting.echoes.contains(&request) {
        Some("its line was the lamp's own words heard back")
    } else if now.saturating_duration_since(waiting.at) > YES_WITHIN {
        Some("it came too late")
    } else if !sends_here {
        eprintln!(
            "lamp-voice: request {request}: a made skill's yes not taken: this route has no echo judge"
        );
        return json!({ "error": NOT_HERE });
    } else {
        None
    };
    if let Some(why) = why {
        eprintln!("lamp-voice: request {request}: a made skill's yes not taken: {why}");
        return json!({ "error": NOT_APPROVED });
    }
    match approve(&waiting.skill, &waiting.writes) {
        Ok(result) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill's {} write(s) approved on the person's yes (#6066)",
                waiting.writes.len()
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

    /// A yes in a later heard request approves exactly the skill's writes,
    /// once; "anyone can see it" only for a public repository.
    #[test]
    fn a_later_heard_yes_approves_the_waiting_writes_once() {
        let now = Instant::now();
        let id = wait_for_yes("leaving-mail", &[mail()], 4, now);
        assert!(awaited(now));
        let mut asked = None;
        let told = approve(&id, 5, true, true, now, |skill, writes| {
            asked = Some((skill.to_owned(), writes.to_vec()));
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["next"], APPROVED_NEXT);
        assert_eq!(asked, Some(("leaving-mail".to_owned(), vec![mail()])));
        assert!(!awaited(now));
        // Once only.
        let again = approve(&id, 6, true, true, now, |_, _| panic!("approved twice"));
        assert_eq!(again["error"], NOT_APPROVED);
        // A public repository is said.
        let id = wait_for_yes("standup", &[json!({"service": "github"})], 7, now);
        let told = approve(&id, 8, true, true, now, |_, _| {
            Ok(json!({"approved": 1, "public": ["acme/site"]}))
        });
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("anyone can see what it posts in acme/site")
        );
    }

    /// The say-back's own last words heard back within the tail are held
    /// (`own_echo::yes_echo`) and their request kept: an approval in it is
    /// refused, and so is one in the make's own request, a turn nobody
    /// spoke, another id, one too late, or one on the Live route.
    #[test]
    fn an_echo_or_an_unheard_or_stale_yes_approves_nothing() {
        let now = Instant::now();
        let never = |_: &str, _: &[Value]| -> Result<Value, String> { panic!("approved") };
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
            let id = wait_for_yes("leaving-mail", &[mail()], 3, now);
            echo_held(5);
            let id = if id_ok { id } else { "yes-other".into() };
            let told = approve(&id, request, heard, sends, at, never);
            assert_eq!(told["error"], NOT_APPROVED, "{request} {heard}");
        }
        let id = wait_for_yes("leaving-mail", &[mail()], 3, now);
        assert_eq!(approve(&id, 4, true, false, now, never)["error"], NOT_HERE);
        // A refusal by lamp-services is told, nothing approved.
        let id = wait_for_yes("leaving-mail", &[mail()], 3, now);
        let told = approve(&id, 4, true, true, now, |_, _| {
            Err("the skill is not on the lamp as a package: nothing was approved".into())
        });
        assert!(told["error"].as_str().unwrap().contains("not on the lamp"));
    }

    /// A decline or a change calls nothing: the wait lapses, and a new make
    /// replaces it, its old id approving nothing.
    #[test]
    fn a_decline_or_a_change_approves_nothing() {
        let now = Instant::now();
        let old = wait_for_yes("leaving-mail", &[mail()], 3, now);
        assert!(!awaited(now + YES_WITHIN + Duration::from_secs(1)));
        let _changed = wait_for_yes("leaving-mail-2", &[mail()], 5, now);
        let told = approve(&old, 6, true, true, now, |_, _| panic!("approved"));
        assert_eq!(told["error"], NOT_APPROVED);
    }
}
