//! How long the person at the desk has been sitting there, read by voice
//! (#7694): "how long have I been at my desk?", "when did I last take a
//! break?". One function the model calls with no arguments, declared only
//! with `LAMP_DESK_TIME=on` in the voice's unit; never a skill. lamp-skills
//! reads the same switch to put the sitting in the wellbeing nudges' notes
//! (`lamp_skills::SITTING_SLOT`), so the follow-up to a nudge agrees with it.
//!
//! It reads `room/activity`'s `sitting_s`: the current sitting as the eyes
//! measure it for the nudges (lamp-eyes' `LAMP_ACTIVITY`), the same message
//! a made skill's trigger reads (`crate::made_skill`'s `ACTIVITY_TOPIC`).
//! The bus's latched value is read at the call ([`told_now`]): a read of
//! what the bus keeps, as `faces_in_view` reads the faces, with no wait and
//! no ask. Without the eyes' activity the topic says nothing, and the answer
//! is the warm [`NOT_CAUGHT`].
//!
//! What it tells, and to whom (the owner, 18 Nov; the first builder on
//! #7694, squared with `crate::doing_told`'s rule that the lamp never
//! volunteers what it saw):
//! - only the current sitting, which the eyes begin again when someone else
//!   sits down and end when the chair empties; nothing is kept here, no
//!   history and no day's total;
//! - only when the person asks, in words, at the lamp: a written turn (a
//!   skill's say) or a typed one is refused ([`ASKED_ONLY`]), so a nudge
//!   never reads it to volunteer it and nothing of it reaches a text;
//! - only with exactly one face in view: with two, the sitting may be the
//!   other's, and it is never about another person;
//! - the minutes rounded and the clock time it began, never the eyes' state
//!   label.
use lamp_gemini::Tool;
use lamp_msgs::{Activity, RoomActivity};
use serde_json::{Value, json};

/// `on` for the function to be declared. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_DESK_TIME";
/// The provider worker's flag for it.
pub const FLAG: &str = "--desk-time";
/// The function's name.
pub const NAME: &str = "desk_time";

/// What the lamp says when it does not know: warm, never "I don't track
/// you" (#7694).
pub const NOT_CAUGHT: &str = "I didn't catch when you sat down.";
/// Told for a turn nobody asked in words at the lamp.
pub const ASKED_ONLY: &str = "Not told: only when the person at the desk asks you in words how long they have been there. Say nothing about it.";

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

pub fn is_tool(name: &str) -> bool {
    name == NAME
}

/// The function, asked about only when the person asks.
pub fn tool() -> Tool {
    Tool {
        name: NAME.into(),
        description: "Call this when the person at the desk asks how long they have been sitting \
                      there, or when they last took a break (\"how long have I been at my desk?\", \
                      \"when did I last get up?\"). It reads the current sitting as your eyes \
                      measure it. Then say what it gives you as \"say\", in a few words, in their \
                      language, rounded the way a person would say it. Never say you don't keep \
                      track of them: when it has no time, say warmly that you didn't catch when \
                      they sat down. Never bring it up unless you were asked, and never about \
                      anyone else."
            .into(),
        parameters: None,
        silent: false,
    }
}

/// What the model is told now: [`told`] from the bus's newest
/// `room/activity` and faces, the wall clock and the local offset.
pub fn told_now(heard: bool) -> Value {
    if !heard {
        return told(false, None, None, 0, None);
    }
    let bus = lamp_bus::Bus::from_environment().ok();
    let read = bus.as_ref().and_then(|bus| {
        bus.latest(RoomActivity::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<RoomActivity>().ok())
    });
    let faces = bus.as_ref().and_then(crate::known_faces::faces_in_view);
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    told(true, read, faces, now_s, crate::local_time::offset_here())
}

/// What the model is told: for a line `heard` at the lamp, with exactly one
/// face in view and a sitting under way in `read`, its minutes rounded
/// (`rounded`) and, with the local offset (seconds east of UTC) known,
/// the clock time it began, at `now_s` (seconds since the epoch);
/// otherwise [`NOT_CAUGHT`]. A line not heard is told [`ASKED_ONLY`].
pub fn told(
    heard: bool,
    read: Option<RoomActivity>,
    faces_in_view: Option<usize>,
    now_s: i64,
    offset_s: Option<i32>,
) -> Value {
    if !heard {
        return json!({ "error": ASKED_ONLY });
    }
    let Some(sitting_s) = read
        .filter(|_| faces_in_view == Some(1))
        .filter(|read| read.state != Activity::Away)
        .map(|read| read.sitting_s)
        .filter(|sitting_s| *sitting_s > 0)
    else {
        return json!({
            "known": false,
            "say": format!("Say warmly, in a few words in their language: {NOT_CAUGHT} Never say you don't keep track of them."),
        });
    };
    let mut told = json!({
        "at_the_desk": rounded(sitting_s),
        "say": "Tell them in a few words, in their language, rounded the way a person would say it, such as \"about 45 minutes, since a bit after two\". Their last break was just before they sat down; nothing from before this sitting is kept.",
    });
    if let Some(offset_s) = offset_s {
        let began = (now_s - i64::from(sitting_s) + i64::from(offset_s)).rem_euclid(86_400) / 60;
        told["sat_down_at"] = json!(format!("{}:{:02}", began / 60, began % 60));
    }
    told
}

