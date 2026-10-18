import { decide } from '../engine-client.mjs';
import { catalog } from './catalog.mjs';
import { serviceBindings } from '../../integrations/services.mjs';
export const route = (input = {}) => decide('route', { ...input, catalog, bindings: serviceBindings(input.project ?? { formatVersion: 1 }, input.task ? { 'task.read': { id: input.task } } : {}) });
export const lookup = (project, options) => decide('lookup', { project, options });
