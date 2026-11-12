// The try-it rows that read or set the lamp's own state (twin/controls.js): volume (put back), listening, a state read
// and the joints, against the SDK's fake lamp. Its own file, so a row PR never appends to try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

async function lampWith(t, grants) {
  const fake = await FakeLamp.start({ grants });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const run = (id, values = {}) => {
    const control = CONTROLS.find((c) => c.id === id);
    return control.run({ lamp, release: async () => {} }, checked(control.fields, values));
  };
  return { fake, lamp, run };
}

test('the volume row takes its step and puts the level and the mute back, for each choice', async (t) => {
  const { lamp, run } = await lampWith(t, ['speak']);
  const level = async () => (await lamp.get('volume')).volume;
  const start = await level();
  for (const how of ['step down', 'step up', 'mute']) {
    const said = await run('volume', { how });
    assert.match(said, /^lamp\.volume\(.*\) -> \{.*\}; put back: \{.*\} \(was /, how);
    assert.deepEqual(await level(), start, `${how}: the person's volume is as it was`);
  }
  // A muted lamp stays muted.
  await lamp.volume({ mute: true });
  const muted = await level();
  await run('volume', { how: 'step up' });
  assert.deepEqual(await level(), muted);
});

test('the volume row puts the level back even when the step is refused, and says so when the lamp will not tell its volume', async (t) => {
  const { lamp, run } = await lampWith(t, ['speak']);
  const start = (await lamp.get('volume')).volume;
  const real = lamp.volume.bind(lamp);
  lamp.volume = async (arg) => { if (arg.step) throw new Error('refused: test'); return real(arg); };
  await assert.rejects(run('volume', { how: 'step down' }), /refused: test/);
  assert.deepEqual((await lamp.get('volume')).volume, start);
  lamp.get = async () => ({});
  await assert.rejects(run('volume'), /did not say its volume: nothing was changed/);
});

test('the listening row holds and lifts the mute, the get row reads a state by name, and the joints row aims under the lease', async (t) => {
  const { fake, lamp, run } = await lampWith(t, ['hear', 'light', 'body', 'speak']);
  assert.match(await run('listen', { listen: 'off' }), /^lamp\.setListening\(false\) -> /);
  assert.match(await run('listen', { listen: 'on' }), /^lamp\.setListening\(true\) -> /);
  assert.match(await run('get', { state: 'lamp' }), /^lamp\.get\("lamp"\) -> \{/);
  assert.match(await run('get', { state: 'volume' }), /^lamp\.get\("volume"\) -> \{/);
  await assert.rejects(async () => run('get', { state: 'nonsense' }), /state: one of/);
  assert.equal(await run('joints', { yaw: 10, pitch: 0, within_ms: 1000 }), "lamp.withLease(['body'], (lease) => lamp.joints(lease, 1000, {\"yaw\":10,\"pitch\":0}, { wait: true }))");
  assert.equal(fake.asked('joints').length, 1);
  assert.deepEqual(fake.asked('joints')[0].targets, { yaw: 10, pitch: 0 });
  await assert.rejects(async () => run('joints', { yaw: 500 }), /yaw: a number from -90 to 90/);
  void lamp;
});

test('a row hint is a string of at most 200 characters, describe() carries it, and a real lamp may say it another way', async () => {
  const { describe } = await import('../examples/twin/controls.js');
  for (const entry of [...CONTROLS, ...(await import('../examples/twin/controls.js')).STIMULI]) {
    for (const key of ['hint', 'hintReal']) {
      if (entry[key] !== undefined) assert.ok(typeof entry[key] === 'string' && entry[key].length > 0 && entry[key].length <= 200, `${entry.id}.${key}`);
    }
    assert.ok(entry.hintReal === undefined || entry.hint !== undefined, `${entry.id}: hintReal needs a hint`);
  }
  const table = describe();
  const byId = (id) => table.controls.find((c) => c.id === id);
  assert.equal(byId('listen').hint, 'off leaves the lamp not listening until you press on');
  assert.equal(byId('volume').hint, 'steps the volume once and puts it back (level and mute)');
  assert.equal('hint' in byId('gesture'), false, 'a row without one carries none');
  assert.match(describe('real').controls.find((c) => c.id === 'listen').hint, /real lamp's mute/);
  assert.equal(describe('real').controls.find((c) => c.id === 'volume').hint, byId('volume').hint, 'no hintReal: the same hint');
  assert.deepEqual(Object.keys(table.controls[0]).sort(), ['fields', 'group', 'id', 'label'], 'ops, shows, run and hintReal stay off the page');
});
