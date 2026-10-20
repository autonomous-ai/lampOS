//! The one page (#2141's second step, the lead on #1711, 20 Oct): a
//! sidebar of Overview, Turns, Camera, Body and Health, each section kept
//! fresh by the page's one event stream (`/events`). Each section is HTML
//! made here, with everything a message, a record or a unit carries
//! escaped; the page's few lines of script only put it in its place.
use crate::{
    turns::{Step, Turn},
    units::UnitRow,
    view::{self, escape},
    watch::{BodyReport, plain},
};
use lamp_bus::Message;
use lamp_msgs::{ExpressionBody, Faces, Health, JointState, Level, RoomPresence, VoiceState};
use std::fmt::Write;

/// The sections, in the sidebar's order: each one's id and its name.
pub const SECTIONS: [(&str, &str); 5] = [
    ("overview", "Overview"),
    ("turns", "Turns"),
    ("camera", "Camera"),
    ("body", "Body"),
    ("health", "Health"),
];
/// The most characters of a health item's message shown.
pub const MESSAGE_CHARS: usize = 160;

/// What the overview reads.
pub struct Glance<'a> {
    pub voice: Option<&'a Message>,
    pub faces: Option<&'a Message>,
    pub body: Option<&'a Message>,
    pub presence: Option<&'a Message>,
    /// The camera open, or what the page says instead.
    pub camera: &'a Result<(), String>,
    pub health: Option<&'a Message>,
    pub turns: &'a [Turn],
    pub units: &'a [UnitRow],
}

/// The page itself: the sidebar and an empty place for each section, which
/// the stream fills.
pub fn main_page() -> String {
    let mut nav = String::new();
    let mut sections = String::new();
    for (id, name) in SECTIONS {
        let _ = write!(nav, "<a href=\"#{id}\">{name}</a>");
        let _ = write!(
            sections,
            "<section id=\"{id}\"><h2>{name}</h2><div id=\"{id}-now\">waiting for the lamp...</div></section>"
        );
    }
    let ids = SECTIONS
        .iter()
        .map(|(id, _)| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>lamp-peek</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><style>{STYLE}</style></head>\
         <body><nav><b>lamp-peek</b>{nav}<span class=\"small\">read only</span>\
         <a href=\"/bus\">bus</a><a href=\"/status\">units</a></nav><main>{sections}</main>\
         <script>(function(){{var ids=[{ids}];var es=new EventSource('/events');\
         ids.forEach(function(id){{es.addEventListener(id,function(e){{\
         document.getElementById(id+'-now').innerHTML=e.data;}});}});\
         function show(){{var at=(location.hash||'#overview').slice(1);\
         if(ids.indexOf(at)<0){{at='overview';}}\
         ids.forEach(function(id){{document.getElementById(id).hidden=id!==at;}});\
         document.querySelectorAll('nav a').forEach(function(a){{\
         a.className=a.getAttribute('href')==='#'+at?'on':'';}});}}\
         window.addEventListener('hashchange',show);show();}})();</script></body></html>"
    )
}

const STYLE: &str = "body{font-family:sans-serif;margin:0;display:flex;min-height:100vh}\
nav{width:150px;background:#20242a;color:#e8e8e8;padding:12px;display:flex;flex-direction:column;gap:6px}\
nav a{color:#e8e8e8;text-decoration:none;padding:4px 6px;border-radius:4px}nav a.on{background:#3a4048}\
main{flex:1;padding:12px 16px}td,th{padding:2px 8px;text-align:left}.small{font-size:12px;color:#9aa0a6}\
.ok{color:#1b7f3b}.warn{color:#b26a00}.error{color:#c62828}.stale{color:#6d6d6d}\
.slow{background:#fde0dc}.differs{background:#fde0dc}svg{max-width:640px;width:100%}";

/// A level's name and its colour's class.
fn level_class(level: Level) -> &'static str {
    match level {
        Level::Ok => "ok",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Stale => "stale",
    }
}

fn fresh<T: serde::de::DeserializeOwned>(message: Option<&Message>) -> Option<T> {
    message
        .filter(|message| !message.expired)
        .and_then(|message| message.parse().ok())
}

/// `ms` as the page shows it, or a dash.
fn ms(ms: Option<u64>) -> String {
    ms.map_or_else(|| "–".to_owned(), |ms| format!("{ms} ms"))
}

/// The median of `values`, the lower of the two middle ones.
fn median(mut values: Vec<u64>) -> Option<u64> {
    values.sort_unstable();
    values.get(values.len().saturating_sub(1) / 2).copied()
}

