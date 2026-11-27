//! `lamp-body exit-report`: the last exit report the body printed
//! (`{"port":…,"body":{…}}`, as in `journalctl -u lamp-body`), as a few
//! plain lines for the morning report. Off the motion path: it only reads.

use crate::Joint;
use serde::Deserialize;
use std::fmt::Write;

/// The fields of [`crate::body::BodyReport`] the summary says, by the same
/// names. Every one but `stop` may be missing: a body built before it was
/// added (`bad_temperatures` 2026-10-17 08:25, `temperature_settles` 10:34,
/// `temperature_flaps` with this reader) prints "not in this report". A
/// field renamed in `BodyReport` fails
/// `a_joint_flapping_between_67_and_71_stops_on_heat`, which reads a real
/// report back through [`summary`] and checks each line by name.
#[derive(Deserialize)]
struct Report {
    stop: serde_json::Value,
    #[serde(default)]
    stood: Option<bool>,
    #[serde(default)]
    steps: Option<u64>,
    #[serde(default)]
    bad_temperatures: Option<[u16; 5]>,
    #[serde(default)]
    temperature_flaps: Option<[u16; 5]>,
    #[serde(default)]
    temperature_settles: Option<Vec<(u8, u8, u8)>>,
    #[serde(default)]
    peak_temperature: Option<[u8; 3]>,
    #[serde(default)]
    arm_peak_temperature: Option<[u8; 2]>,
    #[serde(default)]
    bad_reads: Option<[u16; 5]>,
    #[serde(default)]
    goal_writes: Option<u64>,
}

#[derive(Deserialize)]
struct Exit {
    body: Option<Report>,
}

/// What [`summary`] found in the text.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// The newest exit report, as plain lines.
    Read(String),
    /// The newest exit report could not be read (cut short, or not one the
    /// reader knows): why, and the previous readable one if any, labelled.
    Unreadable(String),
}

const NOT_IN: &str = "not in this report";

/// The newest exit report in `text` (one line, or a whole journal; any
/// journal prefix before `{"port":` is said as `at:`), or `None` if there is
/// none. Only the newest counts: if it cannot be read, that is what is said,
/// never an older one in its place.
pub fn summary(text: &str) -> Option<Found> {
    let mut reports = text.lines().rev().filter_map(|line| {
        let start = line.find("{\"port\":")?;
        Some((line[..start].trim(), &line[start..]))
    });
    let (at, json) = reports.next()?;
    match serde_json::from_str::<Exit>(json) {
        Ok(exit) => Some(Found::Read(lines(at, exit))),
        Err(error) => {
            let mut out = format!(
                "newest report unreadable (truncated?): {error}\nat: {}\n",
                at_or_unknown(at)
            );
            let previous =
                reports.find_map(|(at, json)| Some((at, serde_json::from_str::<Exit>(json).ok()?)));
            match previous {
                Some((at, exit)) => {
                    out.push_str("previous readable report, not the newest:\n");
                    out.push_str(&lines(at, exit));
                }
                None => out.push_str("no earlier readable report\n"),
            }
            Some(Found::Unreadable(out))
        }
    }
}

fn at_or_unknown(at: &str) -> &str {
    if at.is_empty() {
        "unknown (no journal prefix: read it with -o short-iso)"
    } else {
        at
    }
}

