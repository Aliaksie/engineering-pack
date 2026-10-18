#!/usr/bin/env node
import { evaluate } from '../hooks/read-only.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault } from '../support/application/context-delivery/prepare.mjs';
await runHook(evaluate, 'codex', {"name":"read-only","event":"before-tool","tool":"shell","handler":"hooks/read-only.mjs"}, prepareEvent, { reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault });
