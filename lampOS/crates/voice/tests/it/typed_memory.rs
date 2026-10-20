//! A text from the Autonomous app keeps and reads the room's memory, never
//! that of the person the eyes see at the lamp (#1673, the reviewer on
//! #1713). Its own test binary, since the eyes' known file is the process's.
use lamp_gemini::ToolCall;
use lamp_voice::tools::{Settings, answer, answer_typed};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

#[test]
fn a_text_keeps_and_reads_the_rooms_memory_not_the_person_at_the_lamp() {
    let dir = std::env::temp_dir().join(format!("typed-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("memory.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let asked: Arc<Mutex<Vec<Value>>> = Arc::default();
    {
        let asked = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap_or_default();
                let reply = match request["op"].as_str() {
                    Some("kept") => json!({"ok": true, "entries": []}),
                    Some("recall") => json!({"ok": true, "entries": []}),
                    _ => json!({"ok": true}),
                };
                asked.lock().unwrap().push(request);
                let _ = writeln!(stream, "{reply}");
            }
        });
    }
    // Sam is at the lamp, as the eyes say.
    let known = dir.join("known");
    std::fs::write(&known, "Sam").unwrap();
    // The memory as it is without the Days switch, whose own client this
    // stand-in doesn't speak.
    assert_ne!(
        std::env::var(lamp_voice::remember::DAYS_ENVIRONMENT).as_deref(),
        Ok("on"),
        "run without the Days memory switch"
    );
    lamp_voice::remember::start(socket.clone(), Some(known.clone()));
    let settings = Settings {
        memory: Some(socket.clone()),
        ..Settings::default()
    };
    let keep = ToolCall {
        id: String::new(),
        name: lamp_voice::remember::REMEMBER.into(),
        args: json!({"text": "the spare key is under the blue mat"}),
    };
    let kept = |asked: &[Value]| -> Vec<Value> {
        asked
            .iter()
            .filter(|request| request["op"] == "remember")
            .cloned()
            .collect()
    };

    // Said at the lamp: kept as Sam's.
    answer(&settings, &keep, 7, true);
    let spoken = kept(&asked.lock().unwrap());
    assert_eq!(spoken.len(), 1, "{spoken:?}");
    assert!(spoken[0]["person"].is_string(), "{spoken:?}");

    // Typed in the app while Sam is still in view: the room's.
    asked.lock().unwrap().clear();
    answer_typed(&settings, &keep, (1 << 48) + 1);
    let typed = kept(&asked.lock().unwrap());
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert!(typed[0].get("person").is_none(), "{typed:?}");

    // A typed recall asks the room's memory alone, never Sam's.
    asked.lock().unwrap().clear();
    let recall = ToolCall {
        id: String::new(),
        name: lamp_voice::remember::RECALL.into(),
        args: json!({"about": "the spare key"}),
    };
    answer_typed(&settings, &recall, (1 << 48) + 2);
    let recalls: Vec<Value> = asked
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request["op"] == "recall")
        .cloned()
        .collect();
    assert_eq!(recalls.len(), 1, "{recalls:?}");
    assert!(recalls[0].get("person").is_none(), "{recalls:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
