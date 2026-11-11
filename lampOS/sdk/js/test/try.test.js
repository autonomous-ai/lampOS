// The try-it app's server (twin/server.js with --stim-dir): the token, the LAN origin, one stimulus per
// button run as `lamp-sim <verb> ... DIR` with an argument array, and one SDK call per call button.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { CONTROLS, STIMULI, checked, describe } from '../examples/twin/controls.js';
import { originOk } from '../examples/twin/guard.js';

const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));

test('the page origin may be the host it was opened at only on the LAN', () => {
  assert.equal(originOk('http://192.168.1.5:8737', 8737, '192.168.1.5:8737'), true);
  assert.equal(originOk('http://192.168.1.5:8737', 8737), false);
  assert.equal(originOk('http://evil.example', 8737, '192.168.1.5:8737'), false);
  assert.equal(originOk('http://localhost:8737', 8737), true);
});

const freePort = () => new Promise((resolve) => {
  const probe = createServer();
  probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
});

async function started(t, flags) {
  const folder = mkdtempSync(join(tmpdir(), 'try-test-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  // A stand-in lamp-sim: writes its arguments, one per line, and exits 0.
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, `#!/bin/sh\nfor a in "$@"; do printf '%s\\n' "$a"; done > "${join(folder, 'argv')}"\n`);
  chmodSync(stub, 0o755);
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const child = spawn(process.execPath, [server, '--fake', '--port', String(await freePort()), '--glb', glb, ...flags(folder, stub)], { stdio: ['ignore', 'pipe', 'inherit'] });
  t.after(() => child.kill('SIGTERM'));
  const url = await new Promise((resolve, reject) => {
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; const found = /http:\/\/localhost:(\d+)\/\?t=(\S+)/.exec(text); if (found) resolve({ port: found[1], token: found[2] }); });
    child.on('close', (code) => reject(new Error(`the server ended (${code})`)));
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

test('without --stim-dir there is no panel; with it the token is still needed', async (t) => {
  const plain = await started(t, () => []);
  assert.equal((await fetch(`http://127.0.0.1:${plain.port}/panel.js?t=${plain.token}`)).status, 404);
  const tried = await started(t, (folder, stub) => ['--stim-dir', folder, '--lamp-sim', stub]);
  assert.equal((await fetch(`http://127.0.0.1:${tried.port}/panel.js`)).status, 403);
  assert.equal((await fetch(`http://127.0.0.1:${tried.port}/panel.js?t=${tried.token}`)).status, 200);
  assert.deepEqual(Object.keys(await (await fetch(`http://127.0.0.1:${tried.port}/try.json?t=${tried.token}`)).json()), ['controls', 'stimuli', 'camera', 'microphone']);
});

test('a stimulus runs lamp-sim with an argument array and the lamp folder last; a call is one SDK call', async (t) => {
  const { port, token, folder } = await started(t, (dir, stub) => ['--stim-dir', dir, '--lamp-sim', stub]);
  const { socket, results } = await talk(port, token);
  t.after(() => socket.close());
  // The text is ONE argument even with spaces and shell characters.
  socket.send(JSON.stringify({ type: 'stim', id: 1, name: 'touch', args: { kind: 'pet' } }));
  await until(() => results.length >= 1);
  assert.deepEqual(readFileSync(join(folder, 'argv'), 'utf8').trimEnd().split('\n'), ['touch', 'pet', folder]);
  socket.send(JSON.stringify({ type: 'stim', id: 2, name: 'air', args: { kind: 'stuffy' } }));
  await until(() => results.length >= 2);
  assert.deepEqual(readFileSync(join(folder, 'argv'), 'utf8').trimEnd().split('\n'), ['air', 'co2_ppm=1600', 'pm2_5=45', folder]);
  // A refused value never reaches lamp-sim; the answer says why.
  socket.send(JSON.stringify({ type: 'stim', id: 3, name: 'touch', args: { kind: '; rm -rf /' } }));
  socket.send(JSON.stringify({ type: 'stim', id: 4, name: 'nothing' }));
  socket.send(JSON.stringify({ type: 'call', id: 5, name: 'light', args: { colour: 'green', level: 3, for_ms: 2000 } }));
  socket.send(JSON.stringify({ type: 'call', id: 6, name: 'gesture', args: { name: 'NOT A GESTURE' } }));
  // A move needs the body lease and the sound clip is `lamp.sound`, not `play` (blue's two bugs on the person's app).
  socket.send(JSON.stringify({ type: 'call', id: 7, name: 'move', args: {} }));
  socket.send(JSON.stringify({ type: 'call', id: 8, name: 'sound', args: {} }));
  await until(() => results.length >= 8);
  const by = Object.fromEntries(results.map((r) => [r.id, r]));
  assert.equal(by[1].ok, true);
  assert.equal(by[3].ok, false);
  assert.match(by[3].answer, /kind: one of tap, pet/);
  assert.equal(by[4].ok, false);
  assert.equal(by[5].ok, true);
  assert.equal(by[5].call, 'lamp.light("green", 3, 2000)');
  assert.equal(by[6].ok, false);
  assert.match(by[6].answer, /^bad_request: name: one of nod/);
  assert.match(by[7].call, /^lamp\.withLease\(\['body'\], \(\) => lamp\.movePoses\(/);
  assert.equal(by[8].call, "lamp.sound('hum')");
});

test('a frame over the cap closes that socket and the server still answers the next one', async (t) => {
  const { port, token } = await started(t, (dir, stub) => ['--stim-dir', dir, '--lamp-sim', stub]);
  const { socket } = await talk(port, token);
  const closed = new Promise((resolve) => socket.on('close', (code) => resolve(code)));
  socket.send('x'.repeat(40_000));
  assert.equal(await closed, 1009);
  // The server lives: a new page's socket and a plain request both work.
  const again = await talk(port, token);
  t.after(() => again.socket.close());
  assert.equal((await fetch(`http://127.0.0.1:${port}/try.json?t=${token}`)).status, 200);
});

test('the table: every control and stimulus has a label, its fields are checked, and each runs with its defaults', async (t) => {
  // The table the page is drawn from.
  const { controls, stimuli } = describe();
  assert.equal(controls.length, CONTROLS.length);
  assert.equal(stimuli.length, STIMULI.length);
  for (const entry of [...CONTROLS, ...STIMULI]) assert.ok(entry.id && entry.group && entry.label, `${entry.id}: id, group, label`);
  assert.equal(new Set(CONTROLS.map((c) => c.id)).size, CONTROLS.length, 'a control id is used once');
  assert.equal(new Set(STIMULI.map((c) => c.id)).size, STIMULI.length, 'a stimulus id is used once');
  // A value outside its field never reaches a call; a default always passes.
  const light = CONTROLS.find((c) => c.id === 'light');
  assert.throws(() => checked(light.fields, { colour: 'NOT A COLOUR' }), /colour: one of/);
  assert.throws(() => checked(light.fields, { level: 9 }), /level: a number from 1 to 5/);
  assert.throws(() => checked(CONTROLS.find((c) => c.id === 'say').fields, { text: '' }), /text: 1 to 200 characters/);
  for (const entry of [...CONTROLS, ...STIMULI]) assert.doesNotThrow(() => checked(entry.fields, {}), `${entry.id} defaults`);
  // Each entry, pressed with its defaults against the SDK's fake lamp and a stand-in lamp-sim, answers: taken or
  // done, or the lamp's refusal in words, never a crash.
  const { port, token } = await started(t, (dir, stub) => ['--stim-dir', dir, '--lamp-sim', stub]);
  const { socket, results } = await talk(port, token);
  t.after(() => socket.close());
  let id = 0;
  const ids = {};
  for (const entry of CONTROLS) { ids[++id] = entry.id; socket.send(JSON.stringify({ type: 'call', id, name: entry.id, args: {} })); }
  for (const entry of STIMULI) { ids[++id] = entry.id; socket.send(JSON.stringify({ type: 'stim', id, name: entry.id, args: {} })); }
  await until(() => results.length >= id);
  for (const result of results) assert.ok(result.ok || !/^failed:/.test(result.answer), `${ids[result.id]}: ${result.answer}`);
  assert.ok(results.filter((r) => r.ok).length >= STIMULI.length, 'every stimulus ran');
});
