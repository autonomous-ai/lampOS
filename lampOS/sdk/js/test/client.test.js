// The SDK against the fake lamp: pairing, hello, events, actions, leases and
// refusals, as docs/control.md says them. No lamp needed.
import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, LampError, NotPaired, WrongLamp, byCode, fromReply, schema } from '../src/index.js';
import { FakeLamp, HAND_PATHS, handEvents, handTrack } from '../src/testing.js';

// A move the body plays (and lamp-control takes): 4 frames of 500 ms, 1.5 s in
// all, from rest and back, well inside each joint's reach and speed.
const PLAYABLE = [[0, 0, 0, 0, 0], [20, 0, 30, 0, 0], [20, 0, 30, 0, 0], [0, 0, 0, 0, 0]];
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
  const call = (op) => op.replace(/\.(\w)/g, (_, c) => c.toUpperCase());
  for (const op of schema.ops) assert.ok(plumbing.has(op) || typeof Lamp.prototype[call(op)] === 'function', op);
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

test('the debug log redacts what would pair or hello as this computer', async () => {
  const debug = await import('../src/debug.js');
  const frame = {
    id: 1, op: 'pair', name: 'laptop', key: 'K'.repeat(44), proof: 'P'.repeat(44),
    lamp: { id: 'lamp-1', fp: 'f'.repeat(16) }, nonce: 'N'.repeat(44),
    skills: [{ sig: 'S'.repeat(88) }, { pin: 'a'.repeat(64) }],
    text: 'Build passed.', data: 'D'.repeat(1000),
  };
  const shown = debug.redact(frame);
  for (const field of ['key', 'proof', 'nonce']) assert.equal(shown[field], '<redacted>');
  assert.deepEqual(shown.lamp, { id: 'lamp-1', fp: '<redacted>' });
  assert.deepEqual(shown.skills, [{ sig: '<redacted>' }, { pin: '<redacted>' }]);
  // Fails closed: a field nobody decided (a newer lamp's) is hidden, at any depth.
  assert.deepEqual(debug.redact({ ev: 'lamp.changed', token: 't0ps3cret', lamp: { id: 'lamp-1', badge: 'b4dge' } }),
    { ev: 'lamp.changed', token: '<not shown>', lamp: { id: 'lamp-1', badge: '<not shown>' } });
  // What a lamp sends beyond the schema's own fields is decided too.
  for (const frame of [
    { re: 3, ok: true, to: 'body', move: 'glad-lift', light: true },
    { re: 1, ok: true, limits: { rates: 'off', refused: [{ most: 20, within_ms: 1000 }], phrases: [{ most: 6, within_ms: 60000 }] } },
    { re: 2, ok: true, state: { mood: { energy: 0.5, warmth: 0.6, awake: true, asleep: false }, light: { on: true, colour: 'warm', level: 3, shows: { on: true, colour: 'warm', level: 3 } } } },
    { ev: 'hand', seen: true, x: 0.4, y: 0.5, size: 0.12, open: true, confidence: 0.9 },
    { re: 4, ok: true, to: 'body', follow: 'hand' },
    { re: 5, ok: true, limits: { say: { most: 6, within_ms: 60000 }, hold_max_ms: 30000, take_max_ms: 600000, say_max_chars: 300, quiet_hours: { from: '22:00', to: '07:00' } } },
  ]) assert.ok(!JSON.stringify(debug.redact(frame)).includes('<not shown>'), JSON.stringify(frame));
  assert.ok(shown.text.startsWith('<13 chars, tag ') && !shown.text.includes('Build'));
  // Keyed for this run: the same text tags the same, and the unkeyed SHA-256's start isn't it.
  const { createHash } = await import('node:crypto');
  assert.equal(debug.redact({ text: 'Build passed.' }).text, shown.text);
  assert.ok(!shown.text.includes(createHash('sha256').update('Build passed.').digest('hex').slice(0, 8)));
  assert.equal(shown.data, '<1000 chars>');
  assert.equal(frame.key, 'K'.repeat(44)); // the frame itself is left as it was
  // A pair request's code is the owner's one-time code; a reply's is a refusal's, kept.
  assert.equal(debug.redact({ id: 1, op: 'pair', code: '12345678' }).code, '<redacted>');
  const refused = { re: 4, ok: false, error: { code: 'no_lease', why: 'take a body lease first' } };
  assert.deepEqual(debug.redact(refused), refused);
});

