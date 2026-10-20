//! The server: four views, each read-only and asked for with GET, and the
//! token's one form. With a token, every view asks for it: in the
//! `Authorization` header, or once in the form's body and then in a cookie,
//! never in the address (the first builder on #2141, 10:26: an address
//! lands in a browser's history and a proxy's log). Without one, it answers
//! only to this machine's own names in `Host`, so that no other page in
//! the browser can reach it under a name of its own (DNS rebinding; the
//! reviewer on #2150, P3 6).
use crate::{
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
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use lamp_bus::Bus;
use std::{
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

/// What the server holds: what the views read, and `/status` as last read.
struct Served {
    peek: Arc<Peek>,
    status: Mutex<Option<(Instant, Status)>>,
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
        status: Mutex::new(None),
    });
    Router::new()
        .route("/", get(index))
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
    Html(view::index_page())
}

async fn cam() -> Html<String> {
    Html(view::cam_page())
}

async fn cam_svg(State(served): State<Arc<Served>>) -> Response {
    let peek = &served.peek;
    let camera = {
        let seen = peek.seen.lock().unwrap_or_else(PoisonError::into_inner);
        view::camera(seen.camera.as_ref(), Instant::now())
    };
    let read = |topic| peek.bus.latest(topic).ok().flatten();
    let svg = match camera {
        Ok(()) => view::cam_svg(
            Ok(()),
            read(lamp_msgs::Faces::TOPIC).as_ref(),
            read(lamp_msgs::ExpressionBody::TOPIC).as_ref(),
            read(lamp_msgs::RoomPresence::TOPIC).as_ref(),
        ),
        // With the camera not open, nothing of what it saw is read.
        Err(why) => view::cam_svg(Err(why), None, None, None),
    };
    ([(header::CONTENT_TYPE, "image/svg+xml")], svg).into_response()
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

/// The units as last read, read again once they are [`STATUS_KEPT`] old.
async fn status(State(served): State<Arc<Served>>) -> Html<String> {
    let kept = served
        .status
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|(at, _)| at.elapsed() < STATUS_KEPT);
    let (at, (rows, endpoint)) = match kept {
        Some(kept) => kept,
        None => {
            let (units, sessions) = (Arc::clone(&served.peek.units), served.peek.sessions.clone());
            let read =
                tokio::task::spawn_blocking(move || (units(), units::endpoint_worker(&sessions)))
                    .await
                    .unwrap_or_default();
            let at = Instant::now();
            *served.status.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((at, read.clone()));
            (at, read)
        }
    };
    Html(view::status_page(&rows, endpoint.as_ref(), at.elapsed()))
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
