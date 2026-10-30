// A build's result on the ring: green breathing when it passes, a red pulse
// and a word when it fails.   node ci-light.js pass|fail
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
if ((process.argv[2] ?? 'pass') === 'pass') {
  await lamp.light('green', 3, 20_000, 'breathe');
} else {
  await lamp.light('red', 3, 20_000, 'pulse');
  await lamp.say('The build failed.');
}
// Closing gives the light back at once (lamp-control's server.rs): stay
// while it shows.
await new Promise((resolve) => setTimeout(resolve, 20_000));
await lamp.close();