test('every field of the protocol is decided for the debug log', async () => {
  const debug = await import('../src/debug.js');
  const names = new Set();
  const walk = (value) => {
    if (Array.isArray(value)) value.forEach(walk);
    else if (value && typeof value === 'object') {
      Object.keys(value.properties ?? {}).forEach((name) => names.add(name));
      Object.values(value).forEach(walk);
    }
  };
  walk(schema.schema);
  assert.ok(names.size);
  // A new field in the schema is put in SECRET, SUMMARISED or SHOWN before it can reach a log.
  const undecided = [...names].filter((name) => !debug.SECRET.has(name) && !debug.SUMMARISED.has(name) && !debug.SHOWN.has(name));
  assert.deepEqual(undecided, []);
  assert.ok([...debug.SHOWN].every((name) => !debug.SECRET.has(name) && !debug.SUMMARISED.has(name)));
});

test('LAMPOS_LOG=debug logs each frame with its secrets redacted', async () => {
  const { b64 } = await import('../src/keys.js');
  const written = [];
  const write = process.stderr.write;
  process.stderr.write = (chunk, ...rest) => {
    if (String(chunk).startsWith('lampos ')) { written.push(String(chunk)); return true; }
    return write.call(process.stderr, chunk, ...rest);
  };
  const before = process.env.LAMPOS_LOG;
  try {
    delete process.env.LAMPOS_LOG;
    await withLamp({ grants: ['light', 'speak'] }, async (lamp) => { await lamp.say('The secret plan is ready.'); });
    assert.equal(written.length, 0); // off by default
    process.env.LAMPOS_LOG = 'debug';
    let pin; let key; let code;
    await withLamp({ grants: ['light', 'speak'] }, async (lamp, fake, keyring) => {
      pin = fake.pin; key = b64(keyring.public()); code = fake.code;
      await lamp.say('The secret plan is ready.');
      await lamp.light('warm', 3, 1000);
    });
    const err = written.join('');
    const lines = err.trim().split('\n');
    assert.ok(lines.length && lines.every((line) => /^lampos (->|<-) \{/.test(line)));
    const sent = lines.filter((line) => line.startsWith('lampos -> ')).map((line) => JSON.parse(line.slice(10)));
    for (const op of ['pair', 'hello', 'say', 'light']) assert.ok(sent.some((frame) => frame.op === op), op);
    assert.ok(sent.find((frame) => frame.op === 'say').text.startsWith('<25 chars, tag '));
    for (const secret of [code, pin, pin.slice(0, 16), key, 'The secret plan']) assert.ok(!err.includes(secret), secret.slice(0, 20));
    for (const field of ['nonce', 'sig', 'proof', 'key']) assert.ok(err.includes(`"${field}":"<redacted>"`), field);
  } finally {
    process.stderr.write = write;
    if (before === undefined) delete process.env.LAMPOS_LOG; else process.env.LAMPOS_LOG = before;
  }
});

// Moves the body would drop, and its words for each (lamp_body::moves::check;
// the Python SDK's test has the same list).
const UNPLAYABLE = [
  [[[0, 0, 0, 0, 0], [20, 0, 0, 0, 0], [20, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 200, 'frames: a move this body cannot play: it lasts 600 ms, not 1500 to 10000'],
  [[[0, 0, 0, 0, 0], [0, 0, 151, 0, 0], [0, 0, 0, 0, 0]], 1000, 'frames: a move this body cannot play: frame 1 takes joint 2 151 counts, farther than 150'],
  [[[0, 0, 0, 0, 0], [5, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 1000, null], // 2 s: one the body plays
  [[...Array(7).fill([0, 0, 0, 0, 0]), ...Array(8).fill([0, 0, 0, 0, 300]), [0, 0, 0, 0, 0]], 100, 'frames: a move this body cannot play: frame 7 asks joint 4 for 3000 counts a second, faster than 800'],
  [[[1, 0, 0, 0, 0], [0, 0, 0, 0, 0], [0, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 500, 'frames: start and end at home, all five 0'],
];

test('the move rules are the body\'s and lamp-control\'s own', async (t) => {
  // The fakes' numbers, read against lamp-body's moves.rs and lamp-control's
  // protocol.rs, so a change there fails here.
  const { MOVE_FARTHEST, MOVE_FASTEST, MOVE_STEP_MS, MOVE_LASTS_MS, MOVE_FRAMES } = await import('../src/testing.js');
  const body = new URL('../../../crates/body/src/moves.rs', import.meta.url);
  const door = new URL('../../../crates/control/src/protocol.rs', import.meta.url);
  if (!existsSync(body) || !existsSync(door)) return t.skip('the Rust crates are not beside this copy');
  const [bodyText, doorText] = [readFileSync(body, 'utf8'), readFileSync(door, 'utf8')];
  const numbers = (text) => [...text.matchAll(/[0-9][0-9_]*(?:\.[0-9]+)?/g)].map((m) => Number(m[0].replaceAll('_', '')));
  assert.deepEqual(numbers(bodyText.match(/pub const FARTHEST: \[i16; 5\] = \[([^\]]*)\]/)[1]), MOVE_FARTHEST);
  assert.deepEqual(numbers(bodyText.match(/pub const FASTEST: \[f32; 5\] = \[([^\]]*)\]/)[1]), MOVE_FASTEST);
  const shortest = numbers(bodyText.match(/const SHORTEST_US: u64 = ([0-9_]+);/)[1])[0];
  const longest = numbers(bodyText.match(/const LONGEST_US: u64 = ([0-9_]+);/)[1])[0];
  assert.deepEqual([shortest / 1000, longest / 1000], MOVE_LASTS_MS);
  assert.deepEqual(numbers(bodyText.match(/if !\(([0-9_]+)\.\.=([0-9_]+)\)\.contains\(&file\.step_ms\)/)[0]).slice(0, 2), MOVE_STEP_MS);
  assert.deepEqual(numbers(doorText.match(/pub const STEP_MS: [^=]*= ([0-9_]+)\.\.=([0-9_]+);/)[0]).slice(-2), MOVE_STEP_MS);
  assert.deepEqual(numbers(doorText.match(/pub const FRAMES: [^=]*= ([0-9_]+)\.\.=([0-9_]+);/)[0]).slice(-2), MOVE_FRAMES);
});

test('the fake refuses a move the body would drop, with its words, before the lease', async () => {
  const { moveRefusal } = await import('../src/testing.js');
  assert.equal(moveRefusal(500, PLAYABLE), null);
  for (const [frames, stepMs, refusal] of UNPLAYABLE) assert.equal(moveRefusal(stepMs, frames), refusal, JSON.stringify(frames));
  assert.equal(moveRefusal(20, PLAYABLE), 'step_ms: from 40 to 1000');
  assert.equal(moveRefusal(500, [[0, 0, 0, 0, 0]]), 'frames: from 2 to 500');
  await withLamp({}, async (lamp, fake) => {
    // Refused before the lease is asked for, as lamp-control's protocol.rs checks it.
    await assert.rejects(lamp.move(UNPLAYABLE[0][0], 200), (error) => error instanceof byCode.bad_request && error.message.includes('a move this body cannot play: it lasts 600 ms'));
    await lamp.withLease(['body'], async () => {
      for (const [frames, stepMs, refusal] of UNPLAYABLE) {
        if (refusal === null) { await lamp.move(frames, stepMs); continue; }
        await assert.rejects(lamp.move(frames, stepMs), (error) => error instanceof byCode.bad_request && error.why === refusal);
      }
      await lamp.move(PLAYABLE, 500);
    }, { forMs: 5000 });
    assert.equal(fake.asked('move').length, 2);
  });
});

test('the fake refuses a move whose frames lamp-control cannot parse, and keeps the connection', async () => {
  await withLamp({}, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      for (const middle of [[1, 2], null, [0, 0, 'a', 0, 0], [0, 0, 1.5, 0, 0]]) {
        // Sent past the client's own check, as another computer's code might.
        const frames = [[0, 0, 0, 0, 0], middle, [0, 0, 0, 0, 0], [0, 0, 0, 0, 0]];
        await assert.rejects(lamp.send(lamp.nextId++, 'move', { step_ms: 500, frames }), (error) => error instanceof byCode.bad_request && error.why === 'frames: each frame is five whole counts');
      }
      for (const stepMs of [1.5, '500']) {
        await assert.rejects(lamp.send(lamp.nextId++, 'move', { step_ms: stepMs, frames: PLAYABLE }), (error) => error instanceof byCode.bad_request && error.why === 'step_ms: a whole number of ms');
      }
      await lamp.move(PLAYABLE, 500);
    }, { forMs: 5000 });
    assert.equal(fake.asked('move').length, 1);
  });
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

test('every error code says what to do, on the error and in the README', async () => {
  const { WHAT_TO_DO } = await import('../src/errors.js');
  assert.deepEqual(Object.keys(WHAT_TO_DO).sort(), [...schema.errorCodes].sort());
  const refused = fromReply({ code: 'no_grant', why: 'not granted: look' });
  assert.equal(refused.whatToDo, WHAT_TO_DO.no_grant);
  assert.equal(refused.message, 'no_grant: not granted: look');
  assert.equal(new LampError('x', { code: 'closed' }).whatToDo, '');
  const readme = readFileSync(new URL('../README.md', import.meta.url), 'utf8');
  for (const [code, line] of Object.entries(WHAT_TO_DO)) assert.ok(readme.includes(`| \`${code}\` | ${line} |`), code);
});

test('greet a person who arrives', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    let greeted;
    const done = new Promise((resolve) => { greeted = resolve; });
    let at;
    await lamp.on('person.arrived', async (event) => { at = event.at; await lamp.gesture('greeting'); greeted(); });
    fake.emit('person.arrived', { count: 1, near: 0.3 });
    await done;
    assert.deepEqual(fake.asked('gesture'), [{ name: 'greeting' }]);
    // `at` as lamp-control stamps it: UTC, to the second, ending Z.
    assert.match(at, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);
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

// blue's P2 and P3 on #4023: `face` and `name` refused in parts.rs's words;
// an angle taken by the body, its `done` with where it got to.
test('the fake answers look as lamp-control does', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp) => {
    const dones = [];
    lamp.on('done', (done) => dones.push(done));
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.look('face'), (error) => error.code === 'refused' && error.why === 'not in this release: look at a face or a name');
      await assert.rejects(lamp.look('name'), (error) => error.code === 'refused' && error.why === 'not in this release: look at a face or a name');
      assert.equal((await lamp.look({ yaw: -10, pitch: 5 })).to, 'body');
      for (let i = 0; i < 50 && !dones.length; i += 1) await new Promise((resolve) => setTimeout(resolve, 10));
    });
    assert.equal(dones.length, 1);
    const { how, yaw, pitch, clamped } = dones[0];
    assert.deepEqual({ how, yaw, pitch, clamped }, { how: 'finished', yaw: -10, pitch: 5, clamped: false });
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
    assert.throws(() => lamp.on('camera.zoomed', () => {}), LampError);
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
      await assert.rejects(lamp.move(PLAYABLE, 500), byCode.no_lease);
    }, { forMs: 1000 });
    assert.deepEqual(fake.asked('lease.release'), []);
  });
});

