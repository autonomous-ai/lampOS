// startTwin (test/twin-start.js): the one start every test that runs the twin as a child uses. Tried here with small scripts that
// stand in for the twin: one that ends, one that never prints, one whose first port is "taken", one that prints its address.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { startTwin } from './twin-start.js';

const script = (t, body) => {
  const folder = mkdtempSync(join(tmpdir(), 'twin-start-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const file = join(folder, 'twin.js');
  writeFileSync(file, body);
  return { file, folder };
};

test('a twin that prints its address gives its port and token', async (t) => {
  const { file } = script(t, "console.log('http://localhost:' + process.argv[2] + '/?t=abc123'); setInterval(() => {}, 1000);");
  const started = await startTwin(t, file, (port) => [String(port)]);
  assert.equal(started.token, 'abc123');
  assert.ok(started.port > 0);
});

test('a twin that ends says its exit code and its stderr at once, not after the wait', async (t) => {
  const { file } = script(t, "console.error('no such glb'); process.exit(3);");
  const began = Date.now();
  await assert.rejects(startTwin(t, file, () => []), /the twin ended \(3\) before it printed its address: no such glb/);
  assert.ok(Date.now() - began < 5000, 'not after the wait');
});

test('a twin that never prints is said after the wait, with the wait named', async (t) => {
  const { file } = script(t, 'setInterval(() => {}, 1000);');
  await assert.rejects(startTwin(t, file, () => [], { ms: 300 }), /the twin printed no address in 0\.3 s/);
});

test('a start whose port was taken in between is tried again on another', async (t) => {
  const { file, folder } = script(t, `const fs = require('node:fs'); const mark = process.argv[3];
    if (!fs.existsSync(mark)) { fs.writeFileSync(mark, 'x'); console.error('Error: listen EADDRINUSE: address already in use'); process.exit(1); }
    console.log('http://localhost:' + process.argv[2] + '/?t=second'); setInterval(() => {}, 1000);`);
  const mark = join(folder, 'tried');
  const started = await startTwin(t, file, (port) => [String(port), mark]);
  assert.equal(started.token, 'second');
  assert.ok(existsSync(mark));
});
