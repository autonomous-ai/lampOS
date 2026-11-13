//! OpenClaw's own settings on the lamp (#6226), as [`crate::hermes_config`]
//! does for Hermes: `openclaw.json` under `OPENCLAW_STATE_DIR`, merged so
//! OpenClaw asks the lamp's DeepSeek through lamp-cloud's loopback
//! forwarder (`lamp_cloud::hermes_llm`, the same one and the same token as
//! Hermes'), never the relay with the lamp's key.
//!
//! What is set, and nothing else:
//! - `models.mode` = `merge`, and `models.providers.lampos`: the forwarder
//!   at `http://127.0.0.1:<port>/v1/deepseek`, OpenAI-compatible chat
//!   (`api: openai-completions`), its key `${LAMP_HERMES_TOKEN}`, which
//!   OpenClaw reads from its environment (the wrapper lamp-update installs
//!   exports it, as Hermes' does), and a loopback address allowed
//!   (`request.allowPrivateNetwork`); its one model the forwarder's
//!   [`MODEL`];
//! - `agents.defaults.model.primary` = `lampos/<model>`;
//! - `agents.defaults.workspace`: the lamp's own work folder
//!   (`runtime::LAMP_PROJECT`), where OpenClaw reads its `AGENTS.md`, the
//!   lamp's guidance, and its tools run, as Hermes works in it;
//! - `tools.exec.mode` = [`EXEC_MODE`]: OpenClaw asks before a command
//!   not allowed already (Hermes' `approvals.mode: manual`);
//! - with [`SKILLS_SWITCH`] on, [`SKILLS_DIR`] in `skills.load.extraDirs`:
//!   OpenClaw reads the lamp's own skills where they are, read-only, never
//!   copied into its state; the dirs a person added there are kept;
//! - `plugins.allow` = [`PLUGINS_ALLOW`], only when there is no `allow`
//!   list yet: one a person set is kept as it is, and no `deny` is added.
//!
//! Every other key is kept as it was, the `gateway` block lamp-update
//! writes above all. The file is read as JSON, the form lamp-update and
//! OpenClaw write; one that isn't (JSON5 comments, trailing commas), or a
//! key of ours that holds something else than what is set, is refused and
//! left as it is, never rewritten. A file already right is not written.
//!
//! [`write`] must run as OpenClaw's own user (`lampos`), so the file it
//! writes is OpenClaw's and no one else's; never through a link.
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value, json};

pub use crate::hermes_config::{MODEL, Refused, SKILLS_DIR, TOKEN_ENV};
use crate::hermes_config::{read_or_empty, replace};

/// OpenClaw's settings file, in its state folder.
pub const CONFIG_FILE: &str = "openclaw.json";
/// The provider's id in `models.providers`.
pub const PROVIDER: &str = "lampos";
/// The model's context window told to OpenClaw. The forwarder does not say
/// it; DeepSeek's chat models take at least this much.
pub const CONTEXT_WINDOW: u64 = 128_000;
/// The most tokens one answer asks for: DeepSeek chat's default ceiling.
pub const MAX_TOKENS: u64 = 8_192;
/// OpenClaw's exec mode that asks before a command not allowed already.
pub const EXEC_MODE: &str = "ask";
/// The plugins OpenClaw loads on the lamp when no `plugins.allow` is set
/// (#6226, measured on lamp-c0a4): with OpenClaw's default 41 of 69
/// bundled plugins on, a turn stalled 92 s and timed out; with only these
/// two the Gateway was ready in 27-50 s and two Harness turns answered in
/// 43-73 s. `openai` serves the lamp's OpenAI-compatible provider,
/// `memory-core` OpenClaw's own memory.
pub const PLUGINS_ALLOW: [&str; 2] = ["openai", "memory-core"];
/// The switch for the lamp's skills in OpenClaw, as `LAMP_HERMES_SKILLS`
/// for Hermes: on unless `off` ([`skills_on`]).
pub const SKILLS_SWITCH: &str = "LAMP_OPENCLAW_SKILLS";

/// Whether [`SKILLS_SWITCH`] is on: anything but `off` exactly, the
/// product's default being on.
pub fn skills_on(value: Option<&str>) -> bool {
    value != Some("off")
}

/// The forwarder's base for OpenClaw, on `port`.
pub fn base_url(port: u16) -> String {
    crate::hermes_config::base_url(port)
}