test('refusals are typed and say when to try again', async () => {
  await withLamp({ grants: ['body'] }, async (lamp, fake) => {
    await assert.rejects(lamp.say('hello'), byCode.no_grant);
    fake.refuseNext('gesture', 'rate', 'motion: 1 each second', 500);
    await assert.rejects(lamp.gesture('nod'), (error) => error instanceof byCode.rate && error.retryMs === 500);
    fake.refuseNext('gesture', 'private', 'the privacy switch is closed');
    await assert.rejects(lamp.gesture('nod'), byCode.private);
  });
});

test('get answers the states asked', async () => {
  await withLamp({ grants: ['look'] }, async (lamp, fake) => {
    fake.state.presence = { count: 2, near: 0.4, facing: true };
    const state = await lamp.get('presence', 'lamp');
    // As lamp-control answers: the states by the names asked, `lamp` included, and nothing else.
    assert.deepEqual(state, { presence: { count: 2, near: 0.4, facing: true }, lamp: { talking: false, paused: false, private: false, asleep: false } });
  });
});

test('the fake light state shows what the ring draws', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    let now = 100_000;
    fake.clock = () => now;
    // Nothing heard yet: null, as lamp-control answers.
    assert.deepEqual(await lamp.get('light', 'mood'), { light: null, mood: null });
    const own = { on: true, colour: 'warm', level: 3 };
    fake.state.light = { ...own };
    assert.deepEqual(await lamp.get('light'), { light: { ...own, shows: own } });
    await lamp.light('green', 2, 5000, 'breathe');
    assert.deepEqual(await lamp.get('light'), { light: { ...own, shows: { on: true, colour: 'green', level: 2 } } });
    now += 5000; // it lapses: the person's own again
    assert.deepEqual(await lamp.get('light'), { light: { ...own, shows: own } });
    fake.state.light = {};
    await lamp.light('blue', 4, 2000);
    assert.deepEqual(await lamp.get('light'), { light: { shows: { on: true, colour: 'blue', level: 4 } } });
  });
});

