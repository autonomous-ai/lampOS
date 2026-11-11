//! Service contacts (`LAMP_SERVICE_CONTACTS`, beside `LAMP_SERVICE_RECORDS`,
//! the connector records plan, PR 1c): the addresses of the owner's mails,
//! saved as contacts in the Days memory (`service_contact`), so "mail
//! Octavia" fills in her address a day later, and the read-back still says
//! it.
//!
//! Off unless the second worker has `LAMP_SERVICE_CONTACTS=on` and
//! `LAMP_SERVICE_RECORDS=on` with the Days memory; off, nothing is saved,
//! looked up or filled in, and a mail is prepared exactly as before.
//!
//! - **Saved only from a confirmed read-back.** A contact is planned at the
//!   read-back ([`plan`]) and saved only when that write's send went
//!   (`sent:true`, [`sent`]): an address misheard and read back, then
//!   corrected, cancelled or let go, is never saved.
//! - **Named only by the owner.** The name comes from the owner's own lines
//!   ("send it to Octavia") or from the greeting of the draft they asked
//!   for ("Hi Octavia,"), never from what a service read gave: a mail
//!   someone else sent names no contact. An address with no such name is
//!   not saved.
//! - **Used in the mail step** ([`fill`]): a name alone in `to` or `cc` is
//!   filled in with its saved address before the services prepare it, so
//!   the read-back says the address. A name with none saved is answered
//!   with a plain ask for the address, nothing prepared.
//! - **Never replaced silently.** An address for a name that has another
//!   saved adds a question to the read-back ([`replace_note`]); only a
//!   confirmed send of it replaces the saved one.
//! - **Whose.** The owner's memory id alone (`p-` ids), as the services'
//!   owner check gives it (`OWNER_ONLY`): the room, a guest or a voice not
//!   the owner's never reach one.
//! - **Forgotten** with forget everything (`days::empty`) and by a forget
//!   about ("forget Octavia's address", `service_contacts_forget`); nothing
//!   at the service is undone.
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Duration};

/// Only the exact value `on`, with `LAMP_SERVICE_RECORDS` on, on the second
/// route.
pub const ENVIRONMENT: &str = "LAMP_SERVICE_CONTACTS";
/// The longest a lookup in the mail step waits for the memory: a local
/// socket, answered in milliseconds.
const LOOKUP_DEADLINE: Duration = Duration::from_millis(400);
/// The longest one save waits, and how many tries it gets.
#[cfg(not(test))]
const SAVE_DEADLINE: Duration = Duration::from_secs(2);
#[cfg(not(test))]
const SAVE_TRIES: u32 = 3;
/// The most planned contacts held, one a waiting write.
const PLANS_KEPT: usize = 8;
/// The most bytes of a name.
const NAME_BYTES: usize = 64;

/// What the model is told when a name in `to` or `cc` has no address
/// saved: nothing is prepared.
pub const NO_CONTACT: &str = "Not prepared: no address is saved for {name}. Ask them for the address, and have them spell it.";
/// What the model is told when the saved addresses can't be read now.
pub const NO_CONTACTS_NOW: &str = "Not prepared: the saved addresses can't be read now. Ask them for the address, and have them spell it.";
/// What a mail tool's `to` is told, so the model gives a name alone.
pub const TO_NOTE: &str = "A name alone, such as \"Octavia\", is filled in with the address saved for it, and the read_back says it.";

/// One contact a waiting write would save: its name and address, and the
/// other address saved for that name, if one is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub name: String,
    pub address: String,
    pub replaces: Option<String>,
}

/// The contacts a waiting write `send_id` would save once sent, `person`'s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub send_id: String,
    pub person: String,
    pub contacts: Vec<Planned>,
}

#[derive(Default)]
pub struct Plans {
    plans: VecDeque<Plan>,
}

#[cfg(not(test))]
mod kept {
    use super::Plans;
    use std::sync::{LazyLock, Mutex};

    static PLANS: LazyLock<Mutex<Plans>> = LazyLock::new(Mutex::default);

