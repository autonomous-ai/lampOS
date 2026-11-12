// The try-it app on a real lamp (twin/real.js, try.js --real), against the SDK's fake lamp: the code and pin never leave the
// environment, the pairing folder is private and goes with the run, the page is a real-lamp page (banner data, grants, no
// stimuli, rows that say why they cannot run), and what it prints holds none of the secrets.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';
import { credentials, parseReal } from '../examples/twin/real.js';

const tryJs = new URL('../examples/twin/try.js', import.meta.url).pathname;
const freePort = () => new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); }); });

test('the code and the pin are refused on the command line, with the way to give them', () => {
  assert.throws(() => parseReal(['--real', '--host', 'h', '--pin', 'abc']), /--pin on the command line shows in `ps`: give it as LAMP_TRY_PIN/);
  assert.throws(() => parseReal(['--real', '--host', 'h', '--code=12345678']), /--code on the command line shows in `ps`: give it as LAMP_TRY_CODE/);
  assert.throws(() => parseReal(['--real']), /--real needs --host H/);
  assert.throws(() => parseReal(['--real', '--host', 'h', '--keyring', 'd']), /give one of them/);
  assert.throws(() => parseReal(['--real', '--host', 'h', '--nope']), /unknown argument --nope/);
  assert.deepEqual([...parseReal(['--real', '--host', 'h', '--camera']).flags].sort(), ['camera', 'real']);
});

test('the code and pin come from the environment first, then a prompt that is asked in order', async () => {
  assert.deepEqual(await credentials({ LAMP_TRY_CODE: '11112222', LAMP_TRY_PIN: 'ab' }, async () => { throw new Error('asked'); }), { code: '11112222', pin: 'ab' });
  const asked = [];
  const got = await credentials({}, async (label) => { asked.push(label); return `v${asked.length}`; });
  assert.deepEqual(got, { code: 'v1', pin: 'v2' });
  assert.match(asked[0], /code/i);
  assert.match(asked[1], /pin/i);
});

