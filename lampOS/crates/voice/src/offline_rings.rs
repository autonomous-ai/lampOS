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
//! With #8708, a reminder (never a timer) rings in its own words: its clip
//! is rendered while the link is up (`crate::ring_clips`), and offline it
//! plays ([`ring_for`]). Late ones joined into one ring play each clip in
//! turn. One with no clip of its own (set offline, or its render failed)
//! rings with the generic line and is marked "rung, not told": it is not
//! rung offline again, and once the link is back it is said in its words,
//! once. The mark is kept across a restart (the consumer's ledger).
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

/// What a due timer or reminder rings with no link (#8708).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ring {
    /// Its own words, and those of the late ones joined to it, in turn.
    Own(Vec<i16>),
    /// The generic line, "Your timer is done." or "You have a reminder.".
    Generic(Vec<i16>),
    /// Nothing rings yet, and why: it waits for the link.
    Wait(&'static str),
}

/// Why a reminder rung offline from the generic line is not rung again.
pub(crate) const RUNG_WAITS: &str =
    "rung already from the generic line: its words wait for the link";
/// Why a ring with no clip at all waits.
pub(crate) const NO_CLIP: &str = "no clip kept";
/// Why a ring before any session was seen waits: no session to prove it under.
pub(crate) const NO_SESSION: &str = "no link seen yet to prove it under";

/// What `offer` rings with no link: a reminder's own clip, and each joined
/// one's in turn, when every one is kept for its words in `cache`'s voice
/// (`clips`); else the generic line, if kept; else nothing. A reminder rung
/// from the generic line before ([`crate::reminders::Offer::rung_offline`])
/// waits for the link. Reads only files, never renders.
pub(crate) fn ring_for(
    offer: &crate::reminders::Offer,
    cache: Option<&crate::say::tts_cache::TtsCache>,
    clips: &crate::ring_clips::Clips,
) -> Ring {
    if offer.rung_offline {
        return Ring::Wait(RUNG_WAITS);
    }
    let Some(cache) = cache else {
        return Ring::Wait(NO_CLIP);
    };
    if !offer.timer {
        let (voice, owners) = (cache.identity(), cache.owners());
        let store = &offer.token.key.store_id;
        let own = std::iter::once((offer.token.key.id, offer.text.as_str()))
            .chain(
                offer
                    .also
                    .iter()
                    .map(|joined| (joined.token.key.id, joined.text.as_str())),
            )
            .try_fold(Vec::new(), |mut all, (id, text)| {
                let words = crate::ring_clips::words(owners, text);
                all.extend(clips.own(store, id, &words, voice)?);
                Some(all)
            });
        if let Some(own) = own {
            return Ring::Own(own);
        }
    }
    cache
        .ring_clip(offer.timer)
        .map_or(Ring::Wait(NO_CLIP), Ring::Generic)
}

/// The coordinator's offline ring, pass by pass (#8708, Twelve's P3 1 on
/// #8687): which item was last found waiting, so its wait is said once and
/// nothing is read from the disk for it again while it waits.
#[derive(Debug, Default)]
pub(crate) struct OfflineRinger {
    waiting: Option<String>,
}

