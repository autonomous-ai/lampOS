//! The pages, as text: the camera's frame with the faces as boxes, the
//! bus, and the units. Everything a message or a unit carries is escaped
//! before it is put in a page.
use crate::{
    units::UnitRow,
    watch::{self, Seen},
};
use lamp_bus::Message;
use lamp_msgs::{ExpressionBody, Faces, JointState, RoomPresence, VoiceState};
use std::{
    fmt::Write,
    time::{Duration, Instant},
};

/// The kept values the page reads, newest each time it is asked.
pub const LATEST: [&str; 5] = [
    VoiceState::TOPIC,
    Faces::TOPIC,
    RoomPresence::TOPIC,
    ExpressionBody::TOPIC,
    JointState::TOPIC,
];
/// The frame the boxes are drawn on: the detector's picture, 320 by 256
/// (`crates/eyes/src/picture.rs`), of which each box is a fraction.
pub const WIDTH: f32 = 320.0;
pub const HEIGHT: f32 = 256.0;
/// How long the eyes' word on the camera holds: they say it a little more
/// often than once a second.
pub const CAMERA_HOLDS: Duration = Duration::from_secs(3);
/// How often the frame is asked for again.
const CAM_EVERY_MS: u32 = 250;

/// `text` safe inside an HTML or SVG element or attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

/// Whether the camera is open as far as the page may draw: the eyes said
/// `open` within [`CAMERA_HOLDS`]. Otherwise, what to say instead.
pub fn camera(said: Option<&(String, Instant)>, now: Instant) -> Result<(), String> {
    match said {
        Some((state, at)) if now.saturating_duration_since(*at) < CAMERA_HOLDS => {
            if state == "open" {
                Ok(())
            } else {
                Err(format!("camera {state}"))
            }
        }
        Some(_) => Err("no word from the eyes for 3 s".into()),
        None => Err("no word from the eyes yet".into()),
    }
}

/// What the camera's frame shows now: the faces as boxes, the face the
/// lamp follows and its point, and who is there. While the camera is not
/// open ([`camera`]) only that is said: no box, no point, no presence.
pub fn cam_svg(
    camera: Result<(), String>,
    faces: Option<&Message>,
    body: Option<&Message>,
    presence: Option<&Message>,
) -> String {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
         font-family=\"sans-serif\" font-size=\"11\">\
         <rect class=\"frame\" width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"#20242a\"/>"
    );
    if let Err(why) = camera {
        let _ = write!(
            svg,
            "<text class=\"closed\" x=\"{}\" y=\"{}\" fill=\"#e8e8e8\" font-size=\"18\" \
             text-anchor=\"middle\">{}</text></svg>",
            WIDTH / 2.0,
            HEIGHT / 2.0,
            escape(&why)
        );
        return svg;
    }
    let gaze = fresh::<ExpressionBody>(body).and_then(|body| body.gaze);
    match fresh::<Faces>(faces) {
        Some(faces) => {
            for face in &faces.faces {
                let followed = gaze.as_ref().is_some_and(|gaze| gaze.face_id == face.id);
                let (x, y) = (face.xmin * WIDTH, face.ymin * HEIGHT);
                let (w, h) = (
                    (face.xmax - face.xmin) * WIDTH,
                    (face.ymax - face.ymin) * HEIGHT,
                );
                let mut label = escape(&watch::plain(&face.id));
                match face.facing {
                    Some(true) => label.push_str(" facing"),
                    Some(false) => label.push_str(" turned away"),
                    None => {}
                }
                if face
                    .speaking
                    .as_ref()
                    .is_some_and(|speaking| speaking.state == lamp_msgs::SpeakingState::Yes)
                {
                    label.push_str(" speaking");
                }
                if followed {
                    label.push_str(" (followed)");
                }
                let colour = if followed { "#ffb000" } else { "#4fc3f7" };
                let _ = write!(
                    svg,
                    "<rect class=\"face\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" \
                     height=\"{h:.1}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\"/>\
                     <text x=\"{x:.1}\" y=\"{:.1}\" fill=\"{colour}\">{label}</text>",
                    (y - 3.0).max(10.0)
                );
            }
            if faces.faces.is_empty() {
                svg.push_str("<text x=\"8\" y=\"16\" fill=\"#9aa0a6\">no face in view</text>");
            }
        }
        None => svg.push_str("<text x=\"8\" y=\"16\" fill=\"#9aa0a6\">no look within 2 s</text>"),
    }
    if let Some(gaze) = gaze {
        let _ = write!(
            svg,
            "<circle class=\"gaze\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"5\" fill=\"none\" \
             stroke=\"#ffb000\" stroke-width=\"2\"/>",
            (gaze.x + 1.0) / 2.0 * WIDTH,
            (gaze.y + 1.0) / 2.0 * HEIGHT
        );
    }
    let caption = match fresh::<RoomPresence>(presence) {
        Some(presence) if presence.someone => "someone here".to_owned(),
        Some(presence) => format!("nobody for {} s", presence.nobody_s),
        None => "presence not known".to_owned(),
    };
    let _ = write!(
        svg,
        "<text x=\"8\" y=\"{}\" fill=\"#e8e8e8\">{caption}</text></svg>",
        HEIGHT - 8.0
    );
    svg
}

