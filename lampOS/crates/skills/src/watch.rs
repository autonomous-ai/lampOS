//! A watch: a skill that is on only while a person has asked for it, and
//! tells them by text who came to the desk while they were out. Guard mode
//! is the first (#1685, `docs/guard-mode.md`); "was anyone here while I was
//! out?" can count the same visits.
//!
//! - **On only when asked, by voice.** Its tool's `[choice]` turns it `on`
//!   or `off`. It is turned on only when the lamp heard someone speak within
//!   `[watch] asked_within` of the call (a request typed in the app is not
//!   heard), and only when its mark can be shown. It never starts itself: a
//!   watch has no trigger, and a load or a restart leaves it off.
//! - **A sign while on.** Its faint `mark` is asked of the ring while it is
//!   on, so nobody is watched without a sign.
//! - **Visits.** It counts a visit only once the desk has been empty since
//!   it was asked (`room/presence`, nobody): the person who asked has left.
//!   A visit is someone at the lamp, from when `room/presence` says so (or
//!   a face is seen near) until it says nobody again.
//! - **One text a while.** At a visit's end it sends one line, "Someone was
//!   at your desk at 14:31, for about 2 minutes.", at most once each
//!   `[watch] every`. Who it was is said only as "someone you've introduced
//!   me to" or "someone I don't know", never a name.
//! - **Ends.** On its tool's `off`, or when the eyes know the face of the
//!   person who asked again, after they had left.
//! - **The privacy switch wins.** With the switch closed it can't see: it
//!   says so once by text and is quiet after. While it is closed the watch
//!   itself takes nothing it is told about the desk, whoever says it: no
//!   visit begins or ends, nothing is texted, and the asker's face ends
//!   nothing (reviewer-two's P3 on #1807): the eyes closing with the switch
//!   is not what keeps it.
//!
//! No picture is read or kept: only `room/presence` (counts and times), the
//! faces' sizes on `humans/faces/tracked`, and whether the face at the lamp
//! is known, from the eyes' known file, which the program reads. A name read
//! there is kept in the program only to tell the person who asked from
//! anyone else: it is never in a text, an event or the journal.
use crate::{Call, LightFile, Skill, duration_us, light_call};
use lamp_msgs::Faces;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The topics a watch listens to: who is at the lamp, the faces in view,
/// and what was heard (a call is turned on only just after someone spoke).
pub const LISTENS: [&str; 3] = ["room/presence", "humans/faces/tracked", "voice/heard"];
/// The privacy switch, read as the watch goes: `physical_muted` is the
/// switch closed. Said a hundred times a second, so looked at, never
/// listened to.
pub const PRIVACY: &str = "voice/privacy";
/// A face at least this share of the picture's height is someone at the
/// desk: the eyes recognise from 0.10 (`lamp-eyes`, `NEAR_ENOUGH`).
pub const NEAR: f32 = 0.10;
/// The tool's two values.
pub const ON: &str = "on";
pub const OFF: &str = "off";

/// The one line said by text when the switch is closed (#1685).
pub const BLIND: &str = "I can't see right now, the privacy switch is closed.";

const ON_NOTE: &str = "Say in one short line that you'll keep an eye on things while they're out and text them if someone comes to the desk.";
const ON_NOTE_UNKNOWN: &str = "Say in one short line that you'll keep an eye on things while they're out and text them if someone comes to the desk. You don't know their face, so ask them to tell you to stop when they're back.";
const ALREADY_ON: &str = "You're already keeping an eye on things. Say so in one short line.";
const OFF_NOTE: &str = "Say in one short line that you've stopped keeping an eye on things.";
const WAS_OFF: &str = "You weren't keeping an eye on things. Say so in one short line.";
/// Refusals, in words the lamp can say.
const NOT_ASKED: &str = "I keep an eye on things only when someone asks me out loud, at the lamp.";
const NO_MARK: &str = "I can't show that I'm keeping an eye on things just now, so I won't do it.";

