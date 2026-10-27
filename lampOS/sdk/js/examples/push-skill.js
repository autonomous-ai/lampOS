// Put a skill of your own on your lamp, then list what this computer has put
// there. The pairing needs the `skills` grant; the skill keeps running after
// this program ends, never beyond the computer's other grants.
//   node push-skill.js path/to/my-skill
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
const skill = await lamp.pushSkill(process.argv[2]);
if (skill.held) console.log(`${skill.name} ${skill.version} waits for the owner's yes: ${skill.held}`);
else console.log(`${skill.name} ${skill.version} is on, asking ${skill.shown.join(', ') || 'nothing'}`);
for (const kept of await lamp.skills()) console.log(`  ${kept.name} ${kept.version}${kept.on ? '' : ' (off)'}`);
await lamp.close();
