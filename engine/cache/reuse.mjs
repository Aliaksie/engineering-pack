export function hintValid(previous, key, now) {
  return Boolean(previous?.key === key && now >= previous.emittedAt && now - previous.emittedAt < 15 * 60 * 1000);
}
export const hintEligible = (event, result) => Boolean(event.routing && event.project?.routing?.dedupeHints === true && event.session && event.contextSupported === true && result.context);

export function validateHint(state) {
  if (state && (state.formatVersion !== 1 || !(state.key === null || /^[a-f0-9]{64}$/.test(state.key)) || !Number.isFinite(state.emittedAt))) throw new Error('invalid routing hint state');
  return state;
}
