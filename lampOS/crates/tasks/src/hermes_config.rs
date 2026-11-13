//! Hermes' own settings on the lamp (#5978, part 2): its `config.yaml` and
//! `.env` under `HERMES_HOME`, merged so Hermes asks the lamp's DeepSeek
//! through lamp-cloud's loopback forwarder (`lamp_cloud::hermes_llm`), never
//! the relay with the lamp's key.
//!
//! What is set, and nothing else:
//! - `model.provider` = `custom:lamp`, `model.default` = `deepseek-flash`
//!   (the voice's DeepSeek model), with any `model.base_url`, `model.api_key`
//!   or `model.api_mode` left from another provider removed;
//! - `providers.lamp`: the forwarder at `http://127.0.0.1:<port>/v1/deepseek`,
//!   OpenAI-compatible chat (`api_mode: chat_completions`), its key read from
//!   [`TOKEN_ENV`];
//! - `approvals.mode` = `manual`: Hermes asks before a dangerous command;
//! - [`SKILLS_DIR`] in `skills.external_dirs`: Hermes reads the lamp's own
//!   skills where they are, read-only, never copied into `HERMES_HOME`;
//!   the dirs a person added there are kept;
//! - [`RUN_ENV`] in `terminal.shell_init_files` (#6684): Hermes's launcher
//!   replaces the `PYTHONPATH` it is started with, so a skill run's SDK
//!   path reaches Hermes's commands only through the files its terminal
//!   sources before its login snapshot. A list there replaces Hermes's
//!   defaults ([`SHELL_DEFAULTS`]), so they are listed first when the merge
//!   makes the list (unless `terminal.auto_source_bashrc` is off); the
//!   files a person listed are kept;
//! - `.env`'s [`TOKEN_ENV`] line: the forwarder's local token, never the
//!   relay's key;
//! - with [`AGENT_SERVICES_SWITCH`] on, `mcp_servers.lamp-services`
//!   ([`mcp_entry`]): the lamp's agent's connectors, `lamp-services mcp`,
//!   given a skill run's `LAMP_CONTROL_SOCKET` and one-run token
//!   `LAMP_CONTROL_TOKEN` by Hermes's own `${VAR}` interpolation, so it
//!   reaches the linked services only through lamp-control under the run's
//!   grants; with it off, that one entry is taken out (a person's other
//!   servers are kept).
//!
//! Every other line is kept as it was, Harness's `hooks:` block above all
//! (openharness `engines/hermes/contract.ts`). The workspace has no YAML
//! crate, so the merge is line-based over block-style YAML, the style Hermes
//! and Harness write; a file it can't merge safely (tabs, several
//! documents, a flow-style or sequence value where a mapping is set) is
//! refused and left as it is, never rewritten.
//!
//! [`write`] must run as Hermes' own user (`lampos`), so the files it
//! writes are Hermes' and no one else's. The token it is given is lamp-cloud's
//! [`TOKEN_PATH`], which lamp-tasks (also `lampos`) reads. Neither file is
//! followed through a link, and one that isn't a regular file is refused.
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The custom provider's name in `providers:`.
pub const PROVIDER: &str = "lamp";
/// The model asked: the voice's DeepSeek default (`second_config.rs`'s
/// `DEEPSEEK_MODEL`).
pub const MODEL: &str = "deepseek-flash";
/// Where Hermes' key for `lamp` is read: `.env`'s line, the local token.
pub const TOKEN_ENV: &str = "LAMP_HERMES_TOKEN";
/// lamp-cloud's local token for its forwarder
/// (`lamp_cloud::hermes_llm::TOKEN_FILE` in its state folder), read by
/// lamp-tasks and handed to [`write`].
pub const TOKEN_PATH: &str = "/var/lib/lampos-cloud/hermes-token";
/// Provider keys taken out of `.env`: Hermes asks only the forwarder, and
/// no provider's own key is kept where Hermes, or what it runs, reads it.
pub const DROPPED_KEYS: [&str; 5] = [
    "DEEPSEEK_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "AUTONOMOUS_API_KEY",
    "OPENROUTER_API_KEY",
];
/// Hermes' approval mode that asks before a dangerous command.
pub const APPROVALS: &str = "manual";
/// The lamp's own skills, the folder lamp-skills reads (`lamp-skills
/// --dir`), given to Hermes in place through `skills.external_dirs`.
pub const SKILLS_DIR: &str = "/var/lib/lampOS/skills";

/// lamp-update's root-owned file (`lamp_update::hermes_install::RUN_ENV`)
/// that puts the lamp's SDK on a skill run's `PYTHONPATH`, given to Hermes
/// in `terminal.shell_init_files` (#6684).
pub const RUN_ENV: &str = "/usr/local/lib/lampos/hermes-run-env.sh";

/// What Hermes's terminal sources while `terminal.shell_init_files` is
/// unset (and `auto_source_bashrc` on), in its order: an explicit list
/// replaces them, so the merge keeps them when it makes the list.
pub const SHELL_DEFAULTS: [&str; 3] = ["~/.profile", "~/.bash_profile", "~/.bashrc"];

/// The lamp's agent's connectors' switch, `on` exactly; off unless set
/// (`lamp_services::AGENT_SERVICES_ENVIRONMENT`), in lamp-tasks' own unit.
pub const AGENT_SERVICES_SWITCH: &str = "LAMP_AGENT_SERVICES";
/// The lamp's own entry in `mcp_servers`.
pub const MCP_NAME: &str = "lamp-services";
/// What it runs (`lamp_services::mcp_server::COMMAND`), with `mcp`.
pub const MCP_COMMAND: &str = "/opt/lampOS/services/current/lamp-services";

/// Whether [`AGENT_SERVICES_SWITCH`] is on in this process.
pub fn agent_services_on() -> bool {
    std::env::var(AGENT_SERVICES_SWITCH).ok().as_deref() == Some("on")
}

/// The lamp's `mcp_servers` entry, its `lamp-services:` line first: the
/// run's socket and token are Hermes's to fill in from the run's own
/// environment (`${VAR}`), never written here; outside a skill's run they
/// stay unfilled and the server reaches nothing.
pub fn mcp_entry() -> Vec<String> {
    vec![
        format!("{MCP_NAME}:"),
        format!("  command: {}", quoted(MCP_COMMAND)),
        "  args:".to_owned(),
        format!("  - {}", quoted("mcp")),
        "  env:".to_owned(),
        format!(
            "    LAMP_CONTROL_SOCKET: {}",
            quoted("${LAMP_CONTROL_SOCKET}")
        ),
        format!(
            "    LAMP_CONTROL_TOKEN: {}",
            quoted("${LAMP_CONTROL_TOKEN}")
        ),
    ]
}

/// The forwarder's base for Hermes, on `port`.
pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1/deepseek")
}

/// Why a merge was refused; the files are left as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hermes' config.yaml is not merged: {}", self.0)
    }
}

impl std::error::Error for Refused {}

