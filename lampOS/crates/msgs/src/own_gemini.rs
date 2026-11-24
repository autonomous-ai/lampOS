//! Where a person's own Gemini key and model go (#8420): Google's Gemini API,
//! or, on a test lamp only, the address `LAMP_OWN_GEMINI_URL` names (the
//! #8420 lamp row: the relay's Gemini, `<base>/google-search/v1beta`, with the
//! relay's key, as #8394's `LAMP_OWN_ELEVENLABS_URL` does for the voice). Read
//! here for both its users, root's keys helper (`lamp-setup keys`, the check)
//! and the voice (the chat), so they never disagree. Unset, both use
//! [`GEMINI_ROOT`] exactly as before.

/// The test lamps' switch: an https address the person's own Gemini key and
/// model are sent to in place of Google's own API. It sends the person's key
/// there: never set on a customer's lamp.
pub const ENVIRONMENT: &str = "LAMP_OWN_GEMINI_URL";

/// Google's Gemini API root; its OpenAI-compatible endpoint is `/openai`
/// under it.
pub const GEMINI_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta";

/// The address `var` (the switch's value) names, when it is an https URL;
/// `None` unset, empty or anything else (`own_voice::switched`'s rule).
pub fn switched(var: Option<&str>) -> Option<String> {
    crate::own_voice::switched(var)
}

/// The switch from this process's environment ([`switched`]).
pub fn switched_from_environment() -> Option<String> {
    switched(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// The Gemini root the person's chat is asked at: the switch's address, else
/// [`GEMINI_ROOT`].
pub fn root(_switched: Option<&str>) -> &str {
    GEMINI_ROOT // red: the switch not yet followed
}

/// How a key and model are checked at `switched` (`None`: Google's own API):
/// the one model read, `GET <root>/openai/models/<model>`, as before; at a
/// switched address, which may be the relay (its Gemini serves the chat the
/// lamp asks; a model read there isn't known to be served), one token asked
/// of that model as the voice asks it, `POST <root>/openai/chat/completions`
/// with [`check_body`]. `(post, url)`.
pub fn check(switched: Option<&str>, model: &str) -> (bool, String) {
    match switched {
        None => (false, format!("{GEMINI_ROOT}/openai/models/{model}")),
        Some(_) => (false, format!("{GEMINI_ROOT}/openai/models/{model}")), // red
    }
}

/// The switched check's body: one word asked of `model`, one token answered.
pub fn check_body(model: &str) -> String {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hi."}],
        "max_tokens": 1,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_names_an_https_address_or_nothing() {
        assert_eq!(switched(None), None);
        assert_eq!(switched(Some("")), None);
        assert_eq!(switched(Some("http://relay.example.invalid/v1beta")), None);
        assert_eq!(
            switched(Some("https://relay.example.invalid/google-search/v1beta/")).as_deref(),
            Some("https://relay.example.invalid/google-search/v1beta")
        );
        assert_eq!(root(None), GEMINI_ROOT);
        assert_eq!(
            root(Some("https://relay.example.invalid/google-search/v1beta")),
            "https://relay.example.invalid/google-search/v1beta"
        );
    }

    #[test]
    fn unswitched_the_check_reads_the_one_model_at_google_as_before() {
        assert_eq!(
            check(None, "gemini-2.5-flash"),
            (
                false,
                "https://generativelanguage.googleapis.com/v1beta/openai/models/gemini-2.5-flash"
                    .to_owned()
            )
        );
    }

    #[test]
    fn switched_the_check_asks_the_given_address_for_one_token_of_the_model() {
        let at = "https://campaign-api.autonomous.ai/api/v1/ai/v1/google-search/v1beta";
        assert_eq!(
            check(Some(at), "gemini-2.5-flash"),
            (
                true,
                "https://campaign-api.autonomous.ai/api/v1/ai/v1/google-search/v1beta/openai/chat/completions"
                    .to_owned()
            )
        );
        let body: serde_json::Value =
            serde_json::from_str(&check_body("gemini-2.5-flash")).unwrap();
        assert_eq!(body["model"], "gemini-2.5-flash");
        assert_eq!(body["max_tokens"], 1);
        assert_eq!(body["messages"][0]["content"], "Hi.");
    }
}
