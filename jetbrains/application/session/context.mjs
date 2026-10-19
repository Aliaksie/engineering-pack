import { observe } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { contextFiles } from '../../platform/index.mjs';
function contextReportOperation(bundleRoot, plan, { budgetBytes } = {}) {
  const requests = decide('context-requests', { plan });
  return decide('context-summary', { ...contextFiles(bundleRoot, requests), budgetBytes });
}

export const contextReport = (...args) => observe('context.observed', () => contextReportOperation(...args), result => ({ guidanceFiles: result.totals.files, guidanceBytes: result.totals.bytes, estimatedTokens: result.totals.estimatedTokens }), { failureCode:'CONTEXT_UNAVAILABLE' });
