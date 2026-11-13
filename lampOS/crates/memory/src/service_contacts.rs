//! Service contacts (the voice's `LAMP_SERVICE_CONTACTS`, beside
//! `LAMP_SERVICE_RECORDS`, the connector records plan, PR 1c): the owner's
//! mail contacts, a name and one address, so "mail Octavia" needs no
//! spelling a day later.
//!
//! - **One address a name**, keyed by the name folded (lower case, without
//!   accents): a put of a known name with another address replaces it
//!   ([`Days::service_contact_put`]). The voice puts one only after the
//!   owner confirmed a read-back naming that address with a send, and asks
//!   first in that read-back when the name had another.
//! - **A known person's alone** (`p-` ids): the owner's. Never the room's, a
//!   rules scope's or a guest's.
//! - **Kept until forgotten**, in a table of its own that no expiry, tidy or
//!   nightly pass reads. Forget everything and forgetting the person take
//!   them all ([`empty`]); a forget about a thing takes the contacts whose
//!   name, address or the words "contact", "address", "email" and "mail"
//!   hold every key word of it ([`Days::service_contacts_forget`]).
//!   Nothing at the service is undone.
//! - **Bounded**: at most [`MOST_PER_PERSON`] a person, the one changed
//!   longest ago going first past it.
use crate::{
    days::{Days, Error, Result, ensure_person},
    service_records::{self, FORGOT_ABOUT_HELD, folded, has_word},
    store::Person,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};

/// The most contacts one person keeps.
pub const MOST_PER_PERSON: usize = 2_000;
/// The most bytes of a name.
const NAME_BYTES: usize = 64;
/// The words every contact is found by in a forget about, besides its name
/// and address: "forget Octavia's address", "forget that contact".
const CONTACT_WORDS: &str = "contact address email mail";

/// What a forget about removed of the contacts
/// ([`Days::service_contacts_forget`]): how many, their addresses, and how
/// many service records sent to one of them went with them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContactsForgot {
    pub removed: u64,
    pub addresses: Vec<String>,
    pub records: u64,
}

fn owner_only(person: &Person) -> Result<()> {
    if lamp_msgs::known::is_person_id(person.as_str()) {
        Ok(())
    } else {
        Err(Error::new("bad_person", "only a known person has contacts"))
    }
}

/// A mail address's shape, as the services check it: one `@`, a dotted
/// domain, nothing a header could break on.
fn address_ok(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    address.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && address.matches('@').count() == 1
        && address
            .chars()
            .all(|c| c.is_ascii_graphic() && !"<>()[],;:\\\"".contains(c))
}

/// A name's shape: 1 to 64 bytes of letters, spaces, hyphens and
/// apostrophes, with a letter in it.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_BYTES
        && name.chars().any(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'' || c == '’')
}

