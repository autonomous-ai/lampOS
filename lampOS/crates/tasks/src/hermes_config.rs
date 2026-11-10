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
//! - `.env`'s [`TOKEN_ENV`] line: the forwarder's local token, never the
//!   relay's key.
//!
//! Every other line is kept as it was, Harness's `hooks:` block above all
//! (openharness `engines/hermes/contract.ts`). The workspace has no YAML
//! crate, so the merge is line-based over block-style YAML, the style Hermes
//! and Harness write; a file it can't merge safely (tabs, several
//! documents, a flow-style or sequence value where a mapping is set) is
//! refused and left as it is, never rewritten.
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The custom provider's name in `providers:`.
pub const PROVIDER: &str = "lamp";
/// The model asked: the voice's DeepSeek default (`second_config.rs`'s
/// `DEEPSEEK_MODEL`).
pub const MODEL: &str = "deepseek-flash";
/// Where Hermes' key for `lamp` is read: `.env`'s line, the local token.
pub const TOKEN_ENV: &str = "LAMP_HERMES_TOKEN";
/// Hermes' approval mode that asks before a dangerous command.
pub const APPROVALS: &str = "manual";

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
    let body = &part.lines[1..];
    let child_indent = body
        .iter()
        .find(|line| !is_blank_or_comment(line))
        .map(|line| indent(line))
        .unwrap_or(2);
    if child_indent == 0 {
        return Err(Refused(format!("`{top}` has nothing indented under it")));
    }
    // Where each child of the mapping starts, in `part.lines`.
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
                return Err(Refused(format!("`{top}` is a list, not a mapping")));
            }
            let Some(key) = key_of(line) else {
                return Err(Refused(format!("`{top}` has a line that is not a key")));
            };
            starts.push((index, key));
        }
    }
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
            let begin = starts[position].0;
            // The child runs to the next child, or to the mapping's last
            // non-blank line: a blank or comment after it stays.
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
/// `port`. The same text given twice gives the same text back.
pub fn merge_config(text: &str, port: u16) -> Result<String, Refused> {
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
    let mut out = String::new();
    for part in &parts {
        for line in &part.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// `.env`'s text with [`TOKEN_ENV`] set to `token` and every other line
/// kept. `None` for a token that isn't [`token_is_plain`].
pub fn merge_env(text: &str, token: &str) -> Option<String> {
    if !token_is_plain(token) {
        return None;
    }
    let mut out = String::new();
    let prefix = format!("{TOKEN_ENV}=");
    let exported = format!("export {TOKEN_ENV}=");
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) || trimmed.starts_with(&exported) {
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

fn read_or_empty(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// `text` in place of `path`, 0600, through a new file beside it renamed
/// over it, so a reader never sees half a file.
fn replace(path: &Path, text: &str) -> io::Result<()> {
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
    let config_path = hermes_home.join("config.yaml");
    let env_path = hermes_home.join(".env");
    let config = read_or_empty(&config_path)?;
    let env = read_or_empty(&env_path)?;
    let merged_config = merge_config(&config, port).map_err(io::Error::other)?;
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
}
