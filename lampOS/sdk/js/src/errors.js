// The lamp's refusals as JavaScript errors: one class per error code.
import { errorCodes } from './schema.js';

// What a developer can do about each code, said with the error (`whatToDo`)
// and in both READMEs; the Python SDK has the same words (errors.py).
export const WHAT_TO_DO = Object.freeze({
  unpaired: "Pair again with a new code from the owner's app (`lamp pair CODE --pin PIN`): this computer isn't paired, was revoked, or its key no longer matches.",
  no_hello: "Say hello first: `Lamp.connect()` does; with `request()` alone, send `hello` before anything else.",
  no_grant: "Ask the owner to tick this grant for this computer in the app, then connect again: grants are read at hello.",
  no_lease: "Hold a `body` lease first (Python `lamp.lease(\"body\")`, JS `lamp.withLease(['body'], ...)`); `joints` names it.",
  lease_lost: "The lamp took the body back; `why` says why (a conversation, a hand, private, a stop). Let it be, and take a new lease later.",
  busy: "A motion still plays (a gesture, move or feeling's move, whoever asked): wait `retry_ms` and send it again. Without `retry_ms`, someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first.",
  refused: "The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again.",
  quiet_hours: "It's the lamp's quiet hours (`limits.quiet_hours`, by default 22:00 to 07:00): `say` and `sound` wait until they end. A light is still fine.",
  private: "The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false.",
  paused: "The person paused the lamp. Wait for `lamp.changed` with `paused` false.",
  asleep: "The lamp is asleep. Wait for `lamp.changed` with `asleep` false.",
  rate: "Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded.",
  bad_request: "The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json).",
  off: "Developer access is off on this lamp: the owner turns it on in the app.",
});

export class LampError extends Error {
  constructor(why, { code = 'error', retryMs } = {}) {
    super(`${code}: ${why}`);
    this.name = 'LampError';
    this.code = code;
    this.why = why;
    this.retryMs = retryMs;
  }

  // What to do about it, for a code the protocol has; '' otherwise.
  get whatToDo() { return WHAT_TO_DO[this.code] ?? ''; }
}

export class NotPaired extends LampError {
  constructor(why) { super(why, { code: 'not_paired' }); this.name = 'NotPaired'; }
}

export class WrongLamp extends LampError {
  constructor(why) { super(why, { code: 'wrong_lamp' }); this.name = 'WrongLamp'; }
}

const className = (code) => code.split('_').map((w) => w[0].toUpperCase() + w.slice(1)).join('') + 'Error';

// UnpairedError, NoGrantError, RateError, ...: made from the schema's codes.
export const byCode = Object.fromEntries(errorCodes.map((code) => {
  const cls = { [className(code)]: class extends LampError {
    constructor(why, options = {}) { super(why, { ...options, code }); this.name = className(code); }
  } }[className(code)];
  return [code, cls];
}));

export function fromReply(error = {}) {
  const cls = byCode[error.code];
  const options = { code: error.code, retryMs: error.retry_ms };
  return cls ? new cls(error.why ?? '', options) : new LampError(error.why ?? '', options);
}
