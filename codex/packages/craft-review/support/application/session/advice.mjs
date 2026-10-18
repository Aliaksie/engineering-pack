import { clock } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
import { catalog } from '../routing/catalog.mjs';
export const sessionAdvice = (input, project, options = {}) => decide('session-advice', { input, project, catalog, options: { now: clock().ms, ...options } });
