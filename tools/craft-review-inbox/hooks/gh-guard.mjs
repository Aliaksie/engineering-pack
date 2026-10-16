import { allow, commands, invocation, option, reject } from './lib/hook.mjs';

const forbidden = new Set([
  'repo:delete', 'repo:archive', 'repo:rename', 'secret:set', 'secret:delete',
  'variable:set', 'variable:delete', 'release:delete', 'release:delete-asset',
  'run:delete', 'cache:delete', 'auth:logout', 'gist:create', 'ruleset:delete'
]);
const changes = {
  pr: ['comment', 'review', 'merge', 'close', 'reopen', 'ready', 'edit', 'lock', 'unlock'],
  issue: ['comment', 'close', 'reopen', 'edit', 'delete'],
  release: ['create', 'edit', 'upload'], workflow: ['run', 'enable', 'disable'],
  run: ['cancel', 'rerun'], label: ['create', 'delete', 'edit']
};
export function evaluate(event) {
  for (const words of commands(event)) {
    const { executable, args, environment } = invocation(words);
    if (executable === 'git' && args.includes('push') && args.some((arg) => /(?:^|[\\/])(?:\.pr-loop[\\/]scratch|pr-loop[\\/](?:work|mirrors))(?:[\\/]|$)/.test(arg))) {
      return reject('Inbox scratch checkouts are local review artifacts. Push from the intended project checkout.');
    }
    if (executable !== 'gh') continue;
    const [group, action] = args;
    const method = option(args, '-X', '--method')?.toUpperCase();
    if (forbidden.has(`${group}:${action}`) || group === 'api' && method === 'DELETE') {
      return reject('This repository-administration command is outside the optional inbox policy. Use a separate authorized operation.');
    }
    let writes = changes[group]?.includes(action) ?? false;
    if (group === 'api') {
      if (args.includes('graphql')) {
        const documents = args.filter((arg) => arg.startsWith('query=')).map((arg) => arg.slice(6));
        // File indirection and unknown documents cannot establish a read-only query.
        writes = documents.length !== 1 || !/^\s*(?:query\b|\{)/.test(documents[0]) || /\bmutation\s*(?:\w+\s*)?[({]/.test(documents[0]);
      } else {
        writes = method ? !['GET', 'HEAD', 'OPTIONS'].includes(method) : args.some((arg) => ['-f', '-F', '--field', '--raw-field'].includes(arg) || /^--(?:raw-)?field=/.test(arg));
      }
    }
    if (writes && environment.get('PR_LOOP_ALLOW_WRITE') !== '1') {
      return reject('The inbox prepares local drafts. Remote changes need separate user authorization; PR_LOOP_ALLOW_WRITE=1 records a deliberate policy exception, not permission.');
    }
  }
  return allow();
}
