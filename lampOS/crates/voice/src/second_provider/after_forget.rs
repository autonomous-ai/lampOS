//! A forget starts the next chat from static instructions and this question.
//! No copied memory, prior exchange, tool payload, reasoning or picture survives.
use super::{Message, Value};
use serde_json::json;

pub(super) fn is_memory_forget(name: &str) -> bool {
    matches!(
        name,
        crate::remember::FORGET | crate::remember::FORGET_EVERYTHING
    )
}

/// Only outcome fields, never a tool's arbitrary strings, arguments or ids.
/// Earlier actions remain accounted for without retaining what they read.
pub(super) fn status(name: &str, result: &Value) -> Value {
    // A hallucinated or user-defined function name can itself hold private
    // words. Only built-in names survive the boundary.
    let name = if is_memory_forget(name)
        || matches!(
            name,
            crate::remember::REMEMBER | crate::remember::RECALL | crate::tools::THE_TIME
        )
        || crate::tools::SCHEDULE_NAMES.contains(&name)
        || crate::agent_tasks::is_tool(name)
        || crate::known_faces::is_tool(name)
        || matches!(
            name,
            "light_on"
                | "light_off"
                | "light_brighter"
                | "light_dimmer"
                | "light_warmer"
                | "light_cooler"
                | "volume_up"
                | "volume_down"
        ) {
        name
    } else {
        "other_call"
    };
    let mut outcome = serde_json::Map::new();
    for field in ["ok", "forgotten", "everything", "all", "remembered"] {
        if let Some(value) = result[field].as_bool() {
            outcome.insert(field.to_owned(), json!(value));
        }
    }
    if result["scope"] == "room" {
        outcome.insert("scope".into(), json!("room"));
    }
    if result["missed"]
        .as_str()
        .is_some_and(|missed| !missed.is_empty())
    {
        outcome.insert("some_subjects_not_found".into(), json!(true));
    }
    if result["left"].as_u64().is_some_and(|left| left > 0) {
        outcome.insert("some_items_left".into(), json!(true));
    }
    if result.get("error").is_some() {
        outcome.insert("error".into(), json!(true));
    }
    if result.get("face").is_some() {
        outcome.insert(
            "face_forgotten".into(),
            json!(result["face"] == "forgotten too"),
        );
    }
    if result.get("face_note").is_some() {
        outcome.insert("old_face_records_kept_apart".into(), json!(true));
    }
    if result.get("setup_name").is_some() {
        outcome.insert("setup_name_kept".into(), json!(true));
    }
    let mut incomplete = outcome.contains_key("error")
        || outcome.get("face_forgotten") == Some(&json!(false))
        || outcome.contains_key("old_face_records_kept_apart")
        || outcome.contains_key("setup_name_kept")
        || outcome.contains_key("some_subjects_not_found")
        || outcome.contains_key("some_items_left")
        || outcome.get("all") == Some(&json!(false));
    if is_memory_forget(name) {
        // These fields are deletion steps, not a person's name or place.
        // Match the producers' fixed outcomes; never retain their text.
        for (field, key, kept) in [
            ("name", "setup_name_result", crate::person::NAME_KEPT),
            ("place", "place_result", crate::owner_place::PLACE_KEPT),
        ] {
            if let Some(value) = result.get(field) {
                let state = if value == "forgotten too" {
                    "forgotten"
                } else if value == kept {
                    "kept"
                } else {
                    "uncertain"
                };
                incomplete |= state != "forgotten";
                outcome.insert(key.into(), json!(state));
            }
        }
        if let Some(value) = result.get("reminders") {
            let state = if value["cancelled"].as_u64().is_some_and(|count| count > 0) {
                "cancelled"
            } else if value == crate::tools::schedule_whose::REMINDERS_KEPT {
                "kept"
            } else {
                "uncertain"
            };
            incomplete |= state != "cancelled";
            outcome.insert("reminders_result".into(), json!(state));
        }
    }
    // A reminders failure does not downgrade the producer's everything=true.
    // Do not carry that overbroad claim past an explicit failed/uncertain step.
    if incomplete && outcome.get("everything") == Some(&json!(true)) {
        outcome.insert("everything".into(), json!(false));
    }
    json!({"tool": name, "outcome": outcome})
}

