//! Synthetic contract exercise only. This does not load or score a persona.
use crate::{
    Result, invalid,
    model::{Client, Prices, Recorded},
    model_worker::Worker,
    protocol::*,
    runner::Runner,
};
use serde_json::{Value, json};
use std::{
    io::Cursor,
    os::unix::fs::DirBuilderExt,
    path::Path,
    time::{Duration, Instant},
};
pub fn run(executable: &Path, arm: &str, revision: &str, state: &Path) -> Result<Value> {
    run_with_args(executable, &[], arm, revision, state)
}
pub fn run_with_args(
    executable: &Path,
    args: &[String],
    arm: &str,
    revision: &str,
    state: &Path,
) -> Result<Value> {
    if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("smoke requires full adapter revision"));
    }
    std::fs::DirBuilder::new().mode(0o700).create(state)?;
    let state = std::fs::canonicalize(state)?;
    let executable = std::fs::canonicalize(executable)?;
    let mut runner = Runner::spawn(&executable, args, &state)?;
    let mut client = Worker::start(Client::new(
        Recorded::read(Cursor::new(Vec::<u8>::new()))?,
        Prices {
            source: "synthetic-contract-test".into(),
            date: "2030-01-01".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        10_000,
    )?)?;
    let init = runner.execute(
        Request::new(
            "smoke-init",
            Op::Init,
            json!({"run_id":"contract-smoke","arm":arm,"state_dir":state,
        "owner_id":"owner-smoke","timezone":"Asia/Ho_Chi_Minh","at_unix_ms":0,"config":{}}),
        ),
        &mut client,
    )?;
    if init.data["implementation_revision"] != revision {
        return Err(invalid(
            "adapter revision differs from requested smoke revision",
        ));
    }
    runner.execute(
        Request::new(
            "smoke-sleep",
            Op::Sleep,
            json!({"at_unix_ms":1,"cadence":"nightly"}),
        ),
        &mut client,
    )?;
    let event = Event {
        record_id: "smoke-record".into(),
        at_unix_ms: 2,
        session_id: "smoke-session".into(),
        kind: EventKind::Utterance,
        person: Person::Owner,
        person_id: Some("owner-smoke".into()),
        presence: Presence::OwnerAlone,
        heard: "My favorite pebble is cobalt marble.".into(),
        said: String::new(),
        stt_confidence: None,
        cues: Cues::default(),
    };
    runner.execute(
        Request::new("smoke-ingest", Op::Ingest, json!({"event":event})),
        &mut client,
    )?;
    let inspect = |id: &str| Request::new(id, Op::Inspect, json!({"person_id":"owner-smoke"}));
    let before = runner.execute(inspect("smoke-before"), &mut client)?.data;
    runner.execute(
        Request::new(
            "smoke-probe",
            Op::ProbeBegin,
            json!({"probe_id":"smoke-question","at_unix_ms":3}),
        ),
        &mut client,
    )?;
    let query = Query {
        probe_id: "smoke-question".into(),
        at_unix_ms: 4,
        person: Person::Owner,
        person_id: Some("owner-smoke".into()),
        presence: Presence::OwnerAlone,
        heard: "What is my favorite pebble?".into(),
        cues: Cues::default(),
        max_context_bytes: 16_384,
    };
    let memory = runner.memory(&query, &mut client)?;
    memory.require_current(&runner)?;
    runner.execute(Request::new("smoke-forget",Op::Forget,json!({"at_unix_ms":5,"person_id":"owner-smoke","record_ids":["smoke-record"],"text":""})),&mut client)?;
    if memory.require_current(&runner).is_ok() {
        return Err(invalid("mutation left a stale prompt usable"));
    }
    let inside = runner.execute(inspect("smoke-inside"), &mut client)?.data;
    if inside["retained_record_ids"]
        .as_array()
        .is_some_and(|ids| ids.contains(&json!("smoke-record")))
    {
        return Err(invalid("component forget retained its source"));
    }
    runner.execute(
        Request::new(
            "smoke-end",
            Op::ProbeEnd,
            json!({"probe_id":"smoke-question"}),
        ),
        &mut client,
    )?;
    let after = runner.execute(inspect("smoke-after"), &mut client)?.data;
    if before["retained_record_ids"] != after["retained_record_ids"] {
        return Err(invalid("probe altered main source retention"));
    }
    runner.execute(
        Request::new("smoke-close", Op::Close, json!({})),
        &mut client,
    )?;
    let account = client.finish(Instant::now() + Duration::from_secs(1))?;
    if !account.receipts.is_empty() {
        return Err(invalid("smoke attempted an unrecorded model request"));
    }
    Ok(
        json!({"kind":"synthetic_contract_smoke","protocol":VERSION,"arm":arm,"adapter_revision":revision,
        "passed":true,"operations":12,"snapshot_missed":memory.snapshot_missed,"recall_missed":memory.recall_missed,
        "model_calls":0,"scores":null}),
    )
}
