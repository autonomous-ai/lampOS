// SPDX-License-Identifier: Apache-2.0
// Original score for this example, composed directly as chord voicings and
// sparse upper notes. No recording, sample, existing melody or transcription.
// Frequencies are millihertz, rounded from equal temperament with A4 = 440 Hz.

const DURATION_MS: u32 = 48_000;
const BAR_MS: u32 = 6_000;

// Eight slow changes. The one-second overlaps soften each change; common
// tones remain in the same register. The last voicing settles on D minor.
const PAD_CHORDS: [[u32; 4]; 8] = [
    [146_832, 220_000, 329_628, 349_228], // D3 A3 E4 F4
    [116_541, 174_614, 220_000, 261_626], // Bb2 F3 A3 C4
    [195_998, 246_942, 293_665, 440_000], // G3 B3 D4 A4
    [130_813, 195_998, 293_665, 329_628], // C3 G3 D4 E4
    [110_000, 164_814, 195_998, 246_942], // A2 E3 G3 B3
    [146_832, 220_000, 261_626, 329_628], // D3 A3 C4 E4
    [116_541, 174_614, 261_626, 293_665], // Bb2 F3 C4 D4
    [146_832, 220_000, 293_665, 349_228], // D3 A3 D4 F4
];

// (Start in milliseconds, frequency in millihertz). Uneven spacing leaves
// breathing room; these tones are part of this score, not a repeated loop.
const UPPER_NOTES: [(u32, u32); 16] = [
    (1_700, 440_000),  // A4
    (4_300, 523_251),  // C5
    (7_900, 349_228),  // F4
    (10_600, 440_000), // A4
    (13_400, 587_330), // D5
    (16_100, 493_883), // B4
    (19_200, 391_995), // G4
    (21_800, 329_628), // E4
    (25_500, 493_883), // B4
    (28_100, 391_995), // G4
    (31_400, 329_628), // E4
    (34_200, 523_251), // C5
    (37_300, 293_665), // D4
    (40_000, 349_228), // F4
    (43_100, 440_000), // A4
    (45_300, 293_665), // D4
];
