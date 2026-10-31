#![cfg(feature = "bench")]
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
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
            .stderr(Stdio::null())
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
        assert!(self.child.wait().unwrap().success());
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
fn all_arms_initialize_and_nightly_removal_makes_no_model_request() {
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
fn visitor_and_unknown_presence_cannot_retain_or_read_owner_context() {
    let mut p = ArmProcess::new("tiers");
    p.ingest("owner-record", "Remember the blue notebook.", 1000);
    p.begin("visitor-probe", 1200);
    let visitor=p.request("ingest",json!({"event":{"record_id":"visitor-record","at_unix_ms":1200,"session_id":"visitor-session","kind":"utterance","person":"unsure","person_id":null,"presence":"unknown","heard":"I keep a private number.","said":"","stt_confidence":null,"cues":{"activity":null,"objects":[],"topic":null}}}));
    assert_eq!(visitor["data"]["accepted"], false);
    let result=p.request("recall",json!({"query":{"probe_id":"visitor-probe","at_unix_ms":1200,"person":"owner","person_id":"owner-a","presence":"unknown","heard":"blue notebook","cues":{"activity":null,"objects":[],"topic":null},"max_context_bytes":16384}}));
    assert_eq!(result["data"]["context"], "");
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
    assert!(!p.child.wait().unwrap().success());
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
    let refusal = p.read();
    assert_eq!(refusal["error"]["code"], "timeout");
    assert!(!p.child.wait().unwrap().success());
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
    assert_eq!(rejected["data"]["pending"], true);
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
    assert!(!p.child.wait().unwrap().success());
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
    assert!(!p.child.wait().unwrap().success());
    let main =
        lamp_memory_tiers::Store::open(p._dir.path().join("tiers.sqlite"), "owner-a").unwrap();
    assert_eq!(
        main.records(lamp_memory_tiers::Access::OWNER_ALONE)
            .unwrap()
            .len(),
        1
    );
}