/// The overview: the voice, who is there, the camera, the health, the
/// newest turn's first audio and the last ten's median, and any unit
/// running another build than its release.
pub fn overview(glance: &Glance) -> String {
    let mut rows: Vec<(String, String)> = Vec::new();
    let voice = glance.voice.filter(|message| !message.expired).map_or_else(
        || "not known".to_owned(),
        |message| view::latest_what(VoiceState::TOPIC, message),
    );
    rows.push(("voice".into(), escape(&voice)));
    let room = match fresh::<RoomPresence>(glance.presence) {
        Some(presence) if presence.someone => "someone here".to_owned(),
        Some(presence) => format!("nobody for {} s", presence.nobody_s),
        None => "not known".to_owned(),
    };
    rows.push(("room".into(), escape(&room)));
    let seen = match glance.camera {
        Ok(()) => {
            let count = fresh::<Faces>(glance.faces).map(|faces| faces.faces.len());
            let followed = fresh::<ExpressionBody>(glance.body)
                .and_then(|body| body.gaze)
                .map(|gaze| plain(&gaze.face_id));
            match (count, followed) {
                (Some(count), Some(face)) => {
                    format!("camera open, {count} in view, following {face}")
                }
                (Some(count), None) => format!("camera open, {count} in view"),
                (None, _) => "camera open, no look within 2 s".to_owned(),
            }
        }
        Err(why) => why.clone(),
    };
    rows.push(("camera".into(), escape(&seen)));
    let health = match fresh::<Health>(glance.health) {
        Some(health) => {
            let worst = health
                .items
                .iter()
                .find(|item| item.level != Level::Ok)
                .map(|item| format!(": {} {}", plain(&item.name), cut(&item.message)))
                .unwrap_or_default();
            format!(
                "<span class=\"{}\">{}</span>{}",
                level_class(health.level),
                level_class(health.level),
                escape(&worst)
            )
        }
        None => "not known".to_owned(),
    };
    rows.push(("health".into(), health));
    let first = glance.turns.iter().find_map(Turn::first_audio_ms);
    let ten: Vec<u64> = glance
        .turns
        .iter()
        .filter_map(Turn::first_audio_ms)
        .take(10)
        .collect();
    rows.push((
        "first audio".into(),
        format!(
            "newest {}, median of the last {} {}",
            ms(first),
            ten.len(),
            ms(median(ten.clone()))
        ),
    ));
    let differs: Vec<String> = glance
        .units
        .iter()
        .filter(|row| row.running.is_some() && row.release.is_some() && row.running != row.release)
        .map(|row| escape(&row.id))
        .collect();
    rows.push((
        "builds".into(),
        if differs.is_empty() {
            "each unit runs its release".to_owned()
        } else {
            format!(
                "<span class=\"error\">another build than its release: {}</span>",
                differs.join(", ")
            )
        },
    ));
    let mut html = String::from("<table>");
    for (name, value) in rows {
        let _ = write!(html, "<tr><th>{name}</th><td>{value}</td></tr>");
    }
    html.push_str("</table>");
    html
}

/// `message` cut to [`MESSAGE_CHARS`], with a mark when it was.
fn cut(message: &str) -> String {
    let mut chars = message.chars();
    let mut cut: String = chars.by_ref().take(MESSAGE_CHARS).collect();
    if chars.next().is_some() {
        cut.push('…');
    }
    cut
}

/// What a step is called on the page.
fn step_name(step: Step) -> &'static str {
    match step {
        Step::Words => "final words",
        Step::Chat => "first token",
        Step::Audio => "first audio",
    }
}

/// The turns, newest first: each one's steps from the end of speech, the
/// slowest marked, and how it ended or what held it. Numbers and kinds
/// only.
pub fn turns(turns: &[Turn]) -> String {
    if turns.is_empty() {
        return "<p>No turn in the newest session yet.</p>".into();
    }
    let mut html = String::from(
        "<p class=\"small\">Each from the end of speech; a step's own time in brackets. \
         The slowest step is marked.</p><table><tr><th>turn</th><th>taken as</th>\
         <th>final words</th><th>first token</th><th>first audio</th><th>ended</th></tr>",
    );
    for turn in turns {
        let slowest = turn.slowest();
        let mut cells = String::new();
        for ((step, own), at) in
            turn.steps()
                .into_iter()
                .zip([turn.words_ms, turn.chat_ms, turn.audio_ms])
        {
            let at = match (step, at) {
                (Step::Audio, None) => turn.after_decision_ms,
                (_, at) => at,
            };
            let own = own.map_or_else(String::new, |own| format!(" ({own})"));
            let _ = write!(
                cells,
                "<td{} title=\"{}\">{}{own}</td>",
                if slowest == Some(step) {
                    " class=\"slow\""
                } else {
                    ""
                },
                step_name(step),
                ms(at)
            );
        }
        let ended = match (&turn.outcome, &turn.held) {
            (Some(outcome), Some(held)) => format!("{outcome}; {held}"),
            (Some(outcome), None) => outcome.clone(),
            (None, Some(held)) => format!("going on; {held}"),
            (None, None) => "going on".to_owned(),
        };
        let _ = write!(
            html,
            "<tr><td>{}</td><td>{}</td>{cells}<td>{}</td></tr>",
            turn.turn,
            escape(turn.basis.as_deref().unwrap_or("?")),
            escape(&ended)
        );
    }
    html.push_str("</table>");
    html
}

