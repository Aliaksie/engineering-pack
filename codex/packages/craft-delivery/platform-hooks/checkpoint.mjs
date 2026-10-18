#!/usr/bin/env node
import { evaluate } from '../hooks/context.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint } from '../support/runtime/hooks.mjs';
await runHook(evaluate, 'codex', {"name":"checkpoint","event":"before-compact","tool":"any","handler":"hooks/context.mjs"}, prepareEvent, { reuse: reuseRoutingHint, commit: commitRoutingHint });
