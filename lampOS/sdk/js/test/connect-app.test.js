// An installed app's socket (#7415), to nine's messages (piece 3): `Lamp.connectApp()` reads LAMP_APP_SOCKET, needs no
// pairing, key or token, and gets the manifest's grants; `listenFor`, `took`, `settings` and `panel` are its ops, and
// `app.trigger`, `app.settings` and `phrase.heard` its events. The fake lamp serves the same.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Lamp, byCode } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

const settle = () => new Promise((resolve) => setTimeout(resolve, 60));

async function withApp(options, body) {
  const fake = await FakeLamp.start({ app: true, ...options });
  const lamp = await Lamp.connectApp({ path: fake.appPath });
  try { await body(fake, lamp); } finally { await lamp.close().catch(() => {}); await fake.stop(); }
}

test('connectApp by path gets the manifest\'s grants, the app\'s name and version', async () => {
  await withApp({ appName: 'lessons', appGrants: ['speak', 'hear'], appVersion: '1.2.0' }, async (fake, lamp) => {
    assert.deepEqual(lamp.grants, ['speak', 'hear']);
    assert.equal(lamp.app, 'lessons');
    assert.equal(lamp.appVersion, '1.2.0');
  });
});

test('connectApp reads LAMP_APP_SOCKET, and says which variable when there is no socket', async () => {
  const fake = await FakeLamp.start({ app: true });
  process.env.LAMP_APP_SOCKET = fake.appPath;
  try {
    const lamp = await Lamp.connectApp();
    assert.ok(lamp.grants.includes('speak'));
    await lamp.close();
    process.env.LAMP_APP_SOCKET = `${fake.appPath}.none`;
    await assert.rejects(Lamp.connectApp(), (error) => error.code === 'no_app_socket' && /LAMP_APP_SOCKET/.test(error.message));
  } finally { delete process.env.LAMP_APP_SOCKET; await fake.stop(); }
});

test('pairing is refused on an app\'s socket', async () => {
  await withApp({}, async (fake, lamp) => {
    await assert.rejects(lamp.request('pair', { name: 'x', key: 'AAAA', proof: 'AAAA' }), (error) => error.code === 'bad_request');
  });
});

test('listenFor replaces the phrases whole and tells only the phrase', async () => {
  await withApp({}, async (fake, lamp) => {
    const heard = [];
    const answer = await lamp.listenFor(['lesson time', 'next word please'], (event) => heard.push(event));
    assert.equal(answer.phrases, 2);
    fake.hearPhrase('lesson time');
    fake.hearPhrase('something else');
    await settle();
    assert.deepEqual(heard.map((e) => e.phrase), ['lesson time']);
    assert.deepEqual(Object.keys(heard[0]).sort(), ['at', 'ev', 'phrase']);
    await lamp.listenFor(['another phrase']);
    fake.hearPhrase('lesson time');
    await settle();
    assert.equal(heard.length, 1);
    assert.equal((await lamp.listenFor([])).phrases, 0);
  });
});

test('listenFor keeps the door\'s limits and needs hear', async () => {
  await withApp({ appGrants: ['speak'] }, async (fake, lamp) => {
    await assert.rejects(lamp.listenFor(['lesson time']), (error) => error.code === 'no_grant' && /hear/.test(error.message));
  });
  await withApp({}, async (fake, lamp) => {
    await assert.rejects(lamp.listenFor(['short']), (error) => error.code === 'bad_request');
    await assert.rejects(lamp.listenFor(Array.from({ length: 17 }, (_, n) => `phrase number ${String.fromCharCode(97 + n)}${String.fromCharCode(97 + n)}`)), (error) => error.code === 'bad_request');
    await assert.rejects(lamp.request('phrases.set', { phrases: ['a'.repeat(65)] }), (error) => error.code === 'bad_request');
  });
});