test('the tour runs every step against a fake lamp', async () => {
  const { spawn } = await import('node:child_process');
  const fake = await FakeLamp.start({ grants: [...schema.grants] });
  const home = folder();
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(home) });
    // Not execFileSync: the fake answers on this process's event loop.
    const tour = new URL('../examples/tour.js', import.meta.url).pathname;
    const said = await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [tour], { env: { ...process.env, LAMPOS_HOME: home } });
      let out = '';
      child.stdout.on('data', (chunk) => { out += chunk; });
      child.stderr.on('data', (chunk) => { out += chunk; });
      child.on('error', reject);
      child.on('close', (code) => (code === 0 ? resolve(out) : reject(new Error(`exit ${code}: ${out}`))));
    });
    // A step's refusal prints as "name: refused, …" (`limits` has a `refused`
    // window, so the word alone isn't one).
    assert.ok(!said.includes(': refused,'), said);
    for (const op of ['get', 'light', 'say', 'gesture', 'express', 'look', 'joints', 'stop']) assert.ok(fake.asked(op).length, `${op}: ${said}`);
    assert.deepEqual(fake.asked('joints')[0].targets, { pitch: 5 });
  } finally {
    await fake.stop();
  }
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

test('the README reference names every public call, command and option', async () => {
  const { Lease } = await import('../src/client.js');
  const readme = readFileSync(new URL('../README.md', import.meta.url), 'utf8');
  const reference = readme.slice(readme.indexOf('## Reference'));
  // The client's own plumbing, not for programs: what the calls go through.
  const internal = new Set(['constructor', 'act', 'send', 'frame', 'renew', 'event']);
  for (const [name, cls] of [['Lamp', Lamp], ['Lease', Lease]]) {
    const own = [...Object.getOwnPropertyNames(cls.prototype), ...Object.getOwnPropertyNames(cls)
      .filter((key) => typeof cls[key] === 'function' && !['length', 'name', 'prototype'].includes(key))];
    for (const method of own.filter((key) => !internal.has(key))) {
      assert.ok(new RegExp(`\\.${method}[(\`,]`).test(reference), `${name}.${method}`);
    }
  }
  for (const word of ['info', 'grants', 'limits', 'find(', 'LAMPOS_HOME']) assert.ok(reference.includes(word), word);
  for (const code of schema.errorCodes) assert.ok(reference.includes(`\`${code}\``), code);
  for (const gesture of schema.gestures) assert.ok(reference.includes(`\`${gesture}\``), gesture);
  for (const colour of schema.colours) assert.ok(reference.includes(`\`${colour}\``), colour);
  // Every command and option of `lamp`, read from cli.js itself.
  const cli = readFileSync(new URL('../src/cli.js', import.meta.url), 'utf8');
  const options = Object.keys(Function(`return ${cli.match(/options: (\{[^\n]*\}),\n/)[1]}`)());
  for (const option of options) assert.ok(reference.includes(`--${option}`), `--${option}`);
  for (const command of [...cli.matchAll(/command === '([a-z]+)'/g)].map((m) => m[1])) {
    assert.ok(reference.includes(`lamp ${command}`), `lamp ${command}`);
  }
  // Everything a computer can read: every event the schema lists, the two that
  // come unasked, and every state `get` answers.
  const readable = readme.slice(readme.indexOf('## Everything you can read'), readme.indexOf('## Reference'));
  for (const event of [...schema.events, 'lease', 'done']) assert.ok(readable.includes(`| \`${event}\``), event);
  for (const state of schema.states) assert.ok(readable.includes(`| \`${state}\``), state);
  assert.ok(readable.includes('### Never readable'));
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
      const move = await lamp.move(PLAYABLE, 500, { wait: true });
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
    // The hum is routed, as lamp-control's sound.
    assert.equal((await lamp.sound('hum')).to, 'voice');
  });
});

