// The try-it app's "Light pixels" row: a lamp's own `shows.pixels` beats lamp-sim's ring file (twin/server.js).
// Its own file, so a new try-it test never appends to the tail of try.test.js (every row PR did, and conflicted).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';

const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
const freePort = () => new Promise((resolve) => {
  const probe = createServer();
  probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
});
const until = async (ready) => { for (let i = 0; i < 100 && !ready(); i++) await new Promise((r) => setTimeout(r, 50)); };

// The try-it server on the SDK's fake lamp with a stand-in lamp-sim, and a ring file the test writes.
async function started(t) {
  const folder = mkdtempSync(join(tmpdir(), 'try-pixels-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, '#!/bin/sh\nexit 0\n');
  chmodSync(stub, 0o755);
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const child = spawn(process.execPath, [server, '--fake', '--port', String(await freePort()), '--glb', glb, '--stim-dir', folder, '--lamp-sim', stub, '--ring-file', join(folder, 'ring.json')], { stdio: ['ignore', 'pipe', 'inherit'] });
  t.after(() => child.kill('SIGTERM'));
  const found = await new Promise((resolve, reject) => {
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; const m = /http:\/\/localhost:(\d+)\/\?t=(\S+)/.exec(text); if (m) resolve({ port: m[1], token: m[2] }); });
    child.on('close', (code) => reject(new Error(`the server ended (${code})`)));
  });
  return { ...found, folder };
}

test('a lamp\'s own shows.pixels beats the ring file', async (t) => {
  const { port, token, folder } = await started(t);
  const file = Array(32).fill('#123456');
  writeFileSync(join(folder, 'ring.json'), JSON.stringify({ pixels: file }));
  const socket = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { headers: { origin: `http://127.0.0.1:${port}` } });
  const states = [];
  const results = [];
  socket.on('message', (data) => { const m = JSON.parse(data); if (m.type === 'state') states.push(m.state); if (m.type === 'result') results.push(m); });
  await new Promise((resolve, reject) => { socket.on('open', resolve); socket.on('error', reject); });
  t.after(() => socket.close());
  const shown = () => states.at(-1)?.lamp?.light?.shows?.pixels;
  await until(() => shown());
  assert.deepEqual(shown(), file, 'with no light of its own, the ring file shows');
  socket.send(JSON.stringify({ type: 'call', id: 1, name: 'light_pixels', args: { preset: 'half red, half blue', for_ms: 20_000 } }));
  await until(() => results.length > 0);
  assert.equal(results[0].ok, true, results[0].answer);
  await until(() => shown()?.[0] === '#ff0000');
  assert.equal(shown()[31], '#0000ff', 'the lamp\'s own pixels, not the file\'s');
});
