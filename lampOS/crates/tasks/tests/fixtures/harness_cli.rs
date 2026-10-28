//! Scripted executable for adapter tests only. This never contacts Harness,
//! opens an account, or starts an agent. Its plan and logs live under test HOME.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn private_write(path: PathBuf, bytes: &[u8]) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

pub fn main() -> io::Result<()> {
    let home =
        PathBuf::from(env::var_os("HOME").ok_or_else(|| io::Error::other("fixture HOME missing"))?);
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "__hold") {
        // The kernel drops this lock only when this process exits, after any
        // system call it was in. A heartbeat file cannot prove that: kill()
        // returns before SIGKILL takes effect, and an in-flight write lands.
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(home.join("descendant.lock"))?;
        lock.lock()?;
        private_write(
            home.join("descendant.pid"),
            std::process::id().to_string().as_bytes(),
        )?;
        thread::sleep(Duration::from_secs(5));
        private_write(home.join("descendant.finished"), b"")?;
        return Ok(());
    }
    let plan: Value = serde_json::from_slice(&fs::read(home.join("plan.json"))?)?;
    // `machines engines` (openharness#1888) is planned as `engines`, apart
    // from `machines`.
    let operation_of = |first: Option<&str>, second: Option<&str>| match (first, second) {
        (Some("machines"), Some("engines")) => "engines".to_owned(),
        (first, _) => first.unwrap_or_default().to_owned(),
    };
    let operation = operation_of(
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    );
    let operation = operation.as_str();
    // `{"each": [step, …]}`: the operation's first call takes the first
    // step, its next the next, and the last step stands after them.
    let before = fs::read_to_string(home.join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|call| {
            operation_of(call["args"][0].as_str(), call["args"][1].as_str()) == operation
        })
        .count();
    let step = match plan[operation]["each"].as_array() {
        Some(steps) if !steps.is_empty() => &steps[before.min(steps.len() - 1)],
        _ => &plan[operation],
    };
    let environment = env::vars().collect::<std::collections::BTreeMap<_, _>>();
    let mut calls = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(home.join("calls.jsonl"))?;
    writeln!(
        calls,
        "{}",
        json!({"args":args,"environment":environment,"pid":std::process::id()})
    )?;
    calls.flush()?;
    if step["stdin_eof"] == true {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        if !input.is_empty() {
            return Err(io::Error::other("fixture stdin was not null"));
        }
    }
    if step["remove_executable"] == true {
        let path = env::current_exe()?.canonicalize()?;
        if !path.starts_with(home.canonicalize()?) {
            return Err(io::Error::other(
                "fixture refuses to remove a shared binary",
            ));
        }
        fs::remove_file(path)?;
    }
    if step["held_descendant"] == true {
        let output = || {
            if step["redirect_descendant"] == true {
                Stdio::null()
            } else {
                Stdio::inherit()
            }
        };
        let _descendant = Command::new(env::current_exe()?)
            .arg("__hold")
            .stdin(Stdio::null())
            .stdout(output())
            .stderr(output())
            .spawn()?;
        // Exit only once the descendant holds its lock. The caller's own
        // deadline bounds this wait if the descendant never gets there.
        while !home.join("descendant.pid").exists() {
            thread::sleep(Duration::from_millis(1));
        }
    }
    if let Some(text) = step["stderr"].as_str() {
        io::stderr().write_all(text.as_bytes())?;
    }
    // Harness 0.4.23's `login --code --json` (openharness#1410): the code,
    // kept alive once, the question, the answer read from stdin and kept
    // under HOME, and the result for it.
    if step["code_flow"] == true {
        let code = json!({"type":"code","code":"12345678","expiresIn":120});
        println!("{code}");
        println!("{code}");
        // The code stays the only thing said until this file exists, or for
        // two minutes, so a played check can ask for it again first (#2839).
        if let Some(release) = step["code_hold_until"].as_str() {
            io::stdout().flush()?;
            let until = std::time::Instant::now() + Duration::from_secs(120);
            while !std::path::Path::new(release).exists() && std::time::Instant::now() < until {
                thread::sleep(Duration::from_millis(20));
            }
        }
        println!(
            "{}",
            json!({"type":"confirm","email":"someone@example.com"})
        );
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        private_write(home.join("answer"), answer.as_bytes())?;
        println!(
            "{}",
            if answer == "yes\n" {
                json!({"type":"result","status":"success","email":"someone@example.com"})
            } else {
                json!({"type":"result","status":"error","code":"DENIED"})
            }
        );
    }
    if let Some(ms) = step["delay_ms"].as_u64() {
        thread::sleep(Duration::from_millis(ms.min(5000)));
    }
    if let Some(count) = step["stderr_bytes"].as_u64() {
        io::stderr().write_all(&vec![b's'; count.min(128 * 1024) as usize])?;
    }
    if let Some(count) = step["stdout_bytes"].as_u64() {
        io::stdout().write_all(&vec![b'x'; count.min(128 * 1024) as usize])?;
    } else if let Some(text) = step["stdout"].as_str() {
        io::stdout().write_all(text.as_bytes())?;
    } else if step["default"] == true {
        let flag = |name: &str| {
            args.iter()
                .position(|arg| arg == name)
                .and_then(|at| args.get(at + 1))
                .cloned()
                .unwrap_or_default()
        };
        match operation {
            "machines" => println!(
                "{}",
                json!({"machineId":"0123456789abcdef0123456789abcdef","current":true})
            ),
            // As Harness answers: the mode asked for, and with none asked
            // (Devin, #1854) none, and no bypass (`newAgentPayload`).
            "new" => {
                let moded = args.iter().any(|arg| arg == "--mode");
                println!(
                    "{}",
                    json!({"ok":true,"machineId":flag("--machine"),"agent":{
                        "id":"11111111-1111-4111-8111-111111111111","name":flag("--name"),"engine":args.get(1),
                        "permissionMode":if moded { json!(flag("--mode")) } else { Value::Null },
                        "bypassPermission":moded
                    }})
                )
            }
            "show" => println!(
                "{}",
                json!({"id":args.get(3),"state":"idle","ask":"Earlier unrelated question","answer":"Observed answer","truncated":false,"tools":2})
            ),
            _ => return Err(io::Error::other("fixture operation unknown")),
        }
    }
    io::stdout().flush()?;
    io::stderr().flush()?;
    if let Some(ms) = step["after_output_ms"].as_u64() {
        thread::sleep(Duration::from_millis(ms.min(5000)));
    }
    std::process::exit(step["exit"].as_i64().unwrap_or(0).clamp(0, 255) as i32);
}
