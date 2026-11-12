//! Weekly chapters (the memory owner's extraction plan, B2, after nightly
//! pass B): one short summary of a known person's local week, Monday to
//! Sunday, written by the voice's night thread
//! (`LAMP_MEMORY_WEEKLY_CHAPTERS`) from what pass B already checked and
//! kept of that week: the person's current episodes, facts, ahead items,
//! patterns and feelings whose day is in it. A chapter never reads an
//! exchange; it cites the items it rests on and is a child of every item the
//! job gave (the model read them all), so a forget of anything under them
//! takes it (`nightly::with_doomed`), while a source's expiry does not.
//!
//! Split as the nightly pass is: `weekly_prepare` gives a bounded input
//! and a job that keeps only ids, the voice asks the lamp's model, and
//! `weekly_apply` checks the one chapter it proposes and keeps it or counts
//! why not. At most one chapter a person a week. Only a known person's own
//! scope: the room, a rules scope and an unknown speaker are `bad_person`.
//! A chapter is never in the session note; `chapters` finds a person's
//! chapters of a span for a recall. No model is called here and nothing
//! is spoken or shown. The contract is `docs/memory-days.md`, "Weekly
//! chapters".
use crate::{
    days::{Days, Error, Result, ensure_person, next_id},
    nightly::{DAY_MS, MAX_ITEMS, bad, known, private},
    search,
    store::{Day, Person},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// A chapter's text at most, in bytes.
pub const CHAPTER_BYTES: usize = 600;
/// A week's input at most: items and encoded bytes.
pub const WEEK_ITEMS: usize = 48;
pub const WEEK_INPUT_BYTES: usize = 24 * 1024;
/// A week with fewer items than this has no chapter.
pub const LEAST_ITEMS: usize = 2;
/// A chapter's cites at most.
pub const CHAPTER_CITES: usize = 16;
/// A chapter closes this many days after its week's Sunday.
pub const CHAPTER_DAYS: u32 = 90;
/// What `chapters` gives at most: chapters and bytes of the answer's list.
pub const FOUND_MOST: usize = 2;
pub const FOUND_BYTES: usize = 1_200;
/// The share of a chapter's content words its cited items must hold: the
/// model sums up, it does not add.
const GROUNDED_SHARE: f64 = 1.0 / 3.0;
/// The kinds a chapter is made from.
const FROM: &str = "('episode','fact','ahead','pattern','feeling')";

/// Words that join a summary and say nothing of its own: never counted
/// against its grounding.
const JOINING: &[&str] = &[
    "about",
    "after",
    "again",
    "also",
    "been",
    "before",
    "from",
    "have",
    "into",
    "lamp",
    "more",
    "most",
    "much",
    "said",
    "some",
    "talk",
    "talked",
    "talking",
    "that",
    "their",
    "them",
    "then",
    "they",
    "this",
    "told",
    "very",
    "week",
    "were",
    "what",
    "when",
    "which",
    "while",
    "will",
    "with",
    "would",
    "your",
    "mentioned",
    "shared",
    "spoke",
    "week's",
];

fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.trim_matches('\'').to_lowercase())
        .filter(|word| word.chars().count() >= 4 && !JOINING.contains(&word.as_str()))
        .collect()
}

/// At least [`GROUNDED_SHARE`] of the chapter's content words are in its
/// cited items' words; a chapter with none is not grounded.
pub fn grounded(chapter: &str, cited: &str) -> bool {
    let words = content_words(chapter);
    if words.is_empty() {
        return false;
    }
    let known: BTreeSet<String> = content_words(cited).into_iter().collect();
    let found = words.iter().filter(|word| known.contains(*word)).count();
    found as f64 >= words.len() as f64 * GROUNDED_SHARE
}

/// A week's first day, checked: a Monday whose Sunday is before today.
fn week_of(week: &str, today: Day) -> Result<(Day, Day)> {
    let bad_week = || {
        Error::new(
            "bad_day",
            "week is a Monday, YYYY-MM-DD, of a week already over",
        )
    };
    let monday = Day::parse(week).ok_or_else(bad_week)?;
    let sunday = monday.after(6).ok_or_else(bad_week)?;
    if !monday.is_monday() || sunday >= today {
        return Err(bad_week());
    }
    Ok((monday, sunday))
}

fn job_number(job: &str) -> Result<u64> {
    job.strip_prefix('w')
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n > 0 && *n < i64::MAX as u64)
        .ok_or_else(|| bad("invalid weekly job ID"))
}

/// A chapter checked: its text, its cites' (rid, time) and every given
/// item's rid (its `sources`), or why not.
type Checked = std::result::Result<(String, Vec<(i64, u64)>, Vec<i64>), &'static str>;

/// Whether the person has a chapter of the week starting `monday`.
fn has_chapter(db: &rusqlite::Connection, person: &Person, monday: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND n.kind='chapter' AND n.night=?2)",
        params![person.as_str(), monday],
        |r| r.get(0),
    )?)
}

