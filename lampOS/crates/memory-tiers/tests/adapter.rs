use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
struct ArmProcess {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Value>,
    _dir: tempfile::TempDir,
    seq: u32,
}
impl ArmProcess {
    fn new(arm: &str) -> Self {
        Self::configured(arm, json!({}))
    }
    fn configured(arm: &str, config: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-memory-tiers-arm"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut p = Self {
            child,
            input,
            output: rx,
            _dir: dir,
            seq: 0,
        };
        let state = p._dir.path().to_string_lossy().into_owned();
        let init=p.request("init",json!({"run_id":"recorded-run","arm":arm,"state_dir":state,"owner_id":"owner-a","timezone":"Asia/Ho_Chi_Minh","at_unix_ms":1000,"config":config}));
        assert_eq!(init["ok"], true, "{init}");
        p
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }
    fn read(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("bounded adapter response")
    }
    fn start(&mut self, op: &str, data: Value) -> String {
        self.seq += 1;
        let id = format!("r{}", self.seq);
        self.send(json!({"v":2,"id":id,"op":op,"data":data}));
        id
    }
    fn request(&mut self, op: &str, data: Value) -> Value {
        let id = self.start(op, data);
        let value = self.read();
        assert_eq!(value["id"], id);
        value
    }
    fn ingest(&mut self, id: &str, text: &str, at: i64) -> Value {
        self.request("ingest",json!({"event":{"record_id":id,"at_unix_ms":at,"session_id":"session-a","kind":"utterance","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":text,"said":"","stt_confidence":null,"cues":{"activity":"desk","objects":[],"topic":null}}}))
    }
    fn begin(&mut self, probe: &str, at: i64) {
        let v = self.request("probe_begin", json!({"probe_id":probe,"at_unix_ms":at}));
        assert_eq!(v["ok"], true, "{v}");
    }
    fn end_session(&mut self, at: i64) {
        let id = format!("session-end-{}", self.seq);
        assert_eq!(self.request("ingest",json!({"event":{"record_id":id,"at_unix_ms":at,"session_id":"session-a","kind":"session_end","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"","said":"","stt_confidence":null,"cues":{"activity":"away","objects":[],"topic":null}}}))["ok"],true);
    }
    fn end(&mut self, probe: &str) {
        assert_eq!(
            self.request("probe_end", json!({"probe_id":probe}))["ok"],
            true
        );
    }
    fn query(&mut self, op: &str, probe: &str, at: i64, limit: usize) -> Value {
        self.request(op,json!({"query":{"probe_id":probe,"at_unix_ms":at,"person":"owner","person_id":"owner-a","presence":"owner_alone","heard":if op=="snapshot"{""}else{"What about the notebook?"},"cues":{"activity":"desk","objects":["notebook"],"topic":null},"max_context_bytes":limit}}))
    }
    fn sleep(&mut self, at: i64, proposal: &str) -> (Value, Value) {
        let id = self.start("sleep", json!({"at_unix_ms":at,"cadence":"nightly"}));
        let call = self.read();
        assert!(call["model_request"].is_object(), "{call}");
        self.send(json!({"v":2,"id":id,"model_response":{"call_id":call["model_request"]["call_id"],"ok":true,"content":proposal,"tool_calls":[],"usage":{"input_tokens":100,"output_tokens":100}}}));
        (call, self.read())
    }
    fn inspect(&mut self) -> Value {
        self.request("inspect", json!({"person_id":"owner-a"}))
    }
    fn close(&mut self) {
        let v = self.request("close", json!({}));
        assert_eq!(v["ok"], true, "{v}");
        assert!(self.wait_exit().success());
    }
    fn wait_exit_with_timeout(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("read child status") {
                return Some(status);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
    fn wait_exit(&mut self) -> ExitStatus {
        self.wait_exit_with_timeout(Duration::from_secs(10))
            .expect("adapter exits within 10 seconds")
    }
}
impl Drop for ArmProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn proposal() -> String {
    json!({"facts":[{"key":"note","claim":"May prefer a blue notebook.","record_ids":[1],"strength":"said","kind":"preference","true_since_ms":1000,"true_until_ms":null,"supersedes":null}],"lines":[{"replaces":null,"kind":"owner_model","text":"May prefer a blue notebook.","fact_ids":[],"fact_keys":["note"]}],"episodes":[]}).to_string()
}
fn model_input(call: &Value) -> Value {
    serde_json::from_str(
        call["model_request"]["messages"][1]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
fn assert_topic_stats(p: &mut ArmProcess, calls: u64, skipped: u64, batches: u64, fallbacks: u64) {
    let inspect = p.inspect();
    let stats = &inspect["data"]["stats"];
    assert_eq!(stats["model_calls"], calls, "{inspect}");
    assert_eq!(stats["skipped_sleeps"], skipped, "{inspect}");
    assert_eq!(stats["topic_batches"], batches, "{inspect}");
    assert_eq!(stats["topic_fallbacks"], fallbacks, "{inspect}");
}
#[test]
fn diagnostics_are_count_only_and_usage_survives_probe_restoration() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    p.sleep(1100, &proposal());
    let before = p.inspect();
    assert_eq!(before["v"], 2);
    assert_eq!(before["data"]["diagnostics"]["schema"], "tier-usage-v1");
    let before = &before["data"]["diagnostics"]["tiers"];
    assert_eq!(before["facts"]["items"], 1);
    assert_eq!(before["owner_model"]["items"], 1);
    for metrics in before.as_object().unwrap().values() {
        let keys: std::collections::BTreeSet<_> = metrics
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["items", "text_bytes", "recall_calls", "snapshot_calls"]
                .into_iter()
                .collect()
        );
        assert_eq!(metrics["recall_calls"], 0);
        assert_eq!(metrics["snapshot_calls"], 0);
    }
    assert!(!before.to_string().contains("notebook"));
    p.begin("diagnostic-probe", 1200);
    let snapshot = p.query("snapshot", "diagnostic-probe", 1200, 4096);
    let recall = p.query("recall", "diagnostic-probe", 1200, 4096);
    assert!(snapshot["data"].get("diagnostics").is_none());
    assert!(recall["data"].get("diagnostics").is_none());
    let used = p.inspect();
    assert_eq!(
        used["data"]["diagnostics"]["tiers"]["owner_model"]["snapshot_calls"],
        1
    );
    assert_eq!(
        used["data"]["diagnostics"]["tiers"]["facts"]["recall_calls"],
        1
    );
    assert_eq!(p.query("snapshot", "diagnostic-probe", 1200, 1)["ok"], true);
    assert_eq!(p.query("recall", "diagnostic-probe", 1200, 1)["ok"], true);
    assert_eq!(
        p.inspect()["data"]["diagnostics"],
        used["data"]["diagnostics"]
    );
    p.ingest("temporary-forget", "Forget everything", 1300);
    let empty = p.inspect();
    assert_eq!(empty["data"]["diagnostics"]["tiers"]["facts"]["items"], 0);
    assert_eq!(
        empty["data"]["diagnostics"]["tiers"]["owner_model"]["items"],
        0
    );
    p.end("diagnostic-probe");
    assert_eq!(
        p.inspect()["data"]["diagnostics"],
        used["data"]["diagnostics"]
    );
    let denied = p.request("inspect", json!({"person_id":"other-owner"}));
    assert!(denied["data"].get("diagnostics").is_none());
    p.close();
}

#[test]
fn diagnostics_separate_proof_rows_from_recallable_facts() {
    let mut p = ArmProcess::new("tiers_minus_facts");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    p.sleep(1100, &proposal());
    let initial = p.inspect();
    let tiers = &initial["data"]["diagnostics"]["tiers"];
    assert_eq!(tiers["facts"]["items"], 0);
    assert_eq!(tiers["facts"]["text_bytes"], 0);
    assert_eq!(tiers["evidence_only_facts"]["items"], 1);
    assert!(tiers["evidence_only_facts"]["text_bytes"].as_u64().unwrap() > 0);
    assert_eq!(tiers["owner_model"]["items"], 1);
    p.begin("proof-probe", 1200);
    p.query("snapshot", "proof-probe", 1200, 4096);
    p.query("recall", "proof-probe", 1200, 4096);
    let used = p.inspect();
    for tier in ["facts", "evidence_only_facts"] {
        assert_eq!(
            used["data"]["diagnostics"]["tiers"][tier]["recall_calls"],
            0
        );
        assert_eq!(
            used["data"]["diagnostics"]["tiers"][tier]["snapshot_calls"],
            0
        );
    }
    assert_eq!(
        used["data"]["diagnostics"]["tiers"]["owner_model"]["snapshot_calls"],
        1
    );
    p.end("proof-probe");
    p.ingest("forget", "Forget everything", 1300);
    let erased = p.inspect();
    assert_eq!(
        erased["data"]["diagnostics"]["tiers"]["evidence_only_facts"]["items"],
        0
    );
    assert_eq!(
        erased["data"]["diagnostics"]["tiers"]["owner_model"]["items"],
        0
    );
    p.close();
}
#[test]
fn child_exit_wait_is_bounded_when_adapter_keeps_running() {
    let mut p = ArmProcess::new("tiers");
    let start = Instant::now();
    assert!(
        p.wait_exit_with_timeout(Duration::from_millis(20))
            .is_none()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(p.inspect()["ok"], true);
    p.close();
}
#[test]
fn broker_citations_aggregate_cap_and_ordinary_forget_survive_sleep() {
    let mut p = ArmProcess::new("tiers");
    assert_eq!(
        p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000)["ok"],
        true
    );
    let (call, result) = p.sleep(1100, &proposal());
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["source_ids"], json!(["source-1"]));
    let input: Value = serde_json::from_str(
        call["model_request"]["messages"][1]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(input["records"][0]["stt_confidence"], Value::Null);
    p.begin("q1", 1200);
    let snapshot = p.query("snapshot", "q1", 1200, 500);
    assert_eq!(snapshot["ok"], true, "{snapshot}");
    let recall = p.query("recall", "q1", 1200, 500);
    assert_eq!(recall["ok"], true, "{recall}");
    let snap_bytes = snapshot["data"]["owner_model"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["text"].as_str().unwrap().len())
        .sum::<usize>()
        + snapshot["data"]["memory_instructions"]
            .as_str()
            .unwrap()
            .len();
    let recall_bytes = recall["data"]["context"].as_str().unwrap().len()
        + recall["data"]["memory_instructions"]
            .as_str()
            .unwrap()
            .len();
    assert!(snap_bytes + recall_bytes <= 500);
    // Wire framing/citations are reported by the runner separately. They do
    // not consume the common UTF-8 memory-text allowance.
    assert!(snapshot["data"].to_string().len() + recall["data"].to_string().len() > 500);
    assert_eq!(recall["data"]["owner_model"], json!([]));
    p.end("q1");
    assert_eq!(
        p.ingest(
            "forget-1",
            "Please forget what I told you about the blue notebook.",
            1300
        )["ok"],
        true
    );
    assert_eq!(p.inspect()["data"]["retained_record_ids"], json!([]));
    assert_eq!(
        p.ingest("repeat-1", "Maybe I prefer a blue notebook.", 1400)["data"]["source_ids"],
        json!([])
    );
    let no_work = p.request("sleep", json!({"at_unix_ms":1500,"cadence":"nightly"}));
    assert_eq!(no_work["data"]["pending"], false);
    assert_eq!(no_work["data"]["reason"], "no_eligible_records");
    p.close();
}
#[test]
fn probe_discards_deletions_and_new_words_without_resetting_broker_ids() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    p.begin("probe-a", 1100);
    let (a, result) = p.sleep(1100, &proposal());
    assert_eq!(result["ok"], true);
    p.ingest("erase", "forget everything", 1200);
    p.end("probe-a");
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["source-1"])
    );
    assert_eq!(p.inspect()["data"]["owner_model"], json!([]));
    let (b, result) = p.sleep(1100, &proposal());
    assert_eq!(result["ok"], true);
    assert_ne!(a["model_request"]["call_id"], b["model_request"]["call_id"]);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 2);
    p.begin("probe-b", 1300);
    p.ingest("temporary", "Remember my paper crane.", 1400);
    p.end("probe-b");
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["source-1"])
    );
    p.close();
}
#[test]
fn invalid_order_and_duplicate_ids_do_not_move_the_clock() {
    let mut p = ArmProcess::new("tiers");
    p.begin("probe-a", 1100);
    assert_eq!(
        p.request(
            "probe_begin",
            json!({"probe_id":"nested","at_unix_ms":9000})
        )["ok"],
        false
    );
    assert_eq!(p.request("close", json!({}))["ok"], false);
    p.ingest("probe-source", "A quiet day.", 1200);
    p.end("probe-a");
    assert_eq!(p.ingest("main-source", "A green vase.", 1050)["ok"], true);
    assert_eq!(p.ingest("main-source", "A red vase.", 1200)["ok"], false);
    assert_eq!(
        p.ingest("another-source", "A silver ruler.", 1100)["ok"],
        true
    );
    let id = p.start("inspect", json!({"person_id":"owner-a"}));
    assert_eq!(p.read()["ok"], true);
    p.send(json!({"v":2,"id":id,"op":"inspect","data":{"person_id":"owner-a"}}));
    assert_eq!(p.read()["ok"], false);
    p.close();
}
#[test]
fn all_arms_initialize_and_nightly_removal_skips_consolidation_for_open_sessions() {
    for arm in lamp_memory_tiers::Arm::ALL {
        let mut p = ArmProcess::new(&arm.name());
        assert_eq!(
            p.inspect()["data"]["stats"]["arm_description"],
            arm.description()
        );
        if arm == lamp_memory_tiers::Arm::TiersMinusNightlySleep {
            p.ingest("record", "A blue notebook.", 1000);
            let result = p.request("sleep", json!({"at_unix_ms":1100,"cadence":"nightly"}));
            assert_eq!(result["data"]["reason"], "not_applicable");
            assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 0);
        }
        p.close();
    }
}

