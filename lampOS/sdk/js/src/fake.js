// `lamp fake`: a fake lamp on this computer, with every grant, paired here at
// once, that says a person arrived, a phrase heard and the person left, every
// `every` seconds, and prints what it is asked. Try an example against it in
// another terminal, with the same LAMPOS_HOME.
import { Lamp } from './client.js';
import { Keyring } from './keys.js';
import * as schema from './schema.js';
import { FakeLamp, LAMP_CONTROL_REFUSES } from './testing.js';

const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));

// `everything`: answer every op; by default it refuses what lamp-control
// doesn't route yet, as a lamp does.
export async function fake({ every = 10, cycles, everything = false } = {}) {
  const refuses = everything ? {} : LAMP_CONTROL_REFUSES;
  const lamp = await FakeLamp.start({ grants: schema.grants, name: 'Fake lamp (lamp fake)', refuses });
  try {
    const keyring = new Keyring();
    const pairing = await Lamp.pair(lamp.code, lamp.host, { pin: lamp.pin, port: lamp.port, keyring });
    console.log(`a fake lamp at ${lamp.host}:${lamp.port}, paired as ${pairing.client} in ${keyring.folder}`);
    console.log('run an example in another terminal with the same LAMPOS_HOME; Ctrl-C ends this');
    let shown = 0;
    for (let cycle = 0; cycles === undefined || cycle < cycles; cycle++) {
      for (const [event, fields] of [
        ['person.arrived', { count: 1, near: 0.3 }],
        ['phrase.heard', { phrase: 'good night lamp' }],
        ['person.left', { count: 0 }],
      ]) {
        await sleep(every / 3);
        lamp.emit(event, fields);
        console.log(`said ${event}`);
        for (const { id, ...request } of lamp.requests.slice(shown)) console.log('  asked', JSON.stringify(request));
        shown = lamp.requests.length;
      }
    }
  } finally {
    await lamp.stop();
  }
  return 0;
}
