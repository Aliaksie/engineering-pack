// The CLI uses the same normalized result as adapters; unknown telemetry stays visible.
export function renderSessionStatus(status) {
  const value = (item) => item === null || item === undefined ? 'unknown' : String(item);
  const { usage, host, packContext, activity } = status;
  const suggestion = status.recommendation ?? status.suggestion;
  const contextLine = usage.status === 'observed'
    ? `${Math.round(usage.usedRatio * 100)}% (${usage.currentTokens}/${usage.capacityTokens}); ${status.pressure}; ${usage.source}`
    : `unknown — ${usage.reason}`;
  const lines = [
    'Engineering Pack — Session', `Pack version: ${value(status.packVersion)}`, `Host: ${host.name} | Model: ${host.model}`,
    `Context: ${contextLine}`,
    `Pack: ${packContext.estimatedTokens === null ? 'unknown' : `~${packContext.estimatedTokens}`} estimated tokens selected; ${value(packContext.skills)} skills / ${value(packContext.references)} references`,
    `Task: ${status.operation} | Attempts: ${value(activity.attempts)}/${value(activity.maxAttempts)}`,
    `Discovery: ${value(activity.reuseHits)} reused / ${value(activity.freshLookups)} fresh / ${value(activity.batchedOperations)} batches (pack activity)`,
    `Capability: ${status.capability?.class ?? 'unknown'} (current: ${status.capability?.currentClass ?? 'unknown'})${status.modelAdvice?.profile ? `; configured profile ${status.modelAdvice.profile}` : ''}`,
    `Checkpoint: ${status.checkpoint?.status ?? 'unknown'}`,
    '', `Advice: ${suggestion?.action ?? 'continue'}`,
    `Reason: ${suggestion?.reason ?? (usage.status === 'unknown'
      ? 'No reliable occupancy observation; continue useful work without inferring context pressure.'
      : 'Continue useful work; no context change is indicated.')}`
  ];
  if (status.capability?.recommendation) lines.push(`Model advice: ${status.capability.recommendation}`);
  if (suggestion?.afterCheckpoint) lines.push(`After checkpoint: ${suggestion.afterCheckpoint}`);
  if (suggestion?.atBoundary) lines.push(`At boundary: ${suggestion.atBoundary}`);
  return `${lines.join('\n')}\n`;
}