/// `[watch]` as `skill.toml` says it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WatchFile {
    /// At most one text in this long.
    every: String,
    /// Turned on only when someone was heard at the lamp this lately.
    asked_within: String,
    /// The faint light shown while it is on.
    mark: LightFile,
}

/// A watch as loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub every_us: u64,
    pub asked_within_us: u64,
    /// A light, at level 1 or 2, steady.
    pub mark: Call,
}

/// `[watch]` checked: a text at most every 1 minute to a day, asked within 5
/// seconds to 2 minutes, and a faint, steady mark.
pub(crate) fn spec(file: WatchFile) -> Result<Spec, String> {
    let every_us = duration_us(&file.every)?;
    if !(60_000_000..=86_400_000_000).contains(&every_us) {
        return Err("[watch] every is 1m to 24h".into());
    }
    let asked_within_us = duration_us(&file.asked_within)?;
    if !(5_000_000..=120_000_000).contains(&asked_within_us) {
        return Err("[watch] asked_within is 5s to 2m".into());
    }
    let mark = light_call(file.mark)?;
    match &mark {
        Call::Light {
            level: Some(1 | 2),
            lasting: None,
            pattern: None,
            keep: false,
            until_ends: false,
            ..
        } => {}
        _ => {
            return Err(
                "[watch] mark is a faint light: a colour at level 1 or 2, steady, with no for and not kept"
                    .into(),
            );
        }
    }
    Ok(Spec {
        every_us,
        asked_within_us,
        mark,
    })
}

/// What a watch does.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Show the faint mark, this light, until [`Out::Unmark`].
    Mark(Call),
    /// Give the mark back.
    Unmark,
    /// One short line for the person's phone, on `messages/out`.
    Text(String),
    /// What happened, for `skills/events` and the journal: never a name.
    Said(String),
}

/// Someone at the desk while the watch is on.
#[derive(Debug)]
struct Visit {
    began_us: u64,
    /// The local time it began, in minutes of the day, if known.
    began_minute: Option<u16>,
    /// The last moment someone was there.
    seen_us: u64,
    /// A face was seen near.
    face: bool,
    /// A face the eyes know, not the person who asked, was seen.
    known: bool,
}

impl Visit {
    /// "Someone was at your desk at 14:31, for about 2 minutes.": who only
    /// as known or not, never a name.
    fn text(&self) -> String {
        let who = if self.known {
            "Someone you've introduced me to"
        } else if self.face {
            "Someone I don't know"
        } else {
            "Someone"
        };
        let at = self.began_minute.map_or(String::new(), |minute| {
            format!(" at {}:{:02}", minute / 60, minute % 60)
        });
        let seconds = self.seen_us.saturating_sub(self.began_us) / 1_000_000;
        let long = match (seconds + 30) / 60 {
            _ if seconds < 60 => "for under a minute".to_owned(),
            1 => "for about a minute".to_owned(),
            minutes => format!("for about {minutes} minutes"),
        };
        format!("{who} was at your desk{at}, {long}.")
    }
}

/// A watch that is on.
#[derive(Debug)]
struct On {
    /// The known person who asked, if the eyes knew them: their face back
    /// ends the watch. Kept in the program only.
    asker: Option<String>,
    /// The desk has been empty since it was asked: visits count from then.
    away: bool,
    visit: Option<Visit>,
    /// When the last text about a visit went.
    texted_us: Option<u64>,
    /// The privacy switch's line was sent.
    blind_told: bool,
    /// The privacy switch is closed: nothing about the desk is taken.
    closed: bool,
}

impl On {
    fn visit(&mut self, now_us: u64, minute: Option<u16>) -> &mut Visit {
        let visit = self.visit.get_or_insert(Visit {
            began_us: now_us,
            began_minute: minute,
            seen_us: now_us,
            face: false,
            known: false,
        });
        visit.seen_us = now_us;
        visit
    }

