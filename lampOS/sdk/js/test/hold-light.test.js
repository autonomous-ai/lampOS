// holdLight keeps a light on past one ask's 30 s, and stops when told.
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

async function withLamp(body) {
  const fake = await FakeLamp.start();
  const keyring = new Keyring(mkdtempSync(join(tmpdir(), 'lampos-hold-')));
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try { await body(lamp, fake); } finally { await lamp.close(); }
  } finally { await fake.stop(); }
}

test('holdLight asks again at half the hold limit until stopped', async () => {
  await withLamp(async (lamp, fake) => {
    lamp.limits.hold_max_ms = 2000; // as a lamp that holds a light 2 s, so the test needn't wait 15 s
    const holding = await lamp.holdLight('warm', 3);
    assert.equal(fake.asked('light').length, 1);
    assert.equal(fake.asked('light')[0].for_ms, 2000);
    await new Promise((r) => setTimeout(r, 2300));
    holding.stop();
    const asks = fake.asked('light').length;
    assert.ok(asks >= 3, `asked ${asks} times`);
    await new Promise((r) => setTimeout(r, 1200));
    assert.equal(fake.asked('light').length, asks);
  });
});

test('a refused renewal ends the holding and goes to onError', async () => {
  await withLamp(async (lamp, fake) => {
    lamp.limits.hold_max_ms = 2000;
    let failure;
    const holding = await lamp.holdLight('warm', 3, undefined, { onError: (e) => { failure = e; } });
    fake.refuses.light = 'test';
    await new Promise((r) => setTimeout(r, 1500));
    holding.stop();
    assert.ok(failure, 'onError was called');
  });
});
