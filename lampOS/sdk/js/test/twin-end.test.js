// How a real run's twin ends (twin/twin-end.js): one clean-up however many asks; a third ask ends at once; the watch for a dead
// parent is one ask, not a stream of them (it used to call the end every 2 s, and the third call exited 1 inside the clean-up).
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { endHandler, watchParent } from '../examples/twin/twin-end.js';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test('a dead parent starts ONE clean-up: the watch stops at its first call, and a slow clean-up still ends with exit 0', async () => {
  const exits = [];
  let cleanups = 0;
  const end = endHandler({ exit: (code) => exits.push(code), cleanup: async () => { cleanups += 1; await sleep(120); }, mostMs: 5000 });
  let ppid = 111;
  const stop = watchParent({ parent: 111, ppid: () => ppid, end, everyMs: 10 });
  ppid = 1; // try.js died without a word
  await sleep(350); // 35 ticks of the old code: its third would have exited 1 inside the clean-up
  stop();
  assert.equal(cleanups, 1);
  assert.deepEqual(exits, [0], 'one clean exit, no exit 1');
});

test('a second ask during the clean-up does not cut it short; a third does, with exit 1', async () => {
  const exits = [];
  const end = endHandler({ exit: (code) => exits.push(code), cleanup: async () => { await sleep(80); }, mostMs: 5000 });
  const first = end();
  await end();
  assert.deepEqual(exits, [], 'the second ask waits for the clean-up');
  await end();
  assert.deepEqual(exits, [1], 'the third ask ends at once');
  await first;
});

test('a clean-up past its limit exits 1', async () => {
  const exits = [];
  const end = endHandler({ exit: (code) => exits.push(code), cleanup: () => sleep(200), mostMs: 30 });
  await end();
  assert.ok(exits.includes(1), 'the limit fired');
});
