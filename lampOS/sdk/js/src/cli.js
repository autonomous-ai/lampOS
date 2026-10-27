#!/usr/bin/env node
// `lamp`: pair this computer with a lamp, find lamps, and check a pairing.
//   lamp find
//   lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
//   lamp hello [LAMP_ID]
//   lamp fake [--every 10] [--cycles N]
import { parseArgs } from 'node:util';
import { Lamp } from './client.js';
import { find } from './discovery.js';
import { PORT } from './schema.js';
import { fake } from './fake.js';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { host: { type: 'string' }, port: { type: 'string' }, name: { type: 'string' }, pin: { type: 'string' }, every: { type: 'string' }, cycles: { type: 'string' }, everything: { type: 'boolean' } },
});
const [command, arg] = positionals;

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
  } else if (command === 'hello') {
    const lamp = await Lamp.connect(arg);
    console.log(`${lamp.info.id}: grants ${lamp.grants.join(', ')}`);
    await lamp.close();
  } else {
    console.error('usage: lamp find | lamp pair CODE --pin PIN [--host HOST] [--name NAME] | lamp hello [LAMP_ID] | lamp fake [--every S] [--cycles N]');
    return 2;
  }
  return 0;
}

main().then((code) => process.exit(code), (error) => { console.error(`lamp: ${error.message}`); process.exit(1); });
