import { pathToFileURL } from 'node:url';

const nodes = (connection) => Array.isArray(connection?.nodes) ? connection.nodes : [];
const milliseconds = (value) => Number.isFinite(Date.parse(value)) ? Date.parse(value) : 0;
const timestamp = (value) => value ? new Date(value).toISOString().replace(/\.000Z$/, 'Z') : null;
const present = (value) => value ?? null;

function observations(pr, identity) {
  const events = [];
  const append = (author, at, substantive) => {
    const time = milliseconds(at);
    if (author && time) events.push({ author, time, substantive });
  };
  for (const review of nodes(pr.reviews)) {
    append(review.author?.login, review.submittedAt, review.state !== 'APPROVED' || Boolean(review.body?.trim()));
  }
  for (const comment of nodes(pr.comments)) append(comment.author?.login, comment.createdAt, true);
  const participating = [];
  for (const thread of nodes(pr.reviewThreads)) {
    const comments = nodes(thread.comments);
    for (const comment of comments) append(comment.author?.login, comment.createdAt, true);
    if (!comments.some((comment) => comment.author?.login === identity)) continue;
    const newest = comments.reduce((result, comment) => milliseconds(comment.createdAt) > milliseconds(result?.createdAt) ? comment : result, null);
    participating.push({
      id: present(thread.id), path: present(thread.path), line: present(thread.line),
      isResolved: present(thread.isResolved), isOutdated: present(thread.isOutdated),
      last_comment_author: present(newest?.author?.login), last_comment_at: present(newest?.createdAt),
      replied_by_other: Boolean(newest?.author?.login && newest.author.login !== identity),
      summary: Array.from((comments[0]?.body ?? '').split(/\r?\n/).find((line) => line.trim()) ?? '').slice(0, 120).join('')
    });
  }
  return { events, participating };
}

/** Derive one local recommendation from remote observations and explicit local facts. */
export function derive(pr = {}, options = {}) {
  pr ??= {};
  const { me, repoShort, repoName, now, staleDays = 5 } = options;
  const prior = options.prev ?? {};
  const artifacts = options.artifacts ?? {};
  const { events, participating } = observations(pr, me);
  const latest = (predicate) => events.reduce((value, event) => predicate(event) ? Math.max(value, event.time) : value, 0);
  const mine = latest((event) => event.author === me);
  const other = latest((event) => event.author !== me);
  const reply = latest((event) => event.author !== me && event.substantive);
  const approvals = nodes(pr.reviews).filter((review) => review.author?.login && review.author.login !== me && review.state === 'APPROVED');
  const approval = approvals.reduce((value, review) => Math.max(value, milliseconds(review.submittedAt)), 0);
  const commit = nodes(pr.commits)[0]?.commit;
  const head = pr.headRefOid ?? commit?.oid ?? null;
  const commitAt = commit?.committedDate ?? pr.updatedAt ?? null;
  const committed = milliseconds(commitAt);
  const drafted = milliseconds(artifacts.comment_mtime);
  const posted = milliseconds(prior.posted_at);
  const pendingDraft = drafted > posted;
  const moved = Boolean(prior.reviewed_head_sha && prior.reviewed_head_sha !== head);
  const previouslyResolved = new Set((prior.threads ?? []).filter((thread) => thread.isResolved === true).map((thread) => thread.id));
  const resolved = participating.filter((thread) => thread.isResolved === true && !previouslyResolved.has(thread.id)).map((thread) => thread.id);
  const open = participating.filter((thread) => thread.isResolved === false).length;
  const age = mine ? Math.floor((milliseconds(now) - mine) / 86400000) : null;
  const rules = [
    ['RE_REVIEW_NEEDED', pendingDraft && (moved || committed > drafted)],
    ['DRAFT_READY', pendingDraft],
    ['NEW', !mine],
    ['FIXED_PENDING_VERIFY', committed > mine || resolved.length > 0],
    ['ANSWERED', reply > mine],
    ['CLEAN', approval > mine && open === 0],
    ['STALE_MINE', age > Number(staleDays)],
    ['AWAITING_AUTHOR', true]
  ];
  const status = rules.find(([, applies]) => applies)[0];
  const history = [...(prior.history ?? [])];
  if (prior.status !== status) history.push({ at: now, event: `status ${prior.status ?? 'none'} → ${status}`, head: head?.slice(0, 7) ?? null });
  return {
    repo: repoShort, repo_name: repoName, number: present(pr.number), title: present(pr.title), url: present(pr.url),
    author: pr.author?.login ?? 'unknown', is_own_pr: pr.author?.login === me, is_draft: present(pr.isDraft),
    base_ref: present(pr.baseRefName), head_ref: present(pr.headRefName),
    size: { files: present(pr.changedFiles), additions: present(pr.additions), deletions: present(pr.deletions) },
    head_sha: head, last_commit_at: commitAt, created_at: present(pr.createdAt), updated_at: present(pr.updatedAt),
    status, first_seen: prior.first_seen ?? now, last_scan: now,
    my_last_signal_at: timestamp(mine), others_last_signal_at: timestamp(other),
    author_reply_after: Boolean(mine && reply > mine), approved_after_my_comment: Boolean(mine && approval > mine),
    commits_after_my_comment: Boolean(mine && committed > mine), days_since_my_comment: age,
    open_threads_mine: open, replied_threads_mine: participating.filter((thread) => thread.replied_by_other).length,
    newly_resolved_threads: resolved, approved_by_other: approvals.length > 0,
    changes_requested_by_other: nodes(pr.reviews).some((review) => review.author?.login && review.author.login !== me && review.state === 'CHANGES_REQUESTED'),
    reviewed_head_sha: prior.reviewed_head_sha ?? null, reviewed_at: prior.reviewed_at ?? null,
    local_verdict: prior.local_verdict ?? null, head_moved_since_review: moved,
    drafted_at: timestamp(drafted), posted_at: timestamp(posted), posted_inferred: prior.posted_inferred === true,
    artifacts: { review: artifacts.review ?? null, validation: artifacts.validation ?? null, comment: artifacts.comment ?? null },
    threads: participating,
    reviews: nodes(pr.reviews).map((review) => ({ author: review.author?.login ?? null, state: present(review.state), at: present(review.submittedAt) })),
    history
  };
}

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
