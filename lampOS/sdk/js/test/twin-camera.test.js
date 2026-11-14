// The twin's "what the lamp sees" panel asks the page's server for /camera.jpg, which asks the lamp
// (the `camera` grant) for its newest picture: with the twin's token only, and the lamp's own refusals
// come back as a reason, not a picture. Runs on the twin's fake lamp with a stand-in CAD file.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { startTwin } from './twin-start.js';

test('the twin serves the lamp\'s newest picture to its own page only', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'twin-camera-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const { port, token } = await startTwin(t, fileURLToPath(new URL('../examples/twin/server.js', import.meta.url)), (port) => ['--fake', '--port', String(port), '--glb', glb]);
  const base = `http://127.0.0.1:${port}`;
  assert.equal((await fetch(`${base}/camera.jpg`)).status, 403, 'no token, no picture');
  assert.equal((await fetch(`${base}/camera.jpg?t=wrong`)).status, 403);
  const picture = await fetch(`${base}/camera.jpg?t=${token}&n=1`);
  assert.equal(picture.status, 200);
  assert.equal(picture.headers.get('content-type'), 'image/jpeg');
  assert.equal(picture.headers.get('cache-control'), 'no-store');
  assert.ok((await picture.arrayBuffer()).byteLength > 0);
  // Asked again at once, the last picture again (the lamp is not asked more than every 700 ms).
  assert.equal((await fetch(`${base}/camera.jpg?t=${token}&n=2`)).status, 200);
});
