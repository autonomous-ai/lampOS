// The SDK against the fake lamp: pairing, hello, events, actions, leases and
// refusals, as docs/control.md says them. No lamp needed.
import assert from 'node:assert/strict';
import { execFile, execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, LampError, NotPaired, WrongLamp, byCode, fromReply, schema } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-test-'));
const started = (t, options) => FakeLamp.start(options).then((fake) => { t.after(() => fake.stop()); return fake; });
const until = async (ready) => { while (!ready()) await new Promise((r) => setTimeout(r, 10)); };

async function withLamp(options, body) {
  const fake = await FakeLamp.start(options);
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try { await body(lamp, fake, keyring); } finally { await lamp.close(); }
  } finally { await fake.stop(); }
}

test('the packaged schema is the repository\'s', (t) => {
  // Outside the repository (an installed copy) there is nothing to compare;
  // the lampOS SDK job in CI runs it in the repository.
  const repoSchema = new URL('../../../docs/control.schema.json', import.meta.url);
  if (!existsSync(repoSchema)) return t.skip('docs/control.schema.json is not beside this copy');
  assert.deepEqual(JSON.parse(readFileSync(repoSchema, 'utf8')), schema.schema);
});

test('proofs name what they are for', async () => {
  const { HELLO, PAIR, proof } = await import('../src/keys.js');
  const { createHmac } = await import('node:crypto');
  assert.equal(PAIR.toString(), 'lampos/v1/pair\0');
  assert.equal(HELLO.toString(), 'lampos/v1/hello\0');
  const key = Buffer.alloc(32);
  const nonce = Buffer.from([...Array(32).keys()]);
  const bare = createHmac('sha256', '12345678').update(Buffer.concat([key, nonce])).digest();
  assert.ok(!proof('12345678', Buffer.alloc(32), key, nonce).equals(bare));
  // The pin is inside, so a proof relayed through a host in the middle fails.
  const pin = Buffer.alloc(32, 1);
  const full = createHmac('sha256', '12345678').update(Buffer.concat([PAIR, pin, key, nonce])).digest();
  assert.ok(proof('12345678', pin, key, nonce).equals(full));
  assert.ok(!proof('12345678', Buffer.alloc(32, 2), key, nonce).equals(full));
});

test('every op has a call and every error code a class', () => {
  const plumbing = new Set(['pair', 'hello', 'subscribe', 'unsubscribe', 'get', 'lease.take', 'lease.renew', 'lease.release']);
  const named = { 'skill.push': 'pushSkill', 'skill.list': 'skills', 'skill.remove': 'removeSkill' };
  for (const op of schema.ops) assert.ok(plumbing.has(op) || typeof Lamp.prototype[named[op] ?? op] === 'function', op);
  for (const code of schema.errorCodes) assert.equal(new byCode[code]('x').code, code);
  assert.equal(fromReply({ code: 'rate', why: 'x', retry_ms: 900 }).retryMs, 900);
  assert.ok(fromReply({ code: 'no_grant', why: 'x' }) instanceof byCode.no_grant);
});

test('pairing keeps the key private and the lamp pinned', async (t) => {
  const fake = await started(t, { grants: ['body', 'light'] });
  const keyring = new Keyring(folder());
  const pairing = await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
  assert.deepEqual([pairing.lamp, pairing.fp, pairing.grants], ['lamp-fake', fake.pin, ['body', 'light']]);
  assert.equal(fake.clients.get(pairing.client).name, 'test laptop');
  assert.equal(statSync(join(keyring.folder, 'key')).mode & 0o777, 0o600);
  assert.ok(!readFileSync(join(keyring.folder, 'lamps.json'), 'utf8').includes(fake.code));
});

test('a wrong code is refused and nothing is kept', async (t) => {
  const fake = await started(t);
  const keyring = new Keyring(folder());
  await assert.rejects(Lamp.pair('87654321', fake.host, { pin: fake.pin, port: fake.port, keyring }), byCode.unpaired);
  await assert.rejects(Lamp.pair('1234', fake.host, { pin: fake.pin, port: fake.port, keyring }), LampError);
  assert.deepEqual(keyring.pairings(), {});
});