/// The provider block, ours whole.
fn provider(port: u16) -> Value {
    json!({
        "baseUrl": base_url(port),
        "apiKey": format!("${{{TOKEN_ENV}}}"),
        "api": "openai-completions",
        "request": {"allowPrivateNetwork": true},
        "models": [{
            "id": MODEL,
            "name": MODEL,
            "contextWindow": CONTEXT_WINDOW,
            "maxTokens": MAX_TOKENS,
        }],
    })
}

/// The object at `key` in `map`, made when missing or `null`; refused
/// when it holds anything else.
fn object<'a>(
    map: &'a mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a mut Map<String, Value>, Refused> {
    let value = map.entry(key).or_insert(Value::Null);
    if value.is_null() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .ok_or_else(|| Refused(format!("`{path}` is not an object")))
}

fn same_path(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}

/// `openclaw.json`'s text with the lamp's keys merged in, on the
/// forwarder's `port`, with `workspace` the lamp's work folder and the
/// lamp's skills when `skills`. The text given back unchanged when nothing
/// changes, so a second merge is idle.
pub fn merge_config(
    text: &str,
    port: u16,
    workspace: &Path,
    skills: bool,
) -> Result<String, Refused> {
    let workspace = workspace
        .to_str()
        .filter(|_| workspace.is_absolute())
        .ok_or_else(|| Refused("the work folder is not a plain absolute path".into()))?;
    let was: Value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|_| Refused("the file is not plain JSON".into()))?
    };
    let mut value = was.clone();
    let root = value
        .as_object_mut()
        .ok_or_else(|| Refused("the file is not an object".into()))?;

    let models = object(root, "models", "models")?;
    models.insert("mode".into(), json!("merge"));
    object(models, "providers", "models.providers")?.insert(PROVIDER.into(), provider(port));

    let agents = object(root, "agents", "agents")?;
    let defaults = object(agents, "defaults", "agents.defaults")?;
    // A model given as one string is OpenClaw's short form for `primary`.
    if defaults.get("model").is_some_and(Value::is_string) {
        defaults.remove("model");
    }
    object(defaults, "model", "agents.defaults.model")?
        .insert("primary".into(), json!(format!("{PROVIDER}/{MODEL}")));
    defaults.insert("workspace".into(), json!(workspace));

    let tools = object(root, "tools", "tools")?;
    object(tools, "exec", "tools.exec")?.insert("mode".into(), json!(EXEC_MODE));

    // Only an allow list not there yet: a person's own is theirs.
    let plugins = object(root, "plugins", "plugins")?;
    if plugins.get("allow").is_none_or(Value::is_null) {
        plugins.insert("allow".into(), json!(PLUGINS_ALLOW));
    }

    if skills {
        let skills = object(root, "skills", "skills")?;
        let load = object(skills, "load", "skills.load")?;
        let dirs = load.entry("extraDirs").or_insert(Value::Null);
        if dirs.is_null() {
            *dirs = json!([]);
        }
        let dirs = dirs
            .as_array_mut()
            .ok_or_else(|| Refused("`skills.load.extraDirs` is not a list".into()))?;
        if dirs.iter().any(|dir| !dir.is_string()) {
            return Err(Refused(
                "`skills.load.extraDirs` is not a list of paths".into(),
            ));
        }
        if !dirs
            .iter()
            .filter_map(Value::as_str)
            .any(|dir| same_path(dir, SKILLS_DIR))
        {
            dirs.push(json!(SKILLS_DIR));
        }
    }

    if value == was {
        return Ok(text.to_owned());
    }
    let mut out = serde_json::to_string_pretty(&value).expect("a JSON value");
    out.push('\n');
    Ok(out)
}

/// `openclaw.json` under `state_dir` merged ([`merge_config`]); `true`
/// when it was written, `false` when it was right already. Nothing is
/// written when it can't be merged.
pub fn write(state_dir: &Path, port: u16, workspace: &Path, skills: bool) -> io::Result<bool> {
    let path = state_dir.join(CONFIG_FILE);
    let text = read_or_empty(&path)?;
    let merged = merge_config(&text, port, workspace, skills).map_err(|refused| {
        io::Error::other(format!(
            "OpenClaw's {CONFIG_FILE} is not merged: {}",
            refused.0
        ))
    })?;
    if merged == text {
        return Ok(false);
    }
    fs::create_dir_all(state_dir)?;
    replace(&path, &merged)?;
    Ok(true)
}

