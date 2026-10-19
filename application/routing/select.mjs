import { observe, emitTelemetry } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { catalog } from './catalog.mjs';
import { serviceBindings } from '../../integrations/services.mjs';
const selectRoute = (input = {}) => decide('route', { ...input, catalog, bindings: serviceBindings(input.project ?? { formatVersion: 1 }, input.task ? { 'task.read': { id: input.task } } : {}) });
export const lookup = (project, options) => decide('lookup', { project, options });

export const route = (input = {}) => observe('routing.completed', () => {
  const result = selectRoute(input);
  for (const skill of [...new Set(result.skills)].slice(0,32)) emitTelemetry('guidance.selected',{ skill });
  return result;
}, result => ({ intent: result.intent, capabilityClass: result.capability.class, selectedSkills: result.skills.length }));
