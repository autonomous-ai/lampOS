//! An installed app's tools for the model (#8406, `LAMP_APP_TOOLS`, off).
//! lamp-control keeps the tools each running app's manifest names, as
//! checked (`apps/tools`, the newest kept); the voice offers them to the
//! model on the second route as functions of their own, and a call the
//! model makes goes to lamp-control (`apps/call`), which hands it to the
//! app and tells the voice its answer (`apps/called`). All three are local
//! to the lamp; none carries audio, a transcript or a person's name.
//!
//! The checks here are the Store's own (docs/app-tools.md, "The Store's
//! checks"), held again on the lamp at each read: a tool's name, its
//! description in plain words, and its arguments in a small JSON Schema
//! subset, which [`check_args`] holds each call to before anything is sent.
use crate::Invalid;
use crate::app_triggers::name_ok;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Duration;

/// At most this many tools an app.
pub const MOST_TOOLS: usize = 8;
/// A tool's name: `^[a-z][a-z0-9_]{0,29}$`.
pub const NAME_MOST: usize = 30;
/// A tool's description, in characters.
pub const DESCRIPTION_MOST: usize = 150;
/// A parameter's description, in characters.
pub const FIELD_DESCRIPTION_MOST: usize = 120;
/// A tool's `parameters`, as JSON.
pub const PARAMETERS_MOST_BYTES: usize = 1_024;
/// At most this many properties a tool.
pub const MOST_FIELDS: usize = 6;
/// At most this many values in a string's `enum`.
pub const MOST_ENUM: usize = 16;
/// The most a string's `maxLength` may be.
pub const MAX_LENGTH_MOST: u64 = 200;
/// A call's arguments, and an answer's `result`, as JSON.
pub const ARGS_MOST_BYTES: usize = 2_048;
/// An answer's `error`, in characters.
pub const ERROR_MOST: usize = 200;
/// The app's name with `-` as `_`, `_`, and the tool's: the link's bound
/// on a function's name (`lamp_gemini::Tool`: 48 bytes).
pub const FUNCTION_MOST: usize = 48;
/// An app's title, as shown, in bytes.
pub const TITLE_MOST_BYTES: usize = 64;

/// A tool's own name.
pub fn tool_name_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= NAME_MOST
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The function the model is given for `app`'s `tool`: `gym-coach` and
/// `set_plan` are `gym_coach_set_plan`.
pub fn function_name(app: &str, tool: &str) -> String {
    format!("{}_{tool}", app.replace('-', "_"))
}

/// Plain words, as `start_app`'s trigger words are held to: one line, no
/// double or curly quote (an apostrophe in a word is kept), backtick, markup, brace, bracket, link, control or bidi
/// character, so an app's words can sit inside a quote as data.
pub fn plain_words(words: &str, most_chars: usize) -> bool {
    let count = words.chars().count();
    (1..=most_chars).contains(&count)
        && !words.trim().is_empty()
        && !words.contains("://")
        && !words.contains("www.")
        && words.chars().all(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '"' | '`' | '<' | '>' | '{' | '}' | '[' | ']' | '\\' | '|'
                        | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}'
                        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                        | '\u{200B}'..='\u{200F}' | '\u{FEFF}'
                )
        })
}

/// A string in an enum: a word or a few, plain, at most 64 bytes.
fn enum_value_ok(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.len() <= 64 && plain_words(value, 64))
}

/// A field's optional `description`.
fn field_description_ok(field: &Map<String, Value>) -> bool {
    field.get("description").is_none_or(|said| {
        said.as_str()
            .is_some_and(|said| plain_words(said, FIELD_DESCRIPTION_MOST))
    })
}

