// Count how many times this program has run, in a note the lamp keeps for
// this computer, and show the count as the light's level for a moment. The
// note stays across this program's restarts and the lamp's. `notes` and
// `light` are the grants it needs.
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
const runs = (await lamp.notes.get('runs', 0)) + 1;
await lamp.notes.set('runs', runs);
console.log(`run ${runs}`);
await lamp.light('warm', Math.min(runs, 5), 3000);
// Closing gives the light back at once: stay while it shows.
await new Promise((resolve) => setTimeout(resolve, 3000));
await lamp.close();
