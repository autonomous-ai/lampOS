use lamp_memory_tiers::{model::*, *};
use std::time::Duration;
const EMPTY: &str = include_str!("fixtures/empty.json");
struct Fixture {
    _store: Store,
    batch: SleepInput,
}
fn input() -> Fixture {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    s.ingest(Incoming {
        text: "I like quiet mornings.".into(),
        speaker: Speaker::Owner,
        at_ms: DAY_MS,
        stt_confidence: Some(0.9),
        strength: Strength::Said,
        addressed_to_lamp: true,
        topic: "taste".into(),
        remember: false,
    })
    .unwrap();
    let batch = s
        .prepare_sleep(DAY_MS + 1, PassKind::Nightly)
        .unwrap()
        .unwrap();
    Fixture { _store: s, batch }
}
fn limits() -> Limits {
    Limits {
        timeout: Duration::from_millis(50),
        max_output_tokens: 512,
        input_rate: 1_000_000,
        output_rate: 2_000_000,
        budget_microdollars: 100_000,
        max_calls: 2,
    }
}
fn reply(json: &str) -> Completion {
    Completion {
        json: json.into(),
        usage: Some(Usage {
            input_tokens: 100,
            output_tokens: 20,
        }),
    }
}
#[tokio::test]
async fn budget_precedes_call_and_usage_is_measured() {
    let mut tiny = limits();
    tiny.budget_microdollars = 1;
    let mut c = ModelClient::new(
        RecordedTransport::new(vec![reply(EMPTY)], Duration::ZERO),
        tiny,
    )
    .unwrap();
    assert!(matches!(
        c.propose(&input().batch).await,
        Err(Error::Budget)
    ));
    assert_eq!(c.calls(), 0);
    let mut c = ModelClient::new(
        RecordedTransport::new(
            vec![reply(EMPTY), reply(EMPTY), reply(EMPTY)],
            Duration::ZERO,
        ),
        limits(),
    )
    .unwrap();
    let (_, r) = c.propose(&input().batch).await.unwrap();
    assert_eq!(r.charged_microdollars, 140);
    assert_eq!(c.spent_microdollars(), 140);
    c.propose(&input().batch).await.unwrap();
    assert!(matches!(
        c.propose(&input().batch).await,
        Err(Error::Budget)
    ));
}
#[tokio::test(start_paused = true)]
async fn timeout_cancels_and_keeps_cost_reserve() {
    let mut c = ModelClient::new(
        RecordedTransport::new(vec![reply(EMPTY)], Duration::from_secs(10)),
        limits(),
    )
    .unwrap();
    assert!(matches!(
        c.propose(&input().batch).await,
        Err(Error::Timeout)
    ));
    assert_eq!(c.calls(), 1);
    assert!(c.spent_microdollars() > 140);
}
#[tokio::test]
async fn malformed_and_oversized_output_keep_reserve() {
    for text in ["oops".into(), "x".repeat(MAX_OUTPUT_BYTES + 1)] {
        let mut c = ModelClient::new(
            RecordedTransport::new(vec![reply(&text)], Duration::ZERO),
            limits(),
        )
        .unwrap();
        assert!(c.propose(&input().batch).await.is_err());
        assert!(c.spent_microdollars() > 140);
    }
}
#[tokio::test]
async fn missing_usage_charges_bound_and_excess_usage_is_refused() {
    let mut r = reply(EMPTY);
    r.usage = None;
    let mut c =
        ModelClient::new(RecordedTransport::new(vec![r], Duration::ZERO), limits()).unwrap();
    let (_, r) = c.propose(&input().batch).await.unwrap();
    assert_eq!(r.charged_microdollars, r.reserved_microdollars);
    let mut r = reply(EMPTY);
    r.usage.as_mut().unwrap().output_tokens = 100_000;
    let mut c =
        ModelClient::new(RecordedTransport::new(vec![r], Duration::ZERO), limits()).unwrap();
    assert!(c.propose(&input().batch).await.is_err());
}

#[tokio::test]
async fn forgotten_prepared_input_never_reaches_transport() {
    let mut f = input();
    f._store.forget_records(&[1]).unwrap();
    let mut c = ModelClient::new(
        RecordedTransport::new(vec![reply(EMPTY)], Duration::ZERO),
        limits(),
    )
    .unwrap();
    assert!(matches!(c.propose(&f.batch).await, Err(Error::Stale)));
    assert_eq!(c.calls(), 0);
    assert!(matches!(f.batch.json(), Err(Error::Stale)));
}
#[tokio::test(start_paused = true)]
async fn forget_cancels_an_inflight_model_future() {
    let mut f = input();
    let mut c = ModelClient::new(
        RecordedTransport::new(vec![reply(EMPTY)], Duration::from_secs(10)),
        limits(),
    )
    .unwrap();
    let operation = c.propose(&f.batch);
    tokio::pin!(operation);
    tokio::select! {biased;result=&mut operation=>panic!("unexpected completion: {}",result.is_ok()),_=tokio::time::sleep(Duration::from_millis(1))=>{}}
    f._store.forget_records(&[1]).unwrap();
    assert!(matches!(operation.await, Err(Error::Stale)));
}

