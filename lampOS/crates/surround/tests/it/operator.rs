use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use lamp_surround::{config::Secret, operator::*};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::mpsc;
use tower::ServiceExt;

const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
fn app(enabled: bool) -> (Router, Shared, mpsc::Receiver<Submission>, Arc<AtomicU64>) {
    let (tx, rx) = mpsc::channel(1);
    let shared = Shared::new(enabled);
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let read = clock.clone();
    let router = router(
        Secret::new(CODE.as_bytes().to_vec()).unwrap(),
        "https://lamp.example",
        shared.clone(),
        tx,
        Arc::new(move || read.load(Ordering::Relaxed)),
    )
    .unwrap();
    (router, shared, rx, clock)
}
fn request(method: &str, path: &str, cookie: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "lamp.example")
        .header("origin", "https://lamp.example");
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    req.body(body.map_or_else(Body::empty, |value| {
        Body::from(serde_json::to_vec(&value).unwrap())
    }))
    .unwrap()
}
async fn login(app: &Router) -> String {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/login",
            None,
            Some(json!({"secret":CODE})),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    for flag in ["HttpOnly", "SameSite=Strict", "Secure", "Max-Age=900"] {
        assert!(cookie.contains(flag));
    }
    cookie.split(';').next().unwrap().into()
}
async fn value(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap()
}

