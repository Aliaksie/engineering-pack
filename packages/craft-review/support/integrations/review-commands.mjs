import { commands } from './commands.mjs';
// A narrow command guardrail for explicitly configured read-only review sessions.
// Tool-side authorization and an isolated runner remain the enforcement boundary.
// Recognize simple command segments only. Expansions, aliases, indirect execution,
// shell-specific escaping and scripts require tool-side restrictions.
function publishes(words) {
  const [executable, ...args] = words;
  if (executable === 'git') {
    let index = 0;
    while (index < args.length && args[index].startsWith('-')) {
      const option = args[index++];
      if (['-C', '-c', '--git-dir', '--work-tree', '--namespace', '--config-env', '--super-prefix'].includes(option)) index++;
    }
    return args[index] === 'push';
  }
  if (!['gh', 'glab'].includes(executable)) return false;
  if (['pr', 'mr', 'issue'].includes(args[0]) && ['comment', 'note', 'review', 'approve', 'merge', 'close', 'reopen', 'edit'].includes(args[1])) return true;
  if (args[0] !== 'api') return false;
  return args.some((argument, index) => /\bmutation\b/.test(argument)
    || (/^(?:-X|--method)$/.test(argument) && /^(?:POST|PUT|PATCH|DELETE)$/i.test(args[index + 1] ?? ''))
    || /^(?:-X|--method)=(?:POST|PUT|PATCH|DELETE)$/i.test(argument));
}


export const publishingCommand = command => commands({ command: command ?? '' }, 'review').some(publishes);
