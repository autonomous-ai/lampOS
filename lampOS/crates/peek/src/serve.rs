//! The server: the one page and its event stream, and three views, each
//! read-only and asked for with GET, and the token's one form. With a
//! token, every view asks for it: in the `Authorization` header, or once
//! in the form's body and then in a cookie, never in the address (the
//! first builder on #2141, 10:26: an address lands in a browser's history
//! and a proxy's log). Without one, it answers only to this machine's own
//! names in `Host`, so that no other page in the browser can reach it
//! under a name of its own (DNS rebinding; the reviewer on #2150, P3 6).
use crate::{
    page,
    turns::{Turn, Turns},
    units::{self, UnitRow},
    view,
    watch::Seen,
};
use axum::{
    Router,
    body::Bytes,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{
        Html, IntoResponse, Redirect, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures_util::stream::{self, Stream, StreamExt};
use lamp_bus::Bus;
use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The cookie that carries the token after the form.
pub const COOKIE: &str = "peek";
/// How long `/status`'s rows are kept before they are read again: a read
/// runs `systemctl` once and `journalctl` for each unit, and reads the
/// newest session's records (the reviewer on #2150, P3 3).
pub const STATUS_KEPT: Duration = Duration::from_secs(30);
/// The names this machine answers to when there is no token.
const LOCAL: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// `/status`'s rows and the end-of-speech worker's state, as read.
type Status = (Vec<UnitRow>, Option<(String, u64)>);

/// What the server holds: what the views read, `/status` as last read, and
/// the turns read so far with when they were last read on.
struct Served {
    peek: Arc<Peek>,
    /// Held across a read, so one read runs however many streams ask at
    /// once (the reviewer on #2259, P3).
    status: tokio::sync::Mutex<Option<(Instant, Status)>>,
    turns: Arc<Mutex<Turns>>,
    turns_read: Mutex<Option<Instant>>,
}

/// What the views read.
pub struct Peek {
    pub bus: Bus,
    pub seen: Arc<Mutex<Seen>>,
    /// Asked of every request when there is one.
    pub token: Option<String>,
    /// Where the voice keeps its sessions.
    pub sessions: PathBuf,
    /// The units, as `/status` lists them: systemd's on the lamp
    /// ([`units::read`]), a stand-in in the tests.
    pub units: Arc<dyn Fn() -> Vec<UnitRow> + Send + Sync>,
}

pub fn router(peek: Arc<Peek>) -> Router {
    let served = Arc::new(Served {
        peek,
        status: tokio::sync::Mutex::new(None),
        turns: Arc::default(),
        turns_read: Mutex::new(None),
    });
    Router::new()
        .route("/", get(index))
        .route("/events", get(events))
        .route("/cam", get(cam))
        .route("/cam.svg", get(cam_svg))
        .route("/bus", get(bus))
        .route("/status", get(status))
        .route("/token", post(token))
        .layer(middleware::from_fn_with_state(Arc::clone(&served), guard))
        .with_state(served)
}

/// Whether `Host` names this machine: `127.0.0.1`, `localhost` or `[::1]`,
/// with or without a port.
fn local_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let name = match host.split_once(']') {
        Some((address, _)) => format!("{address}]"),
        None => host.split(':').next().unwrap_or_default().to_owned(),
    };
    LOCAL.contains(&name.to_ascii_lowercase().as_str())
}

/// `a` and `b` the same, taking as long whatever they share.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// The token a request carries in its header or cookie.
fn carried(headers: &HeaderMap) -> Option<String> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if let Some(bearer) = bearer {
        return Some(bearer.to_owned());
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            pair.trim()
                .strip_prefix(&format!("{COOKIE}="))
                .map(str::to_owned)
        })
}

/// Whether the address names a token: refused even when it is the right one.
fn token_in_address(request: &Request) -> bool {
    request.uri().query().is_some_and(|query| {
        query
            .split('&')
            .any(|pair| pair.split('=').next() == Some("token"))
    })
}

