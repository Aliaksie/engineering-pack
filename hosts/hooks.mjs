import { HOSTS, hookMapping } from './registry.mjs';

export function normalize(payload, platform, definition) {
  let input = payload.tool_input ?? payload.toolArgs ?? {};
  if (typeof input === 'string') { try { input = JSON.parse(input); } catch { input = {}; } }
  const name = String(payload.tool_name ?? payload.toolName ?? '');
  const tool = /^(?:Bash|Shell|powershell|pwsh|exec_command|run_shell_command)$/i.test(name) ? 'shell'
    : /^(?:Edit|Write|MultiEdit|edit|write|apply_patch|replace|write_file)$/i.test(name) ? 'edit'
    : /^(?:Read|read|read_file|view)$/i.test(name) ? 'read' : 'other';
  const rawSession = payload.session_id ?? payload.sessionId ?? payload.conversation_id;
  return { event: definition.event, host: platform, tool,
    contextSupported: hookMapping(platform, definition)?.context === true,
    cwd: typeof payload.cwd === 'string' ? payload.cwd : process.cwd(),
    command: typeof input?.command === 'string' ? input.command : typeof input?.cmd === 'string' ? input.cmd : '',
    file: typeof input?.file_path === 'string' ? input.file_path : typeof input?.path === 'string' ? input.path : undefined,
    prompt: typeof payload.prompt === 'string' ? payload.prompt : '',
    session: typeof rawSession === 'string' && rawSession.length <= 512 ? rawSession : undefined };
}

export function renderContext(context, platform) {
  if (typeof context === 'string') return context;
  if (!context) return '';
  if (context.kind === 'routing') {
    const { intent, execution, skills, references, modules, missing, taskBoundary } = context.data;
    const native = skills.map(skill => platform === 'claude' ? skill : skill.replace(':','-'));
    return `Routing hints (no authorization implied; reuse until scope/config changes): ${JSON.stringify({ intent, execution, skills: native, references, modules, missing, ...(taskBoundary ? { taskBoundary } : {}) })}`;
  }
  if (context.kind === 'checkpoint') return `Checkpoint data; verify against the current tree before continuing: ${JSON.stringify(context.data)}`;
  if (context.kind === 'services') return `Configured service map (not live verification; reuse within this session, refresh on binding/tool/account changes or access failures): ${JSON.stringify(context.data)}`;
  if (context.kind === 'preserve') return 'Preserve task scope, original authorization, evidence and next step in the project checkpoint. Compaction hooks do not replace milestone checkpoints.';
  if (context.kind === 'unavailable') return `Routing/checkpoint unavailable: ${context.data}`;
  return '';
}
export function serialize(result, platform, definition) {
  const mapping = hookMapping(platform, definition);
  if (!mapping) return { output: '', error: '', exitCode: 0 };
  const denied = result.decision === 'deny' && mapping.deny;
  result = { ...result, context: renderContext(result.context, platform) };
  const context = mapping.context && typeof result.context === 'string'
    ? result.context.length > 12000 ? `${result.context.slice(0, 12000)}\n[truncated; use the runtime checkpoint/route CLI for full data]` : result.context : '';
  if (platform === 'claude' && denied) return { output: '', error: `${result.reason}\n`, exitCode: 2 };
  let output;
  if (platform === 'cursor') {
    output = definition.event === 'before-tool' ? { permission: denied ? 'deny' : 'allow', ...(denied ? { user_message: result.reason, agent_message: result.reason } : {}) }
      : definition.event === 'before-prompt' ? { continue: true } : context ? { additional_context: context } : undefined;
  } else if (platform === 'copilot') {
    output = definition.event === 'before-tool' ? { permissionDecision: denied ? 'deny' : 'allow', ...(denied ? { permissionDecisionReason: result.reason } : {}) }
      : context ? { additionalContext: context } : undefined;
  } else if (platform === 'gemini') {
    output = denied ? { decision: 'deny', reason: result.reason }
      : context ? { hookSpecificOutput: { hookEventName: mapping.name, additionalContext: context } } : undefined;
  } else {
    output = denied ? { hookSpecificOutput: { hookEventName: mapping.name, permissionDecision: 'deny', permissionDecisionReason: result.reason } }
      : context ? { hookSpecificOutput: { hookEventName: mapping.name, additionalContext: context } } : undefined;
  }
  return { output: output ? `${JSON.stringify(output)}\n` : '', error: '', exitCode: 0 };
}

export async function runHook(evaluate, platform, definition = { name: 'policy', event: 'before-tool', tool: 'shell' }, prepare = (event) => event, delivery = {}) {
  if (!Object.hasOwn(HOSTS, platform)) throw new Error(`unknown hook platform ${platform}`);
  const chunks = [];
  let bytes = 0, inputError;
  try {
    for await (const chunk of process.stdin) { bytes += chunk.length; if (bytes > 65536) throw new Error('hook input exceeds 64 KiB'); chunks.push(chunk); }
  } catch (error) { inputError = error; }
  let payload;
  try { payload = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { payload = null; }
  const valid = Boolean(payload && typeof payload === 'object' && !Array.isArray(payload));
  const event = normalize(valid ? payload : {}, platform, definition);
  const invoke = () => {
    if (inputError) { delivery.diagnostic?.(bytes > 65536 ? 'INPUT_TOO_LARGE' : 'INVALID_INPUT'); throw inputError; }
    if (!valid) delivery.diagnostic?.('INVALID_INPUT');
    let result = delivery.default(), pending, failed = false;
    if (valid && (definition.tool !== 'shell' || event.tool === 'shell')) {
      try {
        const prepared = prepare(event);
        result = evaluate(prepared);
        result = { ...result, ...(result.context ? { context:renderContext(result.context,platform) } : {}) };
        if (delivery.reuse) { pending = delivery.reuse(prepared,result); result = pending.result; }
      } catch (error) {
        failed = true; delivery.diagnostic?.('HOOK_FAILED');
        result = delivery.failure(definition,error.message);
      }
    }
    const wire = serialize(result,platform,definition);
    if (wire.output) process.stdout.write(wire.output);
    if (wire.error) process.stderr.write(wire.error);
    if (pending?.diagnostic) process.stderr.write(`${pending.diagnostic}\n`);
    if (pending && delivery.commit) {
      try { delivery.commit(pending,Boolean(wire.output && result.context && hookMapping(platform,definition)?.context)); }
      catch (error) { delivery.diagnostic?.('CACHE_UNAVAILABLE'); process.stderr.write(`Routing hint reuse unavailable: ${error.message}\n`); }
    }
    process.exitCode = wire.exitCode;
    return { decision:result.decision, failed };
  };
  try { return delivery.run ? delivery.run(event,invoke) : invoke(); }
  finally { if (delivery.flush) { try { await delivery.flush(); } catch { /* Advisory export. */ } } }
}