test('a manifest trigger comes unasked and took it is kept', async () => {
  await withApp({}, async (fake, lamp) => {
    const got = [];
    await lamp.on('app.trigger', (event) => got.push(event));
    fake.trigger('start', 'start my english lesson', 't-9');
    await settle();
    assert.equal(got[0].trigger, 'start');
    assert.equal(got[0].turn, 't-9');
    assert.equal(got[0].phrase, 'start my english lesson');
    await lamp.took(got[0].turn);
    assert.deepEqual(fake.appTookTurns, ['t-9']);
  });
});

test('settings get and the owner\'s change event', async () => {
  await withApp({ appSettings: { level: 'a1', hints: true } }, async (fake, lamp) => {
    assert.deepEqual(await lamp.settings(), { level: 'a1', hints: true });
    const changes = [];
    await lamp.on('app.settings', (event) => changes.push(event));
    fake.setAppSetting({ level: 'b2' });
    await settle();
    assert.deepEqual(changes[0].changed, ['level']);
    assert.deepEqual(changes[0].settings, { level: 'b2', hints: true });
    assert.equal((await lamp.settings()).level, 'b2');
  });
});

test('the panel counts up and is one a minute', async () => {
  await withApp({}, async (fake, lamp) => {
    assert.equal(await lamp.panel('Lessons', ['3 words today', 'streak: 4']), 1);
    assert.deepEqual(fake.appPanel, { title: 'Lessons', lines: ['3 words today', 'streak: 4'], rev: 1 });
    await assert.rejects(lamp.panel('Lessons', ['again']), (error) => error.code === 'rate' && error.retryMs > 0);
  });
  await withApp({ panelRateS: 0 }, async (fake, lamp) => {
    assert.equal(await lamp.panel('A'), 1);
    assert.equal(await lamp.panel('B'), 2);
  });
});

test('the panel is text in its limits', async () => {
  await withApp({}, async (fake, lamp) => {
    for (const [title, lines] of [['', []], ['x'.repeat(41), []], ['ok', Array(7).fill('l')], ['ok', ['x'.repeat(81)]], ['ok', ['a\nb']], ['a\x07', []]]) {
      await assert.rejects(lamp.panel(title, lines), (error) => error.code === 'bad_request', JSON.stringify([title, lines]));
    }
  });
});

test('the owner stopping the app closes with the reason and no event', async () => {
  await withApp({}, async (fake, lamp) => {
    const seen = [];
    await lamp.on('app.settings', (event) => seen.push(event));
    const closed = new Promise((resolve) => lamp.ws.once('close', resolve));
    fake.stopApp('the owner stopped the app');
    await closed;
    assert.equal(lamp.closeReason, 'the owner stopped the app');
    assert.deepEqual(seen, []);
  });
});

test('off unsubscribes a door event on an app\'s socket (Heron\'s P2 on #7672)', async () => {
  await withApp({ appGrants: ['speak', 'hear', 'look'] }, async (fake, lamp) => {
    const handler = () => {};
    await lamp.on('person.arrived', handler);
    await lamp.off('person.arrived', handler);
    assert.deepEqual(fake.requests.map((r) => r.op), ['subscribe', 'unsubscribe']);
    // phrase.heard and the app's own events were never subscribed: nothing to take back.
    await lamp.on('app.trigger', handler);
    await lamp.off('app.trigger', handler);
    assert.deepEqual(fake.requests.map((r) => r.op), ['subscribe', 'unsubscribe']);
  });
});

test('on phrase.heard with phrases on an app\'s socket points to listenFor', async () => {
  await withApp({}, async (fake, lamp) => {
    assert.throws(() => lamp.on('phrase.heard', () => {}, { phrases: ['lesson time'] }), (error) => error.code === 'bad_request' && /listenFor/.test(error.message));
  });
});

test('a refused listenFor leaves no handler behind', async () => {
  await withApp({ appGrants: ['speak'] }, async (fake, lamp) => {
    await assert.rejects(lamp.listenFor(['lesson time'], () => {}), (error) => error.code === 'no_grant');
    assert.equal((lamp.handlers.get('phrase.heard') ?? []).length, 0);
  });
});

