import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { derive } from '../bin/derive.mjs';
import { repoName, prNumber } from './config.mjs';
import { atomicWrite, jsonFile, noSymlinks, run, saveJson, withLock } from './io.mjs';
import { render, states } from './render.mjs';

const query = fs.readFileSync(new URL('../bin/query.graphql', import.meta.url), 'utf8');
const statePath = (c, short, number) => { repoName(c, short); return path.join(c.state, short, `pr_${prNumber(number)}.json`); };
const envFor = (c) => ({ ...process.env, GH_HOST: c.host, GH_PROMPT_DISABLED: '1' });
function artifacts(config, short, number) {
  const directory = path.join(config.artifacts, short, `pr_${number}`);
  const result = {};
  for (const [key, name] of [['review', 'review.md'], ['validation', 'validation-report.md'], ['comment', 'pr-comment.md']]) {
    const file = path.join(directory, name); noSymlinks(file);
    if (fs.existsSync(file)) {
      result[key] = path.relative(config.root, file).split(path.sep).join('/');
      if (key === 'comment') result.comment_mtime = fs.statSync(file).mtime.toISOString();
    }
  }
  return result;
}
function connection(value, label) {
  if (!value || !Array.isArray(value.nodes) || typeof value.pageInfo?.hasNextPage !== 'boolean') throw new Error(`invalid ${label} connection`);
  if (value.pageInfo.hasNextPage) throw new Error(`incomplete nested ${label}; retain previous state rather than infer a clean review`);
}
export function scan(config, { repo, now = new Date().toISOString(), exec = run } = {}) {
  if (repo) repoName(config, repo);
  return withLock(config, () => {
    const failures = [];
    let updated = 0;
    for (const [short, name] of Object.entries(config.repos)) {
      if (repo && repo !== short) continue;
      try {
        const nodes = [];
        let cursor = null;
        const cursors = new Set();
        for (;;) {
          const args = ['api', '--hostname', config.host, 'graphql', '-f', `query=${query}`, '-F', `owner=${config.owner}`, '-F', `name=${name}`];
          if (cursor) args.push('-f', `cursor=${cursor}`);
          const response = JSON.parse(exec('gh', args, { env: envFor(config) }));
          const list = response.data?.repository?.pullRequests;
          if (response.errors?.length || !Array.isArray(list?.nodes) || typeof list.pageInfo?.hasNextPage !== 'boolean') throw new Error('invalid or incomplete GraphQL repository response');
          for (const node of list.nodes) {
            prNumber(node.number);
            if (typeof node.isDraft !== 'boolean' || typeof node.headRefOid !== 'string' || !node.headRefOid || typeof node.title !== 'string' || node.commits?.nodes?.[0]?.commit?.oid !== node.headRefOid || !Number.isFinite(Date.parse(node.commits?.nodes?.[0]?.commit?.committedDate))) throw new Error('invalid PR response');
            if (!node.isDraft) {
              for (const field of ['reviews', 'comments', 'reviewThreads']) connection(node[field], field);
              for (const thread of node.reviewThreads.nodes) connection(thread.comments, 'thread comments');
            }
            nodes.push(node);
          }
          if (!list.pageInfo.hasNextPage) break;
          cursor = list.pageInfo.endCursor;
          if (typeof cursor !== 'string' || !cursor || cursors.has(cursor) || cursors.size >= 1000) throw new Error('invalid or excessive pagination');
          cursors.add(cursor);
        }
        if (new Set(nodes.map((node) => node.number)).size !== nodes.length) throw new Error('duplicate PRs across pages; retry a fresh scan');
        // Validate and derive the full repository response before changing its stored state.
        const updates = nodes.filter((node) => !node.isDraft).map((node) => {
          const file = statePath(config, short, node.number);
          return [file, derive(node, { me: config.me, repoShort: short, repoName: name, now, staleDays: config.staleDays, prev: jsonFile(file, null), artifacts: artifacts(config, short, node.number) })];
        });
        // Keep known drafts current without interpreting potentially truncated review signals.
        // New drafts remain outside the review ledger until they become ready for review.
        for (const draft of nodes.filter((node) => node.isDraft)) {
          const file = statePath(config, short, draft.number);
          const previous = jsonFile(file, null);
          if (previous) updates.push([file, { ...previous, is_draft: true, title: draft.title,
            head_sha: draft.headRefOid, head_ref: draft.headRefName, base_ref: draft.baseRefName,
            updated_at: draft.updatedAt ?? previous.updated_at, last_scan: now }]);
        }
        const open = new Set(nodes.map((node) => node.number));
        for (const previous of states(config).filter((state) => state.repo === short)) {
          if (!open.has(previous.number) && previous.status !== 'CLOSED_OR_MERGED') updates.push([statePath(config, short, previous.number), { ...previous, status: 'CLOSED_OR_MERGED', last_scan: now, history: [...(previous.history ?? []), { at: now, event: `status ${previous.status} → CLOSED_OR_MERGED`, head: previous.head_sha?.slice(0, 7) }] }]);
        }
        for (const [file, state] of updates) { saveJson(file, state); updated++; }
      } catch (error) { failures.push({ repo: short, reason: error.message }); }
    }
    render(config, { now, failures: failures.map(({ repo, reason }) => `${repo}: ${reason}`) });
    if (!failures.length && !repo) saveJson(path.join(config.state, '.last-scan.json'), { at: now });
    return { updated, failures, complete: !failures.length };
  });
}
export function tick(config, { force = false, hours = config.tickHours, now = new Date().toISOString(), ...options } = {}) {
  if (!Number.isFinite(hours) || hours < 0 || !Number.isFinite(Date.parse(now))) throw new Error('invalid tick hours/time');
  const marker = jsonFile(path.join(config.state, '.last-scan.json'), null);
  const age = Date.parse(now) - Date.parse(marker?.at);
  if (!force && age >= 0 && age < hours * 3600000) return { skipped: true };
  return scan(config, { ...options, now });
}
function markState(config, short, number, event, { verdict, note, head, now = new Date().toISOString() } = {}) {
  const file = statePath(config, short, number);
  const state = jsonFile(file);
  if (!['reviewed', 'posted', 'note'].includes(event)) throw new Error('event must be reviewed, posted or note');
  if (verdict && !['APPROVE', 'REQUEST-CHANGES', 'COMMENT'].includes(verdict)) throw new Error('invalid verdict');
  if (event === 'note' && !note) throw new Error('note requires --note');
  if (event === 'reviewed') {
    if (!head || head !== state.head_sha) throw new Error('reviewed requires --head matching the exact reviewed state head; rescan stale state');
    if (state.is_own_pr && verdict && verdict !== 'COMMENT') throw new Error('own PR supports COMMENT verdict only');
    const draft = path.join(config.artifacts, short, `pr_${number}`, 'pr-comment.md');
    noSymlinks(draft);
    if (fs.existsSync(draft)) atomicWrite(path.join(config.home, 'voice/drafts', `${short}-pr${number}-${now.replace(/[:.]/g, '-')}-${randomUUID()}.md`), fs.readFileSync(draft, 'utf8'));
    state.reviewed_head_sha = head; state.reviewed_at = now;
    if (verdict) state.local_verdict = verdict;
  }
  if (event === 'posted') { state.posted_at = now; state.posted_inferred = false; }
  state.local_event_pending_scan = event;
  state.history = [...(state.history ?? []), { at: now, event: `${event}${verdict ? ` · ${verdict}` : ''}${note ? ` · ${note}` : ''}`, head: state.head_sha?.slice(0, 7) }];
  saveJson(file, state);
  render(config, { now });
  return state;
}
export const mark = (config, short, number, event, options) => withLock(config, () => markState(config, short, prNumber(number), event, options));
const stripVerdict = (body) => body.replace(/\r\n/g, '\n').split('\n').filter((line) => !/^\s*(?:\*\*)?Suggested verdict:/i.test(line)).join('\n').replace(/\n{3,}/g, '\n\n').trim();
export function recordPosted(config, short, number, { file, text, now = new Date().toISOString() } = {}) {
  number = prNumber(number); repoName(config, short);
  return withLock(config, () => {
    const state = jsonFile(statePath(config, short, number));
    if (!file && text === undefined) throw new Error('record-posted requires --file or --stdin containing the text actually posted');
    if (file && text !== undefined) throw new Error('choose --file or --stdin');
    if (file) { noSymlinks(file); text = fs.readFileSync(file, 'utf8'); }
    const body = stripVerdict(text);
    if (!body) throw new Error('posted text is empty after removing verdict metadata');
    const sample = path.join(config.home, 'voice/samples', `${short}-pr${number}-${randomUUID()}.md`);
    atomicWrite(sample, `---\nkind: review-comment\nrepo: ${short}\npr: ${number}\ndate: ${now}\n---\n\n${body}\n`);
    const drafts = path.join(config.home, 'voice/drafts'); noSymlinks(drafts);
    const names = fs.existsSync(drafts) ? fs.readdirSync(drafts).filter((name) => name.startsWith(`${short}-pr${number}-`) && name.endsWith('.md')).sort() : [];
    if (names.length) {
      const snapshot = path.join(drafts, names.at(-1)); noSymlinks(snapshot);
      const before = stripVerdict(fs.readFileSync(snapshot, 'utf8'));
      if (before !== body) {
        const edits = path.join(config.home, 'voice/EDITS.md'); noSymlinks(edits);
        const existing = fs.existsSync(edits) ? fs.readFileSync(edits, 'utf8') : '# Posted-text corrections\n';
        // Keep both actual texts, without depending on a platform diff executable.
        atomicWrite(edits, `${existing}\n## ${now} · ${short} #${number}\n\nDraft: ${path.relative(config.home, snapshot)}\n\n${JSON.stringify({ draft: before, posted: body }, null, 2)}\n`);
      }
    }
    markState(config, short, number, 'posted', { now, note: 'actual posted text recorded locally' });
    return { sample, head: state.head_sha };
  });
}
export function doctor(config, { exec = run } = {}) {
  const checks = [];
  for (const [command, args] of [['git', ['--version']], ['gh', ['--version']], ['gh', ['auth', 'status', '--hostname', config.host]]]) {
    try { exec(command, args, { env: envFor(config) }); checks.push({ command, ok: true }); }
    catch (error) { checks.push({ command, ok: false, reason: error.message }); }
  }
  if (checks.every((check) => check.ok)) {
    try { exec('gh', ['api', '--hostname', config.host, `repos/${config.owner}/${Object.values(config.repos)[0]}`], { env: envFor(config) }); checks.push({ command: 'repository read', ok: true }); }
    catch (error) { checks.push({ command: 'repository read', ok: false, reason: error.message }); }
  }
  return { checks, ok: checks.every((check) => check.ok) };
}
export function checkout(config, short, number, { exec = run } = {}) {
  number = prNumber(number);
  const name = repoName(config, short);
  const state = jsonFile(statePath(config, short, number));
  if (!/^[a-f0-9]{40}(?:[a-f0-9]{24})?$/.test(state.head_sha)) throw new Error('scan must supply an exact head SHA before checkout');
  noSymlinks(config.scratch); fs.mkdirSync(config.scratch, { recursive: true });
  // A fresh owned directory avoids destructive reuse/reset of any existing checkout.
  const directory = fs.mkdtempSync(path.join(config.scratch, `${short}-pr${number}-`));
  try {
    const args = { env: { ...envFor(config), GIT_TERMINAL_PROMPT: '0' } };
    exec('git', ['init', '--quiet', directory], args);
    const url = `https://${config.host}/${config.owner}/${name}.git`;
    exec('git', ['-c', 'core.hooksPath=', '-C', directory, 'fetch', '--no-tags', url, `refs/pull/${number}/head`], args);
    const head = exec('git', ['-C', directory, 'rev-parse', 'FETCH_HEAD'], args).trim();
    if (head !== state.head_sha) throw new Error('remote PR head changed since scan; rescan before review');
    exec('git', ['-c', 'core.hooksPath=', '-C', directory, 'checkout', '--quiet', '--detach', head], args);
    if (typeof state.base_ref !== 'string' || !state.base_ref || state.base_ref.startsWith('-')) throw new Error('scan did not provide a valid base ref');
    exec('git', ['-C', directory, 'check-ref-format', `refs/heads/${state.base_ref}`], args);
    exec('git', ['-c', 'core.hooksPath=', '-C', directory, 'fetch', '--no-tags', url, `${state.base_ref}:refs/remotes/origin/${state.base_ref}`], args);
    return { directory, head, base: `origin/${state.base_ref}` };
  } catch (error) { fs.rmSync(directory, { recursive: true, force: true }); throw error; }
}
