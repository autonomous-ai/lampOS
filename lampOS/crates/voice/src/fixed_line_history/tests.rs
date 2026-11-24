use super::*;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
use crate::second_provider::net::Message;
use crate::thread::Thread;
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
};
use std::time::{SystemTime, UNIX_EPOCH};

// Proofs observe the process-wide forget revision. Share the existing worker
// test lock for their full lifetime, and finish any earlier pending forget.
fn isolated_history() -> crate::provider_worker::tests::OneThread {
    let one = crate::provider_worker::tests::one_thread();
    let mut thread = crate::thread::THREAD.lock().unwrap();
    crate::remember::clear_forgotten_thread(&mut thread);
    drop(thread);
    one
}

fn owner(index: u64) -> TurnOwner {
    let now = MonoTime::from_micros(1);
    let mut controller = Controller::new(BootId::new([71; 16]).unwrap(), now);
    controller
        .set_microphone_permission(now, Permission::Allowed)
        .unwrap();
    controller
        .set_capture(
            now,
            CaptureState::RetainingUntil(MonoTime::from_micros(500_000)),
        )
        .unwrap();
    controller
        .set_admission(
            now,
            AdmissionState::OpenUntil(MonoTime::from_micros(500_000)),
        )
        .unwrap();
    let mut last = controller.admit(now, AdmittedInput::NewTurn).unwrap();
    for _ in 0..=index {
        last = controller.admit(now, AdmittedInput::NewTurn).unwrap();
    }
    last
}

fn route() -> Route {
    let route = Route::default();
    route.authority(u64::MAX);
    route.activate(1, true).unwrap();
    route.privacy((true, 3, true, 4));
    let (_, mut ready) = crate::second_provider::link::Link::initial(1, 1);
    ready.session = 2;
    route.link(&ready);
    route
}

fn request(scope: ScopeSnapshot) -> RequestScope {
    RequestScope::new(
        RequestKey {
            worker: BootId::new([72; 16]).unwrap(),
            session: 2,
            request: 1,
        },
        scope,
    )
}

fn receipt(index: u64) -> Receipt {
    let issued = lamp_ipc::monotonic_us().saturating_sub(100_000).max(1);
    Receipt {
        line: FixedLine {
            origin: FixedLineOrigin {
                worker_boot: crate::process::boot_text(BootId::new([72; 16]).unwrap()),
                provider_session: 2,
                request: 1,
                call_id: format!("fixed-{index}"),
                route_epoch: 1,
                microphone_generation: 4,
                camera_generation: 3,
                issued_us: issued,
            },
            canonical: crate::language::UNPLUG_STAY.into(),
            language: "en".into(),
            text: crate::language::UNPLUG_STAY.into(),
        },
        owner: owner(index),
        played_at_us: issued + 100_000 + index * 1000,
    }
}

fn add(
    history: &mut History,
    receipt: &Receipt,
    scope: ScopeSnapshot,
    route: &Route,
    now: Instant,
) {
    let request = request(scope);
    assert!(history.register(receipt.line.origin.clone(), &request.lease(), route));
    drop(request); // Successful playback can settle after ordinary turn retirement.
    assert!(history.accept(receipt, receipt.played_at_us, now));
}

fn message(history: &History, scope: Option<&str>, now: Instant) -> Message {
    let mut recent = history.recent(scope, now);
    let recent = recent.pop().unwrap().1;
    let mut message = Message::assistant(recent.lamp);
    message.fixed_line = recent.fixed_line;
    message
}

