//! The public skill format, version 1 (`docs/skill-format.md`): every skill
//! folder in the repo validates against `docs/skill-format.schema.json`, the
//! schema refuses what lamp-skills refuses in shape, and its lists stay in
//! step with the code's.
//!
//! The validator here takes only the JSON Schema keywords the schema uses,
//! and refuses any other, so the schema can't use one this test would
//! quietly skip.
use serde_json::Value;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn schema() -> Value {
    let text = std::fs::read_to_string(repo().join("docs/skill-format.schema.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Every folder with a `skill.toml`: `skills/*` and `examples/*`.
fn skill_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in ["skills", "examples"] {
        for entry in std::fs::read_dir(repo().join(root)).unwrap() {
            let path = entry.unwrap().path().join("skill.toml");
            if path.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn as_json(toml_text: &str) -> Value {
    let value: toml::Value = toml::from_str(toml_text).unwrap();
    serde_json::to_value(value).unwrap()
}

const KEYWORDS: [&str; 24] = [
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "enum",
    "const",
    "properties",
    "additionalProperties",
    "required",
    "items",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "minProperties",
    "oneOf",
];

/// Why `value` at `at` doesn't fit `schema`, each reason with its path.
fn errors(root: &Value, schema: &Value, value: &Value, at: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rules) = schema.as_object() else {
        panic!("{at}: a schema that is not an object");
    };
    for key in rules.keys() {
        assert!(
            KEYWORDS.contains(&key.as_str()),
            "the schema uses {key}, which this test can't check"
        );
    }
    if let Some(reference) = rules.get("$ref").and_then(Value::as_str) {
        let name = reference.strip_prefix("#/$defs/").expect("a local $ref");
        out.extend(errors(root, &root["$defs"][name], value, at));
    }
    if let Some(kind) = rules.get("type").and_then(Value::as_str) {
        let fits = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            other => panic!("type {other}"),
        };
        if !fits {
            out.push(format!("{at}: not a {kind}"));
            return out;
        }
    }
    if let Some(choices) = rules.get("enum").and_then(Value::as_array)
        && !choices.contains(value)
    {
        out.push(format!("{at}: {value} is not one of {choices:?}"));
    }
    if let Some(only) = rules.get("const")
        && only != value
    {
        out.push(format!("{at}: {value} is not {only}"));
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if rules
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|least| length < least)
        {
            out.push(format!("{at}: shorter than its least"));
        }
        if rules
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|most| length > most)
        {
            out.push(format!("{at}: longer than its most"));
        }
        if let Some(pattern) = rules.get("pattern").and_then(Value::as_str)
            && !regex::Regex::new(pattern).unwrap().is_match(text)
        {
            out.push(format!("{at}: {text:?} doesn't match {pattern}"));
        }
    }
    if let Some(number) = value.as_f64() {
        if rules
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|least| number < least)
        {
            out.push(format!("{at}: {number} under its minimum"));
        }
        if rules
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|most| number > most)
        {
            out.push(format!("{at}: {number} over its maximum"));
        }
        if rules
            .get("exclusiveMinimum")
            .and_then(Value::as_f64)
            .is_some_and(|least| number <= least)
        {
            out.push(format!(
                "{at}: {number} not over {least:?}",
                least = rules["exclusiveMinimum"]
            ));
        }
    }
    if let Some(list) = value.as_array() {
        let count = list.len() as u64;
        if rules
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|least| count < least)
        {
            out.push(format!("{at}: fewer than its least items"));
        }
        if rules
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|most| count > most)
        {
            out.push(format!("{at}: more than its most items"));
        }
        if rules.get("uniqueItems") == Some(&Value::Bool(true)) {
            for (index, item) in list.iter().enumerate() {
                if list[..index].contains(item) {
                    out.push(format!("{at}: {item} twice"));
                }
            }
        }
        if let Some(items) = rules.get("items") {
            for (index, item) in list.iter().enumerate() {
                out.extend(errors(root, items, item, &format!("{at}[{index}]")));
            }
        }
    }
    if let Some(table) = value.as_object() {
        if rules
            .get("minProperties")
            .and_then(Value::as_u64)
            .is_some_and(|least| (table.len() as u64) < least)
        {
            out.push(format!("{at}: fewer keys than its least"));
        }
        for needed in rules
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let needed = needed.as_str().unwrap();
            if !table.contains_key(needed) {
                out.push(format!("{at}: {needed} is missing"));
            }
        }
        let known = rules.get("properties").and_then(Value::as_object);
        for (key, item) in table {
            let path = format!("{at}.{key}");
            match (
                known.and_then(|known| known.get(key)),
                rules.get("additionalProperties"),
            ) {
                (Some(rule), _) => out.extend(errors(root, rule, item, &path)),
                (None, Some(Value::Bool(false))) => {
                    out.push(format!("{path}: not a key of the format"))
                }
                (None, Some(rule @ Value::Object(_))) => {
                    out.extend(errors(root, rule, item, &path))
                }
                (None, _) => {}
            }
        }
    }
    if let Some(ways) = rules.get("oneOf").and_then(Value::as_array) {
        let fitting = ways
            .iter()
            .filter(|way| errors(root, way, value, at).is_empty())
            .count();
        if fitting != 1 {
            out.push(format!(
                "{at}: fits {fitting} of its {} forms, not one",
                ways.len()
            ));
        }
    }
    out
}

fn check(document: &Value) -> Vec<String> {
    let schema = schema();
    errors(&schema, &schema, document, "skill.toml")
}

