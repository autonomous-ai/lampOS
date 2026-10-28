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
import { WebSocketServer } from 'ws';
import * as schema from './schema.js';
import { HELLO, b64, pinOf, proof, unb64 } from './keys.js';
import { asksOf, openPackage } from './push.js';
import { parse as parseToml } from 'smol-toml';

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
    // Pushed skills by name, and the one push in flight on the lamp.
    this.pushed = new Map();
    this.pushing = null;
    this.subscribed = new Map();
    this.state = { presence: { count: 0 }, light: {}, mood: {}, lamp_state: { talking: false, paused: false, private: false, asleep: false } };
    this.refusals = new Map();
    this.nextLease = 1;
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

  // A part of a push, as lamp-control and the helper take it: in order from
  // 0, one push at a time, the whole checked against its SHA-256 and its
  // manifest, then kept as this computer's.
  push(ws, client, id, request) {
    let going = this.pushing;
    if (request.part === 0) {
      if (going && going.client !== client) return this.refuse(ws, id, 'busy', "another computer's push is in flight");
      going = this.pushing = { client, part: -1, chunks: [], asks: request.asks };
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
    // The package and its signature, as the lamp reads them first.
    let opened;
    try {
      opened = openPackage(data);
    } catch (error) {
      return this.refuse(ws, id, 'bad_request', error.why ?? error.message);
    }
    if (opened.name !== request.name) return this.refuse(ws, id, 'refused', 'not a package of that skill');
    if (opened.author && !going.asks) return this.refuse(ws, id, 'bad_request', 'a signed package is pushed with asks on part 0');
    const kept = this.pushed.get(request.name);
    if (kept && kept.client !== client) return this.refuse(ws, id, 'refused', "another computer's skill has that name");
    let asks = [];
    try {
      asks = asksOf(parseToml((opened.files.get('skill.toml') ?? Buffer.alloc(0)).toString('utf8')));
    } catch {
      // A fake: the lamp's own load check says what's wrong with skill.toml.
    }
    // `author`, the signer's fingerprint, only when it is signed (Ten's shape on #3931).
    const skill = { name: opened.name, version: opened.version, sha256: sha, on: true, held: null, ...(opened.author ? { author: opened.author } : {}) };
    this.pushed.set(request.name, { ...skill, client, pushed_at: now() });
    return this.ok(ws, id, { skill: { ...skill, asks, shown: [] } });
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
    this.ok(ws, request.id, { lamp: this.lamp, grants: paired.grants, limits: { joints_hz: 20 } });
    return request.client;
  }

  // The grant each event and state needs, as lamp-control's own (#3736).
  static EVENT_GRANT = {
    'person.arrived': 'look', 'person.left': 'look', 'person.stood_up': 'look', 'person.facing': 'look',
    'phrase.heard': 'hear', 'name.heard': 'hear', 'sound.heard': 'sound',
  };

  needs(op, request) {
    if (op === 'subscribe') return [...new Set((request.events ?? []).map((e) => FakeLamp.EVENT_GRANT[e]).filter(Boolean))];
    if (op === 'get') return (request.state ?? []).includes('presence') ? ['look'] : [];
    if (op === 'look') return request.at === 'face' ? ['body', 'look'] : ['body'];
    if (['gesture', 'move', 'play', 'joints'].includes(op)) return ['body'];
    if (op === 'lease.take') return (request.parts ?? []).map((p) => ({ body: 'body', light: 'light', voice: 'speak' })[p]);
    if (op.startsWith('skill.')) return ['skills'];
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
        const asked = new Set(request.state.map((s) => (s === 'lamp' ? 'lamp_state' : s)));
        return this.ok(ws, id, Object.fromEntries(Object.entries(this.state).filter(([k]) => asked.has(k))));
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
      case 'skill.push':
        return this.push(ws, client, id, request);
      case 'skill.list':
        return this.ok(ws, id, { skills: [...this.pushed.values()].filter((s) => s.client === client) });
      case 'skill.remove': {
        const kept = this.pushed.get(request.name);
        if (!kept || kept.client !== client) return this.refuse(ws, id, 'refused', "not one of this computer's pushed skills");
        this.pushed.delete(request.name);
        return this.ok(ws, id);
      }
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
