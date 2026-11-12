// The speaker stream (`speaker.stream`, the `speaker` grant): the client and the fake lamp, as docs/control.md's "The
// speaker" says it. Its own file, so a new test never appends to the tail of client.test.js.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, byCode } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';
import { WebSocket } from '../src/ws.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-speaker-'));
const until = async (ready, ms = 5000) => {
  const end = Date.now() + ms;
  while (!ready()) {
    if (Date.now() > end) throw new Error('timed out');
    await new Promise((r) => setTimeout(r, 10));
  }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function withLamp(options, body) {
  const fake = await FakeLamp.start({ grants: ['speaker'], ...options });
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try { await body(lamp, fake, keyring); } finally { await lamp.close(); }
  } finally { await fake.stop(); }
}

// PCM of `ms` ms at `rate`: a ramp, so a byte-for-byte compare means something.
const pcm = (ms, rate = 16000) => {
  const samples = Buffer.alloc(((rate * ms) / 1000) * 2);
  for (let n = 0; n < samples.length / 2; n += 1) samples.writeInt16LE((n * 7) % 3000, n * 2);
  return samples;
};
// A WAV of PCM16 mono at `rate` around `samples`.
const wav = (samples, rate = 16000, { channels = 1, bits = 16 } = {}) => {
  const header = Buffer.alloc(44);
  header.write('RIFF', 0); header.writeUInt32LE(36 + samples.length, 4); header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(channels, 22);
  header.writeUInt32LE(rate, 24); header.writeUInt32LE(rate * 2, 28); header.writeUInt16LE(2, 32); header.writeUInt16LE(bits, 34);
  header.write('data', 36); header.writeUInt32LE(samples.length, 40);
  return Buffer.concat([header, samples]);
};
// A binary message by hand, as the lamp reads it: header length, header, samples.
const message = (header, samples) => {
  const head = Buffer.from(JSON.stringify(header));
  const length = Buffer.alloc(4);
  length.writeUInt32BE(head.length);
  return Buffer.concat([length, head, samples]);
};

test('a speaker stream opens with the lamp\'s answer, plays what is written and ends with off', async () => {
  await withLamp({}, async (lamp, fake) => {
    const stream = await lamp.speakerStream();
    assert.deepEqual([stream.sampleRate, stream.turn, stream.frameMs, stream.bufferMs], [16000, 1, [10, 100], 300]);
    assert.deepEqual([stream.reply.format, stream.reply.source, stream.reply.stream], ['s16le', 'speaker', 'on']);
    assert.deepEqual(fake.asked('speaker.stream'), [{ on: true, sample_rate: 16000 }]);
    const sound = pcm(100);
    await stream.speakerWrite(sound);
    await until(() => fake.played().length === sound.length);
    assert.ok(fake.played().equals(sound), 'what the lamp played is what was written');
    await stream.stop();
    assert.equal(stream.ended, true);
    assert.deepEqual(fake.asked('speaker.stream').at(-1), { on: false });
    assert.equal(fake.speaker, null);
    await stream.stop(); // twice is fine
  });
});

test('frames carry a header and a running sequence, and none passes 9,600 bytes', async () => {
  await withLamp({}, async (lamp, fake) => {
    const stream = await lamp.speakerStream(24000);
    assert.equal(stream.sampleRate, 24000);
    // 1 s at 24 kHz is 48,000 bytes: 20 ms frames of 960 bytes.
    await stream.speakerWrite(pcm(1000, 24000));
    await until(() => fake.playedFrames.length === 50);
    assert.deepEqual(fake.playedFrames.map((f) => f.sequence), Array.from({ length: 50 }, (_, n) => n));
    assert.ok(fake.playedFrames.every((f) => f.data.length === 960 && f.rate === 24000));
    assert.equal(fake.speakerStray, 0);
    // Another write goes on from the sequence.
    await stream.speakerWrite(pcm(20, 24000));
    await until(() => fake.playedFrames.length === 51);
    assert.equal(fake.playedFrames.at(-1).sequence, 50);
    // An odd number of bytes is not whole samples: refused here, before it is sent.
    await assert.rejects(stream.speakerWrite(Buffer.alloc(3)), (e) => e.code === 'bad_request');
    await stream.stop();
  });
});

