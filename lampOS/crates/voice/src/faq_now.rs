//! Which agent and text model this lamp uses now, told beside the FAQ's
//! agent section (#8729).
//!
//! Without it the FAQ had no agent or model facts, so an agent could be
//! taken for a linked account and no model could be named. Both differ per
//! lamp (the owner's choice of agent in the app; the relay's model or a
//! person's own key's), so the FAQ's notes (`skills/faq/knowledge.md`, `## agent`) say
//! where they are changed, and the voice adds what this lamp has now under
//! [`KEY`], read at answer time: the lamp's default agent as lamp-tasks'
//! targets last said it (`agent_tasks`, read every 30 s with
//! `LAMP_TASKS_WHO`), and the text model the second route started with
//! ([`keep_text_model`], from `second.json` and the person's own keys). What
//! can't be read is said so, never guessed.
use serde_json::Value;
use std::sync::Mutex;

/// The FAQ skill's tool (`skills/faq/skill.toml`).
pub const TOOL: &str = "lamp_help";
/// Its choice's words this is told beside: where "which agent" or "which
/// model" may be sent.
pub const TOPICS: [&str; 2] = ["agent", "about"];
/// The key of the answer it is told under, as the agent section names it.
pub const KEY: &str = "this_lamp_now";

/// The text model as the person hears it, set once the route's services
/// are made (latest wins).
static TEXT_MODEL: Mutex<Option<String>> = Mutex::new(None);

/// Keep `words` as the text model this voice answers with.
pub fn keep_text_model(words: String) {
    if let Ok(mut kept) = TEXT_MODEL.lock() {
        *kept = Some(words);
    }
}

fn text_model() -> Option<String> {
    TEXT_MODEL.lock().ok().and_then(|kept| kept.clone())
}

/// The line for `agent` and `model`, each `None` when it couldn't be read.
pub fn line(agent: Option<&str>, model: Option<&str>) -> String {
    let agent = agent.map_or_else(
        || "the lamp's agent couldn't be read just now".to_owned(),
        |agent| format!("the lamp's agent is {agent}"),
    );
    let model = model.map_or_else(
        || "its text model couldn't be read just now".to_owned(),
        |model| format!("its text model is {model}"),
    );
    format!("Read from this lamp's own settings now: {agent}; {model}.")
}

/// Add [`KEY`] to `told`, the answer to a call of `name` with `args`,
/// when it is the FAQ's with one of [`TOPICS`] (or none: its default is
/// `abilities`, so not then). An error, or an answer that is not an object, is left as it is.
pub fn add(name: &str, args: &Value, told: &mut Value) {
    let topic = args.get("topic").and_then(Value::as_str);
    if name != TOOL || !topic.is_some_and(|topic| TOPICS.contains(&topic)) {
        return;
    }
    if let Some(answer) = told.as_object_mut()
        && !answer.contains_key("error")
    {
        let agent = crate::agent_tasks::lamp_agent_name();
        answer.insert(
            KEY.to_owned(),
            Value::String(line(agent, text_model().as_deref())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_agent_topic_is_told_what_this_lamp_has_now_and_others_are_not() {
        let mut told = json!({"next": "notes"});
        add(TOOL, &json!({"topic": "agent"}), &mut told);
        let now = told[KEY].as_str().unwrap();
        assert!(
            now.starts_with("Read from this lamp's own settings now: "),
            "{now}"
        );
        for (name, args) in [
            (TOOL, json!({"topic": "connectors"})),
            (TOOL, json!({})),
            ("dance", json!({"topic": "agent"})),
        ] {
            let mut told = json!({"next": "notes"});
            add(name, &args, &mut told);
            assert!(told.get(KEY).is_none(), "{name}");
        }
        let mut refused = json!({"error": "no"});
        add(TOOL, &json!({"topic": "agent"}), &mut refused);
        assert!(refused.get(KEY).is_none());
    }

    #[test]
    fn what_was_read_is_named_and_what_was_not_is_said_so() {
        assert_eq!(
            line(
                Some("OpenClaw"),
                Some("DeepSeek's deepseek-flash, through Autonomous's relay")
            ),
            "Read from this lamp's own settings now: the lamp's agent is OpenClaw; its text model is DeepSeek's deepseek-flash, through Autonomous's relay."
        );
        let none = line(None, None);
        assert!(none.contains("agent couldn't be read") && none.contains("model couldn't be read"));
    }
}
