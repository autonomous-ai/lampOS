// LAMPOS_LOG=debug: every frame to and from the lamp, one line each on stderr.
// Off by default. What would let someone pair or say hello as this computer,
// or read the lamp's pin, never reaches the log: `proof`, `sig`, `key`,
// `nonce`, `pin` and `fp` are replaced, and so is a pair request's `code`, the
// owner's one-time code (a reply's `code` is a refusal's, and is kept). A
// say's `text` is given only as its length and the start of its SHA-256, and
// a pushed skill part's `data` only as its length.
import { createHash } from 'node:crypto';

// Fields replaced wherever they appear in a frame.
export const SECRET = new Set(['proof', 'sig', 'key', 'nonce', 'pin', 'fp']);

// Read at each frame, so setting LAMPOS_LOG in a running program takes effect.
export const on = () => (process.env.LAMPOS_LOG ?? '').trim().toLowerCase() === 'debug';

// A copy of `value` safe to print.
export function redact(value, key) {
  if (SECRET.has(key) && value !== undefined && value !== null) return '<redacted>';
  if (key === 'text' && typeof value === 'string') {
    return `<${value.length} chars, sha256 ${createHash('sha256').update(value).digest('hex').slice(0, 8)}>`;
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
