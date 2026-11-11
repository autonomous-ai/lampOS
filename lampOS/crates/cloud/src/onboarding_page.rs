//! The app's onboarding pages, done on the lamp (#6076, the app owner's
//! ask; one line a page tied to the app, #6353): the app tells which page
//! is open with [`KIND`], `{"page": "try" | "add" | "create" | "done" |
//! null, "added"?: [id], "walk_id"?}`. The lamp says one line for the page
//! and pushes [`DONE_KIND`] once a page, as its first line is handed out,
//! one unasked `data` reply on `fd_channel`, `{"page", "how"?, "walk_id"?}`,
//! as `onboarding.step` is pushed.
//!
//! - **try:** the first Try line run while the page is open: the light
//!   turned on or to another colour since the page opened (`light/state`,
//!   `how: "turn_blue"` for blue, else `"light"`), or a `dance` run done
//!   since (`skills/events`, `how: "dance"`). The lamp says [`TRY_COLOUR`]
//!   with the colour set ([`COLOUR_NAMES`]), else [`TRY_DONE`], and the
//!   page is done, once. A turn said to the lamp (`voice/turn`, answered)
//!   with no light change [`NUDGE_WAIT`] after it gets [`TRY_NUDGE`], once
//!   a page.
//! - **add:** one message a tap, `"added": [id]`: the skill's own name
//!   from the lamp's installed skills (`skills.list`'s `title`, never the
//!   app's words) in [`ADDED_FIRST`] for the page's first, [`ADDED_MORE`]
//!   after. An id not installed gets nothing; the lamp installs nothing.
//!   A #6076 app (`onboarding_page_version` < 2) sends its picks as it
//!   leaves: an `added` of none or several ids ends the page, done with
//!   the first installed id's line, or at once with no line if none is
//!   installed.
//! - **create:** nothing here: lamp-voice says the made skill's own line
//!   (`made_skill::on_line`, "I made <Name>, and it's on.").
//! - **done:** [`DONE`], once.
//!
//! Each line goes to the voice's say socket: as written (`exact`, no chat)
//! for an English owner; as a note in the person's language otherwise,
//! these lines having no translation (`crate::say` in lamp-voice: said only
//! once nobody is talking and the lamp's own line has ended). Behind
//! `onboarding.done`'s switch (`LAMP_APP_ONBOARDING_DONE`), with
//! `onboarding_page_supported: true` and [`VERSION`] in `system.info`. No
//! words heard.
use lamp_msgs::{Light, SkillEvent, SkillStage};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The kind the app tells its open page by.
pub const KIND: &str = "onboarding.page";

/// The kind the lamp tells a page done by.
pub const DONE_KIND: &str = "onboarding.page_done";

/// `system.info`'s `onboarding_page_version`: 2 is this per-tap Add and
/// the `done` page (#6353); an app that reads none is a #6076 one.
pub const VERSION: u64 = 2;

/// The asker's name on the say socket.
pub const SAY_SKILL: &str = "onboarding-app";

// The pages' lines (#6353), one place each: `{colour}` and `{name}` are
// filled in as said.

/// Said after the first Try line ran, the colour not known (a dance).
pub const TRY_DONE: &str = "That's your first skill. Tap Next in the app.";

/// Said after the light was turned on or to a colour on Try.
pub const TRY_COLOUR: &str = "{colour} it is! That's your first skill. Tap Next in the app.";

/// Said once a Try page to a turn that changed no light.
pub const TRY_NUDGE: &str = "Want to try a skill? Say turn blue.";

/// Said for the Add page's first skill added.
pub const ADDED_FIRST: &str =
    "{name} is on! Add more skills if you like, or tap Next to make your own.";

/// Said for each later skill added on the same Add page.
pub const ADDED_MORE: &str = "{name} is on too.";

/// Said once as the app's last page opens.
pub const DONE: &str =
    "You're a skill master now! Keep exploring, just say Hey Lamp anytime. Have fun!";

