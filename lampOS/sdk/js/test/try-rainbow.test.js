// The try-it app's endless rainbow (twin/rainbow.js and the rainbow_on / rainbow_off rows of controls.js), against the SDK's
// fake lamp: frames that turn, the lamp's own pace, a lamp that says "busy" and is asked again, a refused first frame in the
// lamp's words, and a stop that leaves nothing sending.
import assert from 'node:assert/strict';
import { setTimeout as sleep } from 'node:timers/promises';
import { test } from 'node:test';
import { CONTROLS, checked, stopRainbow } from '../examples/twin/controls.js';
import { FASTEST_MS, frameGap, rainbowFrame, startRainbow } from '../examples/twin/rainbow.js';
import { FakeLamp } from '../src/testing.js';

const rowOf = (lamp) => (id, values = {}) => {
  const control = CONTROLS.find((c) => c.id === id);
  return control.run({ lamp, release: async () => {} }, checked(control.fields, values));
};

test('the gap between frames keeps under every light window hello says, and is the ring\'s refresh with the rates off', () => {
  assert.equal(frameGap({ rates: 'off' }), FASTEST_MS);
  assert.equal(frameGap({}), FASTEST_MS);
  assert.equal(frameGap({ rates: { light: [{ most: 5, within_ms: 1000 }] } }), 221);
  assert.equal(frameGap({ rates: { light: [{ most: 5, within_ms: 1000 }, { most: 30, within_ms: 60_000 }] } }), 2200);
  assert.equal(frameGap({ rates: { light: [{ most: 100, within_ms: 1000 }] } }), FASTEST_MS);
});

test('a frame is 32 valid colours and turning the wheel moves every colour one pixel along', () => {
  const frame = rainbowFrame(32, 0);
  assert.equal(frame.length, 32);
  assert.ok(frame.every((c) => /^#[0-9a-f]{6}$/.test(c)));
  assert.equal(frame[0], '#ff0000');
  assert.deepEqual(rainbowFrame(32, 1 / 32).slice(0, 31), frame.slice(1));
  assert.notDeepEqual(rainbowFrame(32, 0.5), frame);
});

test('the rainbow rows send frames that turn until the off row, and nothing is sent after it', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  const run = rowOf(lamp);
  assert.match(await run('rainbow_on', { seconds_per_turn: 2 }), /^lamp\.ring\(rainbow frames, \{ forMs: 250 \}\) every 25 ms, one turn in 2 s, until switched off$/);
  await sleep(400);
  const before = fake.asked('light');
  assert.ok(before.length >= 5, `frames went on: ${before.length}`);
  assert.ok(before.every((m) => m.pixels.length === 32 && m.for_ms === 250));
  assert.ok(new Set(before.map((m) => m.pixels[0])).size > 1, 'the first pixel changes colour: the wheel turns');
  const said = await run('rainbow_off');
  assert.match(said, /^the rainbow ended: \d+ frames sent every 25 ms, \d+ answered, 0 refused$/);
  const after = fake.asked('light').length;
  await sleep(200);
  assert.equal(fake.asked('light').length, after, 'nothing is sent after the off row');
  assert.equal(await run('rainbow_off'), 'the rainbow was not on');
  assert.equal(CONTROLS.find((c) => c.id === 'rainbow_on').needs[0], 'light');
});

test('a lamp that says busy is asked again and the rainbow comes back by itself, counted and said once', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const said = [];
  for (let i = 0; i < 3; i += 1) fake.refuseNext('light', 'busy', 'a conversation holds the ring');
  const rainbow = startRainbow(lamp, { say: (line) => said.push(line) });
  await rainbow.first;
  const answered = async () => { for (let i = 0; i < 100 && rainbow.stats().answered < 3; i += 1) await sleep(50); };
  await answered();
  const stats = rainbow.stats();
  assert.equal(stats.byCode.busy, 3);
  assert.ok(stats.answered >= 3, 'frames were answered after the busy ones');
  assert.equal(said.filter((line) => /busy/.test(line)).length, 1, 'said once, not every frame');
  assert.match(said[0], /comes back by itself/);
  await rainbow.stop();
});

