// The try-it app on a real lamp (twin/real.js, try.js --real), against the SDK's fake lamp: the code and pin never leave the
// environment, the pairing folder is private and goes with the run, the page is a real-lamp page (banner data, grants, no
// stimuli, rows that say why they cannot run), and what it prints holds none of the secrets. (The run's own clean-up, the
// moving rows' yes and the dead-parent cases are in try-real-clean.test.js.)
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';
import { credentials, parseReal } from '../examples/twin/real.js';
import { run, raw, secretsOf } from './try-real-helper.js';

const tryJs = new URL('../examples/twin/try.js', import.meta.url).pathname;

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