/// The camera's frame, as `/cam.svg` draws it.
pub fn camera(svg: &str) -> String {
    format!("<p class=\"small\">Faces as boxes, never a picture. Nothing is kept.</p>{svg}")
}

/// Each joint where it is against where it was asked to be, and the body's
/// own word on how it is, at `now_unix_ms`.
pub fn body(joints: Option<&Message>, report: Option<&BodyReport>, now_unix_ms: u64) -> String {
    let mut html = String::new();
    match fresh::<JointState>(joints) {
        Some(joints) => {
            html.push_str(
                "<table><tr><th>joint</th><th>at</th><th>asked</th><th>off by</th><th>effort</th></tr>",
            );
            for (index, name) in joints.name.iter().enumerate() {
                let at = joints.position.get(index).copied();
                let asked = joints.commanded.get(index).copied();
                let off = at.zip(asked).map(|(at, asked)| (at - asked).abs());
                let effort = joints.effort.get(index).copied();
                let number = |value: Option<f64>| {
                    value.map_or_else(|| "–".into(), |value| format!("{value:.3}"))
                };
                let _ = write!(
                    html,
                    "<tr{}><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    if off.is_some_and(|off| off > OFF_BY) {
                        " class=\"slow\""
                    } else {
                        ""
                    },
                    escape(&plain(name)),
                    number(at),
                    number(asked),
                    number(off),
                    effort.map_or_else(|| "–".into(), |effort| format!("{:.0}%", effort * 100.0))
                );
            }
            html.push_str(
                "</table><p class=\"small\">In radians; a joint more than 0.05 off where \
                 it was asked to be is marked.</p>",
            );
        }
        None => html.push_str("<p>No joints within 1 s: the body is not telling where it is.</p>"),
    }
    match report {
        Some(report) => {
            let _ = write!(
                html,
                "<h3>The body says <span class=\"{}\">{}</span></h3>",
                level_class(report.level),
                level_class(report.level)
            );
            if !report.message.is_empty() {
                let _ = write!(html, "<p>{}</p>", escape(&cut(&report.message)));
            }
            html.push_str("<table>");
            for (key, value) in &report.values {
                let _ = write!(
                    html,
                    "<tr><th>{}</th><td>{}</td></tr>",
                    escape(key),
                    escape(value)
                );
            }
            html.push_str("</table>");
            if let Some((stop, at_unix_ms)) = &report.last_stop {
                let _ = write!(
                    html,
                    "<p>Its last stop, {} s ago: {}</p>",
                    now_unix_ms.saturating_sub(*at_unix_ms) / 1000,
                    escape(&cut(stop))
                );
            }
        }
        None => html.push_str("<p>No word from the body yet.</p>"),
    }
    html
}

/// How far a joint may be from where it was asked to be, in radians,
/// before the page marks it.
const OFF_BY: f64 = 0.05;

/// Each part's health item as it says it, the parts never heard from, and
/// the units' builds.
pub fn health(health: Option<&Message>, units: &[UnitRow]) -> String {
    let mut html = String::new();
    match fresh::<Health>(health) {
        Some(health) => {
            let _ = write!(
                html,
                "<p>The lamp: <span class=\"{}\">{}</span></p><table><tr><th>part</th><th>level</th>\
                 <th>says</th><th>heard</th></tr>",
                level_class(health.level),
                level_class(health.level)
            );
            for item in &health.items {
                let _ = write!(
                    html,
                    "<tr><td>{}</td><td class=\"{}\">{}</td><td>{}</td><td>{:.1} s ago</td></tr>",
                    escape(&plain(&item.name)),
                    level_class(item.level),
                    level_class(item.level),
                    escape(&cut(&item.message)),
                    item.age_ms as f64 / 1000.0
                );
            }
            html.push_str("</table>");
            if !health.missing.is_empty() {
                let missing: Vec<String> = health
                    .missing
                    .iter()
                    .map(|name| escape(&plain(name)))
                    .collect();
                let _ = write!(
                    html,
                    "<p class=\"stale\">Never heard from: {}</p>",
                    missing.join(", ")
                );
            }
        }
        None => html.push_str("<p>No word from lamp-health within 15 s.</p>"),
    }
    html.push_str(
        "<h3>Units</h3><table><tr><th>unit</th><th>state</th><th>running</th><th>release</th></tr>",
    );
    for row in units {
        let short = |build: &Option<String>| {
            build
                .as_deref()
                .map_or_else(|| "?".to_owned(), |build| build.chars().take(12).collect())
        };
        let differs = row.running.is_some() && row.release.is_some() && row.running != row.release;
        let _ = write!(
            html,
            "<tr{}><td>{}</td><td>{} ({})</td><td>{}</td><td>{}</td></tr>",
            if differs { " class=\"differs\"" } else { "" },
            escape(&row.id),
            escape(&row.active),
            escape(&row.sub),
            escape(&short(&row.running)),
            escape(&short(&row.release))
        );
    }
    html.push_str("</table>");
    html
}
