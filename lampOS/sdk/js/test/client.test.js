// The SDK against the fake lamp: pairing, hello, events, actions, leases and
// refusals, as docs/control.md says them. No lamp needed.
import assert from 'node:assert/strict';
import { execFile, execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, LampError, NotPaired, WrongLamp, byCode, fromReply, schema } from '../src/index.js';
import { AIR, DEVICE, FakeLamp, HAND_PATHS, handEvents, handTrack } from '../src/testing.js';

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

test('light_off is a black ring for a while', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    const count = lamp.ringPixels;
    assert.ok(count);
    await lamp.lightOff({ forMs: 5000 });
    const asked = fake.received.filter((f) => f.op === 'light').at(-1);
    assert.deepEqual(asked.pixels, Array(count).fill('#000000'));
    assert.equal(asked.for_ms, 5000);
    await lamp.lightOff();
    assert.equal(fake.received.filter((f) => f.op === 'light').at(-1).for_ms, 30000);
  });
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
  // The clips' and skills' calls read as English, not as their ops, and a
  // dotted op that is a program's own action is a call named for what it does
  // (the Python SDK's test has the same); any other is its camelCase.
  const named = {
    'sound.upload': 'uploadSound', 'sound.list': 'sounds', 'sound.remove': 'removeSound',
    'skill.push': 'pushSkill', 'skill.list': 'skills', 'skill.remove': 'removeSkill',
    'turn.start': 'startTurn', 'turn.end': 'endTurn', 'listen.set': 'setListening',
  };
  const call = (op) => named[op] ?? op.replace(/\.(\w)/g, (_, c) => c.toUpperCase());
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

