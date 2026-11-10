//! The app's onboarding pages, done on the lamp (#6076, the app owner's
//! ask): the app tells which page is open with [`KIND`], `{"page": "try" |
//! "add" | "create" | null, "added"?: [name], "walk_id"?}`, and goes on to
//! the next page about 1 s after the lamp pushes [`DONE_KIND`], one unasked
//! `data` reply on `fd_channel`, `{"page", "how"?, "walk_id"?}`, as
//! `onboarding.step` is pushed.
//!
//! - **try:** the first Try line run while the page is open: the light
//!   turned on or to another colour since the page opened (`light/state`,
//!   `how: "turn_blue"` for blue, else `"light"`), or a `dance` run done
//!   since (`skills/events`, `how: "dance"`). The lamp then says
//!   [`TRY_DONE`] and pushes the page done, once.
//! - **add:** the pick is sent as the page is left, so the lamp says one
//!   line for it ([`ADDED`], else [`ADD_DONE`]) and pushes the page done.
//! - **create:** nothing: the app follows `skills.made` (#5985).
//!
//! The line goes to the voice's say socket as a note, as a skill's say
//! does: said in the person's language, only once nobody is talking and the
//! lamp's own line has ended, never over another line (the say service's
//! gates, `crate::say` in lamp-voice). Behind `onboarding.done`'s switch
//! (`LAMP_APP_ONBOARDING_DONE`), with `onboarding_page_supported: true` in
//! `system.info`. No names, no words heard.
use lamp_msgs::{Light, SkillEvent, SkillStage};
use serde_json::{Value, json};
use std::sync::Mutex;

/// The kind the app tells its open page by.
pub const KIND: &str = "onboarding.page";

/// The kind the lamp tells a page done by.
pub const DONE_KIND: &str = "onboarding.page_done";

/// The asker's name on the say socket.
pub const SAY_SKILL: &str = "onboarding-app";

/// Said after the first Try line ran.
pub const TRY_DONE: &str = "That's your first skill.";

/// Said for a skill picked on the Add page, by its name.
pub const ADDED: [(&str, &str); 1] = [("sip-of-water", "I'll remind you to sip water.")];

/// Said for any other pick on the Add page.
pub const ADD_DONE: &str = "Got it. Those are on now.";

/// How long the voice may hold the line before it is let go, in ms.
const SAY_EXPIRES_MS: u64 = 30_000;

/// An app page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Try,
    Add,
    Create,
}

/// The page open, and what the lamp showed as it opened.
struct Open {
    page: Page,
    walk_id: Option<Value>,
    /// `light/state`'s on and colour as the page opened.
    light: Option<(bool, String)>,
    /// `skills/events`' last message as the page opened: its publisher's
    /// boot and seq.
    run: Option<(String, u64)>,
    /// The Add page's line.
    line: String,
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
pub type Run = ((String, u64), SkillEvent);

impl Pages {
    /// The app's `data`, with the light shown and the last skill run now.
    pub fn open(
        &mut self,
        data: &Value,
        light: Option<&Light>,
        run: Option<&(String, u64)>,
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
                _ => return Err(PAGE.into()),
            },
            Some(_) => return Err(PAGE.into()),
        };
        let added: Vec<&str> = data
            .get("added")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let line = match added.as_slice() {
            [one] => ADDED
                .iter()
                .find(|(name, _)| name == one)
                .map_or(ADD_DONE, |(_, line)| line),
            _ => ADD_DONE,
        };
        self.open = Some(Open {
            page,
            walk_id: data.get("walk_id").filter(|id| id.is_u64()).cloned(),
            light: light.map(|light| (light.on, light.colour.clone())),
            run: run.cloned(),
            line: line.to_owned(),
        });
        Ok(json!({}))
    }

    /// The page done now, its reply's `data` and the line to say: Try once
    /// the light changed or a dance ran since it opened, Add at once, once.
    pub fn done(&mut self, light: Option<&Light>, run: Option<&Run>) -> Option<(Value, String)> {
        let open = self.open.as_ref()?;
        let (how, line) = match open.page {
            Page::Create => return None,
            Page::Add => (None, open.line.clone()),
            Page::Try => {
                let danced = run.is_some_and(|(key, run)| {
                    open.run.as_ref() != Some(key)
                        && run.skill == "dance"
                        && run.stage == SkillStage::Done
                        && !run.why.starts_with("not made")
                });
                // ponytail: a light already blue as the page opened is not
                // seen to turn blue; the dance still counts.
                let lit = light.filter(|light| {
                    light.on && open.light.as_ref() != Some(&(light.on, light.colour.clone()))
                });
                let how = match (danced, lit) {
                    (true, _) => "dance",
                    (false, Some(light)) if light.colour == "blue" => "turn_blue",
                    (false, Some(_)) => "light",
                    (false, None) => return None,
                };
                (Some(how), TRY_DONE.to_owned())
            }
        };
        let open = self.open.take()?;
        let mut data = json!({"page": if open.page == Page::Add { "add" } else { "try" }});
        if let Some(how) = how {
            data["how"] = json!(how);
        }
        if let Some(walk_id) = open.walk_id {
            data["walk_id"] = walk_id;
        }
        Some((data, line))
    }
}

