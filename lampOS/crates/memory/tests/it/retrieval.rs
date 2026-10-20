//! Search behavior and the offline evaluator's public command. Fixtures are
//! synthetic, local, and deliberately include assistant-only evidence and
//! an abstention with the original answer session removed.

use lamp_memory::search::{self, SearchIndex};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn legacy_words_and_matches_preserve_the_existing_behavior() {
    let words = search::words("A COFFEE coffee café cafe\u{301} đèn");
    assert_eq!(words, ["coffee", "café", "cafe", "đèn"]);
    assert_eq!(search::matched(&words, &["coffees cafeteria", "CAFÉ"]), 1);
    assert_eq!(search::matched(&words, &["coffee coffee", "coffee ĐÈN"]), 2);
    assert_eq!(search::matched(&search::words("đèn"), &["den"]), 0);
}

#[test]
fn first_sixteen_distinct_words_are_the_only_query_terms() {
    let input = (0..30)
        .map(|number| format!("word{number}"))
        .collect::<Vec<_>>()
        .join(" ");
    let words = search::words(&input);
    assert_eq!(words.len(), 16);
    assert_eq!(words[15], "word15");
    let query = search::query(&format!("word0 WORD0 {input}")).unwrap();
    assert_eq!(query.split(" OR ").count(), 16);
    assert!(!query.contains("word16"));
    assert_eq!(search::matched(&words, &["word16"]), 0);
}

#[test]
fn query_uses_literal_terms_and_cannot_supply_fts_operators() {
    assert_eq!(search::query("a ? ' : * -"), None);
    assert_eq!(
        search::query("coffee OR NOT \"tea\" body:milk*"),
        Some("\"coffee\" OR \"or\" OR \"not\" OR \"tea\" OR \"body\" OR \"milk\"".into())
    );
    let index = SearchIndex::new().unwrap();
    index.add(1, "coffee").unwrap();
    index.add(2, "tea").unwrap();
    let mut ids = index.rank("coffee NOT tea", 10).unwrap();
    ids.sort();
    assert_eq!(ids, [1, 2]);
    assert!(index.rank("a ?", 10).unwrap().is_empty());
}

#[test]
fn nfc_lowercase_and_d_with_stroke_are_shared_by_query_and_index() {
    assert_eq!(search::text("TO\u{302}I ĐẶNG"), "tôi dặng");
    assert_eq!(search::query("ĐÈN đèn de\u{300}n"), Some("\"dèn\"".into()));
    let original = "Tôi uống cà phê đen ở Việt Nam.";
    let index = SearchIndex::new().unwrap();
    index.add(1, original).unwrap();
    assert_eq!(index.rank("CA PHE DEN VIET UONG", 5).unwrap(), [1]);
    assert_eq!(index.rank("uống", 5).unwrap(), [1]);
    assert_eq!(original, "Tôi uống cà phê đen ở Việt Nam.");
}

#[test]
fn decomposed_vietnamese_and_multiple_diacritics_match() {
    let index = SearchIndex::new().unwrap();
    index
        .add(1, "uo\u{302}\u{301}ng ca\u{300} phe\u{302} de\u{300}n")
        .unwrap();
    index.add(2, "ộ ố ở mỗi").unwrap();
    assert_eq!(index.rank("uống", 5).unwrap(), [1]);
    assert_eq!(index.rank("uống đèn", 5).unwrap(), [1]);
    assert_eq!(index.rank("moi", 5).unwrap(), [2]);
}

#[test]
fn bm25_uses_term_rarity_and_has_deterministic_ties() {
    let index = SearchIndex::new().unwrap();
    for id in 1..=10 {
        index
            .add(
                id,
                if id == 1 {
                    "coffee cardamom"
                } else {
                    "coffee coffee"
                },
            )
            .unwrap();
    }
    assert_eq!(index.rank("coffee cardamom", 1).unwrap(), [1]);
    let ties = SearchIndex::new().unwrap();
    ties.add(1, "same words").unwrap();
    ties.add(3, "same words").unwrap();
    ties.add(2, "same words").unwrap();
    assert_eq!(ties.rank("same", 2).unwrap(), [3, 2]);
    assert!(ties.rank("same", 0).unwrap().is_empty());
    assert!(ties.rank("absent", 10).unwrap().is_empty());
    assert!(ties.add(1, "replacement").is_err());
}

