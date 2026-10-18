import { HOSTS, hookMapping } from './hosts.mjs';

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

export function serialize(result, platform, definition) {
  const mapping = hookMapping(platform, definition);
  if (!mapping) return { output: '', error: '', exitCode: 0 };
  const denied = result.decision === 'deny' && mapping.deny;
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
  for await (const chunk of process.stdin) chunks.push(chunk);
  let payload;
  try { payload = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { payload = null; }
  let result = { decision: 'allow' };
  let pending;
  if (payload && typeof payload === 'object' && !Array.isArray(payload)) {
    const event = normalize(payload, platform, definition);
    if (definition.tool !== 'shell' || event.tool === 'shell') {
      try {
        const prepared = prepare(event);
        if (prepared.routing) prepared.routing.nativeSkills = prepared.routing.skills.map((skill) => platform === 'claude' ? skill : skill.replace(':', '-'));
        result = evaluate(prepared);
        if (delivery.reuse) {
          pending = delivery.reuse(prepared, result);
          result = pending.result;
        }
      }
      catch (error) {
        result = definition.tool === 'shell' && definition.event === 'before-tool'
          ? { decision: 'deny', reason: `Policy could not be evaluated: ${error.message}` }
          : { decision: 'allow', context: `Routing/checkpoint unavailable: ${error.message}` };
      }
    }
  }
  const wire = serialize(result, platform, definition);
  if (wire.output) process.stdout.write(wire.output);
  if (wire.error) process.stderr.write(wire.error);
  if (pending?.diagnostic) process.stderr.write(`${pending.diagnostic}\n`);
  if (pending && delivery.commit) {
    try { delivery.commit(pending, Boolean(wire.output && result.context && hookMapping(platform, definition)?.context)); }
    catch (error) { process.stderr.write(`Routing hint reuse unavailable: ${error.message}\n`); }
  }
  process.exitCode = wire.exitCode;
}