test('another certificate than the pinned one is refused', async (t) => {
  const fake = await started(t);
  const keyring = new Keyring(folder());
  // Another certificate than the pin: refused before any frame is sent.
  await assert.rejects(Lamp.pair(fake.code, fake.host, { pin: '0'.repeat(64), port: fake.port, keyring }), WrongLamp);
  // No pin, or mDNS's short hint: no pairing, and no connection.
  for (const pin of [undefined, '', fake.fp]) {
    await assert.rejects(Lamp.pair(fake.code, fake.host, { pin, port: fake.port, keyring }), WrongLamp);
  }
  assert.deepEqual(fake.received, []);
  await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
  const other = await started(t);
  keyring.remember({ ...keyring.pairing(), port: other.port });
  await assert.rejects(Lamp.connect(undefined, { keyring }), WrongLamp);
});

test('connect without a pairing says so', async () => {
  await assert.rejects(Lamp.connect(undefined, { keyring: new Keyring(folder()) }), NotPaired);
});

test('hello with another computer\'s key is unpaired', async (t) => {
  const fake = await started(t);
  const mine = new Keyring(folder());
  await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: mine });
  const stranger = new Keyring(folder());
  stranger.remember(mine.pairing());
  await assert.rejects(Lamp.connect(undefined, { keyring: stranger }), byCode.unpaired);
});

test('greet a person who arrives', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    let greeted;
    const done = new Promise((resolve) => { greeted = resolve; });
    await lamp.on('person.arrived', async () => { await lamp.gesture('greeting'); greeted(); });
    fake.emit('person.arrived', { count: 1, near: 0.3 });
    await done;
    assert.deepEqual(fake.asked('gesture'), [{ name: 'greeting' }]);
  });
});

test('phrases go with their subscription, and events carry only the phrase', async () => {
  await withLamp({ grants: ['body', 'light', 'speak', 'hear'] }, async (lamp, fake) => {
    const heard = [];
    await lamp.on('phrase.heard', (event) => heard.push(event.phrase), { phrases: ['good night lamp'] });
    assert.deepEqual(fake.asked('subscribe'), [{ events: ['phrase.heard'], phrases: ['good night lamp'] }]);
    fake.emit('phrase.heard', { phrase: 'good night lamp' });
    await until(() => heard.length);
    assert.deepEqual(heard, ['good night lamp']);
  });
});

// The lamp's floor and its words (lamp-control's PHRASE_MIN_LETTERS, #3901),
// with the phrases Fourteen tried there: a short one never leaves the computer.
test('a phrase under the lamp\'s floor is refused before sending', async () => {
  const short = ['is a', 'it is', 'ok lamp', 'you are', 'i want to', 't\u00f4i mu\u1ed1n'];
  // The Hindi lines have vowel signs, which the lamp counts as letters: the
  // SDK must never be stricter than the lamp (Nine's and harness-reviewer's P2).
  const long = ['thank you', 'what is the', "i don't know", 't\u00f4i kh\u00f4ng', '12345678', 'नमस्ते दोस्त', 'सुनो लैंप'];
  await withLamp({ grants: ['body', 'light', 'speak', 'hear'] }, async (lamp, fake) => {
    for (const phrase of short) {
      const refused = (error) => error.code === 'bad_request' && error.why === `phrases: "${phrase}" is too short: 8 letters at least`;
      assert.throws(() => lamp.on('phrase.heard', () => {}, { phrases: [phrase, 'good night lamp'] }), refused);
      await assert.rejects(lamp.subscribe(['phrase.heard'], [phrase]), refused);
    }
    assert.deepEqual(fake.asked('subscribe'), []);
    await lamp.subscribe(['phrase.heard'], long);
    assert.deepEqual(fake.asked('subscribe'), [{ events: ['phrase.heard'], phrases: long }]);
    // The refused on() kept no handler and no phrase.
    assert.deepEqual(lamp.phrases, []);
    assert.equal(lamp.handlers.get('phrase.heard'), undefined);
  });
});

// blue's P2 on #4020: the skill's three feelings, each its move, and any
// other name bad_request.
test('the fake answers express as lamp-control does', async () => {
  await withLamp({}, async (lamp) => {
    const { to, move, light } = await lamp.express('glad');
    assert.deepEqual({ to, move, light }, { to: 'body', move: 'glad-lift', light: true });
    assert.equal((await lamp.express('glad', 0.25)).move, 'glad-lift-50');
    assert.equal((await lamp.express('low', 0.25)).move, 'low-droop-75');
    assert.equal((await lamp.express('glad', 0.0625)).move, 'glad-lift-25');
    await assert.rejects(lamp.express('happy'), (error) => error.code === 'bad_request' && error.why === 'feeling: one of glad, lively, low');
  });
});

