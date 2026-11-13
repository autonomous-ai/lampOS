// A real run paired for speech alone (no body grant): the twin used to die on `body.state` with a stack (lamp-75c9, 13 Nov, the
// GUIDE row). Its own file: try-real.test.js is near the runner's 60 s limit for one file.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { runPreflight } from '../examples/twin/preflight.js';
import { page } from './preflight-page.js';

test('a real run paired for speech alone starts, draws the body rows off as the page\'s own refusal, and still says a line', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak', 'hear', 'speech'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-nobody-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  // Without the fix the twin ends with NoGrantError (body.state) and page() throws "ended 1" before it has a URL.
  const { url } = await page(t, { real: true, fake, home });
  const run = await runPreflight({ url, only: ['call:say', 'call:gesture'], settleMs: 60, expectMs: 300 });
  const row = (id) => run.rows.find((r) => r.id === id);
  assert.equal(row('call:say').result, 'PASS', 'the say row works on a speech pairing');
  assert.equal(row('call:gesture').result, 'REFUSED', 'a body row is drawn off for the missing grant');
  assert.match(row('call:gesture').evidence, /did not grant body/);
});
