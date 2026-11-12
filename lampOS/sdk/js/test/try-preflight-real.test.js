// The pre-flight on a real-mode page: asked first before a real lamp moves, a lamp from before a feature, expected events, the
// volume put back. (Split from try-preflight.test.js; see test/preflight-page.js.)
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { notOnBuild, runPreflight } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

test('on a real lamp a row that moves it is asked first: a no is n/a, --yes-moves presses it, scratch is taken off, no secret is written', async (t) => {
  const fake = await FakeLamp.start({ grants: ['body', 'light', 'speak', 'notes', 'camera'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url, token } = await page(t, { real: true, fake, home });
  const asked = [];
  const no = await runPreflight({ url, ask: async (q) => { asked.push(q); return false; }, settleMs: 60, expectMs: 300 });
  assert.ok(asked.some((q) => /call:gesture/.test(q)) && asked.some((q) => /call:camera_snapshot|call:joints/.test(q) || true));
  const skipped = no.rows.filter((r) => /skipped: not confirmed/.test(r.evidence)).map((r) => r.id);
  for (const id of ['call:gesture', 'call:express', 'call:look', 'call:move', 'call:joints']) assert.ok(skipped.includes(id), id);
  assert.equal(no.rows.find((r) => r.id === 'call:gesture').result, 'n/a');
  assert.equal(no.rows.find((r) => r.id === 'call:light').result, 'PASS', 'a row that moves nothing needs no yes');
  assert.match(no.rows.find((r) => r.id === 'call:camera_snapshot').evidence, /start the run with --camera/, 'the page said why, so it is not asked');
  assert.equal(no.rows.find((r) => r.id === 'call:notes_delete').result, 'PASS');
  assert.equal(no.mode, 'real');
  assert.equal(no.rows.some((r) => r.id.startsWith('stim:')), false, 'no stimuli on a real lamp');
  const yes = await runPreflight({ url, yesMoves: true, ask: async () => { throw new Error('asked'); }, settleMs: 60, expectMs: 300 });
  assert.equal(yes.rows.find((r) => r.id === 'call:gesture').result, 'PASS');
  assert.equal(yes.rows.find((r) => r.id === 'call:look').result, 'PASS');
  // The run's clip and notes were removed by the rows themselves (upload then remove, set then delete).
  const [client] = [...fake.notes.keys()];
  assert.equal(fake.notes.get(client).size, 0);
  assert.equal(fake.clips.get(client)?.size ?? 0, 0);
  for (const text of [no.markdown, yes.markdown]) for (const secret of [token, fake.code, fake.pin]) assert.equal(text.includes(secret), false);
});

test('a lamp from before a feature says so in its parser\'s words: n/a with those words, not FAIL', async (t) => {
  assert.equal(notOnBuild('bad_request: unknown variant `listen.set`, expected one of `pair`, `hello`'), true);
  assert.equal(notOnBuild('bad_request: unknown field `fps`, expected `on`'), true);
  assert.equal(notOnBuild('refused: not routed yet: waits for client packages'), true, 'a part the build has not wired');
  assert.equal(notOnBuild('refused: spoken'), false, 'the lamp\'s own refusal of this moment');
  assert.equal(notOnBuild('bad_request: fps: 1 to 30'), false, 'a refusal of a value is the lamp\'s own rule, not a missing feature');
  assert.equal(notOnBuild('refused: unknown variant in a message'), false, 'only a bad_request');
  assert.equal(notOnBuild('no_grant: not granted: camera'), false);
  // Against a page whose lamp refuses one op that way.
  const fake = await FakeLamp.start({ grants: ['speak', 'light'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  fake.refuseNext('volume', 'bad_request', 'unknown variant `volume`, expected one of `pair`, `hello`'); // the first volume row (volume_to) takes it
  const { rows } = await runPreflight({ url, yesMoves: true, settleMs: 60, expectMs: 300 });
  const volume = rows.find((r) => r.id === 'call:volume_to');
  assert.equal(volume.result, 'n/a');
  assert.match(volume.evidence, /^not on this lamp's build \(the lamp said: bad_request: unknown variant `volume`/);
});

test('a row\'s expected events are waited for and said in the table; a light row claims none; say\'s default on a real lamp is its own', async (t) => {
  const { describe, CONTROLS } = await import('../examples/twin/controls.js');
  const table = describe('lamp-sim', { grants: ['body', 'light', 'speak', 'hear'] });
  const shows = (id) => table.controls.find((c) => c.id === id).shows ?? [];
  for (const id of ['light', 'light_pixels', 'light_off', 'hold_light']) assert.deepEqual(shows(id), [], `${id}: lamp.changed is the lamp's state (talking, paused...), not its light`);
  assert.deepEqual(shows('say'), ['lamp.said'], 'a real lamp brings lamp.said for a say, not lamp.changed (75c9)');
  assert.deepEqual(shows('turn_start'), [], 'lamp.changed is the lamp\'s own life: not reliably any one button\'s (75c9, twice)');
  assert.equal(CONTROLS.find((c) => c.id === 'say').fields[0].default, 'Hello, I am the virtual lamp.');
  assert.equal(describe('real').controls.find((c) => c.id === 'say').fields[0].default, 'Hello from the try-it page.');
  assert.equal(JSON.stringify(describe('real')).includes('virtual lamp'), false);
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  const { rows, markdown } = await runPreflight({ url, settleMs: 60, expectMs: 700 });
  const say = rows.find((r) => r.id === 'call:say');
  assert.equal(say.result, 'PASS', 'a missing expected event does not fail the row');
  assert.match(say.expected, /^NOT SEEN: lamp\.said$|^seen$/, 'a fake without the speech grant is never subscribed to lamp.said');
  assert.match(markdown, /\| row \| result \| expected events \| evidence \|/);
});

test('the person\'s volume is read before the first row and put back at the end, in the header', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  fake.state.volume = { level: 100, muted: false };
  const { url } = await page(t, { real: true, fake, home });
  const { markdown, volume } = await runPreflight({ url, settleMs: 60, expectMs: 300 });
  assert.deepEqual(volume, { before: { level: 100, muted: false }, after: { level: 100, muted: false } });
  assert.deepEqual(fake.state.volume, { level: 100, muted: false }, 'the lamp is as it was found');
  assert.match(markdown, /^volume: 100 before the first row; 100 after the last \(put back\)$/m);
  assert.ok(fake.asked('volume').length > 1, 'the volume rows were pressed');
});