test('a sample rate the lamp does not take is refused', async () => {
  await withLamp({}, async (lamp) => {
    await assert.rejects(lamp.speakerStream(44100), (e) => e.code === 'bad_request' && /16000, 24000/.test(e.why));
    await assert.rejects(lamp.request('speaker.stream', { on: true, sample_rate: 44100 }), byCode.bad_request);
    await assert.rejects(lamp.request('speaker.stream', {}), byCode.bad_request);
    const plain = await lamp.request('speaker.stream', { on: true });
    assert.equal(plain.sample_rate, 16000);
  });
});

test('without the speaker grant the lamp says the way out, as for the camera', async () => {
  await withLamp({ grants: ['body'] }, async (lamp, fake) => {
    await assert.rejects(lamp.speakerStream(), (e) => e instanceof byCode.no_grant && /^not granted: speaker \(the app does not grant it yet/.test(e.why));
    assert.equal(fake.speaker, null);
  });
});

test('"speaker" is in no default grant set', () => {
  assert.equal(new FakeLamp().grants.includes('speaker'), false);
});

test('the lamp turning private or muted ends the stream and refuses a new one', async () => {
  await withLamp({}, async (lamp, fake) => {
    const ended = [];
    lamp.on('speaker.ended', (e) => ended.push(e));
    const stream = await lamp.speakerStream();
    fake.setPrivate();
    await until(() => ended.length === 1);
    assert.deepEqual(ended[0], { ev: 'speaker.ended', why: 'the lamp is private', turn: stream.turn });
    assert.equal(stream.endedWhy, 'the lamp is private');
    await assert.rejects(stream.speakerWrite(pcm(20)), (e) => e.code === 'closed' && /the lamp is private/.test(e.why));
    await assert.rejects(lamp.speakerStream(), byCode.private);
    fake.setPrivate(false);
    fake.setVolume(0);
    await assert.rejects(lamp.speakerStream(), (e) => /the lamp is muted/.test(e.why));
    fake.setVolume(50);
    const again = await lamp.speakerStream();
    fake.setVolume(0);
    await until(() => again.endedWhy === 'the lamp is muted');
  });
});

test('the grant going ends the stream; a forced end and a stale turn are told apart', async () => {
  await withLamp({}, async (lamp, fake) => {
    const stream = await lamp.speakerStream();
    // An old stream's end (another turn) is not this stream's.
    fake.sayStaleSpeakerEnded(stream.turn + 7);
    await sleep(50);
    assert.equal(stream.endedWhy, null);
    await stream.speakerWrite(pcm(20));
    fake.endSpeaker('the lamp has something to say');
    await until(() => stream.endedWhy !== null);
    assert.equal(stream.endedWhy, 'the lamp has something to say');
    const [client] = [...fake.clients.keys()];
    const next = await lamp.speakerStream();
    fake.dropGrant(client, 'speaker');
    await until(() => next.endedWhy !== null);
    assert.equal(next.endedWhy, 'the grant was taken');
    await assert.rejects(lamp.speakerStream(), byCode.no_grant);
  });
});

test('one stream on the lamp: on again replaces it and tells the old connection, and a second computer is told busy when the voice is', async () => {
  await withLamp({}, async (lamp, fake, keyring) => {
    const first = await lamp.speakerStream();
    const other = await Lamp.connect(undefined, { keyring });
    try {
      const old = first;
      const second = await other.speakerStream();
      assert.equal(second.turn, first.turn + 1);
      await until(() => old.endedWhy === 'replaced');
      // Frames of the replaced stream's connection are no stream's own: stray, not played.
      fake.clearPlayed();
      await old.stop();
      await second.speakerWrite(pcm(20));
      await until(() => fake.playedFrames.length === 1);
      assert.equal(fake.playedFrames[0].turn, second.turn);
      // The voice speaking is `busy`, and the stream that runs is left be.
      fake.speakerBusy = 'the lamp is speaking';
      await assert.rejects(lamp.speakerStream(), (e) => e instanceof byCode.busy && e.why === 'the lamp is speaking');
      assert.equal(fake.speaker.turn, second.turn);
      fake.speakerBusy = null;
      // The same connection asking again replaces its own.
      const third = await other.speakerStream();
      assert.equal(third.turn, second.turn + 1);
      await until(() => second.endedWhy === 'replaced'); // told its own turn, by the lamp
    } finally { await other.close(); }
  });
});

test('binary messages with no stream, from another connection, or of a size it does not take are stray and get no answer', async () => {
  await withLamp({}, async (lamp, fake) => {
    const raw = lamp.ws;
    const send = (header, samples) => new Promise((resolve) => raw.send(message(header, samples), resolve));
    const header = (sequence) => ({ ev: 'speaker.audio', sequence });
    // No stream yet.
    await send(header(0), pcm(20));
    await until(() => fake.speakerStray === 1);
    const stats = [];
    lamp.on('speaker.stats', (e) => stats.push(e));
    fake.speakerStatsMs = 40;
    const stream = await lamp.speakerStream();
    await send(header(0), Buffer.alloc(0));
    await send(header(1), Buffer.alloc(3));
    await send(header(2), Buffer.alloc(9602));
    await send({ ev: 'something.else', sequence: 3 }, pcm(20));
    await new Promise((resolve) => raw.send(Buffer.from([1, 2]), resolve));
    await send(header(4), Buffer.alloc(9600)); // the most it takes
    await until(() => fake.playedFrames.length === 1);
    await until(() => stats.some((s) => s.stray === 5));
    const last = stats.at(-1);
    assert.deepEqual(Object.keys(last).sort(), ['buffered_ms', 'ev', 'limited', 'lost', 'stray', 'turn', 'underruns']);
    assert.equal(last.turn, stream.turn);
    assert.equal(fake.playedFrames[0].data.length, 9600);
    assert.equal(lamp.ws.readyState, WebSocket.OPEN, 'no error, the connection goes on');
    assert.equal(fake.received.filter((r) => !['speaker.stream', 'subscribe', 'hello', 'pair'].includes(r.op)).length, 0);
    await stream.stop();
  });
});

test('a gap in the sequence is sound lost, and an idle stream ends after its time', async () => {
  await withLamp({}, async (lamp, fake) => {
    fake.speakerIdleMs = 150;
    fake.speakerStatsMs = 40;
    const stats = [];
    lamp.on('speaker.stats', (e) => stats.push(e));
    const stream = await lamp.speakerStream();
    const send = (sequence) => new Promise((resolve) => lamp.ws.send(message({ ev: 'speaker.audio', sequence }, pcm(20)), resolve));
    await send(0);
    await send(3);
    await until(() => stats.some((s) => s.lost === 2));
    await until(() => stream.endedWhy === 'idle');
  });
});

test('playWav sends a file in real time at its rate, and refuses what the speaker cannot take', async () => {
  const dir = folder();
  try {
    await withLamp({}, async (lamp, fake) => {
      const sound = pcm(400);
      const path = join(dir, 'tone.wav');
      writeFileSync(path, wav(sound));
      const stream = await lamp.speakerStream();
      const began = Date.now();
      const result = await stream.playWav(path);
      const took = Date.now() - began;
      assert.deepEqual(result, { ms: 400, ended: null });
      // 400 ms of sound, sent 100 ms ahead of real time, then waited out (its play time and the lamp's buffer), so a
      // `stop()` right after does not cut the tail: never all at once and never much later.
      assert.ok(took >= 400 + (stream.bufferMs ?? 0) - 50 && took < 2500, `took ${took} ms`);
      await until(() => fake.played().length === sound.length);
      assert.ok(fake.played().equals(sound));
      await stream.stop();
      // Not the stream's rate, not 16-bit mono PCM, not a WAV.
      writeFileSync(join(dir, '24k.wav'), wav(pcm(20, 24000), 24000));
      const again = await lamp.speakerStream();
      await assert.rejects(again.playWav(join(dir, '24k.wav')), (e) => e.code === 'bad_request' && /speakerStream\(24000\)/.test(e.why));
      writeFileSync(join(dir, '44k.wav'), wav(pcm(20), 44100));
      await assert.rejects(again.playWav(join(dir, '44k.wav')), (e) => /44100 Hz: the speaker takes 16000 or 24000 Hz/.test(e.why));
      writeFileSync(join(dir, 'stereo.wav'), wav(pcm(20), 16000, { channels: 2 }));
      await assert.rejects(again.playWav(join(dir, 'stereo.wav')), (e) => /16-bit mono PCM only/.test(e.why));
      writeFileSync(join(dir, 'junk.wav'), 'not a wav at all');
      await assert.rejects(again.playWav(join(dir, 'junk.wav')), (e) => /not a WAV file/.test(e.why));
    });
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('playWav stops when the lamp ends the stream and says why', async () => {
  const dir = folder();
  try {
    await withLamp({}, async (lamp, fake) => {
      const path = join(dir, 'long.wav');
      writeFileSync(path, wav(pcm(2000)));
      const stream = await lamp.speakerStream();
      const playing = stream.playWav(path);
      await until(() => fake.playedFrames.length > 3);
      fake.endSpeaker('the lamp has something to say');
      const result = await playing;
      assert.equal(result.ended, 'the lamp has something to say');
      assert.ok(result.ms < 2000);
    });
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('the debug log shows a speaker stream\'s shape and counters, never the sound', async () => {
  const debug = await import('../src/debug.js');
  for (const name of ['buffered_ms', 'buffer_ms', 'frame_ms', 'underruns', 'limited', 'stray', 'lost', 'turn']) {
    assert.ok(debug.SHOWN.has(name), name);
  }
  const shown = { ev: 'speaker.stats', turn: 1, buffered_ms: 20, lost: 0, underruns: 0, limited: 0, stray: 0 };
  assert.deepEqual(debug.redact(shown), shown);
});

test('lamp speaker plays a WAV through the paired lamp, and refuses a file the speaker cannot take', async () => {
  const dir = folder();
  const fake = await FakeLamp.start({ grants: ['speaker'] });
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(dir) });
    // Not execFileSync: the fake answers on this process's event loop.
    const lamp = (...args) => new Promise((done) => {
      execFile(process.execPath, [new URL('../src/cli.js', import.meta.url).pathname, ...args], { env: { ...process.env, LAMPOS_HOME: dir } },
        (error, stdout, stderr) => done({ code: error?.code ?? 0, out: stdout.trim(), err: stderr.trim() }));
    });
    const sound = pcm(200, 24000);
    writeFileSync(join(dir, 'a.wav'), wav(sound, 24000));
    const played = await lamp('speaker', join(dir, 'a.wav'));
    assert.deepEqual([played.code, played.out], [0, 'played 200 ms']);
    assert.ok(fake.played().equals(sound));
    assert.equal(fake.playedFrames[0].rate, 24000, 'the stream was opened at the file\'s rate');
    writeFileSync(join(dir, 'b.wav'), wav(pcm(20), 8000));
    const bad = await lamp('speaker', join(dir, 'b.wav'));
    assert.deepEqual([bad.code, bad.err], [1, 'lamp: bad_request: wav: 8000 Hz: the speaker takes 16000 or 24000 Hz']);
  } finally { await fake.stop(); rmSync(dir, { recursive: true, force: true }); }
});
