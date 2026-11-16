//! The one page (#2141's second step, the lead on #1711, 20 Oct): a
//! sidebar of Overview, Turns, Camera, Body and Health, each section kept
//! fresh by the page's one event stream (`/events`). Each section is HTML
//! made here, with everything a message, a record or a unit carries
//! escaped; the page's few lines of script only put it in its place.
use crate::{
    timeline::{self, Agent, Decision, DecisionKind, Entry, StepKind},
    turns::{ReadFailure, ReadStatus, Snapshot, Step, Turn},
    units::UnitRow,
    view::{self, escape},
    watch::{BodyReport, plain},
};
use lamp_bus::Message;
use lamp_msgs::{
    AgentState, ExpressionBody, Faces, HandStage, Health, JointState, Level, Light, RoomPresence,
    Sign, TaskStatus, TurnVerdict, VoiceState,
};
use std::fmt::Write;

/// The sections, in the sidebar's order: each one's id and its name.
pub const SECTIONS: [(&str, &str); 7] = [
    ("overview", "Overview"),
    ("timeline", "Timeline"),
    ("turns", "Turns"),
    ("tasks", "Tasks"),
    ("camera", "Camera"),
    ("body", "Body"),
    ("health", "Health"),
];
/// The line at the top of the page while words show (the first builder on
/// #5883, 10 Nov: everyone's words in the room, not only the owner's).
pub const WORDS_SHOWN: &str = "Words anyone says to the lamp show here while this is on.";
/// The most characters of a health item's message shown.
pub const MESSAGE_CHARS: usize = 160;

/// What the overview reads.
pub struct Glance<'a> {
    pub voice: Option<&'a Message>,
    /// The light shown (`light/state`).
    pub light: Option<&'a Message>,
    pub faces: Option<&'a Message>,
    pub body: Option<&'a Message>,
    pub presence: Option<&'a Message>,
    /// The camera open, or what the page says instead.
    pub camera: &'a Result<(), String>,
    pub health: Option<&'a Message>,
    pub turns: &'a Snapshot,
    pub units: &'a [UnitRow],
    /// The Harness tasks the lamp handed over (`tasks/status`).
    pub tasks: Option<&'a TaskStatus>,
}

/// Each section's icon in the nav, in [`SECTIONS`]' order.
const ICONS: [&str; SECTIONS.len()] = ["◉", "☰", "⇄", "✓", "◎", "⚙", "♥"];

/// The page itself: the top bar with the lamp's `name` and `build`, the
/// nav and an empty place for each section, which the stream fills; with
/// `words`, [`WORDS_SHOWN`] on top. Its look is V1's Flow Monitor's dark
/// theme (#7208): no font, script or picture from anywhere else.
pub fn main_page(words: bool, name: &str, build: &str) -> String {
    let top = if words {
        format!("<p class=\"words\">{WORDS_SHOWN}</p>")
    } else {
        String::new()
    };
    let mut nav = String::new();
    let mut sections = String::new();
    for ((id, section), icon) in SECTIONS.into_iter().zip(ICONS) {
        let _ = write!(
            nav,
            "<a href=\"#{id}\"><span class=\"icon\">{icon}</span>{section}</a>"
        );
        let _ = write!(
            sections,
            "<section id=\"{id}\"><h2>{section}</h2><div class=\"card\" id=\"{id}-now\">waiting for the lamp...</div></section>"
        );
    }
    let ids = SECTIONS
        .iter()
        .map(|(id, _)| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",");
    let name = escape(name);
    let build = escape(&build.chars().take(12).collect::<String>());
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>lamp-peek · {name}</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><style>{STYLE}{TURN_STYLE}</style></head>\
         <body><nav><b class=\"brand\">lamp-peek</b>{nav}<span class=\"grow\"></span>\
         <a href=\"/bus\"><span class=\"icon\">≡</span>bus</a><a href=\"/status\"><span class=\"icon\">▤</span>units</a></nav>\
         <div class=\"page\"><header><span id=\"live\" class=\"dot\" title=\"live while the lamp streams\"></span>\
         <b>{name}</b><span class=\"badge badge-grey\">build {build}</span>\
         <span class=\"badge badge-amber\">read only</span></header><main>{top}{sections}</main></div>\
         <script>(function(){{var ids=[{ids}];var es=new EventSource('/events');\
         var dot=document.getElementById('live');var last=0;\
         ids.forEach(function(id){{es.addEventListener(id,function(e){{last=Date.now();\
         var el=document.getElementById(id+'-now');\
         var open=[].map.call(el.querySelectorAll('details[open]'),function(d){{return d.id;}});\
         el.innerHTML=e.data;open.forEach(function(o){{var d=document.getElementById(o);\
         if(d){{d.open=true;}}}});}});}});\
         es.onerror=function(){{last=0;}};\
         setInterval(function(){{dot.className=Date.now()-last<3000?'dot live':'dot';}},500);\
         function show(){{var at=(location.hash||'#overview').slice(1);\
         if(ids.indexOf(at)<0){{at='overview';}}\
         ids.forEach(function(id){{document.getElementById(id).hidden=id!==at;}});\
         document.querySelectorAll('nav a').forEach(function(a){{\
         a.className=a.getAttribute('href')==='#'+at?'on':'';}});}}\
         window.addEventListener('hashchange',show);show();}})();</script></body></html>"
    )
}

