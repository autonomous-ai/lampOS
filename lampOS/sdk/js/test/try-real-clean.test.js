// The try-it app on a real lamp, the run's end and its guards: the scratch it pushed comes off the lamp, a Ctrl-C and a dead parent
// still clean up, a moving row waits for the page's yes, the keyring is try.js's alone. (Split from try-real.test.js; see
// test/try-real-helper.js.)
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { run, raw } from './try-real-helper.js';

// The first child of `pid`, by one `ps` call both BSD (macOS) and GNU procps take: `ps --ppid` is procps only, and on macOS
// it exits 1 with `illegal option -- -`.
const childOf = (pid) => Number(execFileSync('ps', ['-A', '-o', 'pid=', '-o', 'ppid=']).toString().trim().split('\n')
  .map((line) => line.trim().split(/\s+/).map(Number)).find(([, parent]) => parent === pid)?.[0]);

const tryJs = new URL('../examples/twin/try.js', import.meta.url).pathname;

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
  const twin = childOf(real.child.pid);
  assert.ok(twin > 0, 'the twin is a child of try.js');
  assert.notEqual(Number(execFileSync('ps', ['-o', 'pgid=', '-p', String(twin)]).toString().trim()), Number(execFileSync('ps', ['-o', 'pgid=', '-p', String(real.child.pid)]).toString().trim()), 'the twin has its own process group');
  process.kill(twin, 'SIGINT');
  real.child.kill('SIGINT');
  assert.equal(await real.ended, 0);
  assert.equal(fake.notes.get(client).size, 0, `the run's notes are gone: ${real.out.text}`);
  assert.equal(fake.clips.get(client).size, 0, 'the run\'s clip is gone');
});

test('if try.js dies without a word the twin cleans up and ends by itself', async (t) => {
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
  const twin = childOf(real.child.pid);
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