// As lamp-control's session.rs: no lease is no_lease, and only a held lease
// reaches the "not routed yet" refusal (Reviewer-two's P3 on #3903).
test('the fake checks the lease before the routing refusal', async () => {
  const { LAMP_CONTROL_REFUSES } = await import('../src/testing.js');
  await withLamp({ grants: schema.grants, refuses: LAMP_CONTROL_REFUSES }, async (lamp) => {
    await assert.rejects(lamp.look('face'), (error) => error.code === 'no_lease');
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.look('face'), (error) => error.code === 'refused' && error.why === "the body's lease socket is not on here");
    });
  });
});

test('the fake light keeps lamp-control\'s pattern levels and the ring\'s pulse gap', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    for (const [pattern, level, why] of [
      ['breathe', 1, "a breath's low is from 1 to under the light's level"],
      ['candle', 2, 'a candle flickers at level 3 and up'],
      ['pulse', 1, 'a pulse is seen at level 2 and up'],
    ]) {
      await assert.rejects(lamp.light('warm', level, 5000, pattern), (error) => error instanceof byCode.refused && error.message.includes(why));
    }
    assert.deepEqual(fake.asked('light'), []);
    assert.equal((await lamp.light('warm', 1, 5000)).to, 'ring');
    assert.equal((await lamp.light('green', 2, 5000, 'breathe')).to, 'ring');
    let now = 1_000_000;
    fake.clock = () => now;
    await lamp.light('red', 3, 5000, 'pulse');
    now += 59_000;
    await lamp.light('red', 3, 5000, 'pulse'); // answered, but the ring drops it
    now += 2_000;
    await lamp.light('red', 3, 5000, 'pulse');
    assert.equal(fake.pulsesPlayed.length, 2);
    assert.equal(fake.pulsesDropped.length, 1);
    assert.equal(fake.asked('light').length, 5);
  });
});

