//! A person's own model key (#8362, #8420, #8735, `docs/own-keys.md`): which
//! of the kept keys is the lamp's model, and the endpoint each named service
//! is asked at. Read here by its three users, root's keys helper (`lamp-setup
//! keys`, the check), the voice (its chat) and lamp-cloud's forwarder (the
//! agent's chat), so they never disagree on which key is in use or where it
//! goes.

/// OpenRouter's OpenAI-compatible API: fixed, so its key goes nowhere else.
pub const OPENROUTER_API: &str = "https://openrouter.ai/api/v1";
/// Google's Gemini API, as the voice is given it: its OpenAI-compatible chat
/// is under [`GEMINI_OPENAI_API`].
pub const GEMINI_API: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Gemini's OpenAI-compatible API, `<GEMINI_API>/openai`: its chat and its
/// models.
pub const GEMINI_OPENAI_API: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
/// Anthropic's API, whose OpenAI-compatible chat is `<it>/chat/completions`.
pub const ANTHROPIC_API: &str = "https://api.anthropic.com/v1";
/// DeepSeek's own OpenAI-compatible API, when the person names no endpoint.
pub const DEEPSEEK_API: &str = "https://api.deepseek.com";

/// A service a person's own model key is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// Any OpenAI-compatible endpoint, its URL and model kept beside the key.
    OpenAiCompatible,
    OpenRouter,
    Gemini,
    Anthropic,
    /// DeepSeek's own, or the endpoint the person gave beside the key.
    DeepSeek,
}

/// Which key is the lamp's model when more than one is somehow kept: the
/// first here (the helper keeps one at a time, #8362).
pub const PRECEDENCE: [Model; 5] = [
    Model::OpenAiCompatible,
    Model::OpenRouter,
    Model::Gemini,
    Model::Anthropic,
    Model::DeepSeek,
];

impl Model {
    /// The service's name, as the app and the reply say it and as its files
    /// are named in the own folder (`<id>.key`, `<id>.model`, `<id>.url`,
    /// `<id>.checked`).
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::OpenRouter => "openrouter",
            Self::Gemini => "gemini",
            Self::Anthropic => "anthropic",
            Self::DeepSeek => "deepseek",
        }
    }

    /// The endpoint the service is asked at, as the voice is given it: fixed
    /// for the named services, DeepSeek's own when no URL is kept, and none
    /// for an OpenAI-compatible one, whose URL is kept beside its key.
    pub fn base(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => None,
            Self::OpenRouter => Some(OPENROUTER_API),
            Self::Gemini => Some(GEMINI_API),
            Self::Anthropic => Some(ANTHROPIC_API),
            Self::DeepSeek => Some(DEEPSEEK_API),
        }
    }

    /// Where `<it>/chat/completions` is asked for `base` ([`Self::base`] or
    /// the kept URL): Gemini's under `/openai`, any other as given.
    pub fn chat_base(self, base: &str) -> String {
        let base = base.trim_end_matches('/');
        match self {
            Self::Gemini => format!("{base}/openai"),
            _ => base.to_owned(),
        }
    }

    /// Whether a model's name is kept beside the key (all but DeepSeek's,
    /// which the voice asks by its own name).
    pub fn has_model_file(self) -> bool {
        self != Self::DeepSeek
    }
}

/// A model's name as it goes in a request's body: 1 to 128 letters, digits,
/// `-`, `.`, `_`, `/` and `:`.
pub fn fits_model(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':'))
}

/// A service URL as the voice takes one: https, a host, no `?`, `#` or `@`,
/// at most 2048 bytes.
pub fn https_url(url: &str) -> bool {
    let host = url
        .strip_prefix("https://")
        .map(|rest| rest.split('/').next().unwrap_or(""));
    url.len() <= 2048 && !url.contains(['?', '#', '@']) && host.is_some_and(|host| !host.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geminis_chat_is_under_its_openai_path() {
        assert_eq!(Model::Gemini.chat_base(GEMINI_API), GEMINI_OPENAI_API);
        assert_eq!(Model::OpenRouter.chat_base(OPENROUTER_API), OPENROUTER_API);
        assert_eq!(
            Model::OpenAiCompatible.chat_base("https://models.example.com/v1/"),
            "https://models.example.com/v1"
        );
    }

    #[test]
    fn the_precedence_is_compatible_then_the_named_then_deepseek() {
        let ids: Vec<_> = PRECEDENCE.iter().map(|model| model.id()).collect();
        assert_eq!(
            ids,
            [
                "openai_compatible",
                "openrouter",
                "gemini",
                "anthropic",
                "deepseek"
            ]
        );
    }

    #[test]
    fn only_https_urls_with_a_host() {
        assert!(https_url("https://models.example.com/v1"));
        for url in [
            "http://models.example.com",
            "https://",
            "https://a.example.com/?x",
            "https://user@a.example.com",
        ] {
            assert!(!https_url(url), "{url}");
        }
    }
}
