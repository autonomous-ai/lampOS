// On the lamp itself, as the user the owner turned on, `Lamp.connect()` needs no pairing: it uses the
// agent socket (docs/control.md, "The local agent"). A pairing, or a `host`, keeps the old way.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, NotPaired } from '../src/index.js';
import { WebSocketServer } from '../src/ws.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-agent-'));

/** What lamp-control's agent socket says: a welcome, then hello answered with the grants. */
function door(path, seen) {
  const http = createServer();
  const wss = new WebSocketServer({ server: http });
  wss.on('connection', (ws) => {
    ws.send(JSON.stringify({ ev: 'welcome', nonce: Buffer.alloc(32).toString('base64'), lamp: { id: 'lamp-t' } }));
    ws.on('message', (data) => {
      const hello = JSON.parse(data.toString());
      seen.push(hello);
      ws.send(JSON.stringify({ ok: true, re: hello.id, lamp: { id: 'lamp-t', name: 'Lamp', v: 1 }, grants: ['body', 'light', 'look', 'skills', 'speak'], limits: {} }));
    });
  });
  return new Promise((resolve) => http.listen(path, () => resolve(() => { wss.close(); http.close(); })));
}

test('with no pairing the agent socket is used and hello names no key', async () => {
  const dir = folder();
  const path = join(dir, 'agent.sock');
  const seen = [];
  const stop = await door(path, seen);
  process.env.LAMPOS_AGENT_SOCKET = path;
  try {
    const lamp = await Lamp.connect(undefined, { keyring: new Keyring(join(dir, 'home')) });
    assert.deepEqual([...lamp.grants].sort(), ['body', 'light', 'look', 'skills', 'speak']);
    await lamp.close();
  } finally { delete process.env.LAMPOS_AGENT_SOCKET; stop(); rmSync(dir, { recursive: true, force: true }); }
  assert.equal(seen[0].op, 'hello');
  assert.equal(seen[0].client, 'c_0');
  assert.deepEqual(Buffer.from(seen[0].sig, 'base64'), Buffer.alloc(64));
});

test('without a pairing or a socket it is not paired, and a file that is not a socket is not one', async () => {
  const dir = folder();
  const plain = join(dir, 'agent.sock');
  writeFileSync(plain, 'x');
  process.env.LAMPOS_AGENT_SOCKET = join(dir, 'none.sock');
  try {
    await assert.rejects(Lamp.connect(undefined, { keyring: new Keyring(join(dir, 'home')) }), NotPaired);
    process.env.LAMPOS_AGENT_SOCKET = plain;
    await assert.rejects(Lamp.connect(undefined, { keyring: new Keyring(join(dir, 'home')) }), NotPaired);
  } finally { delete process.env.LAMPOS_AGENT_SOCKET; rmSync(dir, { recursive: true, force: true }); }
});

test('a host asked for never uses the socket', async () => {
  const dir = folder();
  const path = join(dir, 'agent.sock');
  const stop = await door(path, []);
  process.env.LAMPOS_AGENT_SOCKET = path;
  try {
    await assert.rejects(Lamp.connect(undefined, { host: 'lamp-x.local', keyring: new Keyring(join(dir, 'home')) }), NotPaired);
  } finally { delete process.env.LAMPOS_AGENT_SOCKET; stop(); rmSync(dir, { recursive: true, force: true }); }
});
