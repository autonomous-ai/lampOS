//! `lamp-voice-eval mood-minute DIR`: the numbers of Mood's lamp minute
//! (#1670, `docs/mood.md`, "Mood on a lamp"), from the tap files
//! `tools/mood-minute.sh` writes into `DIR`: `off-body.jsonl` and
//! `on-body.jsonl` (`expression/body`), `on-mood.jsonl` (`mood/state`), and
//! the marks `on-clap-at` and `on-stopped-at` (seconds into the on minute).
//!
//! It says the head's breath period, its fit to the plain breath with Mood
//! off and to Mood's breath with Mood on (R of `head_pitch` against the sine
//! and cosine of the phase, 0 to 1; in step at [`IN_STEP`]), `mood/state`'s
//! checks, and after the marks the clap's pace and the head's fit once Mood
//! stopped. Mood's phase at each sample is [`MoodState::breath_at`]'s, the
//! plain one [`plain_breath_phase`]'s: the readers' own. Numbers only.
use lamp_msgs::{MOOD_PACE_QUICKEST, MOOD_PACE_SLOWEST, MoodState, plain_breath_phase};
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
};

/// At or over this fit, the head keeps in step with the breath.
pub const IN_STEP: f64 = 0.8;
/// A clap quickens the breath past this pace.
pub const QUICKENED: f32 = 1.1;
/// After a clap, the pace is looked at this long.
pub const CLAP_WINDOW_US: u64 = 10_000_000;
/// After Mood stops, the head is judged from this long on: past Mood's
/// 2.5 s and the readers' easing back.
pub const AFTER_STOP_US: u64 = 3_000_000;

/// One tap line: its header and its payload, as `lamp-bus tap` prints them.
pub fn tap_line(line: &str) -> Option<(Value, Value)> {
    let mut values = serde_json::Deserializer::from_str(line).into_iter::<Value>();
    let header = values.next()?.ok()?;
    let payload = values.next()?.ok()?;
    Some((header, payload))
}

fn lines(path: &Path) -> Vec<(Value, Value)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(tap_line)
        .collect()
}

/// The head's pitch offsets, by the header's monotonic time.
pub fn pitch(lines: &[(Value, Value)]) -> Vec<(u64, f64)> {
    lines
        .iter()
        .filter_map(|(header, body)| {
            Some((
                header["at_us"].as_u64()?,
                body["offsets"]["head_pitch"].as_f64()?,
            ))
        })
        .collect()
}

/// R of the samples against the sine and cosine of `phase_of`'s phase, from
/// 0 to 1, or `None` with under 20 samples given a phase or no movement.
pub fn fit(samples: &[(u64, f64)], phase_of: impl Fn(u64) -> Option<f64>) -> Option<f64> {
    let rows: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|&(at, value)| phase_of(at).map(|phase| (value, phase)))
        .collect();
    if rows.len() < 20 {
        return None;
    }
    let mean = rows.iter().map(|(v, _)| v).sum::<f64>() / rows.len() as f64;
    let (mut ss, mut cc, mut sc, mut ys, mut yc, mut yy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for (value, phase) in rows {
        let y = value - mean;
        let (s, c) = (std::f64::consts::TAU * phase).sin_cos();
        ss += s * s;
        cc += c * c;
        sc += s * c;
        ys += y * s;
        yc += y * c;
        yy += y * y;
    }
    let det = ss * cc - sc * sc;
    if det <= 0.0 || yy <= 0.0 {
        return None;
    }
    let a = (ys * cc - yc * sc) / det;
    let b = (yc * ss - ys * sc) / det;
    Some(((a * ys + b * yc) / yy).clamp(0.0, 1.0).sqrt())
}

/// The breath's period from the samples' rising crossings of their mean, in
/// seconds: the median gap over one second.
pub fn period_s(samples: &[(u64, f64)]) -> Option<f64> {
    if samples.len() < 20 {
        return None;
    }
    let mean = samples.iter().map(|(_, v)| v).sum::<f64>() / samples.len() as f64;
    let ups: Vec<u64> = samples
        .windows(2)
        .filter(|pair| pair[0].1 < mean && mean <= pair[1].1)
        .map(|pair| pair[1].0)
        .collect();
    let mut gaps: Vec<u64> = ups
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap > 1_000_000)
        .collect();
    gaps.sort_unstable();
    gaps.get(gaps.len() / 2).map(|gap| *gap as f64 / 1e6)
}

/// Mood's states from the tap, from source `mood` only, by time.
pub fn moods(lines: &[(Value, Value)]) -> Vec<MoodState> {
    let mut states: Vec<MoodState> = lines
        .iter()
        .filter(|(header, _)| header["source"] == "mood")
        .filter_map(|(_, body)| serde_json::from_value(body.clone()).ok())
        .collect();
    states.sort_by_key(|state| state.stamp.at_us);
    states
}

