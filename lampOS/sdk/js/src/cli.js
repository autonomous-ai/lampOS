#!/usr/bin/env node
// `lamp`: pair this computer with a lamp, find lamps, and check a pairing.
//   lamp find
//   lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
//   lamp hello [LAMP_ID]
//   lamp fake [--every 10] [--cycles N]
//   lamp skill push FOLDER [LAMP_ID] | lamp skill list [LAMP_ID] | lamp skill remove NAME [LAMP_ID]
//   lamp skill install FILE|URL [LAMP_ID] [--yes]
//   lamp skill keygen | lamp skill pack FOLDER [--sign] [--out DIR] | lamp skill check PACKAGE.tar
import { createPublicKey } from 'node:crypto';
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline/promises';
import { parseArgs } from 'node:util';
import { Lamp } from './client.js';
import { find } from './discovery.js';
import { PORT } from './schema.js';
import { fake } from './fake.js';
import { LampError } from './errors.js';
import { Keyring } from './keys.js';
import { PACKAGE_MAX, asksOf, fetchPackage, fingerprint, openPackage, pack } from './push.js';
import { parse as parseToml } from 'smol-toml';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { host: { type: 'string' }, port: { type: 'string' }, name: { type: 'string' }, pin: { type: 'string' }, every: { type: 'string' }, cycles: { type: 'string' }, everything: { type: 'boolean' }, sign: { type: 'boolean' }, out: { type: 'string' }, yes: { type: 'boolean' } },
});
const [command, arg, third, fourth] = positionals;

async function main() {
  if (command === 'find') {
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
    return fake({ every: Number(values.every ?? 10), cycles: values.cycles === undefined ? undefined : Number(values.cycles), everything: Boolean(values.everything) });
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
  } else if (command === 'skill' && arg === 'install' && third) {
    return install(third, fourth);
  } else if (command === 'skill' && arg === 'keygen') {
    const ring = new Keyring();
    const made = !ring.authorKey();
    const spki = createPublicKey(ring.authorKey({ make: true })).export({ type: 'spki', format: 'der' });
    console.log(`author key ${fingerprint(spki.subarray(spki.length - 32))} ${made ? 'made' : 'already there'}: ${join(ring.folder, 'author.pem')}`);
  } else if (command === 'skill' && arg === 'pack' && third) {
    let author;
    if (values.sign) {
      author = new Keyring().authorKey();
      if (!author) throw new LampError('no author key: make one with `lamp skill keygen`', { code: 'bad_request' });
    }
    const made = pack(third, { author });
    const out = values.out ?? '.';
    mkdirSync(out, { recursive: true });
    const path = join(out, `${made.name}-${made.version}.tar`);
    writeFileSync(path, made.data);
    console.log(`${path} ${made.sha256}${author ? ' signed' : ''}`);
  } else if (command === 'skill' && arg === 'check' && third) {
    // The package's signature and files; the skill itself is checked by the
    // Python SDK's `lamp skill check`, and by the lamp again.
    const opened = openPackage(readFileSync(third));
    // Integrity, not trust: anyone can make a key (Fourteen, #3504).
    console.log(`${opened.name}: package ok${opened.author ? `, signed by key ${opened.author}, not the Store` : ', not signed'}`);
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
    console.error('usage: lamp find | lamp pair CODE --pin PIN [--host HOST] [--name NAME] | lamp hello [LAMP_ID] | lamp fake [--every S] [--cycles N] | lamp skill push FOLDER | list | remove NAME [LAMP_ID] | lamp skill install FILE|URL [LAMP_ID] [--yes] | lamp skill keygen | pack FOLDER [--sign] [--out DIR] | check PACKAGE.tar');
    return 2;
  }
  return 0;
}

// What each ask lets a skill do, in the words a person is shown.
const ASK_WORDS = {
  body: 'move the lamp',
  camera: "see the camera's picture",
  light: 'change the light',
  look: 'know when someone is at the lamp',
  speak: 'speak and play sounds',
};

async function install(source, lampId) {
  let data;
  // A URL is fetched over https only; anything else is a file here.
  if (source.includes('://')) {
    data = await fetchPackage(source);
  } else {
    if (statSync(source).size > PACKAGE_MAX) throw new LampError(`${source}: a shared skill is 1 MiB at most`, { code: 'bad_request' });
    data = readFileSync(source);
  }
  const opened = openPackage(data);
  let asks = [];
  try {
    asks = asksOf(parseToml((opened.files.get('skill.toml') ?? Buffer.alloc(0)).toString('utf8')));
  } catch {
    // The lamp says what's wrong with it; installPackage refuses it first.
  }
  // Integrity, not trust: anyone can make a key (Fourteen, #3504).
  console.log(`${opened.name} ${opened.version}`);
  console.log(opened.author ? `  signed by key ${opened.author}, not the Store` : '  not signed, not the Store');
  for (const ask of asks) console.log(`  asks to ${ASK_WORDS[ask]} (${ask})`);
  if (asks.includes('camera')) console.log("  It asks for the camera's picture: the lamp keeps it off until the owner says yes in the app.");
  if (!values.yes) {
    const prompt = createInterface({ input: process.stdin, output: process.stdout });
    const answer = (await prompt.question(`Put ${opened.name} on your lamp? [y/N] `)).trim().toLowerCase();
    prompt.close();
    if (answer !== 'y' && answer !== 'yes') {
      console.log('not installed');
      return 1;
    }
  }
  const lamp = await Lamp.connect(lampId);
  try {
    const skill = await lamp.installPackage(data);
    console.log(`${skill.name} ${skill.version} installed, ${skill.on ? 'on' : 'off'}${skill.held ? `, held: ${skill.held}` : ''}`);
  } finally {
    await lamp.close();
  }
  return 0;
}

main().then((code) => process.exit(code), (error) => { console.error(`lamp: ${error.message}`); process.exit(1); });
