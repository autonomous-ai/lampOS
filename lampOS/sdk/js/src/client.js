// The lamp, from a developer's computer: pair once, then connect, listen and act.
//
//   const lamp = await Lamp.connect();
//   lamp.on('person.arrived', () => lamp.gesture('greeting'));
//
// Every message is docs/control.schema.json's. Names the schema enumerates are
// checked before sending, and a refusal rejects with its typed error.
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { hostname } from 'node:os';
import WebSocket from 'ws';
import * as debug from './debug.js';
import * as schema from './schema.js';
import { LampError, NotPaired, WrongLamp, byCode, fromReply } from './errors.js';
import { Keyring, b64, isPin, pinOf, proof, unb64 } from './keys.js';

/** A fast stream's binary `camera.frame`: its header, with the JPEG as `data`. */
export function binaryFrame(message) {
  const length = message.readUInt32BE(0);
  const frame = JSON.parse(message.subarray(4, 4 + length).toString());
  frame.data = message.subarray(4 + length);
  return frame;
}

// Events that come with a request, never subscribed to: a camera stream's.
const CAMERA_EVENTS = ['camera.frame', 'camera.ended'];

// `keepConnected`'s pauses before connecting again, in seconds: the first after
// a closed connection, then longer while the lamp can't be reached (a restart
// takes a few seconds; an update about a minute).
export const RECONNECT_PAUSES = Object.freeze([1, 2, 5, 10]);

// The actions lamp-control answers `{"to":"body"}` with no `done` after
// (docs/control.md): a wait for them ends at that answer. `say` and
// `turn.start` answer `{"to":"voice"}` and their `done` does follow.
export const ENDS_AT_REPLY = new Set(['gesture', 'move']);
// A phrase's floor in letters and digits, lamp-control's PHRASE_MIN_LETTERS.
// The SDK's check is a pre-check: it may let through what the lamp refuses,
// never the other way.
export const PHRASE_MIN_LETTERS = 8;

// The lamp's refusal of a phrase under the floor, or undefined; the lamp
// checks it again.
function shortPhrase(phrases) {
  const short = (phrases ?? []).find((p) => (p.match(/[\p{Alphabetic}\p{N}]/gu) ?? []).length < PHRASE_MIN_LETTERS);
  if (short === undefined) return undefined;
  return new byCode.bad_request(`phrases: ${JSON.stringify(short)} is too short: ${PHRASE_MIN_LETTERS} letters at least`);
}

// One connection: its frames as they come, and the first frame, which must be
// the lamp's welcome. The certificate is checked against `pin` before any
// frame is sent.
function open(host, port, pin) {
  return new Promise((resolve, reject) => {
    if (!isPin(pin)) {
      reject(new WrongLamp("no pin to check the lamp by: the 64 hex the owner's app shows with the code"));
      return;
    }
    // The lamp's certificate is its own, self-signed: it is checked against its
    // pin (the full SHA-256 the owner's app shows), not a CA.
    const ws = new WebSocket(`wss://${host}:${port}/v${schema.VERSION}`, { rejectUnauthorized: false, maxPayload: 1 << 20 });
    const frames = [];
    let wake = null;
    let seen = null;
    ws.on('upgrade', (response) => {
      seen = pinOf(response.socket.getPeerCertificate(true).raw);
      if (seen !== pin) {
        ws.terminate();
        reject(new WrongLamp(`the certificate at ${host} is not the lamp's: ${seen.slice(0, 16)}..., not ${pin.slice(0, 16)}...`));
      }
    });
    ws.on('message', (data) => {
      const frame = JSON.parse(data.toString());
      debug.frame('<-', frame);
      frames.push(frame);
      wake?.();
    });
    ws.on('close', () => wake?.());
    ws.on('error', reject);
    const next = async (ms = 10_000) => {
      const until = Date.now() + ms;
      while (!frames.length) {
        if (ws.readyState >= WebSocket.CLOSING || Date.now() > until) {
          throw new LampError('the lamp did not answer', { code: 'closed' });
        }
        await new Promise((r) => { wake = r; setTimeout(r, 50); });
      }
      return frames.shift();
    };
    ws.on('open', async () => {
      try {
        const welcome = await next(5_000);
        if (welcome.ev !== 'welcome' || !welcome.nonce) throw new LampError("the lamp's first frame was not a welcome", { code: 'bad_welcome' });
        try {
          welcome.nonceBytes = unb64(welcome.nonce, 32);
        } catch (error) {
          throw new LampError(`the welcome's nonce is not 32 bytes of base64: ${error.message}`, { code: 'bad_welcome' });
        }
        resolve({ ws, seen, welcome, next, frames });
      } catch (error) { ws.close(); reject(error); }
    });
  });
}

