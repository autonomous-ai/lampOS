//! An app's `ops` and `views` (#7415, `docs/apps.md` "App views"): the screens the phone draws from small JSON an app's
//! own op returns. This module only READS and CHECKS the manifest's part of them: the shape of a view, the op rule and
//! each kind's own fields. lamp-control's `app_view` (the answers' limits, the bounds) is a later piece, and the switch
//! `LAMP_APP_VIEWS` with it. A manifest without either field is read and written back byte for byte as before: both
//! are left out when empty.
use super::app_package::Choice;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// At most this many ops an app answers on its socket.
pub const MOST_OPS: usize = 16;
/// At most this many views.
pub const MOST_VIEWS: usize = 8;
/// A form's fields, and a `rows` field's columns and rows.
pub const MOST_FIELDS: usize = 16;
pub const MOST_COLUMNS: usize = 4;
pub const MOST_ROWS: u32 = 50;
/// A `choice`'s options.
pub const MOST_OPTIONS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    List,
    Chart,
    Form,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartStyle {
    Line,
    Bar,
}

/// One view: `{id, kind, title, op}` and the fields of its own kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    pub kind: ViewKind,
    pub title: String,
    pub op: String,
    /// `list`: shown when there are no items, up to 80.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<String>,
    /// `chart`: required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ChartStyle>,
    /// `chart`: up to 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// `chart`: the goal line's name, up to 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_label: Option<String>,
    /// `form`: 1 to 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Field>>,
    /// `form`: the button's words, 1 to 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Number,
    Toggle,
    Choice,
    Rows,
}

/// One field of a form, or one column of a `rows` field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// `text`: the most characters (1 to 200). `number`: the top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// `number`: the bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// `number`: above 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// `choice`: 2 to 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Choice>>,
    /// `rows`: 1 to 4 columns of `text`, `number` or `choice`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<Field>>,
    /// `rows`: 1 to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<u32>,
}

/// `^[a-z][a-z0-9_]{0,31}$`: an op's name and a view's id.
pub fn name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// `^[a-z0-9_]{1,32}$`: a field's key.
fn key_ok(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A choice's value: `[a-z0-9][a-z0-9_-]` up to 32 (the settings' rule).
fn choice_value_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=32).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// Text on one line: 1 to `most` characters, no control character, U+2028, U+2029 or bidi mark, trimmed.
fn line_ok(text: &str, most: usize) -> bool {
    let chars = text.chars().count();
    (1..=most).contains(&chars)
        && text.trim() == text
        && !text.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{061c}' | '\u{200e}' | '\u{200f}'
                )
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
        })
}

/// The manifest's `ops` and `views` checked against their rules. `Ok` for a manifest with neither.
pub fn check(ops: &[String], views: &[View]) -> Result<(), String> {
    if ops.len() > MOST_OPS {
        return Err(format!("ops: at most {MOST_OPS}"));
    }
    let mut known = BTreeSet::new();
    for op in ops {
        if !name_ok(op) {
            return Err(format!("ops: {op:?} is not ^[a-z][a-z0-9_]{{0,31}}$"));
        }
        if !known.insert(op.as_str()) {
            return Err(format!("ops: {op:?} twice"));
        }
    }
    if views.is_empty() {
        return Ok(());
    }
    if ops.is_empty() {
        return Err("views: need ops, the names their ops are among".into());
    }
    if views.len() > MOST_VIEWS {
        return Err(format!("views: at most {MOST_VIEWS}"));
    }
    let mut ids = BTreeSet::new();
    for view in views {
        let at = |why: &str| format!("view {:?}: {why}", view.id);
        if !name_ok(&view.id) {
            return Err(at("an id is ^[a-z][a-z0-9_]{0,31}$"));
        }
        if !ids.insert(view.id.as_str()) {
            return Err(at("an id twice"));
        }
        if !line_ok(&view.title, 40) {
            return Err(at("a title is 1 to 40 characters on one line"));
        }
        if !known.contains(view.op.as_str()) {
            return Err(at(&format!("op {:?} is not one of the app's ops", view.op)));
        }
        view_ok(view).map_err(|why| at(&why))?;
    }
    Ok(())
}

