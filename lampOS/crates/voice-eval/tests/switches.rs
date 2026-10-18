//! `docs/switches.md`'s counts, worked out from its rows (the first builder,
//! #705, 18 October 07:28). The page keeps no count by hand, so two pull
//! requests that each add a switch never touch the same line; each row says
//! how it is counted, and this fails on a row that doesn't.
//!
//! To see the counts: `cargo test -p lamp-voice-eval --test switches -- --nocapture`.

const PAGE: &str = include_str!("../../../docs/switches.md");

/// The page's four tables, by the start of their headings, and the counts'
/// name for each.
const TABLES: [(&str, &str); 4] = [
    ("## The voice: hearing", "Voice: hearing, turns, ring"),
    (
        "## The voice: tasks",
        "Voice: tasks, memory, schedule, skills, looks",
    ),
    ("## The voice's own flags", "Voice: its own flags"),
    ("## The other parts", "Other parts"),
];

/// What a row's **Counted as** may say, and what each counts as.
const COUNTED: [(&str, &str); 5] = [
    ("on", "On already, or right at its default"),
    ("now", "Proposed to turn on now"),
    ("until", "Off until its named lamp check"),
    ("off", "Stays off (diagnostic, turned down, or not needed)"),
    ("setting", "A setting, not a behaviour"),
];

struct Row {
    table: usize,
    counted: usize,
}

fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// Every switch row of the four tables, each with its table and how it is
/// counted. Panics, naming the line, on a row whose cells don't match its
/// header or whose **Counted as** isn't one of [`COUNTED`].
fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let mut table = None;
    let mut header: Option<Vec<&str>> = None;
    let mut seen = [false; 4];
    for (number, line) in PAGE
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
    {
        if line.starts_with("## ") {
            table = TABLES
                .iter()
                .position(|(heading, _)| line.starts_with(heading));
            header = None;
            continue;
        }
        let Some(table) = table else {
            continue;
        };
        if !line.starts_with('|') {
            continue;
        }
        let line_cells = cells(line);
        let Some(header) = header.as_ref() else {
            assert_eq!(
                line_cells.last(),
                Some(&"Counted as"),
                "switches.md:{number}: the table's last column is Counted as"
            );
            header = Some(line_cells);
            seen[table] = true;
            continue;
        };
        if line.starts_with("|---") {
            continue;
        }
        assert_eq!(
            line_cells.len(),
            header.len(),
            "switches.md:{number}: {} has {} cells, its table {}",
            line_cells[0],
            line_cells.len(),
            header.len()
        );
        let said = line_cells[line_cells.len() - 1];
        let counted = COUNTED
            .iter()
            .position(|(word, _)| *word == said)
            .unwrap_or_else(|| {
                panic!(
                    "switches.md:{number}: {} is counted as {said:?}, not one of {:?}",
                    line_cells[0],
                    COUNTED.map(|(word, _)| word)
                )
            });
        rows.push(Row { table, counted });
    }
    for (index, (heading, _)) in TABLES.iter().enumerate() {
        assert!(seen[index], "switches.md: no table under {heading:?}");
    }
    rows
}

#[test]
fn every_switch_says_how_it_is_counted() {
    let rows = rows();
    for (index, (heading, _)) in TABLES.iter().enumerate() {
        assert!(
            rows.iter().any(|row| row.table == index),
            "switches.md: the table under {heading:?} has no switches"
        );
    }
}

#[test]
fn the_counts_by_table_and_in_all() {
    let rows = rows();
    let count = |table: Option<usize>, counted: usize| {
        rows.iter()
            .filter(|row| row.counted == counted && table.is_none_or(|table| row.table == table))
            .count()
    };
    println!("{} switches:\n", rows.len());
    let names: Vec<&str> = TABLES.iter().map(|(_, name)| *name).collect();
    println!("| | {} | All |", names.join(" | "));
    println!("|---|{}---|", "---|".repeat(TABLES.len()));
    for (counted, (_, meaning)) in COUNTED.iter().enumerate() {
        let by_table: Vec<String> = (0..TABLES.len())
            .map(|table| count(Some(table), counted).to_string())
            .collect();
        println!(
            "| {meaning} | {} | **{}** |",
            by_table.join(" | "),
            count(None, counted)
        );
    }
    let total: usize = (0..COUNTED.len()).map(|counted| count(None, counted)).sum();
    assert_eq!(total, rows.len());
}
