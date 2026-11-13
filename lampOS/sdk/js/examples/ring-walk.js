// The ring pixel by pixel, as a check you can watch (or film): pixel 0 white,
// then one green pixel walking all the way around, then the whole ring blue at
// half brightness, then the ring given back.
//
//   node ring-walk.js        # 2 pixels a second
//   node ring-walk.js 4      # 4 a second
//
// It shows where pixel 0 is and which way the ring counts. Each step is a new
// `light`, which replaces the last under the same claim; the person's own
// light, a turn's face and the privacy marks stay over it. ring_walk.py does
// the same in Python.
import { Lamp } from 'lampos';

const perSecond = Number(process.argv[2] ?? 2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const lamp = await Lamp.connect();
try {
  const count = lamp.ringPixels;
  if (count === undefined) {
    console.error("this lamp's lamp-control draws no pixels yet (no ring in hello's limits)");
    process.exitCode = 1;
  } else {
    const dark = Array(count).fill('#000000');
    console.log('pixel 0 white, 3 s');
    await lamp.ring(dark.map((c, at) => (at === 0 ? '#ffffff' : c)), { forMs: 4000 });
    await sleep(3000);
    console.log(`one green pixel walking around, ${perSecond} a second`);
    for (let at = 0; at < count; at++) {
      await lamp.ring(dark.map((c, i) => (i === at ? '#00ff00' : c)), { forMs: 2000 });
      await sleep(1000 / perSecond);
    }
    console.log('all blue at brightness 50, 3 s');
    await lamp.lightRgb('#0000ff', { brightness: 50, forMs: 4000 });
    await sleep(3000);
    console.log('off: the ring given back');
    await lamp.stop();
  }
} finally {
  await lamp.close();
}