/// One property in the subset: a `string` with an `enum` (at most
/// [`MOST_ENUM`]) or a `maxLength` (at most [`MAX_LENGTH_MOST`]), an
/// `integer` or `number` with both `minimum` and `maximum`, or a
/// `boolean`, each with an optional plain `description`; nothing else.
fn field_ok(field: &Value) -> Result<(), &'static str> {
    let field = field.as_object().ok_or("a property is not an object")?;
    if !field_description_ok(field) {
        return Err("a property's description is not plain words of at most 120 characters");
    }
    let keys = |allowed: &[&str]| field.keys().all(|key| allowed.contains(&key.as_str()));
    match field.get("type").and_then(Value::as_str) {
        Some("string") => {
            let listed = field.get("enum");
            let long = field.get("maxLength");
            match (listed, long) {
                (Some(Value::Array(values)), None)
                    if (1..=MOST_ENUM).contains(&values.len())
                        && values.iter().all(enum_value_ok)
                        && keys(&["type", "enum", "description"]) =>
                {
                    Ok(())
                }
                (None, Some(most))
                    if most
                        .as_u64()
                        .is_some_and(|most| (1..=MAX_LENGTH_MOST).contains(&most))
                        && keys(&["type", "maxLength", "description"]) =>
                {
                    Ok(())
                }
                _ => Err(
                    "a string property takes an enum of at most 16 plain values or a maxLength of at most 200",
                ),
            }
        }
        Some("integer" | "number") => {
            let integer = field.get("type").and_then(Value::as_str) == Some("integer");
            let bound = |key: &str| {
                field.get(key).is_some_and(|value| {
                    if integer {
                        value.as_i64().is_some()
                    } else {
                        value.as_f64().is_some_and(f64::is_finite)
                    }
                })
            };
            let ordered = field.get("minimum").and_then(Value::as_f64)
                <= field.get("maximum").and_then(Value::as_f64);
            if bound("minimum")
                && bound("maximum")
                && ordered
                && keys(&["type", "minimum", "maximum", "description"])
            {
                Ok(())
            } else {
                Err("a number property takes both a minimum and a maximum")
            }
        }
        Some("boolean") if keys(&["type", "description"]) => Ok(()),
        _ => Err("a property is a string, integer, number or boolean, and nothing else"),
    }
}

/// A tool's `parameters` in the subset: `type: "object"`, `properties`
/// (at most [`MOST_FIELDS`], each [`field_ok`], named as a tool is) and
/// `required` (names among them), at most [`PARAMETERS_MOST_BYTES`].
pub fn schema_ok(schema: &Value) -> Result<(), Invalid> {
    let bad = |why: &str| Err(Invalid(why.into()));
    let Some(object) = schema.as_object() else {
        return bad("parameters is not an object");
    };
    if schema.to_string().len() > PARAMETERS_MOST_BYTES {
        return bad("parameters is over 1,024 bytes");
    }
    if object.get("type").and_then(Value::as_str) != Some("object")
        || !object
            .keys()
            .all(|key| matches!(key.as_str(), "type" | "properties" | "required"))
    {
        return bad("parameters is type object, with properties and required only");
    }
    let empty = Map::new();
    let properties = match object.get("properties") {
        None => &empty,
        Some(Value::Object(properties)) => properties,
        Some(_) => return bad("properties is not an object"),
    };
    if properties.len() > MOST_FIELDS {
        return bad("more than 6 properties");
    }
    for (name, field) in properties {
        if !tool_name_ok(name) {
            return bad("a property's name is not a-z, 0-9 and _");
        }
        field_ok(field).map_err(|why| Invalid(why.into()))?;
    }
    match object.get("required") {
        None => {}
        Some(Value::Array(required))
            if required.iter().enumerate().all(|(index, name)| {
                name.as_str()
                    .is_some_and(|name| properties.contains_key(name))
                    && !required[..index].contains(name)
            }) => {}
        Some(_) => return bad("required names only its own properties, once each"),
    }
    Ok(())
}

