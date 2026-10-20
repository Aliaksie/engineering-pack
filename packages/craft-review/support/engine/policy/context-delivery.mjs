// Portable context delivery decisions. Lifecycle facts are supplied by the application;
// native event names, hook execution and host wire formats never enter this policy.
export function contextDeliveryPlan({ session, request = {}, lifecycle = {} }, project) {
  let intent = null;
  if (request.edit && project.routing?.editHints === true) intent = 'implement';
  else if (request.prompt !== undefined) {
    for (const [pattern, candidate] of [[/^\s*(?:review|\/review)\b/i,'review'], [/^\s*(?:plan|\/plan)\b/i,'plan'], [/^\s*(?:verify|\/verify)\b/i,'verify'], [/^\s*(?:find|locate|where)\b/i,'locate']]) if (pattern.test(request.prompt)) { intent = candidate; break; }
  }
  const checkpoint = Boolean(session && project.checkpoint?.enabled === true);
  return { intent,
    invalidateHint: Boolean(session && project.routing?.dedupeHints === true && (lifecycle.started || lifecycle.contextChanging || lifecycle.contextRestored)),
    services: Boolean(lifecycle.started && Object.keys(project.services ?? {}).length),
    saveCheckpoint: Boolean(checkpoint && (lifecycle.milestone || lifecycle.turnFinished || lifecycle.contextChanging)),
    recoverCheckpoint: Boolean(checkpoint && (lifecycle.started || lifecycle.contextRestored)),
    readCheckpoint: Boolean(checkpoint && lifecycle.contextChanging) };
}
export function contextDelivery({ routing, checkpoint, services, preserveContinuation }) {
  const context = routing ? { kind: 'routing', data: routing }
    : checkpoint?.saved ? { kind: 'checkpoint', data: checkpoint }
    : services ? { kind: 'services', data: services.services }
    : preserveContinuation ? { kind: 'preserve' } : null;
  return { decision: 'allow', ...(context ? { context } : {}) };
}
export function deliveryFailure(enforcementRequired, message) {
  return enforcementRequired
    ? { decision: 'deny', reason: `Policy could not be evaluated: ${message}` }
    : { decision: 'allow', context: { kind: 'unavailable', data: message } };
}