const TOPICS: &str = r#"{"segments":[{"first_record_id":1,"last_record_id":1}]}"#;
#[tokio::test]
async fn topic_and_extraction_calls_share_one_budget_and_call_limit() {
    let mut f = input();
    f._store.owner_left(DAY_MS + 2).unwrap();
    let topics = f._store.prepare_topics(DAY_MS + 3).unwrap().unwrap();
    let mut client = ModelClient::new(
        RecordedTransport::new(vec![reply(TOPICS), reply(EMPTY)], Duration::ZERO),
        limits(),
    )
    .unwrap();
    let (proposal, receipt) = client.propose_topics(&topics).await.unwrap();
    assert_eq!(receipt.charged_microdollars, 140);
    f._store.apply_topics(&topics, &proposal).unwrap();
    let extraction = f
        ._store
        .prepare_sleep(DAY_MS + 3, PassKind::Nightly)
        .unwrap()
        .unwrap();
    client.propose(&extraction).await.unwrap();
    assert_eq!(client.spent_microdollars(), 280);
    assert_eq!(client.calls(), 2);
    assert!(matches!(
        client.propose(&extraction).await,
        Err(Error::Budget)
    ));
}

#[tokio::test(start_paused = true)]
async fn topic_timeout_retains_reservation_and_forget_cancels_inflight_call() {
    let mut f = input();
    f._store.owner_left(DAY_MS + 2).unwrap();
    let topics = f._store.prepare_topics(DAY_MS + 3).unwrap().unwrap();
    let mut client = ModelClient::new(
        RecordedTransport::new(vec![reply(TOPICS)], Duration::from_secs(10)),
        limits(),
    )
    .unwrap();
    assert!(matches!(
        client.propose_topics(&topics).await,
        Err(Error::Timeout)
    ));
    assert!(client.spent_microdollars() > 140);
    let operation = client.propose_topics(&topics);
    tokio::pin!(operation);
    tokio::select! {biased;result=&mut operation=>panic!("unexpected completion: {}",result.is_ok()),_=tokio::time::sleep(Duration::from_millis(1))=>{}}
    f._store.forget_records(&[1]).unwrap();
    assert!(matches!(operation.await, Err(Error::Stale)));
    let mut fresh = ModelClient::new(
        RecordedTransport::new(vec![reply(TOPICS)], Duration::ZERO),
        limits(),
    )
    .unwrap();
    assert!(matches!(
        fresh.propose_topics(&topics).await,
        Err(Error::Stale)
    ));
    assert_eq!(fresh.calls(), 0);
}

#[tokio::test]
async fn topic_reply_and_reported_usage_cannot_exceed_the_smaller_allowance() {
    let mut f = input();
    f._store.owner_left(DAY_MS + 2).unwrap();
    let topics = f._store.prepare_topics(DAY_MS + 3).unwrap().unwrap();
    assert_eq!(topics.output_allowance().unwrap(), 119);
    let mut excess_usage = reply(TOPICS);
    excess_usage.usage.as_mut().unwrap().output_tokens = 120;
    let mut excess_bytes = reply(&format!("{TOPICS}{}", " ".repeat(120)));
    excess_bytes.usage = None;
    for completion in [excess_usage, excess_bytes] {
        let mut client = ModelClient::new(
            RecordedTransport::new(vec![completion], Duration::ZERO),
            limits(),
        )
        .unwrap();
        assert!(matches!(
            client.propose_topics(&topics).await,
            Err(Error::Model(_))
        ));
        assert_eq!(client.calls(), 1);
        assert!(client.spent_microdollars() > 140);
    }
}

#[tokio::test]
async fn topic_reply_whitespace_is_allowed_with_usage_within_the_token_ceiling() {
    let mut f = input();
    f._store.owner_left(DAY_MS + 2).unwrap();
    let topics = f._store.prepare_topics(DAY_MS + 3).unwrap().unwrap();
    let formatted = format!("{TOPICS}{}", " ".repeat(120));
    assert!(formatted.len() > topics.output_allowance().unwrap() as usize);
    let mut client = ModelClient::new(
        RecordedTransport::new(vec![reply(&formatted)], Duration::ZERO),
        limits(),
    )
    .unwrap();
    let (result, _) = client.propose_topics(&topics).await.unwrap();
    f._store.apply_topics(&topics, &result).unwrap();
}