/// The model's `args` for a tool whose `parameters` is `schema` (checked
/// by [`schema_ok`]): every required field there, no other field, each
/// value of its type and in its enum, range or length. The error names
/// the field, never its value, and is what the model is told.
pub fn check_args(schema: Option<&Value>, args: &Value) -> Result<(), String> {
    let empty = Map::new();
    let properties = schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let given = match args {
        Value::Object(given) => given,
        Value::Null => &empty,
        _ => return Err("the arguments are not an object".into()),
    };
    if args.to_string().len() > ARGS_MOST_BYTES {
        return Err("the arguments are too long".into());
    }
    for name in schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !given.contains_key(name) {
            return Err(format!("{name} is needed"));
        }
    }
    for (name, value) in given {
        let Some(field) = properties.get(name) else {
            return Err(format!("{name} is not one of this tool's fields"));
        };
        let in_range = |number: f64| {
            field
                .get("minimum")
                .and_then(Value::as_f64)
                .is_none_or(|least| number >= least)
                && field
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_none_or(|most| number <= most)
        };
        let fits = match field.get("type").and_then(Value::as_str) {
            Some("string") => value.as_str().is_some_and(|said| {
                match field.get("enum").and_then(Value::as_array) {
                    Some(values) => values.iter().any(|listed| listed.as_str() == Some(said)),
                    None => field
                        .get("maxLength")
                        .and_then(Value::as_u64)
                        .is_some_and(|most| said.chars().count() as u64 <= most),
                }
            }),
            Some("integer") => value.as_i64().is_some_and(|number| in_range(number as f64)),
            Some("number") => value.as_f64().is_some_and(in_range),
            Some("boolean") => value.is_boolean(),
            _ => false,
        };
        if !fits {
            return Err(match field.get("type").and_then(Value::as_str) {
                Some(kind @ ("integer" | "number")) => format!(
                    "{name} takes {} from {} to {}",
                    if kind == "integer" {
                        "a whole number"
                    } else {
                        "a number"
                    },
                    field["minimum"],
                    field["maximum"]
                ),
                Some("string") => match field.get("enum").and_then(Value::as_array) {
                    Some(values) => format!(
                        "{name} takes one of: {}",
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => format!(
                        "{name} takes words of at most {} characters",
                        field["maxLength"]
                    ),
                },
                Some("boolean") => format!("{name} takes true or false"),
                _ => format!("{name} is not a value this tool takes"),
            });
        }
    }
    Ok(())
}

/// One of an app's tools, as its checked tools file gives it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTool {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    /// In the file's `outside` list: it acts outside the lamp and the
    /// app's own data, so it needs the person's plain yes first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside: bool,
}

impl AppTool {
    pub fn check(&self) -> Result<(), Invalid> {
        if !tool_name_ok(&self.name) {
            return Err(Invalid(
                "a tool's name is not ^[a-z][a-z0-9_]{0,29}$".into(),
            ));
        }
        if !plain_words(&self.description, DESCRIPTION_MOST) {
            return Err(Invalid(
                "a tool's description is not plain words of at most 150 characters".into(),
            ));
        }
        if let Some(schema) = &self.parameters {
            schema_ok(schema)?;
        }
        Ok(())
    }
}

/// A running app's tools: its manifest name, its title as shown, when its
/// session began (the one running longest keeps the voice's section), its
/// tools, and the phrases its connections listen for (`phrase.heard`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppsTools {
    pub app: String,
    pub title: String,
    pub since_ms: u64,
    pub tools: Vec<AppTool>,
    /// The phrases lamp-control tells this app as `phrase.heard` when a
    /// finished line holds one ([`phrase_in`]): the voice offers none of
    /// the app's tools in such a line's turn, so the app's phrase and its
    /// tool never both act on one line (#8386). None, and left out of the
    /// message, when the app listens for none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<String>,
}

/// The most phrases told of one app on `apps/tools`: a client's own cap
/// (lamp-control's `PHRASES_MAX`).
pub const MOST_PHRASES: usize = 16;
/// The longest phrase told, in characters (lamp-control's
/// `PHRASE_MAX_CHARS`).
pub const PHRASE_MOST_CHARS: usize = 64;

/// A line's or a phrase's words as the phrase match sees them: split on
/// anything but letters, digits and the apostrophe, lowercased, each
/// without its apostrophes ("it's" and "its" are one).
fn phrase_match_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.to_lowercase().replace('\'', ""))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether the finished line `heard` holds `phrase`, as lamp-control
/// matches it before it tells an app `phrase.heard` (one match for both
/// sides): some run of whole heard words spells the phrase's words, their
/// letters joined, so "goodnight" holds "good night" and the other way
/// round; the heard run may end in one "s" more than the phrase ("lamps"
/// for "lamp"), never fewer.
pub fn phrase_in(heard: &str, phrase: &str) -> bool {
    let wanted: String = phrase_match_words(phrase).concat();
    if wanted.is_empty() {
        return false;
    }
    let heard = phrase_match_words(heard);
    (0..heard.len()).any(|start| {
        let mut joined = String::new();
        for word in &heard[start..] {
            joined.push_str(word);
            if joined == wanted || joined.strip_suffix('s') == Some(wanted.as_str()) {
                return true;
            }
            if joined.len() > wanted.len() {
                return false;
            }
        }
        false
    })
}