/// The page's look: V1's dark tokens (`system/web/src/index.css`'s
/// `.lm-root`, copied) as variables, and the classes the sections use:
/// `card`, `badge` with `badge-amber` (the device), `badge-teal` (the
/// lamp's parts), `badge-blue` (the agent), `badge-green`, `badge-red` and
/// `badge-grey`, `chip`, and the nav's `on`. Under 640 px the nav is a
/// strip on top.
const STYLE: &str = ":root{--bg:#0C0B09;--sidebar:#111009;--card:#17160F;--surface:#1E1D14;\
--border:#2A2820;--border-hi:#3A3828;--amber:#F59E0B;--amber-dim:rgba(245,158,11,0.12);\
--teal:#2DD4BF;--teal-dim:rgba(45,212,191,0.12);--green:#34D399;--green-dim:rgba(52,211,153,0.12);\
--red:#F87171;--red-dim:rgba(248,113,113,0.10);--blue:#60A5FA;--blue-dim:rgba(96,165,250,0.12);\
--text:#F0EEE8;--dim:#9A9080;--muted:#504A3C;color-scheme:dark;}\
*{box-sizing:border-box}body{margin:0;display:flex;min-height:100vh;background:var(--bg);color:var(--text);\
font:14px/1.45 system-ui,-apple-system,'Segoe UI',sans-serif}\
nav{width:190px;flex:none;background:var(--sidebar);border-right:1px solid var(--border);padding:14px 10px;\
display:flex;flex-direction:column;gap:2px;position:sticky;top:0;height:100vh}\
nav .brand{color:var(--amber);padding:4px 10px 12px;letter-spacing:.04em}\
nav a{color:var(--dim);text-decoration:none;padding:7px 10px;border-radius:8px;white-space:nowrap}\
nav a:hover{color:var(--text);background:var(--surface)}\
nav a.on{color:var(--amber);background:var(--amber-dim)}\
.icon{display:inline-block;width:1.4em}.grow{flex:1}\
.page{flex:1;min-width:0;display:flex;flex-direction:column}\
header{display:flex;align-items:center;gap:10px;padding:10px 20px;border-bottom:1px solid var(--border);\
background:var(--sidebar);position:sticky;top:0;z-index:1}\
.dot{width:9px;height:9px;border-radius:50%;background:var(--muted)}\
.dot.live{background:var(--green);box-shadow:0 0 6px var(--green)}\
main{padding:16px 20px;max-width:1100px}h2{font-size:15px;margin:0 0 10px}h3{font-size:14px;margin:14px 0 6px}\
.card{background:var(--card);border:1px solid var(--border);border-radius:12px;padding:12px 14px;margin-bottom:12px;\
overflow-x:auto}\
.badge{display:inline-block;padding:1px 8px;border-radius:999px;font-size:11px;font-weight:600;\
letter-spacing:.04em;border:1px solid currentColor}\
.badge-amber{color:var(--amber);background:var(--amber-dim)}.badge-teal{color:var(--teal);background:var(--teal-dim)}\
.badge-blue{color:var(--blue);background:var(--blue-dim)}.badge-green{color:var(--green);background:var(--green-dim)}\
.badge-red{color:var(--red);background:var(--red-dim)}.badge-grey{color:var(--dim);background:var(--surface)}\
.chip{display:inline-block;padding:1px 8px;margin:2px 4px 2px 0;border-radius:6px;font-size:12px;\
background:var(--surface);border:1px solid var(--border);color:var(--text)}\
table{border-collapse:collapse;width:100%}td,th{padding:5px 8px;text-align:left;border-bottom:1px solid var(--border)}\
th{color:var(--dim);font-weight:500}.small{font-size:12px;color:var(--dim)}\
.ok{color:var(--green)}.warn{color:var(--amber)}.error{color:var(--red)}.stale{color:var(--muted)}\
.slow,.differs{background:var(--red-dim)}.words{background:var(--amber-dim);color:var(--amber);\
border:1px solid var(--border);border-radius:8px;padding:6px 10px}\
svg{max-width:640px;width:100%}\
@media(max-width:640px){body{flex-direction:column}\
nav{width:auto;height:auto;flex-direction:row;overflow-x:auto;position:static;border-right:0;\
border-bottom:1px solid var(--border);padding:6px}nav .brand{display:none}nav .grow{display:none}\
header{padding:8px 16px;flex-wrap:wrap}main{padding:12px 16px}}.card.ended{opacity:.6}";

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
    let light = match fresh::<Light>(glance.light) {
        Some(light) if light.on => format!("on, {} at {}", plain(&light.colour), light.level),
        Some(_) => "off".to_owned(),
        None => "not known".to_owned(),
    };
    rows.push(("light".into(), escape(&light)));
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
    let first = glance.turns.turns.iter().find_map(Turn::first_audio_ms);
    let ten: Vec<u64> = glance
        .turns
        .turns
        .iter()
        .filter_map(Turn::first_audio_ms)
        .take(10)
        .collect();
    rows.push((
        "first audio".into(),
        if glance.turns.status == ReadStatus::Current {
            format!(
                "newest {}, median of the last {} {}",
                ms(first),
                ten.len(),
                ms(median(ten.clone()))
            )
        } else {
            read_notice(glance.turns.status)
        },
    ));
    rows.push(("tasks".into(), tasks_line(glance.tasks)));
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

