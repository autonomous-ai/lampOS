use super::*;

/// #7664: near the end of the lamp's day, a line let out only as the
/// conversation going on, surely by someone other than the caller, is
/// held; the caller's own follow-up, the same line with nothing to tell
/// the speakers apart (the owner whose face wasn't seen, the
/// Reviewer-lead's P2 on #7738), and a line with any other sign it was
/// said to the lamp (no note), still go; under the line nothing is held.
#[test]
fn near_the_day_end_only_a_line_let_out_on_the_conversation_alone_is_held() {
    use crate::day_shed::Level;
    for level in [Level::Near, Level::Spent] {
        assert!(day_alone_held(
            level,
            Some(worker::CONVERSATION_ALONE),
            true
        ));
        assert!(
            !day_alone_held(level, Some(worker::CONVERSATION_ALONE), false),
            "nothing seen or heard: the same person"
        );
        assert!(!day_alone_held(level, Some(worker::FOLLOW_UP_NOTE), true));
        assert!(!day_alone_held(level, None, true));
    }
    assert!(!day_alone_held(
        Level::Open,
        Some(worker::CONVERSATION_ALONE),
        true
    ));
}