impl OfflineRinger {
    /// For the due item `key`: with no session seen yet (`session`), it
    /// waits, and no clip is read; else `read` says what it rings. Once
    /// found waiting, the item is not read again until another is due or
    /// one rang ([`Self::rang`]). The wait's reason is returned the first
    /// time only, for the journal.
    pub(crate) fn choose(
        &mut self,
        key: &str,
        session: Option<u64>,
        read: impl FnOnce() -> Ring,
    ) -> Result<(u64, Ring), Option<&'static str>> {
        if self.waiting.as_deref() == Some(key) {
            return Err(None);
        }
        let ring = match session {
            None => Ring::Wait(NO_SESSION),
            Some(session) => match read() {
                Ring::Wait(why) => Ring::Wait(why),
                ring => return Ok((session, ring)),
            },
        };
        self.waiting = Some(key.to_owned());
        match ring {
            Ring::Wait(why) => Err(Some(why)),
            _ => Err(None),
        }
    }

    /// A ring was played: the next item's wait is said again.
    pub(crate) fn rang(&mut self) {
        self.waiting = None;
    }
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

    fn offer(id: u64, text: &str, timer: bool) -> crate::reminders::Offer {
        use lamp_schedule::delivery::{Correlation, DeliveryKey, PreparedToken};
        let token = |id| PreparedToken {
            key: DeliveryKey {
                store_id: "0123456789abcdef0123456789abcdef".into(),
                id,
                delivery: 1,
            },
            attempt: 1,
            correlation: Correlation {
                consumer_session: "test-consumer".into(),
                sequence: id,
            },
        };
        crate::reminders::Offer {
            token: token(id),
            text: text.into(),
            late: false,
            late_by_ms: Some(0),
            observed_at_us: 1,
            expires_at_us: 2,
            also: Vec::new(),
            again: false,
            timer,
            rung_offline: false,
        }
    }

    /// #8708: offline, a reminder rings in its own words from the clip
    /// rendered while online, checked against its words and the voice; a
    /// timer, or a reminder with no current clip, rings the generic line;
    /// late ones joined into one ring play each clip in turn.
    #[test]
    fn a_reminder_rings_offline_in_its_own_words_from_its_clip() {
        use crate::ring_clips::{Clips, Renderer, Wanted, words};
        use crate::say::tts_cache::{Render, TtsCache};
        let dir = std::env::temp_dir().join(format!("lamp-own-words-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = |voice: &str| {
            let render: Render = Box::new(|text: &str| Ok(vec![text.len() as i16; 2_400]));
            TtsCache::new(dir.join("cache"), voice, "eleven_v4_turbo", "en", render)
                .keeping_offline_rings(true)
        };
        let rachel = cache("Rachel");
        rachel.warm(&rachel.warm_lines()).unwrap();
        let clips = Clips::new(dir.join("ring-clips"));
        let store = "0123456789abcdef0123456789abcdef";
        let wanted = Wanted {
            store_id: store.into(),
            live: [4, 5].into(),
            reminders: vec![(4, "Take your pills.".into()), (5, "Call mum.".into())],
        };
        Renderer::new(clips.clone(), cache("Rachel")).pass(&wanted, std::time::Instant::now());
        let pills = words(None, "Take your pills.");
        let own = vec![pills.len() as i16; 2_400];
        let generic = rachel.ring_clip(false).expect("the generic line is kept");
        assert_ne!(own, generic);
        // Its own words.
        assert_eq!(
            ring_for(&offer(4, "Take your pills.", false), Some(&rachel), &clips),
            Ring::Own(own.clone())
        );
        // Late ones joined: each clip in turn.
        let mut joined = offer(4, "Take your pills.", false);
        joined.also.push(crate::reminders::Joined {
            token: offer(5, "Call mum.", false).token,
            text: "Call mum.".into(),
        });
        let mum = vec![words(None, "Call mum.").len() as i16; 2_400];
        assert_eq!(
            ring_for(&joined, Some(&rachel), &clips),
            Ring::Own([own.clone(), mum].concat())
        );
        // A timer: the generic line, never a clip of its own.
        assert_eq!(
            ring_for(&offer(4, "Take your pills.", true), Some(&rachel), &clips),
            Ring::Generic(rachel.ring_clip(true).unwrap())
        );
        // Another text under its id, another voice, or no clip: generic.
        assert_eq!(
            ring_for(
                &offer(4, "Take your vitamins.", false),
                Some(&rachel),
                &clips
            ),
            Ring::Generic(generic.clone())
        );
        let sarah = cache("Sarah");
        sarah.warm(&sarah.warm_lines()).unwrap();
        assert!(matches!(
            ring_for(&offer(4, "Take your pills.", false), Some(&sarah), &clips),
            Ring::Generic(_)
        ));
        assert_eq!(
            ring_for(&offer(9, "Feed the cat.", false), Some(&rachel), &clips),
            Ring::Generic(generic)
        );
        // No speech cache: nothing to ring.
        assert_eq!(
            ring_for(&offer(4, "Take your pills.", false), None, &clips),
            Ring::Wait(NO_CLIP)
        );
        let _ = std::fs::remove_dir_all(&dir);
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
