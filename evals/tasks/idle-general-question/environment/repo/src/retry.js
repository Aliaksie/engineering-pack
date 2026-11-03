// Retries a failed delivery with a growing delay.
const DEFAULTS = { retries: 3, baseDelayMs: 200, factor: 2 };

export function delayFor(retry, { baseDelayMs = DEFAULTS.baseDelayMs, factor = DEFAULTS.factor } = {}) {
  return baseDelayMs * factor ** retry;
}

export async function withRetry(deliver, { retries = DEFAULTS.retries, sleep = defaultSleep, ...timing } = {}) {
  let lastError;
  for (let retry = 0; retry <= retries; retry++) {
    if (retry > 0) await sleep(delayFor(retry - 1, timing));
    try {
      return await deliver();
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

function defaultSleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
