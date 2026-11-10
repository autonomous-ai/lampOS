// Watch the joints' temperatures (`body.state`, about 12 a second) and tell
// when one runs hot: a pulsing orange light and a line on the screen, once,
// and again only after every joint has cooled. `body.state` needs the `body`
// grant, the light needs `light`. `node heat-watch.js [limit in degrees C, default 60]`.
import { Lamp, LampError } from 'lampos';

const limit = Number(process.argv[2] ?? 60);
const lamp = await Lamp.connect();
let warned = false;
await lamp.on('body.state', async (event) => {
  const hot = Object.entries(event.joints).filter(([, joint]) => (joint.temp_c ?? 0) >= limit);
  if (hot.length && !warned) {
    warned = true;
    const [name, joint] = hot.reduce((worst, now) => (now[1].temp_c > worst[1].temp_c ? now : worst));
    console.log(`hot: ${name} ${joint.temp_c} C`);
    try {
      await lamp.light('orange', 2, 5000, 'pulse');
    } catch (refused) {
      if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // private, paused, asleep, rate...
      else throw refused;
    }
  } else if (!hot.length && warned) {
    warned = false;
    console.log('cool again');
  }
});
await lamp.closed();
