import { observe, withTelemetry } from '../telemetry/observe.mjs';
import { projectData, readSessionInput } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
export const validateProject = project => decide('validate-project', { project });
export const readProject = root => withTelemetry(root, {}, () => observe('project.loaded', () => validateProject(projectData(root)), () => ({}), { failureCode:'CONFIG_INVALID' }));
export const readInput = file => observe('input.loaded', () => readSessionInput(file), () => ({}), { failureCode:'INVALID_INPUT' });
