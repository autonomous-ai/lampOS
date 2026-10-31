use super::*;

const MEASUREMENT_BUDGET: Duration = Duration::from_millis(500);
const TIERS: [&str; 8] = [
    "record",
    "episodes",
    "life_story",
    "facts",
    "owner_model",
    "how_to_be",
    "ahead",
    "feelings",
];

/// Query-independent eligible inventory, before matching, ranking or byte caps.
/// Unknown measurements are null rather than a partial count or an invented zero.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TierPopulation {
    pub items: Option<u64>,
    pub text_bytes: Option<u64>,
}

/// Group returned item kinds using the benchmark's registered component names.
pub fn memory_tier(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "record" => "record",
        "episode" => "episodes",
        "chapter" | "event" => "life_story",
        "fact" | "person" | "place" => "facts",
        "owner_model" => "owner_model",
        "taught_rule" | "learned_pattern" => "how_to_be",
        "ahead" => "ahead",
        "feeling" => "feelings",
        _ => return None,
    })
}

fn inventory(arm: Arm, known: bool) -> BTreeMap<String, TierPopulation> {
    TIERS
        .into_iter()
        .chain((arm == Arm::TiersMinusFacts).then_some("evidence_only_facts"))
        .map(|name| {
            (
                name.into(),
                TierPopulation {
                    items: known.then_some(0),
                    text_bytes: known.then_some(0),
                },
            )
        })
        .collect()
}

fn add(out: &mut BTreeMap<String, TierPopulation>, tier: &str, bytes: usize) {
    let value = out.get_mut(tier).expect("registered tier");
    *value.items.as_mut().expect("complete measurement") += 1;
    *value.text_bytes.as_mut().expect("complete measurement") += bytes as u64;
}

fn on_time(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}

impl Store {
    /// Count eligible stored tier text without mutating memory or acknowledging
    /// retrieval. UTF-8 text excludes JSON, labels, timestamps and indexes.
    /// Raw episode fallback references record text, so its episode bytes are zero.
    /// Capture pause does not hide existing authorized memory.
    pub fn tier_population(
        &self,
        access: Access,
        now_ms: i64,
        arm: Arm,
    ) -> Result<BTreeMap<String, TierPopulation>> {
        self.measure_population(access, now_ms, arm, MEASUREMENT_BUDGET)
    }

    fn measure_population(
        &self,
        access: Access,
        now_ms: i64,
        arm: Arm,
        budget: Duration,
    ) -> Result<BTreeMap<String, TierPopulation>> {
        if !access.permits() {
            return Ok(BTreeMap::new());
        }
        crate::store::valid_time(now_ms)?;
        let deadline = Instant::now() + budget;
        self.conn
            .progress_handler(128, Some(move || Instant::now() >= deadline))?;
        let guard = Interrupt(&self.conn);
        let measured = self.population_before(now_ms, arm, deadline);
        drop(guard);
        match measured {
            Err(Error::Timeout) => Ok(inventory(arm, false)),
            Err(Error::Database(rusqlite::Error::SqliteFailure(code, _)))
                if code.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                Ok(inventory(arm, false))
            }
            other => other,
        }
    }

    fn population_before(
        &self,
        now_ms: i64,
        arm: Arm,
        deadline: Instant,
    ) -> Result<BTreeMap<String, TierPopulation>> {
        on_time(deadline)?;
        let mut out = inventory(arm, true);
        let mut statement = self
            .conn
            .prepare("SELECT body FROM records WHERE speaker=? AND at_ms<=? ORDER BY id")?;
        for row in statement.query_map(params![encode(&Speaker::Owner)?, now_ms], |r| {
            r.get::<_, String>(0)
        })? {
            let record: Record = decode(&row?)?;
            add(&mut out, "record", record.text.len());
            on_time(deadline)?;
        }
        let mut statement = self.conn.prepare("SELECT id FROM facts ORDER BY id")?;
        for id in statement.query_map([], |r| r.get::<_, i64>(0))? {
            if let Some(fact) = self.read_fact(id?, now_ms)? {
                let bytes =
                    fact.claim.len() + fact.quotes.iter().map(|q| q.text.len()).sum::<usize>();
                add(
                    &mut out,
                    if arm == Arm::TiersMinusFacts {
                        "evidence_only_facts"
                    } else {
                        "facts"
                    },
                    bytes,
                );
            }
            on_time(deadline)?;
        }
        let mut statement = self.conn.prepare("SELECT id,body FROM lines ORDER BY id")?;
        for row in statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, body) = row?;
            let line: MemoryLine = decode(&body)?;
            if let Some(candidate) =
                self.line_candidate(id, now_ms, 0.0, arm, line.kind == LineKind::OwnerModel)?
            {
                add(
                    &mut out,
                    memory_tier(&candidate.item.kind).expect("stored line kind"),
                    line.text.len(),
                );
            }
            on_time(deadline)?;
        }
        if arm != Arm::TiersMinusEpisodes {
            let query = RecallQuery {
                access: Access::OWNER_ALONE,
                now_ms,
                words: String::new(),
                cues: Cues::default(),
                max_bytes: 0,
                budget: Duration::ZERO,
                arm,
            };
            let mut statement = self
                .conn
                .prepare("SELECT id,gist,detail FROM episodes WHERE started_ms<=? ORDER BY id")?;
            for row in statement.query_map([now_ms], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (id, gist, detail) = row?;
                if self.episode_candidate(id, &query, 0.0)?.is_some() {
                    add(&mut out, "episodes", gist.len() + detail.len());
                }
                on_time(deadline)?;
            }
        }
        on_time(deadline)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_inventory_is_unknown_and_releases_sqlite_interrupt() {
        let store = Store::in_memory("fictional-owner").unwrap();
        let result = store
            .measure_population(Access::OWNER_ALONE, 1000, Arm::Tiers, Duration::ZERO)
            .unwrap();
        assert_eq!(result.len(), TIERS.len());
        assert!(
            result
                .values()
                .all(|v| v.items.is_none() && v.text_bytes.is_none())
        );
        let complete = store
            .tier_population(Access::OWNER_ALONE, 1000, Arm::Tiers)
            .unwrap();
        assert!(
            complete
                .values()
                .all(|v| v.items == Some(0) && v.text_bytes == Some(0))
        );
    }
}
