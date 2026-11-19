// `lamp pair CODE`: no `--host` (the lamp found by mDNS) and no `--pin` (the key
// the lamp shows at pairing kept and checked on every later connect).
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, WrongLamp } from '../src/index.js';
import { choose, pairable } from '../src/discovery.js';
import { FakeLamp } from '../src/testing.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-test-'));
const started = (t, options) => FakeLamp.start(options).then((fake) => { t.after(() => fake.stop()); return fake; });
const found = (port, name, fp = '') => ({ id: '', name, host: `${name}.local`, port, fp, address: '127.0.0.1' });
const closedPort = () => new Promise((resolve) => {
  const server = createServer().listen(0, '127.0.0.1', () => { const { port } = server.address(); server.close(() => resolve(port)); });
});

test('pairing with no pin keeps the key the lamp shows, and a later other key is refused', async (t) => {
  const fake = await started(t);
  const keyring = new Keyring(folder());
  const pairing = await Lamp.pair(fake.code, fake.host, { port: fake.port, keyring });
  assert.equal(pairing.fp, fake.pin);
  assert.equal(keyring.pairing().fp, fake.pin);
  const lamp = await Lamp.connect(undefined, { keyring });
  await lamp.close();
  // A reset lamp (a new certificate), or another machine at its address.
  const reset = await started(t);
  keyring.remember({ ...keyring.pairing(), port: reset.port });
  await assert.rejects(Lamp.connect(undefined, { keyring }), (error) => {
    assert.ok(error instanceof WrongLamp);
    assert.match(error.message, /another key than the one this computer paired with/);
    assert.match(error.message, /re-pair if you reset the lamp/);
    return true;
  });
  assert.deepEqual(reset.received, []);
});

test('a given pin is still checked before the code is sent', async (t) => {
  const fake = await started(t);
  const keyring = new Keyring(folder());
  await assert.rejects(Lamp.pair(fake.code, fake.host, { pin: '0'.repeat(64), port: fake.port, keyring }), /is not the lamp's/);
  assert.deepEqual(fake.received, []);
  assert.deepEqual(keyring.pairings(), {});
});

test('pairable keeps only lamps whose control port answers, with no frame sent', async (t) => {
  const fake = await started(t);
  const look = async (ms, devices) => { assert.ok(devices); return [found(fake.port, 'lamp-on'), found(await closedPort(), 'lamp-off')]; };
  const lamps = await pairable(0, { probe: 1000, look });
  assert.deepEqual(lamps.map((lamp) => lamp.name), ['lamp-on']);
  assert.deepEqual(fake.received, []);
});

test('choose picks the one lamp, names several, and says what to do with none', () => {
  const one = found(1, 'lamp-one', 'aa'.repeat(8));
  const two = found(2, 'lamp-two', 'bb'.repeat(8));
  assert.equal(choose([one], undefined).lamp, one);
  assert.equal(choose([one, two], 'bb'.repeat(32)).lamp, two);
  assert.equal(choose([one, two], undefined).why, '2 lamps answered: lamp-one (lamp-one.local), lamp-two (lamp-two.local); say which with --host');
  assert.match(choose([], undefined).why, /developer access.*--host lamp-xxxx\.local/);
});
