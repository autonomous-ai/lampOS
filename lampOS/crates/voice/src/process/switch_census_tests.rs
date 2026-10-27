//! The workers' switch census (#2510). A worker starts with a cleared
//! environment and only what `worker_command` gives its role, so a switch
//! the workers' own code reads but the pass list leaves out is silently off
//! on a lamp (`LAMP_MEMORY_REPLACES` on lamp-06c1). The census is built
//! from the source at test time: every `LAMP_*` the worker side defines, or
//! names by path, or reads as a literal, must reach some worker through
//! `worker_command`, or be named in [`NOT_PASSED`] with why it need not.
//! The worker side is [`WORKER_SIDE`] and, followed from it, every other
//! module's function its code names or calls, and theirs in turn
//! (`crate::memory_scope::gap_from_environment()`), so a switch a worker
//! reads through a helper is in the census too (#2762).

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The code that runs in the Live and second routes' workers, from the
/// voice's `src`; the helpers they call are found from it ([`census`]).
const WORKER_SIDE: &[&str] = &[
    "second_provider.rs",
    "second_provider",
    "second_config.rs",
    "provider_worker.rs",
    "provider_worker",
    "tools.rs",
    "tools",
    "remember.rs",
    "remember",
    "known_faces.rs",
    "thread.rs",
    "early_words.rs",
    "rules.rs",
    "local_time.rs",
    "../../msgs/src/known.rs",
];

/// Functions in [`WORKER_SIDE`]'s files that the coordinator runs, each with
/// its caller and what it gives: the helpers they name are not followed
/// (their own reads are still counted, and [`NOT_PASSED`] says why).
const COORDINATOR_RUN: &[(&str, &str, &str)] = &[
    (
        "tools.rs",
        "arguments_from_environment",
        "coordinator.rs, the Live worker's arguments",
    ),
    (
        "thread.rs",
        "arguments_from_environment",
        "coordinator.rs, the Live worker's arguments",
    ),
    (
        "second_provider/worker.rs",
        "own_key_dropped_from_environment",
        "coordinator/second.rs, once the second worker is dropped",
    ),
];

/// Under [`WORKER_SIDE`] but no worker: the probes are commands run by
/// hand (`lamp-voice probe-*`) with the whole environment.
const NOT_WORKER: &[&str] = &["second_provider/probe"];

/// Read on the worker side's code but never needed in a worker's
/// environment, each with why.
const NOT_PASSED: &[(&str, &str)] = &[
    // `tools::arguments_from_environment`, run by the coordinator: each
    // reaches the Live worker as an argument.
    ("LAMP_TOOLS", "the coordinator's, given as an argument"),
    (
        "LAMP_ACTIONS_DIR",
        "the coordinator's, given as an argument",
    ),
    ("LAMP_SKILLS_DIR", "the coordinator's, given as an argument"),
    (
        "LAMP_MEMORY_SOCKET",
        "the coordinator's, given as an argument",
    ),
    ("LAMP_CLOCK", "the coordinator's, given as an argument"),
    (
        "LAMP_TIME_PLACES",
        "the coordinator's, given as an argument",
    ),
    (
        "LAMP_SCHEDULE_TOOLS",
        "the coordinator's, given as an argument",
    ),
    (
        "LAMP_SCHEDULE_SPEAK",
        "the coordinator's, given as an argument",
    ),
    (
        "LAMP_EYES_WHO_SOCKET",
        "the coordinator's, given as an argument",
    ),
    ("LAMP_NAMES", "the coordinator's, given as an argument"),
    (
        "LAMP_REMEMBER_SPEAKER",
        "the coordinator's, given as --remember-speaker",
    ),
    (
        "LAMP_WHO_WAS_HERE_NAMES",
        "the coordinator's, given as an argument",
    ),
    ("LAMP_ONBOARDING", "the coordinator's, given as an argument"),
    (
        "LAMP_ONBOARDING_FILE",
        "the coordinator's, given as an argument",
    ),
    ("LAMP_USE_SKILL", "the coordinator's, given as an argument"),
    ("LAMP_REPORTS", "the coordinator's, given as an argument"),
    (
        "LAMP_SLEEP_ASKED",
        "the coordinator's, given as an argument (--sleep-asked, #3748)",
    ),
    // `thread::arguments_from_environment`, the same way.
    (
        "LAMP_THREAD_FILE",
        "the coordinator's, given as an argument",
    ),
    // `second_config::load_from_env`, read by the coordinator, which gives
    // the second worker its settings.
    (
        "LAMP_VOICE_SECOND_CONFIG",
        "the coordinator's, read for its settings",
    ),
    // `worker::RouteFlags::from_values`, read by `coordinator::second` and
    // given to the second worker as arguments.
    (
        "LAMP_SECOND_CHAT",
        "the coordinator's route flag, an argument",
    ),
    (
        "LAMP_SECOND_SPEECH",
        "the coordinator's route flag, an argument",
    ),
    (
        "LAMP_SECOND_SEARCH",
        "the coordinator's route flag, an argument",
    ),
    (
        "LAMP_SECOND_WAIT_CUE",
        "the coordinator's route flag, an argument",
    ),
    (
        "LAMP_SECOND_DOWN_LINE",
        "the coordinator's route flag, an argument",
    ),
];

