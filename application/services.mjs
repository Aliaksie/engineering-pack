import { observe, diagnostic } from './telemetry/observe.mjs';
import { decide } from './engine-client.mjs';
import { serviceBindings } from '../integrations/services.mjs';
export const prepareServices = (project, options) => observe('integration.prepared', () => {
  const result = decide('services', { project, options, bindings: serviceBindings(project) });
  const unavailable = result.services.flatMap(service => service.operations).filter(operation => operation.availability === 'not-observed').length;
  if (unavailable) diagnostic('integration','INTEGRATION_UNAVAILABLE',{ unavailableCapabilities:unavailable });
  return result;
}, () => ({}), { failureCode:'INTEGRATION_UNAVAILABLE' });