test('actions send the schema\'s fields and wait for done', async () => {
  await withLamp({}, async (lamp, fake) => {
    await lamp.light('warm', 3, 10_000, 'breathe');
    await lamp.say('Build passed.');
    await lamp.express('glad');
    assert.equal((await lamp.gesture('nod', { wait: true })).to, 'body');
    await lamp.stop();
    assert.deepEqual(fake.asked('light'), [{ colour: 'warm', level: 3, pattern: 'breathe', for_ms: 10000 }]);
    assert.deepEqual(fake.asked('say'), [{ text: 'Build passed.' }]);
    assert.deepEqual(fake.asked('express'), [{ feeling: 'glad' }]);
    assert.deepEqual(fake.asked('stop'), [{}]);
  });
});

test('names outside the schema never leave this computer', async () => {
  await withLamp({}, async (lamp, fake) => {
    for (const call of [lamp.gesture('scanning'), lamp.gesture('idle'), lamp.light('magenta', 3, 1000)]) {
      await assert.rejects(call, (error) => error.code === 'bad_request');
    }
    assert.throws(() => lamp.on('camera.frame', () => {}), LampError);
    await lamp.stop();
    assert.deepEqual(fake.received.filter((f) => ['gesture', 'light'].includes(f.op)), []);
  });
});

test('a move needs a body lease, which renews itself and is released', async () => {
  await withLamp({}, async (lamp, fake) => {
    await assert.rejects(lamp.look({ yaw: -10, pitch: 5 }), byCode.no_lease);
    await lamp.withLease(['body'], async (lease) => {
      assert.ok(lease.held);
      await lamp.look({ yaw: -10, pitch: 5 });
      await lamp.joints(lease, 300, { pitch: 4, yaw: -12 });
      await new Promise((r) => setTimeout(r, 1200));
      assert.ok(fake.asked('lease.renew').length >= 2);
    }, { forMs: 1000, why: 'test' });
    assert.equal(fake.leases.size, 0);
    assert.deepEqual(fake.asked('look'), [{ at: { yaw: -10, pitch: 5 } }]);
    assert.deepEqual(fake.asked('joints'), [{ lease: 'l_1', targets: { pitch: 4, yaw: -12 }, within_ms: 300 }]);
    assert.deepEqual(fake.asked('lease.release'), [{ lease: 'l_1' }]);
  });
});

