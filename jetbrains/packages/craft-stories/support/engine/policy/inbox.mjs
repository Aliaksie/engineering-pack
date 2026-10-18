export function inboxStatus({ pendingDraft, moved, committed, drafted, mine, reply, approval, open, resolved, age, staleDays }) {
  const rules = [
    ['RE_REVIEW_NEEDED', pendingDraft && (moved || committed > drafted)],
    ['DRAFT_READY', pendingDraft],
    ['NEW', !mine],
    ['FIXED_PENDING_VERIFY', committed > mine || resolved > 0],
    ['ANSWERED', reply > mine],
    ['CLEAN', approval > mine && open === 0],
    ['STALE_MINE', age > Number(staleDays)],
    ['AWAITING_AUTHOR', true]
  ];
  return rules.find(([, applies]) => applies)[0];
}

export function inboxPolicy(facts) {
  if (facts.scratchPush) return { decision: 'deny', reason: 'Inbox scratch checkouts are local review artifacts. Push from the intended project checkout.' };
  if (facts.administration) return { decision: 'deny', reason: 'This repository-administration command is outside the optional inbox policy. Use a separate authorized operation.' };
  if (facts.unauthorizedWrite) return { decision: 'deny', reason: 'The inbox prepares local drafts. Remote changes need separate user authorization; PR_LOOP_ALLOW_WRITE=1 records a deliberate policy exception, not permission.' };
  return { decision: 'allow' };
}