/// The key a name is kept under: folded, its words joined by one space.
pub fn name_key(name: &str) -> String {
    folded(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Days {
    /// Saves `address` for `name`, `person`'s: `"new"`, `"same"` (the
    /// address kept already) or `"replaced"` (another address was kept
    /// for that name, and is no longer).
    pub fn service_contact_put(
        &mut self,
        person: &Person,
        name: &str,
        address: &str,
        at_unix_ms: u64,
    ) -> Result<&'static str> {
        owner_only(person)?;
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if !name_ok(&name) {
            return Err(Error::new(
                "bad_name",
                "a contact's name is 1 to 64 bytes of letters, spaces, hyphens and apostrophes",
            ));
        }
        let address = address.trim().to_lowercase();
        if !address_ok(&address) {
            return Err(Error::new("bad_address", "that is not a mail address"));
        }
        let key = name_key(&name);
        let tx = self.db.transaction()?;
        // A save stamped before a forget everything of this person (or of
        // everyone) is refused, as a service record's is (1a's forget marks,
        // `service_records_forgot`): one retried after the forget never
        // brings the address back (Jarvis's P2 on #6122).
        let mark: Option<i64> = tx.query_row(
            "SELECT max(at_unix_ms) FROM service_records_forgot WHERE person IN (?1,'')",
            [person.as_str()],
            |r| r.get(0),
        )?;
        // So is one stamped before a forget about it, held as 1a's records
        // forget holds its subjects (`records_forgot_about`, which
        // [`Self::service_contacts_forget`] writes too; Thirteen's P2 on
        // #6122).
        let about = self
            .records_forgot_about
            .iter()
            .any(|(whose, subjects, at)| {
                whose == person.as_str()
                    && *at > at_unix_ms
                    && contact_matches(&contact_subjects(subjects), &name, &address)
            });
        if about || mark.is_some_and(|mark| u64::try_from(mark).unwrap_or(0) > at_unix_ms) {
            return Err(Error::new(
                "forgotten",
                "the contact was saved before a forget that takes it",
            ));
        }
        ensure_person(&tx, person)?;
        let kept: Option<String> = tx
            .query_row(
                "SELECT address FROM service_contacts WHERE person=?1 AND name_key=?2",
                params![person.as_str(), key],
                |r| r.get(0),
            )
            .optional()?;
        let said = match kept {
            Some(kept) if kept == address => {
                tx.execute(
                    "UPDATE service_contacts SET name=?3, changed_unix_ms=max(changed_unix_ms,?4) WHERE person=?1 AND name_key=?2",
                    params![person.as_str(), key, name, at_unix_ms],
                )?;
                "same"
            }
            Some(_) => {
                tx.execute(
                    "UPDATE service_contacts SET name=?3, address=?4, changed_unix_ms=max(changed_unix_ms,?5) WHERE person=?1 AND name_key=?2",
                    params![person.as_str(), key, name, address, at_unix_ms],
                )?;
                "replaced"
            }
            None => {
                tx.execute(
                    "INSERT INTO service_contacts(person,name_key,name,address,saved_unix_ms,changed_unix_ms) VALUES(?1,?2,?3,?4,?5,?5)",
                    params![person.as_str(), key, name, address, at_unix_ms],
                )?;
                trim(&tx, person)?;
                "new"
            }
        };
        tx.commit()?;
        Ok(said)
    }

    /// `person`'s contacts, by name, or the one of `name` (none, or one).
    pub fn service_contacts(&self, person: &Person, name: Option<&str>) -> Result<Vec<Value>> {
        owner_only(person)?;
        let key = name.map(name_key);
        let mut statement = self.db.prepare(
            "SELECT name,address,saved_unix_ms,changed_unix_ms FROM service_contacts WHERE person=?1 AND (?2 IS NULL OR name_key=?2) ORDER BY name_key LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![person.as_str(), key, MOST_PER_PERSON as i64], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "address": r.get::<_, String>(1)?,
                    "saved_unix_ms": r.get::<_, i64>(2)?,
                    "changed_unix_ms": r.get::<_, i64>(3)?,
                }))
            })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The contacts of `person` about every key word of any of `about`
    /// (each a subject's words, as the voice's forget gives them), compared
    /// lower case and without accents, removed: how many. A one-letter
    /// word ("Octavia's" gives an "s") is passed over.
    /// Every mail contact of `person` removed (`service_contacts_forget`
    /// with `all`), as forget everything removes them.
    pub fn service_contacts_forget_all(&mut self, person: &Person) -> Result<u64> {
        owner_only(person)?;
        let tx = self.db.transaction()?;
        let removed = empty(&tx, Some(person))?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn service_contacts_forget(
        &mut self,
        person: &Person,
        about: &[Vec<String>],
    ) -> Result<ContactsForgot> {
        owner_only(person)?;
        let subjects = contact_subjects(about);
        if subjects.is_empty() {
            return Ok(ContactsForgot::default());
        }
        // Held first, found or not, as 1a's records forget holds its own: a
        // save stamped before it that matches is refused
        // ([`Self::service_contact_put`]).
        if self.records_forgot_about.len() >= FORGOT_ABOUT_HELD {
            self.records_forgot_about.pop_front();
        }
        self.records_forgot_about.push_back((
            person.as_str().to_owned(),
            subjects.clone(),
            self.now,
        ));
        let tx = self.db.transaction()?;
        let rows: Vec<(String, String, String)> = {
            let mut statement = tx.prepare(
                "SELECT name_key, name||' '||address, address FROM service_contacts WHERE person=?1",
            )?;
            statement
                .query_map([person.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut forgot = ContactsForgot::default();
        for (key, text, address) in rows {
            if contact_matches(&subjects, &text, "") {
                forgot.removed += tx.execute(
                    "DELETE FROM service_contacts WHERE person=?1 AND name_key=?2",
                    params![person.as_str(), key],
                )? as u64;
                if !forgot.addresses.contains(&address) {
                    forgot.addresses.push(address);
                }
            }
        }
        // The records sent to a removed address go with it, whatever their
        // words: a `done` mail told to the model would bring the address
        // back (step 5d on lamp-06c1, 11 Nov). One asked before and taken
        // after is refused ([`Self::service_record_put`]).
        forgot.records = service_records::forget_sent_to(&tx, person, &forgot.addresses)?;
        tx.commit()?;
        if !forgot.addresses.is_empty() {
            if self.records_forgot_addresses.len() >= FORGOT_ABOUT_HELD {
                self.records_forgot_addresses.pop_front();
            }
            self.records_forgot_addresses.push_back((
                person.as_str().to_owned(),
                forgot.addresses.clone(),
                self.now,
            ));
        }
        Ok(forgot)
    }
}

/// A forget's subjects as contacts are matched: each word folded, a
/// one-letter word ("Octavia's" gives an "s") passed over.
fn contact_subjects(about: &[Vec<String>]) -> Vec<Vec<String>> {
    about
        .iter()
        .map(|words| {
            words
                .iter()
                .map(|word| folded(word.trim()))
                .filter(|word| word.chars().count() > 1)
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

/// Whether a contact's `name` and `address`, with [`CONTACT_WORDS`], hold
/// every word of any of `subjects`.
fn contact_matches(subjects: &[Vec<String>], name: &str, address: &str) -> bool {
    let text = folded(&format!("{name} {address} {CONTACT_WORDS}"));
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    subjects
        .iter()
        .any(|subject| subject.iter().all(|word| has_word(&words, word)))
}

/// Past [`MOST_PER_PERSON`], `person`'s contact changed longest ago goes.
fn trim(tx: &Transaction<'_>, person: &Person) -> Result<()> {
    tx.execute(
        "DELETE FROM service_contacts WHERE person=?1 AND name_key IN (SELECT name_key FROM service_contacts WHERE person=?1 ORDER BY changed_unix_ms DESC, name_key LIMIT -1 OFFSET ?2)",
        params![person.as_str(), MOST_PER_PERSON as i64],
    )?;
    Ok(())
}

/// Forget everything (`forget_all`, `forget_person`): `person`'s contacts,
/// or everyone's with `None`. How many went.
pub(crate) fn empty(tx: &Transaction<'_>, person: Option<&Person>) -> Result<u64> {
    Ok(tx.execute(
        "DELETE FROM service_contacts WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )? as u64)
}

#[cfg(test)]
mod tests {
    use crate::{
        days::{DAY_MS, Days},
        days_protocol::answer,
        store::{LocalTime, Options},
    };
    use serde_json::{Value, json};
    use std::{fs, path::PathBuf};

    const NOW: u64 = 1_791_892_800_000;
    const OWNER: &str = "p-0123456789abcdef";
    const OTHER: &str = "p-fedcba9876543210";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Days) {
            let dir = std::env::temp_dir().join(format!(
                "lamp-service-contacts-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            let scratch = Self(dir);
            let store = scratch.open();
            (scratch, store)
        }

        fn open(&self) -> Days {
            Days::open(
                Options {
                    dir: self.0.clone(),
                    keep_days: 30,
                    local: LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn send_at(store: &mut Days, request: Value, now: u64) -> Value {
        serde_json::from_str(&answer(store, request.to_string().as_bytes(), now).line).unwrap()
    }

    fn send(store: &mut Days, request: Value) -> Value {
        send_at(store, request, NOW)
    }

    fn put(store: &mut Days, person: &str, name: &str, address: &str) -> Value {
        send(
            store,
            json!({"op":"service_contact","person":person,"name":name,"address":address,"at_unix_ms":NOW}),
        )
    }

    fn contacts(store: &mut Days, person: &str) -> Vec<Value> {
        let listed = send(store, json!({"op":"service_contacts","person":person}));
        assert_eq!(listed["ok"], true, "{listed}");
        listed["contacts"].as_array().unwrap().clone()
    }

    /// A contact is saved, found by its name (any case, accents or not),
    /// replaced by a new address for the same name, and kept across a
    /// restart, the day's expiry and a year.
    #[test]
    fn a_contact_is_saved_found_replaced_and_kept() {
        let (scratch, mut store) = Scratch::new("saved");
        assert_eq!(
            put(&mut store, OWNER, "Octavia", "Octavia@Example.com"),
            json!({"ok":true,"kept":"new"})
        );
        assert_eq!(
            put(&mut store, OWNER, "octavia", "octavia@example.com")["kept"],
            "same"
        );
        let one = send(
            &mut store,
            json!({"op":"service_contacts","person":OWNER,"name":"OCTAVIA"}),
        );
        assert_eq!(one["contacts"][0]["address"], "octavia@example.com");
        assert_eq!(
            put(&mut store, OWNER, "Ngọc Mai", "mai@home.test")["kept"],
            "new"
        );
        let mai = send(
            &mut store,
            json!({"op":"service_contacts","person":OWNER,"name":"ngoc  mai"}),
        );
        assert_eq!(mai["contacts"][0]["address"], "mai@home.test", "{mai}");
        assert_eq!(
            put(&mut store, OWNER, "Octavia", "octavia@work.test")["kept"],
            "replaced"
        );
        drop(store);
        let mut store = scratch.open();
        store.maintain(NOW + 400 * DAY_MS).unwrap();
        let kept = contacts(&mut store, OWNER);
        assert_eq!(kept.len(), 2, "{kept:?}");
        let octavia = kept.iter().find(|c| c["name"] == "Octavia").unwrap();
        assert_eq!(octavia["address"], "octavia@work.test");
        assert!(contacts(&mut store, OTHER).is_empty());
    }

    /// Only a known person's, with a name and a real address.
    #[test]
    fn contacts_are_a_known_persons_with_a_name_and_an_address() {
        let (_scratch, mut store) = Scratch::new("shapes");
        let room = send(
            &mut store,
            json!({"op":"service_contact","name":"Octavia","address":"o@x.test","at_unix_ms":NOW}),
        );
        assert_eq!(room["error"], "bad_person", "{room}");
        assert_eq!(
            put(&mut store, "r-0123456789abcdef", "Octavia", "o@x.test")["error"],
            "bad_person"
        );
        assert_eq!(
            put(&mut store, OWNER, "Octavia", "vu at gmail")["error"],
            "bad_address"
        );
        assert_eq!(
            put(&mut store, OWNER, "Octavia", "o@x.test\r\nBcc: y@z.test")["error"],
            "bad_address"
        );
        assert_eq!(
            put(&mut store, OWNER, "send it now!", "o@x.test")["error"],
            "bad_name"
        );
        assert_eq!(put(&mut store, OWNER, "", "o@x.test")["error"], "bad_name");
        assert!(contacts(&mut store, OWNER).is_empty());
    }

    /// Forget everything takes the person's contacts and counts them;
    /// forgetting the person takes theirs; no one else's.
    #[test]
    fn forget_everything_and_forget_the_person_remove_contacts() {
        let (_scratch, mut store) = Scratch::new("forget-all");
        put(&mut store, OWNER, "Octavia", "octavia@example.com");
        put(&mut store, OTHER, "Mai", "mai@home.test");
        let forgot = send(&mut store, json!({"op":"forget_all","person":OWNER}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(forgot["removed"].as_u64().unwrap_or(0) >= 1, "{forgot}");
        assert!(contacts(&mut store, OWNER).is_empty());
        assert_eq!(contacts(&mut store, OTHER).len(), 1);
        let gone = send(&mut store, json!({"op":"forget_person","person":OTHER}));
        assert_eq!(gone["ok"], true, "{gone}");
        assert!(contacts(&mut store, OTHER).is_empty());
    }

    /// `service_contacts_forget` with `all` takes every contact of that
    /// person and no one else's (the switch-on row on lamp-06c1, 11 Nov).
    #[test]
    fn a_contacts_forget_with_all_takes_every_contact_of_that_person() {
        let (_scratch, mut store) = Scratch::new("forget-contacts-all");
        put(&mut store, OWNER, "Octavia", "octavia@example.com");
        put(&mut store, OWNER, "Lan", "lan@example.com");
        put(&mut store, OTHER, "Mai", "mai@example.com");
        let forgot = send(
            &mut store,
            json!({"op":"service_contacts_forget","person":OWNER,"all":true}),
        );
        assert_eq!(forgot, json!({"ok":true,"removed":2}));
        assert!(contacts(&mut store, OWNER).is_empty());
        assert_eq!(contacts(&mut store, OTHER).len(), 1);
    }

    /// A save stamped before a forget everything is refused and leaves no
    /// row, so one retried after the forget never brings the address back;
    /// one stamped after it is kept (Jarvis's P2 on #6122).
    #[test]
    fn a_contact_saved_before_a_forget_everything_is_refused() {
        let (_scratch, mut store) = Scratch::new("forget-mark");
        let forgot = send_at(
            &mut store,
            json!({"op":"forget_all","person":OWNER}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        let late = send_at(
            &mut store,
            json!({"op":"service_contact","person":OWNER,"name":"Octavia","address":"octavia@example.com","at_unix_ms":NOW}),
            NOW + 1_500,
        );
        assert_eq!(late["ok"], false, "{late}");
        assert_eq!(late["error"], "forgotten", "{late}");
        assert!(contacts(&mut store, OWNER).is_empty());
        let after = send_at(
            &mut store,
            json!({"op":"service_contact","person":OWNER,"name":"Octavia","address":"octavia@example.com","at_unix_ms":NOW + 2_000}),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
        assert_eq!(contacts(&mut store, OWNER).len(), 1);
    }

    /// A save stamped before a forget about it is refused and leaves no
    /// row; one stamped after it, or about someone else, is kept
    /// (Thirteen's P2 on #6122).
    #[test]
    fn a_contact_saved_before_a_forget_about_it_is_refused() {
        let (_scratch, mut store) = Scratch::new("forget-about-mark");
        let forgot = send_at(
            &mut store,
            json!({"op":"service_contacts_forget","person":OWNER,"about":[["octavia"]]}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        let save = |name: &str, address: &str, at: u64| json!({"op":"service_contact","person":OWNER,"name":name,"address":address,"at_unix_ms":at});
        let late = send_at(
            &mut store,
            save("Octavia", "octavia@example.com", NOW),
            NOW + 1_500,
        );
        assert_eq!(late["ok"], false, "{late}");
        assert_eq!(late["error"], "forgotten", "{late}");
        // By its address alone too.
        let by_address = send_at(
            &mut store,
            save("Tavi", "octavia@example.com", NOW),
            NOW + 1_500,
        );
        assert_eq!(by_address["error"], "forgotten", "{by_address}");
        assert!(contacts(&mut store, OWNER).is_empty());
        let other = send_at(&mut store, save("Mai", "mai@home.test", NOW), NOW + 1_500);
        assert_eq!(other["ok"], true, "{other}");
        let after = send_at(
            &mut store,
            save("Octavia", "octavia@example.com", NOW + 2_000),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
        assert_eq!(contacts(&mut store, OWNER).len(), 2);
    }

    /// "Forget Octavia's address" takes that contact alone; "forget the
    /// party" takes none; another person's never.
    #[test]
    fn forget_about_takes_only_the_named_contact() {
        let (_scratch, mut store) = Scratch::new("forget-about");
        put(&mut store, OWNER, "Octavia", "octavia@example.com");
        put(&mut store, OWNER, "Mai", "mai@home.test");
        put(&mut store, OTHER, "Octavia", "octavia@example.com");
        let forget = |store: &mut Days, about: Value| -> Value {
            send(
                store,
                json!({"op":"service_contacts_forget","person":OWNER,"about":about}),
            )
        };
        assert_eq!(forget(&mut store, json!([["party"]]))["removed"], 0);
        assert_eq!(
            forget(&mut store, json!([["octavia", "phone"]]))["removed"],
            0
        );
        assert_eq!(
            forget(&mut store, json!([["octavia", "s", "address"]]))["removed"],
            1
        );
        let left = contacts(&mut store, OWNER);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0]["name"], "Mai");
        assert_eq!(contacts(&mut store, OTHER).len(), 1);
    }
    /// A done send record to a forgotten contact's address, words or not:
    /// "forget Tom's address" takes the contact and every record of that
    /// owner sent to it, in the same forget; a record to another address,
    /// or another person's to it, stays. One asked before the forget and
    /// taken after it is refused (step 5d on lamp-06c1, 11 Nov).
    #[test]
    fn forget_about_a_contact_takes_the_records_sent_to_its_address() {
        let (_scratch, mut store) = Scratch::new("forget-sent-to");
        let mail = |person: &str, key: &str, to: Value, asked: u64| {
            json!({
                "op": "service_record",
                "person": person,
                "key": key,
                "service": "gmail",
                "action": "send_mail",
                "status": "done",
                "reason": "",
                "detail": {"to": to, "subject": "Lunch", "text": "See you at noon"},
                "asked_unix_ms": asked,
                "changed_unix_ms": asked,
            })
        };
        put(&mut store, OWNER, "Tom", "tom@example.com");
        for (person, key, to) in [
            (OWNER, "sent-1", json!(["Tom@Example.com"])),
            (OWNER, "sent-2", json!("other@example.com, tom@example.com")),
            (OWNER, "sent-3", json!(["other@example.com"])),
            (OTHER, "sent-4", json!(["tom@example.com"])),
        ] {
            let kept = send(&mut store, mail(person, key, to, NOW));
            assert_eq!(kept["ok"], true, "{kept}");
        }
        let forgot = send_at(
            &mut store,
            json!({"op":"service_contacts_forget","person":OWNER,"about":[["tom", "s", "address"]]}),
            NOW + 1_000,
        );
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert_eq!(forgot["removed"], 1, "{forgot}");
        assert_eq!(forgot["records"], 2, "{forgot}");
        assert_eq!(forgot["addresses"], json!(["tom@example.com"]), "{forgot}");
        let listed = send(&mut store, json!({"op":"service_records","person":OWNER}));
        let left = listed["records"].as_array().unwrap();
        assert!(
            !left.iter().any(|record| record
                .to_string()
                .to_lowercase()
                .contains("tom@example.com")),
            "{left:?}"
        );
        assert_eq!(left.len(), 1, "{left:?}");
        let theirs = send(&mut store, json!({"op":"service_records","person":OTHER}));
        assert_eq!(theirs["records"].as_array().unwrap().len(), 1, "{theirs}");
        // In flight at the forget: refused, nothing kept.
        let late = send_at(
            &mut store,
            mail(OWNER, "sent-5", json!(["tom@example.com"]), NOW + 500),
            NOW + 1_500,
        );
        assert_eq!(late["error"], "forgotten", "{late}");
        // Asked after it: kept.
        let after = send_at(
            &mut store,
            mail(OWNER, "sent-6", json!(["tom@example.com"]), NOW + 2_000),
            NOW + 2_000,
        );
        assert_eq!(after["ok"], true, "{after}");
    }
}