test('a lease the lamp takes back is lost with its why', async () => {
  await withLamp({}, async (lamp, fake) => {
    await lamp.withLease(['body'], async (lease) => {
      fake.loseLease(lease.id, 'hand');
      await until(() => lease.state === 'lost');
      assert.equal(lease.lostWhy, 'hand');
      await assert.rejects(lamp.move([[0, 0, 0, 0, 0], [20, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 40), byCode.no_lease);
    }, { forMs: 1000 });
    assert.deepEqual(fake.asked('lease.release'), []);
  });
});

test('refusals are typed and say when to try again', async () => {
  await withLamp({ grants: ['body'] }, async (lamp, fake) => {
    await assert.rejects(lamp.say('hello'), byCode.no_grant);
    fake.refuseNext('gesture', 'rate', '1 every 2 s', 1500);
    await assert.rejects(lamp.gesture('nod'), (error) => error instanceof byCode.rate && error.retryMs === 1500);
    fake.refuseNext('gesture', 'private', 'the privacy switch is closed');
    await assert.rejects(lamp.gesture('nod'), byCode.private);
  });
});

test('get answers the states asked', async () => {
  await withLamp({ grants: ['look'] }, async (lamp, fake) => {
    fake.state.presence = { count: 2, near: 0.4, facing: true };
    const state = await lamp.get('presence', 'lamp');
    assert.equal(state.presence.count, 2);
    assert.equal(state.lamp_state.private, false);
  });
});

test('the examples call only what the client has', async () => {
  const { readdirSync } = await import('node:fs');
  const examples = readdirSync(new URL('../examples/', import.meta.url)).filter((f) => f.endsWith('.js'));
  assert.ok(examples.length >= 5);
  for (const example of examples) {
    const text = readFileSync(new URL(`../examples/${example}`, import.meta.url), 'utf8');
    for (const [, name] of text.matchAll(/\blamp\.([a-zA-Z]+)\(/g)) {
      assert.equal(typeof Lamp.prototype[name], 'function', `${example}: lamp.${name}`);
    }
  }
});

test('a refused subscription rejects with its typed error', async () => {
  await withLamp({ grants: ['body'] }, async (lamp) => {
    await assert.rejects(lamp.on('person.arrived', () => {}), byCode.no_grant);
    assert.equal(lamp.handlers.has('person.arrived'), false, 'a refused on() keeps no handler');
  });
});

test('off takes a handler off and unsubscribes the last', async () => {
  await withLamp({}, async (lamp, fake) => {
    const seen = [];
    const first = () => seen.push(1);
    const second = () => seen.push(2);
    await lamp.on('touch', first);
    await lamp.on('touch', second);
    await lamp.off('touch', first);
    fake.emit('touch', { kind: 'pet' });
    await until(() => seen.length);
    assert.deepEqual(seen, [2]);
    assert.deepEqual(fake.asked('unsubscribe'), []);
    await lamp.off('touch', second);
    assert.deepEqual(fake.asked('unsubscribe'), [{ events: ['touch'] }]);
  });
});

test('the fake says done and lease changes to whoever they are for', async () => {
  await withLamp({ autoDone: false }, async (lamp, fake) => {
    await lamp.withLease(['body'], async (lease) => {
      const waiting = lamp.play('wave', { wait: true });
      await until(() => fake.asked('play').length);
      fake.leaseEvent(lease.id, 'suspended', 'conversation');
      await until(() => lease.state === 'suspended');
      fake.done(fake.requests.at(-1).id, 'cut_short', 'conversation');
      assert.equal((await waiting).how, 'cut_short');
      fake.leaseEvent(lease.id, 'held');
      await until(() => lease.held);
    }, { forMs: 5000 });
  });
});

test('base64 is strict and a nonce must be 32 bytes', async () => {
  const { unb64 } = await import('../src/keys.js');
  assert.throws(() => unb64('AAAA', 32));
  assert.throws(() => unb64(`${'A'.repeat(43)}!`, 32));
  assert.deepEqual(unb64(`${'A'.repeat(43)}=`, 32), Buffer.alloc(32));
});

test('the pairing store is this user\'s alone whatever the umask', () => {
  const old = process.umask(0o002);
  const keyring = new Keyring(join(folder(), 'lampos'));
  try {
    keyring.public();
    keyring.remember({ lamp: 'l', client: 'c_1', grants: [], fp: '0'.repeat(64), host: 'h', port: 1 });
    // A folder made group-writable before is closed again.
    chmodSync(keyring.folder, 0o775);
    keyring.remember({ lamp: 'l', client: 'c_1', grants: [], fp: '1'.repeat(64), host: 'h', port: 1 });
  } finally {
    process.umask(old);
  }
  assert.equal(statSync(keyring.folder).mode & 0o777, 0o700);
  assert.equal(statSync(join(keyring.folder, 'key')).mode & 0o777, 0o600);
  assert.equal(statSync(join(keyring.folder, 'lamps.json')).mode & 0o777, 0o600);
});

test('lamp fake pairs here and says an arrival', async () => {
  const home = folder();
  const before = process.env.LAMPOS_HOME;
  process.env.LAMPOS_HOME = home;
  try {
    const { fake } = await import('../src/fake.js');
    const faking = fake({ every: 1.5, cycles: 2 });
    await until(() => existsSync(join(home, 'lamps.json')));
    const lamp = await Lamp.connect(undefined, { keyring: new Keyring(home) });
    assert.deepEqual([...lamp.grants].sort(), [...schema.grants].sort());
    let arrived;
    const arrival = new Promise((resolve) => { arrived = resolve; });
    await lamp.on('person.arrived', () => arrived());
    await arrival;
    await lamp.close();
    assert.equal(await faking, 0);
  } finally {
    if (before === undefined) delete process.env.LAMPOS_HOME; else process.env.LAMPOS_HOME = before;
  }
});

test('a connection that only listens pings, so the lamp keeps it open', async () => {
  const every = Lamp.KEEPALIVE_MS;
  Lamp.KEEPALIVE_MS = 50;
  try {
    await withLamp({}, async (lamp, fake) => {
      await lamp.on('touch', () => {});
      await until(() => fake.pings >= 2);
    });
  } finally {
    Lamp.KEEPALIVE_MS = every;
  }
});

test('a lamp that stops answering pings is let go, not waited on forever', async () => {
  const every = Lamp.KEEPALIVE_MS;
  Lamp.KEEPALIVE_MS = 50;
  try {
    await withLamp({}, async (lamp, fake) => {
      // The lamp goes quiet without a close, as off its Wi-Fi: no pongs.
      for (const ws of fake.wss.clients) ws._socket.pause();
      await Promise.race([lamp.closed(), new Promise((_, no) => setTimeout(() => no(new Error('never let go')), 2000))]);
    });
  } finally {
    Lamp.KEEPALIVE_MS = every;
  }
});

test('a refused waiting action leaves nothing behind to reject on close', async () => {
  const unhandled = [];
  const onUnhandled = (error) => unhandled.push(error);
  process.on('unhandledRejection', onUnhandled);
  try {
    await withLamp({ grants: ['light'] }, async (lamp) => {
      await assert.rejects(lamp.gesture('nod', { wait: true }), byCode.no_grant);
      assert.equal(lamp.done.size, 0);
    });
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.deepEqual(unhandled, []);
  } finally { process.off('unhandledRejection', onUnhandled); }
});

test('a waiting gesture or move returns at the body\'s answer', async () => {
  // lamp-control's reply shape: `{"re":N,"ok":true,"to":"body"}` and no
  // `done` after it; autoDone off, so nothing else ever comes.
  await withLamp({ autoDone: false }, async (lamp) => {
    await lamp.withLease(['body'], async () => {
      const nod = await lamp.gesture('nod', { wait: true });
      const move = await lamp.move([[0, 0, 0, 0, 0], [20, 0, 0, 0, 0]], 40, { wait: true });
      assert.deepEqual([nod.ok, nod.to, move.ok, move.to], [true, 'body', true, 'body']);
    }, { forMs: 5000 });
    assert.equal(lamp.done.size, 0);
  });
});

test('an action refused here takes no id, so the next waiting one keeps its own', async () => {
  await withLamp({ autoDone: false, grants: ['body', 'light', 'speak'] }, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      const bogus = lamp.gesture('bogus', { wait: true });
      const wave = lamp.play('wave', { wait: true });
      await assert.rejects(bogus, (error) => error.code === 'bad_request');
      await until(() => fake.asked('play').length);
      fake.done(fake.requests.at(-1).id);
      assert.equal((await wave).how, 'finished');
    }, { forMs: 5000 });
    assert.equal(lamp.done.size, 0);
  });
});