/// Each of `Light::COLOURS` as [`TRY_COLOUR`] says it.
pub const COLOUR_NAMES: [(&str, &str); 10] = [
    ("yellow", "Yellow"),
    ("red", "Red"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("cyan", "Cyan"),
    ("purple", "Purple"),
    ("orange", "Orange"),
    ("pink", "Pink"),
    ("white", "White"),
    ("warm", "Warm white"),
];

/// How long the voice may hold the line before it is let go, in ms.
const SAY_EXPIRES_MS: u64 = 30_000;

/// How long an open page may wait to be done: an app killed or sent to the
/// background never sends `null`, and its page is let go after this.
const LAPSE: Duration = Duration::from_secs(10 * 60);

/// How long after a turn said to the lamp on Try the light may still
/// change before [`TRY_NUDGE`] is said: the turn's answer runs the light.
pub const NUDGE_WAIT: Duration = Duration::from_secs(8);

/// An app page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Try,
    Add,
    Create,
    Done,
}

impl Page {
    fn name(self) -> &'static str {
        match self {
            Self::Try => "try",
            Self::Add => "add",
            Self::Create => "create",
            Self::Done => "done",
        }
    }
}

/// A bus message's key: its publisher's boot and seq.
pub type Key = (String, u64);

/// What the lamp showed as a page opened, and what it read later.
#[derive(Clone, Copy, Default)]
pub struct Seen<'a> {
    /// `light/state`.
    pub light: Option<&'a Light>,
    /// `skills/events`' last message.
    pub run: Option<&'a Run>,
    /// `voice/turn`'s last answered turn.
    pub turn: Option<&'a Key>,
}

/// The page open, and what the lamp showed as it opened.
struct Open {
    page: Page,
    walk_id: Option<Value>,
    /// `light/state`'s on and colour as the page opened.
    light: Option<(bool, String)>,
    /// `skills/events`' last message as the page opened.
    run: Option<Key>,
    /// The last answered turn looked at, and when a new one was seen.
    turn: Option<Key>,
    heard_at: Option<Instant>,
    nudged: bool,
    /// Skills added on this Add page.
    added: usize,
    /// Lines owed, in order.
    lines: VecDeque<String>,
    /// [`DONE_KIND`] pushed for this page.
    told: bool,
    /// A #6076 leave with no id installed: done with no line.
    end: bool,
    /// When the page opened, for [`LAPSE`].
    opened_at: Instant,
}

/// The page the app has open.
#[derive(Default)]
pub struct Pages {
    open: Option<Open>,
}

/// The one lamp-cloud's open page: one app, one walk at a time.
// ponytail: one process-wide page; keyed by app if two apps ever onboard one lamp.
pub static PAGES: Mutex<Pages> = Mutex::new(Pages { open: None });

/// A skill run seen on `skills/events`: its publisher's boot and seq, and
/// the run.
pub type Run = (Key, SkillEvent);

/// A line due now: the [`DONE_KIND`] reply's `data` when the page is done
/// with it, and the words (none for a #6076 leave with nothing installed).
pub type Due = (Option<Value>, String);

/// The skill `id`'s own name in `skills.list`'s reply (`catalogue`), with
/// nothing that could close a quoted line; none for one not installed.
pub fn name_in(catalogue: &Value, id: &str) -> Option<String> {
    let row = catalogue["skills"]
        .as_array()?
        .iter()
        .find(|row| row["name"] == id)?;
    let name: String = row["title"]
        .as_str()
        .unwrap_or(id)
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '«' | '»'))
        .collect();
    Some(name.trim().to_owned()).filter(|name| !name.is_empty())
}