    /// A visit over: its text, unless one went within `every`.
    fn ended(&mut self, visit: Visit, now_us: u64, spec: &Spec) -> Vec<Out> {
        if let Some(texted_us) = self.texted_us
            && now_us.saturating_sub(texted_us) < spec.every_us
        {
            return vec![Out::Said(format!(
                "a visit not texted: one text each {} s, and the last went {} s ago",
                spec.every_us / 1_000_000,
                now_us.saturating_sub(texted_us) / 1_000_000
            ))];
        }
        self.texted_us = Some(now_us);
        vec![Out::Text(visit.text())]
    }
}

/// One watch: off, or on with what it has seen.
#[derive(Debug, Default)]
pub struct Watch {
    on: Option<On>,
}

impl Watch {
    pub fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// Its tool's call: `on` only when someone was heard at the lamp within
    /// `asked_within` (`heard_us`) and the mark can be shown; `off` always.
    /// `asker` is the known person at the lamp as it is asked. The model's
    /// answer and what to do, or why not, in words the lamp can say.
    pub fn turn(
        &mut self,
        on: bool,
        spec: &Spec,
        now_us: u64,
        heard_us: Option<u64>,
        asker: Option<&str>,
        can_mark: bool,
    ) -> Result<(Value, Vec<Out>), String> {
        if !on {
            return Ok(match self.on.take() {
                Some(_) => (
                    json!({ "next": OFF_NOTE }),
                    vec![Out::Unmark, Out::Said("off, as asked".into())],
                ),
                None => (json!({ "next": WAS_OFF }), Vec::new()),
            });
        }
        if self.on.is_some() {
            return Ok((json!({ "next": ALREADY_ON }), Vec::new()));
        }
        let asked = heard_us.is_some_and(|heard_us| {
            heard_us <= now_us && now_us - heard_us <= spec.asked_within_us
        });
        if !asked {
            return Err(NOT_ASKED.into());
        }
        if !can_mark {
            return Err(NO_MARK.into());
        }
        self.on = Some(On {
            asker: asker.map(str::to_owned),
            away: false,
            visit: None,
            texted_us: None,
            blind_told: false,
            closed: false,
        });
        let (note, by) = match asker {
            Some(_) => (ON_NOTE, ", by someone whose face it knows"),
            None => (ON_NOTE_UNKNOWN, ", by someone whose face it does not know"),
        };
        Ok((
            json!({ "next": note }),
            vec![
                Out::Mark(spec.mark.clone()),
                Out::Said(format!("on, asked at the lamp{by}")),
            ],
        ))
    }

    /// `room/presence`: someone at the lamp or not.
    pub fn presence(
        &mut self,
        someone: bool,
        now_us: u64,
        minute: Option<u16>,
        spec: &Spec,
    ) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        if on.closed {
            return Vec::new();
        }
        if !on.away {
            if someone {
                return Vec::new();
            }
            on.away = true;
            return vec![Out::Said("the desk is empty: keeping watch".into())];
        }
        if someone {
            on.visit(now_us, minute);
            return Vec::new();
        }
        match on.visit.take() {
            Some(visit) => on.ended(visit, now_us, spec),
            None => Vec::new(),
        }
    }

    /// A face near the lamp was seen, or not.
    pub fn face(&mut self, near: bool, now_us: u64, minute: Option<u16>) {
        if let Some(on) = &mut self.on
            && on.away
            && !on.closed
            && near
        {
            on.visit(now_us, minute).face = true;
        }
    }

    /// The eyes know the face at the lamp as `name`, or no face there is
    /// known. The person who asked, back after they left, ends the watch.
    pub fn known(&mut self, name: Option<&str>, now_us: u64, minute: Option<u16>) -> Vec<Out> {
        let (Some(on), Some(name)) = (&mut self.on, name) else {
            return Vec::new();
        };
        if !on.away || on.closed {
            return Vec::new();
        }
        if on.asker.as_deref() == Some(name) {
            self.on = None;
            return vec![
                Out::Unmark,
                Out::Said("off: the person who asked is back".into()),
            ];
        }
        let visit = on.visit(now_us, minute);
        visit.face = true;
        visit.known = true;
        Vec::new()
    }

    /// The privacy switch, closed or not: closed, once, its line; and while
    /// it is closed nothing about the desk is taken.
    pub fn privacy(&mut self, closed: bool) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        on.closed = closed;
        if closed && !on.blind_told {
            on.blind_told = true;
            return vec![Out::Text(BLIND.into())];
        }
        Vec::new()
    }
}

