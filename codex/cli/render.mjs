// The CLI uses the same normalized result as adapters; unknown telemetry stays visible.
export function renderCommand(action, result, { json = false, verbose = false, help = false } = {}) {
  if (!json && !verbose && !help) {
    if (action === 'session' || action === 'session-advice') return renderSessionStatus(result);
    const compact = renderReceipt(action, result);
    if (compact !== null) return compact;
  }
  return `${JSON.stringify(result, null, action === 'locate' ? undefined : 2)}\n`;
}

function checkpointLine(checkpoint) {
  if (!checkpoint) return 'Checkpoint: unknown';
  if (checkpoint.status) return `Checkpoint: ${checkpoint.status}${checkpoint.reason ? ` — ${checkpoint.reason}` : ''}`;
  const incomplete = checkpoint.state?.fingerprints?.complete === false;
  return `Checkpoint: ${checkpoint.unchanged ? 'unchanged' : 'saved'}${incomplete ? ' (repository inspection incomplete)' : ''}${checkpoint.file ? ` — ${checkpoint.file}` : ''}`;
}

function offerLines(result) {
  const offer = result.offer;
  if (!offer) return [`Offer: unavailable — ${result.applicability?.reason ?? 'No action proposed.'}`];
  return [`Offer: ${offer.id}`, `Action: ${offer.action} | Scope: ${offer.scope}`,
    `Decision: ${offer.decision}${result.suppressed ? ' (unchanged; suppressed)' : ''}`,
    `Execution: ${offer.execution ?? 'not-observed'}`];
}

function renderReceipt(action, result) {
  let lines;
  if (action === 'task-step') {
    const receipt = result.recorded;
    lines = [`Step recorded: ${receipt.operation} / ${receipt.target} — ${receipt.outcome}`,
      checkpointLine(result.checkpoint)];
    if (receipt.outcome !== 'passed') lines.push(`Result: ${result.result}`);
    if (result.complete) lines.push(result.acceptance?.assurance === 'attributed' ? 'Task: complete (attributed criterion evidence)' : 'Task: complete (agent-declared)');
    const next = result.nextStep;
    if (next) {
      lines.push(`Next: ${next.action}${next.reason ? ` — ${next.reason}` : ''}`);
      for (const check of next.checks ?? []) lines.push(`Check: ${check.cwd} ${JSON.stringify([check.command, ...(check.args ?? [])])}`);
      if (next.afterCheckpoint) lines.push(`After checkpoint: ${next.afterCheckpoint}`);
    }
    if (result.actionOffer && !result.actionOffer.suppressed) lines.push(...offerLines(result.actionOffer));
  } else if (action === 'task-start') {
    lines = [`Task bound: ${result.task}`, `Execution: ${result.routing.execution.mode} / ${result.routing.execution.role}`];
    if (result.repositories) lines.push(`Repositories: ${result.repositories.join(', ')}`);
    if (result.routing.skills.length) lines.push(`Skills: ${result.routing.skills.join(', ')}`);
    if (result.routing.references.length) lines.push(`References: ${result.routing.references.join(', ')}`);
    lines.push(checkpointLine(result.checkpoint));
    const model = result.modelAdvice;
    if (model) lines.push(`Model advice: ${model.decision}${model.profile ? ` / ${model.profile}` : ''}${model.mainModel?.limitation ? ` — ${model.mainModel.limitation}` : ' (recommendation only)'}`);
    for (const missing of result.routing.missing) lines.push(`Missing: ${missing}`);
  } else if (action === 'checkpoint-save') {
    lines = [checkpointLine(result)];
  } else if (action === 'session-attempt') {
    const receipt = result.recorded;
    lines = [`Attempt recorded: ${receipt.operation} / ${receipt.target} — ${receipt.outcome}`];
  } else if (['task-offer', 'task-decision', 'task-action-check'].includes(action)) {
    lines = offerLines(result);
    if (action === 'task-action-check') lines.push(`Ready: ${result.ready === true}`, result.reason,
      'Readiness is not execution or a permission grant.');
    else lines.push('No action dispatched.');
  } else return null;
  if (result.acceptance) {
    lines.push(`Acceptance: ${result.acceptance.assurance}`);
    for (const criterion of result.acceptance.criteria) lines.push(`Criterion: ${criterion.id} — ${criterion.status}${criterion.required ? '' : ' (optional)'}`);
  }
  if (result.taskContract) lines.push(`Task contract: ${result.taskContract}`);
  if (result.evidenceDigest) lines.push(`Evidence digest: ${result.evidenceDigest}`);
  if (result.binding) lines.push(`Source: ${result.binding.identity ?? 'unknown'}`);
  const status = result.status;
  if (result.workspace?.status === 'retained' && result.workspace.reason) lines.push(`Workspace retained: ${result.workspace.reason}`);
  if (status?.taskEvidenceStale) lines.push('Warning: task evidence is stale.');
  const warnings = [result.activityDiagnostic, status?.diagnostic, status?.actionDiagnostic, status?.noticeDiagnostic, ...(status?.diagnostics ?? [])];
  for (const warning of warnings.filter(Boolean)) lines.push(`Warning: ${warning}`);
  return `${lines.filter(Boolean).join('\n')}\n`;
}

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
  if (status.acceptance) {
    lines.push(`Acceptance: ${status.acceptance.assurance}`);
    for (const criterion of status.acceptance.criteria) lines.push(`Criterion: ${criterion.id} — ${criterion.status}${criterion.required ? '' : ' (optional)'}`);
  }
  if (status.taskContract) lines.push(`Task contract: ${status.taskContract}`);
  if (status.evidenceDigest) lines.push(`Evidence digest: ${status.evidenceDigest}`);
  if (status.binding) lines.push(`Source: ${status.binding.identity ?? 'unknown'}`);
  if (status.capability?.recommendation) lines.push(`Model advice: ${status.capability.recommendation}`);
  if (status.repositories) lines.push(`Repositories: ${status.repositories.join(', ')}`);
  if (status.workspace?.status === 'available') lines.push(`Workspace: scratch ${status.workspace.scratch}; ${status.workspace.evidence.artifacts} evidence artifacts; ${status.workspace.diskBytes} bytes`);
  else if (status.workspace?.status === 'retained') lines.push(`Workspace: retained — ${status.workspace.reason}`);
  if (suggestion?.afterCheckpoint) lines.push(`After checkpoint: ${suggestion.afterCheckpoint}`);
  if (suggestion?.atBoundary) lines.push(`At boundary: ${suggestion.atBoundary}`);
  return `${lines.join('\n')}\n`;
}
