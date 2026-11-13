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
        || crate::rules::is_tool(name)
        || crate::action_claim::unplug_tool(name, &Value::Null).is_some()
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
    // A refusal that asked which (C134): its flag, never its note's words.
    if is_memory_forget(name) && result[crate::remember::ASK_WHICH] == true {
        outcome.insert("asked_which".into(), json!(true));
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
    if name == crate::known_faces::FORGET {
        if let Some(forgotten) = result["ok"].as_bool() {
            outcome.insert("face_forgotten".into(), json!(forgotten));
            incomplete |= !forgotten;
        }
        if let Some(note) = result.get("note") {
            let key = if note == crate::known_faces::FORGOTTEN_SOME_KEPT_APART {
                "old_face_records_kept_apart"
            } else {
                "face_cleanup_uncertain"
            };
            outcome.insert(key.into(), json!(true));
            incomplete = true;
        }
        if let Some(memory) = result.get("memory") {
            let state =
                if memory == "forgotten too" || memory == crate::remember::FACE_MEMORY_PARTIAL {
                    "forgotten"
                } else {
                    "uncertain"
                };
            incomplete |= state != "forgotten";
            outcome.insert("primary_memory_result".into(), json!(state));
        }
        // This inner path never performs the outer setup-name, place,
        // voice-print, journal and reminder cleanup. Retain a partial flag,
        // but never let face success claim aggregate deletion.
        if outcome.get("everything") == Some(&json!(true)) {
            outcome.remove("everything");
        }
    }
    if crate::remember::is_forget(name) {
        for field in [
            "room_memory_result",
            "sound_consent_result",
            "face_expression_consent_result",
        ] {
            if let Some(value) = result.get(field) {
                let state = match value.as_str() {
                    Some("cleared") => "cleared",
                    Some("kept") => "kept",
                    _ => "uncertain",
                };
                incomplete |= state != "cleared";
                outcome.insert(field.into(), json!(state));
            }
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
    let kind = if calls
        .last()
        .is_some_and(|call| call["tool"] == crate::known_faces::FORGET)
    {
        "face"
    } else {
        "memory"
    };
    vec![
        Message::system(instruction),
        Message::system(format!(
            "A {kind} forget call has run in this turn. The earlier conversation, recalled memory, pictures and tool payloads have been discarded. Answer only the person's current words and these content-free call outcomes: {}. Do not repeat any of these calls or reconstruct what was forgotten. Only confirm deletion when the outcome confirms it; an error or missing confirmation means the result is uncertain, not success. forgotten reports memory items only; false means none were removed. face_forgotten is independent and true confirms the face was forgotten, not that memory or every related record was removed. primary_memory_result=forgotten confirms primary memory only; uncertain means it could not be confirmed. old_face_records_kept_apart means old face records remain, and face_cleanup_uncertain means another face cleanup warning could not be interpreted. Neither is complete deletion. setup_name_result, place_result and reminders_result report those steps: forgotten/cancelled confirms that step, kept means it could not be cleared, uncertain means it was not confirmed. room_memory_result, sound_consent_result and face_expression_consent_result report cleanup of the room lines and the two household consents: cleared confirms that step, kept means it was refused, uncertain means completion was not confirmed. A missing step is unreported, never a confirmation. everything=false, all=false, some_subjects_not_found or some_items_left means do not claim complete deletion; distinguish a known kept part from an uncertain outcome. scope=room means only what this room remembers, never a person's private memory. asked_which=true means nothing was removed because it was not clear which thing: ask them which in a few words, and do not call forget again in this turn. Say briefly if a face, old face records, setup name, place, reminders, room lines or household consents could not be cleared. {left} later calls from the old round were not run; if the current words still ask for other work, plan it afresh without the discarded context. Never invent a fact or a reason that the status does not give.",
            serde_json::to_string(calls).expect("JSON values serialize"),
        )),
        question.clone(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thirteen's P2-1 on #5942: a rule's and a face's forget keep their
    /// built-in names and outcomes, which alone back a forget said done
    /// (`action_claim::forgot_in`).
    #[test]
    fn a_rule_or_face_forget_keeps_its_name_and_outcome() {
        let kept = status(
            crate::rules::FORGET_RULE,
            &json!({"forgotten": true, "note": "Stop following it from now on."}),
        );
        assert_eq!(
            kept,
            json!({"tool": crate::rules::FORGET_RULE, "outcome": {"forgotten": true}})
        );
        let kept = status(
            crate::known_faces::FORGET,
            &json!({"error": crate::known_faces::NOT_THEIRS}),
        );
        assert_eq!(
            kept,
            json!({"tool": crate::known_faces::FORGET, "outcome": {"error": true}})
        );
    }

    /// C134: a refusal that asked which keeps that flag, never its note,
    /// past the after-forget chat reset, and the chat says what it means:
    /// ask which, do not call forget again.
    #[test]
    fn a_forget_that_asked_which_stays_asked_past_the_reset() {
        for refused in [
            crate::remember::forget_which(true),
            crate::remember::forget_which(false),
            crate::remember::forget_nothing_mentioned(),
        ] {
            let kept = status(crate::remember::FORGET, &refused);
            assert_eq!(
                kept,
                json!({"tool": crate::remember::FORGET,
                    "outcome": {"forgotten": false, "asked_which": true}})
            );
        }
        // Not on another tool's answer, nor a plain refusal.
        assert!(
            status(crate::remember::REMEMBER, &json!({"ask_which": true}))["outcome"]
                .get("asked_which")
                .is_none()
        );
        assert!(
            status(crate::remember::FORGET, &json!({"forgotten": false}))["outcome"]
                .get("asked_which")
                .is_none()
        );
        let question = Message::user("Lamp, forget that.");
        let chat = context(
            "instructions",
            &question,
            &[status(
                crate::remember::FORGET,
                &crate::remember::forget_which(true),
            )],
            0,
        );
        let said = chat[1].content.as_ref().and_then(Value::as_str).unwrap();
        assert!(said.contains(r#""asked_which":true"#), "{said}");
        assert!(
            said.contains("asked_which=true means nothing was removed")
                && said.contains("do not call forget again in this turn"),
            "{said}"
        );
        // The refusal's words never pass the reset.
        assert!(!said.contains("graduation date"), "{said}");
    }

    #[test]
    fn cleanup_outcome_status_timing() {
        let result = json!({"forgotten":true,"everything":false,
            "room_memory_result":"uncertain", "sound_consent_result":"uncertain",
            "face_expression_consent_result":"cleared"});
        let mut samples = Vec::new();
        for _ in 0..1001 {
            let start = std::time::Instant::now();
            let kept = status("forget_everything", &result);
            samples.push(start.elapsed().as_nanos());
            assert_eq!(kept["outcome"]["everything"], false);
        }
        let first = samples.remove(0);
        samples.sort_unstable();
        eprintln!(
            "forget_cleanup_status_timing {}",
            json!({
                "boundary":"status entry to sanitized outcome; three cleanup fields",
                "first_ns":first,"repeated_count":1000,
                "p50_ns":samples[499],"p95_ns":samples[949],"max_ns":samples[999],
                "target_ns":1_000_000,
                "limits":"host debug build; deletion, IPC, process startup, model and lamp excluded"
            })
        );
    }

    #[test]
    fn cleanup_outcomes_keep_only_fixed_states_and_never_upgrade_partial_results() {
        for field in [
            "room_memory_result",
            "sound_consent_result",
            "face_expression_consent_result",
        ] {
            for (value, expected) in [
                (json!("cleared"), "cleared"),
                (json!("kept"), "kept"),
                (json!("uncertain"), "uncertain"),
                (json!("private canary"), "uncertain"),
                (json!({"path":"private canary"}), "uncertain"),
                (Value::Null, "uncertain"),
            ] {
                let mut result = json!({"forgotten":true,"everything":true});
                result[field] = value;
                let kept = status("forget_everything", &result);
                assert_eq!(kept["outcome"][field], expected);
                assert_eq!(kept["outcome"]["everything"], expected == "cleared");
                assert!(!kept.to_string().contains("private canary"));
                result["everything"] = json!(false);
                assert_eq!(
                    status("forget_everything", &result)["outcome"]["everything"],
                    false
                );
                assert!(
                    status("forget_everything", &json!({}))["outcome"]
                        .get(field)
                        .is_none()
                );
                assert!(status("recall", &result)["outcome"].get(field).is_none());
            }
        }
    }

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

#[cfg(test)]
mod face_context_tests {
    use super::*;

    /// Fresh-process host bookkeeping only; excludes tool execution and model I/O.
    #[test]
    #[ignore = "manual face context reset latency measurement"]
    fn face_forget_context_reset_timing() {
        let result = json!({"ok": true, "everything": false,
            "memory": crate::remember::FACE_MEMORY_PARTIAL,
            "room_memory_result": "kept", "sound_consent_result": "uncertain",
            "face_expression_consent_result": "cleared"});
        let question = Message::user("Forget my face.");
        let mut samples = Vec::new();
        for _ in 0..101 {
            let began = std::time::Instant::now();
            let outcome = status(crate::known_faces::FORGET, &result);
            let fresh =
                std::hint::black_box(context("Static instructions", &question, &[outcome], 1));
            samples.push(began.elapsed().as_nanos());
            assert_eq!(fresh.len(), 3);
        }
        let first = samples.remove(0);
        samples.sort_unstable();
        println!(
            "face_forget_context_timing {}",
            json!({
                "boundary": "receipt sanitizer entry to fresh context vector",
                "first_ns": first, "repeated_p95_ns": samples[94],
                "repeated_max_ns": samples[99], "repeated_count": 100,
                "target_ns": 1_000_000,
            })
        );
    }

    #[test]
    fn face_forget_context_receipts_preserve_steps_without_private_payloads() {
        let partial_memory = "Their face and primary memory are forgotten, but not all related cleanup was confirmed. Tell them which fixed cleanup outcomes are kept or uncertain; do not say everything was forgotten.";
        let result = json!({
            "ok": true,
            "everything": false,
            "memory": partial_memory,
            "note": crate::known_faces::FORGOTTEN_SOME_KEPT_APART,
            "room_memory_result": "kept",
            "sound_consent_result": "uncertain",
            "face_expression_consent_result": "cleared",
            "name": "private result canary",
            "private": "private result canary",
        });
        let kept = status(crate::known_faces::FORGET, &result);
        assert_eq!(kept["outcome"]["face_forgotten"], true);
        assert_eq!(kept["outcome"]["primary_memory_result"], "forgotten");
        assert_eq!(kept["outcome"]["old_face_records_kept_apart"], true);
        assert_eq!(kept["outcome"]["everything"], false);
        for field in [
            "room_memory_result",
            "sound_consent_result",
            "face_expression_consent_result",
        ] {
            assert_eq!(kept["outcome"][field], result[field]);
        }
        assert!(!kept.to_string().contains("private result canary"));
        assert!(kept["outcome"].get("memory").is_none());
        assert!(kept["outcome"].get("note").is_none());
    }

    #[test]
    fn face_forget_context_receipts_do_not_invent_memory_or_complete_deletion() {
        let absent = status(
            crate::known_faces::FORGET,
            &json!({"ok": true, "everything": true}),
        );
        assert_eq!(absent["outcome"]["face_forgotten"], true);
        assert!(absent["outcome"].get("primary_memory_result").is_none());
        assert_ne!(absent["outcome"]["everything"], true);
        let refused = status(crate::known_faces::FORGET, &json!({"ok": false}));
        assert_eq!(refused["outcome"]["face_forgotten"], false);
        let missing = status(crate::known_faces::FORGET, &json!({}));
        assert!(missing["outcome"].get("face_forgotten").is_none());
        for value in [
            json!("private result canary"),
            json!({"name": "private result canary"}),
            Value::Null,
            json!(crate::remember::FACE_MEMORY_KEPT),
        ] {
            let kept = status(
                crate::known_faces::FORGET,
                &json!({"ok": true, "memory": value, "note": "private result canary", "sound_consent_result": "private result canary"}),
            );
            assert_eq!(kept["outcome"]["primary_memory_result"], "uncertain");
            assert_eq!(kept["outcome"]["sound_consent_result"], "uncertain");
            assert_eq!(kept["outcome"]["face_cleanup_uncertain"], true);
            assert_ne!(kept["outcome"]["everything"], true);
            assert!(!kept.to_string().contains("private result canary"));
        }
        let complete = status(
            crate::known_faces::FORGET,
            &json!({"ok": true, "memory": "forgotten too"}),
        );
        assert_eq!(complete["outcome"]["primary_memory_result"], "forgotten");
        assert!(complete["outcome"].get("everything").is_none());
    }
}