impl Pages {
    /// The app's `data`, with what the lamp shows now and the installed
    /// skills' names (`name`, an Add page's only).
    pub fn open(
        &mut self,
        data: &Value,
        seen: Seen,
        name: impl Fn(&str) -> Option<String>,
    ) -> Result<Value, String> {
        let page = match data.get("page") {
            None | Some(Value::Null) => {
                self.open = None;
                return Ok(json!({}));
            }
            Some(Value::String(page)) => match page.as_str() {
                "try" => Page::Try,
                "add" => Page::Add,
                "create" => Page::Create,
                "done" => Page::Done,
                _ => return Err(PAGE.into()),
            },
            Some(_) => return Err(PAGE.into()),
        };
        let same = self.is_open() && self.open.as_ref().is_some_and(|open| open.page == page);
        // Add's taps and a done sent again go on the page already open.
        if !(same && matches!(page, Page::Add | Page::Done)) {
            self.open = Some(Open {
                page,
                walk_id: data.get("walk_id").filter(|id| id.is_u64()).cloned(),
                light: seen.light.map(|light| (light.on, light.colour.clone())),
                run: seen.run.map(|(key, _)| key.clone()),
                turn: seen.turn.cloned(),
                heard_at: None,
                nudged: false,
                added: 0,
                lines: if page == Page::Done {
                    VecDeque::from([DONE.to_owned()])
                } else {
                    VecDeque::new()
                },
                told: false,
                end: false,
                opened_at: Instant::now(),
            });
        }
        let open = self.open.as_mut().ok_or(PAGE)?;
        if page == Page::Add {
            let added = data.get("added").and_then(Value::as_array);
            let ids: Vec<&str> = added
                .map(|ids| ids.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            // An `added` of none or several ids is a #6076 app's picks on
            // leaving: the page ends with them (a per-tap app sends one).
            let left = added.is_some_and(|added| added.len() != 1);
            let before = open.added;
            for &id in &ids {
                if let Some(name) = name(id) {
                    let line = if open.added == 0 {
                        ADDED_FIRST
                    } else {
                        ADDED_MORE
                    };
                    open.lines.push_back(line.replace("{name}", &name));
                    open.added += 1;
                }
            }
            open.end |= left && open.added == before;
        }
        Ok(json!({}))
    }

    /// Whether a page is open, a page open longer than [`LAPSE`] let go.
    pub fn is_open(&mut self) -> bool {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.opened_at.elapsed() > LAPSE)
        {
            self.open = None;
        }
        self.open.is_some()
    }

    /// The line due now, if any: an owed one (Add, done) in turn; Try's
    /// once the light changed or a dance ran since it opened, closing the
    /// page, or its nudge; nothing once the page has lapsed.
    pub fn done(&mut self, seen: Seen) -> Option<Due> {
        if !self.is_open() {
            return None;
        }
        let open = self.open.as_mut()?;
        if let Some(line) = open.lines.pop_front() {
            return Some((open.told(None), line));
        }
        if std::mem::take(&mut open.end) {
            return Some((open.told(None), String::new()));
        }
        if open.page != Page::Try {
            return None;
        }
        let danced = seen.run.is_some_and(|(key, run)| {
            open.run.as_ref() != Some(key)
                && run.skill == "dance"
                && run.stage == SkillStage::Done
                && !run.why.starts_with("not made")
        });
        // ponytail: a light already blue as the page opened is not
        // seen to turn blue; the dance still counts.
        let lit = seen.light.filter(|light| {
            light.on && open.light.as_ref() != Some(&(light.on, light.colour.clone()))
        });
        let (how, line) = match (danced, lit) {
            (true, _) => ("dance", TRY_DONE.to_owned()),
            (false, Some(light)) => (
                if light.colour == "blue" {
                    "turn_blue"
                } else {
                    "light"
                },
                COLOUR_NAMES
                    .iter()
                    .find(|(id, _)| *id == light.colour)
                    .map_or(TRY_DONE.to_owned(), |(_, name)| {
                        TRY_COLOUR.replace("{colour}", name)
                    }),
            ),
            (false, None) => return open.nudge(seen.turn),
        };
        let data = open.told(Some(how));
        self.open = None;
        Some((data, line))
    }
}

impl Open {
    /// This page's [`DONE_KIND`] `data`, the first time only.
    fn told(&mut self, how: Option<&str>) -> Option<Value> {
        if std::mem::replace(&mut self.told, true) {
            return None;
        }
        let mut data = json!({"page": self.page.name()});
        if let Some(how) = how {
            data["how"] = json!(how);
        }
        if let Some(walk_id) = &self.walk_id {
            data["walk_id"] = walk_id.clone();
        }
        Some(data)
    }

    /// Try's nudge: once a page, [`NUDGE_WAIT`] after a new answered turn
    /// that changed no light.
    fn nudge(&mut self, turn: Option<&Key>) -> Option<Due> {
        if self.nudged {
            return None;
        }
        if turn.is_some() && self.turn.as_ref() != turn {
            self.turn = turn.cloned();
            self.heard_at = Some(Instant::now());
        }
        if self.heard_at?.elapsed() < NUDGE_WAIT {
            return None;
        }
        self.nudged = true;
        Some((None, TRY_NUDGE.to_owned()))
    }
}