/// One loaded watch skill: its watch as loaded, its tool and choice, and
/// whether it is on.
#[derive(Debug)]
struct Entry {
    spec: Spec,
    tool: String,
    choice: String,
    watch: Watch,
}

/// Every watch skill's watch, by skill, and when the lamp last heard
/// someone speak.
#[derive(Debug, Default)]
pub struct Watches {
    each: BTreeMap<String, Entry>,
    heard_us: Option<u64>,
}

/// What a watch skill does, by skill.
pub type Outs = Vec<(String, Out)>;
/// A watch's tool turned: its skill, the model's answer, and what to do.
pub type Turned = (String, Value, Vec<Out>);

impl Watches {
    /// The skills loaded again. A watch kept as it was stays as it was; one
    /// whose skill is gone, or whose `[watch]` changed, is off, its mark
    /// given back. A watch is never turned on here.
    pub fn replace(&mut self, skills: &[Skill]) -> Outs {
        let mut made = Vec::new();
        let mut each = BTreeMap::new();
        for skill in skills {
            let (Some(spec), Some(tool), Some(choice)) = (&skill.watch, &skill.tool, &skill.choice)
            else {
                continue;
            };
            let watch = match self.each.remove(&skill.name) {
                Some(old) if old.spec == *spec => old.watch,
                Some(old) => {
                    if old.watch.is_on() {
                        made.push((skill.name.clone(), Out::Unmark));
                        made.push((
                            skill.name.clone(),
                            Out::Said("its [watch] changed: off until asked again".into()),
                        ));
                    }
                    Watch::default()
                }
                None => Watch::default(),
            };
            each.insert(
                skill.name.clone(),
                Entry {
                    spec: spec.clone(),
                    tool: tool.clone(),
                    choice: choice.name.clone(),
                    watch,
                },
            );
        }
        for (name, old) in std::mem::take(&mut self.each) {
            if old.watch.is_on() {
                made.push((name.clone(), Out::Unmark));
                made.push((name, Out::Said("its skill is gone: off".into())));
            }
        }
        self.each = each;
        made
    }

    /// The skill whose watch `tool` turns, if any.
    pub fn owner(&self, tool: &str) -> Option<&str> {
        self.each
            .iter()
            .find(|(_, entry)| entry.tool == tool)
            .map(|(name, _)| name.as_str())
    }

    /// Whether any watch is on.
    pub fn any_on(&self) -> bool {
        self.each.values().any(|entry| entry.watch.is_on())
    }

    /// Whether the skill's watch is on.
    pub fn is_on(&self, skill: &str) -> bool {
        self.each
            .get(skill)
            .is_some_and(|entry| entry.watch.is_on())
    }

    /// A call of a watch's `tool` with `args`: none if no watch has it;
    /// else the skill, the model's answer and what to do, or why not.
    pub fn tool(
        &mut self,
        tool: &str,
        args: &Value,
        now_us: u64,
        asker: Option<&str>,
        can_mark: bool,
    ) -> Option<Result<Turned, String>> {
        let heard_us = self.heard_us;
        let (name, entry) = self.each.iter_mut().find(|(_, entry)| entry.tool == tool)?;
        let turn = match args {
            Value::Object(given) if given.len() == 1 => {
                given.get(&entry.choice).and_then(Value::as_str)
            }
            _ => None,
        };
        let Some(on) = turn.and_then(|turn| match turn {
            ON => Some(true),
            OFF => Some(false),
            _ => None,
        }) else {
            return Some(Err(format!("{} is \"on\" or \"off\"", entry.choice)));
        };
        Some(
            entry
                .watch
                .turn(on, &entry.spec, now_us, heard_us, asker, can_mark)
                .map(|(answer, outs)| (name.clone(), answer, outs)),
        )
    }

