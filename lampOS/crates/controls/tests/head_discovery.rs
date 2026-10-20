use lamp_controls::head_discovery::Discovery;

fn feed(discovery: &mut Discovery, start: u64, end: u64, mask: u8) {
    for at in (start..=end).step_by(5_000) {
        discovery.ingest(at, mask).unwrap();
    }
}

#[test]
fn driven_high_and_low_candidates_need_two_complete_pulses_and_do_not_install_wiring() {
    let mut scan = Discovery::new(
        0,
        [(true, true), (false, false), (false, true), (false, true)],
    )
    .unwrap();
    feed(&mut scan, 0, 600_000, 1);
    for begin in [610_000, 800_000] {
        feed(&mut scan, begin, begin + 100_000, 2);
        feed(&mut scan, begin + 105_000, begin + 175_000, 1);
    }
    let report = scan.report();
    assert_eq!(report.lines[0].active_low_candidate, Some(true));
    assert_eq!(report.lines[1].active_low_candidate, Some(false));
    assert_eq!(report.lines[0].completed_pulses, 2);
    assert_eq!(report.lines[0].longest_pulse_us, 105_000);
    assert!(
        report.lines[2..]
            .iter()
            .all(|line| line.active_low_candidate.is_none())
    );
    assert!(!report.hardware_mapping_installed);
}

#[test]
fn startup_held_floating_chatter_and_unreleased_contact_do_not_qualify() {
    let mut scan =
        Discovery::new(0, [(true, true), (false, true), (true, true), (true, true)]).unwrap();
    feed(&mut scan, 0, 700_000, 0);
    assert!(
        scan.report()
            .lines
            .iter()
            .all(|line| line.completed_pulses == 0)
    );
    feed(&mut scan, 705_000, 800_000, 15);
    for begin in [810_000, 900_000] {
        feed(&mut scan, begin, begin + 10_000, 0);
        feed(&mut scan, begin + 15_000, begin + 75_000, 15);
    }
    feed(&mut scan, 980_000, 1_500_000, 0);
    assert!(
        scan.report()
            .lines
            .iter()
            .all(|line| line.completed_pulses == 0)
    );
}

#[test]
fn missing_samples_cannot_extend_a_pulse_and_recovery_needs_idle_again() {
    let mut scan = Discovery::new(0, [(true, true); 4]).unwrap();
    feed(&mut scan, 0, 600_000, 15);
    feed(&mut scan, 605_000, 620_000, 0);
    scan.ingest(800_000, 0).unwrap();
    feed(&mut scan, 805_000, 900_000, 15);
    assert_eq!(scan.report().gaps, 1);
    assert!(
        scan.report()
            .lines
            .iter()
            .all(|line| line.completed_pulses == 0)
    );
    assert!(scan.ingest(900_000, 15).is_err());
    assert!(scan.ingest(905_000, 16).is_err());
}

