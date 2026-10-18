import { pathToFileURL } from 'node:url';
import { derive } from '../../../application/inbox.mjs';

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    const values = {};
    const allowed = new Set(['me', 'repo-short', 'repo-name', 'now', 'stale-days', 'prev', 'artifacts']);
    for (let index = 2; index < process.argv.length; index += 2) {
      const name = process.argv[index].replace(/^--/, '');
      if (!allowed.has(name) || process.argv[index + 1] === undefined || Object.hasOwn(values, name)) throw new Error('invalid derive arguments');
      values[name] = process.argv[index + 1];
    }
    let input = '';
    process.stdin.setEncoding('utf8');
    for await (const chunk of process.stdin) {
      input += chunk;
      if (Buffer.byteLength(input) > 4 * 1024 * 1024) throw new Error('derive input exceeds 4 MiB');
    }
    process.stdout.write(`${JSON.stringify(derive(JSON.parse(input), {
      me: values.me, repoShort: values['repo-short'], repoName: values['repo-name'], now: values.now,
      staleDays: values['stale-days'], prev: JSON.parse(values.prev ?? 'null'), artifacts: JSON.parse(values.artifacts ?? 'null')
    }))}\n`);
  } catch (error) { process.stderr.write(`derive: ${error.message}\n`); process.exitCode = 1; }
}