/// Mood's breath at `at_us`, from the newest state at or before it.
pub fn mood_phase(states: &[MoodState], at_us: u64) -> Option<f64> {
    let newest = states
        .iter()
        .rev()
        .find(|state| state.stamp.at_us <= at_us)?;
    newest.breath_at(at_us).map(|(phase, _)| phase)
}

fn shown(value: Option<f64>) -> String {
    value.map_or("none".to_owned(), |value| format!("{value:.2}"))
}

fn mark(dir: &Path, name: &str) -> Option<u64> {
    std::fs::read_to_string(dir.join(name))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The minute's lines, from the files in `dir`.
pub fn report(dir: &Path) -> Vec<String> {
    let at = |name: &str| -> PathBuf { dir.join(name) };
    let plain = |at_us: u64| Some(plain_breath_phase(at_us));
    let mut said = Vec::new();
    let off = pitch(&lines(&at("off-body.jsonl")));
    said.push(format!(
        "off head_pitch samples={} period_s={} fit_to_plain_breath={}",
        off.len(),
        shown(period_s(&off)),
        shown(fit(&off, plain))
    ));
    let states = moods(&lines(&at("on-mood.jsonl")));
    let mut sessions: Vec<&str> = states.iter().map(|s| s.session_id.as_str()).collect();
    sessions.dedup();
    let rising = states
        .windows(2)
        .all(|pair| pair[1].stamp.sequence > pair[0].stamp.sequence);
    let paces: Vec<f32> = states
        .iter()
        .filter_map(|state| state.breath.map(|breath| breath.pace))
        .collect();
    let bounded = paces
        .iter()
        .all(|pace| (MOOD_PACE_SLOWEST..=MOOD_PACE_QUICKEST).contains(pace));
    let lowest = paces.iter().copied().reduce(f32::min);
    let highest = paces.iter().copied().reduce(f32::max);
    said.push(format!(
        "on mood/state messages={} sessions={} sequences_rising={rising} pace_min={} pace_max={} pace_bounded={bounded}",
        states.len(),
        sessions.len(),
        shown(lowest.map(f64::from)),
        shown(highest.map(f64::from)),
    ));
    let on = pitch(&lines(&at("on-body.jsonl")));
    let start = on.first().map_or(0, |(at, _)| *at);
    let stopped = mark(dir, "on-stopped-at");
    let running: Vec<(u64, f64)> = on
        .iter()
        .copied()
        .filter(|(at, _)| stopped.is_none_or(|s| *at < start + s * 1_000_000))
        .collect();
    let r = fit(&running, |at_us| mood_phase(&states, at_us));
    said.push(format!(
        "on head_pitch samples={} period_s={} fit_to_mood_breath={} in_step={}",
        on.len(),
        shown(period_s(&running)),
        shown(r),
        r.is_some_and(|r| r >= IN_STEP)
    ));
    if let Some(stopped) = stopped {
        let after: Vec<(u64, f64)> = on
            .iter()
            .copied()
            .filter(|(at, _)| *at >= start + stopped * 1_000_000 + AFTER_STOP_US)
            .collect();
        said.push(format!(
            "stopped at {stopped} s: head_pitch after samples={} fit_to_plain_breath={}",
            after.len(),
            shown(fit(&after, plain))
        ));
    }
    if let (Some(clap), Some(first)) = (mark(dir, "on-clap-at"), states.first()) {
        let from = first.stamp.at_us + clap * 1_000_000;
        let quickest = states
            .iter()
            .filter(|state| state.stamp.at_us >= from && state.stamp.at_us - from <= CLAP_WINDOW_US)
            .filter_map(|state| state.breath.map(|breath| breath.pace))
            .reduce(f32::max);
        said.push(format!(
            "clap at {clap} s: pace_max_after={} quickened={}",
            shown(quickest.map(f64::from)),
            quickest.is_some_and(|pace| pace > QUICKENED)
        ));
    }
    said
}

pub fn run(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "mood-minute needs the folder mood-minute.sh wrote",
        ));
    }
    for line in report(dir) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{ClockStamp, Familiarity, MOOD_BREATH_US, MoodBreath};
    use serde_json::json;

    const START: u64 = 2_000_000_000;

    fn mood(at_us: u64, sequence: u64, phase: f64, pace: f32) -> MoodState {
        MoodState {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us,
                sequence,
            },
            session_id: "s1".into(),
            awake: 0.7,
            energy: 0.3,
            warmth: 0.6,
            asleep: false,
            attention: None,
            familiarity: Familiarity::Unknown,
            familiar: None,
            breath: Some(MoodBreath {
                phase: phase as f32,
                pace,
            }),
        }
    }

    /// A minute of Mood at `pace`, its phase 0.3 at the start, one state a
    /// second, and the head's pitch at 50 a second following `follow`.
    fn minute(dir: &Path, label: &str, pace: f32, follow: impl Fn(u64) -> f64, with_mood: bool) {
        let phase =
            |t: u64| (0.3 + (t - START) as f64 / MOOD_BREATH_US as f64 * f64::from(pace)) % 1.0;
        let body: Vec<String> = (0..3000u64)
            .map(|i| {
                let t = START + i * 20_000;
                format!(
                    "{} {}",
                    json!({"topic": "expression/body", "source": "expression", "at_us": t}),
                    json!({"offsets": {"head_pitch": 0.02 * (std::f64::consts::TAU * follow(t)).sin()}})
                )
            })
            .collect();
        std::fs::write(dir.join(format!("{label}-body.jsonl")), body.join("\n")).unwrap();
        if with_mood {
            let states: Vec<String> = (0..60u64)
                .map(|i| {
                    let t = START + i * 1_000_000;
                    format!(
                        "{} {}",
                        json!({"topic": "mood/state", "source": "mood", "at_us": t}),
                        serde_json::to_string(&mood(t, i + 1, phase(t), pace)).unwrap()
                    )
                })
                .collect();
            std::fs::write(dir.join("on-mood.jsonl"), states.join("\n")).unwrap();
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mood-minute-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_tap_line_is_its_header_then_its_payload() {
        let (header, body) =
            tap_line(r#"{"topic":"mood/state","at_us":5} {"session_id":"s"}"#).unwrap();
        assert_eq!(header["at_us"], 5);
        assert_eq!(body["session_id"], "s");
        assert!(tap_line("lamp-bus: began again").is_none());
        assert!(tap_line(r#"{"topic":"x"}"#).is_none());
    }

    #[test]
    fn a_head_that_follows_moods_quicker_breath_is_in_step() {
        let dir = folder("follows");
        let pace = 1.2;
        let plain = |t: u64| plain_breath_phase(t);
        let follows = move |t: u64| (0.3 + (t - START) as f64 / MOOD_BREATH_US as f64 * 1.2) % 1.0;
        minute(&dir, "off", 1.0, plain, false);
        minute(&dir, "on", pace, follows, true);
        let said = report(&dir);
        assert!(
            said[0]
                .starts_with("off head_pitch samples=3000 period_s=5.20 fit_to_plain_breath=1.00"),
            "{said:?}"
        );
        assert_eq!(
            said[1],
            "on mood/state messages=60 sessions=1 sequences_rising=true pace_min=1.20 pace_max=1.20 pace_bounded=true"
        );
        assert_eq!(
            said[2],
            "on head_pitch samples=3000 period_s=4.34 fit_to_mood_breath=1.00 in_step=true"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_head_that_keeps_the_plain_breath_while_moods_is_quicker_is_not_in_step() {
        let dir = folder("plain");
        minute(&dir, "off", 1.0, plain_breath_phase, false);
        minute(&dir, "on", 1.2, plain_breath_phase, true);
        let said = report(&dir);
        assert!(said[2].ends_with("in_step=false"), "{said:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_marks_say_the_clap_and_the_head_after_mood_stopped() {
        let dir = folder("marks");
        minute(&dir, "off", 1.0, plain_breath_phase, false);
        minute(&dir, "on", 1.2, plain_breath_phase, true);
        std::fs::write(dir.join("on-clap-at"), "20\n").unwrap();
        std::fs::write(dir.join("on-stopped-at"), "40\n").unwrap();
        let said = report(&dir);
        assert!(
            said.contains(&"clap at 20 s: pace_max_after=1.20 quickened=true".to_owned()),
            "{said:?}"
        );
        let after = said
            .iter()
            .find(|line| line.starts_with("stopped at 40 s"))
            .unwrap();
        // 57 s of samples, judged from 43 s: 14 s at 50 a second.
        assert_eq!(
            after,
            "stopped at 40 s: head_pitch after samples=850 fit_to_plain_breath=1.00"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_state_too_old_gives_no_phase_and_too_few_samples_no_fit() {
        let states = vec![mood(START, 1, 0.0, 1.0)];
        assert!(mood_phase(&states, START + 1_000_000).is_some());
        assert!(mood_phase(&states, START + 3_000_000).is_none());
        assert!(mood_phase(&states, START - 1).is_none());
        assert_eq!(fit(&[(START, 0.1); 5], |_| Some(0.0)), None);
        assert_eq!(period_s(&[]), None);
        assert!(run(Path::new("/nonexistent/mood-minute")).is_err());
    }
}