test('a refused on() takes off only its own copy of the handler', async () => {
  // Granted touch, refused person.arrived: the second on() of the same
  // function is the one taken back.
  await withLamp({ grants: ['body'] }, async (lamp) => {
    const handler = () => {};
    await lamp.on('touch', handler);
    lamp.handlers.set('person.arrived', [handler]);
    await assert.rejects(lamp.on('person.arrived', handler), byCode.no_grant);
    assert.deepEqual(lamp.handlers.get('person.arrived'), [handler]);
    assert.deepEqual(lamp.handlers.get('touch'), [handler]);
  });
});

test('only gesture and move end a wait at their reply, as control.md says', async () => {
  const { ENDS_AT_REPLY } = await import('../src/client.js');
  assert.deepEqual([...ENDS_AT_REPLY].sort(), ['gesture', 'move']);
});

test('a request after the lamp has gone is refused at once, never left waiting', async () => {
  await withLamp({}, async (lamp) => {
    lamp.ws.terminate();
    await lamp.closed();
    await assert.rejects(lamp.get('light'), (error) => error.code === 'closed');
    assert.equal(lamp.waiting.size, 0);
  });
});

test('a waiting gesture keeps no done', async () => {
  await withLamp({ autoDone: false }, async (lamp) => {
    const pending = lamp.gesture('nod', { wait: true });
    assert.equal(lamp.done.size, 0, 'a done kept for an action that gets none');
    await pending;
  });
});

test('a waiting action whose socket drops before the reply leaves no rejection unheard', async () => {
  const unhandled = [];
  const onUnhandled = (error) => unhandled.push(String(error));
  process.on('unhandledRejection', onUnhandled);
  try {
    await withLamp({ autoDone: false }, async (lamp) => {
      // Sent and cut in the same turn, so no reply can come first.
      const waiting = lamp.play('wave', { wait: true });
      lamp.ws.terminate();
      await assert.rejects(waiting, (error) => error.code === 'closed');
    });
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.deepEqual(unhandled, []);
  } finally { process.off('unhandledRejection', onUnhandled); }
});

