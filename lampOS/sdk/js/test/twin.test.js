// The twin's page and live link answer only with the token it printed, and
// /live only from its own origin: any website open in the browser can reach
// localhost (examples/twin/guard.js).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { newToken, originOk, tokenOk } from '../examples/twin/guard.js';

test('the token is asked of every request, whole', () => {
  const token = newToken();
  assert.ok(token.length >= 32);
  assert.notEqual(newToken(), token);
  assert.ok(tokenOk(`/?t=${token}`, token));
  assert.ok(tokenOk(`/live?t=${token}`, token));
  assert.ok(!tokenOk('/', token));
  assert.ok(!tokenOk('/?t=', token));
  assert.ok(!tokenOk(`/?t=${token.slice(0, -1)}`, token));
  assert.ok(!tokenOk(`/?t=${token}x`, token));
  assert.ok(!tokenOk(`/?t=${token.slice(0, -1)}${token.endsWith('A') ? 'B' : 'A'}`, token));
});

test('only the page\'s own origin may open the live link', () => {
  assert.ok(originOk('http://localhost:8737', 8737));
  assert.ok(originOk('http://127.0.0.1:8737', 8737));
  assert.ok(!originOk(undefined, 8737));
  assert.ok(!originOk('null', 8737));
  assert.ok(!originOk('https://example.com', 8737));
  assert.ok(!originOk('http://localhost:8738', 8737));
  assert.ok(!originOk('http://localhost.example.com:8737', 8737));
});

const glb = new URL('../../../../robots/lamp/hardware/cad/glb/lamp.glb', import.meta.url);
const hasCad = existsSync(glb) && readFileSync(glb).toString('latin1', 0, 4) === 'glTF';

test('the running twin refuses a request or an upgrade without its token or origin', { skip: !hasCad && 'lamp.glb is not here (Git LFS not pulled)' }, async () => {
  const port = 18000 + Math.floor(Math.random() * 2000);
  const server = spawn(process.execPath, [fileURLToPath(new URL('../examples/twin/server.js', import.meta.url)), '--fake', '--port', String(port)], { stdio: ['ignore', 'pipe', 'inherit'] });
  try {
    const url = await new Promise((resolve, reject) => {
      let out = '';
      server.stdout.on('data', (data) => {
        out += data;
        const found = out.match(/http:\/\/localhost:\d+\/\?t=\S+/);
        if (found) resolve(found[0]);
      });
      server.on('exit', (code) => reject(new Error(`the twin exited ${code}`)));
    });
    const token = new URL(url).searchParams.get('t');
    const base = `http://127.0.0.1:${port}`;
    assert.equal((await fetch(`${base}/`)).status, 403);
    assert.equal((await fetch(`${base}/model.json?t=wrong`)).status, 403);
    assert.equal((await fetch(`${base}/?t=${token}`)).status, 200);
    assert.equal((await fetch(`${base}/model.json?t=${token}`)).status, 200);
    const opens = (path, origin) => new Promise((resolve) => {
      const ws = new WebSocket(`ws://127.0.0.1:${port}${path}`, { origin });
      ws.on('open', () => { ws.close(); resolve(true); });
      ws.on('error', () => resolve(false));
    });
    assert.equal(await opens(`/live?t=${token}`, 'https://example.com'), false);
    assert.equal(await opens('/live', `http://localhost:${port}`), false);
    assert.equal(await opens(`/live?t=${token}`, undefined), false);
    assert.equal(await opens(`/live?t=${token}`, `http://localhost:${port}`), true);
  } finally {
    server.kill('SIGTERM');
  }
});
