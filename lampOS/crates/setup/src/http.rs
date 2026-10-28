//! V1's setup routes over `axum`: `POST /api/device/setup`,
//! `GET /api/device/setup/status`, `GET /api/network`, and, with `page`, this
//! crate's page and 204 answers to the phones' sign-in checks.

use crate::{
    scan::Network,
    serve::{Accepted, Config, Phase, SetupRequest, Status, check, failure, success},
};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::{Arc, Mutex};

pub const PAGE: &str = include_str!("page.html");

type Start = dyn Fn(Accepted) + Send + Sync;
type Scan = dyn Fn() -> std::io::Result<Vec<Network>> + Send + Sync;

pub struct Shared {
    pub config: Config,
    pub status: Arc<Mutex<Status>>,
    /// Begin a setup run in the background (V1: after a 2 s pause).
    pub start: Box<Start>,
    pub scan: Box<Scan>,
    /// One scan at a time, and its list kept for a few seconds.
    pub scanned: Mutex<Option<(std::time::Instant, Vec<Network>)>>,
    /// The last list a scan gave, the one made before the hotspot among
    /// them: answered when the radio refuses a scan while it is the hotspot
    /// (lamp-06c1, 2026-10-16: "Could not look for networks" until a few
    /// tries; V1 scans in station mode only).
    pub last_good: Mutex<Option<Vec<Network>>>,
}

/// The networks around: a list scanned within [`SCAN_KEPT`], else a new
/// scan, else the last list a scan gave. One scan at a time: a second asker
/// waits for this one's list instead of starting the radio again.
pub fn list_networks(shared: &Shared) -> std::io::Result<Vec<Network>> {
    let mut kept = shared.scanned.lock().unwrap();
    if let Some((at, list)) = kept.as_ref()
        && at.elapsed() < SCAN_KEPT
    {
        return Ok(list.clone());
    }
    match (shared.scan)() {
        Ok(list) if !list.is_empty() => {
            *kept = Some((std::time::Instant::now(), list.clone()));
            *shared.last_good.lock().unwrap() = Some(list.clone());
            Ok(list)
        }
        // Some radios as the hotspot answer with no networks rather than a
        // refusal (the fifth on #1045): a list seen before still answers.
        // Kept for SCAN_KEPT like any answer (the reviewer on #1064): a radio
        // that always answers empty is not scanned again at every poll.
        Ok(empty) => {
            let answer = shared.last_good.lock().unwrap().clone().unwrap_or(empty);
            *kept = Some((std::time::Instant::now(), answer.clone()));
            Ok(answer)
        }
        Err(error) => shared.last_good.lock().unwrap().clone().ok_or(error),
    }
}

/// Scan while nobody has asked yet, at most `tries` times `every` apart,
/// until one scan made as the hotspot works, so the page's first ask finds
/// a fresh list. Whether one did.
pub fn warm(shared: &Shared, tries: usize, every: std::time::Duration) -> bool {
    for attempt in 0..tries {
        if attempt > 0 {
            std::thread::sleep(every);
        }
        let mut kept = shared.scanned.lock().unwrap();
        // An ask already had a list scanned as the hotspot.
        if kept.as_ref().is_some_and(|(_, list)| !list.is_empty()) {
            return true;
        }
        if let Ok(list) = (shared.scan)()
            && !list.is_empty()
        {
            *kept = Some((std::time::Instant::now(), list.clone()));
            *shared.last_good.lock().unwrap() = Some(list);
            return true;
        }
    }
    false
}

/// How long one scan's list answers before the radio is asked again.
pub const SCAN_KEPT: std::time::Duration = std::time::Duration::from_secs(5);

fn json(code: StatusCode, body: serde_json::Value) -> Response {
    (
        code,
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(body),
    )
        .into_response()
}

async fn status(State(shared): State<Arc<Shared>>) -> Response {
    let status = shared.status.lock().unwrap().clone();
    json(
        StatusCode::OK,
        success(serde_json::to_value(status).unwrap()),
    )
}

async fn setup(State(shared): State<Arc<Shared>>, body: Bytes) -> Response {
    if body.len() > 16_384 {
        return json(StatusCode::BAD_REQUEST, failure("the request is too large"));
    }
    let Ok(request) = serde_json::from_slice::<SetupRequest>(&body) else {
        return json(
            StatusCode::BAD_REQUEST,
            failure("the request is not valid JSON"),
        );
    };
    let accepted = match check(&request, &shared.config) {
        Ok(accepted) => accepted,
        Err(why) => return json(StatusCode::BAD_REQUEST, failure(&why)),
    };
    // A link named but not kept leaves a line to read (Third on #1401), with
    // the fields' names only.
    if let Err(fields) = crate::serve::backend_link(&request)
        && !fields.is_empty()
    {
        eprintln!(
            "lamp-setup: the request named a backend link, but {} {} missing or not valid; setup goes on without it",
            fields.join(", "),
            if fields.len() == 1 { "is" } else { "are" }
        );
    }
    {
        let mut status = shared.status.lock().unwrap();
        if status.phase == Phase::Connecting {
            return json(StatusCode::CONFLICT, failure("setup is already running"));
        }
        if status.set_up_completed {
            return json(StatusCode::CONFLICT, failure("this lamp is already set up"));
        }
        // Visible at once, before the background run begins. A last run's
        // address must not send V1's page to the wrong place.
        status.phase = Phase::Connecting;
        status.error.clear();
        status.lan_ip.clear();
    }
    (shared.start)(accepted);
    json(StatusCode::OK, success(serde_json::Value::Bool(true)))
}

async fn networks(State(shared): State<Arc<Shared>>) -> Response {
    let scan = Arc::clone(&shared);
    let listed = tokio::task::spawn_blocking(move || list_networks(&scan)).await;
    match listed {
        Ok(Ok(list)) => json(StatusCode::OK, success(serde_json::to_value(list).unwrap())),
        _ => json(
            StatusCode::SERVICE_UNAVAILABLE,
            failure("the lamp could not look for networks"),
        ),
    }
}

async fn page() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        PAGE,
    )
        .into_response()
}

async fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub fn router(shared: Arc<Shared>) -> Router {
    let mut router = Router::new()
        .route("/api/device/setup", post(setup))
        .route("/api/device/setup/status", get(status))
        .route("/api/network", get(networks));
    if shared.config.page {
        router = router
            .route("/", get(page))
            .route("/setup", get(page))
            // As V1's nginx: the phones' sign-in checks get 204, so no
            // sign-in sheet fights the page.
            .route("/generate_204", get(no_content))
            .route("/gen_204", get(no_content))
            .route("/hotspot-detect.html", get(no_content))
            .route("/ncsi.txt", get(no_content))
            .route("/connecttest.txt", get(no_content));
    }
    router.with_state(shared)
}

#[cfg(test)]
mod tests {
    use super::PAGE;

    /// #2862: the lamp's own setup page asks about reports, with the box
    /// unticked, and sends `share_reports` only from that box.
    #[test]
    fn the_setup_page_asks_about_reports_unticked_and_sends_only_the_box() {
        assert!(PAGE.contains("<input id=\"reports\" type=\"checkbox\">"));
        assert!(!PAGE.contains("id=\"reports\" type=\"checkbox\" checked"));
        assert!(PAGE.contains("Send health and crash reports"));
        assert!(PAGE.contains("share_reports: $(\"reports\").checked,"));
        assert_eq!(PAGE.matches("share_reports").count(), 1);
    }
}
