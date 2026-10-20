use axum::{
    Json, Router,
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode},
    middleware,
    routing::{get, post},
    serve::Listener,
};
use futures_util::StreamExt;
use lamp_surround::http_limit::*;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::{mpsc, oneshot},
};
use tower::ServiceExt;

struct FixtureListener(mpsc::Receiver<DuplexStream>);
impl Listener for FixtureListener {
    type Io = DuplexStream;
    type Addr = usize;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        (
            self.0.recv().await.expect("fixture listener remains open"),
            0,
        )
    }
    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(0)
    }
}
fn listener() -> (mpsc::Sender<DuplexStream>, FixtureListener) {
    let (tx, rx) = mpsc::channel(64);
    (tx, FixtureListener(rx))
}
async fn connect(tx: &mpsc::Sender<DuplexStream>) -> DuplexStream {
    let (client, server) = tokio::io::duplex(1024);
    tx.send(server).await.unwrap();
    client
}
fn app(limits: RequestLimits) -> Router {
    Router::new()
        .route(
            "/status",
            get(|| async { Json(serde_json::json!({"ready":false})) }),
        )
        .route(
            "/write",
            post(|Json(_): Json<serde_json::Value>| async { StatusCode::NO_CONTENT }),
        )
        .layer(middleware::from_fn_with_state(limits, limit_requests))
}
fn write(body: Body) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/write")
        .header("content-type", "application/json")
        .body(body)
        .unwrap()
}
fn unfinished() -> (Body, oneshot::Receiver<()>) {
    let (tx, rx) = oneshot::channel();
    let first = futures_util::stream::once(async move {
        tx.send(()).unwrap();
        Ok::<_, io::Error>(Bytes::from_static(b"{"))
    });
    (
        Body::from_stream(first.chain(futures_util::stream::pending())),
        rx,
    )
}

#[tokio::test]
async fn full_write_pool_refuses_before_body_extraction_and_status_keeps_its_own_slots() {
    let app = app(RequestLimits::default());
    let mut pending = Vec::new();
    for _ in 0..MAX_WRITE_REQUESTS {
        let (body, started) = unfinished();
        pending.push(tokio::spawn(app.clone().oneshot(write(body))));
        started.await.unwrap();
    }
    let refused = app.clone().oneshot(write(Body::from("{}"))).await.unwrap();
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    for task in pending {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
    assert_eq!(
        app.oneshot(write(Body::from("{}"))).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_partial_body_hits_total_handling_deadline_and_releases_its_request_slot() {
    let app = app(RequestLimits::with_limits(1, 1, Duration::from_millis(25)).unwrap());
    let (body, started) = unfinished();
    let task = tokio::spawn(app.clone().oneshot(write(body)));
    started.await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let message = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&message).contains("check status"));
    assert_eq!(
        app.oneshot(write(Body::from("{}"))).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn accepted_stream_count_is_bounded_and_drop_admits_a_replacement() {
    let (tx, inner) = listener();
    let mut listener = LimitedListener::new(inner);
    assert_eq!(listener.local_addr().unwrap(), 0);
    let mut held = Vec::new();
    let mut clients = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        clients.push(connect(&tx).await);
        held.push(listener.accept().await.0);
    }
    let mut excess = connect(&tx).await;
    let accepting = tokio::spawn(async move { listener.accept().await.0 });
    let mut byte = [0u8; 1];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), excess.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(!accepting.is_finished());
    held.pop();
    let mut replacement = connect(&tx).await;
    let mut accepted = accepting.await.unwrap();
    accepted.write_all(b"x").await.unwrap();
    replacement.read_exact(&mut byte).await.unwrap();
    assert_eq!(byte, [b'x']);
}

#[tokio::test]
async fn no_header_and_trickled_header_connections_expire_and_new_clients_recover() {
    let (tx, inner) = listener();
    let listener = LimitedListener::with_limits(inner, 2, Duration::from_millis(100)).unwrap();
    let serve = tokio::spawn(async move {
        axum::serve(listener, app(RequestLimits::default()))
            .await
            .unwrap();
    });
    let mut silent = connect(&tx).await;
    let mut partial = connect(&tx).await;
    partial.write_all(b"GET /sta").await.unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), silent.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), partial.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut healthy = connect(&tx).await;
    healthy
        .write_all(b"GET /status HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), healthy.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    serve.abort();
    assert!(serve.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn transport_deadline_also_refuses_writes_after_expiry() {
    let (tx, inner) = listener();
    let mut listener = LimitedListener::with_limits(inner, 1, Duration::from_millis(25)).unwrap();
    let _client = connect(&tx).await;
    let mut io = listener.accept().await.0;
    let error = io.read(&mut [0u8; 1]).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        io.write(b"late").await.unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        io.flush().await.unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(
        io.shutdown().await.unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}

#[test]
fn bounds_cannot_be_disabled_or_lifted_by_configuration() {
    for (reads, writes, time) in [
        (0, 1, REQUEST_DEADLINE),
        (1, 0, REQUEST_DEADLINE),
        (MAX_READ_REQUESTS + 1, 1, REQUEST_DEADLINE),
        (1, MAX_WRITE_REQUESTS + 1, REQUEST_DEADLINE),
        (1, 1, Duration::ZERO),
        (1, 1, REQUEST_DEADLINE + Duration::from_millis(1)),
    ] {
        assert!(RequestLimits::with_limits(reads, writes, time).is_err());
    }
    for (count, time) in [
        (0, CONNECTION_LIFETIME),
        (MAX_CONNECTIONS + 1, CONNECTION_LIFETIME),
        (1, Duration::ZERO),
        (1, CONNECTION_LIFETIME + Duration::from_millis(1)),
    ] {
        assert!(LimitedListener::with_limits((), count, time).is_err());
    }
}
