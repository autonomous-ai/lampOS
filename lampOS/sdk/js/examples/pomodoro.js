// A Pomodoro on the lamp: 25 minutes of focus under a steady warm light, then a
// word and a stretch, then 5 minutes of rest. The light is renewed every 25 s
// (a client's light lasts 30 s at most), so if this stops, the lamp's own light
// comes back by itself.
import { Lamp, LampError } from 'lampos';

const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const lamp = await Lamp.connect();

async function holdLight(colour, seconds) {
  for (let i = 0; i < Math.floor(seconds / 25); i++) {
    await lamp.light(colour, 2, 30_000);
    await sleep(25);
  }
}

for (;;) {
  await holdLight('warm', 25 * 60);
  try {
    await lamp.say('Time for a short break.');
    await lamp.gesture('stretching');
  } catch (refused) {
    if (!(refused instanceof LampError)) throw refused;
    console.log(`break, quietly: ${refused.message}`); // quiet hours, private, someone talking...
  }
  await holdLight('green', 5 * 60);
}