#[test]
fn every_removal_keeps_topic_calls_and_session_only_keeps_old_prompt() {
    let cuts=json!({"segments":[{"first_record_id":1,"last_record_id":1},{"first_record_id":2,"last_record_id":2}]}).to_string();
    let empty = r#"{"facts":[],"lines":[],"episodes":[]}"#;
    for arm in lamp_memory_tiers::Arm::ALL {
        let mut p = ArmProcess::new(&arm.name());
        p.ingest("source-1", "A blue notebook.", 1000);
        p.ingest("source-2", "A fern needs water.", 1001);
        p.end_session(1002);
        if arm == lamp_memory_tiers::Arm::TiersSessionCuttingOnly {
            let (call, result) = p.sleep(1100, empty);
            assert_eq!(
                call["model_request"]["messages"][0]["content"],
                lamp_memory_tiers::model::PROMPT
            );
            assert_eq!(result["data"]["pending"], false);
            assert_eq!(
                p.inspect()["data"]["diagnostics"]["tiers"]["episodes"]["items"],
                1
            );
            assert_topic_stats(&mut p, 1, 0, 0, 0);
        } else {
            let (call, result) = p.sleep(1100, &cuts);
            assert_eq!(call["model_request"]["purpose"], "sleep");
            assert_eq!(call["model_request"]["max_output_tokens"], 160);
            assert_eq!(
                call["model_request"]["messages"][0]["content"],
                lamp_memory_tiers::TOPIC_PROMPT
            );
            assert_eq!(result["data"]["applied"], true, "{result}");
            assert_eq!(
                result["data"]["pending"],
                arm != lamp_memory_tiers::Arm::TiersMinusNightlySleep
            );
            if arm != lamp_memory_tiers::Arm::TiersMinusNightlySleep {
                let (call, result) = p.sleep(1100, empty);
                assert_eq!(
                    call["model_request"]["messages"][0]["content"],
                    lamp_memory_tiers::model::PROMPT
                );
                assert_eq!(result["data"]["pending"], false);
            }
            let inspect = p.inspect();
            assert_eq!(
                inspect["data"]["stats"]["model_calls"],
                if arm == lamp_memory_tiers::Arm::TiersMinusNightlySleep {
                    1
                } else {
                    2
                }
            );
            assert_eq!(inspect["data"]["stats"]["recall_misses"], 0);
            assert_eq!(
                inspect["data"]["diagnostics"]["tiers"]["episodes"]["items"],
                if arm == lamp_memory_tiers::Arm::TiersMinusEpisodes {
                    0
                } else {
                    2
                }
            );
        }
        p.close();
    }
}

