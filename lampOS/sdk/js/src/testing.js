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
import { X509Certificate, createPublicKey, randomBytes, verify } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:https';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WebSocketServer } from 'ws';
import * as schema from './schema.js';
import { HELLO, b64, pinOf, proof, unb64 } from './keys.js';

const now = () => new Date().toISOString();

// `express`'s feelings and their moves, as lamp-control's feelings.rs.
export const FEELING_MOVES = { glad: 'glad-lift', lively: 'perk', low: 'low-droop' };

// `look`'s targets lamp-control doesn't route yet, in parts.rs's words.
export const LOOK_WAITS = {
  face: 'not in this release: look at a face or a name',
  name: 'not in this release: look at a face or a name',
};

// What lamp-control refuses today, in its own words (crates/control/src/
// parts.rs, `waits_for`): pass it as `refuses` for a fake that answers as a
// lamp does now. `lamp fake` does.
export const LAMP_CONTROL_REFUSES = Object.freeze({
  // No product unit passes --lease-socket yet (docs/control.md).
  look: "the body's lease socket is not on here",
  joints: "not routed yet: waits for a goal stream on the body's lease socket",
  play: 'not routed yet: waits for client packages',
  sound: 'not routed yet: waits for client packages',
});
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

// A move, as lamp-body's `moves::check` holds it (crates/body/src/moves.rs): head
// pitch, head roll, base yaw, shoulder, elbow, in counts from rest and counts a
// second. The Python SDK's skillcheck has the same.
export const MOVE_STEP_MS = [40, 1000];
export const MOVE_LASTS_MS = [1500, 10000];
// A computer's move: 2 to 500 frames (lamp-control's protocol.rs FRAMES).
export const MOVE_FRAMES = [2, 500];
export const MOVE_FARTHEST = [200, 200, 150, 350, 350];
export const MOVE_FASTEST = [900, 1300, 1000, 800, 800];

// Rust's `{:.0}`, as the body prints a speed: a half rounds to the even neighbour.
function roundHalfEven(value) {
  const floor = Math.floor(value);
  const part = value - floor;
  if (part !== 0.5) return Math.round(value);
  return floor % 2 === 0 ? floor : floor + 1;
}

// Why the body refuses a move of these steps and frames, in its words, at its
// first problem; null for one it plays.
export function moveWhy(stepMs, frames) {
  if (stepMs < MOVE_STEP_MS[0] || stepMs > MOVE_STEP_MS[1]) return `step_ms ${stepMs} is not from 40 to 1000`;
  const lasts = Math.max(frames.length - 1, 0) * stepMs;
  if (lasts < MOVE_LASTS_MS[0] || lasts > MOVE_LASTS_MS[1]) return `it lasts ${lasts} ms, not 1500 to 10000`;
  const rest = (frame) => Array.isArray(frame) && frame.length === 5 && frame.every((count) => count === 0);
  if (!frames.length || !rest(frames[0]) || !rest(frames.at(-1))) return 'it does not begin and end at nothing';
  const step = stepMs / 1000;
  for (let at = 1; at < frames.length; at++) {
    for (let joint = 0; joint < 5; joint++) {
      const after = frames[at][joint];
      if (Math.abs(after) > MOVE_FARTHEST[joint]) return `frame ${at} takes joint ${joint} ${after} counts, farther than ${MOVE_FARTHEST[joint]}`;
      const speed = Math.abs(after - frames[at - 1][joint]) / step;
      if (speed > MOVE_FASTEST[joint] + 2 / step) {
        return `frame ${at} asks joint ${joint} for ${roundHalfEven(speed)} counts a second, faster than ${MOVE_FASTEST[joint]}`;
      }
    }
  }
  return null;
}

