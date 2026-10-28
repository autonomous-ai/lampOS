//! `play`'s moves: the lamp's own recorded moves, the ones lampOS's skills
//! ship (`skills/*/moves/`), by name. Each is built in here, sent whole on
//! the mood socket as the skill sends it, and held to the body's own check
//! first (`lamp_body::moves::check`), so the door never takes a move the
//! body would drop (#4209). The body's built-in clips are `gesture`'s.

use lamp_body::moves::Move;

/// Each move's name and its file, as the skill ships it.
pub const PLAYS: [(&str, &str); 6] = [
    (
        "glad-lift",
        include_str!("../../../skills/feelings/moves/glad-lift.json"),
    ),
    (
        "perk",
        include_str!("../../../skills/feelings/moves/perk.json"),
    ),
    (
        "low-droop",
        include_str!("../../../skills/feelings/moves/low-droop.json"),
    ),
    (
        "sway",
        include_str!("../../../skills/music-sway/moves/sway.json"),
    ),
    (
        "watch-left",
        include_str!("../../../skills/watch-you-leave/moves/watch-left.json"),
    ),
    (
        "watch-right",
        include_str!("../../../skills/watch-you-leave/moves/watch-right.json"),
    ),
];

/// The move called `name`, if the lamp has one.
pub fn named(name: &str) -> Option<Move> {
    let (_, file) = PLAYS.iter().find(|(play, _)| *play == name)?;
    serde_json::from_str(file).ok()
}

/// Every name, for a refusal's words.
pub fn names() -> String {
    PLAYS.map(|(name, _)| name).join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each is the skill's file, named as it is, and one this body plays.
    #[test]
    fn every_move_is_the_skills_own_and_the_body_plays_it() {
        for (name, _) in PLAYS {
            let file = named(name).unwrap_or_else(|| panic!("{name} parses"));
            assert_eq!(file.name, name);
            lamp_body::moves::check(&file).unwrap_or_else(|why| panic!("{name}: {why}"));
        }
        assert_eq!(named("wave"), None);
        assert_eq!(named("nod"), None, "the body's clips are gesture's");
    }
}