    pub fn plans<R>(f: impl FnOnce(&mut Plans) -> R) -> R {
        f(&mut PLANS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub fn on() -> bool {
        static ON: LazyLock<bool> =
            LazyLock::new(|| std::env::var(super::ENVIRONMENT).as_deref() == Ok("on"));
        *ON
    }

    pub fn exchange(
        line: &serde_json::Value,
        wait: std::time::Duration,
    ) -> Option<serde_json::Value> {
        let socket = crate::remember::days_socket()?;
        crate::tools::exchange(&socket, line, wait)
    }
}

/// Tests: each test thread its own, and a memory of its own.
#[cfg(test)]
mod kept {
    use super::Plans;
    use serde_json::{Value, json};
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };

    thread_local! {
        static PLANS: RefCell<Plans> = RefCell::default();
        pub static ON: Cell<bool> = const { Cell::new(false) };
        /// The memory's contacts: (person, folded name) to (name, address).
        pub static MEMORY: RefCell<BTreeMap<(String, String), (String, String)>> =
            const { RefCell::new(BTreeMap::new()) };
        pub static DOWN: Cell<bool> = const { Cell::new(false) };
    }

    pub fn plans<R>(f: impl FnOnce(&mut Plans) -> R) -> R {
        PLANS.with(|cell| f(&mut cell.borrow_mut()))
    }

    pub fn on() -> bool {
        ON.with(Cell::get)
    }

    /// The memory's two ops, as `lamp-memory` answers them.
    pub fn exchange(line: &Value, _wait: std::time::Duration) -> Option<Value> {
        if DOWN.with(Cell::get) {
            return None;
        }
        let person = line["person"].as_str()?.to_owned();
        let name = line["name"].as_str().unwrap_or_default();
        let key = (person, super::folded(name));
        MEMORY.with(|memory| {
            let mut memory = memory.borrow_mut();
            match line["op"].as_str()? {
                "service_contacts" => Some(json!({"ok":true,"contacts":
                    memory.get(&key).map(|(name, address)| vec![json!({"name":name,"address":address})]).unwrap_or_default()})),
                "service_contact" => {
                    let address = line["address"].as_str()?.to_owned();
                    let kept = match memory.insert(key, (name.to_owned(), address.clone())) {
                        None => "new",
                        Some((_, old)) if old == address => "same",
                        Some(_) => "replaced",
                    };
                    Some(json!({"ok":true,"kept":kept}))
                }
                _ => None,
            }
        })
    }
}

/// Whether contacts are saved and used: [`ENVIRONMENT`], and the service
/// records' own switch.
pub fn on() -> bool {
    kept::on() && crate::service_records::on()
}

/// Tests: contacts on or off on this thread.
#[cfg(test)]
pub fn on_for_test(on: bool) {
    kept::ON.with(|cell| cell.set(on));
}

/// `text` lower case and without accents, its words joined by one space,
/// as the memory keys a name.
fn folded(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .replace(['đ', 'Đ'], "d")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A mail write to Gmail: the only write a contact is for.
fn is_mail(args: &Value) -> bool {
    matches!(
        args.get("service").and_then(Value::as_str),
        Some("gmail" | "gmail-app")
    ) && args.get("action").and_then(Value::as_str) == Some("send_mail")
}

/// The entries of `args[key]`, as the services split them.
fn entries(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::String(text)) => text
            .split(',')
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// A name's shape: letters, spaces, hyphens and apostrophes, a letter in
/// it, at most [`NAME_BYTES`].
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_BYTES
        && name.chars().any(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'' || c == '’')
}

/// The address saved for `name`, `person`'s: `Ok(None)` for none, `Err`
/// when the memory did not answer.
fn saved(person: &str, name: &str) -> Result<Option<(String, String)>, ()> {
    let answer = kept::exchange(
        &json!({"op":"service_contacts","person":person,"name":name}),
        LOOKUP_DEADLINE,
    )
    .filter(|answer| answer["ok"] == true)
    .ok_or(())?;
    Ok(answer["contacts"]
        .as_array()
        .and_then(|contacts| contacts.first())
        .and_then(|contact| {
            Some((
                contact["name"].as_str()?.to_owned(),
                contact["address"].as_str()?.to_lowercase(),
            ))
        }))
}

/// The mail step: each name alone in `to` and `cc` of `owner`'s mail write
/// replaced with its saved address, before the services prepare it. The
/// names filled in, with their addresses; or what the model is told when
/// one has none saved, or the memory did not answer.
pub fn fill(owner: Option<&str>, args: &mut Value) -> Result<Vec<Planned>, Value> {
    let Some(owner) = owner.filter(|owner| lamp_msgs::known::is_person_id(owner)) else {
        return Ok(Vec::new());
    };
    if !on() || !is_mail(args) {
        return Ok(Vec::new());
    }
    let mut filled = Vec::new();
    for key in ["to", "cc"] {
        let given = entries(args, key);
        if !given.iter().any(|one| !one.contains('@') && name_ok(one)) {
            continue;
        }
        let mut out = Vec::with_capacity(given.len());
        for one in given {
            if one.contains('@') || !name_ok(&one) {
                out.push(one);
                continue;
            }
            match saved(owner, &one) {
                Ok(Some((name, address))) => {
                    eprintln!(
                        "lamp-voice: a mail's {key} filled in from a saved contact (LAMP_SERVICE_CONTACTS)"
                    );
                    out.push(address.clone());
                    filled.push(Planned {
                        name,
                        replaces: Some(address.clone()),
                        address,
                    });
                }
                Ok(None) => {
                    eprintln!(
                        "lamp-voice: a mail named someone with no saved address; nothing prepared (LAMP_SERVICE_CONTACTS)"
                    );
                    return Err(json!({"error": NO_CONTACT.replace("{name}", &one)}));
                }
                Err(()) => {
                    eprintln!(
                        "lamp-voice: the memory did not give the saved addresses; nothing prepared (LAMP_SERVICE_CONTACTS)"
                    );
                    return Err(json!({ "error": NO_CONTACTS_NOW }));
                }
            }
        }
        args[key] = json!(out);
    }
    Ok(filled)
}

/// Words a name never is, though said with a capital: the lamp, a weekday,
/// a service.
const NOT_NAMES: [&str; 22] = [
    "i",
    "lamp",
    "lampy",
    "gmail",
    "google",
    "email",
    "mail",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "today",
    "tomorrow",
    "it",
    "him",
    "her",
    "them",
    "me",
    "my",
];
/// Words before a name the owner says a mail is for: "send it to Octavia",
/// "email Octavia", "gửi mail cho Mai".
const BEFORE_NAMES: [&str; 8] = [
    "to", "email", "mail", "e-mail", "message", "cho", "tới", "write",
];
/// Words passed over between those and the name: "to my friend Octavia".
const BETWEEN: [&str; 14] = [
    "my", "friend", "an", "a", "the", "email", "mail", "her", "his", "our", "chị", "anh", "bạn",
    "em",
];
/// A draft's first words that greet: "Hi Octavia,".
const GREETINGS: [&str; 8] = ["hi", "hello", "hey", "dear", "chào", "chao", "gửi", "thân"];

/// Whether `word` is one of `list`, both folded: "chị" is "chi".
fn among(list: &[&str], word: &str) -> bool {
    let word = folded(bare(word));
    list.iter().any(|one| folded(one) == word)
}

fn bare(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'')
        .trim_end_matches("'s")
        .trim_end_matches("’s")
}