#[test]
fn probe_restoration_reverts_topic_membership_but_keeps_model_accounting() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A fern needs water.", 1001);
    p.end_session(1002);
    p.begin("topic-probe", 1100);
    let cuts=json!({"segments":[{"first_record_id":1,"last_record_id":1},{"first_record_id":2,"last_record_id":2}]}).to_string();
    assert_eq!(p.sleep(1100, &cuts).1["data"]["applied"], true);
    assert_eq!(
        p.inspect()["data"]["diagnostics"]["tiers"]["episodes"]["items"],
        2
    );
    assert_topic_stats(&mut p, 1, 0, 0, 0);
    p.end("topic-probe");
    let main = p.inspect();
    assert_eq!(main["data"]["diagnostics"]["tiers"]["episodes"]["items"], 1);
    assert_eq!(main["data"]["stats"]["model_calls"], 1);
    assert_eq!(p.sleep(1100, &cuts).1["data"]["applied"], true);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 2);
    assert_topic_stats(&mut p, 2, 0, 1, 0);
    p.close();
}

#[test]
fn topic_ranges_cannot_cross_wire_sessions_even_if_an_end_event_was_missing() {
    for arm in ["tiers", "tiers_session_cutting_only"] {
        let mut p = ArmProcess::new(arm);
        p.ingest("first-session-source", "First session words.", 1000);
        let result=p.request("ingest",json!({"event":{"record_id":"second-session-source","at_unix_ms":1001,"session_id":"session-b","kind":"utterance","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"Second session words.","said":"","stt_confidence":null,"cues":{"activity":"at_desk","objects":[],"topic":null}}}));
        assert_eq!(result["ok"], true);
        let view = p.inspect();
        assert_eq!(view["data"]["diagnostics"]["tiers"]["episodes"]["items"], 1);
        if arm == "tiers" {
            p.end_session(1002);
            let (call, rejected) = p.sleep(
                1100,
                r#"{"segments":[{"first_record_id":1,"last_record_id":2}]}"#,
            );
            let input: Value = serde_json::from_str(
                call["model_request"]["messages"][1]["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(input["records"].as_array().unwrap().len(), 1);
            assert_eq!(rejected["data"]["applied"], false);
            assert_eq!(
                p.inspect()["data"]["diagnostics"]["tiers"]["episodes"]["items"],
                1
            );
        }
        p.close();
    }
}
#[test]
fn visitor_and_unknown_presence_cannot_retain_or_read_owner_context() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("owner-record", "Remember the blue notebook.", 1000);
    p.begin("visitor-probe", 1200);
    let visitor=p.request("ingest",json!({"event":{"record_id":"visitor-record","at_unix_ms":1200,"session_id":"visitor-session","kind":"utterance","person":"unsure","person_id":null,"presence":"unknown","heard":"I keep a private number.","said":"","stt_confidence":null,"cues":{"activity":null,"objects":[],"topic":null}}}));
    assert_eq!(visitor["data"]["accepted"], false);
    let result=p.request("recall",json!({"query":{"probe_id":"visitor-probe","at_unix_ms":1200,"person":"owner","person_id":"owner-a","presence":"unknown","heard":"blue notebook","cues":{"activity":null,"objects":[],"topic":null},"max_context_bytes":16384}}));
    assert_eq!(result["data"]["context"], "");
    assert_eq!(p.inspect()["data"]["stats"]["recall_misses"], 0);
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["owner-record"])
    );
    p.end("visitor-probe");
    p.close();
}

#[test]
fn broker_refusal_preserves_records_and_terminates_without_retry() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe a blue notebook.", 1000);
    let id = p.start("sleep", json!({"at_unix_ms":1100,"cadence":"nightly"}));
    let call = p.read();
    p.send(json!({"v":2,"id":id,"model_response":{"call_id":call["model_request"]["call_id"],"ok":false,"error":{"code":"budget","message":"recorded budget refusal"}}}));
    let refusal = p.read();
    assert_eq!(refusal["ok"], false);
    assert_eq!(refusal["error"]["code"], "model");
    assert!(!p.wait_exit().success());
    let store =
        lamp_memory_tiers::Store::open(p._dir.path().join("tiers.sqlite"), "owner-a").unwrap();
    assert_eq!(
        store
            .records(lamp_memory_tiers::Access::OWNER_ALONE)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn broker_timeout_is_bounded_and_never_retries() {
    let mut p = ArmProcess::configured("tiers", json!({"model_timeout_ms":20}));
    p.ingest("source-1", "Maybe a blue notebook.", 1000);
    p.start("sleep", json!({"at_unix_ms":1100,"cadence":"nightly"}));
    let call = p.read();
    assert!(call["model_request"].is_object());
    let start = Instant::now();
    let refusal = p.read();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(refusal["error"]["code"], "timeout");
    assert!(!p.wait_exit().success());
}

#[test]
fn rejected_sleep_rolls_back_earlier_facts_and_retries_the_same_records() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    let mut invalid: Value = serde_json::from_str(&proposal()).unwrap();
    let mut outside = invalid["facts"][0].clone();
    outside["key"] = json!("outside");
    outside["record_ids"] = json!([999]);
    invalid["facts"].as_array_mut().unwrap().push(outside);
    let (call, rejected) = p.sleep(1100, &invalid.to_string());
    let prompt = call["model_request"]["messages"][0]["content"]
        .as_str()
        .unwrap();
    assert!(prompt.contains("json"));
    assert!(prompt.contains("32 facts, 32 lines and 32 episodes"));
    assert_eq!(rejected["ok"], true, "{rejected}");
    assert_eq!(rejected["data"]["applied"], false);
    assert_eq!(rejected["data"]["pending"], false);
    assert_eq!(rejected["data"]["next_cursor"], Value::Null);
    assert_eq!(p.inspect()["data"]["owner_model"], json!([]));
    let conn = rusqlite::Connection::open(p._dir.path().join("tiers.sqlite")).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 0,
        "the first fact must roll back with the rejected second fact"
    );
    let (_, next) = p.sleep(1200, &proposal());
    assert_eq!(next["data"]["applied"], true, "{next}");
    assert_eq!(next["data"]["source_ids"], json!(["source-1"]));
    assert_eq!(p.inspect()["data"]["stats"]["skipped_sleeps"], 1);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 2);
    p.close();
}

#[test]
fn rejected_topic_night_preserves_records_for_the_next_partition() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A green garden.", 1001);
    p.end_session(1002);
    // Missing the second record is a well-formed but incomplete partition.
    let (call, rejected) = p.sleep(
        1100,
        r#"{"segments":[{"first_record_id":1,"last_record_id":1}]}"#,
    );
    assert!(
        call["model_request"]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("json")
    );
    assert_eq!(rejected["ok"], true, "{rejected}");
    assert_eq!(rejected["data"]["applied"], false);
    assert_eq!(rejected["data"]["pending"], false);
    assert_eq!(rejected["data"]["next_cursor"], Value::Null);
    assert_eq!(rejected["data"]["source_ids"], json!([]));
    let (retry, next) = p.sleep(
        1200,
        r#"{"segments":[{"first_record_id":1,"last_record_id":2}]}"#,
    );
    let first: Value = serde_json::from_str(
        call["model_request"]["messages"][1]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let again: Value = serde_json::from_str(
        retry["model_request"]["messages"][1]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["records"], again["records"]);
    assert_eq!(next["data"]["applied"], true, "{next}");
    assert_eq!(next["data"]["source_ids"], json!(["source-1", "source-2"]));
    assert_eq!(p.inspect()["data"]["stats"]["skipped_sleeps"], 1);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 2);
    assert_topic_stats(&mut p, 2, 1, 1, 0);
    p.close();
}

