//! Which first audio is told on `lamp/did` as the voice's own line
//! (`LAMP_DID_VOICE`, #7757): only a line it began itself, never a person's
//! answer, a skill's say or a reminder.
use super::*;
use crate::did_voice::Moment;

#[test]
fn only_a_line_the_voice_began_itself_is_told() {
    let (_, mut reply, _) = active_reply();
    let turn = reply.owner.turn();
    reply.input_sequence = 0;
    reply.news.clear();
    reply.say = None;
    reply.reminder = None;
    // An answer to nobody's line: nothing.
    assert_eq!(did_voice_moment(&reply, None), None);
    // The walk's step line of this turn, and the update line.
    for moment in [Moment::Onboarding(Some(2)), Moment::UpdateLine] {
        assert_eq!(did_voice_moment(&reply, Some((turn, moment))), Some(moment));
    }
    // A line kept for another turn is not this one's.
    assert_eq!(
        did_voice_moment(&reply, Some((turn + 1, Moment::UpdateLine))),
        None
    );
    // News the voice tells itself.
    reply.news = vec!["a task".into()];
    assert_eq!(did_voice_moment(&reply, None), Some(Moment::TaskNews));
    // A person's words answered: never.
    reply.input_sequence = 3;
    assert_eq!(did_voice_moment(&reply, None), None);
    assert_eq!(
        did_voice_moment(&reply, Some((turn, Moment::UpdateLine))),
        None
    );
    // A reminder is not told here.
    reply.input_sequence = 0;
    let (receipt, _probe) = crate::reminders::receipt_for_test(reply.owner, 1, 1);
    reply.reminder = Some(receipt);
    assert_eq!(did_voice_moment(&reply, None), None);
}