/// A gap between two turns longer than this draws a break, as V1's
/// session grouping does (`docs/flow-monitor.md`, "Session breaks").
pub const BREAK_MS: u64 = 60_000;

/// The turn cards' own look (#7208): the header line, the IN, DID and OUT
/// lines, the step chips, the break between sessions and ACTIVE's pulse.
const TURN_STYLE: &str = ".turn{padding:10px 12px}\
.turn-head{display:flex;flex-wrap:wrap;align-items:center;gap:6px;margin-bottom:6px}\
.ticon{display:inline-flex;align-items:center;justify-content:center;width:20px;height:20px;\
border-radius:5px;background:var(--surface)}.lat{font-size:12px;font-variant-numeric:tabular-nums}\
.io{margin:3px 0;overflow-wrap:anywhere}.lbl{display:inline-block;width:3.2em;font-size:11px;\
font-weight:600;letter-spacing:.06em;color:var(--muted)}.steps{margin-top:6px}\
.chip.slow{border-color:var(--red)}summary{cursor:pointer;margin-top:4px}\
.follow{margin:8px 0 0 14px;padding-left:10px;border-left:2px solid var(--border-hi)}\
.break{display:flex;align-items:center;gap:8px;color:var(--muted);font-size:11px;margin:2px 0 12px}\
.break::before,.break::after{content:'';flex:1;border-top:1px dashed var(--border-hi)}\
.pulse{animation:pulse 1.4s ease-in-out infinite}@keyframes pulse{50%{opacity:.4}}\
@media(prefers-reduced-motion:reduce){.pulse{animation:none}}";

/// `ms` with V1's colour: green to `green` ms, amber to `amber`, red past.
fn latency(mark: &str, ms: u64, green: u64, amber: u64) -> String {
    let class = if ms <= green {
        "ok"
    } else if ms <= amber {
        "warn"
    } else {
        "error"
    };
    format!("<span class=\"lat {class}\">{mark} {ms} ms</span>")
}

/// ⏱, the whole turn: green to 5 s, amber to 15 s.
fn total(ms: u64) -> String {
    latency("⏱", ms, 5_000, 15_000)
}

