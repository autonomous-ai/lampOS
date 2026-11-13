// The pre-flight when the run is cut short: a Ctrl-C at the y/N question, the page going away (between rows, at a question, in the
// turn_end retry wait), and the exit code. (Split from try-preflight-stop.test.js; see test/preflight-page.js.)
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { askYesNo, exitCodeOf, runPreflight, within } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

test('turn_end pressed once and answered "not begun", then the page goes away in the wait before the retry: it WAS pressed, and the run is cut short', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'hear'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url, child } = await page(t, { real: true, fake, home });
  for (let i = 0; i < 40; i += 1) fake.refuseNext('turn.end', 'busy', "this computer's turn has not begun yet");
  // try.js is killed once the first press is answered; its twin ends on the parent-gone tick inside the 6 s wait.
  const kill = setTimeout(() => child.kill('SIGKILL'), 800);
  t.after(() => clearTimeout(kill));
  const run = await runPreflight({ url, only: ['call:turn_end'], settleMs: 20, expectMs: 100, retryMs: 20_000, retryEveryMs: 6000 });
  assert.equal(run.rows.length, 1);
  assert.doesNotMatch(run.rows[0].evidence, /^not pressed/, 'the row was pressed once');
  assert.match(run.rows[0].evidence, /^the turn never began \(the lamp said: busy: this computer's turn has not begun yet\)/);
  assert.equal(run.cutShort, true);
  assert.equal(exitCodeOf({ aborted: false, interrupted: false, cutShort: run.cutShort, rows: run.rows }), 1);
});

// A terminal that is not one: readline treats an input with isTTY (and an output with isTTY) as a terminal and takes ^C as its own key.
const fakeTerminal = () => {
  const input = new PassThrough();
  input.isTTY = true;
  input.setRawMode = () => input;
  const output = new PassThrough();
  output.isTTY = true;
  output.resume();
  return { input, output };
};

test('a Ctrl-C at the y/N question answers null (the run\'s stop), y is yes, anything else and Ctrl-D are no', async () => {
  for (const [typed, want] of [['\x03', null], ['y\n', true], ['yes\n', true], ['n\n', false], ['\n', false], ['\x04', false]]) {
    const { input, output } = fakeTerminal();
    const answer = askYesNo('Press it?', { input, output });
    input.write(typed);
    assert.equal(await within(answer, 3000, 'unanswered'), want, JSON.stringify(typed));
  }
});

test('a Ctrl-C at a question stops the run, presses nothing more, takes the scratch off and says so in the header', async (t) => {
  const fake = await FakeLamp.start({ grants: ['body', 'notes', 'speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  fake.state.volume = { level: 70, muted: false };
  const { url } = await page(t, { real: true, fake, home });
  const asked = [];
  const run = await runPreflight({ url, ask: async (q) => { asked.push(q); return asked.length === 2 ? null : true; }, settleMs: 60, expectMs: 300 });
  assert.equal(asked.length, 2, 'no question after the Ctrl-C');
  assert.equal(run.interrupted, true);
  const questioned = /call:\w+/.exec(asked[1])[0];
  assert.equal(run.rows.some((r) => r.id === questioned), false, 'the row asked about at the Ctrl-C was not pressed');
  assert.ok(run.rows.length < 12, `nothing after it was pressed (${run.rows.length} rows)`);
  assert.match(run.markdown, /^ended early: stopped by a Ctrl-C at a question; the rest were not pressed$/m);
  assert.deepEqual(fake.state.volume, { level: 70, muted: false });
});

test('a page that goes away mid-run ends the run at once: no row waits out its answer time', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'notes'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url, child } = await page(t, { real: true, fake, home });
  const started = Date.now();
  let killed = false;
  // try.js is killed; the twin it started notices its parent is gone (a 2 s tick) and ends, closing the page's socket mid-run.
  const run = await runPreflight({ url, log: () => { if (!killed) { killed = true; child.kill('SIGKILL'); } }, settleMs: 300, expectMs: 300, answerMs: 20_000 });
  assert.ok(Date.now() - started < 15_000, `ended in ${Date.now() - started} ms`);
  assert.equal(run.cutShort, true, 'the close was seen before the rows were done');
  assert.match(run.markdown, /^ended early: the connection to the page closed after \d+ rows; the rest were not pressed$/m);
  for (const row of run.rows) {
    if (/^pressed;/.test(row.evidence)) assert.equal(row.result, 'FAIL', `${row.id}: sent, unanswered`);
    if (/^not pressed:/.test(row.evidence)) assert.equal(row.result, 'n/a', `${row.id}: never sent`);
  }
  assert.equal(exitCodeOf({ aborted: false, interrupted: run.interrupted, cutShort: run.cutShort, rows: run.rows }), 1, 'a cut-short run never exits 0');
});

test('a page that goes away while the person sits at a question: the row is n/a (not pressed), not left to wait out its answer time', async (t) => {
  const fake = await FakeLamp.start({ grants: ['body', 'speak', 'notes'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url, child } = await page(t, { real: true, fake, home });
  const started = Date.now();
  // The person's yes comes only after the page is gone (try.js killed; its twin ends on the parent-gone tick).
  const ask = async () => { child.kill('SIGKILL'); await new Promise((resolve) => setTimeout(resolve, 6000)); return true; };
  const run = await runPreflight({ url, ask, settleMs: 60, expectMs: 300, answerMs: 40_000 });
  assert.ok(Date.now() - started < 25_000, `ended in ${Date.now() - started} ms, not a 40 s wait`);
  assert.equal(run.cutShort, true);
  const last = run.rows.at(-1);
  // The page was already gone when the person said yes: the row went nowhere, and the table says so.
  assert.equal(last.result, 'n/a');
  assert.match(last.evidence, /^not pressed: the connection to the page had closed/);
  assert.equal(exitCodeOf({ aborted: false, interrupted: false, cutShort: run.cutShort, rows: run.rows }), 1);
});

test('the exit code: 130 for a Ctrl-C, 1 for a FAIL or a run the page cut short, 0 for a whole clean run', () => {
  const ok = [{ result: 'PASS' }, { result: 'REFUSED' }, { result: 'n/a' }];
  assert.equal(exitCodeOf({ aborted: false, interrupted: false, cutShort: false, rows: ok }), 0);
  assert.equal(exitCodeOf({ aborted: false, interrupted: false, cutShort: false, rows: [...ok, { result: 'FAIL' }] }), 1);
  assert.equal(exitCodeOf({ aborted: false, interrupted: false, cutShort: true, rows: ok }), 1, '3 of 40 rows pressed is not a pass');
  assert.equal(exitCodeOf({ aborted: true, interrupted: false, cutShort: false, rows: ok }), 130);
  assert.equal(exitCodeOf({ aborted: false, interrupted: true, cutShort: true, rows: ok }), 130);
});

test('with stderr not a terminal, the process\'s own abort answers the question (readline raises no SIGINT of its own there)', async () => {
  const input = new PassThrough();
  input.isTTY = true;
  input.setRawMode = () => input;
  const output = new PassThrough(); // isTTY unset: readline takes no terminal mode
  output.resume();
  const stop = new AbortController();
  const answer = askYesNo('Press it?', { input, output, signal: stop.signal });
  setTimeout(() => stop.abort(), 50);
  assert.equal(await within(answer, 3000, 'unanswered'), null);
  assert.equal(await askYesNo('again?', { input: Object.assign(new PassThrough(), { isTTY: true, setRawMode() {} }), output, signal: stop.signal }), null, 'already aborted: no question is asked');
});