test('a first frame the lamp cannot take at all is the row\'s own refusal in the lamp\'s words, and nothing keeps sending', async (t) => {
  const fake = await FakeLamp.start({ grants: [] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  await assert.rejects(rowOf(lamp)('rainbow_on'), (error) => error.code === 'no_grant');
  const sent = fake.asked('light').length;
  await sleep(150);
  assert.equal(fake.asked('light').length, sent, 'no loop is left running');
});

test('with the rates on, frames are spaced by the lamp\'s own light window', async () => {
  const times = [];
  const lamp = { ringPixels: 32, limits: { rates: { light: [{ most: 5, within_ms: 1000 }] }, hold_max_ms: 30_000 }, ring: async () => { times.push(Date.now()); } };
  const rainbow = startRainbow(lamp);
  await rainbow.first;
  await sleep(700);
  await rainbow.stop();
  assert.ok(times.length >= 3 && times.length <= 5, `${times.length} frames in 0.7 s`);
  for (let i = 1; i < times.length; i += 1) assert.ok(times[i] - times[i - 1] >= 215, `gap ${times[i] - times[i - 1]} ms`);
  assert.equal(rainbow.stats().leaseMs, 884);
});

// Only the newest loop lives, and "off" always finds it: two presses at once, or an off while the first frame is still out.
test('two rainbow_on at once leave one loop, and off stops it: nothing is sent after', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  const run = rowOf(lamp);
  await Promise.all([run('rainbow_on'), run('rainbow_on')]);
  await sleep(300);
  assert.match(await run('rainbow_off'), /^the rainbow ended: /);
  const after = fake.asked('light').length;
  await sleep(300);
  assert.equal(fake.asked('light').length, after, 'no second loop outlived the off');
  assert.equal(await run('rainbow_off'), 'the rainbow was not on');
});

test('off during the first frame finds the rainbow and stops it: nothing keeps sending', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  const run = rowOf(lamp);
  const on = run('rainbow_on');
  const off = await run('rainbow_off');
  assert.match(off, /^the rainbow ended: /, 'the off row found the rainbow that was still starting');
  assert.match(await on, /switched off before its first frame was answered|^lamp\.ring/);
  const after = fake.asked('light').length;
  await sleep(300);
  assert.equal(fake.asked('light').length, after, 'nothing is sent after the off');
});

test('a first frame the lamp cannot take clears the row, so a later on starts clean', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  const run = rowOf(lamp);
  fake.refuseNext('light', 'bad_request', 'no');
  await assert.rejects(run('rainbow_on'), /bad_request/);
  assert.equal(await run('rainbow_off'), 'the rainbow was not on');
  assert.match(await run('rainbow_on'), /^lamp\.ring/);
});

// docs/control.md: a touch ends a computer's light. A loop that sent again in 25 ms would leave the person no way to take the light back.
test('a touch on the lamp ends the rainbow and it stays ended, said once', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  const said = [];
  const loop = startRainbow(lamp, { say: (line) => said.push(line) });
  await loop.first;
  await sleep(150);
  fake.emit('touch', { kind: 'pet', where: 'head' });
  await loop.ended;
  assert.match(loop.stats().ended, /^touch:/);
  const after = fake.asked('light').length;
  await sleep(300);
  assert.equal(fake.asked('light').length, after, 'no frame after the touch');
  assert.equal(said.filter((l) => /touch/.test(l)).length, 1);
  // A later press starts a rainbow again, and the earlier touch does not end it.
  assert.match(await rowOf(lamp)('rainbow_on'), /^lamp\.ring/);
  await sleep(150);
  assert.ok(fake.asked('light').length > after, 'a new rainbow sends');
});

// The server's page-close and run-end paths call `stopRainbow`: it leaves nothing sending, and is harmless when nothing is on.
test('stopRainbow (the last page closing, the run ending) leaves nothing sending, and twice is fine', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await stopRainbow(); await lamp.close(); await fake.stop(); });
  await rowOf(lamp)('rainbow_on');
  await sleep(200);
  await stopRainbow();
  await stopRainbow();
  const after = fake.asked('light').length;
  await sleep(300);
  assert.equal(fake.asked('light').length, after);
});
