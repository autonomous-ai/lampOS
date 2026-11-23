//! Where a person's own ElevenLabs key and voice ID go (#8367): ElevenLabs'
//! own API, or, on a test lamp only, the address `LAMP_OWN_ELEVENLABS_URL`
//! names (#8394's lamp row, the lead: the relay's address with its key and
//! voice). Read here for both its users, root's keys helper (`lamp-setup
//! keys`, the check) and the voice (the speech), so they never disagree.
//! Unset, both use [`ELEVENLABS_API`] exactly as before.

/// The test lamps' switch: an https address the person's own ElevenLabs key
/// and voice ID are sent to in place of ElevenLabs' own API. It sends the
/// person's key there: never set on a customer's lamp.
pub const ENVIRONMENT: &str = "LAMP_OWN_ELEVENLABS_URL";

/// ElevenLabs' own API.
pub const ELEVENLABS_API: &str = "https://api.elevenlabs.io/v1";

/// The model the check at a switched address asks for its one word: the
/// voice's ElevenLabs default (`net::tts::ELEVENLABS_MODEL`).
pub const CHECK_MODEL: &str = "eleven_flash_v2_5";

/// The address `var` (the switch's value) names, when it is an https URL;
/// `None` unset, empty or anything else.
pub fn switched(var: Option<&str>) -> Option<String> {
    let _ = var;
    None
}

/// The switch from this process's environment ([`switched`]).
pub fn switched_from_environment() -> Option<String> {
    switched(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// How a key and voice ID are checked at `switched` (`None`: ElevenLabs'
/// own API): ElevenLabs' `GET <api>/voices/<id>`; at a switched address,
/// which may be the relay (no `/voices` there: 404 on 23 Nov), one word
/// said as the voice says it, `POST <root>/text-to-speech/<id>/stream`, the
/// root as [`root`] gives it. `(post, url)`.
pub fn check(switched: Option<&str>, voice_id: &str) -> (bool, String) {
    let _ = switched;
    (false, format!("{ELEVENLABS_API}/voices/{voice_id}"))
}

/// ElevenLabs' API root at `base` as the voice speaks to it
/// (`net::tts::service_base`): as given on ElevenLabs' own hosts, else the
/// relay's `/elevenlabs`, under the `/v1` the company relay's `/ai` takes
/// (`net::openai_base`).
pub fn root(base: &str) -> String {
    base.trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_names_an_https_address_or_nothing() {
        assert_eq!(switched(None), None);
        assert_eq!(switched(Some("")), None);
        assert_eq!(switched(Some("http://relay.example.invalid/api/v1/ai")), None);
        assert_eq!(switched(Some("relay.example.invalid")), None);
        assert_eq!(
            switched(Some("https://relay.example.invalid/api/v1/ai/")).as_deref(),
            Some("https://relay.example.invalid/api/v1/ai")
        );
    }

    #[test]
    fn unswitched_the_check_asks_elevenlabs_for_the_voice_as_before() {
        assert_eq!(
            check(None, "Voice8367"),
            (false, "https://api.elevenlabs.io/v1/voices/Voice8367".to_owned())
        );
    }

    #[test]
    fn switched_the_check_asks_the_given_address_for_one_word_in_that_voice() {
        assert_eq!(
            check(Some("https://campaign-api.autonomous.ai/api/v1/ai"), "Voice8367"),
            (
                true,
                "https://campaign-api.autonomous.ai/api/v1/ai/v1/elevenlabs/text-to-speech/Voice8367/stream?output_format=pcm_24000"
                    .to_owned()
            )
        );
        assert_eq!(
            check(Some("https://relay.example.invalid/tts"), "Voice8367").1,
            "https://relay.example.invalid/tts/elevenlabs/text-to-speech/Voice8367/stream?output_format=pcm_24000"
        );
        assert_eq!(
            check(Some("https://api.eu.elevenlabs.io/v1"), "Voice8367").1,
            "https://api.eu.elevenlabs.io/v1/text-to-speech/Voice8367/stream?output_format=pcm_24000"
        );
    }
}
