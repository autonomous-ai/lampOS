// The try-it app's follow, joints, volume and read rows (controls.js): each answers in the lamp's own words.
// Its own file, so a row PR's test never conflicts with the others' at the tail of try.test.js; the server is
// started as try.test.js does (a stand-in lamp-sim, the SDK's fake lamp).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { onFreePort } from './free-port.js';

const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));


async function started(t, flags) {
  const folder = mkdtempSync(join(tmpdir(), 'try-test-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  // A stand-in lamp-sim: writes its arguments, one per line, and exits 0.
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, `#!/bin/sh\nfor a in "$@"; do printf '%s\\n' "$a"; done > "${join(folder, 'argv')}"\necho run >> "${join(folder, 'runs')}"\n`);
  chmodSync(stub, 0o755);
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const url = await onFreePort(async (port) => {
    const child = spawn(process.execPath, [server, '--fake', '--port', String(port), '--glb', glb, ...flags(folder, stub)], { stdio: ['ignore', 'pipe', 'pipe'] });
    t.after(() => child.kill('SIGTERM'));
    let errors = '';
    child.stderr.on('data', (chunk) => { errors += chunk; process.stderr.write(chunk); });
    return new Promise((resolve, reject) => {
      let text = '';
      child.stdout.on('data', (chunk) => { text += chunk; const found = /http:\/\/localhost:(\d+)\/\?t=(\S+)/.exec(text); if (found) resolve({ port: found[1], token: found[2] }); });
      child.on('close', (code) => reject(new Error(`the server ended (${code}): ${errors}`)));
    });
  });
  return { ...url, folder };
}

const talk = (port, token) => new Promise((resolve, reject) => {
  const socket = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { headers: { origin: `http://127.0.0.1:${port}` } });
  const results = [];
  socket.on('message', (data) => { const m = JSON.parse(data); if (m.type === 'result') results.push(m); });
  socket.on('open', () => resolve({ socket, results }));
  socket.on('error', reject);
});
const until = async (ready) => { for (let i = 0; i < 100 && !ready(); i++) await new Promise((r) => setTimeout(r, 50)); };

test('the follow, joints, volume and read rows each answer in words, the lamp\'s own', async (t) => {
  const { port, token } = await started(t, (dir, stub) => ['--stim-dir', dir, '--lamp-sim', stub]);
  const { socket, results } = await talk(port, token);
  t.after(() => socket.close());
  let id = 0;
  const press = async (name, args = {}) => {
    const mine = ++id;
    socket.send(JSON.stringify({ type: 'call', id: mine, name, args }));
    await until(() => results.some((r) => r.id === mine));
    return results.find((r) => r.id === mine);
  };
  const followed = await press('follow_hand', { for_ms: 2000 });
  assert.equal(followed.ok, true);
  assert.equal(followed.call, 'lamp.followHand({ forMs: 2000 }).start()');
  // The following holds the body: a joints call lets it go first and takes the body itself.
  const aimed = await press('joints', { yaw: 4, pitch: 2, within_ms: 200 });
  assert.equal(aimed.ok, true, aimed.answer);
  assert.match(aimed.call, /^lamp\.withLease\(\['body'\], \(lease\) => lamp\.joints\(lease, 200, \{"yaw":4,"pitch":2\}, \{ wait: true \}\)\)$/);
  assert.match((await press('volume_to', { to: 55 })).call, /^lamp\.volume\(\{ to: 55 \}\) -> \{"level":55,"muted":false\}$/);
  assert.match((await press('volume_step', { step: 'down' })).call, /^lamp\.volume\(\{ step: "down" \}\) -> \{"level":\d+,"muted":false\}$/);
  assert.match((await press('volume_mute', { mute: 'on' })).call, /^lamp\.volume\(\{ mute: true \}\) -> \{"level":\d+,"muted":true\}$/);
  assert.match((await press('get', { state: 'light' })).call, /^lamp\.get\("light"\) -> \{"light":/);
  assert.match((await press('body_info')).call, /^lamp\.bodyInfo\(\) -> \{"known":true/);
  // The air and device reads need their grants: the fake lamp has neither, and the server refuses them as the page draws them (off).
  for (const name of ['air', 'device']) {
    const read = await press(name);
    assert.equal(read.ok, false);
    assert.match(read.answer, new RegExp(`^bad_request: n/a: the lamp did not grant ${name}$`));
  }
  const refused = await press('volume_to', { to: 101 });
  assert.equal(refused.ok, false);
  assert.match(refused.answer, /to: a number from 0 to 100/);
});