// '#rrggbb' from '#rrggbb' or [r, g, b], each 0-255.
function hex(colour) {
  if (typeof colour === 'string') return colour;
  if (!Array.isArray(colour) || colour.length !== 3 || !colour.every((c) => Number.isInteger(c) && c >= 0 && c <= 255)) {
    throw new LampError(`a colour's channels are 0 to 255: ${JSON.stringify(colour)}`, { code: 'bad_request' });
  }
  return `#${colour.map((c) => c.toString(16).padStart(2, '0')).join('')}`;
}

// The longest say's text in UTF-8 bytes: the voice's own cap, which
// lamp-control holds too (protocol.rs SAY_MAX_BYTES, lamp-voice's
// MOST_WORDS_BYTES), checked here at the same bytes, never stricter.
export const SAY_MAX_BYTES = 900;

function check(op, fields) {
  if (!schema.ops.includes(op)) throw new LampError(`no op ${op} in v${schema.VERSION}`, { code: 'bad_request' });
  if (op === 'say' && typeof fields.text === 'string') {
    // Any control character (Unicode Cc, as Rust's char::is_control), a newline
    // or tab too: the door and the voice refuse them.
    if (/\p{Cc}/u.test(fields.text)) throw new LampError('text: words, no control characters', { code: 'bad_request' });
    if (Buffer.byteLength(fields.text, 'utf8') > SAY_MAX_BYTES) {
      throw new LampError(`text: at most ${SAY_MAX_BYTES} bytes as the voice takes it`, { code: 'bad_request' });
    }
  }
  // A gesture's name is held to its shape only, as the door holds it: any body
  // clip, `idle` and `scanning` too (never stricter than the lamp).
  if (op === 'gesture' && fields.name !== undefined && !(typeof fields.name === 'string' && schema.gesturePattern.test(fields.name))) {
    throw new LampError(`name ${fields.name}: a lowercase letter, then lowercase letters, digits, _ or -, at most 64`, { code: 'bad_request' });
  }
  const allowed = { colour: schema.colours, pattern: schema.patterns };
  for (const [field, names] of Object.entries(allowed)) {
    const value = fields[field];
    if (names && value !== undefined && !names.includes(value)) {
      throw new LampError(`${field} ${value}: one of ${names.join(', ')}`, { code: 'bad_request' });
    }
  }
  for (const event of fields.events ?? []) {
    if (!schema.events.includes(event)) throw new LampError(`no event ${event}`, { code: 'bad_request' });
  }
  // A lease's parts; a `sound.upload`'s or `skill.push`'s `parts` is a count.
  for (const part of op === 'lease.take' ? fields.parts ?? [] : []) {
    if (!schema.parts.includes(part)) throw new LampError(`no part ${part}`, { code: 'bad_request' });
  }
}

// A lease on some of the lamp's parts, renewed at half its time until it is
// released or the lamp takes it back; `state` follows the lamp's lease events.
export class Lease {
  constructor(lamp, parts, forMs, why) {
    Object.assign(this, { lamp, parts, forMs, why, id: null, state: 'none', lostWhy: null, timer: null });
  }

  async take() {
    const reply = await this.lamp.request('lease.take', { parts: this.parts, for_ms: this.forMs, ...(this.why ? { why: this.why } : {}) });
    this.id = reply.lease;
    this.state = 'held';
    this.lamp.leases.set(this.id, this);
    this.timer = setInterval(() => this.renew(), this.forMs / 2);
    return this;
  }

  async renew() {
    if (this.state !== 'held' && this.state !== 'suspended') return clearInterval(this.timer);
    try {
      await this.lamp.request('lease.renew', { lease: this.id, for_ms: this.forMs });
    } catch (error) {
      this.state = 'lost';
      this.lostWhy = error.why;
      clearInterval(this.timer);
    }
  }

