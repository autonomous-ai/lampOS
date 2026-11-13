//! `LAMP_SERVICE_CONTACTS` (the connector records plan, PR 1c): a contact
//! is saved only from a read-back the owner confirmed with a send, named
//! only by the owner, filled into a mail by name, and never replaced
//! without the read-back asking.
use super::*;
use crate::services::{Pending, Turn, WRITE, answered, began_in, gate, said_in};
use lamp_gemini::ToolCall;
use serde_json::json;
use std::time::{Duration, Instant};

const OWNER: &str = "p-0123456789abcdef";

fn fresh(on: bool) {
    on_for_test(on);
    crate::service_records::on_for_test(true);
    kept::DOWN.with(|down| down.set(false));
    kept::MEMORY.with(|memory| memory.borrow_mut().clear());
    forgot(None);
}

/// The memory's contact for `name`, if one is saved.
fn kept_for(name: &str) -> Option<String> {
    kept::MEMORY.with(|memory| {
        memory
            .borrow()
            .get(&(OWNER.to_owned(), folded(name)))
            .map(|(_, address)| address.clone())
    })
}

fn kept_count() -> usize {
    kept::MEMORY.with(|memory| memory.borrow().len())
}

fn mail(to: Value, text: &str) -> ToolCall {
    ToolCall {
        id: "c1".into(),
        name: WRITE.into(),
        args: json!({
            "service": "gmail",
            "action": "send_mail",
            "to": to,
            "subject": "Sunday",
            "text": text,
        }),
    }
}

fn send(id: &str) -> ToolCall {
    ToolCall {
        id: "c2".into(),
        name: WRITE.into(),
        args: json!({"service":"gmail","send_id":id,"said":"send"}),
    }
}

/// A turn heard at the lamp from the owner; the words only name the case
/// (the model decides whether a line confirmed).
fn heard<'a>(_words: &'a str, _previous: u64) -> Turn<'a> {
    Turn {
        heard: true,
        owner: Some(OWNER),
        sends: true,
        alike: Some(0.9),
        seen_owner: true,
        ..Turn::default()
    }
}

/// The read-back the services give a mail, as `writes::prepare` words it.
fn read_back_of(args: &Value) -> String {
    let to: Vec<String> = entries(args, "to");
    format!(
        "Send an email from me@home.test to {}. Subject: \"Sunday\". It says: \"{}\". Only the people named get it.",
        to.join(", "),
        args["text"].as_str().unwrap_or_default()
    )
}

/// What `services::run` does for a mail prepared in `request` from the
/// owner's `lines`: names filled in, the services' read-back, the contacts
/// planned, the write kept waiting and read back in full. The read-back
/// said, or what the model was told instead.
fn prepare(
    pending: &mut Option<Pending>,
    call: &ToolCall,
    send_id: &str,
    request: u64,
    lines: &[&str],
    now: Instant,
) -> Result<String, Value> {
    let mut call = call.clone();
    let filled = fill(Some(OWNER), &mut call.args)?;
    let mut told =
        json!({"sent": false, "send_id": send_id, "read_back": read_back_of(&call.args)});
    let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    plan(Some(OWNER), &call.args, &filled, &lines, &mut told);
    answered(pending, &call, request, &mut told, now, OWNER);
    let read_back = told["read_back"].as_str().unwrap().to_owned();
    said_in(pending, request, &read_back, now);
    began_in(pending, now + Duration::from_secs(20));
    Ok(read_back)
}

/// The owner's "send" in `request`, through the gate, and the service's
/// `sent:true`: what `services::run` does with a send that went.
fn confirm(pending: &mut Option<Pending>, send_id: &str, request: u64, now: Instant) {
    gate(
        pending,
        &send(send_id),
        request,
        &heard("go ahead and send it", request - 1),
        now + Duration::from_secs(21),
    )
    .expect("a timely send passes");
    sent(send_id);
}

