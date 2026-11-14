// A page to run the pre-flight against (shared by the try-preflight*.test.js files, which are split so that each ends well inside
// the runner's 60 s limit for one file on a slow machine).
import { spawn } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const tryJs = new URL('../examples/twin/try.js', import.meta.url).pathname;
const serverJs = new URL('../examples/twin/server.js', import.meta.url).pathname;
const freePort = () => new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); }); });

// A page to run against: the twin on its own fake lamp with the try-it panel (as try.test.js starts it), or a real-mode run.
export async function page(t, { real, fake, home } = {}) {
  const folder = mkdtempSync(join(tmpdir(), 'try-preflight-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, '#!/bin/sh\nexit 0\n');
  chmodSync(stub, 0o755);
  // The port is probed free, closed, then the twin binds it: another test file can take it in between (EADDRINUSE, a red
  // Node 22 on #6745). So the start is tried again on a new port when the twin ended with that.
  let twinPort;
  let child;
  let text;
  let ended;
  let token;
  for (let attempt = 1; ; attempt += 1) {
    twinPort = await freePort();
    const [file, args, env] = real
      ? [tryJs, ['--real', '--host', fake.host, '--port', String(fake.port), '--twin-port', String(twinPort), '--glb', glb], { ...process.env, HOME: home, LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }]
      : [serverJs, ['--fake', '--port', String(twinPort), '--glb', glb, '--stim-dir', folder, '--lamp-sim', stub], process.env];
    child = spawn(process.execPath, [file, ...args], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    text = '';
    let errors = '';
    child.stdout.on('data', (c) => { text += c; });
    child.stderr.on('data', (c) => { errors += c; process.stderr.write(c); });
    ended = new Promise((resolve) => child.on('close', resolve));
    const started = await new Promise((resolve) => {
      const timer = setInterval(() => { const m = /\?t=(\S+)/.exec(text); if (m) { clearInterval(timer); resolve({ token: m[1] }); } }, 50);
      ended.then((code) => { clearInterval(timer); resolve({ code, errors }); });
    });
    if (started.token) { token = started.token; break; }
    if (attempt < 5 && /EADDRINUSE/.test(started.errors + text)) continue;
    throw new Error(`ended ${started.code}`);
  }
  t.after(async () => {
    child.kill('SIGTERM');
    let timer;
    await Promise.race([ended, new Promise((resolve) => { timer = setTimeout(resolve, 10000); })]);
    clearTimeout(timer); // a bare 10 s timer would keep the test file's process alive that long after its last test
    child.kill('SIGKILL');
  });
  return { url: `http://127.0.0.1:${twinPort}/?t=${token}`, token, child, ended };
}
