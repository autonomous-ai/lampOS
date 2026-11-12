// The try-it panel (served only with --stim-dir), drawn from the table in controls.js (the server sends it as
// /try.json): one row per SDK call (a button and its fields), one row per thing that happens in the room, and the
// lamp's own events. Nothing here is written per feature: a new SDK call is a new entry in controls.js.
const send = (message) => window.twinSend?.(message);
let counter = 0;
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const child of children) node.append(child);
  return node;
};
document.head.append(el('style', { textContent: `
  #panel { margin-top: 10px; font-size: 13px; }
  #panel h2 { font-size: 13px; margin: 12px 0 6px; color: var(--soft); text-transform: uppercase; letter-spacing: .04em; }
  #panel h3 { font-size: 12px; margin: 8px 0 2px; color: var(--soft); font-weight: 600; }
  #panel .row { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0; align-items: center; }
  #panel .row label { display: inline-flex; gap: 4px; align-items: center; color: var(--soft); }
  #panel button { cursor: pointer; }
  #panel input[type=text] { flex: 1; min-width: 120px; }
  #panel input[type=number] { width: 64px; }
  #panel .world button { border-style: dashed; }
  #panel .hint { flex-basis: 100%; font-size: 11px; color: var(--soft); margin: -2px 0 2px 2px; }
  #events { height: 220px; overflow: auto; font: 12px/1.35 ui-monospace, monospace; border: 1px solid var(--line); border-radius: 6px; padding: 6px; white-space: pre-wrap; }
  #events .ok { color: var(--accent); } #events .no { color: #d9534f; } #events .ev { color: var(--soft); }
` }));

// The lamp's own names and the table of buttons, from the server.
const table = await (await fetch(`/try.json${location.search}`)).json();

// One input for a field, with its limits and default.
function input(field) {
  if (field.type === 'select') {
    const node = el('select');
    for (const option of field.options) node.append(el('option', { value: option, textContent: option === '' ? '(none)' : option }));
    node.value = field.default ?? field.options[0];
    return node;
  }
  if (field.type === 'number') return el('input', { type: 'number', min: field.min, max: field.max, value: field.default ?? field.min, step: field.integer ? 1 : 'any' });
  return el('input', { type: 'text', value: field.default ?? '' });
}

// A row: the fields (named), then the button, which sends the entry's id and the values.
function row(entry, kind) {
  const inputs = entry.fields.map((field) => [field, input(field)]);
  const values = () => Object.fromEntries(inputs.map(([field, node]) => [field.name, field.type === 'number' ? Number(node.value) : node.value]));
  const children = inputs.map(([field, node]) => (field.type === 'text' ? node : el('label', {}, field.label ?? field.name, node)));
  children.push(el('button', { textContent: entry.label, onclick: () => send({ type: kind, id: ++counter, name: entry.id, args: values() }) }));
  // What the row does that its label does not say, under it (textContent: never markup).
  if (typeof entry.hint === 'string') children.push(el('div', { className: 'hint', textContent: entry.hint }));
  return el('div', { className: 'row' }, ...children);
}

// The entries of one list, under a small heading for each group in the order they first appear.
function section(entries, kind) {
  const nodes = [];
  let group;
  for (const entry of entries) {
    if (entry.group !== group) { group = entry.group; nodes.push(el('h3', { textContent: group })); }
    nodes.push(row(entry, kind));
  }
  return nodes;
}

const events = el('div', { id: 'events' });
const say = (cls, text) => {
  events.prepend(el('div', { className: cls, textContent: `${new Date().toLocaleTimeString()} ${text}` }));
  while (events.childElementCount > 200) events.lastElementChild.remove();
};

const panel = el('section', { id: 'panel' },
  el('h2', { textContent: 'The lamp, through the SDK' }),
  ...section(table.controls, 'call'),
  el('div', { className: 'world' },
    el('h2', { textContent: 'The room (not the SDK)' }),
    el('p', { className: 'sub', textContent: 'The room starts empty. The lamp calls someone an arrival only after 5 s with nobody there.' }),
    ...section(table.stimuli, 'stim')),
  el('h2', { textContent: 'What the lamp says' }),
  el('p', { className: 'sub', textContent: table.camera || table.microphone
    ? `${[table.camera && 'Camera', table.microphone && 'Microphone'].filter(Boolean).join(' and ')} ON for this run: the page can show the room to anyone on your network with this link (plain HTTP). The lamp sees the room only while it stands up (press a gesture first); folded down its picture is black.`
    : 'Camera and microphone are off: the lamp sees and hears only what the buttons above make. The log shows each line the lamp says out loud (lamp.said, the speech grant).' }),
  events,
);
// The panel first, the joint sliders (still there) folded away below it.
const joints = document.getElementById('joints');
const fold = el('details', {}, el('summary', { textContent: 'Joints (drag a slider to move the body)' }));
joints.before(panel);
panel.after(fold);
fold.append(joints);

window.addEventListener('twin-message', ({ detail }) => {
  if (detail.type === 'result') say(detail.ok ? 'ok' : 'no', `${detail.call} -> ${detail.answer}`);
  if (detail.type === 'event') say('ev', `event ${detail.event} ${JSON.stringify(detail.frame)}`);
});
