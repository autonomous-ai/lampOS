//! A person's own keys (#2094): root's helper keeps a key only once its
//! service took it, removes it on "use the lamp's own again", and never says
//! a key in a reply or a journal line.
use lamp_setup::{
    keys::{self, Check, Given, Service, Store, Verdict},
    serve::{self, SetupRequest},
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

const PLANTED: &str = "sk-planted-2094-aaaaaaaaaaaaaaaaaaaa";

fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "lamp-setup-keys-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&folder);
    folder.join("own")
}

/// A check that answers as told and remembers what it was asked.
struct Told(Verdict, Vec<Given>);

impl Check for Told {
    fn check(&mut self, given: &Given) -> Verdict {
        self.1.push(given.clone());
        self.0
    }
}

fn set(service: &str, extra: &str) -> String {
    format!(r#"{{"set":"{service}","key":"{PLANTED}"{extra}}}"#)
}

/// Neither a reply nor the journal's line holds the key.
fn never_said(reply: &Value, done: &Option<keys::Done>) {
    assert!(!reply.to_string().contains(PLANTED), "{reply}");
    assert!(!reply.to_string().contains("sk-"), "{reply}");
    if let Some(done) = done {
        assert!(!done.said.contains(PLANTED), "{}", done.said);
    }
}

#[test]
fn a_key_absent_present_and_removed() {
    let store = Store(folder("cycle"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"deepseek": "lamp", "deepgram": "lamp", "since": {"deepseek": null, "deepgram": null}, "deepseek_host": null}})
    );
    for service in ["deepseek", "deepgram"] {
        let (reply, done) = keys::answer(&set(service, ""), &store, &mut works, true);
        never_said(&reply, &done);
        let answer = reply["done"]["answer"].as_str().unwrap();
        assert_eq!(
            answer,
            match service {
                "deepseek" =>
                    "that key works: your conversations now go to api.deepseek.com with your key: your words, what the lamp remembers that's sent with them, and what it sees when asked to look",
                _ =>
                    "that key works: your speech now goes to Deepgram under your account and its retention terms",
            }
        );
        assert_eq!(reply["done"]["uses"], "own");
        assert_eq!(reply["done"]["code"], "works");
        assert_eq!(reply["done"]["service"], service);
        assert!(done.unwrap().restart_voice);
    }
    // DeepSeek's endpoint when none was given: DeepSeek's own API.
    assert_eq!(
        works.1[0].base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    for file in ["deepseek.key", "deepgram.key", "deepseek.url"] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{file}");
    }
    assert_eq!(
        fs::metadata(&store.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read_to_string(store.0.join("deepseek.key")).unwrap(),
        PLANTED
    );
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    assert_eq!(
        (&which["deepseek"], &which["deepgram"]),
        (&json!("own"), &json!("own"))
    );
    assert_eq!(which["deepseek_host"], "api.deepseek.com");
    for service in ["deepseek", "deepgram"] {
        let since = which["since"][service].as_str().unwrap();
        assert!(since.ends_with('Z') && since.len() == 20, "{since}");
    }
    never_said(&reply, &None);
    let (reply, done) = keys::answer(r#"{"remove":"deepseek"}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"service": "deepseek", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    assert!(!store.0.join("deepseek.key").exists());
    assert!(!store.0.join("deepseek.url").exists());
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    assert_eq!(
        (&which["deepseek"], &which["deepgram"]),
        (&json!("lamp"), &json!("own"))
    );
    assert_eq!(which["deepseek_host"], Value::Null);
    assert_eq!(which["since"]["deepseek"], Value::Null);
    // Removing one that is not there is done all the same.
    let (reply, _) = keys::answer(r#"{"remove":"deepseek"}"#, &store, &mut works, true);
    assert!(reply.get("done").is_some(), "{reply}");
}

#[test]
fn a_refused_or_unchecked_key_is_not_kept() {
    let store = Store(folder("refused"));
    for (verdict, words, code) in [
        (Verdict::Refused, "that key was refused", "refused"),
        (
            Verdict::Unreached,
            "the lamp couldn't reach DeepSeek to check that key; nothing changed",
            "unreached",
        ),
    ] {
        let mut check = Told(verdict, vec![]);
        let (reply, done) = keys::answer(
            &set(
                "deepseek",
                r#","base_url":"https://deepseek.example.invalid/v1/""#,
            ),
            &store,
            &mut check,
            true,
        );
        never_said(&reply, &done);
        assert_eq!(reply, json!({"refused": words, "code": code}));
        assert!(!done.unwrap().restart_voice);
        assert!(!store.0.join("deepseek.key").exists());
        assert!(!store.0.join("deepseek.url").exists());
        assert_eq!(
            check.1[0].base_url.as_deref(),
            Some("https://deepseek.example.invalid/v1")
        );
    }
    // A refused key leaves the one kept before as it was.
    let mut works = Told(Verdict::Works, vec![]);
    keys::answer(&set("deepgram", ""), &store, &mut works, true);
    let mut refused = Told(Verdict::Refused, vec![]);
    let other = r#"{"set":"deepgram","key":"dg-other-key"}"#;
    keys::answer(other, &store, &mut refused, true);
    assert_eq!(
        fs::read_to_string(store.0.join("deepgram.key")).unwrap(),
        PLANTED
    );
}

/// The reviewer's P3 2 on #2167: a key that worked but couldn't be written
/// is never said as "nothing changed". Keeping DeepSeek's takes the old key
/// away first, so the answer is `not_kept`, the lamp's own, with the old
/// key gone and the voice restarted.
#[test]
fn a_key_that_worked_but_could_not_be_written_is_not_kept() {
    let store = Store(folder("not-kept"));
    let mut works = Told(Verdict::Works, vec![]);
    let (_, done) = keys::answer(&set("deepseek", ""), &store, &mut works, true);
    assert!(done.unwrap().restart_voice);
    assert!(store.0.join("deepseek.key").is_file());
    // The endpoint's place made unwritable: a folder with a file in it,
    // which no rename can put a file over.
    fs::remove_file(store.0.join("deepseek.url")).unwrap();
    fs::create_dir_all(store.0.join("deepseek.url/held")).unwrap();
    let (reply, done) = keys::answer(
        r#"{"set":"deepseek","key":"sk-planted-2094-new"}"#,
        &store,
        &mut works,
        true,
    );
    assert!(!reply.to_string().contains("sk-"), "{reply}");
    assert_eq!(
        reply,
        json!({"refused": "the key could not be kept; the lamp uses its own now", "code": "not_kept"})
    );
    let done = done.unwrap();
    assert!(done.restart_voice, "{done:?}");
    assert!(!done.said.contains("sk-"), "{}", done.said);
    assert!(!store.0.join("deepseek.key").exists());
    assert_eq!(
        keys::answer(r#"{"which":true}"#, &store, &mut works, true).0["done"]["deepseek"],
        "lamp"
    );
}

#[test]
fn a_bad_request_is_refused_before_any_check_and_never_quoted() {
    let store = Store(folder("bad"));
    for line in [
        format!(r#"{{"set":"openai","key":"{PLANTED}"}}"#),
        format!(r#"{{"set":"deepseek","key":"{PLANTED} x"}}"#),
        format!(r#"{{"set":"deepseek","key":"{PLANTED}","base_url":"http://plain.invalid"}}"#),
        format!(r#"{{"set":"deepgram","key":"{PLANTED}","base_url":"https://x.invalid"}}"#),
        format!(r#"{{"set":"deepseek","key":"{PLANTED}","model":"x"}}"#),
        format!(r#"{{"set":"deepseek","key":"{}"}}"#, "k".repeat(513)),
        r#"{"set":"deepseek","key":""}"#.to_owned(),
        r#"{"remove":"deepseek","which":true}"#.to_owned(),
        r#"{"which":false}"#.to_owned(),
        format!("not json {PLANTED}"),
    ] {
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert!(reply.get("refused").is_some(), "{line}: {reply}");
        assert_eq!(reply["code"], "bad_field", "{line}");
        never_said(&reply, &done);
        assert!(check.1.is_empty(), "{line}");
        assert!(done.is_none());
    }
    assert!(!store.0.exists());
}

/// An endpoint with a user, a query or a fragment could carry a key past
/// its header, so it is refused before any check, in the same fixed words
/// from the helper and from setup's page, and no part of it is said back.
#[test]
fn an_endpoint_with_a_user_query_or_fragment_is_refused_and_never_quoted() {
    let store = Store(folder("endpoint"));
    for url in [
        "https://user:secret@api.deepseek.com",
        "https://api.deepseek.com/v1?api_key=sk-made-up-query-2094",
        "https://api.deepseek.com/#sk-made-up-fragment-2094",
    ] {
        assert_eq!(
            keys::fit_url(url),
            Err("the endpoint must be an https address"),
            "{url}"
        );
        let line = json!({"set": "deepseek", "key": PLANTED, "base_url": url}).to_string();
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(
            reply,
            json!({"refused": "the endpoint must be an https address", "code": "bad_field"}),
            "{url}"
        );
        never_said(&reply, &done);
        for part in [
            "user",
            "secret",
            "deepseek.com",
            "api_key",
            "made-up",
            "?",
            "#",
            "@",
        ] {
            assert!(
                !reply.to_string().contains(part),
                "{url}: {part} in {reply}"
            );
        }
        assert!(check.1.is_empty(), "{url}");
        assert!(done.is_none(), "{url}");
        // Setup's page: the same check, and the relay stays.
        let request: SetupRequest =
            serde_json::from_value(json!({"deepseek_api_key": PLANTED, "deepseek_base_url": url}))
                .unwrap();
        match serve::own_keys(&request).as_slice() {
            [Err((Service::DeepSeek, why))] => {
                assert_eq!(*why, "the endpoint must be an https address", "{url}")
            }
            other => panic!("{url}: {}", other.len()),
        }
    }
    assert!(!store.0.exists());
}

/// A plain endpoint, with a path or without, is still taken and checked.
#[test]
fn a_plain_endpoint_is_still_taken() {
    for url in ["https://api.deepseek.com", "https://api.deepseek.com/v1"] {
        assert_eq!(keys::fit_url(url).as_deref(), Ok(url));
        let store = Store(folder("plain"));
        let line = json!({"set": "deepseek", "key": PLANTED, "base_url": url}).to_string();
        let mut check = Told(Verdict::Refused, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(reply["code"], "refused", "{url}: {reply}");
        never_said(&reply, &done);
        assert_eq!(check.1.len(), 1, "{url}");
        assert_eq!(check.1[0].base_url.as_deref(), Some(url));
        let request: SetupRequest =
            serde_json::from_value(json!({"deepseek_api_key": PLANTED, "deepseek_base_url": url}))
                .unwrap();
        match serve::own_keys(&request).as_slice() {
            [Ok(given)] => assert_eq!(given.base_url.as_deref(), Some(url)),
            other => panic!("{url}: {}", other.len()),
        }
    }
}

/// Off: nothing is checked, kept or read.
#[test]
fn with_the_switch_off_nothing_changes() {
    let store = Store(folder("off"));
    let mut check = Told(Verdict::Works, vec![]);
    for line in [set("deepseek", ""), r#"{"which":true}"#.into()] {
        let (reply, done) = keys::answer(&line, &store, &mut check, false);
        assert_eq!(
            reply,
            json!({"refused": "own keys are off on this lamp", "code": "unavailable"})
        );
        assert!(done.is_none());
    }
    assert!(check.1.is_empty());
    assert!(!store.0.exists());
    assert!(keys::on(Some("on")));
    for off in [None, Some(""), Some("1"), Some("ON")] {
        assert!(!keys::on(off));
    }
}

/// `Debug` hides the key.
#[test]
fn a_given_key_is_never_shown_by_debug() {
    let given = keys::given(Service::Deepgram, PLANTED, None).unwrap();
    assert!(!format!("{given:?}").contains(PLANTED));
}

/// Only `lamp-cloud`'s processes may ask (Third's privacy read): by the
/// cgroup only root can put a process in, never by the user all parts share.
#[test]
fn only_lamp_clouds_processes_are_the_asker() {
    for cgroup in [
        "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service\n",
        "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service/worker\n",
        "12:cpu:/x\n1:name=systemd:/lampos.slice/lampos-parts.slice/lamp-cloud.service\n0::/\n",
    ] {
        assert!(keys::from_asker(cgroup), "{cgroup}");
    }
    for cgroup in [
        "0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n",
        "0::/system.slice/lamp-cloud.service.d\n",
        // Only the system's place for it (the reviewer's P3 1 on #2167): not
        // a lamp-cloud.service a user's own manager made, nor one elsewhere.
        "0::/user.slice/user-1001.slice/user@1001.service/app.slice/lamp-cloud.service\n",
        "0::/system.slice/lamp-cloud.service\n",
        "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service2\n",
        "0::/lampos.slice/lampos-parts.slice/x.service/lamp-cloud.service\n",
        "0::/system.slice/not-lamp-cloud.service\n",
        "0::/user.slice/user-1000.slice/lamp-cloud.services\n",
        "4:memory:/system.slice/lamp-cloud.service\n0::/system.slice/lamp-eyes.service\n",
        "",
        "garbage",
    ] {
        assert!(!keys::from_asker(cgroup), "{cgroup}");
    }
}

/// The helper's units: its socket, root with the network for the one
/// check, writing only the keys' folder, which the reset plan wipes whole,
/// and lamp-update's lock.
#[test]
fn the_helpers_units_are_as_the_helper_reads_them() {
    let unit = include_str!("../../../../deploy/product/lamp-setup-keys@.service");
    assert!(unit.contains(&format!(
        "lamp-setup keys --folder {} --bus-root /run/lampos-bus --enable\n",
        keys::FOLDER
    )));
    assert!(unit.contains("User=root\n"));
    assert!(unit.contains("ConfigurationDirectory=lampOS/voice/own\n"));
    assert!(unit.contains("ConfigurationDirectoryMode=0700\n"));
    assert!(unit.contains("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6\n"));
    assert!(!unit.contains("PrivateNetwork=yes"));
    assert!(!unit.contains("ReadWritePaths=/etc"));
    // lamp-update's records, only for the lock held through the voice's
    // restart (`change::quiet_and_unlocked`).
    assert!(unit.contains("ReadWritePaths=/run/lampos-bus -/var/lib/lampos-update\n"));
    let socket = include_str!("../../../../deploy/product/lamp-setup-keys.socket");
    assert!(socket.contains("ListenStream=/run/lampos-setup-keys/keys.sock\n"));
    assert!(socket.contains("SocketMode=0660\n"));
    assert_eq!(keys::ASKER_UNIT, "lamp-cloud.service");
}

/// The reviewer's P3 (15:07): the asker's cgroup is where the product unit
/// puts `lamp-cloud`. systemd nests a dashed slice under each of its
/// prefixes, so a moved `Slice=` would refuse every `keys.*` quietly.
#[test]
fn the_asker_cgroup_is_the_product_units_slice() {
    const UNIT: &str = include_str!("../../../../deploy/product/lamp-cloud.service");
    let slice = UNIT
        .lines()
        .find_map(|line| line.strip_prefix("Slice="))
        .expect("lamp-cloud.service names its slice")
        .trim();
    let name = slice.strip_suffix(".slice").unwrap();
    let mut path = String::new();
    for (at, _) in name.match_indices('-').chain([(name.len(), "")]) {
        path.push_str(&format!("/{}.slice", &name[..at]));
    }
    assert_eq!(format!("{path}/lamp-cloud.service"), keys::ASKER_CGROUP);
}
