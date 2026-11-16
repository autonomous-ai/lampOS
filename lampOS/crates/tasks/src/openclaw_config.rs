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
//! - with [`V1_TOOLS_SWITCH`] on (the product's, #7245): V1's tool grants,
//!   mirrored from the platform's `runtimes/openclaw/service_setup.go`
//!   (lines 136 and 237-250, the design source): `tools.profile` =
//!   `full`; `tools.exec` = `{host: gateway, mode: full}` (V1's
//!   `security: full, ask: off`, written as the one `mode` that means
//!   exactly that in OpenClaw 2026.11.2, `resolveExecPolicyForMode`; see
//!   below for why never `security`/`ask`); `tools.elevated.enabled` =
//!   `true` with `allowFrom.`[`ELEVATED_CHANNEL`] = `["*"]` (V1 keys it by
//!   its messaging channel; a Harness turn comes in on OpenClaw's own
//!   `webchat`), the other channels kept; `agents.defaults.elevatedDefault`
//!   = `full`; `agents.defaults.sandbox.mode` = `off`. So the lamp's
//!   OpenClaw runs a command headless as V1's did, with no ask nobody can
//!   answer; its namespace (`openclaw-ns`) still bounds what it sees;
//! - with it off: `tools.exec.mode` = [`EXEC_MODE`], OpenClaw asking
//!   before a command not allowed already (Hermes' `approvals.mode:
//!   manual`), and V1's values above taken out where they are V1's, so the
//!   file is today's again;
//! - either way `security` and `ask` are never written and always taken
//!   out of `tools.exec`: OpenClaw refuses `mode` beside either (exit 78,
//!   the gateway down), and today's stable lamp-tasks, the rollback
//!   target, inserts `mode = "ask"` into the exec object and removes
//!   nothing else (#7245, Twelve's P1 on #7737). A file with `mode` alone
//!   stays one OpenClaw takes after that writer. (The intermediate V1-tools
//!   build that wrote `security`/`ask` and kept `mode` was never stable; a
//!   rollback to it breaks this shape and every other that keeps V1's
//!   policy, so the stable target wins.);
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
/// OpenClaw's exec mode for V1's `security: full, ask: off`: exactly that
/// policy (`resolveExecPolicyForMode("full")`, OpenClaw 2026.11.2).
pub const EXEC_MODE_V1: &str = "full";
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
/// The kill switch for V1's tool grants (#7245): on unless `off`
/// ([`v1_tools_on`]), on in the product's lamp-tasks unit.
pub const V1_TOOLS_SWITCH: &str = "LAMP_OPENCLAW_V1_TOOLS";
/// The channel `tools.elevated.allowFrom` names with V1's tools on:
/// OpenClaw's own internal channel (its `INTERNAL_MESSAGE_CHANNEL`), the
/// one a Harness turn on the gateway comes in on.
pub const ELEVATED_CHANNEL: &str = "webchat";

/// Whether [`V1_TOOLS_SWITCH`] is on: anything but `off` exactly.
pub fn v1_tools_on(value: Option<&str>) -> bool {
    value != Some("off")
}

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

/// `key` taken out of `map` when it holds `value`, V1's.
fn take(map: &mut Map<String, Value>, key: &str, value: &Value) {
    if map.get(key) == Some(value) {
        map.remove(key);
    }
}

/// `key` taken out of `map` when it is an empty object.
fn take_empty(map: &mut Map<String, Value>, key: &str) {
    if map
        .get(key)
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        map.remove(key);
    }
}