/// Every role a worker is started as.
const ROLES: &[&str] = &[
    "provider", "second", "capture", "speaker", "denoise", "echo", "ring", "camera", "privacy",
];

/// The values the pass list accepts for one switch or another.
const VALUES: &[&str] = &[
    "on",
    "off",
    "linear",
    "6-7",
    "4",
    "Hanoi",
    "/etc/lampOS/person/name",
    "/var/lib/lampOS/voice/tasks-told",
    "/run/credentials/lamp-voice.service",
    "https://relay.example.invalid/v1beta",
];

/// The source without its test modules and comment lines (rustfmt's
/// layout: a `#[cfg(test)]` module ends at its own indent's `}`).
fn code(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let next = lines.get(index + 1).map_or("", |next| next.trim_start());
        if line.trim() == "#[cfg(test)]"
            && (next.starts_with("mod ") || next.starts_with("pub mod "))
            && next.ends_with('{')
        {
            let indent = &lines[index + 1][..lines[index + 1].len() - next.len()];
            let end = format!("{indent}}}");
            index += 2;
            while index < lines.len() && lines[index] != end {
                index += 1;
            }
            index += 1;
            continue;
        }
        if !line.trim_start().starts_with("//") {
            kept.push(line);
        }
        index += 1;
    }
    kept.join("\n")
}

fn rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            rust_files(&entry, files);
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        files.push(path.to_owned());
    }
}

/// A test file or folder, never a worker's.
fn is_test(relative: &Path) -> bool {
    relative.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        part == "tests" || part.ends_with("tests.rs")
    })
}

/// The module path of a file: `crate::…` in the voice, `lamp_msgs::…`.
fn module_of(file: &Path, voice: &Path, msgs: &Path) -> String {
    let (root, relative) = match file.strip_prefix(voice) {
        Ok(relative) => ("crate", relative),
        Err(_) => ("lamp_msgs", file.strip_prefix(msgs).unwrap()),
    };
    let mut parts = vec![root.to_owned()];
    parts.extend(
        relative
            .with_extension("")
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .filter(|part| !matches!(part.as_str(), "lib" | "main" | "mod")),
    );
    parts.join("::")
}