#[test]
fn fixed_history_rejects_unknown_mismatched_stale_and_unrendered_receipts() {
    let _one = isolated_history();
    let route = route();
    let request = request(ScopeSnapshot::room());
    let good = receipt(0);
    let now = Instant::now();
    let mut history = History::new();
    assert!(!history.accept(&good, good.played_at_us, now));
    for field in [
        "worker_boot",
        "provider_session",
        "request",
        "route_epoch",
        "microphone_generation",
        "camera_generation",
    ] {
        let mut bad = good.line.origin.clone();
        match field {
            "worker_boot" => bad.worker_boot.push('x'),
            "provider_session" => bad.provider_session += 1,
            "request" => bad.request += 1,
            "route_epoch" => bad.route_epoch += 1,
            "microphone_generation" => bad.microphone_generation += 1,
            _ => bad.camera_generation += 1,
        }
        assert!(!history.register(bad, &request.lease(), &route), "{field}");
    }
    assert!(history.register(good.line.origin.clone(), &request.lease(), &route));
    let mut changed_origin = good.line.origin.clone();
    changed_origin.issued_us += 1;
    assert!(!history.register(changed_origin, &request.lease(), &route));
    for case in 0..6 {
        let mut bad = good.clone();
        let checked_at = match case {
            0 => {
                bad.line.text.push('!');
                good.played_at_us
            }
            1 => {
                bad.line.origin.call_id.push('x');
                good.played_at_us
            }
            2 => {
                bad.played_at_us = bad.line.origin.issued_us - 1;
                bad.played_at_us
            }
            3 => good.played_at_us - 1,
            4 => good.played_at_us + DELIVERY_US + 1,
            _ => {
                bad.line.language = "unrecognized".into();
                good.played_at_us
            }
        };
        assert!(!history.accept(&bad, checked_at, now), "case {case}");
        assert!(history.recent(None, now).is_empty());
    }
    let mut nonlater = good.clone();
    nonlater.line.origin.request = nonlater.owner.turn();
    let matching = RequestScope::new(
        RequestKey {
            worker: BootId::new([72; 16]).unwrap(),
            session: 2,
            request: nonlater.line.origin.request,
        },
        ScopeSnapshot::room(),
    );
    let mut separate = History::new();
    assert!(separate.register(nonlater.line.origin.clone(), &matching.lease(), &route));
    assert!(
        !separate.accept(&nonlater, nonlater.played_at_us, now),
        "playback must follow the originating request"
    );
    assert!(history.accept(&good, good.played_at_us + DELIVERY_US, now));
    assert!(history.accept(&good, good.played_at_us + DELIVERY_US, now));
    assert_eq!(
        history.recent(None, now).len(),
        1,
        "owner retry is idempotent"
    );
    let mut changed_time = good.clone();
    changed_time.played_at_us += 1;
    assert!(!history.accept(&changed_time, changed_time.played_at_us, now));
    let mut conflict = receipt(1);
    conflict.owner = good.owner;
    assert!(history.register(conflict.line.origin.clone(), &request.lease(), &route));
    assert!(!history.accept(&conflict, conflict.played_at_us, now));
    assert_eq!(history.recent(None, now).len(), 1);
}

#[test]
fn fixed_history_exact_scope_and_narrowing_survive_completion_without_widening() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut history = History::new();
    let a = ScopeSnapshot::of("A");
    let b = ScopeSnapshot::of("B");
    add(
        &mut history,
        &receipt(0),
        ScopeSnapshot::room(),
        &route,
        now,
    );
    let personal = request(a.clone());
    let lease = personal.lease();
    let said = receipt(1);
    assert!(history.register(said.line.origin.clone(), &lease, &route));
    drop(personal);
    assert!(history.accept(&said, said.played_at_us, now));
    assert_eq!(history.recent(None, now).len(), 1);
    assert_eq!(history.recent(a.person(), now).len(), 1);
    assert!(history.recent(b.person(), now).is_empty());
    let copied = message(&history, a.person(), now).clone();
    lease.narrow_to_room();
    assert!(!copied.fixed_line.as_ref().unwrap().valid());
    assert!(history.recent(a.person(), now).is_empty());
    assert_eq!(
        history.recent(None, now).len(),
        1,
        "narrowed personal history is dropped, not moved to room"
    );
}