    /// A message on a topic a watch reads, at `now_us`, at local `minute`.
    /// `voice/privacy` is taken too, as a replay has it.
    pub fn observe(
        &mut self,
        topic: &str,
        payload: &[u8],
        now_us: u64,
        minute: Option<u16>,
    ) -> Outs {
        let Ok(value) = serde_json::from_slice::<Value>(payload) else {
            return Vec::new();
        };
        let mut made = Vec::new();
        match topic {
            "voice/heard" => {
                if value["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
                {
                    self.heard_us = Some(now_us);
                }
            }
            "room/presence" => {
                if let Some(someone) = value["someone"].as_bool() {
                    for (name, entry) in &mut self.each {
                        for out in entry.watch.presence(someone, now_us, minute, &entry.spec) {
                            made.push((name.clone(), out));
                        }
                    }
                }
            }
            "humans/faces/tracked" => {
                if let Ok(faces) = serde_json::from_value::<Faces>(value) {
                    let near = faces.faces.iter().any(|face| face.ymax - face.ymin >= NEAR);
                    for entry in self.each.values_mut() {
                        entry.watch.face(near, now_us, minute);
                    }
                }
            }
            PRIVACY => made = self.privacy(value["physical_muted"] == Value::Bool(true)),
            _ => {}
        }
        made
    }

    /// The eyes know the face at the lamp as `name`, or none there is known.
    pub fn known(&mut self, name: Option<&str>, now_us: u64, minute: Option<u16>) -> Outs {
        let mut made = Vec::new();
        for (skill, entry) in &mut self.each {
            for out in entry.watch.known(name, now_us, minute) {
                made.push((skill.clone(), out));
            }
        }
        made
    }

    /// The privacy switch, closed or not.
    pub fn privacy(&mut self, closed: bool) -> Outs {
        let mut made = Vec::new();
        for (skill, entry) in &mut self.each {
            for out in entry.watch.privacy(closed) {
                made.push((skill.clone(), out));
            }
        }
        made
    }
}

/// The local minute of the day at `unix_ms`, `offset_minutes` east of UTC.
pub fn minute_of(unix_ms: u64, offset_minutes: i32) -> u16 {
    let minutes = i128::from(unix_ms).div_euclid(60_000) + i128::from(offset_minutes);
    minutes.rem_euclid(1_440) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000;
    const MINUTE: u64 = 60 * SECOND;

    fn spec() -> Spec {
        Spec {
            every_us: 10 * MINUTE,
            asked_within_us: 30 * SECOND,
            mark: Call::Light {
                colour: "blue".into(),
                level: Some(1),
                lasting: None,
                pattern: None,
                keep: false,
                until_ends: false,
            },
        }
    }

    /// On, asked at 0 by `asker`, heard a second before.
    fn on(asker: Option<&str>) -> Watch {
        let mut watch = Watch::default();
        watch
            .turn(true, &spec(), SECOND, Some(0), asker, true)
            .unwrap();
        watch
    }

    fn texts(outs: &[Out]) -> Vec<&str> {
        outs.iter()
            .filter_map(|out| match out {
                Out::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The person leaves, someone comes at `at` (14:31 local) and stays
    /// `stays`: what the watch does when they go.
    fn visit(watch: &mut Watch, at: u64, stays: u64) -> Vec<Out> {
        let spec = spec();
        let mut outs = watch.presence(false, at - MINUTE, None, &spec);
        outs.extend(watch.presence(true, at, Some(14 * 60 + 31), &spec));
        // `room/presence` once a second while they are there.
        let mut now = at;
        while now < at + stays {
            now += SECOND;
            outs.extend(watch.presence(true, now, Some(14 * 60 + 31), &spec));
        }
        outs.extend(watch.presence(false, now + 20 * SECOND, None, &spec));
        outs
    }

    #[test]
    fn it_is_never_on_by_itself() {
        let spec = spec();
        let mut watch = Watch::default();
        assert!(!watch.is_on());
        // Nothing it sees turns it on.
        assert!(watch.presence(false, MINUTE, None, &spec).is_empty());
        assert!(watch.presence(true, 2 * MINUTE, None, &spec).is_empty());
        watch.face(true, 2 * MINUTE, None);
        assert!(watch.known(Some("known-a"), 2 * MINUTE, None).is_empty());
        assert!(watch.privacy(true).is_empty());
        assert!(!watch.is_on());
        // Asked with nothing heard at the lamp (a text), or heard too long
        // before: refused.
        let not_asked = watch.turn(true, &spec, MINUTE, None, None, true);
        assert_eq!(not_asked.unwrap_err(), NOT_ASKED);
        let long_ago = watch.turn(true, &spec, MINUTE, Some(MINUTE - 31 * SECOND), None, true);
        assert_eq!(long_ago.unwrap_err(), NOT_ASKED);
        assert!(!watch.is_on());
        // No mark to show: refused.
        let unmarked = watch.turn(true, &spec, MINUTE, Some(MINUTE - SECOND), None, false);
        assert_eq!(unmarked.unwrap_err(), NO_MARK);
        assert!(!watch.is_on());
        // Asked out loud, the mark shown.
        let (answer, outs) = watch
            .turn(true, &spec, MINUTE, Some(MINUTE - 30 * SECOND), None, true)
            .unwrap();
        assert!(watch.is_on());
        assert_eq!(outs[0], Out::Mark(spec.mark.clone()));
        assert!(
            answer["next"]
                .as_str()
                .unwrap()
                .contains("tell you to stop")
        );
    }

    /// #1685's test: someone arriving and leaving gives one text with the
    /// right time and length.
    #[test]
    fn a_visit_gives_one_text_with_its_time_and_length() {
        let mut watch = on(None);
        let outs = visit(&mut watch, 10 * MINUTE, 2 * MINUTE);
        assert_eq!(
            texts(&outs),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
        let short = visit(&mut on(None), 10 * MINUTE, 10 * SECOND);
        assert_eq!(
            texts(&short),
            ["Someone was at your desk at 14:31, for under a minute."]
        );
        let minute = visit(&mut on(None), 10 * MINUTE, 80 * SECOND);
        assert_eq!(
            texts(&minute),
            ["Someone was at your desk at 14:31, for about a minute."]
        );
        // No local time: no time said.
        let mut watch = on(None);
        let spec = spec();
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 2 * MINUTE, None, &spec);
        watch.presence(true, 5 * MINUTE, None, &spec);
        assert_eq!(
            texts(&watch.presence(false, 5 * MINUTE + SECOND, None, &spec)),
            ["Someone was at your desk, for about 3 minutes."]
        );
    }

    /// #1685's test: a second arrival within 10 minutes of a text gives
    /// none; one after them, one.
    #[test]
    fn a_second_visit_within_ten_minutes_is_not_texted() {
        let mut watch = on(None);
        assert_eq!(texts(&visit(&mut watch, 10 * MINUTE, MINUTE)).len(), 1);
        let again = visit(&mut watch, 16 * MINUTE, MINUTE);
        assert!(texts(&again).is_empty(), "{again:?}");
        assert!(matches!(&again[..], [Out::Said(why)] if why.contains("one text each 600 s")));
        let later = visit(&mut watch, 23 * MINUTE, MINUTE);
        assert_eq!(texts(&later).len(), 1, "{later:?}");
    }

    /// #1685's test: the face of the person who asked, back after they left,
    /// ends it, with no text; while they are still at the desk it does not.
    #[test]
    fn the_face_of_the_person_who_asked_ends_it() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        // Still at the desk, being recognised as they leave.
        assert!(watch.known(Some("known-a"), 2 * SECOND, None).is_empty());
        assert!(watch.is_on());
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 30 * MINUTE, Some(15 * 60), &spec);
        let back = watch.known(Some("known-a"), 30 * MINUTE + 2 * SECOND, None);
        assert_eq!(back[0], Out::Unmark);
        assert!(!watch.is_on());
        assert!(texts(&back).is_empty());
        // Nothing after: they are back.
        assert!(watch.presence(false, 32 * MINUTE, None, &spec).is_empty());
        // Someone else known does not end it.
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 30 * MINUTE, Some(15 * 60), &spec);
        assert!(watch.known(Some("known-b"), 30 * MINUTE, None).is_empty());
        assert!(watch.is_on());
        // An asker the eyes did not know is never ended by a face.
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec);
        assert!(watch.known(Some("known-a"), 2 * MINUTE, None).is_empty());
        assert!(watch.is_on());
    }