test('a second pulse within a minute shows without its pulse, and a refused light never', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    let now = 100_000;
    fake.clock = () => now;
    await lamp.light('red', 3, 30000, 'pulse');
    now += 10_000;
    await lamp.light('blue', 2, 30000, 'pulse'); // within a minute: shown, no pulse
    assert.deepEqual((await lamp.get('light')).light.shows, { on: true, colour: 'blue', level: 2 });
    assert.deepEqual([fake.pulsesPlayed.length, fake.pulsesDropped.length], [1, 1]);
    await assert.rejects(lamp.light('green', 1, 30000, 'breathe'), byCode.refused); // refused: never shown
    assert.equal((await lamp.get('light')).light.shows.colour, 'blue');
  });
});


test('hand events need the look grant and carry only where the hand is', async () => {
  await withLamp({ grants: ['body'] }, async (lamp) => {
    await assert.rejects(lamp.on('hand', () => {}), byCode.no_grant);
  });
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    const seen = [];
    await lamp.on('hand', (event) => seen.push(event));
    const track = handTrack('circle', { seconds: 2, hz: 15 });
    const said = handEvents(track).map(([, event]) => event);
    await fake.emitHandTrack(track);
    await until(() => seen.length === said.length);
    assert.deepEqual(seen.map(({ ev, at, ...hand }) => hand), said);
    assert.deepEqual(Object.keys(said[0]).sort(), ['confidence', 'open', 'seen', 'size', 'x', 'y']);
    assert.deepEqual(said.at(-1), { seen: false });
  });
});

test('hand events are told by change, four a second at most, and gone after a second', () => {
  const track = handTrack('still', { seconds: 2, hz: 15 });
  const still = handEvents(track);
  assert.deepEqual(still.map(([, e]) => e.seen), [true, false]);
  assert.ok(still[1][0] - track.at(-1).t >= 1_000_000 && still[1][0] - track.at(-1).t < 1_250_000);
  const told = handEvents(handTrack('wave', { seconds: 2, hz: 15 })).filter(([, e]) => e.seen);
  assert.ok(told.length >= 6);
  for (let n = 1; n < told.length; n++) assert.ok(told[n][0] - told[n - 1][0] >= 250_000);
  const closing = [...handTrack('still', { seconds: 1, hz: 10 }), ...handTrack('still', { seconds: 1, hz: 10, open: false, t0: 1_000_000 })];
  assert.deepEqual(handEvents(closing).filter(([, e]) => e.seen).map(([, e]) => e.open), [true, false]);
  const gap = [...handTrack('still', { seconds: 0.5, hz: 10 }), ...handTrack('still', { seconds: 0.5, hz: 10, t0: 3_000_000 })];
  assert.deepEqual(handEvents(gap).map(([, e]) => e.seen), [true, false, true, false]);
  assert.deepEqual(handEvents(handTrack('still', { seconds: 1 }).map((h) => ({ ...h, confidence: 0.3 }))), []);
});

