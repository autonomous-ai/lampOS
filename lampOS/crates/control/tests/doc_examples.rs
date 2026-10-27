//! docs/control.md is the contract the SDKs are built to: every request it
//! shows must be one this crate takes, so the page and the code can't drift
//! apart (the reviewer-lead's P2 on #3714). A request is a line of a ```json
//! block that starts with `{"id":`; placeholders in angle brackets stand for
//! the base64 the real request carries.
use lamp_control::protocol::parse;

const PAGE: &str = include_str!("../../../docs/control.md");

/// A placeholder's stand-in: valid base64 of the length the field needs.
fn filled(line: &str) -> String {
    line.replace("<32 bytes, base64>", &format!("{}=", "A".repeat(43)))
        .replace("<64 bytes, base64>", &format!("{}==", "A".repeat(86)))
}

fn requests() -> Vec<String> {
    let mut inside = false;
    let mut found = Vec::new();
    for line in PAGE.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            inside = line == "```json";
            continue;
        }
        if inside && line.starts_with("{\"id\":") {
            found.push(filled(line));
        }
    }
    found
}

#[test]
fn every_request_the_page_shows_parses() {
    let requests = requests();
    // The page shows pair, hello, subscribe, get, unsubscribe, the three
    // lease ops and every action: if a block's fence changes, this says so.
    assert!(requests.len() >= 16, "{} requests found", requests.len());
    for request in &requests {
        if let Err(refused) = parse(request) {
            panic!("{request}: {}", refused.error.why);
        }
    }
}

#[test]
fn the_pages_ops_are_every_op() {
    let shown: std::collections::BTreeSet<String> = requests()
        .iter()
        .map(|request| {
            serde_json::from_str::<serde_json::Value>(request).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    for op in [
        "pair",
        "hello",
        "subscribe",
        "unsubscribe",
        "get",
        "lease.take",
        "lease.renew",
        "lease.release",
        "express",
        "gesture",
        "look",
        "move",
        "joints",
        "light",
        "say",
        "stop",
    ] {
        assert!(shown.contains(op), "docs/control.md shows no {op} request");
    }
}
