#!/usr/bin/env node
// `lamp`: pair this computer with a lamp, find lamps, and check a pairing.
//   lamp find
//   lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
//   lamp hello [LAMP_ID]
//   lamp fake [--every 10] [--cycles N] [--everything] [--hands]
//   lamp speaker FILE.wav|- [--rate 16000|24000]
//   lamp skill push FOLDER [LAMP_ID] | lamp skill list [LAMP_ID] | lamp skill remove NAME [LAMP_ID]
import { parseArgs } from 'node:util';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Lamp, SPEAKER_RATES, wavSamples } from './client.js';
import { find } from './discovery.js';
import { PORT } from './schema.js';
import { fake } from './fake.js';

const USAGE = `usage: lamp find
       lamp pair CODE --pin PIN [--host HOST] [--port PORT] [--name NAME]
       lamp hello [LAMP_ID]
       lamp fake [--every S] [--cycles N] [--everything] [--hands]
       lamp speaker FILE.wav|- [LAMP_ID] [--rate N]
       lamp skill push FOLDER [LAMP_ID] | list [LAMP_ID] | remove NAME [LAMP_ID]

  find    lamps on this network
  pair    pair this computer with a lamp, with the owner's code
  hello   connect to a paired lamp and show its grants
  speaker play a WAV (or raw PCM16 on stdin) through the lamp's speaker, in real time (the speaker grant)
  fake    a fake lamp here, paired at once, saying an arrival now and then
  skill   this computer's pushed skills (the skills grant): push, list, remove`;

let parsed;
try {
  parsed = parseArgs({
    allowPositionals: true,
    options: { help: { type: 'boolean', short: 'h' }, host: { type: 'string' }, port: { type: 'string' }, name: { type: 'string' }, pin: { type: 'string' }, every: { type: 'string' }, cycles: { type: 'string' }, everything: { type: 'boolean' }, hands: { type: 'boolean' }, rate: { type: 'string' } },
  });
} catch (error) {
  // A misspelt option is one line and the usage, not a stack trace.
  console.error(`lamp: ${error.message.split('. To specify')[0]}\n${USAGE}`);
  process.exit(2);
}
const { values, positionals } = parsed;
const [command, arg, third, fourth] = positionals;

async function main() {
  if (values.help || command === 'help') {
    console.log(USAGE);
  } else if (command === 'find') {
    for (const lamp of await find()) console.log(`${lamp.id}\t${lamp.name}\t${lamp.host}:${lamp.port}\tpin starts ${lamp.fp}`);
  } else if (command === 'pair' && arg && values.pin) {
    const pin = values.pin.replaceAll(' ', '').replaceAll(':', '').toLowerCase();
    let host = values.host;
    let port = Number(values.port ?? PORT);
    if (!host) {
      // mDNS only finds where the lamp is; its pin, checked before anything is
      // sent, is what says it is the lamp.
      const lamps = (await find()).filter((lamp) => lamp.fp && pin.startsWith(lamp.fp));
      if (lamps.length !== 1) {
        console.error(`found ${lamps.length} lamps with that pin; say which with --host`);
        return 2;
      }
      ({ host, port } = lamps[0]);
    }
    const pairing = await Lamp.pair(arg, host, { pin, port, name: values.name });
    console.log(`paired with ${pairing.lamp} (${pairing.name}) as ${pairing.client}: ${pairing.grants.join(', ')}`);
  } else if (command === 'fake') {
    return fake({ every: Number(values.every ?? 10), cycles: values.cycles === undefined ? undefined : Number(values.cycles), everything: Boolean(values.everything), hands: Boolean(values.hands) });
  } else if (command === 'speaker' && arg) {
    // Sound through the lamp's speaker (the `speaker` grant), in real time: a PCM16 mono WAV, or `-` for raw PCM16
    // mono samples on stdin at --rate (16000 unless given).
    const lamp = await Lamp.connect(third);
    try {
      let path = arg;
      let rate;
      let folder;
      if (arg === '-') {
        rate = Number(values.rate ?? 16000);
        if (!SPEAKER_RATES.includes(rate)) throw new Error(`--rate: one of ${SPEAKER_RATES.join(', ')}`);
        const pcm = readFileSync(0);
        // A WAV header around the samples, so one path plays both.
        const header = Buffer.alloc(44);
        header.write('RIFF', 0); header.writeUInt32LE(36 + pcm.length, 4); header.write('WAVEfmt ', 8);
        header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22);
        header.writeUInt32LE(rate, 24); header.writeUInt32LE(rate * 2, 28); header.writeUInt16LE(2, 32); header.writeUInt16LE(16, 34);
        header.write('data', 36); header.writeUInt32LE(pcm.length, 40);
        folder = mkdtempSync(join(tmpdir(), 'lamp-speaker-'));
        path = join(folder, 'stdin.wav');
        writeFileSync(path, Buffer.concat([header, pcm]));
      } else {
        rate = wavSamples(readFileSync(arg)).rate;
      }
      try {
        const stream = await lamp.speakerStream(rate);
        const { ms, ended } = await stream.playWav(path);
        await stream.stop();
        console.log(ended === null ? `played ${ms} ms` : `the lamp ended the stream after ${ms} ms: ${ended}`);
        if (ended !== null) return 1;
      } finally {
        if (folder) rmSync(folder, { recursive: true, force: true });
      }
    } finally {
      await lamp.close();
    }
  } else if (command === 'skill' && third && (arg === 'push' || arg === 'remove')) {
    // A skill of this computer's own on its paired lamp (the skills grant).
    const lamp = await Lamp.connect(fourth);
    try {
      if (arg === 'push') {
        const skill = await lamp.pushSkill(third);
        console.log(`${skill.name} ${skill.version} pushed, ${skill.on ? 'on' : 'off'}${skill.held ? `, held: ${skill.held}` : ''}`);
        for (const shown of skill.shown ?? []) console.log(`  ${shown}`);
      } else {
        await lamp.removeSkill(third);
        console.log(`${third} removed`);
      }
    } finally {
      await lamp.close();
    }
  } else if (command === 'skill' && arg === 'list') {
    const lamp = await Lamp.connect(third);
    try {
      for (const skill of await lamp.skills()) console.log(`${skill.name}\t${skill.version}\t${skill.on ? 'on' : 'off'}${skill.held ? ` (held: ${skill.held})` : ''}`);
    } finally {
      await lamp.close();
    }
  } else if (command === 'hello') {
    const lamp = await Lamp.connect(arg);
    console.log(`${lamp.info.id}: grants ${lamp.grants.join(', ')}`);
    await lamp.close();
  } else {
    console.error(USAGE);
    return 2;
  }
  return 0;
}

main().then((code) => process.exit(code), (error) => { console.error(`lamp: ${error.message}`); process.exit(1); });