  event(event) {
    this.state = event.state;
    if (event.state === 'lost') { this.lostWhy = event.why ?? null; clearInterval(this.timer); }
  }

  get held() { return this.state === 'held'; }

  async release() {
    clearInterval(this.timer);
    if (this.id && (this.state === 'held' || this.state === 'suspended')) {
      this.state = 'released';
      await this.lamp.request('lease.release', { lease: this.id }).catch(() => {});
    }
  }
}

// The head following a hand (`look` at `'hand'`), under a body lease of its
// own. lamp-body follows on the lamp, waiting while the hand is gone, until
// `stop()`, the client's `stop`, another look, or the lamp taking the body
// back (a hand on the head, a conversation). lamp-control answers
// `{"to":"body","follow":"hand"}` and says no `done`, so `ended` is told
// here: `{ how: 'finished', why: 'stopped' }`, or `{ how: 'cut_short', why:
// 'stop' | 'another look' | the lease's why }`. The client's `stop` lets go of
// its lease too. After another look the lease stays held, since that look runs
// under it, and the lamp's own life stands back until `stop()`; a new
// `followHand()` lets go of it first. Where the hand is comes as `hand` events:
// `seen` with `x`, `y`, `size`, `open`, `confidence`, and `seen: false` once
// it's gone. Needs the `body` and `look` grants.
export class Following {
  constructor(lamp, forMs, why) {
    this.lamp = lamp;
    this.lease = new Lease(lamp, ['body'], forMs, why);
    this.result = null;
    this.ended = new Promise((resolve) => { this.resolveEnded = resolve; });
    this.onLease = (event) => {
      if (event.lease === this.lease.id && event.state === 'lost') this.end('cut_short', event.why ?? null);
    };
  }

  end(how, why) {
    if (!this.result) {
      this.result = { how, why };
      this.resolveEnded(this.result);
    }
  }

  async start() {
    // One following a connection: the one before it ends, and its lease is let
    // go, or this one's take would be refused `busy`.
    const before = this.lamp.following;
    if (before) {
      before.end('cut_short', 'another look');
      await before.finish();
    }
    await this.lease.take();
    this.lamp.on('lease', this.onLease);
    let reply;
    try {
      reply = await this.lamp.request('look', { at: 'hand' });
    } catch (error) {
      await this.finish();
      throw error;
    }
    if (reply.follow !== 'hand') {
      // A lamp that took the look but doesn't say it follows (a lamp-control
      // from before #4183): let go rather than claim it does.
      await this.finish();
      throw new byCode.refused("this lamp doesn't follow a hand yet");
    }
    // Another look or a stop from this client ends it (`look`, `stop`).
    this.lamp.following = this;
    return this;
  }

  get following() { return !this.result && this.lease.held; }

  // Until it ends: how, and why.
  wait() { return this.ended; }

  // Stop following: the lease is let go, and the head goes back to the lamp's
  // own pose. Says how it ended (`finished`, 'stopped', if still following).
  async stop() {
    this.end('finished', 'stopped');
    await this.finish();
    return this.result;
  }

  async finish() {
    // The lamp keeps it while its lease is held, so a new following can let go
    // of it.
    if (this.lamp.following === this) this.lamp.following = null;
    await this.lamp.off('lease', this.onLease);
    await this.lease.release();
  }
}

export class Lamp {
  // A ping this often keeps a connection that only listens open: the lamp
  // closes one quiet for 2 minutes (lamp-control's QUIET_MAX, #3778). A ping
  // with no pong by the next one means the lamp is gone without a close (off
  // its Wi-Fi): the connection is ended, as Python's `websockets` does.
  static KEEPALIVE_MS = 20_000;

