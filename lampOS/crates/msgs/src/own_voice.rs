//! Where a person's own ElevenLabs key and voice ID go (#8367): ElevenLabs'
//! own API, or, on a test lamp only, the address `LAMP_OWN_ELEVENLABS_URL`
//! names (#8394's lamp row, the lead: the relay's address with its key and
//! voice), read only under its gate, the voice's switch for test addresses
//! (#8394's P2, the first builder). Read here for both its users, root's
//! keys helper (`lamp-setup keys`, the check) and the voice (the speech),
//! so they never disagree. Unset, or without the gate, both use
//! [`ELEVENLABS_API`] exactly as before.

/// The test lamps' switch: an https address the person's own ElevenLabs key
/// and voice ID are sent to in place of ElevenLabs' own API, under [`GATE`].
/// It sends the person's key there: never set on a customer's lamp.
pub const ENVIRONMENT: &str = "LAMP_OWN_ELEVENLABS_URL";

/// The switch's gate (#8394's P2, the first builder: the switch alone could
/// send a person's key to any https host): the voice's switch for test
/// addresses, lamp-voice's `second_config::LOOPBACK_SERVICES_ENV`, `on`
/// exactly. No product unit sets it or [`ENVIRONMENT`] (lamp-voice's tests
/// read every unit for both), and without it the switch is ignored, so the
/// person's key and voice go to [`ELEVENLABS_API`] alone.
pub const GATE: &str = "LAMP_VOICE_LOOPBACK_SERVICES";

/// ElevenLabs' own API.
pub const ELEVENLABS_API: &str = "https://api.elevenlabs.io/v1";

/// The model the check at a switched address asks for its one word: the
/// voice's ElevenLabs default (`net::tts::ELEVENLABS_MODEL`).
pub const CHECK_MODEL: &str = "eleven_flash_v2_5";

/// The address the switch names, `var` giving each variable of the unit's
/// environment: an https URL, with [`GATE`] `on` exactly; `None` unset,
/// empty, anything else, or without the gate.
pub fn switched(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    if var(GATE).as_deref() != Some("on") {
        return None;
    }
    var(ENVIRONMENT)
        .filter(|url| url.starts_with("https://") && url.len() > "https://".len())
        .map(|url| url.trim_end_matches('/').to_owned())
}

/// The switch from this process's environment ([`switched`]).
pub fn switched_from_environment() -> Option<String> {
    switched(|name| std::env::var(name).ok())
}

/// How a key and voice ID are checked at `switched` (`None`: ElevenLabs'
/// own API): ElevenLabs' `GET <api>/voices/<id>`; at a switched address,
/// which may be the relay (no `/voices` there: 404 on 23 Nov), one word
/// said as the voice says it, `POST <root>/text-to-speech/<id>/stream`, the
/// root as [`root`] gives it. `(post, url)`.
pub fn check(switched: Option<&str>, voice_id: &str) -> (bool, String) {
    match switched {
        None => (false, format!("{ELEVENLABS_API}/voices/{voice_id}")),
        Some(base) => (
            true,
            format!(
                "{}/text-to-speech/{voice_id}/stream?output_format=pcm_24000",
                root(base)
            ),
        ),
    }
}

/// ElevenLabs' API root at `base` as the voice speaks to it
/// (`net::tts::service_base`): as given on ElevenLabs' own hosts, else the
/// relay's `/elevenlabs`, under the `/v1` the company relay's `/ai` takes
/// (`net::openai_base`).
pub fn root(base: &str) -> String {
    let mut base = base.trim_end_matches('/').to_owned();
    if base.contains("campaign-api.autonomous.ai") && base.ends_with("/ai") {
        base.push_str("/v1");
    }
    let host = base
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', ':']).next())
        .unwrap_or_default();
    if ELEVENLABS_HOSTS
        .iter()
        .any(|own| host.eq_ignore_ascii_case(own))
    {
        base
    } else {
        format!("{base}/elevenlabs")
    }
}

/// ElevenLabs' own hosts (`net::tts::ELEVENLABS_HOSTS`).
pub const ELEVENLABS_HOSTS: &[&str] = &[
    "api.elevenlabs.io",
    "api.us.elevenlabs.io",
    "api.eu.elevenlabs.io",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The environment of a unit that sets `set` alone.
    fn unit(set: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let set: Vec<(String, String)> = set
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| {
            set.iter()
                .find(|(set, _)| set == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn the_switch_names_an_https_address_or_nothing() {
        let gated = |url: &str| switched(unit(&[(GATE, "on"), (ENVIRONMENT, url)]));
        assert_eq!(switched(unit(&[(GATE, "on")])), None);
        assert_eq!(gated(""), None);
        assert_eq!(gated("http://relay.example.invalid/api/v1/ai"), None);
        assert_eq!(gated("relay.example.invalid"), None);
        assert_eq!(
            gated("https://relay.example.invalid/api/v1/ai/").as_deref(),
            Some("https://relay.example.invalid/api/v1/ai")
        );
    }

    /// #8394's P2 (the first builder): without its gate `on` exactly, the
    /// switch is ignored whatever it names, and the person's key and voice
    /// are checked at ElevenLabs' own API, where the speech goes too.
    #[test]
    fn without_its_gate_the_switch_is_ignored() {
        let at = "https://relay.example.invalid/api/v1/ai";
        for gate in [
            None,
            Some(""),
            Some("ON"),
            Some("On"),
            Some("1"),
            Some("true"),
            Some(" on"),
            Some("on "),
        ] {
            let mut set = vec![(ENVIRONMENT, at)];
            set.extend(gate.map(|gate| (GATE, gate)));
            let named = switched(unit(&set));
            assert_eq!(named, None, "{gate:?}");
            assert_eq!(
                check(named.as_deref(), "Voice8394"),
                (
                    false,
                    "https://api.elevenlabs.io/v1/voices/Voice8394".to_owned()
                ),
                "{gate:?}"
            );
        }
    }

    #[test]
    fn unswitched_the_check_asks_elevenlabs_for_the_voice_as_before() {
        assert_eq!(
            check(None, "Voice8367"),
            (
                false,
                "https://api.elevenlabs.io/v1/voices/Voice8367".to_owned()
            )
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