#[test]
fn fixed_history_route_and_privacy_reopen_never_restore_copied_proofs() {
    let _one = isolated_history();
    let now = Instant::now();
    for privacy in [false, true] {
        let route = route();
        let mut history = History::new();
        add(
            &mut history,
            &receipt(0),
            ScopeSnapshot::room(),
            &route,
            now,
        );
        let copied = message(&history, None, now).clone();
        let json = serde_json::to_value(&copied).unwrap();
        assert!(json.get("fixed_line").is_none());
        assert_eq!(json["content"], crate::language::UNPLUG_STAY);
        if privacy {
            route.privacy((true, 3, false, 5));
            route.privacy((true, 3, true, 6));
        } else {
            route.activate(1, false).unwrap();
            assert!(route.activate(1, true).is_err());
            route.activate(2, true).unwrap();
        }
        assert!(!copied.fixed_line.as_ref().unwrap().valid());
        assert!(history.recent(None, now).is_empty());
        assert!(!history.accept(&receipt(0), receipt(0).played_at_us, now));
    }
    let route = route();
    let mut history = History::new();
    add(
        &mut history,
        &receipt(0),
        ScopeSnapshot::room(),
        &route,
        now,
    );
    let copied = message(&history, None, now);
    route.privacy((false, 4, true, 4));
    route.privacy((true, 5, true, 4));
    assert!(
        !copied.fixed_line.as_ref().unwrap().valid(),
        "camera close/reopen also invalidates"
    );
}

#[test]
fn fixed_history_forget_and_reload_revoke_copies_without_persisting_transcript() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let wall = SystemTime::now();
    for reload in [false, true] {
        let mut thread = Thread::new();
        add(
            &mut thread.fixed_lines,
            &receipt(0),
            ScopeSnapshot::room(),
            &route,
            now,
        );
        let copied = message(&thread.fixed_lines, None, now).clone();
        assert!(
            thread
                .recent_for(None, now, Duration::from_secs(600), 12)
                .is_empty()
        );
        assert!(thread.recall_for(None, now, 4096).is_none());
        assert!(
            thread
                .summary_due(0, now + Duration::from_secs(599))
                .is_empty()
        );
        assert!(!thread.lamp_asked(1));
        assert!(!thread.going_on(now));
        assert!(
            thread.save(now, wall).is_none(),
            "receipt does not mark durable history changed"
        );
        if reload {
            let path = std::env::temp_dir().join(format!(
                "fixed-history-reload-{}-{}.json",
                std::process::id(),
                wall.duration_since(UNIX_EPOCH).unwrap().as_nanos()
            ));
            std::fs::write(&path, "[]").unwrap();
            assert!(thread.reload(&path, now, wall));
            std::fs::remove_file(path).unwrap();
        } else {
            thread.forget();
            let saved = thread.save(now, wall).unwrap();
            assert_eq!(saved, "[]");
        }
        assert!(thread.fixed_lines.recent(None, now).is_empty());
        assert!(!copied.fixed_line.as_ref().unwrap().valid());
        assert!(
            !thread
                .fixed_lines
                .accept(&receipt(0), receipt(0).played_at_us, now)
        );
    }
}

#[test]
fn fixed_history_bounds_revoke_evicted_origins_and_keep_latest_twelve() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut history = History::new();
    add(
        &mut history,
        &receipt(0),
        ScopeSnapshot::room(),
        &route,
        now,
    );
    let copied = message(&history, None, now);
    for index in 1..=16 {
        add(
            &mut history,
            &receipt(index),
            ScopeSnapshot::room(),
            &route,
            now + Duration::from_millis(index),
        );
    }
    assert_eq!(history.recent(None, now + Duration::from_secs(1)).len(), 12);
    assert!(
        !copied.fixed_line.as_ref().unwrap().valid(),
        "evicting origin revokes delayed copies"
    );
    assert!(!history.accept(&receipt(0), receipt(0).played_at_us, now));
}

