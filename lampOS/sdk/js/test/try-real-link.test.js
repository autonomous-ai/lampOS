// The real-lamp page keeps its connection to the lamp (examples/twin/link.js): when the lamp restarts, the page is told why at once, the
// server connects again, and the next row works: before this, one drop left every row saying "closed" until the page was restarted.
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';
import { raw, run } from './try-real-helper.js';

test('a restart of the lamp is said to the page, the server connects again and the next row works', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light', 'body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  const kept = join(home, 'kept');
  mkdirSync(kept, { mode: 0o700 });
  chmodSync(kept, 0o700);
  await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'mine', keyring: new Keyring(kept) });
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--keyring', kept], env: { LAMP_TRY_CODE: '', LAMP_TRY_PIN: '' }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  page.send({ type: 'moves_ok' });
  const first = await page.press('light', { colour: 'warm', level: 2 });
  assert.equal(first.ok, true, JSON.stringify(first));
  fake.restart();
  const said = async (pattern, what) => {
    for (let i = 0; i < 200; i += 1) { const found = page.messages.find((m) => m.type === 'said' && pattern.test(m.text)); if (found) return found; await sleep(50); }
    assert.fail(`never said: ${what}: ${JSON.stringify(page.messages.filter((m) => m.type === 'said').slice(-4))}`);
  };
  await said(/the lamp's connection ended: the lamp restarts; reconnecting/, 'why it ended');
  await said(/the lamp's connection is back/, 'that it is back');
  const after = await page.press('light', { colour: 'blue', level: 2 });
  assert.equal(after.ok, true, JSON.stringify(after));
  assert.equal(/closed/.test(real.out.text) && /the connection to the lamp closed\b(?!:)/.test(JSON.stringify(after)), false);
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
});
