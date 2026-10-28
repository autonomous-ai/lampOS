// Put someone else's shared skill on your lamp: read the package from a file
// or an https URL, show who signed it and what it asks, and install it only
// on a yes. A signature says the bytes are that key's, not who is behind the
// key, so it is never called "verified".
//   node install-shared.js https://example.com/desk-wave-1.0.0.tar
import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline/promises';
import { Lamp } from 'lampos';
import { asksOf, fetchPackage, openPackage } from 'lampos/push';
import { parse } from 'smol-toml';

const source = process.argv[2];
const data = source.includes('://') ? await fetchPackage(source) : readFileSync(source); // https only, 1 MiB at most
const opened = openPackage(data); // the signature, every path and file
const asks = asksOf(parse(opened.files.get('skill.toml').toString('utf8')));
const signer = opened.author ? `signed by key ${opened.author}` : 'not signed';
console.log(`${opened.name} ${opened.version}, ${signer}, not the Store; asks: ${asks.join(', ') || 'nothing'}`);
const prompt = createInterface({ input: process.stdin, output: process.stdout });
const yes = (await prompt.question('Install it? [y/N] ')).trim().toLowerCase() === 'y';
prompt.close();
if (yes) {
  const lamp = await Lamp.connect();
  const skill = await lamp.installPackage(data);
  if (skill.held) console.log(`${skill.name} waits for the owner's yes in the app: ${skill.held}`);
  else console.log(`${skill.name} ${skill.version} is on`);
  await lamp.close();
}