/// ⚡, to the first token: green to 3 s, amber to 8 s.
fn first_token(ms: u64) -> String {
    latency("⚡", ms, 3_000, 8_000)
}

/// Whether the voice's record says the answer failed.
fn failed(record: Option<&Turn>) -> bool {
    record.is_some_and(|turn| {
        turn.outcome.as_deref() == Some("failed")
            || turn.held.as_deref() == Some("the answer failed")
    })
}

/// V1's status badge. A turn with no end that is not the newest is not
/// going on any more: it says so, in grey.
fn status(failed: bool, ended: bool, newest: bool) -> &'static str {
    if failed {
        "<span class=\"badge badge-red\">ERROR</span>"
    } else if ended {
        "<span class=\"badge badge-green\">DONE</span>"
    } else if newest {
        "<span class=\"badge badge-amber pulse\">ACTIVE</span>"
    } else {
        "<span class=\"badge badge-grey\">NO END</span>"
    }
}

/// The type icon: someone's words, or the lamp's own.
fn type_icon(spoken: bool) -> &'static str {
    if spoken { "🎙" } else { "✦" }
}

/// The turns, newest first, as V1's cards: why it was taken, its status,
/// ⚡ to the first token, each step from the end of speech with the
/// slowest marked, and how it ended or what held it. Numbers and kinds
/// only.
pub fn turns(snapshot: &Snapshot) -> String {
    let mut html = read_notice(snapshot.status);
    if snapshot.turns.is_empty() {
        if snapshot.status == ReadStatus::Current {
            html.push_str("<p>No turn in the newest session yet.</p>");
        }
        return html;
    }
    html.push_str(
        "<p class=\"small\">Each from the end of speech; a step's own time in brackets. \
         The slowest step is marked.</p>",
    );
    for (index, turn) in snapshot.turns.iter().enumerate() {
        let basis = turn.basis.as_deref().unwrap_or("?");
        let _ = write!(
            html,
            "<div class=\"turn card\"><div class=\"turn-head\"><span class=\"ticon\">{}</span>\
             <b>{}</b>{}{}<span class=\"small\">turn {}</span></div><div class=\"steps\">",
            type_icon(basis == "local_speech"),
            escape(basis),
            status(failed(Some(turn)), turn.outcome.is_some(), index == 0),
            turn.chat_ms.map(first_token).unwrap_or_default(),
            turn.turn,
        );
        let slowest = turn.slowest();
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
                html,
                "<span class=\"chip{}\" title=\"{}\">{} {}{own}</span>",
                if slowest == Some(step) { " slow" } else { "" },
                step_name(step),
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
            "</div><div class=\"io\"><span class=\"lbl\">END</span> {}</div></div>",
            escape(&ended)
        );
    }
    html
}

/// The timeline, newest turn first, as V1's turn cards: the header line
/// (type icon, path, status, ⏱ total and ⚡ first token), then IN, DID and
/// OUT, then each step from the end of speech with the slowest marked. The
/// words heard and said show, escaped, only with `words` (monitor mode,
/// whose [`WORDS_SHOWN`] banner is the page's own, shown once on top by
/// [`main_page`]); otherwise the card says they are hidden, whatever an
/// entry carries. A gap over [`BREAK_MS`] draws a break.
///
/// #7267: the path is the voice's decision (LOCAL with its tools, AGENT
/// with the engine and task, HELD BACK, or an error's code), or `PATH ?`
/// where the bus has none (an older lamp). Clicking a card opens its
/// events: the step chips, then each step of the task's agent (the tool,
/// and with `words` its arguments and result), its reply or error. A
/// steered follow-up shows inside its host's card.
pub fn timeline(entries: &[Entry], records: &Snapshot, words: bool) -> String {
    let mut html = String::new();
    if entries.is_empty() {
        html.push_str("<p>No turn heard yet.</p>");
        return html;
    }
    html.push_str(
        "<p class=\"small\">Each step from the end of speech; the slowest step is marked. \
         Clock times in UTC. Click a turn for its events.</p>",
    );
    let mut newer: Option<&Entry> = None;
    for (index, entry) in entries.iter().enumerate() {
        if host_of(entries, index).is_some() {
            continue;
        }
        if let Some(newer) = newer {
            let last = entry.steps.iter().map(|(at, _)| *at).max().unwrap_or(0);
            let gap = newer.start().saturating_sub(entry.ended_at.unwrap_or(last));
            if gap > BREAK_MS {
                let _ = write!(
                    html,
                    "<div class=\"break\">{} min quiet</div>",
                    gap / 60_000
                );
            }
        }
        newer = Some(entry);
        card(&mut html, entries, index, records, words);
    }
    html
}