fn lines(at: &str, exit: Exit) -> String {
    let mut out = format!("at: {}\n", at_or_unknown(at));
    let Some(body) = exit.body else {
        out.push_str("no body report: the port was not run\n");
        return out;
    };
    let _ = writeln!(out, "stop: {}", stop_line(&body.stop));
    let stood = match body.stood {
        Some(true) => "yes",
        Some(false) => "no",
        None => NOT_IN,
    };
    let steps = body
        .steps
        .map_or(NOT_IN.to_owned(), |steps| steps.to_string());
    let _ = writeln!(out, "stood: {stood}, steps {steps}");
    let all = Joint::ALL.map(Joint::name);
    let _ = writeln!(
        out,
        "bad_temperatures: {}",
        per_joint(&all, body.bad_temperatures)
    );
    let _ = writeln!(
        out,
        "temperature_flaps: {}",
        per_joint(&all, body.temperature_flaps)
    );
    let settles = match body.temperature_settles {
        Some(settles) => {
            let each: Vec<String> = settles
                .iter()
                .map(|(joint, from, to)| format!("{} {from}->{to}", joint_name(*joint)))
                .collect();
            format!(
                "{}{}{}",
                each.len(),
                if each.is_empty() { "" } else { ": " },
                each.join(", ")
            )
        }
        None => NOT_IN.to_owned(),
    };
    let _ = writeln!(out, "temperature_settles: {settles}");
    let commanded = crate::body::COMMANDED.map(Joint::name);
    let _ = writeln!(
        out,
        "peak_temperature: {}",
        per_joint(&commanded, body.peak_temperature)
    );
    let arm = crate::body::ARM.map(Joint::name);
    let _ = writeln!(
        out,
        "arm_peak_temperature: {}",
        per_joint(&arm, body.arm_peak_temperature)
    );
    let _ = writeln!(out, "bad_reads: {}", per_joint(&all, body.bad_reads));
    let writes = body
        .goal_writes
        .map_or(NOT_IN.to_owned(), |writes| writes.to_string());
    let _ = writeln!(out, "goal_writes: {writes}");
    out
}

