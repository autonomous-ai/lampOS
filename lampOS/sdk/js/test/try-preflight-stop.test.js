// The pre-flight's stops and retries on a real-mode page: a Ctrl-C part-way, say's missing event, turn_end pressed again, --only.
// (Split from try-preflight.test.js; see test/preflight-page.js.)
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { runPreflight } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

test('a Ctrl-C part-way through the volume rows still puts the volume and the mute back', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  fake.state.volume = { level: 80, muted: false };
  const { url } = await page(t, { real: true, fake, home });
  const stop = new AbortController();
  // Stop after the mute row has run: the level and the mute are both changed by then.
  const log = (line) => { if (line.startsWith('call:volume_mute')) stop.abort(); };
  const { markdown, volume } = await runPreflight({ url, signal: stop.signal, log, settleMs: 60, expectMs: 300 });
  assert.notDeepEqual(volume.before, undefined);
  assert.deepEqual(fake.state.volume, { level: 80, muted: false }, 'put back after an early stop');
  assert.match(markdown, /volume: 80 before the first row; 80 after the last \(put back\)/);
});

test('say on a fake page: the result cell and the count say the expected event was not seen', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  const { markdown, rows } = await runPreflight({ url, settleMs: 60, expectMs: 400 });
  const say = rows.find((r) => r.id === 'call:say');
  assert.equal(say.result, 'PASS');
  assert.deepEqual(say.missing, ['lamp.said'], 'the fake lamp has no speech grant, so lamp.said never comes');
  assert.match(markdown, /\| call:say \| PASS \(expected event not seen: lamp\.said\) \|/);
  assert.match(markdown, /\d+ PASS \(\d+ with an expected event not seen\)/);
  assert.match(markdown, /\| call:gesture \| (PASS|n\/a|REFUSED)/);
});

test('turn_end is pressed again while the lamp says its turn has not begun, then passes; one that never begins is n/a, not FAIL', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'hear'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  // Two "not begun yet", then the lamp takes it.
  fake.autoDone = false;
  fake.refuseNext('turn.end', 'busy', "this computer's turn has not begun yet");
  fake.refuseNext('turn.end', 'busy', "this computer's turn has not begun yet");
  const late = await runPreflight({ url, only: ['call:turn_start', 'call:turn_end'], settleMs: 20, expectMs: 100, retryEveryMs: 50 });
  assert.deepEqual(late.rows.map((r) => r.result), ['PASS', 'PASS'], late.rows.map((r) => r.evidence).join(' | '));
  assert.equal(fake.asked('turn.end').length, 1, 'the lamp took the third press: the first two were its "not begun yet", pressed again');
  assert.equal(fake.refusals.get('turn.end').length, 0, 'both queued refusals were met');
  // Busy for good: after the wait, n/a with the lamp's words.
  for (let i = 0; i < 40; i += 1) fake.refuseNext('turn.end', 'busy', "this computer's turn has not begun yet");
  const never = await runPreflight({ url, only: ['call:turn_end'], settleMs: 20, expectMs: 100, retryMs: 400, retryEveryMs: 50 });
  assert.equal(never.rows[0].result, 'n/a');
  assert.match(never.rows[0].evidence, /^the turn never began \(the lamp said: busy: this computer's turn has not begun yet\)/);
  // Any other refusal of turn_end is not retried: the fake's "not this computer's turn" stays the turn being over.
  fake.refusals.delete('turn.end');
  fake.refuseNext('turn.end', 'refused', 'spoken');
  fake.refuseNext('turn.end', 'refused', 'spoken');
  const over = await runPreflight({ url, only: ['call:turn_end'], settleMs: 20, expectMs: 100, retryEveryMs: 50 });
  assert.equal(over.rows[0].result, 'n/a');
  assert.match(over.rows[0].evidence, /^the lamp's turn was over before end was pressed \(the lamp said: refused: spoken\)/);
  assert.equal(fake.refusals.get('turn.end').length, 1, 'not pressed again: the second queued refusal was never met');
});

test('--only presses just the named rows in the page\'s order, says so in the header, and refuses an id the page does not have', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'light', 'body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  const { rows, markdown } = await runPreflight({ url, only: ['call:say', 'call:light'], settleMs: 20, expectMs: 100 });
  assert.deepEqual(rows.map((r) => r.id), ['call:light', 'call:say'], 'the page\'s order, not the order given');
  assert.match(markdown, /\nonly: call:say, call:light\n/);
  assert.equal(fake.asked('say').length, 1);
  assert.equal(fake.asked('gesture').length, 0, 'nothing else was pressed');
  await assert.rejects(runPreflight({ url, only: ['call:say', 'call:nope'] }), /--only: no such row call:nope/);
  const bad = spawnSync(process.execPath, [new URL('../examples/twin/preflight.js', import.meta.url).pathname, '--url', url, '--only', 'call:nope'], { encoding: 'utf8' });
  assert.equal(bad.status, 2);
  assert.match(bad.stderr, /--only: no such row call:nope/);
  assert.equal(bad.stderr.includes(url), false, 'the url is not printed');
  assert.equal(bad.stderr.includes(new URL(url).searchParams.get('t')), false);
});