#[test]
fn repeated_invalid_topic_nights_fall_back_then_reach_consolidation_and_later_sessions() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A green garden.", 1001);
    p.end_session(1002);
    let invalid = r#"{"segments":[{"first_record_id":1,"last_record_id":999}]}"#;
    let (first, rejected) = p.sleep(1100, invalid);
    assert_eq!(rejected["ok"], true, "{rejected}");
    assert_eq!(rejected["data"]["applied"], false);
    assert_eq!(rejected["data"]["pending"], false);
    assert_eq!(rejected["data"]["next_cursor"], Value::Null);
    assert_topic_stats(&mut p, 1, 1, 0, 0);

    let (second, fallback) = p.sleep(1200, invalid);
    assert_ne!(
        first["model_request"]["call_id"],
        second["model_request"]["call_id"]
    );
    assert_eq!(
        model_input(&first)["records"],
        model_input(&second)["records"]
    );
    assert_eq!(fallback["ok"], true, "{fallback}");
    assert_eq!(fallback["data"]["applied"], true);
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_eq!(
        fallback["data"]["source_ids"],
        json!(["source-1", "source-2"])
    );
    assert_eq!(fallback["data"]["pending"], true);
    assert!(fallback["data"]["next_cursor"].is_string());
    assert_topic_stats(&mut p, 2, 2, 1, 1);

    // Code selects one range from the offered records, never the model's 999.
    let conn = rusqlite::Connection::open(p._dir.path().join("tiers.sqlite")).unwrap();
    let assigned: (i64, i64) = conn
        .query_row(
            "SELECT count(*),count(DISTINCT topic_episode_id) FROM topic_assignments",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(assigned, (2, 1));
    let records: Vec<(i64, String)> = conn
        .prepare("SELECT id,json_extract(body,'$.text') FROM records ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        records,
        vec![
            (1, "A blue notebook.".into()),
            (2, "A green garden.".into())
        ]
    );
    drop(conn);

    let empty = r#"{"facts":[],"lines":[],"episodes":[]}"#;
    let (call, consolidated) = p.sleep(1200, empty);
    assert_eq!(
        call["model_request"]["messages"][0]["content"],
        lamp_memory_tiers::model::PROMPT
    );
    assert_eq!(consolidated["data"]["applied"], true, "{consolidated}");
    assert_eq!(consolidated["data"]["pending"], false);
    assert_eq!(
        consolidated["data"]["source_ids"],
        json!(["source-1", "source-2"])
    );

    p.ingest("source-3", "A silver ruler.", 1300);
    p.ingest("source-4", "A yellow pencil.", 1301);
    p.end_session(1302);
    let (call, later) = p.sleep(
        1400,
        r#"{"segments":[{"first_record_id":3,"last_record_id":4}]}"#,
    );
    let ids: Vec<i64> = model_input(&call)["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![3, 4]);
    assert_eq!(later["data"]["applied"], true, "{later}");
    assert_eq!(later["data"]["reason"], Value::Null);
    assert_eq!(later["data"]["source_ids"], json!(["source-3", "source-4"]));
    let (_, consolidated) = p.sleep(1400, empty);
    assert_eq!(consolidated["data"]["applied"], true, "{consolidated}");
    assert_eq!(consolidated["data"]["pending"], false);
    assert_eq!(
        consolidated["data"]["source_ids"],
        json!(["source-3", "source-4"])
    );
    assert_topic_stats(&mut p, 5, 2, 2, 1);
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["source-1", "source-2", "source-3", "source-4"])
    );
    p.close();
}

