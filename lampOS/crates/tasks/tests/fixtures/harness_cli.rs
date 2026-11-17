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
    // `{"by_arg": {"ARG": step, …}}` beside a step: a call with ARG among
    // its arguments takes that step, so calls made together (the stopped
    // rows' shows, #5978) are answered by what they ask, not by their turn.
    let step = match plan[operation]["by_arg"].as_object() {
        Some(steps) => args.iter().find_map(|arg| steps.get(arg)).unwrap_or(step),
        None => step,
    };
    // `{"with_new": step}` beside a step: a call that asks for `--new NAME`
    // takes it instead, so a plain folder and a made one can be answered
    // apart (#5541).
    let step = match &plan[operation]["with_new"] {
        with_new if with_new.is_object() && args.iter().any(|arg| arg == "--new") => with_new,
        _ => step,
    };
    let environment = env::vars().collect::<std::collections::BTreeMap<_, _>>();
    let mut calls = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(home.join("calls.jsonl"))?;
    // One write of the whole line: calls made together (the stopped rows'
    // shows, #5978) append from several processes at once, and `writeln!`
    // writes in pieces that interleave between them.
    let line = format!(
        "{}\n",
        json!({"args":args,"environment":environment,"pid":std::process::id()})
    );
    calls.write_all(line.as_bytes())?;
    calls.flush()?;
    // `{"program": true}`: the openharness#4393 program-session contract
    // v1, kept across calls in `program-sessions.json` under HOME
    // ([`program`]). Every other step is answered as before.
    if step["program"] == true {
        let (said, exit) = program::run(&home, operation, &args, step)?;
        println!("{said}");
        io::stdout().flush()?;
        std::process::exit(exit);
    }
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
        // `login --qr --json` (#5198): the QR's link in place of the code.
        let code = if args.iter().any(|arg| arg == "--qr") {
            json!({"type":"qr","url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expiresIn":120})
        } else {
            json!({"type":"code","code":"12345678","expiresIn":120})
        };
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
    // `{"write_answer": "TEXT"}`: as a voice make's agent would (#7245),
    // TEXT written to `answer.md` in the folder the call names (its first
    // argument that is an absolute folder).
    if let Some(text) = step["write_answer"].as_str()
        && let Some(folder) = args
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_absolute() && path.is_dir())
    {
        private_write(folder.join("answer.md"), text.as_bytes())?;
    }
    // `{"live_in_new_folder": "ID"}` (`ps`): agent ID live in the folder
    // the last `new` named (its first absolute folder argument), as an
    // uncertain `new` Harness finished late would be (#7245).
    if let Some(id) = step["live_in_new_folder"].as_str() {
        let folder = fs::read_to_string(home.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .rev()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|call| call["args"][0] == "new")
            .and_then(|call| {
                call["args"].as_array().and_then(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .find(|arg| arg.starts_with('/'))
                        .map(str::to_owned)
                })
            })
            .unwrap_or_default();
        println!(
            "{}",
            json!({"harnesses":[{"id":id,"name":"make","folder":folder,"state":"working"}]})
        );
    }
    if let Some(ms) = step["delay_ms"].as_u64() {
        thread::sleep(Duration::from_millis(ms.min(5000)));
    }
    // `{"lines": [{"text": "...", "delay_ms": N}, …]}`: each line written
    // and flushed on its own after its delay, as `harness watch --json`
    // prints one event at a time.
    if let Some(lines) = step["lines"].as_array() {
        for line in lines {
            if let Some(ms) = line["delay_ms"].as_u64() {
                thread::sleep(Duration::from_millis(ms.min(5000)));
            }
            let mut out = io::stdout();
            writeln!(out, "{}", line["text"].as_str().unwrap_or_default())?;
            out.flush()?;
        }
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
            // As openharness#2157's `reply --json` answers once the words
            // are queued in the harness's mailbox.
            "reply" => println!(
                "{}",
                json!({"ok":true,"name":"lamp-task-001","id":args.get(3),"message":"m1","state":"queued"})
            ),
            // As harness route's design (29 Oct) answers new work.
            "route" => println!("{}", json!({"decided":"new","reason":"fixture"})),
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

/// openharness#4393 contract v1, a program-owned harness: `capabilities`,
/// `ensure`, an acknowledged `reply`, `wait --after` and `close`, as the
/// Harness builder answered it (exits: 0 ok, 1 failed, 2 usage, 3 needs
/// the person, 4 stopped, 5 refused, 124 timeout; a refusal says exactly
/// `{"error":CODE,"detail":TEXT}`, with `cancelled` and `inputId` beside
/// them on a reply's `TIMEOUT`). `close --forget` is not built upstream:
/// `--forget` is passed over and `close` says what it always says.
///
/// Sessions are kept by name in `program-sessions.json` under HOME, read
/// and written under `program-sessions.lock`, so calls made together see
/// one another; one closed by `ensure` (`--fresh`, or an engine that
/// cannot continue) moves to `closed`, so `wait --after` still finds the
/// input it took. A token is checked as the daemon checks it: a regular
/// file, not a link, opened once and its open file's metadata read, owned
/// by the caller, no group or other permission bits (so 0600 and 0400
/// pass), and 32 bytes or more once surrounding whitespace is taken off;
/// only its SHA-256 is kept. A taken input is `program:<uuid v4>` with
/// `takenAt` in ISO-8601 UTC, as Harness p4393.4 says on a lamp.
///
/// What a test drives, from the step: `ensure` with `"starting": true`
/// leaves the engine starting, and `reply` refuses `ENGINE_NOT_READY`
/// unless its step says `"ready": true`; `ensure` with `"not_ready": true`
/// refuses `ENGINE_NOT_READY` itself, as while a crashed engine is still
/// exiting, and changes nothing; with `"crashed": true` the engine of a
/// live session crashed and the same id is resumed, never a new one; with
/// `"engine_session": TEXT` a hermes engine has reported its id (until then
/// hermes says `null`; openclaw always says `agent:main:harness-<id>`);
/// with `"why": TEXT`
/// the engine cannot continue a closed session and a new one starts; with
/// `"daemon_start_ms": N` the daemon was down and started in N ms. `reply`
/// with `"timeout": "cancelled"` types nothing; with `"timeout"` set to
/// `"typed"`, `"no_answer"`, `"enter_withheld"` or `"paste_failed"` it takes
/// the input and says `cancelled:false` with an `inputId` `wait --after`
/// follows (p4393.5). `close` with `"not_ready": true` refuses
/// `ENGINE_NOT_READY` and changes nothing (p4393.5). `wait` with
/// `"ends": "person" | "stopped" | "timeout"` ends so. `"exits": {"CODE": N}` sets a refusal's
/// exit. Without a token, `ensure`, `resume`, `reply` and `close` on a
/// program-owned harness are `NOT_YOURS`; `resume` is answered only to
/// refuse.
mod program {
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{self, Read, Write},
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
    };

    /// The most a token file may hold, as the daemon reads one.
    const MOST_TOKEN_BYTES: u64 = 4096;
    /// Options that take a value, so a value is not read as a word.
    const VALUED: [&str; 5] = [
        "--name",
        "--program-token-file",
        "--timeout",
        "--after",
        "--machine",
    ];

    /// A refusal's exit when the step sets none (openharness `EXIT`).
    fn exit_of(code: &str, step: &Value) -> i32 {
        if let Some(exit) = step["exits"][code].as_i64() {
            return exit.clamp(0, 255) as i32;
        }
        match code {
            "BAD_TOKEN_FILE" | "NAME_TAKEN" | "NOT_YOURS" | "NOT_IDLE" | "ENGINE_NOT_READY" => 5,
            "NOT_RUNNING" => 4,
            "TIMEOUT" => 124,
            "USAGE" => 2,
            // UNKNOWN_HARNESS, UNKNOWN_INPUT.
            _ => 1,
        }
    }

    /// A refusal: its code, its detail, and the fields said beside them.
    struct Refusal {
        code: &'static str,
        detail: String,
        beside: Option<Value>,
    }

    fn refusal(code: &'static str, detail: impl Into<String>) -> Refusal {
        Refusal {
            code,
            detail: detail.into(),
            beside: None,
        }
    }

    fn refused(refusal: Refusal, step: &Value) -> (Value, i32) {
        let mut said = json!({"error":refusal.code,"detail":refusal.detail});
        if let Some(Value::Object(beside)) = refusal.beside {
            for (key, value) in beside {
                said[key] = value;
            }
        }
        (said, exit_of(refusal.code, step))
    }

    /// The words before `--`; after it is the reply's text.
    fn options(args: &[String]) -> &[String] {
        &args[..args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len())]
    }

    fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
        let options = options(args);
        options
            .iter()
            .position(|arg| arg == name)
            .and_then(|at| options.get(at + 1))
            .map(String::as_str)
    }

    fn has(args: &[String], name: &str) -> bool {
        options(args).iter().any(|arg| arg == name)
    }

    /// The first word after the verb that is no option or option value:
    /// `ensure`'s engine, the harness `reply`, `wait` and `close` name.
    fn first_word(args: &[String]) -> Option<&str> {
        let mut rest = options(args).iter().skip(1);
        while let Some(arg) = rest.next() {
            if !arg.starts_with("--") {
                return Some(arg);
            }
            if VALUED.contains(&arg.as_str()) {
                rest.next();
            }
        }
        None
    }

    /// The token's SHA-256, or `BAD_TOKEN_FILE`. The file is opened once,
    /// a link refused by `O_NOFOLLOW` (and a FIFO not waited on), and the
    /// open file's own metadata checked, so nothing is checked by its path
    /// and then read by it again.
    fn token_hash(args: &[String]) -> Result<String, Refusal> {
        let path = flag(args, "--program-token-file")
            .ok_or_else(|| refusal("USAGE", "This needs --program-token-file <path>."))?;
        let bad = |why: &str| {
            refusal(
                "BAD_TOKEN_FILE",
                format!(
                    "{path}: {why}. A program token file is a regular file of yours, with no group or other permissions, holding 32 or more bytes."
                ),
            )
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path)
            .map_err(|_| bad("it cannot be read, or it is a link"))?;
        let meta = file.metadata().map_err(|_| bad("it cannot be read"))?;
        if !meta.is_file() {
            return Err(bad("it is not a regular file"));
        }
        if meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(bad("it is not owned by you"));
        }
        // As upstream: only group or other bits are refused, so 0400 and
        // 0600 both pass.
        if meta.mode() & 0o077 != 0 {
            return Err(bad(&format!(
                "its mode is {:o}, open to group or others",
                meta.mode() & 0o777
            )));
        }
        let mut bytes = Vec::new();
        file.take(MOST_TOKEN_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| bad("it cannot be read"))?;
        if bytes.len() as u64 > MOST_TOKEN_BYTES {
            return Err(bad("it is too large"));
        }
        let token = bytes.trim_ascii();
        if token.len() < 32 {
            return Err(bad(&format!("it holds {} bytes", token.len())));
        }
        Ok(Sha256::digest(token)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    fn not_yours(name: &str) -> Refusal {
        refusal(
            "NOT_YOURS",
            format!("\"{name}\" is not a harness this token owns."),
        )
    }

    /// The token's SHA-256 ([`token_hash`]); with no token given, `NOT_YOURS`
    /// for a harness `name` a program owns.
    fn hash_for(state: &Value, args: &[String], name: Option<&str>) -> Result<String, Refusal> {
        if flag(args, "--program-token-file").is_none()
            && let Some(name) = name
            && state["sessions"].get(name).is_some()
        {
            return Err(refusal(
                "NOT_YOURS",
                format!(
                    "\"{name}\" is owned by a program; this call gave no --program-token-file."
                ),
            ));
        }
        token_hash(args)
    }

    /// The program's harness `name`: `UNKNOWN_HARNESS` when there is none,
    /// `NOT_YOURS` when another token holds it.
    fn owned<'a>(state: &'a mut Value, name: &str, hash: &str) -> Result<&'a mut Value, Refusal> {
        let session = state["sessions"].get_mut(name).ok_or_else(|| {
            refusal(
                "UNKNOWN_HARNESS",
                format!(
                    "No harness named \"{name}\" is owned by this token on this computer. Start it with harness ensure."
                ),
            )
        })?;
        if session["tokenSha256"] != hash {
            return Err(not_yours(name));
        }
        Ok(session)
    }

    /// A taken input's id as Harness gives it: `program:` and a random
    /// UUID v4.
    fn input_id() -> io::Result<String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| io::Error::other("no random bytes"))?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(format!(
            "program:{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        ))
    }

    /// Now as JavaScript's `toISOString` writes it,
    /// `2026-11-17T04:59:27.216Z`.
    fn iso_now() -> String {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let (secs, millis) = (since.as_secs() as i64, since.subsec_millis());
        let (days, of_day) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        // A civil date from days since 1970: eras of 400 years that begin
        // on 1 March.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let of_era = z - era * 146_097;
        let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
        let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * of_year + 2) / 153;
        let day = of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
            of_day / 3_600,
            of_day % 3_600 / 60,
            of_day % 60
        )
    }

    fn next(state: &mut Value, counter: &str) -> u64 {
        let n = state[counter].as_u64().unwrap_or(0) + 1;
        state[counter] = json!(n);
        n
    }

    /// A session `ensure` closes to start another: stopped, and kept in
    /// `closed` with the inputs it took.
    fn retire(state: &mut Value, mut session: Value) {
        session["state"] = json!("stopped");
        if !state["closed"].is_array() {
            state["closed"] = json!([]);
        }
        if let Some(closed) = state["closed"].as_array_mut() {
            closed.push(session);
        }
    }

    pub fn run(
        home: &Path,
        operation: &str,
        args: &[String],
        step: &Value,
    ) -> io::Result<(Value, i32)> {
        if operation == "capabilities" {
            return Ok((json!({"programSessions":1}), 0));
        }
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(home.join("program-sessions.lock"))?;
        lock.lock()?;
        let path = home.join("program-sessions.json");
        let mut state: Value = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"sessions":{}}),
            Err(error) => return Err(error),
        };
        let answer = match answer(&mut state, operation, args, step) {
            Ok(answer) => answer,
            Err(refusal) => refused(refusal, step),
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(serde_json::to_string(&state)?.as_bytes())?;
        Ok(answer)
    }

    fn answer(
        state: &mut Value,
        operation: &str,
        args: &[String],
        step: &Value,
    ) -> Result<(Value, i32), Refusal> {
        let usage = || refusal("USAGE", "Name the harness.");
        match operation {
            "ensure" => {
                let hash = hash_for(state, args, flag(args, "--name"))?;
                let name = flag(args, "--name").ok_or_else(usage)?.to_owned();
                let engine = first_word(args)
                    .ok_or_else(|| {
                        refusal(
                            "USAGE",
                            "Name the engine: harness ensure <engine> --name <name>.",
                        )
                    })?
                    .to_owned();
                let fresh = has(args, "--fresh");
                let held = state["sessions"].get(&name).cloned();
                if let Some(held) = &held {
                    if held["tokenSha256"] != hash.as_str() {
                        return Err(refusal(
                            "NAME_TAKEN",
                            format!("\"{name}\" is held by another program."),
                        ));
                    }
                    if held["engine"] != engine.as_str() {
                        return Err(refusal(
                            "NAME_TAKEN",
                            format!(
                                "\"{name}\" runs {}, not {engine}. Close it and pick another name.",
                                held["engine"].as_str().unwrap_or_default()
                            ),
                        ));
                    }
                }
                // A crashed engine still exiting: refused, nothing changed.
                if step["not_ready"] == true {
                    return Err(refusal(
                        "ENGINE_NOT_READY",
                        format!(
                            "{name}'s last engine is still stopping; nothing was started. Try harness ensure again."
                        ),
                    ));
                }
                let why = step["why"].as_str();
                let up = if step["starting"] == true {
                    "starting"
                } else {
                    "idle"
                };
                let (result, mut session) = match held {
                    // A crashed engine: the same id resumed, never a new one.
                    Some(mut held) if !fresh && step["crashed"] == true => {
                        held["state"] = json!(up);
                        ("resumed", held)
                    }
                    Some(held) if !fresh && held["state"] != "stopped" => ("live", held),
                    Some(mut held) if !fresh && why.is_none() => {
                        held["state"] = json!(up);
                        ("resumed", held)
                    }
                    held => {
                        // `--fresh` on a live one closes it first.
                        if let Some(held) = held {
                            retire(state, held);
                        }
                        let id = next(state, "nextId");
                        let id = format!("{id:08x}-0000-4000-8000-{id:012x}");
                        // openclaw's full session key from the first answer;
                        // hermes's id only once the engine reports one.
                        let conversation = match engine.as_str() {
                            "openclaw" => json!(format!("agent:main:harness-{id}")),
                            "hermes" => Value::Null,
                            _ => json!(format!(
                                "{engine}-session-{}",
                                next(state, "nextEngineSession")
                            )),
                        };
                        (
                            "new",
                            json!({
                                "id":id,
                                "name":name,
                                "engine":engine,
                                "engineSession":conversation,
                                "state":up,
                                "tokenSha256":hash,
                                "inputs":[],
                            }),
                        )
                    }
                };
                if let (Some(reported), "hermes") =
                    (step["engine_session"].as_str(), engine.as_str())
                {
                    session["engineSession"] = json!(reported);
                }
                state["sessions"][&name] = session.clone();
                // The contract's three: one at work or with the person is
                // working, as far as `ensure` says.
                let said_state = session["state"]
                    .as_str()
                    .filter(|now| matches!(*now, "idle" | "starting"))
                    .unwrap_or("working");
                let mut answer = json!({
                    "result":result,"id":session["id"],"name":name,"engine":session["engine"],
                    "engineSession":session["engineSession"],"state":said_state,
                });
                if let (Some(why), "new") = (why, result) {
                    answer["why"] = json!(why);
                }
                if let Some(ms) = step["daemon_start_ms"].as_u64() {
                    answer["daemonStarted"] = json!(true);
                    answer["daemonStartMs"] = json!(ms);
                }
                Ok((answer, 0))
            }
            "reply" => {
                let name = first_word(args).ok_or_else(usage)?.to_owned();
                let hash = hash_for(state, args, Some(&name))?;
                let text = args
                    .iter()
                    .position(|arg| arg == "--")
                    .map(|at| args[at + 1..].join(" "))
                    .unwrap_or_default();
                let now = owned(state, &name, &hash)?["state"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                match now.as_str() {
                    "stopped" => {
                        return Err(refusal(
                            "NOT_RUNNING",
                            format!(
                                "{name} is not running: it was closed or stopped. Run harness ensure to open it again."
                            ),
                        ));
                    }
                    "needs-input" => {
                        return Err(refusal(
                            "NOT_IDLE",
                            format!("{name} waits for the person (a question or a permission)."),
                        ));
                    }
                    "working" => {
                        return Err(refusal(
                            "NOT_IDLE",
                            format!(
                                "{name} is still working on the last words. Wait for it (harness wait {name}), then reply."
                            ),
                        ));
                    }
                    // Waited for within --timeout (30 s by default), here
                    // answered at once as if it passed.
                    "starting" if step["ready"] != true => {
                        return Err(refusal(
                            "ENGINE_NOT_READY",
                            format!(
                                "{name}'s engine is not ready for words yet. Nothing was typed; try again."
                            ),
                        ));
                    }
                    _ => {}
                }
                if step["timeout"] == "cancelled" {
                    return Err(Refusal {
                        beside: Some(json!({"cancelled":true})),
                        ..refusal(
                            "TIMEOUT",
                            format!(
                                "{name} did not take the words in time. Nothing was typed; send them again."
                            ),
                        )
                    });
                }
                let input_id = input_id()
                    .map_err(|_| refusal("FAILED", "No random bytes for an input id."))?;
                let taken_at = iso_now();
                let session = &mut state["sessions"][&name];
                session["state"] = json!("working");
                if let Some(inputs) = session["inputs"].as_array_mut() {
                    inputs.push(json!({"inputId":input_id,"takenAt":taken_at,"text":text}));
                }
                // p4393.5: every TIMEOUT that is not `cancelled` carries an
                // input id `wait --after` can follow, whether the words were
                // typed and not seen to start, never answered, or their Enter
                // was withheld or their paste failed.
                if matches!(
                    step["timeout"].as_str(),
                    Some("typed" | "no_answer" | "enter_withheld" | "paste_failed")
                ) {
                    return Err(Refusal {
                        beside: Some(json!({"cancelled":false,"inputId":input_id})),
                        ..refusal(
                            "TIMEOUT",
                            format!(
                                "{name} was given the words but has not been seen to take them. Follow it with harness wait {name} --after {input_id} before sending them again."
                            ),
                        )
                    });
                }
                Ok((
                    json!({"state":"taken","inputId":input_id,"takenAt":taken_at}),
                    0,
                ))
            }
            // The plain `harness wait --json` reply (`parse_wait`): the rows
            // that ended, by exit 0 idle, 3 the person, 4 stopped, 124 none.
            "wait" => {
                let after = flag(args, "--after")
                    .ok_or_else(|| {
                        refusal(
                            "USAGE",
                            "This stand-in answers only harness wait <name> --after <inputId>.",
                        )
                    })?
                    .to_owned();
                let hash = match flag(args, "--program-token-file") {
                    Some(_) => Some(token_hash(args)?),
                    None => None,
                };
                let name = first_word(args).ok_or_else(usage)?.to_owned();
                let unknown = || {
                    refusal(
                        "UNKNOWN_INPUT",
                        format!("No harness here took an input {after}."),
                    )
                };
                let took = |session: &Value| {
                    session["inputs"]
                        .as_array()
                        .is_some_and(|inputs| inputs.iter().any(|input| input["inputId"] == after))
                };
                // The session that took it: live by its name, or closed.
                let live = state["sessions"]
                    .as_object()
                    .and_then(|sessions| sessions.values().find(|session| took(session)))
                    .cloned();
                let closed = state["closed"]
                    .as_array()
                    .and_then(|closed| closed.iter().find(|session| took(session)))
                    .cloned();
                let holder = live.clone().or(closed).ok_or_else(unknown)?;
                if holder["name"] != name.as_str() {
                    return Err(unknown());
                }
                if hash.is_some_and(|hash| holder["tokenSha256"] != hash.as_str()) {
                    return Err(not_yours(&name));
                }
                let id = holder["id"].clone();
                let (end, exit) = match step["ends"].as_str() {
                    _ if live.is_none() || holder["state"] == "stopped" => ("stopped", 4),
                    Some("person") => ("needs-input", 3),
                    Some("stopped") => ("stopped", 4),
                    Some("timeout") => ("timeout", 124),
                    _ => ("idle", 0),
                };
                if live.is_some() && end != "timeout" {
                    state["sessions"][&name]["state"] = json!(end);
                }
                Ok((
                    json!({"harnesses":[{"id":id,"name":name,"state":end}]}),
                    exit,
                ))
            }
            // Today's `close --json` reply, unchanged.
            "close" => {
                let name = first_word(args).ok_or_else(usage)?.to_owned();
                let hash = hash_for(state, args, Some(&name))?;
                let session = owned(state, &name, &hash)?;
                // p4393.5: a close the engine cannot take yet is refused
                // (exit 5) and changes nothing.
                if step["not_ready"] == true {
                    return Err(refusal(
                        "ENGINE_NOT_READY",
                        format!(
                            "{name}'s engine cannot be closed yet. Nothing was changed; try again."
                        ),
                    ));
                }
                session["state"] = json!("stopped");
                Ok((
                    json!({
                        "name":name,"id":session["id"],"closed":true,
                        "effect":"Its conversation is kept; the person can resume it.","next":[],
                    }),
                    0,
                ))
            }
            // `resume` of a program-owned harness is the program's own:
            // refused without its token, and with it pointed to `ensure`.
            "resume" => {
                let name = first_word(args).ok_or_else(usage)?.to_owned();
                let hash = hash_for(state, args, Some(&name))?;
                owned(state, &name, &hash)?;
                Err(refusal(
                    "USAGE",
                    "This stand-in resumes a program's harness only by harness ensure.",
                ))
            }
            _ => Err(refusal("USAGE", "Unknown program-session verb.")),
        }
    }
}
