import { observe, diagnostic, withTelemetry } from '../telemetry/observe.mjs';
import { nativeLocate } from '../../platform/index.mjs';
export const locate = (root, input, { repositories } = {}) => withTelemetry(root, {}, () => observe('search.completed', () => {
  const result = nativeLocate(root, input, repositories);
  if (result.truncated || Object.values(result.skipped).some(Boolean)) diagnostic('discovery','SEARCH_INCOMPLETE');
  return result;
}, result => ({ evidenceCount:result.matches.length, complete:!result.truncated }), { failureCode:'SEARCH_FAILED' }));
