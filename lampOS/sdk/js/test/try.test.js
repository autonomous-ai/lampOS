// The try-it app's server (twin/server.js with --stim-dir): the token, the LAN origin, one stimulus per
// button run as `lamp-sim <verb> ... DIR` with an argument array, and one SDK call per call button.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { request } from 'node:http';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { chooseLan, lanAddresses } from '../examples/twin/guard.js';
import { CONTROLS, STIMULI, checked, describe } from '../examples/twin/controls.js';
import { hostOk, originOk } from '../examples/twin/guard.js';
import { grantsFor, parseArguments } from '../examples/twin/virtual-lamp.js';

const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));

test('only the printed LAN address, localhost and 127.0.0.1 are the twin\'s own Host and Origin', () => {
  assert.equal(hostOk('192.168.1.5:8737', 8737, '192.168.1.5:8737'), true);
  assert.equal(hostOk('evil.example:8737', 8737, '192.168.1.5:8737'), false);
  assert.equal(hostOk('192.168.1.9:8737', 8737, '192.168.1.5:8737'), false);
  assert.equal(hostOk('192.168.1.5:8737', 8737), false);
  assert.equal(hostOk('127.0.0.1:8737', 8737), true);
  assert.equal(originOk('http://evil.example:8737', 8737, '192.168.1.5:8737'), false);
});

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
  writeFileSync(stub, `#!/bin/sh\nfor a in "$@"; do printf '%s\\n' "$a"; done > "${join(folder, 'argv')}"\necho run >> "${join(folder, 'runs')}"\n`);
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

test('the try-it lamp has no camera or microphone grant unless asked, and every grant for the virtual lamp', () => {
  const grants = (...args) => grantsFor(parseArguments(args).flags, true).split(',');
  assert.equal(grants('--try').includes('camera'), false);
  assert.equal(grants('--try').includes('microphone'), false);
  assert.equal(grants('--try', '--camera').includes('camera'), true);
  assert.equal(grants('--try', '--camera').includes('microphone'), false);
  assert.equal(grants('--try', '--camera', '--microphone').includes('microphone'), true);
  assert.equal(grantsFor([], false).includes('camera'), true);
});

test('on the network the camera picture is given only while a page says it is on, and only to the twin\'s own Host', async (t) => {
  const { port, token } = await started(t, (dir, stub) => ['--lan', '--stim-dir', dir, '--lamp-sim', stub]);
  const url = `http://127.0.0.1:${port}/camera.jpg?t=${token}`;
  // A token alone gets no picture.
  assert.equal((await fetch(url)).status, 403);
  // A page that has the camera on says so over its live link, then asks.
  const { socket } = await talk(port, token);
  t.after(() => socket.close());
  socket.send(JSON.stringify({ type: 'camera_on' }));
  await new Promise((resolve) => setTimeout(resolve, 300));
  const picture = await fetch(url);
  assert.equal(picture.status, 200);
  assert.equal(picture.headers.get('content-type'), 'image/jpeg');
  // Another Host name (a rebinding page) is refused before the token is looked at.
  const status = await new Promise((resolve, reject) => {
    const asked = request({ host: '127.0.0.1', port, path: `/try.json?t=${token}`, headers: { host: `evil.example:${port}` } }, (response) => { response.resume(); resolve(response.statusCode); });
    asked.on('error', reject);
    asked.end();
  });
  assert.equal(status, 403);
});

test('the vendored three.js is served from a fixed list without the token, and nothing else under /vendor/ is', async (t) => {
  const { port, token } = await started(t, (dir, stub) => ['--lan', '--stim-dir', dir, '--lamp-sim', stub]);
  const base = `http://127.0.0.1:${port}`;
  for (const path of ['/vendor/three/three.module.min.js', '/vendor/three/jsm/loaders/GLTFLoader.js', '/vendor/three/jsm/controls/OrbitControls.js', '/vendor/three/jsm/utils/BufferGeometryUtils.js']) {
    const got = await fetch(`${base}${path}`);
    assert.equal(got.status, 200, path);
    assert.match(got.headers.get('content-type'), /^text\/javascript/, path);
    assert.ok((await got.text()).length > 1000, path);
  }
  // The page's import map points at the same list, and the lamp's own files still need the token.
  const page = await (await fetch(`${base}/?t=${token}`)).text();
  assert.match(page, /"three": "\/vendor\/three\/three\.module\.min\.js"/);
  assert.equal((await fetch(`${base}/try.json`)).status, 403);
  // Not on the list: no token, no file; with the token, no such file (a file that is in the folder is not served).
  for (const path of ['/vendor/three/README.md', '/vendor/three/jsm/', '/vendor/../server.js', '/vendor/three/%2e%2e/server.js', '/vendor/three/jsm/loaders/GLTFLoader.js/x']) {
    assert.equal((await fetch(`${base}${path}`)).status, 403, path);
    assert.equal((await fetch(`${base}${path}?t=${token}`)).status, 404, path);
  }
  // Only a read: no other method.
  assert.equal((await fetch(`${base}/vendor/three/three.module.min.js`, { method: 'POST' })).status, 403);
  // The Host check still comes first on the network.
  const status = await new Promise((resolve, reject) => {
    const asked = request({ host: '127.0.0.1', port, path: '/vendor/three/three.module.min.js', headers: { host: `evil.example:${port}` } }, (response) => { response.resume(); resolve(response.statusCode); });
    asked.on('error', reject);
    asked.end();
  });
  assert.equal(status, 403);
});