async function run(t, { args = [], env = {}, home }) {
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

const secretsOf = (fake) => [fake.code, fake.pin];

test('a code run pairs as try-controller into a private folder, serves a real-lamp page, and leaves nothing behind on SIGINT', async (t) => {
  const fake = await FakeLamp.start({ name: 'Desk lamp', grants: ['body', 'light', 'speak', 'camera'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  // The private folder, named by the run's pid, mode 0700, holding the pairing.
  const folders = readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-'));
  assert.equal(folders.length, 1);
  assert.equal(statSync(join(home, folders[0])).mode & 0o777, 0o700);
  assert.deepEqual([...fake.clients.values()].map((c) => c.name), ['try-controller']);
  // The page's table.
  const table = await (await fetch(`${real.base}/try.json?t=${real.token}`)).json();
  assert.equal(table.mode, 'real');
  assert.deepEqual(table.stimuli, [], 'no stimuli on a real lamp');
  assert.deepEqual(table.lamp, { id: 'lamp-fake', name: 'Desk lamp' });
  assert.deepEqual([...table.grants].sort(), ['body', 'camera', 'light', 'speak']);
  assert.equal(table.host, fake.host);
  const row = (id) => table.controls.find((c) => c.id === id);
  assert.equal(row('camera_snapshot').disabled, 'start the run with --camera', 'the grant is there, the run did not say yes');
  assert.equal(row('microphone_stream').disabled, 'the lamp did not grant microphone');
  assert.equal(row('notes_set').disabled, 'the lamp did not grant notes');
  assert.equal(row('gesture').disabled, undefined);
  assert.equal(row('gesture').moves, true, 'a row that moves the body says so');
  assert.match(row('listen').hint, /real lamp's mute/);
  assert.equal(table.camera, false);
  // A stimulus is refused: there is no lamp-sim.
  const { WebSocket } = await import('ws');
  const socket = new WebSocket(`ws://127.0.0.1:${new URL(real.base).port}/live?t=${real.token}`, { headers: { origin: real.base } });
  const messages = [];
  socket.on('message', (d) => messages.push(JSON.parse(d)));
  await new Promise((resolve) => socket.on('open', resolve));
  socket.send(JSON.stringify({ type: 'stim', id: 1, name: 'arrive', args: {} }));
  for (let i = 0; i < 60 && !messages.some((m) => m.type === 'result'); i += 1) await new Promise((r) => setTimeout(r, 50));
  socket.close();
  assert.match(messages.find((m) => m.type === 'result').answer, /no stimuli on a real lamp/);
  // SIGINT: the folder goes, the client to revoke is said.
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.deepEqual(readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-')), [], 'the pairing folder is removed');
  assert.match(real.out.text, /revoke it in the app, or on the lamp: lamp-control revoke c_0001/);
  assert.match(real.out.text, /client try-controller \(c_0001\)/);
  for (const secret of [...secretsOf(fake), join(home, folders[0]), folders[0]]) assert.equal(real.out.text.includes(secret), false, `printed: ${secret.slice(0, 6)}...`);
});

test('SIGTERM ends a code run the same way, and a wrong code says the lamp\'s words and leaves no folder', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port), '--camera'], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  assert.match(real.out.text, /CAMERA ON: anyone on your network who has this link can see or hear the room/);
  real.child.kill('SIGTERM');
  assert.equal(await real.ended, 0);
  assert.deepEqual(readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-')), []);
  // A wrong code.
  const bad = spawn(process.execPath, [tryJs, '--real', '--host', fake.host, '--port', String(fake.port)], { env: { ...process.env, HOME: home, LAMP_TRY_CODE: '00000000', LAMP_TRY_PIN: fake.pin }, stdio: ['ignore', 'pipe', 'pipe'] });
  let text = '';
  bad.stdout.on('data', (c) => { text += c; });
  bad.stderr.on('data', (c) => { text += c; });
  const code = await new Promise((resolve) => bad.on('close', resolve));
  assert.equal(code, 1);
  assert.match(text, /the code does not match/);
  for (const secret of ['00000000', fake.pin]) assert.equal(text.includes(secret), false);
  assert.deepEqual(readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-')), []);
});

test('--keyring reuses a pairing: no code is asked for, the folder is never removed, and no client is said to revoke', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light', 'body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  const kept = join(home, 'kept');
  mkdirSync(kept, { mode: 0o700 });
  chmodSync(kept, 0o700);
  await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'mine', keyring: new Keyring(kept) });
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--keyring', kept], env: { LAMP_TRY_CODE: '', LAMP_TRY_PIN: '' }, fake, home });
  const table = await (await fetch(`${real.base}/try.json?t=${real.token}`)).json();
  assert.equal(table.mode, 'real');
  assert.deepEqual([...table.grants].sort(), ['body', 'light']);
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.equal(existsSync(kept), true, 'a keyring that was given stays');
  assert.equal(/revoke/.test(real.out.text), false);
  assert.equal(real.out.text.includes(kept), false, 'its path is not printed');
  assert.deepEqual([...fake.clients.values()].map((c) => c.name), ['mine'], 'no second client');
});

// The server, not the page, is what refuses: a raw /live call is held to what the table draws.
async function raw(real) {
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

test('a raw call of a camera row without --camera is refused and no picture comes; with --camera it is shown', async (t) => {
  const fake = await FakeLamp.start({ grants: ['camera'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const without = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(without);
  t.after(() => page.socket.close());
  const refused = await page.press('camera_snapshot');
  assert.equal(refused.ok, false);
  assert.match(refused.answer, /n\/a: start the run with --camera/);
  assert.equal(page.messages.some((m) => m.type === 'picture'), false, 'no picture');
  assert.equal(fake.asked('camera.snapshot').length, 0, 'the lamp was never asked');
  without.child.kill('SIGINT');
  assert.equal(await without.ended, 0);
  const fake2 = await FakeLamp.start({ grants: ['camera'] });
  t.after(() => fake2.stop());
  const home2 = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(() => rmSync(home2, { recursive: true, force: true }));
  const withFlag = await run(t, { args: ['--host', fake2.host, '--port', String(fake2.port), '--camera'], env: { LAMP_TRY_CODE: fake2.code, LAMP_TRY_PIN: fake2.pin }, fake: fake2, home: home2 });
  const page2 = await raw(withFlag);
  t.after(() => page2.socket.close());
  assert.equal((await page2.press('camera_snapshot')).ok, true);
  assert.equal(page2.messages.filter((m) => m.type === 'picture').length, 1);
});

test('the say row speaks its own default on a real lamp, not the virtual lamp\'s, and keeps the virtual one on lamp-sim', async (t) => {
  const { describe, checked, CONTROLS } = await import('../examples/twin/controls.js');
  const say = (mode) => describe(mode, { grants: ['speak'] }).controls.find((c) => c.id === 'say').fields[0];
  assert.equal(say('lamp-sim').default, 'Hello, I am the virtual lamp.');
  assert.equal(say('real').default, 'Hello from the try-it page.');
  assert.equal('defaultReal' in say('real'), false, 'the page is not told the other default');
  // A real lamp's page never says "virtual lamp" (a hint of the virtual lamp's gets its hintReal).
  assert.equal(JSON.stringify(describe('real', { grants: ['speak', 'hear'] })).includes('virtual lamp'), false);
  const fields = CONTROLS.find((c) => c.id === 'say').fields;
  assert.equal(checked(fields, {}, 'real').text, 'Hello from the try-it page.');
  assert.equal(checked(fields, {}).text, 'Hello, I am the virtual lamp.');
  assert.equal(checked(fields, { text: 'mine' }, 'real').text, 'mine');
  // On the page's real run, a say with no text sends the real default to the lamp.
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  assert.equal((await page.press('say')).ok, true);
  assert.deepEqual(fake.asked('say'), [{ text: 'Hello from the try-it page.' }]);
});

test('a note key that the run\'s prefix would push past the lamp\'s 64 bytes is refused in words; the longest that fits is taken', async (t) => {
  const fake = await FakeLamp.start({ grants: ['notes'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  const longest = await page.press('notes_set', { key: 'k'.repeat(53), value: '1' });
  assert.equal(longest.ok, true, longest.answer);
  const over = await page.press('notes_set', { key: 'k'.repeat(54), value: '1' });
  assert.equal(over.ok, false);
  assert.match(over.answer, /1 to 53 bytes here \(this run's own prefix takes 11 of the lamp's 64\)/);
  const listed = await page.press('notes_list', { prefix: 'p'.repeat(54) });
  assert.equal(listed.ok, false);
  assert.match(listed.answer, /1 to 53 bytes here/);
  assert.equal(fake.asked('notes.set').length, 1, 'the lamp was asked once: the refused ones never reached it');
});

test('the try-wave skill this run pushed and did not remove is taken off the lamp at the end; one it removed is not removed twice', async (t) => {
  const fake = await FakeLamp.start({ grants: ['skills', 'body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  assert.equal((await page.press('skill_push')).ok, true);
  assert.equal(fake.pushed.size, 1, 'the skill is on the lamp');
  page.socket.close();
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.equal(fake.pushed.size, 0, 'taken off at the end');
  assert.equal(fake.asked('skill.remove').length, 1);
  // Pushed and removed by the person: nothing is asked again.
  const again = await FakeLamp.start({ grants: ['skills', 'body'] });
  t.after(() => again.stop());
  const home2 = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(() => rmSync(home2, { recursive: true, force: true }));
  const second = await run(t, { args: ['--host', again.host, '--port', String(again.port)], env: { LAMP_TRY_CODE: again.code, LAMP_TRY_PIN: again.pin }, home: home2 });
  const page2 = await raw(second);
  await page2.press('skill_push');
  await page2.press('skill_remove');
  page2.socket.close();
  second.child.kill('SIGINT');
  assert.equal(await second.ended, 0);
  assert.equal(again.asked('skill.remove').length, 1, 'only the person\'s own remove');
});

test('the twin started without try.js\'s keyring is refused, and does not fall to the SDK\'s default one', async () => {
  const { spawnSync } = await import('node:child_process');
  const env = { ...process.env };
  delete env.LAMP_TRY_KEYRING;
  const alone = spawnSync(process.execPath, [new URL('../examples/twin/server.js', import.meta.url).pathname, '--real', '--port', '0'], { env, encoding: 'utf8', timeout: 15000 });
  assert.equal(alone.status, 2);
  assert.match(alone.stderr, /a real run takes its keyring from try\.js --real \(LAMP_TRY_KEYRING\)/);
});

test('a raw call of a row that moves the lamp is refused until the page says yes, and runs after', async (t) => {
  const fake = await FakeLamp.start({ grants: ['body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  const before = await page.press('gesture', { name: 'nod' });
  assert.equal(before.ok, false);
  assert.match(before.answer, /this moves the real lamp/);
  assert.equal(fake.asked('gesture').length, 0, 'the lamp was never asked');
  page.send({ type: 'moves_ok' });
  const after = await page.press('gesture', { name: 'nod' });
  assert.equal(after.ok, true, after.answer);
  assert.equal(fake.asked('gesture').length, 1);
});

test('this run\'s scratch notes and clip come off the lamp when the real run ends', async (t) => {
  const fake = await FakeLamp.start({ grants: ['notes', 'speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const { WebSocket } = await import('ws');
  const socket = new WebSocket(`ws://127.0.0.1:${new URL(real.base).port}/live?t=${real.token}`, { headers: { origin: real.base } });
  const results = [];
  socket.on('message', (d) => { const m = JSON.parse(d); if (m.type === 'result') results.push(m); });
  await new Promise((resolve) => socket.on('open', resolve));
  const press = async (id, name, args = {}) => { socket.send(JSON.stringify({ type: 'call', id, name, args })); for (let i = 0; i < 80 && !results.some((r) => r.id === id); i += 1) await new Promise((r) => setTimeout(r, 50)); return results.find((r) => r.id === id); };
  const note = await press(1, 'notes_set', { key: 'mine', value: '1' });
  assert.match(note.call, /^lamp\.notesSet\("try-[0-9a-f]{6}\.mine"/, 'a scratch key with the run\'s prefix');
  assert.equal((await press(2, 'sound_upload', { ms: 200 })).ok, true);
  const [client] = [...fake.notes.keys()];
  assert.equal(fake.notes.get(client).size, 1);
  assert.equal(fake.clips.get(client).size, 1);
  socket.close();
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.equal(fake.notes.get(client).size, 0, `the run's notes are gone: ${real.out.text}`);
  assert.equal(fake.clips.get(client).size, 0, 'the run\'s clip is gone');
});

// What a terminal does: Ctrl-C reaches try.js AND (if it shared the group) the twin; then try.js asks the twin to end.
test('a Ctrl-C that reaches both processes still takes this run\'s notes and clip off the lamp, and the exit is 0', async (t) => {
  const { execFileSync } = await import('node:child_process');
  const fake = await FakeLamp.start({ grants: ['notes', 'speak'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const { WebSocket } = await import('ws');
  const socket = new WebSocket(`ws://127.0.0.1:${new URL(real.base).port}/live?t=${real.token}`, { headers: { origin: real.base } });
  const results = [];
  socket.on('message', (d) => { const m = JSON.parse(d); if (m.type === 'result') results.push(m); });
  await new Promise((resolve) => socket.on('open', resolve));
  const press = async (id, name, args = {}) => { socket.send(JSON.stringify({ type: 'call', id, name, args })); for (let i = 0; i < 80 && !results.some((r) => r.id === id); i += 1) await new Promise((r) => setTimeout(r, 50)); };
  await press(1, 'notes_set', { key: 'mine', value: '1' });
  await press(2, 'sound_upload', { ms: 200 });
  socket.close();
  const [client] = [...fake.notes.keys()];
  assert.equal(fake.notes.get(client).size, 1);
  assert.equal(fake.clips.get(client).size, 1);
  // The twin is try.js's child, in a group of its own: a terminal's signal does not reach it, but send it anyway.
  const twin = Number(execFileSync('ps', ['--ppid', String(real.child.pid), '-o', 'pid=']).toString().trim().split('\n')[0]);
  assert.ok(twin > 0, 'the twin is a child of try.js');
  assert.notEqual(Number(execFileSync('ps', ['-o', 'pgid=', '-p', String(twin)]).toString().trim()), Number(execFileSync('ps', ['-o', 'pgid=', '-p', String(real.child.pid)]).toString().trim()), 'the twin has its own process group');
  process.kill(twin, 'SIGINT');
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.equal(fake.notes.get(client).size, 0, `the run's notes are gone: ${real.out.text}`);
  assert.equal(fake.clips.get(client).size, 0, 'the run\'s clip is gone');
});

test('if try.js dies without a word the twin cleans up and ends by itself', async (t) => {
  const { execFileSync } = await import('node:child_process');
  const fake = await FakeLamp.start({ grants: ['notes'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const { WebSocket } = await import('ws');
  const socket = new WebSocket(`ws://127.0.0.1:${new URL(real.base).port}/live?t=${real.token}`, { headers: { origin: real.base } });
  const results = [];
  socket.on('message', (d) => { const m = JSON.parse(d); if (m.type === 'result') results.push(m); });
  await new Promise((resolve) => socket.on('open', resolve));
  socket.send(JSON.stringify({ type: 'call', id: 1, name: 'notes_set', args: { key: 'mine', value: '1' } }));
  for (let i = 0; i < 80 && !results.length; i += 1) await new Promise((r) => setTimeout(r, 50));
  socket.close();
  const [client] = [...fake.notes.keys()];
  const twin = Number(execFileSync('ps', ['--ppid', String(real.child.pid), '-o', 'pid=']).toString().trim().split('\n')[0]);
  real.child.kill('SIGKILL');
  await real.ended;
  for (let i = 0; i < 100 && fake.notes.get(client).size > 0; i += 1) await new Promise((r) => setTimeout(r, 100));
  assert.equal(fake.notes.get(client).size, 0, 'the twin took the run\'s notes off the lamp');
  for (let i = 0; i < 50; i += 1) { try { process.kill(twin, 0); } catch { break; } await new Promise((r) => setTimeout(r, 100)); }
  assert.throws(() => process.kill(twin, 0), 'the twin has ended');
  // A crash leaves the private folder (and no revoke line was said): the person removes it, the doc says so.
  assert.equal(readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-')).length, 1);
});

test('a pin that does not match the lamp is said without the pin\'s digits', async (t) => {
  const fake = await FakeLamp.start({ grants: ['light'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const wrong = 'ab12cd34ef56ab78'.repeat(4);
  const bad = spawn(process.execPath, [tryJs, '--real', '--host', fake.host, '--port', String(fake.port)], { env: { ...process.env, HOME: home, LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: wrong }, stdio: ['ignore', 'pipe', 'pipe'] });
  let text = '';
  bad.stdout.on('data', (c) => { text += c; });
  bad.stderr.on('data', (c) => { text += c; });
  assert.equal(await new Promise((resolve) => bad.on('close', resolve)), 1);
  assert.match(text, /is not the lamp's/);
  assert.equal(text.includes(wrong.slice(0, 16)), false, 'the pin\'s first digits are not printed');
  assert.equal(text.includes(fake.code), false);
  assert.deepEqual(readdirSync(home).filter((n) => n.startsWith('.lampos-try-real-')), []);
});
