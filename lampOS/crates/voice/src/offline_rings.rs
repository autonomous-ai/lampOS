//! A timer or reminder that comes due with no link rings on time
//! (`LAMP_OFFLINE_RINGS`, #8632).
//!
//! On lamp-06c1 on 25 Nov a 4-minute timer set before a cut came due with
//! the link down and stayed silent for 409 s: its ring is a model turn, and
//! the turn's gate needs a ready cloud session (`unasked_session`). With
//! the switch, a due timer or reminder with the link down past its grace
//! ([`crate::bus_out::LINK_GRACE_US`]) is rung from a clip rendered ahead
//! into the speech cache, "Your timer is done." or "You have a reminder."
//! in the owner's language, as a turn of the lamp's own. A clip played
//! whole completes the item in Schedule, so nothing about it is said again
//! when the link returns. With no clip kept, the item waits for the link
//! as before.
//!
//! In the same outage, a follow-up scrap shorter than [`SCRAP_NO_LINE_MS`]
//! of speech, ended within [`SCRAP_AFTER_LINE_US`] of the outage's line,
//! gets no second "can't reach the internet" line: on lamp-06c1 the lamp's
//! own tail or the room after 6 of 10 lines was answered with the line
//! again. Such a turn only exists offline, so online nothing changes.
//!
//! On the second route as the lamp's only voice, the link reads down only
//! once a turn fails on the network, so in an outage where nobody spoke a
//! ring was first tried as a model turn and rang late. With the switch,
//! [`PROBE_BEFORE_DUE_US`] before the soonest ring is due, the voice asks
//! the worker once for the existing check (no service host answers,
//! `second_provider::link`), skipped when a turn or a check found the link
//! within [`LINK_CONFIRMED_WITHIN_US`]. It reads down before the due time,
//! so the ring plays from its clip then. Nothing is checked with no ring
//! due.
//!
//! Off (anything but exactly `on`), every path is as before.

/// Exactly `on` turns it on; anything else, or unset, is off.
pub const ENVIRONMENT: &str = "LAMP_OFFLINE_RINGS";

/// Offline, a follow-up with less speech than this (the endpoint's
/// `speech_ms`) just after the outage's line gets no line of its own. The
/// scraps on lamp-06c1 (25 Nov) were 100 to 330 ms; the shortest asks said
/// to a lamp are 340 ms and more (`not_speech::SCRAP_BLOCKS`' note). For
/// the owner's table.
pub const SCRAP_NO_LINE_MS: u64 = 400;

/// How long after the outage's line was last heard such a scrap is taken
/// for its tail: the scraps on lamp-06c1 came about 3 to 4.5 s after the
/// line, and ended under a second later. For the owner's table.
pub const SCRAP_AFTER_LINE_US: u64 = 8_000_000;

/// How long before the soonest ring is due the link is checked, once: the
/// check waits up to `second_provider::link::PROBE_WITHIN` (4 s) and the
/// link must then be down past its grace
/// ([`crate::bus_out::LINK_GRACE_US`], 5 s), both before the due time. For
/// the owner's table.
pub const PROBE_BEFORE_DUE_US: u64 = 10_000_000;

/// A turn completed, or a check that found a host answering, within this
/// long needs no check before a ring: the link was just seen. For the
/// owner's table.
pub const LINK_CONFIRMED_WITHIN_US: u64 = 60_000_000;

/// Whether the worker is asked now to check the link, for the soonest ring
/// not yet due (`soonest`: its key and when it is due): the switch on, it
/// due within [`PROBE_BEFORE_DUE_US`] and not yet, and not asked already
/// for it (`asked_for`). The worker skips the check within
/// [`LINK_CONFIRMED_WITHIN_US`] of the link found.
pub fn asks_link_check<K: PartialEq>(
    switch: bool,
    soonest: Option<(&K, u64)>,
    asked_for: Option<&K>,
    now_us: u64,
) -> bool {
    switch
        && soonest.is_some_and(|(key, due_at_us)| {
            now_us < due_at_us
                && due_at_us - now_us <= PROBE_BEFORE_DUE_US
                && asked_for != Some(key)
        })
}

/// Whether `value` turns the switch on: exactly `on`.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    on_from(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether a due timer or reminder is rung from its clip now: the switch
/// on, no cloud session ready, the link down past its grace (a session
/// renewing itself is not an outage), and nothing the lamp starts by itself
/// paused (the keeper's pause, signed out).
pub fn rings_offline(
    switch: bool,
    ready_session: Option<u64>,
    link_down_since: Option<u64>,
    now_us: u64,
    paused: bool,
) -> bool {
    switch
        && ready_session.is_none()
        && !paused
        && link_down_since
            .is_some_and(|since| now_us.saturating_sub(since) >= crate::bus_out::LINK_GRACE_US)
}

