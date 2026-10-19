import { observe } from '../telemetry/observe.mjs';
import { clock } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
import { catalog } from '../routing/catalog.mjs';
export const sessionAdvice = (input, project, options = {}) => observe('session.advice', () => decide('session-advice', { input, project, catalog, options: { now: clock().ms, ...options } }), result => ({ decision:result.recommendation?.action, pressure:result.pressure }), { failureCode:'VALIDATION_FAILED' });
