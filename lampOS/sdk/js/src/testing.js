// A fake lamp for tests: the v1 protocol on localhost, no lamp needed.
//
//   const fake = await FakeLamp.start({ code: '12345678' });
//   await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
//   const lamp = await Lamp.connect(undefined, { keyring });
//   await lamp.gesture('nod');
//   assert.deepEqual(fake.asked('gesture'), [{ name: 'nod' }]);
//   await lamp.close(); await fake.stop();
//
// A welcome with a fresh nonce, pairing checked against its code, hello
// checked against the paired key, grants, leases and `done`. It does what a
// test tells it (`emit`, `refuseNext`, `loseLease`) and keeps every frame. Its
// certificate is made by `openssl` at start. It moves nothing, so it never
// stands in for a lamp check.
import { execFileSync } from 'node:child_process';
import { X509Certificate, createHash, createPublicKey, randomBytes, verify } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:https';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WebSocketServer } from './ws.js';
import * as schema from './schema.js';
import { HELLO, b64, pinOf, proof, unb64 } from './keys.js';
import { MOVE_FRAMES_MIN, MOVE_STEP_MS, moveWhy } from './moves.js';

// As lamp-control stamps it (server.rs `rfc3339_utc`): UTC, to the second, ending `Z`.
const now = () => new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

// `express`'s feelings and their moves, as lamp-control's feelings.rs.
// The fake's camera picture: the smallest whole JPEG (start and end markers).
export const FAKE_JPEG = Buffer.from([0xff, 0xd8, 0xff, 0xd9]);

// The fake's microphone, as the ears share it on mic1/audio: a 20 ms block of
// 320 samples, mono, 16 kHz, S16LE.
export const AUDIO_RATE = 16_000;
export const AUDIO_BLOCK = 320;
export const AUDIO_EVERY_MS = 20;
// Why lamp-control refuses or ends a microphone stream with more than one
// person in view (microphone.rs `SOMEONE_ELSE`).
export const SOMEONE_ELSE = 'someone else is here';

// Block `sequence` of a quiet tone, as signed 16-bit little-endian samples:
// 640 bytes, continuous from one block to the next.
export function toneBlock(sequence, hz = 440, level = 1000) {
  const samples = Buffer.alloc(AUDIO_BLOCK * 2);
  const start = sequence * AUDIO_BLOCK;
  for (let n = 0; n < AUDIO_BLOCK; n += 1) {
    samples.writeInt16LE(Math.trunc(level * Math.sin((2 * Math.PI * hz * (start + n)) / AUDIO_RATE)), n * 2);
  }
  return samples;
}

export const FEELING_MOVES = { glad: 'glad-lift', lively: 'perk', low: 'low-droop' };

// `look`'s targets lamp-control doesn't route, in parts.rs's words: `name`
// (the lamp has no sound direction). A lamp-control before the face look
// (stable 11091 and older) refuses `face` too: `faceLook: false`.
export const LOOK_WAITS = {
  name: "not in this release: look at who said the lamp's name (the lamp has no sound direction)",
};
export const LOOK_FACE_OLD = 'not in this release: look at a face or a name';
// What the body says when it has no face to turn to.
export const NO_FACE_IN_VIEW = 'no face in view';

// What lamp-control refuses today, in its own words (crates/control/src/
// parts.rs, `waits_for`): pass it as `refuses` for a fake that answers as a
// lamp does now. `lamp fake` does.
// lamp-control's words for `joints` naming roll, shoulder or elbow (parts.rs).
export const JOINTS_WAIT = "not routed yet: waits for the body's goal stream for roll, shoulder and elbow; yaw and pitch go now";
// And for a `joints` leaving out yaw or pitch while the body hasn't said where it is.
export const JOINTS_UNKNOWN = 'name both yaw and pitch while the body has not said where it is in the last 15 s (get body)';
export const LAMP_CONTROL_REFUSES = Object.freeze({
  play: 'not routed yet: waits for client packages',
});
// `look` and `joints` are not in it: the product lamp-body opens its lease socket (stable 10824 and later), so a
// lamp takes a look, and `joints` at yaw and pitch, refusing roll, shoulder and elbow with JOINTS_WAIT (above).
// A light's pattern, as lamp-control checks it before the ring (lamp_msgs'
// LightPattern::check, with the breath's low at 1): the least level each is
// seen at, and the words it refuses with.
export const PATTERN_LEAST_LEVEL = Object.freeze({
  breathe: [2, "a breath's low is from 1 to under the light's level"],
  candle: [3, 'a candle flickers at level 3 and up'],
  pulse: [2, 'a pulse is seen at level 2 and up'],
});
// The ring plays one pulse a minute for one asker, and shows a second steady,
// without its pulse (lamp_msgs' PULSE_GAP_US): the door has answered by then.
export const PULSE_GAP_MS = 60_000;

// Hello's `limits`, as lamp-control says them with its rates off (session.rs
// `limits`): no rate but its two guards, refused actions and phrase changes.
export const LIMITS = Object.freeze({
  rates: 'off',
  refused: [{ most: 20, within_ms: 1_000 }],
  phrases: [{ most: 6, within_ms: 60_000 }],
  hold_max_ms: 30_000,
  lease_max_ms: 600_000,
  say_max_bytes: 900,
  sounds: {
    rate_hz: 24_000,
    clip_max_ms: 30_000,
    clip_max_bytes: 1_444_140,
    part_max_chars: 49_152,
    parts_max: 64,
    clips_max: 64,
    store_max_bytes: 20_000_000,
  },
  notes: {
    key_max_bytes: 64,
    value_max_bytes: 16_384,
    keys_max: 256,
    bytes_max: 262_144,
    writes: [{ most: 30, within_ms: 60_000 }],
    reads: [{ most: 120, within_ms: 60_000 }],
  },
  ring: { pixels: 32 },
  quiet_hours: 'off',
});

// A clip as sound.upload and sound.list say it, and why lamp-control would refuse one: its format and length (the
// fake leaves the loudness cap to the lamp). A WAV's frames are read from its `fmt ` and `data` chunks.
function clipOf(name, data) {
  return { name, bytes: data.length, ms: Math.floor(((wavFrames(data)?.frames ?? 0) * 1000) / 24_000) };
}

function wavFrames(data) {
  if (data.length < 12 || data.toString('ascii', 0, 4) !== 'RIFF' || data.toString('ascii', 8, 12) !== 'WAVE') return undefined;
  let format;
  for (let at = 12; at + 8 <= data.length;) {
    const id = data.toString('ascii', at, at + 4);
    const size = data.readUInt32LE(at + 4);
    if (id === 'fmt ') format = { channels: data.readUInt16LE(at + 10), rate: data.readUInt32LE(at + 12), bits: data.readUInt16LE(at + 22) };
    // The frames the header says, as hound's `duration` does; `cutShort` when the file holds fewer bytes.
    if (id === 'data') return format ? { format, frames: Math.floor(size / 2), at: at + 8, cutShort: data.length - at - 8 < size } : undefined;
    at += 8 + size + (size % 2);
  }
  return undefined;
}

// Why lamp-control would refuse this clip, in sounds.rs `check_clip`'s words and order: its format, its length,
// a file shorter than its header says, then its loudness; undefined for one it keeps.
function clipRefusal(data) {
  const read = wavFrames(data);
  if (read === undefined) return 'not a WAV file';
  if (read.format.channels !== 1 || read.format.bits !== 16 || read.format.rate !== 24_000) return 'not 16-bit mono PCM at 24000 Hz';
  if (read.frames < 1 || read.frames > 24_000 * 30) return 'not 1 to 720000 frames (30 s at most)';
  // Fewer bytes than the header's frames (an odd or an even count short): hound's samples end early.
  if (read.cutShort) return 'a WAV file cut short';
  return clipLoudnessRefusal(data, read);
}

