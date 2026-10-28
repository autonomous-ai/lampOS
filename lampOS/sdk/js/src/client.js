// The lamp, from a developer's computer: pair once, then connect, listen and act.
//
//   const lamp = await Lamp.connect();
//   lamp.on('person.arrived', () => lamp.gesture('greeting'));
//
// Every message is docs/control.schema.json's. Names the schema enumerates are
// checked before sending, and a refusal rejects with its typed error.
import { hostname } from 'node:os';
import WebSocket from 'ws';
import * as debug from './debug.js';
import * as schema from './schema.js';
import { LampError, NotPaired, WrongLamp, byCode, fromReply } from './errors.js';
import { Keyring, b64, isPin, pinOf, proof, unb64 } from './keys.js';

// The actions lamp-control answers `{"to":"body"}` with no `done` after
// (docs/control.md): a wait for them ends at that answer. `say` answers
// `{"to":"voice"}` and its `done` does follow.
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

function check(op, fields) {
  if (!schema.ops.includes(op)) throw new LampError(`no op ${op} in v${schema.VERSION}`, { code: 'bad_request' });
  const allowed = { name: op === 'gesture' ? schema.gestures : null, colour: schema.colours, pattern: schema.patterns };
  for (const [field, names] of Object.entries(allowed)) {
    const value = fields[field];
    if (names && value !== undefined && !names.includes(value)) {
      throw new LampError(`${field} ${value}: one of ${names.join(', ')}`, { code: 'bad_request' });
    }
  }
  for (const event of fields.events ?? []) {
    if (!schema.events.includes(event)) throw new LampError(`no event ${event}`, { code: 'bad_request' });
  }
  for (const part of fields.parts ?? []) {
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
    this.waiting = new Map();
    this.done = new Map();
    this.handlers = new Map();
    this.phrases = [];
    this.leases = new Map();
    ws.removeAllListeners('message');
    for (const frame of frames.splice(0)) this.frame(frame);
    ws.on('message', (data) => {
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
    if (!schema.events.includes(event) && event !== 'lease' && event !== 'done') {
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

  // `const lease = await lamp.lease('body').take()` ... `await lease.release()`,
  // or `await lamp.withLease(['body'], async (lease) => ...)`.
  lease(...parts) { return new Lease(this, parts, 30_000, undefined); }

  async withLease(parts, body, { forMs = 30_000, why } = {}) {
    const lease = await new Lease(this, parts, forMs, why).take();
    try { return await body(lease); } finally { await lease.release(); }
  }

  express(feeling, strength) { return this.act('express', { feeling, strength }); }
  gesture(name, { wait = false } = {}) { return this.act('gesture', { name }, wait); }
  look(at) { return this.act('look', { at }); }
  move(frames, stepMs, { wait = false } = {}) { return this.act('move', { step_ms: stepMs, frames }, wait); }
  play(name, { wait = false } = {}) { return this.act('play', { name }, wait); }
  joints(lease, withinMs, targets) { return this.act('joints', { lease: lease.id, targets, within_ms: withinMs }); }
  light(colour, level, forMs, pattern) { return this.act('light', { colour, level, pattern, for_ms: forMs }); }
  say(text) { return this.act('say', { text }); }
  sound(name) { return this.act('sound', { name }); }
  stop() { return this.act('stop', {}); }
}
