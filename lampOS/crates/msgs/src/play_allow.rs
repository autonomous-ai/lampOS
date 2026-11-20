//! What the lamp's cloud tells `lamp-control` about one play (#7415: the lamp
//! plays a URL): which asker may play which cloud answer, and every host the
//! play may touch, redirect hops and tracker hosts included. lamp-cloud makes
//! the cloud call for an app, and before it gives the app the answer (an
//! episode's URL) it sends this to lamp-control, which accepts a
//! `speaker.play_url` only for that asker, that `play_id` and a URL whose host
//! is in the set (`lamp-play` checks every hop again).
//!
//! **It is not a bus message, on purpose.** A bus topic's `source` is a name
//! the publisher writes into its own header, and every `lampos` unit (22 of
//! them, among them the ones that start skill and agent code) may publish to
//! any topic (the bus's directories are 0770 and its sockets 0660, one
//! group), so a subscriber cannot know who published and the bus cannot keep
//! a grant to one publisher. It goes instead as a request on lamp-control's
//! own socket, `{"play_allow": <this>}`, which lamp-control accepts from
//! lamp-cloud's cgroup alone (`SO_PEERCRED`'s pid, `/proc/<pid>/cgroup`, as
//! it does for `app_state` and the apps' yes). This type is only the checked
//! payload, shared by the two parts. It holds no audio and no personal data,
//! and the sets are kept in memory only.
use crate::Invalid;
use serde::{Deserialize, Serialize};