// lamp-control's refusal of a computer's `move` (protocol.rs), in its words, as
// `bad_request`: its own shape first, then the body's check; null for one it takes.
export function moveRefusal(stepMs, frames) {
  if (stepMs < MOVE_STEP_MS[0] || stepMs > MOVE_STEP_MS[1]) return `step_ms: from ${MOVE_STEP_MS[0]} to ${MOVE_STEP_MS[1]}`;
  if (frames.length < MOVE_FRAMES[0] || frames.length > MOVE_FRAMES[1]) return `frames: from ${MOVE_FRAMES[0]} to ${MOVE_FRAMES[1]}`;
  const home = (frame) => Array.isArray(frame) && frame.length === 5 && frame.every((count) => count === 0);
  if (!home(frames[0]) || !home(frames.at(-1))) return 'frames: start and end at home, all five 0';
  const why = moveWhy(stepMs, frames);
  return why === null ? null : `frames: a move this body cannot play: ${why}`;
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

  // `autoDone: false` leaves a play's or say's `done` to the test.
  // `refuses`: ops refused `refused` with this why, as a lamp that doesn't
  // route them yet does (LAMP_CONTROL_REFUSES); none by default.
  constructor({ code = '12345678', grants = ['body', 'light', 'speak'], lampId = 'lamp-fake', name = 'Fake lamp', autoDone = true, refuses = {}, followsHand = true } = {}) {
    // `followsHand: false` answers a look at `hand` as a lamp-control from
    // before #4183 does: taken, with no `follow`, and nothing follows it.
    Object.assign(this, { code, grants: [...grants], host: '127.0.0.1', port: 0, pin: '', fp: '', autoDone, refuses: { ...refuses }, followsHand });
    this.asker = new Map();
    this.lamp = { id: lampId, name, v: schema.VERSION };
    this.clients = new Map();
    this.requests = [];
    this.received = [];
    // WebSocket pings from clients (their keepalive).
    this.pings = 0;
    this.leases = new Map();
    this.subscribed = new Map();
    this.state = { presence: { count: 0, near: 0, facing: false }, light: null, mood: null, lamp: { talking: false, paused: false, private: false, asleep: false },
      // Made-up readings, as lamp-control answers them; `setAir` changes the
      // air and tells `air.changed`, `{ known: false }` for none.
      air: { ...AIR }, device: { ...DEVICE, temperature_c: { ...DEVICE.temperature_c } } };
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

  async stop() {
    for (const ws of this.wss.clients) ws.terminate();
    await new Promise((resolve) => this.wss.close(resolve));
    this.https.closeAllConnections();
    await new Promise((resolve) => this.https.close(resolve));
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

  // The ring shows a computer's light while it lasts.
  show(request) {
    this.shown = { light: { on: true, colour: request.colour, level: request.level }, until: this.clock() + request.for_ms };
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

  unfollow(lease) {
    for (const [held, following] of [...this.following]) if (following.lease === lease) this.following.delete(held);
  }

  // Tell a lease's holder it is `held`, `suspended` or `lost` (and, lost, end it here).
  leaseEvent(lease, state, why) {
    const held = this.leases.get(lease);
    if (!held) return;
    if (state === 'lost') this.leases.delete(lease);
    held.ws.send(JSON.stringify({ ev: 'lease', lease, state, ...(why ? { why } : {}) }));
    if (state === 'lost') this.unfollow(lease);
  }

  loseLease(lease, why = 'hand') { this.leaseEvent(lease, 'lost', why); }

  // End the action whose request id was `re`, to the client that asked.
  done(re, how = 'finished', why) {
    this.asker.get(re)?.send(JSON.stringify({ ev: 'done', re, how, ...(why ? { why } : {}) }));
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
      if (request.op === 'pair') return this.pair(ws, request, nonce);
      if (request.op === 'hello') { client = this.hello(ws, request, nonce); return undefined; }
      if (!client) return this.refuse(ws, id, 'no_hello', 'hello first');
      return this.op(ws, client, request);
    });
  }

  refuse(ws, re, code, why, retryMs) {
    ws.send(JSON.stringify({ re, ok: false, error: { code, why, ...(retryMs !== undefined ? { retry_ms: retryMs } : {}) } }));
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
    this.ok(ws, request.id, { lamp: this.lamp, grants: paired.grants, limits: { joints_hz: 40 } });
    return request.client;
  }

  // The grant each event and state needs, as lamp-control's own (#3736).
  static EVENT_GRANT = {
    'person.arrived': 'look', 'person.left': 'look', 'person.stood_up': 'look', 'person.facing': 'look',
    hand: 'look',
    'phrase.heard': 'hear', 'name.heard': 'hear', 'sound.heard': 'sound',
    'air.changed': 'air',
  };
  static STATE_GRANT = { presence: 'look', air: 'air', device: 'device' };

  needs(op, request) {
    if (op === 'subscribe') return [...new Set((request.events ?? []).map((e) => FakeLamp.EVENT_GRANT[e]).filter(Boolean))];
    if (op === 'get') return [...new Set((request.state ?? []).map((s) => FakeLamp.STATE_GRANT[s]).filter(Boolean))];
    if (op === 'look') return ['face', 'hand'].includes(request.at) ? ['body', 'look'] : ['body'];
    if (['gesture', 'move', 'play', 'joints'].includes(op)) return ['body'];
    if (op === 'lease.take') return (request.parts ?? []).map((p) => ({ body: 'body', light: 'light', voice: 'speak' })[p]);
    return { express: ['body', 'light'], light: ['light'], say: ['speak'], sound: ['speak'] }[op] ?? [];
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
      case 'stop':
        // The holder's stop ends its following, as any of its actions.
        this.stopFollowing(ws);
        return this.ok(ws, id);
      case 'express': {
        // As lamp-control's feelings.rs: the feelings skill's three, its
        // move to the body (smaller below strength 0.5) and its light; no
        // `done` follows. The fake always shows the light.
        const name = FEELING_MOVES[request.feeling];
        if (!name) return this.refuse(ws, id, 'bad_request', 'feeling: one of glad, lively, low');
        const rounded = Math.round(Math.min(Math.max((request.strength ?? 0.5) / 0.5, 0), 1) * 4);
        // `low` goes no smaller than three quarters (feelings.rs `least_step`).
        const step = rounded === 0 ? 0 : Math.max(rounded, request.feeling === 'low' ? 3 : 1);
        const move = step === 0 ? null : step === 4 ? name : `${name}-${step * 25}`;
        return this.ok(ws, id, { to: 'body', move, light: true });
      }
      case 'look': {
        // As lamp-control with --lease-socket (parts.rs `look`): `face` and
        // `name` aren't routed; `hand` is followed until a stop, another look
        // or the lease ends, so no `done` follows it; the body takes the rest,
        // and its `done` says where the head got to.
        if (typeof request.at === 'string' && LOOK_WAITS[request.at]) return this.refuse(ws, id, 'refused', LOOK_WAITS[request.at]);
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
        this.ok(ws, id, { to: 'body' });
        if (this.autoDone) {
          const { yaw = 0, pitch = 0 } = typeof request.at === 'object' ? request.at : {};
          ws.send(JSON.stringify({ ev: 'done', re: id, how: 'finished', yaw, pitch, clamped: false }));
        }
        return undefined;
      }
      case 'gesture':
      case 'move':
        // As lamp-control routes them: the body takes them, and no `done`
        // follows.
        return this.ok(ws, id, { to: 'body' });
      case 'say':
        // As lamp-control: the voice takes it, and its `done` follows.
        this.ok(ws, id, { to: 'voice' });
        if (this.autoDone) this.done(id);
        return undefined;
      default:
        this.ok(ws, id);
        if (op === 'play' && this.autoDone) this.done(id);
        return undefined;
    }
  }
}