pub(super) fn context(
    instruction: &str,
    question: &Message,
    calls: &[Value],
    left: usize,
) -> Vec<Message> {
    vec![
        Message::system(instruction),
        Message::system(format!(
            "A memory forget call has run in this turn. The earlier conversation, recalled memory, pictures and tool payloads have been discarded. Answer only the person's current words and these content-free call outcomes: {}. Do not repeat any of these calls or reconstruct what was forgotten. Only confirm deletion when the outcome confirms it; an error or missing confirmation means the result is uncertain, not success. forgotten reports memory items only; false means none were removed. face_forgotten is independent and true confirms the face was forgotten. setup_name_result, place_result and reminders_result report those steps: forgotten/cancelled confirms that step, kept means it could not be cleared, uncertain means it was not confirmed. A missing step is unreported, never a confirmation. everything=false, all=false, some_subjects_not_found or some_items_left means do not claim complete deletion; distinguish a known kept part from an uncertain outcome. scope=room means only what this room remembers, never a person's private memory. Say briefly if a face, old face records, setup name, place or reminders could not be cleared. {left} later calls from the old round were not run; if the current words still ask for other work, plan it afresh without the discarded context. Never invent a fact or a reason that the status does not give.",
            serde_json::to_string(calls).expect("JSON values serialize"),
        )),
        question.clone(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_step_warnings_survive_without_their_payloads() {
        // These are the producers' returned shapes. In particular, a failed
        // reminder cancellation leaves everything=true in the original result.
        for (field, value, key) in [
            (
                "reminders",
                json!(crate::tools::schedule_whose::REMINDERS_KEPT),
                "reminders_result",
            ),
            ("name", json!(crate::person::NAME_KEPT), "setup_name_result"),
            (
                "place",
                json!(crate::owner_place::PLACE_KEPT),
                "place_result",
            ),
        ] {
            let mut result = json!({"forgotten": true, "everything": true,
                "about": "secret orchid", "note": "secret orchid"});
            result[field] = value;
            let kept = status(crate::remember::FORGET_EVERYTHING, &result);
            assert_eq!(kept["outcome"][key], "kept", "{kept}");
            assert_eq!(kept["outcome"]["forgotten"], true);
            assert_eq!(kept["outcome"]["everything"], false);
            let encoded = kept.to_string();
            assert!(!encoded.contains("secret orchid"));
            assert!(!encoded.contains(result[field].as_str().unwrap()));

            // A new/invalid result must stay uncertain, not be silently lost
            // or repeat arbitrary words from a tool result.
            result[field] = json!("secret orchid");
            let uncertain = status(crate::remember::FORGET_EVERYTHING, &result);
            assert_eq!(uncertain["outcome"][key], "uncertain");
            assert_eq!(uncertain["outcome"]["everything"], false);
            assert!(!uncertain.to_string().contains("secret orchid"));
        }
    }

    #[test]
    fn confirmed_and_unreported_deletion_steps_stay_distinct() {
        let kept = status(
            crate::remember::FORGET_EVERYTHING,
            &json!({"forgotten": true, "everything": true,
                "name": "forgotten too", "place": "forgotten too",
                "reminders": {"cancelled": 2, "items": ["secret orchid"]}}),
        );
        assert_eq!(
            kept["outcome"],
            json!({"forgotten": true, "everything": true,
            "setup_name_result": "forgotten", "place_result": "forgotten",
            "reminders_result": "cancelled"})
        );
        assert!(!kept.to_string().contains("secret orchid"));
        let absent = status("forget_everything", &json!({"forgotten": true}));
        for key in ["setup_name_result", "place_result", "reminders_result"] {
            assert!(absent["outcome"].get(key).is_none());
        }
        // Other calls' name/place fields are not deletion outcomes.
        let other = status(
            "recall",
            &json!({"name": "secret orchid", "place": "secret orchid"}),
        );
        assert_eq!(other["outcome"], json!({}));
    }

    #[test]
    fn existing_kept_parts_also_preclude_an_everything_claim() {
        for fields in [
            json!({"setup_name": crate::remember::SETUP_NAME_KEPT}),
            json!({"face": crate::remember::MEMORY_FACE_KEPT}),
            json!({"face_note": "secret orchid"}),
        ] {
            let mut result = json!({"forgotten": true, "everything": true});
            result
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let kept = status("forget_everything", &result);
            assert_eq!(kept["outcome"]["everything"], false);
            assert!(!kept.to_string().contains("secret orchid"));
        }
    }

    #[test]
    fn partial_room_forget_and_unknown_tool_names_are_content_free() {
        let result = json!({"forgotten": true, "all": false, "scope": "room",
            "missed": "secret orchid", "left": 2, "about": "secret orchid"});
        let kept = status("forget", &result);
        assert_eq!(
            kept["outcome"],
            json!({"forgotten": true, "all": false,
            "scope": "room", "some_subjects_not_found": true, "some_items_left": true})
        );
        assert!(!kept.to_string().contains("secret orchid"));
        let unknown = status("secret_orchid", &json!({"error": "secret orchid"}));
        assert_eq!(
            unknown,
            json!({"tool": "other_call", "outcome": {"error": true}})
        );
        let empty = status("forget", &json!({"forgotten": false, "scope": "room"}));
        assert_eq!(
            empty["outcome"],
            json!({"forgotten": false, "scope": "room"})
        );
    }

    #[test]
    fn partial_and_uncertain_results_keep_no_personal_payload() {
        let result = json!({
            "forgotten": true,
            "everything": false,
            "face": crate::remember::MEMORY_FACE_KEPT,
            "setup_name": crate::remember::SETUP_NAME_KEPT,
            "about": "secret orchid",
            "note": "secret orchid",
            "error": "secret orchid",
            "source_id": 17,
        });
        let kept = status(crate::remember::FORGET_EVERYTHING, &result);
        assert_eq!(
            kept["outcome"],
            json!({
                "forgotten": true, "everything": false, "face_forgotten": false,
                "setup_name_kept": true, "error": true,
            })
        );
        assert!(!kept.to_string().contains("secret orchid"));
        assert!(kept.get("source_id").is_none());
        assert_eq!(status("forget", &json!({}))["outcome"], json!({}));
        assert_eq!(
            status(
                "forget_everything",
                &json!({
                    "forgotten": false, "face": "forgotten too"
                })
            )["outcome"],
            json!({"forgotten": false, "face_forgotten": true})
        );
    }
}