/// A kept value that has not run out, as a `T`.
fn fresh<T: serde::de::DeserializeOwned>(message: Option<&Message>) -> Option<T> {
    message
        .filter(|message| !message.expired)
        .and_then(|message| message.parse().ok())
}

/// The camera's page: the frame, asked for again a few times a second.
pub fn cam_page() -> String {
    page(
        "camera",
        None,
        true,
        &format!(
            "<p>Faces as boxes, never a picture. Nothing is kept.</p>\
             <img id=\"cam\" src=\"/cam.svg\" alt=\"the camera's frame\" width=\"640\" height=\"512\">\
             <script>setInterval(function(){{document.getElementById('cam').src='/cam.svg?'+Date.now();}},{CAM_EVERY_MS});</script>"
        ),
    )
}

/// What a kept value says, in a few words: states and numbers, never words.
pub fn latest_what(topic: &str, message: &Message) -> String {
    let unreadable = || "unreadable".to_owned();
    match topic {
        VoiceState::TOPIC => message
            .parse::<VoiceState>()
            .map_or_else(|_| unreadable(), |state| name_of(&state.state)),
        Faces::TOPIC => message.parse::<Faces>().map_or_else(
            |_| unreadable(),
            |faces| format!("{} in view", faces.faces.len()),
        ),
        RoomPresence::TOPIC => message.parse::<RoomPresence>().map_or_else(
            |_| unreadable(),
            |presence| {
                if presence.someone {
                    "someone".into()
                } else {
                    format!("nobody for {} s", presence.nobody_s)
                }
            },
        ),
        ExpressionBody::TOPIC => message.parse::<ExpressionBody>().map_or_else(
            |_| unreadable(),
            |body| match (body.hold, body.gaze) {
                (true, _) => "held".into(),
                (false, Some(gaze)) => format!("following {}", watch::plain(&gaze.face_id)),
                (false, None) => "no gaze".into(),
            },
        ),
        JointState::TOPIC => message.parse::<JointState>().map_or_else(
            |_| unreadable(),
            |joints| {
                joints
                    .name
                    .iter()
                    .zip(&joints.position)
                    .map(|(name, at)| format!("{} {at:.2}", watch::plain(name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ),
        _ => format!("{} bytes", message.payload.len()),
    }
}

fn name_of(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(watch::plain))
        .unwrap_or_else(|| "?".into())
}

/// How long ago `at_unix_ms` was, as a person reads it.
fn ago(now_unix_ms: u64, at_unix_ms: u64) -> String {
    let ms = now_unix_ms.saturating_sub(at_unix_ms);
    if ms < 10_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} s", ms / 1000)
    }
}

/// The bus's page: the kept values ([`LATEST`]), the events' counts, and
/// the newest events first, each only when its topic holds `filter`.
pub fn bus_page(
    latest: &[(&str, Option<Message>)],
    seen: &Seen,
    filter: Option<&str>,
    now_unix_ms: u64,
) -> String {
    let shown = |topic: &str| filter.is_none_or(|filter| topic.contains(filter));
    let mut body = String::from(
        "<form method=\"get\" action=\"/bus\"><input name=\"topic\" placeholder=\"topic holds...\">\
         <button>filter</button></form><h2>Kept values</h2>\
         <table><tr><th>topic</th><th>from</th><th>what</th><th>ago</th></tr>",
    );
    for (topic, message) in latest.iter().filter(|(topic, _)| shown(topic)) {
        match message {
            Some(message) => {
                let _ = write!(
                    body,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}{}</td></tr>",
                    escape(topic),
                    escape(&watch::plain(&message.header.source)),
                    escape(&latest_what(topic, message)),
                    ago(now_unix_ms, message.header.at_unix_ms),
                    if message.expired { " (ran out)" } else { "" }
                );
            }
            None => {
                let _ = write!(
                    body,
                    "<tr><td>{}</td><td></td><td>nothing kept</td><td></td></tr>",
                    escape(topic)
                );
            }
        }
    }
    body.push_str(
        "</table><h2>Events</h2><table><tr><th>topic</th><th>messages</th><th>last</th></tr>",
    );
    for topic in watch::EVENTS.iter().filter(|topic| shown(topic)) {
        let count = seen.counts.get(*topic).copied().unwrap_or_default();
        let last = if count.messages == 0 {
            String::new()
        } else {
            ago(now_unix_ms, count.last_unix_ms)
        };
        let _ = write!(
            body,
            "<tr><td>{}</td><td>{}</td><td>{last}</td></tr>",
            escape(topic),
            count.messages
        );
    }
    body.push_str(
        "</table><h2>Newest first</h2><table><tr><th>ago</th><th>topic</th><th>from</th><th>what</th></tr>",
    );
    for line in seen.tail.iter().rev().filter(|line| shown(&line.topic)) {
        let _ = write!(
            body,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            ago(now_unix_ms, line.at_unix_ms),
            escape(&line.topic),
            escape(&line.source),
            escape(&line.what)
        );
    }
    body.push_str("</table>");
    page("bus", Some(1), true, &body)
}