/// The tools of the apps running now, each with a session open, the newest
/// kept (`apps/tools`, from lamp-control).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTools {
    pub apps: Vec<AppsTools>,
}

impl AppTools {
    pub const TYPE: &str = "lamp_msgs/AppTools";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/tools";
    pub const MAX_APPS: usize = 8;

    /// Said by lamp-control at each change and at least every 5 s while
    /// apps run; a reading older than 30 s is a dead publisher's, and the
    /// voice offers none.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(5))
            .lasting(Duration::from_secs(30))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.apps.len() > Self::MAX_APPS {
            return Err(Invalid("too many apps with tools".into()));
        }
        for (index, app) in self.apps.iter().enumerate() {
            if !name_ok(&app.app) || self.apps[..index].iter().any(|seen| seen.app == app.app) {
                return Err(Invalid(
                    "an app's name is not a manifest name, or repeats".into(),
                ));
            }
            if app.title.len() > TITLE_MOST_BYTES || app.title.contains(['\n', '\r']) {
                return Err(Invalid(
                    "an app's title is not one line of at most 64 bytes".into(),
                ));
            }
            if app.tools.len() > MOST_TOOLS {
                return Err(Invalid("more than 8 tools an app".into()));
            }
            if app.phrases.len() > MOST_PHRASES
                || app.phrases.iter().any(|phrase| {
                    phrase.chars().count() > PHRASE_MOST_CHARS || phrase.contains(['\n', '\r'])
                })
            {
                return Err(Invalid(
                    "an app's phrases are at most 16 lines of at most 64 characters".into(),
                ));
            }
            for (at, tool) in app.tools.iter().enumerate() {
                tool.check()?;
                if app.tools[..at].iter().any(|seen| seen.name == tool.name) {
                    return Err(Invalid("a tool's name repeats".into()));
                }
                if function_name(&app.app, &tool.name).len() > FUNCTION_MOST {
                    return Err(Invalid(
                        "an app's name and a tool's are over 47 bytes together".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A call the model made, as checked by the voice (`apps/call`): `call` is
/// the voice's own id for it, unique and never reused (the app's
/// idempotency key), `name` the tool's own name and `args` its checked
/// arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppCall {
    pub call: String,
    pub app: String,
    pub name: String,
    pub args: Value,
}

/// A call's id: `app-<boot>-<request>-<n>`, each a number: `boot` is the
/// voice process's own start (its milliseconds), so a call made after a
/// restart never repeats an id from before it (the app's idempotency key).
pub fn call_id_ok(call: &str) -> bool {
    let parts: Vec<&str> = call.split('-').collect();
    call.len() <= 64
        && parts.len() == 4
        && parts[0] == "app"
        && parts[1..]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

impl AppCall {
    pub const TYPE: &str = "lamp_msgs/AppCall";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/call";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !call_id_ok(&self.call) || !name_ok(&self.app) || !tool_name_ok(&self.name) {
            return Err(Invalid(
                "an app call names its id, an app and a tool".into(),
            ));
        }
        if !self.args.is_object() || self.args.to_string().len() > ARGS_MOST_BYTES {
            return Err(Invalid(
                "an app call's args are an object of at most 2,048 bytes".into(),
            ));
        }
        Ok(())
    }
}

/// An app's answer to a call, or the lamp's word for why there is none
/// (`apps/called`, from lamp-control, once a call): exactly one of
/// `result`, a JSON object, and `error`, one plain line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppCalled {
    pub call: String,
    pub app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AppCalled {
    pub const TYPE: &str = "lamp_msgs/AppCalled";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/called";

    /// The lamp's own words for a call with no answer of the app's.
    pub const LAMP_WORDS: [&str; 7] = [
        "timeout",
        "too_long",
        "busy",
        "private",
        "stopped",
        "unknown tool",
        "not running",
    ];

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !call_id_ok(&self.call) || !name_ok(&self.app) {
            return Err(Invalid("an app's answer names its call and app".into()));
        }
        match (&self.result, &self.error) {
            (Some(result), None)
                if result.is_object() && result.to_string().len() <= ARGS_MOST_BYTES => {}
            (None, Some(error))
                if error.chars().count() <= ERROR_MOST && !error.contains(['\n', '\r']) => {}
            _ => {
                return Err(Invalid(
                    "an app's answer is a result object of at most 2,048 bytes or one error line of at most 200 characters".into(),
                ));
            }
        }
        Ok(())
    }
}

pub fn app_tool_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (AppTools::TOPIC, AppTools::declaration()),
        (AppCall::TOPIC, AppCall::declaration()),
        (AppCalled::TOPIC, AppCalled::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn coach() -> AppsTools {
        AppsTools {
            app: "gym-coach".into(),
            title: "Gym Coach".into(),
            since_ms: 1,
            tools: vec![AppTool {
                name: "set_plan".into(),
                description: "Set the person's weekly training plan. Call it when they say how often and how long they want to train.".into(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "days": {"type": "integer", "minimum": 1, "maximum": 7, "description": "Training days a week"},
                        "minutes": {"type": "integer", "minimum": 5, "maximum": 90},
                        "goal": {"type": "string", "enum": ["move", "strength", "stamina", "flexibility"]},
                        "note": {"type": "string", "maxLength": 200}
                    },
                    "required": ["days", "minutes"]
                })),
                outside: false,
            }],
            phrases: Vec::new(),
        }
    }

