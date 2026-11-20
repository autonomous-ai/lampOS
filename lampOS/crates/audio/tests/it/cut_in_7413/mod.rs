//! Cut-ins over the lamp's story that were never yielded to, replayed from
//! the hearing the lamp recorded (#7413).
//!
//! `trails.txt` holds numbers only: the `duplex_hearing` records of four
//! tries on lamp-52e6 (q-cutin-h try 8, row-8159d run 1 tries 10 and 15,
//! run 2 try 4), copied verbatim from Sixth's read-7413. What runs here is
//! the real [`Interruption`] fed each recorded block as the mask fed it.
//! What does not run is the canceller or [`lamp_audio::duplex::Hearing`]:
//! the blocks are replayed as they were heard, so only the blocks up to the
//! first yield are what the lamp would hear with a change. After a yield the
//! lamp falls silent, and what it would have heard then was not recorded.
use lamp_audio::duplex::{Confirmation, Heard, Interruption, Step};

struct Block {
    heard: Heard,
    plain: bool,
}

/// One try's blocks, its records joined in order. Each record must follow
/// the one before it without a gap in capture.
fn trail(name: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut next: Option<u64> = None;
    for line in include_str!("trails.txt").lines() {
        let mut fields = line.split(' ');
        if fields.next() != Some("H") || fields.next() != Some(name) {
            continue;
        }
        let field = |key: &str| {
            line.split(' ')
                .find_map(|field| field.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or_else(|| panic!("{name}: no {key} in {line}"))
        };
        let first: u64 = field("first_frame_sequence").parse().unwrap();
        assert!(
            next.is_none_or(|next| next == first),
            "{name}: a gap at {first}"
        );
        let (heard, explained, age) = (field("heard"), field("explained"), field("alignment_age"));
        assert_eq!(heard.len(), explained.len(), "{name}");
        assert_eq!(heard.len(), age.len(), "{name}");
        next = Some(first + heard.len() as u64);
        for ((heard, explained), age) in heard.chars().zip(explained.chars()).zip(age.chars()) {
            let heard = match heard {
                '0' => Heard::Still,
                '1' => Heard::Clear,
                '2' => Heard::Under,
                '3' => Heard::Own,
                '4' => Heard::Over,
                // Unknown with a fresh alignment that explained the block is
                // Over before the lamp's sound was matched in this reply
                // (`turn_taking.rs`, `MATCHED_BLOCKS`), which the mask hands
                // to the yield as Under. Stale (40 blocks or more) or with no
                // echo at all, it is Unknown from `Hearing::heard`.
                '5' if explained != '-' && ('0'..='3').contains(&age) => Heard::Under,
                '5' => Heard::Unknown,
                other => panic!("{name}: heard {other}"),
            };
            // Plain is at most 20% explained; the trail keeps tenths,
            // floored, so only 0 and 1 are surely plain.
            let plain = heard == Heard::Over && ('0'..='1').contains(&explained);
            blocks.push(Block { heard, plain });
        }
    }
    assert!(!blocks.is_empty(), "{name}: no records");
    blocks
}

fn replay(blocks: &[Block], switch: bool) -> Vec<(usize, Step)> {
    let mut interruption = Interruption::new(Confirmation::EVALUATED);
    if switch {
        interruption.count_unknown_onsets();
    }
    blocks
        .iter()
        .enumerate()
        .filter_map(
            |(index, block)| match interruption.observe(block.heard, block.plain, true) {
                Step::Carry => None,
                step => Some((index, step)),
            },
        )
        .collect()
}

/// Each try's trail starts this many blocks before its clip played: the
/// blocks before it are the lamp's story alone, nobody talking.
const TRIES: [(&str, usize); 3] = [
    // The clip's words came late, from block 516 (+3.15 s), after the
    // kit's +3 s window: "Wait" heard 6 Own and 4 Over, "hold on." 25
    // blocks Unknown, 13 with no echo and 12 at an alignment 50-60 old.
    ("q-cutin-h_t8", 201),
    // In a pause of the story, from block 377 (+1.30 s): 3 Own, Unknown and Clear
    // with no echo, then 24 Unknown (13 with no echo, 11 at an alignment
    // 70-80 old), then 5 Over.
    ("8159d_r1_t10", 247),
    // In a pause of the story, from block 313 (+1.32 s): 12 Unknown with
    // no echo (the render silent where its echo would be), then 1 Over.
    ("8159d_r2_t4", 181),
];

/// `LAMP_CUT_IN_STALE_ONSET=on`: a person heard Unknown past the reply's
/// start counts toward the run that makes the lamp yield, so each of these
/// misses yields inside the person's words. Without the switch none of them
/// yields, as on the lamp.
#[test]
fn cut_ins_heard_unknown_past_the_replys_start_yield_only_with_the_switch() {
    for (name, yields_at) in [
        ("q-cutin-h_t8", 554),
        ("8159d_r1_t10", 423),
        ("8159d_r2_t4", 319),
    ] {
        let blocks = trail(name);
        assert_eq!(replay(&blocks, false), [], "{name}: off");
        assert_eq!(
            replay(&blocks, true).first(),
            Some(&(yields_at, Step::Yield)),
            "{name}: on"
        );
    }
}

/// The lamp's own story before each clip yields to nothing with the switch.
/// And try 15, whose yield at -0.39 s was on the lamp's own sound after a
/// pause (7 plain Over) and whose real cut-in came after the resume, heard
/// Over before the lamp's sound was matched again: the switch changes
/// nothing there. That yield is not this switch's to fix, and the blocks
/// before the match are left as they are (#8159).
#[test]
fn the_lamps_own_voice_yields_to_nothing_more_with_the_switch() {
    for (name, clip) in TRIES {
        let blocks = trail(name);
        assert_eq!(replay(&blocks[..clip], true), [], "{name}");
    }
    let blocks = trail("8159d_r1_t15");
    let off = replay(&blocks, false);
    assert_eq!(off.first(), Some(&(184, Step::Yield)));
    assert_eq!(replay(&blocks, true), off);
}
