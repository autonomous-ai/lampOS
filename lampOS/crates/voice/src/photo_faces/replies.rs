//! Photo consent shares the ordinary face tools without sharing their effects.
use super::*;
use crate::known_faces::{FORGET, REMEMBER};

pub const PARAMETER: &str = "photo_reply";

/// Only the photo-enabled schema changes; the ordinary schema is kept as is.
pub fn augment(tools: &mut [Tool]) {
    for tool in tools {
        let extra = match tool.name.as_str() {
            REMEMBER => {
                " If you just asked whether to remember their face added from a photo and they say yes, call this with photo_reply true and no other arguments. That confirms only that pending photo face; it never starts a new camera capture. Otherwise omit photo_reply and give their name as they said or spelled it."
            }
            FORGET => {
                " If they say no to your question about remembering their face added from a photo, call this with photo_reply true and no other arguments. That deletes only the pending photo face, leaving known faces unchanged. Otherwise omit photo_reply and give the name for their explicit request to forget a known face."
            }
            _ => continue,
        };
        tool.description.push_str(extra);
        if let Some(schema) = tool.parameters.as_mut() {
            schema["properties"][PARAMETER] = json!({
                "type": "boolean",
                "description": "True only for their answer to the question about a face added from a photo. Omit all other arguments then; otherwise omit this and give name."
            });
            schema["required"] = json!([]);
        }
    }
}

/// Presence of an attempted discriminator must not fall through to live capture.
pub fn requested(call: &ToolCall) -> bool {
    call.args
        .get(PARAMETER)
        .is_some_and(|value| value != &json!(false))
}

fn answer(call: &ToolCall) -> Result<bool, Value> {
    let yes = match call.name.as_str() {
        REMEMBER => true,
        FORGET => false,
        _ => return Err(crate::tools::unavailable()),
    };
    if call.args.get(PARAMETER) != Some(&json!(true))
        || call.args.as_object().is_none_or(|args| args.len() != 1)
    {
        return Err(
            json!({"error": "A photo answer needs photo_reply true and no other arguments. Nothing was changed."}),
        );
    }
    Ok(yes)
}

/// An exact narrow refusal; ordinary forget and mixed arguments are never one.
pub fn is_refusal(name: &str, args: &Value) -> bool {
    name == FORGET
        && args
            .as_object()
            .is_some_and(|args| args.len() == 1 && args.get(PARAMETER) == Some(&json!(true)))
}

/// On a gap-held turn only this pending-photo refusal may be declared.
pub fn held_refusal(tool: &Tool) -> Option<Tool> {
    (tool.name == FORGET
        && tool.parameters.as_ref()?.get("properties")?.get(PARAMETER).is_some())
    .then(|| Tool {
        name: FORGET.into(),
        description: "Only if they say no to your question about remembering their face added from a photo: call with photo_reply true and no other arguments. This only deletes that pending photo face. You cannot forget any known face on this turn.".into(),
        parameters: Some(json!({
            "type": "object",
            "properties": {PARAMETER: {"type": "boolean", "enum": [true]}},
            "required": [PARAMETER],
            "additionalProperties": false
        })),
        silent: false,
    })
}

/// Resolve the pending face in Eyes; no failure ever falls back to ordinary tools.
pub fn dispatch(
    socket: &Path,
    call: &ToolCall,
    enabled: bool,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<(Value, bool)> {
    if !requested(call) {
        return None;
    }
    if !enabled {
        return Some((crate::tools::unavailable(), false));
    }
    let yes = match answer(call) {
        Ok(yes) => yes,
        Err(error) => return Some((error, true)),
    };
    let Some(pending) = pending(socket).filter(|pending| pending.asked == Some(true)) else {
        return Some((
            json!({"error": "No asked photo face is at the lamp now, so nothing was changed."}),
            true,
        ));
    };
    // A revoked grant cannot keep a face. A refusal still deletes only the
    // unconfirmed print resolved above, even if its speaker has since left.
    if yes && super::speaker_for(&pending, request, lease).is_none() {
        return Some((crate::tools::unavailable(), false));
    }
    // Keep the resolved name across the exchange: a different pending face
    // must never become this answer's subject. Eyes rechecks presence on yes.
    let legacy = ToolCall {
        id: call.id.clone(),
        name: if yes { YES } else { NO }.into(),
        args: json!({}),
    };
    Some(super::run_named(socket, &legacy, &pending, request, lease))
}

/// The second route's hold instructions apply only to ordinary forgetting,
/// never to the separate refusal of a pending photo.
pub fn scope_forget_description(mut tools: Vec<Tool>) -> Vec<Tool> {
    for tool in &mut tools {
        if tool.name == FORGET
            && held_refusal(tool).is_some()
            && let Some(ordinary) = tool
                .description
                .strip_suffix(crate::remember::HELD_DESCRIBED)
        {
            tool.description = format!(
                "{ordinary} For an ordinary request to forget a known face only:{}",
                crate::remember::HELD_DESCRIBED
            );
        }
    }
    tools
}
