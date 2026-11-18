use super::*;

/// #7664: near the end of the lamp's day, a line let out only as the
/// conversation going on, by someone other than the caller, is held; the
/// caller's own follow-up, and a line with any other sign it was said to
/// the lamp (no note), still go; under the line nothing is held.
#[test]
fn near_the_day_end_only_a_line_let_out_on_the_conversation_alone_is_held() {
    use crate::day_shed::Level;
    for level in [Level::Near, Level::Spent] {
        assert!(day_alone_held(level, Some(worker::CONVERSATION_ALONE)));
        assert!(!day_alone_held(level, Some(worker::FOLLOW_UP_NOTE)));
        assert!(!day_alone_held(level, None));
    }
    assert!(!day_alone_held(
        Level::Open,
        Some(worker::CONVERSATION_ALONE)
    ));
}