/// Each `const NAME: &str = "LAMP_…";` in the code, as (NAME, value).
fn constants(code: &str) -> Vec<(String, String)> {
    code.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("const ")?;
            let (name, rest) = rest.split_once(": &str = \"")?;
            let (value, _) = rest.split_once('"')?;
            (value.starts_with("LAMP_")
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
            .then(|| (name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Each path to an upper-case item (`crate::a::B`, `super::B`), absolute.
fn paths(code: &str, module: &str) -> Vec<String> {
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .map(|run| run.trim_matches(':'))
        .filter(|run| {
            ["crate::", "super::", "lamp_msgs::"]
                .iter()
                .any(|root| run.starts_with(root))
                && run
                    .rsplit("::")
                    .next()
                    .is_some_and(|last| last.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        })
        .map(|run| {
            let mut base: Vec<&str> = module.split("::").collect();
            let mut rest = run;
            while let Some(after) = rest.strip_prefix("super::") {
                base.pop();
                rest = after;
            }
            if run.starts_with("super::") {
                format!("{}::{rest}", base.join("::"))
            } else {
                run.to_owned()
            }
        })
        .collect()
}

/// Each `var("LAMP_…")` or `var_os("LAMP_…")` literal read.
fn literal_reads(code: &str) -> Vec<String> {
    let mut reads = Vec::new();
    for opening in ["var(\"LAMP_", "var_os(\"LAMP_"] {
        for (at, _) in code.match_indices(opening) {
            let start = at + opening.len() - "LAMP_".len();
            if let Some(end) = code[start..].find('"') {
                reads.push(code[start..start + end].to_owned());
            }
        }
    }
    reads
}

/// A function of the voice's code: its top-level module, its `impl`'s type
/// (empty for a free function) and its name.
type Function = (String, String, String);

/// Each function's type (empty when free), name and body (rustfmt's
/// layout: an `impl` or a `fn` ends at its own indent's `}`); a nested
/// function's lines are in its outer one's too.
fn functions(code: &str) -> Vec<(String, String, String)> {
    let mut done = Vec::new();
    let mut impls: Vec<(String, String)> = Vec::new();
    let mut open: Vec<(String, String, String, String)> = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
            let mut rest = &trimmed["impl".len()..];
            if rest.starts_with('<') {
                let mut depth = 0;
                let end = rest
                    .char_indices()
                    .find(|(_, c)| {
                        depth += match c {
                            '<' => 1,
                            '>' => -1,
                            _ => 0,
                        };
                        depth == 0
                    })
                    .map_or(rest.len(), |(at, _)| at + 1);
                rest = &rest[end..];
            }
            if let Some((_, after)) = rest.rsplit_once(" for ") {
                rest = after;
            }
            let owner: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            impls.push((format!("{indent}}}"), owner));
        }
        let after = trimmed
            .split_once("fn ")
            .filter(|(before, _)| {
                before.split_whitespace().all(|word| {
                    word.starts_with("pub") || matches!(word, "const" | "async" | "unsafe")
                })
            })
            .map(|(_, after)| after);
        if let Some(after) = after {
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() && !line.trim_end().ends_with([';', '}']) {
                let owner = impls
                    .last()
                    .map(|(_, owner)| owner.clone())
                    .unwrap_or_default();
                open.push((format!("{indent}}}"), owner, name, String::new()));
            }
        }
        for (_, _, _, body) in &mut open {
            body.push_str(line);
            body.push('\n');
        }
        while open.last().is_some_and(|(end, _, _, _)| line == end) {
            let (_, owner, name, body) = open.pop().unwrap();
            done.push((owner, name, body));
        }
        while impls.last().is_some_and(|(end, _)| line == end) {
            impls.pop();
        }
    }
    done
}

/// The voice's top-level module a file under its `src` is in.
fn top_module(relative: &Path) -> Option<String> {
    let first = relative.components().next()?.as_os_str().to_string_lossy();
    let name = first.strip_suffix(".rs").unwrap_or(&first);
    (!matches!(name, "lib" | "main" | "..")).then(|| name.to_owned())
}

/// What a file brings in by `use crate::…`: each name, with its top-level
/// module.
fn imports(code: &str, modules: &BTreeSet<String>) -> BTreeMap<String, String> {
    let mut brought = BTreeMap::new();
    for (at, _) in code.match_indices("use crate::") {
        let item = &code[at + "use crate::".len()..];
        let item = &item[..item.find(';').unwrap_or(item.len())];
        let mut module: Option<&str> = None;
        for word in item.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if modules.contains(word) {
                module = Some(word);
            } else if let Some(module) = module.filter(|_| !word.is_empty() && word != "self") {
                brought.insert(word.to_owned(), module.to_owned());
            }
        }
    }
    brought
}

/// One body of a function, with the module of the file it is in and what
/// that file brings in.
struct Body {
    module: String,
    shown: String,
    text: String,
    imports: std::rc::Rc<BTreeMap<String, String>>,
}

/// The functions this code names or calls, `top` its own module (none for
/// another crate's) and `owner` its `impl`'s type: another module's by a
/// `crate::` path (`crate::m::f`, `crate::m::Type::f`) or by what the file
/// brings in (`use crate::m::{f, Type}`, then `f(` or `Type::f`), and its
/// own module's called bare (`f(`), as `Self::f` or as `Type::f`. A method
/// call (`.f(`) is not followed: the census follows paths, not types.
fn named_functions(
    code: &str,
    top: Option<&str>,
    owner: &str,
    imports: &BTreeMap<String, String>,
    bodies: &BTreeMap<Function, Vec<Body>>,
) -> BTreeSet<Function> {
    let mut named = BTreeSet::new();
    let mut name = |module: &str, owner: &str, function: &str| {
        let function = (module.to_owned(), owner.to_owned(), function.to_owned());
        if bodies.contains_key(&function) {
            named.insert(function);
        }
    };
    let mut runs =
        code.match_indices(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'));
    let mut start = 0;
    loop {
        let (end, next) = runs.next().unwrap_or((code.len(), ""));
        let run = &code[start..end];
        let method = code[..start].ends_with('.');
        start = end + next.len();
        let segments: Vec<&str> = run
            .split("::")
            .filter(|segment| !segment.is_empty())
            .collect();
        match segments.as_slice() {
            ["crate", module, rest @ ..] if !rest.is_empty() && Some(*module) != top => {
                let last = rest[rest.len() - 1];
                name(module, "", last);
                if rest.len() >= 2 {
                    name(module, rest[rest.len() - 2], last);
                }
            }
            ["Self", function] => name(top.unwrap_or(""), owner, function),
            [kind, function] => match imports.get(*kind) {
                Some(module) => name(module, kind, function),
                None => name(top.unwrap_or(""), kind, function),
            },
            [function] if !method => match imports.get(*function) {
                Some(module) => name(module, "", function),
                None if next == "(" => name(top.unwrap_or(""), "", function),
                None => {}
            },
            _ => {}
        }
        if end == code.len() {
            break;
        }
    }
    named
}

/// What the census saw: each switch with where, and each helper function
/// it followed from [`WORKER_SIDE`] (`module::Type::function`) with what
/// named it.
struct Census {
    read: BTreeMap<String, BTreeSet<String>>,
    helpers: BTreeMap<String, String>,
}

fn shown_function((top, owner, name): &Function) -> String {
    if owner.is_empty() {
        format!("{top}::{name}")
    } else {
        format!("{top}::{owner}::{name}")
    }
}

/// Every `LAMP_*` the worker side reads, with where it was seen.
fn census() -> Census {
    census_leaving_out(&[])
}

/// The census with the helper modules `left_out` never followed, for the
/// controls that show a helper's switches come from following it.
///
/// Past [`WORKER_SIDE`], the census follows functions, not whole modules:
/// each function of another module a worker file names, and each function
/// that one names or calls, and so on, counting the switches their bodies
/// read. A whole module would bring in its coordinator's reads
/// (`agent_tasks::arguments_from_environment`, run by the coordinator,
/// beside `agent_tasks::named_ask_on`, run by the second worker).
fn census_leaving_out(left_out: &[&str]) -> Census {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let voice = manifest.join("src").canonicalize().unwrap();
    let msgs = manifest.join("../msgs/src").canonicalize().unwrap();
    let mut known = BTreeMap::new();
    let mut sources = Vec::new();
    for root in [&voice, &msgs] {
        let mut files = Vec::new();
        rust_files(root, &mut files);
        for file in files {
            let module = module_of(&file, &voice, &msgs);
            let code = code(&std::fs::read_to_string(&file).unwrap());
            for (name, value) in constants(&code) {
                known.insert(format!("{module}::{name}"), value);
            }
            sources.push((file, module, code));
        }
    }
    let modules: BTreeSet<String> = sources
        .iter()
        .filter_map(|(file, _, _)| top_module(file.strip_prefix(&voice).ok()?))
        .collect();
    let mut bodies: BTreeMap<Function, Vec<Body>> = BTreeMap::new();
    for (file, module, code) in &sources {
        let Ok(relative) = file.strip_prefix(&voice) else {
            continue;
        };
        let Some(top) = top_module(relative).filter(|_| !is_test(relative)) else {
            continue;
        };
        let brought = std::rc::Rc::new(imports(code, &modules));
        for (owner, name, text) in functions(code) {
            bodies
                .entry((top.clone(), owner, name))
                .or_default()
                .push(Body {
                    module: module.clone(),
                    shown: relative.display().to_string(),
                    text,
                    imports: brought.clone(),
                });
        }
    }
    let switches = |text: &str, module: &str| -> Vec<String> {
        let mut names: Vec<String> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter_map(|word| known.get(&format!("{module}::{word}")).cloned())
            .collect();
        names.extend(
            paths(text, module)
                .iter()
                .filter_map(|path| known.get(path).cloned()),
        );
        names.extend(literal_reads(text));
        names
    };
    let sides: BTreeSet<String> = WORKER_SIDE
        .iter()
        .filter_map(|side| top_module(Path::new(side)))
        .collect();
    let mut read: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut follow: Vec<(Function, String)> = Vec::new();
    for side in WORKER_SIDE {
        let mut files = Vec::new();
        rust_files(&voice.join(side).canonicalize().unwrap(), &mut files);
        for file in files {
            let shown = file
                .strip_prefix(&voice)
                .map_or_else(|_| side.to_string(), |path| path.display().to_string());
            let relative = file.strip_prefix(&voice).unwrap_or(&file);
            if is_test(relative)
                || NOT_WORKER
                    .iter()
                    .any(|not| relative.to_string_lossy().starts_with(not))
            {
                continue;
            }
            let module = module_of(&file, &voice, &msgs);
            let code = code(&std::fs::read_to_string(&file).unwrap());
            let top = top_module(relative);
            let brought = imports(&code, &modules);
            let mut run_here = code.clone();
            for (_, name, body) in functions(&code) {
                if COORDINATOR_RUN
                    .iter()
                    .any(|(file, run, _)| *file == shown && *run == name)
                {
                    run_here = run_here.replace(&body, "");
                }
            }
            for function in named_functions(&run_here, top.as_deref(), "", &brought, &bodies) {
                follow.push((function, shown.clone()));
            }
            let mut names: Vec<String> = constants(&code)
                .into_iter()
                .map(|(_, value)| value)
                .collect();
            names.extend(switches(&code, &module));
            for name in names {
                read.entry(name).or_default().insert(shown.clone());
            }
        }
    }
    let mut helpers = BTreeMap::new();
    let mut followed = BTreeSet::new();
    while let Some((function, from)) = follow.pop() {
        let top = function.0.as_str();
        if sides.contains(top) || left_out.contains(&top) || !followed.insert(function.clone()) {
            continue;
        }
        let shown = shown_function(&function);
        let mut reads = false;
        for body in &bodies[&function] {
            let names = switches(&body.text, &body.module);
            reads |= !names.is_empty();
            for name in names {
                read.entry(name)
                    .or_default()
                    .insert(format!("{} ({shown})", body.shown));
            }
            for next in named_functions(&body.text, Some(top), &function.1, &body.imports, &bodies)
            {
                follow.push((next, shown.clone()));
            }
        }
        if reads {
            helpers.insert(shown, from);
        }
    }
    Census { read, helpers }
}

/// The switches the second worker reads only through its helpers (#2762):
/// the face gap's hold in `memory_scope`, the speaking faces' two in
/// `speaking_faces`.
const THROUGH_HELPERS: &[(&str, &str)] = &[
    ("LAMP_FACE_GAP_HOLD", "memory_scope::gap_from_environment"),
    (
        "LAMP_SECOND_SPEAKING_FACE",
        "speaking_faces::from_environment",
    ),
    (
        "LAMP_EYES_SPEAKING",
        "speaking_faces::eyes_from_environment",
    ),
];

/// Whether `worker_command` gives the switch to some role with some value.
/// The stall switches it gives from the voice's own environment
/// (`stalls::passed_on`), which the census's `var` cannot set.
fn passed(name: &str) -> bool {
    if [crate::stalls::ENVIRONMENT, crate::stalls::DIR_ENVIRONMENT].contains(&name) {
        return true;
    }
    let boot = super::new_boot().unwrap();
    ROLES.iter().any(|role| {
        VALUES.iter().any(|value| {
            let only = |asked: &str| (asked == name).then(|| OsString::from(value));
            let command = super::worker_command(
                Path::new("/bin/true"),
                Path::new("/"),
                role,
                (boot, boot),
                &[],
                only,
            );
            command
                .get_envs()
                .any(|(key, given)| key == name && given.is_some())
        })
    })
}

#[test]
fn session_read_access_is_owned_by_the_coordinator_and_never_given_to_a_worker() {
    let setting = crate::session_reader::ENVIRONMENT;
    assert!(
        !census().read.contains_key(setting),
        "a worker must not resolve or grant session-reader access"
    );
    assert!(
        !passed(setting),
        "the reader group belongs only to continuous coordinator startup"
    );
}

#[test]
fn remember_speaker_arguments_survive_both_workers_cleared_environments() {
    let boot = super::new_boot().unwrap();
    let flags = [
        "--who-socket",
        "/tmp/eyes.sock",
        "--names-checked",
        crate::remember_speaker::FLAG,
    ];
    for role in ["provider", "second"] {
        let command = super::worker_command(
            Path::new("/bin/true"),
            Path::new("/"),
            role,
            (boot, boot),
            &flags,
            |_| None,
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect();
        let at = args.iter().position(|arg| arg == "--who-socket").unwrap();
        let (settings, rest) = crate::tools::take(&args[at..]).unwrap();
        assert!(settings.remember_speaker && settings.names_checked);
        assert!(rest.is_empty());
        assert!(
            !command
                .get_envs()
                .any(|(key, _)| key == crate::remember_speaker::ENVIRONMENT)
        );
    }
}

#[test]
fn every_switch_the_workers_read_reaches_a_worker_or_says_why_not() {
    let Census { read, .. } = census();
    // The scan sees the switches it must: one by its const, one by a path
    // from another crate.
    for sure in [
        "LAMP_MEMORY_REPLACES",
        "LAMP_MEMORY_STABLE_ID",
        "LAMP_TASKS_NAMED_ASK",
    ] {
        assert!(read.contains_key(sure), "the census misses {sure}");
    }
    let allowed: BTreeSet<&str> = NOT_PASSED.iter().map(|(name, _)| *name).collect();
    let missing: Vec<String> = read
        .iter()
        .filter(|(name, _)| !allowed.contains(name.as_str()) && !passed(name))
        .map(|(name, seen)| {
            format!(
                "{name} (read in {})",
                Vec::from_iter(seen.clone()).join(", ")
            )
        })
        .collect();
    assert!(
        missing.is_empty(),
        "read by a worker but never passed to one (process.rs's pass list) nor named in NOT_PASSED: {missing:?}"
    );
    let stale: Vec<&str> = NOT_PASSED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !read.contains_key(*name) || passed(name))
        .collect();
    assert!(
        stale.is_empty(),
        "NOT_PASSED names a switch now passed or no longer read: {stale:?}"
    );
}

#[test]
fn the_census_follows_the_helpers_a_worker_calls_and_sees_their_switches() {
    let Census { read, helpers } = census();
    for (switch, helper) in THROUGH_HELPERS {
        assert!(
            helpers.contains_key(*helper),
            "the census never follows {helper}: {helpers:?}"
        );
        assert!(read.contains_key(*switch), "the census misses {switch}");
    }
}

#[test]
fn a_helper_left_out_loses_its_switches() {
    // The omission control: not following one helper loses exactly the
    // switches only it reads, so the assertion above would fail.
    for left_out in ["memory_scope", "speaking_faces"] {
        let Census { read, helpers } = census_leaving_out(&[left_out]);
        assert!(!helpers.keys().any(|helper| helper.starts_with(left_out)));
        for (switch, helper) in THROUGH_HELPERS {
            assert_eq!(
                read.contains_key(*switch),
                !helper.starts_with(left_out),
                "{switch} with {left_out} left out"
            );
        }
    }
}

#[test]
fn a_helper_is_named_by_its_functions_not_by_its_types() {
    let body = || Body {
        module: "crate::memory_scope".to_owned(),
        shown: "memory_scope.rs".to_owned(),
        text: String::new(),
        imports: Default::default(),
    };
    let function =
        |owner: &str, name: &str| ("memory_scope".to_owned(), owner.to_owned(), name.to_owned());
    let bodies = BTreeMap::from([
        (function("", "gap_from_environment"), vec![body()]),
        (function("Holds", "from_environment"), vec![body()]),
        (function("", "held"), vec![body()]),
    ]);
    let modules = BTreeSet::from(["memory_scope".to_owned(), "second_provider".to_owned()]);
    let gap = function("", "gap_from_environment");
    let holds = function("Holds", "from_environment");
    for (code, named) in [
        (
            "let gap = crate::memory_scope::gap_from_environment();",
            Some(&gap),
        ),
        (
            "let on = crate::memory_scope::Holds::from_environment();",
            Some(&holds),
        ),
        (
            "use crate::memory_scope::{FaceCache, gap_from_environment};\nlet gap = gap_from_environment();",
            Some(&gap),
        ),
        (
            "use crate::{\n    memory_scope::{Holds, ScopeLease},\n};\nlet on = Holds::from_environment();",
            Some(&holds),
        ),
        (
            "use crate::memory_scope::{FaceCache, GAP, ScopeLease};\nlet gap = GAP;",
            None,
        ),
        (
            "let cache = crate::memory_scope::FaceCache::holding_gaps(gap);",
            None,
        ),
        ("let gap = other::gap_from_environment();", None),
        ("let on = holds.from_environment();", None),
    ] {
        let brought = imports(code, &modules);
        let found = named_functions(code, Some("second_provider"), "", &brought, &bodies);
        assert_eq!(found, named.into_iter().cloned().collect(), "{code}");
    }
    // Inside its own module: bare calls and `Self::`, never a method call.
    let found = named_functions(
        "held(x) && Self::from_environment() && self.gap_from_environment()",
        Some("memory_scope"),
        "Holds",
        &BTreeMap::new(),
        &bodies,
    );
    assert_eq!(found, BTreeSet::from([function("", "held"), holds.clone()]));
    let code = "fn on() -> bool {\n    std::env::var(X).is_ok()\n}\nfn off() {}\nimpl<T> A<T> {\n    pub fn read() -> bool {\n        std::env::var_os(Y).is_some()\n    }\n}\nimpl Default for B {\n    fn default() -> Self {\n        B\n    }\n}";
    let found: Vec<(String, String)> = functions(code)
        .into_iter()
        .map(|(owner, name, _)| (owner, name))
        .collect();
    assert_eq!(
        found,
        [
            (String::new(), "on".to_owned()),
            ("A".to_owned(), "read".to_owned()),
            ("B".to_owned(), "default".to_owned()),
        ]
    );
}

#[test]
fn the_coordinators_functions_are_still_in_their_files() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, run, _) in COORDINATOR_RUN {
        let code = code(&std::fs::read_to_string(source.join(file)).unwrap());
        assert!(
            functions(&code)
                .iter()
                .any(|(owner, name, _)| owner.is_empty() && name == run),
            "COORDINATOR_RUN names {file}'s {run}, no longer there"
        );
    }
}