    /// Known or not, never whose: the text says only which.
    #[test]
    fn a_text_says_known_or_not_and_never_a_name() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec);
        watch.known(Some("known-b"), 10 * MINUTE + SECOND, None);
        watch.presence(true, 12 * MINUTE, None, &spec);
        let outs = watch.presence(false, 12 * MINUTE + SECOND, None, &spec);
        assert_eq!(
            texts(&outs),
            ["Someone you've introduced me to was at your desk at 14:31, for about 2 minutes."]
        );
        assert!(!format!("{outs:?}").contains("known-b"));
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 10 * MINUTE, Some(9 * 60 + 5), &spec);
        watch.face(true, 10 * MINUTE, None);
        watch.presence(true, 13 * MINUTE, None, &spec);
        assert_eq!(
            texts(&watch.presence(false, 13 * MINUTE + SECOND, None, &spec)),
            ["Someone I don't know was at your desk at 9:05, for about 3 minutes."]
        );
        // A face far off is not someone at the desk's.
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec);
        watch.presence(true, 10 * MINUTE, Some(9 * 60 + 5), &spec);
        watch.face(false, 10 * MINUTE, None);
        assert!(
            texts(&watch.presence(false, 13 * MINUTE, None, &spec))[0].starts_with("Someone was")
        );
    }

    /// #1685's test: with the switch closed, one line, and quiet after.
    #[test]
    fn the_privacy_switch_closed_gives_one_line_and_then_quiet() {
        let mut watch = on(None);
        assert!(watch.privacy(false).is_empty());
        assert_eq!(watch.privacy(true), [Out::Text(BLIND.into())]);
        assert!(watch.privacy(true).is_empty());
        assert!(watch.privacy(false).is_empty());
        assert!(watch.privacy(true).is_empty());
        // Off, nothing.
        let mut off = Watch::default();
        assert!(off.privacy(true).is_empty());
    }

    /// reviewer-two's P3 on #1807: with the switch closed, whoever says
    /// someone is at the desk, no visit begins, none ends, none is texted,
    /// and the asker's face ends nothing; open again, visits count again.
    #[test]
    fn with_the_switch_closed_nothing_about_the_desk_is_taken() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec);
        assert_eq!(watch.privacy(true), [Out::Text(BLIND.into())]);
        assert!(
            watch
                .presence(true, 2 * MINUTE, Some(600), &spec)
                .is_empty()
        );
        watch.face(true, 2 * MINUTE, None);
        assert!(watch.known(Some("known-b"), 2 * MINUTE, None).is_empty());
        assert!(watch.known(Some("known-a"), 3 * MINUTE, None).is_empty());
        assert!(watch.is_on());
        assert!(watch.presence(false, 4 * MINUTE, None, &spec).is_empty());
        // Open: the next visit is the first.
        assert!(watch.privacy(false).is_empty());
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec);
        watch.presence(true, 12 * MINUTE, None, &spec);
        assert_eq!(
            texts(&watch.presence(false, 12 * MINUTE + SECOND, None, &spec)),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
    }

    /// Off when asked, from anywhere: no voice needed to stop it.
    #[test]
    fn off_when_asked_gives_the_mark_back() {
        let spec = spec();
        let mut watch = on(None);
        let (answer, outs) = watch
            .turn(false, &spec, 5 * MINUTE, None, None, false)
            .unwrap();
        assert_eq!(outs[0], Out::Unmark);
        assert_eq!(answer["next"], OFF_NOTE);
        assert!(!watch.is_on());
        let (answer, outs) = watch
            .turn(false, &spec, 6 * MINUTE, None, None, false)
            .unwrap();
        assert!(outs.is_empty());
        assert_eq!(answer["next"], WAS_OFF);
        // On twice is on once.
        let mut watch = on(None);
        let (answer, outs) = watch
            .turn(true, &spec, 2 * SECOND, Some(SECOND), None, true)
            .unwrap();
        assert!(outs.is_empty());
        assert_eq!(answer["next"], ALREADY_ON);
    }

    /// The repository's guard mode, as the program holds it: a reload
    /// never turns a watch on; the same skill again leaves it as it was; a
    /// changed `[watch]` or a removed skill turns it off and gives its mark
    /// back; and its tool takes only on or off.
    #[test]
    fn a_reload_never_turns_a_watch_on_and_a_changed_or_removed_one_is_off() {
        let skill = crate::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/guard-mode"),
        )
        .unwrap();
        let heard = br#"{"text":"keep an eye on things while I'm out","finished":true}"#;
        let on = json!({"turn": "on"});
        let mut watches = Watches::default();
        assert!(watches.replace(std::slice::from_ref(&skill)).is_empty());
        assert!(!watches.any_on());
        assert_eq!(watches.owner("keep_watch"), Some("guard-mode"));
        assert!(watches.tool("dance", &on, SECOND, None, true).is_none());
        for wrong in [
            json!({"turn": "maybe"}),
            json!({}),
            Value::Null,
            json!({"on": true}),
        ] {
            let refused = watches.tool("keep_watch", &wrong, SECOND, None, true);
            assert!(matches!(refused, Some(Err(_))), "{wrong}");
        }
        watches.observe("voice/heard", heard, SECOND, None);
        let (name, _, outs) = watches
            .tool("keep_watch", &on, 2 * SECOND, None, true)
            .unwrap()
            .unwrap();
        assert_eq!(name, "guard-mode");
        assert!(matches!(outs[0], Out::Mark(_)));
        assert!(watches.is_on("guard-mode"));
        // Loaded again as it was: still on, nothing done.
        assert!(watches.replace(std::slice::from_ref(&skill)).is_empty());
        assert!(watches.is_on("guard-mode"));
        // Its [watch] changed: off, the mark given back.
        let mut changed = skill.clone();
        changed.watch.as_mut().unwrap().every_us = 20 * MINUTE;
        let outs = watches.replace(std::slice::from_ref(&changed));
        assert_eq!(outs[0], ("guard-mode".to_owned(), Out::Unmark));
        assert!(!watches.is_on("guard-mode"));
        // On again, then the skill removed: off, and its tool no one's.
        watches.observe("voice/heard", heard, 3 * SECOND, None);
        watches
            .tool("keep_watch", &on, 4 * SECOND, None, true)
            .unwrap()
            .unwrap();
        let outs = watches.replace(&[]);
        assert_eq!(outs[0], ("guard-mode".to_owned(), Out::Unmark));
        assert!(!watches.any_on());
        assert!(watches.owner("keep_watch").is_none());
    }

    #[test]
    fn the_local_minute_is_the_wall_clock_east_of_utc() {
        // 2026-10-19 07:31 UTC is 14:31 in Ho Chi Minh City (+07:00).
        let unix_ms = 1_792_395_060_000;
        assert_eq!(minute_of(unix_ms, 420), 14 * 60 + 31);
        assert_eq!(minute_of(unix_ms, 0), 7 * 60 + 31);
        assert_eq!(minute_of(unix_ms, -480), 23 * 60 + 31);
    }

    #[test]
    fn the_topics_are_the_messages_own() {
        assert_eq!(LISTENS[0], lamp_msgs::RoomPresence::TOPIC);
        assert_eq!(LISTENS[1], Faces::TOPIC);
        assert_eq!(LISTENS[2], lamp_msgs::Speech::TOPICS[0]);
        assert_eq!(PRIVACY, lamp_msgs::MicrophonePrivacy::TOPIC);
    }
}