test('a fake like lamp-control refuses what it does not route yet', async () => {
  const { LAMP_CONTROL_REFUSES } = await import('../src/testing.js');
  await withLamp({ grants: schema.grants, refuses: LAMP_CONTROL_REFUSES }, async (lamp) => {
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.play('wave'), (error) => error.code === 'refused' && error.why === 'not routed yet: waits for client packages');
    });
    await lamp.express('glad');
    assert.equal((await lamp.gesture('nod')).to, 'body');
    await lamp.light('warm', 3, 1000);
  });
});

// As lamp-control's session.rs: no lease is no_lease, and only a held lease
// reaches the "not routed yet" refusal (Reviewer-two's P3 on #3903).
test('the fake checks the lease before the routing refusal', async () => {
  const { LAMP_CONTROL_REFUSES } = await import('../src/testing.js');
  await withLamp({ grants: schema.grants, refuses: LAMP_CONTROL_REFUSES }, async (lamp) => {
    await assert.rejects(lamp.look('face'), (error) => error.code === 'no_lease');
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.look('face'), (error) => error.code === 'refused' && error.why.startsWith('not routed yet'));
    });
  });
});

function skillFolder(name, rules = 'version = "0.1.0"\n[permissions]\nlight = true\n') {
  const dir = join(folder(), name);
  mkdirSync(join(dir, 'moves'), { recursive: true });
  writeFileSync(join(dir, 'SKILL.md'), `---\nname: ${name}\ndescription: A test skill.\n---\n`);
  writeFileSync(join(dir, 'skill.toml'), rules);
  writeFileSync(join(dir, 'moves', 'wave.json'), '{}');
  writeFileSync(join(dir, '.DS_Store'), '\0mac');
  return dir;
}

test('a skill is packed as lamp-skills packs it, and tar reads it', async () => {
  const { pack } = await import('../src/push.js');
  const dir = skillFolder('desk-wave');
  const pkg = pack(dir);
  assert.deepEqual(pack(dir).data, pkg.data, 'the same folder packs to the same bytes');
  const file = join(folder(), 'p.tar');
  writeFileSync(file, pkg.data);
  const listed = execFileSync('tar', ['-tvf', file], { encoding: 'utf8' }).trim().split('\n');
  assert.deepEqual(listed.map((line) => line.split(/\s+/).at(-1)), ['lamp-skill.json', 'desk-wave/SKILL.md', 'desk-wave/moves/wave.json', 'desk-wave/skill.toml']);
  assert.ok(listed.every((line) => line.startsWith('-rw-r--r--')), listed.join('\n'));
  const manifest = JSON.parse(execFileSync('tar', ['-xOf', file, 'lamp-skill.json'], { encoding: 'utf8' }));
  assert.deepEqual([manifest.format, manifest.name, manifest.version], [1, 'desk-wave', '0.1.0']);
  assert.deepEqual(manifest.files.map((f) => f.path), ['SKILL.md', 'moves/wave.json', 'skill.toml']);
});

// Reviewer-lead's P3 on #4029: a literal string is TOML too, and Python packs it.
test('a version in a literal string packs as in a basic one', async () => {
  const { pack } = await import('../src/push.js');
  const pkg = pack(skillFolder('desk-lit', "version = '0.2.0'\n[permissions]\nlight = true\n"));
  assert.equal(pkg.version, '0.2.0');
});

test('a skill is pushed in parts, listed and removed', async () => {
  const push = await import('../src/push.js');
  await withLamp({ grants: ['light', 'skills'] }, async (lamp, fake) => {
    const skill = await lamp.pushSkill(skillFolder('desk-wave'));
    assert.deepEqual([skill.name, skill.version, skill.on], ['desk-wave', '0.1.0', true]);
    assert.equal(fake.asked('skill.push').length, push.pack(skillFolder('desk-wave')).parts.length);
    assert.deepEqual((await lamp.skills()).map((s) => s.name), ['desk-wave']);
    await lamp.removeSkill('desk-wave');
    assert.deepEqual(await lamp.skills(), []);
  });
});