/// Where the follow-up at `index` shows: the index of its host, when the
/// host is kept and older (so a card never holds itself).
fn host_of(entries: &[Entry], index: usize) -> Option<usize> {
    let host = entries[index].decision()?.host_request?;
    (index + 1..entries.len()).find(|at| entries[*at].request == host)
}

/// The path badge: what the voice decided.
fn path(decision: Option<&Decision>) -> String {
    let Some(decision) = decision else {
        return "<span class=\"badge badge-grey\">PATH ?</span>".to_owned();
    };
    match decision.kind {
        DecisionKind::Answered if decision.tools.is_empty() => {
            "<span class=\"badge badge-green\">LOCAL</span>".to_owned()
        }
        DecisionKind::Answered => format!(
            "<span class=\"badge badge-green\">LOCAL · {}</span>",
            escape(&decision.tools.join(", "))
        ),
        DecisionKind::Handed => {
            let mut text = format!(
                "AGENT · {}",
                engine_name(decision.engine.as_deref().unwrap_or("?"))
            );
            if let Some(task) = decision.task {
                let _ = write!(text, " · task {task}");
            }
            if let Some(stage) = decision.stage {
                text.push_str(match stage {
                    HandStage::Prepared => " · prepared",
                    HandStage::Started => " · started",
                    HandStage::Declined => " · declined",
                });
            }
            format!("<span class=\"badge badge-blue\">{}</span>", escape(&text))
        }
        DecisionKind::HeldBack => "<span class=\"badge badge-grey\">HELD BACK</span>".to_owned(),
        DecisionKind::Error => format!(
            "<span class=\"badge badge-red\">{}</span>",
            escape(decision.error_code.as_deref().unwrap_or("error"))
        ),
    }
}

/// An engine as people know it.
fn engine_name(engine: &str) -> &str {
    match engine {
        "openclaw" => "OpenClaw",
        "hermes" => "Hermes",
        other => other,
    }
}

/// `words` escaped, or a dash when there are none.
fn or_dash(words: &str) -> String {
    if words.is_empty() {
        "–".to_owned()
    } else {
        escape(words)
    }
}

