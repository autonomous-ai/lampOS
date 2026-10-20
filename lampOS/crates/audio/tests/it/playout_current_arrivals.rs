//! Fixed recorded enqueue times only: no microphone, model, or physical timing.
//! 20261011T062138Z Gemini turn 2, canonical runtime 7dde135af.
//! The source event hash and limitations are recorded in docs/playout-start.md.
use lamp_audio::playout::StartGate;

const COMPLETE_US: u64 = 3445439;
const ARRIVALS: &[(u64, usize)] = &[
    (0, 960),
    (228, 960),
    (3_983, 240),
    (162_784, 960),
    (162_972, 960),
    (164_850, 960),
    (165_035, 960),
    (166_879, 960),
    (167_082, 960),
    (168_954, 960),
    (329_723, 960),
    (329_905, 960),
    (333_826, 960),
    (333_997, 960),
    (335_928, 960),
    (336_106, 960),
    (504_999, 960),
    (505_179, 960),
    (506_986, 960),
    (507_166, 960),
    (670_225, 960),
    (670_629, 960),
    (674_339, 960),
    (674_748, 960),
    (827_572, 960),
    (828_000, 960),
    (829_663, 960),
    (830_059, 960),
    (986_184, 960),
    (986_372, 960),
    (988_228, 960),
    (988_412, 960),
    (1_142_790, 960),
    (1_142_994, 960),
    (1_146_885, 960),
    (1_147_076, 960),
    (1_307_802, 960),
    (1_307_992, 960),
    (1_309_853, 960),
    (1_310_031, 960),
    (1_464_450, 960),
    (1_464_662, 960),
    (1_468_564, 960),
    (1_468_750, 960),
    (1_635_456, 960),
    (1_635_684, 960),
    (1_637_525, 960),
    (1_637_711, 960),
    (1_641_653, 960),
    (1_641_833, 960),
    (1_643_734, 960),
    (1_643_901, 960),
    (1_647_825, 960),
    (1_647_992, 960),
    (1_649_863, 960),
    (1_650_029, 960),
    (1_651_976, 960),
    (2_146_491, 960),
    (2_146_664, 960),
    (2_148_552, 960),
    (2_148_735, 960),
    (2_150_608, 960),
    (2_150_782, 960),
    (2_154_719, 960),
    (2_154_898, 960),
    (2_468_009, 960),
    (2_468_194, 960),
    (2_470_057, 960),
    (2_470_296, 960),
    (2_472_166, 960),
    (2_472_334, 960),
    (2_476_250, 960),
    (2_476_430, 960),
    (2_478_308, 960),
    (2_478_477, 960),
    (2_870_043, 960),
    (2_870_378, 960),
    (2_874_168, 960),
    (2_874_472, 960),
    (2_878_245, 960),
    (2_878_564, 960),
    (2_882_419, 960),
    (2_882_724, 960),
    (2_884_441, 960),
    (3_233_280, 960),
    (3_233_575, 960),
    (3_237_395, 960),
    (3_237_666, 960),
    (3_239_411, 960),
];

// A single 10 ms clock model with the real gate and existing final-block
// holdback. Real provider pacing, IPC, driver delay, and AEC are not simulated.
fn replay(horizon: u64, phase: u64) -> (u64, u64) {
    let mut gate = StartGate::new(horizon);
    let mut next = 0;
    let mut queued = 0;
    let mut first = None;
    let mut zeros = 0;
    let mut played = 0;
    let total: usize = ARRIVALS.iter().map(|(_, count)| count).sum();
    for at in (phase..6_000_000).step_by(10_000) {
        while let Some(&(arrived, frames)) = ARRIVALS.get(next).filter(|(time, _)| *time <= at) {
            gate.audio_arrived(arrived);
            queued += frames;
            next += 1;
        }
        let complete = at >= COMPLETE_US;
        if (queued > 240 || (complete && queued > 0)) && gate.open(at, queued, complete) {
            let count = queued.min(240);
            queued -= count;
            played += count;
            first.get_or_insert(at);
        } else if first.is_some() {
            zeros += 240;
        }
        if played == total {
            return (first.unwrap(), zeros);
        }
    }
    panic!("bounded replay did not finish");
}

#[test]
fn held_out_two_turn_arrivals_show_the_start_latency_and_zero_frame_tradeoff() {
    assert_eq!(ARRIVALS.len(), 89);
    assert_eq!(
        ARRIVALS.iter().map(|(_, count)| count).sum::<usize>(),
        84_720
    );
    for phase in (0..10_000).step_by(500) {
        let baseline = replay(0, phase);
        let candidate = replay(200_000, phase);
        assert!((1_920..=2_160).contains(&baseline.1));
        assert_eq!(candidate.1, 0);
        assert_eq!(candidate.0 - baseline.0, 110_000);
        // These exact modeled counts are not accepted/acoustic device results.
    }
}
