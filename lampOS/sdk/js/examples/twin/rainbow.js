// "Endless rainbow": the ring turns through the rainbow until it is switched off, the page closes or the run ends. One `light`
// op with `pixels` per frame (docs/control.md, "The ring, pixel by pixel": "each `light` replaces the last under the same claim,
// so a computer can send a frame at a time"). The lamp's own lights (a conversation, someone seen, a touch, privacy) are drawn
// over a computer's pixels and a touch ends the computer's light, so the loop never relies on the lamp resuming it: it sends the
// next frame as soon as the last was answered (or after a pause when the lamp said it was busy), each frame's lease short, so the
// rainbow is back within one frame of whatever was on top ending, and a page that is gone leaves no light behind for long.
import { setTimeout as sleep } from 'node:timers/promises';

// `#rrggbb` of a hue, 0 to 1 round the wheel.
export const hue = (h) => `#${[5, 3, 1].map((n) => { const k = (n + h * 6) % 6; return Math.round(255 * (1 - Math.max(0, Math.min(k, 4 - k, 1)))).toString(16).padStart(2, '0'); }).join('')}`;

// The ring's resting refresh is 40 a second (docs/control.md), so a frame sooner than 25 ms is never drawn.
export const FASTEST_MS = 25;
// The slowest a refused frame is tried again, and the pause when the lamp says its own light holds the ring.
export const SLOWEST_MS = 1000;

// The gap between frames that keeps under every `light` window hello says (`limits.rates.light`: a list of
// `{ most, within_ms }`, 5 in 1000 ms with the rates on), 10% over; with the rates off (`"off"`, the default), the ring's refresh.
export function frameGap(limits) {
  const windows = limits?.rates && limits.rates !== 'off' ? limits.rates.light : undefined;
  if (!Array.isArray(windows) || windows.length === 0) return FASTEST_MS;
  return Math.max(FASTEST_MS, ...windows.map(({ most, within_ms: within }) => Math.ceil((within / most) * 1.1)));
}

// The 32 colours of a frame: the wheel, turned by `turns` (0 to 1).
export const rainbowFrame = (count, turns) => Array.from({ length: count }, (_, k) => hue((k / count + turns) % 1));

// What the lamp says that is the lamp's own light or state on top, or a pause in its ears: try again later, never stop.
const LATER = new Set(['busy', 'rate', 'private', 'paused', 'asleep', 'quiet_hours', 'refused']);

// Starts the loop on `lamp` (the SDK's Lamp). `secondsPerTurn`: how long one turn of the wheel takes. Returns `{ stop(), stats(), ended }`;
// `ended` resolves when the loop has ended (stopped, or by an answer that cannot be tried again: the reason is in `stats().ended`).
// A touch on the lamp (the lamp's `touch` event, no grant needed) ends the loop: docs/control.md says a touch ends a computer's light, and a
// loop that sent again 25 ms later would give the person at the lamp no way to take the light back. (The person's own light change is
// not told to a computer by the protocol, so the loop cannot see it: it is the one thing a rainbow still sends over.)
export function startRainbow(lamp, { secondsPerTurn = 6, say = () => {}, now = () => Date.now(), stopOnTouch = true } = {}) {
  const count = lamp.ringPixels ?? 32;
  const gap = frameGap(lamp.limits);
  const lease = Math.min(lamp.limits?.hold_max_ms ?? 30_000, Math.max(250, gap * 4));
  const stats = { gapMs: gap, leaseMs: lease, sent: 0, answered: 0, refusals: 0, byCode: {}, ended: undefined };
  let stopping = false;
  const wake = new AbortController();
  const began = now();
  let pause = 0;
  let lastSaid;
  // `first`: settled by the first frame's answer: it answers, or the lamp said to wait (it comes back by itself): resolved; the lamp
  // said it cannot be done at all (no grant, a bad frame, a closed door): rejected with that answer, so a row says it in the lamp's words.
  let settleFirst;
  let firstDone = false;
  const first = new Promise((resolve, reject) => { settleFirst = { resolve: () => { firstDone = true; resolve(); }, reject: (error) => { firstDone = true; reject(error); } }; });
  first.catch(() => {});
  const run = async () => {
    while (!stopping) {
      const turns = (((now() - began) / 1000) / secondsPerTurn) % 1;
      try {
        stats.sent += 1;
        await lamp.ring(rainbowFrame(count, turns), { forMs: lease });
        stats.answered += 1;
        settleFirst.resolve();
        pause = 0;
        lastSaid = undefined;
      } catch (error) {
        const code = error?.code ?? 'error';
        // Stopped while the first frame was out and the lamp then refused it for good (no grant, a bad frame): the row that started the
        // loop still hears the refusal in the lamp's words, not a silent success.
        if (stopping) { if (!firstDone && !LATER.has(code)) settleFirst.reject(error); break; }
        stats.refusals += 1;
        stats.byCode[code] = (stats.byCode[code] ?? 0) + 1;
        if (!LATER.has(code)) { stats.ended = `${code}: ${error?.why ?? error?.message ?? error}`; settleFirst.reject(error); say(`the rainbow ended: ${stats.ended}`); break; }
        settleFirst.resolve();
        // The lamp's own light or state is on top, or it asked for slower: wait as long as it said (`retryMs`), else double.
        pause = Math.min(SLOWEST_MS, Math.max(error?.retryMs ?? 0, pause * 2 || gap));
        if (lastSaid !== code) { say(`the rainbow waits: the lamp said ${code}${error?.why ? ` (${error.why})` : ''}; it comes back by itself`); lastSaid = code; }
      }
      try { await sleep(Math.max(pause, gap), undefined, { signal: wake.signal }); } catch { break; }
    }
  };
  const onTouch = () => {
    if (stopping) return;
    stats.ended = 'touch: a touch on the lamp ended the rainbow; the lamp\'s own light is back';
    say(`the rainbow ended: ${stats.ended}`);
    stopping = true;
    wake.abort();
  };
  const touchWatched = stopOnTouch && typeof lamp.on === 'function';
  if (touchWatched) Promise.resolve(lamp.on('touch', onTouch)).catch(() => {});
  // However the loop ends (off, a touch, an answer that cannot be tried again, a closed connection), the touch handler goes and the first
  // answer is settled, so nothing waits on a loop that is over.
  const ended = run().finally(() => {
    if (touchWatched) Promise.resolve(lamp.off('touch', onTouch)).catch(() => {});
    if (!firstDone) settleFirst.resolve();
  });
  return {
    ended,
    first,
    stats: () => ({ ...stats, byCode: { ...stats.byCode } }),
    stop: async () => {
      stopping = true;
      wake.abort();
      await ended;
      // The last frame lapses with its lease; nothing else of ours is left on the ring.
    },
  };
}
