//! `lamp-policy-replay` itself (#871): a decisions log asked again of two
//! models through a played service on this machine. Each model is asked the
//! text that was sent on the day, and nothing printed holds a word heard.
use lamp_policy::attention::{self, Context, Room};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::mpsc,
    thread,
    time::Duration,
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-policy-replay");

/// One request read whole from `stream`: its body.
fn body(stream: &mut TcpStream) -> Option<String> {
    let mut held = Vec::new();
    let mut piece = [0u8; 4096];
    let start = loop {
        if let Some(at) = held.windows(4).position(|four| four == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    };
    let head = String::from_utf8(held[..start].to_vec())
        .ok()?
        .to_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:")?.trim().parse().ok())?;
    while held.len() < start + length {
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    }
    String::from_utf8(held[start..].to_vec()).ok()
}

/// The service, played: `old` answers every question 0.05, `new` 0.95, each
/// as a dated model; every request is told to the test.
fn service() -> (u16, mpsc::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, requests) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let told = told.clone();
            // One thread a connection: the program keeps one for each model.
            thread::spawn(move || {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                // A kept connection carries one request after another.
                while let Some(body) = body(&mut stream) {
                    let asked: Value = serde_json::from_str(&body).unwrap();
                    let model = asked["model"].as_str().unwrap().to_owned();
                    let yes = if model == "old" { 0.05 } else { 0.95 };
                    let answers: serde_json::Map<String, Value> = asked["questions"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(|name| (name.clone(), json!({"type": "noul", "noul": yes})))
                        .collect();
                    let answer = json!({"model": format!("{model}-20261014"), "answers": answers})
                        .to_string();
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                            answer.len()
                        )
                        .as_bytes(),
                    );
                    if told.send(asked).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, requests)
}

#[test]
fn the_log_is_asked_again_of_each_model_and_nothing_heard_is_printed() {
    let dir = std::env::temp_dir().join(format!("policy-replay-program-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("logs")).unwrap();
    fs::write(dir.join("key"), "not-a-real-key\n").unwrap();
    let named = Context {
        heard: "Hey Lamp, what is the weather like in Hanoi today?".into(),
        room: Room::Busy,
        lamp_is_speaking: false,
        lamp_last_said: Some("Canberra.".into()),
        seconds_since_lamp_spoke: Some(12.0),
        name_heard: Some(0.97),
        like_asker: None,
    };
    let talk = Context {
        heard: "Voglio mangiare sushi.".into(),
        room: Room::Quiet,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    };
    let line = |at: u64, context: &Context, choice: &str| {
        json!({"at_unix_ms": at, "about": "attention", "shadow": true, "context": context,
               "judgement": {"probability": 0.3, "choice": choice, "from_rule": false},
               "by": "typesafe/jev-1.13-20260917"})
        .to_string()
    };
    fs::write(
        dir.join("logs/decisions-2026-10-15.jsonl"),
        [
            line(1_760_541_000_000, &named, "stay_quiet"),
            line(1_760_541_060_000, &talk, "stay_quiet"),
        ]
        .join("\n"),
    )
    .unwrap();
    fs::write(
        dir.join("labels.jsonl"),
        "{\"at_unix_ms\": 1760541000000, \"should\": \"answer\"}\n{\"at_unix_ms\": 1760541060000, \"should\": \"stay_quiet\"}\n",
    )
    .unwrap();
    let (port, requests) = service();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let output = Command::new(PROGRAM)
        .args([
            "--key-file",
            &path("key"),
            "--log-dir",
            &path("logs"),
            "--days",
            "2026-10-14,2026-10-15",
            "--models",
            "old,new",
            "--labels",
            &path("labels.jsonl"),
            "--endpoint",
            &format!("http://127.0.0.1:{port}/api/alpha/decisions"),
            "--pace-ms",
            "0",
        ])
        .output()
        .unwrap();
    fs::remove_dir_all(&dir).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    for word in [
        "Hanoi",
        "weather",
        "Voglio",
        "sushi",
        "Canberra",
        "not-a-real-key",
    ] {
        assert!(!stdout.contains(word) && !stderr.contains(word), "{word}");
    }

    // Each model was asked the text sent on the day, in turn.
    let asked: Vec<Value> = requests.try_iter().collect();
    let models: Vec<&str> = asked.iter().map(|a| a["model"].as_str().unwrap()).collect();
    assert_eq!(models, ["old", "new", "old", "new"]);
    let sent: Value = serde_json::from_str(&attention::state_text(&named)).unwrap();
    assert_eq!(asked[0]["state"], sent);
    assert_eq!(asked[1]["state"], sent);

    let lines: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["id"], "2026-10-15#1");
    assert_eq!(
        (
            &lines[0]["names_lamp"],
            &lines[0]["name_heard"],
            &lines[0]["room"]
        ),
        (&json!(true), &json!(true), &json!("busy"))
    );
    assert_eq!(lines[1]["other_language"], true);
    assert_eq!(lines[0]["judges"][1]["by"], "new-20261014");
    // Each choice is the one the same weights make of that model's answers.
    let choice_of = |context: &Context, yes: f32| {
        let answers = attention::Answers::of(
            &attention::questions()
                .into_iter()
                .map(|(name, _)| (name, lamp_policy::decide::Answer::Noul(yes)))
                .collect(),
        )
        .unwrap();
        attention::combine(&answers, context).choice.as_str()
    };
    for (index, context) in [&named, &talk].into_iter().enumerate() {
        assert_eq!(
            lines[index]["judges"][0]["choice"],
            choice_of(context, 0.05)
        );
        assert_eq!(
            lines[index]["judges"][1]["choice"],
            choice_of(context, 0.95)
        );
    }
    let summary = &lines[2];
    assert_eq!(summary["status"], "replay_summary");
    for model in ["old", "new"] {
        assert_eq!(summary["models"][model]["answered"], 2);
        assert_eq!(summary["models"][model]["right"]["labelled"], 2);
        assert_eq!(summary["models"][model]["right_named"]["labelled"], 1);
        assert_eq!(
            summary["models"][model]["right_other_language"]["labelled"],
            1
        );
        assert_eq!(summary["models"][model]["right_busy"]["labelled"], 1);
    }
    assert_eq!(summary["pairs"][0]["both_answered"], 2);
}

/// An earlier run counted again with `--refit`: the summary, then the
/// refit's line for each model, fitted where it has enough labelled lines
/// with answers and said why not where it has none.
#[test]
fn an_earlier_run_is_fitted_again_to_each_models_answers() {
    use lamp_policy::replay::{Logged, Replayed, line};
    let dir = std::env::temp_dir().join(format!("policy-refit-program-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let names: Vec<String> = attention::questions()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let (mut lines, mut labels) = (String::new(), String::new());
    for index in 0..40_u64 {
        // Every other line calls the lamp and is to be answered.
        let call = index % 2 == 0;
        let at = 1_760_541_000_000 + index * 1_000;
        let context = Context {
            heard: "Hey Lamp, what is the time?".into(),
            room: Room::Busy,
            lamp_is_speaking: false,
            lamp_last_said: None,
            seconds_since_lamp_spoke: None,
            name_heard: None,
            like_asker: None,
        };
        let yes = if call { 0.9 } else { 0.1 };
        let new = Replayed::Chose {
            by: "new-20261014".into(),
            choice: if call { "answer" } else { "stay_quiet" }.into(),
            probability: yes,
            took_ms: 300,
            answers: names.iter().map(|name| (name.clone(), yes)).collect(),
        };
        let logged = Logged {
            id: format!("2026-10-15#{}", index + 1),
            at_unix_ms: at,
            context,
            choice: "answer".into(),
            probability: 0.9,
            by: "typesafe/jev-1.13-20260917".into(),
            follows_lamp: None,
        };
        // `old` is a run from before the answers were kept.
        let old = Replayed::Chose {
            by: "old-20260917".into(),
            choice: "answer".into(),
            probability: 0.9,
            took_ms: 300,
            answers: Default::default(),
        };
        let mut printed = line(&logged, None, &[("old".into(), old), ("new".into(), new)]);
        printed["judges"][0]
            .as_object_mut()
            .unwrap()
            .remove("answers");
        lines.push_str(&format!("{printed}\n"));
        let should = if call { "answer" } else { "stay_quiet" };
        labels.push_str(&format!(
            "{}\n",
            json!({"at_unix_ms": at, "should": should})
        ));
    }
    fs::write(dir.join("replay.jsonl"), lines).unwrap();
    fs::write(dir.join("labels.jsonl"), labels).unwrap();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let output = Command::new(PROGRAM)
        .args([
            "--again",
            &path("replay.jsonl"),
            "--labels",
            &path("labels.jsonl"),
            "--refit",
        ])
        .output()
        .unwrap();
    fs::remove_dir_all(&dir).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stdout.contains("Hey Lamp"));
    let printed: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(printed.len(), 2);
    assert_eq!(printed[0]["status"], "replay_summary");
    let refit = &printed[1];
    assert_eq!(refit["status"], "refit_summary");
    assert_eq!(refit["models"]["old"]["fitted"], false);
    assert_eq!(refit["models"]["old"]["without_answers"], 40);
    let new = &refit["models"]["new"];
    assert_eq!(new["fitted"], true);
    assert_eq!(
        new["refitted"]["right"],
        json!({"labelled": 20.0, "right": 20.0})
    );
    assert_eq!(
        new["day_weights"]["right"],
        json!({"labelled": 20.0, "right": 20.0})
    );
    assert!(
        new["weights_on_every_label"]["calls_lamp"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}
