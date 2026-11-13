// The controller pre-flight (twin/preflight.js): it presses every enabled row of a running page with its defaults and writes
// PASS / FAIL / n/a with the evidence, and never prints or writes the page's token. (The real-lamp runs are in
// try-preflight-real.test.js, the stops and the turn_end retry in try-preflight-stop.test.js.)
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { CONTROLS } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';
import { expectedEvents, resultText, runPreflight, tableOf } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

test('the table says the counts and each row once, a pipe in the evidence cannot break a line', () => {
  const md = tableOf({ lamp: { id: 'l1', name: 'Desk' }, mode: 'real', at: '2026-11-12T02:00:00Z', rows: [
    { id: 'call:gesture', result: 'PASS', evidence: 'lamp.gesture("nod"); events: body.state' },
    { id: 'call:say', result: 'FAIL', evidence: 'refused: a | b' },
    { id: 'call:camera_snapshot', result: 'n/a', evidence: 'start the run with --camera' },
  ] });
  assert.match(md, /^# Controller pre-flight: Desk \(l1, real\) 2026-11-12T02:00:00Z/);
  assert.match(md, /1 PASS, 1 FAIL, 0 refused as designed, 1 n\/a, of 3 rows/, 'a plain PASS count when every claim was seen');
  assert.match(md, /\| call:say \| FAIL \| {2}\| refused: a \\\| b \|/);
});

test('on the fake twin every enabled row is pressed once with its defaults and none is left without a result', async (t) => {
  const { url, token } = await page(t);
  const lines = [];
  const { rows, markdown } = await runPreflight({ url, log: (l) => lines.push(l), settleMs: 80, expectMs: 400 });
  const { describe } = await import('../examples/twin/controls.js');
  const table = describe('fake', { grants: ['body', 'light', 'speak', 'look', 'notes', 'camera', 'skills'] });
  assert.equal(rows.length, table.controls.length + table.stimuli.length, 'one line per row of the page');
  assert.ok(rows.filter((r) => r.result === 'PASS').length >= 25, 'most rows pass');
  for (const row of rows) assert.ok(['PASS', 'FAIL', 'REFUSED', 'n/a'].includes(row.result) && row.evidence.length > 0, `${row.id}: ${row.result} has evidence`);
  assert.equal(rows.find((r) => r.id === 'call:microphone_stream').result, 'REFUSED', 'the fake twin has no microphone grant: refused as designed, said, not hidden');
  assert.match(rows.find((r) => r.id === 'call:microphone_stream').evidence, /did not grant microphone/);
  assert.match(rows.find((r) => r.id === 'call:microphone_stream').evidence, /^refused by the page, from the grants the lamp named in hello/, 'whose refusal it is');
  assert.ok(rows.some((r) => /; events: .*done/.test(r.evidence)), `an event the page logged is the evidence: ${rows.map((r) => r.evidence).join(' | ').slice(0, 300)}`);
  // The only FAIL the fake gives is its own words for ending a turn that is not this computer's.
  assert.deepEqual(rows.filter((r) => r.result === 'FAIL').map((r) => r.id), [], 'no unexplained FAIL');
  assert.equal(rows.find((r) => r.id === 'call:turn_end').result, 'n/a');
  assert.match(rows.find((r) => r.id === 'call:turn_end').evidence, /^the lamp's turn was over before end was pressed \(the lamp said: busy: not this computer's turn\)/);
  assert.equal(rows.find((r) => r.id === 'stim:hear').result, 'REFUSED', 'a phrase needs the hear grant, which this lamp lacks: refused as designed, not a failure');
  assert.equal(`${markdown}\n${lines.join('\n')}`.includes(token), false, 'the token is in no output');
});

