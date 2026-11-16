//! For tests that plant words and then check that nothing kept them
//! (#7311): the words a JSON text holds, and none of its numbers.
//!
//! Such a test used to search the kept text as it stood. A planted digit
//! code then matched the clock's own digits: `"at_unix_ms":1794821403292`
//! holds "4821", so for the 1,000 s that lasted on 16 November 2026 the
//! sign-in test failed with nothing of the person's kept, and merge train
//! 970 with it. The same four digits come back in the lower places for a
//! second every few hours.

use serde_json::Value;

/// Every string in `json`, the names of its fields among them, one to a
/// line; no number, so no time. Panics on text that is not JSON: it is a
/// test's own kept text.
pub(crate) fn strings_of(json: &str) -> String {
    fn walk(value: &Value, out: &mut String) {
        match value {
            Value::String(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            Value::Object(fields) => {
                for (name, item) in fields {
                    out.push_str(name);
                    out.push('\n');
                    walk(item, out);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    let value: Value = serde_json::from_str(json).expect("the kept text is JSON");
    let mut out = String::new();
    walk(&value, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::strings_of;

    /// Train 970's own line: the time holds the planted code, and no word
    /// does.
    #[test]
    fn a_times_digits_are_not_words() {
        let kept = r#"[{"at_unix_ms":1794821403292,"person":"","lamp":"(a sign-in code and account were said)"}]"#;
        assert!(kept.contains("4821"));
        let words = strings_of(kept);
        assert!(!words.contains("4821"), "{words}");
        assert!(words.contains("a sign-in code and account"), "{words}");
    }

    /// A code that was really kept is still found: in a field, in a field
    /// inside a field, in a list, and as a field's name.
    #[test]
    fn a_kept_word_is_found_wherever_it_is() {
        for kept in [
            r#"[{"at_unix_ms":1,"lamp":"The code was 4821."}]"#,
            r#"{"turns":[{"note":{"heard":"4821"}}]}"#,
            r#"[["4821"]]"#,
            r#"{"4821":true}"#,
        ] {
            assert!(strings_of(kept).contains("4821"), "{kept}");
        }
    }

    /// Text that is not JSON is the test's fault, and says so.
    #[test]
    #[should_panic(expected = "the kept text is JSON")]
    fn text_that_is_not_json_panics() {
        strings_of("");
    }
}