#[test]
fn probe_topic_fallbacks_restore_retry_identity_and_do_not_count_main_batches() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A green garden.", 1001);
    p.end_session(1002);
    let invalid = r#"{"segments":[{"first_record_id":1,"last_record_id":999}]}"#;
    p.begin("fresh-retry-probe", 1100);
    assert_eq!(p.sleep(1100, invalid).1["data"]["applied"], false);
    let (_, fallback) = p.sleep(1200, invalid);
    assert_eq!(fallback["data"]["applied"], true, "{fallback}");
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_topic_stats(&mut p, 2, 2, 0, 0);
    p.end("fresh-retry-probe");

    // The probe's rejected passes cannot make this main-session first try fall back.
    let (_, rejected) = p.sleep(1100, invalid);
    assert_eq!(rejected["data"]["applied"], false, "{rejected}");
    assert_topic_stats(&mut p, 3, 3, 0, 0);
    p.begin("inherited-retry-probe", 1200);
    let (_, fallback) = p.sleep(1200, invalid);
    assert_eq!(fallback["data"]["applied"], true, "{fallback}");
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_topic_stats(&mut p, 4, 4, 0, 0);
    p.end("inherited-retry-probe");

    // Clearing retry state after probe success cannot clear the saved main state.
    let (_, fallback) = p.sleep(1200, invalid);
    assert_eq!(fallback["data"]["applied"], true, "{fallback}");
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_eq!(
        fallback["data"]["source_ids"],
        json!(["source-1", "source-2"])
    );
    assert_topic_stats(&mut p, 5, 5, 1, 1);
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["source-1", "source-2"])
    );
    p.close();
}