/// A token as lamp-cloud makes it: 32 to 128 of `[A-Za-z0-9_-]`, so its
/// `.env` line can carry nothing else.
pub fn token_is_plain(token: &str) -> bool {
    (32..=128).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// One top-level part of the file: a `key:` line and the lines under it,
/// or lines outside any key (comments, blanks).
#[derive(Debug, Clone)]
struct Part {
    key: Option<String>,
    lines: Vec<String>,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank_or_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// `key` of a `key:` or `key: value` line, quotes taken off; `None` for any
/// other line.
fn key_of(line: &str) -> Option<String> {
    let trimmed = line.trim_start_matches(' ');
    let (key, rest) = if let Some(quoted) = trimmed.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else if let Some(quoted) = trimmed.strip_prefix('\'') {
        let end = quoted.find('\'')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        let end = trimmed.find(':')?;
        (&trimmed[..end], &trimmed[end..])
    };
    let rest = rest.strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with(' ')) || key.is_empty() || key.contains(' ') {
        return None;
    }
    Some(key.to_owned())
}

/// What follows `key:` on its line, an inline comment taken off.
fn inline_value(line: &str) -> &str {
    let Some(colon) = line.find(':') else {
        return "";
    };
    let rest = line[colon + 1..].trim();
    if rest.starts_with('#') { "" } else { rest }
}

fn parts(text: &str) -> Result<Vec<Part>, Refused> {
    let mut parts: Vec<Part> = Vec::new();
    for line in text.lines() {
        if line.contains('\t') && !line.trim_start().starts_with('#') {
            return Err(Refused("a tab in the file".into()));
        }
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." || trimmed.starts_with("--- ") {
            return Err(Refused("a document marker in the file".into()));
        }
        // PyYAML writes a list under a key at the key's own indent
        // (`hooks:` then `- command: …`): those items belong to that key.
        let list_item = line.starts_with("- ") || line == "-";
        let under_a_key = parts.last().is_some_and(|part| part.key.is_some());
        if indent(line) == 0 && !is_blank_or_comment(line) && !(list_item && under_a_key) {
            if line.starts_with('-') || line.starts_with('{') || line.starts_with('[') {
                return Err(Refused("the file is not a mapping".into()));
            }
            let Some(key) = key_of(line) else {
                return Err(Refused("a top-level line that is not a key".into()));
            };
            if parts.iter().any(|part| part.key.as_deref() == Some(&key)) {
                return Err(Refused(format!("`{key}` is there twice")));
            }
            parts.push(Part {
                key: Some(key),
                lines: vec![line.to_owned()],
            });
            continue;
        }
        match parts.last_mut() {
            // A column-0 comment or blank after a key's lines stays with
            // them; it is only ever moved with the key, never dropped.
            Some(part) => part.lines.push(line.to_owned()),
            None => parts.push(Part {
                key: None,
                lines: vec![line.to_owned()],
            }),
        }
    }
    Ok(parts)
}

/// The mapping `part`'s child indent, and where each of its children
/// starts (index in `part.lines`, key). Refused when its lines are not a
/// block mapping evenly indented.
fn children(part: &Part, top: &str) -> Result<(usize, Vec<(usize, String)>), Refused> {
    let body = &part.lines[1..];
    let child_indent = body
        .iter()
        .find(|line| !is_blank_or_comment(line))
        .map(|line| indent(line))
        .unwrap_or(2);
    if child_indent == 0 {
        return Err(Refused(format!("`{top}` has nothing indented under it")));
    }
    let mut starts = Vec::new();
    for (index, line) in part.lines.iter().enumerate().skip(1) {
        if is_blank_or_comment(line) {
            continue;
        }
        let at_indent = indent(line);
        if at_indent < child_indent {
            return Err(Refused(format!("`{top}`'s lines are not evenly indented")));
        }
        if at_indent == child_indent {
            if line.trim_start().starts_with('-') {
                // PyYAML writes a child's list at the child's own indent
                // (`external_dirs:` then `- /path`): those items belong to
                // the child above them.
                if starts.is_empty() {
                    return Err(Refused(format!("`{top}` is a list, not a mapping")));
                }
                continue;
            }
            let Some(key) = key_of(line) else {
                return Err(Refused(format!("`{top}` has a line that is not a key")));
            };
            starts.push((index, key));
        }
    }
    Ok((child_indent, starts))
}

/// Where the child at `position` of [`children`]'s `starts` runs in
/// `part.lines`: to the next child, or to the mapping's last non-blank
/// line, so a blank or comment after it stays.
fn child_span(part: &Part, starts: &[(usize, String)], position: usize) -> (usize, usize) {
    let begin = starts[position].0;
    let end = match starts.get(position + 1) {
        Some((next, _)) => *next,
        None => {
            let mut end = part.lines.len();
            while end > begin + 1 && is_blank_or_comment(&part.lines[end - 1]) {
                end -= 1;
            }
            end
        }
    };
    (begin, end)
}

/// The child `child` of the mapping `top` set to `lines` (its `child:` line
/// first, at indent 0; indented by the mapping's own child indent here), or
/// removed with `None`. The mapping is made when missing; an inline value
/// there (`{}`, `null`, a flow mapping) is replaced by the block.
fn set_child(
    parts: &mut Vec<Part>,
    top: &str,
    child: &str,
    lines: Option<&[String]>,
) -> Result<(), Refused> {
    let Some(at) = parts
        .iter()
        .position(|part| part.key.as_deref() == Some(top))
    else {
        if let Some(lines) = lines {
            let mut block = vec![format!("{top}:")];
            block.extend(lines.iter().map(|line| format!("  {line}")));
            // Keep a file ending with a newline; a new key goes last.
            parts.push(Part {
                key: Some(top.to_owned()),
                lines: block,
            });
        }
        return Ok(());
    };
    let part = &mut parts[at];
    if !inline_value(&part.lines[0]).is_empty() {
        let value = inline_value(&part.lines[0]);
        if value.starts_with('&') || value.starts_with('*') || value.starts_with('!') {
            return Err(Refused(format!("`{top}` is an anchor, alias or tag")));
        }
        if part.lines[1..]
            .iter()
            .any(|line| !is_blank_or_comment(line))
        {
            return Err(Refused(format!("`{top}` has a value and lines under it")));
        }
        // `approvals: {}` after a reset: the block takes its place.
        part.lines.truncate(1);
        part.lines[0] = format!("{top}:");
    }
    let (child_indent, starts) = children(part, top)?;
    let found: Vec<usize> = starts
        .iter()
        .enumerate()
        .filter(|(_, (_, key))| key == child)
        .map(|(position, _)| position)
        .collect();
    if found.len() > 1 {
        return Err(Refused(format!("`{top}.{child}` is there twice")));
    }
    let pad = " ".repeat(child_indent);
    let new: Vec<String> = lines
        .unwrap_or_default()
        .iter()
        .map(|line| format!("{pad}{line}"))
        .collect();
    match found.first() {
        Some(&position) => {
            let (begin, end) = child_span(part, &starts, position);
            part.lines.splice(begin..end, new);
        }
        None => {
            let mut end = part.lines.len();
            while end > 1 && is_blank_or_comment(&part.lines[end - 1]) {
                end -= 1;
            }
            part.lines.splice(end..end, new);
        }
    }
    Ok(())
}

/// A value that stands for nothing set yet: what a reset leaves.
fn is_unset(value: &str) -> bool {
    matches!(value, "{}" | "[]" | "null" | "~")
}

