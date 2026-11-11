// LAMPOS_LOG=debug: every frame to and from the lamp, one line each on stderr.
// Off by default. What would let someone pair or say hello as this computer,
// or read the lamp's pin, never reaches the log: `proof`, `sig`, `key`,
// `nonce`, `pin` and `fp` are replaced, and so is a pair request's `code`, the
// owner's one-time code (a reply's `code` is a refusal's, and is kept). A
// say's `text` and a turn's `prompt` are given only as their length and a
// tag keyed for this run (an HMAC), and a pushed skill part's or an uploaded
// clip's `data` only as its length.
import { createHmac, randomBytes } from 'node:crypto';

// Fields replaced wherever they appear in a frame.
// A note's key and value and a list's prefix are the program's own: `key` was
// secret already (the pairing key), and `value` and `prefix` join it.
export const SECRET = new Set(['proof', 'sig', 'key', 'nonce', 'pin', 'fp', 'value', 'prefix']);
// Fields given only as a summary: a said text or a turn's prompt, a pushed
// skill part's bytes, a camera picture.
export const SUMMARISED = new Set(['text', 'prompt', 'data', 'jpeg']);
// Every other field the protocol has, printed as it is. A test fails on any
// field of control.schema.json in none of these, so a new one is decided here,
// not found later in a log; and at run time a field in none of them prints as
// `<not shown>`, so one from a newer lamp is hidden until decided (fails closed). Shown as they are, so on a terminal or in the logs
// an MCP client keeps of stderr: a registered phrase and a `phrase.heard`
// phrase, events' fields, and a refusal's `why`.
export const SHOWN = new Set([
  // hello's lamp: the owner's language (en, vi, ja, zh, ko).
  'language',
  // A dev lamp's `--rates off`, said in hello's limits.
  'rates',
  // `say`'s choice of the model's words over the exact ones.
  'rephrase',
  // `get` `air` and `device`, and `air.changed`: readings, never who.
  'air', 'co2_ppm', 'cpu_load', 'device', 'disk_free', 'feel', 'humidity', 'known', 'memory_used', 'pm10', 'pm1_0',
  'pm2_5', 'pm4_0', 'temperature', 'temperature_c', 'uptime_s',
  // A camera picture's size, age and a stream's rate (never the picture).
  'age_ms', 'bytes', 'fast', 'fps', 'fps_now', 'stream',
  // A computer's own notes (`notes.*`) and hello's limits for them: whether a key was found or deleted, how many it
  // keeps and the bounds; a key, a value and a prefix are secret above, never shown.
  'notes', 'found', 'deleted', 'keys', 'count', 'total', 'keys_max', 'bytes_max', 'key_max_bytes', 'value_max_bytes', 'writes', 'reads',
  // `lamp.said`: who had the line said and whether it was cut (its `text` is summarised).
  'by', 'cut',
  // A computer's own clips (`sound.upload`, `sound.list`, `sound.remove`) and hello's limits for them: names, sizes
  // and lengths, never the bytes; a pushed skill's whole hash and each part's number (`skill.push`).
  'sound', 'sounds', 'removed', 'ms', 'sha256', 'part', 'rate_hz', 'clip_max_ms', 'clip_max_bytes',
  'part_max_chars', 'parts_max', 'clips_max', 'store_max_bytes',
  // `light`'s own colours (the ring, pixel by pixel) and hello's ring.
  'brightness', 'fade_ms', 'pixels', 'rgb', 'ring',
  // The `volume` op, its `get` state and `volume.changed`.
  'muted', 'step', 'volume',
  // `get` `body`: the body's bounds and where each joint is, its numbers, never who.
  'acceleration_deg_s2', 'body', 'counts', 'counts_per_turn', 'deg', 'folded_counts', 'heat_stop_c', 'home_counts',
  'load_limit', 'max_counts', 'max_deg', 'min_counts', 'min_deg', 'speed_deg_s', 'stand_counts', 'stands',
  // Each joint of a `body.state` beyond `get` `body`'s.
  'goal_deg', 'load', 'moving', 'temp_c',
  // A microphone stream's shape and a block's count (never the sound, which
  // comes binary and is never logged).
  'block_ms', 'channels', 'format', 'lost', 'sample_rate', 'sequence', 'source',
  'asleep', 'at', 'clamped', 'class', 'client', 'code', 'colour', 'confidence', 'count', 'elbow', 'error', 'ev', 'events',
  'express', 'facing', 'from', 'feeling', 'for_ms', 'frames', 'got', 'grants', 'hold_max_ms', 'how', 'id', 'lease_max_ms', 'say_max_bytes',
  'joints', 'kind', 'lamp', 'lamp_state', 'lease', 'level', 'light', 'limits', 'manage', 'mood',
  'most', 'motion', 'motion_done', 'mute', 'name', 'near', 'ok', 'on', 'op', 'open', 'parts', 'pattern', 'paused', 'phrase', 'phrases',
  'pitch', 'presence', 'press', 'private', 'push', 'quiet_hours', 're', 'refused', 'retry_ms',
  'roll', 'say', 'say_max_chars', 'score', 'seen', 'shoulder', 'shows', 'side', 'size', 'skill', 'skills', 'started', 'state',
  'step_ms', 'strength', 'take_max_ms', 'talking', 'targets', 'to', 'turn', 'until', 'v', 'where', 'why',
  'within_ms', 'x', 'y', 'yaw',
]);
// Fields a lamp sends that the schema leaves open (`limits`, an `express`
// answer, `mood`, `light`, a pushed skill's parts, Nine's `shows` and `hand`,
// and a `look` at `hand`'s `follow`).
export const SHOWN_BEYOND_SCHEMA = new Set([
  'to', 'move', 'on', 'energy', 'warmth', 'awake', 'shows', 'seen', 'x', 'y', 'size', 'open',
  'confidence', 'follow',
]);
// A said text's tag is keyed with this process's own random key: the same text
// gives the same tag within a run, and nobody holding the log can test a guess.
const TAG_KEY = randomBytes(16);

// Read at each frame, so setting LAMPOS_LOG in a running program takes effect.
export const on = () => (process.env.LAMPOS_LOG ?? '').trim().toLowerCase() === 'debug';

// A copy of `value` safe to print.
export function redact(value, key) {
  if (SECRET.has(key) && value !== undefined && value !== null) return '<redacted>';
  if ((key === 'text' || key === 'prompt') && typeof value === 'string') {
    return `<${value.length} chars, tag ${createHmac('sha256', TAG_KEY).update(value).digest('hex').slice(0, 8)}>`;
  }
  if (key === 'data' && typeof value === 'string') return `<${value.length} chars>`;
  if (Array.isArray(value)) return value.map((item) => redact(item));
  if (value && typeof value === 'object') {
    const pairing = value.op === 'pair';
    const decided = (k) => SECRET.has(k) || SUMMARISED.has(k) || SHOWN.has(k) || SHOWN_BEYOND_SCHEMA.has(k);
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, pairing && k === 'code' ? '<redacted>' : decided(k) ? redact(v, k) : '<not shown>']));
  }
  return value;
}

// Print one frame, `->` sent or `<-` received, when LAMPOS_LOG=debug.
export function frame(direction, value) {
  if (on()) process.stderr.write(`lampos ${direction} ${JSON.stringify(redact(value))}\n`);
}
