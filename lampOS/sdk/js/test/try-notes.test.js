// The try-it app's notes and held-light entries (twin/controls.js), in a file of their own so a row PR
// never appends to the tail of try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

// The notes and held-light entries (the notes door: grant `notes`, no lease): what each one keeps and says, and a
// refusal in the lamp's own words, against the SDK's fake lamp.
test('the notes entries keep, read, list and delete a note, and say the lamp\'s refusal', async (t) => {
  const fake = await FakeLamp.start({ grants: ['notes', 'light'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const run = (id, values = {}) => {
    const control = CONTROLS.find((c) => c.id === id);
    return control.run({ lamp, release: async () => {} }, checked(control.fields, values));
  };
  assert.equal(await run('notes_set', { key: 'try', value: '{"n": 1}' }), 'lamp.notesSet("try", {"n":1})');
  assert.equal(await run('notes_get', { key: 'try' }), 'lamp.notesGet("try") -> {"n":1}');
  assert.equal(await run('notes_get', { key: 'absent' }), 'lamp.notesGet("absent") -> null');
  assert.match(await run('notes_list'), /^lamp\.notesList\(""\) -> .*"try"/);
  assert.equal(await run('notes_delete', { key: 'try' }), 'lamp.notesDelete("try") -> true');
  assert.equal(await run('notes_delete', { key: 'try' }), 'lamp.notesDelete("try") -> false');
  // A value that is not JSON never reaches the lamp; a key the lamp does not take is refused in its words.
  await assert.rejects(run('notes_set', { value: 'not json' }), /value: JSON/);
  await assert.rejects(run('notes_set', { key: 'a key with spaces' }), (error) => error.code === 'bad_request' && /key: up to 64 bytes/.test(error.why));
});

test('the held light is kept until stopped, one at a time, and the stop says whether one was held', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const run = (id, values = {}) => {
    const control = CONTROLS.find((c) => c.id === id);
    return control.run({ lamp, release: async () => {} }, checked(control.fields, values));
  };
  assert.equal(await run('hold_light_stop'), 'held.stop() (none was held)');
  assert.equal(await run('hold_light', { colour: 'warm', level: 2 }), 'lamp.holdLight("warm", 2)');
  assert.equal(fake.asked('light').length, 1, 'asked at once');
  assert.deepEqual([fake.asked('light')[0].colour, fake.asked('light')[0].level], ['warm', 2]);
  assert.equal(await run('hold_light', { colour: 'blue', level: 3 }), 'lamp.holdLight("blue", 3)');
  assert.equal(await run('hold_light_stop'), 'held.stop() (a held light ended)');
  assert.equal(await run('hold_light_stop'), 'held.stop() (none was held)');
});