test('synthetic hand tracks are in the topic\'s shape and repeat by seed', () => {
  for (const path of HAND_PATHS) {
    const track = handTrack(path, { seconds: 2, hz: 15, seed: 3 });
    assert.equal(track.length, 30);
    assert.deepEqual(track, handTrack(path, { seconds: 2, hz: 15, seed: 3 }));
    assert.deepEqual(Object.keys(track[0]).sort(), ['confidence', 'open', 'size', 't', 'x', 'y']);
    assert.equal(track[1].t - track[0].t, Math.round(1_000_000 / 15));
    for (const hand of track) {
      assert.ok(hand.x >= 0 && hand.x <= 1 && hand.y >= 0 && hand.y <= 1 && hand.size > 0 && hand.size < 1);
      assert.ok(hand.confidence >= 0.5 && hand.confidence <= 1 && Number.isInteger(hand.t));
    }
  }
  const xs = handTrack('wave', { seconds: 2 }).map((h) => h.x);
  assert.ok(Math.max(...xs) - Math.min(...xs) > 0.5);
  assert.throws(() => handTrack('scan'));
});

// The same track and the same events in both SDKs' tests (and lamp-control's
// watch.rs): across, closed, then gone.
const FIXED_TRACK = Array.from({ length: 40 }, (_, n) => ({ x: 0.1 + n * 0.02, y: 0.5, size: 0.125, open: n < 30, confidence: 0.875, t: n * 66_667 }));
const FIXED_EVENTS = [[0, {"seen": true, "x": 0.1, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [250000, {"seen": true, "x": 0.16, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [500000, {"seen": true, "x": 0.24, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [750000, {"seen": true, "x": 0.32, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [1000000, {"seen": true, "x": 0.38, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [1250000, {"seen": true, "x": 0.46, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [1500000, {"seen": true, "x": 0.54, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [1750000, {"seen": true, "x": 0.62, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [2000000, {"seen": true, "x": 0.68, "y": 0.5, "size": 0.13, "open": true, "confidence": 0.88}], [2250000, {"seen": true, "x": 0.76, "y": 0.5, "size": 0.13, "open": false, "confidence": 0.88}], [2500000, {"seen": true, "x": 0.84, "y": 0.5, "size": 0.13, "open": false, "confidence": 0.88}], [3750000, {"seen": false}]];

test('a fixed track tells the same hand events in both SDKs', () => {
  assert.deepEqual(handEvents(FIXED_TRACK), FIXED_EVENTS);
});

test('followHand follows under its own lease and waits while the hand is gone', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    const following = await lamp.followHand({ forMs: 1000, why: 'test' }).start();
    assert.ok(following.following && fake.following.size === 1);
    await fake.emitHandTrack(handTrack('wave', { seconds: 0.5 }));
    await new Promise((r) => setTimeout(r, 50));
    assert.ok(following.following && fake.following.size === 1);
    await lamp.look('ahead');
    assert.deepEqual(await following.wait(), { how: 'cut_short', why: 'another look' });
    assert.ok(!following.following);
    await following.stop();
    await until(() => fake.leases.size === 0);
    assert.equal(fake.following.size, 0);
    assert.deepEqual(fake.asked('look'), [{ at: 'hand' }, { at: 'ahead' }]);
    assert.deepEqual(fake.asked('lease.take'), [{ parts: ['body'], for_ms: 1000, why: 'test' }]);
  });
});

test('followHand stops when asked, at a stop, or when the lamp takes the body back', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    let following = await lamp.followHand().start();
    assert.equal((await following.stop()).how, 'finished');
    assert.equal(fake.leases.size, 0);
    assert.equal(fake.following.size, 0);
    following = await lamp.followHand().start();
    await lamp.stop();
    assert.equal((await following.wait()).why, 'stop');
    // The client's stop lets go of its lease too.
    assert.equal(fake.leases.size, 0);
    assert.equal(following.lease.held, false);
    await following.stop();
    following = await lamp.followHand().start();
    fake.loseLease(following.lease.id, 'hand');
    assert.deepEqual(await following.wait(), { how: 'cut_short', why: 'hand' });
    await following.stop();
    assert.deepEqual(fake.asked('lease.release'), [{ lease: 'l_1' }, { lease: 'l_2' }]);
  });
});

test('followHand keeps its lease after another look, and a new followHand lets it go', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    const first = await lamp.followHand().start();
    await lamp.look('ahead');
    assert.deepEqual(await first.wait(), { how: 'cut_short', why: 'another look' });
    // The look runs under the following's lease, so it is still held.
    assert.equal(first.lease.held, true);
    assert.equal(fake.leases.size, 1);
    // A new following lets go of it first, rather than being refused busy.
    const second = await lamp.followHand().start();
    assert.equal(second.following, true);
    assert.equal(first.lease.held, false);
    assert.equal(fake.leases.size, 1);
    // And one still following ends the same way.
    const third = await lamp.followHand().start();
    assert.deepEqual(await second.wait(), { how: 'cut_short', why: 'another look' });
    assert.equal(third.following, true);
    assert.equal(fake.leases.size, 1);
    await third.stop();
    assert.equal(fake.leases.size, 0);
  });
});

test('a look at the hand is answered as lamp-control does, with no done', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      const reply = await lamp.request('look', { at: 'hand' });
      assert.deepEqual([reply.to, reply.follow], ['body', 'hand']);
      assert.equal(fake.following.size, 1);
      await lamp.request('look', { at: 'ahead' });
      assert.equal(fake.following.size, 0);
    });
  });
});

test('followHand needs the look grant and lets go of its lease when refused', async () => {
  await withLamp({ grants: ['body'] }, async (lamp, fake) => {
    await assert.rejects(lamp.followHand().start(), byCode.no_grant);
    await until(() => fake.leases.size === 0);
  });
});

test('followHand lets go on a lamp that takes the look without following', async () => {
  await withLamp({ grants: ['body', 'look'], followsHand: false }, async (lamp, fake) => {
    await assert.rejects(lamp.followHand().start(), (error) => error instanceof byCode.refused && /doesn't follow a hand yet/.test(error.message));
    await until(() => fake.leases.size === 0);
    assert.equal(fake.following.size, 0);
    assert.deepEqual(fake.asked('lease.release'), [{ lease: 'l_1' }]);
  });
});

test('a look at the hand needs the body\'s lease socket, as other looks do', async () => {
  // lamp-control without --lease-socket refuses every look, `hand` too
  // (parts.rs: "the body's lease socket"), and followHand lets go of its lease.
  const { LAMP_CONTROL_REFUSES } = await import('../src/testing.js');
  await withLamp({ grants: ['body', 'look'], refuses: LAMP_CONTROL_REFUSES }, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      for (const at of ['ahead', 'hand']) {
        await assert.rejects(lamp.look(at), (error) => error instanceof byCode.refused && /lease socket/.test(error.message));
      }
    });
    await assert.rejects(lamp.followHand().start(), byCode.refused);
    assert.equal(fake.leases.size, 0);
  });
});

test('the camera stream asks up to 30 a second, and 31 is refused before sending', () => withLamp({ grants: ['light', 'camera'] }, async (lamp) => {
  const started = await lamp.cameraStream(true, 30);
  assert.equal(started.fps, 30);
  assert.ok('fps_now' in started);
  await assert.rejects(lamp.cameraStream(true, 31), LampError);
}));

test('a fast binary frame is its header and the JPEG as data', async () => {
  const { binaryFrame } = await import('../src/client.js');
  const header = Buffer.from(JSON.stringify({ ev: 'camera.frame', bytes: 4, width: 640, height: 480, fps_now: 24.8 }));
  const length = Buffer.alloc(4);
  length.writeUInt32BE(header.length);
  const jpeg = Buffer.from([0xff, 0xd8, 0xff, 0xd9]);
  const frame = binaryFrame(Buffer.concat([length, header, jpeg]));
  assert.equal(frame.ev, 'camera.frame');
  assert.equal(frame.width, 640);
  assert.deepEqual([...frame.data], [...jpeg]);
});
