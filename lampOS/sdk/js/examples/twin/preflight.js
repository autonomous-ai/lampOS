// The controller app's pre-flight: presses every row of a running try-it page with its defaults, one by one, and writes
// what happened as a markdown table: PASS (the lamp took it), FAIL (it refused or did not answer: the words are the
// evidence), REFUSED (as designed: the lamp did not grant what the row takes, drawn off by the page or answered `no_grant`
// under this pairing: the reason is in the lamp's words) or n/a (not on this build, the run's flag is off, the turn was over,
// skipped by the person). A REFUSED does not fail the run: the exit code is 1 only for a FAIL. The same file runs on the virtual
// lamp and on a real one.
//
//   node examples/twin/preflight.js --url 'http://HOST:PORT/?t=TOKEN' [--out FILE] [--yes-moves] [--only call:ROW,call:ROW]
//
// `--only` presses just those rows (ids as the table writes them, in the page's order; any other id is an error, exit 2) and says
// `only: ...` in the table's header: to rerun a few rows on a lamp. Cleanup is unchanged.
//
// It speaks the page's own websocket with the URL's token, so it can do what a person at the page can and no more. In a
// real-lamp run a row that moves the body, or that shows the room (camera, microphone), is asked about first on the
// terminal (y/N) unless `--yes-moves` says yes to all of them; a "no" is n/a, not FAIL. It ends with this run's clip and notes
// taken off (the rows' own removes) and the volume put back: its level and mute are read before the first row and set back at the very end, both said in the table's header. The URL, with its token,
// is never printed or written; neither is a pairing code or pin (it never sees them).
import { createInterface } from 'node:readline';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// `work` or `value` after `ms`, whichever comes first; the timer is cleared either way. (A race against a bare sleep leaves its
// timer running: 49 rows leave 49 of them, and the process lives on until the longest ends.)
export const within = (work, ms, value) => {
  let timer;
  return Promise.race([work, new Promise((resolve) => { timer = setTimeout(() => resolve(value), ms); })]).finally(() => clearTimeout(timer));
};
const cell = (text) => String(text ?? '').replace(/\|/g, '\\|').replace(/\s+/g, ' ').trim();

// What the result cell says: PASS, FAIL, REFUSED, n/a, or a PASS that the page's own claim did not bear out: the events the row said it
// would show and that were not seen are named, nothing more (whether a lamp or a sim should have brought it is for the reader).
export function resultText(row) {
  if (row.result !== 'PASS' || !row.missing?.length) return row.result;
  return `PASS (expected event not seen: ${row.missing.join(', ')})`;
}

const volumeText = ({ level, muted }) => `${level}${muted ? ' (muted)' : ''}`;

// The markdown of a run: the counts, then one line per row in the page's order.
export function tableOf({ lamp, mode, rows, at, only, volume, ended }) {
  const count = (result) => rows.filter((r) => r.result === result).length;
  const unseen = rows.filter((r) => r.result === 'PASS' && r.missing?.length).length;
  return [
    `# Controller pre-flight: ${cell(lamp.name || lamp.id)} (${cell(lamp.id)}, ${mode}) ${at}`, '',
    ...(only ? [`only: ${only.join(', ')}`, ''] : []),
    ...(ended ? [`ended early: ${ended}`, ''] : []),
    ...(volume ? [`volume: ${volumeText(volume.before)} before the first row; ${volume.after ? `${volumeText(volume.after)} after the last${volume.after.level === volume.before.level && volume.after.muted === volume.before.muted ? ' (put back)' : ' (NOT put back)'}` : 'not read again'}`, ''] : []),
    `${count('PASS')} PASS${unseen ? ` (${unseen} with an expected event not seen)` : ''}, ${count('FAIL')} FAIL, ${count('REFUSED')} refused as designed, ${count('n/a')} n/a, of ${rows.length} rows`, '',
    '| row | result | expected events | evidence |', '| --- | --- | --- | --- |',
    ...rows.map((r) => `| ${cell(r.id)} | ${cell(resultText(r))} | ${cell(r.expected)} | ${cell(r.evidence)} |`), '',
  ].join('\n');
}

