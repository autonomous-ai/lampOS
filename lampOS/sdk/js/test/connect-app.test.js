// An installed app's socket (#7415), to nine's messages (piece 3): `Lamp.connectApp()` reads LAMP_APP_SOCKET, needs no
// pairing, key or token, and gets the manifest's grants; `listenFor`, `took`, `settings` and `panel` are its ops, and
// `app.trigger`, `app.settings` and `phrase.heard` its events. The fake lamp serves the same.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Lamp } from '../src/index.js';
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