/// The units' page, read `age` ago: each unit's state, its running build
/// against its release, and systemd's last error line about it; and the
/// voice's end-of-speech worker.
pub fn status_page(rows: &[UnitRow], endpoint: Option<&(String, u64)>, age: Duration) -> String {
    let mut body = format!(
        "<p>Read {} s ago; read again every {} s.</p>\
         <table><tr><th>unit</th><th>state</th><th>running</th><th>release</th>\
         <th>systemd's last error</th></tr>",
        age.as_secs(),
        crate::serve::STATUS_KEPT.as_secs()
    );
    for row in rows {
        let short = |build: &Option<String>| {
            build
                .as_deref()
                .map_or_else(|| "?".to_owned(), |build| build.chars().take(12).collect())
        };
        let differs = row.running.is_some() && row.release.is_some() && row.running != row.release;
        let _ = write!(
            body,
            "<tr{}><td>{}</td><td>{} ({})</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            if differs { " class=\"differs\"" } else { "" },
            escape(&row.id),
            escape(&row.active),
            escape(&row.sub),
            escape(&short(&row.running)),
            escape(&short(&row.release)),
            escape(row.last_error.as_deref().unwrap_or(""))
        );
    }
    body.push_str("</table><h2>The voice's end-of-speech worker</h2>");
    match endpoint {
        Some((state, at_us)) => {
            let _ = write!(
                body,
                "<p>{} (at {at_us} µs of the newest session)</p>",
                escape(&watch::plain(state))
            );
        }
        None => body.push_str("<p>no state in the newest session</p>"),
    }
    page("status", Some(5), true, &body)
}

/// The list of views.
pub fn index_page() -> String {
    page(
        "lamp-peek",
        None,
        true,
        "<p>Read only: nothing here changes the lamp.</p><ul>\
         <li><a href=\"/cam\">camera</a>: faces as boxes</li>\
         <li><a href=\"/bus\">bus</a>: states and counts</li>\
         <li><a href=\"/status\">status</a>: units and builds</li></ul>",
    )
}

/// What a request without the token gets: a form that sends it in the
/// body, never in the address (the first builder on #2141, 10:26).
pub fn token_page(why: &str) -> String {
    page(
        "token",
        None,
        false,
        &format!(
            "<p>{}</p><form method=\"post\" action=\"/token\">\
             <input type=\"password\" name=\"token\" autocomplete=\"off\">\
             <button>open</button></form>",
            escape(why)
        ),
    )
}

/// A whole page around `body`, asked for again every `refresh` seconds,
/// with the views' links when `nav` (not on the page that asks for the
/// token, which shows nothing of the lamp).
fn page(title: &str, refresh: Option<u32>, nav: bool, body: &str) -> String {
    let refresh = refresh.map_or_else(String::new, |seconds| {
        format!("<meta http-equiv=\"refresh\" content=\"{seconds}\">")
    });
    let nav = if nav {
        "<p><a href=\"/\">lamp-peek</a> · <a href=\"/cam\">camera</a> · \
         <a href=\"/bus\">bus</a> · <a href=\"/status\">status</a></p>"
    } else {
        ""
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">{refresh}\
         <title>{title}</title><style>body{{font-family:sans-serif;margin:16px}}\
         td,th{{padding:2px 8px;text-align:left}}tr.differs{{background:#fde0dc}}</style></head>\
         <body>{nav}{body}</body></html>"
    )
}