// One y/N question on the terminal; no terminal means no. A Ctrl-C at the question (readline holds the terminal in raw mode, so the
// key reaches the reader as `SIGINT` and never the process) answers `null`: not a no to this row, but the run's stop. The input
// closing (Ctrl-D) is a no.
export function askYesNo(question, { input = process.stdin, output = process.stderr, signal } = {}) {
  if (!input.isTTY) return Promise.resolve(false);
  if (signal?.aborted) return Promise.resolve(null);
  return new Promise((resolve) => {
    const reader = createInterface({ input, output });
    let settled = false;
    const settle = (value) => { if (!settled) { settled = true; signal?.removeEventListener('abort', onAbort); resolve(value); } reader.close(); };
    // With stderr not a terminal (2>run.log) readline takes no raw mode and raises no SIGINT of its own: the process's own handler
    // aborts `signal`, and that answers the question too.
    const onAbort = () => settle(null);
    signal?.addEventListener('abort', onAbort, { once: true });
    reader.on('SIGINT', () => settle(null));
    reader.on('close', () => settle(false));
    reader.question(`${question} [y/N] `, (answer) => settle(/^y(es)?$/i.test(answer.trim())));
  });
}

// The page's reason for a row it draws off because of a grant (describe() in controls.js): the only kind of "off" that is REFUSED.
const GRANT_REASON = /^the lamp did not grant /;
// Whether `answer` is the lamp's `no_grant` for a grant this pairing lacks. `grants` are the page's (hello's); a page that
// does not say them leaves any `no_grant` as a refusal by design.
export function grantRefusal(answer, grants) {
  const text = String(answer ?? '');
  if (!/^no_grant:|not granted/.test(text)) return false;
  if (!Array.isArray(grants)) return true;
  // The lamp names what it lacks after `not granted:`, one grant or a list.
  // (A line may go on after the names: `not granted: camera (only the owner can add it: ...)`.)
  const named = (/not granted: ([a-z_]+(?:, ?[a-z_]+)*)/.exec(text)?.[1] ?? '').split(/,\s*/).filter(Boolean);
  return named.length === 0 || named.some((grant) => !grants.includes(grant));
}

// The lamp's words for an op or a field its build does not know (lamp-control's parser: serde's `unknown variant ...` for an
// op, `unknown field ...` for a field of a newer SDK, `not routed yet` for a feature the build has not wired): not a failure of
// the row, a lamp from before the feature.
// A part the build has not wired yet says `not routed yet` (a `play` on lamp-sim, whose lamp-control waits for client packages).
export const NOT_ON_THIS_BUILD = /^(bad_request: .*(unknown variant|unknown field)|refused: not routed yet)/s;
export const notOnBuild = (answer) => NOT_ON_THIS_BUILD.test(String(answer ?? ''));