#[test]
fn forgetting_clears_topic_retry_identity_even_when_the_offered_batch_survives() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A green garden.", 1001);
    p.end_session(1002);
    p.ingest("source-3", "A disposable orange pencil.", 1003);
    let invalid = r#"{"segments":[{"first_record_id":1,"last_record_id":999}]}"#;
    let (first, rejected) = p.sleep(1100, invalid);
    assert_eq!(rejected["data"]["applied"], false);
    let forgotten = p.request(
        "forget",
        json!({"at_unix_ms":1150,"person_id":"owner-a","record_ids":["source-3"],"text":""}),
    );
    assert_eq!(forgotten["ok"], true, "{forgotten}");
    assert_eq!(forgotten["data"]["removed_count"], 1);

    let (after_forget, rejected) = p.sleep(1200, invalid);
    assert_eq!(
        model_input(&first)["records"],
        model_input(&after_forget)["records"]
    );
    assert_eq!(rejected["data"]["applied"], false, "{rejected}");
    assert_eq!(rejected["data"]["pending"], false);
    assert_topic_stats(&mut p, 2, 2, 0, 0);
    let (_, fallback) = p.sleep(1300, invalid);
    assert_eq!(fallback["data"]["applied"], true, "{fallback}");
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_eq!(
        fallback["data"]["source_ids"],
        json!(["source-1", "source-2"])
    );
    assert_topic_stats(&mut p, 3, 3, 1, 1);
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["source-1", "source-2"])
    );
    p.close();
}

#[test]
fn pause_and_resume_reset_topic_retry_identity_for_the_same_batch() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "A blue notebook.", 1000);
    p.ingest("source-2", "A green garden.", 1001);
    p.end_session(1002);
    let invalid = r#"{"segments":[{"first_record_id":1,"last_record_id":999}]}"#;
    let (first, rejected) = p.sleep(1100, invalid);
    assert_eq!(rejected["data"]["applied"], false);
    assert_eq!(p.ingest("pause", "pause memory", 1150)["ok"], true);
    let paused = p.request("sleep", json!({"at_unix_ms":1160,"cadence":"nightly"}));
    assert_eq!(paused["ok"], true, "{paused}");
    assert_eq!(paused["data"]["applied"], false);
    assert_topic_stats(&mut p, 1, 1, 0, 0);
    assert_eq!(p.ingest("resume", "resume memory", 1170)["ok"], true);

    let (after_resume, rejected) = p.sleep(1200, invalid);
    assert_eq!(
        model_input(&first)["records"],
        model_input(&after_resume)["records"]
    );
    assert_eq!(rejected["data"]["applied"], false, "{rejected}");
    assert_eq!(rejected["data"]["pending"], false);
    assert_topic_stats(&mut p, 2, 2, 0, 0);
    let (_, fallback) = p.sleep(1300, invalid);
    assert_eq!(fallback["data"]["applied"], true, "{fallback}");
    assert_eq!(fallback["data"]["reason"], "topic session-only fallback");
    assert_eq!(
        fallback["data"]["source_ids"],
        json!(["source-1", "source-2"])
    );
    assert_topic_stats(&mut p, 3, 3, 1, 1);
    p.close();
}

#[test]
fn an_over_cap_night_is_skipped_and_the_next_night_still_applies() {
    let mut p = ArmProcess::new("tiers");
    assert_eq!(
        p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000)["ok"],
        true
    );
    let facts: Vec<Value> = (0..33)
        .map(|i| json!({"key":format!("note-{i}"),"claim":"May prefer a blue notebook.","record_ids":[1],"strength":"said","kind":"preference","true_since_ms":1000,"true_until_ms":null,"supersedes":null}))
        .collect();
    let over = json!({"facts":facts,"lines":[],"episodes":[]}).to_string();
    let (_, skipped) = p.sleep(1100, &over);
    assert_eq!(skipped["ok"], true, "{skipped}");
    assert_eq!(skipped["data"]["applied"], false);
    assert_eq!(skipped["data"]["pending"], false);
    assert_eq!(skipped["data"]["next_cursor"], Value::Null);
    assert_eq!(skipped["data"]["source_ids"], json!([]));
    assert!(
        skipped["data"]["reason"]
            .as_str()
            .unwrap()
            .contains("too many proposals"),
        "{skipped}"
    );
    let (_, next) = p.sleep(1200, &proposal());
    assert_eq!(next["ok"], true, "{next}");
    assert_eq!(next["data"]["applied"], true);
    assert_eq!(next["data"]["source_ids"], json!(["source-1"]));
    assert_eq!(p.inspect()["data"]["stats"]["skipped_sleeps"], 1);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 2);
    p.close();
}
#[test]
fn bounded_sleep_reports_pending_and_rejection_keeps_the_batch() {
    let mut p = ArmProcess::new("tiers");
    for i in 0..129 {
        assert_eq!(
            p.ingest(
                &format!("source-{i}"),
                &format!("Ordinary statement number {i}."),
                1000 + i
            )["ok"],
            true
        );
    }
    let empty = r#"{"facts":[],"lines":[],"episodes":[]}"#;
    let (_, first) = p.sleep(2000, empty);
    assert_eq!(first["data"]["pending"], true);
    assert!(first["data"]["next_cursor"].is_string());
    assert_eq!(first["data"]["source_ids"].as_array().unwrap().len(), 128);
    let (_, rejected) = p.sleep(2000, &proposal());
    assert_eq!(rejected["data"]["applied"], false);
    // A refused night is skipped with no pending work (the runner refuses pending work
    // without a cursor); its batch stays for the next night.
    assert_eq!(rejected["data"]["pending"], false);
    assert_eq!(rejected["data"]["next_cursor"], Value::Null);
    assert_eq!(
        p.ingest("backdated", "An earlier observation.", 1900)["ok"],
        false
    );
    let (_, last) = p.sleep(2000, empty);
    assert_eq!(last["data"]["source_ids"], json!(["source-128"]));
    assert_eq!(last["data"]["pending"], false);
    p.close();
}

