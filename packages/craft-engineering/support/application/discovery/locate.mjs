import { observe, diagnostic, withTelemetry } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { scan } from '../../platform/index.mjs';
export const locateScope = value => decide('locate-scope', { value });
export const locate = (root, input) => withTelemetry(root, {}, () => observe('search.completed', () => {
  const result = scan(root, decide('locate-request', { input }), observation => decide('locate-match', observation));
  if (result.truncated || Object.values(result.skipped).some(Boolean)) diagnostic('discovery','SEARCH_INCOMPLETE');
  return result;
}, result => ({ evidenceCount:result.matches.length, complete:!result.truncated }), { failureCode:'SEARCH_FAILED' }));
