// The page server's kept connection to a real lamp (examples/twin/link.js), and the SDK's word on a connection it ends itself. Against the
// SDK's fake lamp: a restart and a drop with no word end the connection; the link says why at once, connects again, listens again to what
// the page listened to, and a call made meanwhile answers the real reason, not a bare "closed".
import assert from 'node:assert/strict';
import { setTimeout as sleep } from 'node:timers/promises';
import { test } from 'node:test';
import { linked } from '../examples/twin/link.js';
import { Lamp, LampError } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

const until = async (check, what, ms = 4000) => {
  for (let i = 0; i < ms / 20 && !check(); i += 1) await sleep(20);
  assert.ok(check(), `never: ${what}`);
};

test('a restart is said at once, the connection is made again, and what was listened to is listened to again', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light', 'look'] });
  const said = [];
  const lamp = await linked(() => fake.connect(), { pauses: [0.02], say: (line) => said.push(line) });
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const heard = [];
  await lamp.on('person.arrived', (frame) => heard.push(frame));
  fake.restart();
  await until(() => said.some((line) => /connection is back/.test(line)), 'the connection is back');
  assert.match(said[0], /^the lamp's connection ended: the lamp restarts; reconnecting$/);
  assert.equal(said.length, 2, `said once each way: ${said}`);
  // The same proxy works on the new connection, and the earlier subscription hears the lamp again.
  await lamp.light('warm', 2, 1000);
  fake.emit('person.arrived', { count: 1 });
  await until(() => heard.length > 0, 'the person.arrived the page listened to before the restart');
});

test('a drop with no word says so, and a call made while it is down says the real reason', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const said = [];
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const lamp = await linked(() => fake.connect(), { pauses: [0.02], say: (line) => said.push(line), wait: async () => gate });
  t.after(async () => { release(); await lamp.close(); await fake.stop(); });
  fake.drop();
  await until(() => said.length > 0, 'the drop is said');
  assert.match(said[0], /^the lamp's connection ended: the connection dropped, with no reason given; reconnecting$/);
  await assert.rejects(lamp.light('warm', 2, 1000), (error) => error instanceof LampError && error.code === 'closed' && /the lamp's connection ended: the connection dropped.*reconnecting/.test(error.message));
  await assert.rejects(lamp.on('person.arrived', () => {}), /reconnecting/);
  release();
  await until(() => said.some((line) => /is back/.test(line)), 'back after the gate');
  await lamp.light('warm', 2, 1000);
});

test('what trying again cannot mend ends the link, said, and nothing keeps calling', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const said = [];
  let ended;
  let tries = 0;
  const lamp = await linked(async () => {
    tries += 1;
    if (tries > 1) throw new LampError('this computer was revoked by the owner', { code: 'unpaired' });
    return fake.connect();
  }, { pauses: [0.02], say: (line) => said.push(line), onEnd: (error) => { ended = error; } });
  t.after(async () => { await lamp.close().catch(() => {}); await fake.stop(); });
  fake.restart();
  await until(() => ended, 'the link ends');
  assert.equal(ended.code, 'unpaired');
  assert.ok(said.some((line) => /cannot be made again: unpaired: this computer was revoked/.test(line)), said.join(' | '));
  const after = tries;
  await sleep(150);
  assert.equal(tries, after, 'no more tries after an end that trying cannot mend');
});

test('closing the link does not connect again', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  let tries = 0;
  const lamp = await linked(async () => { tries += 1; return fake.connect(); }, { pauses: [0.02] });
  t.after(() => fake.stop());
  await lamp.close();
  await sleep(150);
  assert.equal(tries, 1);
});

test('the SDK says why when its own keepalive ends a connection: no answer to the ping', async (t) => {
  const was = Lamp.KEEPALIVE_MS;
  Lamp.KEEPALIVE_MS = 40;
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { Lamp.KEEPALIVE_MS = was; await lamp.close().catch(() => {}); await fake.stop(); });
  assert.equal(lamp.endedWhy, undefined, 'no reason while it is open');
  lamp.ws.ping = () => {}; // no ping goes out, so no pong comes back
  await lamp.closed();
  assert.match(lamp.endedWhy, /^no answer to the keepalive ping for 0\.04 s$/);
});

test('a connection the lamp closes with a word keeps its word, and one that just drops says so', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  t.after(() => fake.stop());
  const a = await fake.connect();
  fake.restart();
  await a.closed();
  assert.equal(a.endedWhy, 'the lamp restarts');
  const b = await fake.connect();
  fake.drop();
  await b.closed();
  assert.equal(b.endedWhy, 'the connection dropped, with no reason given');
});
