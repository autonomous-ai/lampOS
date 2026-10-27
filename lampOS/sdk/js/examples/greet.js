// Greet a person who arrives, at most once a minute.
import { Lamp, LampError } from 'lampos';

const lamp = await Lamp.connect();
let last = 0;
await lamp.on('person.arrived', async () => {
  if (Date.now() - last < 60_000) return;
  last = Date.now();
  try {
    await lamp.gesture('greeting');
  } catch (refused) {
    if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // private, paused, asleep, rate...
    else throw refused;
  }
});
await lamp.closed();