/// One plain list item's text as the path it names: quotes taken off, an
/// inline comment of an unquoted one taken off. `None` for anything that
/// is not a plain scalar (a mapping, a nested list, an anchor, alias or
/// tag, a block scalar).
fn plain_item(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('"') {
        let mut out = String::new();
        let mut characters = rest.chars();
        while let Some(character) = characters.next() {
            match character {
                '"' => return Some(out),
                '\\' => out.push(characters.next()?),
                other => out.push(other),
            }
        }
        return None;
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return rest.find('\'').map(|end| rest[..end].to_owned());
    }
    let text = text.split(" #").next().unwrap_or_default().trim();
    let first = text.chars().next()?;
    if "{[&*!|>-#%@`".contains(first) || text.contains(": ") || text.ends_with(':') {
        return None;
    }
    Some(text.to_owned())
}

fn same_path(left: &str, right: &str) -> bool {
    let trim = |path: &str| path.trim_end_matches('/').to_owned();
    trim(left) == trim(right)
}

/// `top.child`, a list of paths, with `item` in it: added last when it is
/// not there, every other item kept as it was; nothing changed when it is.
/// The mapping and the list are made when missing, and a value a reset
/// left (`{}`, `[]`, `null`) takes the block. A flow list of plain items
/// (`[a, b]`) or one plain path becomes a block list of the same items and
/// `item`. Anything else there is refused, never rewritten.
fn add_to_list(parts: &mut Vec<Part>, top: &str, child: &str, item: &str) -> Result<(), Refused> {
    let entry = format!("- {}", quoted(item));
    let fresh = [format!("{child}:"), entry.clone()];
    let Some(at) = parts
        .iter()
        .position(|part| part.key.as_deref() == Some(top))
    else {
        return set_child(parts, top, child, Some(&fresh));
    };
    let top_value = inline_value(&parts[at].lines[0]);
    if !top_value.is_empty() {
        if is_unset(top_value) && top_value != "[]" {
            return set_child(parts, top, child, Some(&fresh));
        }
        return Err(Refused(format!("`{top}` is not a block mapping")));
    }
    let part = &mut parts[at];
    let (child_indent, starts) = children(part, top)?;
    let found: Vec<usize> = starts
        .iter()
        .enumerate()
        .filter(|(_, (_, key))| key == child)
        .map(|(position, _)| position)
        .collect();
    let position = match found.as_slice() {
        [] => return set_child(parts, top, child, Some(&fresh)),
        [position] => *position,
        _ => return Err(Refused(format!("`{top}.{child}` is there twice"))),
    };
    let (begin, end) = child_span(part, &starts, position);
    let pad = " ".repeat(child_indent);
    let value = inline_value(&part.lines[begin]).to_owned();
    let not_plain = || Refused(format!("`{top}.{child}` is not a list of plain paths"));
    if value.is_empty() {
        // A block list: its items, the last one's line, their indent.
        let mut items = Vec::new();
        let mut last = begin;
        let mut item_pad = None;
        for (index, line) in part.lines.iter().enumerate().take(end).skip(begin + 1) {
            if is_blank_or_comment(line) {
                continue;
            }
            let trimmed = line.trim_start();
            let text = trimmed
                .strip_prefix("- ")
                .or_else(|| (trimmed == "-").then_some(""))
                .ok_or_else(not_plain)?;
            items.push(plain_item(text).ok_or_else(not_plain)?);
            item_pad.get_or_insert_with(|| " ".repeat(indent(line)));
            last = index;
        }
        if items.iter().any(|path| same_path(path, item)) {
            return Ok(());
        }
        let line = format!("{}{entry}", item_pad.unwrap_or(pad));
        part.lines.insert(last + 1, line);
        return Ok(());
    }
    if part.lines[begin + 1..end]
        .iter()
        .any(|line| !is_blank_or_comment(line))
    {
        return Err(Refused(format!(
            "`{top}.{child}` has a value and lines under it"
        )));
    }
    let kept: Vec<String> = if is_unset(&value) {
        Vec::new()
    } else if let Some(inner) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        if inner.contains(['[', ']', '{', '}', '"', '\'', '#']) {
            return Err(not_plain());
        }
        inner
            .split(',')
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| {
                plain_item(text)
                    .map(|_| text.to_owned())
                    .ok_or_else(not_plain)
            })
            .collect::<Result<_, _>>()?
    } else {
        plain_item(&value).ok_or_else(not_plain)?;
        vec![value.clone()]
    };
    if kept
        .iter()
        .any(|text| plain_item(text).is_some_and(|path| same_path(&path, item)))
    {
        return Ok(());
    }
    let mut block = vec![format!("{pad}{child}:")];
    block.extend(kept.iter().map(|text| format!("{pad}- {text}")));
    block.push(format!("{pad}{entry}"));
    part.lines.splice(begin..end, block);
    Ok(())
}

/// The inline value of `top.child`, `Some("")` for one with lines under
/// it; `None` when `top` or `child` is not there, or `top` is not a block
/// mapping (then [`add_to_list`] says what it is).
fn child_value(parts: &[Part], top: &str, child: &str) -> Result<Option<String>, Refused> {
    let Some(part) = parts.iter().find(|part| part.key.as_deref() == Some(top)) else {
        return Ok(None);
    };
    if !inline_value(&part.lines[0]).is_empty() {
        return Ok(None);
    }
    let (_, starts) = children(part, top)?;
    Ok(starts
        .iter()
        .find(|(_, key)| key == child)
        .map(|(index, _)| inline_value(&part.lines[*index]).to_owned()))
}

/// Whether `top.child` lists anything, as Hermes reads it
/// (`cfg.get(child) or []`): a block with an item under it, or an inline
/// value that is not empty or unset. A bare `child:` (YAML's null), `[]`,
/// `{}`, `null` or `~` lists nothing, so Hermes's defaults apply.
fn lists_any(parts: &[Part], top: &str, child: &str) -> Result<bool, Refused> {
    let Some(part) = parts.iter().find(|part| part.key.as_deref() == Some(top)) else {
        return Ok(false);
    };
    if !inline_value(&part.lines[0]).is_empty() {
        return Ok(false);
    }
    let (_, starts) = children(part, top)?;
    let Some(position) = starts.iter().position(|(_, key)| key == child) else {
        return Ok(false);
    };
    let (begin, end) = child_span(part, &starts, position);
    let value = inline_value(&part.lines[begin]);
    if value.is_empty() {
        return Ok(part.lines[begin + 1..end]
            .iter()
            .any(|line| !is_blank_or_comment(line)));
    }
    Ok(!is_unset(value))
}

/// Whether Hermes sources its defaults: `terminal.auto_source_bashrc`
/// read as Python's `bool()` of what PyYAML gives. Missing, true. Unquoted,
/// YAML 1.1's false spellings (`false`, `no`, `off`, any case), a null
/// (bare, `null`, `~`) or `0` are false. Quoted, any text but the empty one
/// is true (`bool("false")` is `True`).
fn sources_defaults(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let value = value.trim();
    for quote in ['"', '\''] {
        if let Some((inner, _)) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.split_once(quote))
        {
            return !inner.is_empty();
        }
    }
    let bare = value.split(" #").next().unwrap_or_default().trim();
    !["", "false", "no", "off", "null", "~", "0"]
        .iter()
        .any(|off| bare.eq_ignore_ascii_case(off))
}

