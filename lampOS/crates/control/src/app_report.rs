//! `log.send`: the one report an app holding `report` may send out (#8726,
//! `docs/apps.md`, "Lamp Doctor's grants"). The frame carries a
//! `lamp_msgs::doctor::Report`, a fixed schema of numbers, the tier, a score,
//! failure categories from a fixed list, the route and the build: no field
//! takes free text, so a sentence or a name has nowhere to go, and a report
//! that holds anything else is refused whole, with nothing kept or sent. The
//! refusal never repeats what was in it.
//!
//! Where it goes is #7452's report endpoint, which is not answered yet. Until
//! it is, `LAMP_DOCTOR_REPORT=on` keeps each accepted report as one line in
//! `doctor-reports/<app>.jsonl` in lamp-control's state folder (the last
//! [`KEEP`] of each app, the oldest dropped as a new one is kept) and sends
//! nothing; off, an app is told so. At most one
//! report an hour an app. The journal says the app and the size, never the
//! report.
use crate::app_yes::{self, Live, Place};
use crate::protocol::{Error, ErrorCode};
use lamp_msgs::doctor::Report;
use serde_json::{Map, Value, json};
use std::io::Write as _;
use std::time::{Duration, Instant};

pub const ENVIRONMENT: &str = lamp_skills::app_package::REPORT_ENVIRONMENT;
/// The folder the accepted reports are kept in, in the state folder: one file
/// an app, so one app's reports never fill another's.
pub const FOLDER: &str = "doctor-reports";
/// The file stops taking reports at this size.
pub const KEEP: usize = 30;
/// One report an hour an app.
pub const EVERY: Duration = Duration::from_secs(3600);
/// What an app is told with the switch off.
pub const OFF: &str = "reports are not sent from this lamp yet (LAMP_DOCTOR_REPORT)";

/// Whether [`ENVIRONMENT`] is `on` in this process.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

fn refuse(re: u64, code: ErrorCode, why: &str) -> Value {
    crate::protocol::refused(Some(re), &Error::new(code, why))
}