fn capital(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase) && !among(&NOT_NAMES, word)
}

/// The name of up to two capitalised words at `words[at..]`, ending at a
/// comma or a full stop.
fn name_at(words: &[&str], at: usize) -> Option<String> {
    let mut name: Vec<&str> = Vec::new();
    for word in words.iter().skip(at).take(2) {
        let plain = bare(word);
        if plain.is_empty() || !capital(plain) {
            break;
        }
        name.push(plain);
        if word.ends_with([',', '.', '!', '?', ':', ';']) || word.ends_with("'s") {
            break;
        }
    }
    let name = name.join(" ");
    name_ok(&name).then_some(name)
}

/// The name the draft greets in its first words: "Hi Octavia," gives
/// "Octavia", "Chào chị Mai," "Mai".
pub fn greeted(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().take(5).collect();
    let first = folded(bare(words.first()?));
    let mut at = usize::from(among(&GREETINGS, &first));
    if at == 0 {
        // "Xin chào Mai".
        if first == "xin" && words.get(1).is_some_and(|w| folded(bare(w)) == "chao") {
            at = 2;
        } else {
            return None;
        }
    }
    while words.get(at).is_some_and(|word| among(&BETWEEN, word)) {
        at += 1;
    }
    name_at(&words, at)
}

/// The names the owner said a mail is for, in their own `lines`: "send it
/// to Octavia", "email my friend Octavia", "gửi mail cho Mai".
pub fn said_names(lines: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (n, word) in words.iter().enumerate() {
            if !among(&BEFORE_NAMES, word) {
                continue;
            }
            let mut at = n + 1;
            while at < words.len()
                && at <= n + 3
                && among(&BETWEEN, words[at])
                && !capital(bare(words[at]))
            {
                at += 1;
            }
            if let Some(name) = name_at(&words, at)
                && !names.iter().any(|known| folded(known) == folded(&name))
            {
                names.push(name);
            }
        }
    }
    names
}

