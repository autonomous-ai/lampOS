//! The voice's own `reports_stop` and the skills' `dance` on the second
//! route (`LAMP_VOICE_ROUTE=second`, the first builder at 19:48 on #1722:
//! a voice feature is done when it works here): each declared from the
//! flags the coordinator passes this worker, as `serve` declares them to
//! the chat, and run by the route's own runner ([`tool_runner`]).
use super::*;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The tools this worker is given by `flags`, as `serve` takes them.
fn tools_of(flags: &[String]) -> crate::tools::Settings {
    let (_, _, files, _) = parse_flags(Path::new("/run/lamp-voice"), flags).unwrap();
    files.tools
}

fn names(tools: &[lamp_gemini::Tool]) -> Vec<&str> {
    tools.iter().map(|tool| tool.name.as_str()).collect()
}

/// A short folder: a socket's whole path has to fit in 103 bytes.
fn folder(name: &str) -> PathBuf {
    let base = if Path::new("/private/tmp").is_dir() {
        PathBuf::from("/private/tmp")
    } else {
        std::env::temp_dir()
    };
    let path = base.join(format!(
        "lrc-{name}-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The skills' socket in `folder`: answers one call, and gives back what it
/// was asked.
fn skills_socket(folder: &Path) -> std::thread::JoinHandle<serde_json::Value> {
    let listener = UnixListener::bind(folder.join(crate::tools::CALL_SOCKET)).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let _ = stream.write_all(b"{\"ok\":true,\"result\":{\"next\":\"Danced.\"}}\n");
        serde_json::from_str(&line).unwrap()
    })
}

/// `LAMP_REPORTS=on` (#1683): `reports_stop` is declared to the second
/// route's chat, and only then; with `LAMP_SECOND_FEW_TOOLS=on` it waits
/// behind `more_functions`, which names it. Its call is run on the reports,
/// never told "not running" as a call nobody here runs is: on a machine
/// with no `lamp-cloud` on the bus, the reports' own words say so.
#[tokio::test]
async fn reports_stop_is_declared_with_its_switch_and_run_on_the_reports() {
    let plain = tools_of(&strings(&["--tools"]));
    assert!(!names(&crate::tools::declared(&plain)).contains(&crate::reports_stop::NAME));
    let mut flags = strings(&["--tools"]);
    flags.extend(crate::reports_stop::arguments(Some("on")));
    let tools = tools_of(&flags);
    assert!(tools.reports_stop);
    let declared = with_search(crate::tools::declared(&tools), &[]);
    assert!(names(&declared).contains(&crate::reports_stop::NAME));
    let few = few_tools(declared, &[]);
    assert!(!names(&few).contains(&crate::reports_stop::NAME));
    let more = few.iter().find(|tool| tool.name == MORE).unwrap();
    assert!(more.description.contains(crate::reports_stop::NAME));

    let runner = tool_runner(tools, None, None);
    let told = runner(7, true, crate::reports_stop::NAME.into(), json!({})).await;
    assert_ne!(told, crate::tools::unavailable());
    let words = told.to_string();
    assert!(words.contains("report"), "{words}");
    // Unheard, it stops nothing: only a person can.
    let runner = tool_runner(tools_of(&flags), None, None);
    assert_eq!(
        runner(8, false, crate::reports_stop::NAME.into(), json!({})).await,
        json!({"error": "Only a person can stop the reports."})
    );
}

/// The skills' `dance` (#1012, `skills/dance`): declared to the second
/// route's chat from the skills' `tools.json`, by its own name or, with
/// `LAMP_USE_SKILL=on`, through `use_skill`; either call reaches the
/// skills' socket as `dance`.
#[tokio::test]
async fn dance_is_declared_from_the_skills_tools_file_and_reaches_their_socket() {
    let skills = folder("dance");
    std::fs::write(
        skills.join(crate::tools::CATALOG_FILE),
        r#"{"tools":[{"name":"dance","description":"When someone asks the lamp to dance.",
            "parameters":{"type":"object","properties":{"style":{"type":"string"}}}}],
            "body":["dance"]}"#,
    )
    .unwrap();
    let skills_dir = skills.display().to_string();
    let flags = strings(&["--tools", crate::tools::SKILLS_FLAG, &skills_dir]);
    let tools = tools_of(&flags);
    let declared = with_search(crate::tools::declared(&tools), &[]);
    assert!(
        names(&declared).contains(&"dance"),
        "{:?}",
        names(&declared)
    );

    let served = skills_socket(&skills);
    let runner = tool_runner(tools, None, None);
    assert_eq!(
        runner(7, true, "dance".into(), json!({"style": "jazz"})).await,
        json!({"next": "Danced."})
    );
    assert_eq!(
        served.join().unwrap(),
        json!({"op":"call","name":"dance","args":{"style":"jazz"}})
    );

    // Through `use_skill`: the one function names it, and its call dances.
    let mut flags = flags;
    flags.extend(crate::use_skill::arguments(Some("on")));
    let tools = tools_of(&flags);
    let declared = with_search(crate::tools::declared(&tools), &[]);
    assert!(!names(&declared).contains(&"dance"));
    let use_skill = declared
        .iter()
        .find(|tool| tool.name == crate::use_skill::NAME)
        .unwrap();
    assert!(
        serde_json::to_string(use_skill).unwrap().contains("dance"),
        "{use_skill:?}"
    );
    std::fs::remove_file(skills.join(crate::tools::CALL_SOCKET)).unwrap();
    let served = skills_socket(&skills);
    let runner = tool_runner(tools, None, None);
    assert_eq!(
        runner(
            9,
            true,
            crate::use_skill::NAME.into(),
            json!({"skill": "dance"})
        )
        .await,
        json!({"next": "Danced."})
    );
    assert_eq!(
        served.join().unwrap(),
        json!({"op":"call","name":"dance","args":{}})
    );
    std::fs::remove_dir_all(skills).unwrap();
}