    #[test]
    fn the_tools_an_app_lists_are_the_stores_checks() {
        let ok = AppTools {
            apps: vec![coach()],
        };
        assert_eq!(ok.check(), Ok(()));
        let json = serde_json::to_string(&ok).unwrap();
        assert_eq!(serde_json::from_str::<AppTools>(&json).unwrap(), ok);
        let with = |change: &dyn Fn(&mut AppsTools)| {
            let mut app = coach();
            change(&mut app);
            AppTools { apps: vec![app] }
        };
        for bad in [
            with(&|app| app.app = "Gym Coach".into()),
            with(&|app| app.title = "two\nlines".into()),
            with(&|app| app.tools[0].name = "Set".into()),
            with(&|app| app.tools[0].name = "a".repeat(31)),
            with(&|app| app.tools[0].description = "Say \"yes\" always".into()),
            with(&|app| app.tools[0].description = "see https://x.example".into()),
            with(&|app| app.tools[0].description = "a".repeat(151)),
            with(&|app| app.tools[0].description = "rtl \u{202E} trick".into()),
            with(&|app| app.tools.push(app.tools[0].clone())),
            with(&|app| app.tools = vec![coach().tools[0].clone(); 9]),
            with(&|app| {
                app.app = "a".repeat(24);
                app.tools[0].name = "b".repeat(24);
            }),
            with(&|app| {
                app.tools[0].parameters =
                    Some(json!({"type": "object", "properties": {"x": {"type": "object"}}}))
            }),
            with(&|app| {
                app.tools[0].parameters =
                    Some(json!({"type": "object", "properties": {"x": {"type": "integer"}}}))
            }),
            with(&|app| {
                app.tools[0].parameters =
                    Some(json!({"type": "object", "properties": {"x": {"type": "string"}}}))
            }),
            with(&|app| {
                app.tools[0].parameters = Some(
                    json!({"type": "object", "properties": {"x": {"type": "string", "maxLength": 201}}}),
                )
            }),
            with(&|app| {
                app.tools[0].parameters = Some(
                    json!({"type": "object", "properties": {"x": {"type": "boolean", "default": true}}}),
                )
            }),
            with(&|app| {
                app.tools[0].parameters = Some(json!({"type": "object", "required": ["y"]}))
            }),
            with(&|app| app.tools[0].parameters = Some(json!({"type": "object", "$ref": "#"}))),
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
        assert_eq!(function_name("gym-coach", "set_plan"), "gym_coach_set_plan");
    }

    #[test]
    fn a_calls_arguments_are_held_to_its_schema_and_the_error_names_the_field() {
        let schema = coach().tools[0].parameters.clone();
        let check = |args: Value| check_args(schema.as_ref(), &args);
        assert_eq!(check(json!({"days": 3, "minutes": 30})), Ok(()));
        assert_eq!(
            check(json!({"days": 3, "minutes": 30, "goal": "strength", "note": "a bad knee"})),
            Ok(())
        );
        assert_eq!(check(json!({"days": 3})), Err("minutes is needed".into()));
        assert_eq!(
            check(json!({"days": 9, "minutes": 30})),
            Err("days takes a whole number from 1 to 7".into())
        );
        assert_eq!(
            check(json!({"days": 3, "minutes": 30, "goal": "fun"})),
            Err("goal takes one of: move, strength, stamina, flexibility".into())
        );
        assert_eq!(
            check(json!({"days": 3, "minutes": 30, "note": "x".repeat(201)})),
            Err("note takes words of at most 200 characters".into())
        );
        assert_eq!(
            check(json!({"days": 3, "minutes": 30, "other": 1})),
            Err("other is not one of this tool's fields".into())
        );
        assert_eq!(
            check(json!({"days": "3", "minutes": 30})),
            Err("days takes a whole number from 1 to 7".into())
        );
        assert_eq!(
            check(json!([1])),
            Err("the arguments are not an object".into())
        );
        assert_eq!(check_args(None, &json!({})), Ok(()));
        assert_eq!(check_args(None, &Value::Null), Ok(()));
        assert!(check_args(None, &json!({"x": 1})).is_err());
    }

    #[test]
    fn a_call_and_its_answer_name_a_call_an_app_and_nothing_else() {
        let call = AppCall {
            call: "app-1700000000000-12-1".into(),
            app: "gym-coach".into(),
            name: "set_plan".into(),
            args: json!({"days": 3}),
        };
        assert_eq!(call.check(), Ok(()));
        for id in [
            "app-12-1",
            "kept-1-12-1",
            "app-1--1",
            "app-1-1-x",
            "app-1-2-3-4",
        ] {
            let bad = AppCall {
                call: id.into(),
                ..call.clone()
            };
            assert!(bad.check().is_err(), "{id}");
        }
        let done = AppCalled {
            call: "app-1700000000000-12-1".into(),
            app: "gym-coach".into(),
            result: Some(json!({"ok": true})),
            error: None,
        };
        assert_eq!(done.check(), Ok(()));
        let both = AppCalled {
            error: Some("x".into()),
            ..done.clone()
        };
        assert!(both.check().is_err());
        let neither = AppCalled {
            result: None,
            ..done.clone()
        };
        assert!(neither.check().is_err());
        let long = AppCalled {
            result: Some(json!({"x": "y".repeat(2_100)})),
            ..done.clone()
        };
        assert!(long.check().is_err());
        let failed = AppCalled {
            result: None,
            error: Some("timeout".into()),
            ..done
        };
        assert_eq!(failed.check(), Ok(()));
        assert!(
            serde_json::from_str::<AppCalled>(
                r#"{"call":"app-1-1-1","app":"a","error":"x","extra":1}"#
            )
            .is_err()
        );
    }

    /// #8386: the phrase match both lamp-control and the voice read.
    #[test]
    fn a_phrase_is_in_a_line_as_lamp_control_tells_it() {
        assert!(phrase_in("Hey Lamp, end workout.", "end workout"));
        assert!(phrase_in("ENDWORKOUT", "end workout"));
        assert!(phrase_in("goodnight lamps", "good night lamp"));
        assert!(phrase_in("it's done now", "its done"));
        assert!(!phrase_in("end the workout", "end workout"));
        assert!(!phrase_in("goodnights lamp", "good night lamp"));
        assert!(!phrase_in("anything", ""));
        assert!(!phrase_in("", "end workout"));
    }

    /// An app's phrases on `apps/tools` are held to a client's own caps,
    /// and an old message without them still reads.
    #[test]
    fn an_apps_phrases_are_held_to_the_clients_caps() {
        let with = |phrases: Vec<String>| AppTools {
            apps: vec![AppsTools { phrases, ..coach() }],
        };
        assert_eq!(with(vec!["end workout".into()]).check(), Ok(()));
        assert!(
            with((0..17).map(|n| format!("phrase {n}")).collect())
                .check()
                .is_err()
        );
        assert!(with(vec!["x".repeat(65)]).check().is_err());
        assert!(with(vec!["end\nworkout".into()]).check().is_err());
        let old: AppsTools = serde_json::from_value(json!({
            "app": "gym-coach", "title": "Gym Coach", "since_ms": 1, "tools": []
        }))
        .unwrap();
        assert!(old.phrases.is_empty());
        assert!(serde_json::to_value(&old).unwrap().get("phrases").is_none());
    }
}