#[test]
fn independently_created_indexes_do_not_leak_between_people_or_questions() {
    let first = SearchIndex::new().unwrap();
    let second = SearchIndex::new().unwrap();
    first.add(1, "private cardamom preference").unwrap();
    second.add(1, "chocolate").unwrap();
    assert_eq!(first.rank("cardamom", 1).unwrap(), [1]);
    assert!(second.rank("cardamom", 1).unwrap().is_empty());
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lamp-memory-retrieval-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, filename: &str) -> PathBuf {
        self.0.join(filename)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Value {
    json!([
        {
            "question_id":"user-evidence", "question_type":"single-session-user",
            "question":"coffee", "question_date":"2026/01/03 (Sat) 12:00", "answer":42,
            "answer_session_ids":["answer_first"],
            "haystack_session_ids":["answer_first", "background"],
            "haystack_dates":["2026/01/01 (Thu) 12:00", "2026/01/02 (Fri) 12:00"],
            "haystack_sessions":[
                [{"role":"user","content":"coffee black","has_answer":true},
                 {"role":"assistant","content":"understood","has_answer":false}],
                [{"role":"user","content":"tea"}]
            ]
        },
        {
            "question_id":"assistant-evidence", "question_type":"single-session-assistant",
            "question":"espresso", "answer":"espresso",
            "answer_session_ids":["answer_second"],
            "haystack_session_ids":["answer_second"],
            "haystack_dates":["2026/01/02 (Fri) 12:00"],
            "haystack_sessions":[
                [{"role":"user","content":"pick a drink","has_answer":false},
                 {"role":"assistant","content":"espresso","has_answer":true}]
            ]
        },
        {
            "question_id":"missing_abs", "question_type":"single-session-user",
            "question":"something never said", "answer":"unknown",
            "answer_session_ids":["answer_removed"],
            "haystack_session_ids":["background"],
            "haystack_dates":["2026/01/02 (Fri) 12:00"],
            "haystack_sessions":[[{"role":"user","content":"tea"}]]
        }
    ])
}

fn write_fixture(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn evaluator() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lamp-memory-eval"))
}

#[test]
fn cli_reports_distinct_denominators_hashes_bytes_and_repeated_timings() {
    let scratch = Scratch::new();
    let input = scratch.path("questions.json");
    let per_query = scratch.path("results.jsonl");
    write_fixture(&input, &fixture());
    let output = evaluator()
        .arg(&input)
        .args([
            "--units",
            "both",
            "--repeats",
            "2",
            "--machine",
            "synthetic-test",
        ])
        .arg("--per-query")
        .arg(&per_query)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["questions_in_file"], 3);
    assert_eq!(result["machine"], "synthetic-test");
    assert_eq!(result["dataset"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        result["dataset"]["bytes"],
        fs::metadata(&input).unwrap().len()
    );
    let tracks = result["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 4);
    for track in tracks {
        let official = track["name"].as_str().unwrap().starts_with("official");
        let expected = if official { 1 } else { 2 };
        assert_eq!(track["excluded_abstentions"], 1);
        assert_eq!(track["excluded_no_target"], if official { 1 } else { 0 });
        assert_eq!(track["full_context_reference"]["recall_all"], 1.0);
        for method in ["whole_word", "fts5_bm25"] {
            let overall = &track["methods"][method]["overall"];
            assert_eq!(overall["questions"], expected);
            assert_eq!(overall["first_query_ms"]["n"], expected);
            assert_eq!(overall["repeated_query_ms"]["n"], expected);
            assert_eq!(overall["depths"]["1"]["recall_all"], 1.0);
        }
    }
    assert_eq!(fs::read_to_string(per_query).unwrap().lines().count(), 6);
}

#[test]
fn cli_rejects_missing_labels_and_never_overwrites_per_query_output() {
    let scratch = Scratch::new();
    let input = scratch.path("questions.json");
    let per_query = scratch.path("keep.txt");
    write_fixture(&input, &fixture());
    fs::write(&per_query, "keep me").unwrap();
    let output = evaluator()
        .arg(&input)
        .arg("--per-query")
        .arg(&per_query)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(&per_query).unwrap(), "keep me");
    let mut bad = fixture();
    bad[0]["answer_session_ids"] = json!(["absent"]);
    write_fixture(&input, &bad);
    let output = evaluator().arg(&input).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing from history"));
}

#[test]
fn cli_marks_partial_runs_and_does_not_score_all_abstention_input() {
    let scratch = Scratch::new();
    let input = scratch.path("questions.json");
    write_fixture(&input, &fixture());
    let output = evaluator()
        .arg(&input)
        .args(["--limit", "1", "--repeats", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["complete_file_evaluated"], false);
    assert_eq!(result["questions_selected"], 1);
    write_fixture(&input, &json!([fixture()[2].clone()]));
    let output = evaluator().arg(&input).output().unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    for track in result["tracks"].as_array().unwrap() {
        assert_eq!(track["excluded_abstentions"], 1);
        assert!(track["full_context_reference"].is_null());
        assert!(track["methods"].as_object().unwrap().is_empty());
    }
}
