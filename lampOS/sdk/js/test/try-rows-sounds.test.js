// The try-it clip rows (twin/controls.js): upload, list and remove touch ONLY this run's scratch clip, against the SDK's
// fake lamp. Its own file, so a row PR never appends to try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked, cleanScratch, quietTone } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

test('the clip rows keep, list and remove only this run\'s scratch clip, and leave the lamp\'s other clips alone', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const run = (id, values = {}) => {
    const control = CONTROLS.find((c) => c.id === id);
    return control.run({ lamp, release: async () => {} }, checked(control.fields, values));
  };
  // A clip that was there before the run: the rows never remove it.
  await lamp.uploadSound('before-the-run', quietTone(200));
  assert.match(await run('sound_remove'), /^none of this run's clips is kept \(nothing was removed/);
  // A name other than a test clip's own is refused before any call; an earlier run's test clip is removed on its name.
  await lamp.uploadSound('try-0a1b2c', quietTone(200));
  const before = fake.requests.length;
  for (const name of ['hum', 'try-zzzzzz', 'try-0A1B2C', 'try-0a1b2c ', 'before-the-run', 'try-0a1b2c0']) {
    await assert.rejects(async () => run('sound_remove', { name }), /name: only this app's test clips/, name);
  }
  assert.equal(fake.requests.length, before, 'no call reached the lamp');
  assert.ok((await run('sound_list')).includes('try-0a1b2c 200 ms (an earlier run\'s test clip)'));
  assert.match(await run('sound_remove', { name: 'try-0a1b2c' }), /^lamp\.removeSound\("try-0a1b2c"\) -> "try-0a1b2c"/);
  assert.deepEqual((await lamp.sounds()).map((c) => c.name), ['before-the-run']);
  assert.deepEqual((await lamp.sounds()).map((c) => c.name), ['before-the-run']);
  const kept = await run('sound_upload', { ms: 300 });
  const name = /uploadSound\("(try-[0-9a-f]{6})"/.exec(kept)?.[1];
  assert.ok(name, kept);
  assert.match(name, /^[a-z][a-z0-9_-]{0,63}$/, 'a name the lamp takes');
  assert.match(kept, /a 300 ms quiet tone\) -> \{.*"ms":300/);
  const listed = await run('sound_list');
  assert.ok(listed.includes('before-the-run 200 ms') && listed.includes(`${name} 300 ms (this run's)`), listed);
  assert.match(await run('sound_remove'), new RegExp(`^lamp\\.removeSound\\("${name}"\\) -> "${name}"`));
  assert.deepEqual((await lamp.sounds()).map((c) => c.name), ['before-the-run'], 'only the scratch clip went');
  assert.match(await run('sound_remove'), /^none of this run's clips/, 'a second remove removes nothing');
  // The end of a run takes the clip off the lamp too, and only when this run put it there.
  assert.equal(await cleanScratch(lamp), false);
  await run('sound_upload');
  assert.equal(await cleanScratch(lamp), true);
  assert.deepEqual((await lamp.sounds()).map((c) => c.name), ['before-the-run']);
});

test('the clip rows carry their hints', () => {
  const hint = (id) => CONTROLS.find((c) => c.id === id).hint;
  assert.equal(hint('sound_list'), "clips named try-... are this app's test clips; remove them from the list");
  assert.ok(hint('sound_remove').length <= 200);
});

test('the quiet tone is a WAV the voice\'s loudness cap takes: mono 16-bit 24 kHz, peak under -12 dBFS, 10 ms RMS under -30 dBFS', () => {
  const wav = quietTone(300);
  assert.equal(wav.toString('ascii', 0, 4), 'RIFF');
  assert.equal(wav.readUInt16LE(22), 1);
  assert.equal(wav.readUInt32LE(24), 24_000);
  assert.equal(wav.readUInt16LE(34), 16);
  assert.equal(wav.readUInt32LE(40), 7200 * 2);
  let peak = 0;
  for (let at = 44; at < wav.length; at += 2) peak = Math.max(peak, Math.abs(wav.readInt16LE(at)));
  assert.ok(peak / 32768 < 10 ** (-12 / 20), `peak ${peak}`);
  const block = 240;
  for (let from = 0; from + block <= 7200; from += block) {
    let sum = 0;
    for (let k = 0; k < block; k += 1) sum += wav.readInt16LE(44 + (from + k) * 2) ** 2;
    assert.ok(Math.sqrt(sum / block) / 32768 < 10 ** (-30 / 20), `10 ms RMS at ${from}`);
  }
});