/// Whether an offline speech turn ending now is a scrap after the outage's
/// line, which gets no line of its own: the switch on, its speech known and
/// under [`SCRAP_NO_LINE_MS`], and the line heard within
/// [`SCRAP_AFTER_LINE_US`] before now. An unknown speech length is no scrap.
pub fn scrap_after_line(
    switch: bool,
    speech_ms: Option<u64>,
    line_heard_at_us: Option<u64>,
    now_us: u64,
) -> bool {
    switch
        && speech_ms.is_some_and(|ms| ms < SCRAP_NO_LINE_MS)
        && line_heard_at_us.is_some_and(|heard| now_us.saturating_sub(heard) <= SCRAP_AFTER_LINE_US)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_out::LINK_GRACE_US;

    #[test]
    fn only_exactly_on_turns_it_on() {
        assert!(on_from(Some("on")));
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some(" on"),
            Some("1"),
        ] {
            assert!(!on_from(value), "{value:?}");
        }
    }

    /// #8632: the 25 Nov timer stayed silent for 409 s with no session.
    #[test]
    fn a_due_ring_is_rung_from_its_clip_only_offline_past_the_grace_and_unpaused() {
        let down = Some(1_000_000);
        let past = 1_000_000 + LINK_GRACE_US;
        assert!(rings_offline(true, None, down, past, false));
        // Off: never.
        assert!(!rings_offline(false, None, down, past, false));
        // A ready session: the model turn rings it, as before.
        assert!(!rings_offline(true, Some(3), down, past, false));
        // The link not seen down, or within its grace: not yet.
        assert!(!rings_offline(true, None, None, past, false));
        assert!(!rings_offline(true, None, down, past - 1, false));
        // Paused by the keeper or signed out: nothing unasked begins.
        assert!(!rings_offline(true, None, down, past, true));
    }

    /// #8632, the first builder's call: once, about 10 s before a due ring,
    /// and nothing with no ring due.
    #[test]
    fn the_link_is_checked_once_ten_seconds_before_a_due_ring_and_never_with_none_due() {
        assert_eq!(PROBE_BEFORE_DUE_US, 10_000_000);
        let due = 100_000_000;
        let item = (4_u64, 1_u64);
        let soonest = Some((&item, due));
        // No ring due: never.
        assert!(!asks_link_check::<(u64, u64)>(true, None, None, due - 1));
        // Not yet within 10 s; then at 10 s and up to the due time.
        assert!(!asks_link_check(
            true,
            soonest,
            None,
            due - PROBE_BEFORE_DUE_US - 1
        ));
        assert!(asks_link_check(
            true,
            soonest,
            None,
            due - PROBE_BEFORE_DUE_US
        ));
        assert!(asks_link_check(true, soonest, None, due - 1));
        // Due already: the ring's own path, no check.
        assert!(!asks_link_check(true, soonest, None, due));
        // Once for each ring.
        assert!(!asks_link_check(
            true,
            soonest,
            Some(&item),
            due - 5_000_000
        ));
        let next = (5_u64, 1_u64);
        assert!(asks_link_check(
            true,
            Some((&next, due)),
            Some(&item),
            due - 5_000_000
        ));
        // Off: never.
        assert!(!asks_link_check(false, soonest, None, due - 5_000_000));
    }

    /// #8632: nobody spoke in the outage. The check asked 10 s before the
    /// ring reads down, and the ring plays from its clip at the due time.
    #[test]
    fn a_ring_whose_check_reads_down_plays_from_its_clip_at_due() {
        use crate::provider_link::ProviderLinkState;
        use crate::second_provider::link::{Link, PROBE_WITHIN};
        let due = 400_000_000;
        let item = (4_u64, 1_u64);
        let asked_at = due - PROBE_BEFORE_DUE_US;
        assert!(asks_link_check(true, Some((&item, due)), None, asked_at));
        let (mut link, _) = Link::initial(1_000_000, 1_000_010);
        assert!(link.ask(asked_at));
        // The slowest check: no host answers in all of its wait.
        let read = asked_at + PROBE_WITHIN.as_micros() as u64;
        let down = link.probed(asked_at, read, false).expect("down");
        assert!(matches!(down.state, ProviderLinkState::Unavailable { .. }));
        // The voice sees it down from then, and at the due time it is past
        // its grace: rung from the clip, not tried as a model turn.
        let since = Some(down.observed_at_us);
        assert!(rings_offline(true, None, since, due, false));
        assert!(PROBE_WITHIN.as_micros() as u64 + LINK_GRACE_US <= PROBE_BEFORE_DUE_US);
    }

    /// #8632: after 6 of 10 offline lines a 100 to 330 ms scrap 3 to 4.5 s
    /// later was answered with the internet line again.
    #[test]
    fn a_short_scrap_just_after_the_line_gets_no_line_of_its_own() {
        let line = Some(10_000_000);
        let soon = 10_000_000 + 4_500_000;
        for ms in [100, 180, 330, SCRAP_NO_LINE_MS - 1] {
            assert!(scrap_after_line(true, Some(ms), line, soon), "{ms} ms");
        }
        // Long enough to be an ask: its line, as before.
        assert!(!scrap_after_line(true, Some(SCRAP_NO_LINE_MS), line, soon));
        assert!(!scrap_after_line(true, Some(1_200), line, soon));
        // Its length unknown: as before.
        assert!(!scrap_after_line(true, None, line, soon));
        // No line heard yet this outage, or heard too long ago: as before.
        assert!(!scrap_after_line(true, Some(180), None, soon));
        let late = 10_000_000 + SCRAP_AFTER_LINE_US + 1;
        assert!(!scrap_after_line(true, Some(180), line, late));
        assert!(scrap_after_line(
            true,
            Some(180),
            line,
            10_000_000 + SCRAP_AFTER_LINE_US
        ));
        // Off: every scrap as before.
        assert!(!scrap_after_line(false, Some(180), line, soon));
    }
}