fn per_joint<T: std::fmt::Display, const N: usize>(
    names: &[&str],
    values: Option<[T; N]>,
) -> String {
    let Some(values) = values else {
        return NOT_IN.to_owned();
    };
    names
        .iter()
        .zip(values)
        .map(|(name, value)| format!("{name} {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn joint_name(id: u8) -> &'static str {
    Joint::try_from(id).map_or("unknown joint", Joint::name)
}

/// One plain phrase for the stop's kind, then its values by name.
fn stop_line(stop: &serde_json::Value) -> String {
    let Some(fields) = stop.as_object() else {
        return stop.to_string();
    };
    let field = |key: &str| fields.get(key).map(ToString::to_string).unwrap_or_default();
    let joint = fields
        .get("joint")
        .and_then(serde_json::Value::as_u64)
        .and_then(|id| u8::try_from(id).ok())
        .map(|id| format!("{} (joint {id}): ", joint_name(id)))
        .unwrap_or_default();
    match fields.get("kind").and_then(serde_json::Value::as_str) {
        Some("cancelled") => "cancelled, an ordinary stop".to_owned(),
        Some("bad_temperature") => format!(
            "{joint}temperature unreadable; last read {}, last good {}",
            field("temperature"),
            field("from")
        ),
        Some("temperature") => format!("{joint}too hot at {}", field("temperature")),
        Some("bad_read") => format!(
            "{joint}position unreadable; last read {}, last good {}",
            field("position"),
            field("from")
        ),
        kind => {
            let mut line = format!("{joint}{}", kind.unwrap_or("?"));
            for (key, value) in fields {
                if key != "kind" && key != "joint" {
                    let _ = write!(line, ", {key} {value}");
                }
            }
            line
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Found, summary};

    /// lamp-4ace's 15:25:38 exit on 2026-10-17, as quoted on #705 at 16:23:
    /// the stop, steps, stood, `bad_temperatures`, the shoulder's ten settles
    /// and `arm_peak_temperature` are the lamp's; `peak_temperature` and
    /// `goal_writes` were not quoted and are made up. It predates
    /// `temperature_flaps`. A journal line before it is skipped.
    const LAMP_4ACE_1525: &str = concat!(
        "2026-10-17T15:25:30+0700 lamp-4ace lamp-body[811]: lamp-body: step 618580 a face is looked at\n",
        "2026-10-17T15:25:38+0700 lamp-4ace lamp-body[811]: {\"port\":{\"inspection\":null},\"body\":{",
        "\"steps\":618587,\"goal_writes\":618001,\"stood\":true,",
        "\"bad_reads\":[0,0,0,0,0],\"bad_temperatures\":[817,9354,2356,448,323],",
        "\"temperature_settles\":[[2,59,55],[2,61,55],[2,57,53],[2,56,51],[2,56,52],",
        "[2,57,52],[2,58,54],[2,54,50],[2,58,54],[2,59,55]],",
        "\"peak_temperature\":[50,48,47],\"arm_peak_temperature\":[62,58],",
        "\"peak_load\":[120,90,80],",
        "\"stop\":{\"kind\":\"bad_temperature\",\"joint\":2,\"temperature\":55,\"from\":60}}}\n",
        "2026-10-17T15:25:53+0700 lamp-4ace systemd[1]: lamp-body.service: Scheduled restart job.\n",
    );

    const LAMP_4ACE_1525_LINES: &str = "at: 2026-10-17T15:25:38+0700 lamp-4ace lamp-body[811]:\n\
         stop: base_pitch (joint 2): temperature unreadable; last read 55, last good 60\n\
         stood: yes, steps 618587\n\
         bad_temperatures: base_yaw 817, base_pitch 9354, elbow_pitch 2356, wrist_roll 448, wrist_pitch 323\n\
         temperature_flaps: not in this report\n\
         temperature_settles: 10: base_pitch 59->55, base_pitch 61->55, base_pitch 57->53, base_pitch 56->51, base_pitch 56->52, base_pitch 57->52, base_pitch 58->54, base_pitch 54->50, base_pitch 58->54, base_pitch 59->55\n\
         peak_temperature: wrist_pitch 50, wrist_roll 48, base_yaw 47\n\
         arm_peak_temperature: base_pitch 62, elbow_pitch 58\n\
         bad_reads: base_yaw 0, base_pitch 0, elbow_pitch 0, wrist_roll 0, wrist_pitch 0\n\
         goal_writes: 618001\n";

    #[test]
    fn lamp_4ace_1525_reads_as_plain_lines() {
        assert_eq!(
            summary(LAMP_4ACE_1525),
            Some(Found::Read(LAMP_4ACE_1525_LINES.to_owned()))
        );
    }

    /// A real exit of lamp-52e6 on 2026-10-14, a body built before
    /// `bad_reads`, `bad_temperatures`, `temperature_settles` and
    /// `temperature_flaps`: it still reads, those said as missing.
    #[test]
    fn a_body_older_than_the_temperature_fields_still_reads() {
        let text = include_str!(
            "../../../docs/verification/wake-lamp-52e6-20261014/wake-yaw-home-back.json"
        );
        assert_eq!(
            summary(text),
            Some(Found::Read(
                "at: unknown (no journal prefix: read it with -o short-iso)\n\
                 stop: cancelled, an ordinary stop\n\
                 stood: no, steps 1170\n\
                 bad_temperatures: not in this report\n\
                 temperature_flaps: not in this report\n\
                 temperature_settles: not in this report\n\
                 peak_temperature: wrist_pitch 44, wrist_roll 43, base_yaw 46\n\
                 arm_peak_temperature: base_pitch 0, elbow_pitch 0\n\
                 bad_reads: not in this report\n\
                 goal_writes: 300\n"
                    .to_owned()
            ))
        );
    }

    /// The newest report cut short is said as unreadable, never replaced by
    /// the older one, which follows only labelled as not the newest.
    #[test]
    fn a_truncated_newest_report_is_said_not_replaced_by_an_older_one() {
        let report = LAMP_4ACE_1525.lines().nth(1).unwrap();
        let cut = report.replace("15:25:38", "16:00:00");
        let text = format!("{report}\n{}\n", &cut[..cut.len() / 2]);
        let Some(Found::Unreadable(out)) = summary(&text) else {
            panic!("{:?}", summary(&text));
        };
        assert!(
            out.starts_with("newest report unreadable (truncated?): "),
            "{out}"
        );
        assert!(
            out.contains("\nat: 2026-10-17T16:00:00+0700 lamp-4ace lamp-body[811]:\n"),
            "{out}"
        );
        assert!(
            out.contains("previous readable report, not the newest:\nat: 2026-10-17T15:25:38"),
            "{out}"
        );
        let alone = summary(&cut[..cut.len() / 2]);
        assert!(
            matches!(&alone, Some(Found::Unreadable(out)) if out.ends_with("no earlier readable report\n")),
            "{alone:?}"
        );
    }

    #[test]
    fn the_last_report_wins_and_none_is_none() {
        let two = LAMP_4ACE_1525.replace("618587", "1") + LAMP_4ACE_1525;
        assert_eq!(
            summary(&two),
            Some(Found::Read(LAMP_4ACE_1525_LINES.to_owned()))
        );
        assert_eq!(summary("lamp-body: step 1\n"), None);
        assert_eq!(
            summary("{\"port\":{},\"body\":null}"),
            Some(Found::Read(
                "at: unknown (no journal prefix: read it with -o short-iso)\n\
                 no body report: the port was not run\n"
                    .to_owned()
            ))
        );
    }
}