/// Whether `name` is in the address's own part: "Octavia" in
/// "octavia@example.com".
fn in_address(name: &str, address: &str) -> bool {
    let local: String = folded(address.split('@').next().unwrap_or_default())
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect();
    folded(name)
        .split(' ')
        .next()
        .is_some_and(|first| first.chars().count() >= 3 && local.contains(first))
}

/// What the read-back adds when `planned` would replace a saved address:
/// both said, and that only a send replaces it.
pub fn replace_note(planned: &Planned) -> Option<String> {
    let old = planned.replaces.as_deref()?;
    (old != planned.address).then(|| {
        format!(
            " For {} I have {old} saved, not {}. Sending this saves {} for {} instead; if you meant {old}, say so.",
            planned.name, planned.address, planned.address, planned.name
        )
    })
}

/// A mail write of `owner`'s prepared and given a read-back in `told`:
/// the contacts its send would save, held under its `send_id`. Each
/// address is named only from the owner's own `lines` or the draft's
/// greeting; a name filled in by [`fill`] (`filled`) keeps its own. A name
/// with another address saved adds [`replace_note`] to the read-back,
/// which the model is told to say in full. Nothing is saved here.
pub fn plan(
    owner: Option<&str>,
    args: &Value,
    filled: &[Planned],
    lines: &[String],
    told: &mut Value,
) {
    let Some(owner) = owner.filter(|owner| lamp_msgs::known::is_person_id(owner)) else {
        return;
    };
    if !on() || !is_mail(args) {
        return;
    }
    let Some(send_id) = told
        .get("send_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
    else {
        return;
    };
    if told.get("read_back").and_then(Value::as_str).is_none() {
        return;
    }
    let addresses: Vec<String> = ["to", "cc"]
        .iter()
        .flat_map(|key| entries(args, key))
        .map(|one| one.to_lowercase())
        .filter(|one| lamp_services::mail::address_ok(one))
        .collect();
    let greeting = args.get("text").and_then(Value::as_str).and_then(greeted);
    let said = said_names(lines);
    let candidates: Vec<String> = greeting.iter().cloned().chain(said.clone()).collect();
    let mut contacts: Vec<Planned> = Vec::new();
    for address in &addresses {
        if contacts.iter().any(|planned| planned.address == *address) {
            continue;
        }
        if let Some(planned) = filled.iter().find(|planned| planned.address == *address) {
            contacts.push(planned.clone());
            continue;
        }
        let name = candidates
            .iter()
            .find(|name| in_address(name, address))
            .cloned()
            .or_else(|| {
                (addresses.len() == 1)
                    .then(|| {
                        greeting
                            .clone()
                            .or_else(|| (said.len() == 1).then(|| said[0].clone()))
                    })
                    .flatten()
            });
        let Some(name) = name else { continue };
        if contacts
            .iter()
            .any(|planned| folded(&planned.name) == folded(&name))
        {
            continue;
        }
        // Not known whether another is saved: never replaced unasked.
        let Ok(kept) = saved(owner, &name) else {
            continue;
        };
        contacts.push(Planned {
            name,
            address: address.clone(),
            replaces: kept.map(|(_, address)| address),
        });
    }
    let notes: String = contacts.iter().filter_map(replace_note).collect();
    if !notes.is_empty()
        && let Some(read_back) = told.get("read_back").and_then(Value::as_str)
    {
        told["read_back"] = json!(format!("{read_back}{notes}"));
        eprintln!(
            "lamp-voice: a read-back asks before a saved address is replaced (LAMP_SERVICE_CONTACTS)"
        );
    }
    let contacts: Vec<Planned> = contacts
        .into_iter()
        .filter(|planned| planned.replaces.as_deref() != Some(planned.address.as_str()))
        .collect();
    // One write waits at a time: a new one's read-back lets every earlier
    // plan of theirs go, so a write let go (a misheard address corrected)
    // never saves.
    kept::plans(|plans| plans.plans.retain(|plan| plan.person != owner));
    if contacts.is_empty() {
        return;
    }
    kept::plans(|plans| {
        if plans.plans.len() >= PLANS_KEPT {
            plans.plans.pop_front();
        }
        plans.plans.push_back(Plan {
            send_id,
            person: owner.to_owned(),
            contacts,
        });
    });
}

/// The waiting write `send_id` was sent: the read-back the owner confirmed
/// with their send. Its planned contacts are saved, a new address
/// replacing a saved one only now.
pub fn sent(send_id: &str) {
    if !on() {
        return;
    }
    let Some(plan) = kept::plans(|plans| {
        let at = plans
            .plans
            .iter()
            .position(|plan| plan.send_id == send_id)?;
        plans.plans.remove(at)
    }) else {
        return;
    };
    let replaced = plan
        .contacts
        .iter()
        .filter(|planned| planned.replaces.is_some())
        .count();
    eprintln!(
        "lamp-voice: a confirmed send saves {} mail contact(s), {replaced} replacing a saved address (LAMP_SERVICE_CONTACTS)",
        plan.contacts.len()
    );
    for planned in plan.contacts {
        save(json!({
            "op": "service_contact",
            "person": plan.person,
            "name": planned.name,
            "address": planned.address,
            "at_unix_ms": unix_ms(),
        }));
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// One contact written to the memory, beside the turn: a thread of its own
/// tries [`SAVE_TRIES`] times, and the journal says how it went, never the
/// name or the address.
#[cfg(not(test))]
fn save(line: Value) {
    let spawned = std::thread::Builder::new()
        .name("service-contact".into())
        .spawn(move || {
            for attempt in 0..SAVE_TRIES {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_secs(u64::from(attempt)));
                }
                if let Some(answer) = kept::exchange(&line, SAVE_DEADLINE) {
                    if answer["ok"] == true {
                        eprintln!(
                            "lamp-voice: a mail contact saved: {} (LAMP_SERVICE_CONTACTS)",
                            answer["kept"].as_str().unwrap_or("?")
                        );
                    } else {
                        eprintln!(
                            "lamp-voice: a mail contact was refused by the memory: {} (LAMP_SERVICE_CONTACTS)",
                            answer["error"].as_str().unwrap_or("no reason")
                        );
                    }
                    return;
                }
            }
            eprintln!(
                "lamp-voice: the memory did not take a mail contact after {SAVE_TRIES} tries; it is not saved (LAMP_SERVICE_CONTACTS)"
            );
        });
    if spawned.is_err() {
        eprintln!("lamp-voice: a mail contact was not saved: no thread (LAMP_SERVICE_CONTACTS)");
    }
}

#[cfg(test)]
fn save(line: Value) {
    let _ = kept::exchange(&line, LOOKUP_DEADLINE);
}

/// The memory forgot `person`'s everything, or everyone's (`None`): the
/// contacts planned for their waiting writes go too, so none is saved by a
/// later send.
pub fn forgot(person: Option<&str>) {
    kept::plans(|plans| {
        plans
            .plans
            .retain(|plan| person.is_some_and(|person| plan.person != person));
    });
}

/// A forget about removed `person`'s contacts in the memory: the planned
/// ones of theirs go too, so a later send saves none again.
pub fn forgot_about(person: &str) {
    forgot(Some(person));
}

/// The contacts planned now, oldest first (tests).
pub fn planned() -> Vec<Plan> {
    kept::plans(|plans| plans.plans.iter().cloned().collect())
}

#[cfg(test)]
mod tests;