/// The lamp's guidance for OpenClaw: the same words as Hermes'
/// ([`crate::hermes_config::guidance_for`]), with OpenClaw's own name and
/// state folder.
pub fn guidance() -> String {
    crate::hermes_config::guidance_for("OpenClaw", crate::runtime::OPENCLAW_STATE_DIR)
}

/// [`guidance`] in `folder`'s `AGENTS.md`, as
/// [`crate::hermes_config::write_guidance`] writes Hermes'.
pub fn write_guidance(folder: &Path) -> io::Result<bool> {
    crate::hermes_config::write_guidance_text(folder, &guidance())
}

/// The agent a skill's run is one turn of on OpenClaw's warm gateway
/// (#6685 step 4; lamp-update's `GATEWAY_DATA` names it as the gateway
/// agent's `agent`): its fence, written only with
/// `LAMP_SKILL_DEFAULT_AGENT` on ([`write_skill_run_fence`]).
pub const SKILL_RUN_AGENT: &str = "lamp-skill-run";
/// OpenClaw's own default agent's id, listed first when the fence makes
/// `agents.list`, so the person's spoken tasks keep their agent.
pub const MAIN_AGENT: &str = "main";
/// The fence agent's workspace in lamp-tasks' Harness project: its own
/// folder, never a run's (`skill-*`) nor the lamp's work folder.
pub const SKILL_RUN_WORKSPACE: &str = "gateway-skill-runs";
/// OpenClaw's exec approvals, under its `HOME` (OpenClaw's `PLACE`).
pub const APPROVALS_FILE: &str = ".openclaw/exec-approvals.json";

/// The fence agent's entry in `agents.list`, ours whole: its own
/// workspace, `exec` its only tool, run on the gateway's host by an
/// allow-list with no prompt and nothing elevated.
fn skill_run_agent(workspace: &str) -> Value {
    json!({
        "id": SKILL_RUN_AGENT,
        "workspace": workspace,
        "sandbox": {"mode": "off"},
        "tools": {
            "allow": ["exec"],
            "elevated": {"enabled": false},
            "exec": {"host": "gateway", "security": "allowlist", "ask": "off", "safeBins": []},
        },
    })
}

/// `openclaw.json`'s text with the fence agent ([`SKILL_RUN_AGENT`]) set
/// in `agents.list`, its workspace `workspace`: made with OpenClaw's own
/// default first ([`MAIN_AGENT`], `default`) when there is no list; one
/// there kept, our entry set in it. Unchanged text when nothing changes.
pub fn merge_skill_run_agent(text: &str, workspace: &Path) -> Result<String, Refused> {
    let workspace = workspace
        .to_str()
        .filter(|_| workspace.is_absolute())
        .ok_or_else(|| Refused("the fence's workspace is not a plain absolute path".into()))?;
    let was: Value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|_| Refused("the file is not plain JSON".into()))?
    };
    let mut value = was.clone();
    let root = value
        .as_object_mut()
        .ok_or_else(|| Refused("the file is not an object".into()))?;
    let agents = object(root, "agents", "agents")?;
    let list = agents.entry("list").or_insert(Value::Null);
    if list.is_null() {
        *list = json!([{"id": MAIN_AGENT, "default": true}]);
    }
    let list = list
        .as_array_mut()
        .ok_or_else(|| Refused("`agents.list` is not a list".into()))?;
    let ours = skill_run_agent(workspace);
    match list
        .iter_mut()
        .find(|agent| agent.get("id").and_then(Value::as_str) == Some(SKILL_RUN_AGENT))
    {
        Some(agent) => *agent = ours,
        None => list.push(ours),
    }
    if value == was {
        return Ok(text.to_owned());
    }
    let mut out = serde_json::to_string_pretty(&value).expect("a JSON value");
    out.push('\n');
    Ok(out)
}

