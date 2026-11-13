// The twin keeps an idle keep-alive connection open past Node's own limit (5 s; 6 s on Node 24): a page that spends seconds
// parsing the lamp model between two requests reuses its connection, and a request that races the server's close is aborted
// in the browser with no status (the try-it page's buttons were lost that way, 1 load in 8 drew them, on a loaded laptop).
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { startTwin } from './twin-start.js';

const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));

test('an idle connection to the twin is still open after 8 s (Node closes its own at 5 to 6 s)', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'try-keepalive-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const { port, token } = await startTwin(t, server, (port) => ['--fake', '--port', String(port), '--glb', glb]);
  const socket = connect(port, '127.0.0.1');
  t.after(() => socket.destroy());
  let answer = '';
  let closedAfter = null;
  const started = Date.now();
  socket.on('data', (chunk) => { answer += chunk; });
  socket.on('close', () => { closedAfter ??= Date.now() - started; });
  socket.write(`GET /model.json?t=${token} HTTP/1.1\r\nHost: localhost:${port}\r\nConnection: keep-alive\r\n\r\n`);
  await new Promise((r) => setTimeout(r, 8000));
  assert.match(answer, /^HTTP\/1\.1 200 /, 'the first request was answered');
  assert.equal(closedAfter, null, `the twin closed the idle connection after ${closedAfter} ms`);
  // And the same connection still serves a second request.
  socket.write(`GET /model.json?t=${token} HTTP/1.1\r\nHost: localhost:${port}\r\nConnection: close\r\n\r\n`);
  for (let i = 0; i < 40 && (answer.match(/HTTP\/1\.1 200/g) ?? []).length < 2; i += 1) await new Promise((r) => setTimeout(r, 50));
  assert.equal((answer.match(/HTTP\/1\.1 200/g) ?? []).length, 2, 'a second request on the same connection was answered');
});