/// The card of `entries[index]`, with the follow-ups steered under it.
fn card(html: &mut String, entries: &[Entry], index: usize, records: &Snapshot, words: bool) {
    let shown = |kept: &Option<String>| match kept {
        Some(kept) if words => quoted(Some(kept)),
        _ => "<span class=\"small\">words hidden</span>".to_owned(),
    };
    let hidden = "<span class=\"small\">words hidden</span>";
    let entry = &entries[index];
    {
        let mut steps = entry.steps.clone();
        steps.sort_by_key(|(at, _)| *at);
        let record = records.turns.iter().find(|turn| turn.turn == entry.request);
        let ended = entry.speech_ended();
        let heard = steps.iter().find_map(|(_, step)| match step {
            timeline::Step::Heard(kept) => Some(kept),
            _ => None,
        });
        let said = steps.iter().find_map(|(_, step)| match step {
            timeline::Step::Said(kept) => Some(kept),
            _ => None,
        });
        let did: Vec<&String> = steps
            .iter()
            .filter_map(|(_, step)| match step {
                timeline::Step::Did(did) => Some(did),
                _ => None,
            })
            .collect();
        let decision = entry.decision();
        let agent = entry.agent.as_ref();
        let failed = failed(record)
            || decision.is_some_and(|decision| decision.kind == DecisionKind::Error)
            || agent.is_some_and(Agent::failed);
        // A task handed over keeps the turn going until its agent ends.
        let handed = decision.is_some_and(|decision| {
            decision.kind == DecisionKind::Handed && decision.stage != Some(HandStage::Declined)
        });
        let working = handed && !agent.is_some_and(Agent::ended);
        let kind = record
            .and_then(|turn| turn.basis.as_deref())
            .unwrap_or(if heard.is_some() { "voice" } else { "lamp" });
        let _ = write!(
            html,
            "<div class=\"turn card\"><div class=\"turn-head\"><span class=\"ticon\">{}</span>\
             <b>{}</b>{}{}{}{}<span class=\"small\">request {} · {}</span></div>",
            type_icon(heard.is_some()),
            escape(kind),
            path(decision),
            status(
                failed,
                entry.ended_at.is_some() && !working,
                index == 0 || working
            ),
            entry
                .ended_at
                .map(|at| total(at.saturating_sub(entry.start())))
                .unwrap_or_default(),
            decision
                .and_then(|decision| decision.first_reply_ms)
                .or_else(|| record.and_then(|turn| turn.chat_ms))
                .map(first_token)
                .unwrap_or_default(),
            entry.request,
            clock(ended)
        );
        let _ = write!(
            html,
            "<div class=\"io\"><span class=\"lbl\">IN</span> {}</div>",
            heard.map_or_else(
                || "<span class=\"small\">the lamp began it</span>".to_owned(),
                shown
            )
        );
        if !did.is_empty() {
            html.push_str("<div class=\"io\"><span class=\"lbl\">DID</span> ");
            for did in did {
                let _ = write!(html, "<span class=\"chip\">{}</span>", escape(did));
            }
            html.push_str("</div>");
        }
        let mut out = said.map_or_else(
            || "<span class=\"small\">nothing said</span>".to_owned(),
            shown,
        );
        if let Some(record) = record {
            let mut about = Vec::new();
            if let Some(audio) = record.first_audio_ms() {
                about.push(format!("first audio {audio} ms"));
            }
            if let Some(outcome) = &record.outcome {
                about.push(escape(outcome));
            }
            if !about.is_empty() {
                let _ = write!(out, " <span class=\"small\">· {}</span>", about.join(", "));
            }
        }
        let _ = write!(
            html,
            "<div class=\"io\"><span class=\"lbl\">OUT</span> {out}</div>\
             <details id=\"turn-{}-{}\"><summary class=\"small\">{} events</summary>\
             <div class=\"steps small\">",
            entry.request,
            entry.start(),
            steps.len() + agent.map_or(0, |agent| agent.steps.len()),
        );
        // The slowest: the longest wait after the step before.
        let waits: Vec<u64> = steps
            .windows(2)
            .map(|pair| pair[1].0.saturating_sub(pair[0].0))
            .collect();
        let slowest = (waits.len() > 1)
            .then(|| {
                waits
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, wait)| **wait)
                    .map(|(at, _)| at + 1)
            })
            .flatten();
        for (index, (at, step)) in steps.iter().enumerate() {
            let (name, what) = match step {
                timeline::Step::Heard(_) => ("heard", String::new()),
                timeline::Step::Understood(verdict) => {
                    let mut what = match verdict {
                        TurnVerdict::Answered => " answered".to_owned(),
                        TurnVerdict::HeldBack => " held back".to_owned(),
                    };
                    if let Some(record) = record {
                        if let Some(basis) = &record.basis {
                            let _ = write!(what, ", {}", escape(basis));
                        }
                        if let Some(words_ms) = record.words_ms {
                            let _ = write!(what, ", final words {words_ms} ms");
                        }
                    }
                    ("understood", what)
                }
                timeline::Step::Decided(decision) => (
                    "decided",
                    match decision.kind {
                        DecisionKind::Answered => " answered",
                        DecisionKind::Handed => " handed",
                        DecisionKind::HeldBack => " held back",
                        DecisionKind::Error => " error",
                    }
                    .to_owned(),
                ),
                timeline::Step::Did(_) => ("did", String::new()),
                timeline::Step::Said(_) => ("said", String::new()),
            };
            let _ = write!(
                html,
                "<span class=\"chip{}\">{name} +{} ms{what}</span>",
                if slowest == Some(index) { " slow" } else { "" },
                at.saturating_sub(ended),
            );
        }
        html.push_str("</div>");
        // The agent's steps: names and numbers always, words with `words`.
        for step in agent.map_or(&[][..], |agent| &agent.steps) {
            let (label, text) = match step.kind {
                StepKind::Call => (
                    "CALL",
                    format!(
                        "{} · {} · {}",
                        step.n,
                        or_dash(&step.tool),
                        if words {
                            format!("{} → {}", or_dash(&step.args), or_dash(&step.result))
                        } else {
                            hidden.to_owned()
                        }
                    ),
                ),
                StepKind::Reply if words => ("REPLY", or_dash(&step.result)),
                StepKind::Reply => ("REPLY", hidden.to_owned()),
                StepKind::Error => (
                    "ERROR",
                    format!(
                        "{} · {} · {}",
                        step.n,
                        or_dash(&step.tool),
                        if words {
                            or_dash(&step.result)
                        } else {
                            hidden.to_owned()
                        }
                    ),
                ),
            };
            let _ = write!(
                html,
                "<div class=\"io small\"><span class=\"lbl\">{label}</span> {text}</div>"
            );
        }
        match agent {
            Some(agent) if agent.more > 0 => {
                let _ = write!(html, "<div class=\"small\">{} more</div>", agent.more);
            }
            None if handed => html.push_str(
                "<div class=\"io small\"><span class=\"lbl\">AGENT</span> \
                 no step on the bus yet</div>",
            ),
            _ => {}
        }
        html.push_str("</details>");
        let follows: Vec<usize> = (0..index)
            .filter(|at| host_of(entries, *at) == Some(index))
            .collect();
        if !follows.is_empty() {
            html.push_str("<div class=\"follow\">");
            for at in follows {
                card(html, entries, at, records, words);
            }
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }
}

