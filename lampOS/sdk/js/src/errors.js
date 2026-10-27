// The lamp's refusals as JavaScript errors: one class per error code.
import { errorCodes } from './schema.js';

export class LampError extends Error {
  constructor(why, { code = 'error', retryMs } = {}) {
    super(`${code}: ${why}`);
    this.name = 'LampError';
    this.code = code;
    this.why = why;
    this.retryMs = retryMs;
  }
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