// `turn_end` after `turn_start`: the lamp's turn is often over already, and it says so in its own words; that is the turn
// being over, not the row failing.
export const TURN_ALREADY_OVER = /^(refused: spoken|busy: not this computer's turn)/;
// And the other way round: pressed right after `turn_start`, a real lamp says the turn has not begun; it begins a moment later.
export const TURN_NOT_BEGUN = /^busy: this computer's turn has not begun yet/;

// The events a row said it would show (`shows`), against the ones heard since the press: 'seen', or 'NOT SEEN: ...'. A
// real lamp's events can come seconds after the press (a person.left after its delay), so they are waited for.
export function expectedEvents(shows, heard) {
  if (!shows?.length) return '';
  const missing = shows.filter((name) => !heard.includes(name));
  return missing.length ? `NOT SEEN: ${missing.join(', ')}` : 'seen';
}

// The exit code of a run: 130 for a Ctrl-C, 1 for a FAIL or a run the page cut short (3 rows of 40 is not a pass), else 0.
export const exitCodeOf = ({ aborted, interrupted, cutShort, rows }) => (aborted || interrupted ? 130 : cutShort || rows.some((r) => r.result === 'FAIL') ? 1 : 0);

// Runs the pre-flight against the page at `url`. `log` gets one short line per row (never the url).
export async function runPreflight({ url, signal, yesMoves = false, ask = askYesNo, log = () => {}, settleMs = 2000, expectMs = 6000, retryMs = 8000, retryEveryMs = 1000, only, answerMs = 40_000, now = () => new Date().toISOString() }) {
  const page = new URL(url);
  const token = page.searchParams.get('t');
  if (!token) throw new Error('the url has no ?t= token');
  const base = `http://${page.host}`;
  const reply = await fetch(`${base}/try.json?t=${encodeURIComponent(token)}`);
  if (!reply.ok) throw new Error(`the page answered ${reply.status} for its table (is the token in the url right and current?)`);
  const table = await reply.json();
  const socket = new WebSocket(`ws://${page.host}/live?t=${encodeURIComponent(token)}`, { headers: { origin: base } });
  const heard = [];
  const waiting = new Map();
  // A socket that closes mid-run ends the run: every row waiting is answered at once, and no later row is pressed (each would wait
  // out its whole answer time for nothing).
  // `sent` says which: the row went out and its answer never came, or the socket was already closed and nothing was sent.
  const CLOSED = { ok: false, answer: 'the connection to the page closed', closed: true };
  let closed = false;
  let cutShort = false; // the page went away before every row was pressed: the run did not finish
  let interrupted = false;
  const rows = [];
  socket.on('close', () => {
    closed = true;
    for (const resolve of waiting.values()) resolve({ ...CLOSED, sent: true });
  });
  socket.on('message', (data) => {
    let message;
    try { message = JSON.parse(data); } catch { return; }
    if (message.type === 'result') waiting.get(message.id)?.(message);
    else if (message.type === 'event') heard.push({ event: message.event, at: Date.now(), frame: message.frame });
    else if (message.type === 'picture') heard.push({ event: `picture (${Buffer.from(message.jpeg ?? '', 'base64').length} bytes)`, at: Date.now() });
  });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let id = 0;
  const real = table.mode === 'real';
  let movesTold = false;
  // `only`: ids like call:say, in the page's order; one the page does not have is an error, said before anything is pressed.
  const known = [...table.controls.map((c) => `call:${c.id}`), ...table.stimuli.map((s) => `stim:${s.id}`)];
  if (only) {
    const unknown = only.filter((name) => !known.includes(name));
    if (unknown.length) { socket.close(); throw new Error(`--only: no such row ${unknown.join(', ')} (rows: ${known.join(', ')})`); }
  }
  // The one way anything is sent to the page: its answer, `timedOut` after `ms`, or CLOSED at once when the socket is not open
  // (a socket that closes later answers the waiting ones in its close handler).
  const send = async (message, ms, timedOut) => {
    if (socket.readyState !== WebSocket.OPEN) return { ...CLOSED, sent: false };
    id += 1;
    const mine = id;
    const answer = new Promise((resolve) => waiting.set(mine, resolve));
    socket.send(JSON.stringify({ ...message, id: mine }));
    const result = await within(answer, ms, timedOut);
    waiting.delete(mine);
    return result;
  };
  // One row pressed for its answer (not a row of the table): `{ ok, call, answer }`, or null when it is not on this page, off, or
  // the page is gone.
  const quietly = async (name, args = {}) => {
    const entry = table.controls.find((c) => c.id === name);
    if (!entry || entry.disabled) return null;
    const result = await send({ type: 'call', name, args }, 8000, null);
    return result?.closed ? null : result;
  };
  // The lamp's level and mute, as `get volume` answers them (`{"volume":{"level":N,"muted":B}}` after the arrow in its line).
  const volumeNow = async () => {
    const result = await quietly('get', { state: 'volume' });
    try {
      const said = JSON.parse(String(result?.call ?? '').split(' -> ').slice(1).join(' -> '));
      const volume = said.volume ?? said;
      return typeof volume.level === 'number' ? { level: volume.level, muted: Boolean(volume.muted) } : null;
    } catch { return null; }
  };
  // The person's own volume, read BEFORE any row is pressed, and put back at the very end (also after a Ctrl-C).
  const volumeBefore = only && !only.some((name) => /volume/.test(name)) ? null : await volumeNow();
  let volumeAfter = null;
  try {
    for (const [kind, list] of [['call', table.controls], ['stim', table.stimuli]]) {
      for (const entry of list) {
        if (closed && !signal?.aborted && !interrupted) cutShort = true;
        if (signal?.aborted || interrupted || closed) break;
        const rowId = `${kind === 'call' ? 'call' : 'stim'}:${entry.id}`;
        if (only && !only.includes(rowId)) continue;
        if (entry.disabled) {
          // Off because the lamp did not grant what the row takes: refused as designed. Off for another reason (the run's flag):
          // n/a.
          const refused = GRANT_REASON.test(entry.disabled);
          // A grant refusal here is the PAGE's (drawn from the grants in the lamp's hello): the lamp was sent nothing for this row.
          rows.push({ id: rowId, result: refused ? 'REFUSED' : 'n/a', evidence: refused ? `refused by the page, from the grants the lamp named in hello (nothing was sent to the lamp): ${entry.disabled}` : entry.disabled });
          log(`${rowId}: ${refused ? 'REFUSED' : 'n/a'}`);
          continue;
        }
        // A real lamp: what moves it or shows the room is the person's call, row by row.
        const touches = entry.moves || entry.needs?.some((g) => g === 'camera' || g === 'microphone');
        if (real && touches && !yesMoves) {
          const said = await ask(`Press ${rowId} (${entry.label}) on the REAL lamp?`, { signal });
          // A Ctrl-C at the question stops the run: this row is not pressed, and the clean-up below still runs.
          if (said === null) { interrupted = true; log(`${rowId}: stopped by Ctrl-C at the question`); break; }
          if (!said) {
            rows.push({ id: rowId, result: 'n/a', evidence: 'skipped: not confirmed by the person' });
            log(`${rowId}: skipped`);
            continue;
          }
        }
        // The person's yes (the question just answered, or --yes-moves) is the page's "this moves the real lamp": the server holds
        // a move until it has been told, once per connection.
        if (real && entry.moves && !movesTold) { socket.send(JSON.stringify({ type: 'moves_ok' })); movesTold = true; }
        const since = Date.now();
        const noAnswer = { ok: false, answer: `no answer in ${answerMs / 1000} s` };
        let result = await send({ type: kind, name: entry.id, args: {} }, answerMs, noAnswer);
        // Only the sequence is waited for, never the row: `turn_end` is pressed again every second while the lamp says its turn has
        // not begun, for up to 8 s.
        let neverBegan = false;
        if (entry.id === 'turn_end') {
          const until = Date.now() + retryMs;
          while (!result.ok && TURN_NOT_BEGUN.test(String(result.answer)) && Date.now() < until) {
            await sleep(retryEveryMs);
            const next = await send({ type: 'call', name: entry.id, args: {} }, answerMs, noAnswer);
            // The page went away in the wait before the retry: nothing was sent, so the row keeps the lamp's earlier answer (it WAS
            // pressed, once) and the run is cut short.
            if (next.closed && !next.sent) { cutShort = true; break; }
            result = next;
          }
          neverBegan = !result.ok && TURN_NOT_BEGUN.test(String(result.answer));
        }
        // The page went away: either while this row was out (it was pressed, its answer never came: FAIL) or before it went out (it
        // was NOT pressed: n/a, and the table must not say a moving row was pressed when it was not). No events are waited for.
        if (result.closed) {
          cutShort = true;
          const verdict = result.sent ? 'FAIL' : 'n/a';
          rows.push({ id: rowId, result: verdict, evidence: result.sent ? 'pressed; the connection to the page closed before its answer' : 'not pressed: the connection to the page had closed' });
          log(`${rowId}: ${verdict}`);
          break;
        }
        // The events it said it would show are waited for (up to expectMs after the press), the rest settle for settleMs.
        const heardNow = () => heard.filter((h) => h.at >= since).map((h) => h.event);
        const shows = entry.shows ?? [];
        if (shows.length) {
          while (Date.now() - since < expectMs && !shows.every((name) => heardNow().includes(name))) await sleep(100);
          await sleep(Math.min(settleMs, 300));
        } else await sleep(settleMs);
        // A turn the lamp took by the call and then refused in its `done` (how dropped, why "refused: the lamp is speaking") did not
        // run, whatever the call said: pressed again every `retryEveryMs` for up to `retryMs` (the say before it may still be
        // playing), and if it is still refused, n/a with the lamp's words, never a PASS.
        const turnRefusal = (from) => heard.find((h) => h.at >= from && h.event === 'done' && h.frame?.how === 'dropped' && /^refused/.test(String(h.frame?.why)))?.frame?.why;
        let refusedTurn = entry.id === 'turn_start' && result.ok ? turnRefusal(since) : undefined;
        if (refusedTurn) {
          const until = Date.now() + retryMs;
          while (refusedTurn && Date.now() < until) {
            await sleep(retryEveryMs);
            const from = Date.now();
            const previous = result;
            result = await send({ type: 'call', name: entry.id, args: {} }, answerMs, noAnswer);
            // The page went away: before the retry went out (nothing was sent: the row keeps what the lamp said, the refusal stands)
            // or after (sent, never answered: FAIL). Either way the run is cut short.
            if (result.closed) { cutShort = true; if (result.sent) refusedTurn = undefined; else result = previous; break; }
            // A retry the lamp did not take (a refusal, no answer) is that call's own failure, not the earlier refused turn: FAIL.
            if (!result.ok) { refusedTurn = undefined; break; }
            await sleep(settleMs);
            refusedTurn = turnRefusal(from);
          }
        }
        const events = [...new Set(heardNow())];
        // A lamp from before the feature says so in its parser's words: n/a, with the words beside it.
        const missing = !result.ok && notOnBuild(result.answer);
        const over = !result.ok && entry.id === 'turn_end' && (TURN_ALREADY_OVER.test(String(result.answer)) || neverBegan);
        // The lamp refusing for a grant this pairing does not hold (`no_grant`, naming it) is the design working.
        const refusedForGrant = !result.ok && !missing && !over && grantRefusal(result.answer, table.grants);
        const verdict = refusedTurn ? 'n/a' : result.ok ? 'PASS' : refusedForGrant ? 'REFUSED' : missing || over ? 'n/a' : 'FAIL';
        const said = refusedTurn ? `the lamp took the call and refused the turn (the lamp said: ${refusedTurn})`
          : missing ? `not on this lamp's build (the lamp said: ${String(result.answer).slice(0, 200)})`
          : over ? `${neverBegan ? 'the turn never began' : "the lamp's turn was over before end was pressed"} (the lamp said: ${result.answer})`
            : `${result.call ?? entry.id}${result.ok ? '' : ` -> ${result.answer}`}`;
        rows.push({ id: rowId, result: verdict, missing: verdict === 'PASS' ? shows.filter((name) => !events.includes(name)) : [], expected: expectedEvents(shows, events), evidence: [said, events.length ? `events: ${events.join(', ')}` : ''].filter(Boolean).join('; ') });
        log(`${rowId}: ${verdict}`);
      }
    }
  } finally {
    // However it ends (a finished run, a Ctrl-C), this run's own clip and note come off the lamp: the rows that remove them are
    // pressed again (they touch only what this run made, and say so when none is kept); then the page's own end does the rest.
    for (const name of ['sound_remove', 'notes_delete']) {
      const entry = table.controls.find((c) => c.id === name);
      if (!entry || entry.disabled) continue;
      await send({ type: 'call', name, args: {} }, 5000, null);
    }
    // The volume the rows left is put back to what it was before the first row (only what differs is pressed).
    if (volumeBefore) {
      let found = await volumeNow();
      if (found && found.level !== volumeBefore.level) { await quietly('volume_to', { to: volumeBefore.level }); found = await volumeNow(); }
      if (found && found.muted !== volumeBefore.muted) { await quietly('volume_mute', { mute: volumeBefore.muted ? 'on' : 'off' }); found = await volumeNow(); }
      volumeAfter = found;
    }
    socket.close();
  }
  const volume = volumeBefore ? { before: volumeBefore, after: volumeAfter } : null;
  const ended = cutShort ? `the connection to the page closed after ${rows.length} rows; the rest were not pressed` : interrupted ? 'stopped by a Ctrl-C at a question; the rest were not pressed' : null;
  return { markdown: tableOf({ lamp: table.lamp, mode: table.mode, rows, at: now(), only, volume, ended }), rows, mode: table.mode, volume, interrupted, cutShort };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const get = (name) => { const k = args.indexOf(name); return k >= 0 ? args[k + 1] : undefined; };
  const url = get('--url');
  if (!url || args.some((a) => a.startsWith('--') && !['--url', '--out', '--yes-moves', '--only'].includes(a))) {
    console.error("usage: node examples/twin/preflight.js --url 'http://HOST:PORT/?t=TOKEN' [--out FILE] [--yes-moves] [--only call:ROW,call:ROW]");
    process.exit(2);
  }
  // A Ctrl-C stops pressing rows, still cleans this run's scratch off the lamp, and still writes what was done.
  const stopper = new AbortController();
  for (const signalName of ['SIGINT', 'SIGTERM']) process.on(signalName, () => stopper.abort());
  try {
    const { markdown, rows, interrupted, cutShort } = await runPreflight({ url, signal: stopper.signal, yesMoves: args.includes('--yes-moves'), only: get('--only')?.split(',').map((x) => x.trim()).filter(Boolean), log: (line) => console.error(line) });
    if (get('--out')) writeFileSync(get('--out'), markdown); else console.log(markdown);
    process.exit(exitCodeOf({ aborted: stopper.signal.aborted, interrupted, cutShort, rows }));
  } catch (error) {
    // Neither the url nor its token (as typed or as encoded in a request line) is ever printed.
    const token = (() => { try { return new URL(url).searchParams.get('t'); } catch { return null; } })();
    let said = String(error.message).replaceAll(url, '<the url>');
    for (const secret of token ? [token, encodeURIComponent(token)] : []) said = said.replaceAll(secret, '<token>');
    console.error(`preflight: ${said}`);
    process.exit(2);
  }
}