/// Words heard or said, escaped and quoted, when kept.
fn quoted(words: Option<&str>) -> String {
    words.map_or_else(String::new, |words| format!("“{}”", escape(words)))
}

/// A wall clock time as `HH:MM:SS` UTC.
fn clock(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000 % 86_400;
    format!(
        "{:02}:{:02}:{:02} UTC",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Fixed wording only: a failed read must never send a private path or
/// an OS error containing one to the browser.
fn read_notice(status: ReadStatus) -> String {
    let why = match status {
        ReadStatus::Current => return String::new(),
        ReadStatus::Pending => return "<p>Reading the voice's sessions.</p>".into(),
        ReadStatus::Unavailable(why) | ReadStatus::Stale(why) => why,
    };
    let message = match why {
        ReadFailure::Permission => "Can't read the voice's sessions: permission.",
        ReadFailure::Missing => "The voice's sessions are not available yet.",
        ReadFailure::Other => "Can't read the voice's sessions: read failed.",
    };
    let stale = if matches!(status, ReadStatus::Stale(_)) {
        " Last read is stale."
    } else {
        ""
    };
    format!("<p class=\"warn\">{message}{stale}</p>")
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

/// An engine's name as people know it, from its name on the bus
/// (`EngineKind::bus_name`); another name as it came, escaped.
fn engine_name(engine: &str) -> String {
    match engine {
        "openclaw" => "OpenClaw",
        "hermes" => "Hermes",
        "claude" => "Claude",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "devin" => "Devin",
        "devin_cloud" => "Devin Cloud",
        other => return escape(other),
    }
    .to_owned()
}

/// A state's word and its badge's colour; a settled `idle` is `done`.
fn state_badge(state: AgentState, settled: bool) -> (&'static str, &'static str) {
    match state {
        AgentState::Working => ("working", "badge-blue"),
        AgentState::Starting => ("starting", "badge-amber"),
        AgentState::Installing => ("installing", "badge-amber"),
        AgentState::NeedsInput => ("needs input", "badge-amber"),
        AgentState::Idle if settled => ("done", "badge-green"),
        AgentState::Idle => ("idle", "badge-green"),
        AgentState::Failed => ("failed", "badge-red"),
        AgentState::Stopped => ("stopped", "badge-grey"),
        AgentState::Offline => ("offline", "badge-grey"),
        AgentState::Unknown => ("unknown", "badge-grey"),
        AgentState::Missing => ("missing", "badge-grey"),
    }
}

fn sign_name(sign: Sign) -> &'static str {
    match sign {
        Sign::TestsPass => "tests pass",
        Sign::BuildWorks => "build works",
        Sign::Fixed => "fixed",
        Sign::Found => "found the cause",
        Sign::Done => "done",
        Sign::PullRequest => "pull request",
    }
}

/// `seconds` as a person reads it: `40 s`, `5 min`, `2 h 10 min`.
fn span(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h {} min", seconds / 3600, seconds / 60 % 60),
    }
}

/// The overview's tasks line: how many are working, and per engine of the
/// tasks it lists (watched or ended), how many of them, in the order
/// they were handed over (`none working` when none is), then how many
/// were just handed over, if any. HTML: an engine's name is escaped.
pub fn tasks_line(status: Option<&TaskStatus>) -> String {
    let Some(status) = status else {
        return "not known".to_owned();
    };
    let mut engines: Vec<(&str, usize)> = Vec::new();
    // In the order the tasks were handed over.
    let mut named: Vec<(u64, &str, bool)> = status
        .tasks
        .iter()
        .map(|task| {
            (
                task.task,
                task.engine.as_str(),
                task.state == AgentState::Working,
            )
        })
        .chain(
            status
                .settled
                .iter()
                .map(|task| (task.task, task.engine.as_str(), false)),
        )
        .collect();
    named.sort_unstable();
    for (_, engine, working) in named {
        match engines.iter_mut().find(|(name, _)| *name == engine) {
            Some((_, count)) => *count += usize::from(working),
            None => engines.push((engine, usize::from(working))),
        }
    }
    let working: usize = engines.iter().map(|(_, count)| count).sum();
    let each: Vec<String> = engines
        .iter()
        .map(|(engine, count)| format!("{} {count}", engine_name(engine)))
        .collect();
    let mut line = if working == 0 {
        "none working".to_owned()
    } else {
        format!("{working} working ({})", each.join(", "))
    };
    if !status.unread.is_empty() {
        let _ = write!(line, ", {} just handed over", status.unread.len());
    }
    line
}

/// The Harness tasks the lamp handed over, at `now_unix_ms`: a card for
/// each watched task with its engine, state, start, last activity, changes
/// and signs; one for each just handed over; and, greyed under Ended, each
/// that settled, with how and when. Only `tasks/status`'s typed fields: a
/// task's words, prompt, folder and answer are never on the bus, and
/// nothing else about tasks is read.
pub fn tasks(status: Option<&TaskStatus>, now_unix_ms: u64) -> String {
    let Some(status) = status else {
        return "<p>No word from lamp-tasks: its progress (tasks/status) is not on the bus.</p>"
            .to_owned();
    };
    if status.tasks.is_empty() && status.unread.is_empty() && status.settled.is_empty() {
        return "<p>Nothing is being worked on.</p>".to_owned();
    }
    let mut html = String::new();
    if status.tasks.is_empty() && status.unread.is_empty() {
        html.push_str("<p>Nothing is being worked on.</p>");
    }
    for task in &status.tasks {
        let (state, colour) = state_badge(task.state, false);
        let _ = write!(
            html,
            "<div class=\"card task\"><b>task {}</b> <span class=\"badge\">{}</span>\
             <span class=\"badge {colour}\">{state}</span><div class=\"small\">",
            task.task,
            engine_name(&task.engine)
        );
        let mut said = Vec::new();
        if let Some(working_s) = task.working_s {
            said.push(format!("started {} ago", span(working_s)));
        }
        if let Some(at) = task.last_active_unix_ms {
            said.push(format!(
                "last active {} ago",
                span(now_unix_ms.saturating_sub(at) / 1000)
            ));
        }
        said.push(format!(
            "{} change{}",
            task.changes,
            if task.changes == 1 { "" } else { "s" }
        ));
        html.push_str(&said.join(" · "));
        html.push_str("</div>");
        for sign in &task.signs {
            let _ = write!(html, "<span class=\"chip\">{}</span>", sign_name(*sign));
        }
        html.push_str("</div>");
    }
    for task in &status.unread {
        let _ = write!(
            html,
            "<div class=\"card task\"><b>task {task}</b> \
             <span class=\"badge badge-amber\">just handed over</span></div>"
        );
    }
    if !status.settled.is_empty() {
        html.push_str("<h3>Ended</h3>");
    }
    for task in &status.settled {
        let (state, colour) = state_badge(task.state, true);
        let _ = write!(
            html,
            "<div class=\"card task ended\"><b>task {}</b> <span class=\"badge\">{}</span>\
             <span class=\"badge {colour}\">{state}</span>\
             <div class=\"small\">ended {}, {} ago</div></div>",
            task.task,
            engine_name(&task.engine),
            clock(task.at_unix_ms),
            span(now_unix_ms.saturating_sub(task.at_unix_ms) / 1000)
        );
    }
    html
}