/// #711 and #1764: with a hands-away phase, a line whose rest changes is
/// counted (lamp-cb94's line 97 flipped), a steady one has none, and each
/// candidate says how a touch showed: pulses under a resting hand, or its
/// level held.
#[test]
fn the_hands_away_phase_counts_a_rest_that_changes_and_each_candidate_says_how_it_reads() {
    use lamp_controls::touch_pins::Reads;
    // Lines 37 and 96 rest low, 97 high, 98 low.
    let mut scan = Discovery::with_rest(
        0,
        [(false, false), (false, false), (true, true), (false, false)],
        2_000_000,
    )
    .unwrap();
    // Hands away from 0.5 to 2.5 s: 98 flips high at 1 s and stays.
    feed(&mut scan, 0, 995_000, 0b0100);
    feed(&mut scan, 1_000_000, 2_495_000, 0b1100);
    // Touches: 37 pulses 40 ms at a time, twice, twice over; 96 is held
    // 800 ms, twice; 98 stays high, 97 at rest.
    feed(&mut scan, 2_500_000, 2_995_000, 0b1100);
    for begin in [3_000_000, 3_400_000] {
        for pulse in [0, 80_000] {
            feed(&mut scan, begin + pulse, begin + pulse + 35_000, 0b1101);
            feed(
                &mut scan,
                begin + pulse + 40_000,
                begin + pulse + 75_000,
                0b1100,
            );
        }
        feed(&mut scan, begin + 160_000, begin + 395_000, 0b1100);
    }
    feed(&mut scan, 3_800_000, 3_995_000, 0b1100);
    for begin in [4_000_000, 5_000_000] {
        feed(&mut scan, begin, begin + 795_000, 0b1110);
        feed(&mut scan, begin + 800_000, begin + 995_000, 0b1100);
    }
    let lines = &scan.report().lines;
    assert_eq!(
        lines
            .iter()
            .map(|line| line.rest_changes)
            .collect::<Vec<_>>(),
        [Some(0), Some(0), Some(0), Some(1)]
    );
    assert_eq!(lines[0].reads, Some(Reads::Pulses));
    assert_eq!(lines[1].reads, Some(Reads::Level));
    assert_eq!(lines[2].reads, None);
    // Without the phase, nothing is said of the rest.
    let plain = Discovery::new(0, [(false, false); 4]).unwrap();
    assert!(
        plain
            .report()
            .lines
            .iter()
            .all(|line| line.rest_changes.is_none())
    );
}

/// The reviewer on #1965: two quick touches on a pad that holds its level
/// (300 ms each, under the 500 ms hold) are neither a held level nor a
/// pulsing pad, so it says no `reads` and the factory step asks again, rather
/// than writing `pulses` for good.
#[test]
fn two_quick_touches_on_a_level_pad_say_nothing_of_how_it_reads() {
    let mut scan = Discovery::with_rest(0, [(false, false); 4], 1_000_000).unwrap();
    feed(&mut scan, 0, 1_995_000, 0);
    for begin in [2_000_000, 2_800_000] {
        feed(&mut scan, begin, begin + 295_000, 1);
        feed(&mut scan, begin + 300_000, begin + 795_000, 0);
    }
    let line = &scan.report().lines[0];
    assert_eq!(line.active_low_candidate, Some(false));
    assert_eq!(line.fast_changes, 0);
    assert_eq!(line.reads, None);
}

/// thirteen-P4, after lamp-c0a4 (#1965): a person asked to touch each pad
/// twice made one long press instead, three runs in a row, and nothing was
/// found. One held press of 500 ms or more now finds the pad, read as a
/// level; a lone short press still does not, nor does a line that changed
/// at rest.
#[test]
fn one_long_hold_finds_a_pad_and_a_lone_short_press_does_not() {
    use lamp_controls::touch_pins::Reads;
    // 37 and 97 rest low; 96 and 98 low too.
    let mut scan = Discovery::with_rest(0, [(false, false); 4], 1_000_000).unwrap();
    feed(&mut scan, 0, 1_995_000, 0);
    // 37: one press of 1.3 s, as c0a4's two touches merged into.
    feed(&mut scan, 2_000_000, 3_295_000, 0b0001);
    feed(&mut scan, 3_300_000, 3_995_000, 0);
    // 97: one press of 3.6 s.
    feed(&mut scan, 4_000_000, 7_595_000, 0b0100);
    feed(&mut scan, 7_600_000, 7_995_000, 0);
    // 96: one short press of 200 ms, nothing more.
    feed(&mut scan, 8_000_000, 8_195_000, 0b0010);
    feed(&mut scan, 8_200_000, 8_995_000, 0);
    let lines = &scan.report().lines;
    assert_eq!(lines[0].active_low_candidate, Some(false));
    assert_eq!(lines[0].reads, Some(Reads::Level));
    assert_eq!(lines[2].active_low_candidate, Some(false));
    assert_eq!(lines[2].reads, Some(Reads::Level));
    assert_eq!(lines[1].active_low_candidate, None, "a lone short press");
    assert_eq!(lines[3].active_low_candidate, None);
}
