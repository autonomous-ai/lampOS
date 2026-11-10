// Answer a tap on the lamp's head with a warm light for a moment, and a pet
// with a glad feeling. `touch` needs no grant; the light needs `light`, the
// feeling `body` and `light`.
import { Lamp, LampError } from 'lampos';

const lamp = await Lamp.connect();
await lamp.on('touch', async (event) => {
  try {
    if (event.kind === 'tap') await lamp.light('warm', 3, 3000);
    else if (event.kind === 'pet') await lamp.express('glad');
    console.log(event.kind);
  } catch (refused) {
    if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // private, paused, asleep, rate...
    else throw refused;
  }
});
await lamp.closed();
