// The try-it page's STOP (a real run against the SDK's fake lamp): one press ends what the page sends and HOLDS the pose where it is (the
// body lease kept, one last ask naming the present pose); a second press gives the body back; no drag goes out between; the button is on the page.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { FakeLamp } from '../src/testing.js';
import { raw, run } from './try-real-helper.js';

const until = async (what, ok, ms = 8000) => {
  for (let i = 0; i < ms / 50 && !ok(); i += 1) await new Promise((r) => setTimeout(r, 50));
  assert.ok(ok(), what);
};

test('STOP holds the pose and sends nothing more; Resume sends again; the second press gives the body back', async (t) => {
  const fake = await FakeLamp.start({ name: 'Desk lamp', grants: ['body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  const html = await (await fetch(`${real.base}/?t=${real.token}`)).text();
  assert.match(html, /id="stopbtn"/, 'the STOP button is on the page');
  assert.match(html, /id="givebtn"/, 'and the second press, to let go');

  page.send({ type: 'moves_ok' });
  page.send({ type: 'drag', joint: 'pitch', deg: 3, final: true });
  await until('a drag reaches the lamp', () => fake.asked('joints').length >= 1);
  assert.equal(fake.leases.size, 1, 'the slider holds the body lease');
  // The body's state, as the page draws it: the present pose is what STOP holds.
  fake.emit('body.state', { joints: { pitch: { deg: 5.04, goal_deg: 8 }, shoulder: { deg: 20.26 }, elbow: { deg: 30 }, yaw: { deg: 0 }, roll: { deg: 0 } } });
  await new Promise((r) => setTimeout(r, 300));

  page.send({ type: 'stop_all' });
  await until('the page says it is done', () => page.messages.some((m) => m.type === 'stopped' && m.done === true));
  const done = page.messages.filter((m) => m.type === 'stopped' && m.done).at(-1);
  assert.equal(done.hold, true, 'the page is told the pose is held (the second press is offered)');
  assert.match(done.text, /Letting go is a second press/);
  assert.match(done.text, /70 to 90 degrees a second/, 'it says how fast the arm goes back when let go');
  assert.equal(fake.leases.size, 1, 'the body lease is KEPT: nothing eases the arm back');
  assert.ok(fake.asked('stop').length >= 1, 'the client\'s own actions are ended (lamp.stop)');
  // The last ask names the present pose (this fake refuses the arm, so it falls back to the head, which it says).
  assert.deepEqual(fake.asked('joints').at(-1).targets, { pitch: 5 }, 'one last ask holds the present pitch');
  assert.match(done.text, /only the head is held/);

  const asked = fake.asked('joints').length;
  for (const deg of [4, 5, 6]) page.send({ type: 'drag', joint: 'pitch', deg, final: deg === 6 });
  await new Promise((r) => setTimeout(r, 600));
  assert.equal(fake.asked('joints').length, asked, 'no drag goes out after STOP');
  assert.equal(fake.leases.size, 1, 'and the lease is still held, not let go by a timer');

  page.send({ type: 'resume' });
  await until('resumed', () => page.messages.some((m) => m.type === 'resumed'));
  page.send({ type: 'drag', joint: 'pitch', deg: 2, final: true });
  await until('a drag goes out again after Resume', () => fake.asked('joints').length > asked);

  page.send({ type: 'stop_all' });
  await until('stopped again', () => page.messages.filter((m) => m.type === 'stopped' && m.done).length >= 2);
  page.send({ type: 'give_back' });
  await until('the body lease is given back', () => fake.leases.size === 0);
  await until('the page says so', () => page.messages.some((m) => m.type === 'stopped' && /LET GO/.test(m.text)));
  const after = fake.asked('joints').length;
  page.send({ type: 'drag', joint: 'pitch', deg: 1, final: true });
  await new Promise((r) => setTimeout(r, 400));
  assert.equal(fake.asked('joints').length, after, 'after the let-go nothing goes out until Resume');
});

test('STOP with no body lease of this page\'s own takes none and sends no joints: the body is left to the lamp', async (t) => {
  const fake = await FakeLamp.start({ name: 'Desk lamp', grants: ['body'] });
  const home = mkdtempSync(join(tmpdir(), 'try-real-home-'));
  t.after(async () => { await fake.stop(); rmSync(home, { recursive: true, force: true }); });
  const real = await run(t, { args: ['--host', fake.host, '--port', String(fake.port)], env: { LAMP_TRY_CODE: fake.code, LAMP_TRY_PIN: fake.pin }, fake, home });
  const page = await raw(real);
  t.after(() => page.socket.close());
  page.send({ type: 'moves_ok' });
  // The body says where it is, as it does all the time: STOP must not use that to take the body and hold it.
  fake.emit('body.state', { joints: { pitch: { deg: 5, goal_deg: 5 }, shoulder: { deg: 20 }, elbow: { deg: 30 }, yaw: { deg: 0 }, roll: { deg: 0 } } });
  await new Promise((r) => setTimeout(r, 300));
  const leasesAsked = fake.asked('lease').length;
  const joints = fake.asked('joints').length;
  assert.equal(fake.leases.size, 0, 'this page holds no lease before STOP');

  page.send({ type: 'stop_all' });
  await until('the page says it is done', () => page.messages.some((m) => m.type === 'stopped' && m.done === true));
  const done = page.messages.filter((m) => m.type === 'stopped' && m.done).at(-1);
  assert.equal(done.hold, false, 'nothing is held, so no second press is offered');
  assert.match(done.text, /held no body lease, so the body is left to the lamp/);
  assert.equal(fake.leases.size, 0, 'STOP took no lease');
  assert.equal(fake.asked('lease').length, leasesAsked, 'and asked for none');
  assert.equal(fake.asked('joints').length, joints, 'and sent no joints');
  assert.ok(fake.asked('stop').length >= 1, 'it still ends the client\'s own actions (lamp.stop)');
});
