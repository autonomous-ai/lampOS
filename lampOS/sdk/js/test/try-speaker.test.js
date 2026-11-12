// The try-it app's "Speaker tone" row (twin/controls.js `speaker_tone`), in a file of its own so a row PR never
// appends to the tail of try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked, describe } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

const row = CONTROLS.find((c) => c.id === 'speaker_tone');
const run = (lamp) => row.run({ lamp, release: async () => {} }, checked(row.fields, {}));
const until = async (ready) => { for (let i = 0; i < 200 && !ready(); i++) await new Promise((r) => setTimeout(r, 25)); };

test('the speaker row is in the table the page is drawn from', () => {
  assert.ok(row);
  assert.deepEqual(describe().controls.find((c) => c.id === 'speaker_tone'), { id: 'speaker_tone', group: row.group, label: row.label, fields: [] });
});

test('the speaker row plays 1 s of 440 Hz at -20 dBFS and the log gets the lamp\'s stats and its end', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speaker'] });
  // The lamp's own times are 2 s; shorter here so the test says what it means in less time.
  fake.speakerStatsMs = 300;
  fake.speakerIdleMs = 800;
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const events = [];
  lamp.on('speaker.stats', (e) => events.push(e.ev));
  lamp.on('speaker.ended', (e) => events.push(`${e.ev}:${e.why}`));
  const said = await run(lamp);
  assert.match(said, /^lamp\.speakerStream\(16000\)\.playPcm\(440 Hz, 1 s\) \/\/ 1000 ms sent$/);
  const played = fake.played();
  assert.equal(played.length, 32000, '1 s of 16 kHz mono S16LE');
  let peak = 0;
  for (let n = 0; n < played.length / 2; n += 1) peak = Math.max(peak, Math.abs(played.readInt16LE(n * 2)));
  const dbfs = 20 * Math.log10(peak / 32768);
  assert.ok(dbfs > -20.5 && dbfs <= -19.9, `peak ${dbfs.toFixed(2)} dBFS`);
  await until(() => events.includes('speaker.ended:idle'));
  assert.ok(events.includes('speaker.stats'), events.join(','));
  assert.ok(events.includes('speaker.ended:idle'), events.join(','));
  assert.equal(fake.speaker, null, 'the stream is stopped at the end');
});

test('the speaker row says the lamp\'s refusal in its words', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  await assert.rejects(run(lamp), (error) => error.code === 'no_grant' && /^not granted: speaker/.test(error.why));
  const granted = await FakeLamp.start({ grants: ['speaker'] });
  const other = await granted.connect();
  t.after(async () => { await other.close(); await granted.stop(); });
  granted.setPrivate();
  await assert.rejects(run(other), (error) => error.code === 'private' && error.why === 'the lamp is private');
  assert.equal(granted.played().length, 0);
});