/// [`RUN_ENV`] in `terminal.shell_init_files`. Making the list (none
/// there, or one listing nothing, [`lists_any`]), Hermes's
/// [`SHELL_DEFAULTS`] go in first, as Hermes would have sourced them, unless
/// it would not have ([`sources_defaults`]); a list already there is kept
/// as it is, [`RUN_ENV`] added last. Worked on a copy: a list it can't add
/// to leaves every line as it was.
fn add_shell_init(parts: &mut Vec<Part>) -> Result<(), Refused> {
    const TOP: &str = "terminal";
    const CHILD: &str = "shell_init_files";
    let listed = lists_any(parts, TOP, CHILD)?;
    let auto = sources_defaults(child_value(parts, TOP, "auto_source_bashrc")?.as_deref());
    let mut merged = parts.clone();
    if !listed && auto {
        for file in SHELL_DEFAULTS {
            add_to_list(&mut merged, TOP, CHILD, file)?;
        }
    }
    add_to_list(&mut merged, TOP, CHILD, RUN_ENV)?;
    *parts = merged;
    Ok(())
}

fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// `config.yaml`'s text with the lamp's lines merged in, on the forwarder's
/// `port`, with the lamp's agent's connectors off. The same text given
/// twice gives the same text back.
pub fn merge_config(text: &str, port: u16) -> Result<String, Refused> {
    merge_config_for(text, port, false)
}

