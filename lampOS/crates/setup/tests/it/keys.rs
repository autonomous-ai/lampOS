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
    // With #8367's and #8362's fields beside today's (`offers` is pinned
    // below).
    let none = json!({"deepseek": null, "deepgram": null, "elevenlabs": null, "openai_compatible": null, "openrouter": null, "gemini": null, "anthropic": null});
    assert_eq!(
        reply,
        json!({"done": {"deepseek": "lamp", "deepgram": "lamp", "elevenlabs": "lamp", "openai_compatible": "lamp", "openrouter": "lamp", "gemini": "lamp", "anthropic": "lamp", "since": none, "deepseek_host": null, "voice_id": {"elevenlabs": null}, "openai_compatible_host": null, "model": none, "checked": none, "in_use": {"model": null, "agent": "relay", "agent_why": "no_model_key"}, "offers": reply["done"]["offers"]}})
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
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    assert_eq!(
        fs::metadata(&store.0).unwrap().permissions().mode() & 0o777,
        0o750
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

/// The unplug's turn-off (#5747): lamp-skills' processes are the change
/// helper's one other peer, by their cgroup only, never lamp-cloud's as
/// it nor it as lamp-cloud.
#[test]
fn only_lamp_skills_processes_are_the_turn_off_asker() {
    for cgroup in [
        "0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n",
        "1:name=systemd:/lampos.slice/lampos-parts.slice/lamp-skills.service\n0::/\n",
    ] {
        assert!(keys::from_turn_off_asker(cgroup), "{cgroup}");
        assert!(!keys::from_asker(cgroup), "{cgroup}");
    }
    for cgroup in [
        "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service\n",
        "0::/system.slice/lamp-skills.service\n",
        "0::/user.slice/user-1001.slice/user@1001.service/app.slice/lamp-skills.service\n",
        "0::/lampos.slice/lampos-parts.slice/lamp-skills.service2\n",
        "0::/lampos.slice/lampos-voice.slice/lamp-voice.service\n",
        "",
    ] {
        assert!(!keys::from_turn_off_asker(cgroup), "{cgroup}");
    }
    // Where the product unit puts lamp-skills, as for lamp-cloud above.
    const UNIT: &str = include_str!("../../../../deploy/product/lamp-skills.service");
    assert!(UNIT.contains("\nSlice=lampos-parts.slice\n"));
    assert_eq!(
        keys::TURN_OFF_CGROUP,
        format!("/lampos.slice/lampos-parts.slice/{}", keys::TURN_OFF_UNIT)
    );
    // It asks on the change helper's socket.
    let socket = include_str!("../../../../deploy/product/lamp-setup-change.socket");
    let path = socket
        .lines()
        .find_map(|line| line.strip_prefix("ListenStream="))
        .unwrap();
    assert!(UNIT.contains(&format!("\nEnvironment=LAMP_UNPLUG_OFF_SOCKET={path}\n")));
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
    assert!(unit.contains("ConfigurationDirectoryMode=0750\n"));
    // #8735: systemd leaves an existing folder's mode and group, so the
    // product's tmpfiles bring a folder kept before to root:lampos 0750, 0640.
    let tmpfiles = include_str!("../../../../deploy/product/lampos.conf");
    for line in [
        "z /etc/lampOS/voice/own 0750 root lampos -",
        "z /etc/lampOS/voice/own/* 0640 root lampos -",
    ] {
        assert!(tmpfiles.lines().any(|have| have == line), "{line}");
    }
    for name in ["lamp-voice.service", "own-keys-voice.conf"] {
        let text = fs::read_to_string(format!(
            "{}/../../deploy/product/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        assert!(text.contains("ConfigurationDirectoryMode=0750\n"), "{name}");
    }
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

const VOICE: &str = "PersonVoice8367abcdef";

/// #8367: `keys.get` lists what the lamp takes a person's key for, the
/// model services and the voice apart (`kind`), and the voice ID in use;
/// the key itself never comes back.
#[test]
fn elevenlabs_is_offered_beside_the_model_services() {
    let store = Store(folder("offers"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    assert_eq!(
        which["offers"],
        json!([
            {"service": "deepseek", "name": "DeepSeek", "kind": "model", "models": [], "base_url": true, "fields": [], "agent": true},
            {"service": "deepgram", "name": "Deepgram", "kind": "model", "models": [], "base_url": false, "fields": [], "agent": false},
            {"service": "elevenlabs", "name": "ElevenLabs", "kind": "voice", "models": [], "base_url": false, "fields": ["voice_id"], "agent": false},
            {"service": "openai_compatible", "name": "OpenAI-compatible", "kind": "model", "models": [], "base_url": true, "fields": ["model"], "agent": true},
            {"service": "openrouter", "name": "OpenRouter", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://openrouter.ai/api/v1", "agent": true},
            {"service": "gemini", "name": "Gemini", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://generativelanguage.googleapis.com/v1beta/openai", "agent": true},
            {"service": "anthropic", "name": "Anthropic", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://api.anthropic.com/v1", "agent": true},
        ])
    );
    assert_eq!(which["elevenlabs"], "lamp");
    assert_eq!(which["voice_id"], json!({"elevenlabs": null}));
    // Today's fields stay as they are, for an older app.
    assert_eq!(which["deepseek"], "lamp");
    assert_eq!(which["deepgram"], "lamp");
}

/// #8367: a key and voice ID that ElevenLabs takes are kept beside the
/// others, 0600, said in the lamp's own words, and the voice restarts to
/// speak in it from the next thing it says; neither the reply nor the
/// journal's line holds the key.
#[test]
fn an_elevenlabs_key_and_voice_are_checked_kept_and_never_said() {
    let store = Store(folder("voice"));
    let mut works = Told(Verdict::Works, vec![]);
    let line = set("elevenlabs", &format!(r#","voice_id":"{VOICE}""#));
    let (reply, done) = keys::answer(&line, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(
        reply,
        json!({"done": {"service": "elevenlabs", "uses": "own", "code": "works", "answer":
            "that key and voice work: the lamp now speaks in your ElevenLabs voice, and what it says goes to ElevenLabs under your account and its terms"}})
    );
    let done = done.unwrap();
    assert!(done.restart_voice);
    assert_eq!(done.said, "the person's ElevenLabs key: it works, kept");
    assert_eq!(works.1.len(), 1, "one check");
    assert_eq!(
        fs::read_to_string(store.0.join("elevenlabs.key")).unwrap(),
        PLANTED
    );
    assert_eq!(
        fs::read_to_string(store.0.join("elevenlabs.voice")).unwrap(),
        format!("{VOICE}\n")
    );
    for file in ["elevenlabs.key", "elevenlabs.voice"] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    never_said(&reply, &None);
    assert_eq!(reply["done"]["elevenlabs"], "own");
    assert_eq!(reply["done"]["voice_id"], json!({"elevenlabs": VOICE}));
    // "Use the lamp's own again": both files gone, the lamp's voice back.
    let (reply, done) = keys::answer(r#"{"remove":"elevenlabs"}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"service": "elevenlabs", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    assert!(!store.0.join("elevenlabs.key").exists());
    assert!(!store.0.join("elevenlabs.voice").exists());
}

/// #8367: with no `key`, `keys.set` changes only the voice, checked with
/// the kept key, and only when one is kept; a refused key leaves the one
/// kept before.
#[test]
fn a_voice_alone_changes_only_the_voice_beside_a_kept_key() {
    let store = Store(folder("voice-alone"));
    let mut works = Told(Verdict::Works, vec![]);
    let alone = format!(r#"{{"set":"elevenlabs","voice_id":"{VOICE}"}}"#);
    let (reply, done) = keys::answer(&alone, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"refused": "a voice ID is changed beside a kept ElevenLabs key: give the key too", "code": "bad_field"})
    );
    assert!(done.is_none());
    assert!(works.1.is_empty(), "nothing checked");
    let first = set("elevenlabs", r#","voice_id":"FirstVoice8367""#);
    let (reply, _) = keys::answer(&first, &store, &mut works, true);
    assert_eq!(reply["done"]["code"], "works", "{reply}");
    let (reply, done) = keys::answer(&alone, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["code"], "works", "{reply}");
    assert!(done.unwrap().restart_voice);
    assert_eq!(works.1.len(), 2, "the new voice checked with the kept key");
    assert_eq!(
        fs::read_to_string(store.0.join("elevenlabs.key")).unwrap(),
        PLANTED
    );
    assert_eq!(
        fs::read_to_string(store.0.join("elevenlabs.voice")).unwrap(),
        format!("{VOICE}\n")
    );
    // A voice ID is letters and digits only, never quoted back.
    let bad = format!(r#"{{"set":"elevenlabs","key":"{PLANTED}","voice_id":"../x"}}"#);
    let (reply, _) = keys::answer(&bad, &store, &mut works, true);
    assert_eq!(reply["code"], "bad_field");
    assert!(!reply.to_string().contains("../x"), "{reply}");
    never_said(&reply, &None);
}

/// #8367: off, an ElevenLabs line changes nothing, as every other.
#[test]
fn with_the_switch_off_an_elevenlabs_key_changes_nothing() {
    let store = Store(folder("voice-off"));
    let mut check = Told(Verdict::Works, vec![]);
    let line = set("elevenlabs", &format!(r#","voice_id":"{VOICE}""#));
    let (reply, done) = keys::answer(&line, &store, &mut check, false);
    assert_eq!(
        reply,
        json!({"refused": "own keys are off on this lamp", "code": "unavailable"})
    );
    assert!(done.is_none());
    assert!(check.1.is_empty());
    assert!(!store.0.exists());
}

/// #8362 step 1b: a person's own model at an OpenAI-compatible endpoint (Grok
/// and the like), on the same `keys.*` path. `offers` names it with
/// `base_url: true` and `fields: ["model"]` (#8367's shape); its key, endpoint and model are kept as three private
/// files once the check passes; `keys.get` gives the endpoint's host and the
/// model, never the key; and one own model is kept at a time, so the model
/// in use is never a guess.
#[test]
fn an_openai_compatible_model_is_kept_with_its_endpoint_and_model() {
    let store = Store(folder("compatible"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    let offers = which["offers"].as_array().expect("offers");
    let compatible = offers
        .iter()
        .find(|offer| offer["service"] == "openai_compatible")
        .expect("openai_compatible offered");
    assert_eq!(
        compatible,
        &json!({"service": "openai_compatible", "name": "OpenAI-compatible", "kind": "model", "models": [], "base_url": true, "fields": ["model"], "agent": true})
    );
    assert_eq!(which["openai_compatible"], "lamp");
    assert_eq!(which["model"]["openai_compatible"], Value::Null);
    assert_eq!(
        which["in_use"],
        json!({"model": null, "agent": "relay", "agent_why": "no_model_key"})
    );

    // DeepSeek's own first, then the OpenAI-compatible one over it.
    keys::answer(&set("deepseek", ""), &store, &mut works, true);
    let line = json!({"set": "openai_compatible", "key": PLANTED, "base_url": "https://api.compatible.invalid/v1/", "model": "grok-4"}).to_string();
    let (reply, done) = keys::answer(&line, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["service"], "openai_compatible");
    assert_eq!(reply["done"]["uses"], "own");
    assert_eq!(reply["done"]["code"], "works");
    let answer = reply["done"]["answer"].as_str().unwrap();
    assert!(
        answer.starts_with(
            "that key works: your conversations now go to api.compatible.invalid with your key"
        ),
        "{answer}"
    );
    assert!(done.unwrap().restart_voice);
    let asked = works.1.last().unwrap();
    assert_eq!(
        asked.base_url.as_deref(),
        Some("https://api.compatible.invalid/v1")
    );
    for file in [
        "openai_compatible.key",
        "openai_compatible.url",
        "openai_compatible.model",
    ] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap_or_else(|error| panic!("{file}: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    assert_eq!(
        fs::read_to_string(store.0.join("openai_compatible.model"))
            .unwrap()
            .trim(),
        "grok-4"
    );
    // One own model at a time: DeepSeek's own key was taken away.
    assert!(!store.0.join("deepseek.key").exists());
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    never_said(&reply, &None);
    assert_eq!(which["openai_compatible"], "own");
    assert_eq!(which["deepseek"], "lamp");
    assert_eq!(which["openai_compatible_host"], "api.compatible.invalid");
    assert_eq!(which["model"]["openai_compatible"], "grok-4");
    assert_eq!(which["checked"]["openai_compatible"], "works");
    assert_eq!(
        which["in_use"],
        json!({"model": "openai_compatible", "agent": "own", "agent_why": null})
    );
    // Removed: the lamp's own model again, and nothing of it left.
    let (reply, done) = keys::answer(
        r#"{"remove":"openai_compatible"}"#,
        &store,
        &mut works,
        true,
    );
    assert_eq!(
        reply,
        json!({"done": {"service": "openai_compatible", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    for file in [
        "openai_compatible.key",
        "openai_compatible.url",
        "openai_compatible.model",
    ] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    assert_eq!(
        reply["done"]["in_use"],
        json!({"model": null, "agent": "relay", "agent_why": "no_model_key"})
    );
}

/// An OpenAI-compatible key needs its https endpoint and a model, each
/// refused plainly before any check, with no part of the key, the endpoint or
/// the model said back.
#[test]
fn an_openai_compatible_key_needs_an_https_endpoint_and_a_model() {
    let store = Store(folder("compatible-bad"));
    for (line, words) in [
        (
            json!({"set": "openai_compatible", "key": PLANTED, "base_url": "http://api.compatible.invalid/v1", "model": "grok-4"}),
            "the endpoint must be an https address",
        ),
        (
            json!({"set": "openai_compatible", "key": PLANTED, "base_url": "not a url", "model": "grok-4"}),
            "the endpoint must be an https address",
        ),
        (
            json!({"set": "openai_compatible", "key": PLANTED, "model": "grok-4"}),
            "an OpenAI-compatible key needs its endpoint",
        ),
        (
            json!({"set": "openai_compatible", "key": PLANTED, "base_url": "https://api.compatible.invalid/v1"}),
            "an OpenAI-compatible key needs its model",
        ),
        (
            json!({"set": "openai_compatible", "key": PLANTED, "base_url": "https://api.compatible.invalid/v1", "model": "grok 4; drop"}),
            "a model name is letters, digits, '-', '.', '_', '/' and ':'",
        ),
    ] {
        let line = line.to_string();
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(
            reply,
            json!({"refused": words, "code": "bad_field"}),
            "{line}"
        );
        never_said(&reply, &done);
        for part in ["compatible.invalid", "grok", "drop"] {
            assert!(!reply.to_string().contains(part), "{line}: {part}");
        }
        assert!(check.1.is_empty(), "{line}");
        assert!(done.is_none(), "{line}");
    }
    assert!(!store.0.exists());
}

/// #8420: a person's own OpenRouter key and model, by name. `offers` gives
/// OpenRouter's fixed endpoint for the app to show (`base_url: false`, so
/// the key only ever goes to OpenRouter); an endpoint sent with it is
/// refused before any check. Its key and model are kept as private files,
/// `keys.get` names the model and `in_use.model`, and keeping it takes the
/// other own models away.
#[test]
fn an_openrouter_model_is_kept_with_its_model_at_openrouters_own_endpoint() {
    let store = Store(folder("openrouter"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    let offer = which["offers"]
        .as_array()
        .expect("offers")
        .iter()
        .find(|offer| offer["service"] == "openrouter")
        .expect("openrouter offered")
        .clone();
    assert_eq!(
        offer,
        json!({"service": "openrouter", "name": "OpenRouter", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://openrouter.ai/api/v1", "agent": true})
    );
    assert_eq!(which["openrouter"], "lamp");
    assert_eq!(which["model"]["openrouter"], Value::Null);

    // An endpoint of the person's, or no model: refused before any check,
    // nothing of either said back.
    for (line, words) in [
        (
            json!({"set": "openrouter", "key": PLANTED, "base_url": "https://elsewhere.invalid/v1", "model": "openai/gpt-4o-mini"}),
            "OpenRouter takes no endpoint",
        ),
        (
            json!({"set": "openrouter", "key": PLANTED}),
            "an OpenRouter key needs its model",
        ),
        (
            json!({"set": "openrouter", "key": PLANTED, "model": "gpt 4; drop"}),
            "a model name is letters, digits, '-', '.', '_', '/' and ':'",
        ),
    ] {
        let line = line.to_string();
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(
            reply,
            json!({"refused": words, "code": "bad_field"}),
            "{line}"
        );
        never_said(&reply, &done);
        assert!(!reply.to_string().contains("elsewhere"), "{line}");
        assert!(check.1.is_empty(), "{line}");
    }
    assert!(!store.0.exists());

    // DeepSeek's own and an OpenAI-compatible one first, then OpenRouter's.
    keys::answer(&set("deepseek", ""), &store, &mut works, true);
    let compatible = json!({"set": "openai_compatible", "key": PLANTED, "base_url": "https://api.compatible.invalid/v1", "model": "grok-4"}).to_string();
    keys::answer(&compatible, &store, &mut works, true);
    let line =
        json!({"set": "openrouter", "key": PLANTED, "model": "openai/gpt-4o-mini"}).to_string();
    let (reply, done) = keys::answer(&line, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["service"], "openrouter");
    assert_eq!(reply["done"]["code"], "works");
    let answer = reply["done"]["answer"].as_str().unwrap();
    assert!(
        answer.starts_with(
            "that key works: your conversations now go to openrouter.ai with your key"
        ),
        "{answer}"
    );
    assert!(done.unwrap().restart_voice);
    // The check went to OpenRouter's own endpoint, with the model.
    let asked = works.1.last().unwrap();
    assert_eq!(
        asked.base_url.as_deref(),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(asked.model.as_deref(), Some("openai/gpt-4o-mini"));
    for file in ["openrouter.key", "openrouter.model", "openrouter.checked"] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap_or_else(|error| panic!("{file}: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    // No endpoint file: OpenRouter's is fixed.
    assert!(!store.0.join("openrouter.url").exists());
    // One own model at a time: the other two are gone, with their files.
    for file in [
        "deepseek.key",
        "deepseek.url",
        "openai_compatible.key",
        "openai_compatible.url",
        "openai_compatible.model",
    ] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    never_said(&reply, &None);
    assert_eq!(which["openrouter"], "own");
    assert_eq!(
        (&which["deepseek"], &which["openai_compatible"]),
        (&json!("lamp"), &json!("lamp"))
    );
    assert_eq!(which["model"]["openrouter"], "openai/gpt-4o-mini");
    assert_eq!(which["checked"]["openrouter"], "works");
    assert_eq!(
        which["in_use"],
        json!({"model": "openrouter", "agent": "own", "agent_why": null})
    );
    // An OpenAI-compatible one over it takes OpenRouter's away in turn.
    keys::answer(&compatible, &store, &mut works, true);
    for file in ["openrouter.key", "openrouter.model", "openrouter.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    assert_eq!(
        reply["done"]["in_use"],
        json!({"model": "openai_compatible", "agent": "own", "agent_why": null})
    );
    // Removed: nothing of it left.
    keys::answer(&line, &store, &mut works, true);
    let (reply, done) = keys::answer(r#"{"remove":"openrouter"}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"service": "openrouter", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    for file in ["openrouter.key", "openrouter.model", "openrouter.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
}

/// #8420: a person's own Gemini key and model, by name. `offers` gives
/// Gemini's fixed OpenAI-compatible endpoint for the app to show
/// (`base_url: false`); an endpoint sent with it is refused before any
/// check. Its key and model are kept as private files, `keys.get` names the
/// model and `in_use.model`, and keeping it takes the other own models
/// away, OpenRouter's among them.
#[test]
fn a_gemini_model_is_kept_with_its_model_at_geminis_own_endpoint() {
    let store = Store(folder("gemini"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    let offer = which["offers"]
        .as_array()
        .expect("offers")
        .iter()
        .find(|offer| offer["service"] == "gemini")
        .expect("gemini offered")
        .clone();
    assert_eq!(
        offer,
        json!({"service": "gemini", "name": "Gemini", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://generativelanguage.googleapis.com/v1beta/openai", "agent": true})
    );
    assert_eq!(which["gemini"], "lamp");
    assert_eq!(which["model"]["gemini"], Value::Null);

    // An endpoint of the person's, or no model: refused before any check,
    // nothing of either said back.
    for (line, words) in [
        (
            json!({"set": "gemini", "key": PLANTED, "base_url": "https://elsewhere.invalid/v1", "model": "gemini-2.5-flash"}),
            "Gemini takes no endpoint",
        ),
        (
            json!({"set": "gemini", "key": PLANTED}),
            "a Gemini key needs its model",
        ),
    ] {
        let line = line.to_string();
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(
            reply,
            json!({"refused": words, "code": "bad_field"}),
            "{line}"
        );
        never_said(&reply, &done);
        assert!(!reply.to_string().contains("elsewhere"), "{line}");
        assert!(check.1.is_empty(), "{line}");
    }
    assert!(!store.0.exists());

    // OpenRouter's first, then Gemini's.
    let openrouter =
        json!({"set": "openrouter", "key": PLANTED, "model": "openai/gpt-4o-mini"}).to_string();
    keys::answer(&openrouter, &store, &mut works, true);
    let line = json!({"set": "gemini", "key": PLANTED, "model": "gemini-2.5-flash"}).to_string();
    let (reply, done) = keys::answer(&line, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["service"], "gemini");
    assert_eq!(reply["done"]["code"], "works");
    let answer = reply["done"]["answer"].as_str().unwrap();
    assert!(
        answer.starts_with(
            "that key works: your conversations now go to generativelanguage.googleapis.com with your key"
        ),
        "{answer}"
    );
    assert!(done.unwrap().restart_voice);
    let asked = works.1.last().unwrap();
    assert_eq!(
        asked.base_url.as_deref(),
        Some("https://generativelanguage.googleapis.com/v1beta/openai")
    );
    assert_eq!(asked.model.as_deref(), Some("gemini-2.5-flash"));
    for file in ["gemini.key", "gemini.model", "gemini.checked"] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap_or_else(|error| panic!("{file}: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    assert!(!store.0.join("gemini.url").exists());
    for file in ["openrouter.key", "openrouter.model", "openrouter.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    never_said(&reply, &None);
    assert_eq!(which["gemini"], "own");
    assert_eq!(which["openrouter"], "lamp");
    assert_eq!(which["model"]["gemini"], "gemini-2.5-flash");
    assert_eq!(which["checked"]["gemini"], "works");
    assert_eq!(
        which["in_use"],
        json!({"model": "gemini", "agent": "own", "agent_why": null})
    );
    // OpenRouter's over it takes Gemini's away in turn.
    keys::answer(&openrouter, &store, &mut works, true);
    for file in ["gemini.key", "gemini.model", "gemini.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    // Removed: nothing of it left.
    keys::answer(&line, &store, &mut works, true);
    let (reply, done) = keys::answer(r#"{"remove":"gemini"}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"service": "gemini", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    for file in ["gemini.key", "gemini.model", "gemini.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
}

/// #8420: a person's own Anthropic key and model, by name. `offers` gives
/// Anthropic's fixed API for the app to show (`base_url: false`); an
/// endpoint sent with it is refused before any check. Its key and model are
/// kept as private files, `keys.get` names the model and `in_use.model`,
/// and keeping it takes the other own models away, Gemini's among them.
#[test]
fn an_anthropic_model_is_kept_with_its_model_at_anthropics_own_endpoint() {
    let store = Store(folder("anthropic"));
    let mut works = Told(Verdict::Works, vec![]);
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    let offer = which["offers"]
        .as_array()
        .expect("offers")
        .iter()
        .find(|offer| offer["service"] == "anthropic")
        .expect("anthropic offered")
        .clone();
    assert_eq!(
        offer,
        json!({"service": "anthropic", "name": "Anthropic", "kind": "model", "models": [], "base_url": false, "fields": ["model"], "endpoint": "https://api.anthropic.com/v1", "agent": true})
    );
    assert_eq!(which["anthropic"], "lamp");
    assert_eq!(which["model"]["anthropic"], Value::Null);

    // An endpoint of the person's, or no model: refused before any check,
    // nothing of either said back.
    for (line, words) in [
        (
            json!({"set": "anthropic", "key": PLANTED, "base_url": "https://elsewhere.invalid/v1", "model": "claude-sonnet-4-5"}),
            "Anthropic takes no endpoint",
        ),
        (
            json!({"set": "anthropic", "key": PLANTED}),
            "an Anthropic key needs its model",
        ),
    ] {
        let line = line.to_string();
        let mut check = Told(Verdict::Works, vec![]);
        let (reply, done) = keys::answer(&line, &store, &mut check, true);
        assert_eq!(
            reply,
            json!({"refused": words, "code": "bad_field"}),
            "{line}"
        );
        never_said(&reply, &done);
        assert!(!reply.to_string().contains("elsewhere"), "{line}");
        assert!(check.1.is_empty(), "{line}");
    }
    assert!(!store.0.exists());

    // Gemini's first, then Anthropic's.
    let gemini = json!({"set": "gemini", "key": PLANTED, "model": "gemini-2.5-flash"}).to_string();
    keys::answer(&gemini, &store, &mut works, true);
    let line =
        json!({"set": "anthropic", "key": PLANTED, "model": "claude-sonnet-4-5"}).to_string();
    let (reply, done) = keys::answer(&line, &store, &mut works, true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["service"], "anthropic");
    assert_eq!(reply["done"]["code"], "works");
    let answer = reply["done"]["answer"].as_str().unwrap();
    assert!(
        answer.starts_with(
            "that key works: your conversations now go to api.anthropic.com with your key"
        ),
        "{answer}"
    );
    assert!(done.unwrap().restart_voice);
    let asked = works.1.last().unwrap();
    assert_eq!(
        asked.base_url.as_deref(),
        Some("https://api.anthropic.com/v1")
    );
    assert_eq!(asked.model.as_deref(), Some("claude-sonnet-4-5"));
    for file in ["anthropic.key", "anthropic.model", "anthropic.checked"] {
        let mode = fs::metadata(store.0.join(file))
            .unwrap_or_else(|error| panic!("{file}: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "{file}");
    }
    assert!(!store.0.join("anthropic.url").exists());
    for file in ["gemini.key", "gemini.model", "gemini.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    let (reply, _) = keys::answer(r#"{"which":true}"#, &store, &mut works, true);
    let which = &reply["done"];
    never_said(&reply, &None);
    assert_eq!(which["anthropic"], "own");
    assert_eq!(which["gemini"], "lamp");
    assert_eq!(which["model"]["anthropic"], "claude-sonnet-4-5");
    assert_eq!(which["checked"]["anthropic"], "works");
    assert_eq!(
        which["in_use"],
        json!({"model": "anthropic", "agent": "own", "agent_why": null})
    );
    // Gemini's over it takes Anthropic's away in turn.
    keys::answer(&gemini, &store, &mut works, true);
    for file in ["anthropic.key", "anthropic.model", "anthropic.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
    // Removed: nothing of it left.
    keys::answer(&line, &store, &mut works, true);
    let (reply, done) = keys::answer(r#"{"remove":"anthropic"}"#, &store, &mut works, true);
    assert_eq!(
        reply,
        json!({"done": {"service": "anthropic", "uses": "lamp"}})
    );
    assert!(done.unwrap().restart_voice);
    for file in ["anthropic.key", "anthropic.model", "anthropic.checked"] {
        assert!(!store.0.join(file).exists(), "{file}");
    }
}

/// #8735: what the agent uses, from the tool check at `keys.set`. A model
/// key whose chat made a tool call serves the agent; one whose chat made
/// none is kept for the voice, while the agent stays on the relay and says
/// why; a voice key alone, and no key, leave it on the relay too.
#[test]
fn the_agent_uses_an_own_model_key_only_once_it_made_a_tool_call() {
    let store = Store(folder("agent"));
    let which = |store: &Store| {
        let (reply, _) = keys::answer(
            r#"{"which":true}"#,
            store,
            &mut Told(Verdict::Works, vec![]),
            true,
        );
        never_said(&reply, &None);
        // Under the 4 096 bytes lamp-cloud reads of a reply.
        assert!(
            reply.to_string().len() < 4_096,
            "{}",
            reply.to_string().len()
        );
        reply["done"].clone()
    };
    let offer = |which: &Value, service: &str| {
        which["offers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|offer| offer["service"] == service)
            .unwrap()["agent"]
            .clone()
    };
    let gemini = json!({"set": "gemini", "key": PLANTED, "model": "gemini-3.6-flash"}).to_string();

    // Tools work: the agent's own.
    let (reply, done) = keys::answer(&gemini, &store, &mut Told(Verdict::Works, vec![]), true);
    never_said(&reply, &done);
    assert_eq!(reply["done"]["code"], "works");
    let now = which(&store);
    assert_eq!(
        now["in_use"],
        json!({"model": "gemini", "agent": "own", "agent_why": null})
    );
    assert_eq!(offer(&now, "gemini"), true);

    // A chat with no tool call: kept, the voice on it, the agent on the relay.
    let (reply, done) = keys::answer(
        &gemini,
        &store,
        &mut Told(Verdict::WorksNoTools, vec![]),
        true,
    );
    never_said(&reply, &done);
    assert_eq!(reply["done"]["uses"], "own");
    assert_eq!(reply["done"]["code"], "works_no_tools");
    let answer = reply["done"]["answer"].as_str().unwrap();
    assert!(
        answer.starts_with("that key works for the lamp's voice: your conversations now go to"),
        "{answer}"
    );
    assert!(
        answer.ends_with("so the lamp's agent stays on the lamp's own model"),
        "{answer}"
    );
    let done = done.unwrap();
    assert!(done.restart_voice);
    assert!(
        done.said.contains("made no tool call, kept"),
        "{}",
        done.said
    );
    assert!(store.0.join("gemini.key").is_file());
    let now = which(&store);
    assert_eq!(now["gemini"], "own");
    assert_eq!(now["checked"]["gemini"], "works");
    assert_eq!(
        now["in_use"],
        json!({"model": "gemini", "agent": "relay", "agent_why": "no_tool_calls"})
    );
    assert_eq!(offer(&now, "gemini"), false);
    assert_eq!(offer(&now, "openrouter"), true);

    // Removed: no model key, the relay.
    keys::answer(
        r#"{"remove":"gemini"}"#,
        &store,
        &mut Told(Verdict::Works, vec![]),
        true,
    );
    let now = which(&store);
    assert_eq!(
        now["in_use"],
        json!({"model": null, "agent": "relay", "agent_why": "no_model_key"})
    );
    assert_eq!(offer(&now, "gemini"), true);

    // An ElevenLabs key alone: the voice, never the agent.
    let voice = json!({"set": "elevenlabs", "key": PLANTED, "voice_id": "Voice8735"}).to_string();
    let (reply, _) = keys::answer(&voice, &store, &mut Told(Verdict::Works, vec![]), true);
    assert_eq!(reply["done"]["code"], "works");
    let now = which(&store);
    assert_eq!(
        now["in_use"],
        json!({"model": null, "agent": "relay", "agent_why": "tts_only"})
    );
    assert_eq!(offer(&now, "elevenlabs"), false);
    assert_eq!(offer(&now, "deepgram"), false);

    // A model key kept before the tool check: the relay until set again.
    fs::write(store.0.join("deepseek.key"), PLANTED).unwrap();
    fs::write(store.0.join("deepseek.checked"), "works").unwrap();
    assert_eq!(
        which(&store)["in_use"],
        json!({"model": "deepseek", "agent": "relay", "agent_why": "unchecked"})
    );
    let _ = fs::remove_dir_all(store.0.parent().unwrap());
}

/// #8735: the folder is root:lampos 0750 and each file 0640, so lamp-cloud
/// (lampos) reads the person's model key; a folder kept 0700 with 0600 files
/// before is brought to the same at the next set, the files it did not write
/// included.
#[test]
fn a_set_leaves_the_folder_0750_and_every_file_0640_an_old_one_too() {
    let store = Store(folder("modes"));
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    fs::create_dir_all(&store.0).unwrap();
    fs::set_permissions(&store.0, fs::Permissions::from_mode(0o700)).unwrap();
    let old = store.0.join("deepgram.key");
    fs::write(&old, PLANTED).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o600)).unwrap();
    let (reply, _) = keys::answer(
        &set("deepseek", ""),
        &store,
        &mut Told(Verdict::Works, vec![]),
        true,
    );
    assert_eq!(reply["done"]["code"], "works", "{reply}");
    assert_eq!(mode(&store.0), 0o750);
    for file in [
        "deepgram.key",
        "deepseek.key",
        "deepseek.url",
        "deepseek.checked",
    ] {
        assert_eq!(mode(&store.0.join(file)), 0o640, "{file}");
    }
    // A new folder the same.
    let fresh = Store(folder("modes-fresh"));
    keys::answer(
        &set("deepgram", ""),
        &fresh,
        &mut Told(Verdict::Works, vec![]),
        true,
    );
    assert_eq!(mode(&fresh.0), 0o750);
    assert_eq!(mode(&fresh.0.join("deepgram.key")), 0o640);
    for store in [store, fresh] {
        let _ = fs::remove_dir_all(store.0.parent().unwrap());
    }
}