/// Why a page is refused.
const PAGE: &str = "page is try, add, create, done or null";

/// The say request for `line`: as written for an English owner (`exact`),
/// else a note the model says in the person's language.
pub fn say_request(line: &str, english: bool) -> Value {
    if english {
        return json!({
            "skill": SAY_SKILL,
            "text": line,
            "exact": true,
            "expires_in_ms": SAY_EXPIRES_MS,
        });
    }
    json!({
        "skill": SAY_SKILL,
        "note": format!("(Say only this, nothing before or after it: \"{line}\")"),
        "expires_in_ms": SAY_EXPIRES_MS,
    })
}

/// Hands `line` to the voice's say socket (`LAMP_SAY_SOCKET`, else lamp-voice's
/// default), on a thread of its own: nothing here waits for it to be said.
pub fn say(line: &str, english: bool) {
    let request = say_request(line, english).to_string() + "\n";
    let socket =
        std::env::var_os("LAMP_SAY_SOCKET").unwrap_or_else(|| "/run/lampos-say/say.sock".into());
    std::thread::spawn(move || {
        use std::io::{BufRead, Write};
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&socket) else {
            return;
        };
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(
            SAY_EXPIRES_MS + 10_000,
        )));
        if stream.write_all(request.as_bytes()).is_ok() {
            // The outcome, once known; the connection stays open until then.
            let mut outcome = String::new();
            let mut lines = std::io::BufReader::new(stream);
            while lines.read_line(&mut outcome).is_ok_and(|read| read > 0)
                && outcome.contains("\"begun\"")
            {
                outcome.clear();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(on: bool, colour: &str) -> Light {
        Light {
            on,
            colour: colour.into(),
            level: 3,
        }
    }

    fn run(seq: u64, skill: &str, why: &str) -> Run {
        (
            ("b".into(), seq),
            SkillEvent {
                skill: skill.into(),
                stage: SkillStage::Done,
                why: why.into(),
                allowed: None,
            },
        )
    }

    fn lit(light: &Light) -> Seen<'_> {
        Seen {
            light: Some(light),
            ..Seen::default()
        }
    }

    /// The installed skills, as `skills.list` names them.
    fn installed(id: &str) -> Option<String> {
        let catalogue = json!({"skills": [
            {"name": "sip-of-water", "title": "Sip of water"},
            {"name": "rest-your-eyes", "title": "Rest your \"eyes\""},
        ]});
        name_in(&catalogue, id)
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    /// #6076, #6353: Try is done by the light turned to a colour, said by
    /// its name, or by a dance run after the page opened, once; nothing
    /// before it counts.
    #[test]
    fn try_names_the_colour_set_once() {
        let warm = light(true, "warm");
        let before = run(4, "dance", "danced");
        let mut pages = Pages::default();
        let opened = Seen {
            light: Some(&warm),
            run: Some(&before),
            turn: None,
        };
        pages
            .open(&json!({"page": "try", "walk_id": 7}), opened, none)
            .unwrap();
        // The light unchanged and the dance from before: not yet.
        assert!(pages.done(opened).is_none());
        // A dance not made (one under way) is no run.
        let not_made = run(5, "dance", "not made: its last run is still under way");
        assert!(
            pages
                .done(Seen {
                    run: Some(&not_made),
                    ..opened
                })
                .is_none()
        );
        let blue = light(true, "blue");
        let (data, line) = pages.done(lit(&blue)).unwrap();
        assert_eq!(
            data,
            Some(json!({"page": "try", "how": "turn_blue", "walk_id": 7}))
        );
        assert_eq!(
            line,
            "Blue it is! That's your first skill. Tap Next in the app."
        );
        // Once.
        assert!(pages.done(lit(&light(true, "red"))).is_none());
        pages.open(&json!({"page": "try"}), opened, none).unwrap();
        let pink = light(true, "pink");
        assert_eq!(
            pages.done(lit(&pink)).unwrap().1,
            "Pink it is! That's your first skill. Tap Next in the app."
        );
        // A colour with no name here, and a dance: the line without one.
        pages.open(&json!({"page": "try"}), opened, none).unwrap();
        let (data, line) = pages.done(lit(&light(true, "teal"))).unwrap();
        assert_eq!(data, Some(json!({"page": "try", "how": "light"})));
        assert_eq!(line, TRY_DONE);
        pages.open(&json!({"page": "try"}), opened, none).unwrap();
        let danced = run(5, "dance", "danced");
        let (data, line) = pages
            .done(Seen {
                run: Some(&danced),
                ..opened
            })
            .unwrap();
        assert_eq!(data, Some(json!({"page": "try", "how": "dance"})));
        assert_eq!(line, TRY_DONE);
    }

    /// #6353, ask 4: a turn said to the lamp on Try that changes no light
    /// gets the nudge once a page, [`NUDGE_WAIT`] after it; never before,
    /// and never for the turn before the page opened.
    #[test]
    fn try_nudges_once_after_a_turn_with_no_light_change() {
        let warm = light(true, "warm");
        let before: Key = ("b".into(), 1);
        let mut pages = Pages::default();
        let opened = Seen {
            light: Some(&warm),
            turn: Some(&before),
            ..Seen::default()
        };
        pages.open(&json!({"page": "try"}), opened, none).unwrap();
        let back = |pages: &mut Pages| {
            let open = pages.open.as_mut().unwrap();
            if let Some(at) = open.heard_at.as_mut() {
                *at = Instant::now().checked_sub(NUDGE_WAIT).unwrap();
            }
        };
        // The turn from before the page: nothing.
        assert!(pages.done(opened).is_none());
        back(&mut pages);
        assert!(pages.done(opened).is_none());
        let asked: Key = ("b".into(), 2);
        let after = Seen {
            turn: Some(&asked),
            ..opened
        };
        // A new turn, the light may still change: not yet.
        assert!(pages.done(after).is_none());
        back(&mut pages);
        assert_eq!(pages.done(after), Some((None, TRY_NUDGE.to_owned())));
        // Once a page, whatever is said after.
        let again: Key = ("b".into(), 3);
        let later = Seen {
            turn: Some(&again),
            ..opened
        };
        assert!(pages.done(later).is_none());
        back(&mut pages);
        assert!(pages.done(later).is_none());
        // The page is still open: the light still does it, its page done.
        let blue = light(true, "blue");
        assert_eq!(
            pages.done(lit(&blue)).unwrap().0,
            Some(json!({"page": "try", "how": "turn_blue"}))
        );
    }

    /// #6353: Add says each tap's skill by its own installed name, the
    /// first of the page with what comes next, each later one short; its
    /// page done goes once, with the first.
    #[test]
    fn add_says_the_first_skill_then_each_later_one_short() {
        let mut pages = Pages::default();
        let tap = |pages: &mut Pages, id: &str| {
            pages
                .open(
                    &json!({"page": "add", "added": [id]}),
                    Seen::default(),
                    installed,
                )
                .unwrap();
        };
        tap(&mut pages, "sip-of-water");
        assert_eq!(
            pages.done(Seen::default()),
            Some((
                Some(json!({"page": "add"})),
                "Sip of water is on! Add more skills if you like, or tap Next to make your own."
                    .to_owned()
            ))
        );
        assert!(pages.done(Seen::default()).is_none());
        tap(&mut pages, "rest-your-eyes");
        assert_eq!(
            pages.done(Seen::default()),
            Some((None, "Rest your eyes is on too.".to_owned()))
        );
        // A new Add page starts over.
        pages
            .open(&json!({"page": null}), Seen::default(), none)
            .unwrap();
        tap(&mut pages, "rest-your-eyes");
        assert!(
            pages
                .done(Seen::default())
                .unwrap()
                .1
                .ends_with("make your own.")
        );
    }

    /// #6353: an id the lamp has not installed gets no line, and is not
    /// the page's first; nothing is installed.
    #[test]
    fn an_id_not_installed_gets_no_line() {
        let mut pages = Pages::default();
        pages
            .open(
                &json!({"page": "add", "added": ["glow-green"]}),
                Seen::default(),
                installed,
            )
            .unwrap();
        assert!(pages.done(Seen::default()).is_none());
        pages
            .open(
                &json!({"page": "add", "added": ["sip-of-water"]}),
                Seen::default(),
                installed,
            )
            .unwrap();
        assert!(
            pages
                .done(Seen::default())
                .unwrap()
                .1
                .ends_with("make your own.")
        );
    }

    /// #6366: a #6076 app's picks sent on leaving (none or several ids)
    /// still end the page: with the first installed id's line, else at
    /// once, no line. A page opened with no `added` waits for taps.
    #[test]
    fn an_old_apps_picks_on_leaving_end_the_page() {
        let mut pages = Pages::default();
        let leave = |pages: &mut Pages, ids: Value| {
            pages
                .open(
                    &json!({"page": "add", "added": ids}),
                    Seen::default(),
                    installed,
                )
                .unwrap();
        };
        pages
            .open(&json!({"page": "add"}), Seen::default(), installed)
            .unwrap();
        assert!(pages.done(Seen::default()).is_none());
        leave(&mut pages, json!([]));
        assert_eq!(
            pages.done(Seen::default()),
            Some((Some(json!({"page": "add"})), String::new()))
        );
        pages
            .open(&json!({"page": null}), Seen::default(), none)
            .unwrap();
        leave(&mut pages, json!(["glow-green", "glow-red"]));
        assert_eq!(
            pages.done(Seen::default()),
            Some((Some(json!({"page": "add"})), String::new()))
        );
        assert!(pages.done(Seen::default()).is_none());
        pages
            .open(&json!({"page": null}), Seen::default(), none)
            .unwrap();
        leave(
            &mut pages,
            json!(["glow-green", "sip-of-water", "rest-your-eyes"]),
        );
        let (data, line) = pages.done(Seen::default()).unwrap();
        assert_eq!(data, Some(json!({"page": "add"})));
        assert!(line.starts_with("Sip of water is on!"), "{line}");
    }

    /// #6353: Create says nothing here (lamp-voice says the made line);
    /// done says its line once; `null` closes the page; an unknown page is
    /// refused.
    #[test]
    fn create_waits_done_says_once_and_null_closes() {
        let mut pages = Pages::default();
        let blue = light(true, "blue");
        pages
            .open(&json!({"page": "create"}), Seen::default(), none)
            .unwrap();
        assert!(pages.done(lit(&blue)).is_none());
        pages
            .open(&json!({"page": "done"}), Seen::default(), none)
            .unwrap();
        assert_eq!(
            pages.done(Seen::default()),
            Some((Some(json!({"page": "done"})), DONE.to_owned()))
        );
        pages
            .open(&json!({"page": "done"}), Seen::default(), none)
            .unwrap();
        assert!(pages.done(Seen::default()).is_none());
        pages
            .open(&json!({"page": "try"}), Seen::default(), none)
            .unwrap();
        pages
            .open(&json!({"page": null}), Seen::default(), none)
            .unwrap();
        assert!(pages.done(lit(&blue)).is_none());
        assert!(
            pages
                .open(&json!({"page": "next"}), Seen::default(), none)
                .is_err()
        );
    }

    /// Reviewer-lead's P2 on #6132: a Try page left open by an app killed
    /// before it sent `null` is let go after [`LAPSE`]; a light turned on
    /// later is no first skill.
    #[test]
    fn a_page_open_past_the_lapse_is_let_go() {
        let mut pages = Pages::default();
        let off = light(false, "warm");
        pages
            .open(&json!({"page": "try"}), lit(&off), none)
            .unwrap();
        assert!(pages.is_open());
        pages.open.as_mut().unwrap().opened_at = Instant::now()
            .checked_sub(LAPSE + Duration::from_secs(1))
            .unwrap();
        assert!(pages.done(lit(&light(true, "blue"))).is_none());
        assert!(!pages.is_open());
    }

    /// #6353: an English owner's line goes as written, no chat; any other
    /// owner's as a note, these lines having no translation.
    #[test]
    fn the_line_is_exact_for_english_else_a_note() {
        let asked = say_request(DONE, true);
        assert_eq!(asked["skill"], SAY_SKILL);
        assert_eq!(asked["text"], DONE);
        assert_eq!(asked["exact"], true);
        let asked = say_request(DONE, false);
        assert!(asked["note"].as_str().unwrap().contains(DONE));
        assert!(asked.get("text").is_none());
    }
}
