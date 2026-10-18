import { decide } from './engine-client.mjs';
import { serviceBindings } from '../integrations/services.mjs';
export const planOperation = (project, operation, params = {}) => decide('operation', { project, operation, params, bindings: serviceBindings(project, { [operation]: params }) });
export const prepareServices = (project, options) => decide('services', { project, options, bindings: serviceBindings(project) });