/// Why a page is refused.
const PAGE: &str = "page is try, add, create or null";

/// The say request for `line`: a note, said as written in the person's
/// language.
pub fn say_request(line: &str) -> Value {
    json!({
        "skill": SAY_SKILL,
        "note": format!("(Say only this, nothing before or after it: \"{line}\")"),
        "expires_in_ms": SAY_EXPIRES_MS,
    })
}

/// Hands `line` to the voice's say socket (`LAMP_SAY_SOCKET`, else lamp-voice's
/// default), on a thread of its own: nothing here waits for it to be said.
pub fn say(line: &str) {
    let request = say_request(line).to_string() + "\n";
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

    /// #6076: Try is done by the light turned blue, or by a dance run after
    /// the page opened, once; nothing before it counts.
    #[test]
    fn try_is_done_by_the_first_try_line_run_after_it_opened() {
        let warm = light(true, "warm");
        let before = run(4, "dance", "danced");
        let mut pages = Pages::default();
        pages
            .open(
                &json!({"page": "try", "walk_id": 7}),
                Some(&warm),
                Some(&before.0),
            )
            .unwrap();
        // The light unchanged and the dance from before: not yet.
        assert!(pages.done(Some(&warm), Some(&before)).is_none());
        // A dance not made (one under way) is no run.
        assert!(
            pages
                .done(
                    Some(&warm),
                    Some(&run(
                        5,
                        "dance",
                        "not made: its last run is still under way"
                    ))
                )
                .is_none()
        );
        let (data, line) = pages
            .done(Some(&light(true, "blue")), Some(&before))
            .unwrap();
        assert_eq!(
            data,
            json!({"page": "try", "how": "turn_blue", "walk_id": 7})
        );
        assert_eq!(line, TRY_DONE);
        // Once.
        assert!(
            pages
                .done(Some(&light(true, "red")), Some(&before))
                .is_none()
        );
        pages
            .open(&json!({"page": "try"}), Some(&warm), Some(&before.0))
            .unwrap();
        let (data, _) = pages
            .done(Some(&warm), Some(&run(5, "dance", "danced")))
            .unwrap();
        assert_eq!(data, json!({"page": "try", "how": "dance"}));
    }

    /// #6076: Add is done at once with its pick's line; Create never;
    /// `null` closes the page; an unknown page is refused.
    #[test]
    fn add_says_its_pick_create_waits_and_null_closes() {
        let mut pages = Pages::default();
        pages
            .open(
                &json!({"page": "add", "added": ["sip-of-water"]}),
                None,
                None,
            )
            .unwrap();
        let (data, line) = pages.done(None, None).unwrap();
        assert_eq!(data, json!({"page": "add"}));
        assert_eq!(line, "I'll remind you to sip water.");
        assert!(pages.done(None, None).is_none());
        pages
            .open(
                &json!({"page": "add", "added": ["stretch", "posture"]}),
                None,
                None,
            )
            .unwrap();
        assert_eq!(pages.done(None, None).unwrap().1, ADD_DONE);
        pages.open(&json!({"page": "create"}), None, None).unwrap();
        assert!(
            pages
                .done(Some(&light(true, "blue")), Some(&run(9, "dance", "danced")))
                .is_none()
        );
        pages.open(&json!({"page": "try"}), None, None).unwrap();
        pages.open(&json!({"page": null}), None, None).unwrap();
        assert!(pages.done(Some(&light(true, "blue")), None).is_none());
        assert!(pages.open(&json!({"page": "next"}), None, None).is_err());
    }

    /// #6076: the line goes to the say socket as a note.
    #[test]
    fn the_line_is_a_note_for_the_say_socket() {
        let asked = say_request(TRY_DONE);
        assert_eq!(asked["skill"], SAY_SKILL);
        assert!(
            asked["note"]
                .as_str()
                .unwrap()
                .contains("\"That's your first skill.\"")
        );
        assert!(asked.get("text").is_none());
    }
}
