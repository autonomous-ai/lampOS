// The pre-flight's turn_start when the lamp takes the call and then refuses the turn in its `done` (lamp-75c9, 13 Nov, the GUIDE
// row: say -> turn_start back to back, the say still playing, the row written PASS). (See test/preflight-page.js.)
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { runPreflight } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

// A `done` goes to the connection that asked, not to subscribers, as the lamp does it: told to every connection the fake holds.
const sendDone = (fake, frame) => { for (const ws of fake.wss.clients) ws.send(JSON.stringify({ ev: 'done', at: Date.now(), ...frame })); };

test('a turn the lamp took by the call and then refused in its done is not a PASS: pressed again while it says it is speaking, else n/a with its words', async (t) => {
  // lamp-75c9, 13 Nov: say -> turn_start back to back; the call was accepted and the done said how=dropped why="refused: the lamp is
  // speaking" (the say was still playing), and the row was written PASS.
  const fake = await FakeLamp.start({ grants: ['speak', 'hear'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  const refuse = { how: 'dropped', why: 'refused: the lamp is speaking' };
  // Only the first press is refused in its done: the second runs. (The refusing done comes 250 ms after the press: the twin's log
  // shows one `done` in 200 ms, and the fake's own `done` for the press comes first.)
  let told = 0;
  const watcher = setInterval(() => { const n = fake.asked('turn.start').length; if (n > told) { told = n; if (n === 1) setTimeout(() => sendDone(fake, { re: 90 + n, ...refuse }), 250); } }, 10);
  t.after(() => clearInterval(watcher));
  const late = await runPreflight({ url, only: ['call:turn_start'], settleMs: 700, expectMs: 100, retryMs: 5000, retryEveryMs: 100 });
  assert.equal(late.rows[0].result, 'PASS', late.rows[0].evidence);
  assert.equal(fake.asked('turn.start').length, 2, 'pressed again after the refusal');
  // Refused every time: after the wait, n/a with the lamp's words.
  clearInterval(watcher);
  let seen = fake.asked('turn.start').length;
  const everyTime = setInterval(() => { const n = fake.asked('turn.start').length; if (n > seen) { seen = n; setTimeout(() => sendDone(fake, { re: 200 + n, ...refuse }), 250); } }, 10);
  t.after(() => clearInterval(everyTime));
  const before = fake.asked('turn.start').length;
  const never = await runPreflight({ url, only: ['call:turn_start'], settleMs: 700, expectMs: 100, retryMs: 1500, retryEveryMs: 100 });
  assert.equal(never.rows[0].result, 'n/a');
  assert.match(never.rows[0].evidence, /^the lamp took the call and refused the turn \(the lamp said: refused: the lamp is speaking\)/);
  assert.ok(fake.asked('turn.start').length - before >= 2, 'pressed again while it was refused');
  // A plain turn (no refusing done) is a PASS as before, and turn_end's own done (dropped: ended by the computer) is not a refusal.
  clearInterval(everyTime);
  const plain = await runPreflight({ url, only: ['call:turn_start', 'call:turn_end'], settleMs: 60, expectMs: 100, retryMs: 400, retryEveryMs: 50 });
  assert.equal(plain.rows[0].result, 'PASS', plain.rows[0].evidence);
});

test('a retry of a refused turn that the lamp itself refuses is a FAIL with that answer, not "refused the turn" n/a', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'hear'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url } = await page(t, { real: true, fake, home });
  // The first press is accepted and then refused in its done; the pressed-again call is refused by the lamp (bad_request).
  let seen = 0;
  const watcher = setInterval(() => {
    const n = fake.asked('turn.start').length;
    if (n > seen) {
      seen = n;
      if (n === 1) { fake.refuseNext('turn.start', 'bad_request', 'no such turn'); setTimeout(() => sendDone(fake, { re: 77, how: 'dropped', why: 'refused: the lamp is speaking' }), 250); }
    }
  }, 10);
  t.after(() => clearInterval(watcher));
  const run = await runPreflight({ url, only: ['call:turn_start'], settleMs: 700, expectMs: 100, retryMs: 5000, retryEveryMs: 100 });
  assert.equal(run.rows[0].result, 'FAIL', run.rows[0].evidence);
  assert.match(run.rows[0].evidence, /bad_request: no such turn/);
  assert.doesNotMatch(run.rows[0].evidence, /refused the turn/);
});

test('the page goes away in the wait before the retry of a refused turn: the row keeps the lamp\'s refusal (n/a) and the run is cut short', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'hear'] });
  const home = mkdtempSync(join(tmpdir(), 'try-preflight-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const { url, child } = await page(t, { real: true, fake, home });
  let seen = 0;
  const watcher = setInterval(() => {
    const n = fake.asked('turn.start').length;
    if (n > seen) { seen = n; setTimeout(() => sendDone(fake, { re: 55, how: 'dropped', why: 'refused: the lamp is speaking' }), 250); setTimeout(() => child.kill('SIGKILL'), 450); }
  }, 10);
  t.after(() => clearInterval(watcher));
  // try.js is killed after the refusing done; its twin ends on the parent-gone tick inside the 6 s wait before the retry.
  const run = await runPreflight({ url, only: ['call:turn_start'], settleMs: 700, expectMs: 100, retryMs: 20_000, retryEveryMs: 6000 });
  assert.equal(run.cutShort, true);
  assert.equal(fake.asked('turn.start').length, 1, 'nothing was pressed again');
  assert.equal(run.rows[0].result, 'n/a');
  assert.match(run.rows[0].evidence, /^the lamp took the call and refused the turn \(the lamp said: refused: the lamp is speaking\)/);
});