// The voice's loudness cap on a clip, as lamp-control's sounds.rs `check_clip`
// holds it at upload: no sample past CLIP_PEAK_DBFS, and no 10 ms window
// (CLIP_WINDOW frames, the last one shorter) louder than CLIP_LOUDEST_DBFS RMS.
export const CLIP_PEAK_DBFS = -12;
export const CLIP_LOUDEST_DBFS = -30;
export const CLIP_WINDOW = 240;

function clipLoudnessRefusal(data, read) {
  const full = 32_767;
  const sample = (i) => data.readInt16LE(read.at + 2 * i);
  let peak = 0;
  for (let i = 0; i < read.frames; i += 1) peak = Math.max(peak, Math.abs(sample(i)));
  if (peak > full * 10 ** (CLIP_PEAK_DBFS / 20)) return `a peak past ${CLIP_PEAK_DBFS} dBFS`;
  let loudest = 0;
  for (let at = 0; at < read.frames; at += CLIP_WINDOW) {
    const end = Math.min(at + CLIP_WINDOW, read.frames);
    let power = 0;
    for (let i = at; i < end; i += 1) power += sample(i) ** 2;
    loudest = Math.max(loudest, Math.sqrt(power / (end - at)));
  }
  if (loudest > full * 10 ** (CLIP_LOUDEST_DBFS / 20)) return `louder than ${CLIP_LOUDEST_DBFS} dBFS RMS over 10 ms`;
  return undefined;
}

// A move's bounds and the body's check of it live in moves.js, which the
// client uses too; they are exported from here as before.
export { MOVE_FARTHEST, MOVE_FASTEST, MOVE_FRAMES_MIN, MOVE_LASTS_MS, MOVE_STEP_MS, moveWhy } from './moves.js';

// lamp-control's refusal of a computer's `move` (protocol.rs), in its words, as
// `bad_request`: its own shape first, then the body's check; null for one it takes.
export function moveRefusal(stepMs, frames) {
  if (stepMs < MOVE_STEP_MS[0] || stepMs > MOVE_STEP_MS[1]) return `step_ms: from ${MOVE_STEP_MS[0]} to ${MOVE_STEP_MS[1]}`;
  if (frames.length < MOVE_FRAMES_MIN) return `frames: at least ${MOVE_FRAMES_MIN}`;
  const home = (frame) => Array.isArray(frame) && frame.length === 5 && frame.every((count) => count === 0);
  if (!home(frames[0]) || !home(frames.at(-1))) return 'frames: start and end at home, all five 0';
  const why = moveWhy(stepMs, frames);
  return why === null ? null : `frames: a move this body cannot play: ${why}`;
}

// lamp-control's `light` limits (protocol.rs, msgs' arbitration.rs): a ring
// light's fade, how long it is held, the ring's pixels, the colour levels.
export const LIGHT_FADE_MS = Object.freeze([1_000, 60_000]);
export const LIGHT_FOR_MS = Object.freeze([1, 30_000]);
export const LIGHT_LEVELS = Object.freeze([1, 5]);
export const RING_PIXELS = 32;
const hexRgb = (text) => typeof text === 'string' && /^#[0-9a-fA-F]{6}$/.test(text);

// lamp-control's refusal of a computer's `light` (protocol.rs), in its words, as
// `bad_request`, in its order; null for one it takes. Only for a light it parses
// (whole numbers where it wants them): anything else is the schema's to refuse.
export function lightRefusal(request) {
  for (const field of ['level', 'brightness', 'fade_ms', 'for_ms']) {
    const value = request[field];
    if (value !== undefined && value !== null && !Number.isInteger(value)) return null;
  }
  const given = (value) => value !== undefined && value !== null;
  const { colour, rgb, pixels } = request;
  if ([colour, rgb, pixels].filter(given).length !== 1) return 'one of colour, rgb or pixels';
  if (given(colour)) {
    if (!schema.colours.includes(colour)) return `colour: one of ${schema.colours.join(', ')}`;
    const level = request.level;
    if (!given(level) || level < LIGHT_LEVELS[0] || level > LIGHT_LEVELS[1]) return `level: from ${LIGHT_LEVELS[0]} to ${LIGHT_LEVELS[1]}, with colour`;
    if (given(request.brightness)) return 'brightness goes with rgb or pixels; colour has level';
  } else {
    if (given(request.level) || given(request.pattern)) return 'level and pattern go with colour';
    if (given(request.brightness) && (request.brightness < 0 || request.brightness > 100)) return 'brightness: from 0 to 100';
  }
  if (given(rgb) && !hexRgb(rgb)) return 'rgb: #rrggbb';
  if (given(pixels)) {
    if (!Array.isArray(pixels) || pixels.length !== RING_PIXELS) return `pixels: ${RING_PIXELS} of them, one per pixel of the ring`;
    if (!pixels.every(hexRgb)) return 'pixels: each #rrggbb';
  }
  const fade = request.fade_ms;
  if (given(fade) && (fade < LIGHT_FADE_MS[0] || fade > LIGHT_FADE_MS[1])) return `fade_ms: from ${LIGHT_FADE_MS[0]} to ${LIGHT_FADE_MS[1]}`;
  const forMs = request.for_ms;
  if (given(forMs) && (forMs < LIGHT_FOR_MS[0] || forMs > LIGHT_FOR_MS[1])) return `for_ms: from ${LIGHT_FOR_MS[0]} to ${LIGHT_FOR_MS[1]}`;
  return null;
}

// How a synthetic hand moves (`handTrack`): across the picture and back
// (`wave`), round in a circle, left to right once, or held still.
export const HAND_PATHS = Object.freeze(['wave', 'circle', 'across', 'still']);
// How lamp-control turns `humans/hands/tracked` into `hand` events (Nine's
// watch.rs): told when first seen, moved or grown by `HAND_STEP`, or opened or
// closed, at most every `HAND_EVERY_US` with the newest held back until then;
// `seen: false` once, `HAND_GONE_US` after the last message. A message under
// `HAND_SURE`, or a share outside 0 to 1, is no hand. Its check runs every
// `HAND_CHECK_US`.
export const HAND_STEP = 0.05;
export const HAND_EVERY_US = 250_000;
export const HAND_GONE_US = 1_000_000;
export const HAND_SURE = 0.5;
export const HAND_CHECK_US = 250_000;
const HAND_FIELDS = ['x', 'y', 'size', 'open', 'confidence'];
// Rust's `round` to 2 places: halves away from zero (shares are never negative).
const two = (value) => Math.floor(value * 100 + 0.5) / 100;

// The `hand` events lamp-control says for a `humans/hands/tracked` track, as
// its watch.rs does, each `[at, event]` with the topic time (µs) it is said
// at: `seen` with where the hand is, rounded to 2 places, never the tracker's
// clock; `seen: false` once it's gone. Each message is taken at its `t`, and
// the 250 ms check runs from the first.
export function handEvents(track) {
  const events = [];
  let newest = null;
  let told = null;
  let toldUs = null;
  const news = (now) => {
    if (!newest) return;
    const [hand, heard] = newest;
    if (now - heard >= HAND_GONE_US) {
      newest = null;
      if (!told) return;
      told = null;
      toldUs = now;
      events.push([now, { seen: false }]);
      return;
    }
    const fresh = !told || hand.open !== told.open || ['x', 'y', 'size'].some((k) => Math.abs(hand[k] - told[k]) >= HAND_STEP);
    if (!fresh || (toldUs !== null && now - toldUs < HAND_EVERY_US)) return;
    told = hand;
    toldUs = now;
    events.push([now, { seen: true, ...Object.fromEntries(HAND_FIELDS.map((k) => [k, k === 'open' ? hand[k] : two(hand[k])])) }]);
  };
  let check = track.length ? track[0].t + HAND_CHECK_US : 0;
  for (const hand of track) {
    for (; check < hand.t; check += HAND_CHECK_US) news(check);
    if (hand.confidence < HAND_SURE || !['x', 'y', 'size', 'confidence'].every((k) => hand[k] >= 0 && hand[k] <= 1)) continue;
    newest = [hand, hand.t];
    news(hand.t);
  }
  for (; newest; check += HAND_CHECK_US) news(check);
  return events;
}

