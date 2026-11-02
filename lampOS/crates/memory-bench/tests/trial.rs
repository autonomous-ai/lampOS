use lamp_memory_bench::{
    Result, files,
    ledger::Mode,
    model::{Call, Client, Prices, Provider, Purpose, Reply, Usage},
    model_worker::Worker,
    protocol::{Op, Request},
    runner::Runner,
    trial::Trial,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "bench-trial-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Fake {
    counts: Arc<Mutex<(usize, usize)>>,
}
impl Provider for Fake {
    fn invoke(&mut self, call: &Call, _deadline: Instant) -> Result<Reply> {
        let Call::Chat {
            purpose, messages, ..
        } = call
        else {
            panic!("only chat expected")
        };
        let content = match purpose {
            Purpose::Answer => {
                self.counts.lock().unwrap().0 += 1;
                json!({"answer":"I don't know","evidence_ids":[],"abstain":true})
            }
            Purpose::Judge => {
                self.counts.lock().unwrap().1 += 1;
                let packet: Value =
                    serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
                assert!(packet.get("arm").is_none());
                assert!(packet.get("run").is_none());
                let n = packet["rubric"].as_array().unwrap().len();
                json!({"awarded":vec![false;n],"explanation":"Offline structure check only.","evidence_ids":[]})
            }
            _ => panic!("no-memory should not call maintenance models"),
        };
        Ok(Reply {
            content: Some(content.to_string()),
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
            ..Reply::default()
        })
    }
}
#[test]
fn registered_month_runs_answering_judging_and_restore_without_any_scores() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/memory-bench/development");
    let corpus = files::read_corpus(&fixture).unwrap();
    let dir = Dir::new();
    let counts = Arc::new(Mutex::new((0, 0)));
    let client = Client::new(
        Fake {
            counts: counts.clone(),
        },
        Prices {
            source: "offline".into(),
            date: "2026-10-31".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        1_000_000,
    )
    .unwrap();
    let mut worker = Worker::start(client).unwrap();
    let revision = "a".repeat(40);
    let mut runner = Runner::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_lamp-memory-bench")),
        &["arm".into(), revision],
        &dir.0,
    )
    .unwrap();
    runner.execute(Request::new("init",Op::Init,json!({"run_id":"offline-month","arm":"none","state_dir":dir.0,"owner_id":corpus.persona.id,"timezone":corpus.persona.timezone,"at_unix_ms":corpus.events[0].at_unix_ms,"config":{}})),&mut worker).unwrap();
    let mut judged = 0;
    let result = Trial {
        corpus: &corpus,
        observed: &corpus.events,
        arm: "none",
        mode: Mode::Recorded,
        runner: &mut runner,
        worker: &mut worker,
    }
    .run(|kind, data| {
        assert_ne!(kind, "question_evaluation");
        if kind == "judge_plumbing" {
            judged += 1;
            assert!(data["scores"].is_null());
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result["events"], 2220);
    assert_eq!(result["questions"], 200);
    assert_eq!(result["answer_turns"], 224);
    assert_eq!(result["nights"], 30);
    assert_eq!(result["weekly_passes"], 4);
    assert_eq!(result["label"], "plumbing check");
    for kind in result["by_kind"].as_object().unwrap().values() {
        assert!(kind["mean"].is_null());
        assert_eq!(kind["scored"], 0);
    }
    assert_eq!(judged, 76);
    assert_eq!(*counts.lock().unwrap(), (224, 76));
    runner
        .execute(Request::new("close", Op::Close, json!({})), &mut worker)
        .unwrap();
    drop(runner);
    assert_eq!(
        worker
            .finish(Instant::now() + Duration::from_secs(5))
            .unwrap()
            .receipts
            .len(),
        300
    );
}