#[tokio::test]
async fn page_is_self_contained_and_status_requires_authentication() {
    let (app, _, _, _) = app(false);
    for path in ["/", "/app.js", "/app.css"] {
        let response = app
            .clone()
            .oneshot(request("GET", path, None, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/status", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app).await;
    let reply = value(
        app.oneshot(request("GET", "/api/status", Some(&cookie), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(reply["data"]["enabled"], false);
    assert_eq!(reply["status"], 1);
    assert!(!reply.to_string().contains(CODE));
}

#[tokio::test]
async fn foreign_missing_duplicate_host_and_origins_and_query_secrets_are_refused() {
    let (app, _, _, _) = app(true);
    let cookie = login(&app).await;
    for scenario in 0..6 {
        let mut req = request(
            "POST",
            "/api/operation",
            Some(&cookie),
            Some(json!({"operation":"probe_network"})),
        );
        match scenario {
            0 => {
                req.headers_mut()
                    .insert("origin", "https://attacker.example".parse().unwrap());
            }
            1 => {
                req.headers_mut().remove("origin");
            }
            2 => {
                req.headers_mut()
                    .insert("host", "attacker.example".parse().unwrap());
            }
            3 => {
                req.headers_mut()
                    .append("host", "lamp.example".parse().unwrap());
            }
            4 => {
                req.headers_mut()
                    .append("origin", "https://lamp.example".parse().unwrap());
            }
            _ => {
                *req.uri_mut() = "/api/operation?credential=private".parse().unwrap();
            }
        }
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn sessions_expire_exactly_and_clock_reversal_revokes_them() {
    let (app, _, _, clock) = app(false);
    let cookie = login(&app).await;
    clock.store(1_000_000 + SESSION_US - 1, Ordering::Relaxed);
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/status", Some(&cookie), None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    clock.store(1_000_000 + SESSION_US, Ordering::Relaxed);
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/status", Some(&cookie), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app).await;
    clock.store(0, Ordering::Relaxed);
    assert_eq!(
        app.oneshot(request("GET", "/api/status", Some(&cookie), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn only_four_sessions_are_kept_and_logout_revokes_all() {
    let (app, _, _, _) = app(false);
    let first = login(&app).await;
    let second = login(&app).await;
    for _ in 0..3 {
        login(&app).await;
    }
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/status", Some(&first), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                "/api/logout",
                Some(&second),
                Some(json!({}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(request("GET", "/api/status", Some(&second), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_attempts_are_bounded_and_never_echo_credentials() {
    let (app, _, _, clock) = app(false);
    for _ in 0..10 {
        let reply = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/login",
                None,
                Some(json!({"secret":"do-not-echo-me"})),
            ))
            .await
            .unwrap();
        assert_eq!(reply.status(), StatusCode::UNAUTHORIZED);
        assert!(!value(reply).await.to_string().contains("do-not-echo-me"));
    }
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                "/api/login",
                None,
                Some(json!({"secret":CODE}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    clock.store(60_000_000, Ordering::Relaxed);
    login(&app).await;
}

#[tokio::test]
async fn duplicate_or_malformed_cookie_cannot_authenticate() {
    let (app, _, _, _) = app(false);
    let cookie = login(&app).await;
    for cookie in [
        format!("{cookie}; {cookie}"),
        "lamp_session=guess".into(),
        "lamp_session=".into(),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", "/api/status", Some(&cookie), None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn disabled_operation_never_reaches_worker() {
    let (app, _, mut rx, _) = app(false);
    let cookie = login(&app).await;
    assert_eq!(
        app.oneshot(request(
            "POST",
            "/api/operation",
            Some(&cookie),
            Some(json!({"operation":"probe_network"}))
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::FORBIDDEN
    );
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn worker_must_admit_before_http_ack_and_status_is_independent() {
    let (app, shared, mut rx, _) = app(true);
    let cookie = login(&app).await;
    let task = tokio::spawn(app.clone().oneshot(request(
        "POST",
        "/api/operation",
        Some(&cookie),
        Some(json!({"operation":"probe_network"})),
    )));
    let submission = rx.recv().await.unwrap();
    assert!(!task.is_finished());
    assert_eq!(submission.command.kind(), Kind::ProbeNetwork);
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/api/status", Some(&cookie), None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                "/api/operation",
                Some(&cookie),
                Some(json!({"operation":"probe_network"}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let id = submission.id.clone();
    submission.admitted.send(Ok(())).unwrap();
    let reply = task.await.unwrap().unwrap();
    assert_eq!(reply.status(), StatusCode::ACCEPTED);
    assert_eq!(value(reply).await["data"]["id"], id);
    assert!(shared.busy.load(Ordering::Acquire));
    shared.finished();
}

#[tokio::test]
async fn storage_refusal_is_not_an_accepted_operation() {
    let (app, shared, mut rx, _) = app(true);
    let cookie = login(&app).await;
    let task = tokio::spawn(app.oneshot(request(
        "POST",
        "/api/operation",
        Some(&cookie),
        Some(json!({"operation":"probe_network"})),
    )));
    rx.recv()
        .await
        .unwrap()
        .admitted
        .send(Err(ErrorCode::Storage))
        .unwrap();
    // The worker owns completion once delivered. Simulate it releasing its
    // operation and a newer operation taking the slot before this HTTP reply.
    shared.finished();
    shared.busy.store(true, Ordering::Release);
    assert_eq!(
        task.await.unwrap().unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(shared.busy.load(Ordering::Acquire));
}

#[tokio::test]
async fn malformed_oversized_and_unknown_requests_are_redacted() {
    let (app, _, mut rx, _) = app(true);
    let cookie = login(&app).await;
    for data in [
        json!({"operation":"raw_shell","command":"do-not-echo"}),
        json!({"operation":"probe_network","credential":"do-not-echo"}),
        json!({"operation":"stage_release","manifest_target":"X".repeat(MAX_REQUEST_BYTES)}),
    ] {
        let reply = app
            .clone()
            .oneshot(request("POST", "/api/operation", Some(&cookie), Some(data)))
            .await
            .unwrap();
        assert_eq!(reply.status(), StatusCode::BAD_REQUEST);
        assert!(!value(reply).await.to_string().contains("do-not-echo"));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn nonlocal_plaintext_origins_and_weak_codes_are_not_allowed() {
    for origin in [
        "http://lamp.example",
        "https://lamp.example/",
        "https://user:secret@lamp.example",
        "https://lamp.example/path",
        "https://lamp.example?key=secret",
    ] {
        let (tx, _) = mpsc::channel(1);
        assert!(
            router(
                Secret::new(CODE.as_bytes().to_vec()).unwrap(),
                origin,
                Shared::new(false),
                tx,
                Arc::new(|| 1)
            )
            .is_err()
        );
    }
    let (tx, _) = mpsc::channel(1);
    assert!(
        router(
            Secret::new(b"weak-code".to_vec()).unwrap(),
            "https://lamp.example",
            Shared::new(false),
            tx,
            Arc::new(|| 1)
        )
        .is_err()
    );
}
