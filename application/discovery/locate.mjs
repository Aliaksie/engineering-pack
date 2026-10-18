import { decide } from '../engine-client.mjs';
import { scan } from '../../platform/index.mjs';
export const locateScope = value => decide('locate-scope', { value });
export const locate = (root, input) => scan(root, decide('locate-request', { input }), observation => decide('locate-match', observation));
