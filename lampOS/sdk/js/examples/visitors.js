// A visitor count: the lamp keeps how many times someone arrived today in a
// note, quietly, and says the number when asked "how many visitors".
//
// An arrival is the lamp's own (a face after the room was empty for a while);
// the lamp tells this computer only that someone arrived, never who. Nothing is
// said or lit when someone arrives: a lamp that announces every comer is a
// doorbell. The count is kept per day (this program's own clock, since an
// event's `at` is UTC), so a restart does not forget it.
//
// Needs the `look` (arrivals), `hear` (the phrase), `speak` and `notes` grants.
// Try it on the virtual lamp: `lamp-sim people 0 DIR`, wait about 5 s, then
// `lamp-sim people 1 DIR` for an arrival, then `lamp-sim hear "how many visitors" DIR`.
import { Lamp, LampError } from 'lampos';

// The day key by this program's own clock.
const today = () => {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
};

const lamp = await Lamp.connect();
let counting = Promise.resolve(); // one note write at a time: a second one is refused `busy`
const alone = (job) => {
  const turn = counting.then(job);
  counting = turn.catch(() => {});
  return turn;
};

await lamp.on('person.arrived', () => alone(async () => {
  const key = `visits-${today()}`;
  try {
    const count = (await lamp.notesGet(key, 0)) + 1;
    await lamp.notesSet(key, count);
    console.log(`visit ${count} today`);
  } catch (refused) {
    if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // notes paused, over its limits...
    else throw refused;
  }
}));

await lamp.on('phrase.heard', () => alone(async () => {
  try {
    const count = await lamp.notesGet(`visits-${today()}`, 0);
    await lamp.say(count === 0 ? 'Nobody yet today.' : `${count} so far today.`);
  } catch (refused) {
    if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // private, paused, asleep, quiet hours...
    else throw refused;
  }
}), { phrases: ['how many visitors'] });

console.log('counting arrivals; Ctrl-C to stop');
await lamp.closed();
