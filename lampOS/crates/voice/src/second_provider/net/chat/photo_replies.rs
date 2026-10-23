//! The real request narrows the shared forget to a pending-photo refusal.
use super::*;

#[test]
fn a_held_chat_declares_only_the_pending_photo_form_of_forget() {
    for photos in [false, true] {
        let chat = Chat::with(
            "http://127.0.0.1:9/v1",
            "model",
            "synthetic",
            Arc::new(move || {
                crate::known_faces::tools_for(crate::known_faces::FaceSwitches {
                    checked: true,
                    visits: false,
                    photos,
                })
            }),
        )
        .unwrap();
        let without = [crate::known_faces::REMEMBER, crate::known_faces::FORGET];
        let body = chat.body_scoped(&[Message::user("No.")], None, &without, true);
        let tools = body.get("tools").and_then(Value::as_array);
        if photos {
            let tools = tools.unwrap();
            assert_eq!(tools.len(), 1);
            let function = &tools[0]["function"];
            assert_eq!(function["name"], crate::known_faces::FORGET);
            assert_eq!(function["parameters"]["required"], json!(["photo_reply"]));
            assert_eq!(
                function["parameters"]["properties"],
                json!({"photo_reply": {"type": "boolean", "enum": [true]}})
            );
            assert_eq!(function["parameters"]["additionalProperties"], false);
            assert!(
                function["description"]
                    .as_str()
                    .unwrap()
                    .contains("cannot forget any known face")
            );
        } else {
            assert!(tools.is_none());
        }
        // Ordinary `send_without` keeps its exact old removal contract.
        assert!(
            chat.body_without(&[], None, &without)
                .get("tools")
                .is_none()
        );
    }
}
