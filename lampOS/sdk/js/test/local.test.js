// The control socket for a skill's run: `Lamp.local(token)` against the fake lamp's unix socket.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Lamp, byCode } from 'lampos';
import { FakeLamp } from 'lampos/testing';
import { redact } from '../src/debug.js';

async function withLocal(grants, body) {
  const fake = await FakeLamp.start({ local: true, grants });
  try { await body(fake); } finally { await fake.stop(); }
}

test('a skill\'s run connects on the control socket with its token: its grants and no more, its run, refused without a good one', async () => {
  await withLocal(['body', 'light'], async (fake) => {
    const token = fake.mintToken(['light'], 'r1', 'water');
    const lamp = await Lamp.local(token, { path: fake.localPath });
    assert.deepEqual([lamp.grants, lamp.run, lamp.skill], [['light'], 'r1', 'water']);
    assert.equal((await lamp.light('warm', 3, 1000)).to, 'ring');
    await assert.rejects(lamp.gesture('nod'), (error) => error instanceof byCode.no_grant && /not granted: body|needs body/.test(error.why));
    const closed = lamp.closed();
    fake.endRun('r1');
    await closed;
    assert.equal(lamp.closeReason, 'the run ended');
    for (const bad of ['A'.repeat(43), token]) {
      await assert.rejects(Lamp.local(bad, { path: fake.localPath }), (error) => error instanceof byCode.no_grant && /no token for this run/.test(error.why));
    }
    const saved = { ...process.env };
    try {
      delete process.env.LAMP_CONTROL_TOKEN;
      await assert.rejects(Lamp.local(undefined, { path: fake.localPath }), (error) => /LAMP_CONTROL_TOKEN/.test(error.message));
      process.env.LAMP_CONTROL_TOKEN = fake.mintToken(['body'], 'r2');
      process.env.LAMP_CONTROL_SOCKET = fake.localPath;
      const viaEnv = await Lamp.local();
      assert.deepEqual([viaEnv.grants, viaEnv.run], [['body'], 'r2']);
      await viaEnv.close();
    } finally { process.env = saved; }
    // A second mint for one run replaces the first.
    const first = fake.mintToken(['body'], 'r3');
    const second = fake.mintToken(['light'], 'r3');
    await assert.rejects(Lamp.local(first, { path: fake.localPath }), (error) => error instanceof byCode.no_grant);
    const third = await Lamp.local(second, { path: fake.localPath });
    assert.deepEqual(third.grants, ['light']);
    await third.close();
  });
});

test('the control socket is not for pairing, and a token is never in the debug trace', async () => {
  await withLocal(['light'], async (fake) => {
    const { WebSocket } = await import('ws');
    const ws = new WebSocket(`ws+unix://${fake.localPath}:/v1`);
    const frames = [];
    ws.on('message', (data) => frames.push(JSON.parse(data.toString())));
    await new Promise((resolve, reject) => { ws.on('open', resolve); ws.on('error', reject); });
    ws.send(JSON.stringify({ id: 1, op: 'pair', name: 'x', key: 'A'.repeat(43) + '=', proof: 'A'.repeat(43) + '=' }));
    for (let i = 0; i < 100 && frames.length < 2; i++) await new Promise((r) => setTimeout(r, 20));
    assert.deepEqual(frames.find((f) => f.re === 1)?.error, { code: 'bad_request', why: "this socket is not for pairing: it is a skill run's" });
    ws.close();
  });
  assert.equal(redact({ op: 'hello', token: 'T'.repeat(43) }).token, '<redacted>');
});

test("the control socket refuses a real client's id or signature beside a token, as lamp-control does (bad_request)", async () => {
  await withLocal(['light'], async (fake) => {
    const { WebSocket } = await import('ws');
    const token = fake.mintToken(['light']);
    const zero = Buffer.alloc(64).toString('base64');
    for (const [client, sig] of [['c_1', zero], ['c_0', Buffer.alloc(64, 1).toString('base64')]]) {
      const ws = new WebSocket(`ws+unix://${fake.localPath}:/v1`);
      const frames = [];
      ws.on('message', (data) => frames.push(JSON.parse(data.toString())));
      await new Promise((resolve, reject) => { ws.on('open', resolve); ws.on('error', reject); });
      ws.send(JSON.stringify({ id: 1, op: 'hello', client, sig, token }));
      for (let i = 0; i < 100 && !frames.some((f) => f.re === 1); i++) await new Promise((r) => setTimeout(r, 20));
      assert.deepEqual(frames.find((f) => f.re === 1)?.error, { code: 'bad_request', why: 'on this socket hello is client c_0, a zero sig and the token' });
      ws.close();
    }
  });
});

test('connect() with no pairing and a run token is the run\'s local connection; an empty token, a host or a pairing keep the old way', async () => {
  const { mkdtempSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { Keyring, NotPaired } = await import('../src/index.js');
  const saved = { ...process.env };
  try {
    await withLocal(['body', 'light'], async (fake) => {
      const keyring = () => new Keyring(mkdtempSync(join(tmpdir(), 'lampos-token-')));
      process.env.LAMPOS_AGENT_SOCKET = join(tmpdir(), 'lampos-no-agent.sock');
      process.env.LAMP_CONTROL_SOCKET = fake.localPath;
      process.env.LAMP_CONTROL_TOKEN = fake.mintToken(['light'], 'r1', 'water');
      const lamp = await Lamp.connect(undefined, { keyring: keyring() });
      assert.deepEqual([lamp.grants, lamp.run, lamp.skill], [['light'], 'r1', 'water']);
      await lamp.close();
      for (const unset of ['', '   ']) {
        process.env.LAMP_CONTROL_TOKEN = unset;
        await assert.rejects(Lamp.connect(undefined, { keyring: keyring() }), NotPaired);
      }
      process.env.LAMP_CONTROL_TOKEN = fake.mintToken(['light'], 'r2');
      await assert.rejects(Lamp.connect(undefined, { host: '127.0.0.1', keyring: keyring() }), NotPaired);
      const bad = 'B'.repeat(43);
      process.env.LAMP_CONTROL_TOKEN = bad;
      await assert.rejects(Lamp.connect(undefined, { keyring: keyring() }), (error) => error instanceof byCode.no_grant && !String(error.message).includes(bad) && !String(error.why).includes(bad));
    });
  } finally { process.env = saved; }
});