test('an empty panel line is refused by the SDK and the fake alike, and panel(t, null) is the SDK\'s own', async () => {
  await withApp({}, async (fake, lamp) => {
    await assert.rejects(lamp.panel('ok', ['a', '', 'b']), (error) => error.code === 'bad_request');
    await assert.rejects(lamp.request('app.panel.set', { title: 'ok', lines: ['a', '', 'b'] }), (error) => error.code === 'bad_request');
    assert.equal(await lamp.panel('ok', null), 1);
  });
});

// What an app tells Lamp's memory: `remember` (apps.remember) and `context` (apps.context), with the `remember` grant.
const MEMORY_GRANTS = ['speak', 'hear', 'remember'];
const MEMORY_REFUSALS = ['off', 'no_grant', 'bad_request', 'private', 'unavailable', 'rate', 'refused'];

test('remember keeps a fact and says kept', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    assert.equal(await lamp.remember('squats 3x10, the 6th too shallow'), true);
    assert.deepEqual(fake.asked('apps.remember'), [{ text: 'squats 3x10, the 6th too shallow' }]);
    assert.deepEqual(fake.appFacts, ['squats 3x10, the 6th too shallow']);
  });
});

test('remember and context need the remember grant', async () => {
  await withApp({}, async (fake, lamp) => {
    await assert.rejects(lamp.remember('squats'), byCode.no_grant);
    await assert.rejects(lamp.context('set 1 of 3'), byCode.no_grant);
  });
});

test('each memory refusal is its own error', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    for (const [op, call] of [['apps.remember', () => lamp.remember('squats')], ['apps.context', () => lamp.context('set 1 of 3')]]) {
      for (const code of MEMORY_REFUSALS) {
        fake.refuseNext(op, code, `the lamp says ${code}`, code === 'rate' ? 1234 : undefined);
        await assert.rejects(call(), (error) => error instanceof byCode[code] && error.code === code
          && error.why === `the lamp says ${code}` && error.retryMs === (code === 'rate' ? 1234 : undefined), `${op} ${code}`);
      }
    }
    assert.deepEqual(fake.appFacts, []);
    assert.equal(fake.appContext, null);
  });
});

test('remember is three a minute with retryMs', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    for (let n = 0; n < 3; n++) assert.equal(await lamp.remember(`fact ${n}`), true);
    await assert.rejects(lamp.remember('fact 3'), (error) => error instanceof byCode.rate && error.retryMs > 0 && error.retryMs <= 60_000);
    assert.deepEqual(fake.appFacts, ['fact 0', 'fact 1', 'fact 2']);
  });
});

test('remember is refused while private and over 280', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    await assert.rejects(lamp.remember('x'.repeat(281)), byCode.bad_request);
    fake.setPrivate(true);
    await assert.rejects(lamp.remember('squats'), byCode.private);
    await assert.rejects(lamp.context('set 1 of 3'), byCode.private);
    assert.deepEqual(fake.appFacts, []);
  });
});

test('context is renewed each rep and cleared at the end', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    let now = 100_000;
    fake.clock = () => now;
    await lamp.context('set 2 of 3, 6 squats', { forMs: 30_000 });
    now += 1000;
    await lamp.context('set 2 of 3, 7 squats', { forMs: 30_000 });
    assert.deepEqual(fake.appContext, { line: 'set 2 of 3, 7 squats', forMs: 30_000 });
    now += 1000;
    await lamp.context('set 3 of 3, 1 squat');
    assert.deepEqual(fake.appContext, { line: 'set 3 of 3, 1 squat', forMs: 60_000 });
    await lamp.clearContext();
    assert.equal(fake.appContext, null);
    now += 1000;
    await lamp.context('again');
    await lamp.context('');
    assert.equal(fake.appContext, null);
    assert.deepEqual(fake.asked('apps.context').at(-1), { line: '' });
    for (const [line, forMs] of [['x'.repeat(121), 1000], ['ok', 0], ['ok', 600_001]]) {
      await assert.rejects(lamp.context(line, { forMs }), byCode.bad_request, JSON.stringify([line, forMs]));
    }
  });
});

test('context answers ok alone', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    for (const fields of [{ line: 'set 1 of 3', for_ms: 1000 }, { line: '' }]) {
      const reply = await lamp.request('apps.context', fields);
      assert.deepEqual(reply, { re: reply.re, ok: true });
    }
  });
});