#[test]
fn fixed_history_uses_original_playback_time_and_merges_with_actual_recent_order() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut thread = Thread::new();
    let first = receipt(0);
    let request = request(ScopeSnapshot::room());
    assert!(
        thread
            .fixed_lines
            .register(first.line.origin.clone(), &request.lease(), &route)
    );
    assert!(
        thread
            .fixed_lines
            .accept(&first, first.played_at_us + DELIVERY_US, now)
    );
    let original = now - Duration::from_micros(DELIVERY_US);
    thread.person(
        "a normal question",
        true,
        original + Duration::from_millis(50),
    );
    thread.lamp(1, "a normal answer", original + Duration::from_millis(50));
    let recent =
        thread.recent_with_fixed_lines(None, now, Duration::from_secs(600), 12, None, false);
    assert_eq!(
        recent.iter().map(|r| r.lamp.as_str()).collect::<Vec<_>>(),
        [crate::language::UNPLUG_STAY, "a normal answer"]
    );
    assert!(recent[0].fixed_line.is_some());
    assert!(recent[1].fixed_line.is_none());
    assert_eq!(
        thread
            .fixed_lines
            .recent(None, original + Duration::from_micros(HISTORY_US))
            .len(),
        1
    );
    assert!(
        thread
            .fixed_lines
            .recent(None, original + Duration::from_micros(HISTORY_US + 1))
            .is_empty()
    );
    for index in 1..=12 {
        add(
            &mut thread.fixed_lines,
            &receipt(index),
            ScopeSnapshot::room(),
            &route,
            now + Duration::from_millis(index),
        );
    }
    let recent = thread.recent_with_fixed_lines(
        None,
        now + Duration::from_secs(1),
        Duration::from_secs(600),
        12,
        None,
        false,
    );
    assert_eq!(recent.len(), 12);
    assert!(
        recent.iter().all(|r| r.fixed_line.is_some()),
        "combined limit keeps chronologically newest lines"
    );
}

#[test]
fn fixed_history_one_origin_can_finish_distinct_lines_but_evicted_owners_cannot_replay() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut history = History::new();
    let first = receipt(0);
    add(&mut history, &first, ScopeSnapshot::room(), &route, now);
    for index in 1..=12 {
        let mut later = first.clone();
        later.owner = owner(index);
        later.played_at_us += index * 1000;
        assert!(history.accept(
            &later,
            later.played_at_us,
            now + Duration::from_millis(index)
        ));
    }
    assert_eq!(history.recent(None, now + Duration::from_secs(1)).len(), 12);
    assert!(
        !history.accept(&first, first.played_at_us, now),
        "an evicted playback owner cannot become new history again"
    );
    history.clear();
    let request = request(ScopeSnapshot::room());
    assert!(history.register(first.line.origin.clone(), &request.lease(), &route));
    assert!(
        !history.accept(&first, first.played_at_us, now),
        "clear must not reset playback replay protection"
    );
}

#[test]
fn fixed_history_copied_message_proof_rechecks_time_at_final_dispatch() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut history = History::new();
    let said = receipt(0);
    add(&mut history, &said, ScopeSnapshot::room(), &route, now);
    let original = message(&history, None, now);
    let retry = original.clone();
    let proof = retry.fixed_line.as_ref().unwrap();
    assert!(
        !proof.valid_at(said.line.origin.issued_us - 1),
        "future-origin metadata is never valid"
    );
    assert!(proof.valid_at(said.line.origin.issued_us));
    assert!(proof.valid_at(said.line.origin.issued_us + HISTORY_US));
    assert!(
        !proof.valid_at(said.line.origin.issued_us + HISTORY_US + 1),
        "cloning history into a delayed retry does not renew its age"
    );
    history.clear();
    assert!(
        !proof.valid_at(said.line.origin.issued_us),
        "time validity never overrides forget"
    );
}