#[test]
fn controls_work_in_mixed_presence_and_pause_stops_following_capture() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("kept", "Remember the blue notebook.", 1000);
    let v=p.request("ingest",json!({"event":{"record_id":"erase-with-visitor","at_unix_ms":1100,"session_id":"session-a","kind":"utterance","person":"owner","person_id":"owner-a","presence":"others","heard":"Forget the blue notebook.","said":"","stt_confidence":null,"cues":{"activity":null,"objects":[],"topic":null}}}));
    assert_eq!(v["data"]["accepted"], true);
    assert_eq!(p.inspect()["data"]["retained_record_ids"], json!([]));
    assert_eq!(p.ingest("pause", "Don't remember this.", 1200)["ok"], true);
    p.ingest("private", "A private story about a violet kite.", 1300);
    assert_eq!(p.inspect()["data"]["retained_record_ids"], json!([]));
    p.ingest("resume", "Resume remembering.", 1400);
    p.ingest("after-resume", "An orange pencil.", 1500);
    assert_eq!(
        p.inspect()["data"]["retained_record_ids"],
        json!(["after-resume"])
    );
    p.close();
}
#[test]
fn same_session_forget_discards_the_runner_snapshot_before_reuse() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    p.sleep(1100, &proposal());
    p.begin("same-session", 1200);
    let snapshot = p.query("snapshot", "same-session", 1200, 500);
    assert!(
        !snapshot["data"]["owner_model"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    p.ingest("forget-in-session", "Forget the blue notebook.", 1300);
    // Proposed common runner rule: every mutation invalidates cached memory;
    // obtain a fresh snapshot before reusing it in the same session.
    let refreshed = p.query("snapshot", "same-session", 1300, 500);
    let recall = p.query("recall", "same-session", 1300, 500);
    assert_eq!(refreshed["data"]["owner_model"], json!([]));
    assert_eq!(recall["data"]["context"], "");
    assert_eq!(refreshed["data"]["memory_instructions"], "");
    // The adapter's counter includes both snapshot and recall requests.
    let stats = p.inspect();
    assert_eq!(stats["data"]["stats"]["recall_misses"], 0);
    p.end("same-session");
    p.close();
}

#[test]
fn unknown_usage_keeps_the_runner_as_cost_owner() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    let id = p.start("sleep", json!({"at_unix_ms":1100,"cadence":"nightly"}));
    let call = p.read();
    p.send(json!({"v":2,"id":id,"model_response":{"call_id":call["model_request"]["call_id"],"ok":true,"content":proposal(),"tool_calls":[],"usage":null}}));
    let result = p.read();
    assert_eq!(result["data"]["applied"], true);
    assert_eq!(p.inspect()["data"]["stats"]["model_calls"], 1);
    p.close();
}

#[test]
fn v2_forget_effects_drive_recent_history_without_guessing_intent() {
    let mut p = ArmProcess::new("tiers");
    let ordinary = p.ingest("source-1", "Maybe I prefer a blue notebook.", 1000);
    assert_eq!(ordinary["v"], 2);
    assert_eq!(
        ordinary["data"]["forget_effect"],
        json!({"ran":false,"suppress_confirmation":false})
    );
    let refused = p.ingest("empty-target", "Forget my", 1100);
    assert_eq!(
        refused["data"]["forget_effect"],
        json!({"ran":false,"suppress_confirmation":false})
    );
    let no_match = p.ingest("no-match", "Forget the diamond lock.", 1200);
    assert_eq!(
        no_match["data"]["forget_effect"],
        json!({"ran":true,"suppress_confirmation":false})
    );
    let mut recent = vec!["Earlier exchange containing the blue notebook."];
    let erased = p.ingest("erase", "Forget the blue notebook.", 1300);
    assert_eq!(
        erased["data"]["forget_effect"],
        json!({"ran":true,"suppress_confirmation":true})
    );
    // A minimal v2 consumer uses only actual effects, never text/intent labels.
    if erased["data"]["forget_effect"]["ran"] == true {
        recent.clear();
    }
    if erased["data"]["forget_effect"]["suppress_confirmation"] != true {
        recent.push("The scripted confirmation.");
    }
    assert!(recent.is_empty());
    assert_eq!(p.inspect()["data"]["retained_record_ids"], json!([]));
    p.close();
}
#[test]
fn direct_forget_reports_effects_and_v1_is_never_upgraded() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Remember an orange pencil.", 1000);
    let result = p.request(
        "forget",
        json!({"at_unix_ms":1100,"person_id":"owner-a","record_ids":["source-1"],"text":""}),
    );
    assert_eq!(
        result["data"]["forget_effect"],
        json!({"ran":true,"suppress_confirmation":true})
    );
    p.send(json!({"v":1,"id":"old-v1","op":"inspect","data":{"person_id":"owner-a"}}));
    let old = p.read();
    assert_eq!(old["v"], 1);
    assert_eq!(old["ok"], false);
    assert!(old.get("data").is_none());
    p.close();
}