test('context is one line a second and only set lines count', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    let now = 100_000;
    fake.clock = () => now;
    // Refused lines (too long, private) are not counted: the first set line goes.
    await assert.rejects(lamp.context('x'.repeat(121)), byCode.bad_request);
    fake.setPrivate(true);
    await assert.rejects(lamp.context('rep 1'), byCode.private);
    fake.setPrivate(false);
    await lamp.context('rep 1');
    now += 400;
    await assert.rejects(lamp.context('rep 2'), (error) => error instanceof byCode.rate && error.why === 'one line a second' && error.retryMs === 600);
    // The rated line did not count: a second after the set one, the next goes.
    now += 600;
    await lamp.context('rep 2');
    assert.equal(fake.appContext.line, 'rep 2');
  });
});

test('a clear is never rated nor refused while private', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    let now = 100_000;
    fake.clock = () => now;
    await lamp.context('rep 1');
    await lamp.clearContext();
    await lamp.clearContext();
    assert.equal(fake.appContext, null);
    fake.setPrivate(true);
    await lamp.clearContext();
    await lamp.context('');
  });
});

test('a clear with for_ms is a bad request', async () => {
  await withApp({ appGrants: MEMORY_GRANTS }, async (fake, lamp) => {
    await assert.rejects(lamp.request('apps.context', { line: '', for_ms: 1000 }), byCode.bad_request);
  });
});

// An app's tools (#8406, LAMP_APP_TOOLS): the model's call comes as `app.call`, the app answers with `app.answer`.

test('app.call and app.call.stopped are an app\'s events, and hello says tools', async () => {
  await withApp({ appTools: true }, async (fake, lamp) => { assert.equal(lamp.tools, true); });
  await withApp({}, async (fake, lamp) => {
    assert.equal(lamp.tools, false);
    lamp.on('app.call', () => {});
    lamp.on('app.call.stopped', () => {});
    await assert.rejects(lamp.answer('app-1-1-1', { result: { ok: true } }), (error) => error.code === 'off');
  });
});

test('a tool call is answered with a result once, or with the app\'s own error', async () => {
  await withApp({ appTools: true }, async (fake, lamp) => {
    const taken = [];
    const onCall = async (event) => {
      assert.deepEqual(Object.keys(event).sort(), ['args', 'call', 'ev', 'name']);
      taken.push(await lamp.answer(event.call, { result: { plan: 'legs', sets: event.args.sets } }));
    };
    lamp.on('app.call', onCall);
    const call = fake.callTool('set_plan', { sets: 3 });
    await settle();
    lamp.off('app.call', onCall);
    assert.deepEqual(taken, [true]);
    assert.deepEqual(fake.appAnswers.get(call), { result: { plan: 'legs', sets: 3 } });
    assert.equal(await lamp.answer(call, { result: { plan: 'arms' } }), false);
    const other = fake.callTool('set_plan', {});
    await assert.rejects(lamp.answer(other, { error: 'timeout' }), (error) => error.code === 'bad_request');
    for (const bad of [{}, { result: { a: 1 }, error: 'x' }, { error: 'two\nlines' }, { result: [1] }]) {
      await assert.rejects(lamp.answer(other, bad), (error) => error.code === 'bad_request');
    }
    assert.equal(await lamp.answer(other, { error: 'no plan picked yet' }), true);
    assert.deepEqual(fake.appAnswers.get(other), { error: 'no plan picked yet' });
  });
});

test('a stopped call is told and a late answer is not taken', async () => {
  await withApp({ appTools: true }, async (fake, lamp) => {
    const stopped = [];
    lamp.on('app.call.stopped', (event) => stopped.push(event));
    const call = fake.callTool('set_plan', {});
    fake.stopCall(call);
    await settle();
    assert.deepEqual(stopped, [{ ev: 'app.call.stopped', call, why: 'timeout' }]);
    assert.equal(await lamp.answer(call, { result: { late: true } }), false);
    assert.deepEqual(fake.appAnswers.get(call), { error: 'timeout' });
  });
});
