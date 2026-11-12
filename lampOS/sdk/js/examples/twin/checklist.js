// The try-it page's checklist (served with panel.js; also run by test/try-checklist.test.js, so it touches no page API
// of its own): one result per row, PASS, FAIL or a note, kept in the browser's localStorage under a key of the lamp's
// id and the mode, so two lamps never share ticks. Export is the record: a JSON file and a short markdown table,
// built from the rows and the results ONLY: never from the page's URL, the token, a pairing code, a pin or a keyring.
export const RESULTS = ['pass', 'fail'];
const PREFIX = 'lampos-try-checklist';

// The key: the lamp's id and the mode (`lamp-sim` or `real`), each cut to safe characters.
export const keyFor = (lampId, mode) => `${PREFIX}:${String(mode).replace(/[^\w.-]/g, '_')}:${String(lampId).replace(/[^\w.-]/g, '_').slice(0, 64)}`;

// The rows of the page's table (`/try.json`): its controls then its stimuli, ids `call:ID` and `stim:ID`; a row that needs a
// grant the lamp lacks (`needs`: 'camera' or 'microphone', and the table's flag of that name false) is not on the page, so it
// is not a row of the checklist: not counted in "n of N ticked", not in the export.
// ONE test for both: the page draws a row exactly when the checklist counts it.
export const visible = (table, entry) => !entry.needs || table[entry.needs] === true;
export function rowsOf(table) {
  const shown = (entry) => visible(table, entry);
  return [...table.controls.filter(shown).map((c) => ({ ...c, kind: 'call' })), ...table.stimuli.filter(shown).map((s) => ({ ...s, kind: 'stim' }))]
    .map((r) => ({ id: `${r.kind}:${r.id}`, label: r.label, group: r.group, kind: r.kind }));
}

// A store over `storage` (the page's localStorage, or any object with getItem/setItem; throws are swallowed:
// private windows and blocked storage give a checklist that works for this page's life only).
export function openChecklist({ storage, key, rowIds, now = () => new Date().toISOString() }) {
  const known = new Set(rowIds);
  let marks = {};
  try {
    const stored = JSON.parse(storage?.getItem(key) ?? '{}');
    for (const [id, mark] of Object.entries(stored)) {
      if (known.has(id) && mark && typeof mark === 'object') marks[id] = clean(mark);
    }
  } catch { marks = {}; }
  const save = () => { try { storage?.setItem(key, JSON.stringify(marks)); } catch { /* kept for this page only */ } };
  return {
    get: (id) => marks[id],
    // `result` pass or fail (or null to clear it); `note` text up to 500 characters.
    set(id, { result, note }) {
      if (!known.has(id)) throw new RangeError(`no row ${id}`);
      const mark = clean({ result: result ?? undefined, note, at: now() });
      if (!mark.result && !mark.note) delete marks[id]; else marks[id] = mark;
      save();
    },
    clear() { marks = {}; save(); },
    // "n of N ticked": a row is ticked once it has a result.
    count: () => ({ ticked: Object.values(marks).filter((m) => m.result).length, of: known.size }),
    marks: () => ({ ...marks }),
  };
}

function clean({ result, note, at }) {
  return {
    ...(RESULTS.includes(result) ? { result } : {}),
    ...(typeof note === 'string' && note.trim() ? { note: note.trim().slice(0, 500) } : {}),
    ...(typeof at === 'string' ? { at: at.slice(0, 40) } : {}),
  };
}

// The export: JSON (rows in table order) and a markdown table, with the lamp's name and id, the mode and the time.
export function exportChecklist({ lamp, mode, rows, marks, now = () => new Date().toISOString() }) {
  const list = rows.map(({ id, label, group, kind }) => ({ row: id, label, group, kind, result: marks[id]?.result ?? 'not run', note: marks[id]?.note ?? '', time: marks[id]?.at ?? '' }));
  const json = { lamp: { id: String(lamp.id), name: String(lamp.name) }, mode, exported_at: now(), rows: list };
  const cell = (text) => String(text).replace(/\|/g, '\\|').replace(/\s+/g, ' ');
  const markdown = [`# Try-it checklist: ${cell(lamp.name)} (${cell(lamp.id)}, ${mode})`, '', `${list.filter((r) => r.result !== 'not run').length} of ${list.length} ticked, ${json.exported_at}`, '', '| row | result | note | time |', '| --- | --- | --- | --- |',
    ...list.map((r) => `| ${cell(r.row)} | ${r.result} | ${cell(r.note)} | ${cell(r.time)} |`), ''].join('\n');
  return { json: `${JSON.stringify(json, null, 2)}\n`, markdown };
}
