export function validateEvent(event) {
  if (typeof event.account !== 'string' || !event.account || typeof event.id !== 'string' || !event.id || !Number.isSafeInteger(event.delta)) throw new Error('invalid event');
}
