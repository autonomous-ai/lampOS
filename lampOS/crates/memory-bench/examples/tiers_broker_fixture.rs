//! Synthetic broker/cap integration fixture. No live calls or scores.
use lamp_memory_bench::{
    Result,
    model::{Call, Client, Prices, Provider, Purpose, Reply, Usage},
    model_worker::Worker,
    protocol::*,
    runner::Runner,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::DirBuilderExt,
    path::Path,
    time::{Duration, Instant},
};
struct Fixture;
impl Provider for Fixture {
    fn invoke(&mut self, call: &Call, _: Instant) -> Result<Reply> {
        let Call::Chat {
            purpose: Purpose::Sleep,
            messages,
            tools,
            tool_choice,
            ..
        } = call
        else {
            panic!("unexpected fixture request")
        };
        assert!(tools.is_empty());
        assert_eq!(tool_choice, "none");
        let input: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap())?;
        assert_eq!(
            input["records"][0]["text"],
            "Maybe I prefer a blue notebook."
        );
        Ok(Reply { content:Some(json!({"facts":[{"key":"note","claim":"May prefer a blue notebook.","record_ids":[1],"strength":"said","kind":"preference","true_since_ms":1000,"true_until_ms":null,"supersedes":null}],"lines":[{"replaces":null,"kind":"owner_model","text":"May prefer a blue notebook.","fact_ids":[],"fact_keys":["note"]}],"episodes":[]}).to_string()),usage:Some(Usage {input_tokens:100,output_tokens:100}),..Default::default() })
    }
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err(lamp_memory_bench::invalid(
            "usage: tiers_broker_fixture ABSOLUTE_ADAPTER NEW_ABSOLUTE_STATE_DIRECTORY",
        ));
    }
    let state = Path::new(&args[2]);
    std::fs::DirBuilder::new().mode(0o700).create(state)?;
    let mut r = Runner::spawn(Path::new(&args[1]), &[], state)?;
    let mut c = Worker::start(Client::new(
        Fixture,
        Prices {
            source: "synthetic fixture".into(),
            date: "2030-01-01".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        100_000,
    )?)?;
    r.execute(Request::new("init",Op::Init,json!({"run_id":"broker-fixture","arm":"tiers","state_dir":state,"owner_id":"owner-a","timezone":"Asia/Ho_Chi_Minh","at_unix_ms":1000,"config":{}})),&mut c)?;
    r.execute(Request::new("ingest",Op::Ingest,json!({"event":{"record_id":"source-1","at_unix_ms":1000,"session_id":"session-a","kind":"utterance","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"Maybe I prefer a blue notebook.","said":"","stt_confidence":null,"cues":{"activity":"desk","objects":[],"topic":null}}})),&mut c)?;
    r.execute(
        Request::new(
            "begin-a",
            Op::ProbeBegin,
            json!({"probe_id":"probe-a","at_unix_ms":1100}),
        ),
        &mut c,
    )?;
    let first = r.execute(
        Request::new(
            "sleep-a",
            Op::Sleep,
            json!({"at_unix_ms":1100,"cadence":"nightly"}),
        ),
        &mut c,
    )?;
    assert_eq!(first.data["applied"], true);
    r.execute(
        Request::new("end-a", Op::ProbeEnd, json!({"probe_id":"probe-a"})),
        &mut c,
    )?;
    let second = r.execute(
        Request::new(
            "sleep-main",
            Op::Sleep,
            json!({"at_unix_ms":1100,"cadence":"nightly"}),
        ),
        &mut c,
    )?;
    assert_eq!(second.data["applied"], true);
    r.execute(
        Request::new(
            "begin-b",
            Op::ProbeBegin,
            json!({"probe_id":"probe-b","at_unix_ms":1200}),
        ),
        &mut c,
    )?;
    let mut q = Query {
        probe_id: "probe-b".into(),
        at_unix_ms: 1200,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "What about the notebook?".into(),
        cues: Cues::default(),
        max_context_bytes: 16_384,
    };
    let memory = r.memory(&q, &mut c)?;
    memory.require_current(&r)?;
    assert!(memory.text.contains("source-1"));
    assert!(memory.text.contains("maybe"));
    q.max_context_bytes = 500;
    let small = r.memory(&q, &mut c)?;
    small.require_current(&r)?;
    assert!(small.memory_text_bytes <= 500);
    let small_result = json!({"ok":true,"memory_text_bytes":small.memory_text_bytes,"framing_bytes":small.framing_bytes,"rendered_bytes":small.text.len()});
    r.execute(
        Request::new("end-b", Op::ProbeEnd, json!({"probe_id":"probe-b"})),
        &mut c,
    )?;
    r.execute(Request::new("close", Op::Close, json!({})), &mut c)?;
    let account = c.finish(Instant::now() + Duration::from_secs(1))?;
    assert_eq!(account.receipts.len(), 2);
    assert_eq!(account.charged_microusd, 400);
    println!(
        "{}",
        json!({"label":"synthetic plumbing check","broker_fixture_passed":true,"model_calls":2,"synthetic_charge_microusd":400,"small_cap":small_result,"scores":null})
    );
    Ok(())
}