/// V1's tool grants (`runtimes/openclaw/service_setup.go`, lines 136 and
/// 237-250) in `root`, or with `on` false taken out where they are V1's,
/// with `tools.exec.mode` = [`EXEC_MODE`] as before #7245; `security` and
/// `ask` never written, always taken out.
fn v1_tools(root: &mut Map<String, Value>, on: bool) -> Result<(), Refused> {
    let defaults = object(
        object(root, "agents", "agents")?,
        "defaults",
        "agents.defaults",
    )?;
    if on {
        defaults.insert("elevatedDefault".into(), json!("full"));
        object(defaults, "sandbox", "agents.defaults.sandbox")?.insert("mode".into(), json!("off"));
    } else {
        take(defaults, "elevatedDefault", &json!("full"));
        if let Some(sandbox) = defaults.get_mut("sandbox").and_then(Value::as_object_mut) {
            take(sandbox, "mode", &json!("off"));
        }
        take_empty(defaults, "sandbox");
    }
    let tools = object(root, "tools", "tools")?;
    if on {
        tools.insert("profile".into(), json!("full"));
        let exec = object(tools, "exec", "tools.exec")?;
        // V1's `security: full, ask: off` as its one `mode`: OpenClaw
        // refuses `mode` beside `security` or `ask`, and the stable
        // rollback target inserts `mode` without removing them.
        exec.remove("security");
        exec.remove("ask");
        exec.insert("host".into(), json!("gateway"));
        exec.insert("mode".into(), json!(EXEC_MODE_V1));
        let elevated = object(tools, "elevated", "tools.elevated")?;
        elevated.insert("enabled".into(), json!(true));
        object(elevated, "allowFrom", "tools.elevated.allowFrom")?
            .insert(ELEVATED_CHANNEL.into(), json!(["*"]));
    } else {
        take(tools, "profile", &json!("full"));
        let exec = object(tools, "exec", "tools.exec")?;
        take(exec, "host", &json!("gateway"));
        // Left beside `mode`, they would make the file one OpenClaw refuses.
        exec.remove("security");
        exec.remove("ask");
        exec.insert("mode".into(), json!(EXEC_MODE));
        if let Some(elevated) = tools.get_mut("elevated").and_then(Value::as_object_mut) {
            if let Some(from) = elevated.get_mut("allowFrom").and_then(Value::as_object_mut) {
                take(from, ELEVATED_CHANNEL, &json!(["*"]));
            }
            take_empty(elevated, "allowFrom");
            take(elevated, "enabled", &json!(true));
        }
        take_empty(tools, "elevated");
    }
    Ok(())
}