  constructor(ws, welcome, hello, frames) {
    this.ws = ws;
    this.welcome = welcome;
    this.info = hello.lamp ?? welcome.lamp;
    this.grants = hello.grants ?? [];
    this.limits = hello.limits ?? {};
    this.nextId = 1;
    // The hand this connection's head follows, if any (`followHand`).
    this.following = null;
    this.waiting = new Map();
    this.done = new Map();
    this.handlers = new Map();
    this.phrases = [];
    this.leases = new Map();
    ws.removeAllListeners('message');
    for (const frame of frames.splice(0)) this.frame(frame);
    ws.on('message', (data, isBinary) => {
      // A fast camera frame: the header's length, the header, then the JPEG,
      // given as `data` (a Buffer).
      if (isBinary) return this.frame(binaryFrame(data));
      const frame = JSON.parse(data.toString());
      debug.frame('<-', frame);
      this.frame(frame);
    });
    let answered = true;
    ws.on('pong', () => { answered = true; });
    this.keepalive = setInterval(() => {
      if (!answered) {
        ws.terminate();
        return;
      }
      answered = false;
      ws.ping();
    }, Lamp.KEEPALIVE_MS);
    this.keepalive.unref?.();
    ws.on('close', () => {
      clearInterval(this.keepalive);
      for (const { reject } of [...this.waiting.values(), ...this.done.values()]) {
        reject(new LampError('the connection to the lamp closed', { code: 'closed' }));
      }
      for (const lease of this.leases.values()) clearInterval(lease.timer);
    });
  }

  // Pair this computer with the lamp at `host`, with the owner's one-time code
  // and the pin the app shows with it (the certificate's full SHA-256, 64 hex).
  // The certificate is checked against the pin before any frame is sent (only
  // the TLS handshake and the WebSocket's upgrade request go first), so the
  // code's proof never reaches another host; there is no pairing without a pin.
  static async pair(code, host, { pin, port = schema.PORT, name, keyring = new Keyring() } = {}) {
    pin = String(pin ?? '').replaceAll(' ', '').replaceAll(':', '').toLowerCase();
    code = String(code).replaceAll(' ', '');
    if (!/^\d{8}$/.test(code)) throw new LampError('the code is the 8 digits the app shows or the lamp says', { code: 'bad_request' });
    const { ws, welcome, next } = await open(host, port, pin);
    let reply;
    try {
      const key = keyring.public();
      const ask = {
        id: 1, op: 'pair', name: (name ?? hostname()).slice(0, 64),
        key: b64(key), proof: b64(proof(code, Buffer.from(pin, 'hex'), key, welcome.nonceBytes)),
      };
      debug.frame('->', ask);
      ws.send(JSON.stringify(ask));
      reply = await next();
    } finally { ws.close(); }
    if (!reply.ok) throw fromReply(reply.error);
    const pairing = {
      lamp: welcome.lamp.id, name: welcome.lamp.name ?? '', client: reply.client,
      grants: reply.grants ?? [], fp: pin, host, port,
    };
    keyring.remember(pairing);
    return pairing;
  }

  // Connect to a paired lamp (by its id, or the only one paired) and say hello.
  static async connect(lamp, { host, keyring = new Keyring() } = {}) {
    const pairing = keyring.pairing(lamp);
    if (!pairing) throw new NotPaired('no pairing here for that lamp: run `lamp pair` first');
    const { ws, welcome, next, frames } = await open(host ?? pairing.host, pairing.port, pairing.fp);
    let reply;
    try {
      const hello = { id: 0, op: 'hello', client: pairing.client, sig: b64(keyring.hello(welcome.nonceBytes)) };
      debug.frame('->', hello);
      ws.send(JSON.stringify(hello));
      reply = await next();
    } catch (error) { ws.close(); throw error; }
    if (!reply.ok) { ws.close(); throw fromReply(reply.error); }
    return new Lamp(ws, welcome, reply, frames);
  }

