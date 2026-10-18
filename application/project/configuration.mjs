import { projectData } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
export const validateProject = project => decide('validate-project', { project });
export const readProject = root => validateProject(projectData(root));
