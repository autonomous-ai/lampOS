// Shared by the try-real*.test.js files (split so that each ends well inside the runner's 60 s limit for one file on a slow
// machine): a real run of try.js against the fake lamp, a raw page socket, and the secrets to look for.
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';

const tryJs = new URL('../examples/twin/try.js', import.meta.url).pathname;
const freePort = () => new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); }); });

export async function run(t, { args = [], env = {}, home }) {
  const glb = join(home, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const child = spawn(process.execPath, [tryJs, '--real', '--twin-port', String(await freePort()), '--glb', glb, ...args], {
    env: { ...process.env, HOME: home, ...env }, stdio: ['ignore', 'pipe', 'pipe'],
  });
  const out = { text: '' };
  child.stdout.on('data', (c) => { out.text += c; });
  child.stderr.on('data', (c) => { out.text += c; });
  const ended0 = new Promise((resolve) => child.on('close', resolve));
  // A test that failed leaves try.js running: end it as a user would (SIGTERM takes its twin down too), and only then kill.
  t.after(async () => { child.kill('SIGTERM'); await Promise.race([ended0, new Promise((r) => setTimeout(r, 10000))]); child.kill('SIGKILL'); });
  const ended = new Promise((resolve) => child.on('close', (code) => resolve(code)));
  const url = await Promise.race([
    new Promise((resolve) => { const timer = setInterval(() => { const m = /(http:\/\/[\d.a-z]+:\d+)\/\?t=(\S+)/.exec(out.text); if (m) { clearInterval(timer); resolve({ base: `http://127.0.0.1:${new URL(m[1]).port}`, token: m[2] }); } }, 50); }),
    ended.then((code) => { throw new Error(`ended ${code}: ${out.text}`); }),
  ]);
  return { child, out, ended, ...url };
}

export const secretsOf = (fake) => [fake.code, fake.pin];

// The server, not the page, is what refuses: a raw /live call is held to what the table draws.
export async function raw(real) {
  const { WebSocket } = await import('ws');
  const socket = new WebSocket(`ws://127.0.0.1:${new URL(real.base).port}/live?t=${real.token}`, { headers: { origin: real.base } });
  const messages = [];
  socket.on('message', (d) => messages.push(JSON.parse(d)));
  await new Promise((resolve) => socket.on('open', resolve));
  let id = 0;
  const press = async (name, args = {}) => {
    const mine = ++id;
    socket.send(JSON.stringify({ type: 'call', id: mine, name, args }));
    for (let i = 0; i < 80 && !messages.some((m) => m.type === 'result' && m.id === mine); i += 1) await new Promise((r) => setTimeout(r, 50));
    return messages.find((m) => m.type === 'result' && m.id === mine);
  };
  return { socket, messages, press, send: (m) => socket.send(JSON.stringify(m)) };
}
