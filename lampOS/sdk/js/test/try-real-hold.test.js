// A pose set with a slider on a real lamp is kept past the old 3 s: the body lease is held after the last drag until HOLD_MS (25 s), so the
// lamp's own pose does not come straight back. (The 25 s itself is not waited for here; the check is that the old 3 s is over and it is held.)
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { raw, run } from './try-real-helper.js';

test('the slider\'s lease is still held 4 s after the last drag, and a let_go gives it back at once', async (t) => {
  const fake = await FakeLamp.start({ name: 'Desk lamp', grants: ['body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  page.send({ type: 'moves_ok' });
  page.send({ type: 'drag', joint: 'pitch', deg: 3, final: true });
  for (let i = 0; i < 100 && fake.asked('joints').length < 1; i += 1) await new Promise((r) => setTimeout(r, 50));
  assert.equal(fake.leases.size, 1, 'the drag took the body lease');
  await new Promise((r) => setTimeout(r, 4000));
  assert.equal(fake.leases.size, 1, 'it is still held after the old 3 s');
  page.send({ type: 'let_go' });
  for (let i = 0; i < 100 && fake.leases.size > 0; i += 1) await new Promise((r) => setTimeout(r, 50));
  assert.equal(fake.leases.size, 0, 'a let_go gives it back at once');
});
