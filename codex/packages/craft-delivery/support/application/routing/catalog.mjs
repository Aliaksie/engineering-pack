import { catalogData } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
export const validatePlaybookCatalog = catalog => decide('validate-catalog', { catalog });
export const catalog = validatePlaybookCatalog(catalogData());