/// [`merge_config`] with [`AGENT_SERVICES_SWITCH`] as `agent_services`:
/// on, the lamp's `mcp_servers` entry ([`mcp_entry`]) set; off, that entry
/// taken out, and `mcp_servers` with it when nothing else is left there.
pub fn merge_config_for(text: &str, port: u16, agent_services: bool) -> Result<String, Refused> {
    let mut parts = parts(text)?;
    let one = |key: &str, value: &str| vec![format!("{key}: {}", quoted(value))];
    set_child(
        &mut parts,
        "model",
        "provider",
        Some(&one("provider", &format!("custom:{PROVIDER}"))),
    )?;
    set_child(&mut parts, "model", "default", Some(&one("default", MODEL)))?;
    // Another provider's own address or key would be asked in place of ours.
    for stale in ["base_url", "api_key", "api_mode"] {
        set_child(&mut parts, "model", stale, None)?;
    }
    let provider = vec![
        format!("{PROVIDER}:"),
        format!("  name: {}", quoted(PROVIDER)),
        format!("  base_url: {}", quoted(&base_url(port))),
        format!("  key_env: {}", quoted(TOKEN_ENV)),
        format!("  api_mode: {}", quoted("chat_completions")),
        format!("  default_model: {}", quoted(MODEL)),
    ];
    set_child(&mut parts, "providers", PROVIDER, Some(&provider))?;
    // Quoted: YAML 1.1 (PyYAML) would read a bare off/on as a boolean.
    set_child(
        &mut parts,
        "approvals",
        "mode",
        Some(&one("mode", APPROVALS)),
    )?;
    add_to_list(&mut parts, "skills", "external_dirs", SKILLS_DIR)?;
    // A `terminal:` it can't merge (flow style, an anchor) keeps the rest:
    // only the SDK's path for a skill's run is left out, and said.
    if let Err(Refused(why)) = add_shell_init(&mut parts) {
        eprintln!(
            "[lamp-tasks] Hermes's terminal.shell_init_files not set (a skill's run won't have the SDK on its path): {why}"
        );
    }
    merge_mcp_servers(&mut parts, agent_services)?;
    let mut out = String::new();
    for part in &parts {
        for line in &part.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// What `mcp_servers` is in the file, for the lamp's entry.
enum Servers {
    /// No `mcp_servers` key.
    Missing,
    /// What a reset leaves (`{}`, `null`, …) on the key's own line.
    Unset,
    /// A block mapping evenly indented; whether it holds [`MCP_NAME`].
    Block { holds_ours: bool },
    /// Anything else: a flow mapping, a list, an anchor, alias or tag, an
    /// uneven indent. Never touched.
    Other,
}

fn servers(parts: &[Part]) -> Servers {
    let Some(part) = parts
        .iter()
        .find(|part| part.key.as_deref() == Some("mcp_servers"))
    else {
        return Servers::Missing;
    };
    let value = inline_value(&part.lines[0]);
    let below = part.lines[1..]
        .iter()
        .any(|line| !is_blank_or_comment(line));
    if !value.is_empty() {
        return if is_unset(value) && !below {
            Servers::Unset
        } else {
            Servers::Other
        };
    }
    match children(part, "mcp_servers") {
        Ok((_, starts)) => Servers::Block {
            holds_ours: starts.iter().any(|(_, key)| key == MCP_NAME),
        },
        Err(_) => Servers::Other,
    }
}

/// The lamp's `mcp_servers` entry ([`mcp_entry`]) set with
/// `agent_services`, or taken out without it. Only a missing key, a reset's
/// unset value or a block mapping is ever changed: any other shape (a flow
/// mapping, a list, an anchor, an uneven indent) is the person's and is
/// kept byte for byte, with the lamp's entry not added, so the rest of the
/// merge still goes on. Off, the key is touched only when it is a block
/// holding the lamp's entry, and goes only when nothing else is left in it.
fn merge_mcp_servers(parts: &mut Vec<Part>, agent_services: bool) -> Result<(), Refused> {
    match (servers(parts), agent_services) {
        (Servers::Missing | Servers::Unset | Servers::Block { .. }, true) => {
            let entry = mcp_entry();
            set_child(parts, "mcp_servers", MCP_NAME, Some(&entry[..]))
        }
        (Servers::Block { holds_ours: true }, false) => {
            set_child(parts, "mcp_servers", MCP_NAME, None)?;
            parts.retain(|part| {
                part.key.as_deref() != Some("mcp_servers")
                    || part.lines[1..]
                        .iter()
                        .any(|line| !is_blank_or_comment(line))
            });
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `.env`'s text with [`TOKEN_ENV`] set to `token`, the [`DROPPED_KEYS`]
/// taken out and every other line kept. `None` for a token that isn't
/// [`token_is_plain`].
pub fn merge_env(text: &str, token: &str) -> Option<String> {
    if !token_is_plain(token) {
        return None;
    }
    let mut out = String::new();
    let prefix = format!("{TOKEN_ENV}=");
    for line in text.lines() {
        let trimmed = line.trim_start();
        let trimmed = trimmed
            .strip_prefix("export ")
            .unwrap_or(trimmed)
            .trim_start();
        let name = trimmed.split('=').next().unwrap_or_default().trim();
        if trimmed.contains('=') && (name == TOKEN_ENV || DROPPED_KEYS.contains(&name)) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&prefix);
    out.push_str(token);
    out.push('\n');
    Some(out)
}

/// What [`write`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub config_changed: bool,
    pub env_changed: bool,
}

/// `path`'s text, never through a link (`O_NOFOLLOW`), and only of a
/// regular file; empty when there is none.
pub(crate) fn read_or_empty(path: &Path) -> io::Result<String> {
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// `text` in place of `path`, 0600, through a new file beside it renamed
/// over it, so a reader never sees half a file.
pub(crate) fn replace(path: &Path, text: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| io::Error::other("no random bytes"))?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let temporary: PathBuf = dir.join(format!(".{name}.{suffix}.new"));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Both files under `hermes_home` merged, on the forwarder's `port`, with
/// its local `token`; a file already right is not written. Nothing is
/// written when `config.yaml` can't be merged or the token isn't plain.
pub fn write(hermes_home: &Path, port: u16, token: &str) -> io::Result<Written> {
    write_for(hermes_home, port, token, agent_services_on())
}

/// [`write`] with the lamp's agent's connectors as `agent_services`.
pub fn write_for(
    hermes_home: &Path,
    port: u16,
    token: &str,
    agent_services: bool,
) -> io::Result<Written> {
    let config_path = hermes_home.join("config.yaml");
    let env_path = hermes_home.join(".env");
    let config = read_or_empty(&config_path)?;
    let env = read_or_empty(&env_path)?;
    let merged_config =
        merge_config_for(&config, port, agent_services).map_err(io::Error::other)?;
    let merged_env = merge_env(&env, token)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the token is not plain"))?;
    fs::create_dir_all(hermes_home)?;
    let written = Written {
        config_changed: merged_config != config,
        env_changed: merged_env != env,
    };
    if written.env_changed {
        replace(&env_path, &merged_env)?;
    }
    if written.config_changed {
        replace(&config_path, &merged_config)?;
    }
    Ok(written)
}

/// The lamp's guidance for Hermes, `AGENTS.md` in the lamp's work folder
/// (`runtime::LAMP_PROJECT`): Hermes reads it from its working folder as
/// the project's context. lampOS's own file, never one under
/// `HERMES_HOME`.
pub const GUIDANCE_FILE: &str = "AGENTS.md";

/// What [`GUIDANCE_FILE`] says to Hermes: [`guidance_for`] it.
pub fn guidance() -> String {
    guidance_for("Hermes", "~/.hermes")
}

/// What [`GUIDANCE_FILE`] says to the lamp's own `engine` (its name, as
/// "Hermes" or "OpenClaw"), whose own settings are in `own_home`: where the
/// lamp's skills are, how to run one's tool ([`crate::skill_call`]), and
/// the owner's rules for them. The same words for every lamp engine
/// (#6226), only the name and its home differ.
pub fn guidance_for(engine: &str, own_home: &str) -> String {
    let program = crate::skill_call::PROGRAM;
    format!(
        "\
# Working on the lamp

You are {engine}, running on the lamp itself as the user `lampos`. The lamp's \
voice (a fast model) hands you the longer tasks a person asks for; you work \
on its behalf. lampOS writes this file; changes to it are written over.

## The lamp's skills

- The lamp's own skills are in `{SKILLS_DIR}/<name>/`: a `SKILL.md` (what it \
does and when) and a `skill.toml` (its triggers, steps and tool). They are in \
your skills list as external skills. The folder is read-only and owned by the \
lamp: never edit, move or copy files there, and never copy them into \
`{own_home}` or anywhere else.
- Skills with a tool can be run by you, the same way the voice runs them: \
`{program} skill list` shows them as JSON, and \
`{program} skill call NAME '{{\"choice\":\"value\"}}'` runs one and prints \
the skill's answer as JSON (`ok` true or false). The arguments are the ones \
the list gives for that tool, or none.
- The lamp's skills service still applies its own checks (cooldowns, the \
lamp's pause, its policy). If it refuses, tell the person why, and do not \
retry it or try another way around it.
- Run a skill only when the person's task asks for it (for example \"light the \
desk while you work\" or \"dance when you're done\"). Never move, light up or \
play sounds on your own.
- Do not use any other socket under `/run/lampos-*`.
- To write a new skill, read two or three existing ones for the format, write \
it in this work folder, and tell the person where it is. Installing it on the \
lamp is the person's step, never yours.
"
    )
}

/// [`guidance`] in `folder`'s [`GUIDANCE_FILE`], written only when what is
/// there differs; `true` when it was written. Never through a link: a
/// folder or a file that is a link, or a file that isn't a regular one, is
/// refused and left as it is.
pub fn write_guidance(folder: &Path) -> io::Result<bool> {
    write_guidance_text(folder, &guidance())
}

/// `text` in `folder`'s [`GUIDANCE_FILE`], as [`write_guidance`] writes
/// Hermes' guidance: only when it differs, never through a link.
pub(crate) fn write_guidance_text(folder: &Path, text: &str) -> io::Result<bool> {
    if !fs::symlink_metadata(folder)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the work folder is not a folder",
        ));
    }
    let path = folder.join(GUIDANCE_FILE);
    if read_or_empty(&path)? == text {
        return Ok(false);
    }
    replace(&path, text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// What Harness writes for session discovery, as PyYAML dumps it beside
    /// Hermes' own keys.
    const HARNESS: &str = "\
# Hermes settings
model:
  default: anthropic/claude-sonnet
  provider: openrouter
  base_url: https://openrouter.ai/api/v1
  context_length: 200000
terminal:
  cwd: /var/lib/lampos-tasks/harnesses
hooks:
  on_session_start:
  - command: harness hook hermes session-start
    timeout: 10
  post_tool_call:
  - matcher: '*'
    command: harness hook hermes tool
approvals:
  timeout: 60
  cron_mode: deny
custom_providers:
- name: other
  base_url: https://example.invalid/v1
";

    fn hooks_block(text: &str) -> String {
        let start = text.find("hooks:").expect("hooks: there");
        let rest = &text[start..];
        let end = rest
            .lines()
            .skip(1)
            .position(|line| !line.starts_with(' '))
            .map(|lines| {
                rest.lines()
                    .take(lines + 1)
                    .map(|line| line.len() + 1)
                    .sum()
            })
            .unwrap_or(rest.len());
        rest[..end].to_owned()
    }

    const SERVERS: &str = "mcp_servers:\n  lamp-services:\n    command: \"/opt/lampOS/services/current/lamp-services\"\n    args:\n    - \"mcp\"\n    env:\n      LAMP_CONTROL_SOCKET: \"${LAMP_CONTROL_SOCKET}\"\n      LAMP_CONTROL_TOKEN: \"${LAMP_CONTROL_TOKEN}\"\n";

    /// LAMP_AGENT_SERVICES: on, the lamp's MCP server is added, once, with
    /// the run's socket and token left to Hermes's interpolation; off, the
    /// lamp's entry alone is taken out, a person's server kept, and an
    /// emptied `mcp_servers` goes.
    #[test]
    fn the_lamps_mcp_server_is_added_on_and_removed_off() {
        let on = merge_config_for(HARNESS, 7741, true).unwrap();
        assert!(on.ends_with(SERVERS), "{on}");
        assert_eq!(merge_config_for(&on, 7741, true).unwrap(), on);
        assert!(!on.contains("LAMP_CONTROL_TOKEN="));
        let off = merge_config_for(&on, 7741, false).unwrap();
        assert!(
            !off.contains("mcp_servers") && !off.contains(MCP_NAME),
            "{off}"
        );
        assert_eq!(off, merge_config(HARNESS, 7741).unwrap());
        // A person's own server beside it: kept both ways.
        let theirs = "mcp_servers:\n  notes:\n    command: /home/me/notes-mcp\n";
        let both = merge_config_for(theirs, 7741, true).unwrap();
        assert!(
            both.contains("  notes:\n    command: /home/me/notes-mcp\n  lamp-services:\n"),
            "{both}"
        );
        let kept = merge_config_for(&both, 7741, false).unwrap();
        assert!(
            kept.contains("mcp_servers:\n  notes:\n    command: /home/me/notes-mcp\n"),
            "{kept}"
        );
        assert!(!kept.contains(MCP_NAME));
        // What a reset leaves (`mcp_servers` cut), merged on again.
        let reset = merge_config_for("updates:\n  check: false\n", 7741, true).unwrap();
        assert!(reset.ends_with(SERVERS), "{reset}");
    }

    /// Off, `mcp_servers` is changed only when it is a block holding the
    /// lamp's entry; any other shape is kept byte for byte and never makes
    /// the merge refuse, so the provider lines are still written. On, a
    /// shape that isn't a block is kept too, without the lamp's entry.
    #[test]
    fn only_a_block_holding_the_lamps_entry_is_changed_off() {
        let provider = format!("  {PROVIDER}:\n");
        // A flow mapping: kept untouched, both ways.
        let flow = "mcp_servers: {notes: {command: /x}}\n";
        for on in [false, true] {
            let merged = merge_config_for(flow, 7741, on).unwrap();
            assert!(merged.starts_with(flow), "{merged}");
            assert!(merged.contains(&provider) && !merged.contains(MCP_NAME));
        }
        // A list, an anchor, an uneven indent: kept, provider lines written.
        for other in [
            "mcp_servers:\n- notes\n",
            "mcp_servers:\n  - notes\n",
            "mcp_servers: &servers\n  notes:\n    command: /x\n",
            "mcp_servers:\n    notes:\n      command: /x\n  lamp-services:\n    command: /y\n",
        ] {
            for on in [false, true] {
                let merged = merge_config_for(other, 7741, on).unwrap();
                assert!(merged.starts_with(other), "{other} -> {merged}");
                assert!(merged.contains(&provider), "{merged}");
            }
        }
        // A block holding only the lamp's entry: the key goes.
        let ours = merge_config_for("", 7741, true).unwrap();
        assert!(ours.contains(SERVERS), "{ours}");
        let off = merge_config_for(&ours, 7741, false).unwrap();
        assert!(!off.contains("mcp_servers"), "{off}");
        // A person's server beside the lamp's: the person's is kept.
        let both = format!(
            "mcp_servers:\n  notes:\n    command: /x\n{}",
            &SERVERS[13..]
        );
        let kept = merge_config_for(&both, 7741, false).unwrap();
        assert!(
            kept.starts_with("mcp_servers:\n  notes:\n    command: /x\n"),
            "{kept}"
        );
        assert!(!kept.contains(MCP_NAME), "{kept}");
        // A block without the lamp's entry, off: byte for byte.
        let theirs = "mcp_servers:\n  notes:\n    command: /x\n\n# mine\n";
        assert!(
            merge_config_for(theirs, 7741, false)
                .unwrap()
                .starts_with(theirs)
        );
    }

    /// A skill's run sees the server with its own token: the entry names
    /// exactly the two variables lamp-tasks gives the run
    /// (`skill_task::SkillRun`'s environment), for Hermes to fill in.
    #[test]
    fn the_lamps_mcp_server_takes_the_runs_socket_and_token() {
        let entry = mcp_entry().join("\n");
        for name in [crate::skill_task::SOCKET_ENV, crate::skill_task::TOKEN_ENV] {
            assert!(
                entry.contains(&format!("{name}: \"${{{name}}}\"")),
                "{entry}"
            );
        }
        assert!(entry.contains(&format!("command: \"{MCP_COMMAND}\"")));
    }

    /// The file is written with the entry on, and without it off.
    #[test]
    fn the_settings_written_follow_the_switch() {
        let dir = tempfile::tempdir().unwrap();
        let token = "a".repeat(43);
        write_for(dir.path(), 7741, &token, true).unwrap();
        let config = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(config.contains(SERVERS), "{config}");
        assert!(
            write_for(dir.path(), 7741, &token, false)
                .unwrap()
                .config_changed
        );
        let config = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!config.contains(MCP_NAME), "{config}");
    }

    #[test]
    fn harness_hooks_block_is_kept_line_for_line() {
        let merged = merge_config(HARNESS, 7741).unwrap();
        assert_eq!(hooks_block(&merged), hooks_block(HARNESS));
        assert!(merged.contains("  - command: harness hook hermes session-start\n"));
    }

    #[test]
    fn the_lamp_provider_is_set_and_other_keys_are_kept() {
        let merged = merge_config(HARNESS, 7741).unwrap();
        assert!(merged.starts_with("# Hermes settings\nmodel:\n"));
        assert!(merged.contains("  provider: \"custom:lamp\"\n"));
        assert!(merged.contains("  default: \"deepseek-flash\"\n"));
        assert!(merged.contains("  context_length: 200000\n"), "kept");
        assert!(
            !merged.contains("openrouter.ai"),
            "the old provider's base is gone"
        );
        assert!(merged.contains("  cwd: /var/lib/lampos-tasks/harnesses\n"));
        assert!(merged.contains("  timeout: 60\n  cron_mode: deny\n  mode: \"manual\"\n"));
        assert!(merged.contains("- name: other\n  base_url: https://example.invalid/v1\n"));
        assert!(merged.contains(
            "providers:\n  lamp:\n    name: \"lamp\"\n    \
             base_url: \"http://127.0.0.1:7741/v1/deepseek\"\n    \
             key_env: \"LAMP_HERMES_TOKEN\"\n    api_mode: \"chat_completions\"\n    \
             default_model: \"deepseek-flash\"\n"
        ));
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let once = merge_config(HARNESS, 7741).unwrap();
        assert_eq!(merge_config(&once, 7741).unwrap(), once);
        let empty = merge_config("", 7741).unwrap();
        assert_eq!(merge_config(&empty, 7741).unwrap(), empty);
        let env = merge_env("OTHER=1\n", TOKEN).unwrap();
        assert_eq!(merge_env(&env, TOKEN).unwrap(), env);
    }

    #[test]
    fn a_changed_port_replaces_the_provider_in_place() {
        let once = merge_config(HARNESS, 7741).unwrap();
        let moved = merge_config(&once, 7742).unwrap();
        assert!(moved.contains("127.0.0.1:7742"));
        assert!(!moved.contains("127.0.0.1:7741"));
        assert_eq!(moved.matches("  lamp:\n").count(), 1);
    }

    #[test]
    fn a_reset_left_scalar_takes_the_block() {
        let merged = merge_config("approvals: {}\nmodel: null\nhooks: {}\n", 7741).unwrap();
        assert!(merged.contains("approvals:\n  mode: \"manual\"\n"));
        assert!(merged.contains("model:\n  provider: \"custom:lamp\"\n"));
        assert!(
            merged.contains("hooks: {}\n"),
            "a key not ours is never touched"
        );
    }

    #[test]
    fn other_providers_and_their_indent_are_kept() {
        let text = "providers:\n    mine:\n        base_url: https://example.invalid\n";
        let merged = merge_config(text, 7741).unwrap();
        assert!(merged.contains("    mine:\n        base_url: https://example.invalid\n"));
        assert!(merged.contains("    lamp:\n      name: \"lamp\"\n"));
        assert_eq!(merge_config(&merged, 7741).unwrap(), merged);
    }

    #[test]
    fn what_cannot_be_merged_safely_is_refused() {
        for text in [
            "model:\n\tprovider: x\n",
            "---\nmodel: {}\n",
            "- a\n- b\n",
            "model: {}\nmodel: {}\n",
            "approvals:\n- off\n",
            "model: &base\n",
        ] {
            assert!(merge_config(text, 7741).is_err(), "{text:?}");
        }
    }

    #[test]
    fn the_env_carries_the_local_token_and_nothing_of_the_relay() {
        let env = merge_env("TELEGRAM_BOT_TOKEN=x\nLAMP_HERMES_TOKEN=old\n", TOKEN).unwrap();
        assert_eq!(
            env,
            format!("TELEGRAM_BOT_TOKEN=x\nLAMP_HERMES_TOKEN={TOKEN}\n")
        );
        assert!(merge_env("", "short").is_none());
        assert!(merge_env("", &format!("{TOKEN}\nAUTONOMOUS_API_KEY=x")).is_none());
    }

    #[test]
    fn written_files_hold_no_relay_key_and_a_second_write_is_idle() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("config.yaml"), HARNESS).unwrap();
        let first = write(home.path(), 7741, TOKEN).unwrap();
        assert!(first.config_changed && first.env_changed);
        let again = write(home.path(), 7741, TOKEN).unwrap();
        assert!(!again.config_changed && !again.env_changed);
        let config = fs::read_to_string(home.path().join("config.yaml")).unwrap();
        let env = fs::read_to_string(home.path().join(".env")).unwrap();
        for text in [&config, &env] {
            assert!(!text.contains("campaign-api"), "no relay address");
            assert!(!text.contains("AUTONOMOUS_API_KEY"), "no relay key");
        }
        assert_eq!(hooks_block(&config), hooks_block(HARNESS));
        for name in ["config.yaml", ".env"] {
            let mode = fs::metadata(home.path().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }

    #[test]
    fn the_env_gains_only_the_token_and_loses_only_provider_keys() {
        let before = "\
# Hermes' own
TELEGRAM_BOT_TOKEN=x
DEEPSEEK_API_KEY=a
export OPENAI_API_KEY=b
ANTHROPIC_API_KEY=c
AUTONOMOUS_API_KEY=d
OPENROUTER_API_KEY=e
API_SERVER_KEY=kept

LAMP_HERMES_TOKEN=old
";
        let merged = merge_env(before, TOKEN).unwrap();
        let kept: Vec<&str> = before
            .lines()
            .filter(|line| {
                let name = line
                    .trim_start_matches("export ")
                    .split('=')
                    .next()
                    .unwrap();
                !(line.contains('=') && (name == TOKEN_ENV || DROPPED_KEYS.contains(&name)))
            })
            .collect();
        let token_line = format!("{TOKEN_ENV}={TOKEN}");
        let mut want = kept.clone();
        want.push(&token_line);
        assert_eq!(merged.lines().collect::<Vec<_>>(), want);
        for line in merged.lines() {
            assert!(
                line == token_line || before.lines().any(|old| old == line),
                "a line that was not there: {line}"
            );
        }
        for name in DROPPED_KEYS {
            assert!(!merged.contains(name), "{name}");
        }
    }

    #[test]
    fn links_and_other_kinds_of_file_are_refused() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("config.yaml");
        fs::write(&target, "model: {}\n").unwrap();
        std::os::unix::fs::symlink(&target, home.path().join("config.yaml")).unwrap();
        assert!(write(home.path(), 7741, TOKEN).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "model: {}\n");
        assert!(!home.path().join(".env").exists());
        let other = tempfile::tempdir().unwrap();
        fs::create_dir(other.path().join(".env")).unwrap();
        assert!(write(other.path(), 7741, TOKEN).is_err());
        assert!(!other.path().join("config.yaml").exists());
    }

    #[test]
    fn a_refused_config_is_left_untouched() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("config.yaml"), "- not a mapping\n").unwrap();
        assert!(write(home.path(), 7741, TOKEN).is_err());
        assert_eq!(
            fs::read_to_string(home.path().join("config.yaml")).unwrap(),
            "- not a mapping\n"
        );
        assert!(!home.path().join(".env").exists());
    }

    const SKILLS: &str = "skills:\n  external_dirs:\n  - \"/var/lib/lampOS/skills\"\n";

    #[test]
    fn the_lamps_skills_dir_is_added_once_and_hooks_are_kept() {
        let merged = merge_config(HARNESS, 7741).unwrap();
        assert!(merged.ends_with(SKILLS), "{merged}");
        assert_eq!(hooks_block(&merged), hooks_block(HARNESS));
        assert_eq!(merge_config(&merged, 7741).unwrap(), merged);
        assert_eq!(merged.matches("/var/lib/lampOS/skills").count(), 1);
        let reset = merge_config("skills: {}\n", 7741).unwrap();
        assert!(reset.starts_with(SKILLS), "{reset}");
    }

    #[test]
    fn a_persons_own_skills_dirs_and_keys_are_kept() {
        let text = "\
skills:
  disabled:
  - old
  external_dirs:
  - ~/my-skills   # mine
  - '/srv/team skills'
  creation_nudge_interval: 5
";
        let merged = merge_config(text, 7741).unwrap();
        assert!(
            merged.starts_with(
                "skills:\n  disabled:\n  - old\n  external_dirs:\n  - ~/my-skills   # mine\n  \
                 - '/srv/team skills'\n  - \"/var/lib/lampOS/skills\"\n  creation_nudge_interval: 5\n"
            ),
            "{merged}"
        );
        assert_eq!(merge_config(&merged, 7741).unwrap(), merged);
        let deeper = "skills:\n    external_dirs:\n        - /a\n";
        let merged = merge_config(deeper, 7741).unwrap();
        assert!(
            merged.starts_with(
                "skills:\n    external_dirs:\n        - /a\n        - \"/var/lib/lampOS/skills\"\n"
            ),
            "{merged}"
        );
    }

    #[test]
    fn the_dir_already_there_in_any_spelling_is_left_as_it_is() {
        for text in [
            "skills:\n  external_dirs:\n  - /var/lib/lampOS/skills/\n",
            "skills:\n  external_dirs:\n  - '/var/lib/lampOS/skills'\n",
            "skills:\n  external_dirs: [/x, /var/lib/lampOS/skills]\n",
            "skills:\n  external_dirs: /var/lib/lampOS/skills\n",
        ] {
            let merged = merge_config(text, 7741).unwrap();
            assert!(merged.starts_with(text), "{merged}");
            assert_eq!(merged.matches("lampOS/skills").count(), 1, "{merged}");
        }
    }

    #[test]
    fn a_flow_list_a_single_path_or_a_reset_becomes_a_block_list() {
        for (text, want) in [
            (
                "skills:\n  external_dirs: [/a, ~/b]\n",
                "skills:\n  external_dirs:\n  - /a\n  - ~/b\n  - \"/var/lib/lampOS/skills\"\n",
            ),
            (
                "skills:\n  external_dirs: /a\n",
                "skills:\n  external_dirs:\n  - /a\n  - \"/var/lib/lampOS/skills\"\n",
            ),
            ("skills:\n  external_dirs: []\n", SKILLS),
            ("skills:\n  external_dirs: null\n", SKILLS),
            ("skills:\n  external_dirs:\n", SKILLS),
            ("skills:\n", SKILLS),
        ] {
            let merged = merge_config(text, 7741).unwrap();
            assert!(merged.starts_with(want), "{text:?} gave {merged:?}");
            assert_eq!(merge_config(&merged, 7741).unwrap(), merged);
        }
    }

    #[test]
    fn skills_that_cannot_be_merged_safely_are_refused() {
        for text in [
            "skills: [a]\n",
            "skills: {external_dirs: [/a]}\n",
            "skills:\n  external_dirs: &dirs\n",
            "skills:\n  external_dirs: [[/a]]\n",
            "skills:\n  external_dirs: [\"/a, b\"]\n",
            "skills:\n  external_dirs:\n  - path: /a\n",
            "skills:\n  external_dirs:\n  - /a\n    more: x\n",
            "skills:\n  external_dirs: |\n    /a\n",
            "skills:\n  external_dirs: []\n  external_dirs: []\n",
            "skills:\n- /a\n",
        ] {
            assert!(merge_config(text, 7741).is_err(), "{text:?}");
        }
    }

    #[test]
    fn the_guidance_is_written_once_never_through_a_link() {
        let folder = tempfile::tempdir().unwrap();
        assert!(write_guidance(folder.path()).unwrap());
        let text = fs::read_to_string(folder.path().join(GUIDANCE_FILE)).unwrap();
        assert_eq!(text, guidance());
        assert!(!write_guidance(folder.path()).unwrap(), "unchanged");
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("AGENTS.md");
        fs::write(&target, "theirs\n").unwrap();
        let linked = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&target, linked.path().join(GUIDANCE_FILE)).unwrap();
        assert!(write_guidance(linked.path()).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "theirs\n");
        let parent = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(folder.path(), parent.path().join("lamp")).unwrap();
        assert!(write_guidance(&parent.path().join("lamp")).is_err());
    }

    #[test]
    fn the_guidance_says_where_the_skills_are_how_to_run_one_and_the_rules() {
        let text = guidance();
        for words in [
            "`/var/lib/lampOS/skills/<name>/`",
            "/opt/lampOS/tasks/current/lamp-tasks skill list",
            "/opt/lampOS/tasks/current/lamp-tasks skill call NAME '{\"choice\":\"value\"}'",
            "never edit, move or copy files there",
            "do not\nretry",
            "Never move, light up or\nplay sounds on your own.",
            "Installing it on the\nlamp is the person's step",
        ] {
            assert!(
                text.replace('\n', " ").contains(&words.replace('\n', " ")),
                "{words}"
            );
        }
        assert!(!text.contains("~/.hermes/skills/<"), "never a copy");
    }

    /// The lines under `terminal.shell_init_files` in `text`, trimmed.
    fn init_files(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let Some(at) = lines
            .iter()
            .position(|line| line.trim() == "shell_init_files:")
        else {
            return Vec::new();
        };
        lines[at + 1..]
            .iter()
            .take_while(|line| line.trim_start().starts_with("- "))
            .map(|line| line.trim().to_owned())
            .collect()
    }

    #[test]
    fn the_run_env_goes_after_hermess_defaults_when_the_list_is_made() {
        let quoted = |path: &str| format!("- \"{path}\"");
        let made = [
            SHELL_DEFAULTS[0],
            SHELL_DEFAULTS[1],
            SHELL_DEFAULTS[2],
            RUN_ENV,
        ]
        .map(quoted);
        for text in [
            "",
            HARNESS,
            "terminal: {}\n",
            "terminal:\n  backend: local\n",
            "terminal:\n  shell_init_files: []\n",
            "terminal:\n  auto_source_bashrc: true\n",
        ] {
            let merged = merge_config(text, 7741).unwrap();
            assert_eq!(init_files(&merged), made, "{text:?}");
            assert_eq!(
                merge_config(&merged, 7741).unwrap(),
                merged,
                "twice: {text:?}"
            );
        }
        let kept = merge_config("terminal:\n  backend: local\n", 7741).unwrap();
        assert!(kept.contains("  backend: local\n"), "{kept}");
        assert!(
            merge_config(HARNESS, 7741)
                .unwrap()
                .contains(&hooks_block(HARNESS))
        );
    }

    #[test]
    fn a_persons_list_is_kept_and_the_defaults_stay_off_when_they_turned_them_off() {
        let theirs =
            merge_config("terminal:\n  shell_init_files:\n  - ~/.nvm/nvm.sh\n", 7741).unwrap();
        assert_eq!(
            init_files(&theirs),
            ["- ~/.nvm/nvm.sh".to_owned(), format!("- \"{RUN_ENV}\"")]
        );
        let there = format!("terminal:\n  shell_init_files:\n  - {RUN_ENV}/\n");
        assert_eq!(
            merge_config(&there, 7741).unwrap(),
            merge_config(&there, 7741).unwrap()
        );
        assert_eq!(
            init_files(&merge_config(&there, 7741).unwrap()),
            [format!("- {RUN_ENV}/")],
            "there already"
        );
        for off in ["false", "False", "no", "off", "null", "false # mine"] {
            let text = format!("terminal:\n  auto_source_bashrc: {off}\n");
            let merged = merge_config(&text, 7741).unwrap();
            assert_eq!(init_files(&merged), [format!("- \"{RUN_ENV}\"")], "{off}");
        }
    }

    #[test]
    fn a_terminal_it_cannot_merge_keeps_every_other_lamp_line() {
        for text in [
            "terminal: local\n",
            "terminal: {backend: local}\n",
            "terminal: &t\n  backend: local\n",
            "terminal:\n  shell_init_files: {a: b}\n",
            "terminal:\n  shell_init_files:\n  - [a]\n",
        ] {
            let merged = merge_config(text, 7741).unwrap();
            // The lamp's provider and skills folder are still written; the
            // terminal block is left as it was.
            assert!(merged.contains("provider: \"custom:lamp\""), "{merged}");
            assert!(merged.contains(SKILLS_DIR), "{merged}");
            assert!(merged.starts_with(text), "{text:?}: {merged}");
            assert!(!merged.contains(RUN_ENV), "{text:?}");
        }
    }

    /// The Reviewer-lead's P3s on #6759: a bare `shell_init_files:` (null)
    /// lists nothing, so the defaults go in; `auto_source_bashrc` is read
    /// as Hermes's `bool()` reads PyYAML's value.
    #[test]
    fn a_null_list_gets_the_defaults_and_auto_source_bashrc_reads_as_hermes_does() {
        let made = [
            SHELL_DEFAULTS[0],
            SHELL_DEFAULTS[1],
            SHELL_DEFAULTS[2],
            RUN_ENV,
        ]
        .map(|path| format!("- \"{path}\""));
        for text in [
            "terminal:\n  shell_init_files:\n",
            "terminal:\n  shell_init_files:\n  # none yet\n  backend: local\n",
            "terminal:\n  shell_init_files: null\n",
            "terminal:\n  shell_init_files: ~\n",
        ] {
            let merged = merge_config(text, 7741).unwrap();
            assert_eq!(init_files(&merged), made, "{text:?}: {merged}");
            assert_eq!(merge_config(&merged, 7741).unwrap(), merged);
        }
        assert!(sources_defaults(None));
        for on in ["true", "yes", "On", "'false'", "\"no\"", "1", "'x' # c"] {
            assert!(sources_defaults(Some(on)), "{on}");
        }
        for off in [
            "false",
            "False",
            "NO",
            "off",
            "null",
            "~",
            "",
            "0",
            "false # mine",
            "''",
        ] {
            assert!(!sources_defaults(Some(off)), "{off}");
        }
        let quoted = merge_config("terminal:\n  auto_source_bashrc: 'false'\n", 7741).unwrap();
        assert_eq!(
            init_files(&quoted),
            made,
            "a quoted false is true to Hermes"
        );
    }
}