/// Saved only after a confirmed read-back: a prepared mail is not a
/// contact; its send is.
#[test]
fn a_contact_is_saved_only_after_the_read_back_is_confirmed() {
    fresh(true);
    let now = Instant::now();
    let mut pending = None;
    let read_back = prepare(
        &mut pending,
        &mail(
            json!(["octavia@example.com"]),
            "Hi Octavia, see you Sunday.",
        ),
        "s1",
        10,
        &["Send an email to Octavia, octavia at example dot com"],
        now,
    )
    .unwrap();
    assert!(read_back.contains("octavia@example.com"));
    assert_eq!(kept_count(), 0, "a read-back alone saves nothing");
    assert_eq!(planned().len(), 1);
    confirm(&mut pending, "s1", 11, now);
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@example.com"));
    assert!(planned().is_empty());
}

/// An unconfirmed mishearing is never saved: the read-back corrected
/// ("no, it's ...") cancels the write, and nothing of it is saved, even
/// when a later send of the corrected one goes.
#[test]
fn an_unconfirmed_mishearing_is_never_saved() {
    fresh(true);
    let now = Instant::now();
    let mut pending = None;
    prepare(
        &mut pending,
        &mail(json!(["vu@example.com"]), "Hi Octavia, see you Sunday."),
        "s1",
        10,
        &["Send an email to Octavia"],
        now,
    )
    .unwrap();
    // "No, that's wrong": the lamp answers it with no send, so a send
    // after it is refused and the write is cancelled.
    said_in(&mut pending, 11, "Okay, cancelled.", now);
    let refused = gate(
        &mut pending,
        &send("s1"),
        12,
        &heard("no, that's wrong", 11),
        now + Duration::from_secs(21),
    );
    assert!(refused.is_err());
    sent("s2");
    assert_eq!(kept_count(), 0);
    assert_eq!(kept_for("Octavia"), None);
}

/// A name taken from a received mail is ignored: a reply to a mail from
/// "Bob Stone", whose name only the read and the lamp's words held, with
/// no greeting in the draft, saves no contact.
#[test]
fn a_name_from_a_received_mail_is_ignored() {
    fresh(true);
    let now = Instant::now();
    let mut pending = None;
    prepare(
        &mut pending,
        &mail(json!(["bob.stone@work.test"]), "Thanks, I'll be there."),
        "s1",
        10,
        &[
            "What's my latest mail?",
            "Reply to it saying thanks, I'll be there.",
        ],
        now,
    )
    .unwrap();
    confirm(&mut pending, "s1", 11, now);
    assert_eq!(kept_count(), 0, "no name the owner gave");
    // The owner's words name the contact; the read's sender never does.
    assert!(said_names(&["Reply to it".to_owned()]).is_empty());
    assert_eq!(
        said_names(&["send it to my friend Octavia please".to_owned()]),
        vec!["Octavia".to_owned()]
    );
    assert_eq!(greeted("Chào chị Mai, hẹn gặp."), Some("Mai".to_owned()));
    assert_eq!(greeted("Thanks, Bob Stone wrote."), None);
}

/// "Mail Octavia" fills in her saved address, and the read-back says it; a
/// name with none saved prepares nothing and asks for the address.
#[test]
fn mail_octavia_fills_in_the_address_and_the_read_back_shows_it() {
    fresh(true);
    let now = Instant::now();
    kept::exchange(
        &json!({"op":"service_contact","person":OWNER,"name":"Octavia","address":"octavia@example.com","at_unix_ms":1}),
        LOOKUP_DEADLINE,
    );
    let mut pending = None;
    let read_back = prepare(
        &mut pending,
        &mail(json!("Octavia"), "Hi Octavia, see you Sunday."),
        "s1",
        10,
        &["Mail Octavia that I'll see her Sunday"],
        now,
    )
    .unwrap();
    assert!(read_back.contains("to octavia@example.com."), "{read_back}");
    assert!(!read_back.contains("saved"), "no question: {read_back}");
    assert!(planned().is_empty(), "the same address plans nothing");
    // A name with none saved.
    let refused = prepare(
        &mut None,
        &mail(json!(["Mai"]), "Hi Mai."),
        "s2",
        12,
        &["Mail Mai"],
        now,
    )
    .unwrap_err();
    assert_eq!(refused["error"], NO_CONTACT.replace("{name}", "Mai"));
    // The memory not answering: nothing prepared, never a guess.
    kept::DOWN.with(|down| down.set(true));
    let refused = prepare(
        &mut None,
        &mail(json!(["Octavia"]), "Hi."),
        "s3",
        13,
        &[],
        now,
    )
    .unwrap_err();
    assert_eq!(refused["error"], NO_CONTACTS_NOW);
}

