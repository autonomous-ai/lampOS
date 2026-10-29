// LAMPOS_LOG=debug: every frame to and from the lamp, one line each on stderr.
// Off by default. What would let someone pair or say hello as this computer,
// or read the lamp's pin, never reaches the log: `proof`, `sig`, `key`,
// `nonce`, `pin` and `fp` are replaced, and so is a pair request's `code`, the
// owner's one-time code (a reply's `code` is a refusal's, and is kept). A
// say's `text` is given only as its length and a tag keyed for this run (an
// HMAC), and a pushed skill part's `data` only as its length.
import { createHmac, randomBytes } from 'node:crypto';

// Fields replaced wherever they appear in a frame.
export const SECRET = new Set(['proof', 'sig', 'key', 'nonce', 'pin', 'fp']);
// Fields given only as a summary: a said text, a pushed skill part's bytes.
export const SUMMARISED = new Set(['text', 'data']);
// Every other field the protocol has, printed as it is. A test fails on any
// field of control.schema.json in none of these, so a new one is decided here,
// not found later in a log. Shown as they are, so on a terminal or in the logs
// an MCP client keeps of stderr: a registered phrase and a `phrase.heard`
// phrase, events' fields, and a refusal's `why`.
export const SHOWN = new Set([
  // A dev lamp's `--rates off`, said in hello's limits.
  'rates',
  'asleep', 'at', 'clamped', 'class', 'client', 'code', 'colour', 'confidence', 'count', 'elbow', 'error', 'ev', 'events',
  'express', 'facing', 'from', 'feeling', 'for_ms', 'frames', 'got', 'grants', 'hold_max_ms', 'how', 'id',
  'joints', 'kind', 'lamp', 'lamp_state', 'lease', 'level', 'light', 'limits', 'manage', 'mood',
  'most', 'motion', 'name', 'near', 'ok', 'on', 'op', 'open', 'parts', 'pattern', 'paused', 'phrase', 'phrases',
  'pitch', 'presence', 'press', 'private', 'push', 'quiet_hours', 're', 'refused', 'retry_ms',
  'roll', 'say', 'say_max_chars', 'score', 'seen', 'shoulder', 'shows', 'side', 'size', 'skill', 'skills', 'state',
  'step_ms', 'strength', 'take_max_ms', 'talking', 'targets', 'to', 'until', 'v', 'where', 'why',
  'within_ms', 'x', 'y', 'yaw',
]);
// A said text's tag is keyed with this process's own random key: the same text
// gives the same tag within a run, and nobody holding the log can test a guess.
const TAG_KEY = randomBytes(16);

// Read at each frame, so setting LAMPOS_LOG in a running program takes effect.
export const on = () => (process.env.LAMPOS_LOG ?? '').trim().toLowerCase() === 'debug';

// A copy of `value` safe to print.
export function redact(value, key) {
  if (SECRET.has(key) && value !== undefined && value !== null) return '<redacted>';
  if (key === 'text' && typeof value === 'string') {
    return `<${value.length} chars, tag ${createHmac('sha256', TAG_KEY).update(value).digest('hex').slice(0, 8)}>`;
  }
  if (key === 'data' && typeof value === 'string') return `<${value.length} chars>`;
  if (Array.isArray(value)) return value.map((item) => redact(item));
  if (value && typeof value === 'object') {
    const pairing = value.op === 'pair';
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, pairing && k === 'code' ? '<redacted>' : redact(v, k)]));
  }
  return value;
}

// Print one frame, `->` sent or `<-` received, when LAMPOS_LOG=debug.
export function frame(direction, value) {
  if (on()) process.stderr.write(`lampos ${direction} ${JSON.stringify(redact(value))}\n`);
}
