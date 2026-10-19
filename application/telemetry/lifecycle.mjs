import { emitTelemetry } from './observe.mjs';

export const observational = true;
// Lifecycle observation is independent of delivery routing and project configuration.
export function prepare(event) {
  emitTelemetry('host.observed', { lifecycle: event.event, contextSupported: event.contextSupported });
  return event;
}
export const evaluate = () => ({ decision: 'allow' });
