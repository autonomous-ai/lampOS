//! The stable notes' "Built but not on yet" part (#3976, the first builder's
//! 20:59 on 29 Oct): each `switches.md` row counted `until` or `now` whose
//! switch no product unit turns on, in its row's own words, so nobody
//! expects a feature the lamp doesn't run. Read from the code: the product
//! units' `Environment=` lines, `ExecStart` flags and settings files, never
//! the row's own "Product unit" cell. The notes are also kept on the
//! build's own release as an asset, `NOTES.md`; its one-line body is the
//! provenance the promotion reads and is never touched. Never a gate.
use std::{collections::BTreeMap, fs, io, path::Path, process::Command};

pub const HEADING: &str = "### Built but not on yet";
/// The part after it, which the developers step owns.
pub const DEVELOPERS: &str = "### For developers";
/// The build's own copy of the notes, an asset beside its files.
pub const ASSET: &str = "NOTES.md";
/// The page's table of what a person would notice with each switch on, one
/// plain line per switch, which the list prints (the first builder on
/// #3976, 29 Oct 23:4x: the rows' own words are for developers).
pub const PLAIN: &str = "## In plain words";
/// The longest plain line.
pub const PLAIN_MOST: usize = 110;
/// The headings of the page's four tables of switches, and the name each
/// is listed under.
const TABLES: [(&str, &str); 4] = [
    (
        "## The voice: hearing",
        "The voice: hearing, turns and the ring",
    ),
    (
        "## The voice: tasks",
        "The voice: tasks, memory, schedule, skills and looks",
    ),
    ("## The voice's own flags", "The voice's own flags"),
    ("## The other parts", "The other parts"),
];
/// How a row is counted when it is a feature waiting for its check.
const WAITING: [&str; 2] = ["until", "now"];
/// A "Product unit" cell for a switch on by its code's default, which no
/// product file sets: the one place the notes take the row's cell, since no
/// file states that default (Second on #3976, 30 Oct 02:0x, for
/// `LAMP_FORGET_ASKED`). Exactly this, so another wording isn't taken.
pub const ON_UNSET: &str = "on (unset)";
/// The column of a switch table that says what the product unit does.
const PRODUCT_COLUMN: &str = "Product unit";
/// The column of a switch table that says when the product should take it.
const PROPOSED_COLUMN: &str = "Proposed product default";
/// A value that leaves a switch off.
const OFF: [&str; 5] = ["", "off", "0", "false", "no"];
/// One sentence of a row's words at most this long.
const MOST_WORDS: usize = 140;

/// A switch that is built but not on.
#[derive(Debug, PartialEq, Eq)]
pub struct Waiting {
    /// The name of its table.
    pub part: &'static str,
    pub switch: String,
    pub words: String,
    /// Its line in [`PLAIN`], if the page has one.
    pub plain: Option<String>,
    /// Its row's "Product unit" cell, if its table has one.
    pub product: Option<String>,
}

fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// The switch a row names: its first `LAMP_…` name or `--flag`, without
/// backticks.
fn switch_of(cell: &str) -> Option<String> {
    cell.split(|c: char| c.is_whitespace() || c == '/' || c == ',')
        .map(|word| word.trim_matches('`'))
        .find(|word| {
            (word.starts_with("LAMP_") || word.starts_with("--"))
                && word.len() > 2
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .map(str::to_owned)
}

/// A row's words: its "Values and default" cell after the values, to the
/// end of that sentence.
fn words_of(cell: &str) -> String {
    let after = cell.split_once(". ").map_or(cell, |(_, rest)| rest);
    let sentence = after.split(". ").next().unwrap_or(after).trim();
    let sentence = sentence.trim_end_matches('.');
    if sentence.chars().count() <= MOST_WORDS {
        return format!("{sentence}.");
    }
    // At the last whole word, and before a code span the cut would leave
    // open (Third's P3 on #4400).
    let cut: String = sentence.chars().take(MOST_WORDS).collect();
    let mut cut = cut
        .rsplit_once(' ')
        .map_or(cut.as_str(), |(whole, _)| whole);
    if cut.matches('`').count() % 2 == 1
        && let Some((before, _)) = cut.rsplit_once('`')
    {
        cut = before;
    }
    let cut = cut.trim_end_matches(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ';');
    format!("{cut}…")
}

/// Each switch's line in the page's [`PLAIN`] table.
pub fn plain_lines(page: &str) -> BTreeMap<String, String> {
    let mut lines = BTreeMap::new();
    let mut inside = false;
    for line in page.lines() {
        if line.starts_with("## ") {
            inside = line.trim() == PLAIN;
            continue;
        }
        if !inside || !line.starts_with('|') {
            continue;
        }
        let row = cells(line);
        if row.len() < 2 || row[0].starts_with("---") {
            continue;
        }
        if let Some(switch) = switch_of(row[0])
            && !row[1].is_empty()
        {
            lines.insert(switch, row[1].to_owned());
        }
    }
    lines
}

/// What the product units and settings turn on: each `NAME=value` of an
/// `Environment=` line or a settings `.env` file with a value that isn't
/// off, and each `--flag` of an `ExecStart` line.
pub fn turned_on(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut on = BTreeMap::new();
    for (name, text) in files {
        let env = name.ends_with(".env");
        for line in text.lines().map(str::trim) {
            if line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("ExecStart=") {
                for word in rest.split_whitespace().filter(|w| w.starts_with("--")) {
                    on.insert(word.to_owned(), name.clone());
                }
                continue;
            }
            let assignments = match line.strip_prefix("Environment=") {
                Some(rest) => rest,
                None if env => line,
                None => continue,
            };
            for assignment in assignments.split_whitespace() {
                let assignment = assignment.trim_matches('"');
                if let Some((key, value)) = assignment.split_once('=')
                    && !OFF.contains(&value.trim_matches('"'))
                {
                    on.insert(key.to_owned(), name.clone());
                }
            }
        }
    }
    on
}

/// One row of the page's switch tables that names a switch.
pub struct Row {
    /// The name of its table.
    pub part: &'static str,
    pub switch: String,
    /// Its **Counted as**: `on`, `now`, `until`, `off` or `setting`.
    pub counted: String,
    /// Its "Values and default" cell.
    values: String,
    /// Its "Product unit" cell, if its table has one.
    pub product: Option<String>,
    /// Its "Proposed product default" cell, if its table has one.
    pub proposed: Option<String>,
}

/// Every row of the page's switch tables that names a switch, in the
/// page's order.
pub fn rows(page: &str) -> Vec<Row> {
    let mut found = Vec::new();
    let mut part = None;
    let mut header = false;
    let mut product_at = None;
    let mut proposed_at = None;
    for line in page.lines() {
        if line.starts_with("## ") {
            part = TABLES
                .iter()
                .find(|(heading, _)| line.starts_with(heading))
                .map(|(_, name)| *name);
            header = false;
            product_at = None;
            proposed_at = None;
            continue;
        }
        let Some(part) = part else {
            continue;
        };
        if !line.starts_with('|') {
            continue;
        }
        if !header {
            header = true;
            product_at = cells(line).iter().position(|cell| *cell == PRODUCT_COLUMN);
            proposed_at = cells(line).iter().position(|cell| *cell == PROPOSED_COLUMN);
            continue;
        }
        let row = cells(line);
        if row.len() < 3 || row[0].starts_with("---") {
            continue;
        }
        let Some(switch) = switch_of(row[0]) else {
            continue;
        };
        found.push(Row {
            part,
            switch,
            counted: row.last().copied().unwrap_or_default().to_owned(),
            values: row[1].to_owned(),
            product: product_at
                .and_then(|at| row.get(at))
                .map(|cell| (*cell).to_owned()),
            proposed: proposed_at
                .and_then(|at| row.get(at))
                .map(|cell| (*cell).to_owned()),
        });
    }
    found
}

/// Every row of the page's switch tables counted `until` or `now` whose
/// switch isn't in `on`, in the page's order, each switch once. A row whose
/// "Product unit" cell is exactly [`ON_UNSET`] is on by its code's default,
/// and is not listed.
pub fn waiting(page: &str, on: &BTreeMap<String, String>) -> Vec<Waiting> {
    let plain = plain_lines(page);
    let mut found: Vec<Waiting> = Vec::new();
    for row in rows(page) {
        if !WAITING.contains(&row.counted.as_str())
            || on.contains_key(&row.switch)
            || row.product.as_deref() == Some(ON_UNSET)
            || found.iter().any(|w| w.switch == row.switch)
        {
            continue;
        }
        found.push(Waiting {
            part: row.part,
            plain: plain.get(&row.switch).cloned(),
            words: words_of(&row.values),
            product: row.product,
            switch: row.switch,
        });
    }
    found
}

/// The part, from its heading to its last line: how many, then the list by
/// table in plain words, folded, since it runs to many lines, and then each
/// switch's name and its row's words, folded, for developers. A switch with
/// no plain line is listed in its row's words.
pub fn section(waiting: &[Waiting]) -> String {
    let mut lines = vec![HEADING.to_owned()];
    if waiting.is_empty() {
        lines.push("Nothing: every feature waiting for a lamp check is on.".to_owned());
        return lines.join("\n") + "\n";
    }
    lines.push(format!(
        "{} features are in this build but off on every lamp, each until its own \
         check on a lamp passes (docs/switches.md). Don't expect them yet.",
        waiting.len()
    ));
    lines.push(String::new());
    lines.push(format!(
        "<details><summary>The {}, by part</summary>",
        waiting.len()
    ));
    by_part(&mut lines, waiting, |w| {
        format!("- {}", w.plain.as_deref().unwrap_or(&w.words))
    });
    lines.push(String::new());
    lines.push("</details>".to_owned());
    lines.push(String::new());
    lines.push(
        "<details><summary>For developers: each switch and its row's words \
         (docs/switches.md)</summary>"
            .to_owned(),
    );
    by_part(&mut lines, waiting, |w| {
        format!("- `{}`: {}", w.switch, w.words)
    });
    lines.push(String::new());
    lines.push("</details>".to_owned());
    lines.join("\n") + "\n"
}

/// `waiting`'s lines under their tables' names, in order.
fn by_part(lines: &mut Vec<String>, waiting: &[Waiting], line: impl Fn(&Waiting) -> String) {
    let mut part = None;
    for w in waiting {
        if part != Some(w.part) {
            lines.push(String::new());
            lines.push(format!("**{}**", w.part));
            part = Some(w.part);
        }
        lines.push(line(w));
    }
}

/// `body` with its part replaced by `part`: in place when it is there, else
/// before the developers part, else at the end. Nothing else changes.
pub fn placed(body: &str, part: &str) -> String {
    let body = body.trim_end_matches('\n');
    if let Some(start) = body.find(HEADING) {
        let after = &body[start + HEADING.len()..];
        let end = after
            .find("\n### ")
            .map_or(body.len(), |at| start + HEADING.len() + at + 1);
        let rest = body[end..].trim_start_matches('\n');
        let kept = body[..start].trim_end_matches('\n');
        let mut new = format!("{kept}\n\n{part}");
        if !rest.is_empty() {
            new.push('\n');
            new.push_str(rest);
            new.push('\n');
        }
        return new;
    }
    if let Some(start) = body.find(&format!("\n{DEVELOPERS}")) {
        let kept = body[..start].trim_end_matches('\n');
        let rest = &body[start + 1..];
        return format!("{kept}\n\n{part}\n{rest}\n");
    }
    if body.is_empty() {
        return part.to_owned();
    }
    format!("{body}\n\n{part}")
}

/// The product's files a switch can be turned on in: every unit and
/// drop-in in `deploy/product` that `lamp-update install` places, and its
/// settings' `.env` files. A file there that install never places is on no
/// lamp, so nothing in it is on: `own-keys-voice.conf` goes on a test lamp
/// by hand, and the keeper's restart and the own keys' helpers wait for
/// their lamp checks (install's left-out list).
pub fn product_files(lamp_os: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let product = lamp_os.join("deploy/product");
    for folder in [product.clone(), product.join("settings")] {
        for entry in fs::read_dir(&folder)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            let unit = [".service", ".conf", ".socket", ".timer"]
                .iter()
                .any(|end| name.ends_with(end));
            if path.is_file() && (unit || name.ends_with(".env")) {
                let text = fs::read_to_string(&path)?;
                if installed(&name, &text) {
                    files.insert(name, text);
                }
            }
        }
    }
    Ok(files)
}

/// Whether `lamp-update install` places this file, as this build carries
/// it: its name and its text both (Second's P3 on #4666), so a hand drop-in
/// that happens to hold an installed file's text is not taken for it. A
/// unit by its name; a setting, the exit note and the tmpfiles conf by the
/// last part of the path install writes them to.
fn installed(name: &str, text: &str) -> bool {
    use lamp_update::install::{
        EXIT_NOTE, PRODUCT, SETTINGS, STORE_SETTINGS, TMPFILES, TMPFILES_PATH,
    };
    let file = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
    PRODUCT
        .iter()
        .map(|(unit, placed)| ((*unit).to_owned(), *placed))
        .chain(
            SETTINGS
                .iter()
                .chain(STORE_SETTINGS)
                .map(|(path, placed)| (file(path), *placed)),
        )
        .chain([
            ("exit-note.conf".to_owned(), EXIT_NOTE),
            (file(TMPFILES_PATH), TMPFILES),
        ])
        .any(|(placed_name, placed)| placed_name == name && placed == text)
}

/// One `gh` call's standard output, or why not.
fn gh(arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("gh")
        .args(arguments)
        .env("GH_HOST", "github.com")
        .env_remove("LAMPOS_UPDATE_KEY")
        .output()
        .map_err(|_| "cannot start GitHub CLI".to_owned())?;
    if !output.status.success() {
        let first = String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("no stderr")
            .chars()
            .take(200)
            .collect::<String>();
        return Err(format!("gh {} failed: {first}", arguments[0]));
    }
    String::from_utf8(output.stdout).map_err(|_| "GitHub CLI said non-UTF-8".to_owned())
}

/// Fills the stable release's part and keeps the notes on build
/// `sequence`'s own release as [`ASSET`]. What it did, for the log.
pub fn fill(lamp_os: &Path, sequence: u64, scratch: &Path) -> Result<String, String> {
    let page = fs::read_to_string(lamp_os.join("docs/switches.md"))
        .map_err(|_| "docs/switches.md cannot be read".to_owned())?;
    let files = product_files(lamp_os).map_err(|_| "the product units cannot be read")?;
    let waiting = waiting(&page, &turned_on(&files));
    let repo = crate::REPO;
    let body = gh(&[
        "release",
        "view",
        crate::STABLE,
        "--repo",
        repo,
        "--json",
        "body",
        "--jq",
        ".body",
    ])?;
    let header = format!("## lampOS build {sequence}");
    if body.lines().next() != Some(header.as_str()) {
        return Err(format!(
            "the stable notes are not build {sequence}'s yet: nothing written"
        ));
    }
    let notes = placed(&body, &section(&waiting));
    let file = scratch.join(ASSET);
    fs::write(&file, &notes).map_err(|_| "the notes cannot be staged".to_owned())?;
    let path = file.to_str().ok_or("the notes' path is not UTF-8")?;
    gh(&[
        "release",
        "edit",
        crate::STABLE,
        "--repo",
        repo,
        "--notes-file",
        path,
    ])?;
    let tag = format!("lampos-lamp-voice-{sequence}");
    let kept = gh(&["release", "upload", &tag, path, "--repo", repo, "--clobber"]).map_or_else(
        |why| format!("not kept on {tag}: {why}"),
        |_| format!("kept on {tag} as {ASSET}"),
    );
    Ok(format!(
        "{} switches built but not on; the stable notes written; {kept}",
        waiting.len()
    ))
}
