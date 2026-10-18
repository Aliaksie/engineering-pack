// Shared policy. Host normalization and optional checkpoint IO belong to the runtime.
export function evaluate(event) {
  if (event.routing) {
    const { intent, execution, nativeSkills, skills, references, modules, missing, taskBoundary } = event.routing;
    return { decision: 'allow', context: `Routing hints (no authorization implied; reuse until scope/config changes): ${JSON.stringify({ intent, execution, skills: nativeSkills ?? skills, references, modules, missing, ...(taskBoundary ? { taskBoundary } : {}) })}` };
  }
  if (event.checkpoint?.saved) return { decision: 'allow', context: `Checkpoint data; verify against the current tree before continuing: ${JSON.stringify(event.checkpoint)}` };
  if (event.services) return { decision: 'allow', context: `Configured service map (not live verification; reuse within this session, refresh on binding/tool/account changes or access failures): ${JSON.stringify(event.services.services)}` };
  if (event.event === 'before-compact') return { decision: 'allow', context: 'Preserve task scope, original authorization, evidence and next step in the project checkpoint. Compaction hooks do not replace milestone checkpoints.' };
  return { decision: 'allow' };
}