  /**
   * Run a long-running program across the lamp's restarts (every update
   * restarts it, and the connection closes). Connects, runs
   * `await setup(lamp)` (subscribe, ask for leases, as after `connect`), and
   * waits until the connection ends; then connects again and runs `setup`
   * again on the new connection, after a pause that grows along `pauses`
   * (seconds) while the lamp can't be reached and starts over once it can.
   *
   * Never resolves by itself: pass `signal` (an AbortSignal) to stop it.
   * Rejects at once with what connecting again can't mend: no pairing
   * (`NotPaired`), another lamp at the address (`WrongLamp`), a refusal of
   * the hello (`unpaired`, revoked), and anything `setup` throws, a refused
   * subscription it awaited (`no_grant`) included.
   */
  static async keepConnected(setup, { lamp, host, keyring, pauses = RECONNECT_PAUSES, signal } = {}) {
    // An abort already fired doesn't fire again for a listener added later,
    // so each step checks `signal.aborted` first (Second's P2 on #5833).
    const pause = (seconds) => new Promise((resolve) => {
      if (signal?.aborted) { resolve(); return; }
      const timer = setTimeout(done, seconds * 1000);
      function done() { clearTimeout(timer); signal?.removeEventListener('abort', done); resolve(); }
      signal?.addEventListener('abort', done, { once: true });
    });
    let failures = 0;
    while (!signal?.aborted) {
      let connection;
      try {
        connection = await Lamp.connect(lamp, { host, keyring });
      } catch (error) {
        // `closed`: the lamp went while saying hello, as a restart does.
        if (error instanceof LampError && error.code !== 'closed') throw error;
        const wait = pauses[Math.min(failures, pauses.length - 1)];
        if (debug.on()) process.stderr.write(`lampos .. not reached (${error.code ?? error.name}), again in ${wait} s\n`);
        failures += 1;
        await pause(wait);
        continue;
      }
      failures = 0;
      if (signal?.aborted) {
        // Aborted while it connected: closed again, setup never run.
        await connection.close().catch(() => {});
        break;
      }
      const stop = () => connection.close();
      signal?.addEventListener('abort', stop, { once: true });
      try {
        await setup(connection);
        await connection.closed();
      } catch (error) {
        // The lamp went in the middle of setup: connect again.
        if (!(error instanceof LampError) || error.code !== 'closed') throw error;
      } finally {
        signal?.removeEventListener('abort', stop);
        await connection.close().catch(() => {});
      }
      if (!signal?.aborted) await pause(pauses[0]);
    }
  }

  async close() {
    for (const lease of [...this.leases.values()]) await lease.release();
    this.ws.close();
  }

  closed() { return new Promise((resolve) => (this.ws.readyState === WebSocket.CLOSED ? resolve() : this.ws.once('close', resolve))); }

  // Send one request; resolves with its reply, rejects with its typed error.
  request(op, fields = {}) {
    try { check(op, fields); } catch (error) { return Promise.reject(error); }
    return this.send(this.nextId++, op, fields);
  }

