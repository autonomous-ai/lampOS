// Stuffy air: the lamp's light turns a soft orange while the air is stuffy, and
// the program says what its sensor and its own computer read. Needs the `air`,
// `device` and `light` grants; a lamp with no air sensor answers `known: false`.
import { Lamp, LampError } from 'lampos';

const lamp = await Lamp.connect();
const device = await lamp.device();
if (device.known) {
  const hottest = Math.max(...Object.values(device.temperature_c));
  console.log(`lamp: load ${device.cpu_load.toFixed(2)}, memory ${Math.round(device.memory_used * 100)}%, hottest ${hottest} °C`);
}
const air = await lamp.air();
console.log('air:', air.known ? air : 'no reading (no sensor, or not yet)');
// The lamp tells the air by change: a value past its step, at most every 5 s.
await lamp.on('air.changed', async (event) => {
  console.log(`air: ${event.co2_ppm ?? '?'} ppm CO2, ${event.pm2_5 ?? '?'} µg/m³ PM2.5, ${event.feel}`);
  if (event.feel === 'stuffy' || event.feel === 'very_stuffy') {
    try {
      await lamp.light('orange', 2, 30_000, 'breathe');
    } catch (refused) {
      if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // private, paused, asleep...
      else throw refused;
    }
  }
});
await lamp.closed();