test('lamp skill push, list and remove run as the Python CLI\'s do', async () => {
  const fake = await FakeLamp.start({ grants: ['light', 'skills'] });
  const home = folder();
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(home) });
    // Not execFileSync: the fake answers on this process's event loop.
    const lamp = (...args) => new Promise((done) => {
      execFile(process.execPath, [new URL('../src/cli.js', import.meta.url).pathname, ...args], { env: { ...process.env, LAMPOS_HOME: home } },
        (error, stdout, stderr) => done({ code: error?.code ?? 0, out: stdout.trim(), err: stderr.trim() }));
    });
    assert.deepEqual(await lamp('skill', 'push', skillFolder('desk-wave')), { code: 0, out: 'desk-wave 0.1.0 pushed, on', err: '' });
    assert.equal((await lamp('skill', 'list')).out, 'desk-wave\t0.1.0\ton');
    assert.equal((await lamp('skill', 'remove', 'desk-wave')).out, 'desk-wave removed');
    assert.equal((await lamp('skill', 'list')).out, '');
    const core = await lamp('skill', 'push', skillFolder('core', 'version = "0.1.0"\nkind = "core"\n'));
    assert.deepEqual([core.code, core.err], [1, 'lamp: refused: kind = "core": a pushed skill is installable']);
    assert.deepEqual(fake.asked('skill.remove'), [{ name: 'desk-wave' }]);
  } finally { await fake.stop(); }
});

test('a signed package names its author, and any change is refused', async () => {
  const { pack, openPackage, SIGNATURE } = await import('../src/push.js');
  const dir = skillFolder('desk-wave');
  assert.equal(openPackage(pack(dir).data).author, null);
  const ring = new Keyring(folder());
  assert.equal(ring.authorKey(), undefined);
  const key = ring.authorKey({ make: true });
  assert.equal(statSync(join(ring.folder, 'author.pem')).mode & 0o777, 0o600);
  const { data } = pack(dir, { author: key });
  const file = join(folder(), 'signed.tar');
  writeFileSync(file, data);
  // The signature is the second entry, as lamp-skills writes it.
  assert.deepEqual(execFileSync('tar', ['-tf', file], { encoding: 'utf8' }).trim().split('\n').slice(0, 2), ['lamp-skill.json', SIGNATURE]);
  const signed = JSON.parse(execFileSync('tar', ['-xOf', file, SIGNATURE], { encoding: 'utf8' }));
  const opened = openPackage(data);
  assert.equal(opened.author, signed.key);
  assert.equal(signed.key.length, 16);
  assert.deepEqual([...opened.files.keys()].sort(), ['SKILL.md', 'moves/wave.json', 'skill.toml']);
  const changed = Buffer.from(data.toString('latin1').replace('light = true', 'light = TRUE'), 'latin1');
  assert.throws(() => openPackage(changed), (error) => /does not match lamp-skill.json/.test(error.why));
  const swapped = Buffer.from(data.toString('latin1').replace(signed.key, '0'.repeat(16)), 'latin1');
  assert.throws(() => openPackage(swapped), (error) => /not the fingerprint of public/.test(error.why));
});

test('lamp skill keygen, pack --sign and check run as the Python CLI\'s do', async () => {
  const home = folder();
  const out = folder();
  const lamp = (...args) => new Promise((done) => {
    execFile(process.execPath, [new URL('../src/cli.js', import.meta.url).pathname, ...args], { env: { ...process.env, LAMPOS_HOME: home } },
      (error, stdout, stderr) => done({ code: error?.code ?? 0, said: `${stdout}${stderr}`.trim() }));
  });
  const dir = skillFolder('desk-wave');
  assert.deepEqual(await lamp('skill', 'pack', dir, '--sign', '--out', out), { code: 1, said: 'lamp: bad_request: no author key: make one with `lamp skill keygen`' });
  const made = await lamp('skill', 'keygen');
  const fpr = made.said.split(' ')[2];
  assert.ok(made.code === 0 && made.said.endsWith('author.pem') && fpr.length === 16, made.said);
  assert.ok((await lamp('skill', 'keygen')).said.startsWith(`author key ${fpr} already there`));
  assert.ok((await lamp('skill', 'pack', dir, '--sign', '--out', out)).said.endsWith(' signed'));
  assert.deepEqual(await lamp('skill', 'check', join(out, 'desk-wave-0.1.0.tar')), { code: 0, said: `desk-wave: package ok, signed by key ${fpr}, not the Store` });
});

