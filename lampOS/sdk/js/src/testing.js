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

// What lamp-control refuses today, in its own words (crates/control/src/
// parts.rs, `waits_for`): pass it as `refuses` for a fake that answers as a
// lamp does now. `lamp fake` does.
export const LAMP_CONTROL_REFUSES = Object.freeze({
  look: "not routed yet: waits for the body's lease socket",
  joints: "not routed yet: waits for the body's lease socket",
  play: 'not routed yet: waits for client packages',
  sound: 'not routed yet: waits for client packages',
});
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

export class FakeLamp {
  static async start(options = {}) {
    const fake = new FakeLamp(options);
    await fake.listen();
    return fake;
  }

  // `autoDone: false` leaves a play's or say's `done` to the test.
  // `refuses`: ops refused `refused` with this why, as a lamp that doesn't
  // route them yet does (LAMP_CONTROL_REFUSES); none by default.
  constructor({ code = '12345678', grants = ['body', 'light', 'speak'], lampId = 'lamp-fake', name = 'Fake lamp', autoDone = true, refuses = {} } = {}) {
    Object.assign(this, { code, grants: [...grants], host: '127.0.0.1', port: 0, pin: '', fp: '', autoDone, refuses: { ...refuses } });
    this.asker = new Map();
    this.lamp = { id: lampId, name, v: schema.VERSION };
    this.clients = new Map();
    this.requests = [];
    this.received = [];
    // WebSocket pings from clients (their keepalive).
    this.pings = 0;
    this.leases = new Map();
    this.subscribed = new Map();
    this.state = { presence: { count: 0, near: 0, facing: false }, light: {}, mood: {}, lamp: { talking: false, paused: false, private: false, asleep: false } };
    this.refusals = new Map();
    this.nextLease = 1;
    // The newest light a computer asked for, and until when (the fake's
    // `clock`, ms): what the ring shows while it lasts (`lightState`).
    this.shown = null;
    this.clock = () => Date.now();
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
  lightState() {
    const own = { ...(this.state.light ?? {}) };
    if (this.shown && this.shown.until > this.clock()) own.shows = { ...this.shown.light };
    else if (['on', 'colour', 'level'].some((key) => key in own)) {
      own.shows = Object.fromEntries(['on', 'colour', 'level'].filter((key) => key in own).map((key) => [key, own[key]]));
    }
    return own;
  }

  // Tell a lease's holder it is `held`, `suspended` or `lost` (and, lost, end it here).
  leaseEvent(lease, state, why) {
    const held = this.leases.get(lease);
    if (!held) return;
    if (state === 'lost') this.leases.delete(lease);
    held.ws.send(JSON.stringify({ ev: 'lease', lease, state, ...(why ? { why } : {}) }));
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
      for (const [lease, held] of this.leases) if (held.ws === ws) this.leases.delete(lease);
    });
    ws.on('message', (data) => {
      let request;
      try { request = JSON.parse(data.toString()); } catch { return this.refuse(ws, null, 'bad_request', 'not JSON'); }
      this.received.push(request);
      const id = Number.isInteger(request.id) && request.id >= 0 ? request.id : null;
      if (id === null || !schema.ops.includes(request.op)) return this.refuse(ws, id, 'bad_request', 'not a v1 request');
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
  };

  needs(op, request) {
    if (op === 'subscribe') return [...new Set((request.events ?? []).map((e) => FakeLamp.EVENT_GRANT[e]).filter(Boolean))];
    if (op === 'get') return (request.state ?? []).includes('presence') ? ['look'] : [];
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
      case 'light':
        this.show(request);
        return this.ok(ws, id);
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
