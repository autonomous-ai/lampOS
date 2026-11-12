// The try-it camera and microphone rows (twin/controls.js), against the SDK's fake lamp: one press, one use; a picture goes
// to the page that pressed and nowhere else; sound is counted, never kept. Its own file, so a row PR never appends to
// try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked, describe } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

async function lampWith(t, grants) {
  const fake = await FakeLamp.start({ grants });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const shown = [];
  const run = (id, values = {}) => {
    const control = CONTROLS.find((c) => c.id === id);
    return control.run({ lamp, release: async () => {}, show: (jpeg) => shown.push(jpeg) }, checked(control.fields, values));
  };
  return { fake, lamp, run, shown };
}

test('the camera rows are named for the camera grant, the microphone row for the microphone, and describe() says so', () => {
  const table = describe().controls;
  const needs = Object.fromEntries(table.filter((c) => c.needs).map((c) => [c.id, c.needs]));
  assert.deepEqual(needs, { camera_snapshot: 'camera', camera_stream: 'camera', microphone_stream: 'microphone' });
  assert.equal('needs' in table.find((c) => c.id === 'gesture'), false);
});

test('one camera snapshot is one picture handed to show(), and the answer says its size and keeps the bytes out of the log', async (t) => {
  const { run, shown, fake } = await lampWith(t, ['camera']);
  const said = await run('camera_snapshot');
  assert.equal(shown.length, 1);
  assert.equal(shown[0][0], 0xff, 'a JPEG');
  assert.equal(said, `lamp.cameraSnapshot() -> a JPEG of ${shown[0].length} bytes, on this page only`);
  assert.equal(fake.asked('camera.snapshot').length, 1, 'one press, one ask');
});

test('a short camera stream counts frames, shows the last one, and turns the stream off', async (t) => {
  const { run, shown, fake } = await lampWith(t, ['camera']);
  const said = await run('camera_stream', { seconds: 1, fps: 2 });
  assert.match(said, /^lamp\.cameraStream\(true, 2\) for 1 s -> [1-9]\d* frames; the last is on this page only$/);
  assert.equal(shown.length, 1);
  assert.deepEqual(fake.asked('camera.stream').map((r) => r.on), [true, false], 'on, then off');
});

test('a short microphone stream counts blocks and bytes, plays and keeps nothing, and turns the stream off', async (t) => {
  const { run, shown, fake } = await lampWith(t, ['microphone']);
  const said = await run('microphone_stream', { seconds: 1 });
  const found = /-> (\d+) blocks, (\d+) bytes, (\d+) lost \(counted, not played or kept\)$/.exec(said);
  assert.ok(found, said);
  assert.ok(Number(found[1]) >= 10 && Number(found[2]) === Number(found[1]) * 640, said);
  assert.equal(shown.length, 0, 'no sound or picture reaches the page');
  assert.deepEqual(fake.asked('microphone.stream').map((r) => r.on), [true, false]);
});

test('without the grant a row says the lamp\'s refusal, and a private lamp refuses the microphone in its words', async (t) => {
  const { run } = await lampWith(t, ['light']);
  await assert.rejects(run('camera_snapshot'), /not granted: camera/);
  await assert.rejects(run('microphone_stream', { seconds: 1 }), /not granted: microphone/);
  const { run: heard, fake } = await lampWith(t, ['microphone']);
  fake.setPrivate(true);
  await assert.rejects(heard('microphone_stream', { seconds: 1 }), /the lamp is private/);
  assert.deepEqual(fake.asked('microphone.stream').map((r) => r.on), [true, false], 'the stream is asked off even after a refusal');
});

// The server: the picture goes to the page that pressed, as one message to it, and to no other page or log.
test('the twin sends a row\'s picture to the page that pressed it, not to another page, and the log line has no picture', async (t) => {
  const { mkdtempSync, rmSync, writeFileSync, chmodSync } = await import('node:fs');
  const { spawn } = await import('node:child_process');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { createServer } = await import('node:net');
  const WebSocket = (await import('ws')).default;
  const server = new URL('../examples/twin/server.js', import.meta.url).pathname;
  const folder = mkdtempSync(join(tmpdir(), 'try-media-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, '#!/bin/sh\nexit 0\n');
  chmodSync(stub, 0o755);
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const port = await new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port: p } = s.address(); s.close(() => resolve(p)); }); });
  const child = spawn(process.execPath, [server, '--fake', '--port', String(port), '--glb', glb, '--stim-dir', folder, '--lamp-sim', stub], { stdio: ['ignore', 'pipe', 'inherit'] });
  t.after(() => child.kill('SIGTERM'));
  const token = await new Promise((resolve, reject) => {
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; const found = /\?t=(\S+)/.exec(text); if (found) resolve(found[1]); });
    child.on('close', (code) => reject(new Error(`the server ended (${code})`)));
  });
  const open = () => new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { headers: { origin: `http://127.0.0.1:${port}` } });
    const got = [];
    socket.on('message', (data) => got.push(JSON.parse(data)));
    socket.on('open', () => resolve({ socket, got }));
    socket.on('error', reject);
  });
  const [one, two] = [await open(), await open()];
  t.after(() => { one.socket.close(); two.socket.close(); });
  one.socket.send(JSON.stringify({ type: 'call', id: 7, name: 'camera_snapshot', args: {} }));
  for (let i = 0; i < 100 && !one.got.some((m) => m.type === 'result'); i += 1) await new Promise((r) => setTimeout(r, 50));
  const picture = one.got.find((m) => m.type === 'picture');
  assert.equal(picture?.id, 7, 'the pressing page got its picture');
  assert.equal(Buffer.from(picture.jpeg, 'base64')[0], 0xff);
  assert.equal(two.got.some((m) => m.type === 'picture'), false, 'another page got none');
  const result = one.got.find((m) => m.type === 'result');
  assert.equal(result.ok, true);
  assert.equal(JSON.stringify([...one.got.filter((m) => m.type !== 'picture'), ...two.got]).includes(picture.jpeg), false, 'the picture is in no log line or other message');
});
