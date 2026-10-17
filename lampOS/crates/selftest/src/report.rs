//! Where a run is kept: `/var/lib/lampOS/selftest/`, one JSON report per run
//! named by when it started, and `latest.json` and `latest.txt` (one line a
//! part) for the newest. Each file is written whole and then renamed, so a
//! reader or a power cut never finds half a report.
use lamp_msgs::SelfTest;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Where the lamp keeps its self-tests.
pub const DIR: &str = "/var/lib/lampOS/selftest";

/// How many runs are kept besides the newest. Older ones are removed.
pub const KEEP: usize = 20;

/// What a person reads: one line a part, then the run's verdict.
pub fn text(test: &SelfTest) -> String {
    let mut out = String::new();
    for part in &test.parts {
        out.push_str(&part.line());
        out.push('\n');
    }
    out.push_str(&format!(
        "selftest   {} in {:.0} s\n",
        test.verdict.words(),
        test.took
    ));
    out
}

/// Keeps the run in `dir` and returns the path of its own report.
pub fn keep(dir: &Path, test: &SelfTest) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(test).map_err(io::Error::other)?;
    let own = dir.join(format!("{}.json", test.started as u64));
    write_whole(&own, &json)?;
    write_whole(&dir.join("latest.json"), &json)?;
    write_whole(&dir.join("latest.txt"), text(test).as_bytes())?;
    prune(dir, KEEP)?;
    Ok(own)
}

fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, bytes)?;
    fs::File::open(&part)?.sync_all()?;
    fs::rename(&part, path)
}

/// Removes all but the newest `keep` dated reports.
fn prune(dir: &Path, keep: usize) -> io::Result<()> {
    let mut dated: Vec<(u64, PathBuf)> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.parse::<u64>().ok()?;
            (p.extension()? == "json").then_some((stem, p))
        })
        .collect();
    dated.sort_unstable_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (_, old) in dated.into_iter().skip(keep) {
        fs::remove_file(old)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{Occasion, PartResult, Verdict};

    fn test_at(started: f64) -> SelfTest {
        let parts = vec![
            PartResult::new("camera", Verdict::Pass, ""),
            PartResult::new("ring", Verdict::Fail, "red did not light"),
        ];
        SelfTest {
            occasion: Occasion::FirstBoot,
            started,
            took: 61.4,
            verdict: SelfTest::verdict_of(&parts),
            parts,
            build: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lamp-selftest-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_person_reads_one_line_a_part_and_the_verdict() {
        assert_eq!(
            text(&test_at(1.0)),
            "camera     pass\nring       fail        red did not light\nselftest   fail in 61 s\n"
        );
    }

    #[test]
    fn a_run_is_kept_whole_with_the_newest_beside_it() {
        let dir = scratch("keep");
        let own = keep(&dir, &test_at(1_760_520_000.0)).unwrap();
        assert_eq!(own, dir.join("1760520000.json"));
        let back: SelfTest = serde_json::from_slice(&fs::read(&own).unwrap()).unwrap();
        assert_eq!(back, test_at(1_760_520_000.0));
        assert_eq!(
            fs::read(dir.join("latest.json")).unwrap(),
            fs::read(&own).unwrap()
        );
        assert!(
            fs::read_to_string(dir.join("latest.txt"))
                .unwrap()
                .contains("ring       fail")
        );
        assert!(!dir.join("latest.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_the_newest_runs_are_kept() {
        let dir = scratch("prune");
        for at in 1..=(KEEP as u64 + 3) {
            keep(&dir, &test_at(at as f64)).unwrap();
        }
        let mut dated: Vec<u64> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.unwrap().path().file_stem()?.to_str()?.parse().ok())
            .collect();
        dated.sort_unstable();
        assert_eq!(dated.len(), KEEP);
        assert_eq!(dated[0], 4);
        assert!(dir.join("latest.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
