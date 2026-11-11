// The launcher starts a lamp-sim, pairs with it and opens the twin on it; here
// the lamp-sim is a stub (a script that runs the SDK's fake lamp behind the
// door's port and prints its pairing code), so no Rust binary is needed.
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { appCommands, parseArguments, pairingFrom, portFree, riggedCad, start } from '../examples/twin/virtual-lamp.js';

const testing = new URL('../src/testing.js', import.meta.url).href;

const STUB = `#!${process.execPath}
// CommonJS on purpose: a file with no extension is a script, not a module, on every Node the SDK supports.
const { writeFileSync, readFileSync } = require('node:fs');
const { createConnection, createServer } = require('node:net');
const { join } = require('node:path');
(async () => {
  const [, , what, dir, ...rest] = process.argv;
  if (what === 'run') {
    const { FakeLamp } = await import(${JSON.stringify(testing)});
    const fake = await FakeLamp.start({ grants: ['body', 'light', 'speak', 'look'] });
    writeFileSync(join(dir, 'stub.json'), JSON.stringify({ code: fake.code, pin: fake.pin }));
    const door = Number(rest[rest.indexOf('--listen') + 1].split(':')[1]);
    // The door's port forwards to the fake's own.
    createServer((client) => { const to = createConnection(fake.port, '127.0.0.1'); client.pipe(to); to.pipe(client); to.on('error', () => client.destroy()); client.on('error', () => to.destroy()); }).listen(door, '127.0.0.1');
    process.on('SIGTERM', () => process.exit(0));
  } else if (what === 'code') {
    console.log('some words on the way');
    console.log(readFileSync(join(dir, 'stub.json'), 'utf8'));
  }
})();
`;

const freePort = () => new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); }); });
const closed = (port) => new Promise((resolve) => { const probe = createServer(); probe.once('error', () => resolve(false)); probe.listen(port, '127.0.0.1', () => probe.close(() => resolve(true))); });

test('the launcher reads its arguments and the pairing code it is given', () => {
  const options = parseArguments(['--lamp-sim', '/x/lamp-sim', '--port', '7740', '--no-open', '--camera=always', '--microphone']);
  assert.equal(options['lamp-sim'], '/x/lamp-sim');
  assert.equal(options.port, '7740');
  assert.deepEqual(options.flags, ['--no-open', '--camera=always', '--microphone']);
  assert.throws(() => parseArguments(['--wat']), /unknown argument/);
  assert.deepEqual(pairingFrom('words\n{"code":"12345678","pin":"ab"}\n'), { code: '12345678', pin: 'ab' });
  assert.throws(() => pairingFrom('{"nope":1}'), /no pairing code/);
});

test('the twin opens in a window of its own where a Chromium-based browser can be asked for one', () => {
  const url = 'http://localhost:8737/?t=abc';
  const linux = appCommands(url, 'linux');
  assert.deepEqual(linux[0], ['google-chrome', [`--app=${url}`, '--window-size=1100,720']]);
  assert.ok(linux.some(([program]) => program === 'chromium'));
  assert.deepEqual(appCommands(url, 'darwin')[0], ['open', ['-na', 'Google Chrome', '--args', `--app=${url}`, '--window-size=1100,720']]);
  assert.equal(appCommands(url, 'win32')[0][0], 'cmd');
  assert.deepEqual(appCommands(url, 'plan9'), [], 'no window where nothing is known: the default browser opens it');
  assert.deepEqual(parseArguments(['--browser']).flags, ['--browser']);
});

test('one command: the lamp, the pairing and the twin on it, and all of it stops together', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'vlamp-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, STUB);
  chmodSync(stub, 0o755);
  const [port, twinPort] = [await freePort(), await freePort()];
  // The twin wants the rigged CAD (an LFS file a clean clone does not have): any file that says it is a glTF will do for the page.
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const running = await start({ 'lamp-sim': stub, dir: join(folder, 'lamp'), port: String(port), 'twin-port': String(twinPort), glb, flags: ['--no-open'] }, () => {});
  try {
    assert.match(running.url, new RegExp(`^http://localhost:${twinPort}/\\?t=`));
    const page = await fetch(running.url);
    assert.equal(page.status, 200);
    assert.match(await page.text(), /Lamp twin/);
    assert.notEqual((await fetch(`http://localhost:${twinPort}/`)).status, 200, 'the page answers only with its token');
  } finally {
    running.stop();
  }
  for (let waited = 0; waited < 5_000 && !(await closed(twinPort) && await closed(port)); waited += 100) await new Promise((r) => setTimeout(r, 100));
  assert.ok(await closed(twinPort), 'the twin stopped');
  assert.ok(await closed(port), 'the lamp stopped');
});

test('a twin without its rigged CAD is said before anything starts', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'vlamp-cad-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const pointer = join(folder, 'lamp.glb');
  writeFileSync(pointer, 'version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 1\n');
  assert.equal(riggedCad(pointer), false);
  assert.equal(riggedCad(join(folder, 'nope.glb')), false);
  await assert.rejects(start({ 'lamp-sim': '/no/such/lamp-sim', glb: pointer, flags: ['--no-open'] }, () => {}), /no rigged CAD here.*git lfs pull/);
});

test('a busy port is said before anything starts', async (t) => {
  // A stub rigged CAD: the check for the real one (an LFS file CI doesn't pull) comes first.
  const folder = mkdtempSync(join(tmpdir(), 'vlamp-port-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const holder = createServer();
  const port = await new Promise((resolve) => holder.listen(0, '127.0.0.1', () => resolve(holder.address().port)));
  try {
    assert.equal(await portFree(port), false);
    await assert.rejects(start({ 'lamp-sim': '/no/such/lamp-sim', port: String(port), glb, flags: ['--no-open'] }, () => {}), /door port \d+ is in use.*--port/);
    await assert.rejects(start({ 'lamp-sim': '/no/such/lamp-sim', 'twin-port': String(port), port: String(await freePort()), glb, flags: ['--no-open'] }, () => {}), /page port \d+ is in use.*--twin-port/);
  } finally { await new Promise((resolve) => holder.close(resolve)); }
  assert.equal(await portFree(port), true);
});
