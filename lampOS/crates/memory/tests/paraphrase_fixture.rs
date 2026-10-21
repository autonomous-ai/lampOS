//! #2510: the shape of the paraphrase recall set that
//! `training/memory-embed/bench.py` reads now and the lamp's own Rust bench
//! will read later. It only loads the file; nothing is embedded or sent.
use lamp_memory::store::TEXT_BYTES;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE: &str = include_str!("fixtures/paraphrase-recall.json");
const LANGS: [&str; 2] = ["en", "vi"];

fn text(item: &Value) -> &str {
    let text = item["text"].as_str().expect("text is a string");
    assert!(!text.trim().is_empty(), "empty text: {item}");
    assert!(
        text.len() <= TEXT_BYTES,
        "{} bytes is over the kept cap: {item}",
        text.len()
    );
    text
}

fn lang(item: &Value) -> &str {
    let lang = item["lang"].as_str().expect("lang is a string");
    assert!(LANGS.contains(&lang), "lang {lang:?} is not en or vi");
    lang
}

#[test]
fn paraphrase_fixture_has_its_shape() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
    assert!(
        fixture["about"]
            .as_str()
            .is_some_and(|about| !about.is_empty())
    );
    let kept = fixture["kept"].as_array().expect("kept is a list");
    let questions = fixture["questions"]
        .as_array()
        .expect("questions is a list");

    let mut ids = BTreeSet::new();
    let mut kept_lang = BTreeMap::new();
    for entry in kept {
        let id = entry["id"].as_u64().expect("id is a whole number");
        assert!(ids.insert(id), "duplicate id {id}");
        text(entry);
        kept_lang.insert(id, lang(entry));
    }

    assert!(
        questions.len() >= 30,
        "{} pairs, fewer than 30",
        questions.len()
    );
    let mut targets = BTreeSet::new();
    let mut per_lang = [0usize; 2];
    for question in questions {
        text(question);
        let asked = lang(question);
        let target = question["kept"].as_u64().expect("kept is an id");
        let Some(&target_lang) = kept_lang.get(&target) else {
            panic!("question points at no kept thing: {question}");
        };
        assert_eq!(
            asked, target_lang,
            "question and its kept thing differ in language"
        );
        assert!(targets.insert(target), "two questions share kept {target}");
        per_lang[LANGS.iter().position(|l| *l == asked).unwrap_or(0)] += 1;
    }
    let half = questions.len() / 2;
    for (lang, count) in LANGS.iter().zip(per_lang) {
        assert!(
            count.abs_diff(half) <= 1,
            "{count} {lang} pairs, not about half"
        );
    }

    for lang in LANGS {
        let distractors = kept_lang
            .iter()
            .filter(|(id, l)| **l == lang && !targets.contains(*id))
            .count();
        assert!(distractors > 0, "no {lang} distractors");
    }
}
