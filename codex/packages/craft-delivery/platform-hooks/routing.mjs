#!/usr/bin/env node
import { evaluate } from '../hooks/context.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent } from '../support/runtime/hooks.mjs';
await runHook(evaluate, 'codex', {"name":"routing","event":"before-tool","tool":"any","handler":"hooks/context.mjs"}, prepareEvent);