/// OpenClaw's exec approvals' text with the fence agent's own: an
/// allow-list of `start` alone (each run's start, `<project>/skill-*/start`),
/// no prompt, a prompt's fallback denied, no skill's program allowed by
/// itself. Every other key kept; unchanged text when nothing changes.
pub fn merge_skill_run_approvals(text: &str, start: &str) -> Result<String, Refused> {
    let was: Value = if text.trim().is_empty() {
        json!({"version": 1})
    } else {
        serde_json::from_str(text).map_err(|_| Refused("the file is not plain JSON".into()))?
    };
    let mut value = was.clone();
    let root = value
        .as_object_mut()
        .ok_or_else(|| Refused("the file is not an object".into()))?;
    object(root, "agents", "agents")?.insert(
        SKILL_RUN_AGENT.into(),
        json!({
            "security": "allowlist",
            "ask": "off",
            "askFallback": "deny",
            "autoAllowSkills": false,
            "allowlist": [{"pattern": start}],
        }),
    );
    if value == was && !text.trim().is_empty() {
        return Ok(text.to_owned());
    }
    let mut out = serde_json::to_string_pretty(&value).expect("a JSON value");
    out.push('\n');
    Ok(out)
}

/// The fence for a skill's run on OpenClaw's gateway (#6685 step 4),
/// only with `LAMP_SKILL_DEFAULT_AGENT` on: the fence agent in
/// `state_dir`'s `openclaw.json` ([`merge_skill_run_agent`]) with its
/// workspace made 0700 in `project`, and its approvals in `home`'s
/// [`APPROVALS_FILE`] ([`merge_skill_run_approvals`]), each written only
/// when it changes. `true` when either was written.
pub fn write_skill_run_fence(state_dir: &Path, home: &Path, project: &Path) -> io::Result<bool> {
    use std::os::unix::fs::DirBuilderExt;
    let workspace = project.join(SKILL_RUN_WORKSPACE);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&workspace)?;
    let start = project.join(format!(
        "{}*/{}",
        crate::skill_task::RUN_FOLDER_PREFIX,
        crate::gateway_run::START_FILE
    ));
    let start = start
        .to_str()
        .ok_or_else(|| io::Error::other("the project is not a plain path"))?
        .to_owned();
    let mut changed = false;
    for (path, merge) in [
        (
            state_dir.join(CONFIG_FILE),
            &(|text: &str| merge_skill_run_agent(text, &workspace))
                as &dyn Fn(&str) -> Result<String, Refused>,
        ),
        (home.join(APPROVALS_FILE), &|text: &str| {
            merge_skill_run_approvals(text, &start)
        }),
    ] {
        let text = read_or_empty(&path)?;
        let merged = merge(&text).map_err(|refused| {
            io::Error::other(format!(
                "{} is not merged for a skill's run: {}",
                path.display(),
                refused.0
            ))
        })?;
        if merged != text {
            if let Some(parent) = path.parent() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)?;
            }
            replace(&path, &merged)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const WORKSPACE: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp";

    /// What lamp-update writes at install: the Gateway block only.
    const INSTALLED: &str = r#"{
  "gateway": {
    "mode": "local",
    "bind": "loopback",
    "port": 18789,
    "auth": {"mode": "token", "token": "kept-gateway-token"}
  }
}
"#;

    fn merged(text: &str, skills: bool) -> Value {
        serde_json::from_str(&merge_config(text, 7741, Path::new(WORKSPACE), skills).unwrap())
            .unwrap()
    }

    #[test]
    fn the_lamp_provider_is_set_and_the_gateway_is_kept() {
        let value = merged(INSTALLED, true);
        let installed: Value = serde_json::from_str(INSTALLED).unwrap();
        assert_eq!(value["gateway"], installed["gateway"]);
        assert_eq!(value["models"]["mode"], "merge");
        assert_eq!(
            value["models"]["providers"]["lampos"],
            json!({
                "baseUrl": "http://127.0.0.1:7741/v1/deepseek",
                "apiKey": "${LAMP_HERMES_TOKEN}",
                "api": "openai-completions",
                "request": {"allowPrivateNetwork": true},
                "models": [{"id": "deepseek-flash", "name": "deepseek-flash",
                    "contextWindow": 128000, "maxTokens": 8192}],
            })
        );
        assert_eq!(
            value["agents"]["defaults"]["model"]["primary"],
            "lampos/deepseek-flash"
        );
        assert_eq!(value["agents"]["defaults"]["workspace"], WORKSPACE);
        assert_eq!(value["tools"]["exec"]["mode"], "ask");
        assert_eq!(
            value["skills"]["load"]["extraDirs"],
            json!(["/var/lib/lampOS/skills"])
        );
        assert_eq!(
            value["plugins"],
            json!({"allow": ["openai", "memory-core"]})
        );
    }

    /// #6226, lamp-c0a4: a fresh file gets exactly the two plugins; an
    /// allow list already set is kept as it is, with no deny added; the
    /// models and agents merge is the same either way.
    #[test]
    fn the_plugins_allowed_are_set_only_when_none_are() {
        let fresh = merged("", true);
        assert_eq!(
            fresh["plugins"],
            json!({"allow": ["openai", "memory-core"]})
        );
        let own = merged(
            r#"{"plugins": {"allow": ["telegram"], "entries": {"x": {"enabled": true}}}}"#,
            true,
        );
        assert_eq!(
            own["plugins"],
            json!({"allow": ["telegram"], "entries": {"x": {"enabled": true}}})
        );
        let empty = merged(r#"{"plugins": {"allow": []}}"#, true);
        assert_eq!(
            empty["plugins"],
            json!({"allow": []}),
            "a person's empty list"
        );
        for value in [&own, &empty] {
            assert_eq!(value["models"], fresh["models"]);
            assert_eq!(value["agents"], fresh["agents"]);
        }
        assert!(merge_config("{\"plugins\": []}", 7741, Path::new(WORKSPACE), true).is_err());
    }

    #[test]
    fn merging_twice_changes_nothing() {
        for text in [INSTALLED, "", "{}"] {
            let once = merge_config(text, 7741, Path::new(WORKSPACE), true).unwrap();
            assert_eq!(
                merge_config(&once, 7741, Path::new(WORKSPACE), true).unwrap(),
                once
            );
        }
    }

    #[test]
    fn other_keys_providers_and_skills_dirs_are_kept() {
        let text = r#"{
  "models": {"providers": {"mine": {"baseUrl": "https://example.invalid"}}},
  "agents": {"defaults": {"model": {"primary": "mine/x", "fallbacks": ["mine/y"]},
             "skipBootstrap": true}, "list": [{"id": "main"}]},
  "tools": {"exec": {"askFallback": "deny"}},
  "skills": {"load": {"extraDirs": ["~/my-skills", "/var/lib/lampOS/skills/"], "watch": false},
             "entries": {"a": {"enabled": false}}},
  "channels": {"telegram": {"enabled": false}}
}"#;
        let value = merged(text, true);
        assert_eq!(
            value["models"]["providers"]["mine"]["baseUrl"],
            "https://example.invalid"
        );
        assert_eq!(
            value["agents"]["defaults"]["model"]["fallbacks"],
            json!(["mine/y"])
        );
        assert_eq!(value["agents"]["defaults"]["skipBootstrap"], true);
        assert_eq!(value["agents"]["list"], json!([{"id": "main"}]));
        assert_eq!(value["tools"]["exec"]["askFallback"], "deny");
        assert_eq!(
            value["skills"]["load"]["extraDirs"],
            json!(["~/my-skills", "/var/lib/lampOS/skills/"]),
            "already there in another spelling"
        );
        assert_eq!(value["skills"]["load"]["watch"], false);
        assert_eq!(value["skills"]["entries"]["a"]["enabled"], false);
        assert_eq!(value["channels"]["telegram"]["enabled"], false);
        // A short-form model string becomes the lamp's primary.
        let short = merged(r#"{"agents": {"defaults": {"model": "mine/x"}}}"#, true);
        assert_eq!(
            short["agents"]["defaults"]["model"],
            json!({"primary": "lampos/deepseek-flash"})
        );
    }

    #[test]
    fn with_the_skills_switch_off_no_skills_dir_is_added() {
        let value = merged(INSTALLED, false);
        assert_eq!(value.get("skills"), None);
        assert!(skills_on(None) && skills_on(Some("on")) && !skills_on(Some("off")));
    }

    #[test]
    fn what_cannot_be_merged_safely_is_refused() {
        for text in [
            "// a comment\n{}",
            "{\"a\": 1,}",
            "[]",
            "{\"models\": []}",
            "{\"models\": {\"providers\": \"x\"}}",
            "{\"agents\": {\"defaults\": 1}}",
            "{\"tools\": {\"exec\": true}}",
            "{\"skills\": {\"load\": {\"extraDirs\": \"/a\"}}}",
            "{\"skills\": {\"load\": {\"extraDirs\": [1]}}}",
            "{\"plugins\": true}",
        ] {
            assert!(
                merge_config(text, 7741, Path::new(WORKSPACE), true).is_err(),
                "{text:?}"
            );
        }
        assert!(merge_config("{}", 7741, Path::new("relative"), true).is_err());
    }

    #[test]
    fn the_file_is_written_0600_once_and_a_refused_one_is_left() {
        let state = tempfile::tempdir().unwrap();
        let path = state.path().join(CONFIG_FILE);
        fs::write(&path, INSTALLED).unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true).unwrap());
        assert!(!write(state.path(), 7741, Path::new(WORKSPACE), true).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("kept-gateway-token"));
        assert!(!text.contains("AUTONOMOUS_API_KEY") && !text.contains("campaign-api"));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::write(&path, "{\"models\": []}").unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"models\": []}");
    }

    #[test]
    fn a_linked_file_is_refused() {
        let state = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join(CONFIG_FILE);
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, state.path().join(CONFIG_FILE)).unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
    }

    #[test]
    fn the_guidance_is_hermes_words_with_openclaws_name_and_state() {
        let text = guidance();
        assert!(text.contains("You are OpenClaw, running on the lamp itself"));
        assert!(text.contains("`/var/lib/lampos-openclaw/state` or anywhere else"));
        assert_eq!(
            text.replace("OpenClaw", "Hermes")
                .replace("/var/lib/lampos-openclaw/state", "~/.hermes"),
            crate::hermes_config::guidance()
        );
        let folder = tempfile::tempdir().unwrap();
        assert!(write_guidance(folder.path()).unwrap());
        assert!(!write_guidance(folder.path()).unwrap());
        assert_eq!(
            fs::read_to_string(folder.path().join("AGENTS.md")).unwrap(),
            text
        );
    }

    #[test]
    fn the_skill_run_fence_keeps_the_main_agent_and_is_idle_twice() {
        let ws = Path::new("/var/lib/lampos-tasks/harnesses/lamp-tasks/gateway-skill-runs");
        let once = merge_skill_run_agent(INSTALLED, ws).unwrap();
        let value: Value = serde_json::from_str(&once).unwrap();
        let list = value["agents"]["list"].as_array().unwrap();
        assert_eq!(list[0], json!({"id": MAIN_AGENT, "default": true}));
        assert_eq!(list[1]["id"], SKILL_RUN_AGENT);
        assert_eq!(list[1]["tools"]["allow"], json!(["exec"]));
        assert_eq!(list[1]["tools"]["exec"]["security"], "allowlist");
        assert_eq!(
            value["gateway"],
            serde_json::from_str::<Value>(INSTALLED).unwrap()["gateway"]
        );
        assert_eq!(merge_skill_run_agent(&once, ws).unwrap(), once);
        // A person's own list is kept; ours is set in it.
        let own = r#"{"agents":{"list":[{"id":"work","default":true}]}}"#;
        let merged: Value = serde_json::from_str(&merge_skill_run_agent(own, ws).unwrap()).unwrap();
        assert_eq!(merged["agents"]["list"][0]["id"], "work");
        assert_eq!(merged["agents"]["list"].as_array().unwrap().len(), 2);
        assert!(merge_skill_run_agent(r#"{"agents":{"list":3}}"#, ws).is_err());
        let start = "/p/skill-*/start";
        let approvals = merge_skill_run_approvals("", start).unwrap();
        let value: Value = serde_json::from_str(&approvals).unwrap();
        assert_eq!(
            value["agents"][SKILL_RUN_AGENT]["allowlist"],
            json!([{"pattern": start}])
        );
        assert_eq!(value["agents"][SKILL_RUN_AGENT]["askFallback"], "deny");
        assert_eq!(
            merge_skill_run_approvals(&approvals, start).unwrap(),
            approvals
        );
    }

    #[test]
    fn the_fence_is_written_once_in_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let project = dir.path().join("project");
        assert!(write_skill_run_fence(&state, dir.path(), &project).unwrap());
        assert!(!write_skill_run_fence(&state, dir.path(), &project).unwrap());
        assert!(project.join(SKILL_RUN_WORKSPACE).is_dir());
        let approvals = fs::read_to_string(dir.path().join(APPROVALS_FILE)).unwrap();
        assert!(approvals.contains(&format!("{}/skill-*/start", project.display())));
        let mode = fs::metadata(state.join(CONFIG_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