#[test]
fn fixed_history_origin_mint_rejects_future_dispatches_without_evicting_valid_history() {
    let _one = isolated_history();
    let route = route();
    let now = Instant::now();
    let mut history = History::new();
    let request = request(ScopeSnapshot::room());
    add(
        &mut history,
        &receipt(0),
        ScopeSnapshot::room(),
        &route,
        now,
    );
    let copied = message(&history, None, now);
    for index in 1..16 {
        assert!(history.register(receipt(index).line.origin, &request.lease(), &route));
    }
    let mut future = receipt(16).line.origin;
    future.issued_us = lamp_ipc::monotonic_us() + 1_000_000;
    assert!(!history.register(future, &request.lease(), &route));
    assert!(
        copied.fixed_line.as_ref().unwrap().valid(),
        "rejected metadata cannot evict accepted history"
    );
    assert_eq!(history.recent(None, now).len(), 1);
}

#[test]
fn fixed_history_final_model_boundary_removes_revoked_copies() {
    let _one = isolated_history();
    let route = route();
    let mut history = History::new();
    let at = Instant::now();
    add(&mut history, &receipt(0), ScopeSnapshot::room(), &route, at);
    let kept = message(&history, None, at);
    let serialized = serde_json::to_value(&kept).unwrap();
    assert!(serialized.get("fixed_line").is_none());
    let unrelated = Message::assistant("An ordinary model answer.");
    let before = vec![unrelated.clone(), kept.clone()];
    assert_eq!(
        crate::second_provider::forgotten_notes_let_go(before.clone()),
        before
    );
    route.privacy((true, 3, false, 5));
    route.privacy((true, 3, true, 6));
    assert_eq!(
        crate::second_provider::forgotten_notes_let_go(before),
        vec![unrelated.clone()]
    );
    let next_route = super::tests::route();
    let mut history = History::new();
    add(
        &mut history,
        &receipt(0),
        ScopeSnapshot::room(),
        &next_route,
        at,
    );
    let delayed = message(&history, None, at);
    history.clear();
    assert_eq!(
        crate::second_provider::forgotten_notes_let_go(vec![unrelated.clone(), delayed]),
        vec![unrelated]
    );
}

#[test]
fn fixed_history_copied_message_requires_fresh_control_authority() {
    let _one = isolated_history();
    let route = route();
    let mut history = History::new();
    let at = Instant::now();
    add(&mut history, &receipt(0), ScopeSnapshot::room(), &route, at);
    let copied = message(&history, None, at);
    let proof = copied.fixed_line.as_ref().unwrap();
    let time = lamp_ipc::monotonic_us();
    route.authority(time);
    assert!(proof.valid_at(time - 1));
    assert!(!proof.valid_at(time));
    route.authority(time + 1_000);
    assert!(proof.valid_at(time));
    route.privacy((true, 3, false, 5));
    route.privacy((true, 3, true, 6));
    assert!(
        !proof.valid_at(time),
        "renewal cannot undo a privacy generation change"
    );
}

#[test]
fn fixed_history_said_once_does_not_hide_a_displaced_ordinary_exchange() {
    let _one = isolated_history();
    let at = Instant::now();
    let mut thread = Thread::new();
    for n in 0..12 {
        thread.lamp_for(
            n + 1,
            &format!("Ordinary answer {n}."),
            at - Duration::from_millis(20 - n),
            None,
        );
    }
    add(
        &mut thread.fixed_lines,
        &receipt(0),
        ScopeSnapshot::room(),
        &route(),
        at,
    );
    let (recent, selected) =
        thread.recent_with_fixed_selection(None, at, Duration::from_secs(600), 12, None, false);
    assert_eq!(recent.len(), 12);
    assert_eq!(selected.len(), 11);
    assert!(!recent.iter().any(|line| line.lamp == "Ordinary answer 0."));
    let recall = thread
        .recall_for_leaving_out(None, at, 4096, None, &selected)
        .unwrap();
    assert!(
        recall.contains("Ordinary answer 0."),
        "displaced ordinary exchange must remain in recall: {recall}"
    );
    assert!(!recall.contains(crate::language::UNPLUG_STAY));
}