impl Days {
    /// `weekly_prepare`: `person`'s current nightly items of the week
    /// starting `week` (a Monday, the week over), at most [`WEEK_ITEMS`]
    /// and [`WEEK_INPUT_BYTES`]; `None` with fewer than [`LEAST_ITEMS`].
    pub fn weekly_prepare(&mut self, person: &Person, week: &str) -> Result<Option<Value>> {
        known(person)?;
        let today = Day::at(self.now, &self.local)
            .ok_or_else(|| Error::new("bad_day", "the clock is outside the calendar"))?;
        let (monday, sunday) = week_of(week, today)?;
        let (from, to) = (monday.to_string(), sunday.to_string());
        ensure_person(&self.db, person)?;
        let tx = self.db.transaction()?;
        if has_chapter(&tx, person, &from)? {
            return Err(Error::new("done", "this week already has its chapter"));
        }
        let rows = tx
            .prepare(&format!("SELECT e.id,n.kind,e.day,e.text,n.detail FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.active=1 AND n.kind IN {FROM} AND e.day BETWEEN ?2 AND ?3 ORDER BY e.day,e.id"))?
            .query_map(params![person.as_str(), from, to], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut input = json!({"job":"w0000000000000000000","person":person.as_str(),"from":from,"to":to,"items":[]});
        let mut bytes = input.to_string().len();
        let mut items = vec![];
        for (id, kind, day, text, detail) in rows {
            let item = crate::nightly::told(id, &kind, &day, &text, &detail);
            let size = item.to_string().len() + 1;
            if items.len() == WEEK_ITEMS || bytes + size > WEEK_INPUT_BYTES {
                break;
            }
            bytes += size;
            items.push(item);
        }
        if items.len() < LEAST_ITEMS {
            return Ok(None);
        }
        let ids: Vec<u64> = items.iter().filter_map(|i| i["id"].as_u64()).collect();
        let ids_text = serde_json::to_string(&ids).map_err(|_| bad("encoding"))?;
        tx.execute(
            "INSERT INTO nightly_weeks(person,week,at_unix_ms,ids,state) VALUES(?1,?2,?3,?4,'pending') ON CONFLICT(person,week) DO UPDATE SET at_unix_ms=?3,ids=?4,state='pending',result=''",
            params![person.as_str(), from, self.now, ids_text],
        )?;
        let job: u64 = tx.query_row(
            "SELECT job FROM nightly_weeks WHERE person=? AND week=?",
            params![person.as_str(), from],
            |r| r.get(0),
        )?;
        tx.commit()?;
        input["job"] = json!(format!("w{job}"));
        input["items"] = Value::Array(items);
        Ok(Some(input))
    }

    /// `weekly_apply`: the one chapter of `proposal` (`{"chapter":{"text",
    /// "cites"}}`) checked and kept, or dropped and its reason counted.
    pub fn weekly_apply(&mut self, person: &Person, job: &str, proposal: &Value) -> Result<Value> {
        known(person)?;
        let job = job_number(job)?;
        let now = self.now;
        let tx = self.db.transaction()?;
        let saved: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT state,ids,week,result FROM nightly_weeks WHERE job=? AND person=?",
                params![job, person.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((state, ids, week, result)) = saved else {
            return Err(Error::new(
                "stale",
                "the weekly job is gone or belongs to another person",
            ));
        };
        if state == "done" {
            return serde_json::from_str(&result)
                .map_err(|_| bad("stored weekly result is invalid"));
        }
        let given: Vec<u64> =
            serde_json::from_str(&ids).map_err(|_| bad("stored weekly job is invalid"))?;
        let checked = (|| -> Result<Checked> {
            let Some(chapter) = proposal.get("chapter").filter(|c| !c.is_null()) else {
                return Ok(Err("none"));
            };
            let text = match chapter.get("text").and_then(Value::as_str) {
                Some(text)
                    if !text.trim().is_empty()
                        && text.len() <= CHAPTER_BYTES
                        && !text.chars().any(char::is_control) =>
                {
                    text.to_owned()
                }
                _ => return Ok(Err("shape")),
            };
            let cites: Option<Vec<u64>> =
                chapter
                    .get("cites")
                    .and_then(Value::as_array)
                    .and_then(|list| {
                        list.iter()
                            .map(|id| id.as_u64().filter(|id| *id > 0))
                            .collect()
                    });
            let Some(cites) = cites.filter(|cites| {
                !cites.is_empty()
                    && cites.len() <= CHAPTER_CITES
                    && cites.iter().collect::<BTreeSet<_>>().len() == cites.len()
            }) else {
                return Ok(Err("shape"));
            };
            if cites.iter().any(|id| !given.contains(id)) {
                return Ok(Err("unknown_source"));
            }
            let mut sources = vec![];
            let mut words = String::new();
            let mut explicit = true;
            for id in &cites {
                let found: Option<(i64, u64, String, String, bool)> = tx
                    .query_row(
                        &format!("SELECT e.rid,e.at_unix_ms,e.text,n.detail,EXISTS (SELECT 1 FROM sources s JOIN entries p ON p.rid=s.parent WHERE s.child=e.rid AND (p.kind='kept' OR EXISTS (SELECT 1 FROM sources s2 JOIN entries q ON q.rid=s2.parent WHERE s2.child=p.rid AND q.kind='kept'))) FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.id=?2 AND e.active=1 AND n.kind IN {FROM}"),
                        params![person.as_str(), id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                    .optional()?;
                let Some((rid, at, text, detail, kept)) = found else {
                    return Ok(Err("forgotten"));
                };
                words.push_str(&text);
                words.push(' ');
                if let Ok(detail) = serde_json::from_str::<Value>(&detail)
                    && let Some(topic) = detail["topic"].as_str()
                {
                    words.push_str(topic);
                    words.push(' ');
                }
                explicit &= kept;
                sources.push((rid, at));
            }
            // The model read every item the job gave, so the chapter may
            // tell one it does not cite: each given item is a source, and
            // one gone since the prepare drops the chapter.
            let mut edges = vec![];
            for id in &given {
                let found: Option<i64> = tx
                    .query_row(
                        &format!("SELECT e.rid FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.id=?2 AND e.active=1 AND n.kind IN {FROM}"),
                        params![person.as_str(), id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(rid) = found else {
                    return Ok(Err("forgotten"));
                };
                edges.push(rid);
            }
            if !grounded(&text, &words) {
                return Ok(Err("grounded"));
            }
            if !explicit && private(&text) {
                return Ok(Err("private_kind"));
            }
            let items: u64 = tx.query_row(
                "SELECT count(*) FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND e.active=1",
                [person.as_str()],
                |r| r.get(0),
            )?;
            let observations: u64 = tx.query_row(
                "SELECT count(*) FROM entries WHERE person=? AND kind='observation' AND coalesce(category,'')!='talk'",
                [person.as_str()],
                |r| r.get(0),
            )?;
            if items >= MAX_ITEMS || observations >= crate::days::MAX_OBSERVATIONS {
                return Ok(Err("full"));
            }
            Ok(Ok((text, sources, edges)))
        })()?;
        let monday = Day::parse(&week).ok_or_else(|| bad("stored weekly job is invalid"))?;
        let sunday = monday
            .after(6)
            .ok_or_else(|| bad("stored weekly job is invalid"))?;
        let answer = match checked {
            Ok(_) if has_chapter(&tx, person, &week)? => {
                json!({"ok":true,"accepted":0,"reasons":{"done":1}})
            }
            Ok((text, sources, edges)) => {
                let id = match next_id(&tx, person) {
                    Ok(id) => Some(id),
                    Err(error) if error.code == "full" => None,
                    Err(error) => return Err(error),
                };
                match id {
                    None => json!({"ok":true,"accepted":0,"reasons":{"full":1}}),
                    Some(id) => {
                        let at = sources.iter().map(|(_, at)| *at).max().unwrap_or(now);
                        let expires = sunday.after(CHAPTER_DAYS).map(|day| day.to_string());
                        tx.execute(
                            "INSERT INTO entries(person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(?1,?2,'observation',?3,?4,'','',?5,'chapter',?6,1)",
                            params![person.as_str(), id, at, sunday.to_string(), text, now],
                        )?;
                        let rid = tx.last_insert_rowid();
                        tx.execute(
                            "INSERT INTO search(rowid,body) VALUES(?,?)",
                            params![rid, search::text(&text)],
                        )?;
                        for parent in &edges {
                            tx.execute(
                                "INSERT OR IGNORE INTO sources(child,parent) VALUES(?1,?2)",
                                params![rid, parent],
                            )?;
                        }
                        tx.execute(
                            "INSERT INTO nightly_items(rid,person,kind,night,expires,detail) VALUES(?1,?2,'chapter',?3,?4,?5)",
                            params![
                                rid,
                                person.as_str(),
                                week,
                                expires,
                                json!({"from":week,"to":sunday.to_string()}).to_string()
                            ],
                        )?;
                        json!({"ok":true,"accepted":1,"reasons":{},"id":id})
                    }
                }
            }
            Err("none") => json!({"ok":true,"accepted":0,"reasons":{}}),
            Err(reason) => json!({"ok":true,"accepted":0,"reasons":{reason:1}}),
        };
        tx.execute(
            "UPDATE nightly_weeks SET state='done',result=?2,at_unix_ms=?3 WHERE job=?1",
            params![job, answer.to_string(), now],
        )?;
        tx.commit()?;
        Ok(answer)
    }

    /// `chapters`: `person`'s current chapters whose week meets the days
    /// `from` to `to` (or the newest, without days), newest first, at most
    /// [`FOUND_MOST`] and [`FOUND_BYTES`]. Only a known person's own.
    pub fn chapters(
        &mut self,
        person: &Person,
        days: Option<(String, String)>,
    ) -> Result<Vec<Value>> {
        known(person)?;
        let (from, to) = days.unzip();
        let rows = self
            .db
            .prepare("SELECT e.id,json_extract(n.detail,'$.from'),json_extract(n.detail,'$.to'),e.text FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.active=1 AND n.kind='chapter' AND (?2 IS NULL OR (json_extract(n.detail,'$.from')<=?3 AND json_extract(n.detail,'$.to')>=?2)) ORDER BY e.day DESC,e.id DESC LIMIT ?4")?
            .query_map(
                params![person.as_str(), from, to, FOUND_MOST as u64],
                |r| {
                    Ok(json!({"id":r.get::<_,u64>(0)?,"from":r.get::<_,String>(1)?,"to":r.get::<_,String>(2)?,"text":r.get::<_,String>(3)?}))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut room = FOUND_BYTES;
        let mut found = vec![];
        for chapter in rows {
            let size = chapter.to_string().len();
            if size > room {
                break;
            }
            room -= size;
            found.push(chapter);
        }
        // What a recall returns is used.
        let tx = self.db.transaction()?;
        for chapter in &found {
            tx.execute(
                "UPDATE entries SET used_unix_ms=max(used_unix_ms,?3) WHERE person=?1 AND id=?2",
                params![person.as_str(), chapter["id"].as_u64(), self.now],
            )?;
        }
        tx.commit()?;
        Ok(found)
    }
}

/// Expiry: weekly jobs past a day go (their chapters stay).
pub(crate) fn expire(tx: &rusqlite::Transaction<'_>, now: u64) -> Result<()> {
    tx.execute(
        "DELETE FROM nightly_weeks WHERE at_unix_ms<?",
        [now.saturating_sub(DAY_MS)],
    )?;
    Ok(())
}

/// A person's weekly jobs (every person's for `None`), on `forget_all` and
/// `forget_person`; their chapters go with their entries.
pub(crate) fn empty(tx: &rusqlite::Transaction<'_>, person: Option<&Person>) -> Result<()> {
    tx.execute(
        "DELETE FROM nightly_weeks WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{days_protocol::answer, store::Options};
    use std::{fs, path::PathBuf};

    /// Tuesday 2026-10-13, 00:00 UTC: last week is 2026-10-05 to 10-11.
    const NOW: u64 = 1_791_849_600_000;
    const MAI: &str = "p-0123456789abcdef";
    const LAN: &str = "p-fedcba9876543210";
    const WEEK: &str = "2026-10-05";

    struct Scratch(PathBuf, Days);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("lamp-chapters-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let store = Days::open(
                Options {
                    dir: dir.clone(),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap();
            Self(dir, store)
        }

        fn ask(&mut self, request: Value, now: u64) -> Value {
            serde_json::from_str(&answer(&mut self.1, request.to_string().as_bytes(), now).line)
                .unwrap()
        }

        /// `who`'s night of `day_ms`: their lines, and the items `items`
        /// (a nightly proposal, ids by place) proposes on them; the kept
        /// items' ids and the lines' ids.
        fn night(
            &mut self,
            who: &str,
            day_ms: u64,
            lines: &[&str],
            items: impl Fn(&[u64]) -> Value,
        ) -> (Vec<u64>, Vec<u64>) {
            let person = Person::new(who).unwrap();
            let ids: Vec<u64> = lines
                .iter()
                .enumerate()
                .map(|(n, heard)| {
                    self.1
                        .append(&person, day_ms + 3_600_000 + n as u64, heard, "Nice.", &[])
                        .unwrap()
                })
                .collect();
            let day = Day::at(day_ms, &crate::store::LocalTime::Offset(0))
                .unwrap()
                .to_string();
            let input = self.ask(json!({"op":"nightly_prepare","person":who,"day":day}), NOW);
            let got = self.ask(
                json!({"op":"nightly_apply","person":who,"job":input["input"]["job"],"proposal":items(&ids)}),
                NOW,
            );
            assert_eq!(got["reasons"], json!({}), "{got}");
            let kept = got["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap())
                .collect();
            (kept, ids)
        }

        /// Mai's week: the library on Tuesday, the sister's visit and the
        /// pottery class on Thursday.
        fn week(&mut self) -> (Vec<u64>, Vec<u64>) {
            let (mut items, mut lines) = self.night(
                MAI,
                NOW - 7 * DAY_MS,
                &["I started at the city library today.", "I shelve the history books."],
                |ids| {
                    json!({
                        "episodes":[{"first":ids[0],"last":ids[1],"topic":"new library job","gist":"Started work at the city library, shelving history books."}],
                        "facts":[{"kind":"work","claim":"Works at the city library.","cites":[ids[0]]}]
                    })
                },
            );
            let (more, more_lines) = self.night(
                MAI,
                NOW - 5 * DAY_MS,
                &["My sister Lan visited and we went to a pottery class."],
                |ids| {
                    json!({
                        "facts":[{"kind":"person","claim":"Sister Lan visited; they went to a pottery class.","cites":[ids[0]]}]
                    })
                },
            );
            items.extend(more);
            lines.extend(more_lines);
            (items, lines)
        }

        fn prepare(&mut self, who: &str, week: &str, now: u64) -> Value {
            self.ask(json!({"op":"weekly_prepare","person":who,"week":week}), now)
        }

        fn apply(&mut self, who: &str, job: &Value, proposal: Value, now: u64) -> Value {
            self.ask(
                json!({"op":"weekly_apply","person":who,"job":job,"proposal":proposal}),
                now,
            )
        }

        fn chapters(&mut self, who: &str, from: &str, to: &str, now: u64) -> Value {
            self.ask(
                json!({"op":"chapters","person":who,"day_from":from,"day_to":to}),
                now,
            )
        }

        fn count(&self, sql: &str) -> u64 {
            self.1.db.query_row(sql, [], |r| r.get(0)).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const GOOD: &str = "A new start at the city library shelving history books, and sister Lan visited for a pottery class.";

    /// Mai's week made into its chapter: its id.
    fn chaptered(s: &mut Scratch) -> (u64, Vec<u64>, Vec<u64>) {
        let (items, lines) = s.week();
        let input = s.prepare(MAI, WEEK, NOW + 10);
        let got = s.apply(
            MAI,
            &input["input"]["job"],
            json!({"chapter":{"text":GOOD,"cites":items}}),
            NOW + 20,
        );
        assert_eq!(got["accepted"], 1, "{got}");
        (got["id"].as_u64().unwrap(), items, lines)
    }

    #[test]
    fn a_weeks_items_make_one_chapter_found_for_that_week() {
        let mut s = Scratch::new("made");
        let (items, _) = s.week();
        let input = s.prepare(MAI, WEEK, NOW + 10);
        assert_eq!(input["input"]["from"], WEEK, "{input}");
        assert_eq!(input["input"]["to"], "2026-10-11");
        let given: Vec<u64> = input["input"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_u64().unwrap())
            .collect();
        assert_eq!(given, items, "the week's episode and facts, in day order");
        // The job keeps ids only, never words.
        let job: String =
            s.1.db
                .query_row("SELECT ids FROM nightly_weeks", [], |r| r.get(0))
                .unwrap();
        assert!(!job.contains("library"));
        let got = s.apply(
            MAI,
            &input["input"]["job"],
            json!({"chapter":{"text":GOOD,"cites":items},"other":1}),
            NOW + 20,
        );
        assert_eq!(got["accepted"], 1, "{got}");
        // Applied again: the same answer, nothing written twice.
        let again = s.apply(MAI, &input["input"]["job"], json!({}), NOW + 25);
        assert_eq!(again, got);
        assert_eq!(
            s.count("SELECT count(*) FROM nightly_items WHERE kind='chapter'"),
            1
        );
        // One chapter a week.
        assert_eq!(s.prepare(MAI, WEEK, NOW + 30)["error"], "done");
        // "What did we talk about last week": a span meeting the week.
        let found = s.chapters(MAI, "2026-10-06", "2026-10-12", NOW + 40);
        assert_eq!(found["chapters"].as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found["chapters"][0]["text"], GOOD);
        assert_eq!(found["chapters"][0]["from"], WEEK);
        assert_eq!(found["chapters"][0]["to"], "2026-10-11");
        // A span of another week has none.
        let before = s.chapters(MAI, "2026-09-21", "2026-10-04", NOW + 40);
        assert_eq!(before["chapters"], json!([]));
    }

    #[test]
    fn a_chapter_is_each_persons_own_and_never_the_rooms() {
        let mut s = Scratch::new("people");
        s.night(
            LAN,
            NOW - 6 * DAY_MS,
            &["I play jazz piano on Fridays.", "I want a new piano teacher."],
            |ids| {
                json!({"facts":[
                    {"kind":"habit","claim":"Plays jazz piano on Fridays.","cites":[ids[0]]},
                    {"kind":"plan","claim":"Wants a new piano teacher.","cites":[ids[1]],"strength":"maybe"}
                ]})
            },
        );
        chaptered(&mut s);
        // Mai's week never shows Lan's items; Lan's job has only hers.
        let theirs = s.prepare(LAN, WEEK, NOW + 30);
        let text = theirs.to_string();
        assert!(text.contains("jazz") && !text.contains("library"), "{text}");
        // Lan's span finds no chapter of Mai's.
        let found = s.chapters(LAN, WEEK, "2026-10-11", NOW + 40);
        assert_eq!(found["chapters"], json!([]));
        // The room, a rules scope, an unknown speaker: none of the three.
        for person in [json!(null), json!("everyone"), json!("r-0123456789abcdef")] {
            for mut request in [
                json!({"op":"weekly_prepare","week":WEEK}),
                json!({"op":"weekly_apply","job":"w1","proposal":{}}),
                json!({"op":"chapters","day_from":WEEK,"day_to":"2026-10-11"}),
            ] {
                if !person.is_null() {
                    request["person"] = person.clone();
                }
                assert_eq!(s.ask(request, NOW + 50)["error"], "bad_person");
            }
        }
    }

    #[test]
    fn a_forget_under_a_chapter_takes_it() {
        let mut s = Scratch::new("forget");
        let (chapter, items, lines) = chaptered(&mut s);
        let active = |s: &Scratch, id: u64| {
            s.count(&format!(
                "SELECT count(*) FROM entries WHERE person='{MAI}' AND id={id}"
            ))
        };
        assert_eq!(active(&s, chapter), 1);
        // Forgetting Thursday's line takes its fact, and the chapter on it.
        s.ask(json!({"op":"forget","person":MAI,"id":lines[2]}), NOW + 40);
        assert_eq!(active(&s, items[2]), 0);
        assert_eq!(active(&s, chapter), 0, "the chapter rests on the fact");
        assert_eq!(active(&s, items[0]), 1);
        let found = s.chapters(MAI, WEEK, "2026-10-11", NOW + 50);
        assert_eq!(found["chapters"], json!([]));
    }

    #[test]
    fn forgetting_everything_takes_every_chapter_and_job() {
        let mut s = Scratch::new("everything");
        chaptered(&mut s);
        // A pending job of the week before too.
        s.night(
            MAI,
            NOW - 12 * DAY_MS,
            &[
                "I baked bread twice this week.",
                "The rye loaf came out best.",
            ],
            |ids| {
                json!({"facts":[
                    {"kind":"habit","claim":"Bakes bread.","cites":[ids[0]]},
                    {"kind":"taste","claim":"Likes the rye loaf best.","cites":[ids[1]]}
                ]})
            },
        );
        assert!(s.prepare(MAI, "2026-09-28", NOW + 30)["input"]["job"].is_string());
        s.ask(json!({"op":"forget_all","person":MAI}), NOW + 40);
        assert_eq!(
            s.count("SELECT (SELECT count(*) FROM nightly_items WHERE kind='chapter')+(SELECT count(*) FROM nightly_weeks)"),
            0
        );
    }

    #[test]
    fn a_chapter_is_checked_alone_and_dropped_by_reason() {
        let mut s = Scratch::new("checks");
        let (items, lines) = s.week();
        let job = |s: &mut Scratch| {
            s.1.db.execute("DELETE FROM nightly_weeks", []).unwrap();
            s.prepare(MAI, WEEK, NOW + 10)["input"]["job"].clone()
        };
        let long = "library ".repeat(80);
        for (proposal, reason) in [
            (json!({"chapter":{"text":"","cites":[items[0]]}}), "shape"),
            (json!({"chapter":{"text":long,"cites":[items[0]]}}), "shape"),
            (
                json!({"chapter":{"text":"Library\u{7}week.","cites":[items[0]]}}),
                "shape",
            ),
            (json!({"chapter":{"text":GOOD,"cites":[]}}), "shape"),
            (
                json!({"chapter":{"text":GOOD,"cites":[items[0], items[0]]}}),
                "shape",
            ),
            (json!({"chapter":{"text":GOOD}}), "shape"),
            (
                json!({"chapter":{"text":GOOD,"cites":[999]}}),
                "unknown_source",
            ),
            (
                json!({"chapter":{"text":"Planned a skydiving trip to Peru with the mountain club.","cites":items}}),
                "grounded",
            ),
            (
                json!({"chapter":{"text":"Started at the city library; worried about her blood pressure medicine.","cites":items}}),
                "private_kind",
            ),
        ] {
            let job = job(&mut s);
            let got = s.apply(MAI, &job, proposal.clone(), NOW + 20);
            assert_eq!(got["reasons"], json!({reason:1}), "{proposal}: {got}");
            assert_eq!(got["accepted"], 0);
        }
        // Nothing proposed: closed with nothing kept.
        let job_id = job(&mut s);
        let got = s.apply(MAI, &job_id, json!({}), NOW + 20);
        assert_eq!(got, json!({"ok":true,"accepted":0,"reasons":{}}));
        // A forget during the pass: the chapter citing it is dropped.
        let job_id = job(&mut s);
        s.ask(json!({"op":"forget","person":MAI,"id":lines[2]}), NOW + 15);
        let got = s.apply(
            MAI,
            &job_id,
            json!({"chapter":{"text":GOOD,"cites":items}}),
            NOW + 20,
        );
        assert_eq!(got["reasons"], json!({"forgotten":1}), "{got}");
        assert_eq!(
            s.count("SELECT count(*) FROM nightly_items WHERE kind='chapter'"),
            0
        );
    }

    #[test]
    fn only_a_week_over_with_two_items_has_a_chapter() {
        let mut s = Scratch::new("weeks");
        // This week is not over; a Tuesday is no week's start.
        assert_eq!(s.prepare(MAI, "2026-10-12", NOW)["error"], "bad_day");
        assert_eq!(s.prepare(MAI, "2026-10-06", NOW)["error"], "bad_day");
        assert_eq!(s.prepare(MAI, "2026-13-01", NOW)["error"], "bad_day");
        // An empty week, or one item: nothing.
        assert_eq!(s.prepare(MAI, WEEK, NOW)["reason"], "nothing");
        s.night(
            MAI,
            NOW - 7 * DAY_MS,
            &["I started at the city library today."],
            |ids| json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[ids[0]]}]}),
        );
        assert_eq!(s.prepare(MAI, WEEK, NOW)["reason"], "nothing");
    }

    #[test]
    fn a_chapter_outlives_a_plan_it_cites() {
        let mut s = Scratch::new("plan");
        let (mut items, lines) = s.night(
            MAI,
            NOW - 7 * DAY_MS,
            &["I started at the city library today.", "I shelve the history books."],
            |ids| {
                json!({"episodes":[{"first":ids[0],"last":ids[1],"topic":"new library job","gist":"Started work at the city library, shelving history books."}]})
            },
        );
        let until = Day::at(NOW + 3 * DAY_MS, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        let (plan, _) = s.night(
            MAI,
            NOW - 5 * DAY_MS,
            &["My sister Lan visits on Friday."],
            |ids| {
                json!({"ahead":[{"what":"plan","text":"Sister Lan visits on Friday.","until":until,"cites":[ids[0]]}]})
            },
        );
        items.extend(plan);
        let input = s.prepare(MAI, WEEK, NOW + 10);
        let got = s.apply(
            MAI,
            &input["input"]["job"],
            json!({"chapter":{"text":"A new start at the city library shelving history books, and sister Lan visits on Friday.","cites":items}}),
            NOW + 20,
        );
        assert_eq!(got["accepted"], 1, "{got}");
        let chapter = got["id"].as_u64().unwrap();
        let active = |s: &Scratch, id: u64| {
            s.count(&format!(
                "SELECT count(*) FROM entries WHERE person='{MAI}' AND id={id}"
            ))
        };
        // The plan's day passes: the plan closes, the chapter stays.
        s.ask(json!({"op":"status","person":MAI}), NOW + 10 * DAY_MS);
        assert_eq!(active(&s, items[1]), 0, "the plan closes on its day");
        assert_eq!(active(&s, chapter), 1, "a source's expiry is no correction");
        // A forget still takes it.
        s.ask(
            json!({"op":"forget","person":MAI,"id":lines[0]}),
            NOW + 10 * DAY_MS + 10,
        );
        assert_eq!(active(&s, chapter), 0, "the chapter rests on the episode");
    }

    #[test]
    fn a_forget_of_an_item_given_but_not_cited_takes_the_chapter() {
        let mut s = Scratch::new("uncited");
        let (items, lines) = s.week();
        let input = s.prepare(MAI, WEEK, NOW + 10);
        // The text tells Thursday's visit; only Tuesday's items are cited.
        let got = s.apply(
            MAI,
            &input["input"]["job"],
            json!({"chapter":{"text":GOOD,"cites":items[..2]}}),
            NOW + 20,
        );
        assert_eq!(got["accepted"], 1, "{got}");
        let chapter = got["id"].as_u64().unwrap();
        s.ask(json!({"op":"forget","person":MAI,"id":lines[2]}), NOW + 40);
        assert_eq!(
            s.count(&format!(
                "SELECT count(*) FROM entries WHERE person='{MAI}' AND id={chapter}"
            )),
            0,
            "the chapter may tell what it was given"
        );
        assert_eq!(
            s.chapters(MAI, WEEK, "2026-10-11", NOW + 50)["chapters"],
            json!([])
        );
    }

    #[test]
    fn an_item_given_but_not_cited_forgotten_during_the_pass_drops_the_chapter() {
        let mut s = Scratch::new("uncited-pass");
        let (items, lines) = s.week();
        let input = s.prepare(MAI, WEEK, NOW + 10);
        s.ask(json!({"op":"forget","person":MAI,"id":lines[2]}), NOW + 15);
        let got = s.apply(
            MAI,
            &input["input"]["job"],
            json!({"chapter":{"text":GOOD,"cites":items[..2]}}),
            NOW + 20,
        );
        assert_eq!(got["reasons"], json!({"forgotten":1}), "{got}");
        assert_eq!(
            s.count("SELECT count(*) FROM nightly_items WHERE kind='chapter'"),
            0
        );
    }

    #[test]
    fn a_chapter_is_never_in_the_session_note_and_closes_after_ninety_days() {
        let mut s = Scratch::new("told");
        let (chapter, _, _) = chaptered(&mut s);
        let opened = s.ask(
            json!({"op":"session_open","person":MAI,"session":"s1"}),
            NOW + 30,
        );
        assert!(
            !opened.to_string().contains("pottery class."),
            "the note has no chapter: {opened}"
        );
        let active = |s: &Scratch| {
            s.count(&format!(
                "SELECT count(*) FROM entries WHERE person='{MAI}' AND id={chapter}"
            ))
        };
        s.ask(json!({"op":"status","person":MAI}), NOW + 80 * DAY_MS);
        assert_eq!(active(&s), 1, "not with its week's exchanges (30 days)");
        s.ask(json!({"op":"status","person":MAI}), NOW + 92 * DAY_MS);
        assert_eq!(active(&s), 0, "90 days after its Sunday");
    }

    #[test]
    fn what_is_given_and_found_is_bounded() {
        let mut s = Scratch::new("bounds");
        // A week of many facts: the input keeps to its bounds.
        for day in 0..7u64 {
            s.night(
                MAI,
                NOW - (8 - day) * DAY_MS,
                &[
                    &format!("On day {day} I read a long novel about sailing the northern seas and whales and storms."),
                    &format!("On day {day} I cooked a big pot of vegetable soup with lentils, carrots and thyme."),
                    &format!("On day {day} I walked along the river to the old stone bridge and back home again."),
                    &format!("On day {day} I phoned my cousin about the family reunion planned in the summer."),
                    &format!("On day {day} I practised the violin scales for an hour before the evening news."),
                    &format!("On day {day} I watered the tomato plants on the balcony and picked a few."),
                    &format!("On day {day} I fixed the squeaky kitchen door hinge with a little oil."),
                    &format!("On day {day} I sorted old photographs into albums by year and place."),
                ],
                |ids| {
                    json!({"facts":ids.iter().enumerate().map(|(n, id)| json!({"kind":"other","claim":format!("Did thing number {n} of the day, a long description so the input grows quite fast {id}."),"cites":[id]})).collect::<Vec<_>>()})
                },
            );
        }
        let input = s.prepare(MAI, WEEK, NOW + 10);
        let items = input["input"]["items"].as_array().unwrap();
        assert!(items.len() <= WEEK_ITEMS, "{}", items.len());
        assert!(input["input"].to_string().len() <= WEEK_INPUT_BYTES);
        // Chapters found: at most two and their bytes.
        for (n, monday) in ["2026-09-21", "2026-09-28", WEEK].iter().enumerate() {
            let week_start = Day::parse(monday).unwrap();
            s.night(
                LAN,
                NOW - (22 - 7 * n as u64) * DAY_MS,
                &[
                    &format!("In week {n} I played jazz piano at the {monday} gig."),
                    &format!("In week {n} I met piano teacher number {n}."),
                ],
                |ids| {
                    json!({"facts":[
                        {"kind":"event","claim":format!("Played jazz piano at the {monday} gig."),"cites":[ids[0]]},
                        {"kind":"person","claim":format!("Met piano teacher number {n}."),"cites":[ids[1]]}
                    ]})
                },
            );
            let input = s.prepare(LAN, &week_start.to_string(), NOW + 10);
            let cites: Vec<u64> = input["input"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["id"].as_u64().unwrap())
                .collect();
            let text = format!(
                "{} Jazz piano at a gig and a piano teacher.",
                "Jazz piano. ".repeat(40)
            );
            let got = s.apply(
                LAN,
                &input["input"]["job"],
                json!({"chapter":{"text":text,"cites":cites}}),
                NOW + 20,
            );
            assert_eq!(got["accepted"], 1, "{got}");
        }
        let found = s.chapters(LAN, "2026-09-21", "2026-10-11", NOW + 30);
        let list = found["chapters"].as_array().unwrap();
        assert!(!list.is_empty() && list.len() <= FOUND_MOST, "{found}");
        assert!(found["chapters"].to_string().len() <= FOUND_BYTES + 2);
        assert_eq!(list[0]["from"], WEEK, "newest first");
    }

    #[test]
    fn grounding_counts_content_words_only() {
        let cited = "Works at the city library. Sister Lan visited for pottery.";
        assert!(grounded(
            "This week she talked about the city library and pottery.",
            cited
        ));
        assert!(!grounded(
            "A skydiving trip to Peru with the mountain club.",
            cited
        ));
        assert!(!grounded("This week.", cited));
    }
}
