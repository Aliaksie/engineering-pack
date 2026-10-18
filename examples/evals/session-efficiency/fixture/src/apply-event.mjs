import { eventKey } from './event-key.mjs';
import { validateEvent } from './validate-event.mjs';
import { markApplied } from './mark-applied.mjs';
import { readTotal } from './read-total.mjs';
import { receipt } from './receipt.mjs';

export function applyEvent(state, event) {
  validateEvent(event);
  const key = eventKey(event);
  const total = readTotal(state, event.account) + event.delta;
  state.totals.set(event.account, total);
  markApplied(state, key);
  return receipt(event, total, false);
}