/// A new address for a known name: the read-back asks which one is meant,
/// and only a confirmed send replaces the saved one; a send never said
/// keeps the old.
#[test]
fn a_new_address_for_a_known_name_asks_and_replaces_only_when_sent() {
    fresh(true);
    let now = Instant::now();
    kept::exchange(
        &json!({"op":"service_contact","person":OWNER,"name":"Octavia","address":"octavia@example.com","at_unix_ms":1}),
        LOOKUP_DEADLINE,
    );
    let mut pending = None;
    let read_back = prepare(
        &mut pending,
        &mail(json!(["octavia@work.test"]), "Hi Octavia, see you Sunday."),
        "s1",
        10,
        &["Email Octavia at octavia at work dot test"],
        now,
    )
    .unwrap();
    assert!(
        read_back.contains("For Octavia I have octavia@example.com saved, not octavia@work.test"),
        "{read_back}"
    );
    // The waiting write holds the read-back with the question: it must be
    // said whole before a send.
    assert_eq!(pending.as_ref().unwrap().read_back, read_back);
    // Not sent: the old one stays.
    let _ = gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("no, the saved one", 10),
        now + Duration::from_secs(21),
    );
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@example.com"));
    // Asked again and sent: replaced.
    let mut pending = None;
    prepare(
        &mut pending,
        &mail(json!(["octavia@work.test"]), "Hi Octavia, see you Sunday."),
        "s2",
        12,
        &["Email Octavia at octavia at work dot test"],
        now,
    )
    .unwrap();
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@example.com"));
    confirm(&mut pending, "s2", 13, now);
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@work.test"));
}

/// Forget everything and a forget about take the planned contacts of a
/// waiting write, so its send saves none; the memory's own removal is the
/// memory crate's (`service_contacts::tests`). The forget about's answer
/// says how many went and that nothing sent is undone, never a name.
#[test]
fn forget_everything_and_forget_about_remove_contacts() {
    for forget in [
        (|| forgot(Some(OWNER))) as fn(),
        || forgot(None),
        || forgot_about(OWNER),
    ] {
        fresh(true);
        let now = Instant::now();
        let mut pending = None;
        prepare(
            &mut pending,
            &mail(json!(["octavia@example.com"]), "Hi Octavia,"),
            "s1",
            10,
            &[],
            now,
        )
        .unwrap();
        assert_eq!(planned().len(), 1);
        forget();
        assert!(planned().is_empty());
        confirm(&mut pending, "s1", 11, now);
        assert_eq!(kept_count(), 0);
    }
    let (told, answered) = crate::remember::contacts_forgotten(
        (json!({"forgotten": true, "removed": 0}), true),
        Some(1),
    );
    assert!(answered);
    assert_eq!(told["contacts"], 1);
    let note = told["contacts_note"].as_str().unwrap();
    assert!(note.contains("does not undo"), "{note}");
    assert!(!note.contains("Octavia"));
    let (told, _) = crate::remember::contacts_forgotten(
        (json!({"forgotten": false, "error": "nothing"}), true),
        Some(2),
    );
    assert_eq!(told["forgotten"], true);
    assert_eq!(told["contacts"], 2);
    let untouched = crate::remember::contacts_forgotten((json!({"x":1}), false), Some(0));
    assert_eq!(untouched, (json!({"x":1}), false));
}

