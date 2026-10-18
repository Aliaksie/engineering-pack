#!/usr/bin/env node
import { evaluate } from '../hooks/read-only.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint } from '../support/runtime/hooks.mjs';
await runHook(evaluate, 'claude', {"name":"read-only","event":"before-tool","tool":"shell","handler":"hooks/read-only.mjs"}, prepareEvent, { reuse: reuseRoutingHint, commit: commitRoutingHint });