/// A view's own kind's fields, and none of another kind's.
fn view_ok(view: &View) -> Result<(), String> {
    let has = (
        view.empty.is_some(),
        view.style.is_some(),
        view.unit.is_some(),
        view.goal_label.is_some(),
        view.fields.is_some(),
        view.submit.is_some(),
    );
    match view.kind {
        ViewKind::List => {
            if has.1 || has.2 || has.3 || has.4 || has.5 {
                return Err("a list has only `empty` of its own".into());
            }
            if let Some(empty) = &view.empty
                && !line_ok(empty, 80)
            {
                return Err("`empty` is 1 to 80 characters on one line".into());
            }
        }
        ViewKind::Chart => {
            if has.0 || has.4 || has.5 {
                return Err("a chart has only `style`, `unit` and `goal_label` of its own".into());
            }
            if view.style.is_none() {
                return Err("a chart needs `style`, line or bar".into());
            }
            if let Some(unit) = &view.unit
                && !line_ok(unit, 16)
            {
                return Err("`unit` is 1 to 16 characters".into());
            }
            if let Some(label) = &view.goal_label
                && !line_ok(label, 24)
            {
                return Err("`goal_label` is 1 to 24 characters".into());
            }
        }
        ViewKind::Form => {
            if has.0 || has.1 || has.2 || has.3 {
                return Err("a form has only `fields` and `submit` of its own".into());
            }
            let Some(fields) = &view.fields else {
                return Err("a form needs `fields`, 1 to 16".into());
            };
            if !(1..=MOST_FIELDS).contains(&fields.len()) {
                return Err(format!("a form has 1 to {MOST_FIELDS} fields"));
            }
            match &view.submit {
                Some(submit) if line_ok(submit, 24) => {}
                _ => return Err("a form needs `submit`, 1 to 24 characters".into()),
            }
            let mut keys = BTreeSet::new();
            for field in fields {
                field_ok(field, false)?;
                if !keys.insert(field.key.as_str()) {
                    return Err(format!("field {:?} twice", field.key));
                }
            }
        }
    }
    Ok(())
}

/// One field (or, with `column`, one column of a `rows` field: no `rows` inside `rows`).
fn field_ok(field: &Field, column: bool) -> Result<(), String> {
    let at = |why: &str| format!("field {:?}: {why}", field.key);
    if !key_ok(&field.key) {
        return Err(at("a key is 1 to 32 of a-z, 0-9 and _"));
    }
    if !line_ok(&field.label, 40) {
        return Err(at("a label is 1 to 40 characters on one line"));
    }
    if let Some(hint) = &field.hint
        && !line_ok(hint, 120)
    {
        return Err(at("a hint is 1 to 120 characters on one line"));
    }
    let has = (
        field.max.is_some(),
        field.min.is_some(),
        field.unit.is_some(),
        field.step.is_some(),
        field.options.is_some(),
        field.columns.is_some(),
        field.max_rows.is_some(),
    );
    match field.kind {
        FieldKind::Text => {
            if has.1 || has.2 || has.3 || has.4 || has.5 || has.6 {
                return Err(at("a text field has only `max` of its own"));
            }
            match field.max {
                Some(most) if most.fract() == 0.0 && (1.0..=200.0).contains(&most) => {}
                _ => return Err(at("a text field's `max` is a whole number, 1 to 200")),
            }
        }
        FieldKind::Number => {
            if has.4 || has.5 || has.6 {
                return Err(at("a number field has min, max, unit and step only"));
            }
            let (Some(min), Some(max)) = (field.min, field.max) else {
                return Err(at("a number field needs `min` and `max`"));
            };
            if !min.is_finite() || !max.is_finite() || min >= max {
                return Err(at("`min` is below `max`, both finite"));
            }
            if let Some(unit) = &field.unit
                && !line_ok(unit, 16)
            {
                return Err(at("a unit is 1 to 16 characters"));
            }
            if let Some(step) = field.step
                && !(step.is_finite() && step > 0.0)
            {
                return Err(at("`step` is above 0"));
            }
        }
        FieldKind::Toggle => {
            if has.0 || has.1 || has.2 || has.3 || has.4 || has.5 || has.6 {
                return Err(at("a toggle has only key, label and hint"));
            }
        }
        FieldKind::Choice => {
            if has.0 || has.1 || has.2 || has.3 || has.5 || has.6 {
                return Err(at("a choice has only `options` of its own"));
            }
            let Some(options) = &field.options else {
                return Err(at("a choice needs `options`, 2 to 16"));
            };
            if !(2..=MOST_OPTIONS).contains(&options.len()) {
                return Err(at("a choice has 2 to 16 options"));
            }
            let mut values = BTreeSet::new();
            for option in options {
                if !choice_value_ok(&option.value) || !line_ok(&option.label, 40) {
                    return Err(at(
                        "an option's value is [a-z0-9][a-z0-9_-] up to 32, its label 1 to 40 characters",
                    ));
                }
                if !values.insert(option.value.as_str()) {
                    return Err(at("an option's value twice"));
                }
            }
        }
        FieldKind::Rows => {
            if column {
                return Err(at("a column is text, number or choice, never rows"));
            }
            if has.0 || has.1 || has.2 || has.3 || has.4 {
                return Err(at(
                    "a rows field has only `columns` and `max_rows` of its own",
                ));
            }
            match field.max_rows {
                Some(rows) if (1..=MOST_ROWS).contains(&rows) => {}
                _ => return Err(at("`max_rows` is 1 to 50")),
            }
            let Some(columns) = &field.columns else {
                return Err(at("a rows field needs `columns`, 1 to 4"));
            };
            if !(1..=MOST_COLUMNS).contains(&columns.len()) {
                return Err(at("a rows field has 1 to 4 columns"));
            }
            let mut keys = BTreeSet::new();
            for each in columns {
                if each.kind == FieldKind::Toggle {
                    return Err(at("a column is text, number or choice"));
                }
                field_ok(each, true)?;
                if !keys.insert(each.key.as_str()) {
                    return Err(at(&format!("column {:?} twice", each.key)));
                }
            }
        }
    }
    Ok(())
}