/// An app's name as a manifest gives it (`^[a-z][a-z0-9-]{0,23}$`).
fn app_ok(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 24
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A paired computer's id: `c_` and 8 to 32 lower-case hex digits.
fn computer_ok(name: &str) -> bool {
    name.strip_prefix("c_").is_some_and(|hex| {
        (8..=32).contains(&hex.len())
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// A plain lower-case DNS name: labels of `a-z`, `0-9` and `-` (not first or
/// last), 1 to 63 bytes each, 253 in all, at least two labels, a last label
/// that is not a number (so never an address).
fn host_ok(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && !labels
            .last()
            .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()))
}

/// Which asker may play which cloud answer, and where it may go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayAllow {
    /// The asker: a Store app's manifest name, or a paired computer's id.
    pub app: String,
    /// The cloud answer's id for this play, from 1; the app passes it back in
    /// `speaker.play_url`.
    pub play_id: u64,
    /// Every host the play may touch (exact names), redirect hops included.
    /// Empty withdraws that `play_id`.
    pub hosts: Vec<String>,
    /// How long the set holds, from the message's receipt on lamp-control's
    /// own monotonic clock: 1 to [`PlayAllow::MAX_TTL_MS`]. Ignored when
    /// `hosts` is empty.
    pub ttl_ms: u32,
    /// The exact URL, when the cloud pins it: `speaker.play_url`'s must equal
    /// it. Without it any URL on an allowed host is taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl PlayAllow {
    pub const MAX_HOSTS: usize = 16;
    pub const MAX_TTL_MS: u32 = 600_000;
    pub const MAX_URL: usize = 2_048;

    pub fn check(&self) -> Result<(), Invalid> {
        if !app_ok(&self.app) && !computer_ok(&self.app) {
            return Err(Invalid(
                "a play's asker is an app's name or a paired computer's id".into(),
            ));
        }
        if self.play_id == 0 {
            return Err(Invalid("a play's id starts at 1".into()));
        }
        if self.hosts.len() > Self::MAX_HOSTS
            || !self.hosts.iter().all(|host| host_ok(host))
            || self
                .hosts
                .iter()
                .enumerate()
                .any(|(at, host)| self.hosts[..at].contains(host))
        {
            return Err(Invalid(
                "a play's hosts are at most 16 plain lower-case names, each once".into(),
            ));
        }
        if !self.hosts.is_empty() && !(1..=Self::MAX_TTL_MS).contains(&self.ttl_ms) {
            return Err(Invalid("a play's set holds 1 ms to 10 minutes".into()));
        }
        if let Some(url) = &self.url
            && (url.len() > Self::MAX_URL
                || !url.starts_with("https://")
                || url.chars().any(|c| c.is_control()))
        {
            return Err(Invalid("a pinned URL is https, short and one line".into()));
        }
        Ok(())
    }

    /// Whether this message withdraws the play's set.
    pub fn withdraws(&self) -> bool {
        self.hosts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> PlayAllow {
        PlayAllow {
            app: "radio-drama".into(),
            play_id: 7,
            hosts: vec![
                "traffic.example.fm".into(),
                "cdn.tracker.example.com".into(),
            ],
            ttl_ms: 600_000,
            url: None,
        }
    }

    #[test]
    fn a_whole_message_passes_and_round_trips_without_a_url_field_when_there_is_none() {
        assert_eq!(allow().check(), Ok(()));
        let text = serde_json::to_string(&allow()).unwrap();
        assert!(!text.contains("url"), "{text}");
        assert_eq!(serde_json::from_str::<PlayAllow>(&text).unwrap(), allow());
        let pinned = PlayAllow {
            url: Some("https://traffic.example.fm/a.mp3".into()),
            ..allow()
        };
        assert_eq!(pinned.check(), Ok(()));
        assert_eq!(
            serde_json::from_str::<PlayAllow>(&serde_json::to_string(&pinned).unwrap()).unwrap(),
            pinned
        );
        for asker in [
            "radio-drama",
            "a",
            "c_0123abcd",
            "c_0123456789abcdef0123456789abcdef",
        ] {
            assert_eq!(
                PlayAllow {
                    app: asker.into(),
                    ..allow()
                }
                .check(),
                Ok(()),
                "{asker}"
            );
        }
    }

    #[test]
    fn an_unknown_field_or_a_missing_one_is_refused_at_the_parse() {
        let extra = r#"{"app":"x","play_id":1,"hosts":[],"ttl_ms":1,"more":1}"#;
        assert!(serde_json::from_str::<PlayAllow>(extra).is_err());
        assert!(serde_json::from_str::<PlayAllow>(r#"{"app":"x","play_id":1}"#).is_err());
    }

    #[test]
    fn what_is_out_of_range_is_invalid() {
        let bad: Vec<(&str, PlayAllow)> = vec![
            (
                "asker caps",
                PlayAllow {
                    app: "Radio".into(),
                    ..allow()
                },
            ),
            (
                "asker empty",
                PlayAllow {
                    app: String::new(),
                    ..allow()
                },
            ),
            (
                "asker long",
                PlayAllow {
                    app: "a".repeat(25),
                    ..allow()
                },
            ),
            (
                "asker bad id",
                PlayAllow {
                    app: "c_xyz".into(),
                    ..allow()
                },
            ),
            (
                "asker short id",
                PlayAllow {
                    app: "c_abc".into(),
                    ..allow()
                },
            ),
            (
                "play id 0",
                PlayAllow {
                    play_id: 0,
                    ..allow()
                },
            ),
            (
                "host address",
                PlayAllow {
                    hosts: vec!["93.184.216.34".into()],
                    ..allow()
                },
            ),
            (
                "host caps",
                PlayAllow {
                    hosts: vec!["Cdn.example.com".into()],
                    ..allow()
                },
            ),
            (
                "host wildcard",
                PlayAllow {
                    hosts: vec!["*.example.com".into()],
                    ..allow()
                },
            ),
            (
                "host trailing dot",
                PlayAllow {
                    hosts: vec!["example.com.".into()],
                    ..allow()
                },
            ),
            (
                "host one label",
                PlayAllow {
                    hosts: vec!["localhost".into()],
                    ..allow()
                },
            ),
            (
                "host twice",
                PlayAllow {
                    hosts: vec!["a.example.com".into(), "a.example.com".into()],
                    ..allow()
                },
            ),
            (
                "too many hosts",
                PlayAllow {
                    hosts: (0..17).map(|n| format!("h{n}.example.com")).collect(),
                    ..allow()
                },
            ),
            (
                "ttl 0",
                PlayAllow {
                    ttl_ms: 0,
                    ..allow()
                },
            ),
            (
                "ttl over",
                PlayAllow {
                    ttl_ms: PlayAllow::MAX_TTL_MS + 1,
                    ..allow()
                },
            ),
            (
                "url http",
                PlayAllow {
                    url: Some("http://a.example.com/x".into()),
                    ..allow()
                },
            ),
            (
                "url long",
                PlayAllow {
                    url: Some(format!("https://a.example.com/{}", "x".repeat(2_048))),
                    ..allow()
                },
            ),
            (
                "url newline",
                PlayAllow {
                    url: Some("https://a.example.com/x\ny".into()),
                    ..allow()
                },
            ),
        ];
        for (what, message) in bad {
            assert!(message.check().is_err(), "{what}");
        }
    }

    #[test]
    fn no_hosts_withdraws_the_set_and_needs_no_ttl() {
        let withdraw = PlayAllow {
            hosts: Vec::new(),
            ttl_ms: 0,
            ..allow()
        };
        assert_eq!(withdraw.check(), Ok(()));
        assert!(withdraw.withdraws() && !allow().withdraws());
    }
}