test('the lamp\'s language comes from hello and is English when it says none', async () => {
  for (const [said, shown] of [['en', 'en'], ['vi', 'vi'], ['ko', 'ko'], [null, 'en'], ['klingon', 'en']]) {
    await withLamp({ language: said }, async (lamp) => {
      assert.equal(lamp.language, shown, String(said));
      assert.equal(lamp.info.language, said ?? undefined, String(said));
    });
  }
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
    { re: 5, ok: true, limits: { say: { most: 6, within_ms: 60000 }, hold_max_ms: 30000, lease_max_ms: 600000, take_max_ms: 600000, say_max_chars: 300, quiet_hours: { from: '22:00', to: '07:00' } } },
    { re: 6, ok: true, state: { volume: { level: 40, muted: false }, body: {
      known: true, stands: true, speed_deg_s: 4.4, acceleration_deg_s2: 35.2, load_limit: null, heat_stop_c: 62, counts_per_turn: 4096,
      joints: { yaw: { counts: 2026, deg: 1.0, min_deg: -3.5, max_deg: 3.5, min_counts: 1975, max_counts: 2055, home_counts: 2015, folded_counts: null, stand_counts: null } } } } },
    { re: 7, ok: true, state: { body: { known: false, why: 'the body has not said its limits in the last 15 s' } } },
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
  [[[0, 0, 0, 0, 0], [0, 0, 251, 0, 0], [0, 0, 0, 0, 0]], 1000, "frames: a move this body cannot play: frame 1 takes joint 2 251 counts, farther than 250, this lamp's yaw reach (--reach)"],
  [[[0, 0, 0, 0, 0], [5, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 1000, null], // 2 s: one the body plays
  [[...Array(7).fill([0, 0, 0, 0, 0]), ...Array(8).fill([0, 0, 0, 0, 300]), [0, 0, 0, 0, 0]], 100, 'frames: a move this body cannot play: frame 7 asks joint 4 for 3000 counts a second, faster than 800'],
  [[[1, 0, 0, 0, 0], [0, 0, 0, 0, 0], [0, 0, 0, 0, 0], [0, 0, 0, 0, 0]], 500, 'frames: start and end at home, all five 0'],
];

test('the move rules are the body\'s and lamp-control\'s own', async (t) => {
  // The fakes' numbers, read against lamp-body's moves.rs and lamp-control's
  // protocol.rs, so a change there fails here.
  const { MOVE_FARTHEST, MOVE_FASTEST, MOVE_STEP_MS, MOVE_LASTS_MS, MOVE_FRAMES_MIN } = await import('../src/testing.js');
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
  assert.equal(numbers(doorText.match(/pub const FRAMES_MIN: usize = ([0-9_]+);/)[1])[0], MOVE_FRAMES_MIN);
});

test('the fake refuses a move the body would drop, with its words, before the lease', async () => {
  const { moveRefusal } = await import('../src/testing.js');
  assert.equal(moveRefusal(500, PLAYABLE), null);
  for (const [frames, stepMs, refusal] of UNPLAYABLE) assert.equal(moveRefusal(stepMs, frames), refusal, JSON.stringify(frames));
  assert.equal(moveRefusal(20, PLAYABLE), 'step_ms: from 40 to 1000');
  assert.equal(moveRefusal(500, [[0, 0, 0, 0, 0]]), 'frames: at least 2');
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

const LIGHT_REFUSALS = [
  [{ rgb: '#ff8000', for_ms: 1000, fade_ms: 500 }, 'fade_ms: from 1000 to 60000'],
  [{ rgb: '#ff8000', for_ms: 1000, fade_ms: 60001 }, 'fade_ms: from 1000 to 60000'],
  [{ rgb: '#ff8000', for_ms: 30001 }, 'for_ms: from 1 to 30000'],
  [{ rgb: '#ff8000', for_ms: 0 }, 'for_ms: from 1 to 30000'],
  [{ rgb: '#ff8000', for_ms: 1000, brightness: 101 }, 'brightness: from 0 to 100'],
  [{ rgb: 'ff8000', for_ms: 1000 }, 'rgb: #rrggbb'],
  [{ pixels: Array(33).fill('#000000'), for_ms: 1000 }, 'pixels: 32 of them, one per pixel of the ring'],
  [{ pixels: [...Array(31).fill('#000000'), 'red'], for_ms: 1000 }, 'pixels: each #rrggbb'],
  [{ rgb: '#ff8000', colour: 'red', level: 3, for_ms: 1000 }, 'one of colour, rgb or pixels'],
  [{ for_ms: 1000 }, 'one of colour, rgb or pixels'],
  [{ colour: 'mauve', level: 3, for_ms: 1000 }, 'colour: one of yellow, red, green, blue, cyan, purple, orange, pink, white, warm'],
  [{ colour: 'red', level: 6, for_ms: 1000 }, 'level: from 1 to 5, with colour'],
  [{ colour: 'red', for_ms: 1000 }, 'level: from 1 to 5, with colour'],
  [{ colour: 'red', level: 3, brightness: 50, for_ms: 1000 }, 'brightness goes with rgb or pixels; colour has level'],
  [{ rgb: '#ff8000', level: 3, for_ms: 1000 }, 'level and pattern go with colour'],
];
const LIGHT_TAKEN = [
  { rgb: '#FF8000', for_ms: 1, fade_ms: 1000 },
  { rgb: '#ff8000', for_ms: 30000, fade_ms: 60000, brightness: 0 },
  { pixels: Array(32).fill('#000000'), for_ms: 1000, brightness: 100 },
  { colour: 'warm', level: 3, for_ms: 1000 },
];

test('the fake refuses a light the lamp would refuse, with lamp-control\'s words, and keeps the connection', async () => {
  const { lightRefusal } = await import('../src/testing.js');
  for (const [fields, refusal] of LIGHT_REFUSALS) assert.equal(lightRefusal({ op: 'light', ...fields }), refusal, JSON.stringify(fields).slice(0, 80));
  for (const fields of LIGHT_TAKEN) assert.equal(lightRefusal({ op: 'light', ...fields }), null, JSON.stringify(fields).slice(0, 80));
  await withLamp({}, async (lamp, fake) => {
    // Sent past the client's own check, as another computer's code might.
    for (const [fields, refusal] of LIGHT_REFUSALS) {
      // A colour the client itself knows is wrong is refused by its own check, in its words.
      const own = fields.colour === 'mauve';
      await assert.rejects(lamp.request('light', fields), (error) => (own ? error.code === 'bad_request' : error instanceof byCode.bad_request && error.why === refusal), JSON.stringify(fields).slice(0, 80));
    }
    for (const fields of LIGHT_TAKEN) await lamp.request('light', fields);
    assert.equal(fake.asked('light').length, LIGHT_TAKEN.length);
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

test('a phrase handler hears only its own phrases, and one with none hears all', async () => {
  await withLamp({ grants: ['body', 'light', 'speak', 'hear'] }, async (lamp, fake) => {
    const [started, finished, all] = [[], [], []];
    const start = (event) => started.push(event.phrase);
    await lamp.on('phrase.heard', start, { phrases: ['start studying'] });
    await lamp.on('phrase.heard', async (event) => { finished.push(event.phrase); }, { phrases: ['all done here'] });
    await lamp.on('phrase.heard', (event) => all.push(event.phrase));
    fake.emit('phrase.heard', { phrase: 'start studying' });
    fake.emit('phrase.heard', { phrase: 'All Done Here' });
    await until(() => all.length === 2);
    assert.deepEqual(started, ['start studying']);
    assert.deepEqual(finished, ['All Done Here']);
    // off() takes a filtered handler off by the handler itself.
    await lamp.off('phrase.heard', start);
    fake.emit('phrase.heard', { phrase: 'start studying' });
    await until(() => all.length === 3);
    assert.deepEqual(started, ['start studying']);
  });
});

test('phrases compare lowercased as the lamp and the Python SDK do, not case-folded', async () => {
  await withLamp({ grants: ['body', 'light', 'speak', 'hear'] }, async (lamp, fake) => {
    const heard = [];
    await lamp.on('phrase.heard', (event) => heard.push(event.phrase), { phrases: ['Große Straße'] });
    fake.emit('phrase.heard', { phrase: 'große straße' });
    fake.emit('phrase.heard', { phrase: 'GROSSE STRASSE' });
    await until(() => heard.length);
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.deepEqual(heard, ['große straße']);
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
      await assert.rejects(lamp.look('name'), (error) => error.code === 'refused' && error.why === "not in this release: look at who said the lamp's name (the lamp has no sound direction)");
      // A face: taken, then dropped "no face in view" while the fake sees none.
      assert.equal((await lamp.look('face')).to, 'body');
      for (let i = 0; i < 50 && !dones.length; i += 1) await new Promise((resolve) => setTimeout(resolve, 10));
      assert.deepEqual([dones[0].how, dones[0].why], ['dropped', 'no face in view']);
      dones.length = 0;
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
    // The lamp says `motion_done`: the wait resolves to the gesture's `done`.
    const ended = await lamp.gesture('nod', { wait: true });
    assert.deepEqual([ended.ev, ended.how], ['done', 'finished']);
    await lamp.stop();
    assert.deepEqual(fake.asked('light'), [{ colour: 'warm', level: 3, pattern: 'breathe', for_ms: 10000 }]);
    assert.deepEqual(fake.asked('say'), [{ text: 'Build passed.' }]);
    assert.deepEqual(fake.asked('express'), [{ feeling: 'glad' }]);
    assert.deepEqual(fake.asked('stop'), [{}]);
  });
});

test('a name that is no event says which event it meant', async () => {
  await withLamp({}, async (lamp, fake) => {
    // `lease.ended` is the event `lease` (state `lost`); `lease` and `done` come with no subscription.
    assert.throws(() => lamp.on('lease.ended', () => {}), (error) => error.code === 'bad_request'
      && error.message.includes("no event 'lease.ended'") && error.message.includes("lamp.on('lease', handler)")
      && error.message.includes('lost'));
    assert.throws(() => lamp.on('done.finished', () => {}), (error) => error.message.includes("lamp.on('done', handler)"));
    assert.throws(() => lamp.on('camera.zoomed', () => {}), (error) => error.message.includes('person.arrived')
      && error.message.includes("'lease' or 'done'") && error.message.includes('camera.frame')
      && error.message.includes('microphone.audio'));
    await assert.rejects(lamp.subscribe(['lease']), (error) => error.message.includes('without subscribing'));
    assert.deepEqual(fake.received.filter((f) => f.op === 'subscribe'), []);
  });
});

test('names outside the schema never leave this computer', async () => {
  await withLamp({}, async (lamp, fake) => {
    for (const call of [lamp.gesture('Nod'), lamp.gesture('../nod'), lamp.light('magenta', 3, 1000)]) {
      await assert.rejects(call, (error) => error.code === 'bad_request');
    }
    assert.throws(() => lamp.on('camera.zoomed', () => {}), LampError);
    await lamp.stop();
    assert.deepEqual(fake.received.filter((f) => ['gesture', 'light'].includes(f.op)), []);
  });
});

test('a say is held to the voice\'s 900 bytes here as on the lamp', async () => {
  // 900 UTF-8 bytes, lamp-control's SAY_MAX_BYTES (the voice's own), in bytes:
  // 300 three-byte letters are 900, one more is over.
  await withLamp({ grants: ['speak'] }, async (lamp, fake) => {
    for (const text of ['a'.repeat(900), 'ế'.repeat(300)]) await lamp.say(text);
    for (const text of ['a'.repeat(901), 'ế'.repeat(301)]) {
      await assert.rejects(lamp.say(text), (error) => error.code === 'bad_request' && error.message.includes('text: at most 900 bytes as the voice takes it'));
    }
    // A newline or tab is a control character, refused as the door does.
    for (const text of ['two\nlines', 'a\ttab']) {
      await assert.rejects(lamp.say(text), (error) => error.code === 'bad_request' && error.message.includes('text: words, no control characters'));
    }
    assert.deepEqual(fake.asked('say'), [{ text: 'a'.repeat(900) }, { text: 'ế'.repeat(300) }]);
  });
});

test('any gesture name the door takes is sent, idle and scanning too', async () => {
  // The owner's "no limits for developers" (30 Oct): the door takes any gesture
  // name of the right shape, so the client never refuses one first.
  await withLamp({}, async (lamp, fake) => {
    for (const name of ['idle', 'scanning', 'a_new_clip']) await lamp.gesture(name);
    assert.deepEqual(fake.asked('gesture'), [{ name: 'idle' }, { name: 'scanning' }, { name: 'a_new_clip' }]);
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

test('lamp.said needs the speech grant and comes with who had it said', async () => {
  await withLamp({ grants: ['speech'] }, async (lamp, fake) => {
    let told;
    const said = new Promise((resolve) => { told = resolve; });
    await lamp.on('lamp.said', (event) => told(event));
    fake.emit('lamp.said', { text: 'It is a long', by: 'lamp', source: 'reply', cut: true });
    const event = await said;
    assert.deepEqual([event.text, event.by, event.source, event.cut], ['It is a long', 'lamp', 'reply', true]);
  });
  await withLamp({ grants: ['speak'] }, async (lamp) => {
    await assert.rejects(lamp.on('lamp.said', () => {}), (error) => error.code === 'no_grant');
  });
});

test('air and device need their grants, and air.changed comes on a new reading', async () => {
  await withLamp({ grants: ['air', 'device'] }, async (lamp, fake) => {
    assert.deepEqual(await lamp.air(), AIR);
    assert.deepEqual(await lamp.device(), DEVICE);
    let told;
    const heard = new Promise((resolve) => { told = resolve; });
    await lamp.on('air.changed', (event) => told(event));
    fake.setAir({ pm2_5: 41, co2_ppm: 1350, feel: 'stuffy' });
    const event = await heard;
    assert.deepEqual([event.pm2_5, event.co2_ppm, event.feel], [41, 1350, 'stuffy']);
    assert.deepEqual(await lamp.air(), { pm2_5: 41, co2_ppm: 1350, feel: 'stuffy', known: true });
    // No sensor: known false, nothing else.
    fake.setAir();
    assert.deepEqual(await lamp.air(), { known: false });
  });
  await withLamp({ grants: ['light'] }, async (lamp) => {
    await assert.rejects(lamp.air(), /no_grant/);
    await assert.rejects(lamp.device(), /no_grant/);
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

test('the camera example saves a picture and a frame, and says a refusal', async () => {
  const { spawn } = await import('node:child_process');
  const { FAKE_JPEG } = await import('../src/testing.js');
  const example = new URL('../examples/camera.js', import.meta.url).pathname;
  const run = async (grants) => {
    const fake = await FakeLamp.start({ grants });
    const home = folder();
    const cwd = folder();
    try {
      await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(home) });
      // Not execFileSync: the fake answers on this process's event loop.
      const [code, said] = await new Promise((resolve, reject) => {
        const child = spawn(process.execPath, [example, '0.5'], { cwd, env: { ...process.env, LAMPOS_HOME: home } });
        let out = '';
        child.stdout.on('data', (chunk) => { out += chunk; });
        child.stderr.on('data', (chunk) => { out += chunk; });
        child.on('error', reject);
        child.on('close', (exit) => resolve([exit, out]));
      });
      return { code, said, cwd, streams: fake.asked('camera.stream').map((r) => r.on) };
    } finally {
      await fake.stop();
    }
  };
  const granted = await run(['camera']);
  assert.equal(granted.code, 0, granted.said);
  assert.deepEqual(readFileSync(`${granted.cwd}/snapshot.jpg`), FAKE_JPEG);
  assert.deepEqual(readFileSync(`${granted.cwd}/last-frame.jpg`), FAKE_JPEG);
  assert.deepEqual(granted.streams, [true, false], granted.said);
  const refused = await run(['light']);
  assert.equal(refused.code, 1, refused.said);
  assert.ok(refused.said.includes('snapshot: refused,') && !refused.said.includes('    at '), refused.said);
  assert.ok(!existsSync(`${refused.cwd}/snapshot.jpg`));
});

// A fake lamp with this computer paired and no connection open yet.
async function withPaired(options, body) {
  const fake = await FakeLamp.start(options);
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
    await body(fake, keyring);
  } finally { await fake.stop(); }
}

test('a lease whose connection closed is lost, not held', async () => {
  // The lamp ends a computer's leases when its last connection closes and
  // tells nobody: `state` must not go on saying `held` (the lead's 4-lamp
  // show, 10 Nov: held, then no_lease on the next look).
  await withPaired({ grants: ['body'] }, async (fake, keyring) => {
    const lamp = await Lamp.connect(undefined, { keyring });
    const lease = await lamp.lease('body').take();
    assert.equal(lease.held, true);
    fake.restart();
    await until(() => lease.state !== 'held');
    assert.equal(lease.state, 'lost');
    assert.equal(lease.lostWhy, 'the connection closed');
    assert.equal(lease.held, false);
    await lamp.close();
  });
});

test('keepConnected carries a program across the lamp\'s restart', async () => {
  await withPaired({ grants: ['body', 'look'] }, async (fake, keyring) => {
    const arrivals = [];
    const setups = [];
    const stop = new AbortController();
    const kept = Lamp.keepConnected(async (lamp) => {
      setups.push(lamp);
      // A lease held when the lamp goes: closing after it is quiet.
      await lamp.lease('body').take();
      await lamp.on('person.arrived', (event) => arrivals.push(event.count));
    }, { keyring, pauses: [0.05, 0.1], signal: stop.signal });
    for (const round of [1, 2, 3]) {
      await until(() => fake.asked('subscribe').length >= round);
      fake.emit('person.arrived', { count: round, near: 0.3 });
      await until(() => arrivals.length >= round);
      if (round < 3) fake.restart();
    }
    assert.deepEqual(arrivals, [1, 2, 3]);
    assert.equal(new Set(setups).size, 3);
    stop.abort();
    await kept;
  });
});

test('keepConnected stops at an abort that comes while it connects or pauses', async () => {
  // Second's P2 on #5833: an abort while connect is on its way, or before a
  // pause, is not missed: setup never runs and nothing stays open.
  await withPaired({ grants: ['light'] }, async (fake, keyring) => {
    const stop = new AbortController();
    const connect = Lamp.connect;
    let setups = 0;
    Lamp.connect = async (...args) => {
      const connection = await connect.call(Lamp, ...args);
      stop.abort(); // aborted while the connection was being made
      return connection;
    };
    try {
      await Lamp.keepConnected(async (lamp) => { setups += 1; await lamp.light('green', 2, 1000); }, { keyring, signal: stop.signal });
    } finally {
      Lamp.connect = connect;
    }
    assert.equal(setups, 0);
    assert.equal(fake.asked('light').length, 0);
    await until(() => fake.wss.clients.size === 0);
  });
  // An abort during a failed connect: the pause after it doesn't wait out its 10 s.
  const stop = new AbortController();
  const connect = Lamp.connect;
  Lamp.connect = async () => { stop.abort(); throw Object.assign(new Error('refused'), { code: 'ECONNREFUSED' }); };
  const started = Date.now();
  try {
    await Lamp.keepConnected(() => {}, { keyring: new Keyring(folder()), pauses: [10], signal: stop.signal });
  } finally {
    Lamp.connect = connect;
  }
  assert.ok(Date.now() - started < 2000, `${Date.now() - started} ms`);
});

test('keepConnected stops on what connecting again can\'t mend', async () => {
  await assert.rejects(Lamp.keepConnected(() => {}, { keyring: new Keyring(folder()) }), NotPaired);
  await withPaired({ grants: ['light'] }, async (fake, keyring) => {
    await assert.rejects(
      Lamp.keepConnected(() => { throw new TypeError("the program's own mistake"); }, { keyring, pauses: [0.05] }),
      TypeError,
    );
    // A subscription the lamp refuses (no `look` grant) comes out of it too,
    // instead of connecting again and again.
    await assert.rejects(
      Lamp.keepConnected((lamp) => lamp.on('person.arrived', () => {}), { keyring, pauses: [0.05] }),
      (error) => error instanceof LampError && error.code === 'no_grant',
    );
    assert.equal(fake.received.filter((r) => r.op === 'hello').length, 2);
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
  // A lamp-control from before #4209 (no `motion_done` in hello's limits):
  // `{"re":N,"ok":true,"to":"body"}` and no `done` after it; autoDone off,
  // so nothing else ever comes.
  await withLamp({ autoDone: false, motionDone: false }, async (lamp) => {
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
      const bogus = lamp.gesture('Bogus', { wait: true });
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

test('a waiting motion ends at its done where the lamp says motion_done', async () => {
  // #4209: hello's limits say `motion_done`, so `{ wait: true }` resolves to
  // the `done` the lamp sends once the body says what became of the motion.
  await withLamp({ autoDone: false }, async (lamp, fake) => {
    assert.equal(lamp.limits.motion_done, true);
    await lamp.withLease(['body'], async () => {
      let ended = false;
      const nod = lamp.gesture('nod', { wait: true }).then((done) => { ended = true; return done; });
      await until(() => fake.asked('gesture').length);
      await new Promise((resolve) => setTimeout(resolve, 100));
      assert.equal(ended, false, 'a wait that ended before the motion\'s done');
      fake.done(fake.requests.at(-1).id);
      assert.equal((await nod).how, 'finished');
      const move = lamp.move(PLAYABLE, 500, { wait: true });
      await until(() => fake.asked('move').length);
      fake.done(fake.requests.at(-1).id, 'dropped', 'dropped: the person is using the lamp');
      const dropped = await move;
      assert.deepEqual([dropped.how, dropped.why], ['dropped', 'dropped: the person is using the lamp']);
    }, { forMs: 5000 });
    const glad = lamp.express('glad', undefined, { wait: true });
    await until(() => fake.asked('express').length);
    fake.done(fake.requests.at(-1).id);
    assert.equal((await glad).how, 'finished');
    // Too slight to move: no move, no `done`, the answer at once.
    const still = await lamp.express('glad', 0.01, { wait: true });
    assert.deepEqual([still.to, still.move], ['body', null]);
    assert.equal(lamp.done.size, 0);
  });
});

test('a waiting say resolves to its done', async () => {
  await withLamp({ autoDone: false }, async (lamp, fake) => {
    let ended = false;
    const said = lamp.say('Word for word.', { wait: true }).then((done) => { ended = true; return done; });
    await until(() => fake.asked('say').length);
    await new Promise((resolve) => setTimeout(resolve, 100));
    assert.equal(ended, false, 'a say\'s wait that ended before its done');
    fake.done(fake.requests.at(-1).id);
    assert.equal((await said).how, 'finished');
    assert.equal(lamp.done.size, 0);
  });
});

test('only gesture, move and express end a wait at their reply on a lamp without motion_done', async () => {
  const { ENDS_AT_REPLY } = await import('../src/client.js');
  assert.deepEqual([...ENDS_AT_REPLY].sort(), ['express', 'gesture', 'move']);
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
  await withLamp({ autoDone: false, motionDone: false }, async (lamp) => {
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
    await assert.rejects(lamp.play('wave'), (error) => error.code === 'no_lease');
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.play('wave'), (error) => error.code === 'refused' && error.why === 'not routed yet: waits for client packages');
    });
  });
});

// As lamp-control on stable 10824 (parts.rs `joints`, the product lamp-body's
// lease socket): a look ahead or at an angle is taken, joints naming only yaw
// and pitch go, and one naming roll, shoulder or elbow is refused whole in its
// words; face and name stay "not in this release".
test('the fake takes look and yaw and pitch as a stable lamp does', async () => {
  const { JOINTS_WAIT, LAMP_CONTROL_REFUSES } = await import('../src/testing.js');
  await withLamp({ grants: schema.grants, refuses: LAMP_CONTROL_REFUSES, faceLook: false }, async (lamp) => {
    await lamp.withLease(['body'], async (lease) => {
      assert.equal((await lamp.look({ yaw: 5, pitch: 2 })).to, 'body');
      assert.equal((await lamp.joints(lease, 300, { yaw: -3, pitch: 1 })).to, 'body');
      for (const joint of ['roll', 'shoulder', 'elbow']) {
        await assert.rejects(lamp.joints(lease, 300, { yaw: 0, [joint]: 1 }), (error) => error.code === 'refused' && error.why === JOINTS_WAIT);
      }
      await assert.rejects(lamp.look('face'), (error) => error.code === 'refused' && error.why === 'not in this release: look at a face or a name');
    });
  });
});

// The next build (row 34): a look at a face is one turn toward the face the
// body sees, its `done` saying where the head got to; none seen, dropped.
test('the fake turns once toward a face in view and drops a look with none', async () => {
  const fake = { face: { yaw: 12, pitch: -4 } };
  await withLamp({ grants: ['body', 'look'], ...fake }, async (lamp) => {
    const dones = [];
    lamp.on('done', (done) => dones.push(done));
    await lamp.withLease(['body'], async () => {
      assert.equal((await lamp.look('face')).to, 'body');
      for (let i = 0; i < 50 && !dones.length; i += 1) await new Promise((resolve) => setTimeout(resolve, 10));
      assert.deepEqual([dones[0].how, dones[0].yaw, dones[0].pitch], ['finished', 12, -4]);
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
  // No product unit runs without it since stable 10824; LAMP_CONTROL_REFUSES
  // takes look, so this lamp is named here.
  const noLeaseSocket = { look: "the body's lease socket is not on here" };
  await withLamp({ grants: ['body', 'look'], refuses: noLeaseSocket }, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      for (const at of ['ahead', 'hand']) {
        await assert.rejects(lamp.look(at), (error) => error instanceof byCode.refused && /lease socket/.test(error.message));
      }
    });
    await assert.rejects(lamp.followHand().start(), byCode.refused);
    assert.equal(fake.leases.size, 0);
  });
});

test('a say is exact unless rephrase is asked', async () => {
  await withLamp({ grants: ['speak'] }, async (lamp, fake) => {
    await lamp.say('Build passed, 3 tests.');
    await lamp.say('Tell them the build passed.', { rephrase: true });
    assert.deepEqual(fake.asked('say'), [
      { text: 'Build passed, 3 tests.' },
      { text: 'Tell them the build passed.', rephrase: true },
    ]);
  });
});

test('the fake camera gives the picture a test sets', () => withLamp({ grants: ['camera'] }, async (lamp, fake) => {
  // An app that reads its picture (a brightness meter) needs a real image: `fake.picture` is what a snapshot
  // and a stream's frames carry; the default is the smallest whole JPEG.
  fake.picture = Buffer.from([0xff, 0xd8, 0x6d, 0x69, 0x6e, 0x65, 0xff, 0xd9]);
  assert.deepEqual(await lamp.cameraSnapshot(), fake.picture);
}));

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

// A WAV of `seconds` at 16-bit mono 24 kHz, every sample `level`.
function wav(seconds, level = 1_000) {
  const frames = Math.round(24_000 * seconds);
  const data = Buffer.alloc(44 + 2 * frames);
  data.write('RIFF', 0, 'ascii');
  data.writeUInt32LE(36 + 2 * frames, 4);
  data.write('WAVEfmt ', 8, 'ascii');
  data.writeUInt32LE(16, 16);
  data.writeUInt16LE(1, 20);
  data.writeUInt16LE(1, 22);
  data.writeUInt32LE(24_000, 24);
  data.writeUInt32LE(48_000, 28);
  data.writeUInt16LE(2, 32);
  data.writeUInt16LE(16, 34);
  data.write('data', 36, 'ascii');
  data.writeUInt32LE(2 * frames, 40);
  for (let at = 0; at < frames; at += 1) data.writeInt16LE(level, 44 + 2 * at);
  return data;
}

test('notes are set, read, listed and deleted as this computer\'s own', async () => {
  await withLamp({ grants: ['notes'] }, async (lamp, fake) => {
    assert.equal(lamp.limits.notes.keys_max, 256);
    assert.equal(await lamp.notes.get('mood'), undefined);
    assert.equal(await lamp.notes.get('mood', 'calm'), 'calm');
    assert.deepEqual(await lamp.notes.set('mood', { calm: [1, 2, 3] }), { key: 'mood', bytes: 16 });
    assert.deepEqual(await lamp.notes.get('mood'), { calm: [1, 2, 3] });
    // A stored null is found, not a missing key.
    await lamp.notes.set('nothing', null);
    assert.equal(await lamp.notes.get('nothing', 'missing'), null);
    await lamp.notes.set('mode', 7);
    const listed = await lamp.notes.list('mo');
    assert.deepEqual(listed.keys, [{ key: 'mode', bytes: 1 }, { key: 'mood', bytes: 16 }]);
    assert.deepEqual([listed.count, listed.total, listed.keys_max, listed.bytes_max], [2, 3, 256, 262_144]);
    assert.equal((await lamp.notes.list()).count, 3);
    assert.equal(await lamp.notes.delete('mood'), true);
    assert.equal(await lamp.notes.delete('mood'), false);
    assert.deepEqual(fake.asked('notes.delete'), [{ key: 'mood' }, { key: 'mood' }]);
    // The same calls by their op names, as every op has one.
    assert.equal(await lamp.notesGet('mode'), 7);
    assert.equal((await lamp.notesList('m')).count, 1);
  });
});

test('notes are held to the lamp\'s bounds and belong to one computer', async () => {
  await withLamp({ grants: ['notes'] }, async (lamp, fake, keyring) => {
    for (const bad of ['a b', '', 'k'.repeat(65)]) {
      await assert.rejects(lamp.notes.set(bad, 1), (error) => error.code === 'bad_request', bad);
    }
    await assert.rejects(lamp.notes.list('a/b'), (error) => error.code === 'bad_request');
    const big = 'x'.repeat(16_384 - 2);
    assert.equal((await lamp.notes.set('big', big)).bytes, 16_384);
    await assert.rejects(lamp.notes.set('big', `${big}x`), /16384/);
    assert.equal(await lamp.notes.get('big'), big);
    await lamp.notes.delete('big');
    for (let at = 0; at < 256; at++) await lamp.notes.set(`k${String(at).padStart(4, '0')}`, at);
    await assert.rejects(lamp.notes.set('one-more', 1), /full/);
    await lamp.notes.set('k0000', 'replaced');
    const client = keyring.pairing().client;
    assert.deepEqual([...fake.notes.keys()], [client]);
    fake.revoke(client);
    assert.equal(fake.notes.get(client)?.size ?? 0, 0);
  });
});

test('notes need their grant', async () => {
  await withLamp({ grants: ['speak'] }, async (lamp) => {
    for (const call of [() => lamp.notes.get('k'), () => lamp.notes.set('k', 1), () => lamp.notes.delete('k'), () => lamp.notes.list()]) {
      await assert.rejects(call(), (error) => error.code === 'no_grant');
    }
  });
});

test('a clip is uploaded in parts, played, listed and removed', async () => {
  await withLamp({ grants: ['speak'] }, async (lamp, fake) => {
    // 2 s, about 96 KB: three parts of 36 KiB.
    const clip = wav(2);
    const kept = await lamp.uploadSound('chime', clip);
    assert.deepEqual(kept, { name: 'chime', bytes: clip.length, ms: 2_000 });
    const parts = fake.asked('sound.upload');
    assert.deepEqual(parts.map((p) => p.part), [0, 1, 2]);
    assert.ok(parts.every((p) => p.parts === 3 && p.data.length <= 49_152));
    assert.deepEqual(await lamp.sounds(), [kept]);
    await lamp.sound('chime');
    await lamp.sound('hum');
    assert.deepEqual(fake.asked('sound'), [{ name: 'chime' }, { name: 'hum' }]);
    await lamp.removeSound('chime');
    assert.deepEqual(await lamp.sounds(), []);
    await assert.rejects(lamp.sound('chime'), /no clip named chime/);
    await assert.rejects(lamp.uploadSound('bad', Buffer.from('not a wav')), /not a WAV/);
    // The lamp's loudness cap, in its words (sounds.rs `check_clip`; lamp-75c9
    // refused a louder clip so, 9 Nov).
    await assert.rejects(lamp.uploadSound('loud', wav(0.5, 1_100)), (error) => error.why === 'clip: louder than -30 dBFS RMS over 10 ms');
    const click = wav(0.5, 0);
    click.writeInt16LE(9_000, 44);
    await assert.rejects(lamp.uploadSound('click', click), (error) => error.why === 'clip: a peak past -12 dBFS');
    assert.deepEqual(await lamp.sounds(), []);
    // A file shorter than its header says, by an odd or an even count of bytes: refused as sounds.rs's hound says,
    // and the connection goes on (Reviewer-two's P2 on #5800).
    for (const short of [1, 2]) {
      const clip = wav(0.01, 0);
      await assert.rejects(lamp.uploadSound('short', clip.subarray(0, clip.length - short)), (error) => error.why === 'clip: a WAV file cut short');
      assert.deepEqual(await lamp.sounds(), []);
    }
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
    const [listed, ...more] = await lamp.skills();
    assert.equal(more.length, 0);
    // The helper's own shape (lamp-skills `list_pushed_by`): never the pushing computer's id.
    assert.deepEqual(Object.keys(listed).sort(), ['author', 'held', 'name', 'on', 'pushed_at', 'pushed_at_unix_ms', 'sha256', 'version']);
    assert.equal(listed.name, 'desk-wave');
    assert.match(listed.pushed_at, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);
    assert.equal(Date.parse(listed.pushed_at), listed.pushed_at_unix_ms);
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

// The pairing-without-speak refusal is the Python SDK's pre-check and the
// lamp's; this SDK leaves it to the lamp.
test("the README's push example is pushed with skills and speak", async () => {
  const example = new URL('../../examples/hello-skill', import.meta.url).pathname;
  const readme = readFileSync(new URL('../README.md', import.meta.url), 'utf8');
  assert.ok(readme.includes('examples/hello-skill') && !readme.includes('relaxing-music'));
  await withLamp({ grants: ['speak', 'skills'] }, async (lamp) => {
    const skill = await lamp.pushSkill(example);
    assert.deepEqual([skill.name, skill.version], ['hello-skill', '1.0.0']);
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

test('a turn starts with its prompt, ends by its computer and needs speak and hear', async () => {
  await withLamp({ grants: ['speak', 'hear'], autoDone: false }, async (lamp, fake, keyring) => {
    await assert.rejects(lamp.endTurn(), (error) => error instanceof byCode.busy && /not this computer's turn/.test(error.why));
    assert.equal((await lamp.startTurn("Shall we go over today's list?")).to, 'voice');
    const [begun] = fake.requests.filter((r) => r.op === 'turn.start').map((r) => r.id);
    const ended = await lamp.endTurn();
    assert.deepEqual([ended.turn, ended.started], ['ended', begun]);
    // Waiting, the turn's `done` is its answer.
    const waiting = lamp.startTurn(undefined, { wait: true });
    await until(() => fake.asked('turn.start').length === 2);
    fake.done(fake.turns.get(keyring.pairing().client), 'dropped', 'dropped: nobody spoke');
    assert.equal((await waiting).why, 'dropped: nobody spoke');
    assert.deepEqual(fake.asked('turn.start'), [{ prompt: "Shall we go over today's list?" }, {}]);
    assert.deepEqual(fake.asked('turn.end'), [{}]);
  });
  await withLamp({ grants: ['speak'] }, async (lamp) => {
    await assert.rejects(lamp.startTurn(), (error) => error instanceof byCode.no_grant && /hear/.test(error.why));
  });
  await withLamp({ grants: ['speak', 'hear'] }, async (lamp) => {
    // Taken and finished at once with autoDone; the turn is over.
    assert.equal((await lamp.startTurn('Ready?', { wait: true })).how, 'finished');
    await assert.rejects(lamp.endTurn(), byCode.busy);
  });
});

test("the computer's mute is held, lifted, and ended by a hand or its going", async () => {
  const fake = await FakeLamp.start({ grants: ['hear'] });
  try {
    const keyring = new Keyring(folder());
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
    const client = keyring.pairing().client;
    const lamp = await Lamp.connect(undefined, { keyring });
    assert.equal((await lamp.setListening(true)).mute, 'nothing_held');
    assert.equal((await lamp.setListening(false)).mute, 'held');
    assert.equal((await lamp.setListening(false)).mute, 'held');
    // A stop doesn't lift it.
    await lamp.stop();
    assert.ok(fake.mutes.has(client));
    assert.equal((await lamp.setListening(true)).mute, 'lifted');
    // A hand on the head lifts it, said on the listen.set that held it.
    const dones = [];
    lamp.on('done', (event) => dones.push(event));
    await lamp.setListening(false);
    const held = fake.mutes.get(client).re;
    fake.handOnHead();
    await until(() => dones.length);
    assert.deepEqual(dones, [{ ev: 'done', re: held, how: 'cut_short', why: 'a hand on the head' }]);
    assert.equal((await lamp.setListening(true)).mute, 'nothing_held');
    await lamp.setListening(false);
    // The computer gone lifts it.
    await lamp.close();
    await until(() => !fake.mutes.has(client));
  } finally { await fake.stop(); }
  await withLamp({ grants: ['speak'] }, async (lamp) => {
    await assert.rejects(lamp.setListening(false), byCode.no_grant);
  });
});

test("a turn's prompt is summarised in the debug log as a say's text", async () => {
  const debug = await import('../src/debug.js');
  const shown = debug.redact({ id: 1, op: 'turn.start', prompt: 'Build passed.' });
  assert.equal(shown.prompt, debug.redact({ text: 'Build passed.' }).text);
  assert.deepEqual(debug.redact({ re: 2, ok: true, turn: 'ended', started: 1 }), { re: 2, ok: true, turn: 'ended', started: 1 });
  assert.deepEqual(debug.redact({ re: 3, ok: true, mute: 'held' }), { re: 3, ok: true, mute: 'held' });
});

test('the ring is lit pixel by pixel and in one colour of its own', async () => {
  await withLamp({ grants: ['light'] }, async (lamp, fake) => {
    assert.equal(lamp.ringPixels, 32);
    const pixels = Array(32).fill('#000000');
    pixels[0] = [255, 0, 0];
    assert.deepEqual(await lamp.ring(pixels, { forMs: 3000, brightness: 50 }), { re: 1, ok: true, to: 'ring', pixels: 32 });
    await lamp.lightRgb([0, 0, 255], { forMs: 1000 });
    const asked = fake.asked('light');
    assert.equal(asked[0].pixels[0], '#ff0000');
    assert.equal(asked[0].brightness, 50);
    assert.deepEqual(asked[1], { rgb: '#0000ff', for_ms: 1000 });
    for (const wrong of [Array(31).fill('#000000'), Array(32).fill([256, 0, 0])]) {
      await assert.rejects(lamp.ring(wrong, { forMs: 1000 }), (error) => error.code === 'bad_request');
    }
  });
});

test('the volume is set, stepped, muted and told when the person changes it', async () => {
  await withLamp({ grants: ['speak'] }, async (lamp, fake) => {
    assert.deepEqual(await lamp.get('volume'), { volume: { level: 50, muted: false } });
    assert.deepEqual(await lamp.volume({ to: 35 }), { level: 35, muted: false });
    assert.deepEqual(await lamp.volume({ step: 'up' }), { level: 55, muted: false });
    assert.deepEqual(await lamp.volume({ mute: true }), { level: 0, muted: true });
    assert.deepEqual(await lamp.volume({ mute: false }), { level: 55, muted: false });
    for (const wrong of [{}, { to: 10, mute: true }]) {
      await assert.rejects(lamp.volume(wrong), (error) => error.code === 'bad_request');
    }
    const heard = [];
    await lamp.on('volume.changed', (event) => heard.push(event));
    // The person says "quieter" to the lamp.
    fake.setVolume(30);
    await until(() => heard.length > 0);
    assert.deepEqual([heard[0].level, heard[0].muted], [30, false]);
  });
});

test('the volume needs the speak grant', async () => {
  await withLamp({ grants: ['body'] }, async (lamp) => {
    await assert.rejects(lamp.volume({ to: 10 }), (error) => error.code === 'no_grant');
    await assert.rejects(lamp.get('volume'), (error) => error.code === 'no_grant');
  });
});

// lamp-75c9, 9 Nov 14:33: a joints asked while a conversation had the lease
// suspended was answered { to: 'body' } by the door and refused by the body
// ('the body is not held'); without a wait, nothing told the program. With
// `wait`, the body's `done` comes back. test_client.py has the same.
test('a look or joints waits for the body\'s done and says when a conversation drops it', async () => {
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    const until = async (ready) => { while (!ready()) await new Promise((r) => setTimeout(r, 10)); };
    await lamp.withLease(['body'], async (lease) => {
      const there = await lamp.look({ yaw: -10, pitch: 5 }, { wait: true });
      assert.deepEqual([there.ev, there.how, there.yaw, there.pitch], ['done', 'finished', -10, 5]);
      const aimed = await lamp.joints(lease, 300, { pitch: 2, yaw: -4 }, { wait: true });
      assert.deepEqual([aimed.how, aimed.yaw, aimed.pitch], ['finished', -4, 2]);
      // Without `wait`, only the door's answer, as before.
      assert.equal((await lamp.joints(lease, 300, { yaw: 1 })).to, 'body');
      // A conversation suspends the lease: the door refuses a body op as the
      // suspension it is (session.rs), not as "take a lease first" nor
      // "another computer".
      fake.leaseEvent(lease.id, 'suspended', 'conversation');
      await until(() => lease.state === 'suspended');
      await assert.rejects(lamp.joints(lease, 300, { yaw: -13 }, { wait: true }), (error) => error.code === 'busy' && /suspended while someone talks/.test(error.why));
      await assert.rejects(lamp.look({ yaw: 3, pitch: 1 }, { wait: true }), (error) => error.code === 'busy' && /your lease on the body is suspended/.test(error.why));
      fake.leaseEvent(lease.id, 'held');
      await until(() => lease.state === 'held');
      assert.equal((await lamp.look('ahead', { wait: true })).how, 'finished');
      // A look at the hand has no `done` to wait for.
      await assert.rejects(lamp.look('hand', { wait: true }), /followHand/);
    }, { forMs: 5000 });
    assert.deepEqual(fake.asked('joints')[0], { lease: 'l_1', targets: { pitch: 2, yaw: -4 }, within_ms: 300 });
  });
});

// The race the door cannot see: a look taken, then the conversation suspends
// the lease before the body answers: the body's `done` says it was dropped.
test('a look taken just before a conversation is dropped by the body in its done', async () => {
  await withLamp({ grants: ['body', 'look'], autoDone: false }, async (lamp, fake) => {
    const until = async (ready) => { while (!ready()) await new Promise((r) => setTimeout(r, 10)); };
    await lamp.withLease(['body'], async (lease) => {
      const waiting = lamp.look({ yaw: 3, pitch: 1 }, { wait: true });
      await until(() => fake.asked('look').length > 0);
      fake.leaseEvent(lease.id, 'suspended', 'conversation');
      fake.done(fake.requests.at(-1).id, 'dropped', 'the body is not held');
      const dropped = await waiting;
      assert.deepEqual([dropped.how, dropped.why], ['dropped', 'the body is not held']);
    }, { forMs: 5000 });
  });
});

// Reviewer-two's P2 on #5837: lamp-control ends a hand-follow as soon as
// another look is taken, so the SDK does too, before the wait for the body's
// `done`. A refused look leaves it be, and the fake's `joints` is held to the
// door's rules. test_client.py has the same.
test('a waiting look ends a hand-follow when taken, not at its done', async () => {
  const { JOINTS_UNKNOWN, JOINTS_WAIT } = await import('../src/testing.js');
  await withLamp({ grants: ['body', 'look'] }, async (lamp, fake) => {
    const following = await lamp.followHand().start();
    assert.ok(following.following);
    fake.autoDone = false;
    // Taken, with its `done` held back: the follow ends at once.
    const look = lamp.look({ yaw: 2, pitch: 1 }, { wait: true });
    look.catch(() => {});
    while (fake.asked('look').length < 2) await new Promise((r) => setTimeout(r, 10));
    while (following.following) await new Promise((r) => setTimeout(r, 10));
    fake.done(fake.requests.at(-1).id);
    assert.equal((await look).how, 'finished');
    assert.ok(!following.following);
    fake.autoDone = true;
    // A refused look leaves a following alone.
    const again = await lamp.followHand().start();
    await assert.rejects(lamp.look('name', { wait: true }), byCode.refused);
    assert.ok(again.following);
    await again.stop();
    await lamp.withLease(['body'], async (lease) => {
      // Roll, shoulder and elbow wait for the body's goal stream.
      await assert.rejects(lamp.joints(lease, 300, { elbow: 4 }, { wait: true }), (error) => error.why === JOINTS_WAIT);
      // A joint not named stays where `get` `body` says it is.
      const body = (await lamp.get('body')).body.joints;
      const done = await lamp.joints(lease, 300, { yaw: -3 }, { wait: true });
      assert.deepEqual([done.yaw, done.pitch], [-3, body.pitch.deg]);
      // Unknown where the head is: name both, as the door says.
      fake.state.body.joints.pitch.deg = null;
      await assert.rejects(lamp.joints(lease, 300, { yaw: -3 }, { wait: true }), (error) => error.why === JOINTS_UNKNOWN);
      assert.equal((await lamp.joints(lease, 300, { yaw: -3, pitch: 1 }, { wait: true })).how, 'finished');
    }, { forMs: 5000 });
  });
});

// The same poses give the same frames in both SDKs: this digest is
// test_client.py's too.
const POSES_CASES = [[[{"pitch":10},{"pitch":-5,"yaw":8}],400],[[{"yaw":1}],0],[[{"shoulder":20,"elbow":-12.5},{"roll":-2.25,"pitch":3}],250],[[{"roll":0.5},{"roll":0.5},{"roll":0.5}],0]];
const POSES_DIGEST = 'e35bf686207b90cb4ba49373a765e63f0beeaade9e4b63165e8c09e0ae5ae765';

test('poses become a move the body plays, eased there and back, as Python makes it', async () => {
  const { createHash } = await import('node:crypto');
  const { posesToFrames } = await import('../src/index.js');
  const { COUNTS_PER_DEGREE, moveWhy } = await import('../src/moves.js');
  const made = POSES_CASES.map(([poses, holdMs]) => posesToFrames(poses, { holdMs }));
  assert.equal(createHash('sha256').update(JSON.stringify(made)).digest('hex'), POSES_DIGEST);
  const frames = made[0];
  assert.equal(moveWhy(50, frames), null);
  // Up is positive for pitch, as get('body') and joints count it: fewer counts.
  const up = [-Math.round(10 * COUNTS_PER_DEGREE), 0, 0, 0, 0];
  const at = frames.findIndex((frame) => JSON.stringify(frame) === JSON.stringify(up));
  assert.deepEqual(frames.slice(at, at + 9), Array(9).fill(up));
  assert.ok((made[1].length - 1) * 50 >= 1500);
  assert.equal(made[3].length - 1, 4 * 8);
});

test('poses a move cannot make are refused with the joint, before sending', async () => {
  const { posesToFrames } = await import('../src/index.js');
  for (const [poses, words] of [
    [[], 'at least one pose'],
    [[{}], 'name at least one of pitch, roll, yaw, shoulder, elbow'],
    [[{ nose: 3 }], "'nose' is not a joint"],
    [[{ pitch: true }], 'pitch must be a number of degrees'],
    [[{ pitch: NaN }], 'pitch must be a number of degrees'],
    [[{ pitch: 40 }], 'pitch 40° is farther than a move may take it, 17.6° either way'],
    [[{ yaw: -23 }], 'yaw -23° is farther than a move may take it, 22.0° either way'],
    [Array(12).fill({ shoulder: 30, elbow: -30 }), "longer than a move's 10000"],
  ]) {
    assert.throws(() => posesToFrames(poses), (error) => error instanceof LampError && error.code === 'bad_request' && error.message.includes(words), words);
  }
  assert.throws(() => posesToFrames([{ yaw: 2 }], { holdMs: -1 }), /holdMs/);
  await withLamp({}, async (lamp, fake) => {
    await lamp.withLease(['body'], async () => {
      await assert.rejects(lamp.movePoses([{ pitch: 40 }]), /farther than a move may take it/);
      assert.deepEqual(fake.asked('move'), []);
      await lamp.movePoses([{ pitch: 10 }, { yaw: -8 }], { holdMs: 200 });
    }, { forMs: 5000 });
    const sent = fake.asked('move');
    assert.equal(sent.length, 1);
    assert.equal(sent[0].step_ms, 50);
    assert.deepEqual(sent[0].frames, posesToFrames([{ pitch: 10 }, { yaw: -8 }], { holdMs: 200 }));
  });
});

test('the microphone streams 20 ms blocks with the microphone grant only, says lost blocks, and ends when private', async () => {
  await withLamp({ grants: ['light', 'microphone'] }, async (lamp, fake) => {
    const blocks = [];
    const ended = [];
    lamp.on('microphone.audio', (block) => blocks.push(block));
    lamp.on('microphone.ended', (event) => ended.push(event));
    const reply = await lamp.microphoneStream();
    assert.deepEqual(
      [reply.stream, reply.sample_rate, reply.channels, reply.format, reply.block_ms, reply.source],
      ['on', 16000, 1, 's16le', 20, 'mic1'],
    );
    await until(() => blocks.length >= 3);
    const [first] = blocks;
    assert.equal(first.ev, 'microphone.audio');
    assert.equal(first.lost, 0);
    assert.equal(first.format, 's16le');
    assert.equal(first.bytes, 640);
    assert.equal(first.data.length, 640);
    // Blocks lost on the way are said in the next one.
    fake.loseAudio(4);
    await until(() => blocks.some((block) => block.lost === 4));
    // Off: no more blocks and no event.
    assert.equal((await lamp.microphoneStream(false)).stream, 'off');
    const after = blocks.length;
    await new Promise((r) => setTimeout(r, 80));
    assert.equal(blocks.length, after);
    assert.equal(ended.length, 0);
    // Private: a stream ends at once, and a new one is refused.
    await lamp.microphoneStream(true);
    fake.setPrivate();
    await until(() => ended.length === 1);
    assert.deepEqual(ended[0], { ev: 'microphone.ended', why: 'the lamp is private' });
    await assert.rejects(lamp.microphoneStream(true), byCode.private);
    // Someone else in view: the same, with its own why.
    fake.setPrivate(false);
    await lamp.microphoneStream(true);
    fake.setInView(2);
    await until(() => ended.length === 2);
    assert.deepEqual(ended[1], { ev: 'microphone.ended', why: 'someone else is here' });
    await assert.rejects(lamp.microphoneStream(true), (error) => error instanceof byCode.refused && /someone else/.test(error.message));
  });
  await withLamp({}, async (lamp) => {
    await assert.rejects(lamp.microphoneStream(true), byCode.no_grant);
  });
});

test('fake.connect() gives a connected lamp with its own keyring and stop() cleans up', async () => {
  const home = folder();
  const before = process.env.LAMPOS_HOME;
  process.env.LAMPOS_HOME = join(home, 'own'); // a test must not touch the developer's own
  const fake = await FakeLamp.start();
  try {
    const lamp = await fake.connect();
    const third = await fake.connect();
    const other = await fake.connect({ name: 'second computer' });
    assert.deepEqual([...fake.clients.values()].map((c) => c.name), ['test app 1', 'test app 2', 'second computer']);
    await third.close();
    const reply = await lamp.say('hello');
    assert.deepEqual(fake.asked('say'), [{ text: 'hello' }]);
    assert.ok(reply.ok !== false && other !== lamp);
    assert.equal(fake.folders.length, 3);
    assert.ok(fake.folders.every((f) => existsSync(f)));
    await other.close(); // the test closed one itself: stop() must not trip on it
    const folders = [...fake.folders];
    await fake.stop();
    assert.ok(folders.every((f) => !existsSync(f)));
    assert.equal(existsSync(join(home, 'own')), false);
    await assert.rejects(lamp.say('after the fake is gone'));
  } finally {
    if (before === undefined) delete process.env.LAMPOS_HOME; else process.env.LAMPOS_HOME = before;
  }
});

test("FakeLamp({ grants: 'all' }) takes every grant the protocol names; the default stays narrow", async () => {
  const narrow = await FakeLamp.start();
  try {
    const lamp = await narrow.connect();
    await assert.rejects(lamp.on('phrase.heard', () => {}, { phrases: ['good night lamp'] }), { code: 'no_grant' });
  } finally { await narrow.stop(); }
  const wide = await FakeLamp.start({ grants: 'all' });
  try {
    const lamp = await wide.connect();
    await lamp.on('phrase.heard', () => {}, { phrases: ['good night lamp'] });
    assert.deepEqual(wide.grants, [...schema.grants]);
  } finally { await wide.stop(); }
  for (const slip of ['hear', 'All', '']) assert.throws(() => new FakeLamp({ grants: slip }), TypeError);
});