// A small seeded generator (mulberry32), so a seed gives the same track.
function seeded(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// A synthetic hand track in `humans/hands/tracked`'s shape (Ten's): `x` and
// `y` the palm centre as shares of the picture from its left and top, `size`
// the palm box's width over the picture's, `open`, `confidence`, and `t` the
// lamp's monotonic clock in microseconds, from `t0`. A seeded jitter of a
// hundredth keeps it from being a perfect line.
export function handTrack(path = 'wave', { seconds = 3, hz = 15, open = true, seed = 0, t0 = 0 } = {}) {
  if (!HAND_PATHS.includes(path)) throw new Error(`path: one of ${HAND_PATHS.join(', ')}`);
  const random = seeded(seed);
  const jitter = (by) => (random() * 2 - 1) * by;
  const round = (v, places) => Number(v.toFixed(places));
  const clamp = (v) => Math.min(Math.max(v, 0), 1);
  const frames = Math.max(1, Math.round(seconds * hz));
  return Array.from({ length: frames }, (_, n) => {
    const share = n / Math.max(frames - 1, 1);
    const angle = 2 * Math.PI * share;
    const [x, y] = {
      wave: [0.5 + 0.3 * Math.sin(angle), 0.45],
      circle: [0.5 + 0.25 * Math.cos(angle), 0.5 + 0.25 * Math.sin(angle)],
      across: [0.15 + 0.7 * share, 0.5],
      still: [0.5, 0.5],
    }[path];
    return {
      x: round(clamp(x + jitter(0.01)), 4),
      y: round(clamp(y + jitter(0.01)), 4),
      size: round(0.12 + jitter(0.01), 4),
      open,
      confidence: round(0.9 + jitter(0.05), 3),
      t: t0 + Math.round((n * 1_000_000) / hz),
    };
  });
}

const sleepMs = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const SPKI_ED25519 = Buffer.from('302a300506032b6570032100', 'hex');
const LEASE_OPS = new Set(['look', 'move', 'play', 'joints']);

function certificate() {
  const folder = mkdtempSync(join(tmpdir(), 'fake-lamp-'));
  try {
    execFileSync('openssl', ['req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1',
      '-nodes', '-days', '1', '-subj', '/CN=fake-lamp', '-keyout', join(folder, 'k.pem'), '-out', join(folder, 'c.pem')],
    { stdio: 'ignore' });
    return { cert: readFileSync(join(folder, 'c.pem')), key: readFileSync(join(folder, 'k.pem')) };
  } finally { rmSync(folder, { recursive: true, force: true }); }
}

// A made-up air reading and computer, as lamp-control's `get` answers them
// (watch.rs `air_reading` and `device_reading`): the fake lamp's own.
export const AIR = Object.freeze({ pm1_0: 2.1, pm2_5: 5.3, pm4_0: 6.8, pm10: 7.9, co2_ppm: 620, humidity: 45, temperature: 27.4, feel: 'fresh', known: true });
export const DEVICE = Object.freeze({ cpu_load: 0.25, memory_used: 0.4, disk_free: 0.6, uptime_s: 3600, temperature_c: Object.freeze({ cpu_thermal: 55 }), known: true });

export class FakeLamp {
  static async start(options = {}) {
    const fake = new FakeLamp(options);
    await fake.listen();
    return fake;
  }

  // `autoDone: false` leaves a play's, say's, gesture's or move's `done` to
  // the test. `motionDone: false` answers a gesture or a move as a
  // lamp-control from before #4209: no `motion_done` in hello's limits, and
  // no `done` follows them.
  // `refuses`: ops refused `refused` with this why, as a lamp that doesn't
  // route them yet does (LAMP_CONTROL_REFUSES); none by default.
  constructor({ code = '12345678', grants = ['body', 'light', 'speak'], lampId = 'lamp-fake', name = 'Fake lamp', autoDone = true, refuses = {}, followsHand = true, motionDone = true, faceLook = true, face = null } = {}) {
    // `followsHand: false` answers a look at `hand` as a lamp-control from
    // before #4183 does: taken, with no `follow`, and nothing follows it.
    // 'all': every grant the protocol names, as `lamp fake` pairs with. The default
    // stays narrow, as a real pairing the owner chose is.
    // A bare string would be split into letters: grants: 'hear' is a slip.
    if (typeof grants === 'string' && grants !== 'all') throw new TypeError(`grants is 'all' or an array of grant names, not '${grants}'`);
    Object.assign(this, { code, grants: grants === 'all' ? [...schema.grants] : [...grants], host: '127.0.0.1', port: 0, pin: '', fp: '', autoDone, refuses: { ...refuses }, followsHand, motionDone, faceLook, face });
    this.asker = new Map();
    this.lamp = { id: lampId, name, v: schema.VERSION };
    this.clients = new Map();
    this.requests = [];
    this.received = [];
    // WebSocket pings from clients (their keepalive).
    this.pings = 0;
    this.leases = new Map();
    // Each computer's clips, by name (`sound.upload`), and its upload in flight: as lamp-control's sounds.rs, without
    // its loudness check.
    this.clips = new Map();
    this.uploads = new Map();
    // Each computer's notes (`notes.set`) as lamp-control's notes.rs keeps them: its keys and values, held to the same
    // caps, and gone with a `revoke`.
    this.notes = new Map();
    // Pushed skills by name, and the one push in flight on the lamp.
    this.pushed = new Map();
    this.pushing = null;
    this.subscribed = new Map();
    this.state = { presence: { count: 0, near: 0, facing: false }, light: null, mood: null, lamp: { talking: false, paused: false, private: false, asleep: false },
      // Made-up readings, as lamp-control answers them; `setAir` changes the
      // air and tells `air.changed`, `{ known: false }` for none.
      air: { ...AIR }, device: { ...DEVICE, temperature_c: { ...DEVICE.temperature_c } }, volume: { level: 50, muted: false },
      // A lamp that may stand, as lamp-control's `get` `body` answers it.
      body: {"known": true, "joints": {"yaw": {"counts": 2015.0, "deg": 0.0, "min_deg": -3.5, "max_deg": 3.5, "min_counts": 1975.0, "max_counts": 2055.0, "home_counts": 2015.0, "folded_counts": null, "stand_counts": null}, "shoulder": {"counts": 2050.0, "deg": 0.0, "min_deg": -7.0, "max_deg": 59.8, "min_counts": 1970.0, "max_counts": 2730.0, "home_counts": 2050.0, "folded_counts": 2050.0, "stand_counts": 2650.0}, "elbow": {"counts": 2047.0, "deg": 0.0, "min_deg": -7.0, "max_deg": 55.6, "min_counts": 1967.0, "max_counts": 2680.0, "home_counts": 2047.0, "folded_counts": 2047.0, "stand_counts": 2600.0}, "roll": {"counts": 2044.0, "deg": 0.0, "min_deg": -2.6, "max_deg": 2.6, "min_counts": 2014.0, "max_counts": 2074.0, "home_counts": 2044.0, "folded_counts": null, "stand_counts": null}, "pitch": {"counts": 2000.0, "deg": 0.0, "min_deg": -1.3, "max_deg": 4.0, "min_counts": 1955.0, "max_counts": 2015.0, "home_counts": 2000.0, "folded_counts": null, "stand_counts": null}}, "stands": true, "speed_deg_s": 4.4, "acceleration_deg_s2": 35.2, "load_limit": 0.6, "heat_stop_c": 62, "counts_per_turn": 4096.0} };
    // Where an unmute brings it back to (lamp-actions' `before_mute`).
    this.beforeMute = 50;
    this.refusals = new Map();
    this.nextLease = 1;
    // The newest light a computer asked for, and until when (the fake's
    // `clock`, ms): what the ring shows while it lasts (`lightState`).
    this.shown = null;
    // Each pulse the ring would play, and each it would drop as a second within
    // a minute from the same computer: { client, request }.
    this.pulsesPlayed = [];
    this.pulsesDropped = [];
    this.pulsed = new Map();
    this.clock = () => Date.now();
    // Each connection whose head follows the hand (`look` at `hand`) and its
    // lease, until its stop, another look, or the lease ends; and the last
    // hand said.
    this.following = new Map();
    this.hand = null;
    // Each computer's own turn, by the id of the turn.start that opened it,
    // until its `done`; and each computer's mute, held by the connection and
    // request id of its listen.set false, until it says true, its last
    // connection closes or `handOnHead` lifts it.
    this.turns = new Map();
    this.mutes = new Map();
    this.clientOf = new Map();
    // Each connection's microphone stream (`microphone.stream`), and the
    // made-up ears' next block's `sequence`.
    this.listening = new Map();
    this.audioSequence = 0;
  }

  async listen() {
    const { cert, key } = certificate();
    // The certificate's pin (full SHA-256), as the owner's app shows it, and
    // mDNS's 16-hex hint of it.
    this.pin = pinOf(new X509Certificate(cert).raw);
    this.fp = this.pin.slice(0, 16);
    this.https = createServer({ cert, key });
    this.wss = new WebSocketServer({ server: this.https });
    this.wss.on('connection', (ws) => this.serve(ws));
    await new Promise((resolve) => this.https.listen(0, this.host, resolve));
    this.port = this.https.address().port;
  }

  // A `Lamp` paired with this fake and connected, for a test of your own
  // program: no code, pin or keyring to set up. Its keyring is a folder of its
  // own (removed by `stop()`, your LAMPOS_HOME untouched), and `stop()` closes it.
  // Call it again for a second computer.
  //
  //   const fake = await FakeLamp.start();
  //   const lamp = await fake.connect();
  //   await lamp.say('hello');
  //   assert.deepEqual(fake.asked('say'), [{ text: 'hello' }]);
  async connect({ name } = {}) {
    name ??= `test app ${(this.connected?.length ?? 0) + 1}`; // each computer has its own name
    const { Lamp } = await import('./client.js');
    const { Keyring } = await import('./keys.js');
    const folder = mkdtempSync(join(tmpdir(), 'lampos-fake-'));
    (this.folders ??= []).push(folder);
    const keyring = new Keyring(folder);
    await Lamp.pair(this.code, this.host, { pin: this.pin, port: this.port, name, keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    (this.connected ??= []).push(lamp);
    return lamp;
  }

  async stop() {
    // A test may have dropped or closed it already (a lease held on a dead
    // socket): that must not mask the test's own result.
    try {
      for (const lamp of this.connected ?? []) await lamp.close().catch(() => {});
      this.connected = [];
      for (const ws of [...this.listening.keys()]) this.stopListening(ws);
      for (const ws of this.wss.clients) ws.terminate();
      await new Promise((resolve) => this.wss.close(resolve));
      this.https.closeAllConnections();
      await new Promise((resolve) => this.https.close(resolve));
    } finally {
      // The folders go even if closing the server threw.
      for (const folder of this.folders ?? []) rmSync(folder, { recursive: true, force: true });
      this.folders = [];
    }
  }

  // The fields of every `op` request taken, oldest first.
  asked(op) {
    return this.requests.filter((r) => r.op === op).map(({ id, op: _, ...fields }) => fields);
  }

  refuseNext(op, code, why = 'refused by the test', retryMs) {
    this.refusals.set(op, [...(this.refusals.get(op) ?? []), { code, why, retryMs }]);
  }

  // The air sensor reads `reading` (`{ pm2_5: 40, co2_ppm: 1300, feel: 'stuffy' }`, ...): `get` `air` answers it,
  // and `air.changed` is told, as lamp-control tells a change past a step. With nothing, no sensor: `{ known: false }`.
  setAir(reading) {
    this.state.air = reading ? { ...reading, known: true } : { known: false };
    if (reading) this.emit('air.changed', reading);
  }

  emit(event, fields = {}) {
    const frame = JSON.stringify({ ev: event, at: now(), ...fields });
    for (const ws of this.wss.clients) if (this.subscribed.get(ws)?.has(event)) ws.send(frame);
  }

  // The lamp restarts, as on every update: each connection closes (1012,
  // "service restart"), and pairings stay. A program connects again
  // (`Lamp.keepConnected`).
  restart() {
    for (const ws of this.wss.clients) ws.close(1012, 'the lamp restarts');
  }

  // The ring shows a computer's light while it lasts.
  show(request) {
    this.shown = { light: { on: true, colour: request.colour, level: request.level }, until: this.clock() + request.for_ms };
  }

  // The person changes the volume (by voice or in the app), or a `volume`
  // op does: `get('volume')` follows, and subscribers hear `volume.changed`
  // when it changed, as lamp-control tells it from `speaker/volume`.
  setVolume(level) {
    const now = { level: Math.max(0, Math.min(100, Math.round(level))), muted: level <= 0 };
    const before = this.state.volume;
    this.state.volume = now;
    if (before.level !== now.level || before.muted !== now.muted) this.emit('volume.changed', now);
  }

  // `get`'s light, as lamp-control answers it (Nine's light/drawn): the
  // person's own (`state.light`: on, colour, level) and `shows`, what the ring
  // draws now. That's the newest light a computer asked for while it lasts,
  // else the person's own. With neither, no `shows`. No pattern.
  // null with nothing to say, as lamp-control answers before it has heard
  // light/state or light/drawn. The person's own stands in for light/drawn here,
  // as the ring publishes both when it draws the person's light.
  lightState() {
    const own = { ...(this.state.light ?? {}) };
    if (this.shown && this.shown.until > this.clock()) own.shows = { ...this.shown.light };
    else if (['on', 'colour', 'level'].some((key) => key in own)) {
      own.shows = Object.fromEntries(['on', 'colour', 'level'].filter((key) => key in own).map((key) => [key, own[key]]));
    }
    return Object.keys(own).length ? own : null;
  }

  // Say a `humans/hands/tracked` track (`handTrack`) as lamp-control would
  // (`handEvents`): `hand` events by change, at most four a second, and
  // `seen: false` once the hand is gone. `realtime` paces them by the track's
  // clock; without it they go as fast as they can. A lost hand doesn't end a
  // following: the head waits for it, as the lamp's does.
  async emitHandTrack(track, { realtime = false } = {}) {
    const started = Date.now();
    const first = track.length ? track[0].t : 0;
    for (const [at, event] of handEvents(track)) {
      if (realtime) await sleepMs(Math.max(0, started + (at - first) / 1000 - Date.now()));
      this.hand = event.seen ? event : null;
      this.emit('hand', event);
    }
  }

  // End the following of `ws` (or of every connection). lamp-control says
  // nothing for it: the head just stops following.
  stopFollowing(ws) {
    for (const held of [...this.following.keys()]) if (ws === undefined || held === ws) this.following.delete(held);
  }

  // The made-up ears skip `blocks` blocks, as sound lost on the way: the next
  // block's `sequence` jumps, and each stream says it in `lost`.
  loseAudio(blocks) { this.audioSequence += blocks; }

  // The privacy switch: on, every microphone stream ends at once with
  // `microphone.ended`, as lamp-control's do, and a new one is refused.
  setPrivate(isPrivate = true) {
    this.state.lamp.private = isPrivate;
    if (isPrivate) this.endListening('the lamp is private');
  }

  // How many people are in view (`presence`'s `count`): above one, every
  // microphone stream ends at once with `microphone.ended` ("someone else is
  // here"), and a new one is refused, as lamp-control does, since it can't
  // tell the owner from anyone else.
  setInView(count) {
    this.state.presence.count = count;
    if (count > 1) this.endListening(SOMEONE_ELSE);
  }

  endListening(why) {
    for (const ws of [...this.listening.keys()]) {
      this.stopListening(ws);
      ws.send(JSON.stringify({ ev: 'microphone.ended', why }));
    }
  }

  stopListening(ws) {
    clearInterval(this.listening.get(ws));
    this.listening.delete(ws);
  }

  // One block every 20 ms of a quiet 440 Hz tone, as binary frames: the
  // header's length, the header, then the samples.
  listenFor(ws) {
    let last = null;
    const block = () => {
      const sequence = this.audioSequence;
      this.audioSequence += 1;
      const lost = last === null ? 0 : Math.max(sequence - last - 1, 0);
      last = sequence;
      const samples = toneBlock(sequence);
      const header = Buffer.from(JSON.stringify({
        ev: 'microphone.audio', sequence, lost, sample_rate: AUDIO_RATE, channels: 1, format: 's16le', bytes: samples.length,
      }));
      const length = Buffer.alloc(4);
      length.writeUInt32BE(header.length);
      ws.send(Buffer.concat([length, header, samples]));
    };
    block();
    const timer = setInterval(block, AUDIO_EVERY_MS);
    timer.unref();
    this.listening.set(ws, timer);
  }

  unfollow(lease) {
    for (const [held, following] of [...this.following]) if (following.lease === lease) this.following.delete(held);
  }

  // Tell a lease's holder it is `held`, `suspended` or `lost` (and, lost, end it here).
  leaseEvent(lease, state, why) {
    const held = this.leases.get(lease);
    if (!held) return;
    if (state === 'lost') this.leases.delete(lease);
    // A suspended lease still stands, but the body is not held: a look or
    // `joints` under it is taken by the door and dropped by the body.
    else held.state = state;
    held.ws.send(JSON.stringify({ ev: 'lease', lease, state, ...(why ? { why } : {}) }));
    if (state === 'lost') this.unfollow(lease);
  }

  loseLease(lease, why = 'hand') { this.leaseEvent(lease, 'lost', why); }

  // A look the door takes ({ to: 'body' }), and the body's answer as its
  // `done`: there, or, while the computer's body lease is suspended (a
  // conversation), dropped as the body says it (body_lease.rs `done_of`).
  lookDone(ws, id, yaw, pitch, drop) {
    this.ok(ws, id, { to: 'body' });
    if (!this.autoDone) return undefined;
    const suspended = [...this.leases.values()].some((h) => h.ws === ws && h.parts.includes('body') && h.state === 'suspended');
    ws.send(JSON.stringify(suspended
      ? { ev: 'done', re: id, how: 'dropped', why: 'the body is not held' }
      : drop
        ? { ev: 'done', re: id, how: 'dropped', why: drop }
        : { ev: 'done', re: id, how: 'finished', yaw, pitch, clamped: false }));
    return undefined;
  }

  // End the action whose request id was `re`, to the client that asked.
  done(re, how = 'finished', why) {
    // A turn's outcome ends it, as the voice's does.
    for (const [client, started] of [...this.turns]) if (started === re) this.turns.delete(client);
    this.asker.get(re)?.send(JSON.stringify({ ev: 'done', re, how, ...(why ? { why } : {}) }));
  }

  // A hand on the head lifts every computer's mute, as the voice does: each is
  // told with a `done` `cut_short` on the listen.set that held it.
  handOnHead() {
    const mutes = [...this.mutes.values()];
    this.mutes.clear();
    for (const { ws, re } of mutes) ws.send(JSON.stringify({ ev: 'done', re, how: 'cut_short', why: 'a hand on the head' }));
  }

  serve(ws) {
    const nonce = randomBytes(32);
    let client = null;
    this.subscribed.set(ws, new Set());
    ws.on('ping', () => { this.pings += 1; });
    ws.send(JSON.stringify({ ev: 'welcome', at: now(), nonce: b64(nonce), lamp: this.lamp }));
    ws.on('close', () => {
      this.subscribed.delete(ws);
      this.following.delete(ws);
      // A computer gone lifts its mute: its last connection closed.
      const gone = this.clientOf.get(ws);
      this.clientOf.delete(ws);
      if (gone && ![...this.clientOf.values()].includes(gone)) this.mutes.delete(gone);
      this.stopListening(ws);
      for (const [lease, held] of this.leases) if (held.ws === ws) this.leases.delete(lease);
    });
    ws.on('message', (data) => {
      let request;
      try { request = JSON.parse(data.toString()); } catch { return this.refuse(ws, null, 'bad_request', 'not JSON'); }
      this.received.push(request);
      const id = Number.isInteger(request.id) && request.id >= 0 ? request.id : null;
      if (id === null || !schema.ops.includes(request.op)) return this.refuse(ws, id, 'bad_request', 'not a v1 request');
      // As lamp-control's protocol.rs, before grants or leases: a move it won't
      // take, or the body would drop, is refused in its words (#4217).
      if (request.op === 'move') {
        // Only a move lamp-control parses (`step_ms: u32`, `Vec<[i32; 5]>`)
        // reaches its check. JSON.parse gives 1.0 as 1, so this fake takes a
        // count written `1.0`, which serde refuses.
        const whole = (frame) => Array.isArray(frame) && frame.length === 5 && frame.every(Number.isInteger);
        if (!Number.isInteger(request.step_ms)) return this.refuse(ws, id, 'bad_request', 'step_ms: a whole number of ms');
        if (!Array.isArray(request.frames) || !request.frames.every(whole)) {
          return this.refuse(ws, id, 'bad_request', 'frames: each frame is five whole counts');
        }
        const refusal = moveRefusal(request.step_ms, request.frames);
        if (refusal) return this.refuse(ws, id, 'bad_request', refusal);
      }
      // As lamp-control's protocol.rs for a light: its own limits, in its words.
      if (request.op === 'light') {
        const refusal = lightRefusal(request);
        if (refusal) return this.refuse(ws, id, 'bad_request', refusal);
      }
      if (request.op === 'pair') return this.pair(ws, request, nonce);
      if (request.op === 'hello') {
        client = this.hello(ws, request, nonce);
        if (client) this.clientOf.set(ws, client);
        return undefined;
      }
      if (!client) return this.refuse(ws, id, 'no_hello', 'hello first');
      return this.op(ws, client, request);
    });
  }

  refuse(ws, re, code, why, retryMs) {
    ws.send(JSON.stringify({ re, ok: false, error: { code, why, ...(retryMs !== undefined ? { retry_ms: retryMs } : {}) } }));
  }

  // A part of a push, as lamp-control and the helper take it: in order from
  // 0, one push at a time, the whole checked against its SHA-256 and its
  // manifest, then kept as this computer's.
  push(ws, client, id, request) {
    let going = this.pushing;
    if (request.part === 0) {
      if (going && going.client !== client) return this.refuse(ws, id, 'busy', "another computer's push is in flight");
      going = this.pushing = { client, part: -1, chunks: [] };
    } else if (!going || going.client !== client || request.part !== going.part + 1) {
      return this.refuse(ws, id, 'bad_request', 'parts out of order');
    }
    going.part = request.part;
    going.chunks.push(Buffer.from(request.data, 'base64'));
    if (request.part < request.parts - 1) return this.ok(ws, id, { got: request.part + 1 });
    this.pushing = null;
    const data = Buffer.concat(going.chunks);
    const sha = createHash('sha256').update(data).digest('hex');
    if (data.length !== request.size || sha !== request.sha256.toLowerCase()) {
      return this.refuse(ws, id, 'bad_request', "the package's SHA-256 doesn't match");
    }
    const size = parseInt(data.subarray(124, 135).toString('ascii'), 8);
    const first = data.subarray(0, 100).toString('utf8').replace(/\0.*$/s, '');
    const manifest = JSON.parse(data.subarray(512, 512 + size).toString('utf8'));
    if (first !== 'lamp-skill.json' || manifest.name !== request.name) return this.refuse(ws, id, 'refused', 'not a package of that skill');
    const kept = this.pushed.get(request.name);
    if (kept && kept.client !== client) return this.refuse(ws, id, 'refused', "another computer's skill has that name");
    const skill = { name: manifest.name, version: manifest.version, sha256: sha, on: true, held: null };
    // Listed as the helper lists it (lamp-skills `list_pushed_by`): no `client`, which only this fake keeps; `author` is
    // a signed skill's, never a pushed test folder's.
    const at = Math.floor(Date.now() / 1000) * 1000;
    this.pushed.set(request.name, {
      ...skill, pushed_at: new Date(at).toISOString().replace(/\.\d{3}Z$/, 'Z'), pushed_at_unix_ms: at, author: null, client,
    });
    return this.ok(ws, id, { skill: { ...skill, asks: [], shown: [] } });
  }

  ok(ws, re, fields = {}) { ws.send(JSON.stringify({ re, ok: true, ...fields })); }

  pair(ws, request, nonce) {
    const key = unb64(request.key);
    if (!proof(this.code, Buffer.from(this.pin, 'hex'), key, nonce).equals(unb64(request.proof))) return this.refuse(ws, request.id, 'unpaired', 'the code does not match');
    const client = `c_${(this.clients.size + 1).toString(16).padStart(4, '0')}`;
    this.clients.set(client, { key, name: request.name, grants: [...this.grants] });
    return this.ok(ws, request.id, { client, grants: this.grants });
  }

  hello(ws, request, nonce) {
    const paired = this.clients.get(request.client);
    let good = false;
    if (paired) {
      const key = createPublicKey({ key: Buffer.concat([SPKI_ED25519, paired.key]), format: 'der', type: 'spki' });
      good = verify(null, Buffer.concat([HELLO, nonce]), key, unb64(request.sig));
    }
    if (!good) {
      this.refuse(ws, request.id, 'unpaired', 'not paired, or a bad signature');
      ws.close();
      return null;
    }
    const limits = this.motionDone ? { ...LIMITS, motion_done: true } : LIMITS;
    this.ok(ws, request.id, { lamp: this.lamp, grants: paired.grants, limits });
    return request.client;
  }

  // The grant each event and state needs, as lamp-control's own (#3736).
  static EVENT_GRANT = {
    'person.arrived': 'look', 'person.left': 'look', 'person.stood_up': 'look', 'person.facing': 'look',
    hand: 'look',
    'phrase.heard': 'hear', 'name.heard': 'hear', 'sound.heard': 'sound',
    'air.changed': 'air',
    'phrase.heard': 'hear', 'name.heard': 'hear', 'sound.heard': 'sound', 'volume.changed': 'speak',
  };
  static STATE_GRANT = { presence: 'look', air: 'air', device: 'device', volume: 'speak', body: 'body' };

  needs(op, request) {
    if (op === 'subscribe') return [...new Set((request.events ?? []).map((e) => FakeLamp.EVENT_GRANT[e]).filter(Boolean))];
    if (op === 'get') return [...new Set((request.state ?? []).map((s) => FakeLamp.STATE_GRANT[s]).filter(Boolean))];
    if (op === 'look') return ['face', 'hand'].includes(request.at) ? ['body', 'look'] : ['body'];
    if (['gesture', 'move', 'play', 'joints'].includes(op)) return ['body'];
    if (op === 'lease.take') return (request.parts ?? []).map((p) => ({ body: 'body', light: 'light', voice: 'speak' })[p]);
    const speak = ['say', 'sound', 'sound.upload', 'sound.list', 'sound.remove', 'turn.end', 'volume'];
    if (op.startsWith('skill.')) return ['skills'];
    if (op.startsWith('notes.')) return ['notes'];
    if (speak.includes(op)) return ['speak'];
    // A turn speaks and hears what is said to the lamp.
    return {
      express: ['body', 'light'], light: ['light'],
      'camera.snapshot': ['camera'], 'camera.stream': ['camera'],
      'microphone.stream': ['microphone'],
      'turn.start': ['speak', 'hear'], 'listen.set': ['hear'],
    }[op] ?? [];
  }

  // The owner lets `client` go, as lamp-control's revoke: its key, its clips and its notes are gone.
  revoke(client) {
    this.clients.delete(client);
    this.clips.delete(client);
    this.notes.delete(client);
  }

  // One `notes.*` op, as lamp-control's notes.rs answers it: this computer's own keys only, the value at most 16384
  // bytes serialized, 256 keys and 262144 bytes of keys and values in all.
  notesOp(ws, client, request) {
    const { id, op } = request;
    const room = this.notes.get(client) ?? new Map();
    // As notes.rs: a key is 1 to 64 bytes of letters, digits and `. _ : -`; a prefix up to 64 of them.
    const bad = op === 'notes.list' ? !/^[A-Za-z0-9._:-]{0,64}$/.test(request.prefix ?? '') : !/^[A-Za-z0-9._:-]{1,64}$/.test(request.key);
    if (bad) return this.refuse(ws, id, 'bad_request', `${op === 'notes.list' ? 'prefix' : 'key'}: up to 64 bytes of letters, digits and . _ : -`);
    const size = (value) => Buffer.byteLength(JSON.stringify(value));
    const held = (keys) => [...keys].reduce((sum, [k, v]) => sum + Buffer.byteLength(k) + size(v), 0);
    if (op === 'notes.set') {
      const bytes = size(request.value);
      if (bytes > 16_384) return this.refuse(ws, id, 'bad_request', `value: ${bytes} bytes, over the 16384 one note may take`);
      const after = new Map(room).set(request.key, request.value);
      if (after.size > 256 || held(after) > 262_144) {
        return this.refuse(ws, id, 'refused', `this computer's notes are full: it keeps 256 keys at most and 262144 bytes in all (now ${room.size} keys, ${held(room)} bytes)`);
      }
      this.notes.set(client, after);
      return this.ok(ws, id, { key: request.key, bytes });
    }
    if (op === 'notes.get') {
      return room.has(request.key)
        ? this.ok(ws, id, { key: request.key, found: true, value: room.get(request.key) })
        : this.ok(ws, id, { key: request.key, found: false });
    }
    if (op === 'notes.delete') return this.ok(ws, id, { key: request.key, deleted: room.delete(request.key) });
    const prefix = request.prefix ?? '';
    const keys = [...room].filter(([k]) => k.startsWith(prefix)).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([key, value]) => ({ key, bytes: size(value) }));
    return this.ok(ws, id, { keys, count: keys.length, total: room.size, bytes: held(room), keys_max: 256, bytes_max: 262_144 });
  }

  clipsOf(client) {
    if (!this.clips.has(client)) this.clips.set(client, new Map());
    return this.clips.get(client);
  }

  // One `sound.upload` part, as lamp-control's sounds.rs gathers it: in order from 0, the same clip, then checked and
  // kept whole.
  upload(ws, client, request) {
    const { id } = request;
    const key = JSON.stringify([request.name, request.sha256.toLowerCase(), request.size, request.parts]);
    if (request.part === 0) this.uploads.set(client, { key, next: 0, chunks: [] });
    const flight = this.uploads.get(client);
    if (!flight || flight.key !== key || flight.next !== request.part) {
      this.uploads.delete(client);
      return this.refuse(ws, id, 'bad_request', 'part: out of order; send the upload again from 0');
    }
    flight.chunks.push(Buffer.from(request.data, 'base64'));
    flight.next += 1;
    if (flight.next < request.parts) return this.ok(ws, id, { got: flight.next });
    this.uploads.delete(client);
    const data = Buffer.concat(flight.chunks);
    if (data.length !== request.size || createHash('sha256').update(data).digest('hex') !== request.sha256.toLowerCase()) {
      return this.refuse(ws, id, 'bad_request', "sha256: not the clip's");
    }
    const why = clipRefusal(data);
    if (why) return this.refuse(ws, id, 'bad_request', `clip: ${why}`);
    const kept = [...this.clipsOf(client)].filter(([n]) => n !== request.name);
    if (kept.length >= LIMITS.sounds.clips_max) return this.refuse(ws, id, 'refused', 'this computer keeps 64 clips already: remove one first');
    if (kept.reduce((sum, [, d]) => sum + d.length, 0) + data.length > LIMITS.sounds.store_max_bytes) {
      return this.refuse(ws, id, 'refused', "this computer's clips would take over 20000000 bytes: remove one first");
    }
    this.clipsOf(client).set(request.name, data);
    return this.ok(ws, id, { sound: clipOf(request.name, data) });
  }

  op(ws, client, request) {
    const { op, id } = request;
    const missing = this.needs(op, request).filter((g) => !this.clients.get(client).grants.includes(g));
    if (missing.length) return this.refuse(ws, id, 'no_grant', `needs ${missing[0]}`);
    const queued = this.refusals.get(op);
    if (queued?.length) {
      const { code, why, retryMs } = queued.shift();
      return this.refuse(ws, id, code, why, retryMs);
    }
    if (LEASE_OPS.has(op)) {
      const held = [...this.leases].filter(([, h]) => h.ws === ws && h.parts.includes('body')).map(([l]) => l);
      if (!held.length || (op === 'joints' && !held.includes(request.lease))) return this.refuse(ws, id, 'no_lease', 'take a body lease first');
    }
    if (op in this.refuses) return this.refuse(ws, id, 'refused', this.refuses[op]);
    this.requests.push(request);
    this.asker.set(id, ws);
    switch (op) {
      case 'subscribe':
        request.events.forEach((e) => this.subscribed.get(ws).add(e));
        return this.ok(ws, id);
      case 'unsubscribe':
        request.events.forEach((e) => this.subscribed.get(ws).delete(e));
        return this.ok(ws, id);
      case 'get': {
        // As lamp-control's session.rs answers `get`: the states under `state`, each by the name asked (`lamp`, not
        // `lamp.changed`'s `lamp_state`) (harness-reviewer's lamp run on 75c9, 28 Oct).
        return this.ok(ws, id, { state: Object.fromEntries(request.state.map((k) => [k, k === 'light' ? this.lightState() : this.state[k] ?? null])) });
      }
      case 'lease.take': {
        const taken = [...this.leases.values()].some((h) => h.parts.some((p) => request.parts.includes(p)));
        if (taken) return this.refuse(ws, id, 'busy', 'another client holds it');
        const lease = `l_${this.nextLease++}`;
        this.leases.set(lease, { ws, parts: request.parts });
        return this.ok(ws, id, { lease, until: now() });
      }
      case 'lease.renew':
        if (!this.leases.has(request.lease)) return this.refuse(ws, id, 'lease_lost', 'that lease has ended');
        return this.ok(ws, id, { lease: request.lease, until: now() });
      case 'lease.release':
        this.leases.delete(request.lease);
        this.unfollow(request.lease);
        return this.ok(ws, id);
      case 'light': {
        // A computer's own colours (the ring, pixel by pixel), as lamp-control's
        // parts.rs: to the ring, each pixel at the brightness.
        if (request.pixels || request.rgb) {
          const given = request.pixels ?? Array(32).fill(request.rgb);
          if (given.length !== 32) {
            this.requests.pop();
            return this.refuse(ws, id, 'bad_request', 'pixels: 32 of them, one per pixel of the ring');
          }
          this.shownPixels = given;
          return this.ok(ws, id, { to: 'ring', pixels: given.length });
        }
        // As lamp-control's parts.rs: the pattern checked first, then to the
        // ring, which keeps its own one-pulse-a-minute (not said back).
        const [least, why] = PATTERN_LEAST_LEVEL[request.pattern] ?? [1, ''];
        if (request.level < least) {
          this.requests.pop();
          return this.refuse(ws, id, 'refused', why);
        }
        if (request.pattern === 'pulse') {
          const now = this.clock();
          if (now - (this.pulsed.get(client) ?? -PULSE_GAP_MS) < PULSE_GAP_MS) {
            // The ring shows it without its pulse (ring_light.rs's start_moving
            // starts it "as already played"): its colour and level steady, the
            // old light replaced.
            this.pulsesDropped.push({ client, request });
          } else {
            this.pulsed.set(client, now);
            this.pulsesPlayed.push({ client, request });
          }
        }
        this.show(request);
        return this.ok(ws, id, { to: 'ring' });
      }
      case 'camera.snapshot':
        // A picture with no camera behind it: the smallest whole JPEG.
        return this.ok(ws, id, { jpeg: FAKE_JPEG.toString('base64'), bytes: FAKE_JPEG.length, age_ms: 0 });
      case 'camera.stream':
        // As lamp-control's check: 1 to 30 a second.
        if (request.fps !== undefined && !(Number.isInteger(request.fps) && request.fps >= 1 && request.fps <= 30)) {
          return this.refuse(ws, id, 'bad_request', 'fps: 1 to 30');
        }
        if (!request.on) return this.ok(ws, id, { stream: 'off' });
        this.ok(ws, id, { stream: 'on', fps: request.fps ?? 2, fps_now: null });
        ws.send(JSON.stringify({ ev: 'camera.frame', jpeg: FAKE_JPEG.toString('base64'), bytes: FAKE_JPEG.length, age_ms: 0, fps_now: null }));
        return undefined;
      case 'microphone.stream':
        // As lamp-control's microphone.rs: on again replaces it, off ends it
        // with no event, and none while the lamp is private.
        this.stopListening(ws);
        if (!request.on) return this.ok(ws, id, { stream: 'off' });
        if (this.state.lamp.private) return this.refuse(ws, id, 'private', 'the lamp is private');
        if (this.state.presence.count > 1) return this.refuse(ws, id, 'refused', SOMEONE_ELSE);
        this.ok(ws, id, { stream: 'on', sample_rate: AUDIO_RATE, channels: 1, format: 's16le', block_ms: AUDIO_EVERY_MS, source: 'mic1' });
        this.listenFor(ws);
        return undefined;
      case 'stop':
        // The holder's stop ends its following, as any of its actions, and
        // its microphone stream, with no event.
        this.stopFollowing(ws);
        this.stopListening(ws);
        return this.ok(ws, id);
      case 'express': {
        // As lamp-control's feelings.rs: the feelings skill's three, its
        // move to the body (smaller below strength 0.5) and its light; with
        // motionDone its move's `done` follows (none when no move was sent).
        // The fake always shows the light.
        const name = FEELING_MOVES[request.feeling];
        if (!name) return this.refuse(ws, id, 'bad_request', 'feeling: one of glad, lively, low');
        const rounded = Math.round(Math.min(Math.max((request.strength ?? 0.5) / 0.5, 0), 1) * 4);
        // `low` goes no smaller than three quarters (feelings.rs `least_step`).
        const step = rounded === 0 ? 0 : Math.max(rounded, request.feeling === 'low' ? 3 : 1);
        const move = step === 0 ? null : step === 4 ? name : `${name}-${step * 25}`;
        this.ok(ws, id, { to: 'body', move, light: true });
        if (move !== null && this.motionDone && this.autoDone) this.done(id);
        return undefined;
      }
      case 'look': {
        // As lamp-control with --lease-socket (parts.rs `look`): `name` isn't
        // routed (nor `face`, with `faceLook: false`, a lamp-control before
        // it); `hand` is followed until a stop, another look or the lease
        // ends, so no `done` follows it; the body takes the rest, and its
        // `done` says where the head got to. A `face` is one turn toward
        // `this.face` ({ yaw, pitch }: where the face is from the head, in
        // degrees), or dropped, "no face in view", when none is set.
        if (typeof request.at === 'string' && LOOK_WAITS[request.at]) return this.refuse(ws, id, 'refused', LOOK_WAITS[request.at]);
        if (request.at === 'face' && !this.faceLook) return this.refuse(ws, id, 'refused', LOOK_FACE_OLD);
        // Another look ends a following.
        this.stopFollowing(ws);
        if (request.at === 'hand') {
          // `followsHand: false` is a lamp-control from before #4183: it takes
          // the look without saying it follows.
          if (!this.followsHand) return this.ok(ws, id);
          const [lease] = [...this.leases].find(([, h]) => h.ws === ws && h.parts.includes('body')) ?? [];
          this.following.set(ws, { lease });
          return this.ok(ws, id, { to: 'body', follow: 'hand' });
        }
        if (request.at === 'face') {
          this.stopFollowing(ws);
          if (!this.face) return this.lookDone(ws, id, 0, 0, NO_FACE_IN_VIEW);
          return this.lookDone(ws, id, this.face.yaw, this.face.pitch);
        }
        const { yaw = 0, pitch = 0 } = typeof request.at === 'object' ? request.at : {};
        return this.lookDone(ws, id, yaw, pitch);
      }
      case 'joints': {
        // As lamp-control (parts.rs `joints`): roll, shoulder and elbow wait
        // for the body's goal stream; yaw and pitch go to the body as the
        // holder's look, a joint not named staying where `get` `body` says it
        // is, so its `done` follows as a look's does, and taking it ends a
        // hand-follow as any look does.
        const { targets } = request;
        if (['roll', 'shoulder', 'elbow'].some((joint) => joint in targets)) return this.refuse(ws, id, 'refused', JOINTS_WAIT);
        const now = this.state.body.joints;
        const yaw = targets.yaw ?? now.yaw.deg;
        const pitch = targets.pitch ?? now.pitch.deg;
        // The door's words when the body hasn't said where it is.
        if (yaw === null || yaw === undefined || pitch === null || pitch === undefined) return this.refuse(ws, id, 'refused', JOINTS_UNKNOWN);
        this.stopFollowing(ws);
        return this.lookDone(ws, id, yaw, pitch);
      }
      case 'gesture':
      case 'move':
        // As lamp-control routes them: the body takes them; with motionDone
        // a `done` follows once the body says it ended (`finished`, or
        // `dropped`/`cut_short` with the body's why).
        this.ok(ws, id, { to: 'body' });
        if (this.motionDone && this.autoDone) this.done(id);
        return undefined;
      case 'skill.push':
        return this.push(ws, client, id, request);
      case 'skill.list':
        return this.ok(ws, id, { skills: [...this.pushed.values()].filter((s) => s.client === client).map(({ client: _, ...s }) => s) });
      case 'skill.remove': {
        const kept = this.pushed.get(request.name);
        if (!kept || kept.client !== client) return this.refuse(ws, id, 'refused', "not one of this computer's pushed skills");
        this.pushed.delete(request.name);
        return this.ok(ws, id);
      }
      case 'notes.set':
      case 'notes.get':
      case 'notes.delete':
      case 'notes.list':
        return this.notesOp(ws, client, request);
      case 'sound.upload':
        return this.upload(ws, client, request);
      case 'sound.list':
        return this.ok(ws, id, { sounds: [...this.clipsOf(client)].sort(([a], [b]) => a.localeCompare(b)).map(([n, d]) => clipOf(n, d)) });
      case 'sound.remove':
        if (!this.clipsOf(client).delete(request.name)) {
          return this.refuse(ws, id, 'refused', `no clip named ${request.name} on this lamp for this computer`);
        }
        return this.ok(ws, id, { removed: request.name });
      case 'volume': {
        // As lamp-control through lamp-actions' app calls: a level, the
        // voice's 20-point step within its floor of 10 and ceiling of 100, or
        // a mute that an unmute takes back. The answer is the level it left,
        // and `volume.changed` follows when it changed.
        const { level } = this.state.volume;
        let next = level;
        if (request.to !== undefined) next = request.to;
        else if (request.step === 'up') next = Math.min(100, level + 20);
        else if (request.step === 'down') next = level === 0 ? 0 : Math.max(10, level - 20);
        else if (request.mute === true) { if (level > 0) this.beforeMute = level; next = 0; }
        else if (request.mute === false) next = level > 0 ? level : this.beforeMute;
        this.setVolume(next);
        return this.ok(ws, id, { ...this.state.volume });
      }
      case 'say':
      case 'sound':
        // As lamp-control: the voice takes it (a sound is its hum or one of
        // the computer's clips), and its `done` follows.
        if (op === 'sound' && request.name !== 'hum' && !this.clipsOf(client).has(request.name)) {
          this.requests.pop();
          return this.refuse(ws, id, 'refused', `no clip named ${request.name} on this lamp for this computer: sound.upload it first`);
        }
        this.ok(ws, id, { to: 'voice' });
        if (this.autoDone) this.done(id);
        return undefined;
      case 'turn.start':
        // As lamp-control: handed to the voice, kept as this computer's turn
        // until its `done`; finished at once with autoDone.
        this.turns.set(client, id);
        this.ok(ws, id, { to: 'voice' });
        if (this.autoDone) this.done(id);
        return undefined;
      case 'turn.end': {
        const started = this.turns.get(client);
        if (started === undefined) {
          this.requests.pop();
          return this.refuse(ws, id, 'busy', "not this computer's turn");
        }
        this.turns.delete(client);
        this.ok(ws, id, { turn: 'ended', started });
        this.done(started, 'dropped', 'dropped: ended by the computer');
        return undefined;
      }
      case 'listen.set':
        // As lamp-control: false holds this computer's mute (again, nothing
        // new); true lifts it, or says nothing was held.
        if (!request.on) {
          if (!this.mutes.has(client)) this.mutes.set(client, { ws, re: id });
          return this.ok(ws, id, { mute: 'held' });
        }
        if (!this.mutes.delete(client)) return this.ok(ws, id, { mute: 'nothing_held' });
        return this.ok(ws, id, { mute: 'lifted' });
      default:
        this.ok(ws, id);
        if (op === 'play' && this.autoDone) this.done(id);
        return undefined;
    }
  }
}