/// This lamp's build number from `lamp-update`'s `state.json`
/// (`{"installed":{"sequence":N}}`), 0 when it can't be read.
pub fn build_number() -> u32 {
    std::fs::read_to_string("/var/lib/lampos-update/state.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim()).ok())
        .and_then(|state| state["installed"]["sequence"].as_u64())
        .and_then(|sequence| u32::try_from(sequence).ok())
        .unwrap_or(0)
}

/// `line` appended to the app's file in `folder`, which keeps the last
/// [`KEEP`]: the file is written whole beside itself and renamed over, so a
/// stop in the middle leaves the old one.
fn keep_last(folder: &std::path::Path, name: &str, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(format!("{name}.jsonl"));
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = before.lines().filter(|kept| !kept.is_empty()).collect();
    if lines.len() >= KEEP {
        lines.drain(..lines.len() + 1 - KEEP);
    }
    lines.push(line);
    let beside = folder.join(format!("{name}.jsonl.new"));
    let mut file = std::fs::File::create(&beside)?;
    for kept in lines {
        writeln!(file, "{kept}")?;
    }
    file.sync_all()?;
    std::fs::rename(&beside, &path)
}

/// The answer to `log.send` from app `name`, `switch` being [`on`].
pub fn answer(
    live: &mut Live,
    place: &Place<'_>,
    name: &str,
    frame: &Map<String, Value>,
    re: u64,
    switch: bool,
    now: Instant,
) -> Value {
    let holds = app_yes::installed(place.apps, name).is_some_and(|manifest| {
        manifest
            .grants
            .iter()
            .any(|grant| grant == lamp_skills::app_package::REPORT_GRANT)
    });
    if !holds {
        return refuse(re, ErrorCode::NoGrant, "not granted: report");
    }
    // At each call, whatever the app was installed with: `report` beside a
    // grant that reads what a person says or does sends nothing.
    let held = app_yes::installed(place.apps, name)
        .map(|manifest| manifest.grants)
        .unwrap_or_default();
    if let Some(why) = lamp_skills::app_package::report_refused(&held) {
        return refuse(re, ErrorCode::NoGrant, why);
    }
    if !switch {
        return refuse(re, ErrorCode::Off, OFF);
    }
    // Only `report`, whole: a field beside it is refused as the rest is.
    if frame
        .keys()
        .any(|key| key != "id" && key != "op" && key != "report")
    {
        return refuse(re, ErrorCode::BadRequest, "log.send takes only report");
    }
    // The lamp's own date and build go in first, over anything the app sent
    // for them: an app does not say either.
    let Some((day, build)) = live.report_stamp().cloned() else {
        return refuse(
            re,
            ErrorCode::Refused,
            "the lamp's date is not known yet, so no report is taken",
        );
    };
    let stamped = match frame.get("report") {
        Some(Value::Object(fields)) => {
            let mut fields = fields.clone();
            fields.insert("day".to_owned(), json!(day));
            fields.insert("build".to_owned(), json!(build));
            Value::Object(fields)
        }
        _ => Value::Null,
    };
    let report = match Report::from_value(&stamped) {
        Ok(report) => report,
        Err(_) => {
            return refuse(
                re,
                ErrorCode::BadRequest,
                "report: not the fixed schema, refused whole",
            );
        }
    };
    let Some(state) = place.state else {
        return refuse(re, ErrorCode::Refused, "this lamp keeps no state folder");
    };
    if let Err(wait) = live.may_report(name, now) {
        let mut error = Error::new(ErrorCode::Rate, "one report an hour");
        error.retry_ms = Some(wait.as_millis().max(1) as u64);
        return crate::protocol::refused(Some(re), &error);
    }
    let line = match serde_json::to_string(&json!({"app": name, "report": report})) {
        Ok(line) => line,
        Err(_) => return refuse(re, ErrorCode::Refused, "the report could not be kept"),
    };
    let wrote = keep_last(&state.join(FOLDER), name, &line);
    match wrote {
        Ok(()) => {
            eprintln!(
                "lamp-control: app {name}: a report of {} bytes kept",
                line.len()
            );
            crate::protocol::ok(re, json!({"kept": true, "sent": false}))
        }
        Err(error) => {
            eprintln!("lamp-control: app {name}: a report was not kept: {error}");
            refuse(re, ErrorCode::Refused, "the report could not be kept")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lampos-report-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn install(apps: &std::path::Path, name: &str, grants: Value) {
        let dir = apps.join(name).join("current");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = json!({
            "format": 2, "kind": "app", "name": name, "title": "T", "version": "1.0.0",
            "min_build": 1, "runtime": {"name": "python", "entry": "src/main.py"},
            "grants": grants, "triggers": [], "settings": [], "files": []
        });
        std::fs::write(dir.join("lamp-skill.json"), manifest.to_string()).unwrap();
    }

    fn report() -> Value {
        json!({
            "turns": 120, "answered_within_5s_pct": 83,
            "heard_to_first_sound_ms_median": 1900, "heard_to_first_sound_ms_p90": 4100,
            "asked_again": 6, "held": 3, "cut_ins": 4, "tool_timeouts": 2, "stuck": 1,
            "tier": 3, "score": 7.8, "band": "amber",
            "failures": [{"category": "misheard", "count": 6}],
            "cases": [{"at": "14:05", "kind": "tool", "outcome": "timeout", "fault": "response_timeout", "waited_ms": 4500, "request": 41}],
            "cases_dropped": 0, "route": "second", "diagnosis_version": 1
        })
    }

    fn send(
        live: &mut Live,
        place: &Place<'_>,
        name: &str,
        report: Value,
        switch: bool,
        now: Instant,
    ) -> Value {
        let frame = json!({"id": 1, "op": "log.send", "report": report});
        answer(
            live,
            place,
            name,
            frame.as_object().unwrap(),
            1,
            switch,
            now,
        )
    }

    #[test]
    fn a_report_in_the_fixed_schema_is_kept_and_anything_else_is_refused_whole() {
        let apps = folder("apps");
        let state = folder("state");
        install(&apps, "doctor", json!(["turns", "report"]));
        install(&apps, "plain", json!(["turns"]));
        let place = Place {
            apps: &apps,
            state: Some(&state),
        };
        let mut live = Live::default();
        let now = Instant::now();
        // No date known: no report is taken, whatever it holds.
        let undated = send(&mut live, &place, "doctor", report(), true, now);
        assert!(
            undated.to_string().contains("date is not known"),
            "{undated}"
        );
        live.set_report_stamp(Some(("2026-11-26".to_owned(), 15_800)));
        // No grant, and the switch off, are told apart and keep nothing.
        let denied = send(&mut live, &place, "plain", report(), true, now);
        assert!(
            denied.to_string().contains("not granted: report"),
            "{denied}"
        );
        let off = send(&mut live, &place, "doctor", report(), false, now);
        assert!(off.to_string().contains("LAMP_DOCTOR_REPORT"), "{off}");
        assert!(!state.join(FOLDER).join("doctor.jsonl").exists());
        // At each call: an app whose manifest holds `report` beside a reader
        // (installed by an older lamp, say) is refused before anything is read.
        install(&apps, "reader", json!(["turns", "report", "hear"]));
        let beside = send(&mut live, &place, "reader", report(), true, now);
        assert!(
            beside.to_string().contains("report can't be held beside"),
            "{beside}"
        );
        assert!(!state.join(FOLDER).join("doctor.jsonl").exists());
        // A sentence, a name, a field beside the schema: refused whole, kept
        // nowhere, and the refusal does not say what it was.
        let sentence = "turn off the light in Mai's room";
        let mut with_sentence = report();
        with_sentence["failures"][0]["category"] = json!(sentence);
        let mut with_name = report();
        with_name["route"] = json!("Mai");
        let mut with_field = report();
        with_field["note"] = json!(sentence);
        for bad in [with_sentence, with_name, with_field] {
            let refused = send(&mut live, &place, "doctor", bad, true, now);
            let said = refused.to_string();
            assert!(said.contains("refused whole"), "{said}");
            assert!(!said.contains("Mai") && !said.contains("light"), "{said}");
        }
        assert!(
            !state.join(FOLDER).join("doctor.jsonl").exists(),
            "nothing of a refused report is kept"
        );
        // A fixed one is kept, once an hour.
        let kept = send(&mut live, &place, "doctor", report(), true, now);
        assert_eq!(kept["kept"], true, "{kept}");
        let lines = std::fs::read_to_string(state.join(FOLDER).join("doctor.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 1);
        assert!(lines.contains("\"app\":\"doctor\""));
        // The day and build are the lamp's, over whatever the app sent.
        assert!(lines.contains("\"day\":\"2026-11-26\"") && lines.contains("\"build\":15800"));
        let mut own = report();
        own["day"] = json!("2030-01-01");
        own["build"] = json!(1);
        let overwritten = send(&mut live, &place, "doctor", own, true, now + EVERY);
        assert_eq!(overwritten["kept"], true, "{overwritten}");
        let kept = std::fs::read_to_string(state.join(FOLDER).join("doctor.jsonl")).unwrap();
        assert!(!kept.contains("2030-01-01"), "{kept}");
        let again = send(
            &mut live,
            &place,
            "doctor",
            report(),
            true,
            now + Duration::from_secs(60),
        );
        assert!(again.to_string().contains("one report an hour"), "{again}");
        let later = send(&mut live, &place, "doctor", report(), true, now + EVERY * 2);
        assert_eq!(later["kept"], true, "{later}");
        // The last 30 of an app are kept, and one app's never fill another's.
        let mut at = now + EVERY * 2;
        for _ in 0..35 {
            at += EVERY;
            let sent = send(&mut live, &place, "doctor", report(), true, at);
            assert_eq!(sent["kept"], true, "{sent}");
        }
        let doctor = std::fs::read_to_string(state.join(FOLDER).join("doctor.jsonl")).unwrap();
        assert_eq!(doctor.lines().count(), KEEP);
        install(&apps, "second", json!(["turns", "report"]));
        let other = send(&mut live, &place, "second", report(), true, at);
        assert_eq!(other["kept"], true, "{other}");
        let second = std::fs::read_to_string(state.join(FOLDER).join("second.jsonl")).unwrap();
        assert_eq!(second.lines().count(), 1);
        let _ = std::fs::remove_dir_all(&apps);
        let _ = std::fs::remove_dir_all(&state);
    }
}