/// A sitting's length in rounded words, as a person says it: "a minute or
/// two", "about 7 minutes", "about 45 minutes", "about an hour", "about an
/// hour and a half", "about 2 hours".
fn rounded(sitting_s: u32) -> String {
    let minutes = (sitting_s + 30) / 60;
    match minutes {
        0..=2 => "a minute or two".to_owned(),
        3..=10 => format!("about {minutes} minutes"),
        11..=52 => format!("about {} minutes", (minutes + 2) / 5 * 5),
        53..=74 => "about an hour".to_owned(),
        75..=104 => "about an hour and a half".to_owned(),
        _ => format!("about {} hours", (minutes + 30) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 14:59:00 UTC on a day well past the epoch; the lamp at UTC+7.
    const NOW_S: i64 = 1_800_000_000 - 1_800_000_000 % 86_400 + 14 * 3_600 + 59 * 60;
    const OFFSET_S: i32 = 7 * 3_600;

    fn sitting(sitting_s: u32) -> Option<RoomActivity> {
        Some(RoomActivity {
            state: Activity::AtDesk,
            for_s: 30,
            sitting_s,
            baseline: true,
            yawns: 0,
        })
    }

    /// #7694: Jay asks with no nudge first: the eyes' `sitting_s` is read,
    /// rounded, with the clock time the sitting began.
    #[test]
    fn a_cold_question_reads_the_sitting_rounded_with_when_it_began() {
        let told = told(true, sitting(47 * 60), Some(1), NOW_S, Some(OFFSET_S));
        assert_eq!(told["at_the_desk"], "about 45 minutes");
        // 14:59 UTC is 21:59 at UTC+7; 47 minutes before is 21:12.
        assert_eq!(told["sat_down_at"], "21:12");
        assert!(told["say"].as_str().unwrap().contains("rounded"));
        // With no local offset known, the length alone.
        let told = super::told(true, sitting(47 * 60), Some(1), NOW_S, None);
        assert_eq!(told["at_the_desk"], "about 45 minutes");
        assert!(told.get("sat_down_at").is_none(), "{told}");
    }

    /// #7694: with the eyes' activity off, nobody sitting, or away, the
    /// warm "didn't catch", and never "I don't track you".
    #[test]
    fn with_no_sitting_known_it_says_warmly_it_did_not_catch_it() {
        let away = Some(RoomActivity {
            state: Activity::Away,
            sitting_s: 0,
            ..sitting(0).unwrap()
        });
        for read in [None, sitting(0), away] {
            let told = told(true, read, Some(1), NOW_S, Some(OFFSET_S));
            assert_eq!(told["known"], false, "{told}");
            let say = told["say"].as_str().unwrap();
            assert!(say.contains(NOT_CAUGHT), "{say}");
            assert!(say.contains("Never say you don't keep track"), "{say}");
            assert!(told.get("at_the_desk").is_none(), "{told}");
        }
    }

    /// Never about another person: with two faces in view, or none known,
    /// the sitting may be someone else's, so nothing of it is told.
    #[test]
    fn two_faces_or_none_known_tell_nothing_of_the_sitting() {
        for faces in [Some(0), Some(2), None] {
            let told = told(true, sitting(47 * 60), faces, NOW_S, Some(OFFSET_S));
            assert_eq!(told["known"], false, "{faces:?}");
        }
    }

    /// Never volunteered (`crate::doing_told`'s rule): a written turn, such
    /// as the nudge's own say, or a typed one, is refused, and reads nothing.
    #[test]
    fn a_turn_nobody_asked_in_words_is_told_nothing() {
        let told = told(false, sitting(47 * 60), Some(1), NOW_S, Some(OFFSET_S));
        assert_eq!(told, json!({ "error": ASKED_ONLY }));
        assert_eq!(told_now(false), json!({ "error": ASKED_ONLY }));
    }

    #[test]
    fn the_length_is_said_the_way_a_person_says_it() {
        assert_eq!(rounded(50), "a minute or two");
        assert_eq!(rounded(7 * 60), "about 7 minutes");
        assert_eq!(rounded(47 * 60), "about 45 minutes");
        assert_eq!(rounded(31 * 60 + 40), "about 30 minutes");
        assert_eq!(rounded(65 * 60), "about an hour");
        assert_eq!(rounded(95 * 60), "about an hour and a half");
        assert_eq!(rounded(130 * 60), "about 2 hours");
    }

    #[test]
    fn the_switch_is_on_only_when_set() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(None).is_empty() && arguments(Some("yes")).is_empty());
        assert!(is_tool(NAME) && !is_tool("the_time"));
    }
}