#[test]
fn fixed_history_explicit_close_revokes_copies_without_revoking_newer_origins() {
    let _one = isolated_history();
    let route = route();
    let request = request(ScopeSnapshot::room());
    let mut thread = Thread::new();
    let mut old = receipt(0);
    old.line.origin.issued_us = lamp_ipc::monotonic_us().checked_sub(200_000).unwrap();
    old.played_at_us = lamp_ipc::monotonic_us();
    assert!(
        thread
            .fixed_lines
            .register(old.line.origin.clone(), &request.lease(), &route)
    );
    assert!(
        thread
            .fixed_lines
            .accept(&old, old.played_at_us, Instant::now())
    );
    let old_copy = message(&thread.fixed_lines, None, Instant::now());
    let cutoff = thread.fixed_lines.origins[0].at + Duration::from_micros(1);
    let newer_request = RequestScope::new(
        RequestKey {
            worker: BootId::new([72; 16]).unwrap(),
            session: 2,
            request: 2,
        },
        ScopeSnapshot::room(),
    );
    let mut newer = receipt(1);
    newer.line.origin.request = 2;
    newer.line.origin.issued_us = lamp_ipc::monotonic_us();
    newer.played_at_us = newer.line.origin.issued_us;
    assert!(
        thread
            .fixed_lines
            .register(newer.line.origin.clone(), &newer_request.lease(), &route)
    );
    assert!(thread.fixed_lines.origins[1].at > cutoff);
    assert!(
        thread
            .fixed_lines
            .accept(&newer, newer.played_at_us, Instant::now())
    );
    let new_copy = message(&thread.fixed_lines, None, Instant::now());
    thread.close_conversation(cutoff);
    assert!(!old_copy.fixed_line.as_ref().unwrap().valid());
    assert!(new_copy.fixed_line.as_ref().unwrap().valid());
    assert!(
        !thread
            .fixed_lines
            .register(old.line.origin.clone(), &request.lease(), &route),
        "a still-live old call cannot register again after its close"
    );
    // A delayed older close cannot cancel the later conversation's scope.
    thread.close_conversation(cutoff - Duration::from_micros(1));
    assert!(new_copy.fixed_line.as_ref().unwrap().valid());
    thread.close_conversation_with(2, Instant::now());
    assert!(!new_copy.fixed_line.as_ref().unwrap().valid());
    assert!(thread.fixed_lines.recent(None, Instant::now()).is_empty());
    assert!(!thread.fixed_lines.register(
        newer.line.origin.clone(),
        &newer_request.lease(),
        &route
    ));
}

#[test]
fn fixed_history_binding_cannot_mix_old_version_with_new_authority() {
    let _one = isolated_history();
    for privacy_change in [true, false] {
        let route = route();
        let mut history = History::new();
        let said = receipt(0);
        add(
            &mut history,
            &said,
            ScopeSnapshot::room(),
            &route,
            Instant::now(),
        );
        let copied = message(&history, None, Instant::now());
        let proof = copied.fixed_line.as_ref().unwrap();
        let at = said.played_at_us;
        route.authority(at);
        assert!(!proof.valid_at(at), "old control authority has expired");
        let observed = std::cell::Cell::new(false);
        assert!(
            !proof.binding_valid_at_with(at, || {
                observed.set(true);
                // Force the writer between this reader's version and deadline
                // loads. No instant permits both the old proof and fresh authority.
                if privacy_change {
                    route.privacy((true, 3, false, 5));
                } else {
                    route.activate(1, false).unwrap();
                    route.activate(2, true).unwrap();
                }
                route.authority(at + 100_000);
            }),
            "a fresh deadline cannot validate the version sampled before revocation"
        );
        assert!(observed.get(), "the controlled interleaving actually ran");
        assert!(!proof.valid_at(at));
    }
}