/// Every skill in the repo is a format 1 skill.toml.
#[test]
fn every_skill_folder_in_the_repo_validates_against_format_1() {
    let files = skill_files();
    assert!(files.len() >= 30, "found only {}", files.len());
    let mut failed = Vec::new();
    for file in &files {
        let document = as_json(&std::fs::read_to_string(file).unwrap());
        for error in check(&document) {
            failed.push(format!("{}: {error}", file.display()));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// A skill that loads: each case below changes one thing of it.
const GOOD: &str = r#"
version = "1.0.0"
[permissions]
light = true
body = true
topics = ["humans/faces/tracked"]
[trigger]
topic = "humans/faces/tracked"
when = "appears"
[ask]
policy = "nudge"
[[do]]
light = { colour = "warm", level = 2, for = "20s" }
"#;

/// `toml_text` written as the skill `format-case` in a folder of its own,
/// and what `lamp_skills::load` says of it.
fn loads(toml_text: &str) -> Result<(), String> {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("format-case");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(
        folder.join("SKILL.md"),
        "---\nname: format-case\ndescription: A case of the format's test.\n---\n",
    )
    .unwrap();
    std::fs::write(folder.join("skill.toml"), toml_text).unwrap();
    lamp_skills::load(&folder).map(|_| ())
}

/// What lamp-skills refuses in shape, the schema refuses too: each case is
/// the good skill with one thing changed, refused by the schema and by
/// `lamp_skills::load` alike.
#[test]
fn the_schema_refuses_what_lamp_skills_refuses_in_shape() {
    assert_eq!(loads(GOOD), Ok(()));
    assert_eq!(check(&as_json(GOOD)), Vec::<String>::new());
    let changed = |from: &str, to: &str| {
        assert!(GOOD.contains(from), "{from}");
        GOOD.replacen(from, to, 1)
    };
    for (bad, why) in [
        (
            format!("colour = \"warm\"\n{GOOD}"),
            "a key the format doesn't have",
        ),
        (
            changed("light = true", "light = true\nmicrophone = true"),
            "a permission the format doesn't have",
        ),
        (
            changed("colour = \"warm\"", "colour = \"magenta\""),
            "a colour not among the ten",
        ),
        (changed("level = 2", "level = 6"), "a level over 5"),
        (
            format!("{GOOD}[[do]]\nbody = {{ gesture = \"nod\", spin = true }}\n"),
            "a body key the format doesn't have",
        ),
        (
            changed("when = \"appears\"", "when = \"sometimes\""),
            "a when not among three",
        ),
        (
            changed("for = \"20s\"", "for = \"ten seconds\""),
            "a duration not in its form",
        ),
        (
            changed("version = \"1.0.0\"", "version = \"1.0\""),
            "a version that is not MAJOR.MINOR.PATCH",
        ),
        (format!("{GOOD}[[do]]\n"), "a step with nothing to do"),
        (
            changed("body = true", "body = true\ncore = [\"everything\"]"),
            "a core grant that doesn't exist",
        ),
        (
            format!("kind = \"plugin\"\n{GOOD}"),
            "a kind not among three",
        ),
        (
            format!("{GOOD}[watch]\nevery = \"1h\"\nguess = 1\n"),
            "a watch key the format doesn't have",
        ),
        (
            format!("{GOOD}[[do]]\nbody = {{ gesture = \"nodd\" }}\n"),
            "a gesture the body doesn't have",
        ),
    ] {
        assert!(
            !check(&as_json(&bad)).is_empty(),
            "the schema accepted {why}: {bad}"
        );
        assert!(loads(&bad).is_err(), "lamp-skills loaded {why}: {bad}");
    }
}

fn listed(path: &[&str]) -> Vec<String> {
    let mut at = &schema();
    let owned;
    for key in path {
        at = &at[*key];
    }
    if let Some(reference) = at.get("$ref").and_then(Value::as_str) {
        owned = schema()["$defs"][reference.strip_prefix("#/$defs/").unwrap()].clone();
        at = &owned;
    }
    at.as_array()
        .or_else(|| at["enum"].as_array())
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// The schema's lists are the code's: the colours, the core grants and the
/// add-on vocabulary.
#[test]
fn the_schemas_lists_are_the_codes() {
    let strings = |list: &[&str]| list.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    assert_eq!(
        listed(&["$defs", "colour"]),
        strings(&lamp_msgs::Light::COLOURS)
    );
    assert_eq!(
        listed(&["$defs", "permissions", "properties", "core", "items"]),
        strings(&lamp_skills::CORE_GRANTS)
    );
    assert_eq!(
        listed(&["$defs", "propose", "properties", "act"]),
        strings(&lamp_msgs::ADDON_ACTS)
    );
    assert_eq!(
        listed(&["$defs", "propose", "properties", "why"]),
        strings(&lamp_msgs::ADDON_WHYS)
    );
    assert_eq!(
        listed(&["$defs", "propose", "properties", "gesture"]),
        strings(&lamp_msgs::ADDON_GESTURES)
    );
    // A permission proposes every act but a note, which is refused there.
    let mut proposable = strings(&lamp_msgs::ADDON_ACTS);
    proposable.retain(|act| act != "note");
    assert_eq!(
        listed(&["$defs", "permissions", "properties", "propose", "items"]),
        proposable
    );
    // A gesture is one the protocol names (control.schema.json).
    let control: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("docs/control.schema.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        listed(&["$defs", "gesture"]),
        control["$defs"]["gesture"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    );
    // The kinds are the parser's.
    use lamp_skills::SkillKind;
    assert_eq!(
        listed(&["properties", "kind"]),
        [SkillKind::Core, SkillKind::Installable, SkillKind::Addon].map(SkillKind::name)
    );
}