#[test]
fn storage_failure_ends_the_run_before_any_later_answer() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("source-1", "Remember the blue notebook.", 1000);
    let conn = rusqlite::Connection::open(p._dir.path().join("tiers.sqlite")).unwrap();
    conn.execute_batch("CREATE TRIGGER simulated_failure BEFORE DELETE ON records BEGIN SELECT RAISE(FAIL,'simulated storage failure'); END;").unwrap();
    drop(conn);
    let response = p.ingest("erase", "Forget the blue notebook.", 1100);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "storage");
    assert!(matches!(
        p.output.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert!(!p.wait_exit().success());
    let store =
        lamp_memory_tiers::Store::open(p._dir.path().join("tiers.sqlite"), "owner-a").unwrap();
    assert_eq!(
        store
            .records(lamp_memory_tiers::Access::OWNER_ALONE)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn probe_files_are_private_removed_and_preserve_prior_forgetting() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("original", "Remember my blue notebook.", 1000);
    p.ingest("forget", "Forget everything", 1100);
    p.begin("probe-a", 1200);
    let children: Vec<_> = std::fs::read_dir(p._dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(children.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            children[0].clone(),
            children[0].join("tiers.sqlite"),
            children[0].join("tiers.sqlite.forget-key"),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }
    assert_eq!(
        p.ingest("repeat-probe", "Remember my blue notebook.", 1300)["data"]["source_ids"],
        json!([])
    );
    p.ingest("probe-only", "Remember the paper crane.", 1400);
    p.end("probe-a");
    assert!(!children[0].exists());
    assert_eq!(p.inspect()["data"]["retained_record_ids"], json!([]));
    assert_eq!(
        p.ingest("repeat-main", "Remember my blue notebook.", 1200)["data"]["source_ids"],
        json!([])
    );
    p.close();
}

#[test]
fn failed_probe_cleanup_ends_run_without_overwriting_main_store() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("original", "Remember my blue notebook.", 1000);
    p.begin("probe-a", 1100);
    p.ingest("forget", "Forget everything", 1200);
    let scratch = std::fs::read_dir(p._dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .unwrap();
    // Substitute a regular file so removing a directory fails on every host,
    // including CI running as root where permissions do not induce a failure.
    std::fs::rename(&scratch, p._dir.path().join("quarantined-child")).unwrap();
    std::fs::write(&scratch, b"injected cleanup failure").unwrap();
    let reply = p.request("probe_end", json!({"probe_id":"probe-a"}));
    assert_eq!(reply["ok"], false);
    assert!(!p.wait_exit().success());
    let main =
        lamp_memory_tiers::Store::open(p._dir.path().join("tiers.sqlite"), "owner-a").unwrap();
    assert_eq!(
        main.records(lamp_memory_tiers::Access::OWNER_ALONE)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn paced_episodes_cut_on_topic_or_matching_owner_departure() {
    let mut p = ArmProcess::new("tiers");
    let mut event = json!({"record_id":"pace-1","at_unix_ms":1000,"session_id":"paced-session","kind":"utterance","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"The blue pencil is useful.","said":"","stt_confidence":null,"cues":{"activity":"desk","objects":[],"topic":"sketching"}});
    assert_eq!(p.request("ingest", json!({"event":event}))["ok"], true);
    event["record_id"] = json!("pace-2");
    event["at_unix_ms"] = json!(3000);
    event["heard"] = json!("I keep the pencil by the notebook.");
    assert_eq!(p.request("ingest", json!({"event":event}))["ok"], true);
    let mut departure = event.clone();
    departure["record_id"] = json!("other-departure");
    departure["at_unix_ms"] = json!(4000);
    departure["kind"] = json!("leave");
    departure["person_id"] = json!("other-person");
    departure["heard"] = json!("");
    departure["presence"] = json!("unknown");
    assert_eq!(p.request("ingest", json!({"event":departure}))["ok"], true);
    event["record_id"] = json!("pace-3");
    event["at_unix_ms"] = json!(6000);
    event["heard"] = json!("A soft pencil suits the sketch.");
    assert_eq!(p.request("ingest", json!({"event":event}))["ok"], true);
    event["record_id"] = json!("pace-4");
    event["at_unix_ms"] = json!(8000);
    event["cues"]["topic"] = json!("garden");
    event["heard"] = json!("My fern needs water.");
    assert_eq!(p.request("ingest", json!({"event":event}))["ok"], true);
    departure["record_id"] = json!("owner-departure");
    departure["at_unix_ms"] = json!(10000);
    departure["person_id"] = json!("owner-a");
    assert_eq!(p.request("ingest", json!({"event":departure}))["ok"], true);
    event["record_id"] = json!("pace-5");
    event["at_unix_ms"] = json!(600000);
    event["heard"] = json!("The fern looks green.");
    assert_eq!(p.request("ingest", json!({"event":event}))["ok"], true);
    p.close();
    let store =
        lamp_memory_tiers::Store::open(p._dir.path().join("tiers.sqlite"), "owner-a").unwrap();
    let records = store
        .records(lamp_memory_tiers::Access::OWNER_ALONE)
        .unwrap();
    assert_eq!(records.len(), 5);
    assert_eq!(records[0].episode_id, records[1].episode_id);
    assert_eq!(records[1].episode_id, records[2].episode_id);
    assert_ne!(records[2].episode_id, records[3].episode_id);
    assert_ne!(records[3].episode_id, records[4].episode_id);
}