#[test]
fn fixed_history_binding_allows_current_version_authority_renewal() {
    let _one = isolated_history();
    let route = route();
    let mut history = History::new();
    let said = receipt(0);
    add(
        &mut history,
        &said,
        ScopeSnapshot::room(),
        &route,
        Instant::now(),
    );
    let copied = message(&history, None, Instant::now());
    let proof = copied.fixed_line.as_ref().unwrap();
    let at = said.played_at_us;
    route.authority(at);
    assert!(!proof.valid_at(at));
    assert!(
        proof.binding_valid_at_with(at, || route.authority(at + 100_000)),
        "same-version fresh authority is a valid state after renewal"
    );
    assert!(proof.valid_at(at));
}

#[test]
fn fixed_history_same_worker_reconnect_revokes_old_session_proofs() {
    let _one = isolated_history();
    use crate::second_provider::link::Link;
    let route = route();
    let (mut link, initial) = Link::initial(1, 1);
    route.link(&initial);
    let mut old = receipt(0);
    old.line.origin.provider_session = initial.session;
    let old_request = RequestScope::new(
        RequestKey {
            worker: BootId::new([72; 16]).unwrap(),
            session: initial.session,
            request: 1,
        },
        ScopeSnapshot::room(),
    );
    let mut history = History::new();
    assert!(history.register(old.line.origin.clone(), &old_request.lease(), &route));
    assert!(history.accept(&old, old.played_at_us, Instant::now()));
    let copied = message(&history, None, Instant::now());
    route.link(&initial);
    assert!(
        copied.fixed_line.as_ref().unwrap().valid(),
        "unchanged readiness preserves the proof"
    );

    assert!(link.failed(2, crate::provider_worker::Fault::Transport));
    let down = link.probed(2, 3, false).unwrap();
    route.link(&down);
    assert!(
        !copied.fixed_line.as_ref().unwrap().valid(),
        "unavailability revokes copied context immediately"
    );
    assert!(history.recent(None, Instant::now()).is_empty());
    assert!(!history.register(old.line.origin.clone(), &old_request.lease(), &route));
    let reopened = link.probed(4, 5, true).unwrap();
    assert_eq!(
        reopened.session, 2,
        "real same-worker link recovery advances session"
    );
    route.link(&reopened);
    assert!(!copied.fixed_line.as_ref().unwrap().valid());
    assert!(!history.register(old.line.origin.clone(), &old_request.lease(), &route));
    assert!(!history.accept(&old, old.played_at_us, Instant::now()));
    let new = receipt(1);
    assert_eq!(new.line.origin.provider_session, reopened.session);
    add(
        &mut history,
        &new,
        ScopeSnapshot::room(),
        &route,
        Instant::now(),
    );
    let fresh = message(&history, None, Instant::now());
    route.link(&reopened);
    assert!(fresh.fixed_line.as_ref().unwrap().valid());
    assert_eq!(history.recent(None, Instant::now()).len(), 1);
}

#[test]
fn fixed_history_origin_needs_current_ready_provider_session() {
    let _one = isolated_history();
    let route = Route::default();
    route.authority(u64::MAX);
    route.activate(1, true).unwrap();
    route.privacy((true, 3, true, 4));
    let request = request(ScopeSnapshot::room());
    let said = receipt(0);
    let context = lamp_msgs::RequestedAudioContext {
        worker_boot: said.line.origin.worker_boot.clone(),
        provider_session: said.line.origin.provider_session,
        request: said.line.origin.request,
        privacy_generation: said.line.origin.microphone_generation,
        call_id: said.line.origin.call_id.clone(),
        issued_us: said.line.origin.issued_us,
    };
    let mut history = History::new();
    assert!(
        route.origin(&context).is_none(),
        "initially no link is ready"
    );
    assert!(!history.register(said.line.origin.clone(), &request.lease(), &route));
    let (_, mut ready) = crate::second_provider::link::Link::initial(1, 1);
    route.link(&ready);
    assert!(
        route.origin(&context).is_none(),
        "another ready session cannot mint this context"
    );
    assert!(!history.register(said.line.origin.clone(), &request.lease(), &route));
    ready.session = context.provider_session;
    route.link(&ready);
    assert!(route.origin(&context).is_some());
    assert!(history.register(said.line.origin.clone(), &request.lease(), &route));
}
