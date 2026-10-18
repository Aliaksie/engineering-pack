#!/usr/bin/env node
import { evaluate } from '../hooks/context.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent } from '../support/runtime/hooks.mjs';
await runHook(evaluate, 'claude', {"name":"compact-restore","event":"after-compact","tool":"any","handler":"hooks/context.mjs"}, prepareEvent);
