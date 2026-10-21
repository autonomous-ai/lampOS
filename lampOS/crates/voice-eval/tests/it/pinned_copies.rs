//! Values the Policy keeps as copies of the voice's, since `lamp-policy` does
//! not link `lamp-voice`: held equal here, in a crate that links both, so a
//! change to the voice's bar can't leave the copy counting by the old one
//! (Fourth on #1510).

/// `lamp-policy-replay --agreement` counts a model's `stay_quiet` with
/// `follows_lamp` at the voice's bar or more as taking the turn, as the voice
/// takes it (`StayQuietFollowing`). If the voice's bar moved and the copy
/// didn't, `--agreement` would call a kind certain on the old bar, and
/// `--rule-decides` could hold calls the model's decisions let through.
#[test]
fn the_policys_follows_from_is_the_voices() {
    assert_eq!(
        lamp_policy::replay::FOLLOWS_FROM,
        lamp_voice::policy_holds::FOLLOWS_FROM
    );
}