// A laptop like the one this was found on: Ethernet, Wi-Fi, a tailscale tunnel, Docker and its bridges, loopback.
const table = (name, address, internal = false) => [name, [{ family: 'IPv4', address, internal }]];
const laptop = Object.fromEntries([
  table('lo', '127.0.0.1', true), table('docker0', '172.17.0.1'), table('br-1a2b3c', '172.18.0.1'), table('enp3s0', '172.168.21.8'),
  table('wlp0s20f3', '172.168.20.172'), table('tailscale0', '100.64.0.2'), table('veth9f', '172.19.0.1'),
]);

test('--lan names a real Ethernet or Wi-Fi address, never a Docker, VPN or loopback one, and a chosen address must be the computer\'s own', () => {
  assert.deepEqual(lanAddresses(laptop).map((a) => a.name), ['enp3s0', 'wlp0s20f3']);
  const first = chooseLan(laptop);
  assert.equal(first.address, '172.168.21.8');
  assert.deepEqual(first.others.map((a) => a.address), ['172.168.20.172']);
  // The person's choice: the Wi-Fi one, the other listed.
  const chosen = chooseLan(laptop, '172.168.20.172');
  assert.equal(chosen.address, '172.168.20.172');
  assert.deepEqual(chosen.others.map((a) => a.address), ['172.168.21.8']);
  // Not this computer's address: an error with the addresses it has; a computer with no real address: an error.
  assert.match(chooseLan(laptop, '8.8.8.8').error, /not an IPv4 address of this computer/);
  assert.ok(chooseLan(laptop, '8.8.8.8').own.length >= 5);
  assert.match(chooseLan({ lo: laptop.lo, docker0: laptop.docker0 }).error, /no network address found/);
  // One real address and no choice: nothing to say about others.
  assert.deepEqual(chooseLan({ eth0: laptop.enp3s0 }).others, []);
});

test('--lan-address with an address that is not this computer\'s ends the twin with the list, and without --lan it is refused', async () => {
  const folder = mkdtempSync(join(tmpdir(), 'try-lan-'));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  try {
    for (const [flags, said] of [[['--lan', '--lan-address', '8.8.8.8'], /8\.8\.8\.8 is not an IPv4 address of this computer; this computer's addresses: /], [['--lan-address', '127.0.0.1'], /--lan-address goes with --lan/]]) {
      const { code, err } = await new Promise((resolve) => {
        const child = spawn(process.execPath, [server, '--fake', '--port', '0', '--glb', glb, ...flags], { stdio: ['ignore', 'ignore', 'pipe'] });
        let err = '';
        child.stderr.on('data', (chunk) => { err += chunk; });
        child.on('close', (code) => resolve({ code, err }));
      });
      assert.equal(code, 2, `${flags}: ${err}`);
      assert.match(err, said);
    }
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test('with --lan the twin listens on loopback and the printed address, never on every interface', async (t) => {
  if (!existsSync('/proc/net/tcp')) return t.skip('needs /proc/net/tcp (Linux)');
  const { port } = await started(t, () => ['--lan']);
  const wanted = Number(port).toString(16).toUpperCase().padStart(4, '0');
  const bound = readFileSync('/proc/net/tcp', 'utf8').split('\n').slice(1)
    .map((line) => line.trim().split(/\s+/))
    .filter((columns) => columns[3] === '0A' && columns[1]?.endsWith(`:${wanted}`))
    .map((columns) => columns[1].split(':')[0]);
  assert.ok(bound.includes('0100007F'), `listening on 127.0.0.1: ${bound}`);
  assert.equal(bound.includes('00000000'), false, `not on every interface: ${bound}`);
});

test('a held hand is one touch: repeats within 0.8 s are the same stroke and fork nothing', async (t) => {
  const { port, token, folder } = await started(t, (dir, stub) => ['--stim-dir', dir, '--lamp-sim', stub]);
  const { socket, results } = await talk(port, token);
  t.after(() => socket.close());
  for (let id = 1; id <= 9; id++) socket.send(JSON.stringify({ type: 'stim', id, name: 'touch', args: { kind: 'pet' } }));
  await until(() => results.length >= 9);
  assert.equal(readFileSync(join(folder, 'runs'), 'utf8').trim().split('\n').length, 1, 'lamp-sim ran once');
  assert.equal(results.filter((r) => r.ok && r.answer === 'done').length, 1);
  assert.equal(results.filter((r) => /^ignored: the same stroke/.test(r.answer)).length, 8);
});
