import { allow, commands, invocation, option, reject } from './inbox-commands.mjs';

export function outputAdvice(event) {
  const parsed = commands(event);
  for (let index = 0; index < parsed.length; index++) {
    const { executable, args, environment } = invocation(parsed[index]);
    if (executable !== 'gh' || environment.get('PR_LOOP_RAW') === '1') continue;
    // These are output-handling hints, not verified limits on bytes or model tokens.
    const downstream = invocation(parsed[index + 1] ?? []).executable;
    if (['head', 'tail', 'wc', 'grep', 'rg', 'jq', 'sed', 'awk', 'diffstat', 'sort', 'uniq', 'cut'].includes(downstream)
      || args.some((arg) => arg.startsWith('>'))) continue;
    const [group, action] = args;
    let suggestion;
    if (group === 'pr' && action === 'diff' && !args.includes('--name-only')) suggestion = 'List paths with --name-only, or save the patch as a requested artifact.';
    if (group === 'pr' && action === 'view' && option(args, '--json') === undefined && !args.includes('--web')) suggestion = 'Select the required fields with --json, or consult the local inbox record.';
    if (group === 'run' && action === 'view' && args.includes('--log') && !args.includes('--log-failed')) suggestion = 'Request --log-failed or a specific job excerpt.';
    if (group === 'api' && args.some((arg) => /(?:pulls\/\d+\/(?:files|comments|reviews)|issues\/\d+\/comments|\/commits(?:\?|$))/.test(arg)) && option(args, '--jq', '-q') === undefined) suggestion = 'Project the fields needed for this question with --jq.';
    if (suggestion) return reject(`${suggestion} PR_LOOP_RAW=1 can mark an intentional full-output request.`);
  }
  return allow();
}
