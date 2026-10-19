//! `lamp-voice-eval cost` as it is run: a journal and the shipped prices
//! file, a row an arm and price tier, and no run without prices.
use std::process::Command;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn journal(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("lamp-cost-{name}-{}.log", std::process::id()));
    std::fs::write(
        &path,
        "\
2026-10-19T20:00:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 heard 6 words, the last 210 ms after the line ended
2026-10-19T20:00:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: settled=true, speech_final=true, finals cover 1800 of 2400 ms sent, pending=none
2026-10-19T20:00:06+0700 lamp-4ace lamp-voice[100]: lamp-voice second: chat answer: its first words or call 812 ms and its end 1430 ms after it was asked; 8000 prompt tokens, 6000 from the cache, 2000 not; 100 completion tokens
2026-10-19T20:00:07+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 first answer audio 900 ms after the words were heard
2026-10-19T20:00:08+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 ElevenLabs: 2 of 2 pieces began, 1 on a reused connection, 400 chars; the first piece's first audio byte 300 ms after it went, the slowest 350 ms
2026-10-19T21:00:01+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 1 began
2026-10-19T21:00:03+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: usage total=Some(1500) prompt=Some(1000) prompt_audio=Some(400) response=Some(500) response_audio=Some(300) thoughts=None cached=None
",
    )
    .unwrap();
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-voice-eval"))
        .arg("cost")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn the_shipped_prices_cost_each_arm_a_row_a_price_tier() {
    let path = journal("rows");
    let prices = format!("{FIXTURES}/voice-eval-cost-prices.example.json");
    let output = run(&[
        path.to_str().unwrap(),
        "--prices",
        &prices,
        "--arm",
        "deepgram",
        "20:00",
        "20:15",
        "--arm",
        "live",
        "21:00",
        "21:15",
    ]);
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let table = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<&str> = table
        .lines()
        .skip(1)
        .map(|line| &line[..line.find(" | $").unwrap()])
        .collect();
    assert_eq!(
        rows,
        [
            "deepgram | off-peak | 1 | 1 | 15",
            "deepgram | peak | 1 | 1 | 15",
            "live | list | 1 | 0 | 15"
        ],
        "{table}"
    );
    // Counts and costs only: no words.
    assert!(!table.contains("heard 6 words"), "{table}");
}

#[test]
fn without_prices_or_with_a_voice_it_has_no_price_for_it_does_not_run() {
    let path = journal("refused");
    let journal = path.to_str().unwrap();
    let output = run(&[journal, "--arm", "deepgram", "20:00", "20:15"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("needs --prices"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let unpriced =
        std::env::temp_dir().join(format!("lamp-cost-unpriced-{}.json", std::process::id()));
    let mut prices: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{FIXTURES}/voice-eval-cost-prices.example.json"))
            .unwrap(),
    )
    .unwrap();
    prices["voice_per_character"] = serde_json::json!({});
    std::fs::write(&unpriced, prices.to_string()).unwrap();
    let output = run(&[
        journal,
        "--prices",
        unpriced.to_str().unwrap(),
        "--arm",
        "deepgram",
        "20:00",
        "20:15",
    ]);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&unpriced);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no price for ElevenLabs"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
