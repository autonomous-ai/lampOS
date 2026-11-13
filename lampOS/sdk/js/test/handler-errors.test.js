// A handler that throws is said on stderr, not swallowed; a program that listens for 'handlerError' on lamp.ws takes it over.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

async function connected(t) {
  const folder = mkdtempSync(join(tmpdir(), 'handler-'));
  const fake = await FakeLamp.start({ grants: ['body', 'light', 'speak', 'look'] });
  const keyring = new Keyring(folder);
  await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
  const lamp = await Lamp.connect(undefined, { keyring });
  t.after(async () => { await lamp.close(); await fake.stop(); rmSync(folder, { recursive: true, force: true }); });
  return { lamp, fake };
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('a handler that throws is said on stderr', async (t) => {
  const { lamp, fake } = await connected(t);
  const said = [];
  t.mock.method(console, 'error', (...args) => said.push(args.join(' ')));
  await lamp.on('person.arrived', () => { throw new Error('refused: for_ms: from 1 to 30000'); });
  await fake.emit('person.arrived', { count: 1, near: 0.3 });
  for (let i = 0; i < 50 && !said.length; i++) await sleep(20);
  assert.equal(said.length, 1);
  assert.match(said[0], /your handler for person\.arrived threw: Error: refused: for_ms/);
});

test('a program that listens for handlerError takes it over, and nothing is said', async (t) => {
  const { lamp, fake } = await connected(t);
  const said = [];
  const taken = [];
  t.mock.method(console, 'error', (...args) => said.push(args.join(' ')));
  lamp.ws.on('handlerError', (error) => taken.push(error.message));
  await lamp.on('person.arrived', async () => { throw new Error('mine'); });
  await fake.emit('person.arrived', { count: 1, near: 0.3 });
  for (let i = 0; i < 50 && !taken.length; i++) await sleep(20);
  assert.deepEqual(taken, ['mine']);
  assert.deepEqual(said, []);
});

test('a handler that throws says its stack, and the other handlers and the next event still arrive', async (t) => {
  const { lamp, fake } = await connected(t);
  const said = [];
  t.mock.method(console, 'error', (...args) => said.push(args.join(' ')));
  const seen = [];
  await lamp.on('touch', (event) => event.no_such.key);
  await lamp.on('touch', async () => { throw new RangeError('my async bug'); });
  await lamp.on('touch', (event) => seen.push(event.kind));
  await fake.emit('touch', { kind: 'tap' });
  await fake.emit('touch', { kind: 'pet' });
  for (let i = 0; i < 100 && (seen.length < 2 || said.length < 4); i++) await sleep(20);
  assert.deepEqual(seen, ['tap', 'pet']);
  assert.equal(said.length, 4);
  // The stack, not only the message: a "Cannot read properties" alone does not say where.
  for (const line of said) assert.match(line, /\n\s+at /, line);
});
