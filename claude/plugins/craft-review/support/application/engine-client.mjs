import { evaluate } from '../engine/index.mjs';

// One JSON seam; no host facts, catalog loading, or use-case policy lives here.
export function decide(operation, input = {}) {
  const request = JSON.parse(JSON.stringify({ formatVersion: 1, operation, input }));
  const response = evaluate(request);
  if (!response.ok) { const error = new Error(response.error.message); error.code = response.error.code; throw error; }
  return response.result;
}
export const digest = value => decide('digest', { value });
