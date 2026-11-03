import { orderConfirmed, orderDelayed, orderShipped } from './messages.js';
import { withRetry } from './retry.js';

const TEMPLATES = { confirmed: orderConfirmed, shipped: orderShipped, delayed: orderDelayed };

export async function send(channel, kind, order, ...args) {
  const render = TEMPLATES[kind];
  if (!render) throw new Error(`unknown notification: ${kind}`);
  const text = render(order, ...args);
  return withRetry(() => channel.deliver(order.contact, text));
}