/// With the switch off, or with the records' switch off, nothing is saved,
/// looked up or filled in: a name stays as given, the read-back as the
/// services gave it.
#[test]
fn with_the_switch_off_nothing_is_saved_or_used() {
    for records in [true, false] {
        fresh(!records);
        crate::service_records::on_for_test(records);
        assert!(!on());
        kept::MEMORY.with(|memory| {
            memory.borrow_mut().insert(
                (OWNER.to_owned(), "octavia".to_owned()),
                ("Octavia".to_owned(), "octavia@old.test".to_owned()),
            );
        });
        let mut args = mail(json!(["Octavia"]), "Hi Octavia,").args;
        assert_eq!(fill(Some(OWNER), &mut args), Ok(Vec::new()));
        assert_eq!(args["to"], json!(["Octavia"]));
        let now = Instant::now();
        let mut pending = None;
        let read_back = prepare(
            &mut pending,
            &mail(json!(["octavia@new.test"]), "Hi Octavia,"),
            "s1",
            10,
            &["Mail Octavia"],
            now,
        )
        .unwrap();
        assert!(!read_back.contains("saved"), "{read_back}");
        assert!(planned().is_empty());
        confirm(&mut pending, "s1", 11, now);
        assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@old.test"));
    }
}

/// Only the owner's: no known owner id, nothing filled in or planned.
#[test]
fn only_the_owner_has_contacts() {
    fresh(true);
    let mut args = mail(json!(["Octavia"]), "Hi Octavia,").args;
    assert_eq!(fill(None, &mut args), Ok(Vec::new()));
    assert_eq!(fill(Some("r-0123456789abcdef"), &mut args), Ok(Vec::new()));
    let mut told = json!({"send_id":"s1","read_back":"Send it."});
    let args = mail(json!(["octavia@example.com"]), "Hi Octavia,").args;
    plan(None, &args, &[], &[], &mut told);
    plan(Some("r-0123456789abcdef"), &args, &[], &[], &mut told);
    assert!(planned().is_empty());
}

/// A replay of lamp-3e83 on 10 Nov: an earlier session misheard the
/// address as "vu@example.com" and it was never confirmed; then the tester
/// spelled it, the lamp read back "octavia@example.com" and she said send.
/// The spelled, confirmed one is Octavia's contact; "vu@example.com" never
/// is; and a later "mail Octavia" needs no spelling.
#[test]
fn lamp_3e83_replay_the_confirmed_spelled_address_becomes_the_contact() {
    fresh(true);
    let now = Instant::now();
    let mut pending = None;
    let lines = ["Draft an email to my friend Octavia about the hamburger party"];
    prepare(
        &mut pending,
        &mail(
            json!(["vu@example.com"]),
            "Hi Octavia, come to the hamburger party on Sunday.",
        ),
        "s1",
        10,
        &lines,
        now,
    )
    .unwrap();
    // She spells it: a new write replaces the misheard one, never sent.
    let lines = [
        lines[0],
        "No, her address is o c t a v i a b m u at gmail dot com",
    ];
    let mut call = mail(
        json!(["octavia@example.com"]),
        "Hi Octavia, come to the hamburger party on Sunday.",
    );
    gate(
        &mut pending,
        &call,
        11,
        &heard(lines[1], 10),
        now + Duration::from_secs(21),
    )
    .expect("a new write is prepared");
    assert!(pending.is_none(), "the misheard write let go");
    let read_back = prepare(&mut pending, &call, "s2", 11, &lines, now).unwrap();
    assert!(read_back.contains("octavia@example.com"));
    assert!(!read_back.contains("vu@example.com"));
    confirm(&mut pending, "s2", 12, now);
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@example.com"));
    assert_eq!(kept_count(), 1, "vu@example.com is no one's");
    sent("s1");
    assert_eq!(kept_for("Octavia").as_deref(), Some("octavia@example.com"));
    // A minute later: "mail Octavia", no spelling.
    call = mail(json!(["Octavia"]), "Hi Octavia, it starts at 7.");
    let read_back = prepare(
        &mut None,
        &call,
        "s3",
        14,
        &["Mail Octavia it starts at 7"],
        now,
    )
    .unwrap();
    assert!(read_back.contains("to octavia@example.com."), "{read_back}");
}