/// `openclaw.json`'s text with the lamp's keys merged in, on the
/// forwarder's `port`, with `workspace` the lamp's work folder, the
/// lamp's skills when `skills` and V1's tool grants when `v1` ([`v1_tools`]).
/// The text given back unchanged when nothing changes, so a second merge
/// is idle.
pub fn merge_config(
    text: &str,
    port: u16,
    workspace: &Path,
    skills: bool,
    v1: bool,
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

    v1_tools(root, v1)?;

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
/// written when it can't be merged. Run at every lamp-tasks start, so an
/// existing lamp's file gets V1's tools (or loses them) on its next start
/// or update, its model, workspace and every other key kept; the gateway
/// takes `tools` hot and reads `agents` per turn, with no restart.
pub fn write(
    state_dir: &Path,
    port: u16,
    workspace: &Path,
    skills: bool,
    v1: bool,
) -> io::Result<bool> {
    write_checked(
        state_dir,
        port,
        workspace,
        skills,
        v1,
        Path::new("/usr/local/bin/openclaw"),
    )
}

/// [`write`], with `validator` as OpenClaw's command.
pub fn write_checked(
    state_dir: &Path,
    port: u16,
    workspace: &Path,
    skills: bool,
    v1: bool,
    _validator: &Path,
) -> io::Result<bool> {
    let path = state_dir.join(CONFIG_FILE);
    let text = read_or_empty(&path)?;
    let merged = merge_config(&text, port, workspace, skills, v1).map_err(|refused| {
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

    /// What lamp-tasks wrote from [`INSTALLED`] before #7245 (bda127adf1).
    const TODAY_INSTALLED: &str = include_str!("openclaw_config_today.json");

    fn merged(text: &str, skills: bool) -> Value {
        serde_json::from_str(&merge_config(text, 7741, Path::new(WORKSPACE), skills, true).unwrap())
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
        assert_eq!(
            value["tools"],
            json!({
                "profile": "full",
                "exec": {"host": "gateway", "mode": "full"},
                "elevated": {"enabled": true, "allowFrom": {"webchat": ["*"]}},
            })
        );
        assert_eq!(value["agents"]["defaults"]["elevatedDefault"], "full");
        assert_eq!(
            value["agents"]["defaults"]["sandbox"],
            json!({"mode": "off"})
        );
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
        assert!(merge_config("{\"plugins\": []}", 7741, Path::new(WORKSPACE), true, true).is_err());
    }

    /// #7245: V1's tool grants (`runtimes/openclaw/service_setup.go`,
    /// lines 136 and 237-250) on an existing lamp's file, the one written
    /// before: today's `exec.mode: ask` becomes V1's `full`, the model,
    /// workspace, gateway and a person's own keys stay.
    #[test]
    fn an_existing_lamps_file_gets_v1s_tools_and_keeps_its_keys() {
        let before = merge_config(INSTALLED, 7741, Path::new(WORKSPACE), true, false).unwrap();
        let mut old: Value = serde_json::from_str(&before).unwrap();
        old["tools"]["exec"]["askFallback"] = json!("deny");
        old["tools"]["elevated"] = json!({"allowFrom": {"telegram": ["42"]}});
        old["agents"]["defaults"]["sandbox"] = json!({"mode": "all", "scope": "agent"});
        let value = merged(&old.to_string(), true);
        assert_eq!(value["gateway"], old["gateway"]);
        assert_eq!(value["models"], old["models"]);
        assert_eq!(
            value["agents"]["defaults"]["model"],
            old["agents"]["defaults"]["model"]
        );
        assert_eq!(value["agents"]["defaults"]["workspace"], WORKSPACE);
        assert_eq!(
            value["tools"]["exec"],
            json!({"host": "gateway", "mode": "full", "askFallback": "deny"})
        );
        assert_eq!(
            value["tools"]["elevated"],
            json!({"enabled": true, "allowFrom": {"telegram": ["42"], "webchat": ["*"]}})
        );
        assert_eq!(
            value["agents"]["defaults"]["sandbox"],
            json!({"mode": "off", "scope": "agent"})
        );
        assert!(v1_tools_on(None) && v1_tools_on(Some("on")) && v1_tools_on(Some("x")));
        assert!(!v1_tools_on(Some("off")));
    }

    /// #7245's kill switch: off writes today's file byte for byte (the
    /// text before #7245, its `tools` only `exec.mode: ask`), and turns a
    /// file written with V1's tools back into that one.
    #[test]
    fn with_the_v1_tools_switch_off_the_file_is_todays() {
        let off = |text: &str| merge_config(text, 7741, Path::new(WORKSPACE), true, false).unwrap();
        assert_eq!(off(INSTALLED), TODAY_INSTALLED);
        let on = merge_config(INSTALLED, 7741, Path::new(WORKSPACE), true, true).unwrap();
        assert_ne!(on, TODAY_INSTALLED);
        assert_eq!(off(&on), TODAY_INSTALLED);
        assert_eq!(
            off(""),
            off(&merge_config("", 7741, Path::new(WORKSPACE), true, true).unwrap())
        );
    }

    /// OpenClaw 2026.11.2's own check of one exec object
    /// (`addExecPolicyModeConflictIssue`): `mode` beside `security` or
    /// `ask` is refused, and the gateway exits 78.
    fn openclaw_takes(exec: &Value) -> bool {
        exec.get("mode").is_none() || (exec.get("security").is_none() && exec.get("ask").is_none())
    }

    /// Today's stable lamp-tasks' writer of `tools.exec`, the rollback
    /// target (`openclaw_config.rs` on lamp-voice, 14202): it inserts
    /// `mode = "ask"` and removes nothing else.
    fn stable_writer(exec: &mut Map<String, Value>) {
        exec.insert("mode".into(), json!("ask"));
    }

    /// #7245, Twelve's P1 on #7737: a rollback to stable from either
    /// switch's file, or from an older one with `security`/`ask` in it,
    /// leaves a file OpenClaw takes: ours never has `security` or `ask`,
    /// and V1's policy is the one `mode` that means it.
    #[test]
    fn a_rollback_to_stables_writer_leaves_a_file_openclaw_takes() {
        let mut old: Value = serde_json::from_str(INSTALLED).unwrap();
        old["tools"]["exec"] = json!({"host": "gateway", "security": "full", "ask": "off"});
        let old = serde_json::to_string(&old).unwrap();
        for v1 in [true, false] {
            for text in [INSTALLED, "", old.as_str()] {
                let merged = merge_config(text, 7741, Path::new(WORKSPACE), true, v1).unwrap();
                let mut value: Value = serde_json::from_str(&merged).unwrap();
                assert!(openclaw_takes(&value["tools"]["exec"]), "{v1}: {merged}");
                let exec = value["tools"]["exec"].as_object_mut().unwrap();
                assert!(
                    !exec.contains_key("security") && !exec.contains_key("ask"),
                    "{v1}"
                );
                stable_writer(exec);
                assert!(openclaw_takes(&value["tools"]["exec"]), "{v1}: rolled back");
            }
        }
        let exec = |v1| {
            let merged = merge_config("", 7741, Path::new(WORKSPACE), true, v1).unwrap();
            serde_json::from_str::<Value>(&merged).unwrap()["tools"]["exec"].clone()
        };
        assert_eq!(exec(true), json!({"host": "gateway", "mode": EXEC_MODE_V1}));
        assert_eq!(exec(false), json!({"mode": EXEC_MODE}));
    }

    #[test]
    fn merging_twice_changes_nothing() {
        for v1 in [true, false] {
            for text in [INSTALLED, "", "{}"] {
                let once = merge_config(text, 7741, Path::new(WORKSPACE), true, v1).unwrap();
                assert_eq!(
                    merge_config(&once, 7741, Path::new(WORKSPACE), true, v1).unwrap(),
                    once
                );
            }
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
            "{\"tools\": {\"elevated\": []}}",
            "{\"tools\": {\"elevated\": {\"allowFrom\": 1}}}",
            "{\"agents\": {\"defaults\": {\"sandbox\": \"x\"}}}",
            "{\"skills\": {\"load\": {\"extraDirs\": \"/a\"}}}",
            "{\"skills\": {\"load\": {\"extraDirs\": [1]}}}",
            "{\"plugins\": true}",
        ] {
            assert!(
                merge_config(text, 7741, Path::new(WORKSPACE), true, true).is_err(),
                "{text:?}"
            );
        }
        assert!(merge_config("{}", 7741, Path::new("relative"), true, true).is_err());
    }

    #[test]
    fn the_file_is_written_0600_once_and_a_refused_one_is_left() {
        let state = tempfile::tempdir().unwrap();
        let path = state.path().join(CONFIG_FILE);
        fs::write(&path, INSTALLED).unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true, true).unwrap());
        assert!(!write(state.path(), 7741, Path::new(WORKSPACE), true, true).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("kept-gateway-token"));
        assert!(!text.contains("AUTONOMOUS_API_KEY") && !text.contains("campaign-api"));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::write(&path, "{\"models\": []}").unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true, true).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"models\": []}");
    }

    #[test]
    fn a_linked_file_is_refused() {
        let state = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join(CONFIG_FILE);
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, state.path().join(CONFIG_FILE)).unwrap();
        assert!(write(state.path(), 7741, Path::new(WORKSPACE), true, true).is_err());
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

    /// #7108: the new text is checked by OpenClaw's own `config validate`,
    /// through its wrapper, on a file beside `openclaw.json`, never the
    /// live one: refused, `write` fails with OpenClaw's words and the old
    /// file stays byte for byte with nothing left beside it; taken, the new
    /// text is in place. With no wrapper there it is written as before.
    #[test]
    fn a_text_openclaws_own_validate_refuses_never_lands() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let path = state.join(CONFIG_FILE);
        fs::write(&path, INSTALLED).unwrap();
        // A stand-in wrapper: its `config validate` keeps a copy of the
        // file it was pointed at and answers with the verdict file's
        // status.
        let verdict = dir.path().join("verdict");
        let checked = dir.path().join("checked");
        let wrapper = dir.path().join("openclaw");
        fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\n[ \"$*\" = \"config validate --json\" ] && [ \"$LAMP_OPENCLAW_GATEWAY\" = on ] && [ \"$OPENCLAW_CONFIG_PATH\" != {live} ] || exit 2\ncp \"$OPENCLAW_CONFIG_PATH\" {checked}\necho 'models.providers.lampos: Unrecognized key'\nexit \"$(cat {verdict})\"\n",
                live = path.display(),
                checked = checked.display(),
                verdict = verdict.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let write = |validator: &Path| {
            write_checked(&state, 7741, Path::new(WORKSPACE), true, true, validator)
        };

        fs::write(&verdict, "1").unwrap();
        let error = write(&wrapper).unwrap_err().to_string();
        assert!(error.contains("Unrecognized key"), "{error}");
        assert!(error.contains("status 1"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), INSTALLED);
        assert_eq!(fs::read_dir(&state).unwrap().count(), 1);
        let given = fs::read_to_string(&checked).unwrap();
        assert!(given.contains("kept-gateway-token") && given != INSTALLED);

        fs::write(&verdict, "0").unwrap();
        assert!(write(&wrapper).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), given);
        assert_eq!(fs::read_dir(&state).unwrap().count(), 1);

        fs::write(&path, INSTALLED).unwrap();
        assert!(write(&dir.path().join("none")).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), given);
    }
}