  // Send a checked request under its own id.
  send(id, op, fields) {
    // A socket no longer open drops a frame without a word, and its reply
    // would never come: so a request after the lamp has gone is refused here.
    if (this.ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new LampError('the connection to the lamp closed', { code: 'closed' }));
    }
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      const sent = { id, op, ...fields };
      debug.frame('->', sent);
      this.ws.send(JSON.stringify(sent), (error) => {
        if (error && this.waiting.get(id)?.reject === reject) {
          this.waiting.delete(id);
          reject(new LampError(`the connection to the lamp closed: ${error.message}`, { code: 'closed' }));
        }
      });
    }).then((reply) => {
      if (!reply.ok) throw fromReply(reply.error);
      return reply;
    });
  }

  // An action: its reply when taken, or with `wait` its `done` event; for the
  // actions no `done` follows yet (`ENDS_AT_REPLY`), the reply.
  act(op, fields, wait = false) {
    const clean = Object.fromEntries(Object.entries(fields).filter(([, v]) => v !== undefined));
    // Checked before an id is taken, so an action refused here has no id,
    // and every id is this call's own.
    try { check(op, clean); } catch (error) { return Promise.reject(error); }
    const id = this.nextId++;
    // Handed to the body, which says nothing back, `gesture` and `move` get
    // no `done` (docs/control.md): their wait ends at the answer, so none is
    // kept for them.
    const done = wait && !ENDS_AT_REPLY.has(op)
      ? new Promise((resolve, reject) => this.done.set(id, { resolve, reject }))
      : null;
    // A close rejects every `done`; when it comes before the reply, `taken`
    // carries the rejection to the caller and this one has no listener, which
    // Node would end the program for.
    done?.catch(() => {});
    const taken = this.send(id, op, clean);
    if (!done) return taken;
    // A refused action takes its `done` back, so nothing is left to reject,
    // unheard, when the connection closes.
    return taken.then(() => done, (error) => { this.done.delete(id); throw error; });
  }

  frame(frame) {
    if ('re' in frame && 'ok' in frame) {
      const waiting = this.waiting.get(frame.re);
      if (waiting) { this.waiting.delete(frame.re); waiting.resolve(frame); }
      return;
    }
    if (!frame.ev) return;
    if (frame.ev === 'lease') this.leases.get(frame.lease)?.event(frame);
    if (frame.ev === 'done') {
      const done = this.done.get(frame.re);
      if (done) { this.done.delete(frame.re); done.resolve(frame); }
    }
    for (const handler of this.handlers.get(frame.ev) ?? []) {
      Promise.resolve().then(() => handler(frame)).catch((error) => this.ws.emit('handlerError', error));
    }
  }

  // Call `handler(event)` for each `event` the lamp says; subscribes at once.
  on(event, handler, { phrases } = {}) {
    if (!schema.events.includes(event) && event !== 'lease' && event !== 'done' && !CAMERA_EVENTS.includes(event)) {
      throw new LampError(`no event ${event}`, { code: 'bad_request' });
    }
    const short = shortPhrase(phrases);
    if (short) throw short;
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), handler]);
    if (phrases) this.phrases = [...new Set([...this.phrases, ...phrases])].sort();
    if (!schema.events.includes(event)) return Promise.resolve();
    // A refused subscription takes its handler back off, so a later `on()`
    // for the event, once granted, doesn't call both.
    return this.subscribe([event]).catch((error) => {
      // Only the copy this call added: the same function may be there
      // already for a granted subscription.
      const left = [...(this.handlers.get(event) ?? [])];
      const at = left.lastIndexOf(handler);
      if (at >= 0) left.splice(at, 1);
      if (left.length) this.handlers.set(event, left);
      else this.handlers.delete(event);
      throw error;
    });
  }

  // Stop calling `handler` for `event`; with none left for an event the lamp
  // says, unsubscribe from it.
  off(event, handler) {
    const left = (this.handlers.get(event) ?? []).filter((h) => h !== handler);
    if (left.length) {
      this.handlers.set(event, left);
      return Promise.resolve();
    }
    this.handlers.delete(event);
    return schema.events.includes(event) ? this.request('unsubscribe', { events: [event] }) : Promise.resolve();
  }

  subscribe(events, phrases) {
    const short = shortPhrase(phrases);
    if (short) return Promise.reject(short);
    const wanted = phrases ?? (events.includes('phrase.heard') && this.phrases.length ? this.phrases : undefined);
    return this.request('subscribe', { events, ...(wanted ? { phrases: wanted } : {}) });
  }

  unsubscribe(events) {
    for (const event of events) this.handlers.delete(event);
    return this.request('unsubscribe', { events });
  }

  // The states asked for (presence, light, mood, lamp), by name: the reply's `state`.
  async get(...states) { return (await this.request('get', { state: states })).state ?? {}; }
  // The air sensor's newest reading (the `air` grant), and `known`; only
  // `{ known: false }` with no reading. Each change past a step comes as
  // `lamp.on('air.changed', ...)`.
  async air() { return (await this.get('air')).air; }
  // The lamp's own computer (the `device` grant): `cpu_load`, `memory_used`,
  // `disk_free`, `uptime_s`, `temperature_c`, and `known`.
  async device() { return (await this.get('device')).device; }

  /** What the body may do, joint by joint, and where each joint is now:
   * `get('body')`'s `body` (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`, each
   * with `deg` from its home, `min_deg`, `max_deg` and its counts; the speed,
   * load and heat limits), or `known: false` when the body has said nothing
   * in the last 15 s. The head's and base's bounds are a folded lamp's: a
   * standing lamp's head may sit outside them, so don't clamp a target to
   * them; the body holds every look to its own. */
  async bodyInfo() { return (await this.get('body')).body ?? {}; }

  // `const lease = await lamp.lease('body').take()` ... `await lease.release()`,
  // or `await lamp.withLease(['body'], async (lease) => ...)`.
  lease(...parts) { return new Lease(this, parts, 30_000, undefined); }

  async withLease(parts, body, { forMs = 30_000, why } = {}) {
    const lease = await new Lease(this, parts, forMs, why).take();
    try { return await body(lease); } finally { await lease.release(); }
  }

  express(feeling, strength) { return this.act('express', { feeling, strength }); }
  gesture(name, { wait = false } = {}) { return this.act('gesture', { name }, wait); }
  // Taken, another look ends a following (lamp-control's own rule).
  async look(at) {
    const reply = await this.act('look', { at });
    this.following?.end('cut_short', 'another look');
    return reply;
  }
  // `const following = await lamp.followHand().start()` ... `await
  // following.stop()`, or `await lamp.withHand(async (following) => ...)`:
  // the head turns after the hand the lamp sees. Listen with
  // `lamp.on('hand', ...)` for where the hand is.
  followHand({ forMs = 10_000, why } = {}) { return new Following(this, forMs, why); }
  async withHand(body, options) {
    const following = await this.followHand(options).start();
    try { return await body(following); } finally { await following.stop(); }
  }
  move(frames, stepMs, { wait = false } = {}) { return this.act('move', { step_ms: stepMs, frames }, wait); }
  play(name, { wait = false } = {}) { return this.act('play', { name }, wait); }
  joints(lease, withinMs, targets) { return this.act('joints', { lease: lease.id, targets, within_ms: withinMs }); }
  light(colour, level, forMs, pattern) { return this.act('light', { colour, level, pattern, for_ms: forMs }); }
  // How many pixels the ring has, from hello's limits (undefined from a lamp before pixel lights).
  get ringPixels() { return this.limits.ring?.pixels; }
  // Light the ring pixel by pixel for `forMs` (30 s at most): one colour per pixel, from pixel 0 around the ring,
  // each '#rrggbb' or [r, g, b] from 0 to 255, exactly `ringPixels` of them, at `brightness` 0-100 (100 by default).
  ring(pixels, { forMs, brightness, fadeMs } = {}) {
    if (this.ringPixels !== undefined && pixels.length !== this.ringPixels) {
      return Promise.reject(new LampError(`pixels: ${this.ringPixels} of them, one per pixel of the ring`, { code: 'bad_request' }));
    }
    let hexed;
    try { hexed = pixels.map(hex); } catch (error) { return Promise.reject(error); }
    return this.act('light', { pixels: hexed, brightness, fade_ms: fadeMs, for_ms: forMs });
  }
  // The whole ring in one colour of your own, '#rrggbb' or [r, g, b], for `forMs`, at `brightness` 0-100.
  lightRgb(rgb, { forMs, brightness, fadeMs } = {}) {
    let colour;
    try { colour = hex(rgb); } catch (error) { return Promise.reject(error); }
    return this.act('light', { rgb: colour, brightness, fade_ms: fadeMs, for_ms: forMs });
  }
  // Say `text` in the lamp's voice, exactly as written, once the person isn't talking; `{ rephrase: true }` lets the
  // lamp's model say it in its own words instead, as before.
  say(text, { rephrase } = {}) { return this.act('say', { text, rephrase: rephrase ? true : undefined }); }
  // The speaker's volume, the person's own: one of `to` (0-100), `step` ('up' or 'down'), or `mute` (true or
  // false). Resolves to the level it left, `{ level, muted }`.
  async volume({ to, step, mute } = {}) {
    // Exactly one, as the lamp holds it (control.schema.json's `volume`).
    if ([to, step, mute].filter((v) => v !== undefined).length !== 1) {
      throw new LampError('volume: one of to, step or mute', { code: 'bad_request' });
    }
    const { level, muted } = await this.act('volume', { to, step, mute });
    return { level, muted };
  }

  // The camera (the `camera` grant): the eyes' own newest picture, never
  // while the lamp is private.

  /** The camera's newest picture, a JPEG about 320 across, as a Buffer. */
  async cameraSnapshot() {
    const reply = await this.request('camera.snapshot');
    return unb64(reply.jpeg);
  }

  /** Pictures as `camera.frame` events (`lamp.on('camera.frame', ...)`, each
   * with its `jpeg` in base64), `fps` a second at most (1 to 30; each new frame once as it comes, about 1 to 2 a second today: the answer and each frame carry `fps_now`), until
   * `cameraStream(false)`, `stop`, or a `camera.ended` event. */
  cameraStream(on = true, fps = undefined, size = undefined) {
    const fields = { on };
    if (fps !== undefined) fields.fps = fps;
    if (size !== undefined) fields.size = size;
    return this.request('camera.stream', fields);
  }
  // `hum`, the lamp's own, or one of this computer's clips by name (`uploadSound`), once the person isn't talking.
  sound(name) { return this.act('sound', { name }); }
  // Keep a clip on the lamp as `name`, over one of the same name, to play with `sound(name)`: a WAV of 16-bit mono PCM
  // at 24 kHz, at most 30 s, within the voice's loudness cap. `wav` is its bytes (a Buffer or Uint8Array) or a path.
  // Sent in parts, as the lamp takes them; the lamp checks it whole and refuses it with why. Resolves to the clip as
  // kept: name, bytes, ms.
  async uploadSound(name, wav) {
    const data = typeof wav === 'string' ? await readFile(wav) : Buffer.from(wav);
    const sha256 = createHash('sha256').update(data).digest('hex');
    // 36 KiB a part: 48 Ki characters of base64, a part's most.
    const step = 36 * 1024;
    const parts = Math.max(1, Math.ceil(data.length / step));
    let reply = {};
    for (let part = 0; part < parts; part += 1) {
      const piece = data.subarray(part * step, (part + 1) * step);
      reply = await this.request('sound.upload', {
        name, sha256, size: data.length, part, parts, data: piece.toString('base64'),
      });
    }
    return reply.sound ?? {};
  }
  // This computer's clips on the lamp: each its name, bytes and ms.
  async sounds() { return (await this.request('sound.list', {})).sounds ?? []; }
  // Delete one of this computer's clips from the lamp.
  removeSound(name) { return this.request('sound.remove', { name }); }
  // Turns and listening (docs/control.md): the person wins over each.
  // Opens a conversation turn, as a tap on the head does: the person's next
  // line is answered as spoken to the lamp; with `prompt`, the lamp says it
  // first, as a `say`. The reply is `{ to: 'voice' }`; with `wait`, its
  // `done`: `finished` once the answer has sounded, else `dropped` with the
  // voice's why. Needs speak and hear.
  startTurn(prompt, { wait = false } = {}) { return this.act('turn.start', { prompt }, wait); }
  // Ends the turn this computer started, never another's: `{ turn: 'ended',
  // started: <the turn.start's id> }`. With none of this computer's, `busy`.
  endTurn() { return this.act('turn.end', {}); }
  // `false` holds this computer's mute (`{ mute: 'held' }`) until
  // `setListening(true)` (`{ mute: 'lifted' }`, or `'nothing_held'`), this
  // computer goes, or a hand on the head lifts it: then a `done` `cut_short`,
  // "a hand on the head", names the request that held it
  // (`lamp.on('done', ...)`). A `stop` doesn't lift it. Needs hear.
  setListening(on) { return this.act('listen.set', { on }); }
  async stop() {
    const reply = await this.act('stop', {});
    const following = this.following;
    if (following) {
      // A stop ends what this client asked for; its lease goes too.
      following.end('cut_short', 'stop');
      await following.finish();
    }
    return reply;
  }

  // Skills of this computer's own, put on the lamp (the `skills` grant;
  // docs/control.md, "Skills pushed from a computer"). `pushSkill(folder)`
  // packs the folder and sends it in parts, and resolves with the lamp's
  // `skill`: what it asks, and whether it's on or held for the owner's yes.
  async pushSkill(folder) {
    if (!this.grants.includes('skills')) throw new byCode.no_grant('not granted: skills');
    const { pack } = await import('./push.js');
    const pkg = pack(folder);
    let reply = {};
    for (const [part, data] of pkg.parts.entries()) {
      reply = await this.request('skill.push', {
        name: pkg.name, sha256: pkg.sha256, size: pkg.data.length, part, parts: pkg.parts.length, data,
      });
    }
    return reply.skill ?? {};
  }

  async skills() { return (await this.request('skill.list')).skills ?? []; }

  removeSkill(name) { return this.request('skill.remove', { name }); }
}