/// Every request: no token in the address, and the token when there is
/// one; and nothing a page answers is kept by the browser or passed on.
async fn guard(State(served): State<Arc<Served>>, request: Request, next: Next) -> Response {
    let peek = &served.peek;
    if peek.token.is_none() && !local_host(request.headers()) {
        return (
            StatusCode::FORBIDDEN,
            "This page answers only as 127.0.0.1, localhost or [::1].",
        )
            .into_response();
    }
    if token_in_address(&request) {
        return (
            StatusCode::BAD_REQUEST,
            Html(view::token_page(
                "A token in the address is refused: use the form.",
            )),
        )
            .into_response();
    }
    let open = request.uri().path() == "/token"
        || peek.token.as_deref().is_none_or(|token| {
            carried(request.headers()).is_some_and(|carried| same(&carried, token))
        });
    let mut response = if open {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Html(view::token_page("This page needs its token.")),
        )
            .into_response()
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}

/// The form's token (`token=...`, form-encoded) to a cookie.
async fn token(State(served): State<Arc<Served>>, body: Bytes) -> Response {
    let Some(token) = served.peek.token.as_deref() else {
        return Redirect::to("/").into_response();
    };
    let given = String::from_utf8_lossy(&body)
        .split('&')
        .find_map(|pair| pair.strip_prefix("token=").map(form_decoded));
    match given {
        Some(given) if same(&given, token) => {
            let mut response = Redirect::to("/").into_response();
            if let Ok(cookie) = HeaderValue::from_str(&format!(
                "{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/"
            )) {
                response.headers_mut().insert(header::SET_COOKIE, cookie);
            }
            response
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Html(view::token_page("That is not this page's token.")),
        )
            .into_response(),
    }
}

/// A form value as it was typed: `+` a space, `%XX` a byte.
fn form_decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        at += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn index() -> Html<String> {
    Html(page::main_page())
}

async fn cam() -> Html<String> {
    Html(view::cam_page())
}

async fn cam_svg(State(served): State<Arc<Served>>) -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        frame(&served.peek),
    )
        .into_response()
}

async fn bus(State(served): State<Arc<Served>>, request: Request) -> Html<String> {
    let peek = &served.peek;
    let filter = request.uri().query().and_then(|query| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix("topic=").map(form_decoded))
            .filter(|filter| !filter.is_empty())
    });
    let latest: Vec<_> = view::LATEST
        .iter()
        .map(|topic| (*topic, peek.bus.latest(topic).ok().flatten()))
        .collect();
    let now_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    let seen = peek.seen.lock().unwrap_or_else(PoisonError::into_inner);
    Html(view::bus_page(
        &latest,
        &seen,
        filter.as_deref(),
        now_unix_ms,
    ))
}

async fn status(State(served): State<Arc<Served>>) -> Html<String> {
    let (at, (rows, endpoint)) = status_now(&served).await;
    Html(view::status_page(&rows, endpoint.as_ref(), at.elapsed()))
}

/// How often the stream pushes the camera's frame; the other sections go
/// with every [`SECTIONS_EVERY`]th push, once a second.
pub const PUSH_EVERY: Duration = Duration::from_millis(250);
pub const SECTIONS_EVERY: u32 = 4;

/// The units as last read, read again once they are [`STATUS_KEPT`] old:
/// by one asker, while the others wait for what it reads.
async fn status_now(served: &Arc<Served>) -> (Instant, Status) {
    let mut status = served.status.lock().await;
    if let Some(kept) = status.as_ref().filter(|(at, _)| at.elapsed() < STATUS_KEPT) {
        return kept.clone();
    }
    let (units, sessions) = (Arc::clone(&served.peek.units), served.peek.sessions.clone());
    let read = tokio::task::spawn_blocking(move || (units(), units::endpoint_worker(&sessions)))
        .await
        .unwrap_or_default();
    let at = Instant::now();
    *status = Some((at, read.clone()));
    (at, read)
}