test('a shared package is fetched over https only, and reading stops at 1 MiB', async () => {
  // Fourteen's point 4 on #3504: https only, no redirect to another scheme,
  // and reading stops at the cap, not after the whole download.
  const { PACKAGE_MAX, fetchPackage, readCapped } = await import('../src/push.js');
  for (const url of ['http://example.com/a.tar', 'file:///etc/passwd', 'ftp://example.com/a.tar']) {
    await assert.rejects(fetchPackage(url), (error) => /https only/.test(error.why));
  }
  const stream = (total, chunk = 64 * 1024) => {
    let sent = 0;
    return new ReadableStream({
      pull(controller) {
        if (sent >= total) return controller.close();
        const size = Math.min(chunk, total - sent);
        sent += size;
        controller.enqueue(new Uint8Array(size));
      },
    });
  };
  assert.equal((await readCapped(stream(PACKAGE_MAX))).length, PACKAGE_MAX);
  let pulled = 0;
  const endless = new ReadableStream({ pull(controller) { pulled += 1; controller.enqueue(new Uint8Array(64 * 1024)); } });
  await assert.rejects(readCapped(endless), (error) => /1 MiB at most/.test(error.why));
  // 16 chunks fill the MiB; the 17th goes past it, and nothing more is read.
  assert.ok(pulled <= 18, `pulled ${pulled}`);
});

test('a push without the skills grant, or of a core skill, sends nothing', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    await assert.rejects(lamp.pushSkill(skillFolder('lit')), byCode.no_grant);
    assert.equal(fake.asked('skill.push').length, 0);
  });
  await withLamp({ grants: ['light', 'skills'] }, async (lamp, fake) => {
    await assert.rejects(lamp.pushSkill(skillFolder('core', 'version = "0.1.0"\nkind = "core"\n')), (error) => error.code === 'refused' && /installable/.test(error.why));
    assert.equal(fake.asked('skill.push').length, 0);
  });
});

// harness-reviewer's P3 on #4029: the tar crate's bytes, so this, the Python
// SDK and `lamp-skills pack` give one SHA-256 for one folder (the Python test
// pins the same digest).
test('a skill packs to the bytes lamp-skills writes', async () => {
  const { pack } = await import('../src/push.js');
  const dir = join(folder(), 'desk-golden');
  mkdirSync(join(dir, 'moves'), { recursive: true });
  writeFileSync(join(dir, 'SKILL.md'), '---\nname: desk-golden\ndescription: A test skill.\n---\n');
  writeFileSync(join(dir, 'skill.toml'), 'version = "0.1.0"\n[permissions]\nlight = true\n');
  writeFileSync(join(dir, 'moves', 'wave.json'), '{}');
  const pkg = pack(dir);
  assert.equal(pkg.sha256, '4b956fc5e58e17f1165e68ccb16024df1a135a14e65d50b94febe372bf4a8321');
  assert.match(pkg.data.subarray(148, 156).toString('latin1'), /^[0-7]{7}\0$/);
  assert.equal(pkg.data.length, 5 * 1024);
});

// Reviewer-lead's P3 on #4029: lamp-skills' path rule first, then the tar
// crate's ustar cut, never a name cut short.
test('a path lamp-skills refuses or no header holds is refused before packing', async () => {
  const { pack } = await import('../src/push.js');
  const skill = (name, path) => {
    const dir = join(folder(), name);
    mkdirSync(join(dir, path, '..'), { recursive: true });
    writeFileSync(join(dir, 'skill.toml'), 'version = "0.1.0"\n');
    writeFileSync(join(dir, path), '{}');
    return dir;
  };
  assert.throws(() => pack(skill('desk-part', `${'m'.repeat(101)}.json`)), (error) => /at most 4 parts/.test(error.why));
  assert.throws(() => pack(skill('desk-deep', 'a/b/c/d/e.json')), (error) => /at most 4 parts/.test(error.why));
  const file = join(folder(), 'long.tar');
  writeFileSync(file, pack(skill('desk-long', `${'a'.repeat(100)}/w.json`)).data);
  assert.ok(execFileSync('tar', ['-tf', file], { encoding: 'utf8' }).split('\n').includes(`desk-long/${'a'.repeat(100)}/w.json`));
  assert.throws(() => pack(skill('desk-over', `${'a'.repeat(100)}/${'b'.repeat(100)}/c.json`)), (error) => /longer than a tar header holds/.test(error.why));
});