test('a row the lamp did not grant is REFUSED (as designed), exit 0; a refusal for another reason is FAIL, exit 1', async (t) => {
  const { grantRefusal } = await import('../examples/twin/preflight.js');
  // The words: the lamp's no_grant for a grant this pairing lacks is the design; for one it holds, or any other code, it is not.
  assert.equal(grantRefusal('no_grant: not granted: camera', ['body', 'speak']), true);
  assert.equal(grantRefusal('no_grant: not granted: speak, hear', ['body', 'speak']), true, 'one of a list is missing');
  assert.equal(grantRefusal('no_grant: not granted: speak', ['body', 'speak']), false, 'the pairing HAS it: that is a real refusal');
  // The lamp's own long line for the camera and the microphone names the grant first and goes on.
  const long = 'no_grant: not granted: camera (only the owner can add it: with a new pairing code that names it, or by changing the grants of this computer)';
  assert.equal(grantRefusal(long, ['body', 'speak']), true, 'a pairing without camera: refused as designed');
  assert.equal(grantRefusal(long, ['body', 'camera']), false, 'a pairing WITH camera and the same answer is a real FAIL');
  assert.equal(grantRefusal('refused: spoken', ['body']), false);
  assert.equal(grantRefusal('bad_request: fps: 1 to 30', ['body']), false);
  assert.equal(grantRefusal('no_grant: not granted: camera', undefined), true, 'a page that does not say its grants: by design');
  const fake = await FakeLamp.start({ grants: ['body', 'light', 'speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  // Not spawnSync: the fake lamp lives in this process and must keep answering while the driver runs.
  const cli = (args) => new Promise((resolve) => {
    const child = spawn(process.execPath, [new URL('../examples/twin/preflight.js', import.meta.url).pathname, '--url', url, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (c) => { stdout += c; });
    child.stderr.on('data', (c) => { stderr += c; });
    child.on('close', (status) => resolve({ status, stdout, stderr }));
  });
  // No camera, microphone or speaker grant: three REFUSED rows and a plain exit 0.
  const refused = await cli(['--only', 'call:camera_snapshot,call:microphone_stream,call:speaker_tone,call:language']);
  assert.equal(refused.status, 0, refused.stdout + refused.stderr);
  assert.match(refused.stdout, /\| call:camera_snapshot \| (REFUSED|n\/a) /);
  assert.match(refused.stdout, /\| call:microphone_stream \| REFUSED \|.*did not grant microphone/);
  assert.match(refused.stdout, /\| call:speaker_tone \| REFUSED \|.*did not grant speaker/);
  assert.match(refused.stdout, /1 PASS, 0 FAIL, [23] refused as designed, [01] n\/a, of 4 rows/);
  // A real refusal for another reason is a FAIL: exit 1.
  fake.refuseNext('gesture', 'bad_request', 'something is wrong');
  const failed = await cli(['--only', 'call:gesture', '--yes-moves']);
  assert.equal(failed.status, 1, failed.stdout + failed.stderr);
  assert.match(failed.stdout, /\| call:gesture \| FAIL \|.*something is wrong/);
  assert.match(failed.stdout, /0 PASS, 1 FAIL, 0 refused as designed, 0 n\/a, of 1 rows/);
});

test('the events a row says it shows are checked: seen, or NOT SEEN, and the result stays as it was', () => {
  assert.equal(expectedEvents(undefined, ['x']), '');
  assert.equal(expectedEvents([], ['x']), '');
  assert.equal(expectedEvents(['lamp.said', 'lamp.changed'], ['lamp.said', 'lamp.changed', 'done']), 'seen');
  assert.equal(expectedEvents(['lamp.said', 'lamp.changed'], ['lamp.said']), 'NOT SEEN: lamp.changed');
});

test('a PASS whose expected event was not seen says so in the cell and the count; all seen stays a plain PASS', () => {
  assert.equal(resultText({ result: 'PASS', missing: [] }, 'real'), 'PASS');
  assert.equal(resultText({ result: 'FAIL', missing: ['x'] }, 'real'), 'FAIL', 'only a PASS carries it');
  assert.equal(resultText({ result: 'PASS', missing: ['lamp.said'] }), 'PASS (expected event not seen: lamp.said)');
  assert.equal(resultText({ result: 'PASS', missing: ['lamp.said', 'phrase.heard'] }), 'PASS (expected event not seen: lamp.said, phrase.heard)');
  const rows = [
    { id: 'call:say', result: 'PASS', missing: ['lamp.said'], expected: 'NOT SEEN: lamp.said', evidence: 'lamp.say("x")' },
    { id: 'call:gesture', result: 'PASS', missing: [], expected: '', evidence: 'lamp.gesture("nod")' },
    { id: 'stim:touch', result: 'PASS', missing: [], expected: 'seen', evidence: 'lamp-sim touch tap' },
  ];
  const md = tableOf({ lamp: { id: 'v', name: 'V' }, mode: 'lamp-sim', at: 'now', rows });
  assert.match(md, /3 PASS \(1 with an expected event not seen\), 0 FAIL, 0 refused as designed, 0 n\/a, of 3 rows/);
  assert.match(md, /\| call:say \| PASS \(expected event not seen: lamp\.said\) \| NOT SEEN: lamp\.said \|/);
  assert.match(md, /\| call:gesture \| PASS \| {2}\|/);
  assert.match(md, /\| stim:touch \| PASS \| seen \|/);
});

test('a wrong token says the page refused it, and the message holds neither the url nor the token', async (t) => {
  const { url, token } = await page(t);
  const wrong = url.replace(token, 'not-the-token');
  await assert.rejects(runPreflight({ url: wrong }), (error) => /answered 4\d\d for its table/.test(error.message) && !error.message.includes('not-the-token'));
});

test('follow_hand and volume_mute claim no event they cannot be told to show', () => {
  // follow_hand puts no hand in view (the `hand` stimulus does); volume_mute's default (off) changes nothing on a lamp that is not muted.
  for (const id of ['follow_hand', 'volume_mute']) assert.deepEqual(CONTROLS.find((c) => c.id === id).shows, [], id);
  assert.deepEqual(CONTROLS.find((c) => c.id === 'volume').shows, ['volume.changed'], 'a row whose default does change keeps its claim');
});