/// The turns, read on in the newest session first, at most once a second
/// whoever asks.
async fn turns_now(served: &Arc<Served>) -> Vec<Turn> {
    let due = {
        let mut read = served
            .turns_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let due = read.is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
        if due {
            *read = Some(Instant::now());
        }
        due
    };
    if due {
        let (turns, sessions) = (Arc::clone(&served.turns), served.peek.sessions.clone());
        let _ = tokio::task::spawn_blocking(move || {
            turns
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read_on(&sessions);
        })
        .await;
    }
    served
        .turns
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .newest()
}

/// The camera's frame now, as `/cam.svg` draws it.
fn frame(peek: &Peek) -> String {
    let camera = {
        let seen = peek.seen.lock().unwrap_or_else(PoisonError::into_inner);
        view::camera(seen.camera.as_ref(), Instant::now())
    };
    let read = |topic| peek.bus.latest(topic).ok().flatten();
    match camera {
        Ok(()) => view::cam_svg(
            Ok(()),
            read(lamp_msgs::Faces::TOPIC).as_ref(),
            read(lamp_msgs::ExpressionBody::TOPIC).as_ref(),
            read(lamp_msgs::RoomPresence::TOPIC).as_ref(),
        ),
        // With the camera not open, nothing of what it saw is read.
        Err(why) => view::cam_svg(Err(why), None, None, None),
    }
}

/// One push of the stream: the camera's frame, and with every
/// [`SECTIONS_EVERY`]th the other sections.
async fn push(served: &Arc<Served>, count: u32) -> Vec<Event> {
    let peek = &served.peek;
    let mut events = vec![
        Event::default()
            .event("camera")
            .data(page::camera(&frame(peek))),
    ];
    if !count.is_multiple_of(SECTIONS_EVERY) {
        return events;
    }
    let turns = turns_now(served).await;
    let (_, (units, _)) = status_now(served).await;
    let read = |topic| peek.bus.latest(topic).ok().flatten();
    let camera = {
        let seen = peek.seen.lock().unwrap_or_else(PoisonError::into_inner);
        view::camera(seen.camera.as_ref(), Instant::now())
    };
    // With the camera not open, nothing of what it saw is read.
    let (faces, body, presence) = match camera {
        Ok(()) => (
            read(lamp_msgs::Faces::TOPIC),
            read(lamp_msgs::ExpressionBody::TOPIC),
            read(lamp_msgs::RoomPresence::TOPIC),
        ),
        Err(_) => (None, None, None),
    };
    let (voice, health, joints) = (
        read(lamp_msgs::VoiceState::TOPIC),
        read(lamp_msgs::Health::TOPIC),
        read(lamp_msgs::JointState::TOPIC),
    );
    let report = peek
        .seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .body
        .clone();
    let overview = page::overview(&page::Glance {
        voice: voice.as_ref(),
        faces: faces.as_ref(),
        body: body.as_ref(),
        presence: presence.as_ref(),
        camera: &camera,
        health: health.as_ref(),
        turns: &turns,
        units: &units,
    });
    events.push(Event::default().event("overview").data(overview));
    events.push(Event::default().event("turns").data(page::turns(&turns)));
    events.push(Event::default().event("body").data(page::body(
        joints.as_ref(),
        report.as_ref(),
        now_unix_ms(),
    )));
    events.push(
        Event::default()
            .event("health")
            .data(page::health(health.as_ref(), &units)),
    );
    events
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The one page's stream: a push now, then every [`PUSH_EVERY`], for as
/// long as the page is open.
async fn events(
    State(served): State<Arc<Served>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let pushes = stream::unfold((served, 0u32), |(served, count)| async move {
        if count > 0 {
            tokio::time::sleep(PUSH_EVERY).await;
        }
        let events = push(&served, count).await;
        Some((
            stream::iter(events.into_iter().map(Ok)),
            (served, count.wrapping_add(1)),
        ))
    });
    Sse::new(pushes.flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_value_is_read_as_typed() {
        assert_eq!(form_decoded("a+b%21c"), "a b!c");
        assert_eq!(form_decoded("100%"), "100%");
        assert_eq!(form_decoded("%zz"), "%zz");
        assert_eq!(form_decoded("%4"), "%4");
        assert_eq!(form_decoded("%é1"), "%é1");
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
    }
}
