//! The voice's side of an installed app's spoken triggers (#7415, piece 6).
//! lamp-control keeps what each running app's manifest says it is called by
//! (`apps/triggers`, the newest kept); the voice, holding a final line that
//! says one, tells control which (`apps/trigger`, once per line) and waits a
//! moment for the app to take it (`apps/took`, from control). All three are
//! local to the lamp; none carries audio.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// An app's name as a manifest gives it (`^[a-z][a-z0-9-]{0,23}$`).
fn name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 24
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A trigger as a manifest gives it (apps.md): at most 64 bytes, one line,
/// at least 8 letters.
fn phrase_ok(phrase: &str) -> bool {
    phrase.len() <= 64
        && !phrase.contains('\n')
        && phrase.chars().filter(|c| c.is_alphabetic()).count() >= 8
}

/// The triggers of the apps running now, the newest kept (`apps/triggers`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTriggers {
    pub apps: Vec<AppsTriggers>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppsTriggers {
    pub app: String,
    pub triggers: Vec<String>,
}

impl AppTriggers {
    pub const TYPE: &str = "lamp_msgs/AppTriggers";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/triggers";
    pub const MAX_APPS: usize = 32;
    pub const MAX_TRIGGERS: usize = 8;

    /// Said by lamp-control at least every 10 s while apps run; a reading
    /// older than this is a dead publisher's, and the voice reads none (a
    /// dead lamp-control never costs a matched line a wait for nothing).
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(10))
            .lasting(Duration::from_secs(30))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.apps.len() > Self::MAX_APPS {
            return Err(Invalid("too many apps with triggers".into()));
        }
        for (index, app) in self.apps.iter().enumerate() {
            if !name_ok(&app.app) || self.apps[..index].iter().any(|seen| seen.app == app.app) {
                return Err(Invalid(
                    "an app's name is not a manifest name, or repeats".into(),
                ));
            }
            if app.triggers.len() > Self::MAX_TRIGGERS || !app.triggers.iter().all(|t| phrase_ok(t))
            {
                return Err(Invalid(
                    "an app's triggers are not within the manifest's".into(),
                ));
            }
        }
        Ok(())
    }
}

/// The voice holds a final line that says a trigger: `turn` is its request,
/// `phrase` the words heard, `trigger` the manifest's own (`apps/trigger`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTrigger {
    pub turn: u64,
    pub app: String,
    pub trigger: String,
    pub phrase: String,
    pub at_ms: u64,
}

impl AppTrigger {
    pub const TYPE: &str = "lamp_msgs/AppTrigger";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/trigger";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !name_ok(&self.app) || !phrase_ok(&self.trigger) || self.phrase.len() > 512 {
            return Err(Invalid(
                "an app trigger names an app, a trigger and words".into(),
            ));
        }
        Ok(())
    }
}

/// The app took the line: the voice ends that request and says nothing
/// (`apps/took`, from lamp-control; `turn` is the request it answers).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTook {
    pub turn: u64,
    pub app: String,
}

impl AppTook {
    pub const TYPE: &str = "lamp_msgs/AppTook";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/took";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !name_ok(&self.app) {
            return Err(Invalid("an app's name is not a manifest name".into()));
        }
        Ok(())
    }
}

pub fn app_trigger_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (AppTriggers::TOPIC, AppTriggers::declaration()),
        (AppTrigger::TOPIC, AppTrigger::declaration()),
        (AppTook::TOPIC, AppTook::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(app: &str, triggers: &[&str]) -> AppsTriggers {
        AppsTriggers {
            app: app.into(),
            triggers: triggers.iter().map(|t| (*t).into()).collect(),
        }
    }

    #[test]
    fn the_triggers_an_app_lists_are_the_manifests() {
        let ok = AppTriggers {
            apps: vec![one("english-lessons", &["start my english lesson"])],
        };
        assert!(ok.check().is_ok());
        let json = serde_json::to_string(&ok).unwrap();
        assert_eq!(serde_json::from_str::<AppTriggers>(&json).unwrap(), ok);
        for bad in [
            AppTriggers {
                apps: vec![one("Bad Name", &["start it now"])],
            },
            AppTriggers {
                apps: vec![one("a", &["x"]), one("a", &["y"])],
            },
            AppTriggers {
                apps: vec![one("a", &["   "])],
            },
            AppTriggers {
                apps: vec![one("a", &["z"; 9])],
            },
            AppTriggers {
                apps: vec![one("a", &[&"w".repeat(65)])],
            },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_trigger_and_a_took_name_an_app_and_a_turn_and_nothing_else() {
        let heard = AppTrigger {
            turn: 7,
            app: "english-lessons".into(),
            trigger: "start my english lesson".into(),
            phrase: "please start my english lesson".into(),
            at_ms: 1,
        };
        assert!(heard.check().is_ok());
        let took = AppTook {
            turn: 7,
            app: "english-lessons".into(),
        };
        assert!(took.check().is_ok());
        assert!(serde_json::from_str::<AppTook>(r#"{"turn":7,"app":"a","extra":1}"#).is_err());
        assert!(
            AppTook {
                turn: 7,
                app: "A".into()
            }
            .check()
            .is_err()
        );
    }
}
